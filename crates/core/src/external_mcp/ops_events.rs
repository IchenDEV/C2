//! External MCP tools: `codetwo_events_poll`, `codetwo_events_wait`, `codetwo_turn_wait`.
//!
//! Waiting never errors on timeout: it returns a structured result (`timed_out: true`) so a
//! client can tell "nothing happened yet" from a failure. All cursors follow the rules in
//! [`crate::external_mcp::events`]: absent cursor = head (no replay), a gap is a non-error reset.

use crate::external_mcp::clients::ResolvedClient;
use crate::external_mcp::ctx::{CallContext, ToolError, ToolErrorKind, ToolResult};
use crate::external_mcp::events::{
    EventKind, EventsError, PollRequest, PollResult, ProjectFilter, MAX_TOOL_RESPONSE_BYTES,
    MAX_WAIT_SECS,
};
use crate::external_mcp::gate::resolve_session_project;
use crate::session::{PendingInputKind, SessionRunState};
use crate::Engine;
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

const TOOL_EVENTS_POLL: &str = "codetwo_events_poll";
const TOOL_EVENTS_WAIT: &str = "codetwo_events_wait";
const TOOL_TURN_WAIT: &str = "codetwo_turn_wait";

const DEFAULT_WAIT_MS: u64 = 30_000;
/// Longest park between two snapshots of a waited-on session (events wake it earlier).
const TURN_WAIT_SLICE: Duration = Duration::from_secs(1);
/// How long an idle session may lag its terminal event (activity flips before the event).
const TERMINAL_EVENT_GRACE: Duration = Duration::from_millis(750);

/// Blocking wait entry point. The authorize gate is synchronous: HTTP handlers run it inside
/// `tokio::task::spawn_blocking` so a Tokio worker is not held for up to 55 s.
pub fn events_wait_blocking(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    events_wait(engine, ctx, args)
}

pub fn call(
    engine: &Engine,
    ctx: &CallContext<'_>,
    tool: &str,
    args: &Value,
) -> Option<ToolResult> {
    match tool {
        TOOL_EVENTS_POLL => Some(events_poll(engine, ctx, args)),
        TOOL_EVENTS_WAIT => Some(events_wait(engine, ctx, args)),
        TOOL_TURN_WAIT => Some(turn_wait(engine, ctx, args)),
        _ => None,
    }
}

fn events_poll(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let request = build_poll_request(ctx.client, args)?;
    let result = engine
        .external_mcp_state()
        .hub()
        .poll(request)
        .map_err(map_events_error)?;
    Ok(poll_json_bounded(result))
}

fn events_wait(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let request = build_poll_request(ctx.client, args)?;
    let result = engine
        .external_mcp_state()
        .hub()
        .wait_for(request, timeout_from(args))
        .map_err(map_events_error)?;
    Ok(poll_json_bounded(result))
}

fn timeout_from(args: &Value) -> Duration {
    Duration::from_millis(
        args.get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_WAIT_MS)
            .clamp(1, MAX_WAIT_SECS * 1000),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnStatus {
    Queued,
    Running,
    WaitingApproval,
    WaitingQuestion,
    Completed,
    Cancelled,
    Failed,
    Broken,
    /// The turn is over but no retained record says how.
    Unknown,
}

impl TurnStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::WaitingApproval => "waiting_approval",
            Self::WaitingQuestion => "waiting_question",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Broken => "broken",
            Self::Unknown => "unknown",
        }
    }

    /// Statuses that end the wait without a timeout.
    fn settles(self) -> bool {
        !matches!(self, Self::Queued | Self::Running)
    }

    fn from_terminal(kind: EventKind) -> Self {
        match kind {
            EventKind::TurnCompleted => Self::Completed,
            EventKind::TurnCancelled => Self::Cancelled,
            EventKind::SessionBroken => Self::Broken,
            _ => Self::Failed,
        }
    }
}

/// Where a session stands, from the authoritative activity projection (not from the ring).
struct SessionSnapshot {
    active_turn: Option<String>,
    /// Prompt request id of the active turn: the authoritative receipt → turn link, which does not
    /// depend on the (bounded) event ring still holding `turn.started`.
    active_request: Option<String>,
    waiting: Option<TurnStatus>,
    failed_turn: Option<Option<String>>,
    /// The engine no longer lists the session (its runtime was torn down after the turn). The
    /// ring still holds what happened, so the wait is answered from there.
    gone: bool,
}

fn snapshot(engine: &Engine, session_id: &str) -> Result<SessionSnapshot, ToolError> {
    let Some(session) = engine
        .list_sessions()
        .map_err(|e| ToolError::internal(e.to_string()))?
        .into_iter()
        .find(|s| s.id == session_id)
    else {
        return Ok(SessionSnapshot {
            active_turn: None,
            active_request: None,
            waiting: None,
            failed_turn: None,
            gone: true,
        });
    };
    Ok(match session.activity.state {
        SessionRunState::Running {
            turn_id,
            prompt_request_id,
        } => SessionSnapshot {
            active_turn: Some(turn_id),
            active_request: prompt_request_id,
            waiting: None,
            failed_turn: None,
            gone: false,
        },
        SessionRunState::AwaitingInput {
            turn_id,
            prompt_request_id,
            pending,
        } => SessionSnapshot {
            active_turn: Some(turn_id),
            active_request: prompt_request_id,
            waiting: if pending
                .iter()
                .any(|p| p.kind == PendingInputKind::Permission)
            {
                Some(TurnStatus::WaitingApproval)
            } else if pending
                .iter()
                .any(|p| p.kind == PendingInputKind::Elicitation)
            {
                Some(TurnStatus::WaitingQuestion)
            } else {
                None
            },
            failed_turn: None,
            gone: false,
        },
        SessionRunState::Failed { turn_id, .. } => SessionSnapshot {
            active_turn: None,
            active_request: None,
            waiting: None,
            failed_turn: Some(turn_id),
            gone: false,
        },
        SessionRunState::Idle => SessionSnapshot {
            active_turn: None,
            active_request: None,
            waiting: None,
            failed_turn: None,
            gone: false,
        },
    })
}

/// How a `delivery_id` resolves, from the most to the least authoritative record.
enum Receipt {
    /// The prompt became (or, for steering, joined) this turn.
    Turn(String),
    /// Settled without a turn. `delivered` says whether the provider may have received it.
    Settled {
        status: TurnStatus,
        delivered: Option<bool>,
        reason: &'static str,
    },
    /// A durable queue/steer delivery that has not been sent yet.
    InFlight,
    /// Nothing retained knows this receipt.
    NoRecord,
}

fn resolve_receipt(
    engine: &Engine,
    session_id: &str,
    receipt: &str,
    snap: &SessionSnapshot,
) -> Result<Receipt, ToolError> {
    let state = engine.external_mcp_state();
    let from_ring = state
        .hub()
        .ring()
        .lock()
        .expect("event ring mutex poisoned")
        .turn_for_request(session_id, receipt);
    if let Some(turn) = from_ring {
        return Ok(Receipt::Turn(turn));
    }
    // The activity projection links the receipt to the running turn without the ring.
    if let (Some(request), Some(turn)) = (&snap.active_request, &snap.active_turn) {
        if request == receipt {
            return Ok(Receipt::Turn(turn.clone()));
        }
    }
    if let Some(store) = engine.store() {
        let delivery = store
            .prompt_delivery(receipt)
            .map_err(|e| ToolError::internal(e.to_string()))?;
        if let Some(delivery) = delivery.filter(|d| d.session_id == session_id) {
            let settled = |status, delivered, reason| {
                Ok(Receipt::Settled {
                    status,
                    delivered,
                    reason,
                })
            };
            match delivery.state.as_str() {
                "queued" | "submitting" => return Ok(Receipt::InFlight),
                "accepted" if delivery.mode == "steer" => {
                    return match delivery.expected_turn {
                        Some(turn) => Ok(Receipt::Turn(turn)),
                        None => settled(TurnStatus::Unknown, None, "receipt_not_retained"),
                    }
                }
                // A durable acceptance outweighs a late rejection projection when its turn
                // linkage has been evicted. Its final result is unavailable, never safe to replay.
                "accepted" => {
                    return settled(TurnStatus::Unknown, Some(true), "outcome_not_retained")
                }
                "failed" => return settled(TurnStatus::Failed, Some(false), "not_delivered"),
                "cancelled" => {
                    return settled(TurnStatus::Cancelled, Some(false), "cancelled_before_send")
                }
                "unknown" => return settled(TurnStatus::Unknown, None, "delivery_outcome_unknown"),
                _ => {}
            }
        }
    }
    if state.tap().rejected_request(session_id, receipt) {
        return Ok(Receipt::Settled {
            status: TurnStatus::Failed,
            delivered: Some(false),
            reason: "rejected",
        });
    }
    Ok(Receipt::NoRecord)
}

/// What a consumer may do about the outcome. Only a prompt that provably never reached the
/// provider is safe to resend; everything else (in flight, ended, unknown, no longer retained)
/// must never be replayed automatically.
fn meaning(reason: &str) -> &'static str {
    match reason {
        "in_flight" => "still in flight; do not resend",
        "turn_ended" => "the turn ended; the prompt was delivered, do not resend automatically",
        "turn_failed_in_activity" => "the turn failed after delivery; do not resend automatically",
        "not_delivered" | "rejected" | "cancelled_before_send" => {
            "the prompt was never delivered; resending is safe"
        }
        "delivery_outcome_unknown" => {
            "the provider may have received the prompt; never replay automatically"
        }
        "receipt_not_retained" | "outcome_not_retained" => {
            "no retained record of this outcome (event ring evicted or restarted); never replay automatically"
        }
        _ => "session state only; no delivery information",
    }
}

fn turn_wait(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let session_id = args
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("session_id required"))?;
    let state = engine.external_mcp_state();
    let not_found = || ToolError::new(ToolErrorKind::NotFound, "session not found");
    let project = state
        .session_project(session_id)
        .or_else(|| resolve_session_project(engine, session_id))
        .ok_or_else(not_found)?;
    if !ctx.project_allowed(Path::new(&project)) {
        return Err(not_found());
    }

    let deadline = Instant::now() + timeout_from(args);
    let hub = state.hub();
    let ring = hub.ring();
    // Arm before the first snapshot: any event after this point wakes the loop, so a turn that
    // ends between snapshot and park is never missed, and old events are never re-read.
    let mut cursor = hub.head_cursor();
    let mut target = args
        .get("turn_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let receipt = args.get("delivery_id").and_then(Value::as_str);
    let mut lagging_since: Option<Instant> = None;
    let mut receipt_lag: Option<Instant> = None;

    let respond = |status: TurnStatus,
                   turn: Option<&str>,
                   timed_out: bool,
                   delivered: Option<bool>,
                   reason: &str| {
        Ok(json!({
            "status": status.as_str(),
            "session_id": session_id,
            "turn_id": turn,
            "timed_out": timed_out,
            "delivered": delivered,
            "no_retry": delivered != Some(false),
            "reason": reason,
            "meaning": meaning(reason),
        }))
    };

    loop {
        let mut snap = snapshot(engine, session_id)?;
        if target.is_none() {
            if let Some(receipt) = receipt {
                match resolve_receipt(engine, session_id, receipt, &snap)? {
                    Receipt::Turn(turn) => {
                        target = Some(turn);
                        // The turn may have moved since the snapshot that preceded the lookup.
                        snap = snapshot(engine, session_id)?;
                    }
                    Receipt::Settled {
                        status,
                        delivered,
                        reason,
                    } => return respond(status, None, false, delivered, reason),
                    Receipt::InFlight => {}
                    Receipt::NoRecord => {
                        let since = *receipt_lag.get_or_insert_with(Instant::now);
                        if since.elapsed() >= TERMINAL_EVENT_GRACE {
                            return respond(
                                TurnStatus::Unknown,
                                None,
                                false,
                                None,
                                "receipt_not_retained",
                            );
                        }
                    }
                }
            }
        }
        // (status, delivered, reason)
        let (status, delivered, reason) = match (&target, receipt) {
            (None, Some(_)) => (TurnStatus::Queued, None, "in_flight"),
            (None, None) => match (&snap.active_turn, &snap.failed_turn) {
                (Some(active), _) => {
                    target = Some(active.clone());
                    continue;
                }
                (None, Some(_)) => (TurnStatus::Failed, Some(true), "turn_failed_in_activity"),
                // Legacy "wait on the session" form: an idle session has nothing left to wait for.
                (None, None) if snap.gone => match state.tap().turn_of(session_id) {
                    Some(last) => {
                        target = Some(last);
                        continue;
                    }
                    None => return Err(not_found()),
                },
                (None, None) => (TurnStatus::Completed, None, "session_idle"),
            },
            (Some(turn), _) if snap.active_turn.as_deref() == Some(turn.as_str()) => (
                snap.waiting.unwrap_or(TurnStatus::Running),
                Some(true),
                "in_flight",
            ),
            (Some(turn), _) => {
                let recorded = ring
                    .lock()
                    .expect("event ring mutex poisoned")
                    .terminal_for_turn(session_id, turn);
                match recorded {
                    Some(kind) => (TurnStatus::from_terminal(kind), Some(true), "turn_ended"),
                    None => {
                        let since = *lagging_since.get_or_insert_with(Instant::now);
                        if since.elapsed() < TERMINAL_EVENT_GRACE {
                            (TurnStatus::Running, Some(true), "in_flight")
                        } else if snap
                            .failed_turn
                            .as_ref()
                            .is_some_and(|failed| failed.as_deref().is_none_or(|t| t == turn))
                        {
                            // Durable fallback: the persisted activity still names the failure.
                            (TurnStatus::Failed, Some(true), "turn_failed_in_activity")
                        } else {
                            // The turn is over, but neither the ring (bounded, process-local) nor
                            // the activity projection kept how it ended.
                            (TurnStatus::Unknown, Some(true), "outcome_not_retained")
                        }
                    }
                }
            }
        };
        if status.settles() {
            return respond(status, target.as_deref(), false, delivered, reason);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return respond(status, target.as_deref(), true, delivered, reason);
        }
        let wake = hub
            .wait_for(
                PollRequest {
                    cursor: Some(cursor.clone()),
                    kinds: Some(vec![
                        EventKind::TurnStarted,
                        EventKind::TurnCompleted,
                        EventKind::TurnCancelled,
                        EventKind::TurnFailed,
                        EventKind::SessionBroken,
                        EventKind::ApprovalRequested,
                        EventKind::QuestionRequested,
                    ]),
                    session_id: Some(session_id.to_string()),
                    project_filter: ProjectFilter::from_client(ctx.client),
                    limit: 100,
                },
                remaining.min(TURN_WAIT_SLICE),
            )
            .map_err(map_events_error)?;
        cursor = wake.next_cursor;
    }
}

fn build_poll_request(client: &ResolvedClient, args: &Value) -> Result<PollRequest, ToolError> {
    let cursor = match args.get("cursor") {
        None | Some(Value::Null) => None,
        Some(Value::String(cursor)) => Some(cursor.clone()),
        Some(_) => return Err(ToolError::invalid("cursor must be a string")),
    };
    Ok(PollRequest {
        cursor,
        kinds: parse_type_filter(args.get("types"))?,
        session_id: args
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        project_filter: ProjectFilter::from_client(client),
        limit: args.get("limit").and_then(Value::as_u64).unwrap_or(50) as usize,
    })
}

fn parse_type_filter(value: Option<&Value>) -> Result<Option<Vec<EventKind>>, ToolError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let items = value
        .as_array()
        .ok_or_else(|| ToolError::invalid("types must be an array"))?;
    items
        .iter()
        .map(|item| {
            let name = item
                .as_str()
                .ok_or_else(|| ToolError::invalid("types entries must be strings"))?;
            EventKind::parse(name)
                .ok_or_else(|| ToolError::invalid(format!("unknown event type: {name}")))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn previous_cursor(cursor: &str) -> String {
    match cursor.split_once(':') {
        Some((epoch, seq)) => match seq.parse::<u64>() {
            Ok(seq) => format!("{epoch}:{}", seq.saturating_sub(1)),
            Err(_) => cursor.to_string(),
        },
        None => cursor.to_string(),
    }
}

fn poll_value(result: &PollResult, truncated: bool) -> Value {
    json!({
        "events": result.events,
        "next_cursor": result.next_cursor,
        "reset": result.reset,
        "oldest_cursor": result.oldest_cursor,
        "has_more": result.has_more,
        "truncated": truncated,
        "timed_out": result.timed_out,
    })
}

/// Fits a poll result into the response budget. Dropping events from the tail keeps the cursor
/// honest: `next_cursor` becomes the last retained event (or just before the first dropped one),
/// `has_more` is set, and the client re-polls from there without losing anything.
pub fn shrink_poll_result(mut result: PollResult) -> (PollResult, bool) {
    let first = result.events.first().map(|e| e.cursor.clone());
    let mut truncated = false;
    while !result.events.is_empty()
        && serde_json::to_vec(&poll_value(&result, truncated))
            .map_or(true, |bytes| bytes.len() > MAX_TOOL_RESPONSE_BYTES)
    {
        result.events.pop();
        truncated = true;
    }
    if truncated {
        result.has_more = true;
        result.next_cursor = match (result.events.last(), first) {
            (Some(last), _) => last.cursor.clone(),
            (None, Some(first)) => previous_cursor(&first),
            (None, None) => result.next_cursor,
        };
    }
    (result, truncated)
}

pub fn poll_json_bounded(result: PollResult) -> Value {
    let (result, truncated) = shrink_poll_result(result);
    poll_value(&result, truncated)
}

fn map_events_error(error: EventsError) -> ToolError {
    match error {
        EventsError::MalformedCursor(message) => ToolError::invalid(message),
        EventsError::InvalidResolvedKind => ToolError::internal("invalid resolved event kind"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_mcp::catalog::external_tool_catalog;
    use crate::external_mcp::clients::{ExternalClientRecord, ExternalScope, ProjectScope};
    use chrono::Utc;
    use tempfile::TempDir;

    fn test_client(projects: ProjectScope) -> ResolvedClient {
        ResolvedClient {
            record: ExternalClientRecord {
                id: "client-1".into(),
                name: "test".into(),
                scopes: vec![ExternalScope::Read],
                projects,
                created_at: Utc::now(),
                expires_at: Some(Utc::now() + chrono::Duration::days(1)),
                last_used_at: None,
                revoked_at: None,
            },
        }
    }

    fn ctx<'a>(client: &'a ResolvedClient, tool: &str) -> CallContext<'a> {
        let spec = external_tool_catalog()
            .iter()
            .find(|s| s.name == tool)
            .unwrap();
        CallContext { client, spec }
    }

    fn engine() -> (Engine, TempDir) {
        let dir = TempDir::new().unwrap();
        let (engine, _rx) = Engine::new(vec![], crate::skill::SkillLibrary::new(vec![]));
        engine.external_mcp_state().set_enabled(true);
        engine
            .external_mcp_state()
            .configure_data_dir(dir.path())
            .unwrap();
        (engine, dir)
    }

    #[test]
    fn poll_respects_project_filter_and_null_cursor_is_head() {
        let (engine, dir) = engine();
        let allowed = std::fs::canonicalize(dir.path()).unwrap();
        let child = allowed.join("child");
        std::fs::create_dir(&child).unwrap();
        let outside = TempDir::new().unwrap();
        let ring = engine.external_mcp_state().hub().ring();
        let base = ring.lock().unwrap().head_cursor();
        for (session, project) in [
            ("s-root", &allowed),
            ("s-child", &child),
            ("s-out", &outside.path().to_path_buf()),
        ] {
            ring.lock().unwrap().push(
                EventKind::SessionCreated,
                Some(session.into()),
                Some(project.to_string_lossy().into_owned()),
                None,
                json!({}),
            );
        }
        let client = test_client(ProjectScope::Paths(vec![allowed]));
        let ctx = ctx(&client, TOOL_EVENTS_POLL);
        let head = events_poll(&engine, &ctx, &json!({})).unwrap();
        assert_eq!(
            head["events"].as_array().unwrap().len(),
            0,
            "no cursor = head, no replay"
        );
        let replay = events_poll(&engine, &ctx, &json!({"cursor": base})).unwrap();
        let sessions: Vec<_> = replay["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["session_id"].as_str().unwrap())
            .collect();
        assert_eq!(
            sessions,
            ["s-root", "s-child"],
            "child projects are not dropped"
        );
        assert_eq!(replay["has_more"], false);
        assert_eq!(replay["next_cursor"], ring.lock().unwrap().head_cursor());
    }

    #[test]
    fn wait_times_out_structurally_and_wakes_on_events() {
        let (engine, _dir) = engine();
        let client = test_client(ProjectScope::All);
        let ctx = ctx(&client, TOOL_EVENTS_WAIT);
        let quiet = events_wait(&engine, &ctx, &json!({"timeout_ms": 40})).unwrap();
        assert_eq!(quiet["timed_out"], true);
        assert_eq!(quiet["events"], json!([]));
        assert!(quiet["next_cursor"].as_str().is_some());
        std::thread::scope(|scope| {
            let waiter = scope.spawn(|| events_wait(&engine, &ctx, &json!({"timeout_ms": 5000})));
            std::thread::sleep(Duration::from_millis(40));
            engine
                .external_mcp_state()
                .hub()
                .ring()
                .lock()
                .unwrap()
                .push(
                    EventKind::TurnStarted,
                    Some("s".into()),
                    None,
                    None,
                    json!({}),
                );
            let woke = waiter.join().unwrap().unwrap();
            assert_eq!(woke["timed_out"], false);
            assert_eq!(woke["events"].as_array().unwrap().len(), 1);
        });
    }

    #[test]
    fn bad_arguments_are_invalid_params() {
        let (engine, _dir) = engine();
        let client = test_client(ProjectScope::All);
        let ctx = ctx(&client, TOOL_EVENTS_POLL);
        for args in [
            json!({"types": ["queue.drained"]}),
            json!({"types": "x"}),
            json!({"cursor": 5}),
            json!({"cursor": "bad"}),
        ] {
            assert_eq!(
                events_poll(&engine, &ctx, &args).unwrap_err().kind,
                ToolErrorKind::InvalidParams,
                "{args}"
            );
        }
        assert!(events_poll(&engine, &ctx, &json!({"cursor": null})).is_ok());
    }

    #[test]
    fn oversized_batches_keep_cursor_honest() {
        let (engine, _dir) = engine();
        let ring = engine.external_mcp_state().hub().ring();
        let base = ring.lock().unwrap().head_cursor();
        let big = "x".repeat(60 * 1024);
        for _ in 0..8 {
            ring.lock().unwrap().push(
                EventKind::SubagentUpdated,
                Some("s".into()),
                None,
                None,
                json!({"blob": big}),
            );
        }
        let client = test_client(ProjectScope::All);
        let ctx = ctx(&client, TOOL_EVENTS_POLL);
        let first = events_poll(&engine, &ctx, &json!({"cursor": base, "limit": 50})).unwrap();
        let kept = first["events"].as_array().unwrap().len();
        assert!(kept > 0 && kept < 8);
        assert_eq!(first["truncated"], true);
        assert_eq!(first["has_more"], true);
        assert_eq!(first["next_cursor"], first["events"][kept - 1]["cursor"]);
        assert!(serde_json::to_vec(&first).unwrap().len() <= MAX_TOOL_RESPONSE_BYTES);
        let mut total = kept;
        let mut cursor = first["next_cursor"].as_str().unwrap().to_string();
        for _ in 0..8 {
            let page = events_poll(&engine, &ctx, &json!({"cursor": cursor})).unwrap();
            total += page["events"].as_array().unwrap().len();
            cursor = page["next_cursor"].as_str().unwrap().to_string();
            if page["has_more"] == false {
                break;
            }
        }
        assert_eq!(
            total, 8,
            "paging by next_cursor delivers every event exactly once"
        );
    }

    #[test]
    fn turn_wait_rejects_unknown_and_out_of_scope_sessions() {
        let (engine, dir) = engine();
        let client = test_client(ProjectScope::Paths(vec![
            std::fs::canonicalize(dir.path()).unwrap()
        ]));
        let ctx = ctx(&client, TOOL_TURN_WAIT);
        let error = turn_wait(
            &engine,
            &ctx,
            &json!({"session_id": "nope", "timeout_ms": 10}),
        )
        .unwrap_err();
        assert_eq!(error.kind, ToolErrorKind::NotFound);
    }
}

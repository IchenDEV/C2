//! External MCP event envelope, content policy, and in-memory delivery ring.
//!
//! Ingestion has exactly one authoritative tap: [`EventSender`] wraps the engine's event channel,
//! so every producer (session handlers, the activity tracker, the engine itself) is observed once
//! by [`EventObserver`] before the frontend receives the event. Resolved approval/question
//! receipts have no [`Event`] variant and enter through [`EventRing::push_resolved`].
//!
//! Cursor model: a cursor `<epoch>:<seq>` means "the consumer has seen every event up to `seq`".
//! `cursor: None` means *head* (no replay). Events are replayable only while retained; a cursor
//! with `seq + 1 < oldest` (a gap) or from another epoch (restart) yields a non-error `reset`
//! whose `next_cursor` is positioned just before the oldest retained event.

use crate::event::Event;
use crate::external_mcp::clients::{ProjectScope, ResolvedClient};
use crate::permission::PermissionContextKind;
use crate::provider::ProviderId;
use crate::provider_runtime::ThreadDisposition;
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::sync::Notify;
use uuid::Uuid;

pub const ENVELOPE_VERSION: u8 = 1;
pub const DEFAULT_RING_CAPACITY: usize = 2048;
pub const DEFAULT_TEXT_MAX_CHARS: usize = 120;
pub const MAX_POLL_LIMIT: usize = 100;
pub const MAX_WAIT_SECS: u64 = 55;
/// Stable MCP resource URI for the event ring (`resources/subscribe` / `subscriptions/listen`).
pub const EVENTS_RESOURCE_URI: &str = "codetwo://events";
/// Tool/MCP JSON response budget (matches external MCP transport limit).
pub const MAX_TOOL_RESPONSE_BYTES: usize = 256 * 1024;

/// Stable, versioned event kinds for external MCP consumers. Only kinds the engine really
/// produces are listed: there is no engine event for "queue drained" or "automation run
/// finished", so those kinds are intentionally absent rather than advertised and never sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventKind {
    #[serde(rename = "turn.started")]
    TurnStarted,
    #[serde(rename = "turn.completed")]
    TurnCompleted,
    #[serde(rename = "turn.cancelled")]
    TurnCancelled,
    #[serde(rename = "turn.failed")]
    TurnFailed,
    #[serde(rename = "approval.requested")]
    ApprovalRequested,
    #[serde(rename = "approval.resolved")]
    ApprovalResolved,
    #[serde(rename = "question.requested")]
    QuestionRequested,
    #[serde(rename = "question.resolved")]
    QuestionResolved,
    #[serde(rename = "session.created")]
    SessionCreated,
    #[serde(rename = "session.updated")]
    SessionUpdated,
    #[serde(rename = "session.broken")]
    SessionBroken,
    #[serde(rename = "subagent.updated")]
    SubagentUpdated,
}

impl EventKind {
    pub const ALL: [EventKind; 12] = [
        Self::TurnStarted,
        Self::TurnCompleted,
        Self::TurnCancelled,
        Self::TurnFailed,
        Self::ApprovalRequested,
        Self::ApprovalResolved,
        Self::QuestionRequested,
        Self::QuestionResolved,
        Self::SessionCreated,
        Self::SessionUpdated,
        Self::SessionBroken,
        Self::SubagentUpdated,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::TurnStarted => "turn.started",
            Self::TurnCompleted => "turn.completed",
            Self::TurnCancelled => "turn.cancelled",
            Self::TurnFailed => "turn.failed",
            Self::ApprovalRequested => "approval.requested",
            Self::ApprovalResolved => "approval.resolved",
            Self::QuestionRequested => "question.requested",
            Self::QuestionResolved => "question.resolved",
            Self::SessionCreated => "session.created",
            Self::SessionUpdated => "session.updated",
            Self::SessionBroken => "session.broken",
            Self::SubagentUpdated => "subagent.updated",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == name)
    }

    /// Kinds that end a turn from the consumer's point of view.
    pub fn is_turn_terminal(self) -> bool {
        matches!(
            self,
            Self::TurnCompleted | Self::TurnCancelled | Self::TurnFailed | Self::SessionBroken
        )
    }
}

/// Wire envelope (`v1`). The JSON key for [`EventKind`] is `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub v: u8,
    pub cursor: String,
    pub ts: String,
    #[serde(rename = "type")]
    pub kind: EventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EventsError {
    #[error("malformed cursor: {0}")]
    MalformedCursor(String),
    #[error("push_resolved kind must be approval.resolved or question.resolved")]
    InvalidResolvedKind,
}

/// Project visibility for poll/wait. Matching uses the same canonical prefix rule as
/// [`crate::external_mcp::clients::ExternalClientRegistry::project_allowed`]: an event is visible
/// when its canonical project path lies inside one allowed root, so child directories are not
/// dropped. Events without a resolvable project are only visible to [`ProjectFilter::All`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectFilter {
    All,
    /// Canonical allowed roots. An empty list allows nothing (fail closed).
    Allow(Vec<PathBuf>),
}

impl ProjectFilter {
    pub fn from_scope(scope: &ProjectScope) -> Self {
        match scope {
            ProjectScope::All => Self::All,
            ProjectScope::Paths(paths) => Self::Allow(
                paths
                    .iter()
                    .filter_map(|path| std::fs::canonicalize(path).ok())
                    .collect(),
            ),
        }
    }

    pub fn from_client(client: &ResolvedClient) -> Self {
        Self::from_scope(&client.record.projects)
    }

    fn allows_canonical(&self, project: Option<&Path>) -> bool {
        match self {
            Self::All => true,
            Self::Allow(roots) => {
                project.is_some_and(|project| roots.iter().any(|root| project.starts_with(root)))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollRequest {
    /// `None` means head: no replay, the result carries the current head cursor.
    pub cursor: Option<String>,
    pub kinds: Option<Vec<EventKind>>,
    pub session_id: Option<String>,
    pub project_filter: ProjectFilter,
    pub limit: usize,
}

impl PollRequest {
    pub fn clamped_limit(&self) -> usize {
        self.limit.clamp(1, MAX_POLL_LIMIT)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PollResult {
    pub events: Vec<EventEnvelope>,
    /// Cursor to pass next. Last returned event when `has_more`, otherwise the head the scan
    /// reached (so quiet or fully filtered polls still advance). After a reset it is positioned
    /// before the oldest retained event.
    pub next_cursor: String,
    /// The supplied cursor fell into a gap (evicted events) or belongs to another epoch.
    /// Never an error: continue from `next_cursor`.
    pub reset: bool,
    /// Cursor positioned before the oldest retained event; `None` while the ring is empty.
    pub oldest_cursor: Option<String>,
    /// More matching events are retained beyond this batch.
    pub has_more: bool,
    /// Set only by the waiting entry points when the wait ended without events.
    pub timed_out: bool,
}

/// Strip control characters and truncate on UTF-8 char boundaries (ellipsis when truncated).
pub fn sanitize_text(input: &str, max_chars: usize) -> String {
    let max_chars = max_chars.max(1);
    let cleaned: String = input.chars().filter(|ch| !ch.is_control()).collect();
    let mut chars = cleaned.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        const ELLIPSIS: &str = "…";
        let keep = max_chars.saturating_sub(ELLIPSIS.chars().count());
        let mut out: String = truncated.chars().take(keep).collect();
        out.push('…');
        out
    } else {
        truncated
    }
}

/// One mapped event before ring assignment: kind, session, project, turn, data.
pub type MappedEvent = (
    EventKind,
    Option<String>,
    Option<String>,
    Option<String>,
    Value,
);

/// Maps one engine [`Event`] into zero or more external envelopes without turn correlation.
pub fn map_event(event: &Event, resolver: &dyn Fn(&str) -> Option<String>) -> Vec<MappedEvent> {
    map_event_with(event, resolver, &|_| None)
}

/// Maps one engine [`Event`]; `project_of` resolves a session's project, `turn_of` its current
/// (or last) turn id, so every turn-scoped envelope carries the real turn id.
pub fn map_event_with(
    event: &Event,
    project_of: &dyn Fn(&str) -> Option<String>,
    turn_of: &dyn Fn(&str) -> Option<String>,
) -> Vec<MappedEvent> {
    let mut out = Vec::new();
    match event {
        Event::TurnStarted {
            session,
            request_id,
            ..
        } => out.push((
            EventKind::TurnStarted,
            Some(session.clone()),
            project_of(session),
            turn_of(session),
            json!({ "request_id": request_id.as_deref().map(|id| sanitize_text(id, DEFAULT_TEXT_MAX_CHARS)) }),
        )),
        Event::TurnEnded {
            session,
            stop_reason,
        } => out.push((
            turn_kind_from_stop_reason(stop_reason),
            Some(session.clone()),
            project_of(session),
            turn_of(session),
            json!({ "stop_reason": sanitize_text(stop_reason, 64) }),
        )),
        Event::Error {
            session,
            terminal: true,
            ..
        } => {
            let session_id = session.clone();
            let project_id = session_id.as_deref().and_then(project_of);
            let turn_id = session_id.as_deref().and_then(turn_of);
            out.push((
                EventKind::TurnFailed,
                session_id,
                project_id,
                turn_id,
                json!({ "terminal": true }),
            ));
        }
        Event::PermissionRequest {
            session,
            request_id,
            context,
            ..
        } => out.push(approval_requested(
            session,
            request_id,
            context,
            project_of(session),
            turn_of(session),
        )),
        Event::ElicitationRequest {
            session,
            request_id,
            form,
        } => out.push(question_requested(
            session,
            request_id,
            form.questions().count(),
            project_of(session),
            turn_of(session),
        )),
        Event::ThreadDisposition {
            session,
            disposition: ThreadDisposition::Broken,
        } => out.push((
            EventKind::SessionBroken,
            Some(session.clone()),
            project_of(session),
            turn_of(session),
            json!({}),
        )),
        Event::ThreadDisposition {
            disposition: ThreadDisposition::Reusable,
            ..
        } => {}
        Event::SubagentUpdated { session, subagent } => {
            let title = subagent
                .title
                .as_deref()
                .map(|t| sanitize_text(t, DEFAULT_TEXT_MAX_CHARS));
            out.push((
                EventKind::SubagentUpdated,
                Some(session.clone()),
                project_of(session),
                turn_of(session),
                json!({
                    "id": subagent.id,
                    "status": serde_json::to_value(subagent.status).unwrap_or(Value::Null),
                    "title": title,
                }),
            ));
        }
        Event::SessionCreated { session, .. } => out.push((
            EventKind::SessionCreated,
            Some(session.clone()),
            project_of(session),
            None,
            json!({}),
        )),
        Event::SessionTitleChanged { session, title } => out.push((
            EventKind::SessionUpdated,
            Some(session.clone()),
            project_of(session),
            None,
            json!({ "title": sanitize_text(title, DEFAULT_TEXT_MAX_CHARS) }),
        )),
        Event::ProviderChanged {
            session,
            provider,
            model,
        } => out.push((
            EventKind::SessionUpdated,
            Some(session.clone()),
            project_of(session),
            None,
            json!({
                "provider": provider_id_label(provider.clone()),
                "model": model.as_deref().map(|m| sanitize_text(m, DEFAULT_TEXT_MAX_CHARS)),
            }),
        )),
        _ => {}
    }
    out
}

/// `approval.requested` envelope parts. Shared by the engine-event mapping and by the tap, which
/// derives the same envelope from the authoritative pending snapshot.
pub(crate) fn approval_requested(
    session: &str,
    request_id: &str,
    context: &crate::permission::PermissionContext,
    project: Option<String>,
    turn: Option<String>,
) -> MappedEvent {
    let tool_name = context
        .tool
        .as_deref()
        .filter(|t| is_safe_tool_name(t))
        .map(|t| sanitize_text(t, DEFAULT_TEXT_MAX_CHARS))
        .unwrap_or_default();
    (
        EventKind::ApprovalRequested,
        Some(session.to_owned()),
        project,
        turn,
        json!({
            "request_id": request_id,
            "tool_name": tool_name,
            "kind": permission_context_kind_label(context.kind),
        }),
    )
}

pub(crate) fn question_requested(
    session: &str,
    request_id: &str,
    question_count: usize,
    project: Option<String>,
    turn: Option<String>,
) -> MappedEvent {
    (
        EventKind::QuestionRequested,
        Some(session.to_owned()),
        project,
        turn,
        json!({ "request_id": request_id, "question_count": question_count }),
    )
}

/// Provider stop reasons arrive as `EndTurn` (turn task), `end_turn` / `provider_error` (external
/// turns) and `Cancelled`; compare on letters only so spelling variants map identically.
fn turn_kind_from_stop_reason(stop_reason: &str) -> EventKind {
    let normalized: String = stop_reason
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|ch| ch.to_ascii_lowercase())
        .collect();
    match normalized.as_str() {
        "cancelled" | "canceled" => EventKind::TurnCancelled,
        "endturn" | "idle" => EventKind::TurnCompleted,
        _ => EventKind::TurnFailed,
    }
}

fn permission_context_kind_label(kind: PermissionContextKind) -> &'static str {
    match kind {
        PermissionContextKind::Acp => "acp",
        PermissionContextKind::McpElicitation => "mcp_elicitation",
        PermissionContextKind::WebsiteAccess => "website_access",
        PermissionContextKind::SensitiveWebAction => "sensitive_web_action",
        PermissionContextKind::ComputerUseApplication => "computer_use_application",
        PermissionContextKind::SitesMutation => "sites_mutation",
        PermissionContextKind::SitesProduction => "sites_production",
    }
}

fn provider_id_label(provider: ProviderId) -> String {
    serde_json::to_value(provider)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

#[derive(Debug, Clone)]
struct StoredEvent {
    seq: u64,
    envelope: EventEnvelope,
    /// Canonical project path resolved once at ingestion; `None` when absent or unresolvable.
    canonical_project: Option<PathBuf>,
}

/// Bounded in-memory ring with monotonic sequence and a random per-process epoch.
pub struct EventRing {
    capacity: usize,
    epoch: u64,
    seq: u64,
    events: VecDeque<StoredEvent>,
    notify: Arc<Notify>,
    /// Wakes blocking waiters; always used with this ring's own mutex.
    waiters: Arc<Condvar>,
}

impl Default for EventRing {
    fn default() -> Self {
        Self::new(DEFAULT_RING_CAPACITY)
    }
}

impl EventRing {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            epoch: random_epoch(),
            seq: 0,
            events: VecDeque::new(),
            notify: Arc::new(Notify::new()),
            waiters: Arc::new(Condvar::new()),
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn subscribe(&self) -> EventRingSubscription {
        EventRingSubscription {
            notify: Arc::clone(&self.notify),
        }
    }

    /// Cursor meaning "now": a consumer holding it has seen everything pushed so far.
    pub fn head_cursor(&self) -> String {
        format!("{}:{}", self.epoch, self.seq)
    }

    /// Cursor positioned before the oldest retained event (replays the whole ring).
    pub fn oldest_cursor(&self) -> Option<String> {
        self.events
            .front()
            .map(|stored| format!("{}:{}", self.epoch, stored.seq - 1))
    }

    /// Push one mapped envelope. Returns the stored wire shape.
    pub fn push(
        &mut self,
        kind: EventKind,
        session_id: Option<String>,
        project_id: Option<String>,
        turn_id: Option<String>,
        data: Value,
    ) -> EventEnvelope {
        self.seq = self.seq.saturating_add(1);
        let canonical_project = project_id
            .as_deref()
            .and_then(|project| std::fs::canonicalize(project).ok());
        let envelope = EventEnvelope {
            v: ENVELOPE_VERSION,
            cursor: format!("{}:{}", self.epoch, self.seq),
            ts: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            kind,
            session_id,
            project_id,
            turn_id,
            data,
        };
        if self.events.len() >= self.capacity {
            self.events.pop_front();
        }
        self.events.push_back(StoredEvent {
            seq: self.seq,
            envelope: envelope.clone(),
            canonical_project,
        });
        self.notify.notify_waiters();
        self.waiters.notify_all();
        envelope
    }

    /// Resolution of an approval/question has no engine [`Event`] variant; it enters here.
    pub fn push_resolved(
        &mut self,
        kind: EventKind,
        session_id: Option<String>,
        project_id: Option<String>,
        turn_id: Option<String>,
        data: Value,
    ) -> Result<EventEnvelope, EventsError> {
        match kind {
            EventKind::ApprovalResolved | EventKind::QuestionResolved => {}
            _ => return Err(EventsError::InvalidResolvedKind),
        }
        Ok(self.push(kind, session_id, project_id, turn_id, data))
    }

    pub fn ingest_mapped(
        &mut self,
        mapped: impl IntoIterator<Item = MappedEvent>,
    ) -> Vec<EventEnvelope> {
        mapped
            .into_iter()
            .map(|(kind, session, project, turn, data)| self.push(kind, session, project, turn, data))
            .collect()
    }

    /// Newest retained terminal event of a turn (completed/cancelled/failed/broken), if any.
    pub fn terminal_for_turn(&self, session_id: &str, turn_id: &str) -> Option<EventKind> {
        self.events
            .iter()
            .rev()
            .map(|stored| &stored.envelope)
            .filter(|env| {
                env.kind.is_turn_terminal()
                    && env.session_id.as_deref() == Some(session_id)
                    && env.turn_id.as_deref() == Some(turn_id)
            })
            .map(|env| env.kind)
            .max_by_key(|kind| u8::from(*kind == EventKind::SessionBroken))
    }

    /// Newest retained `turn.started` of a session whose prompt request id equals `request_id`,
    /// so a `turn_send` receipt (delivery id) can be correlated with its turn.
    pub fn turn_for_request(&self, session_id: &str, request_id: &str) -> Option<String> {
        self.events
            .iter()
            .rev()
            .map(|stored| &stored.envelope)
            .find(|env| {
                env.kind == EventKind::TurnStarted
                    && env.session_id.as_deref() == Some(session_id)
                    && env.data.get("request_id").and_then(Value::as_str) == Some(request_id)
            })
            .and_then(|env| env.turn_id.clone())
    }

    pub fn poll(&self, request: PollRequest) -> Result<PollResult, EventsError> {
        let limit = request.clamped_limit();
        let head = self.head_cursor();
        let oldest_cursor = self.oldest_cursor();
        let quiet = |next_cursor: String, reset: bool| PollResult {
            events: Vec::new(),
            next_cursor,
            reset,
            oldest_cursor: oldest_cursor.clone(),
            has_more: false,
            timed_out: false,
        };

        let after_seq = match &request.cursor {
            None => return Ok(quiet(head, false)),
            Some(cursor) => {
                let (epoch, seq) = parse_cursor(cursor)?;
                let gap = self
                    .events
                    .front()
                    .is_some_and(|oldest| seq.saturating_add(1) < oldest.seq);
                if epoch != self.epoch || seq > self.seq || gap {
                    let resume = oldest_cursor.clone().unwrap_or(head);
                    return Ok(quiet(resume, true));
                }
                seq
            }
        };

        let matches = |stored: &&StoredEvent| {
            let env = &stored.envelope;
            stored.seq > after_seq
                && request
                    .session_id
                    .as_deref()
                    .is_none_or(|session| env.session_id.as_deref() == Some(session))
                && request
                    .project_filter
                    .allows_canonical(stored.canonical_project.as_deref())
                && request
                    .kinds
                    .as_ref()
                    .is_none_or(|kinds| kinds.contains(&env.kind))
        };

        let mut matching = self.events.iter().filter(matches);
        let events: Vec<EventEnvelope> = matching
            .by_ref()
            .take(limit)
            .map(|stored| stored.envelope.clone())
            .collect();
        let has_more = matching.next().is_some();
        let next_cursor = match (has_more, events.last()) {
            (true, Some(last)) => last.cursor.clone(),
            _ => head,
        };
        Ok(PollResult {
            events,
            next_cursor,
            reset: false,
            oldest_cursor,
            has_more,
            timed_out: false,
        })
    }
}

/// Awaitable wake handle paired with [`EventRing::subscribe`].
#[derive(Clone)]
pub struct EventRingSubscription {
    notify: Arc<Notify>,
}

impl EventRingSubscription {
    /// Future that completes on the next push. Create it **before** polling the ring and await it
    /// after: `Notify::notify_waiters` wakes every future created earlier, so no push between the
    /// poll and the await is lost.
    pub fn notified(&self) -> tokio::sync::futures::Notified<'_> {
        self.notify.notified()
    }
}

/// Shared ring plus the mapping entry points.
#[derive(Clone)]
pub struct EventHub {
    ring: Arc<Mutex<EventRing>>,
}

fn lock_ring(ring: &Mutex<EventRing>) -> MutexGuard<'_, EventRing> {
    ring.lock().expect("event ring mutex poisoned")
}

impl EventHub {
    pub fn new(ring: EventRing) -> Self {
        Self {
            ring: Arc::new(Mutex::new(ring)),
        }
    }

    pub fn from_shared(ring: Arc<Mutex<EventRing>>) -> Self {
        Self { ring }
    }

    pub fn ring(&self) -> Arc<Mutex<EventRing>> {
        Arc::clone(&self.ring)
    }

    pub fn subscribe(&self) -> EventRingSubscription {
        lock_ring(&self.ring).subscribe()
    }

    pub fn head_cursor(&self) -> String {
        lock_ring(&self.ring).head_cursor()
    }

    pub fn poll(&self, request: PollRequest) -> Result<PollResult, EventsError> {
        lock_ring(&self.ring).poll(request)
    }

    /// Blocking wait. The ring lock is released while parked, so producers are never blocked.
    /// A missing cursor means "events after now". Ends with `timed_out` instead of an error.
    /// Callers on a Tokio worker must use `spawn_blocking`.
    pub fn wait_for(
        &self,
        mut request: PollRequest,
        timeout: Duration,
    ) -> Result<PollResult, EventsError> {
        let deadline = Instant::now() + timeout.min(Duration::from_secs(MAX_WAIT_SECS));
        let mut guard = lock_ring(&self.ring);
        let waiters = Arc::clone(&guard.waiters);
        request.cursor.get_or_insert_with(|| guard.head_cursor());
        loop {
            let mut result = guard.poll(request.clone())?;
            if result.reset || !result.events.is_empty() {
                return Ok(result);
            }
            request.cursor = Some(result.next_cursor.clone());
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                result.timed_out = true;
                return Ok(result);
            }
            guard = waiters
                .wait_timeout(guard, remaining)
                .expect("event ring mutex poisoned")
                .0;
        }
    }

    /// Async wait with the wake future armed before every poll (no lost wake-ups).
    pub async fn wait_for_async(
        &self,
        mut request: PollRequest,
        timeout: Duration,
    ) -> Result<PollResult, EventsError> {
        let deadline = tokio::time::Instant::now() + timeout.min(Duration::from_secs(MAX_WAIT_SECS));
        let subscription = self.subscribe();
        loop {
            let notified = subscription.notified();
            let mut result = {
                let guard = lock_ring(&self.ring);
                request.cursor.get_or_insert_with(|| guard.head_cursor());
                guard.poll(request.clone())?
            };
            if result.reset || !result.events.is_empty() {
                return Ok(result);
            }
            request.cursor = Some(result.next_cursor.clone());
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                result.timed_out = true;
                return Ok(result);
            }
        }
    }

    pub fn ingest(
        &self,
        event: &Event,
        resolver: &dyn Fn(&str) -> Option<String>,
    ) -> Vec<EventEnvelope> {
        self.ingest_with(event, resolver, &|_| None)
    }

    pub fn ingest_with(
        &self,
        event: &Event,
        project_of: &dyn Fn(&str) -> Option<String>,
        turn_of: &dyn Fn(&str) -> Option<String>,
    ) -> Vec<EventEnvelope> {
        // Resolve outside the ring lock: resolvers may touch other locks.
        let mapped = map_event_with(event, project_of, turn_of);
        lock_ring(&self.ring).ingest_mapped(mapped)
    }
}

/// Observer notified once for every event that enters the engine channel.
pub trait EventObserver: Send + Sync {
    fn observe(&self, event: &Event);
}

/// The engine's event sender. Cloning is cheap; all clones share the same tap, which makes the
/// tap the single ingestion point regardless of which component produced the event.
#[derive(Clone)]
pub struct EventSender {
    tx: mpsc::UnboundedSender<Event>,
    tap: Option<Arc<dyn EventObserver>>,
}

impl EventSender {
    pub fn tapped(tx: mpsc::UnboundedSender<Event>, tap: Arc<dyn EventObserver>) -> Self {
        Self { tx, tap: Some(tap) }
    }

    pub fn send(&self, event: Event) -> Result<(), mpsc::error::SendError<Event>> {
        if let Some(tap) = &self.tap {
            tap.observe(&event);
        }
        self.tx.send(event)
    }
}

impl From<mpsc::UnboundedSender<Event>> for EventSender {
    fn from(tx: mpsc::UnboundedSender<Event>) -> Self {
        Self { tx, tap: None }
    }
}

/// Tool identifiers only — no paths, arguments, or free-form command text.
fn is_safe_tool_name(tool: &str) -> bool {
    !tool.is_empty()
        && tool.len() <= DEFAULT_TEXT_MAX_CHARS
        && tool
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

fn random_epoch() -> u64 {
    Uuid::new_v4().as_u128() as u64 ^ (Uuid::new_v4().as_u128() >> 64) as u64
}

fn parse_cursor(cursor: &str) -> Result<(u64, u64), EventsError> {
    let malformed = || EventsError::MalformedCursor(cursor.to_owned());
    let (epoch, seq) = cursor.split_once(':').ok_or_else(malformed)?;
    Ok((
        epoch.parse().map_err(|_| malformed())?,
        seq.parse().map_err(|_| malformed())?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elicitation::{ElicitationField, ElicitationFieldKind, ElicitationForm};
    use crate::permission::PermissionContext;
    use crate::subagent::{SubagentOrigin, SubagentRun, SubagentStatus};
    use crate::ProviderId;

    fn resolver(project: &str) -> impl Fn(&str) -> Option<String> {
        let project = project.to_owned();
        move |session: &str| (!session.starts_with("noproj-")).then(|| project.clone())
    }

    fn request(cursor: Option<&str>, limit: usize) -> PollRequest {
        PollRequest {
            cursor: cursor.map(str::to_owned),
            kinds: None,
            session_id: None,
            project_filter: ProjectFilter::All,
            limit,
        }
    }

    fn push(ring: &mut EventRing, kind: EventKind) -> EventEnvelope {
        ring.push(kind, Some("s".into()), None, None, json!({}))
    }

    #[test]
    fn sanitize_text_strips_control_and_truncates_on_char_boundary() {
        assert_eq!(sanitize_text("hello", 10), "hello");
        assert_eq!(sanitize_text("a\u{0001}b", 10), "ab");
        let out = sanitize_text(&"é".repeat(130), 120);
        assert!(out.chars().count() <= 120);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn kind_names_round_trip_and_match_serde() {
        for kind in EventKind::ALL {
            assert_eq!(EventKind::parse(kind.as_str()), Some(kind));
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(kind.as_str()));
        }
        assert_eq!(EventKind::parse("queue.drained"), None);
        assert_eq!(EventKind::parse("automation.run_finished"), None);
    }

    #[test]
    fn envelope_serializes_type_key() {
        let env = EventEnvelope {
            v: 1,
            cursor: "1:1".into(),
            ts: "2026-01-01T00:00:00Z".into(),
            kind: EventKind::TurnCompleted,
            session_id: Some("s1".into()),
            project_id: None,
            turn_id: None,
            data: json!({}),
        };
        let value = serde_json::to_value(&env).unwrap();
        assert_eq!(value["type"], "turn.completed");
        assert_eq!(value["v"], 1);
    }

    #[test]
    fn turn_end_reasons_map_every_spelling_the_engine_emits() {
        let r = resolver("/proj");
        let kind = |reason: &str| {
            map_event(
                &Event::TurnEnded {
                    session: "s".into(),
                    stop_reason: reason.into(),
                },
                &r,
            )[0]
            .0
        };
        for completed in ["EndTurn", "end_turn", "idle", "End-Turn"] {
            assert_eq!(kind(completed), EventKind::TurnCompleted, "{completed}");
        }
        for cancelled in ["Cancelled", "cancelled"] {
            assert_eq!(kind(cancelled), EventKind::TurnCancelled, "{cancelled}");
        }
        for failed in ["Refusal", "provider_error", "Unknown", "MaxTokens"] {
            assert_eq!(kind(failed), EventKind::TurnFailed, "{failed}");
        }
    }

    #[test]
    fn mapped_turn_events_carry_turn_id_and_receipt_request_id() {
        let mapped = map_event_with(
            &Event::TurnStarted {
                session: "s1".into(),
                request_id: Some("delivery-1".into()),
                transcript_seq: None,
            },
            &resolver("/proj"),
            &|_| Some("turn-9".into()),
        );
        assert_eq!(mapped[0].3.as_deref(), Some("turn-9"));
        assert_eq!(mapped[0].4["request_id"], "delivery-1");
    }

    #[test]
    fn map_permission_and_question_without_leaks() {
        let r = resolver("/proj");
        let secret_cmd = "rm -rf /secret/path --api-key=SUPERSECRET";
        let perm = map_event(
            &Event::PermissionRequest {
                session: "s1".into(),
                request_id: "req-1".into(),
                title: secret_cmd.into(),
                options: vec![("allow".into(), secret_cmd.into())],
                context: PermissionContext {
                    kind: PermissionContextKind::Acp,
                    tool: Some("run_terminal".into()),
                    ..Default::default()
                },
            },
            &r,
        );
        let wire = serde_json::to_string(&perm[0].4).unwrap();
        assert!(!wire.contains("SUPERSECRET") && !wire.contains("/secret/path"));
        assert_eq!(perm[0].4["tool_name"], "run_terminal");

        let form = ElicitationForm {
            message: "What is your password?".into(),
            tool_call_id: None,
            fields: vec![ElicitationField {
                key: "q1".into(),
                kind: ElicitationFieldKind::Text,
                title: Some("Password".into()),
                description: None,
                required: true,
                options: vec![],
                custom_answer_for: None,
            }],
        };
        let q = map_event(
            &Event::ElicitationRequest {
                session: "s1".into(),
                request_id: "el-1".into(),
                form,
            },
            &r,
        );
        assert!(!serde_json::to_string(&q[0].4).unwrap().contains("password"));
        assert_eq!(q[0].4["question_count"], 1);
    }

    #[test]
    fn content_policy_agent_and_tool_fields_never_in_envelope() {
        let r = resolver("/proj");
        let hub = EventHub::new(EventRing::new(64));
        let secret = "PROMPT_LEAK_42";
        hub.ingest(
            &Event::AgentText {
                session: "s1".into(),
                message_id: "m1".into(),
                text: secret.into(),
                transcript_seq: None,
            },
            &r,
        );
        hub.ingest(
            &Event::ToolCall {
                session: "s1".into(),
                id: "t1".into(),
                title: secret.into(),
                status: "running".into(),
                kind: None,
                agent_input: Some(json!({ "command": secret })),
                outputs: vec![],
                transcript_seq: None,
            },
            &r,
        );
        assert!(hub.ring().lock().unwrap().events.is_empty());

        let mapped = map_event(
            &Event::PermissionRequest {
                session: "s1".into(),
                request_id: "p1".into(),
                title: secret.into(),
                options: vec![],
                context: PermissionContext {
                    tool: Some(format!("diff {secret}")),
                    ..Default::default()
                },
            },
            &r,
        );
        let env = EventRing::new(8).push(
            mapped[0].0,
            mapped[0].1.clone(),
            mapped[0].2.clone(),
            mapped[0].3.clone(),
            mapped[0].4.clone(),
        );
        assert!(!serde_json::to_string(&env).unwrap().contains(secret));
    }

    #[test]
    fn null_cursor_means_head_without_replay() {
        let mut ring = EventRing::new(8);
        let empty = ring.poll(request(None, 10)).unwrap();
        assert_eq!(empty.next_cursor, ring.head_cursor());
        push(&mut ring, EventKind::TurnStarted);
        let head = ring.poll(request(None, 10)).unwrap();
        assert!(head.events.is_empty() && !head.reset);
        assert_eq!(head.next_cursor, ring.head_cursor());
        let replay = ring.poll(request(ring.oldest_cursor().as_deref(), 10)).unwrap();
        assert_eq!(replay.events.len(), 1);
    }

    #[test]
    fn gap_condition_is_seq_plus_one_below_oldest() {
        let mut ring = EventRing::new(3);
        let cursors: Vec<String> = (0..5)
            .map(|_| push(&mut ring, EventKind::TurnStarted).cursor)
            .collect();
        // Retained seqs are 3,4,5. Cursor at seq 2 is contiguous (next event is 3): no gap.
        let contiguous = ring.poll(request(Some(&cursors[1]), 10)).unwrap();
        assert!(!contiguous.reset);
        assert_eq!(contiguous.events.len(), 3);
        // Cursor at seq 1 lost event 2: gap, non-error, resumes before the oldest retained event.
        let gap = ring.poll(request(Some(&cursors[0]), 10)).unwrap();
        assert!(gap.reset && gap.events.is_empty());
        assert_eq!(gap.next_cursor, format!("{}:2", ring.epoch()));
        assert_eq!(gap.oldest_cursor.as_deref(), Some(gap.next_cursor.as_str()));
        let resumed = ring.poll(request(Some(&gap.next_cursor), 10)).unwrap();
        assert_eq!(resumed.events.len(), 3);
    }

    #[test]
    fn foreign_epoch_and_future_cursor_reset() {
        let mut ring = EventRing::new(8);
        push(&mut ring, EventKind::TurnStarted);
        for cursor in ["999999:1".to_string(), format!("{}:99", ring.epoch())] {
            let result = ring.poll(request(Some(&cursor), 10)).unwrap();
            assert!(result.reset && result.events.is_empty(), "{cursor}");
            assert_eq!(result.next_cursor, ring.oldest_cursor().unwrap());
        }
        let empty = EventRing::new(2);
        let result = empty.poll(request(Some("1:5"), 10)).unwrap();
        assert!(result.reset);
        assert_eq!(result.next_cursor, empty.head_cursor());
    }

    #[test]
    fn has_more_and_next_cursor_track_limit_and_filters() {
        let mut ring = EventRing::new(16);
        for _ in 0..3 {
            push(&mut ring, EventKind::TurnStarted);
        }
        push(&mut ring, EventKind::TurnCompleted);
        let base = ring.oldest_cursor().unwrap();
        let first = ring.poll(request(Some(&base), 2)).unwrap();
        assert!(first.has_more);
        assert_eq!(first.next_cursor, first.events.last().unwrap().cursor);
        let rest = ring.poll(request(Some(&first.next_cursor), 2)).unwrap();
        assert!(!rest.has_more);
        assert_eq!(rest.events.len(), 2);
        assert_eq!(rest.next_cursor, ring.head_cursor());

        let mut only_completed = request(Some(&base), 1);
        only_completed.kinds = Some(vec![EventKind::TurnCompleted]);
        let filtered = ring.poll(only_completed).unwrap();
        assert_eq!(filtered.events.len(), 1);
        assert!(!filtered.has_more, "non-matching events are not 'more'");
        let mut nothing = request(Some(&base), 5);
        nothing.session_id = Some("other".into());
        let quiet = ring.poll(nothing).unwrap();
        assert!(quiet.events.is_empty());
        assert_eq!(quiet.next_cursor, ring.head_cursor(), "quiet poll advances to head");
    }

    #[test]
    fn project_filter_uses_canonical_prefix_including_children() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let child = root.join("child");
        std::fs::create_dir(&child).unwrap();
        let sibling = tempfile::TempDir::new().unwrap();
        let mut ring = EventRing::new(16);
        let project = |path: &Path| Some(path.to_string_lossy().into_owned());
        ring.push(EventKind::SessionCreated, Some("root".into()), project(&root), None, json!({}));
        ring.push(EventKind::SessionCreated, Some("child".into()), project(&child), None, json!({}));
        ring.push(EventKind::SessionCreated, Some("other".into()), project(sibling.path()), None, json!({}));
        ring.push(EventKind::SessionCreated, Some("gone".into()), project(&root.join("missing")), None, json!({}));
        ring.push(EventKind::SessionCreated, Some("none".into()), None, None, json!({}));
        let base = ring.oldest_cursor();
        let mut scoped = request(base.as_deref(), 10);
        scoped.project_filter = ProjectFilter::Allow(vec![root]);
        let sessions: Vec<_> = ring
            .poll(scoped)
            .unwrap()
            .events
            .into_iter()
            .map(|e| e.session_id.unwrap())
            .collect();
        assert_eq!(sessions, ["root", "child"]);
        let mut empty_scope = request(base.as_deref(), 10);
        empty_scope.project_filter = ProjectFilter::Allow(Vec::new());
        assert!(ring.poll(empty_scope).unwrap().events.is_empty());
        assert_eq!(ring.poll(request(base.as_deref(), 10)).unwrap().events.len(), 5);
    }

    #[test]
    fn terminal_and_receipt_lookup_by_turn() {
        let mut ring = EventRing::new(16);
        ring.push(
            EventKind::TurnStarted,
            Some("s".into()),
            None,
            Some("t1".into()),
            json!({"request_id": "d1"}),
        );
        assert_eq!(ring.turn_for_request("s", "d1").as_deref(), Some("t1"));
        assert_eq!(ring.turn_for_request("s", "nope"), None);
        assert_eq!(ring.terminal_for_turn("s", "t1"), None);
        ring.push(EventKind::TurnFailed, Some("s".into()), None, Some("t1".into()), json!({}));
        ring.push(EventKind::SessionBroken, Some("s".into()), None, Some("t1".into()), json!({}));
        assert_eq!(ring.terminal_for_turn("s", "t1"), Some(EventKind::SessionBroken));
        assert_eq!(ring.terminal_for_turn("s", "t2"), None);
    }

    #[test]
    fn subagent_and_session_mappings_have_no_leaks() {
        let r = resolver("/p");
        let sub = map_event(
            &Event::SubagentUpdated {
                session: "s".into(),
                subagent: SubagentRun {
                    id: "sub-1".into(),
                    parent_tool_call_id: "tc".into(),
                    provider: ProviderId::Codex,
                    origin: SubagentOrigin::ProviderNative,
                    title: Some("long title".into()),
                    prompt_summary: Some("secret summary".into()),
                    model: None,
                    status: SubagentStatus::Running,
                    progress: None,
                    result_summary: None,
                    started_at: None,
                    completed_at: None,
                    native_ref: None,
                },
            },
            &r,
        );
        let wire = serde_json::to_string(&sub[0].4).unwrap();
        assert!(!wire.contains("secret summary") && wire.contains("sub-1"));

        let created = map_event(
            &Event::SessionCreated {
                session: "s".into(),
                cwd: "/secret/cwd".into(),
                project_path: Some("/secret/cwd".into()),
                worktree_path: None,
                worktree_baseline: None,
                request_id: None,
            },
            &r,
        );
        assert_eq!(created[0].0, EventKind::SessionCreated);
        assert!(!serde_json::to_string(&created[0].4).unwrap().contains("/secret/cwd"));
    }

    #[test]
    fn push_resolved_rejects_wrong_kind() {
        let mut ring = EventRing::new(4);
        assert_eq!(
            ring.push_resolved(EventKind::TurnStarted, None, None, None, json!({})),
            Err(EventsError::InvalidResolvedKind)
        );
        assert!(ring
            .push_resolved(EventKind::ApprovalResolved, Some("s".into()), None, None, json!({"request_id":"r1"}))
            .is_ok());
    }

    #[test]
    fn hub_ingest_maps_and_pushes() {
        let hub = EventHub::new(EventRing::new(8));
        let out = hub.ingest_with(
            &Event::TurnStarted {
                session: "s1".into(),
                request_id: Some("req".into()),
                transcript_seq: None,
            },
            &resolver("/proj"),
            &|_| Some("turn-1".into()),
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, EventKind::TurnStarted);
        assert_eq!(out[0].turn_id.as_deref(), Some("turn-1"));
    }

    #[test]
    fn sender_without_tap_forwards_and_with_tap_observes_once() {
        struct Count(Mutex<usize>);
        impl EventObserver for Count {
            fn observe(&self, _: &Event) {
                *self.0.lock().unwrap() += 1;
            }
        }
        let (tx, mut rx) = mpsc::unbounded_channel();
        let plain: EventSender = tx.clone().into();
        plain.send(Event::SessionTitleChanged { session: "s".into(), title: "t".into() }).unwrap();
        let tap = Arc::new(Count(Mutex::new(0)));
        let tapped = EventSender::tapped(tx, tap.clone());
        let clone = tapped.clone();
        tapped.send(Event::SessionTitleChanged { session: "s".into(), title: "t".into() }).unwrap();
        clone.send(Event::SessionTitleChanged { session: "s".into(), title: "t".into() }).unwrap();
        assert_eq!(*tap.0.lock().unwrap(), 2);
        assert!((0..3).all(|_| rx.try_recv().is_ok()));
    }

    #[test]
    fn async_wait_wakes_on_push_and_times_out_structured() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        rt.block_on(async {
            let hub = EventHub::new(EventRing::new(8));
            let waiter = {
                let hub = hub.clone();
                tokio::spawn(async move {
                    hub.wait_for_async(request(None, 10), Duration::from_secs(2)).await
                })
            };
            tokio::time::sleep(Duration::from_millis(20)).await;
            hub.ingest(
                &Event::TurnStarted { session: "s".into(), request_id: None, transcript_seq: None },
                &resolver("/p"),
            );
            assert_eq!(waiter.await.unwrap().unwrap().events.len(), 1);
            let quiet = hub
                .wait_for_async(request(None, 10), Duration::from_millis(30))
                .await
                .unwrap();
            assert!(quiet.timed_out && quiet.events.is_empty());
        });
    }

    #[test]
    fn sync_wait_returns_structured_timeout_and_event() {
        let hub = EventHub::new(EventRing::new(8));
        let started = Instant::now();
        let timed_out = hub.wait_for(request(None, 1), Duration::from_millis(40)).unwrap();
        assert!(timed_out.timed_out && started.elapsed() < Duration::from_secs(2));
        assert_eq!(timed_out.next_cursor, hub.head_cursor());
        std::thread::scope(|scope| {
            let waiting = hub.clone();
            let waiter = scope.spawn(move || waiting.wait_for(request(None, 5), Duration::from_secs(5)));
            std::thread::sleep(Duration::from_millis(30));
            hub.ring().lock().unwrap().push(EventKind::TurnStarted, None, None, None, json!({}));
            let result = waiter.join().unwrap().unwrap();
            assert_eq!(result.events.len(), 1);
            assert!(!result.timed_out);
        });
    }
}

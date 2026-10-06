//! MCP-level event methods and the wire shapes of the event streams.
//!
//! Protocol labels (state of each at the time of writing, 2026-10-06):
//! - `subscriptions/listen` (MCP **2026-07-28**, the current official revision): a POST request
//!   that opens an SSE stream; replaces `resources/subscribe` and the GET endpoint.
//!   <https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/subscriptions>
//! - Streamable HTTP `GET` SSE + `resources/subscribe` (MCP **2025-06-18**): kept as an older
//!   compatibility surface.
//! - MCP Events (`events/list`, `events/poll`, `events/stream`): **draft / experimental**
//!   working-group proposal, not part of any MCP specification revision. Poll and push only;
//!   webhooks are not implemented.
//!   <https://github.com/modelcontextprotocol/experimental-ext-triggers-events>
//!
//! This module holds the pure protocol pieces (validation, notification builders, the
//! non-streaming methods). The long-lived SSE loops live in the server crate.

use crate::external_mcp::audit::{build_audit_record, reason_code};
use crate::external_mcp::catalog::ToolClass;
use crate::external_mcp::clients::{ExternalScope, ResolvedClient};
use crate::external_mcp::ctx::{ToolError, ToolErrorKind};
use crate::external_mcp::events::{
    EventEnvelope, EventKind, PollRequest, PollResult, ProjectFilter, EVENTS_RESOURCE_URI,
    MAX_POLL_LIMIT,
};
use crate::external_mcp::ops_events::poll_json_bounded;
use crate::external_mcp::subscriptions::{parse_listen_filter, ListenFilter, SubscriptionError};
use crate::Engine;
use serde_json::{json, Map, Value};
use std::time::Instant;

pub const PROTOCOL_STREAMABLE_HTTP: &str = "2025-06-18";
pub const PROTOCOL_SUBSCRIPTIONS: &str = "2026-07-28";
pub const MCP_EVENTS_DRAFT_SOURCE: &str =
    "https://github.com/modelcontextprotocol/experimental-ext-triggers-events/blob/main/docs/design-sketch-proposal.md";
pub const MCP_EVENTS_DRAFT_DATE: &str = "2026-02-19";
pub const MCP_EVENTS_EXTENSION: &str = "io.modelcontextprotocol/events";
pub const META_SUBSCRIPTION_ID: &str = "io.modelcontextprotocol/subscriptionId";
pub const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
/// Draft event names are the envelope type behind this prefix (`codetwo.turn.completed`).
pub const DRAFT_NAME_PREFIX: &str = "codetwo.";
/// Poll hint of the draft `events/poll` result.
pub const DRAFT_NEXT_POLL_MS: u64 = 1000;

/// Server capability fragments merged into `initialize` / `codetwo_capabilities`.
pub fn capabilities() -> Value {
    capabilities_with(true)
}

/// `post_streaming` states whether the HTTP layer routes `subscriptions/listen` and
/// `events/stream` POSTs to the SSE stream functions; without it only poll is real and the
/// push/listen claims are withheld.
pub fn capabilities_with(post_streaming: bool) -> Value {
    json!({
        "resources": { "subscribe": true, "listChanged": false },
        "codetwo/eventSupport": {
            "envelope": {
                "version": crate::external_mcp::events::ENVELOPE_VERSION,
                "kinds": EventKind::ALL.iter().map(|kind| kind.as_str()).collect::<Vec<_>>(),
                "cursor": "<epoch>:<seq>; null/absent means head (no replay)",
            },
            "tools": ["codetwo_events_poll", "codetwo_events_wait", "codetwo_turn_wait"],
            "streamableHttp": {
                "spec": PROTOCOL_STREAMABLE_HTTP,
                "status": "older-compatibility",
                "getSse": true,
                "lastEventId": true,
                "resourceSubscribe": true,
            },
            "subscriptionsListen": {
                "spec": PROTOCOL_SUBSCRIPTIONS,
                "status": "official-current-partial",
                "available": post_streaming,
                "transport": "POST + text/event-stream",
                "honored": ["resourceSubscriptions:codetwo://events"],
                "notHonored": ["toolsListChanged", "promptsListChanged", "resourcesListChanged"],
            },
            "mcpEventsDraft": {
                "status": "draft-experimental",
                "official": false,
                "source": MCP_EVENTS_DRAFT_SOURCE,
                "date": MCP_EVENTS_DRAFT_DATE,
                "extension": MCP_EVENTS_EXTENSION,
                "poll": true,
                "push": post_streaming,
                "webhook": false,
            },
        }
    })
}

fn is_event_method(method: &str) -> bool {
    matches!(
        method,
        "resources/list"
            | "resources/read"
            | "resources/subscribe"
            | "resources/unsubscribe"
            | "subscriptions/listen"
            | "events/list"
            | "events/poll"
            | "events/stream"
            | "events/subscribe"
            | "events/unsubscribe"
    )
}

/// Independent gate of every non-tool event entry point (JSON methods, GET stream, POST
/// streams): `read` scope, per-client rate limit, audit record. Generic transport/credential
/// checks stay with the caller; this does not rely on them.
pub fn authorize_event_method(
    engine: &Engine,
    client: &ResolvedClient,
    method: &str,
    params: Option<&Value>,
) -> Result<(), ToolError> {
    let started = Instant::now();
    let state = engine.external_mcp_state();
    let record = |allowed: bool, reason: &str| {
        let record = build_audit_record(
            &client.record.id,
            method,
            ToolClass::R,
            allowed,
            reason,
            None,
            None,
            params,
            started.elapsed().as_millis() as u64,
        );
        // Read class: a failing audit sink does not block (same rule as read tools).
        let _ = state.audit().record(&record);
    };
    if !client.record.scopes.contains(&ExternalScope::Read) {
        record(false, reason_code::SCOPE_DENIED);
        return Err(ToolError::denied("scope not granted"));
    }
    if !state.limiter().try_consume(&client.record.id, false) {
        record(false, reason_code::RATE_LIMITED);
        return Err(ToolError::new(
            ToolErrorKind::RateLimited,
            "client rate limit exceeded",
        ));
    }
    record(true, reason_code::ALLOWED);
    Ok(())
}

/// `None` => not an event method (dispatcher answers method not found).
pub fn handle_method(
    engine: &Engine,
    client: &ResolvedClient,
    method: &str,
    params: &Value,
) -> Option<Result<Value, ToolError>> {
    if !is_event_method(method) {
        return None;
    }
    if let Err(error) = authorize_event_method(engine, client, method, Some(params)) {
        return Some(Err(error));
    }
    Some(match method {
        "resources/list" => Ok(resources_list()),
        "resources/read" => resources_read(engine, client, params),
        "resources/subscribe" => resources_subscribe(engine, client, params),
        "resources/unsubscribe" => resources_unsubscribe(engine, client, params),
        "events/list" => Ok(events_list()),
        "events/poll" => events_poll_draft(engine, client, params),
        "subscriptions/listen" | "events/stream" => Err(ToolError::invalid(format!(
            "{method} opens a long-lived SSE response: POST it with Accept: text/event-stream"
        ))),
        _ => Err(ToolError::invalid(
            "webhook delivery is not implemented on this server",
        )),
    })
}

fn resources_list() -> Value {
    json!({
        "resources": [{
            "uri": EVENTS_RESOURCE_URI,
            "name": "CodeTwo event ring",
            "description": "Bounded v1 envelopes; read with ?since=<cursor>&limit=. No since = head, no replay.",
            "mimeType": "application/json"
        }]
    })
}

fn events_resource_target(params: &Value) -> Result<(&str, Option<&str>), ToolError> {
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("params.uri required"))?;
    let (base, query) = match uri.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (uri, None),
    };
    if base != EVENTS_RESOURCE_URI {
        return Err(ToolError::invalid("unknown resource uri"));
    }
    Ok((base, query))
}

fn query_param<'a>(query: Option<&'a str>, key: &str) -> Option<&'a str> {
    query?
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find_map(|(k, v)| (k == key).then_some(v))
}

fn resources_read(
    engine: &Engine,
    client: &ResolvedClient,
    params: &Value,
) -> Result<Value, ToolError> {
    let (_, query) = events_resource_target(params)?;
    let since = params
        .get("since")
        .and_then(Value::as_str)
        .or_else(|| query_param(query, "since"))
        .map(str::to_string);
    let limit = params
        .get("limit")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .or_else(|| query_param(query, "limit").and_then(|s| s.parse().ok()))
        .unwrap_or(50);
    let result = engine
        .external_mcp_state()
        .hub()
        .poll(PollRequest {
            cursor: since,
            kinds: None,
            session_id: None,
            project_filter: ProjectFilter::from_client(client),
            limit,
        })
        .map_err(|e| ToolError::invalid(e.to_string()))?;
    Ok(json!({
        "contents": [{
            "uri": EVENTS_RESOURCE_URI,
            "mimeType": "application/json",
            "text": poll_json_bounded(result).to_string()
        }]
    }))
}

fn resources_subscribe(
    engine: &Engine,
    client: &ResolvedClient,
    params: &Value,
) -> Result<Value, ToolError> {
    let (uri, _) = events_resource_target(params)?;
    engine
        .external_mcp_state()
        .subscriptions()
        .subscribe_resource(&client.record.id, uri)
        .map_err(|SubscriptionError::UnknownResource| ToolError::invalid("unknown resource"))?;
    Ok(json!({}))
}

fn resources_unsubscribe(
    engine: &Engine,
    client: &ResolvedClient,
    params: &Value,
) -> Result<Value, ToolError> {
    let (uri, _) = events_resource_target(params)?;
    engine
        .external_mcp_state()
        .subscriptions()
        .unsubscribe_resource(&client.record.id, uri);
    Ok(json!({}))
}

// ---- 2026-07-28 subscriptions/listen -------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ListenRequest {
    /// The JSON-RPC id of the `subscriptions/listen` request, echoed in every message.
    pub subscription_id: Value,
    pub filter: ListenFilter,
}

fn valid_request_id(id: Option<&Value>) -> Result<Value, ToolError> {
    match id {
        Some(id) if id.is_string() || id.is_number() => Ok(id.clone()),
        _ => Err(ToolError::invalid("a JSON-RPC request id is required")),
    }
}

pub fn parse_listen_request(id: Option<&Value>, params: &Value) -> Result<ListenRequest, ToolError> {
    let subscription_id = valid_request_id(id)?;
    if let Some(version) = params
        .get("_meta")
        .and_then(|meta| meta.get(META_PROTOCOL_VERSION))
    {
        if version.as_str() != Some(PROTOCOL_SUBSCRIPTIONS) {
            return Err(ToolError::invalid(format!(
                "unsupported protocol version; subscriptions/listen needs {PROTOCOL_SUBSCRIPTIONS}"
            )));
        }
    }
    Ok(ListenRequest {
        subscription_id,
        filter: parse_listen_filter(params),
    })
}

fn subscription_meta(subscription_id: &Value) -> Value {
    json!({ META_SUBSCRIPTION_ID: subscription_id })
}

/// First message of a listen stream: id equals the request id; `notifications` is the honored
/// subset only; the protocol version is stated in `_meta`.
pub fn listen_acknowledged(subscription_id: &Value, filter: &ListenFilter) -> Value {
    let mut notifications = Map::new();
    if !filter.resource_uris.is_empty() {
        notifications.insert(
            "resourceSubscriptions".into(),
            json!(filter.resource_uris.iter().collect::<Vec<_>>()),
        );
    }
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/subscriptions/acknowledged",
        "params": {
            "_meta": {
                META_SUBSCRIPTION_ID: subscription_id,
                META_PROTOCOL_VERSION: PROTOCOL_SUBSCRIPTIONS,
            },
            "notifications": notifications,
        }
    })
}

/// `notifications/resources/updated` for the event ring. `subscription_id` is set on listen
/// streams (2026-07-28) and absent on the legacy GET stream (2025-06-18).
pub fn resource_updated_notification(
    subscription_id: Option<&Value>,
    latest_cursor: &str,
    reset: bool,
) -> Value {
    let mut meta = Map::new();
    if let Some(id) = subscription_id {
        meta.insert(META_SUBSCRIPTION_ID.into(), id.clone());
    }
    meta.insert("codetwo/cursor".into(), json!(latest_cursor));
    if reset {
        meta.insert("codetwo/reset".into(), json!(true));
    }
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/resources/updated",
        "params": { "uri": EVENTS_RESOURCE_URI, "_meta": meta }
    })
}

pub fn json_rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Graceful end of a listen stream (the response to the original request).
pub fn listen_complete_response(subscription_id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": subscription_id,
        "result": { "resultType": "complete", "_meta": subscription_meta(subscription_id) }
    })
}

// ---- MCP Events draft ----------------------------------------------------------------------

pub fn draft_event_name(kind: EventKind) -> String {
    format!("{DRAFT_NAME_PREFIX}{}", kind.as_str())
}

pub fn draft_name_to_kind(name: &str) -> Option<EventKind> {
    name.strip_prefix(DRAFT_NAME_PREFIX).and_then(EventKind::parse)
}

fn events_list() -> Value {
    let events: Vec<Value> = EventKind::ALL
        .iter()
        .map(|kind| {
            json!({
                "name": draft_event_name(*kind),
                "description": format!("CodeTwo `{}` event", kind.as_str()),
                "delivery": ["poll", "push"],
                "inputSchema": {
                    "type": "object",
                    "properties": { "session_id": { "type": "string" } },
                    "additionalProperties": false
                },
                "payloadSchema": {
                    "type": "object",
                    "description": "v1 envelope fields (ids and enums only): v, session_id, project_id, turn_id, payload"
                }
            })
        })
        .collect();
    json!({ "events": events, "nextCursor": null })
}

/// `EventOccurrence` of the draft. `cursor` is only part of push deliveries; poll carries it at
/// the response level.
pub fn envelope_to_occurrence(env: &EventEnvelope, include_cursor: bool) -> Value {
    let mut occurrence = json!({
        "eventId": env.cursor,
        "name": draft_event_name(env.kind),
        "timestamp": env.ts,
        "data": {
            "v": env.v,
            "session_id": env.session_id,
            "project_id": env.project_id,
            "turn_id": env.turn_id,
            "payload": env.data,
        },
    });
    if include_cursor {
        occurrence["cursor"] = json!(env.cursor);
    }
    occurrence
}

pub struct EventsStreamRequest {
    pub subscription_id: Value,
    pub kind: EventKind,
    pub session_id: Option<String>,
    /// `None` = start from now.
    pub cursor: Option<String>,
}

struct DraftSubscription {
    kind: EventKind,
    session_id: Option<String>,
    cursor: Option<String>,
}

fn parse_draft_subscription(params: &Value) -> Result<DraftSubscription, ToolError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("name required"))?;
    let kind = draft_name_to_kind(name)
        .ok_or_else(|| ToolError::not_found(format!("unknown event name: {name}")))?;
    let session_id = match params.get("arguments") {
        None | Some(Value::Null) => None,
        Some(Value::Object(arguments)) => {
            if arguments.keys().any(|key| key != "session_id") {
                return Err(ToolError::invalid("arguments only accept session_id"));
            }
            arguments
                .get("session_id")
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_string)
                        .ok_or_else(|| ToolError::invalid("arguments.session_id must be a string"))
                })
                .transpose()?
        }
        Some(_) => return Err(ToolError::invalid("arguments must be an object")),
    };
    let cursor = match params.get("cursor") {
        None | Some(Value::Null) => None,
        Some(Value::String(cursor)) => Some(cursor.clone()),
        Some(_) => return Err(ToolError::invalid("cursor must be a string or null")),
    };
    Ok(DraftSubscription { kind, session_id, cursor })
}

pub fn parse_events_stream_request(
    id: Option<&Value>,
    params: &Value,
) -> Result<EventsStreamRequest, ToolError> {
    let subscription_id = valid_request_id(id)?;
    let draft = parse_draft_subscription(params)?;
    Ok(EventsStreamRequest {
        subscription_id,
        kind: draft.kind,
        session_id: draft.session_id,
        cursor: draft.cursor,
    })
}

fn events_poll_draft(
    engine: &Engine,
    client: &ResolvedClient,
    params: &Value,
) -> Result<Value, ToolError> {
    let draft = parse_draft_subscription(params)?;
    let max_events = params
        .get("maxEvents")
        .and_then(Value::as_u64)
        .unwrap_or(50)
        .clamp(1, MAX_POLL_LIMIT as u64) as usize;
    let result = engine
        .external_mcp_state()
        .hub()
        .poll(PollRequest {
            cursor: draft.cursor,
            kinds: Some(vec![draft.kind]),
            session_id: draft.session_id,
            project_filter: ProjectFilter::from_client(client),
            limit: max_events,
        })
        .map_err(|e| ToolError::invalid(e.to_string()))?;
    Ok(draft_poll_response(result))
}

/// Draft poll result: a gap is `truncated: true` with a fresh cursor, never an error; hitting
/// the response budget shrinks the batch and reports `hasMore` with the last kept cursor.
fn draft_poll_response(result: PollResult) -> Value {
    let gap = result.reset;
    let (bounded, _) = crate::external_mcp::ops_events::shrink_poll_result(result);
    json!({
        "events": bounded.events.iter().map(|env| envelope_to_occurrence(env, false)).collect::<Vec<_>>(),
        "cursor": bounded.next_cursor,
        "truncated": gap,
        "hasMore": bounded.has_more,
        "nextPollMs": DRAFT_NEXT_POLL_MS,
    })
}

pub fn events_active_notification(subscription_id: &Value, cursor: &str, truncated: bool) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/events/active",
        "params": { "cursor": cursor, "truncated": truncated, "_meta": subscription_meta(subscription_id) }
    })
}

/// The notification params are the `EventOccurrence` itself (plus `_meta`), not `{event: ..}`.
pub fn events_event_notification(subscription_id: &Value, env: &EventEnvelope) -> Value {
    let mut params = envelope_to_occurrence(env, true);
    params["_meta"] = subscription_meta(subscription_id);
    json!({ "jsonrpc": "2.0", "method": "notifications/events/event", "params": params })
}

pub fn events_heartbeat_notification(subscription_id: &Value, cursor: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/events/heartbeat",
        "params": { "cursor": cursor, "_meta": subscription_meta(subscription_id) }
    })
}

pub fn events_terminated_notification(subscription_id: &Value, code: i64, message: &str, reason: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/events/terminated",
        "params": {
            "error": { "code": code, "message": message, "data": { "reason": reason } },
            "_meta": subscription_meta(subscription_id)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_mcp::clients::{ExternalClientRecord, ProjectScope};
    use chrono::Utc;

    fn client(scopes: Vec<ExternalScope>) -> ResolvedClient {
        ResolvedClient {
            record: ExternalClientRecord {
                id: "c".into(),
                name: "t".into(),
                scopes,
                projects: ProjectScope::All,
                created_at: Utc::now(),
                expires_at: Some(Utc::now() + chrono::Duration::days(1)),
                last_used_at: None,
                revoked_at: None,
            },
        }
    }

    fn engine() -> (Engine, tempfile::TempDir) {
        let dir = tempfile::TempDir::new().unwrap();
        let (engine, _) = Engine::new(vec![], Default::default());
        engine.external_mcp_state().set_enabled(true);
        engine.external_mcp_state().configure_data_dir(dir.path()).unwrap();
        (engine, dir)
    }

    #[test]
    fn capabilities_label_versions_and_withhold_unavailable_push() {
        let caps = capabilities();
        let support = &caps["codetwo/eventSupport"];
        assert_eq!(support["streamableHttp"]["spec"], PROTOCOL_STREAMABLE_HTTP);
        assert_eq!(support["subscriptionsListen"]["spec"], PROTOCOL_SUBSCRIPTIONS);
        assert_eq!(support["mcpEventsDraft"]["status"], "draft-experimental");
        assert_eq!(support["mcpEventsDraft"]["official"], false);
        assert_eq!(support["mcpEventsDraft"]["webhook"], false);
        assert_eq!(support["mcpEventsDraft"]["push"], true);
        assert!(caps.get("events").is_none(), "no unofficial top-level capability");
        let kinds = support["envelope"]["kinds"].as_array().unwrap();
        assert!(!kinds.contains(&json!("queue.drained")));
        let without = capabilities_with(false);
        assert_eq!(without["codetwo/eventSupport"]["mcpEventsDraft"]["push"], false);
        assert_eq!(without["codetwo/eventSupport"]["subscriptionsListen"]["available"], false);
    }

    #[test]
    fn listen_request_binds_subscription_id_to_request_id() {
        let params = json!({
            "_meta": { META_PROTOCOL_VERSION: PROTOCOL_SUBSCRIPTIONS },
            "notifications": { "resourceSubscriptions": [EVENTS_RESOURCE_URI], "toolsListChanged": true }
        });
        let request = parse_listen_request(Some(&json!(7)), &params).unwrap();
        assert_eq!(request.subscription_id, json!(7));
        let ack = listen_acknowledged(&request.subscription_id, &request.filter);
        assert_eq!(ack["method"], "notifications/subscriptions/acknowledged");
        assert_eq!(ack["params"]["_meta"][META_SUBSCRIPTION_ID], 7);
        assert_eq!(ack["params"]["_meta"][META_PROTOCOL_VERSION], PROTOCOL_SUBSCRIPTIONS);
        assert_eq!(ack["params"]["notifications"]["resourceSubscriptions"][0], EVENTS_RESOURCE_URI);
        assert!(ack["params"]["notifications"].get("toolsListChanged").is_none());
        assert!(parse_listen_request(None, &params).is_err(), "id is required");
        assert!(parse_listen_request(Some(&json!(null)), &params).is_err());
        let old = json!({ "_meta": { META_PROTOCOL_VERSION: "2025-06-18" } });
        assert!(parse_listen_request(Some(&json!("a")), &old).is_err());
        assert_eq!(
            parse_listen_request(Some(&json!("a")), &json!({})).unwrap().subscription_id,
            json!("a")
        );
    }

    #[test]
    fn draft_notifications_use_direct_occurrence_params() {
        let env = EventEnvelope {
            v: 1,
            cursor: "5:3".into(),
            ts: "2026-01-01T00:00:00Z".into(),
            kind: EventKind::TurnCompleted,
            session_id: Some("s".into()),
            project_id: None,
            turn_id: Some("t".into()),
            data: json!({"stop_reason": "EndTurn"}),
        };
        let note = events_event_notification(&json!(1), &env);
        assert_eq!(note["method"], "notifications/events/event");
        assert!(note["params"].get("event").is_none());
        assert_eq!(note["params"]["eventId"], "5:3");
        assert_eq!(note["params"]["name"], "codetwo.turn.completed");
        assert_eq!(note["params"]["cursor"], "5:3");
        assert_eq!(note["params"]["_meta"][META_SUBSCRIPTION_ID], 1);
        assert!(envelope_to_occurrence(&env, false).get("cursor").is_none());
        let active = events_active_notification(&json!(1), "5:3", true);
        assert_eq!(active["params"]["truncated"], true);
        let beat = events_heartbeat_notification(&json!("x"), "5:3");
        assert_eq!(beat["params"]["cursor"], "5:3");
        assert_eq!(beat["params"]["_meta"][META_SUBSCRIPTION_ID], "x");
    }

    #[test]
    fn draft_request_validation() {
        let ok = parse_events_stream_request(
            Some(&json!(2)),
            &json!({"name": "codetwo.turn.failed", "arguments": {"session_id": "s"}, "cursor": null}),
        )
        .unwrap();
        assert_eq!((ok.kind, ok.session_id.as_deref(), ok.cursor), (EventKind::TurnFailed, Some("s"), None));
        for bad in [
            json!({"name": "codetwo.all"}),
            json!({"name": "codetwo.queue.drained"}),
            json!({"name": "turn.failed"}),
            json!({"name": "codetwo.turn.failed", "arguments": {"x": 1}}),
            json!({"name": "codetwo.turn.failed", "cursor": 5}),
        ] {
            assert!(parse_events_stream_request(Some(&json!(2)), &bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn event_methods_enforce_read_scope_rate_limit_and_audit() {
        let (engine, _dir) = engine();
        let denied = client(vec![ExternalScope::Operate]);
        let error = handle_method(&engine, &denied, "resources/list", &json!({}))
            .unwrap()
            .unwrap_err();
        assert_eq!(error.kind, ToolErrorKind::Denied);
        let reader = client(vec![ExternalScope::Read]);
        assert!(handle_method(&engine, &reader, "resources/list", &json!({})).unwrap().is_ok());
        assert!(handle_method(&engine, &reader, "tools/list", &json!({})).is_none());
        let mut limited = false;
        for _ in 0..500 {
            if let Err(error) = handle_method(&engine, &reader, "events/list", &json!({})).unwrap() {
                assert_eq!(error.kind, ToolErrorKind::RateLimited);
                limited = true;
                break;
            }
        }
        assert!(limited, "event methods share the per-client limiter");
    }

    #[test]
    fn streaming_methods_and_webhooks_are_refused_on_the_json_path() {
        let (engine, _dir) = engine();
        let reader = client(vec![ExternalScope::Read]);
        for method in ["subscriptions/listen", "events/stream", "events/subscribe", "events/unsubscribe"] {
            assert!(handle_method(&engine, &reader, method, &json!({})).unwrap().is_err(), "{method}");
        }
    }
}

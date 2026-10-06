//! External MCP push streams over SSE.
//!
//! Three entry points share one stream loop ([`run_stream`]):
//!
//! - `GET /external-mcp` — MCP **2025-06-18** Streamable HTTP SSE (older compatibility):
//!   `resources/subscribe` notifications, `Last-Event-ID` resume.
//! - `POST subscriptions/listen` — MCP **2026-07-28** (current official): the response is an SSE
//!   stream whose first message is `notifications/subscriptions/acknowledged`.
//! - `POST events/stream` — MCP Events **draft** (experimental, not an official revision):
//!   `notifications/events/{active,event,heartbeat,terminated}`.
//!
//! Every stream is bounded and leak-free: the slot is held by a [`StreamGuard`] owned by the
//! stream task, the task selects on `tx.closed()` (client disconnect), a failed or stalled send
//! ends it, and the credential (token, read scope, project scope) is revalidated before every
//! batch poll, with idle checks scheduled every [`StreamOptions::revalidate`]. A batch already
//! authorized before revocation may finish delivery; saturated filesystem work can delay checks.
//!
//! Transport guards for GET mirror the POST route (loopback peer/Host/Origin, Bearer, surface
//! enabled). [`post_events_stream`] expects the caller to have run the generic POST gate.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use codetwo_core::external_mcp::clients::{
    looks_like_external_token, ExternalScope, ResolveError, ResolvedClient,
};
use codetwo_core::external_mcp::ctx::{ToolError, ToolErrorKind};
use codetwo_core::external_mcp::events::{
    EventKind, PollRequest, ProjectFilter, EVENTS_RESOURCE_URI,
};
use codetwo_core::external_mcp::gate::tool_error_to_rpc_code;
use codetwo_core::external_mcp::mcp_events::{
    authorize_event_method, events_active_notification, events_event_notification,
    events_heartbeat_notification, events_terminated_notification, json_rpc_error,
    listen_acknowledged, parse_events_stream_request, parse_listen_request,
    resource_updated_notification, EventsStreamRequest, ListenRequest,
};
use codetwo_core::external_mcp::state::host_allowed;
use codetwo_core::external_mcp::subscriptions::{StreamGuard, StreamKind, StreamLimitError};
use codetwo_core::Engine;
use futures_util::Stream;
use serde_json::{json, Value};
use tokio::sync::mpsc;

const METHOD_LISTEN: &str = "subscriptions/listen";
const METHOD_EVENTS_STREAM: &str = "events/stream";
const METHOD_GET: &str = "GET /external-mcp";
const BATCH: usize = 50;

pub fn is_loopback_host(value: &str) -> bool {
    let host = value.split(':').next().unwrap_or(value).trim();
    host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "::1"
        || host == "[::1]"
}

/// Timing of one stream. Production uses [`Default`]; tests shorten it.
#[derive(Debug, Clone, Copy)]
pub struct StreamOptions {
    /// Longest silence on an `events/stream` before a `notifications/events/heartbeat` data
    /// frame (the draft requires at least one every 30 s).
    pub heartbeat: Duration,
    /// Longest gap between credential revalidations, and the SSE comment keep-alive of the
    /// GET/listen streams.
    pub revalidate: Duration,
    /// A send that cannot complete within this long (client not reading) ends the stream.
    pub send_timeout: Duration,
}

impl Default for StreamOptions {
    fn default() -> Self {
        Self {
            heartbeat: Duration::from_secs(15),
            revalidate: Duration::from_secs(5),
            send_timeout: Duration::from_secs(10),
        }
    }
}

fn reject_transport(
    headers: &HeaderMap,
    peer: SocketAddr,
    _remote_allowed: bool,
) -> Option<Response> {
    if !peer.ip().is_loopback() {
        return Some((StatusCode::FORBIDDEN, "loopback only").into_response());
    }
    if let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) {
        if !host_allowed(host) {
            return Some((StatusCode::FORBIDDEN, "invalid Host").into_response());
        }
    }
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let allowed = origin
            .strip_prefix("http://")
            .or_else(|| origin.strip_prefix("https://"))
            .map(|rest| rest.split('/').next().unwrap_or(rest))
            .is_some_and(host_allowed);
        if !allowed {
            return Some((StatusCode::FORBIDDEN, "invalid Origin").into_response());
        }
    }
    None
}

fn parse_bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

fn accepts_event_stream(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|accept| accept.contains("text/event-stream"))
}

fn resolve_token_sync(engine: &Engine, token: &str) -> Result<ResolvedClient, Response> {
    if !looks_like_external_token(token) {
        return Err((StatusCode::UNAUTHORIZED, "invalid token").into_response());
    }
    match engine
        .external_mcp_state()
        .with_registry(|reg| reg.resolve(token))
    {
        Some(Ok(client)) => Ok(client),
        Some(Err(ResolveError::Revoked | ResolveError::Expired)) => {
            Err((StatusCode::UNAUTHORIZED, "credential revoked or expired").into_response())
        }
        Some(Err(_)) | None => Err((StatusCode::UNAUTHORIZED, "unauthorized").into_response()),
    }
}

// Registry reads and audit fsync must not block Tokio workers. The permit moves into the
// blocking closure so cancellation cannot release capacity while filesystem work continues.
static AUTH_IO: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(64)));

async fn resolve_token(engine: &Arc<Engine>, token: &str) -> Result<ResolvedClient, Response> {
    let permit = AUTH_IO.clone().try_acquire_owned().map_err(|_| {
        (
            StatusCode::TOO_MANY_REQUESTS,
            "authentication capacity reached",
        )
            .into_response()
    })?;
    let engine = engine.clone();
    let token = token.to_owned();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        resolve_token_sync(&engine, &token)
    })
    .await
    .unwrap_or_else(|_| Err(StatusCode::INTERNAL_SERVER_ERROR.into_response()))
}

async fn authorize_stream(
    engine: &Arc<Engine>,
    client: &ResolvedClient,
    method: &str,
    params: Option<&Value>,
) -> Result<(), ToolError> {
    let permit = AUTH_IO.clone().try_acquire_owned().map_err(|_| ToolError {
        kind: ToolErrorKind::RateLimited,
        message: "authorization capacity reached".into(),
    })?;
    let engine = engine.clone();
    let client = client.clone();
    let method = method.to_owned();
    let params = params.cloned();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        authorize_event_method(&engine, &client, &method, params.as_ref())
    })
    .await
    .unwrap_or_else(|_| Err(ToolError::internal("stream authorization failed")))
}

// ---- credential revalidation -------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Close {
    Revoked,
    ReadScopeLost,
    Disabled,
}

impl Close {
    fn reason(self) -> &'static str {
        match self {
            Self::Revoked => "credential revoked or expired",
            Self::ReadScopeLost => "read scope removed",
            Self::Disabled => "external MCP disabled",
        }
    }
}

/// Fresh view of the credential: token still valid, same client, still allowed to read. The
/// returned record carries the *current* project scope.
fn revalidate_sync(engine: &Engine, token: &str, client_id: &str) -> Result<ResolvedClient, Close> {
    let state = engine.external_mcp_state();
    if !state.is_enabled() {
        return Err(Close::Disabled);
    }
    let client = match state.with_registry(|reg| reg.resolve(token)) {
        Some(Ok(client)) if client.record.id == client_id => client,
        _ => return Err(Close::Revoked),
    };
    if !client.record.scopes.contains(&ExternalScope::Read) {
        return Err(Close::ReadScopeLost);
    }
    Ok(client)
}

// Existing streams use a separate pool: invalid-token traffic cannot evict a valid stream
// merely by saturating initial authentication. Wait for capacity without delivering unchecked data.
static REVALIDATE_IO: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(16)));

async fn revalidate(
    engine: &Arc<Engine>,
    token: &str,
    client_id: &str,
) -> Result<ResolvedClient, Close> {
    let permit = REVALIDATE_IO
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| Close::Disabled)?;
    let engine = engine.clone();
    let token = token.to_owned();
    let client_id = client_id.to_owned();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        revalidate_sync(&engine, &token, &client_id)
    })
    .await
    .unwrap_or(Err(Close::Disabled))
}

// ---- SSE plumbing ------------------------------------------------------------------------

type SseTx = mpsc::Sender<Result<Event, Infallible>>;

struct ReceiverStream(mpsc::Receiver<Result<Event, Infallible>>);

impl Stream for ReceiverStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.0).poll_recv(cx)
    }
}

fn data_event(value: &Value, id: Option<&str>) -> Event {
    let event = Event::default().data(value.to_string());
    match id {
        Some(id) => event.id(id.to_string()),
        None => event,
    }
}

/// `false` when the client is gone or not reading: the caller ends the stream.
async fn send(tx: &SseTx, event: Event, timeout: Duration) -> bool {
    matches!(
        tokio::time::timeout(timeout, tx.send(Ok(event))).await,
        Ok(Ok(()))
    )
}

async fn send_json(tx: &SseTx, value: &Value, id: Option<&str>, timeout: Duration) -> bool {
    send(tx, data_event(value, id), timeout).await
}

fn denied_code() -> i64 {
    tool_error_to_rpc_code(ToolErrorKind::Denied)
}

fn rpc_error_response(status: StatusCode, id: &Value, error: &ToolError) -> Response {
    (
        status,
        Json(json_rpc_error(
            id,
            tool_error_to_rpc_code(error.kind),
            &error.message,
        )),
    )
        .into_response()
}

// ---- the shared stream loop --------------------------------------------------------------

enum Mode {
    /// 2025-06-18 `GET`: resume point from `Last-Event-ID`.
    Get { last_event_id: Option<String> },
    /// 2026-07-28 `subscriptions/listen`.
    Listen(ListenRequest),
    /// MCP Events draft `events/stream`.
    Events(EventsStreamRequest),
}

struct StreamTask {
    engine: Arc<Engine>,
    token: String,
    client: ResolvedClient,
    guard: StreamGuard,
    mode: Mode,
    opts: StreamOptions,
    tx: SseTx,
}

async fn run_stream(task: StreamTask) {
    let StreamTask {
        engine,
        token,
        mut client,
        guard,
        mode,
        opts,
        tx,
    } = task;
    // Dropped when this task ends for any reason (disconnect, revocation, panic), freeing the slot.
    let _guard_slot = &guard;
    let client_id = client.record.id.clone();
    let hub = engine.external_mcp_state().hub().clone();
    let sub = hub.subscribe();

    let (kinds, session_filter): (Option<Vec<EventKind>>, Option<String>) = match &mode {
        Mode::Events(request) => (Some(vec![request.kind]), request.session_id.clone()),
        _ => (None, None),
    };
    let subscription_id: Option<Value> = match &mode {
        Mode::Get { .. } => None,
        Mode::Listen(request) => Some(request.subscription_id.clone()),
        Mode::Events(request) => Some(request.subscription_id.clone()),
    };

    let mut cursor = match &mode {
        Mode::Get { last_event_id } => last_event_id.clone().unwrap_or_else(|| hub.head_cursor()),
        Mode::Listen(_) => hub.head_cursor(),
        Mode::Events(request) => request.cursor.clone().unwrap_or_else(|| hub.head_cursor()),
    };

    if let Mode::Listen(request) = &mode {
        let ack = listen_acknowledged(&request.subscription_id, &request.filter);
        if !send_json(&tx, &ack, None, opts.send_timeout).await {
            return;
        }
    }

    let mut active_sent = false;
    let mut last_sent = Instant::now();
    loop {
        // Armed before the poll: a push between poll and park still wakes the select below.
        let notified = sub.notified();
        tokio::pin!(notified);

        let checked = tokio::select! {
            _ = tx.closed() => return,
            checked = revalidate(&engine, &token, &client_id) => checked,
        };
        match checked {
            Ok(fresh) => client = fresh,
            Err(close) => {
                send_close(
                    &tx,
                    &mode,
                    subscription_id.as_ref(),
                    close,
                    opts.send_timeout,
                )
                .await;
                return;
            }
        }

        let polled = hub.poll(PollRequest {
            cursor: Some(cursor.clone()),
            kinds: kinds.clone(),
            session_id: session_filter.clone(),
            project_filter: ProjectFilter::from_client(&client),
            limit: BATCH,
        });
        let result = match polled {
            Ok(result) => result,
            Err(_) => {
                // Only a client-supplied cursor can be malformed. Treat it like a gap.
                cursor = hub.head_cursor();
                if matches!(mode, Mode::Get { .. }) {
                    let frame = json!({ "codetwo/reset": true, "cursor": cursor });
                    let reset = Event::default()
                        .event("codetwo-reset")
                        .data(frame.to_string());
                    if !send(&tx, reset, opts.send_timeout).await {
                        return;
                    }
                    continue;
                }
                let error = ToolError::invalid("malformed cursor");
                send_error_end(
                    &tx,
                    &mode,
                    subscription_id.as_ref(),
                    &error,
                    opts.send_timeout,
                )
                .await;
                return;
            }
        };

        let delivered = match &mode {
            Mode::Events(_) => {
                if !active_sent || result.reset {
                    let active = events_active_notification(
                        subscription_id.as_ref().unwrap_or(&Value::Null),
                        // Initial acknowledgment must not advance past occurrences which have
                        // not yet been delivered. A gap poll carries no events and resets safely.
                        if result.reset {
                            &result.next_cursor
                        } else {
                            &cursor
                        },
                        result.reset,
                    );
                    if !send_json(&tx, &active, None, opts.send_timeout).await {
                        return;
                    }
                    active_sent = true;
                    last_sent = Instant::now();
                }
                let mut ok = true;
                for envelope in &result.events {
                    let note = events_event_notification(
                        subscription_id.as_ref().unwrap_or(&Value::Null),
                        envelope,
                    );
                    if !send_json(&tx, &note, Some(&envelope.cursor), opts.send_timeout).await {
                        ok = false;
                        break;
                    }
                    last_sent = Instant::now();
                }
                ok
            }
            Mode::Get { .. } | Mode::Listen(_) => {
                deliver_resource_updates(
                    &tx,
                    &mode,
                    &guard,
                    subscription_id.as_ref(),
                    &result,
                    opts.send_timeout,
                )
                .await
            }
        };
        if !delivered {
            return;
        }
        cursor = result.next_cursor.clone();

        if result.reset || result.has_more {
            continue;
        }
        if matches!(mode, Mode::Events(_)) && last_sent.elapsed() >= opts.heartbeat {
            let beat = events_heartbeat_notification(
                subscription_id.as_ref().unwrap_or(&Value::Null),
                &cursor,
            );
            if !send_json(&tx, &beat, None, opts.send_timeout).await {
                return;
            }
            last_sent = Instant::now();
        }

        let until_heartbeat = match mode {
            Mode::Events(_) => opts.heartbeat.saturating_sub(last_sent.elapsed()),
            _ => opts.revalidate,
        };
        let tick = opts
            .revalidate
            .min(until_heartbeat)
            .max(Duration::from_millis(5));
        tokio::select! {
            _ = tx.closed() => return,
            _ = &mut notified => {}
            _ = tokio::time::sleep(tick) => {}
        }
    }
}

/// GET / listen: one `notifications/resources/updated` per batch (the resource is the ring; the
/// client reads it for content). GET only notifies on the newest GET stream of a subscribed
/// client — a message is sent on one stream only.
async fn deliver_resource_updates(
    tx: &SseTx,
    mode: &Mode,
    guard: &StreamGuard,
    subscription_id: Option<&Value>,
    result: &codetwo_core::external_mcp::events::PollResult,
    timeout: Duration,
) -> bool {
    let wants_updates = match mode {
        Mode::Get { .. } => {
            guard.is_primary_get() && guard.resource_subscribed(EVENTS_RESOURCE_URI)
        }
        Mode::Listen(request) => request.filter.resource_uris.contains(EVENTS_RESOURCE_URI),
        Mode::Events(_) => false,
    };
    if result.reset {
        if matches!(mode, Mode::Get { .. }) {
            let frame = json!({
                "codetwo/reset": true,
                "cursor": result.next_cursor,
                "oldest_cursor": result.oldest_cursor,
            });
            let reset = Event::default()
                .event("codetwo-reset")
                .data(frame.to_string());
            if !send(tx, reset, timeout).await {
                return false;
            }
        }
        if wants_updates {
            let note = resource_updated_notification(subscription_id, &result.next_cursor, true);
            return send_json(tx, &note, Some(&result.next_cursor), timeout).await;
        }
        return true;
    }
    if wants_updates {
        if let Some(last) = result.events.last() {
            let note = resource_updated_notification(subscription_id, &last.cursor, false);
            return send_json(tx, &note, Some(&last.cursor), timeout).await;
        }
    }
    true
}

async fn send_close(
    tx: &SseTx,
    mode: &Mode,
    subscription_id: Option<&Value>,
    close: Close,
    timeout: Duration,
) {
    match (mode, subscription_id) {
        (Mode::Events(_), Some(id)) => {
            let note =
                events_terminated_notification(id, denied_code(), close.reason(), "forbidden");
            let _ = send_json(tx, &note, None, timeout).await;
        }
        (Mode::Listen(_), Some(id)) => {
            let note = json_rpc_error(id, denied_code(), close.reason());
            let _ = send_json(tx, &note, None, timeout).await;
        }
        _ => {}
    }
}

async fn send_error_end(
    tx: &SseTx,
    mode: &Mode,
    subscription_id: Option<&Value>,
    error: &ToolError,
    timeout: Duration,
) {
    let code = tool_error_to_rpc_code(error.kind);
    match (mode, subscription_id) {
        (Mode::Events(_), Some(id)) => {
            let note = events_terminated_notification(id, code, &error.message, "error");
            let _ = send_json(tx, &note, None, timeout).await;
        }
        (_, Some(id)) => {
            let _ = send_json(tx, &json_rpc_error(id, code, &error.message), None, timeout).await;
        }
        _ => {}
    }
}

fn spawn_stream(
    task_fields: impl FnOnce(SseTx) -> StreamTask,
    keep_alive: Option<Duration>,
) -> Response {
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(32);
    tokio::spawn(run_stream(task_fields(tx)));
    let sse = Sse::new(ReceiverStream(rx));
    match keep_alive {
        Some(interval) => sse
            .keep_alive(KeepAlive::new().interval(interval))
            .into_response(),
        None => sse.into_response(),
    }
}

// ---- GET (2025-06-18) --------------------------------------------------------------------

pub async fn get_external_mcp_stream(
    State(engine): State<Arc<Engine>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    get_stream_with(engine, peer, headers, StreamOptions::default()).await
}

pub async fn get_stream_with(
    engine: Arc<Engine>,
    peer: SocketAddr,
    headers: HeaderMap,
    opts: StreamOptions,
) -> Response {
    let remote_allowed = !codetwo_core::external_mcp::state::allowed_hosts_snapshot().is_empty();
    if let Some(response) = reject_transport(&headers, peer, remote_allowed) {
        return response;
    }
    if !engine.external_mcp_state().is_enabled() {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !accepts_event_stream(&headers) {
        return (
            StatusCode::NOT_ACCEPTABLE,
            "Accept: text/event-stream required",
        )
            .into_response();
    }
    let Some(token) = parse_bearer(&headers) else {
        return (StatusCode::UNAUTHORIZED, "bearer required").into_response();
    };
    let source_key = format!(
        "{:p}:{}",
        Arc::as_ptr(&engine),
        crate::remote::auth_limiter_source_key(&headers, peer)
    );
    let client = match resolve_token(&engine, &token).await {
        Ok(client) => {
            crate::external_mcp::clear_auth_failures(&source_key);
            client
        }
        Err(response) => {
            if response.status() == StatusCode::UNAUTHORIZED
                && crate::external_mcp::record_auth_failure(source_key)
            {
                return (StatusCode::TOO_MANY_REQUESTS, "too many auth failures").into_response();
            }
            return response;
        }
    };
    // Independent of the POST gate: read scope, per-client limiter and audit.
    if let Err(error) = authorize_stream(&engine, &client, METHOD_GET, None).await {
        let status = match error.kind {
            ToolErrorKind::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ToolErrorKind::Denied => StatusCode::FORBIDDEN,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        return (status, error.message).into_response();
    }
    let guard = match engine
        .external_mcp_state()
        .subscriptions()
        .open_stream(&client.record.id, StreamKind::Get)
    {
        Ok(guard) => guard,
        Err(StreamLimitError::TooManyStreams) => {
            return (StatusCode::TOO_MANY_REQUESTS, "stream limit reached").into_response();
        }
    };
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    spawn_stream(
        |tx| StreamTask {
            engine,
            token,
            client,
            guard,
            mode: Mode::Get { last_event_id },
            opts,
            tx,
        },
        Some(
            opts.revalidate
                .max(Duration::from_secs(1))
                .min(Duration::from_secs(15)),
        ),
    )
}

// ---- POST streams (2026-07-28 subscriptions/listen, Events draft events/stream) -----------

/// True for the two POST methods that answer with an SSE stream instead of a JSON body.
pub fn is_post_stream_request(payload: &Value) -> bool {
    matches!(
        payload.get("method").and_then(Value::as_str),
        Some(METHOD_LISTEN | METHOD_EVENTS_STREAM)
    )
}

/// Serves a `subscriptions/listen` or `events/stream` POST as an SSE response.
///
/// The caller has already run the generic POST gate (loopback/Host/Origin, Bearer, JSON parse)
/// and passes the raw bearer token. This function resolves the credential itself and enforces
/// read scope, the per-client limiter and audit on its own.
pub async fn post_events_stream(
    engine: Arc<Engine>,
    headers: &HeaderMap,
    bearer: String,
    payload: Value,
) -> Response {
    post_events_stream_with(engine, headers, bearer, payload, StreamOptions::default()).await
}

pub async fn post_events_stream_with(
    engine: Arc<Engine>,
    headers: &HeaderMap,
    bearer: String,
    payload: Value,
    opts: StreamOptions,
) -> Response {
    if !engine.external_mcp_state().is_enabled() {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !accepts_event_stream(headers) {
        return (
            StatusCode::NOT_ACCEPTABLE,
            "Accept: text/event-stream required",
        )
            .into_response();
    }
    let client = match resolve_token(&engine, &bearer).await {
        Ok(client) => client,
        Err(response) => return response,
    };
    let method = payload
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let id = payload.get("id").cloned();
    let params = payload.get("params").cloned().unwrap_or(Value::Null);
    let id_for_error = id.clone().unwrap_or(Value::Null);

    if let Err(error) = authorize_stream(&engine, &client, &method, Some(&params)).await {
        let status = if error.kind == ToolErrorKind::RateLimited {
            StatusCode::TOO_MANY_REQUESTS
        } else {
            StatusCode::OK
        };
        return rpc_error_response(status, &id_for_error, &error);
    }

    let mode = match method.as_str() {
        METHOD_LISTEN => match parse_listen_request(id.as_ref(), &params) {
            Ok(request) => Mode::Listen(request),
            Err(error) => return rpc_error_response(StatusCode::OK, &id_for_error, &error),
        },
        METHOD_EVENTS_STREAM => match parse_events_stream_request(id.as_ref(), &params) {
            Ok(request) => {
                if let Some(cursor) = &request.cursor {
                    let probe = engine.external_mcp_state().hub().poll(PollRequest {
                        cursor: Some(cursor.clone()),
                        kinds: None,
                        session_id: None,
                        project_filter: ProjectFilter::Allow(Vec::new()),
                        limit: 1,
                    });
                    if probe.is_err() {
                        let error = ToolError::invalid("malformed cursor");
                        return rpc_error_response(StatusCode::OK, &id_for_error, &error);
                    }
                }
                Mode::Events(request)
            }
            Err(error) => return rpc_error_response(StatusCode::OK, &id_for_error, &error),
        },
        _ => {
            let error = ToolError::new(ToolErrorKind::NotFound, "not a streaming method");
            return rpc_error_response(StatusCode::OK, &id_for_error, &error);
        }
    };

    let kind = match mode {
        Mode::Events(_) => StreamKind::Events,
        _ => StreamKind::Listen,
    };
    let guard = match engine
        .external_mcp_state()
        .subscriptions()
        .open_stream(&client.record.id, kind)
    {
        Ok(guard) => guard,
        Err(StreamLimitError::TooManyStreams) => {
            let error = ToolError::new(ToolErrorKind::RateLimited, "stream limit reached");
            return rpc_error_response(StatusCode::TOO_MANY_REQUESTS, &id_for_error, &error);
        }
    };
    // The draft's heartbeat is a data frame, so its stream carries no SSE comment keep-alive.
    let keep_alive = match mode {
        Mode::Events(_) => None,
        _ => Some(
            opts.revalidate
                .max(Duration::from_secs(1))
                .min(Duration::from_secs(15)),
        ),
    };
    spawn_stream(
        |tx| StreamTask {
            engine,
            token: bearer,
            client,
            guard,
            mode,
            opts,
            tx,
        },
        keep_alive,
    )
}

#[cfg(test)]
mod transport_tests {
    use super::*;

    #[tokio::test]
    async fn revalidation_capacity_waits_without_reporting_disabled() {
        let (engine, _rx) = Engine::new(Vec::new(), codetwo_core::skill::SkillLibrary::default());
        let engine = Arc::new(engine);
        engine.external_mcp_state().set_enabled(true);
        let dir = tempfile::tempdir().unwrap();
        engine
            .external_mcp_state()
            .configure_data_dir(dir.path())
            .unwrap();
        let (record, token) = engine
            .external_mcp_state()
            .with_registry(|registry| {
                registry.create(
                    "revalidation",
                    vec![ExternalScope::Read],
                    codetwo_core::external_mcp::clients::ProjectScope::All,
                    codetwo_core::external_mcp::clients::TtlChoice::Days(1),
                )
            })
            .unwrap()
            .unwrap();
        let held = REVALIDATE_IO.clone().acquire_many_owned(16).await.unwrap();
        assert!(tokio::time::timeout(
            Duration::from_millis(20),
            revalidate(&engine, &token, &record.id)
        )
        .await
        .is_err());
        drop(held);
        assert!(revalidate(&engine, &token, &record.id).await.is_ok());
        // A saturated initial-auth pool has no effect on already authenticated streams.
        let held = AUTH_IO.clone().acquire_many_owned(64).await.unwrap();
        assert!(revalidate(&engine, &token, &record.id).await.is_ok());
        drop(held);
    }

    #[test]
    fn tunnel_configuration_never_allows_a_non_loopback_peer() {
        let peer = "192.0.2.1:44000".parse().unwrap();
        let response = reject_transport(&HeaderMap::new(), peer, true).unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}

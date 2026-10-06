//! External MCP SSE streams: GET (2025-06-18), POST `subscriptions/listen` (2026-07-28) and POST
//! `events/stream` (MCP Events draft), including disconnect, revocation and project-scope
//! behaviour. POST streams are served by a minimal router that calls the reusable stream function
//! exactly like the real POST handler does.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use codetwo_core::external_mcp::clients::{ExternalScope, ProjectScope, TtlChoice};
use codetwo_core::external_mcp::events::{EventKind, EVENTS_RESOURCE_URI};
use codetwo_core::external_mcp::subscriptions::MAX_STREAMS_PER_CLIENT;
use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
use codetwo_core::skill::SkillLibrary;
use codetwo_core::Engine;
use codetwo_server::external_mcp_stream::{
    is_post_stream_request, post_events_stream_with, StreamOptions,
};
use codetwo_server::{bind_and_serve, fanout, AuthState};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const FAST: StreamOptions = StreamOptions {
    heartbeat: Duration::from_millis(200),
    revalidate: Duration::from_millis(100),
    send_timeout: Duration::from_secs(2),
};

struct Fixture {
    engine: Arc<Engine>,
    _dir: TempDir,
    token: String,
    client_id: String,
}

fn fixture_with(scopes: Vec<ExternalScope>, projects: ProjectScope) -> Fixture {
    let dir = TempDir::new().unwrap();
    let (engine, _rx) = Engine::new(
        vec![Provider {
            id: ProviderId::ClaudeCode,
            display_name: "Claude".into(),
            needs_node: false,
            launch: LaunchSpec {
                command: "false".into(),
                args: Vec::new(),
                env: Vec::new(),
                cwd: None,
            },
        }],
        SkillLibrary::default(),
    );
    let state = engine.external_mcp_state();
    state.set_enabled(true);
    state.configure_data_dir(dir.path()).unwrap();
    let (record, token) = state
        .with_registry(|reg| reg.create("sse-test", scopes, projects, TtlChoice::Days(1)))
        .unwrap()
        .unwrap();
    Fixture {
        engine: Arc::new(engine),
        _dir: dir,
        token,
        client_id: record.id,
    }
}

fn fixture() -> Fixture {
    fixture_with(vec![ExternalScope::Read], ProjectScope::All)
}

fn push(f: &Fixture, kind: EventKind, session: &str, project: Option<&str>) {
    f.engine
        .external_mcp_state()
        .hub()
        .ring()
        .lock()
        .unwrap()
        .push(
            kind,
            Some(session.into()),
            project.map(str::to_string),
            None,
            json!({}),
        );
}

async fn post_handler(
    State(engine): State<Arc<Engine>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default()
        .to_string();
    assert!(is_post_stream_request(&payload));
    post_events_stream_with(engine, &headers, bearer, payload, FAST)
        .await
        .into_response()
}

async fn serve_post(engine: Arc<Engine>) -> SocketAddr {
    let app = Router::new()
        .route("/mcp", post(post_handler))
        .with_state(engine);
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    addr
}

/// A raw HTTP connection that reads an SSE body incrementally.
struct Conn {
    stream: TcpStream,
    seen: String,
}

impl Conn {
    async fn open(addr: SocketAddr, request: String) -> Self {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        Self {
            stream,
            seen: String::new(),
        }
    }

    async fn post(addr: SocketAddr, token: &str, accept: &str, body: &Value) -> Self {
        let body = body.to_string();
        Self::open(
            addr,
            format!(
                "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {token}\r\nAccept: {accept}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                addr.port(),
                body.len()
            ),
        )
        .await
    }

    async fn get(addr: SocketAddr, token: &str, extra: &[(&str, &str)]) -> Self {
        let mut request = format!(
            "GET /external-mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {token}\r\nAccept: text/event-stream\r\n",
            addr.port()
        );
        for (name, value) in extra {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        Self::open(addr, request).await
    }

    async fn read_some(&mut self, wait: Duration) -> bool {
        let mut buf = [0u8; 4096];
        match tokio::time::timeout(wait, self.stream.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 => {
                self.seen.push_str(&String::from_utf8_lossy(&buf[..n]));
                true
            }
            _ => false,
        }
    }

    /// Reads until `needle` shows up in what has been received so far.
    async fn wait_for(&mut self, needle: &str, within: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + within;
        while tokio::time::Instant::now() < deadline {
            if self.seen.contains(needle) {
                return true;
            }
            self.read_some(Duration::from_millis(100)).await;
        }
        self.seen.contains(needle)
    }

    fn status(&self) -> u16 {
        self.seen
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }

    /// JSON bodies of every `data:` line received so far.
    fn messages(&self) -> Vec<Value> {
        self.seen
            .lines()
            .filter_map(|line| line.trim().strip_prefix("data:"))
            .filter_map(|data| serde_json::from_str(data.trim()).ok())
            .collect()
    }

    /// The JSON object of a non-SSE response body (errors answered before a stream opens).
    fn body_json(&self) -> Value {
        let body = self
            .seen
            .split_once("\r\n\r\n")
            .map(|(_, b)| b)
            .unwrap_or("");
        let start = body.find('{').expect("json body");
        let end = body.rfind('}').expect("json body end");
        serde_json::from_str(&body[start..=end]).expect("valid json body")
    }

    /// Waits for a complete SSE message with this JSON-RPC `method`.
    async fn message(&mut self, method: &str, within: Duration) -> Value {
        let deadline = tokio::time::Instant::now() + within;
        loop {
            if let Some(found) = self.messages().into_iter().find(|m| m["method"] == method) {
                return found;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "no {method} message in: {}",
                self.seen
            );
            self.read_some(Duration::from_millis(100)).await;
        }
    }

    async fn closed_within(&mut self, within: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + within;
        let mut buf = [0u8; 4096];
        while tokio::time::Instant::now() < deadline {
            if self.seen.ends_with("0\r\n\r\n") {
                return true; // chunked body finished: the server ended the stream
            }
            match tokio::time::timeout(Duration::from_millis(100), self.stream.read(&mut buf)).await
            {
                Ok(Ok(0)) | Ok(Err(_)) => return true,
                Ok(Ok(n)) => self.seen.push_str(&String::from_utf8_lossy(&buf[..n])),
                Err(_) => {}
            }
        }
        false
    }
}

async fn wait_stream_count(f: &Fixture, expected: usize) -> bool {
    let subs = f.engine.external_mcp_state().subscriptions().clone();
    for _ in 0..60 {
        if subs.stream_count(&f.client_id) == expected {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

const SSE: &str = "application/json, text/event-stream";

#[tokio::test]
async fn listen_first_message_is_the_acknowledgement_bound_to_the_request_id() {
    let f = fixture();
    let addr = serve_post(f.engine.clone()).await;
    let request = json!({
        "jsonrpc": "2.0", "id": 7, "method": "subscriptions/listen",
        "params": {
            "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" },
            "notifications": {
                "resourceSubscriptions": [EVENTS_RESOURCE_URI, "codetwo://other"],
                "toolsListChanged": true
            }
        }
    });
    let mut conn = Conn::post(addr, &f.token, SSE, &request).await;
    assert!(
        conn.wait_for(
            "notifications/subscriptions/acknowledged",
            Duration::from_secs(3)
        )
        .await,
        "{}",
        conn.seen
    );
    assert_eq!(conn.status(), 200);
    assert!(conn.seen.to_lowercase().contains("text/event-stream"));
    let first = conn
        .message(
            "notifications/subscriptions/acknowledged",
            Duration::from_secs(3),
        )
        .await;
    assert_eq!(
        first["params"]["_meta"]["io.modelcontextprotocol/subscriptionId"],
        7
    );
    assert_eq!(
        first["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"],
        "2026-07-28"
    );
    assert_eq!(
        first["params"]["notifications"],
        json!({ "resourceSubscriptions": [EVENTS_RESOURCE_URI] }),
        "only the honored subset is acknowledged"
    );

    push(&f, EventKind::TurnStarted, "s1", Some("/proj"));
    assert!(
        conn.wait_for("notifications/resources/updated", Duration::from_secs(3))
            .await,
        "{}",
        conn.seen
    );
    let update = conn
        .message("notifications/resources/updated", Duration::from_secs(3))
        .await;
    assert_eq!(update["params"]["uri"], EVENTS_RESOURCE_URI);
    assert_eq!(
        update["params"]["_meta"]["io.modelcontextprotocol/subscriptionId"],
        7
    );
}

#[tokio::test]
async fn events_stream_sends_active_then_direct_occurrences_then_heartbeat_data_frames() {
    let f = fixture();
    let addr = serve_post(f.engine.clone()).await;
    // Pre-existing events must not replay for a null cursor.
    push(&f, EventKind::TurnStarted, "old", Some("/proj"));
    let request = json!({
        "jsonrpc": "2.0", "id": "evt-1", "method": "events/stream",
        "params": { "name": "codetwo.turn.started", "arguments": {}, "cursor": null }
    });
    let mut conn = Conn::post(addr, &f.token, SSE, &request).await;
    assert!(
        conn.wait_for("notifications/events/active", Duration::from_secs(3))
            .await,
        "{}",
        conn.seen
    );
    let active = conn
        .message("notifications/events/active", Duration::from_secs(3))
        .await;
    assert_eq!(active["params"]["truncated"], false);
    assert!(active["params"]["cursor"].as_str().is_some());
    assert_eq!(
        active["params"]["_meta"]["io.modelcontextprotocol/subscriptionId"],
        "evt-1"
    );

    push(&f, EventKind::SessionCreated, "other-kind", Some("/proj"));
    push(&f, EventKind::TurnStarted, "new", Some("/proj"));
    assert!(
        conn.wait_for("notifications/events/event", Duration::from_secs(3))
            .await,
        "{}",
        conn.seen
    );
    let occurrences: Vec<_> = conn
        .messages()
        .into_iter()
        .filter(|m| m["method"] == "notifications/events/event")
        .collect();
    assert_eq!(
        occurrences.len(),
        1,
        "only the subscribed event name is delivered: {}",
        conn.seen
    );
    let params = &occurrences[0]["params"];
    assert!(
        params.get("event").is_none(),
        "params are the EventOccurrence itself"
    );
    assert_eq!(params["name"], "codetwo.turn.started");
    assert!(params["eventId"].as_str().is_some());
    assert!(params["cursor"].as_str().is_some());
    assert_eq!(params["data"]["session_id"], "new");
    assert!(params["timestamp"].as_str().is_some());

    // Quiet stream: heartbeat is a data frame carrying the cursor, not an SSE comment.
    assert!(
        conn.wait_for("notifications/events/heartbeat", Duration::from_secs(3))
            .await,
        "{}",
        conn.seen
    );
    let beat = conn
        .message("notifications/events/heartbeat", Duration::from_secs(3))
        .await;
    assert!(beat["params"]["cursor"].as_str().is_some());
}

#[tokio::test]
async fn events_stream_resumes_from_a_cursor_and_reports_a_gap_as_truncated() {
    let f = fixture();
    let addr = serve_post(f.engine.clone()).await;
    let base = f.engine.external_mcp_state().hub().head_cursor();
    push(&f, EventKind::TurnStarted, "a", Some("/p"));
    push(&f, EventKind::TurnStarted, "b", Some("/p"));
    let resume = json!({
        "jsonrpc": "2.0", "id": 1, "method": "events/stream",
        "params": { "name": "codetwo.turn.started", "cursor": base }
    });
    let mut conn = Conn::post(addr, &f.token, SSE, &resume).await;
    assert!(
        conn.wait_for("\"session_id\":\"b\"", Duration::from_secs(3))
            .await,
        "{}",
        conn.seen
    );
    let sessions: Vec<String> = conn
        .messages()
        .iter()
        .filter(|m| m["method"] == "notifications/events/event")
        .map(|m| {
            m["params"]["data"]["session_id"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(sessions, ["a", "b"]);
    let active = conn
        .messages()
        .into_iter()
        .find(|message| message["method"] == "notifications/events/active")
        .unwrap();
    assert_eq!(
        active["params"]["cursor"], base,
        "active must preserve the starting checkpoint until occurrences are delivered"
    );
    // A client retaining only the active checkpoint can disconnect and replay without loss.
    drop(conn);
    assert!(
        wait_stream_count(&f, 0).await,
        "initial disconnect releases its slot"
    );
    let replay_request = json!({
        "jsonrpc": "2.0", "id": 3, "method": "events/stream",
        "params": { "name": "codetwo.turn.started", "cursor": active["params"]["cursor"] }
    });
    let mut replay = Conn::post(addr, &f.token, SSE, &replay_request).await;
    assert!(
        replay
            .wait_for("\"session_id\":\"b\"", Duration::from_secs(3))
            .await
    );
    let replayed: Vec<_> = replay
        .messages()
        .into_iter()
        .filter(|message| message["method"] == "notifications/events/event")
        .map(|message| {
            message["params"]["data"]["session_id"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(replayed, ["a", "b"]);
    drop(replay);
    assert!(
        wait_stream_count(&f, 0).await,
        "replay disconnect releases its slot"
    );

    let epoch = base.split(':').next().unwrap();
    let stale = json!({
        "jsonrpc": "2.0", "id": 2, "method": "events/stream",
        "params": { "name": "codetwo.turn.started", "cursor": format!("{epoch}:99999") }
    });
    let mut gap = Conn::post(addr, &f.token, SSE, &stale).await;
    assert!(
        gap.wait_for("notifications/events/active", Duration::from_secs(3))
            .await
    );
    let gap_active = gap
        .message("notifications/events/active", Duration::from_secs(3))
        .await;
    assert_eq!(
        gap_active["params"]["truncated"], true,
        "a gap is not an error"
    );

    let malformed = json!({
        "jsonrpc": "2.0", "id": 3, "method": "events/stream",
        "params": { "name": "codetwo.turn.started", "cursor": "garbage" }
    });
    let mut bad = Conn::post(addr, &f.token, SSE, &malformed).await;
    assert!(
        bad.wait_for("\"error\"", Duration::from_secs(3)).await,
        "{}",
        bad.seen
    );
    assert_eq!(bad.status(), 200);
    assert_eq!(bad.body_json()["id"], 3);
    assert!(
        wait_stream_count(&f, 1).await,
        "only the live gap stream holds a slot; rejected requests hold none"
    );
}

#[tokio::test]
async fn disconnect_releases_the_stream_slot_even_when_idle() {
    let f = fixture();
    let addr = serve_post(f.engine.clone()).await;
    let request = json!({
        "jsonrpc": "2.0", "id": 1, "method": "events/stream",
        "params": { "name": "codetwo.turn.completed" }
    });
    let mut conn = Conn::post(addr, &f.token, SSE, &request).await;
    assert!(
        conn.wait_for("notifications/events/active", Duration::from_secs(3))
            .await
    );
    assert_eq!(
        f.engine
            .external_mcp_state()
            .subscriptions()
            .stream_count(&f.client_id),
        1
    );
    drop(conn);
    assert!(
        wait_stream_count(&f, 0).await,
        "the task must notice the closed connection"
    );

    let listen = json!({
        "jsonrpc": "2.0", "id": 2, "method": "subscriptions/listen",
        "params": { "notifications": { "resourceSubscriptions": [EVENTS_RESOURCE_URI] } }
    });
    let conn = Conn::post(addr, &f.token, SSE, &listen).await;
    assert!(wait_stream_count(&f, 1).await);
    drop(conn);
    assert!(wait_stream_count(&f, 0).await);
}

#[tokio::test]
async fn the_per_client_stream_cap_applies_to_post_streams() {
    let f = fixture();
    let addr = serve_post(f.engine.clone()).await;
    let mut held = Vec::new();
    for i in 0..MAX_STREAMS_PER_CLIENT {
        let request = json!({
            "jsonrpc": "2.0", "id": i, "method": "events/stream",
            "params": { "name": "codetwo.turn.started" }
        });
        let mut conn = Conn::post(addr, &f.token, SSE, &request).await;
        assert!(
            conn.wait_for("notifications/events/active", Duration::from_secs(3))
                .await
        );
        held.push(conn);
    }
    let over = json!({
        "jsonrpc": "2.0", "id": 99, "method": "events/stream",
        "params": { "name": "codetwo.turn.started" }
    });
    let mut refused = Conn::post(addr, &f.token, SSE, &over).await;
    assert!(refused.wait_for("\r\n\r\n", Duration::from_secs(3)).await);
    assert_eq!(refused.status(), 429);
    drop(held);
    assert!(wait_stream_count(&f, 0).await);
}

#[tokio::test]
async fn revoking_the_credential_terminates_open_streams() {
    let f = fixture();
    let addr = serve_post(f.engine.clone()).await;
    let events = json!({
        "jsonrpc": "2.0", "id": "e", "method": "events/stream",
        "params": { "name": "codetwo.turn.started" }
    });
    let listen = json!({
        "jsonrpc": "2.0", "id": "l", "method": "subscriptions/listen",
        "params": { "notifications": { "resourceSubscriptions": [EVENTS_RESOURCE_URI] } }
    });
    let mut events_conn = Conn::post(addr, &f.token, SSE, &events).await;
    let mut listen_conn = Conn::post(addr, &f.token, SSE, &listen).await;
    assert!(
        events_conn
            .wait_for("notifications/events/active", Duration::from_secs(3))
            .await
    );
    assert!(
        listen_conn
            .wait_for("acknowledged", Duration::from_secs(3))
            .await
    );

    f.engine
        .external_mcp_state()
        .with_registry(|reg| reg.revoke(&f.client_id))
        .unwrap()
        .unwrap();

    assert!(
        events_conn
            .wait_for("notifications/events/terminated", Duration::from_secs(3))
            .await,
        "{}",
        events_conn.seen
    );
    assert!(
        listen_conn
            .wait_for("\"error\"", Duration::from_secs(3))
            .await,
        "{}",
        listen_conn.seen
    );
    assert!(wait_stream_count(&f, 0).await, "revocation frees the slots");
}

#[tokio::test]
async fn project_scope_is_applied_to_every_delivery_including_child_projects() {
    let allowed = TempDir::new().unwrap();
    let child = allowed.path().join("child");
    std::fs::create_dir(&child).unwrap();
    let outside = TempDir::new().unwrap();
    let f = fixture_with(
        vec![ExternalScope::Read],
        ProjectScope::Paths(vec![PathBuf::from(allowed.path())]),
    );
    let addr = serve_post(f.engine.clone()).await;
    let request = json!({
        "jsonrpc": "2.0", "id": 1, "method": "events/stream",
        "params": { "name": "codetwo.turn.started" }
    });
    let mut conn = Conn::post(addr, &f.token, SSE, &request).await;
    assert!(
        conn.wait_for("notifications/events/active", Duration::from_secs(3))
            .await
    );

    push(
        &f,
        EventKind::TurnStarted,
        "outside",
        Some(outside.path().to_str().unwrap()),
    );
    push(
        &f,
        EventKind::TurnStarted,
        "child",
        Some(child.to_str().unwrap()),
    );
    push(&f, EventKind::TurnStarted, "unscoped", None);
    assert!(
        conn.wait_for("\"session_id\":\"child\"", Duration::from_secs(3))
            .await,
        "{}",
        conn.seen
    );
    assert!(!conn.seen.contains("\"session_id\":\"outside\""));
    assert!(!conn.seen.contains("\"session_id\":\"unscoped\""));
}

#[tokio::test]
async fn post_streams_enforce_accept_scope_and_request_id_independently() {
    let f = fixture();
    let addr = serve_post(f.engine.clone()).await;
    let request = json!({
        "jsonrpc": "2.0", "id": 1, "method": "events/stream",
        "params": { "name": "codetwo.turn.started" }
    });
    let mut no_accept = Conn::post(addr, &f.token, "application/json", &request).await;
    assert!(no_accept.wait_for("\r\n\r\n", Duration::from_secs(3)).await);
    assert_eq!(no_accept.status(), 406);

    let mut bad_token = Conn::post(addr, "ctmcp_invalid", SSE, &request).await;
    assert!(bad_token.wait_for("\r\n\r\n", Duration::from_secs(3)).await);
    assert_eq!(bad_token.status(), 401);

    let no_id = json!({ "jsonrpc": "2.0", "method": "subscriptions/listen", "params": {} });
    let mut missing = Conn::post(addr, &f.token, SSE, &no_id).await;
    assert!(
        missing.wait_for("\"error\"", Duration::from_secs(3)).await,
        "{}",
        missing.seen
    );

    let wrong_version = json!({
        "jsonrpc": "2.0", "id": 5, "method": "subscriptions/listen",
        "params": { "_meta": { "io.modelcontextprotocol/protocolVersion": "2024-01-01" } }
    });
    let mut wrong = Conn::post(addr, &f.token, SSE, &wrong_version).await;
    assert!(
        wrong.wait_for("\"error\"", Duration::from_secs(3)).await,
        "{}",
        wrong.seen
    );
    assert_eq!(wrong.body_json()["id"], 5);

    let operate_only = fixture_with(vec![ExternalScope::Operate], ProjectScope::All);
    let addr2 = serve_post(operate_only.engine.clone()).await;
    let mut denied = Conn::post(addr2, &operate_only.token, SSE, &request).await;
    assert!(
        denied.wait_for("\"error\"", Duration::from_secs(3)).await,
        "{}",
        denied.seen
    );
    assert_eq!(denied.body_json()["error"]["code"], -32003);
    assert_eq!(
        operate_only
            .engine
            .external_mcp_state()
            .subscriptions()
            .stream_count(&operate_only.client_id),
        0
    );
}

// ---- GET (2025-06-18 compatibility) --------------------------------------------------------

async fn serve_real(engine: Arc<Engine>) -> SocketAddr {
    let auth = Arc::new(AuthState::load(None));
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let (bound, _handle) = bind_and_serve(
        engine,
        fanout(tokio::sync::mpsc::unbounded_channel().1),
        addr,
        auth,
    )
    .await
    .unwrap();
    bound
}

#[tokio::test]
async fn get_requires_enabled_accept_bearer_and_read_scope() {
    let disabled = fixture();
    disabled.engine.external_mcp_state().set_enabled(false);
    let addr = serve_real(disabled.engine.clone()).await;
    let mut conn = Conn::get(addr, &disabled.token, &[]).await;
    assert!(conn.wait_for("\r\n\r\n", Duration::from_secs(3)).await);
    assert_eq!(conn.status(), 404);

    let f = fixture();
    let addr = serve_real(f.engine.clone()).await;
    let mut no_accept = Conn::open(
        addr,
        format!(
            "GET /external-mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\n\r\n",
            addr.port(),
            f.token
        ),
    )
    .await;
    assert!(no_accept.wait_for("\r\n\r\n", Duration::from_secs(3)).await);
    assert_eq!(no_accept.status(), 406);
    let mut bad = Conn::get(addr, "ctmcp_notvalid", &[]).await;
    assert!(bad.wait_for("\r\n\r\n", Duration::from_secs(3)).await);
    assert_eq!(bad.status(), 401);

    let operate_only = fixture_with(vec![ExternalScope::Operate], ProjectScope::All);
    let addr = serve_real(operate_only.engine.clone()).await;
    let mut denied = Conn::get(addr, &operate_only.token, &[]).await;
    assert!(denied.wait_for("\r\n\r\n", Duration::from_secs(3)).await);
    assert_eq!(denied.status(), 403, "GET needs read scope on its own");
}

#[tokio::test]
async fn get_notifies_a_subscribed_client_and_releases_the_slot_on_disconnect() {
    let f = fixture();
    let addr = serve_real(f.engine.clone()).await;
    let mut conn = Conn::get(addr, &f.token, &[]).await;
    assert!(wait_stream_count(&f, 1).await);
    f.engine
        .external_mcp_state()
        .subscriptions()
        .subscribe_resource(&f.client_id, EVENTS_RESOURCE_URI)
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    push(&f, EventKind::TurnStarted, "s", Some("/p"));
    assert!(
        conn.wait_for("notifications/resources/updated", Duration::from_secs(5))
            .await,
        "{}",
        conn.seen
    );
    assert!(
        conn.seen.contains("\nid: "),
        "SSE id carries the cursor for Last-Event-ID resume"
    );
    let note = conn
        .message("notifications/resources/updated", Duration::from_secs(3))
        .await;
    assert!(
        note["params"]["_meta"]
            .get("io.modelcontextprotocol/subscriptionId")
            .is_none(),
        "the legacy stream has no subscription id"
    );
    drop(conn);
    assert!(wait_stream_count(&f, 0).await, "disconnect frees the slot");
}

#[tokio::test]
async fn get_without_subscription_stays_quiet_and_only_the_newest_stream_notifies() {
    let f = fixture();
    let addr = serve_real(f.engine.clone()).await;
    let mut first = Conn::get(addr, &f.token, &[]).await;
    assert!(wait_stream_count(&f, 1).await);
    push(&f, EventKind::TurnStarted, "s", Some("/p"));
    assert!(
        !first
            .wait_for(
                "notifications/resources/updated",
                Duration::from_millis(600)
            )
            .await
    );

    f.engine
        .external_mcp_state()
        .subscriptions()
        .subscribe_resource(&f.client_id, EVENTS_RESOURCE_URI)
        .unwrap();
    let mut second = Conn::get(addr, &f.token, &[]).await;
    assert!(wait_stream_count(&f, 2).await);
    tokio::time::sleep(Duration::from_millis(100)).await;
    push(&f, EventKind::TurnStarted, "s2", Some("/p"));
    assert!(
        second
            .wait_for("notifications/resources/updated", Duration::from_secs(5))
            .await
    );
    assert!(
        !first
            .wait_for(
                "notifications/resources/updated",
                Duration::from_millis(600)
            )
            .await,
        "one message, one stream"
    );
}

#[tokio::test]
async fn get_stream_limit_returns_429_and_stale_last_event_id_emits_reset() {
    let f = fixture();
    let addr = serve_real(f.engine.clone()).await;
    let mut held = Vec::new();
    for _ in 0..MAX_STREAMS_PER_CLIENT {
        held.push(Conn::get(addr, &f.token, &[]).await);
    }
    assert!(wait_stream_count(&f, MAX_STREAMS_PER_CLIENT).await);
    let mut over = Conn::get(addr, &f.token, &[]).await;
    assert!(over.wait_for("\r\n\r\n", Duration::from_secs(3)).await);
    assert_eq!(over.status(), 429);
    drop(held);
    assert!(wait_stream_count(&f, 0).await);

    let epoch = f.engine.external_mcp_state().hub().head_cursor();
    let epoch = epoch.split(':').next().unwrap().to_string();
    let mut stale = Conn::get(
        addr,
        &f.token,
        &[("Last-Event-ID", &format!("{epoch}:999999"))],
    )
    .await;
    assert!(
        stale
            .wait_for("codetwo-reset", Duration::from_secs(3))
            .await,
        "{}",
        stale.seen
    );
    let mut garbage = Conn::get(addr, &f.token, &[("Last-Event-ID", "not-a-cursor")]).await;
    assert!(
        garbage
            .wait_for("codetwo-reset", Duration::from_secs(3))
            .await,
        "{}",
        garbage.seen
    );
}

#[tokio::test]
async fn get_stream_ends_when_the_credential_is_revoked() {
    let f = fixture();
    let addr = serve_real(f.engine.clone()).await;
    let mut conn = Conn::get(addr, &f.token, &[]).await;
    assert!(wait_stream_count(&f, 1).await);
    f.engine
        .external_mcp_state()
        .with_registry(|reg| reg.revoke(&f.client_id))
        .unwrap()
        .unwrap();
    assert!(
        conn.closed_within(Duration::from_secs(12)).await,
        "revalidation closes the stream"
    );
    assert!(wait_stream_count(&f, 0).await);
}

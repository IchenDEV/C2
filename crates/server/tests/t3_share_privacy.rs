//! R25 audit: the secondary T3-compat HTTP routes (`/api/orchestration/*`) are reachable with an
//! ordinary paired credential (`/oauth/token` bootstrap exchange) and must enforce the same
//! creator-only private visibility as `/ws` and `/api/web-ui/call`.
//!
//! FIXTURE: real loopback Axum listener + real `KernelWebUiCommands` + one persisted ShareStore.
//! The agent is a disposable python ACP stub (zero fee, no paid Agent, no user data).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use codetwo_core::canvas::CanvasFeatureGate;
use codetwo_core::plugins::{AppConfig, CoreApp, PluginManager};
use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
use codetwo_core::share::ShareStore;
use codetwo_core::skill::SkillLibrary;
use codetwo_core::{Engine, Store};
use codetwo_server::{bind_and_serve_with_web_ui, fanout, AuthState, DEFAULT_PAIRING_TTL};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;

const SCRIPT: &str = r#"
import json,sys
for line in sys.stdin:
    m=json.loads(line); method=m.get('method'); mid=m.get('id'); result={}
    if method=='initialize': result={'protocolVersion':1}
    elif method=='session/new': result={'sessionId':'fx'}
    elif method=='session/prompt': result={'stopReason':'end_turn'}
    elif mid is None: continue
    print(json.dumps({'jsonrpc':'2.0','id':mid,'result':result}),flush=True)
"#;

async fn http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    body: &str,
) -> (u16, String) {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let ctype = if path == "/oauth/token" {
        "application/x-www-form-urlencoded"
    } else {
        "application/json"
    };
    let auth = bearer
        .map(|b| format!("Authorization: Bearer {b}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Type: {ctype}\r\n{auth}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

async fn t3_bearer(addr: SocketAddr, auth: &AuthState) -> String {
    let token = auth.issue_t3_pairing_token(DEFAULT_PAIRING_TTL);
    let form = format!(
        "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Atoken-exchange&subject_token={token}\
         &subject_token_type=urn%3At3%3Aparams%3Aoauth%3Atoken-type%3Aenvironment-bootstrap\
         &requested_token_type=urn%3Aietf%3Aparams%3Aoauth%3Atoken-type%3Aaccess_token\
         &scope=orchestration%3Aread+orchestration%3Aoperate"
    );
    let (status, body) = http(addr, "POST", "/oauth/token", None, &form).await;
    assert_eq!(status, 200, "{body}");
    serde_json::from_str::<Value>(&body).unwrap()["access_token"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn t3_compat_routes_hide_and_refuse_another_actors_private_session() {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let cwd: String = root.path().canonicalize().unwrap().to_string_lossy().into();
    let app = CoreApp::boot(AppConfig::new(data.path()))
        .await
        .expect("boot");
    let plugin_manager = app.service::<PluginManager>().expect("plugin manager");
    let share = Arc::new(ShareStore::open(data.path().join("t3-shares.json")).unwrap());
    let kernel = Arc::new(codetwo_server::KernelWebUiCommands::with_share_store(
        plugin_manager,
        share.clone(),
    ));
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (engine, rx) = Engine::with_store(
        vec![Provider {
            id: ProviderId::Grok,
            display_name: "T3 privacy fixture (zero-fee)".into(),
            launch: LaunchSpec::new("python3", ["-c", SCRIPT]),
            needs_node: false,
        }],
        SkillLibrary::new(vec![]),
        store.clone(),
    );
    engine.attach_share_store(share.clone());
    let engine = Arc::new(engine);
    let auth = Arc::new(AuthState::load(None));
    let alice_dev = auth
        .pair(&auth.issue_pairing_token(DEFAULT_PAIRING_TTL), "alice")
        .unwrap()
        .device_id;
    let (addr, _handle) = bind_and_serve_with_web_ui(
        engine.clone(),
        fanout(rx),
        "127.0.0.1:0".parse().unwrap(),
        auth.clone(),
        store,
        CanvasFeatureGate::default(),
        None,
        Some(kernel),
        None,
    )
    .await
    .unwrap();

    // Alice creates a PRIVATE task through the ordinary raw socket.
    let ticket = auth.issue_ws_ticket(&alice_dev);
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws?ticket={ticket}"))
        .await
        .unwrap();
    ws.next().await.unwrap().unwrap();
    ws.send(Message::Text(
        json!({"op": "new_session", "provider": "grok", "cwd": cwd, "use_worktree": false,
               "request_id": "p1", "visibility": "private"})
        .to_string(),
    ))
    .await
    .unwrap();
    let mut private = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while private.is_none() {
        let Ok(Some(Ok(msg))) = tokio::time::timeout_at(deadline, ws.next()).await else {
            break;
        };
        if let Ok(v) = serde_json::from_str::<Value>(&msg.into_text().unwrap_or_default()) {
            if let Some(event) = v.get("event") {
                if event.get("cwd").is_some() {
                    private = event["session"].as_str().map(str::to_string);
                }
            }
        }
    }
    let private = private.expect("alice private create");
    let alice_actor = share.participant(&alice_dev);
    assert!(share.can_view(&private, &alice_actor));

    // Bob: a different ordinary paired credential, through the T3 compatibility surface.
    let bob = t3_bearer(addr, &auth).await;
    let (status, shell) = http(addr, "GET", "/api/orchestration/shell", Some(&bob), "").await;
    assert_eq!(status, 200);
    assert!(
        !shell.contains(&private),
        "shell snapshot leaked another actor's private task id"
    );
    let (status, thread) = http(
        addr,
        "GET",
        &format!("/api/orchestration/threads/{private}"),
        Some(&bob),
        "",
    )
    .await;
    assert_eq!(
        status, 404,
        "thread snapshot of a private task must look unknown: {thread}"
    );
    assert!(!thread.contains(&private) || thread.starts_with("unknown thread"));
    let cmd =
        json!({"type": "thread.turn.interrupt", "commandId": "r25-cancel", "threadId": private})
            .to_string();
    let (status, body) = http(
        addr,
        "POST",
        "/api/orchestration/dispatch",
        Some(&bob),
        &cmd,
    )
    .await;
    assert_eq!(
        status, 400,
        "dispatch against a private task must fail like an unknown one: {body}"
    );
    assert!(body.starts_with("unknown thread"), "{body}");

    // Public T3 functionality + creator metadata + actor-scoped receipts. Device A (T3 bearer)
    // creates a default-public thread; device B reuses the SAME commandId for another thread.
    let a = t3_bearer(addr, &auth).await;
    let (_, shell_a) = http(addr, "GET", "/api/orchestration/shell", Some(&a), "").await;
    let shell_a: Value = serde_json::from_str(&shell_a).unwrap();
    let project = shell_a["projects"][0]["id"].as_str().unwrap().to_string();
    let start = |thread: &str| {
        json!({"type": "thread.turn.start", "commandId": "same-cmd", "threadId": thread,
               "message": {"messageId": format!("m-{thread}"), "text": "hi", "attachments": []},
               "interactionMode": "default",
               "bootstrap": {"createThread": {"projectId": project,
                   "modelSelection": {"instanceId": "grok"}}}})
        .to_string()
    };
    let (status, body) = http(
        addr,
        "POST",
        "/api/orchestration/dispatch",
        Some(&a),
        &start("t3-pub-a"),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = http(
        addr,
        "POST",
        "/api/orchestration/dispatch",
        Some(&bob),
        &start("t3-pub-b"),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let (_, shell_b) = http(addr, "GET", "/api/orchestration/shell", Some(&bob), "").await;
    assert!(
        shell_b.contains("t3-pub-a"),
        "public T3 thread must stay visible to participants"
    );
    assert!(
        shell_b.contains("t3-pub-b"),
        "another actor's reused commandId must not swallow this actor's command via its receipt"
    );
    assert!(!shell_b.contains(&private));
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn t3_socket(addr: SocketAddr, bearer: &str) -> Ws {
    let (status, reply) = http(addr, "POST", "/api/auth/websocket-ticket", Some(bearer), "").await;
    assert_eq!(status, 200, "{reply}");
    let ticket = serde_json::from_str::<Value>(&reply).unwrap()["ticket"]
        .as_str()
        .unwrap()
        .to_string();
    tokio_tungstenite::connect_async(format!("ws://{addr}/ws?wsTicket={ticket}"))
        .await
        .unwrap()
        .0
}

async fn rpc(ws: &mut Ws, frame: Value) {
    ws.send(Message::Text(frame.to_string())).await.unwrap();
}

/// Next JSON frame within `wait`, or None. Every received frame is appended to `log`.
async fn frame(ws: &mut Ws, wait: Duration, log: &mut String) -> Option<Value> {
    let Ok(Some(Ok(msg))) = tokio::time::timeout(wait, ws.next()).await else {
        return None;
    };
    let text = msg.into_text().unwrap_or_default();
    log.push_str(&text);
    log.push('\n');
    serde_json::from_str(&text).ok()
}

async fn ack(ws: &mut Ws, request_id: &str) {
    rpc(ws, json!({"_tag": "Ack", "requestId": request_id})).await;
}

/// Request/response RPC: returns the first frame for `id` (acking chunks).
async fn call(ws: &mut Ws, id: &str, tag: &str, payload: Value, log: &mut String) -> Value {
    rpc(ws, json!({"_tag": "Request", "id": id, "tag": tag, "payload": payload, "headers": []}))
        .await;
    loop {
        let f = frame(ws, Duration::from_secs(5), log)
            .await
            .unwrap_or_else(|| panic!("no frame for {id}"));
        if f["requestId"] == id {
            if f["_tag"] == "Chunk" {
                ack(ws, id).await;
            }
            return f;
        }
    }
}

/// Drain/ack every frame until the socket is quiet.
async fn settle(ws: &mut Ws, log: &mut String) {
    while let Some(f) = frame(ws, Duration::from_millis(700), log).await {
        if f["_tag"] == "Chunk" {
            let id = f["requestId"].as_str().unwrap_or_default().to_string();
            ack(ws, &id).await;
        }
    }
}

async fn raw_create(ws: &mut Ws, cwd: &str, request_id: &str, visibility: &str) -> String {
    ws.send(Message::Text(
        json!({"op": "new_session", "provider": "grok", "cwd": cwd, "use_worktree": false,
               "request_id": request_id, "visibility": visibility})
        .to_string(),
    ))
    .await
    .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let Ok(Some(Ok(msg))) = tokio::time::timeout_at(deadline, ws.next()).await else {
            panic!("create {request_id} timed out");
        };
        if let Ok(v) = serde_json::from_str::<Value>(&msg.into_text().unwrap_or_default()) {
            if let Some(event) = v.get("event") {
                if event.get("cwd").is_some() {
                    return event["session"].as_str().unwrap().to_string();
                }
            }
        }
    }
}

/// R26 Priority B: real T3 WebSocket frames between distinct actors.
#[tokio::test]
async fn t3_websocket_frames_enforce_private_visibility_revoke_and_actor_receipts() {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let cwd: String = root.path().canonicalize().unwrap().to_string_lossy().into();
    let app = CoreApp::boot(AppConfig::new(data.path())).await.expect("boot");
    let plugin_manager = app.service::<PluginManager>().expect("plugin manager");
    let share = Arc::new(ShareStore::open(data.path().join("t3-shares.json")).unwrap());
    let kernel = Arc::new(codetwo_server::KernelWebUiCommands::with_share_store(
        plugin_manager,
        share.clone(),
    ));
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (engine, rx) = Engine::with_store(
        vec![Provider {
            id: ProviderId::Grok,
            display_name: "T3 privacy fixture (zero-fee)".into(),
            launch: LaunchSpec::new("python3", ["-c", SCRIPT]),
            needs_node: false,
        }],
        SkillLibrary::new(vec![]),
        store.clone(),
    );
    engine.attach_share_store(share.clone());
    let engine = Arc::new(engine);
    let auth = Arc::new(AuthState::load(None));
    let alice_dev = auth
        .pair(&auth.issue_pairing_token(DEFAULT_PAIRING_TTL), "alice")
        .unwrap()
        .device_id;
    let (addr, _handle) = bind_and_serve_with_web_ui(
        engine.clone(),
        fanout(rx),
        "127.0.0.1:0".parse().unwrap(),
        auth.clone(),
        store,
        CanvasFeatureGate::default(),
        None,
        Some(kernel),
        None,
    )
    .await
    .unwrap();

    let ticket = auth.issue_ws_ticket(&alice_dev);
    let (mut alice, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws?ticket={ticket}"))
        .await
        .unwrap();
    alice.next().await.unwrap().unwrap();
    let private = raw_create(&mut alice, &cwd, "w-private", "private").await;
    let public = raw_create(&mut alice, &cwd, "w-public", "public").await;
    let alice_actor = share.participant(&alice_dev);

    let (bob_token, carol_token) = (t3_bearer(addr, &auth).await, t3_bearer(addr, &auth).await);
    let (mut bob, mut carol) = (
        t3_socket(addr, &bob_token).await,
        t3_socket(addr, &carol_token).await,
    );
    let (mut blog, mut clog) = (String::new(), String::new());

    // Shell: both distinct actors see the public task and never the creator-private one.
    let shell = call(&mut bob, "s-bob", "orchestration.subscribeShell", json!({}), &mut blog).await;
    let shell_text = shell.to_string();
    assert!(shell_text.contains(&public), "public task must reach T3 shell: {shell_text}");
    assert!(!shell_text.contains(&private), "private id leaked in T3 shell frame");
    let shell = call(&mut carol, "s-carol", "orchestration.subscribeShell", json!({}), &mut clog).await;
    assert!(shell.to_string().contains(&public) && !shell.to_string().contains(&private));

    // Direct thread subscription: private is denied exactly like an unknown thread.
    let denied = call(
        &mut bob,
        "t-private",
        "orchestration.subscribeThread",
        json!({"threadId": private}),
        &mut blog,
    )
    .await;
    assert_eq!(denied["_tag"], "Exit", "{denied}");
    assert_eq!(denied["exit"]["_tag"], "Failure", "{denied}");
    assert!(denied.to_string().contains("unknown thread"), "{denied}");
    let granted = call(
        &mut bob,
        "t-public",
        "orchestration.subscribeThread",
        json!({"threadId": public}),
        &mut blog,
    )
    .await;
    assert_eq!(granted["_tag"], "Chunk", "{granted}");

    // Public controls still work for another actor, with actor-scoped receipts (same commandId).
    let project = shell["values"][0]["snapshot"]["projects"][0]["id"].as_str().unwrap().to_string();
    let start = |thread: &str| {
        json!({"type": "thread.turn.start", "commandId": "ws-same", "threadId": thread,
               "message": {"messageId": format!("m-{thread}"), "text": "hi", "attachments": []},
               "interactionMode": "default",
               "bootstrap": {"createThread": {"projectId": project,
                   "modelSelection": {"instanceId": "grok"}}}})
    };
    let a = call(&mut bob, "d-bob", "orchestration.dispatchCommand", start("ws-bob"), &mut blog).await;
    let b = call(&mut carol, "d-carol", "orchestration.dispatchCommand", start("ws-carol"), &mut clog).await;
    assert_eq!(a["exit"]["_tag"], "Success", "{a}");
    assert_eq!(
        b["exit"]["_tag"], "Success",
        "reused commandId from another actor must not collide with Bob's receipt: {b}"
    );
    settle(&mut bob, &mut blog).await;
    settle(&mut carol, &mut clog).await;

    // Revoke: creator flips the public task private while Bob/Carol are subscribed.
    blog.clear();
    clog.clear();
    share.set_visibility(&public, &alice_actor, codetwo_core::share::Visibility::Private).unwrap();
    let mut saw_refresh = false;
    while let Some(f) = frame(&mut bob, Duration::from_secs(3), &mut blog).await {
        if f["_tag"] == "Chunk" {
            let id = f["requestId"].as_str().unwrap_or_default().to_string();
            assert_ne!(id, "t-public", "nested thread update delivered after revoke: {f}");
            assert!(!f.to_string().contains(&public), "revoked task still in pushed snapshot: {f}");
            saw_refresh |= id == "s-bob";
            ack(&mut bob, &id).await;
        }
    }
    assert!(saw_refresh, "shell subscription must refresh (without the task) after revoke");
    assert!(!blog.contains(&public), "revoked id appeared in a post-revoke frame:\n{blog}");
    // A queued/pending update must not resurrect it either, and dispatch is refused.
    let denied = call(
        &mut bob,
        "d-after",
        "orchestration.dispatchCommand",
        json!({"type": "thread.turn.interrupt", "commandId": "ws-after", "threadId": public}),
        &mut blog,
    )
    .await;
    assert_eq!(denied["exit"]["_tag"], "Failure", "{denied}");
    assert!(denied.to_string().contains("unknown thread"), "{denied}");
    // Bob's own public thread (created through T3) stays visible to Carol.
    let shell = call(&mut carol, "s-carol-2", "orchestration.subscribeShell", json!({}), &mut clog).await;
    assert!(shell.to_string().contains("ws-bob") && !shell.to_string().contains(&public));
}

/// R26 Priority A: an engine that owns a ShareStore but has NO share policy wired to the T3
/// adapter must fail closed (no private task read, no ownerless create). Engine without a
/// ShareStore keeps the legacy unshared T3 behaviour.
#[tokio::test]
async fn t3_without_wired_policy_fails_closed_when_engine_has_a_share_store() {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let cwd: String = root.path().canonicalize().unwrap().to_string_lossy().into();
    let share = Arc::new(ShareStore::open(data.path().join("t3-shares.json")).unwrap());
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (engine, rx) = Engine::with_store(
        vec![Provider {
            id: ProviderId::Grok,
            display_name: "T3 privacy fixture (zero-fee)".into(),
            launch: LaunchSpec::new("python3", ["-c", SCRIPT]),
            needs_node: false,
        }],
        SkillLibrary::new(vec![]),
        store.clone(),
    );
    engine.attach_share_store(share.clone());
    let engine = Arc::new(engine);
    let auth = Arc::new(AuthState::load(None));
    let alice_dev = auth
        .pair(&auth.issue_pairing_token(DEFAULT_PAIRING_TTL), "alice")
        .unwrap()
        .device_id;
    // No `web_ui_commands`: the policy is missing although the engine has a ShareStore.
    let (addr, _handle) = bind_and_serve_with_web_ui(
        engine.clone(),
        fanout(rx),
        "127.0.0.1:0".parse().unwrap(),
        auth.clone(),
        store,
        CanvasFeatureGate::default(),
        None,
        None,
        None,
    )
    .await
    .unwrap();
    // Owned/private metadata exists only in the ShareStore: register one for the raw owner.
    let actor = share.participant(&alice_dev);
    share.expect_create("seed", &actor, codetwo_core::share::Visibility::Private);
    let sid = engine
        .submit(codetwo_core::Op::NewSession {
            provider: ProviderId::Grok,
            cwd: cwd.clone(),
            use_worktree: false,
            worktree_base: None,
            worktree_base_sha: None,
            request_id: Some("seed".into()),
            model: None,
            initial_policy: None,
        })
        .await;
    assert!(sid.is_ok());
    tokio::time::sleep(Duration::from_millis(800)).await;
    let session = engine.list_sessions().unwrap().remove(0).id;
    assert!(!share.can_view(&session, &share.participant("someone-else")));

    let bob = t3_bearer(addr, &auth).await;
    let (status, shell) = http(addr, "GET", "/api/orchestration/shell", Some(&bob), "").await;
    assert_eq!(status, 200);
    assert!(!shell.contains(&session), "missing policy must not expose a private session: {shell}");
    let (status, _) =
        http(addr, "GET", &format!("/api/orchestration/threads/{session}"), Some(&bob), "").await;
    assert_eq!(status, 404);
    let project = serde_json::from_str::<Value>(&shell).unwrap()["projects"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let create = json!({"type": "thread.turn.start", "commandId": "np", "threadId": "t3-np",
        "message": {"messageId": "m", "text": "hi", "attachments": []},
        "interactionMode": "default",
        "bootstrap": {"createThread": {"projectId": project, "modelSelection": {"instanceId": "grok"}}}})
    .to_string();
    let (status, body) =
        http(addr, "POST", "/api/orchestration/dispatch", Some(&bob), &create).await;
    assert_eq!(status, 400, "create without a wired policy must be refused: {body}");
    assert!(body.contains("share policy is not wired"), "{body}");
}

/// R26 route triage: `/api/artifacts/:id` is not a capability. A blob referenced only by another
/// actor's private task is not downloadable by an ordinary paired device (404), while the creator
/// and a public task's artifact still work.
#[tokio::test]
async fn artifact_download_follows_the_referencing_task_visibility() {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let cwd: String = root.path().canonicalize().unwrap().to_string_lossy().into();
    let app = CoreApp::boot(AppConfig::new(data.path())).await.expect("boot");
    let plugin_manager = app.service::<PluginManager>().expect("plugin manager");
    let share = Arc::new(ShareStore::open(data.path().join("t3-shares.json")).unwrap());
    let kernel = Arc::new(codetwo_server::KernelWebUiCommands::with_share_store(
        plugin_manager,
        share.clone(),
    ));
    let store = Arc::new(Store::open(data.path().join("art.db").to_str().unwrap()).unwrap());
    let (engine, rx) = Engine::with_store(
        vec![Provider {
            id: ProviderId::Grok,
            display_name: "T3 privacy fixture (zero-fee)".into(),
            launch: LaunchSpec::new("python3", ["-c", SCRIPT]),
            needs_node: false,
        }],
        SkillLibrary::new(vec![]),
        store.clone(),
    );
    engine.attach_share_store(share.clone());
    let engine = Arc::new(engine);
    let auth = Arc::new(AuthState::load(None));
    let alice = auth
        .pair(&auth.issue_pairing_token(DEFAULT_PAIRING_TTL), "alice")
        .unwrap();
    let bob = auth
        .pair(&auth.issue_pairing_token(DEFAULT_PAIRING_TTL), "bob")
        .unwrap();
    let (addr, _handle) = bind_and_serve_with_web_ui(
        engine.clone(),
        fanout(rx),
        "127.0.0.1:0".parse().unwrap(),
        auth.clone(),
        store.clone(),
        CanvasFeatureGate::default(),
        None,
        Some(kernel),
        None,
    )
    .await
    .unwrap();
    let ticket = auth.issue_ws_ticket(&alice.device_id);
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws?ticket={ticket}"))
        .await
        .unwrap();
    ws.next().await.unwrap().unwrap();
    let private = raw_create(&mut ws, &cwd, "a-private", "private").await;
    let public = raw_create(&mut ws, &cwd, "a-public", "public").await;
    let artifacts = codetwo_core::ArtifactStore::from_store(store).expect("file-backed artifacts");
    let secret = artifacts
        .save_document("private payload", "text/plain", Some("s.txt"), &private, "tc1")
        .unwrap();
    let open = artifacts
        .save_document("public payload", "text/plain", Some("p.txt"), &public, "tc2")
        .unwrap();
    let get = |id: String, bearer: String| async move {
        http(addr, "GET", &format!("/api/artifacts/{id}"), Some(&bearer), "").await
    };
    let (status, body) = get(secret.id.clone(), bob.bearer.clone()).await;
    assert_eq!(status, 404, "another actor's private artifact must not download: {body}");
    assert!(!body.contains("private payload"));
    let (status, body) = get(secret.id.clone(), alice.bearer.clone()).await;
    assert_eq!((status, body.as_str()), (200, "private payload"));
    let (status, body) = get(open.id, bob.bearer.clone()).await;
    assert_eq!((status, body.as_str()), (200, "public payload"));

    // R27: real deletion drops artifact_refs but leaves the blob. The id alone is not authority:
    // an orphaned blob of a deleted private task must not become downloadable by anyone.
    assert!(engine.delete_idle_session(&private).unwrap(), "private task really deleted");
    assert!(artifacts.referencing_sessions(&secret.id).unwrap().is_empty(), "ref cleaned");
    let (status, body) = get(secret.id.clone(), bob.bearer).await;
    assert_eq!(status, 404, "orphaned private blob must fail closed: {body}");
    let (status, body) = get(secret.id, alice.bearer).await;
    assert_eq!(status, 404, "no referencing task = no authority, even for the old creator: {body}");
}

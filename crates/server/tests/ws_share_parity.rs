//! Raw ordinary `/ws` parity with HTTP/native for shared sessions: real loopback Axum socket,
//! real `KernelWebUiCommands` adapter + Core policy, two paired devices using the same client id.
//!
//! FIXTURE: the agent is a disposable python ACP stub (zero fee). No paid Agent, no user data.

use std::sync::Arc;
use std::time::Duration;

use codetwo_core::canvas::CanvasFeatureGate;
use codetwo_core::plugins::{AppConfig, CoreApp, PluginManager};
use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
use codetwo_core::share::{ShareStore, Visibility};
use codetwo_core::skill::SkillLibrary;
use codetwo_core::{Engine, Store};
use codetwo_server::{bind_and_serve_with_web_ui, fanout, AuthState, DEFAULT_PAIRING_TTL};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
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

type Ws = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

async fn connect(addr: std::net::SocketAddr, auth: &AuthState, device: &str) -> Ws {
    let ticket = auth.issue_ws_ticket(device);
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws?ticket={ticket}"))
        .await
        .unwrap();
    let welcome = ws.next().await.unwrap().unwrap().into_text().unwrap();
    assert!(welcome.contains("\"kind\":\"sessions\""));
    ws
}

/// Read every frame arriving within `window` (bounded).
async fn drain(ws: &mut Ws, window: Duration) -> Vec<Value> {
    let mut frames = Vec::new();
    let deadline = tokio::time::Instant::now() + window;
    while let Ok(Some(Ok(msg))) = tokio::time::timeout_at(deadline, ws.next()).await {
        if let Ok(text) = msg.into_text() {
            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                frames.push(value);
            }
        }
    }
    frames
}

fn created_for(frames: &[Value], request_id: &str) -> Option<String> {
    frames.iter().find_map(|frame| {
        let event = frame.get("event")?;
        (event["request_id"] == request_id && event.get("cwd").is_some())
            .then(|| event["session"].as_str().map(str::to_string))
            .flatten()
    })
}

async fn wait_idle(engine: &Engine, session: &str) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while engine.current_turn(session).is_some() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn raw_ws_matches_core_policy_for_actor_author_and_private_at_create() {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let cwd: String = root.path().canonicalize().unwrap().to_string_lossy().into();
    // Real adapter wiring: PluginManager from a booted Core + the one persisted ShareStore.
    let app = CoreApp::boot(AppConfig::new(data.path())).await.expect("boot");
    let plugin_manager = app.service::<PluginManager>().expect("plugin manager");
    let share = Arc::new(ShareStore::open(data.path().join("ws-shares.json")).unwrap());
    let kernel = Arc::new(codetwo_server::KernelWebUiCommands::with_share_store(
        plugin_manager,
        share.clone(),
    ));
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (engine, rx) = Engine::with_store(
        vec![Provider {
            id: ProviderId::Grok,
            display_name: "WS parity fixture (zero-fee, not a paid Agent)".into(),
            launch: LaunchSpec::new("python3", ["-c", SCRIPT]),
            needs_node: false,
        }],
        SkillLibrary::new(vec![]),
        store.clone(),
    );
    engine.attach_share_store(share.clone());
    let engine = Arc::new(engine);
    let auth = Arc::new(AuthState::load(None));
    let alice_dev = auth.pair(&auth.issue_pairing_token(DEFAULT_PAIRING_TTL), "alice").unwrap().device_id;
    let bob_dev = auth.pair(&auth.issue_pairing_token(DEFAULT_PAIRING_TTL), "bob").unwrap().device_id;
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
    let (mut alice, mut bob) = (
        connect(addr, &auth, &alice_dev).await,
        connect(addr, &auth, &bob_dev).await,
    );
    let (alice_actor, bob_actor) = (
        share.participant(&alice_dev),
        share.participant(&bob_dev),
    );
    let new_session = |request_id: &str, extra: Value| {
        let mut op = json!({"op": "new_session", "provider": "grok", "cwd": cwd,
            "use_worktree": false, "request_id": request_id});
        op.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        Message::Text(op.to_string())
    };

    // Private-at-create over the raw socket; bob uses the SAME client id for a default session.
    alice.send(new_session("c1", json!({"visibility": "private"}))).await.unwrap();
    bob.send(new_session("c1", json!({}))).await.unwrap();
    let alice_frames = drain(&mut alice, Duration::from_millis(1500)).await;
    let bob_frames = drain(&mut bob, Duration::from_millis(500)).await;
    let private = created_for(&alice_frames, &format!("device:{alice_dev}/c1"))
        .expect("alice receives her private create");
    let public = created_for(&bob_frames, &format!("device:{bob_dev}/c1")).expect("bob's own create");
    assert_ne!(private, public, "same client id, independent actor namespaces");
    let record = share.read(&private, &alice_actor).unwrap();
    assert_eq!((record.creator.as_str(), record.visibility), (alice_actor.id.as_str(), Visibility::Private));
    assert_eq!(share.read(&public, &bob_actor).unwrap().visibility, Visibility::Public);
    // No public-then-toggle: no revoked/visibility event ever existed, and bob/host never saw it.
    let leaked = |frames: &[Value]| frames.iter().any(|f| f.to_string().contains(&private));
    assert!(!leaked(&bob_frames), "private create leaked to another device");
    assert!(!share.can_view(&private, &bob_actor));
    assert!(!share.can_view(&private, &share.participant("")), "host denied private");
    assert!(!alice_frames.iter().any(|f| f.to_string().contains("session_share_changed")));
    wait_idle(&engine, &private).await;

    // Invalid visibility is rejected before any create.
    bob.send(new_session("bad", json!({"visibility": "sorta"}))).await.unwrap();
    let bad = drain(&mut bob, Duration::from_millis(500)).await;
    assert!(bad.iter().any(|f| f.to_string().contains("visibility must be public or private")));
    assert!(created_for(&bad, "device:bob/bad").is_none());

    // Prompts: both actors, same client id, on the public session -> durable per-actor authors.
    let prompt = |session: &str, text: &str| {
        Message::Text(json!({"op": "prompt", "session": session, "request_id": "same",
            "doc": [{"type": "text", "text": text}]}).to_string())
    };
    alice.send(prompt(&public, "from alice")).await.unwrap();
    wait_idle(&engine, &public).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    bob.send(prompt(&public, "from bob")).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    wait_idle(&engine, &public).await;
    let authors = share.message_authors(&public).unwrap();
    let who: Vec<_> = authors.values().map(|a| a.actor.clone()).collect(); // seq order
    assert_eq!(who, vec![alice_actor.id.clone(), bob_actor.id.clone()], "{authors:?}");
    for (seq, author) in &authors {
        assert!(author.request_id.as_deref().unwrap().ends_with("/same"), "seq {seq}");
    }
    let journal = share.read(&public, &bob_actor).unwrap().journal;
    assert!(journal.iter().any(|op| op.kind == "prompt" && op.actor == alice_actor.id));
    assert!(journal.iter().any(|op| op.kind == "prompt" && op.actor == bob_actor.id));

    // Private denied to a guest over the socket: error, no author, no transcript row.
    drain(&mut alice, Duration::from_millis(100)).await;
    bob.send(prompt(&private, "intrusion")).await.unwrap();
    let denied = drain(&mut bob, Duration::from_millis(700)).await;
    assert!(denied.iter().any(|f| f.to_string().contains("session not found")), "{denied:?}");
    assert!(share.message_authors(&private).unwrap().is_empty());

    // Dispatch failure cleanup: a shared session the engine does not know rejects the prompt;
    // no pending author intent survives (a fresh intent for the same scoped id inserts).
    share.register_session("ghost", &alice_actor, Visibility::Public).unwrap();
    alice.send(prompt("ghost", "nobody home")).await.unwrap();
    drain(&mut alice, Duration::from_millis(700)).await;
    assert!(share.message_authors("ghost").unwrap().is_empty());
    assert!(
        share.expect_author("ghost", &format!("device:{alice_dev}/same"), &alice_actor),
        "failed dispatch left a dangling author intent"
    );
}

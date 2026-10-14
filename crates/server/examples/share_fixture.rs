//! Disposable zero-fee sharing E2E fixture (NOT a paid Agent, no user data/auth).
//!
//! ONE real Core (`CoreApp::boot`; the same EngineService/StoreService/EventBus production `serve.rs` uses), real `KernelWebUiCommands` + `ShareStore`, real Axum
//! `bind_and_serve_with_web_ui` serving the Vite production build, and a python ACP stub provider.
//! Prints two one-time pairing links (one per browser origin, `127.0.0.1` and `localhost`) so two
//! independent browser storage contexts pair into DIFFERENT devices. Runs until its own stdin closes
//! SIGTERM from its owner (run.py timeout).
//!
//! Paired devices persist in `<data_dir>/remote-devices.json` (the production `AuthState` store), so a
//! restart on the same `FIXTURE_PORT` keeps the same A/B credentials.
//!
//! Usage: [FIXTURE_PORT=n] share_fixture <ui_dist_dir> <data_dir> <cwd>

use std::path::PathBuf;
use std::sync::Arc;

use codetwo_core::plugins::{
    AppConfig, CanvasService, CoreApp, EngineService, EventBus, PluginManager, StoreService,
};
use codetwo_server::{bind_and_serve_with_web_ui, pairing_url_for_endpoint, AuthState, DEFAULT_PAIRING_TTL};

const SCRIPT: &str = r#"
import json,sys,threading,time
out=threading.Lock(); cancel={}
def send(o):
    with out: print(json.dumps(o),flush=True)
def prompt(mid,sid):
    ev=cancel.setdefault(sid,threading.Event()); ev.clear()
    cancelled=ev.wait(60)  # fixture-only bounded delay so a genuine RUNNING state exists; session/cancel ends it
    send({'jsonrpc':'2.0','id':mid,'result':{'stopReason':'cancelled' if cancelled else 'end_turn'}})
for line in sys.stdin:
    m=json.loads(line); method=m.get('method'); mid=m.get('id'); result={}
    if method=='session/cancel':
        for e in cancel.values(): e.set()
        continue
    if method=='session/prompt':
        threading.Thread(target=prompt,args=(mid,(m.get('params') or {}).get('sessionId','fx')),daemon=True).start(); continue
    if method=='initialize': result={'protocolVersion':1}
    elif method=='session/new': result={'sessionId':'fx'}
    elif mid is None: continue
    send({'jsonrpc':'2.0','id':mid,'result':result})
"#;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let ui = PathBuf::from(&args[1]);
    let data = PathBuf::from(&args[2]);
    let cwd = &args[3];
    let core = CoreApp::boot(AppConfig::new(&data)).await.expect("boot");
    let plugin_manager = core.service::<PluginManager>().expect("plugin manager");
    // Zero-fee provider through the REAL ordinary provider commands: register the python ACP stub
    // and disable every built-in provider, so no paid Agent can run in this Core. The commands
    // reload the plugin graph, so the engine is resolved only after it carries exactly one provider.
    let _ = plugin_manager
        .call(
            "providers.register",
            serde_json::json!({ "configuration": {
                "id": "fixture-acp", "display_name": "Fixture ACP (zero-fee, NOT a paid Agent)",
                "command": "python3", "args": ["-c", SCRIPT] } }),
        )
        .await;
    // Disable every built-in provider (ids from the static registry; no capability probing).
    for provider in codetwo_core::default_registry() {
        let id = provider.id.as_str().to_string();
        // Each change reloads the graph; retry while the command is briefly absent.
        let mut done = false;
        for _ in 0..100 {
            if plugin_manager
                .call("providers.set_enabled", serde_json::json!({ "provider": id, "enabled": false }))
                .await
                .is_ok()
            {
                done = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(done, "could not disable provider {id}");
    }
    let mut engine = None;
    for _ in 0..100 {
        let Some(service) = core.service::<EngineService>() else {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            continue;
        };
        let candidate = service.0.clone();
        let ids: Vec<String> =
            candidate.registered_providers().iter().map(|p| p.id.as_str().to_string()).collect();
        if ids == ["fixture-acp"] {
            engine = Some(candidate);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let engine = engine.expect("engine never reloaded with ONLY the zero-fee provider");
    println!("PROVIDERS {:?}", engine.registered_providers().iter().map(|p| p.id.as_str()).collect::<Vec<_>>());
    let store = core.service::<StoreService>().expect("store plugin").0.clone();
    let events = core.service::<EventBus>().expect("bus plugin").0.clone();
    let canvas_gate = core.service::<CanvasService>().expect("canvas service").gate;
    // Host setup through the REAL ordinary `projects.add` Core command (the browser renderer is
    // not allowed to add projects, so a paired browser creates tasks in this preconfigured project).
    let added = plugin_manager
        .call("projects.add", serde_json::json!({ "path": cwd, "name": "share-fixture" }))
        .await
        .expect("projects.add");
    println!("PROJECT_ADDED {added}");
    // Same wiring as production serve.rs.
    let auth = Arc::new(AuthState::load(Some(data.join("remote-devices.json"))));
    let kernel = Arc::new(
        codetwo_server::KernelWebUiCommands::with_share_store(
            plugin_manager,
            codetwo_core::share::shared_store(data.join("session-shares.json")).unwrap(),
        )
        .with_pairing(auth.clone()),
    );
    let (addr, _h) = bind_and_serve_with_web_ui(
        engine,
        events,
        format!("127.0.0.1:{}", std::env::var("FIXTURE_PORT").unwrap_or_else(|_| "0".into())).parse().unwrap(),
        auth.clone(),
        store,
        canvas_gate,
        None,
        Some(kernel),
        Some(ui),
    )
    .await
    .unwrap();
    let port = addr.port();
    for (label, host) in [("A", "127.0.0.1"), ("B", "localhost")] {
        let tok = auth.issue_pairing_token(DEFAULT_PAIRING_TTL);
        println!("PAIR_{label} {}", pairing_url_for_endpoint(&format!("http://{host}:{port}"), &tok));
    }
    println!("READY port={port}");
    std::future::pending::<()>().await;
}

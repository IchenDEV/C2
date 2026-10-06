//! Engine-level backend selection for native providers, over the real node sidecar with the
//! test-only fake SDK. These pin routing and persistence, not real provider behavior:
//! native stays opt-in, the choice is durable and per session, and a native session is never
//! served by ACP, even after the process restarts or the opt-in is withdrawn.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use codetwo_core::connectors::select::NativeBackends;
use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
use codetwo_core::skill::{DocBlock, SkillLibrary};
use codetwo_core::{Engine, Event, Op, RuntimeBackendKind, Store};
use tokio::sync::mpsc::UnboundedReceiver;

struct Owner(Engine);

impl Drop for Owner {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

fn sidecars() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../script/provider-sidecars")
}

fn fake_sdk() -> String {
    sidecars()
        .join("common/fake-claude-sdk.mjs")
        .to_string_lossy()
        .into_owned()
}

/// The ACP launch of this provider only records that it was started; the fake SDK path rides in
/// the environment so the native sidecar inherits it.
fn claude_provider(acp_marker: &Path) -> Provider {
    Provider {
        id: ProviderId::ClaudeCode,
        display_name: "Claude Code".into(),
        needs_node: false,
        launch: LaunchSpec {
            command: "sh".into(),
            args: vec![
                "-c".into(),
                format!("touch '{}'; exit 1", acp_marker.display()),
            ],
            env: vec![("CODETWO_SIDECAR_SDK_MODULE".into(), fake_sdk())],
            cwd: None,
        },
    }
}

fn native_claude() -> NativeBackends {
    NativeBackends::default()
        .enable(RuntimeBackendKind::ClaudeAgentSdk)
        .with_sidecar_root(sidecars())
}

fn engine(
    store: &Arc<Store>,
    marker: &Path,
    backends: NativeBackends,
) -> (Owner, UnboundedReceiver<Event>) {
    let (engine, events) = Engine::with_store(
        vec![claude_provider(marker)],
        SkillLibrary::default(),
        store.clone(),
    );
    engine.set_native_backends(backends);
    (Owner(engine.clone()), events)
}

async fn create_session(
    owner: &Owner,
    events: &mut UnboundedReceiver<Event>,
) -> Result<String, String> {
    owner
        .0
        .submit(Op::NewSession {
            provider: ProviderId::ClaudeCode,
            cwd: std::env::temp_dir().to_string_lossy().into(),
            use_worktree: false,
            worktree_base: None,
            worktree_base_sha: None,
            request_id: Some("create".into()),
            model: None,
            initial_policy: None,
        })
        .await
        .map_err(|error| error.to_string())?;
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            match events.recv().await.expect("event channel closed") {
                Event::SessionCreated { session, .. } => return Ok(session),
                Event::Error {
                    terminal: true,
                    message,
                    ..
                } => return Err(message),
                _ => {}
            }
        }
    })
    .await
    .expect("session creation timed out")
}

/// Send one prompt and return the agent text and how the turn ended.
async fn prompt(
    owner: &Owner,
    events: &mut UnboundedReceiver<Event>,
    session: &str,
    text: &str,
) -> (String, String) {
    owner
        .0
        .submit(Op::Prompt {
            session: session.into(),
            doc: vec![DocBlock::Text { text: text.into() }],
            request_id: None,
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut said = String::new();
        loop {
            match events.recv().await.expect("event channel closed") {
                Event::AgentText { text, .. } => said.push_str(&text),
                Event::TurnEnded { stop_reason, .. } => return (said, stop_reason),
                Event::Error {
                    terminal: true,
                    message,
                    ..
                } => panic!("{message}"),
                _ => {}
            }
        }
    })
    .await
    .expect("turn timed out")
}

#[tokio::test]
async fn native_is_opt_in_and_existing_behavior_stays_acp() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("acp-started");
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (owner, mut events) = engine(&store, &marker, NativeBackends::default());
    // The ACP command is the one that runs, and it fails: that is the pre-existing behavior.
    let _ = create_session(&owner, &mut events).await;
    assert!(marker.exists(), "default selection must still launch ACP");
}

#[tokio::test]
async fn an_opted_in_session_runs_natively_and_the_choice_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("acp-started");
    let store = Arc::new(Store::open_in_memory().unwrap());

    let (owner, mut events) = engine(&store, &marker, native_claude());
    let session = create_session(&owner, &mut events).await.unwrap();
    assert_eq!(
        store
            .session_runtime_binding(&session)
            .unwrap()
            .map(|(kind, _)| kind),
        Some(RuntimeBackendKind::ClaudeAgentSdk)
    );
    let (said, reason) = prompt(&owner, &mut events, &session, "hello").await;
    assert_eq!(said, "Hello from fake");
    assert_eq!(reason, "EndTurn");
    drop(owner);

    // A new Engine process with the opt-in withdrawn: the session is bound, so it still starts
    // the native sidecar; falling back to ACP would hit the failing marker command.
    let (owner, mut events) = engine(
        &store,
        &marker,
        NativeBackends::default().with_sidecar_root(sidecars()),
    );
    let (said, reason) = prompt(&owner, &mut events, &session, "hello again").await;
    assert_eq!(said, "Hello from fake");
    assert_eq!(reason, "EndTurn");
    assert!(
        !marker.exists(),
        "a native session must never be served by ACP"
    );
}

#[tokio::test]
async fn a_native_backend_that_cannot_start_is_an_error_not_an_acp_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("acp-started");
    let store = Arc::new(Store::open_in_memory().unwrap());
    // Opted in, but the sidecar location is unknown.
    let backends = NativeBackends::default().enable(RuntimeBackendKind::ClaudeAgentSdk);
    let (owner, mut events) = engine(&store, &marker, backends);
    let error = create_session(&owner, &mut events).await.unwrap_err();
    assert!(error.contains("CODETWO_PROVIDER_SIDECARS_DIR"), "{error}");
    assert!(
        !marker.exists(),
        "ACP must not run in place of a native backend"
    );
}

#[tokio::test]
async fn sessions_without_a_binding_stay_acp_even_when_native_is_enabled_later() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("acp-started");
    let store = Arc::new(Store::open_in_memory().unwrap());
    // A session persisted before native backends existed: no backend row.
    let legacy = codetwo_core::session::Session::new(
        ProviderId::ClaudeCode,
        std::env::temp_dir().to_string_lossy().into_owned(),
    );
    store.upsert_session(&legacy).unwrap();
    assert!(store.session_runtime_binding(&legacy.id).unwrap().is_none());

    let (owner, mut events) = engine(&store, &marker, native_claude());
    owner
        .0
        .submit(Op::Prompt {
            session: legacy.id.clone(),
            doc: vec![DocBlock::Text { text: "hi".into() }],
            request_id: None,
        })
        .await
        .ok();
    // Recovery launches the legacy ACP command (which fails here) and never the sidecar.
    tokio::time::timeout(Duration::from_secs(10), async {
        while !marker.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("legacy session was not recovered through ACP");
    assert!(store.session_runtime_binding(&legacy.id).unwrap().is_none());
    while events.try_recv().is_ok() {}
}

#[test]
fn a_session_binding_is_idempotent_and_never_rewritten_to_another_backend() {
    let store = Store::open_in_memory().unwrap();
    let session = codetwo_core::session::Session::new(
        ProviderId::ClaudeCode,
        std::env::temp_dir().to_string_lossy().into_owned(),
    );
    store.upsert_session(&session).unwrap();
    let kind = RuntimeBackendKind::ClaudeAgentSdk;
    store.bind_session_runtime(&session.id, kind, 1).unwrap();
    store.bind_session_runtime(&session.id, kind, 1).unwrap();
    let error = store
        .bind_session_runtime(&session.id, RuntimeBackendKind::Acp, 1)
        .unwrap_err();
    assert!(error.to_string().contains("refusing"), "{error}");
    assert_eq!(
        store.session_runtime_binding(&session.id).unwrap(),
        Some((kind, 1))
    );
}

#[test]
fn a_provider_switch_replaces_the_binding_in_the_same_transaction() {
    let store = Store::open_in_memory().unwrap();
    let session = codetwo_core::session::Session::new(
        ProviderId::ClaudeCode,
        std::env::temp_dir().to_string_lossy().into_owned(),
    );
    store.upsert_session(&session).unwrap();
    store
        .bind_session_runtime(&session.id, RuntimeBackendKind::ClaudeAgentSdk, 1)
        .unwrap();
    let context = serde_json::json!({});

    // Switching to a native provider records its backend with the provider change.
    assert!(store
        .switch_session_provider_with_runtime(
            &session.id,
            &ProviderId::ClaudeCode,
            &ProviderId::Codex,
            None,
            &context,
            Some(RuntimeBackendKind::CodexAppServer),
        )
        .unwrap());
    assert_eq!(
        store
            .session_runtime_binding(&session.id)
            .unwrap()
            .map(|(kind, _)| kind),
        Some(RuntimeBackendKind::CodexAppServer)
    );

    // Switching to an ACP provider leaves no native binding behind.
    assert!(store
        .switch_session_provider_with_runtime(
            &session.id,
            &ProviderId::Codex,
            &ProviderId::Grok,
            None,
            &context,
            None,
        )
        .unwrap());
    assert!(store
        .session_runtime_binding(&session.id)
        .unwrap()
        .is_none());
}

/// Drive one "tool" turn: answer the first permission request with `answer`, and return the
/// permission options offered, the final status of the tool call and how the turn ended.
async fn tool_turn(
    owner: &Owner,
    events: &mut UnboundedReceiver<Event>,
    session: &str,
    answer: impl Fn(&[(String, String)]) -> Option<String>,
) -> (Vec<(String, String)>, String, String) {
    owner
        .0
        .submit(Op::Prompt {
            session: session.into(),
            doc: vec![DocBlock::Text {
                text: "run a tool".into(),
            }],
            request_id: None,
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut offered = Vec::new();
        let mut status = String::new();
        loop {
            match events.recv().await.expect("event channel closed") {
                Event::PermissionRequest {
                    session: from,
                    request_id,
                    options,
                    ..
                } => {
                    assert_eq!(from, session, "approval must carry its own session");
                    let option_id = answer(&options);
                    offered = options;
                    owner
                        .0
                        .submit(Op::AnswerPermission {
                            session: session.into(),
                            request_id,
                            option_id,
                        })
                        .await
                        .unwrap();
                }
                Event::ToolCall { status: next, .. } => status = next,
                Event::TurnEnded { stop_reason, .. } => return (offered, status, stop_reason),
                Event::Error {
                    terminal: true,
                    message,
                    ..
                } => panic!("{message}"),
                _ => {}
            }
        }
    })
    .await
    .expect("tool turn timed out")
}

#[tokio::test]
async fn native_approvals_go_through_the_engine_and_a_dismissal_is_a_denial() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("acp-started");
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (owner, mut events) = engine(&store, &marker, native_claude());
    let session = create_session(&owner, &mut events).await.unwrap();

    // Approve: the sidecar's tool request is shown to the user and the tool then completes.
    let (offered, status, reason) = tool_turn(&owner, &mut events, &session, |options| {
        options
            .iter()
            .find(|(id, _)| id.contains("allow") && !id.contains("always"))
            .map(|(id, _)| id.clone())
    })
    .await;
    assert!(!offered.is_empty(), "the user was never asked");
    assert_eq!(status, "completed");
    assert_eq!(reason, "EndTurn");

    // Dismiss (no option chosen): never treated as approval.
    let (_, status, _) = tool_turn(&owner, &mut events, &session, |_| None).await;
    assert_eq!(status, "failed");
    assert!(!marker.exists());
}

#[tokio::test]
async fn stop_through_the_engine_settles_from_the_sdk_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("acp-started");
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (owner, mut events) = engine(&store, &marker, native_claude());
    let session = create_session(&owner, &mut events).await.unwrap();
    owner
        .0
        .submit(Op::Prompt {
            session: session.clone(),
            doc: vec![DocBlock::Text {
                text: "hang until stopped".into(),
            }],
            request_id: None,
        })
        .await
        .unwrap();
    // The backend is provably mid-turn once its first text arrives.
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Event::AgentText { .. } = events.recv().await.expect("event channel closed") {
                break;
            }
        }
    })
    .await
    .expect("backend never started the turn");
    owner
        .0
        .submit(Op::Cancel {
            session: session.clone(),
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while owner.0.current_turn(&session).is_some() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("stop never settled");

    // The cancelled turn's own terminal receipt must be consumed before the next turn starts.
    let ended = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Event::TurnEnded { stop_reason, .. } =
                events.recv().await.expect("event channel closed")
            {
                return stop_reason;
            }
        }
    })
    .await
    .expect("the cancelled turn never reported its terminal");
    assert_eq!(ended, "Cancelled");

    // The session is usable afterwards.
    let (said, reason) = prompt(&owner, &mut events, &session, "hello").await;
    assert_eq!(said, "Hello from fake");
    assert_eq!(reason, "EndTurn");
}

#[tokio::test]
async fn a_sidecar_crash_mid_turn_ends_the_turn_without_replaying_it() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("acp-started");
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (owner, mut events) = engine(&store, &marker, native_claude());
    let session = create_session(&owner, &mut events).await.unwrap();
    owner
        .0
        .submit(Op::Prompt {
            session: session.clone(),
            doc: vec![DocBlock::Text {
                text: "crash now".into(),
            }],
            request_id: None,
        })
        .await
        .unwrap();
    // The turn must end with an error and release the session; it must not be re-sent.
    let message = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            match events.recv().await.expect("event channel closed") {
                Event::Error { message, .. } => return message,
                Event::TurnEnded { stop_reason, .. } => {
                    panic!("a crash is not a clean end: {stop_reason}")
                }
                _ => {}
            }
        }
    })
    .await
    .expect("crash was never reported");
    assert!(!message.is_empty());
    tokio::time::timeout(Duration::from_secs(10), async {
        while owner.0.current_turn(&session).is_some() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("turn stayed running after the backend died");
    assert!(!marker.exists());
}

#[tokio::test]
async fn stop_right_after_the_turn_starts_still_settles() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("acp-started");
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (owner, mut events) = engine(&store, &marker, native_claude());
    let session = create_session(&owner, &mut events).await.unwrap();
    owner
        .0
        .submit(Op::Prompt {
            session: session.clone(),
            doc: vec![DocBlock::Text {
                text: "hang until stopped".into(),
            }],
            request_id: None,
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while owner.0.current_turn(&session).is_none() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("turn never started");
    owner
        .0
        .submit(Op::Cancel {
            session: session.clone(),
        })
        .await
        .unwrap();
    let settled = tokio::time::timeout(Duration::from_secs(5), async {
        while owner.0.current_turn(&session).is_some() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await;
    let mut seen = Vec::new();
    while let Ok(event) = events.try_recv() {
        seen.push(format!("{event:?}").chars().take(120).collect::<String>());
    }
    assert!(settled.is_ok(), "stop never settled; events: {seen:#?}");
}

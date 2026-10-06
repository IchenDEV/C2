//! Running-turn ownership is a core invariant, not frontend-local state. A parked permission keeps
//! the turn slot occupied; a concurrent prompt is rejected without ending that real turn; and the
//! slot becomes available again only after the provider finishes.

use std::sync::Arc;
use std::time::Duration;

use codetwo_core::event::Event;
use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
use codetwo_core::session::{Part, Role, Session};
use codetwo_core::skill::{DocBlock, SkillLibrary};
use codetwo_core::{Engine, Op, RunFailureReason, SessionRunState, Store};

const BLOCKING_AGENT: &str = r#"
import json, sys

def send(message):
    print(json.dumps(message), flush=True)

pending_prompt = None
prompt_count = 0
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    message = json.loads(line)
    method = message.get("method")
    mid = message.get("id")
    if method == "initialize":
        send({"jsonrpc":"2.0","id":mid,"result":{"protocolVersion":1}})
    elif method == "session/new":
        send({"jsonrpc":"2.0","id":mid,"result":{"sessionId":"agent-session"}})
    elif method == "session/prompt":
        prompt_count += 1
        if prompt_count == 1:
            pending_prompt = mid
            send({"jsonrpc":"2.0","id":1000,"method":"session/request_permission","params":{
                "sessionId":"agent-session",
                "toolCall":{"toolCallId":"tool-1","title":"Wait for approval","kind":"execute"},
                "options":[{"optionId":"allow","name":"Allow","kind":"allow_once"}]
            }})
        else:
            send({"jsonrpc":"2.0","id":mid,"result":{"stopReason":"end_turn"}})
    elif method is None and message.get("id") == 1000 and pending_prompt is not None:
        send({"jsonrpc":"2.0","id":pending_prompt,"result":{"stopReason":"end_turn"}})
        pending_prompt = None
"#;

const FAILING_AGENT: &str = r#"
import json, sys

def send(message):
    print(json.dumps(message), flush=True)

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    message = json.loads(line)
    method = message.get("method")
    mid = message.get("id")
    if method == "initialize":
        send({"jsonrpc":"2.0","id":mid,"result":{"protocolVersion":1}})
    elif method == "session/new":
        send({"jsonrpc":"2.0","id":mid,"result":{"sessionId":"agent-session"}})
    elif method == "session/prompt":
        send({"jsonrpc":"2.0","id":mid,"error":{"code":-32000,"message":"prompt failed"}})
"#;

fn prompt(session: &str, text: &str, request_id: &str) -> Op {
    Op::Prompt {
        session: session.into(),
        doc: vec![DocBlock::Text { text: text.into() }],
        request_id: Some(request_id.into()),
    }
}

async fn next_event(rx: &mut tokio::sync::mpsc::UnboundedReceiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("event before timeout")
        .expect("event stream remains open")
}

#[tokio::test]
async fn an_unknown_session_is_rejected_without_starting_a_turn() {
    let (engine, mut rx) = Engine::new(Vec::new(), SkillLibrary::new(vec![]));

    engine
        .submit(prompt("missing-session", "hello", "missing-request"))
        .await
        .unwrap();

    match next_event(&mut rx).await {
        Event::Error {
            session,
            message,
            terminal,
            request_id,
            ..
        } => {
            assert_eq!(session.as_deref(), Some("missing-session"));
            assert_eq!(message, "no such session");
            assert!(terminal);
            assert_eq!(request_id.as_deref(), Some("missing-request"));
        }
        event => panic!("expected a routed terminal error, got {event:?}"),
    }
    assert!(
        rx.try_recv().is_err(),
        "a rejected prompt must not emit turn_started"
    );
}

#[tokio::test]
async fn persistence_failure_rejects_before_start_and_releases_the_turn_slot() {
    let db_path = std::env::temp_dir().join(format!(
        "codetwo-turn-state-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    let store = Arc::new(Store::open(db_path.to_str().unwrap()).unwrap());
    let session = Session::new(
        ProviderId::Grok,
        std::env::temp_dir().to_string_lossy().to_string(),
    );
    let session_id = session.id.clone();
    store.upsert_session(&session).unwrap();

    // A separate RESERVED transaction still allows the engine's session lookup but rejects its
    // prompt write, exercising the durable-acceptance gate without test-only Store hooks.
    let blocker = rusqlite::Connection::open(&db_path).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let (engine, mut rx) = Engine::with_store(Vec::new(), SkillLibrary::new(vec![]), store.clone());

    engine
        .submit(prompt(&session_id, "not durable", "locked-request"))
        .await
        .unwrap();
    match next_event(&mut rx).await {
        Event::Error {
            session,
            message,
            terminal,
            request_id,
        } => {
            assert_eq!(session.as_deref(), Some(session_id.as_str()));
            assert!(
                message.contains("persist prompt"),
                "unexpected error: {message}"
            );
            assert!(terminal);
            assert_eq!(request_id.as_deref(), Some("locked-request"));
        }
        event => panic!("expected a persistence error, got {event:?}"),
    }
    assert!(
        rx.try_recv().is_err(),
        "failed persistence must not emit turn_started"
    );
    assert!(store.transcript(&session_id).unwrap().is_empty());

    blocker.execute_batch("ROLLBACK").unwrap();
    engine
        .submit(prompt(&session_id, "durable retry", "retry-request"))
        .await
        .unwrap();
    let mut saw_start = false;
    loop {
        match next_event(&mut rx).await {
            Event::TurnStarted {
                session,
                request_id,
                transcript_seq,
            } => {
                assert_eq!(session, session_id);
                assert_eq!(request_id.as_deref(), Some("retry-request"));
                assert_eq!(transcript_seq, Some(0));
                saw_start = true;
            }
            Event::Error {
                session,
                terminal,
                request_id,
                ..
            } if terminal => {
                assert_eq!(session.as_deref(), Some(session_id.as_str()));
                assert_eq!(request_id.as_deref(), Some("retry-request"));
                break;
            }
            _ => {}
        }
    }
    assert!(
        saw_start,
        "persistence rejection must release the turn lease"
    );
    assert!(matches!(
        store.transcript(&session_id).unwrap().as_slice(),
        [(Role::User, Part::Prompt { text, .. })] if text == "durable retry"
    ));

    drop(engine);
    drop(rx);
    drop(store);
    drop(blocker);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn new_session_persistence_failure_emits_no_phantom_and_retains_manual_cleanup() {
    if codetwo_core::provider::which("git").is_none() {
        eprintln!("git not found; skipping new-session rollback test");
        return;
    }
    let base = std::env::temp_dir().join(format!(
        "codetwo-new-session-rollback-{}",
        uuid::Uuid::new_v4()
    ));
    let repo = base.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "test@codetwo.dev"]);
    git(&["config", "user.name", "C2 Test"]);
    git(&["commit", "--allow-empty", "-qm", "init"]);

    let db_path = base.join("store.sqlite");
    let store = Arc::new(Store::open(db_path.to_str().unwrap()).unwrap());
    let blocker = rusqlite::Connection::open(&db_path).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let provider = Provider {
        id: ProviderId::Grok,
        display_name: "Failing mock".into(),
        launch: LaunchSpec::new("python3", ["-c", FAILING_AGENT]),
        needs_node: false,
    };
    let (engine, mut rx) =
        Engine::with_store(vec![provider], SkillLibrary::new(vec![]), store.clone());

    engine
        .submit(Op::NewSession {
            provider: ProviderId::Grok,
            cwd: repo.to_string_lossy().into_owned(),
            use_worktree: true,
            worktree_base: Some(codetwo_core::worktree::WorktreeBaseline::Current),
            worktree_base_sha: None,
            request_id: Some("locked-create".into()),
            model: None,
            initial_policy: None,
        })
        .await
        .unwrap();

    match next_event(&mut rx).await {
        Event::Error {
            session,
            message,
            terminal,
            request_id,
        } => {
            assert_eq!(session, None);
            assert!(
                message.contains("persist new session"),
                "unexpected error: {message}"
            );
            assert!(message.contains("manual cleanup"));
            assert!(terminal);
            assert_eq!(request_id.as_deref(), Some("locked-create"));
        }
        event => panic!("expected correlated persistence error, got {event:?}"),
    }
    assert!(
        rx.try_recv().is_err(),
        "failure must not emit SessionCreated"
    );
    assert!(engine.list_sessions().unwrap().is_empty());
    let registrations = codetwo_core::worktree::registrations(&repo).await.unwrap();
    assert_eq!(registrations.len(), 2);
    assert!(registrations.iter().any(|entry| {
        entry
            .branch
            .as_deref()
            .is_some_and(|branch| branch.starts_with("refs/heads/codetwo/"))
            && entry.path.is_dir()
    }));
    assert!(!git(&["branch", "--list", "codetwo/*"]).is_empty());

    blocker.execute_batch("ROLLBACK").unwrap();
    drop(engine);
    drop(rx);
    drop(store);
    drop(blocker);
    let _ = std::fs::remove_dir_all(&base);
}

#[tokio::test]
async fn provider_prompt_failure_keeps_the_prompt_correlation() {
    let provider = Provider {
        id: ProviderId::Grok,
        display_name: "Failing mock".into(),
        launch: LaunchSpec::new("python3", ["-c", FAILING_AGENT]),
        needs_node: false,
    };
    let (engine, mut rx) = Engine::new(vec![provider], SkillLibrary::new(vec![]));
    engine
        .submit(Op::NewSession {
            provider: ProviderId::Grok,
            cwd: std::env::temp_dir().to_string_lossy().to_string(),
            use_worktree: false,
            worktree_base: None,
            worktree_base_sha: None,
            request_id: Some("create-failure-test".into()),
            model: None,
            initial_policy: None,
        })
        .await
        .unwrap();
    let session = loop {
        if let Event::SessionCreated { session, .. } = next_event(&mut rx).await {
            break session;
        }
    };

    engine
        .submit(prompt(&session, "fail", "failing-request"))
        .await
        .unwrap();
    let mut saw_start = false;
    let mut final_activity = None;
    let mut saw_broken = false;
    loop {
        match next_event(&mut rx).await {
            Event::SessionActivityChanged {
                session: routed,
                activity,
            } if routed == session => {
                final_activity = Some(activity);
            }
            Event::ThreadDisposition {
                session: routed,
                disposition: codetwo_core::ThreadDisposition::Broken,
            } if routed == session => {
                saw_broken = true;
            }
            Event::TurnStarted {
                session: routed,
                request_id,
                transcript_seq,
            } => {
                assert_eq!(routed, session);
                assert_eq!(request_id.as_deref(), Some("failing-request"));
                assert_eq!(transcript_seq, None);
                saw_start = true;
            }
            Event::Error {
                session: routed,
                message,
                terminal,
                request_id,
            } if terminal => {
                assert_eq!(routed.as_deref(), Some(session.as_str()));
                assert!(message.contains("prompt failed"));
                assert_eq!(request_id.as_deref(), Some("failing-request"));
                break;
            }
            _ => {}
        }
    }
    assert!(saw_start);
    // Unknown discards the runtime. The failure snapshot is still published even
    // when this Engine has no Store from which list_sessions could recover it.
    assert!(saw_broken);
    assert!(!engine
        .list_sessions()
        .unwrap()
        .iter()
        .any(|candidate| candidate.id == session));
    let failed = final_activity.expect("failed activity published before terminal error");
    assert_eq!(failed.revision, 2);
    assert!(matches!(
        failed.state,
        SessionRunState::Failed {
            reason: RunFailureReason::ProviderError,
            ref message,
            ..
        } if message.contains("prompt failed")
    ));
}

#[tokio::test]
async fn a_parked_turn_rejects_concurrent_prompts_and_releases_after_completion() {
    let provider = Provider {
        id: ProviderId::Grok,
        display_name: "Blocking mock".into(),
        launch: LaunchSpec::new("python3", ["-c", BLOCKING_AGENT]),
        needs_node: false,
    };
    let (engine, mut rx) = Engine::new(vec![provider], SkillLibrary::new(vec![]));
    engine
        .submit(Op::NewSession {
            provider: ProviderId::Grok,
            cwd: std::env::temp_dir().to_string_lossy().to_string(),
            use_worktree: false,
            worktree_base: None,
            worktree_base_sha: None,
            request_id: Some("create-turn-test".into()),
            model: None,
            initial_policy: None,
        })
        .await
        .unwrap();

    let session = loop {
        if let Event::SessionCreated { session, .. } = next_event(&mut rx).await {
            break session;
        }
    };

    engine
        .submit(prompt(&session, "first", "first-request"))
        .await
        .unwrap();
    let mut permission_id = None;
    let mut started = 0;
    while permission_id.is_none() {
        match next_event(&mut rx).await {
            Event::TurnStarted {
                session: id,
                request_id,
                transcript_seq,
            } => {
                assert_eq!(id, session);
                assert_eq!(request_id.as_deref(), Some("first-request"));
                assert_eq!(transcript_seq, None);
                started += 1;
            }
            Event::PermissionRequest { request_id, .. } => permission_id = Some(request_id),
            _ => {}
        }
    }
    assert_eq!(started, 1, "an accepted prompt emits one start event");

    // The provider is parked on permission, so this attempt must not reach ACP or end the real turn.
    engine
        .submit(prompt(&session, "concurrent", "concurrent-request"))
        .await
        .unwrap();
    loop {
        match next_event(&mut rx).await {
            Event::Error {
                session: routed,
                message,
                terminal,
                request_id,
                ..
            } if message.contains("already running") => {
                assert_eq!(routed.as_deref(), Some(session.as_str()));
                assert!(!terminal, "rejection must not clear the live turn");
                assert_eq!(request_id.as_deref(), Some("concurrent-request"));
                break;
            }
            Event::TurnStarted { .. } => panic!("concurrent prompt acquired a second turn slot"),
            _ => {}
        }
    }

    engine
        .submit(Op::AnswerPermission {
            session: session.clone(),
            request_id: permission_id.unwrap(),
            option_id: Some("allow".into()),
        })
        .await
        .unwrap();
    loop {
        if matches!(next_event(&mut rx).await, Event::TurnEnded { .. }) {
            break;
        }
    }

    // TurnEnded is sent only after the core lease is released, so immediate resubmission succeeds.
    engine
        .submit(prompt(&session, "after completion", "restart-request"))
        .await
        .unwrap();
    let mut saw_restart = false;
    loop {
        match next_event(&mut rx).await {
            Event::TurnStarted {
                session: id,
                request_id,
                transcript_seq,
            } => {
                assert_eq!(id, session);
                assert_eq!(request_id.as_deref(), Some("restart-request"));
                assert_eq!(transcript_seq, None);
                saw_restart = true;
            }
            Event::TurnEnded { session: id, .. } => {
                assert_eq!(id, session);
                break;
            }
            Event::Error { message, .. } => panic!("unexpected error: {message}"),
            _ => {}
        }
    }
    assert!(saw_restart, "the completed turn released its slot");
}

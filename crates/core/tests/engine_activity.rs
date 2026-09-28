//! End-to-end lifecycle projection through the real Engine and ACP stdio transport.

use std::sync::Arc;
use std::time::Duration;

use codetwo_core::event::Event;
use codetwo_core::permission::{ExecutionPolicy, PermissionMode, SandboxPolicy};
use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
use codetwo_core::skill::{DocBlock, SkillLibrary};
use codetwo_core::{
    Engine, Op, RunFailureReason, Session, SessionActivity, SessionRunState, Store,
};

const TWO_PERMISSION_AGENT: &str = r#"
import json, sys

def send(message):
    print(json.dumps(message), flush=True)

prompt_id = None
answers = set()
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
        send({"jsonrpc":"2.0","id":mid,"result":{
            "sessionId":"agent-session",
            "models":{
                "availableModels":[
                    {"modelId":"fast","name":"Fast"},
                    {"modelId":"deep","name":"Deep"}
                ],
                "currentModelId":"fast"
            }
        }})
    elif method == "session/prompt":
        prompt_id = mid
        for request_id, title in [(1000, "First permission"), (1001, "Second permission")]:
            send({"jsonrpc":"2.0","id":request_id,"method":"session/request_permission","params":{
                "sessionId":"agent-session",
                "toolCall":{"toolCallId":f"tool-{request_id}","title":title,"kind":"execute"},
                "options":[{"optionId":"allow","name":"Allow","kind":"allow_once"},
                           {"optionId":"reject","name":"Reject","kind":"reject_once"}]
            }})
    elif method is None and mid in (1000, 1001):
        answers.add(mid)
        if len(answers) == 2 and prompt_id is not None:
            send({"jsonrpc":"2.0","id":prompt_id,"result":{"stopReason":"end_turn"}})
            prompt_id = None
    elif method == "session/set_model":
        send({"jsonrpc":"2.0","id":mid,"result":{}})
    elif method == "session/set_config_option":
        send({"jsonrpc":"2.0","id":mid,"result":{"configOptions":[]}})
"#;

const CANCEL_AGENT: &str = r#"
import json, sys

def send(message):
    print(json.dumps(message), flush=True)

prompt_id = None
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
        prompt_id = mid
        for request_id, title in [(2000, "First permission"), (2001, "Second permission")]:
            send({"jsonrpc":"2.0","id":request_id,"method":"session/request_permission","params":{
                "sessionId":"agent-session",
                "toolCall":{"toolCallId":f"tool-{request_id}","title":title,"kind":"execute"},
                "options":[{"optionId":"allow","name":"Allow","kind":"allow_once"}]
            }})
    elif method == "session/cancel" and prompt_id is not None:
        send({"jsonrpc":"2.0","id":prompt_id,"result":{"stopReason":"cancelled"}})
        prompt_id = None
"#;

fn provider(script: &'static str) -> Provider {
    Provider {
        id: ProviderId::Grok,
        display_name: "Activity mock".into(),
        launch: LaunchSpec::new("python3", ["-c", script]),
        needs_node: false,
    }
}

fn prompt(session: &str, request_id: &str) -> Op {
    Op::Prompt {
        session: session.into(),
        doc: vec![DocBlock::Text {
            text: "exercise lifecycle".into(),
        }],
        request_id: Some(request_id.into()),
    }
}

async fn next_event(rx: &mut tokio::sync::mpsc::UnboundedReceiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("event before timeout")
        .expect("event stream remains open")
}

async fn create_session(
    engine: &Engine,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Event>,
) -> String {
    engine
        .submit(Op::NewSession {
            provider: ProviderId::Grok,
            cwd: std::env::temp_dir().to_string_lossy().into_owned(),
            use_worktree: false,
            worktree_base: None,
            worktree_base_sha: None,
            request_id: Some("create-activity-test".into()),
            model: None,
            initial_policy: None,
        })
        .await
        .unwrap();
    loop {
        if let Event::SessionCreated { session, .. } = next_event(rx).await {
            return session;
        }
    }
}

fn activity(engine: &Engine, session: &str) -> SessionActivity {
    engine
        .list_sessions()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == session)
        .expect("session remains listed")
        .activity
}

#[tokio::test]
async fn concurrent_permissions_are_revisioned_and_strictly_routed() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (engine, mut rx) = Engine::with_store(
        vec![provider(TWO_PERMISSION_AGENT)],
        SkillLibrary::new(vec![]),
        store.clone(),
    );
    let session = create_session(&engine, &mut rx).await;
    engine
        .submit(prompt(&session, "permission-prompt"))
        .await
        .unwrap();

    let mut activities = Vec::new();
    let mut request_ids = Vec::new();
    while request_ids.len() < 2 {
        match next_event(&mut rx).await {
            Event::SessionActivityChanged {
                session: routed,
                activity,
            } => {
                assert_eq!(routed, session);
                activities.push(activity);
            }
            Event::PermissionRequest {
                session: routed,
                request_id,
                ..
            } => {
                assert_eq!(routed, session);
                request_ids.push(request_id);
            }
            Event::Error { message, .. } => panic!("unexpected error: {message}"),
            _ => {}
        }
    }

    let awaiting = activity(&engine, &session);
    assert_eq!(awaiting.revision, 3);
    let SessionRunState::AwaitingInput { pending, .. } = &awaiting.state else {
        panic!("two parked permissions must project AwaitingInput");
    };
    assert_eq!(pending.len(), 2);
    assert!(pending[0].sequence < pending[1].sequence);
    assert!(pending
        .iter()
        .all(|input| request_ids.contains(&input.input_id)));
    assert_eq!(
        store.get_session(&session).unwrap().unwrap().activity,
        awaiting
    );

    // Wrong session, invalid option, and a duplicate answer are all strict no-ops.
    engine
        .submit(Op::AnswerPermission {
            session: "some-other-session".into(),
            request_id: pending[0].input_id.clone(),
            option_id: Some("allow".into()),
        })
        .await
        .unwrap();
    engine
        .submit(Op::AnswerPermission {
            session: session.clone(),
            request_id: pending[0].input_id.clone(),
            option_id: Some("not-advertised".into()),
        })
        .await
        .unwrap();
    assert_eq!(activity(&engine, &session), awaiting);

    let first_id = pending[0].input_id.clone();
    engine
        .submit(Op::AnswerPermission {
            session: session.clone(),
            request_id: first_id.clone(),
            option_id: Some("allow".into()),
        })
        .await
        .unwrap();
    let after_first = activity(&engine, &session);
    assert_eq!(after_first.revision, 4);
    let SessionRunState::AwaitingInput { pending, .. } = &after_first.state else {
        panic!("one unresolved permission must keep AwaitingInput");
    };
    assert_eq!(pending.len(), 1);
    let second_id = pending[0].input_id.clone();

    engine
        .submit(Op::AnswerPermission {
            session: session.clone(),
            request_id: first_id,
            option_id: Some("allow".into()),
        })
        .await
        .unwrap();
    assert_eq!(activity(&engine, &session), after_first);

    engine
        .submit(Op::AnswerPermission {
            session: session.clone(),
            request_id: second_id,
            option_id: Some("reject".into()),
        })
        .await
        .unwrap();

    loop {
        match next_event(&mut rx).await {
            Event::SessionActivityChanged {
                session: routed,
                activity,
            } => {
                assert_eq!(routed, session);
                activities.push(activity);
            }
            Event::TurnEnded {
                session: routed, ..
            } => {
                assert_eq!(routed, session);
                break;
            }
            Event::Error { message, .. } => panic!("unexpected error: {message}"),
            _ => {}
        }
    }

    assert_eq!(
        activities
            .iter()
            .map(|activity| activity.revision)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6]
    );
    assert!(matches!(
        activities[0].state,
        SessionRunState::Running { .. }
    ));
    assert!(matches!(
        activities[1].state,
        SessionRunState::AwaitingInput { ref pending, .. } if pending.len() == 1
    ));
    assert!(matches!(
        activities[2].state,
        SessionRunState::AwaitingInput { ref pending, .. } if pending.len() == 2
    ));
    assert!(matches!(
        activities[3].state,
        SessionRunState::AwaitingInput { ref pending, .. } if pending.len() == 1
    ));
    assert!(matches!(
        activities[4].state,
        SessionRunState::Running { .. }
    ));
    assert!(matches!(activities[5].state, SessionRunState::Idle));
    assert_eq!(
        store.get_session(&session).unwrap().unwrap().activity,
        activities[5]
    );
}

#[tokio::test]
async fn model_switch_is_rejected_while_a_turn_owns_the_session() {
    let (engine, mut rx) = Engine::new(
        vec![provider(TWO_PERMISSION_AGENT)],
        SkillLibrary::new(vec![]),
    );
    let session = create_session(&engine, &mut rx).await;
    engine
        .submit(prompt(&session, "model-switch-running"))
        .await
        .unwrap();

    loop {
        match next_event(&mut rx).await {
            Event::PermissionRequest {
                session: routed, ..
            } => {
                assert_eq!(routed, session);
                break;
            }
            Event::Error { message, .. } => panic!("unexpected prompt error: {message}"),
            _ => {}
        }
    }

    engine
        .submit(Op::SetModel {
            session: session.clone(),
            model: "deep".into(),
        })
        .await
        .unwrap();

    loop {
        match next_event(&mut rx).await {
            Event::Error {
                session: routed,
                message,
                terminal,
                ..
            } => {
                assert_eq!(routed.as_deref(), Some(session.as_str()));
                assert!(!terminal);
                assert!(
                    message.contains("while a turn is running"),
                    "unexpected error: {message}"
                );
                break;
            }
            Event::Models { .. } => {
                panic!("a running session must not send session/set_model to the provider")
            }
            _ => {}
        }
    }

    engine
        .submit(Op::SetConfigOption {
            session: session.clone(),
            config_id: "model".into(),
            value: "deep".into(),
        })
        .await
        .unwrap();

    loop {
        match next_event(&mut rx).await {
            Event::Error {
                session: routed,
                message,
                terminal,
                ..
            } => {
                assert_eq!(routed.as_deref(), Some(session.as_str()));
                assert!(!terminal);
                assert!(
                    message.contains("while a turn is running"),
                    "unexpected error: {message}"
                );
                break;
            }
            Event::ConfigOptions { .. } => {
                panic!("a running session must not send a model config change to the provider")
            }
            _ => {}
        }
    }
}

#[tokio::test]
async fn retired_planning_config_is_rejected_while_supported_choices_still_work() {
    const AGENT: &str = r#"
import json, sys
for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    if method == "initialize":
        result = {"protocolVersion":1}
    elif method == "session/new":
        result = {"sessionId":"config-session", "configOptions":[
            {"id":"collaboration_mode","name":"Mode","type":"select",
             "currentValue":"default","options":[{"value":"plan","name":"Plan"}]},
            {"id":"effort","name":"Effort","type":"select",
             "currentValue":"low","options":[{"value":"high","name":"High"}]}
        ]}
    elif method == "session/prompt":
        result = {"stopReason":"end_turn"}
    elif method == "session/set_config_option":
        result = {"configOptions":[]}
    else:
        continue
    print(json.dumps({"jsonrpc":"2.0","id":message["id"],"result":result}), flush=True)
"#;
    let (engine, mut rx) = Engine::new(vec![provider(AGENT)], SkillLibrary::new(vec![]));
    let session = create_session(&engine, &mut rx).await;
    engine
        .submit(prompt(&session, "config-test"))
        .await
        .unwrap();
    let mut saw_supported_config = false;
    loop {
        match next_event(&mut rx).await {
            Event::ConfigOptions { options, .. } => {
                assert!(options
                    .iter()
                    .all(|option| option.id != "collaboration_mode"));
                saw_supported_config |= options.iter().any(|option| option.id == "effort");
            }
            Event::TurnEnded { .. } => break,
            Event::Error { message, .. } => panic!("unexpected prompt error: {message}"),
            _ => {}
        }
    }
    assert!(saw_supported_config);

    engine
        .submit(Op::SetConfigOption {
            session: session.clone(),
            config_id: "collaboration_mode".into(),
            value: "plan".into(),
        })
        .await
        .unwrap();
    loop {
        match next_event(&mut rx).await {
            Event::Error {
                message, terminal, ..
            } => {
                assert!(!terminal);
                assert!(
                    message.contains("unsupported session config choice"),
                    "{message}"
                );
                break;
            }
            Event::ConfigOptions { .. } => panic!("retired config reached the provider"),
            _ => {}
        }
    }

    engine
        .submit(Op::SetConfigOption {
            session,
            config_id: "effort".into(),
            value: "high".into(),
        })
        .await
        .unwrap();
    loop {
        match next_event(&mut rx).await {
            Event::ConfigOptions { .. } => break,
            Event::Error { message, .. } => panic!("supported config rejected: {message}"),
            _ => {}
        }
    }
}

#[tokio::test]
async fn cancel_drains_permissions_before_acp_finishes_the_turn() {
    let (engine, mut rx) = Engine::new(vec![provider(CANCEL_AGENT)], SkillLibrary::new(vec![]));
    let session = create_session(&engine, &mut rx).await;
    engine
        .submit(prompt(&session, "cancel-prompt"))
        .await
        .unwrap();

    let mut activities = Vec::new();
    let mut requests = 0;
    while requests < 2 {
        match next_event(&mut rx).await {
            Event::SessionActivityChanged { activity, .. } => activities.push(activity),
            Event::PermissionRequest { .. } => requests += 1,
            Event::Error { message, .. } => panic!("unexpected error: {message}"),
            _ => {}
        }
    }
    assert!(matches!(
        activity(&engine, &session).state,
        SessionRunState::AwaitingInput { ref pending, .. } if pending.len() == 2
    ));

    engine
        .submit(Op::Cancel {
            session: session.clone(),
        })
        .await
        .unwrap();
    loop {
        match next_event(&mut rx).await {
            Event::SessionActivityChanged { activity, .. } => activities.push(activity),
            Event::TurnEnded {
                session: routed,
                stop_reason,
            } => {
                assert_eq!(routed, session);
                assert_eq!(stop_reason, "Cancelled");
                break;
            }
            Event::Error { message, .. } => panic!("unexpected error: {message}"),
            _ => {}
        }
    }

    assert_eq!(
        activities
            .iter()
            .map(|activity| activity.revision)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
    assert!(matches!(
        activities[3].state,
        SessionRunState::Running { .. }
    ));
    assert!(matches!(activities[4].state, SessionRunState::Idle));
}

#[tokio::test]
async fn yolo_answers_automatically_without_awaiting_input_and_persists_the_mode() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (engine, mut rx) = Engine::with_store(
        vec![provider(TWO_PERMISSION_AGENT)],
        SkillLibrary::new(vec![]),
        store.clone(),
    );
    let session = create_session(&engine, &mut rx).await;
    engine
        .submit(Op::SetPermissionMode {
            session: session.clone(),
            mode: PermissionMode::Yolo,
        })
        .await
        .unwrap();
    assert_eq!(
        store
            .get_session(&session)
            .unwrap()
            .unwrap()
            .permission_mode,
        PermissionMode::Yolo
    );
    assert_eq!(
        engine
            .list_sessions()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == session)
            .unwrap()
            .permission_mode,
        PermissionMode::Yolo
    );

    engine
        .submit(prompt(&session, "yolo-prompt"))
        .await
        .unwrap();
    let mut activities = Vec::new();
    loop {
        match next_event(&mut rx).await {
            Event::SessionActivityChanged { activity, .. } => activities.push(activity),
            Event::PermissionRequest { .. } => panic!("YOLO must not park permission input"),
            Event::TurnEnded { .. } => break,
            Event::Error { message, .. } => panic!("unexpected error: {message}"),
            _ => {}
        }
    }
    assert_eq!(
        activities
            .iter()
            .map(|activity| activity.revision)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(matches!(
        activities[0].state,
        SessionRunState::Running { .. }
    ));
    assert!(matches!(activities[1].state, SessionRunState::Idle));
    assert_eq!(
        store.get_session(&session).unwrap().unwrap().activity,
        activities[1]
    );
}

#[tokio::test]
async fn initial_execution_policy_governs_the_first_turn_and_is_persisted() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (engine, mut rx) = Engine::with_store(
        vec![provider(TWO_PERMISSION_AGENT)],
        SkillLibrary::new(vec![]),
        store.clone(),
    );
    engine
        .submit(Op::NewSession {
            provider: ProviderId::Grok,
            cwd: std::env::temp_dir().to_string_lossy().into_owned(),
            use_worktree: false,
            worktree_base: None,
            worktree_base_sha: None,
            request_id: Some("create-read-only".into()),
            model: None,
            initial_policy: Some(ExecutionPolicy {
                mode: PermissionMode::Ask,
                sandbox: SandboxPolicy::ReadOnly,
            }),
        })
        .await
        .unwrap();
    let session = loop {
        if let Event::SessionCreated { session, .. } = next_event(&mut rx).await {
            break session;
        }
    };

    let persisted = store.get_session(&session).unwrap().unwrap();
    assert_eq!(persisted.permission_mode, PermissionMode::Ask);
    assert_eq!(persisted.sandbox_policy, SandboxPolicy::ReadOnly);
    let projected = engine
        .list_sessions()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == session)
        .unwrap();
    assert_eq!(projected.permission_mode, PermissionMode::Ask);
    assert_eq!(projected.sandbox_policy, SandboxPolicy::ReadOnly);

    engine
        .submit(prompt(&session, "first-read-only-prompt"))
        .await
        .unwrap();
    loop {
        match next_event(&mut rx).await {
            Event::PermissionRequest { .. } => {
                panic!("the initial read-only ceiling must veto first-turn execute permission")
            }
            Event::TurnEnded {
                session: routed, ..
            } => {
                assert_eq!(routed, session);
                break;
            }
            Event::Error { message, .. } => panic!("unexpected error: {message}"),
            _ => {}
        }
    }
}

#[tokio::test]
async fn combined_execution_policy_survives_engine_restart() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (engine, mut rx) = Engine::with_store(
        vec![provider(TWO_PERMISSION_AGENT)],
        SkillLibrary::new(vec![]),
        store.clone(),
    );
    let session = create_session(&engine, &mut rx).await;
    engine
        .submit(Op::SetExecutionPolicy {
            session: session.clone(),
            mode: PermissionMode::Ask,
            sandbox: SandboxPolicy::ReadOnly,
            request_id: Some("policy-before-restart".into()),
        })
        .await
        .unwrap();
    loop {
        match next_event(&mut rx).await {
            Event::ExecutionPolicyChanged {
                session: routed,
                policy,
                request_id,
            } => {
                assert_eq!(routed, session);
                assert_eq!(policy.mode, PermissionMode::Ask);
                assert_eq!(policy.sandbox, SandboxPolicy::ReadOnly);
                assert_eq!(request_id.as_deref(), Some("policy-before-restart"));
                break;
            }
            Event::Error { message, .. } => panic!("unexpected error: {message}"),
            _ => {}
        }
    }
    let persisted = store.get_session(&session).unwrap().unwrap();
    assert_eq!(persisted.permission_mode, PermissionMode::Ask);
    assert_eq!(persisted.sandbox_policy, SandboxPolicy::ReadOnly);
    drop(engine);
    drop(rx);

    let (restarted, mut restarted_rx) = Engine::with_store(
        vec![provider(TWO_PERMISSION_AGENT)],
        SkillLibrary::new(vec![]),
        store.clone(),
    );
    restarted
        .submit(prompt(&session, "restart-read-only-prompt"))
        .await
        .unwrap();
    loop {
        match next_event(&mut restarted_rx).await {
            Event::PermissionRequest { .. } => {
                panic!("revival must restore the persisted read-only permission ceiling")
            }
            Event::TurnEnded {
                session: routed, ..
            } => {
                assert_eq!(routed, session);
                break;
            }
            Event::Error { message, .. } => panic!("unexpected error: {message}"),
            _ => {}
        }
    }
}

#[tokio::test]
async fn execution_policy_failure_is_correlated_and_keeps_the_authoritative_pair() {
    let db_path = std::env::temp_dir().join(format!(
        "codetwo-policy-failure-{}.db",
        uuid::Uuid::new_v4()
    ));
    let db_path_string = db_path.to_string_lossy().into_owned();
    let store = Arc::new(Store::open(&db_path_string).unwrap());
    let (engine, mut rx) = Engine::with_store(
        vec![provider(TWO_PERMISSION_AGENT)],
        SkillLibrary::new(vec![]),
        store.clone(),
    );
    let session = create_session(&engine, &mut rx).await;
    let before = store.get_session(&session).unwrap().unwrap();

    let trigger_connection = rusqlite::Connection::open(&db_path).unwrap();
    trigger_connection
        .execute_batch(
            "CREATE TRIGGER reject_engine_policy BEFORE UPDATE OF permission_mode, sandbox_policy ON sessions
             BEGIN SELECT RAISE(ABORT, 'simulated engine policy failure'); END;",
        )
        .unwrap();

    engine
        .submit(Op::SetExecutionPolicy {
            session: session.clone(),
            mode: PermissionMode::Yolo,
            sandbox: SandboxPolicy::DangerFullAccess,
            request_id: Some("policy-that-fails".into()),
        })
        .await
        .unwrap();

    loop {
        match next_event(&mut rx).await {
            Event::Error {
                session: routed,
                message,
                terminal,
                request_id,
            } => {
                assert_eq!(routed.as_deref(), Some(session.as_str()));
                assert!(message.contains("simulated engine policy failure"));
                assert!(!terminal);
                assert_eq!(request_id.as_deref(), Some("policy-that-fails"));
                break;
            }
            Event::ExecutionPolicyChanged { .. } => {
                panic!("a failed durable write must not publish a success receipt")
            }
            _ => {}
        }
    }

    let persisted = store.get_session(&session).unwrap().unwrap();
    assert_eq!(persisted.permission_mode, before.permission_mode);
    assert_eq!(persisted.sandbox_policy, before.sandbox_policy);
    let projected = engine
        .list_sessions()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == session)
        .unwrap();
    assert_eq!(projected.permission_mode, before.permission_mode);
    assert_eq!(projected.sandbox_policy, before.sandbox_policy);

    drop(trigger_connection);
    drop(engine);
    drop(rx);
    drop(store);
    std::fs::remove_file(&db_path).unwrap();
}

#[tokio::test]
async fn missing_session_policy_update_is_a_correlated_rejection() {
    let (engine, mut rx) = Engine::new(Vec::new(), SkillLibrary::new(vec![]));
    engine
        .submit(Op::SetExecutionPolicy {
            session: "missing-session".into(),
            mode: PermissionMode::Ask,
            sandbox: SandboxPolicy::ReadOnly,
            request_id: Some("missing-policy".into()),
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
            assert_eq!(session.as_deref(), Some("missing-session"));
            assert!(message.contains("no such session"));
            assert!(!terminal);
            assert_eq!(request_id.as_deref(), Some("missing-policy"));
        }
        event => panic!("expected correlated missing-session error, got {event:?}"),
    }
}

#[test]
fn engine_startup_normalizes_an_unrecoverable_running_snapshot() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let mut session = Session::new(ProviderId::Grok, "/work");
    session.activity = SessionActivity {
        revision: 11,
        state: SessionRunState::Running {
            turn_id: "orphaned-turn".into(),
            prompt_request_id: Some("orphaned-prompt".into()),
        },
    };
    store.upsert_session(&session).unwrap();

    let (engine, _rx) = Engine::with_store(Vec::new(), SkillLibrary::new(vec![]), store.clone());
    let normalized = engine.list_sessions().unwrap().pop().unwrap().activity;
    assert_eq!(normalized.revision, 12);
    assert!(matches!(
        normalized.state,
        SessionRunState::Failed {
            turn_id: Some(ref turn_id),
            reason: RunFailureReason::Interrupted,
            ref message,
        } if turn_id == "orphaned-turn"
            && message == "C2 stopped before the turn finished"
    ));
    assert_eq!(
        store.get_session(&session.id).unwrap().unwrap().activity,
        normalized
    );
}

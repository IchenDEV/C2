//! Mid-conversation provider replacement keeps one durable C2 conversation while treating ACP
//! runtime state as provider-owned. These tests exercise the real Engine and stdio transport.

use std::sync::Arc;
use std::time::Duration;

use codetwo_core::event::Event;
use codetwo_core::permission::{ExecutionPolicy, PermissionMode, SandboxPolicy};
use codetwo_core::provider::{default_registry, LaunchSpec, Provider, ProviderId};
use codetwo_core::skill::{DocBlock, SkillLibrary};
use codetwo_core::{
    AgentId, AgentRole, Engine, Op, ProviderConfiguration, ResultContract, Session, Store,
    StoreError, Task, TaskBudget, TaskId, TaskStatus,
};

const SOURCE_AGENT: &str = r#"
import json, sys

def send(message): print(json.dumps(message), flush=True)
for line in sys.stdin:
    line = line.strip()
    if not line: continue
    message = json.loads(line)
    method, mid = message.get("method"), message.get("id")
    if method == "initialize":
        send({"jsonrpc":"2.0","id":mid,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":True}}})
    elif method == "session/new":
        send({"jsonrpc":"2.0","id":mid,"result":{"sessionId":"source-session"}})
    elif method == "session/prompt":
        sid = message["params"]["sessionId"]
        send({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":sid,
              "update":{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"PRIVATE_REASONING_SECRET"}}}})
        send({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":sid,
              "update":{"sessionUpdate":"tool_call","toolCallId":"tool-1","title":"Compile workspace",
                        "kind":"execute","status":"completed",
                        "rawInput":{"secret":"PRIVATE_INPUT_SECRET"},
                        "rawOutput":{"content":[{"type":"text","text":"PRIVATE_TOOL_OUTPUT_SECRET"}]}}}})
        send({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":sid,
              "update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"from-provider-a"}}}})
        send({"jsonrpc":"2.0","id":mid,"result":{"stopReason":"end_turn"}})
"#;

const TARGET_AGENT: &str = r#"
import json, sys

def send(message): print(json.dumps(message), flush=True)
used_old_cursor = False
for line in sys.stdin:
    line = line.strip()
    if not line: continue
    message = json.loads(line)
    method, mid = message.get("method"), message.get("id")
    if method == "initialize":
        send({"jsonrpc":"2.0","id":mid,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":True,"sessionCapabilities":{"resume":{}}}}})
    elif method in ("session/load", "session/resume"):
        used_old_cursor = True
        send({"jsonrpc":"2.0","id":mid,"error":{"code":-32000,"message":"old cursor must not cross providers"}})
    elif method == "session/new":
        send({"jsonrpc":"2.0","id":mid,"result":{"sessionId":"target-session","models":{"currentModelId":"account-model","availableModels":[{"modelId":"account-model","name":"Account model"}]}}})
    elif method == "session/prompt":
        prompt = json.dumps(message["params"].get("prompt", []))
        required = ["first user request", "from-provider-a", "Compile workspace: completed", "second user request"]
        forbidden = ["PRIVATE_REASONING_SECRET", "PRIVATE_INPUT_SECRET", "PRIVATE_TOOL_OUTPUT_SECRET"]
        ordered = prompt.find("first user request") < prompt.find("second user request")
        ok = all(item in prompt for item in required) and all(item not in prompt for item in forbidden) and not used_old_cursor and ordered
        text = "CONTINUATION_OK" if ok else "CONTINUATION_BAD:" + prompt
        send({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":message["params"]["sessionId"],
              "update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":text}}}})
        send({"jsonrpc":"2.0","id":mid,"result":{"stopReason":"end_turn"}})
"#;

const BLOCKING_AGENT: &str = r#"
import json, sys

def send(message): print(json.dumps(message), flush=True)
for line in sys.stdin:
    line = line.strip()
    if not line: continue
    message = json.loads(line)
    method, mid = message.get("method"), message.get("id")
    if method == "initialize":
        send({"jsonrpc":"2.0","id":mid,"result":{"protocolVersion":1}})
    elif method == "session/new":
        send({"jsonrpc":"2.0","id":mid,"result":{"sessionId":"blocking-session"}})
    elif method == "session/prompt":
        send({"jsonrpc":"2.0","id":9000,"method":"session/request_permission","params":{
              "sessionId":"blocking-session",
              "toolCall":{"toolCallId":"tool-wait","title":"Wait for approval","kind":"execute"},
              "options":[{"optionId":"allow","name":"Allow","kind":"allow_once"}]}})
"#;

const SLOW_TARGET_AGENT: &str = r#"
import json, sys, time

def send(message): print(json.dumps(message), flush=True)
for line in sys.stdin:
    line = line.strip()
    if not line: continue
    message = json.loads(line)
    method, mid = message.get("method"), message.get("id")
    if method == "initialize":
        time.sleep(0.2)
        send({"jsonrpc":"2.0","id":mid,"result":{"protocolVersion":1}})
    elif method == "session/new":
        send({"jsonrpc":"2.0","id":mid,"result":{"sessionId":"slow-target"}})
"#;

fn provider(id: ProviderId, name: &str, script: &'static str) -> Provider {
    Provider {
        id,
        display_name: name.into(),
        launch: LaunchSpec::new("python3", ["-c", script]),
        needs_node: false,
    }
}

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

async fn create_session(
    engine: &Engine,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Event>,
    provider: ProviderId,
) -> String {
    engine
        .submit(Op::NewSession {
            provider,
            cwd: std::env::temp_dir().to_string_lossy().into_owned(),
            use_worktree: false,
            worktree_base: None,
            worktree_base_sha: None,
            request_id: Some("provider-switch-create".into()),
            model: None,
            initial_policy: Some(ExecutionPolicy {
                mode: PermissionMode::Ask,
                sandbox: SandboxPolicy::WorkspaceWrite,
            }),
        })
        .await
        .unwrap();
    loop {
        match next_event(rx).await {
            Event::SessionCreated { session, .. } => return session,
            Event::Error { message, .. } => panic!("session creation failed: {message}"),
            _ => {}
        }
    }
}

async fn run_turn(
    engine: &Engine,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Event>,
    session: &str,
    text: &str,
    request_id: &str,
) -> Vec<String> {
    engine
        .submit(prompt(session, text, request_id))
        .await
        .unwrap();
    let mut replies = Vec::new();
    loop {
        match next_event(rx).await {
            Event::AgentText {
                session: routed,
                text,
                ..
            } if routed == session => replies.push(text),
            Event::TurnEnded {
                session: routed, ..
            } if routed == session => return replies,
            Event::Error {
                session: routed,
                message,
                terminal: true,
                ..
            } if routed.as_deref() == Some(session) => panic!("turn failed: {message}"),
            _ => {}
        }
    }
}

#[tokio::test]
async fn switch_keeps_the_conversation_and_sends_only_provider_neutral_history() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let providers = vec![
        provider(ProviderId::Grok, "Source mock", SOURCE_AGENT),
        provider(ProviderId::Pi, "Target mock", TARGET_AGENT),
    ];
    let (engine, mut rx) = Engine::with_store(providers, SkillLibrary::new(vec![]), store.clone());
    let session = create_session(&engine, &mut rx, ProviderId::Grok).await;
    assert_eq!(
        run_turn(&engine, &mut rx, &session, "first user request", "turn-a").await,
        vec!["from-provider-a"]
    );
    let before = store.get_session(&session).unwrap().unwrap();
    assert!(store
        .transcript(&session)
        .unwrap()
        .iter()
        .any(|(_, part)| matches!(part, codetwo_core::session::Part::Reasoning { text } if text == "PRIVATE_REASONING_SECRET")));

    let switched = engine
        .switch_provider(&session, ProviderId::Pi, None)
        .await
        .unwrap();

    assert_eq!(switched.id, session);
    assert_eq!(switched.provider, ProviderId::Pi);
    assert_eq!(switched.cwd, before.cwd);
    assert_eq!(switched.permission_mode, before.permission_mode);
    assert_eq!(switched.sandbox_policy, before.sandbox_policy);
    assert_eq!(switched.created_at, before.created_at);
    assert_eq!(switched.last_active_at, before.last_active_at);
    assert_eq!(switched.acp_session_id, None);
    let durable = store.get_session(&session).unwrap().unwrap();
    assert_eq!(durable.provider, ProviderId::Pi);
    assert_eq!(durable.created_at, before.created_at);
    assert_eq!(durable.last_active_at, before.last_active_at);
    assert!(store.handoff_context(&session).unwrap().is_some());

    let replies = run_turn(&engine, &mut rx, &session, "second user request", "turn-b").await;
    assert_eq!(replies, vec!["CONTINUATION_OK"]);
    let catalogue = engine.provider_catalog();
    let (_, models) = catalogue
        .iter()
        .find(|(provider, _)| provider.id == ProviderId::Pi)
        .unwrap();
    assert_eq!(
        models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        ["account-model"]
    );
    assert!(store.handoff_context(&session).unwrap().is_none());
    assert_eq!(store.transcript(&session).unwrap().len(), 6);
    engine.shutdown();
}

#[tokio::test]
async fn failed_target_startup_keeps_the_old_provider_usable() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let providers = vec![
        provider(ProviderId::Grok, "Source mock", SOURCE_AGENT),
        Provider {
            id: ProviderId::Pi,
            display_name: "Missing target".into(),
            launch: LaunchSpec::new(
                "/definitely/missing/codetwo-provider",
                std::iter::empty::<&str>(),
            ),
            needs_node: false,
        },
    ];
    let (engine, mut rx) = Engine::with_store(providers, SkillLibrary::new(vec![]), store.clone());
    let session = create_session(&engine, &mut rx, ProviderId::Grok).await;

    let error = engine
        .switch_provider(&session, ProviderId::Pi, None)
        .await
        .unwrap_err();
    assert!(error.contains("couldn't start Missing target"), "{error}");
    assert_eq!(
        store.get_session(&session).unwrap().unwrap().provider,
        ProviderId::Grok
    );
    assert_eq!(
        run_turn(&engine, &mut rx, &session, "still usable", "rollback-turn").await,
        vec!["from-provider-a"]
    );
    engine.shutdown();
}

#[tokio::test]
async fn running_or_awaiting_input_sessions_reject_a_switch_without_mutation() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let providers = vec![
        provider(ProviderId::Grok, "Blocking mock", BLOCKING_AGENT),
        provider(ProviderId::Pi, "Target mock", TARGET_AGENT),
    ];
    let (engine, mut rx) = Engine::with_store(providers, SkillLibrary::new(vec![]), store.clone());
    let session = create_session(&engine, &mut rx, ProviderId::Grok).await;
    engine
        .submit(prompt(&session, "block here", "busy-turn"))
        .await
        .unwrap();
    loop {
        if matches!(next_event(&mut rx).await, Event::PermissionRequest { .. }) {
            break;
        }
    }

    let error = engine
        .switch_provider(&session, ProviderId::Pi, None)
        .await
        .unwrap_err();
    assert!(
        error.contains("turn is running or awaiting input"),
        "{error}"
    );
    assert_eq!(
        store.get_session(&session).unwrap().unwrap().provider,
        ProviderId::Grok
    );
    engine.shutdown();
}

#[tokio::test]
async fn concurrent_switches_have_one_owner_and_one_durable_result() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let providers = vec![
        provider(ProviderId::Grok, "Source mock", SOURCE_AGENT),
        provider(ProviderId::Pi, "Slow target", SLOW_TARGET_AGENT),
    ];
    let (engine, mut rx) = Engine::with_store(providers, SkillLibrary::new(vec![]), store.clone());
    let session = create_session(&engine, &mut rx, ProviderId::Grok).await;
    let credentials = engine.host_mcp_state().registry();
    let (_, old_bearer) = credentials
        .lock()
        .unwrap()
        .issue(codetwo_core::HostMcpScope {
            session_id: session.clone(),
            provider_id: "grok".into(),
            capabilities: codetwo_core::HostMcpCapability::default_read_only_set(),
        })
        .unwrap();
    assert!(credentials.lock().unwrap().resolve(&old_bearer).is_some());

    let (first, second) = tokio::join!(
        engine.switch_provider(&session, ProviderId::Pi, None),
        engine.switch_provider(&session, ProviderId::Pi, None)
    );
    let results = [first, second];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    assert!(results
        .iter()
        .filter_map(|result| result.as_ref().err())
        .any(|error| error.contains("already switching")));
    assert_eq!(
        store.get_session(&session).unwrap().unwrap().provider,
        ProviderId::Pi
    );
    assert!(credentials.lock().unwrap().resolve(&old_bearer).is_none());
    engine.shutdown();
}

#[test]
fn managed_task_session_lease_blocks_provider_identity_changes() {
    let store = Store::open_in_memory().unwrap();
    let task = Task {
        id: TaskId::new("provider-switch-task"),
        status: TaskStatus::Active,
        result_contract: ResultContract {
            goal: "Keep the managed runtime compatible".into(),
            required_deliverables: Vec::new(),
            completion_conditions: Vec::new(),
            boundaries: Vec::new(),
            known_risks: Vec::new(),
            unresolved_facts: Vec::new(),
        },
        provider_configuration: ProviderConfiguration {
            provider: ProviderId::Grok,
            model: None,
            reasoning_effort: None,
        },
        budget: TaskBudget {
            max_cost_microusd: None,
            max_tokens: None,
            max_duration_seconds: None,
        },
    };
    store.create_task(&task, 100).unwrap();
    let session = Session::new(ProviderId::Grok, "/work/project");
    store.upsert_session(&session).unwrap();
    store
        .lease_task_session(
            &task.id,
            &session.id,
            &AgentId::new("executor-1"),
            AgentRole::Executor,
            "grok-compatible",
            200,
        )
        .unwrap();

    let result = store.switch_session_provider(
        &session.id,
        &ProviderId::Grok,
        &ProviderId::Pi,
        None,
        &serde_json::json!({"kind": "provider_switch"}),
    );

    assert!(matches!(
        result,
        Err(StoreError::SessionLeaseConflict { .. })
    ));
    assert_eq!(
        store.get_session(&session.id).unwrap().unwrap().provider,
        ProviderId::Grok
    );
}

#[tokio::test]
async fn repeated_and_unprompted_switches_use_canonical_history() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let providers = vec![
        provider(ProviderId::Grok, "Source mock", SOURCE_AGENT),
        provider(ProviderId::Pi, "Target mock", TARGET_AGENT),
    ];
    let (engine, mut rx) = Engine::with_store(providers, SkillLibrary::new(vec![]), store.clone());
    let session = create_session(&engine, &mut rx, ProviderId::Grok).await;
    run_turn(&engine, &mut rx, &session, "first user request", "first").await;
    for round in 0..4 {
        engine
            .switch_provider(&session, ProviderId::Pi, None)
            .await
            .unwrap();
        // Switch back before the target has consumed the pending continuation.
        engine
            .switch_provider(&session, ProviderId::Grok, None)
            .await
            .unwrap();
        engine
            .switch_provider(&session, ProviderId::Pi, None)
            .await
            .unwrap();
        let context = store.handoff_context(&session).unwrap().unwrap();
        assert_eq!(context["kind"], "provider_switch");
        assert!(!context["history"].to_string().contains("sourceProvider"));
        assert_eq!(
            run_turn(
                &engine,
                &mut rx,
                &session,
                "second user request",
                &format!("round-{round}")
            )
            .await,
            vec!["CONTINUATION_OK"]
        );
        engine
            .switch_provider(&session, ProviderId::Grok, None)
            .await
            .unwrap();
    }
    engine.shutdown();
}

#[tokio::test]
async fn cancelled_first_turn_retains_context_through_restart_and_native_restore() {
    const CANCEL_THEN_RESTORE: &str = r#"
import json, sys
restored = False
for line in sys.stdin:
    m = json.loads(line)
    method, mid = m.get("method"), m.get("id")
    result = {}
    if method == "initialize":
        result = {"protocolVersion":1,"agentCapabilities":{"loadSession":True}}
    elif method == "session/new":
        result = {"sessionId":"retry-session"}
    elif method == "session/load":
        restored = True
    elif method == "session/prompt":
        if not restored:
            result = {"stopReason":"cancelled"}
        else:
            prompt = json.dumps(m["params"]["prompt"])
            ok = "first user request" in prompt and "from-provider-a" in prompt
            print(json.dumps({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"retry-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"RESTORED_OK" if ok else "LOST_CONTEXT"}}}}), flush=True)
            result = {"stopReason":"end_turn"}
    if mid is not None:
        print(json.dumps({"jsonrpc":"2.0","id":mid,"result":result}), flush=True)
"#;
    let store = Arc::new(Store::open_in_memory().unwrap());
    let providers = vec![
        provider(ProviderId::Grok, "Source", SOURCE_AGENT),
        provider(ProviderId::Pi, "Retry target", CANCEL_THEN_RESTORE),
    ];
    let (engine, mut rx) =
        Engine::with_store(providers.clone(), SkillLibrary::new(vec![]), store.clone());
    let session = create_session(&engine, &mut rx, ProviderId::Grok).await;
    run_turn(&engine, &mut rx, &session, "first user request", "first").await;
    engine
        .switch_provider(&session, ProviderId::Pi, None)
        .await
        .unwrap();
    run_turn(&engine, &mut rx, &session, "cancelled request", "cancel").await;
    assert!(store.handoff_context(&session).unwrap().is_some());
    engine.shutdown();
    let (revived, mut rx) = Engine::with_store(providers, SkillLibrary::new(vec![]), store.clone());
    assert_eq!(
        run_turn(&revived, &mut rx, &session, "retry", "retry").await,
        vec!["RESTORED_OK"]
    );
    assert!(store.handoff_context(&session).unwrap().is_none());
    revived.shutdown();
}

#[tokio::test]
async fn unavailable_history_refuses_switch_instead_of_silently_losing_context() {
    let (engine, mut rx) = Engine::new(
        vec![
            provider(ProviderId::Grok, "Source", SOURCE_AGENT),
            provider(ProviderId::Pi, "Target", TARGET_AGENT),
        ],
        SkillLibrary::new(vec![]),
    );
    let session = create_session(&engine, &mut rx, ProviderId::Grok).await;
    assert!(engine
        .switch_provider(&session, ProviderId::Pi, None)
        .await
        .unwrap_err()
        .contains("saved conversation history"));
    assert_eq!(
        run_turn(&engine, &mut rx, &session, "still usable", "first").await,
        vec!["from-provider-a"]
    );
    engine.shutdown();
}

#[tokio::test]
async fn failed_first_prompt_keeps_continuation_for_retry() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    // A broken connection is replaced after Unknown. Keep the fail-once fixture
    // state outside its process so a fresh runtime does not fail its first request again.
    let fixture = tempfile::tempdir().unwrap();
    let failed_marker = fixture.path().join("failed-once");
    let marker_literal = serde_json::to_string(&failed_marker.to_string_lossy()).unwrap();
    let mut target = provider(ProviderId::Pi, "Fail once", TARGET_AGENT);
    target.launch.args[1] = TARGET_AGENT
        // Reconnection may restore the target's own cursor; only a source cursor is foreign.
        .replace(
            "used_old_cursor = True",
            "used_old_cursor = message[\"params\"].get(\"sessionId\") != \"target-session\"",
        )
        .replace(
            "used_old_cursor = False",
            &format!("used_old_cursor = False\nimport os\nfailed = os.path.exists({marker_literal})"),
        )
        .replace(
            "        prompt = json.dumps",
            &format!(r#"        if not failed:
            failed = True
            open({marker_literal}, "w").close()
            send({{"jsonrpc":"2.0","id":mid,"error":{{"code":-32000,"message":"temporary failure"}}}})
            continue
        prompt = json.dumps"#),
        );
    let (engine, mut rx) = Engine::with_store(
        vec![provider(ProviderId::Grok, "Source", SOURCE_AGENT), target],
        SkillLibrary::new(vec![]),
        store.clone(),
    );
    let session = create_session(&engine, &mut rx, ProviderId::Grok).await;
    run_turn(&engine, &mut rx, &session, "first user request", "first").await;
    engine
        .switch_provider(&session, ProviderId::Pi, None)
        .await
        .unwrap();
    engine
        .submit(prompt(&session, "second user request", "failure"))
        .await
        .unwrap();
    let mut broken = false;
    loop {
        let event = next_event(&mut rx).await;
        if matches!(&event, Event::ThreadDisposition { session: routed, disposition: codetwo_core::ThreadDisposition::Broken } if routed == &session)
        {
            broken = true;
        }
        if let Event::Error {
            terminal: true,
            message,
            ..
        } = event
        {
            assert!(message.contains("temporary failure"));
            break;
        }
    }
    assert!(broken, "unknown outcome must discard the failed runtime");
    assert!(failed_marker.is_file());
    assert!(store.handoff_context(&session).unwrap().is_some());
    assert_eq!(
        run_turn(&engine, &mut rx, &session, "second user request", "retry").await,
        vec!["CONTINUATION_OK"]
    );
    assert!(store.handoff_context(&session).unwrap().is_none());
    engine.shutdown();
}

struct LiveEngineGuard(Engine);

impl Drop for LiveEngineGuard {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

fn live_provider_model(provider: &ProviderId) -> Option<String> {
    let key = format!(
        "CODETWO_LIVE_SWITCH_{}_MODEL",
        provider.as_str().to_ascii_uppercase()
    );
    std::env::var(key).ok().filter(|value| !value.is_empty())
}

async fn next_live_event(rx: &mut tokio::sync::mpsc::UnboundedReceiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(180), rx.recv())
        .await
        .expect("live provider event before timeout")
        .expect("live provider event stream remains open")
}

async fn create_live_session(
    engine: &Engine,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Event>,
    provider: ProviderId,
) -> String {
    let cwd = std::env::var("CODETWO_LIVE_SWITCH_CWD")
        .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned());
    let model = live_provider_model(&provider);
    engine
        .submit(Op::NewSession {
            provider,
            cwd,
            use_worktree: false,
            worktree_base: None,
            worktree_base_sha: None,
            request_id: Some("live-provider-switch-create".into()),
            model,
            initial_policy: Some(ExecutionPolicy {
                mode: PermissionMode::Ask,
                sandbox: SandboxPolicy::ReadOnly,
            }),
        })
        .await
        .unwrap();
    loop {
        match next_live_event(rx).await {
            Event::SessionCreated { session, .. } => return session,
            Event::Error { message, .. } => panic!("live session creation failed: {message}"),
            _ => {}
        }
    }
}

async fn run_live_turn(
    engine: &Engine,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Event>,
    session: &str,
    provider: &ProviderId,
    index: usize,
    continuity_key: &str,
) -> String {
    let token = format!("C2_{}_{}_OK", provider.as_str().to_uppercase(), index);
    let expected = format!("{token}_{continuity_key}");
    let instruction = if index == 0 {
        let context_chars = std::env::var("CODETWO_LIVE_SWITCH_CONTEXT_CHARS")
            .map(|value| {
                value
                    .parse::<usize>()
                    .expect("context size must be an integer")
            })
            .unwrap_or(0);
        assert!(
            context_chars <= 256 * 1024,
            "live fixture is limited to 256 Ki characters"
        );
        let filler = "Synthetic reference data: the desktop app uses Rust and TypeScript. ";
        let reference = filler.repeat(context_chars.div_ceil(filler.len()));
        let reference = &reference[..context_chars];
        format!(
            "Remember this continuity key for later provider switches: {continuity_key}. \
             The following reference appendix is synthetic test data.\n{reference}\n\
             Reply with exactly {expected} and no other text. Do not use tools."
        )
    } else {
        format!(
            "From the conversation before this turn, recover the continuity key I asked you to \
             remember. Reply with the literal prefix {token}_ followed immediately by that key, \
             and no other text. Do not use tools."
        )
    };
    engine
        .submit(prompt(
            session,
            &instruction,
            &format!("live-provider-turn-{index}"),
        ))
        .await
        .unwrap();
    let mut reply = String::new();
    loop {
        match next_live_event(rx).await {
            Event::AgentText {
                session: routed,
                text,
                ..
            } if routed == session => reply.push_str(&text),
            Event::PermissionRequest {
                session: routed,
                request_id,
                ..
            } if routed == session => {
                engine
                    .submit(Op::AnswerPermission {
                        session: routed,
                        request_id,
                        option_id: None,
                    })
                    .await
                    .unwrap();
            }
            Event::ElicitationRequest {
                session: routed,
                request_id,
                ..
            } if routed == session => {
                engine
                    .submit(Op::AnswerElicitation {
                        session: routed,
                        request_id,
                        answer: codetwo_core::elicitation::ElicitationAnswer::Decline,
                    })
                    .await
                    .unwrap();
            }
            Event::TurnEnded {
                session: routed, ..
            } if routed == session => break,
            Event::Error {
                session: routed,
                message,
                terminal: true,
                ..
            } if routed.as_deref() == Some(session) => {
                panic!("live provider turn failed: {message}")
            }
            _ => {}
        }
    }
    assert!(
        reply.contains(&expected),
        "{} did not recover {expected}; reply was {reply:?}",
        provider.as_str()
    );
    eprintln!("{} recalled the continuity key", provider.as_str());
    reply
}

fn assert_live_session_has_content(store: &Store, session: &str, continuity_key: &str) {
    let transcript = store.transcript(session).unwrap();
    assert!(
        transcript.iter().any(|(role, part)| {
            role == &codetwo_core::session::Role::User
                && matches!(
                    part,
                    codetwo_core::session::Part::Prompt { text, .. }
                        if text.contains(continuity_key)
                )
        }),
        "the durable Session must contain the user's continuity key before switching"
    );
    let provider_reply = transcript
        .iter()
        .filter_map(|(role, part)| match (role, part) {
            (codetwo_core::session::Role::Agent, codetwo_core::session::Part::Text { text }) => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect::<String>();
    assert!(
        provider_reply.contains(continuity_key),
        "the durable Session must contain a provider reply before switching"
    );
}

#[tokio::test]
#[ignore = "requires locally authenticated real provider CLIs"]
async fn live_providers_switch_in_place_and_back() {
    let providers = default_registry();
    let provider_names = std::env::var("CODETWO_LIVE_SWITCH_PROVIDERS")
        .expect("set CODETWO_LIVE_SWITCH_PROVIDERS to the authenticated provider ids to exercise");
    let sequence = provider_names
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|name| {
            providers
                .iter()
                .find(|provider| provider.id.as_str() == name)
                .unwrap_or_else(|| panic!("provider {name} is not registered"))
                .id
                .clone()
        })
        .collect::<Vec<_>>();
    assert!(sequence.len() >= 2, "provide at least two provider ids");
    assert!(
        sequence.windows(2).all(|pair| pair[0] != pair[1]),
        "adjacent providers must differ"
    );

    let store = Arc::new(Store::open_in_memory().unwrap());
    let (engine, mut rx) =
        Engine::with_store(providers, SkillLibrary::new(vec![]), store.clone());
    let _cleanup = LiveEngineGuard(engine.clone());
    let session = create_live_session(&engine, &mut rx, sequence[0].clone()).await;
    let original_session = session.clone();
    let continuity_key = format!("C2_CONTEXT_{}", uuid::Uuid::new_v4().simple());
    run_live_turn(&engine, &mut rx, &session, &sequence[0], 0, &continuity_key).await;

    for (index, provider) in sequence.iter().enumerate().skip(1) {
        assert_live_session_has_content(&store, &session, &continuity_key);
        let model = live_provider_model(provider);
        let switched = engine
            .switch_provider(&session, provider.clone(), model.clone())
            .await
            .unwrap_or_else(|error| panic!("switch to {} failed: {error}", provider.as_str()));
        assert_eq!(switched.id, original_session);
        assert_eq!(&switched.provider, provider);
        assert_eq!(switched.model, model);
        assert_eq!(
            store.get_session(&session).unwrap().unwrap().provider,
            *provider
        );
        run_live_turn(&engine, &mut rx, &session, provider, index, &continuity_key).await;
    }

    assert_eq!(session, original_session);
    assert_live_session_has_content(&store, &session, &continuity_key);
    engine.shutdown();
}

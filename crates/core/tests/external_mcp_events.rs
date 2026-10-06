//! External MCP events: engine → ring through the single tap (real engine, real stub provider
//! processes), poll/wait semantics, and the MCP event method shapes.

use codetwo_core::elicitation::{
    ElicitationAnswer, ElicitationField, ElicitationFieldKind, ElicitationForm,
};
use codetwo_core::event::{Event, Op};
use codetwo_core::external_mcp::catalog::external_tool_catalog;
use codetwo_core::external_mcp::clients::{ExternalScope, ProjectScope, ResolvedClient, TtlChoice};
use codetwo_core::external_mcp::ctx::CallContext;
use codetwo_core::external_mcp::events::{
    EventEnvelope, EventHub, EventKind, EventObserver, EventRing, PollRequest, ProjectFilter,
    EVENTS_RESOURCE_URI,
};
use codetwo_core::external_mcp::mcp_events::{capabilities, handle_method};
use codetwo_core::external_mcp::ops_events;
use codetwo_core::external_mcp::subscriptions::{
    StreamKind, StreamLimitError, SubscriptionRegistry, MAX_STREAMS_PER_CLIENT,
};
use codetwo_core::permission::PermissionContext;
use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
use codetwo_core::session::{PendingInput, PendingInputKind, SessionActivity, SessionRunState};
use codetwo_core::skill::{DocBlock, SkillLibrary};
use codetwo_core::{Engine, Store};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::mpsc::UnboundedReceiver;

const COMPLETING_AGENT: &str = r#"
import json, sys, time

def send(message):
    print(json.dumps(message), flush=True)

delay = float(sys.argv[1]) if len(sys.argv) > 1 else 0.0
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
        send({"jsonrpc":"2.0","method":"session/update","params":{
            "sessionId":"agent-session",
            "update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"SECRET-AGENT-TEXT"}}}})
        time.sleep(delay)
        send({"jsonrpc":"2.0","id":mid,"result":{"stopReason":"end_turn"}})
"#;

const PERMISSION_AGENT: &str = r#"
import json, sys

def send(message):
    print(json.dumps(message), flush=True)

pending_prompt = None
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
        pending_prompt = mid
        send({"jsonrpc":"2.0","id":1000,"method":"session/request_permission","params":{
            "sessionId":"agent-session",
            "toolCall":{"toolCallId":"tool-1","title":"rm build/","kind":"execute"},
            "options":[{"optionId":"allow","name":"Allow","kind":"allow_once"}]
        }})
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

struct Harness {
    engine: Engine,
    rx: UnboundedReceiver<Event>,
    client: ResolvedClient,
    _data: TempDir,
}

fn harness(script: &str, args: &[&str]) -> Harness {
    build_harness(script, args, None)
}

/// Durable queue/steer deliveries need a Store.
fn harness_with_store(script: &str, args: &[&str]) -> Harness {
    build_harness(
        script,
        args,
        Some(Arc::new(Store::open_in_memory().unwrap())),
    )
}

fn build_harness(script: &str, args: &[&str], store: Option<Arc<Store>>) -> Harness {
    let data = TempDir::new().unwrap();
    let mut launch_args = vec!["-c".to_string(), script.to_string()];
    launch_args.extend(args.iter().map(|a| a.to_string()));
    let provider = Provider {
        id: ProviderId::Grok,
        display_name: "Stub".into(),
        launch: LaunchSpec {
            command: "python3".into(),
            args: launch_args,
            env: Vec::new(),
            cwd: None,
        },
        needs_node: false,
    };
    let (engine, rx) = match store {
        Some(store) => Engine::with_store(vec![provider], SkillLibrary::new(vec![]), store),
        None => Engine::new(vec![provider], SkillLibrary::new(vec![])),
    };
    let state = engine.external_mcp_state();
    state.set_enabled(true);
    state.configure_data_dir(data.path()).unwrap();
    let token = state
        .with_registry(|reg| {
            reg.create(
                "events-test",
                vec![ExternalScope::Read],
                ProjectScope::All,
                TtlChoice::Days(1),
            )
        })
        .unwrap()
        .unwrap()
        .1;
    let client = state
        .with_registry(|reg| reg.resolve(&token))
        .unwrap()
        .unwrap();
    Harness {
        engine,
        rx,
        client,
        _data: data,
    }
}

async fn next_event(rx: &mut UnboundedReceiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(15), rx.recv())
        .await
        .expect("event before timeout")
        .expect("event stream stays open")
}

async fn create_session(h: &mut Harness) -> String {
    h.engine
        .submit(Op::NewSession {
            provider: ProviderId::Grok,
            cwd: std::env::temp_dir().to_string_lossy().to_string(),
            use_worktree: false,
            worktree_base: None,
            worktree_base_sha: None,
            request_id: Some("create".into()),
            model: None,
            initial_policy: None,
        })
        .await
        .unwrap();
    loop {
        if let Event::SessionCreated { session, .. } = next_event(&mut h.rx).await {
            return session;
        }
    }
}

async fn prompt(h: &Harness, session: &str, request_id: &str) {
    h.engine
        .submit(Op::Prompt {
            session: session.into(),
            doc: vec![DocBlock::Text { text: "go".into() }],
            request_id: Some(request_id.into()),
        })
        .await
        .unwrap();
}

/// Drain the channel until `done` matches (the ring was fed by the tap regardless).
async fn pump_until(h: &mut Harness, done: impl Fn(&Event) -> bool) -> Event {
    loop {
        let event = next_event(&mut h.rx).await;
        if done(&event) {
            return event;
        }
    }
}

fn tool_ctx<'a>(client: &'a ResolvedClient, tool: &str) -> CallContext<'a> {
    let spec = external_tool_catalog()
        .iter()
        .find(|s| s.name == tool)
        .unwrap();
    CallContext { client, spec }
}

fn call(h: &Harness, tool: &str, args: Value) -> Value {
    let ctx = tool_ctx(&h.client, tool);
    tokio::task::block_in_place(|| ops_events::call(&h.engine, &ctx, tool, &args))
        .expect("event tool")
        .expect("tool result")
}

fn kinds(poll: &Value) -> Vec<String> {
    poll["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["type"].as_str().unwrap().to_string())
        .collect()
}

fn count(poll: &Value, kind: EventKind) -> usize {
    kinds(&poll.clone())
        .iter()
        .filter(|k| k.as_str() == kind.as_str())
        .count()
}

fn of_kind(poll: &Value, kind: EventKind) -> Vec<Value> {
    poll["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == kind.as_str())
        .cloned()
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_real_completed_turn_reaches_the_ring_once_with_turn_id_and_receipt() {
    let mut h = harness(COMPLETING_AGENT, &["0"]);
    let base = h.engine.external_mcp_state().hub().head_cursor();
    let session = create_session(&mut h).await;
    prompt(&h, &session, "receipt-1").await;
    pump_until(&mut h, |e| matches!(e, Event::TurnEnded { .. })).await;

    let poll = call(
        &h,
        "codetwo_events_poll",
        json!({"cursor": base, "limit": 100}),
    );
    assert_eq!(
        count(&poll, EventKind::SessionCreated),
        1,
        "{:?}",
        kinds(&poll)
    );
    assert_eq!(
        count(&poll, EventKind::TurnStarted),
        1,
        "no duplicate ingest"
    );
    assert_eq!(
        count(&poll, EventKind::TurnCompleted),
        1,
        "end_turn is a completion"
    );
    assert_eq!(count(&poll, EventKind::TurnFailed), 0);

    let started = &of_kind(&poll, EventKind::TurnStarted)[0];
    let completed = &of_kind(&poll, EventKind::TurnCompleted)[0];
    assert_eq!(started["data"]["request_id"], "receipt-1");
    let turn_id = started["turn_id"]
        .as_str()
        .expect("turn id on turn.started");
    assert_eq!(
        completed["turn_id"], turn_id,
        "terminal event carries the same turn id"
    );
    assert_eq!(started["session_id"], session.as_str());
    assert!(
        started["project_id"].as_str().is_some(),
        "project resolved by the tap"
    );
    assert!(
        !poll.to_string().contains("SECRET-AGENT-TEXT"),
        "agent text never reaches the event stream"
    );

    // turn_wait: by turn id and by delivery receipt; both answer immediately (already over).
    let by_turn = call(
        &h,
        "codetwo_turn_wait",
        json!({"session_id": session, "turn_id": turn_id, "timeout_ms": 2000}),
    );
    assert_eq!(by_turn["status"], "completed");
    assert_eq!(by_turn["timed_out"], false);
    let by_receipt = call(
        &h,
        "codetwo_turn_wait",
        json!({"session_id": session, "delivery_id": "receipt-1", "timeout_ms": 2000}),
    );
    assert_eq!(by_receipt["status"], "completed");
    assert_eq!(by_receipt["turn_id"], turn_id);
    let legacy = call(
        &h,
        "codetwo_turn_wait",
        json!({"session_id": session, "timeout_ms": 2000}),
    );
    assert_eq!(legacy["status"], "completed");

    // Null/absent cursor = head: nothing replays.
    let head = call(&h, "codetwo_events_poll", json!({"cursor": null}));
    assert!(head["events"].as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn turn_wait_times_out_structurally_while_running_then_completes() {
    let mut h = harness(COMPLETING_AGENT, &["1.2"]);
    let session = create_session(&mut h).await;
    prompt(&h, &session, "slow").await;
    pump_until(&mut h, |e| matches!(e, Event::TurnStarted { .. })).await;

    let started = std::time::Instant::now();
    let waiting = call(
        &h,
        "codetwo_turn_wait",
        json!({"session_id": session, "timeout_ms": 150}),
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(waiting["status"], "running");
    assert_eq!(waiting["timed_out"], true);
    let turn_id = waiting["turn_id"]
        .as_str()
        .expect("turn id of the running turn")
        .to_string();

    let done = call(
        &h,
        "codetwo_turn_wait",
        json!({"session_id": session, "turn_id": turn_id, "timeout_ms": 10000}),
    );
    assert_eq!(done["status"], "completed", "{done}");
    assert_eq!(done["timed_out"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn approval_flow_is_visible_and_turn_wait_returns_waiting_state_immediately() {
    let mut h = harness(PERMISSION_AGENT, &[]);
    let base = h.engine.external_mcp_state().hub().head_cursor();
    let session = create_session(&mut h).await;
    prompt(&h, &session, "needs-approval").await;
    let request_id =
        match pump_until(&mut h, |e| matches!(e, Event::PermissionRequest { .. })).await {
            Event::PermissionRequest { request_id, .. } => request_id,
            _ => unreachable!(),
        };

    let mid = call(
        &h,
        "codetwo_events_poll",
        json!({"cursor": base, "limit": 100}),
    );
    assert_eq!(count(&mid, EventKind::ApprovalRequested), 1);
    assert_eq!(h.engine.external_mcp_state().list_pending().len(), 1);

    let started = std::time::Instant::now();
    let waiting = call(
        &h,
        "codetwo_turn_wait",
        json!({"session_id": session, "timeout_ms": 20000}),
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "waiting states return immediately"
    );
    assert_eq!(waiting["status"], "waiting_approval");
    assert_eq!(waiting["timed_out"], false);

    assert!(h
        .engine
        .answer_permission(&session, &request_id, Some("allow")));
    pump_until(&mut h, |e| matches!(e, Event::TurnEnded { .. })).await;
    let after = call(
        &h,
        "codetwo_events_poll",
        json!({"cursor": base, "limit": 100}),
    );
    assert_eq!(count(&after, EventKind::ApprovalResolved), 1);
    assert_eq!(count(&after, EventKind::TurnCompleted), 1);
    assert!(h.engine.external_mcp_state().list_pending().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_turn_is_a_failure_and_turn_wait_reports_it_even_after_the_session_is_gone() {
    let mut h = harness(FAILING_AGENT, &[]);
    let base = h.engine.external_mcp_state().hub().head_cursor();
    let session = create_session(&mut h).await;
    prompt(&h, &session, "boom").await;
    pump_until(&mut h, |e| matches!(e, Event::Error { terminal: true, .. })).await;

    let poll = call(
        &h,
        "codetwo_events_poll",
        json!({"cursor": base, "limit": 100}),
    );
    assert!(
        count(&poll, EventKind::TurnFailed) >= 1,
        "{:?}",
        kinds(&poll)
    );
    assert_eq!(count(&poll, EventKind::TurnCompleted), 0);
    let waited = call(
        &h,
        "codetwo_turn_wait",
        json!({"session_id": session, "timeout_ms": 3000}),
    );
    // The provider failure also tears the session runtime down (`session.broken`), which is the
    // more specific terminal fact for that turn; either spelling is a non-success.
    assert!(
        ["failed", "broken"].contains(&waited["status"].as_str().unwrap()),
        "{waited}"
    );
    assert_eq!(waited["timed_out"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn events_wait_timeout_is_a_structured_empty_result() {
    let h = harness(COMPLETING_AGENT, &["0"]);
    let start = std::time::Instant::now();
    let out = call(&h, "codetwo_events_wait", json!({"timeout_ms": 80}));
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(out["timed_out"], true);
    assert_eq!(out["events"], json!([]));
    assert_eq!(out["reset"], false);
    assert!(out["next_cursor"].as_str().is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_event_methods_use_current_labels_and_head_semantics() {
    let h = harness(COMPLETING_AGENT, &["0"]);
    let caps = capabilities();
    assert_eq!(caps["resources"]["subscribe"], true);
    let draft = &caps["codetwo/eventSupport"]["mcpEventsDraft"];
    assert_eq!(draft["status"], "draft-experimental");
    assert_eq!(draft["official"], false);
    assert_eq!(draft["webhook"], false);
    assert_eq!(
        caps["codetwo/eventSupport"]["streamableHttp"]["status"],
        "older-compatibility"
    );

    let list = handle_method(&h.engine, &h.client, "resources/list", &json!({}))
        .unwrap()
        .unwrap();
    assert_eq!(list["resources"][0]["uri"], EVENTS_RESOURCE_URI);
    let events = handle_method(&h.engine, &h.client, "events/list", &json!({}))
        .unwrap()
        .unwrap();
    let names: Vec<_> = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names.len(), EventKind::ALL.len());
    assert!(names.iter().all(|n| n.starts_with("codetwo.")));

    let poll = handle_method(
        &h.engine,
        &h.client,
        "events/poll",
        &json!({"name":"codetwo.turn.started","arguments":{},"cursor":null}),
    )
    .unwrap()
    .unwrap();
    assert_eq!(poll["events"], json!([]));
    assert_eq!(poll["truncated"], false);
    assert_eq!(poll["hasMore"], false);
    assert!(poll["cursor"].as_str().is_some());
    assert!(poll.get("nextPollMs").is_some());

    // Streaming methods are not served on the plain JSON path.
    for method in ["subscriptions/listen", "events/stream"] {
        assert!(
            handle_method(&h.engine, &h.client, method, &json!({}))
                .unwrap()
                .is_err(),
            "{method}"
        );
    }
}

#[test]
fn stream_slots_are_bounded_and_released_on_drop() {
    let registry = Arc::new(SubscriptionRegistry::default());
    let guards: Vec<_> = (0..MAX_STREAMS_PER_CLIENT)
        .map(|_| {
            registry
                .open_stream("cap-client", StreamKind::Events)
                .unwrap()
        })
        .collect();
    assert_eq!(
        registry.open_stream("cap-client", StreamKind::Events).err(),
        Some(StreamLimitError::TooManyStreams)
    );
    drop(guards);
    assert!(registry
        .open_stream("cap-client", StreamKind::Listen)
        .is_ok());
}

#[test]
fn ring_gap_resets_without_error() {
    let mut ring = EventRing::new(4);
    for _ in 0..8 {
        ring.push(
            EventKind::TurnStarted,
            None,
            Some("p".into()),
            None,
            json!({}),
        );
    }
    let stale = ring
        .poll(PollRequest {
            cursor: Some(format!("{}:1", ring.epoch())),
            kinds: None,
            session_id: None,
            project_filter: ProjectFilter::All,
            limit: 10,
        })
        .unwrap();
    assert!(stale.reset);
    assert_eq!(stale.next_cursor, stale.oldest_cursor.clone().unwrap());
}

#[test]
fn waiting_does_not_block_event_producers() {
    let hub = EventHub::new(EventRing::new(8));
    std::thread::scope(|scope| {
        let waiting = hub.clone();
        let waiter = scope.spawn(move || {
            waiting.wait_for(
                PollRequest {
                    cursor: None,
                    kinds: None,
                    session_id: None,
                    project_filter: ProjectFilter::All,
                    limit: 8,
                },
                Duration::from_millis(1000),
            )
        });
        std::thread::sleep(Duration::from_millis(30));
        let producing = hub.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        scope.spawn(move || {
            producing
                .ring()
                .lock()
                .unwrap()
                .push_resolved(
                    EventKind::ApprovalResolved,
                    Some("session".into()),
                    None,
                    None,
                    json!({"request_id": "request"}),
                )
                .unwrap();
            tx.send(()).unwrap();
        });
        rx.recv_timeout(Duration::from_millis(250))
            .expect("producer must run while the waiter is pending");
        assert_eq!(waiter.join().unwrap().unwrap().events.len(), 1);
    });
}

// ---- request lifecycle, receipts, and eviction ------------------------------------------------

/// Asks one form question and reports back what was answered (see `engine_elicitation.rs`).
const QUESTION_AGENT: &str = r#"
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
        send({"jsonrpc":"2.0","id":3000,"method":"elicitation/create","params":{
            "mode":"form","sessionId":"agent-session","toolCallId":"toolu_ask",
            "message":"Which auth method should we use?",
            "requestedSchema":{"type":"object","properties":{"question_0":{
                "type":"string","title":"Auth method",
                "oneOf":[{"const":"OAuth","title":"OAuth"},{"const":"API key","title":"API key"}]}}}}})
    elif method is None and mid == 3000 and prompt_id is not None:
        send({"jsonrpc":"2.0","id":prompt_id,"result":{"stopReason":"end_turn"}})
        prompt_id = None
"#;

fn seq(envelope: &EventEnvelope) -> u64 {
    envelope.cursor.split_once(':').unwrap().1.parse().unwrap()
}

/// Every retained event after `base`, paged (the poll limit is 100).
fn drain(engine: &Engine, base: &str) -> Vec<EventEnvelope> {
    let mut cursor = base.to_string();
    let mut out = Vec::new();
    loop {
        let page = engine
            .external_mcp_state()
            .hub()
            .poll(PollRequest {
                cursor: Some(cursor),
                kinds: None,
                session_id: None,
                project_filter: ProjectFilter::All,
                limit: 100,
            })
            .unwrap();
        out.extend(page.events);
        cursor = page.next_cursor;
        if !page.has_more {
            return out;
        }
    }
}

fn of(events: &[EventEnvelope], kind: EventKind) -> Vec<&EventEnvelope> {
    events.iter().filter(|e| e.kind == kind).collect()
}

fn request_id_of(event: &EventEnvelope) -> &str {
    event.data["request_id"].as_str().unwrap()
}

/// `requested` strictly precedes its single `resolved`, which carries `outcome`.
fn assert_closed_once(
    events: &[EventEnvelope],
    requested: EventKind,
    resolved: EventKind,
    id: &str,
    outcome: &str,
) {
    let opened: Vec<_> = of(events, requested)
        .into_iter()
        .filter(|e| request_id_of(e) == id)
        .collect();
    let closed: Vec<_> = of(events, resolved)
        .into_iter()
        .filter(|e| request_id_of(e) == id)
        .collect();
    assert_eq!(opened.len(), 1, "{id}: one {requested:?}");
    assert_eq!(closed.len(), 1, "{id}: one {resolved:?}");
    assert!(
        seq(opened[0]) < seq(closed[0]),
        "{id}: requested before resolved"
    );
    assert_eq!(closed[0].data["outcome"], outcome, "{id}");
}

fn pending_input(id: &str, kind: PendingInputKind) -> PendingInput {
    PendingInput {
        input_id: id.into(),
        kind,
        title: "title".into(),
        options: vec![("allow".into(), "Allow".into())],
        option_kinds: BTreeMap::new(),
        sequence: 1,
        context: PermissionContext {
            tool: Some("run_terminal".into()),
            ..Default::default()
        },
        form: (kind == PendingInputKind::Elicitation).then(|| ElicitationForm {
            message: "Which?".into(),
            tool_call_id: None,
            fields: vec![ElicitationField {
                key: "q".into(),
                kind: ElicitationFieldKind::Text,
                title: Some("Q".into()),
                description: None,
                required: true,
                options: vec![],
                custom_answer_for: None,
            }],
        }),
    }
}

fn awaiting(session: &str, revision: u64, id: &str, kind: PendingInputKind) -> Event {
    Event::SessionActivityChanged {
        session: session.into(),
        activity: SessionActivity {
            revision,
            state: SessionRunState::AwaitingInput {
                turn_id: "t1".into(),
                prompt_request_id: Some("p".into()),
                pending: vec![pending_input(id, kind)],
            },
        },
    }
}

fn running(session: &str, revision: u64) -> Event {
    Event::SessionActivityChanged {
        session: session.into(),
        activity: SessionActivity {
            revision,
            state: SessionRunState::Running {
                turn_id: "t1".into(),
                prompt_request_id: Some("p".into()),
            },
        },
    }
}

fn enabled_engine() -> (Engine, UnboundedReceiver<Event>) {
    let (engine, rx) = Engine::new(vec![], SkillLibrary::new(vec![]));
    engine.external_mcp_state().set_enabled(true);
    (engine, rx)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_real_question_is_requested_and_resolved_as_a_question_however_it_is_answered() {
    let mut h = harness(QUESTION_AGENT, &[]);
    let base = h.engine.external_mcp_state().hub().head_cursor();
    let session = create_session(&mut h).await;
    let mut ids = Vec::new();
    // Answered with form content, then through the permission-shaped route (a skip): the second
    // used to be reported as `approval.resolved` because the hook knew only a bool.
    for (round, via_permission) in [false, true].into_iter().enumerate() {
        prompt(&h, &session, &format!("ask-{round}")).await;
        let request_id =
            match pump_until(&mut h, |e| matches!(e, Event::ElicitationRequest { .. })).await {
                Event::ElicitationRequest { request_id, .. } => request_id,
                _ => unreachable!(),
            };
        assert_eq!(h.engine.external_mcp_state().list_pending().len(), 1);
        let accepted = if via_permission {
            h.engine.answer_permission(&session, &request_id, None)
        } else {
            let content = serde_json::Map::from_iter([("question_0".to_string(), json!("OAuth"))]);
            h.engine.answer_elicitation(
                &session,
                &request_id,
                ElicitationAnswer::Accept { content },
            )
        };
        assert!(accepted);
        assert!(
            !h.engine.answer_permission(&session, &request_id, None),
            "duplicate answer refused"
        );
        pump_until(&mut h, |e| matches!(e, Event::TurnEnded { .. })).await;
        ids.push(request_id);
    }
    let events = drain(&h.engine, &base);
    for id in &ids {
        assert_closed_once(
            &events,
            EventKind::QuestionRequested,
            EventKind::QuestionResolved,
            id,
            "answered",
        );
    }
    assert!(of(&events, EventKind::ApprovalRequested).is_empty());
    assert!(
        of(&events, EventKind::ApprovalResolved).is_empty(),
        "a question is never an approval"
    );
    assert!(h.engine.external_mcp_state().list_pending().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stop_cancels_a_pending_approval_and_closes_it_exactly_once() {
    let mut h = harness(PERMISSION_AGENT, &[]);
    let base = h.engine.external_mcp_state().hub().head_cursor();
    let session = create_session(&mut h).await;
    prompt(&h, &session, "needs-approval").await;
    let request_id =
        match pump_until(&mut h, |e| matches!(e, Event::PermissionRequest { .. })).await {
            Event::PermissionRequest { request_id, .. } => request_id,
            _ => unreachable!(),
        };
    assert_eq!(h.engine.external_mcp_state().list_pending().len(), 1);

    h.engine
        .submit(Op::Cancel {
            session: session.clone(),
        })
        .await
        .unwrap();
    pump_until(&mut h, |e| matches!(e, Event::TurnEnded { .. })).await;

    let events = drain(&h.engine, &base);
    assert_closed_once(
        &events,
        EventKind::ApprovalRequested,
        EventKind::ApprovalResolved,
        &request_id,
        "cancelled",
    );
    let resolved = seq(of(&events, EventKind::ApprovalResolved)[0]);
    let terminal = events
        .iter()
        .find(|e| e.kind.is_turn_terminal())
        .expect("turn terminal");
    assert!(
        resolved < seq(terminal),
        "cancellation closes the request before the turn ends"
    );
    assert!(
        h.engine.external_mcp_state().list_pending().is_empty(),
        "no stale pending"
    );

    // Late answers and a late echo of the request event change nothing.
    assert!(!h
        .engine
        .answer_permission(&session, &request_id, Some("allow")));
    h.engine
        .external_mcp_state()
        .tap()
        .observe(&Event::PermissionRequest {
            session: session.clone(),
            request_id: request_id.clone(),
            title: "late".into(),
            options: vec![],
            context: PermissionContext::default(),
        });
    assert_eq!(
        drain(&h.engine, &base).len(),
        events.len(),
        "no duplicate envelopes"
    );
    assert!(h.engine.external_mcp_state().list_pending().is_empty());
}

#[test]
fn reordered_and_late_signals_cannot_resolve_before_request_or_duplicate() {
    let (engine, _rx) = enabled_engine();
    let state = engine.external_mcp_state();
    let tap = state.tap();
    let base = state.hub().head_cursor();

    // park publishes the snapshot first; the answer wins the race to the request event.
    tap.observe(&awaiting("s", 2, "q1", PendingInputKind::Elicitation));
    tap.observe(&running("s", 3));
    tap.observe(&Event::ElicitationRequest {
        session: "s".into(),
        request_id: "q1".into(),
        form: pending_input("q1", PendingInputKind::Elicitation)
            .form
            .unwrap(),
    });
    tap.observe(&awaiting("s", 2, "q1", PendingInputKind::Elicitation)); // stale snapshot
    let events = drain(&engine, &base);
    assert_eq!(events.len(), 2, "{events:?}");
    assert_closed_once(
        &events,
        EventKind::QuestionRequested,
        EventKind::QuestionResolved,
        "q1",
        "cancelled",
    );
    assert!(state.list_pending().is_empty());

    // An answer in flight labels the removal; a refused answer does not.
    tap.observe(&awaiting("s", 4, "p1", PendingInputKind::Permission));
    state.begin_answer("p1");
    tap.observe(&running("s", 5));
    state.finish_answer("p1", true);
    tap.observe(&awaiting("s", 6, "p2", PendingInputKind::Permission));
    state.begin_answer("p2");
    state.finish_answer("p2", false);
    tap.observe(&running("s", 7));
    // Without a tracker only the request event exists; the accepted answer closes it.
    tap.observe(&Event::PermissionRequest {
        session: "s".into(),
        request_id: "legacy".into(),
        title: "t".into(),
        options: vec![],
        context: PermissionContext::default(),
    });
    assert_eq!(state.list_pending().len(), 1);
    state.finish_answer("legacy", true);
    state.finish_answer("legacy", true);

    let events = drain(&engine, &base);
    let approvals = |kind| of(&events, kind).len();
    assert_eq!(approvals(EventKind::ApprovalRequested), 3);
    assert_eq!(approvals(EventKind::ApprovalResolved), 3);
    assert_closed_once(
        &events,
        EventKind::ApprovalRequested,
        EventKind::ApprovalResolved,
        "p1",
        "answered",
    );
    assert_closed_once(
        &events,
        EventKind::ApprovalRequested,
        EventKind::ApprovalResolved,
        "p2",
        "cancelled",
    );
    assert_closed_once(
        &events,
        EventKind::ApprovalRequested,
        EventKind::ApprovalResolved,
        "legacy",
        "answered",
    );
    assert!(state.list_pending().is_empty());
}

#[test]
fn open_requests_are_capped_and_expire_with_a_resolved_envelope() {
    let (engine, _rx) = enabled_engine();
    let state = engine.external_mcp_state();
    let base = state.hub().head_cursor();
    for i in 0..1100 {
        state.tap().observe(&awaiting(
            &format!("s{i}"),
            1,
            &format!("r{i}"),
            PendingInputKind::Permission,
        ));
    }
    assert_eq!(state.list_pending().len(), 1024, "storage is bounded");
    let events = drain(&engine, &base);
    let expired: Vec<_> = of(&events, EventKind::ApprovalResolved);
    assert_eq!(expired.len(), 76);
    for event in expired {
        assert_eq!(event.data["outcome"], "expired");
        let id = request_id_of(event);
        assert_closed_once(
            &events,
            EventKind::ApprovalRequested,
            EventKind::ApprovalResolved,
            id,
            "expired",
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_prompt_rejected_before_it_starts_is_not_attributed_to_the_previous_turn() {
    let mut h = harness(COMPLETING_AGENT, &["0"]);
    let base = h.engine.external_mcp_state().hub().head_cursor();
    let session = create_session(&mut h).await;
    prompt(&h, &session, "r1").await;
    pump_until(&mut h, |e| matches!(e, Event::TurnEnded { .. })).await;
    let first = drain(&h.engine, &base);
    let t1 = of(&first, EventKind::TurnStarted)[0]
        .turn_id
        .clone()
        .unwrap();

    // The provider never advertised `/compact`: refused (terminal) before any turn is claimed.
    h.engine
        .submit(Op::Prompt {
            session: session.clone(),
            doc: vec![DocBlock::Text {
                text: "/compact".into(),
            }],
            request_id: Some("r2".into()),
        })
        .await
        .unwrap();
    pump_until(&mut h, |e| matches!(e, Event::Error { terminal: true, .. })).await;

    let events = drain(&h.engine, &base);
    let failed = of(&events, EventKind::TurnFailed);
    assert_eq!(failed.len(), 1, "{events:?}");
    assert_eq!(failed[0].turn_id, None, "must not borrow the old turn's id");
    assert_eq!(failed[0].data["request_id"], "r2");
    assert_eq!(failed[0].data["turn_started"], false);
    let completed = of(&events, EventKind::TurnCompleted);
    assert_eq!(completed.len(), 1);
    assert_eq!(
        completed[0].turn_id.as_deref(),
        Some(t1.as_str()),
        "the old turn stays completed"
    );

    let rejected = call(
        &h,
        "codetwo_turn_wait",
        json!({"session_id": session, "delivery_id": "r2", "timeout_ms": 2000}),
    );
    assert_eq!(rejected["status"], "failed", "{rejected}");
    assert_eq!(rejected["delivered"], false);
    assert_eq!(rejected["no_retry"], false, "provably never delivered");
    assert_eq!(rejected["reason"], "rejected");
    assert_eq!(rejected["turn_id"], Value::Null);
    assert_eq!(rejected["timed_out"], false);
    let old = call(
        &h,
        "codetwo_turn_wait",
        json!({"session_id": session, "turn_id": t1, "timeout_ms": 2000}),
    );
    assert_eq!(old["status"], "completed");
    let by_receipt = call(
        &h,
        "codetwo_turn_wait",
        json!({"session_id": session, "delivery_id": "r1", "timeout_ms": 2000}),
    );
    assert_eq!(
        (
            by_receipt["status"].as_str(),
            by_receipt["turn_id"].as_str()
        ),
        (Some("completed"), Some(t1.as_str()))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_busy_refusal_fails_its_receipt_without_failing_the_running_turn() {
    let mut h = harness(PERMISSION_AGENT, &[]);
    let base = h.engine.external_mcp_state().hub().head_cursor();
    let session = create_session(&mut h).await;
    prompt(&h, &session, "first").await;
    pump_until(&mut h, |e| matches!(e, Event::PermissionRequest { .. })).await;
    prompt(&h, &session, "second").await;
    pump_until(
        &mut h,
        |e| matches!(e, Event::Error { terminal: false, request_id: Some(r), .. } if r == "second"),
    )
    .await;

    assert!(
        of(&drain(&h.engine, &base), EventKind::TurnFailed).is_empty(),
        "a refusal is not a turn failure"
    );
    let refused = call(
        &h,
        "codetwo_turn_wait",
        json!({"session_id": session, "delivery_id": "second", "timeout_ms": 2000}),
    );
    assert_eq!(refused["status"], "failed", "{refused}");
    assert_eq!(
        (
            refused["delivered"].as_bool(),
            refused["no_retry"].as_bool()
        ),
        (Some(false), Some(false))
    );
    let live = call(
        &h,
        "codetwo_turn_wait",
        json!({"session_id": session, "delivery_id": "first", "timeout_ms": 2000}),
    );
    assert_eq!(
        live["status"], "waiting_approval",
        "the running turn is untouched: {live}"
    );
    assert_eq!(live["no_retry"], true);
    assert_eq!(live["reason"], "in_flight");

    h.engine.submit(Op::Cancel { session }).await.unwrap();
    pump_until(&mut h, |e| matches!(e, Event::TurnEnded { .. })).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queue_and_steer_receipts_report_their_durable_delivery_state() {
    let mut h = harness_with_store(COMPLETING_AGENT, &["0"]);
    let session = create_session(&mut h).await;
    let store = h.engine.store().unwrap();
    let doc = || vec![DocBlock::Text { text: "go".into() }];
    fn wait_for_receipt(h: &Harness, session: &str, id: &str, timeout_ms: u64) -> Value {
        call(
            h,
            "codetwo_turn_wait",
            json!({"session_id": session, "delivery_id": id, "timeout_ms": timeout_ms}),
        )
    }

    // A queue delivery that really runs resolves to its turn.
    h.engine
        .deliver_prompt(&session, doc(), "queue", Some("q-ok".into()), None)
        .await
        .unwrap();
    pump_until(&mut h, |e| matches!(e, Event::TurnEnded { .. })).await;
    let ok = wait_for_receipt(&h, &session, "q-ok", 3000);
    assert_eq!(ok["status"], "completed", "{ok}");
    assert!(ok["turn_id"].as_str().is_some());

    // Native steering on an idle session fails before anything is sent.
    h.engine
        .deliver_prompt(&session, doc(), "steer", Some("s-fail".into()), None)
        .await
        .unwrap();
    let steer = wait_for_receipt(&h, &session, "s-fail", 2000);
    assert_eq!(steer["status"], "failed", "{steer}");
    assert_eq!(
        (steer["delivered"].as_bool(), steer["no_retry"].as_bool()),
        (Some(false), Some(false))
    );
    assert_eq!(steer["reason"], "not_delivered");

    for (id, state, status, delivered, no_retry, reason) in [
        (
            "q-accepted",
            "accepted",
            "unknown",
            json!(true),
            true,
            "outcome_not_retained",
        ),
        (
            "q-unknown",
            "unknown",
            "unknown",
            Value::Null,
            true,
            "delivery_outcome_unknown",
        ),
        (
            "q-cancel",
            "cancelled",
            "cancelled",
            json!(false),
            false,
            "cancelled_before_send",
        ),
    ] {
        h.engine
            .enqueue_prompt(&session, doc(), "queue", Some(id.into()), None)
            .unwrap();
        store.finish_delivery(id, state, "test").unwrap();
        if state == "accepted" {
            h.engine.external_mcp_state().tap().observe(&Event::Error {
                session: Some(session.clone()),
                message: "late error".into(),
                terminal: true,
                request_id: Some(id.into()),
            });
        }
        let out = wait_for_receipt(&h, &session, id, 2000);
        assert_eq!(out["status"], status, "{out}");
        assert_eq!(out["delivered"], delivered, "{out}");
        assert_eq!(out["no_retry"], no_retry, "{out}");
        assert_eq!(out["reason"], reason);
        assert_eq!(out["timed_out"], false);
    }
    assert!(wait_for_receipt(&h, &session, "q-unknown", 100)["meaning"]
        .as_str()
        .unwrap()
        .contains("never replay"));

    // Still queued: waiting says so, and says not to resend.
    h.engine
        .enqueue_prompt(&session, doc(), "queue", Some("q-flight".into()), None)
        .unwrap();
    let queued = wait_for_receipt(&h, &session, "q-flight", 150);
    assert_eq!(
        (queued["status"].as_str(), queued["timed_out"].as_bool()),
        (Some("queued"), Some(true))
    );
    assert_eq!(
        (queued["no_retry"].as_bool(), queued["reason"].as_str()),
        (Some(true), Some("in_flight"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_evicted_turn_outcome_is_explicitly_unavailable_and_never_suggests_a_retry() {
    let mut h = harness(COMPLETING_AGENT, &["0"]);
    let base = h.engine.external_mcp_state().hub().head_cursor();
    let session = create_session(&mut h).await;
    prompt(&h, &session, "r1").await;
    pump_until(&mut h, |e| matches!(e, Event::TurnEnded { .. })).await;
    let t1 = of(&drain(&h.engine, &base), EventKind::TurnStarted)[0]
        .turn_id
        .clone()
        .unwrap();

    {
        let ring = h.engine.external_mcp_state().hub().ring();
        let mut ring = ring.lock().unwrap();
        for _ in 0..2100 {
            ring.push(
                EventKind::SessionUpdated,
                Some("other".into()),
                None,
                None,
                json!({}),
            );
        }
    }
    let by_turn = call(
        &h,
        "codetwo_turn_wait",
        json!({"session_id": session, "turn_id": t1, "timeout_ms": 5000}),
    );
    assert_eq!(by_turn["status"], "unknown", "{by_turn}");
    assert_eq!(by_turn["timed_out"], false);
    assert_eq!(by_turn["no_retry"], true);
    assert_eq!(by_turn["reason"], "outcome_not_retained");
    assert!(by_turn["meaning"]
        .as_str()
        .unwrap()
        .contains("never replay"));

    let by_receipt = call(
        &h,
        "codetwo_turn_wait",
        json!({"session_id": session, "delivery_id": "r1", "timeout_ms": 5000}),
    );
    assert_eq!(by_receipt["status"], "unknown", "{by_receipt}");
    assert_eq!(
        (
            by_receipt["no_retry"].as_bool(),
            by_receipt["reason"].as_str()
        ),
        (Some(true), Some("receipt_not_retained"))
    );
}

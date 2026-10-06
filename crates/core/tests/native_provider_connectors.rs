//! Contract fixtures for the native provider connectors.
//!
//! The Codex fixtures replay the JSON-RPC shapes of the installed `codex app-server` (schema
//! generated with `codex app-server generate-json-schema`, CLI 0.160.1) through in-memory pipes, so
//! the oracle is the documented wire format, not the connector's own encoder. Real-binary checks
//! that need no model turn are `#[ignore]`d and run explicitly.

use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use codetwo_core::connectors::codex::{self, item_to_tool_call, CodexRuntime};
use codetwo_core::permission::{ExecutionPolicy, PermissionMode, SandboxPolicy};
use codetwo_core::skill::{McpServer, McpTransport};
use codetwo_core::{
    ProviderRuntime, RuntimeBackendKind, RuntimeCallbacks, RuntimeContent, RuntimeError,
    RuntimeEvent, RuntimePermissionOutcome, RuntimePermissionRequest, RuntimeQuestionOutcome,
    RuntimeQuestionRequest, RuntimeSessionRestore, RuntimeSessionStart, SteerOutcome,
    SteerSupport, StopSupport, TurnOutcome, TurnTerminal,
};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines};

// ---- harness -----------------------------------------------------------------------------------

#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<(String, RuntimeEvent)>>,
    permissions: Mutex<Vec<RuntimePermissionRequest>>,
    questions: Mutex<Vec<RuntimeQuestionRequest>>,
    permission_script: Mutex<VecDeque<RuntimePermissionOutcome>>,
    question_script: Mutex<VecDeque<RuntimeQuestionOutcome>>,
}

#[async_trait]
impl RuntimeCallbacks for Recorder {
    async fn event(&self, backend_session_id: &str, event: RuntimeEvent) {
        self.events
            .lock()
            .unwrap()
            .push((backend_session_id.to_string(), event));
    }

    async fn request_permission(
        &self,
        _id: &str,
        request: RuntimePermissionRequest,
    ) -> RuntimePermissionOutcome {
        self.permissions.lock().unwrap().push(request);
        self.permission_script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(RuntimePermissionOutcome::Cancelled)
    }

    async fn ask_question(
        &self,
        _id: &str,
        request: RuntimeQuestionRequest,
    ) -> RuntimeQuestionOutcome {
        self.questions.lock().unwrap().push(request);
        self.question_script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(RuntimeQuestionOutcome::Declined)
    }
}

impl Recorder {
    fn texts(&self) -> String {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(_, event)| match event {
                RuntimeEvent::AgentText(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }
}

struct Server {
    lines: Lines<BufReader<DuplexStream>>,
    out: DuplexStream,
}

impl Server {
    async fn read(&mut self) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(5), self.lines.next_line())
            .await
            .expect("client wrote a frame within 5s")
            .unwrap()
            .expect("client stream open");
        serde_json::from_str(&line).unwrap()
    }

    async fn send(&mut self, value: Value) {
        self.out
            .write_all(format!("{value}\n").as_bytes())
            .await
            .unwrap();
    }

    /// Read the next frame, which must be a request for `method`, and return it.
    async fn expect(&mut self, method: &str) -> Value {
        let frame = self.read().await;
        assert_eq!(frame["method"], method, "unexpected frame: {frame}");
        frame
    }

    async fn reply(&mut self, request: &Value, result: Value) {
        self.send(json!({"id": request["id"], "result": result})).await;
    }

    async fn fail(&mut self, request: &Value, code: i64, message: &str) {
        self.send(json!({"id": request["id"], "error": {"code": code, "message": message}}))
            .await;
    }

    async fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"method": method, "params": params})).await;
    }
}

fn harness_with(
    config: serde_json::Map<String, Value>,
) -> (Arc<CodexRuntime>, Server, Arc<Recorder>) {
    let (client_out, server_in) = tokio::io::duplex(1 << 20);
    let (server_out, client_in) = tokio::io::duplex(1 << 20);
    let recorder = Arc::new(Recorder::default());
    let runtime = codex::connect(
        client_in,
        client_out,
        None,
        recorder.clone(),
        config,
        "0.0.0-test".into(),
    );
    let server = Server {
        lines: BufReader::new(server_in).lines(),
        out: server_out,
    };
    (runtime, server, recorder)
}

fn harness() -> (Arc<CodexRuntime>, Server, Arc<Recorder>) {
    harness_with(serde_json::Map::new())
}

fn model_list() -> Value {
    json!({"data": [
        {
            "id": "gpt-x", "model": "gpt-x", "displayName": "GPT X", "description": "fast",
            "hidden": false, "isDefault": true, "defaultReasoningEffort": "medium",
            "supportedReasoningEfforts": [
                {"reasoningEffort": "low", "description": "quick"},
                {"reasoningEffort": "medium", "description": "balanced"},
                {"reasoningEffort": "high", "description": "deep"}
            ]
        },
        {
            "id": "plain", "model": "plain", "displayName": "Plain", "description": "",
            "hidden": false, "isDefault": false, "defaultReasoningEffort": "none",
            "supportedReasoningEfforts": []
        },
        {
            "id": "secret", "model": "secret", "displayName": "Hidden", "description": "",
            "hidden": true, "isDefault": false, "defaultReasoningEffort": "none",
            "supportedReasoningEfforts": []
        }
    ]})
}

async fn initialized(runtime: &Arc<CodexRuntime>, server: &mut Server) {
    let rt = runtime.clone();
    let init = tokio::spawn(async move { rt.initialize().await });
    let request = server.expect("initialize").await;
    server
        .reply(
            &request,
            json!({"userAgent": "codetwo/0.160.1 (Mac OS 27.0.1; arm64) dumb", "codexHome": "/x",
                   "platformFamily": "unix", "platformOs": "macos"}),
        )
        .await;
    init.await.unwrap().unwrap();
    server.expect("initialized").await;
}

fn thread_response(id: &str) -> Value {
    json!({
        "thread": {"id": id, "status": {"type": "idle"}},
        "model": "gpt-x", "modelProvider": "openai", "reasoningEffort": "medium",
        "approvalPolicy": "untrusted", "cwd": "/work", "sandbox": {"type": "workspaceWrite"}
    })
}

async fn started_thread(
    runtime: &Arc<CodexRuntime>,
    server: &mut Server,
    thread_id: &str,
    execution: ExecutionPolicy,
) -> Value {
    let rt = runtime.clone();
    let start = tokio::spawn(async move {
        rt.start_session(RuntimeSessionStart {
            cwd: "/work".into(),
            mcp_servers: Vec::new(),
            execution,
        })
        .await
    });
    let models = server.expect("model/list").await;
    server.reply(&models, model_list()).await;
    let request = server.expect("thread/start").await;
    server.reply(&request, thread_response(thread_id)).await;
    start.await.unwrap().unwrap();
    request
}

fn turn_json(id: &str, status: &str) -> Value {
    json!({"id": id, "status": status, "items": []})
}

/// Begin a turn and return its handle plus the `turn/start` request the server saw.
async fn begin_turn(
    runtime: &Arc<CodexRuntime>,
    server: &mut Server,
    thread: &str,
    turn_id: &str,
) -> (tokio::task::JoinHandle<TurnOutcome>, Value) {
    let rt = runtime.clone();
    let thread_owned = thread.to_string();
    let handle = tokio::spawn(async move {
        rt.send_turn(&thread_owned, vec![RuntimeContent::text("hello")])
            .await
    });
    let request = server.expect("turn/start").await;
    server
        .reply(&request, json!({"turn": turn_json(turn_id, "inProgress")}))
        .await;
    server
        .notify(
            "turn/started",
            json!({"threadId": thread, "turn": turn_json(turn_id, "inProgress")}),
        )
        .await;
    (handle, request)
}

async fn complete_turn(server: &mut Server, thread: &str, turn_id: &str, status: &str) {
    server
        .notify(
            "turn/completed",
            json!({"threadId": thread, "turn": turn_json(turn_id, status)}),
        )
        .await;
}

async fn settle<T>(handle: tokio::task::JoinHandle<T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("operation settled within 5s")
        .unwrap()
}

const ASK: ExecutionPolicy = ExecutionPolicy {
    mode: PermissionMode::Ask,
    sandbox: SandboxPolicy::WorkspaceWrite,
};

// ---- Codex: initialize / session ---------------------------------------------------------------

#[tokio::test]
async fn codex_initialize_negotiates_identity_and_capabilities() {
    let (runtime, mut server, _) = harness();
    let rt = runtime.clone();
    let init = tokio::spawn(async move { rt.initialize().await });
    let request = server.expect("initialize").await;
    assert_eq!(request["params"]["clientInfo"]["name"], "codetwo");
    assert_eq!(request["params"]["capabilities"]["experimentalApi"], false);
    assert!(
        request.get("jsonrpc").is_none(),
        "app-server speaks the bare dialect"
    );
    server
        .reply(
            &request,
            json!({"userAgent": "codetwo/0.160.1 (Mac OS 27.0.1; arm64) dumb", "codexHome": "/x",
                   "platformFamily": "unix", "platformOs": "macos"}),
        )
        .await;
    let init = init.await.unwrap().unwrap();
    // `initialized` is a notification: no id.
    let ack = server.read().await;
    assert_eq!(ack["method"], "initialized");
    assert!(ack.get("id").is_none());

    assert_eq!(init.identity.backend, RuntimeBackendKind::CodexAppServer);
    assert_eq!(init.identity.adapter_version.as_deref(), Some("0.160.1"));
    let caps = init.capabilities;
    assert!(caps.resume.resume && !caps.resume.load);
    assert_eq!(caps.steering, SteerSupport::Native);
    assert_eq!(caps.stop, StopSupport::VerifiedTerminal);
    assert!(caps.mcp_stdio && caps.mcp_http && !caps.mcp_sse);
    assert_eq!(runtime.negotiated(), init);
    assert_eq!(runtime.kind(), RuntimeBackendKind::CodexAppServer);
}

#[tokio::test]
async fn codex_start_session_maps_policy_mcp_and_reports_catalog() {
    let mut config = serde_json::Map::new();
    config.insert("developer_instructions".into(), json!("be brief"));
    config.insert("mcp_servers".into(), json!({"node_repl": {"enabled": false}}));
    let (runtime, mut server, _) = harness_with(config);
    initialized(&runtime, &mut server).await;

    let rt = runtime.clone();
    let start = tokio::spawn(async move {
        rt.start_session(RuntimeSessionStart {
            cwd: "/work".into(),
            mcp_servers: vec![
                McpServer {
                    name: "broker".into(),
                    cwd: None,
                    transport: McpTransport::Stdio {
                        command: "bun".into(),
                        args: vec!["broker.ts".into()],
                        env: vec![("TOKEN".into(), "t".into())],
                    },
                },
                McpServer {
                    name: "remote".into(),
                    cwd: None,
                    transport: McpTransport::Http {
                        url: "https://example.test/mcp".into(),
                        headers: vec![("Authorization".into(), "Bearer x".into())],
                    },
                },
            ],
            execution: ExecutionPolicy {
                mode: PermissionMode::Yolo,
                sandbox: SandboxPolicy::ReadOnly,
            },
        })
        .await
    });
    let models = server.expect("model/list").await;
    server.reply(&models, model_list()).await;
    let request = server.expect("thread/start").await;
    let params = &request["params"];
    assert_eq!(params["cwd"], "/work");
    assert_eq!(params["approvalPolicy"], "on-request");
    assert_eq!(params["sandbox"], "read-only");
    assert_eq!(params["config"]["developer_instructions"], "be brief");
    // Host servers merge with, and do not replace, launch config entries.
    assert_eq!(params["config"]["mcp_servers"]["node_repl"]["enabled"], false);
    assert_eq!(params["config"]["mcp_servers"]["broker"]["command"], "bun");
    assert_eq!(params["config"]["mcp_servers"]["broker"]["env"]["TOKEN"], "t");
    assert_eq!(
        params["config"]["mcp_servers"]["remote"]["http_headers"]["Authorization"],
        "Bearer x"
    );
    server.reply(&request, thread_response("th-1")).await;
    let state = start.await.unwrap().unwrap();

    assert_eq!(state.backend_session_id, "th-1");
    let models = state.models.unwrap();
    assert_eq!(models.current, "gpt-x");
    // Hidden models are not offered.
    let ids = models.available.iter().map(|m| m.id.as_str()).collect::<Vec<_>>();
    assert_eq!(ids, ["gpt-x", "plain"]);
    let effort = &state.config_options[0];
    assert_eq!(effort.id, "reasoning_effort");
    assert_eq!(effort.category.as_deref(), Some("thought_level"));
    assert_eq!(effort.current, "medium");
    assert_eq!(effort.choices.len(), 3);
}

#[tokio::test]
async fn codex_rejects_sse_mcp_before_any_provider_call() {
    let (runtime, mut server, _) = harness();
    initialized(&runtime, &mut server).await;
    let error = runtime
        .start_session(RuntimeSessionStart {
            cwd: "/work".into(),
            mcp_servers: vec![McpServer {
                name: "legacy".into(),
                cwd: None,
                transport: McpTransport::Sse {
                    url: "https://example.test/sse".into(),
                    headers: Vec::new(),
                },
            }],
            execution: ASK,
        })
        .await
        .unwrap_err();
    assert!(matches!(error, RuntimeError::Unsupported(_)), "{error:?}");
    // Nothing was written after the handshake.
    let none = tokio::time::timeout(Duration::from_millis(150), server.lines.next_line()).await;
    assert!(none.is_err(), "no request may reach the provider");
}

#[tokio::test]
async fn codex_restore_resumes_without_history_and_checks_identity() {
    let (runtime, mut server, _) = harness();
    initialized(&runtime, &mut server).await;
    let replaying = Arc::new(AtomicBool::new(false));

    let rt = runtime.clone();
    let flag = replaying.clone();
    let restore = tokio::spawn(async move {
        rt.restore_session(
            RuntimeSessionRestore {
                backend_session_id: "old-thread".into(),
                cwd: "/work".into(),
                mcp_servers: Vec::new(),
                execution: ASK,
            },
            &flag,
        )
        .await
    });
    let models = server.expect("model/list").await;
    server.reply(&models, model_list()).await;
    let request = server.expect("thread/resume").await;
    assert_eq!(request["params"]["threadId"], "old-thread");
    assert_eq!(request["params"]["excludeTurns"], true);
    server.reply(&request, thread_response("old-thread")).await;
    assert_eq!(restore.await.unwrap().unwrap().backend_session_id, "old-thread");

    // A resume that answers for another thread must not be adopted as the requested one.
    let rt = runtime.clone();
    let flag = replaying.clone();
    let restore = tokio::spawn(async move {
        rt.restore_session(
            RuntimeSessionRestore {
                backend_session_id: "asked".into(),
                cwd: "/work".into(),
                mcp_servers: Vec::new(),
                execution: ASK,
            },
            &flag,
        )
        .await
    });
    let models = server.expect("model/list").await;
    server.reply(&models, model_list()).await;
    let request = server.expect("thread/resume").await;
    server.reply(&request, thread_response("answered")).await;
    assert!(matches!(
        restore.await.unwrap(),
        Err(RuntimeError::Decode(_))
    ));
}

// ---- Codex: turns and events -------------------------------------------------------------------

#[tokio::test]
async fn codex_turn_streams_neutral_events_and_ends_on_turn_completed() {
    let (runtime, mut server, recorder) = harness();
    initialized(&runtime, &mut server).await;
    started_thread(&runtime, &mut server, "th-1", ASK).await;

    let (handle, request) = begin_turn(&runtime, &mut server, "th-1", "turn-1").await;
    let params = &request["params"];
    assert_eq!(params["threadId"], "th-1");
    assert_eq!(params["input"][0]["type"], "text");
    assert_eq!(params["input"][0]["text"], "hello");
    assert_eq!(params["approvalPolicy"], "untrusted");
    assert_eq!(params["sandboxPolicy"]["type"], "workspaceWrite");
    assert_eq!(params["model"], "gpt-x");
    assert_eq!(params["effort"], "medium");
    assert_eq!(params["cwd"], "/work");

    server
        .notify(
            "item/reasoning/summaryTextDelta",
            json!({"threadId": "th-1", "turnId": "turn-1", "itemId": "r1", "summaryIndex": 0, "delta": "thinking"}),
        )
        .await;
    server
        .notify(
            "item/started",
            json!({"threadId": "th-1", "turnId": "turn-1", "startedAtMs": 1, "item": {
                "type": "commandExecution", "id": "c1", "command": "cargo test", "cwd": "/work",
                "status": "inProgress", "commandActions": []}}),
        )
        .await;
    server
        .notify(
            "item/completed",
            json!({"threadId": "th-1", "turnId": "turn-1", "completedAtMs": 2, "item": {
                "type": "commandExecution", "id": "c1", "command": "cargo test", "cwd": "/work",
                "status": "completed", "exitCode": 0, "aggregatedOutput": "ok\n", "commandActions": []}}),
        )
        .await;
    server
        .notify(
            "item/agentMessage/delta",
            json!({"threadId": "th-1", "turnId": "turn-1", "itemId": "m1", "delta": "Hel"}),
        )
        .await;
    server
        .notify(
            "item/agentMessage/delta",
            json!({"threadId": "th-1", "turnId": "turn-1", "itemId": "m1", "delta": "lo"}),
        )
        .await;
    // The completed item repeats the full text: it must not be emitted twice.
    server
        .notify(
            "item/completed",
            json!({"threadId": "th-1", "turnId": "turn-1", "completedAtMs": 3, "item": {
                "type": "agentMessage", "id": "m1", "text": "Hello"}}),
        )
        .await;
    server
        .notify(
            "thread/tokenUsage/updated",
            json!({"threadId": "th-1", "turnId": "turn-1", "tokenUsage": {
                "last": {"totalTokens": 1200, "inputTokens": 1000, "outputTokens": 200, "cachedInputTokens": 0, "reasoningOutputTokens": 0},
                "total": {"totalTokens": 5000, "inputTokens": 4000, "outputTokens": 1000, "cachedInputTokens": 0, "reasoningOutputTokens": 0},
                "modelContextWindow": 200000}}),
        )
        .await;
    server
        .notify("account/updated", json!({"threadId": "th-1", "authMode": "chatgpt"}))
        .await;
    complete_turn(&mut server, "th-1", "turn-1", "completed").await;

    assert_eq!(settle(handle).await, TurnOutcome::Terminal(TurnTerminal::EndTurn));
    assert_eq!(recorder.texts(), "Hello");
    let events = recorder.events.lock().unwrap();
    assert!(events.iter().all(|(id, _)| id == "th-1"));
    assert!(events
        .iter()
        .any(|(_, e)| matches!(e, RuntimeEvent::AgentThought(t) if t == "thinking")));
    let call = events
        .iter()
        .find_map(|(_, e)| match e {
            RuntimeEvent::ToolCall(call) => Some(call.clone()),
            _ => None,
        })
        .expect("tool call announced");
    assert_eq!(call.id, "c1");
    assert_eq!(call.kind.as_deref(), Some("execute"));
    assert_eq!(call.title.as_deref(), Some("cargo test"));
    assert_eq!(call.status.as_deref(), Some("in_progress"));
    let update = events
        .iter()
        .find_map(|(_, e)| match e {
            RuntimeEvent::ToolUpdate(call) => Some(call.clone()),
            _ => None,
        })
        .expect("tool completion");
    assert_eq!(update.status.as_deref(), Some("completed"));
    assert_eq!(
        update.content.as_ref().unwrap()[0]["content"]["text"],
        "ok\n"
    );
    assert_eq!(update.raw_output.as_ref().unwrap()["exit_code"], 0);
    assert!(events.iter().any(|(_, e)| matches!(
        e,
        RuntimeEvent::Usage { used: 1200, size: 200000, .. }
    )));
    // Unmapped notifications are counted by category only.
    drop(events);
    let diagnostics = runtime.diagnostics().protocol;
    assert!(diagnostics
        .ignored_notification_methods
        .iter()
        .any(|d| d.category == "account/updated" && d.count == 1));
}

#[tokio::test]
async fn codex_completed_item_without_started_is_announced_first() {
    let (runtime, mut server, recorder) = harness();
    initialized(&runtime, &mut server).await;
    started_thread(&runtime, &mut server, "th-1", ASK).await;
    let (handle, _) = begin_turn(&runtime, &mut server, "th-1", "turn-1").await;
    server
        .notify(
            "item/completed",
            json!({"threadId": "th-1", "turnId": "turn-1", "completedAtMs": 2, "item": {
                "type": "mcpToolCall", "id": "t1", "server": "broker", "tool": "ping",
                "status": "completed", "arguments": {"a": 1},
                "result": {"content": [{"type": "text", "text": "pong"}]}}}),
        )
        .await;
    server
        .notify(
            "item/completed",
            json!({"threadId": "th-1", "turnId": "turn-1", "completedAtMs": 3, "item": {
                "type": "agentMessage", "id": "m1", "text": "only completed text"}}),
        )
        .await;
    complete_turn(&mut server, "th-1", "turn-1", "completed").await;
    settle(handle).await;
    let events = recorder.events.lock().unwrap();
    assert!(matches!(&events[0].1, RuntimeEvent::ToolCall(call)
        if call.title.as_deref() == Some("mcp.broker.ping")
            && call.raw_input.as_ref().unwrap()["server"] == "broker"));
    // Without streamed deltas the completed text is the only source and is used once.
    drop(events);
    assert_eq!(recorder.texts(), "only completed text");
}

#[tokio::test]
async fn codex_failed_turn_is_failed_not_terminal_success_or_rejected() {
    let (runtime, mut server, _) = harness();
    initialized(&runtime, &mut server).await;
    started_thread(&runtime, &mut server, "th-1", ASK).await;
    let (handle, _) = begin_turn(&runtime, &mut server, "th-1", "turn-1").await;
    server
        .notify(
            "error",
            json!({"threadId": "th-1", "turnId": "turn-1", "willRetry": true,
                   "error": {"message": "transient"}}),
        )
        .await;
    server
        .notify(
            "turn/completed",
            json!({"threadId": "th-1", "turn": {"id": "turn-1", "status": "failed", "items": [],
                   "error": {"message": "usage limit reached"}}}),
        )
        .await;
    match settle(handle).await {
        TurnOutcome::Failed(RuntimeError::Provider { message, .. }) => {
            assert_eq!(message, "usage limit reached")
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn codex_second_turn_cannot_start_while_one_runs() {
    let (runtime, mut server, _) = harness();
    initialized(&runtime, &mut server).await;
    started_thread(&runtime, &mut server, "th-1", ASK).await;
    let (handle, _) = begin_turn(&runtime, &mut server, "th-1", "turn-1").await;
    let outcome = runtime
        .send_turn("th-1", vec![RuntimeContent::text("again")])
        .await;
    assert!(matches!(outcome, TurnOutcome::NotSent(RuntimeError::Unsupported(_))));
    complete_turn(&mut server, "th-1", "turn-1", "completed").await;
    settle(handle).await;
}

#[tokio::test]
async fn codex_turn_start_error_classification_never_overclaims() {
    let (runtime, mut server, _) = harness();
    initialized(&runtime, &mut server).await;
    started_thread(&runtime, &mut server, "th-1", ASK).await;

    for (code, expect_rejected) in [(-32602, true), (-32603, false)] {
        let rt = runtime.clone();
        let handle = tokio::spawn(async move {
            rt.send_turn("th-1", vec![RuntimeContent::text("x")]).await
        });
        let request = server.expect("turn/start").await;
        server.fail(&request, code, "nope").await;
        match (settle(handle).await, expect_rejected) {
            (TurnOutcome::Rejected(_), true) | (TurnOutcome::Unknown(_), false) => {}
            (other, _) => panic!("code {code}: {other:?}"),
        }
    }
    // A failed start releases the thread: the next send is accepted by the transport again.
    let (handle, _) = begin_turn(&runtime, &mut server, "th-1", "turn-9").await;
    complete_turn(&mut server, "th-1", "turn-9", "completed").await;
    assert_eq!(settle(handle).await, TurnOutcome::Terminal(TurnTerminal::EndTurn));
}

#[tokio::test]
async fn codex_disconnect_after_queueing_is_unknown_and_later_sends_are_not_sent() {
    let (runtime, mut server, _) = harness();
    initialized(&runtime, &mut server).await;
    started_thread(&runtime, &mut server, "th-1", ASK).await;
    let rt = runtime.clone();
    let handle = tokio::spawn(async move {
        rt.send_turn("th-1", vec![RuntimeContent::text("x")]).await
    });
    server.expect("turn/start").await;
    // The provider disappears without answering: the turn may have started.
    let Server { lines, out } = server;
    drop(lines);
    drop(out);
    let outcome = settle(handle).await;
    assert!(matches!(outcome, TurnOutcome::Unknown(RuntimeError::Closed)), "{outcome:?}");
    let again = runtime
        .send_turn("th-1", vec![RuntimeContent::text("y")])
        .await;
    assert!(matches!(again, TurnOutcome::NotSent(_)), "{again:?}");
    assert!(runtime.request_stop("th-1").is_err());
}

#[tokio::test]
async fn codex_disconnect_mid_turn_is_unknown() {
    let (runtime, mut server, _) = harness();
    initialized(&runtime, &mut server).await;
    started_thread(&runtime, &mut server, "th-1", ASK).await;
    let (handle, _) = begin_turn(&runtime, &mut server, "th-1", "turn-1").await;
    let Server { lines, out } = server;
    drop(lines);
    drop(out);
    let outcome = settle(handle).await;
    assert!(matches!(outcome, TurnOutcome::Unknown(RuntimeError::Closed)), "{outcome:?}");
}

// ---- Codex: steer / stop -----------------------------------------------------------------------

#[tokio::test]
async fn codex_steer_requires_expected_turn_and_receipt_must_match() {
    let (runtime, mut server, _) = harness();
    initialized(&runtime, &mut server).await;
    started_thread(&runtime, &mut server, "th-1", ASK).await;

    // No running turn: nothing is sent.
    let none = runtime
        .steer("th-1", vec![RuntimeContent::text("redirect")])
        .await;
    assert!(matches!(none, SteerOutcome::NotSent(_)));

    let (handle, _) = begin_turn(&runtime, &mut server, "th-1", "turn-1").await;
    let rt = runtime.clone();
    let steer = tokio::spawn(async move {
        rt.steer("th-1", vec![RuntimeContent::text("redirect")]).await
    });
    let request = server.expect("turn/steer").await;
    assert_eq!(request["params"]["expectedTurnId"], "turn-1");
    assert_eq!(request["params"]["threadId"], "th-1");
    assert_eq!(request["params"]["input"][0]["text"], "redirect");
    server.reply(&request, json!({"turnId": "turn-1"})).await;
    assert!(matches!(
        settle(steer).await,
        SteerOutcome::Delivered { ref outcome } if outcome == "accepted"
    ));

    // A receipt for another turn proves nothing about ours.
    let rt = runtime.clone();
    let steer = tokio::spawn(async move {
        rt.steer("th-1", vec![RuntimeContent::text("again")]).await
    });
    let request = server.expect("turn/steer").await;
    server.reply(&request, json!({"turnId": "turn-other"})).await;
    assert!(matches!(settle(steer).await, SteerOutcome::Unknown(_)));

    // A precondition error is a definite rejection; an internal error is not.
    for (code, rejected) in [(-32600, true), (-32603, false)] {
        let rt = runtime.clone();
        let steer = tokio::spawn(async move {
            rt.steer("th-1", vec![RuntimeContent::text("x")]).await
        });
        let request = server.expect("turn/steer").await;
        server.fail(&request, code, "not steerable").await;
        match (settle(steer).await, rejected) {
            (SteerOutcome::Rejected(_), true) | (SteerOutcome::Unknown(_), false) => {}
            (other, _) => panic!("{code}: {other:?}"),
        }
    }
    complete_turn(&mut server, "th-1", "turn-1", "completed").await;
    settle(handle).await;
}

#[tokio::test]
async fn codex_stop_interrupts_and_only_turn_completed_releases_the_writer() {
    let (runtime, mut server, recorder) = harness();
    initialized(&runtime, &mut server).await;
    started_thread(&runtime, &mut server, "th-1", ASK).await;
    let (handle, _) = begin_turn(&runtime, &mut server, "th-1", "turn-1").await;

    runtime.request_stop("th-1").unwrap();
    let request = server.expect("turn/interrupt").await;
    assert_eq!(request["params"], json!({"threadId": "th-1", "turnId": "turn-1"}));
    // The empty acknowledgement is not a terminal.
    server.reply(&request, json!({})).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!handle.is_finished(), "ack must not settle the turn");

    server
        .notify(
            "item/agentMessage/delta",
            json!({"threadId": "th-1", "turnId": "turn-1", "itemId": "m1", "delta": "partial"}),
        )
        .await;
    complete_turn(&mut server, "th-1", "turn-1", "interrupted").await;
    assert_eq!(settle(handle).await, TurnOutcome::Terminal(TurnTerminal::Cancelled));
    assert_eq!(recorder.texts(), "partial");
    // Idle stop is a no-op, not an error.
    runtime.request_stop("th-1").unwrap();
}

#[tokio::test]
async fn codex_stop_before_turn_id_is_known_interrupts_on_adoption() {
    let (runtime, mut server, _) = harness();
    initialized(&runtime, &mut server).await;
    started_thread(&runtime, &mut server, "th-1", ASK).await;

    let rt = runtime.clone();
    let handle = tokio::spawn(async move {
        rt.send_turn("th-1", vec![RuntimeContent::text("x")]).await
    });
    let start = server.expect("turn/start").await;
    runtime.request_stop("th-1").unwrap();
    // No turn id yet, so no interrupt can be addressed.
    let none = tokio::time::timeout(Duration::from_millis(100), server.lines.next_line()).await;
    assert!(none.is_err());
    server
        .reply(&start, json!({"turn": turn_json("turn-1", "inProgress")}))
        .await;
    let interrupt = server.expect("turn/interrupt").await;
    assert_eq!(interrupt["params"]["turnId"], "turn-1");
    server.reply(&interrupt, json!({})).await;
    complete_turn(&mut server, "th-1", "turn-1", "interrupted").await;
    assert_eq!(settle(handle).await, TurnOutcome::Terminal(TurnTerminal::Cancelled));
}

// ---- Codex: approvals and questions ------------------------------------------------------------

#[tokio::test]
async fn codex_command_approval_parks_the_turn_and_maps_outcomes_conservatively() {
    let (runtime, mut server, recorder) = harness();
    initialized(&runtime, &mut server).await;
    started_thread(&runtime, &mut server, "th-1", ASK).await;
    let (handle, _) = begin_turn(&runtime, &mut server, "th-1", "turn-1").await;

    let cases = [
        (RuntimePermissionOutcome::Selected("accept".into()), "accept"),
        (
            RuntimePermissionOutcome::Selected("acceptForSession".into()),
            "acceptForSession",
        ),
        (RuntimePermissionOutcome::Selected("decline".into()), "decline"),
        // Never an approval: unknown id, and dismissal.
        (RuntimePermissionOutcome::Selected("made-up".into()), "decline"),
        (RuntimePermissionOutcome::Cancelled, "cancel"),
    ];
    for (index, (outcome, expected)) in cases.into_iter().enumerate() {
        recorder.permission_script.lock().unwrap().push_back(outcome);
        server
            .send(json!({"id": format!("srv-{index}"), "method": "item/commandExecution/requestApproval",
                "params": {"threadId": "th-1", "turnId": "turn-1", "itemId": "c1",
                           "command": "rm -rf build", "startedAtMs": 1, "reason": "needs write"}}))
            .await;
        let response = server.read().await;
        assert_eq!(response["id"], format!("srv-{index}"));
        assert_eq!(response["result"]["decision"], expected);
    }
    let permissions = recorder.permissions.lock().unwrap();
    assert_eq!(permissions[0].tool_call["toolCallId"], "c1");
    assert_eq!(permissions[0].tool_call["kind"], "execute");
    assert_eq!(permissions[0].tool_call["title"], "rm -rf build");
    let kinds = permissions[0]
        .options
        .iter()
        .map(|o| o.kind.as_str())
        .collect::<Vec<_>>();
    assert_eq!(kinds, ["allow_once", "allow_always", "reject_once"]);
    drop(permissions);
    complete_turn(&mut server, "th-1", "turn-1", "completed").await;
    settle(handle).await;
}

#[tokio::test]
async fn codex_unknown_thread_requests_are_denied_without_asking_the_host() {
    let (runtime, mut server, recorder) = harness();
    initialized(&runtime, &mut server).await;
    server
        .send(json!({"id": 1, "method": "item/commandExecution/requestApproval",
            "params": {"threadId": "ghost", "turnId": "t", "itemId": "c", "startedAtMs": 1}}))
        .await;
    let response = server.read().await;
    assert_eq!(response["result"]["decision"], "cancel");
    server
        .send(json!({"id": 2, "method": "item/permissions/requestApproval",
            "params": {"threadId": "ghost", "turnId": "t", "itemId": "c", "cwd": "/",
                       "permissions": {"network": {"enabled": true}}, "startedAtMs": 1}}))
        .await;
    assert_eq!(server.read().await["result"]["permissions"], json!({}));
    assert!(recorder.permissions.lock().unwrap().is_empty());
}

#[tokio::test]
async fn codex_permission_profile_grants_only_on_explicit_accept() {
    let (runtime, mut server, recorder) = harness();
    initialized(&runtime, &mut server).await;
    started_thread(&runtime, &mut server, "th-1", ASK).await;
    let requested = json!({"network": {"enabled": true}});
    for (outcome, granted) in [
        (RuntimePermissionOutcome::Selected("accept".into()), true),
        (RuntimePermissionOutcome::Selected("acceptForSession".into()), false),
        (RuntimePermissionOutcome::Cancelled, false),
    ] {
        recorder.permission_script.lock().unwrap().push_back(outcome);
        server
            .send(json!({"id": 9, "method": "item/permissions/requestApproval",
                "params": {"threadId": "th-1", "turnId": "t", "itemId": "p1", "cwd": "/work",
                           "permissions": requested, "startedAtMs": 1}}))
            .await;
        let response = server.read().await;
        if granted {
            assert_eq!(response["result"]["permissions"], requested);
            assert_eq!(response["result"]["scope"], "turn");
        } else {
            assert_eq!(response["result"]["permissions"], json!({}));
        }
    }
    // `acceptForSession` was never offered for a permission grant.
    let permissions = recorder.permissions.lock().unwrap();
    assert!(permissions[0].options.iter().all(|o| o.id != "acceptForSession"));
}

#[tokio::test]
async fn codex_questions_and_mcp_elicitation_use_the_host_and_default_to_no_answer() {
    let (runtime, mut server, recorder) = harness();
    initialized(&runtime, &mut server).await;
    started_thread(&runtime, &mut server, "th-1", ASK).await;

    // request_user_input → form → {id: {answers: [..]}}
    let mut answer = serde_json::Map::new();
    answer.insert("q1".into(), json!("Blue"));
    recorder
        .question_script
        .lock()
        .unwrap()
        .push_back(RuntimeQuestionOutcome::Answered(answer));
    server
        .send(json!({"id": 1, "method": "item/tool/requestUserInput",
            "params": {"threadId": "th-1", "turnId": "t", "itemId": "i1", "isBlocking": true,
                "questions": [{"id": "q1", "header": "Color", "question": "Pick one",
                    "options": [{"label": "Red", "description": "r"}, {"label": "Blue", "description": "b"}]}]}}))
        .await;
    let response = server.read().await;
    assert_eq!(response["result"]["answers"]["q1"]["answers"], json!(["Blue"]));
    let question = recorder.questions.lock().unwrap()[0].clone();
    assert_eq!(question.schema["properties"]["q1"]["title"], "Color");
    assert_eq!(question.schema["properties"]["q1"]["oneOf"][1]["const"], "Blue");
    assert_eq!(question.schema["required"], json!(["q1"]));

    // Unanswered (declined) leaves Codex with an empty answer set, not an invented one.
    server
        .send(json!({"id": 2, "method": "item/tool/requestUserInput",
            "params": {"threadId": "th-1", "turnId": "t", "itemId": "i2", "isBlocking": true,
                       "questions": [{"id": "q", "header": "h", "question": "?"}]}}))
        .await;
    assert_eq!(server.read().await["result"]["answers"], json!({}));

    // MCP tool approval flavour goes through permissions, flagged for Engine's classifier.
    recorder
        .permission_script
        .lock()
        .unwrap()
        .push_back(RuntimePermissionOutcome::Selected("decline".into()));
    server
        .send(json!({"id": 3, "method": "mcpServer/elicitation/request",
            "params": {"threadId": "th-1", "mode": "form", "message": "Allow broker.ping?",
                "requestedSchema": {"type": "object", "properties": {}},
                "_meta": {"codex_approval_kind": "mcp_tool_call"}}}))
        .await;
    assert_eq!(server.read().await["result"]["action"], "decline");
    let permissions = recorder.permissions.lock().unwrap();
    assert_eq!(
        permissions.last().unwrap().meta.as_ref().unwrap()["is_mcp_tool_approval"],
        true
    );
    drop(permissions);

    // URL-mode elicitation is never answered with data.
    server
        .send(json!({"id": 4, "method": "mcpServer/elicitation/request",
            "params": {"threadId": "th-1", "mode": "url", "message": "log in",
                       "elicitationId": "e", "url": "https://x.test"}}))
        .await;
    assert_eq!(server.read().await["result"]["action"], "decline");

    // Host-owned unsupported surfaces answer truthfully instead of pretending success.
    server
        .send(json!({"id": 5, "method": "item/tool/call",
            "params": {"threadId": "th-1", "turnId": "t", "callId": "c", "tool": "x", "arguments": {}}}))
        .await;
    assert_eq!(server.read().await["result"]["success"], false);
    server
        .send(json!({"id": 6, "method": "account/chatgptAuthTokens/refresh",
            "params": {"reason": "unauthorized"}}))
        .await;
    assert!(server.read().await["error"]["message"]
        .as_str()
        .unwrap()
        .contains("credentials"));
    server
        .send(json!({"id": 7, "method": "no/such/method", "params": {}}))
        .await;
    assert_eq!(server.read().await["error"]["code"], -32601);
}

// ---- Codex: model / config / policy ------------------------------------------------------------

#[tokio::test]
async fn codex_model_effort_and_policy_changes_apply_to_the_next_turn_only_when_valid() {
    let (runtime, mut server, _) = harness();
    initialized(&runtime, &mut server).await;
    started_thread(&runtime, &mut server, "th-1", ASK).await;

    // Unlisted model: refused, never approximated.
    assert!(matches!(
        runtime.set_model("th-1", "no-such-model").await,
        Err(RuntimeError::Unsupported(_))
    ));
    // Flat catalogue variant `model[effort]` is split, and the effort is validated.
    assert!(matches!(
        runtime.set_model("th-1", "gpt-x[ultra]").await,
        Err(RuntimeError::Unsupported(_))
    ));
    runtime.set_model("th-1", "gpt-x[high]").await.unwrap();
    let options = runtime
        .set_config_option("th-1", "reasoning_effort", "low")
        .await
        .unwrap();
    assert_eq!(options[0].current, "low");
    assert!(runtime
        .set_config_option("th-1", "reasoning_effort", "ultra")
        .await
        .is_err());
    assert!(runtime.set_config_option("th-1", "verbosity", "x").await.is_err());
    assert!(runtime.set_mode("th-1", "plan").await.is_err());

    runtime.set_execution_policy(
        "th-1",
        ExecutionPolicy {
            mode: PermissionMode::Yolo,
            sandbox: SandboxPolicy::DangerFullAccess,
        },
    );
    let (handle, request) = begin_turn(&runtime, &mut server, "th-1", "turn-1").await;
    let params = &request["params"];
    assert_eq!(params["model"], "gpt-x");
    assert_eq!(params["effort"], "low");
    assert_eq!(params["approvalPolicy"], "on-request");
    assert_eq!(params["sandboxPolicy"]["type"], "dangerFullAccess");
    // Models cannot change mid-turn.
    assert!(runtime.set_model("th-1", "plain").await.is_err());
    complete_turn(&mut server, "th-1", "turn-1", "completed").await;
    settle(handle).await;

    // Switching to a model without the current effort falls back to that model's own default.
    runtime.set_model("th-1", "plain").await.unwrap();
    let (handle, request) = begin_turn(&runtime, &mut server, "th-1", "turn-2").await;
    assert_eq!(request["params"]["model"], "plain");
    assert_eq!(request["params"]["effort"], "none");
    complete_turn(&mut server, "th-1", "turn-2", "completed").await;
    settle(handle).await;
}

// ---- Codex: pure mapping -----------------------------------------------------------------------

#[test]
fn codex_item_projection_covers_tool_like_items_and_ignores_messages() {
    let file = item_to_tool_call(&json!({
        "type": "fileChange", "id": "f1", "status": "inProgress",
        "changes": [{"path": "a.rs", "diff": "", "kind": {"type": "update", "move_path": null}}]
    }))
    .unwrap();
    assert_eq!(file.title.as_deref(), Some("Edit a.rs"));
    assert_eq!(file.kind.as_deref(), Some("edit"));
    assert_eq!(file.status.as_deref(), Some("in_progress"));

    let deletes = item_to_tool_call(&json!({
        "type": "fileChange", "id": "f2", "status": "completed",
        "changes": [{"path": "a", "diff": "", "kind": {"type": "delete"}},
                    {"path": "b", "diff": "", "kind": {"type": "delete"}}]
    }))
    .unwrap();
    assert_eq!(deletes.kind.as_deref(), Some("delete"));
    assert_eq!(deletes.title.as_deref(), Some("Edit 2 files"));

    // A declined command never ran: closed and unsuccessful.
    let declined = item_to_tool_call(&json!({
        "type": "commandExecution", "id": "c", "command": "x", "status": "declined",
        "cwd": "/", "commandActions": []
    }))
    .unwrap();
    assert_eq!(declined.status.as_deref(), Some("failed"));

    let mcp_error = item_to_tool_call(&json!({
        "type": "mcpToolCall", "id": "m", "server": "s", "tool": "t", "status": "failed",
        "arguments": {}, "error": {"message": "boom"}
    }))
    .unwrap();
    assert_eq!(mcp_error.status.as_deref(), Some("failed"));
    assert_eq!(mcp_error.content.unwrap()[0]["content"]["text"], "boom");

    let image = item_to_tool_call(&json!({
        "type": "imageGeneration", "id": "i", "status": "completed", "result": ""
    }))
    .unwrap();
    assert_eq!(image.title.as_deref(), Some("Image generation"));

    for ignored in [
        json!({"type": "agentMessage", "id": "a", "text": "hi"}),
        json!({"type": "reasoning", "id": "r"}),
        json!({"type": "userMessage", "id": "u", "content": []}),
        json!({"type": "plan", "id": "p", "text": ""}),
        json!({"type": "contextCompaction", "id": "k"}),
    ] {
        assert!(item_to_tool_call(&ignored).is_none(), "{ignored}");
    }
}

#[test]
fn codex_user_input_encodes_text_and_inline_images() {
    let input = codex::user_input(vec![
        RuntimeContent::text("see"),
        RuntimeContent::Image {
            data: "QUJD".into(),
            mime_type: "image/png".into(),
        },
    ]);
    assert_eq!(input[0]["type"], "text");
    assert_eq!(input[1]["url"], "data:image/png;base64,QUJD");
}

// ---- real binary (no model turn) ---------------------------------------------------------------

/// Needs the installed `codex` CLI. Starts a real app-server, initializes it and creates an
/// ephemeral thread; no prompt is sent, so no model request or usage is incurred.
#[tokio::test]
#[ignore = "needs the installed codex CLI; run with --ignored"]
async fn codex_real_app_server_initializes_and_creates_a_thread_without_a_turn() {
    let Some(launch) = codex::CodexLaunch::discover() else {
        eprintln!("codex not installed; skipping");
        return;
    };
    let recorder = Arc::new(Recorder::default());
    let runtime = codex::launch(&launch, recorder).await.unwrap();
    let init = runtime.initialize().await.unwrap();
    assert_eq!(init.identity.backend, RuntimeBackendKind::CodexAppServer);
    assert!(init.identity.adapter_version.is_some());
    let state = runtime
        .start_session(RuntimeSessionStart {
            cwd: std::env::temp_dir().to_string_lossy().into_owned(),
            mcp_servers: Vec::new(),
            execution: ASK,
        })
        .await
        .unwrap();
    assert!(!state.backend_session_id.is_empty());
    let models = state.models.expect("model catalogue");
    assert!(!models.available.is_empty());
    assert!(!models.current.is_empty());
    // Stop on an idle thread is a no-op, and terminate leaves no process behind.
    runtime.request_stop(&state.backend_session_id).unwrap();
    runtime.terminate();
    assert!(runtime.diagnostics().process.termination_requested);
}


// ---- SDK sidecar: real child process ------------------------------------------------------------
//
// These run the real `script/provider-sidecars/claude/sidecar.mjs` under Node with a fake SDK
// module, so the process boundary, line framing, shutdown and the Rust `SidecarRuntime` are
// exercised together. No model, credential or network is involved, and they prove the contract,
// not Claude behavior.

mod sidecar_process {
    use super::*;
    use codetwo_core::connectors::sidecar::{self, SidecarLaunch};
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn node() -> Option<PathBuf> {
        let output = std::process::Command::new("node").arg("--version").output().ok()?;
        output.status.success().then(|| PathBuf::from("node"))
    }

    fn launch_spec(sdk_module: &str) -> Option<SidecarLaunch> {
        let runtime = node()?;
        let root = repo_root().join("script/provider-sidecars");
        Some(SidecarLaunch {
            backend: RuntimeBackendKind::ClaudeAgentSdk,
            runtime,
            args: vec![root.join("claude/sidecar.mjs").to_string_lossy().into_owned()],
            env: vec![(
                "CODETWO_SIDECAR_SDK_MODULE".into(),
                if sdk_module.starts_with('/') {
                    sdk_module.to_string()
                } else {
                    root.join(sdk_module).to_string_lossy().into_owned()
                },
            )],
            cwd: None,
            client_version: "test".into(),
        })
    }

    fn text(value: &str) -> Vec<RuntimeContent> {
        vec![RuntimeContent::Text(value.into())]
    }

    #[tokio::test]
    async fn real_sidecar_process_roundtrip() {
        let Some(spec) = launch_spec("common/fake-claude-sdk.mjs") else {
            panic!("node is required for the sidecar process tests");
        };
        let recorder = Arc::new(Recorder::default());
        let runtime = sidecar::launch(&spec, recorder.clone()).await.unwrap();
        let init = runtime.initialize().await.unwrap();
        assert_eq!(init.identity.backend, RuntimeBackendKind::ClaudeAgentSdk);
        assert_eq!(
            init.identity.adapter_name.as_deref(),
            Some("@anthropic-ai/claude-agent-sdk")
        );
        assert_eq!(init.capabilities.steering, SteerSupport::Unsupported);
        assert_eq!(init.capabilities.stop, StopSupport::VerifiedTerminal);

        let state = runtime
            .start_session(RuntimeSessionStart {
                cwd: std::env::temp_dir().to_string_lossy().into_owned(),
                mcp_servers: Vec::new(),
                execution: ASK,
            })
            .await
            .unwrap();
        assert!(!state.backend_session_id.is_empty());
        assert_eq!(state.models.as_ref().unwrap().available[0].id, "fake");

        // A plain turn: streamed text, then the SDK's own terminal.
        let outcome = runtime
            .send_turn(&state.backend_session_id, text("hello"))
            .await;
        assert!(matches!(outcome, TurnOutcome::Terminal(TurnTerminal::EndTurn)), "{outcome:?}");
        assert_eq!(recorder.texts(), "Hello from fake");

        // A tool turn: the approval crosses the process boundary and back.
        recorder
            .permission_script
            .lock()
            .unwrap()
            .push_back(RuntimePermissionOutcome::Selected("allow".into()));
        let outcome = runtime
            .send_turn(&state.backend_session_id, text("use a tool"))
            .await;
        assert!(matches!(outcome, TurnOutcome::Terminal(TurnTerminal::EndTurn)), "{outcome:?}");
        let permissions = recorder.permissions.lock().unwrap();
        assert_eq!(permissions.len(), 1);
        assert_eq!(permissions[0].tool_call["kind"], "execute");
        assert_eq!(permissions[0].tool_call["title"], "echo hi");
        drop(permissions);

        // A dismissed approval is a denial, never an approval.
        let outcome = runtime
            .send_turn(&state.backend_session_id, text("another tool"))
            .await;
        assert!(matches!(outcome, TurnOutcome::Terminal(TurnTerminal::EndTurn)));
        let tool_results: Vec<_> = recorder
            .events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(_, e)| match e {
                RuntimeEvent::ToolUpdate(call) => call.status.clone(),
                _ => None,
            })
            .collect();
        assert_eq!(tool_results, ["completed", "failed"]);

        runtime.terminate();
    }

    #[tokio::test]
    async fn stop_is_a_request_and_the_turn_ends_from_the_sdk_terminal() {
        let Some(spec) = launch_spec("common/fake-claude-sdk.mjs") else {
            panic!("node is required for the sidecar process tests");
        };
        let recorder = Arc::new(Recorder::default());
        let runtime = sidecar::launch(&spec, recorder).await.unwrap();
        runtime.initialize().await.unwrap();
        let state = runtime
            .start_session(RuntimeSessionStart {
                cwd: std::env::temp_dir().to_string_lossy().into_owned(),
                mcp_servers: Vec::new(),
                execution: ASK,
            })
            .await
            .unwrap();
        let session = state.backend_session_id.clone();
        let turn = {
            let runtime = runtime.clone();
            let session = session.clone();
            tokio::spawn(async move { runtime.send_turn(&session, text("hang")).await })
        };
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!turn.is_finished(), "turn must not end without a terminal");
        runtime.request_stop(&session).unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(5), turn)
            .await
            .expect("terminal arrives after stop")
            .unwrap();
        assert!(matches!(outcome, TurnOutcome::Terminal(TurnTerminal::Cancelled)), "{outcome:?}");
        runtime.terminate();
    }

    #[tokio::test]
    async fn a_missing_sdk_fails_initialize_before_any_session() {
        let Some(spec) = launch_spec("/nonexistent/sdk.mjs") else {
            panic!("node is required for the sidecar process tests");
        };
        let runtime = sidecar::launch(&spec, Arc::new(Recorder::default()))
            .await
            .unwrap();
        let error = runtime.initialize().await.unwrap_err();
        assert!(error.to_string().contains("could not be loaded"), "{error}");
        runtime.terminate();
    }

    #[tokio::test]
    async fn a_sidecar_that_dies_mid_turn_leaves_the_outcome_unknown_and_never_replays() {
        let Some(spec) = launch_spec("common/fake-claude-sdk.mjs") else {
            panic!("node is required for the sidecar process tests");
        };
        let runtime = sidecar::launch(&spec, Arc::new(Recorder::default()))
            .await
            .unwrap();
        runtime.initialize().await.unwrap();
        let state = runtime
            .start_session(RuntimeSessionStart {
                cwd: std::env::temp_dir().to_string_lossy().into_owned(),
                mcp_servers: Vec::new(),
                execution: ASK,
            })
            .await
            .unwrap();
        let session = state.backend_session_id;
        let outcome = runtime.send_turn(&session, text("crash now")).await;
        assert!(matches!(outcome, TurnOutcome::Unknown(_)), "{outcome:?}");
        // The prompt reached the process, so it is never re-sent; a later send is provably unsent.
        let later = runtime.send_turn(&session, text("again")).await;
        assert!(matches!(later, TurnOutcome::NotSent(_)), "{later:?}");
        runtime.terminate();
    }
}

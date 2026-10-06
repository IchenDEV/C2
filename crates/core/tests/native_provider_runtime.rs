//! Contract tests for the provider-neutral runtime boundary through its ACP adapter, driven over
//! in-process loopback pipes (no provider binary). They pin the honest transport outcomes:
//! terminal, rejected, not-sent, unknown, steering receipts, stop-as-request, and capability
//! identity.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use codetwo_core::acp::{AcpClient, Connection, RecordingHandler};
use codetwo_core::connectors::acp::AcpRuntime;
use codetwo_core::{
    ProviderRuntime, RuntimeBackendKind, RuntimeContent, RuntimeError, RuntimeSessionRestore,
    RuntimeSessionStart, SteerOutcome, SteerSupport, StopSupport, Support, TurnOutcome,
    TurnTerminal,
};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

async fn write_line<W: AsyncWrite + Unpin>(w: &mut W, v: Value) {
    let mut s = v.to_string();
    s.push('\n');
    w.write_all(s.as_bytes()).await.unwrap();
    w.flush().await.unwrap();
}

// Real Engine/Store/delivery path over a local ACP protocol fixture. A worker thread holds
// exactly one selected operation; the fixture keeps reading control requests while it waits.
const ENGINE_AGENT: &str = r#"
import json,sys,pathlib,threading,time
stage=sys.argv[1]; root=pathlib.Path(sys.argv[2]); output=threading.Lock(); pending=None
def write(mid,result):
    with output: print(json.dumps({'jsonrpc':'2.0','id':mid,'result':result}),flush=True)
def hold(mid,result,method):
    (root/method).touch()
    while not (root/'release').exists(): time.sleep(.005)
    write(mid,result)
for line in sys.stdin:
    m=json.loads(line); method=m.get('method'); mid=m.get('id')
    if method=='initialize': result={'protocolVersion':1,'_meta':{'steering':{'supported':True}}}
    elif method=='session/new':
        result={'sessionId':'fixture-session'}
        if stage=='create':
            threading.Thread(target=hold,args=(mid,result,'create'),daemon=True).start(); continue
    elif method=='session/prompt':
        if stage=='fast': (root/'prompt').touch(); result={'stopReason':'end_turn'}
        else: pending=mid; (root/'prompt').touch(); continue
    elif method=='_session/steering':
        with (root/'steer-count').open('a') as f: f.write('send\n')
        if stage=='malformed': (root/'steer').touch(); write(mid,{}); continue
        threading.Thread(target=hold,args=(mid,{'outcome':'injected'},'steer'),daemon=True).start(); continue
    elif method=='session/cancel':
        (root/'cancel').touch()
        if pending is not None: write(pending,{'stopReason':'cancelled'})
        pending=None; continue
    elif mid is None: continue
    else: result={}
    write(mid,result)
"#;

struct EngineOwner(codetwo_core::Engine);

impl Drop for EngineOwner {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

async fn wait_until(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !predicate() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("fixture did not reach its expected boundary");
}

async fn engine_slow_operation_is_independent(stage: &str) {
    use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
    use codetwo_core::skill::{DocBlock, SkillLibrary};
    use codetwo_core::{Engine, Event, Op, Store};
    use std::time::{Duration, Instant};

    let root = tempfile::tempdir().unwrap();
    let a_root = root.path().join("a");
    let b_root = root.path().join("b");
    std::fs::create_dir(&a_root).unwrap();
    std::fs::create_dir(&b_root).unwrap();
    let provider = |name: &str, stage: &str, cwd: &std::path::Path| Provider {
        id: ProviderId::Custom(name.into()),
        display_name: name.into(),
        needs_node: false,
        launch: LaunchSpec {
            command: "python3".into(),
            args: vec![
                "-c".into(),
                ENGINE_AGENT.into(),
                stage.into(),
                cwd.to_string_lossy().into(),
            ],
            env: vec![],
            cwd: None,
        },
    };
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (engine, mut events) = Engine::with_store(
        vec![
            provider("a", stage, &a_root),
            provider("b", "fast", &b_root),
        ],
        SkillLibrary::default(),
        store.clone(),
    );
    let _owner = EngineOwner(engine.clone());
    let mut sessions = vec![];
    for name in ["a", "b"] {
        engine
            .submit(Op::NewSession {
                provider: ProviderId::Custom(name.into()),
                cwd: root.path().to_string_lossy().into(),
                use_worktree: false,
                worktree_base: None,
                worktree_base_sha: None,
                request_id: Some(format!("create-{name}")),
                model: None,
                initial_policy: None,
            })
            .await
            .unwrap();
        let session = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match events.recv().await.expect("Engine event channel closed") {
                    Event::SessionCreated { session, .. } => break session,
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
        .unwrap();
        sessions.push(session);
    }
    let a = sessions[0].clone();
    let b = sessions[1].clone();
    let a_engine = engine.clone();
    let a_session = a.clone();
    let a_prompt = tokio::spawn(async move {
        a_engine
            .submit(Op::Prompt {
                session: a_session,
                doc: vec![DocBlock::Text {
                    text: "A holds an operation".into(),
                }],
                request_id: Some("a-prompt".into()),
            })
            .await
    });
    wait_until(|| {
        a_root
            .join(if stage == "create" {
                "create"
            } else {
                "prompt"
            })
            .exists()
    })
    .await;
    if matches!(stage, "steer" | "malformed") {
        engine
            .deliver_prompt(
                &a,
                vec![DocBlock::Text {
                    text: "A correction".into(),
                }],
                "steer",
                Some("a-steer".into()),
                None,
            )
            .await
            .unwrap();
        wait_until(|| a_root.join("steer").exists()).await;
        if stage == "malformed" {
            wait_until(|| store.prompt_delivery("a-steer").unwrap().unwrap().state == "unknown")
                .await;
        } else {
            assert_eq!(
                store.prompt_delivery("a-steer").unwrap().unwrap().state,
                "submitting"
            );
        }
    }
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(1),
        engine.deliver_prompt(
            &b,
            vec![DocBlock::Text {
                text: "B is independent".into(),
            }],
            "queue",
            Some("b-delivery".into()),
            None,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    wait_until(|| store.prompt_delivery("b-delivery").unwrap().unwrap().state == "accepted").await;
    assert!(store
        .command_receipt("c2-delivery-prompt", "b-delivery")
        .unwrap()
        .is_some());
    // The durable acknowledgement alone is not proof of backend dispatch or completion.
    wait_until(|| b_root.join("prompt").exists() && engine.current_turn(&b).is_none()).await;
    let b_elapsed = started.elapsed();
    assert!(
        b_elapsed < Duration::from_secs(1),
        "B waited behind A: {b_elapsed:?}"
    );

    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_millis(250),
        engine.submit(Op::Cancel { session: a.clone() }),
    )
    .await
    .expect("Stop waited behind the slow operation")
    .unwrap();
    if stage != "create" {
        wait_until(|| a_root.join("cancel").exists()).await;
    }
    wait_until(|| engine.current_turn(&a).is_none()).await;
    let stop_elapsed = started.elapsed();
    assert!(
        stop_elapsed < Duration::from_secs(1),
        "Stop did not settle: {stop_elapsed:?}"
    );
    tokio::time::timeout(Duration::from_secs(1), a_prompt)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    if matches!(stage, "steer" | "malformed") {
        // A stop request settles the turn but is not evidence that the pending input arrived.
        // Closing the owned transport makes that send unknown; repeated drains must not replay it.
        engine.shutdown();
        wait_until(|| store.prompt_delivery("a-steer").unwrap().unwrap().state == "unknown").await;
        for _ in 0..3 {
            engine.drain_prompt_deliveries().await.unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(a_root.join("steer-count")).unwrap(),
            "send\n"
        );
    }
    println!("Engine {stage}: B receipt={b_elapsed:?}, Stop terminal={stop_elapsed:?}");
}

#[tokio::test]
async fn engine_runtime_slow_create_does_not_block_independent_delivery_or_stop() {
    engine_slow_operation_is_independent("create").await;
}

#[tokio::test]
async fn engine_runtime_slow_prompt_does_not_block_independent_delivery_or_stop() {
    engine_slow_operation_is_independent("prompt").await;
}

#[tokio::test]
async fn engine_runtime_slow_steering_does_not_block_independent_delivery_or_stop() {
    engine_slow_operation_is_independent("steer").await;
}

#[tokio::test]
async fn engine_runtime_unknown_steering_receipt_is_not_replayed() {
    engine_slow_operation_is_independent("malformed").await;
}

/// Scripted agent. `prompt_behavior`: "end" answers end_turn, "cancel" waits for session/cancel
/// then answers cancelled, "error" answers a JSON-RPC error, "hangup" closes the pipe without
/// answering. Records every method it saw on `seen`.
async fn agent<R, W>(
    reader: R,
    mut writer: W,
    prompt_behavior: &'static str,
    seen: Arc<std::sync::Mutex<Vec<String>>>,
) where
    R: tokio::io::AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    let mut prompt_id: Option<Value> = None;
    while let Ok(Some(line)) = lines.next_line().await {
        let v: Value = serde_json::from_str(&line).unwrap();
        let method = v["method"].as_str().unwrap_or_default().to_string();
        seen.lock().unwrap().push(method.clone());
        let id = v["id"].clone();
        match method.as_str() {
            "initialize" => {
                write_line(
                    &mut writer,
                    json!({"jsonrpc":"2.0","id":id,"result":{
                        "protocolVersion":1,
                        "agentInfo":{"name":"loopback","version":"9.9"},
                        "agentCapabilities":{
                            "loadSession":true,
                            "promptCapabilities":{"image":true}
                        },
                        "_meta":{"steering":{"supported":true}}}}),
                )
                .await
            }
            "session/new" => {
                write_line(
                    &mut writer,
                    json!({"jsonrpc":"2.0","id":id,"result":{
                        "sessionId":"backend-1",
                        "models":{"availableModels":[{"modelId":"m1","name":"M1"}],
                                  "currentModelId":"m1"}}}),
                )
                .await
            }
            "session/load" => {
                write_line(&mut writer, json!({"jsonrpc":"2.0","id":id,"result":null})).await
            }
            "_session/steering" => {
                write_line(
                    &mut writer,
                    json!({"jsonrpc":"2.0","id":id,"result":{"outcome":"injected"}}),
                )
                .await
            }
            "session/prompt" => {
                match prompt_behavior {
                    "end" => {
                        write_line(
                            &mut writer,
                            json!({"jsonrpc":"2.0","id":id,"result":{"stopReason":"end_turn"}}),
                        )
                        .await
                    }
                    "error" => write_line(
                        &mut writer,
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":"nope"}}),
                    )
                    .await,
                    "hangup" => return,
                    _ => prompt_id = Some(id),
                }
            }
            "session/cancel" => {
                if let Some(id) = prompt_id.take() {
                    write_line(
                        &mut writer,
                        json!({"jsonrpc":"2.0","id":id,"result":{"stopReason":"cancelled"}}),
                    )
                    .await;
                }
            }
            _ => {}
        }
    }
}

fn runtime(behavior: &'static str) -> (Arc<AcpRuntime>, Arc<std::sync::Mutex<Vec<String>>>) {
    let (client_end, agent_end) = tokio::io::duplex(64 * 1024);
    let (agent_read, agent_write) = tokio::io::split(agent_end);
    let (client_read, client_write) = tokio::io::split(client_end);
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    tokio::spawn(agent(agent_read, agent_write, behavior, seen.clone()));
    let connection = Connection::new(
        client_read,
        client_write,
        Arc::new(RecordingHandler::default()),
    );
    (
        Arc::new(AcpRuntime::new(AcpClient::new(connection, None))),
        seen,
    )
}

fn text(value: &str) -> Vec<RuntimeContent> {
    vec![
        RuntimeContent::text(value),
        RuntimeContent::Image {
            data: "AAAA".into(),
            mime_type: "image/png".into(),
        },
    ]
}

#[tokio::test]
async fn initialize_reports_honest_identity_and_capabilities() {
    let (runtime, _) = runtime("end");
    let before = runtime.negotiated();
    assert_eq!(before.capabilities.steering, SteerSupport::Unsupported);
    assert!(!before.capabilities.resume.any());

    let init = runtime.initialize().await.unwrap();
    assert_eq!(init.identity.backend, RuntimeBackendKind::Acp);
    assert_eq!(init.identity.adapter_name.as_deref(), Some("loopback"));
    assert_eq!(init.identity.adapter_version.as_deref(), Some("9.9"));
    let caps = init.capabilities;
    assert!(caps.resume.load && !caps.resume.resume);
    assert_eq!(caps.steering, SteerSupport::Native);
    // session/cancel is a notification: no terminal acknowledgement is claimed.
    assert_eq!(caps.stop, StopSupport::RequestOnly);
    assert_eq!(caps.image_input, Support::Supported);
    assert_eq!(caps.model_options, Support::Unverified);
    assert!(caps.mcp_stdio && !caps.mcp_http && !caps.mcp_sse);
    assert_eq!(runtime.negotiated(), init);
}

#[tokio::test]
async fn session_start_and_restore_use_neutral_state() {
    let (runtime, seen) = runtime("end");
    runtime.initialize().await.unwrap();
    let state = runtime
        .start_session(RuntimeSessionStart {
            cwd: "/tmp".into(),
            mcp_servers: Vec::new(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(state.backend_session_id, "backend-1");
    let models = state.models.unwrap();
    assert_eq!(models.current, "m1");
    assert_eq!(models.available[0].name, "M1");

    let replaying = AtomicBool::new(false);
    let restored = runtime
        .restore_session(
            RuntimeSessionRestore {
                backend_session_id: "old-backend".into(),
                cwd: "/tmp".into(),
                mcp_servers: Vec::new(),
                ..Default::default()
            },
            &replaying,
        )
        .await
        .unwrap();
    // The provider id is preserved; replay suppression is released afterwards.
    assert_eq!(restored.backend_session_id, "old-backend");
    assert!(!replaying.load(std::sync::atomic::Ordering::SeqCst));
    assert!(seen.lock().unwrap().iter().any(|m| m == "session/load"));
}

#[tokio::test]
async fn completed_turn_is_a_provider_terminal() {
    let (runtime, _) = runtime("end");
    runtime.initialize().await.unwrap();
    assert_eq!(
        runtime.send_turn("backend-1", text("hi")).await,
        TurnOutcome::Terminal(TurnTerminal::EndTurn)
    );
}

#[tokio::test]
async fn provider_error_without_preexecution_proof_is_unknown() {
    let (runtime, _) = runtime("error");
    runtime.initialize().await.unwrap();
    match runtime.send_turn("backend-1", text("hi")).await {
        TurnOutcome::Unknown(RuntimeError::Provider { code, .. }) => assert_eq!(code, -32000),
        other => panic!("expected Unknown, got {other:?}"),
    }
}

#[tokio::test]
async fn connection_loss_after_transmission_is_unknown_not_failed() {
    let (runtime, seen) = runtime("hangup");
    runtime.initialize().await.unwrap();
    let outcome = runtime.send_turn("backend-1", text("hi")).await;
    assert_eq!(outcome, TurnOutcome::Unknown(RuntimeError::Closed));
    // The adapter itself never retries: exactly one prompt reached the agent.
    assert_eq!(
        seen.lock()
            .unwrap()
            .iter()
            .filter(|m| *m == "session/prompt")
            .count(),
        1
    );
}

#[tokio::test]
async fn stop_is_a_request_and_the_turn_reports_its_own_terminal() {
    let (runtime, _) = runtime("cancel");
    runtime.initialize().await.unwrap();
    let turn = {
        let runtime = runtime.clone();
        tokio::spawn(async move { runtime.send_turn("backend-1", text("long")).await })
    };
    // Stop is synchronous and does not wait on the slow turn.
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
    runtime.request_stop("backend-1").unwrap();
    assert_eq!(
        turn.await.unwrap(),
        TurnOutcome::Terminal(TurnTerminal::Cancelled)
    );
}

#[tokio::test]
async fn steering_is_delivery_receipt_and_gated_by_capability() {
    let (runtime, _) = runtime("end");
    // Before initialize nothing is advertised, so steering is refused without sending.
    assert!(matches!(
        runtime.steer("backend-1", text("more")).await,
        SteerOutcome::NotSent(RuntimeError::Unsupported(_))
    ));
    runtime.initialize().await.unwrap();
    assert_eq!(
        runtime.steer("backend-1", text("more")).await,
        SteerOutcome::Delivered {
            outcome: "injected".into()
        }
    );
}

struct ParentBrokenWriter;
impl tokio::io::AsyncWrite for ParentBrokenWriter {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        _: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::task::Poll::Ready(Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "injected write failure",
        )))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn parent_probe_writer_failure_resolves_queued_request_without_reader_eof() {
    let (reader, _keep_peer_open) = tokio::io::duplex(1024);
    let connection = Connection::new(
        reader,
        ParentBrokenWriter,
        Arc::new(RecordingHandler::default()),
    );
    let runtime = AcpRuntime::new(AcpClient::new(connection, None));
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(250),
        runtime.send_turn(
            "backend-1",
            vec![RuntimeContent::text("must become unknown")],
        ),
    )
    .await;
    assert!(matches!(result,Ok(TurnOutcome::Unknown(_))|Ok(TurnOutcome::NotSent(_))),"writer failure must complete the waiting request as unknown/unsent without waiting for unrelated reader EOF; got {result:?}");
}

#[tokio::test]
async fn parent_probe_ambiguous_rpc_error_is_not_proof_of_rejection_before_execution() {
    let (runtime, seen) = runtime("error");
    runtime.initialize().await.unwrap();
    let result = runtime
        .send_turn(
            "backend-1",
            vec![RuntimeContent::text("work may have happened")],
        )
        .await;
    assert!(seen.lock().unwrap().iter().any(|m| m == "session/prompt"));
    assert!(
        matches!(result, TurnOutcome::Unknown(_)),
        "generic provider RPC error cannot prove no side effects or no live writer; got {result:?}"
    );
}

#[tokio::test]
async fn writer_failure_fences_later_requests_as_not_sent() {
    let (reader, _peer_kept_open) = tokio::io::duplex(1024);
    let connection = Connection::new(
        reader,
        ParentBrokenWriter,
        Arc::new(RecordingHandler::default()),
    );
    let runtime = AcpRuntime::new(AcpClient::new(connection, None));
    let first = tokio::time::timeout(
        std::time::Duration::from_millis(250),
        runtime.send_turn("backend", vec![RuntimeContent::text("first")]),
    )
    .await
    .unwrap();
    assert!(matches!(first, TurnOutcome::Unknown(_)));
    let next = tokio::time::timeout(
        std::time::Duration::from_millis(250),
        runtime.send_turn("backend", vec![RuntimeContent::text("never queued")]),
    )
    .await
    .unwrap();
    assert!(matches!(next, TurnOutcome::NotSent(_)));
}

#[tokio::test]
async fn neutral_diagnostics_preserve_counts_without_prompt_content() {
    let (runtime, _) = runtime("end");
    runtime.initialize().await.unwrap();
    runtime
        .send_turn(
            "backend",
            vec![RuntimeContent::text("private prompt sentinel")],
        )
        .await;
    let diagnostics = runtime.diagnostics();
    let protocol = serde_json::to_string(&diagnostics.protocol).unwrap();
    assert_eq!(diagnostics.protocol.outbound_requests, 2);
    assert!(protocol.contains("session/prompt"));
    assert!(!protocol.contains("private prompt sentinel"));
    assert!(!diagnostics.process.termination_requested);
}

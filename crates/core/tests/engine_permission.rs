//! The engine's hardest bit, tested offline: an ACP `session/request_permission` in `Ask` mode is
//! *parked* (an `Event::PermissionRequest` is emitted, the turn stays open) until the frontend
//! answers via the router — after which the turn completes. Streamed text becomes `Event::AgentText`.

use std::sync::{Arc, Mutex};

use codetwo_core::acp::wire::{
    PermissionOption, PermissionOutcome, RequestPermissionRequest, SessionNotification,
    SessionUpdate, ToolCall,
};
use codetwo_core::acp::{AcpClient, ClientHandler, Connection, ContentBlock, StopReason};
use codetwo_core::event::Event;
use codetwo_core::permission::{
    PermissionContextKind, PermissionMode, PermissionPolicy, SandboxPolicy,
};
use codetwo_core::{PermissionRouter, SessionHandler};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

async fn write_line<W: AsyncWrite + Unpin>(w: &mut W, v: Value) {
    let mut s = v.to_string();
    s.push('\n');
    w.write_all(s.as_bytes()).await.unwrap();
    w.flush().await.unwrap();
}

async fn mock_agent<R, W>(reader: R, mut writer: W)
where
    R: tokio::io::AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    let mut prompt_id: Option<Value> = None;
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(&line).unwrap();
        match v.get("method").and_then(|m| m.as_str()) {
            Some("initialize") => {
                write_line(
                    &mut writer,
                    json!({"jsonrpc":"2.0","id":v["id"],"result":{"protocolVersion":1}}),
                )
                .await;
            }
            Some("session/new") => {
                write_line(
                    &mut writer,
                    json!({"jsonrpc":"2.0","id":v["id"],"result":{"sessionId":"sess-1"}}),
                )
                .await;
            }
            Some("session/prompt") => {
                prompt_id = Some(v["id"].clone());
                write_line(
                    &mut writer,
                    json!({"jsonrpc":"2.0","method":"session/update","params":{
                        "sessionId":"sess-1",
                        "update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"working"}}}}),
                )
                .await;
                write_line(
                    &mut writer,
                    json!({"jsonrpc":"2.0","id":42,"method":"session/request_permission","params":{
                        "sessionId":"sess-1",
                        "toolCall":{"toolCallId":"tc1","title":"rm build/","kind":"execute"},
                        "options":[{"optionId":"allow","name":"Allow","kind":"allow_once"},
                                   {"optionId":"reject","name":"Reject","kind":"reject_once"}]}}),
                )
                .await;
            }
            Some(_) => {}
            None => {
                if let Some(pid) = prompt_id.take() {
                    write_line(
                        &mut writer,
                        json!({"jsonrpc":"2.0","id":pid,"result":{"stopReason":"end_turn"}}),
                    )
                    .await;
                }
            }
        }
    }
}

#[tokio::test]
async fn permission_is_parked_then_answered() {
    let (events_tx, mut events_rx) = mpsc::unbounded_channel::<Event>();
    let router = PermissionRouter::default();
    // Ask mode → the permission must be surfaced, not auto-answered.
    let policy = Arc::new(Mutex::new(PermissionPolicy::default()));
    let handler = Arc::new(SessionHandler::new(
        "s1".into(),
        events_tx,
        policy,
        router.clone(),
        None,
    ));

    let (client_end, agent_end) = tokio::io::duplex(64 * 1024);
    let (cr, cw) = tokio::io::split(client_end);
    let (ar, aw) = tokio::io::split(agent_end);
    tokio::spawn(mock_agent(ar, aw));

    let conn = Connection::new(cr, cw, handler);
    let client = AcpClient::new(conn, None);
    client.initialize(json!({})).await.unwrap();
    let sid = client.new_session("/tmp", vec![]).await.unwrap();

    // Run the turn concurrently; it will block inside request_permission until we answer.
    let turn =
        tokio::spawn(async move { client.prompt(&sid, vec![ContentBlock::text("go")]).await });

    // Collect events until the permission ask surfaces.
    let mut saw_text = false;
    let mut request_id = None;
    while let Some(ev) = events_rx.recv().await {
        match ev {
            Event::AgentText {
                text,
                transcript_seq,
                ..
            } => {
                assert_eq!(text, "working");
                assert_eq!(transcript_seq, None);
                saw_text = true;
            }
            Event::PermissionRequest {
                request_id: rid,
                title,
                options,
                ..
            } => {
                assert_eq!(title, "rm build/");
                assert_eq!(options.len(), 2);
                request_id = Some(rid);
                break;
            }
            _ => {}
        }
    }
    assert!(
        saw_text,
        "agent text should stream before the permission ask"
    );
    let request_id = request_id.expect("permission request should have surfaced");

    // The turn must still be pending (parked on our answer).
    assert!(
        !turn.is_finished(),
        "turn must wait for the permission decision"
    );

    // Answer allow → the turn completes.
    assert!(router.answer(
        &request_id,
        PermissionOutcome::Selected {
            option_id: "allow".into()
        }
    ));
    let stop = turn.await.unwrap().unwrap();
    assert_eq!(stop, StopReason::EndTurn);
}

#[tokio::test]
async fn mcp_elicitation_still_parks_in_full_access() {
    let (events_tx, mut events_rx) = mpsc::unbounded_channel::<Event>();
    let router = PermissionRouter::default();
    let policy = Arc::new(Mutex::new(PermissionPolicy {
        mode: PermissionMode::Yolo,
        sandbox: SandboxPolicy::DangerFullAccess,
        rules: Vec::new(),
    }));
    let handler = Arc::new(SessionHandler::new(
        "s1".into(),
        events_tx,
        policy,
        router.clone(),
        None,
    ));
    let request = RequestPermissionRequest {
        session_id: "provider-session".into(),
        tool_call: json!({
            "toolCallId": "browser-approval",
            "title": "Website access",
            "kind": "other"
        }),
        options: vec![
            PermissionOption {
                option_id: "allow".into(),
                name: "Allow once".into(),
                kind: "allow_once".into(),
            },
            PermissionOption {
                option_id: "reject".into(),
                name: "Reject".into(),
                kind: "reject_once".into(),
            },
        ],
        meta: Some(json!({"is_mcp_tool_approval": true})),
    };

    let pending = tokio::spawn({
        let handler = handler.clone();
        async move { handler.request_permission(request).await }
    });
    let event = events_rx.recv().await.expect("permission event");
    let request_id = match event {
        Event::PermissionRequest {
            request_id,
            context,
            ..
        } => {
            assert_eq!(context.kind, PermissionContextKind::WebsiteAccess);
            request_id
        }
        other => panic!("unexpected event: {other:?}"),
    };
    assert!(
        !pending.is_finished(),
        "Full Access must not auto-answer elicitation"
    );
    assert!(router.answer(
        &request_id,
        PermissionOutcome::Selected {
            option_id: "reject".into()
        }
    ));
    let response = pending.await.unwrap();
    assert!(matches!(
        response.outcome,
        PermissionOutcome::Selected { option_id } if option_id == "reject"
    ));
}

#[tokio::test]
async fn internal_auto_scene_selection_does_not_ask_before_the_broker() {
    let (events_tx, mut events_rx) = mpsc::unbounded_channel::<Event>();
    let router = PermissionRouter::default();
    let policy = Arc::new(Mutex::new(PermissionPolicy::default()));
    let handler = Arc::new(SessionHandler::new(
        "s1".into(),
        events_tx,
        policy,
        router,
        None,
    ));
    handler
        .session_update(SessionNotification {
            session_id: "provider-session".into(),
            update: SessionUpdate::ToolCall(ToolCall {
                tool_call_id: "scene-select".into(),
                title: Some("mcp.codetwo_scenes.scene_select".into()),
                kind: Some("other".into()),
                status: Some("pending".into()),
                content: None,
                raw_input: Some(json!({
                    "server": "codetwo_scenes",
                    "tool": "scene_select"
                })),
                raw_output: None,
                meta: None,
            }),
        })
        .await;
    assert!(matches!(
        events_rx.recv().await,
        Some(Event::ToolCall { .. })
    ));

    let response = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        handler.request_permission(RequestPermissionRequest {
            session_id: "provider-session".into(),
            tool_call: json!({
                "toolCallId": "scene-select",
                "title": "mcp.codetwo_scenes.scene_select",
                "kind": "other"
            }),
            options: vec![
                PermissionOption {
                    option_id: "allow".into(),
                    name: "Allow once".into(),
                    kind: "allow_once".into(),
                },
                PermissionOption {
                    option_id: "reject".into(),
                    name: "Reject".into(),
                    kind: "reject_once".into(),
                },
            ],
            meta: Some(json!({"is_mcp_tool_approval": true})),
        }),
    )
    .await
    .expect("the authenticated internal selector should not be parked");
    assert!(matches!(
        response.outcome,
        PermissionOutcome::Selected { option_id } if option_id == "allow"
    ));
    assert!(
        events_rx.try_recv().is_err(),
        "no permission dialog should surface"
    );
}

#[tokio::test]
async fn internal_auto_scene_permission_escalation_still_asks() {
    let (events_tx, mut events_rx) = mpsc::unbounded_channel::<Event>();
    let router = PermissionRouter::default();
    let handler = Arc::new(SessionHandler::new(
        "s1".into(),
        events_tx,
        Arc::new(Mutex::new(PermissionPolicy::default())),
        router.clone(),
        None,
    ));
    handler
        .session_update(SessionNotification {
            session_id: "provider-session".into(),
            update: SessionUpdate::ToolCall(ToolCall {
                tool_call_id: "scene-escalation".into(),
                title: Some("mcp.codetwo_scenes.scene_select".into()),
                kind: Some("other".into()),
                status: Some("pending".into()),
                content: None,
                raw_input: Some(json!({
                    "server": "codetwo_scenes",
                    "tool": "scene_select"
                })),
                raw_output: None,
                meta: None,
            }),
        })
        .await;
    assert!(matches!(
        events_rx.recv().await,
        Some(Event::ToolCall { .. })
    ));

    let pending = tokio::spawn({
        let handler = handler.clone();
        async move {
            handler
                .request_permission(RequestPermissionRequest {
                    session_id: "provider-session".into(),
                    tool_call: json!({
                        "toolCallId": "scene-escalation",
                        "title": "mcp.codetwo_scenes.scene_select",
                        "kind": "other",
                        "content": [{
                            "type": "content",
                            "content": {
                                "type": "text",
                                "text": "Allow looser scene permissions?"
                            }
                        }]
                    }),
                    options: vec![
                        PermissionOption {
                            option_id: "allow".into(),
                            name: "Allow once".into(),
                            kind: "allow_once".into(),
                        },
                        PermissionOption {
                            option_id: "reject".into(),
                            name: "Reject".into(),
                            kind: "reject_once".into(),
                        },
                    ],
                    meta: Some(json!({"is_mcp_tool_approval": true})),
                })
                .await
        }
    });
    let request_id = match events_rx.recv().await.expect("permission event") {
        Event::PermissionRequest {
            request_id,
            context,
            ..
        } => {
            assert_eq!(context.kind, PermissionContextKind::McpElicitation);
            request_id
        }
        other => panic!("unexpected event: {other:?}"),
    };
    assert!(
        !pending.is_finished(),
        "permission escalation must stay parked"
    );
    assert!(router.answer(
        &request_id,
        PermissionOutcome::Selected {
            option_id: "reject".into()
        }
    ));
    assert!(matches!(
        pending.await.unwrap().outcome,
        PermissionOutcome::Selected { option_id } if option_id == "reject"
    ));
}

#[tokio::test]
async fn sites_production_action_still_parks_in_full_access() {
    let (events_tx, mut events_rx) = mpsc::unbounded_channel::<Event>();
    let router = PermissionRouter::default();
    let policy = Arc::new(Mutex::new(PermissionPolicy {
        mode: PermissionMode::Yolo,
        sandbox: SandboxPolicy::DangerFullAccess,
        rules: Vec::new(),
    }));
    let handler = Arc::new(SessionHandler::new(
        "s1".into(),
        events_tx,
        policy,
        router.clone(),
        None,
    ));
    handler
        .session_update(SessionNotification {
            session_id: "provider-session".into(),
            update: SessionUpdate::ToolCall(ToolCall {
                tool_call_id: "sites-deploy".into(),
                title: Some("Deploy site".into()),
                kind: Some("other".into()),
                status: Some("pending".into()),
                content: None,
                raw_input: Some(json!({
                    "server": "codex_apps",
                    "tool": "sites_deploy_site_version"
                })),
                raw_output: None,
                meta: None,
            }),
        })
        .await;
    assert!(matches!(
        events_rx.recv().await,
        Some(Event::ToolCall { kind: Some(kind), .. }) if kind == "sites"
    ));

    let pending = tokio::spawn({
        let handler = handler.clone();
        async move {
            handler
                .request_permission(RequestPermissionRequest {
                    session_id: "provider-session".into(),
                    tool_call: json!({
                        "toolCallId": "sites-deploy",
                        "title": "Deploy site",
                        "kind": "other"
                    }),
                    options: vec![
                        PermissionOption {
                            option_id: "allow".into(),
                            name: "Allow once".into(),
                            kind: "allow_once".into(),
                        },
                        PermissionOption {
                            option_id: "reject".into(),
                            name: "Reject".into(),
                            kind: "reject_once".into(),
                        },
                    ],
                    meta: None,
                })
                .await
        }
    });
    let request_id = match events_rx.recv().await.expect("permission event") {
        Event::PermissionRequest {
            request_id,
            context,
            ..
        } => {
            assert_eq!(context.kind, PermissionContextKind::SitesProduction);
            assert!(context
                .risk
                .as_deref()
                .is_some_and(|risk| risk.contains("Production")));
            request_id
        }
        other => panic!("unexpected event: {other:?}"),
    };
    assert!(!pending.is_finished(), "Full Access must not deploy Sites");
    assert!(router.answer(
        &request_id,
        PermissionOutcome::Selected {
            option_id: "reject".into()
        }
    ));
    let response = pending.await.unwrap();
    assert!(matches!(
        response.outcome,
        PermissionOutcome::Selected { option_id } if option_id == "reject"
    ));
}

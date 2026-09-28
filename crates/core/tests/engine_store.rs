//! The session handler persists streamed agent output to the store. Uses YOLO mode so the mock's
//! permission request is auto-approved (no parking), keeping the test to the persistence path.

use std::sync::{Arc, Mutex};

use codetwo_core::acp::{AcpClient, Connection, ContentBlock, StopReason};
use codetwo_core::permission::{PermissionMode, PermissionPolicy};
use codetwo_core::session::{Part, Role, Session};
use codetwo_core::{Event, PermissionRouter, ProviderId, SessionHandler, Store};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

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
                        "update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"done deal"}}}}),
                )
                .await;
                write_line(
                    &mut writer,
                    json!({"jsonrpc":"2.0","id":7,"method":"session/request_permission","params":{
                        "sessionId":"sess-1",
                        "toolCall":{"toolCallId":"tc1","title":"write file","kind":"edit"},
                        "options":[{"optionId":"allow","name":"Allow","kind":"allow_once"}]}}),
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
async fn agent_output_is_persisted() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let mut session = Session::new(ProviderId::Codex, "/tmp");
    session.id = "s1".into();
    store.upsert_session(&session).unwrap();
    let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
    let router = PermissionRouter::default();
    // YOLO → auto-approve, no parking.
    let policy = Arc::new(Mutex::new(PermissionPolicy {
        mode: PermissionMode::Yolo,
        ..Default::default()
    }));
    let handler = Arc::new(SessionHandler::new(
        "s1".into(),
        events_tx,
        policy,
        router,
        Some(store.clone()),
    ));

    let (client_end, agent_end) = tokio::io::duplex(64 * 1024);
    let (cr, cw) = tokio::io::split(client_end);
    let (ar, aw) = tokio::io::split(agent_end);
    tokio::spawn(mock_agent(ar, aw));

    let conn = Connection::new(cr, cw, handler);
    let client = AcpClient::new(conn, None);
    client.initialize(json!({})).await.unwrap();
    let sid = client.new_session("/tmp", vec![]).await.unwrap();
    let stop = client
        .prompt(&sid, vec![ContentBlock::text("go")])
        .await
        .unwrap();
    assert_eq!(stop, StopReason::EndTurn);

    let event = events_rx.recv().await.unwrap();
    assert!(matches!(
        event,
        Event::AgentText {
            transcript_seq: Some(0),
            text,
            ..
        } if text == "done deal"
    ));

    let transcript = store.transcript("s1").unwrap();
    assert!(
        transcript
            .iter()
            .any(|(role, part)| matches!(role, Role::Agent)
                && matches!(part, Part::Text { text } if text == "done deal")),
        "agent text should be persisted, got {transcript:?}",
    );
}

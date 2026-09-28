//! Native T3 Code mobile compatibility, exercised through the real HTTP/WebSocket listener.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use codetwo_core::provider::{default_registry, LaunchSpec, Provider, ProviderId};
use codetwo_core::skill::{builtin_skills, SkillLibrary};
use codetwo_core::{Engine, Part, Role, Store};
use codetwo_server::{bind_and_serve, fanout, AuthState, DEFAULT_PAIRING_TTL};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;

const MOCK_AGENT: &str = r#"
import json, sys, time

def send(message):
    print(json.dumps(message), flush=True)

for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    mid = message.get("id")
    if method == "initialize":
        send({"jsonrpc":"2.0","id":mid,"result":{"protocolVersion":1}})
    elif method == "session/new":
        send({"jsonrpc":"2.0","id":mid,"result":{"sessionId":"t3-mobile-agent"}})
    elif method == "session/set_model":
        send({"jsonrpc":"2.0","id":mid,"result":{}})
    elif method == "session/prompt":
        prompt = json.dumps(message.get("params", {}))
        if "hold open" in prompt:
            time.sleep(0.5)
        assert "Before changing anything" not in prompt
        reply = "long reply" if "long request" in prompt else "phone reply"
        split = max(1, len(reply) // 2)
        for chunk in (reply[:split], reply[split:]):
            send({"jsonrpc":"2.0","method":"session/update","params":{
                "sessionId":"t3-mobile-agent",
                "update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":chunk}}
            }})
        send({"jsonrpc":"2.0","id":mid,"result":{"stopReason":"end_turn"}})
"#;

async fn http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    content_type: Option<&str>,
    bearer: Option<&str>,
    body: &str,
) -> (u16, Value) {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let content_type = content_type
        .map(|value| format!("Content-Type: {value}\r\n"))
        .unwrap_or_default();
    let authorization = bearer
        .map(|value| format!("Authorization: Bearer {value}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n{content_type}{authorization}Content-Length: {}\r\n\r\n{body}",
        body.len(),
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let response = String::from_utf8(raw).unwrap();
    let status = response
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, value)| value)
        .unwrap_or_default();
    let value = serde_json::from_str(body)
        .unwrap_or_else(|error| panic!("expected JSON HTTP response, got {body:?}: {error}"));
    (status, value)
}

async fn http_status(
    addr: SocketAddr,
    method: &str,
    path: &str,
    content_type: Option<&str>,
    bearer: Option<&str>,
    body: &str,
) -> u16 {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let content_type = content_type
        .map(|value| format!("Content-Type: {value}\r\n"))
        .unwrap_or_default();
    let authorization = bearer
        .map(|value| format!("Authorization: Bearer {value}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n{content_type}{authorization}Content-Length: {}\r\n\r\n{body}",
        body.len(),
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    String::from_utf8(raw)
        .unwrap()
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

async fn next_json<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("T3 socket response timed out")
            .expect("T3 socket closed")
            .expect("T3 socket read failed");
        match message {
            Message::Text(text) => return serde_json::from_str(&text).unwrap(),
            Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await.unwrap(),
            Message::Pong(_) => {}
            other => panic!("expected T3 JSON frame, got {other:?}"),
        }
    }
}

async fn assert_socket_closes<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let outcome = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .expect("revoked T3 socket did not close");
    assert!(
        matches!(outcome, None | Some(Ok(Message::Close(_)))),
        "revoked T3 socket received data instead of closing: {outcome:?}"
    );
}

#[tokio::test]
async fn native_mobile_bootstrap_uses_t3_http_and_effect_rpc_contracts() {
    let (engine, rx) = Engine::new(default_registry(), SkillLibrary::new(builtin_skills()));
    let auth = Arc::new(AuthState::load(None));
    let token = auth.issue_t3_pairing_token(DEFAULT_PAIRING_TTL);
    let (addr, server) = bind_and_serve(
        Arc::new(engine),
        fanout(rx),
        "127.0.0.1:0".parse().unwrap(),
        auth,
    )
    .await
    .unwrap();

    // Opt-in rendezvous used only by the maintainer compatibility check: it lets the exact
    // upstream Effect RPC client connect to this real test listener. Normal test runs never enter
    // this branch.
    if let Some(rendezvous) = std::env::var_os("CODETWO_T3_UPSTREAM_RENDEZVOUS") {
        let rendezvous = std::path::PathBuf::from(rendezvous);
        std::fs::create_dir_all(&rendezvous).unwrap();
        std::fs::write(
            rendezvous.join("ready.json"),
            json!({ "httpBaseUrl": format!("http://{addr}"), "pairingCode": token }).to_string(),
        )
        .unwrap();
        for _ in 0..1200 {
            if rendezvous.join("done").exists() {
                let outcome = std::fs::read_to_string(rendezvous.join("done")).unwrap();
                assert_eq!(
                    outcome, "ok",
                    "upstream T3 client compatibility check failed"
                );
                server.abort();
                let _ = server.await;
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("upstream T3 client did not finish its compatibility check");
    }

    let (status, descriptor) =
        http(addr, "GET", "/.well-known/t3/environment", None, None, "").await;
    assert_eq!(status, 200);
    assert!(descriptor["environmentId"]
        .as_str()
        .is_some_and(|value| value.starts_with("codetwo-")));
    assert!(descriptor["label"].is_string());
    assert_eq!(descriptor["capabilities"]["connectionProbe"], true);
    if std::env::var_os("CODETWO_DUMP_T3_FIXTURES").is_some() {
        eprintln!("T3_DESCRIPTOR_FIXTURE={descriptor}");
    }

    let form = format!(
        "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Atoken-exchange\
         &subject_token={token}\
         &subject_token_type=urn%3At3%3Aparams%3Aoauth%3Atoken-type%3Aenvironment-bootstrap\
         &requested_token_type=urn%3Aietf%3Aparams%3Aoauth%3Atoken-type%3Aaccess_token\
         &client_label=T3+Code+Mobile&client_device_type=mobile&client_os=iOS"
    );
    let form = form.replace(' ', "");
    assert_eq!(
        http_status(
            addr,
            "POST",
            "/api/pair",
            Some("application/json"),
            None,
            &json!({ "token": token, "device_name": "Downgrade attempt" }).to_string(),
        )
        .await,
        401,
        "a T3 bootstrap code must not be redeemable by the legacy protocol"
    );
    let (status, exchanged) = http(
        addr,
        "POST",
        "/oauth/token",
        Some("application/x-www-form-urlencoded"),
        None,
        &form,
    )
    .await;
    assert_eq!(status, 200, "token exchange failed: {exchanged}");
    assert_eq!(
        exchanged["issued_token_type"],
        "urn:ietf:params:oauth:token-type:access_token"
    );
    assert_eq!(exchanged["token_type"], "Bearer");
    assert!(exchanged["expires_in"].as_i64().is_some_and(|ttl| ttl > 0));
    assert_eq!(
        exchanged["scope"],
        "orchestration:read orchestration:operate terminal:operate review:write relay:read"
    );
    let bearer = exchanged["access_token"].as_str().unwrap();
    if std::env::var_os("CODETWO_DUMP_T3_FIXTURES").is_some() {
        let mut fixture = exchanged.clone();
        fixture["access_token"] = json!("redacted");
        eprintln!("T3_TOKEN_FIXTURE={fixture}");
    }

    let (status, session) = http(addr, "GET", "/api/auth/session", None, Some(bearer), "").await;
    assert_eq!(status, 200);
    assert_eq!(session["authenticated"], true);
    assert_eq!(session["auth"]["policy"], "remote-reachable");
    assert_eq!(session["sessionMethod"], "bearer-access-token");
    assert!(chrono::DateTime::parse_from_rfc3339(session["expiresAt"].as_str().unwrap()).is_ok());
    if std::env::var_os("CODETWO_DUMP_T3_FIXTURES").is_some() {
        eprintln!("T3_SESSION_FIXTURE={session}");
    }

    assert_eq!(
        http_status(addr, "POST", "/api/ws-ticket", None, Some(bearer), "").await,
        401,
        "a scoped T3 bearer must not mint a legacy-protocol ticket"
    );

    let (status, ticket_reply) = http(
        addr,
        "POST",
        "/api/auth/websocket-ticket",
        None,
        Some(bearer),
        "",
    )
    .await;
    assert_eq!(status, 200);
    let ticket = ticket_reply["ticket"].as_str().unwrap();
    assert!(
        chrono::DateTime::parse_from_rfc3339(ticket_reply["expiresAt"].as_str().unwrap()).is_ok()
    );
    assert!(ticket_reply.get("expires_in").is_none());
    if std::env::var_os("CODETWO_DUMP_T3_FIXTURES").is_some() {
        let mut fixture = ticket_reply.clone();
        fixture["ticket"] = json!("redacted");
        eprintln!("T3_TICKET_FIXTURE={fixture}");
    }

    let wrong_protocol =
        tokio_tungstenite::connect_async(format!("ws://{addr}/ws?ticket={ticket}"))
            .await
            .expect_err("a T3 ticket must not authorize the legacy WebSocket protocol");
    assert!(matches!(
        wrong_protocol,
        tokio_tungstenite::tungstenite::Error::Http(response)
            if response.status() == tokio_tungstenite::tungstenite::http::StatusCode::UNAUTHORIZED
    ));

    let (mut socket, _) =
        tokio_tungstenite::connect_async(format!("ws://{addr}/ws?wsTicket={ticket}"))
            .await
            .unwrap();

    socket
        .send(Message::Text(json!({ "_tag": "Ping" }).to_string()))
        .await
        .unwrap();
    assert_eq!(next_json(&mut socket).await, json!({ "_tag": "Pong" }));

    socket
        .send(Message::Text(
            json!({
                "_tag": "Request",
                "id": 1,
                "tag": "server.getConfig",
                "payload": {},
                "headers": [],
            })
            .to_string(),
        ))
        .await
        .unwrap();
    let config = next_json(&mut socket).await;
    if std::env::var_os("CODETWO_DUMP_T3_FIXTURES").is_some() {
        eprintln!("T3_CONFIG_FIXTURE={}", config["exit"]["value"]);
    }
    assert_eq!(config["_tag"], "Exit");
    assert_eq!(config["requestId"], 1);
    assert_eq!(config["exit"]["_tag"], "Success");
    assert_eq!(
        config["exit"]["value"]["environment"]["environmentId"],
        descriptor["environmentId"]
    );
    assert_eq!(config["exit"]["value"]["settings"], json!({}));
    assert_eq!(
        config["exit"]["value"]["observability"]["localTracingEnabled"],
        false
    );

    socket
        .send(Message::Text(
            json!({
                "_tag": "Request",
                "id": "activity-1",
                "tag": "server.reportClientActivity",
                "payload": {
                    "environmentId": descriptor["environmentId"],
                    "clientId": "t3-mobile-test",
                    "clientKind": "mobile",
                    "visible": true,
                    "focused": true,
                    "recentlyInteracted": true,
                    "appState": "active",
                    "scopes": [{ "type": "server-config" }],
                    "observedAt": "2026-08-12T00:00:00.000Z",
                },
                "headers": [],
            })
            .to_string(),
        ))
        .await
        .unwrap();
    let activity = next_json(&mut socket).await;
    assert_eq!(
        activity["exit"],
        json!({ "_tag": "Success", "value": null })
    );

    socket
        .send(Message::Text(
            json!({
                "_tag": "Request",
                "id": "config-sub",
                "tag": "subscribeServerConfig",
                "payload": {},
                "headers": [],
            })
            .to_string(),
        ))
        .await
        .unwrap();
    let config_stream = next_json(&mut socket).await;
    assert_eq!(config_stream["_tag"], "Chunk");
    assert_eq!(config_stream["requestId"], "config-sub");
    assert_eq!(config_stream["values"][0]["version"], 1);
    assert_eq!(config_stream["values"][0]["type"], "snapshot");
    if std::env::var_os("CODETWO_DUMP_T3_FIXTURES").is_some() {
        eprintln!("T3_CONFIG_STREAM_FIXTURE={}", config_stream["values"][0]);
    }
    socket
        .send(Message::Text(
            json!({ "_tag": "Ack", "requestId": "config-sub" }).to_string(),
        ))
        .await
        .unwrap();

    socket
        .send(Message::Text(
            json!({
                "_tag": "Request",
                "id": "lifecycle-sub",
                "tag": "subscribeServerLifecycle",
                "payload": {},
                "headers": [],
            })
            .to_string(),
        ))
        .await
        .unwrap();
    let lifecycle = next_json(&mut socket).await;
    assert_eq!(lifecycle["_tag"], "Chunk");
    assert_eq!(lifecycle["requestId"], "lifecycle-sub");
    assert_eq!(lifecycle["values"][0]["type"], "welcome");
    assert_eq!(lifecycle["values"][1]["type"], "ready");
    if std::env::var_os("CODETWO_DUMP_T3_FIXTURES").is_some() {
        eprintln!("T3_LIFECYCLE_FIXTURE={}", lifecycle["values"]);
    }
    socket
        .send(Message::Text(
            json!({ "_tag": "Ack", "requestId": "lifecycle-sub" }).to_string(),
        ))
        .await
        .unwrap();

    socket
        .send(Message::Text(
            json!({
                "_tag": "Request",
                "id": "shell-1",
                "tag": "orchestration.subscribeShell",
                "payload": {},
                "headers": [],
            })
            .to_string(),
        ))
        .await
        .unwrap();
    let shell = next_json(&mut socket).await;
    if std::env::var_os("CODETWO_DUMP_T3_FIXTURES").is_some() {
        eprintln!("T3_SHELL_FIXTURE={}", shell["values"][0]["snapshot"]);
    }
    assert_eq!(shell["_tag"], "Chunk");
    assert_eq!(shell["requestId"], "shell-1");
    assert_eq!(shell["values"][0]["kind"], "snapshot");
    assert!(shell["values"][0]["snapshot"]["snapshotSequence"].is_u64());
    assert!(shell["values"][0]["snapshot"]["projects"].is_array());
    assert!(shell["values"][0]["snapshot"]["threads"].is_array());

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn native_mobile_creates_a_thread_and_dispatches_a_prompt() {
    let temp = std::env::temp_dir().join(format!("codetwo-t3-mobile-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp).unwrap();
    let store = Arc::new(Store::open(temp.join("codetwo.db").to_str().unwrap()).unwrap());
    let provider = Provider {
        id: ProviderId::Custom("t3mock".into()),
        display_name: "T3 mock agent".into(),
        launch: LaunchSpec::new("python3", ["-u", "-c", MOCK_AGENT]),
        needs_node: false,
    };
    let (engine, rx) = Engine::with_store(
        vec![provider.clone()],
        SkillLibrary::new(builtin_skills()),
        store.clone(),
    );
    let engine = Arc::new(engine);
    let events = fanout(rx);

    let auth_path = temp.join("remote-devices.json");
    let auth = Arc::new(AuthState::load(Some(auth_path.clone())));
    let token = auth.issue_t3_pairing_token(DEFAULT_PAIRING_TTL);
    let (addr, server) = bind_and_serve(engine, events, "127.0.0.1:0".parse().unwrap(), auth)
        .await
        .unwrap();
    let form = format!(
        "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Atoken-exchange\
         &subject_token={token}\
         &subject_token_type=urn%3At3%3Aparams%3Aoauth%3Atoken-type%3Aenvironment-bootstrap\
         &requested_token_type=urn%3Aietf%3Aparams%3Aoauth%3Atoken-type%3Aaccess_token\
         &scope=orchestration%3Aread+orchestration%3Aoperate"
    )
    .replace(' ', "");
    let (status, exchanged) = http(
        addr,
        "POST",
        "/oauth/token",
        Some("application/x-www-form-urlencoded"),
        None,
        &form,
    )
    .await;
    assert_eq!(status, 200);
    let bearer = exchanged["access_token"].as_str().unwrap();

    let (_, ticket_reply) = http(
        addr,
        "POST",
        "/api/auth/websocket-ticket",
        None,
        Some(bearer),
        "",
    )
    .await;
    let ticket = ticket_reply["ticket"].as_str().unwrap();
    let (mut socket, _) =
        tokio_tungstenite::connect_async(format!("ws://{addr}/ws?wsTicket={ticket}"))
            .await
            .unwrap();

    socket
        .send(Message::Text(
            json!({
                "_tag": "Request",
                "id": "shell-before-create",
                "tag": "orchestration.subscribeShell",
                "payload": {},
                "headers": [],
            })
            .to_string(),
        ))
        .await
        .unwrap();
    let shell = next_json(&mut socket).await;
    assert_eq!(shell["_tag"], "Chunk");
    let project_id = shell["values"][0]["snapshot"]["projects"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    socket
        .send(Message::Text(
            json!({ "_tag": "Interrupt", "requestId": "shell-before-create" }).to_string(),
        ))
        .await
        .unwrap();

    let thread_id = "mobile-created-thread";
    let create_command = json!({
        "type": "thread.turn.start",
        "commandId": "mobile-turn-1",
        "threadId": thread_id,
        "message": {
            "messageId": "mobile-message-1",
            "role": "user",
            "text": "hello from phone",
            "attachments": [],
        },
        "runtimeMode": "approval-required",
        "interactionMode": "default",
        "modelSelection": { "instanceId": "t3mock", "model": "phone-model" },
        "titleSeed": "hello from phone",
        "bootstrap": {
            "createThread": {
                "projectId": project_id,
                "title": "hello from phone",
                "modelSelection": { "instanceId": "t3mock", "model": "phone-model" },
                "runtimeMode": "approval-required",
                "interactionMode": "default",
                "branch": null,
                "worktreePath": null,
                "createdAt": "2026-08-12T00:00:00.000Z"
            }
        },
        "createdAt": "2026-08-12T00:00:00.000Z",
    });

    // Worktree semantics include a caller-selected base/branch/setup contract that C2 cannot
    // faithfully map yet. Refuse it before creating a session instead of running in the wrong
    // checkout.
    let mut unsupported_worktree = create_command.clone();
    unsupported_worktree["commandId"] = json!("mobile-worktree-unsupported");
    unsupported_worktree["threadId"] = json!("mobile-worktree-thread");
    unsupported_worktree["bootstrap"]["prepareWorktree"] = json!({
        "projectCwd": temp,
        "baseBranch": "main",
        "branch": "mobile-branch",
        "startFromOrigin": true,
    });
    socket
        .send(Message::Text(
            json!({
                "_tag": "Request",
                "id": "dispatch-worktree-unsupported",
                "tag": "orchestration.dispatchCommand",
                "payload": unsupported_worktree,
                "headers": [],
            })
            .to_string(),
        ))
        .await
        .unwrap();
    let unsupported_receipt = next_json(&mut socket).await;
    assert_eq!(unsupported_receipt["exit"]["_tag"], "Failure");
    assert!(store.list_sessions().unwrap().is_empty());

    socket
        .send(Message::Text(
            json!({
                "_tag": "Request",
                "id": "dispatch-1",
                "tag": "orchestration.dispatchCommand",
                "payload": create_command,
                "headers": [],
            })
            .to_string(),
        ))
        .await
        .unwrap();
    let receipt = next_json(&mut socket).await;
    assert_eq!(receipt["_tag"], "Exit");
    assert_eq!(receipt["requestId"], "dispatch-1");
    assert_eq!(receipt["exit"]["_tag"], "Success");

    let (status, detail) = http(
        addr,
        "GET",
        &format!("/api/orchestration/threads/{thread_id}"),
        None,
        Some(bearer),
        "",
    )
    .await;
    assert_eq!(status, 200, "new thread snapshot failed: {detail}");
    assert_eq!(detail["thread"]["id"], thread_id);
    assert_eq!(detail["thread"]["modelSelection"]["model"], "phone-model");

    socket
        .send(Message::Text(
            json!({
                "_tag": "Request",
                "id": "thread-sub",
                "tag": "orchestration.subscribeThread",
                "payload": { "threadId": thread_id },
                "headers": [],
            })
            .to_string(),
        ))
        .await
        .unwrap();
    let initial = next_json(&mut socket).await;
    assert_eq!(initial["_tag"], "Chunk");
    assert_eq!(initial["values"][0]["kind"], "snapshot");
    let mut latest_detail = initial["values"][0]["snapshot"].clone();
    let mut found_prompt = latest_detail["thread"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["role"] == "user" && message["text"] == "hello from phone");
    let mut found_reply = latest_detail["thread"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["role"] == "assistant" && message["text"] == "phone reply");
    socket
        .send(Message::Text(
            json!({ "_tag": "Ack", "requestId": "thread-sub" }).to_string(),
        ))
        .await
        .unwrap();
    for _ in 0..20 {
        if found_prompt && found_reply {
            break;
        }
        let frame = next_json(&mut socket).await;
        if frame["_tag"] != "Chunk" || frame["requestId"] != "thread-sub" {
            continue;
        }
        latest_detail = frame["values"][0]["snapshot"].clone();
        let messages = latest_detail["thread"]["messages"]
            .as_array()
            .expect("thread messages");
        found_prompt |= messages
            .iter()
            .any(|message| message["role"] == "user" && message["text"] == "hello from phone");
        found_reply |= messages
            .iter()
            .any(|message| message["role"] == "assistant" && message["text"] == "phone reply");
        socket
            .send(Message::Text(
                json!({ "_tag": "Ack", "requestId": "thread-sub" }).to_string(),
            ))
            .await
            .unwrap();
    }
    assert!(
        found_prompt,
        "T3 thread never received the persisted phone prompt"
    );
    assert!(found_reply, "T3 thread never received the agent reply");
    let initial_messages = latest_detail["thread"]["messages"].as_array().unwrap();
    assert_eq!(
        initial_messages
            .iter()
            .filter(|message| message["role"] == "assistant")
            .count(),
        1,
        "ACP text chunks must project as one stable T3 assistant message"
    );
    assert!(initial_messages.iter().any(|message| {
        message["role"] == "user"
            && message["id"] == "mobile-message-1"
            && message["text"] == "hello from phone"
    }));
    socket
        .send(Message::Text(
            json!({ "_tag": "Interrupt", "requestId": "thread-sub" }).to_string(),
        ))
        .await
        .unwrap();

    let mut idle = false;
    for _ in 0..40 {
        let (_, current) = http(
            addr,
            "GET",
            &format!("/api/orchestration/threads/{thread_id}"),
            None,
            Some(bearer),
            "",
        )
        .await;
        idle = current["thread"]["session"]["status"] == "ready";
        if idle {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(idle, "initial phone turn never returned to ready");

    // This adapter advertises non-paginated thread snapshots, so a history larger than the
    // core's 50-turn page limit must still be returned in full to the mobile client.
    let core_id = store.list_sessions().unwrap()[0].id.clone();
    for turn in 0..55 {
        store
            .append_part(
                &core_id,
                Role::User,
                &Part::Prompt {
                    text: format!("history prompt {turn}"),
                    display: format!("history prompt {turn}"),
                },
            )
            .unwrap();
        store
            .append_part(
                &core_id,
                Role::Agent,
                &Part::Text {
                    text: format!("history reply {turn}"),
                },
            )
            .unwrap();
    }
    let (status, history) = http(
        addr,
        "GET",
        &format!("/api/orchestration/threads/{thread_id}"),
        None,
        Some(bearer),
        "",
    )
    .await;
    assert_eq!(status, 200, "full thread snapshot failed: {history}");
    let history_messages = history["thread"]["messages"].as_array().unwrap();
    assert_eq!(history_messages.len(), 112);
    assert_eq!(history_messages[0]["text"], "hello from phone");
    assert_eq!(history_messages[111]["text"], "history reply 54");

    socket
        .send(Message::Text(
            json!({
                "_tag": "Request",
                "id": "mode-1",
                "tag": "orchestration.dispatchCommand",
                "payload": {
                    "type": "thread.interaction-mode.set",
                    "commandId": "mobile-mode-1",
                    "threadId": thread_id,
                    "interactionMode": "plan",
                    "createdAt": "2026-08-12T00:01:00.000Z",
                },
                "headers": [],
            })
            .to_string(),
        ))
        .await
        .unwrap();
    let mode_receipt = next_json(&mut socket).await;
    assert_eq!(mode_receipt["exit"]["_tag"], "Failure");

    let (status, long_detail) = http(
        addr,
        "GET",
        &format!("/api/orchestration/threads/{thread_id}"),
        None,
        Some(bearer),
        "",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(long_detail["thread"]["interactionMode"], "default");

    let long_text = format!("answer this exact long request: {}", "x".repeat(700));
    let long_command = json!({
        "type": "thread.turn.start",
        "commandId": "mobile-turn-long",
        "threadId": thread_id,
        "message": {
            "messageId": "mobile-message-long",
            "role": "user",
            "text": long_text,
            "attachments": [],
        },
        "runtimeMode": "approval-required",
        // A rejected Plan request leaves ordinary turns available.
        "interactionMode": "default",
        "modelSelection": { "instanceId": "t3mock", "model": "phone-model" },
        "createdAt": "2026-08-12T00:02:00.000Z",
    });
    socket
        .send(Message::Text(
            json!({
                "_tag": "Request",
                "id": "dispatch-long",
                "tag": "orchestration.dispatchCommand",
                "payload": long_command,
                "headers": [],
            })
            .to_string(),
        ))
        .await
        .unwrap();
    let long_receipt = next_json(&mut socket).await;
    assert_eq!(long_receipt["exit"]["_tag"], "Success");

    let mut long_snapshot = Value::Null;
    for _ in 0..40 {
        let (_, current) = http(
            addr,
            "GET",
            &format!("/api/orchestration/threads/{thread_id}"),
            None,
            Some(bearer),
            "",
        )
        .await;
        let complete = current["thread"]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "assistant" && message["text"] == "long reply");
        long_snapshot = current;
        if complete {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let long_messages = long_snapshot["thread"]["messages"].as_array().unwrap();
    assert_eq!(long_snapshot["thread"]["interactionMode"], "default");
    assert!(long_messages
        .iter()
        .any(|message| message["role"] == "user" && message["text"] == long_text));
    assert!(long_messages
        .iter()
        .any(|message| message["role"] == "assistant" && message["text"] == "long reply"));
    assert!(!long_messages.iter().any(|message| message["text"]
        .as_str()
        .is_some_and(|text| text.contains("[skill:plan-first]"))));
    assert_eq!(store.list_sessions().unwrap().len(), 1);

    // Dispatch Success means the core accepted the prompt, not merely that it enqueued an Op.
    // Keep one turn open, then verify a second command gets a schema-shaped Failure and is never
    // removed from the mobile outbox as a false success.
    for _ in 0..40 {
        let (_, current) = http(
            addr,
            "GET",
            &format!("/api/orchestration/threads/{thread_id}"),
            None,
            Some(bearer),
            "",
        )
        .await;
        if current["thread"]["session"]["status"] == "ready" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let slow_command = json!({
        "type": "thread.turn.start",
        "commandId": "mobile-turn-slow",
        "threadId": thread_id,
        "message": {
            "messageId": "mobile-message-slow",
            "role": "user",
            "text": "hold open from phone",
            "attachments": [],
        },
        "runtimeMode": "approval-required",
        "interactionMode": "default",
        "modelSelection": { "instanceId": "t3mock", "model": "phone-model" },
        "createdAt": "2026-08-12T00:03:00.000Z",
    });
    socket
        .send(Message::Text(
            json!({
                "_tag": "Request",
                "id": "dispatch-slow",
                "tag": "orchestration.dispatchCommand",
                "payload": slow_command,
                "headers": [],
            })
            .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(next_json(&mut socket).await["exit"]["_tag"], "Success");

    socket
        .send(Message::Text(
            json!({
                "_tag": "Request",
                "id": "dispatch-rejected",
                "tag": "orchestration.dispatchCommand",
                "payload": {
                    "type": "thread.turn.start",
                    "commandId": "mobile-turn-rejected",
                    "threadId": thread_id,
                    "message": {
                        "messageId": "mobile-message-rejected",
                        "role": "user",
                        "text": "must be rejected while busy",
                        "attachments": [],
                    },
                    "runtimeMode": "approval-required",
                    "interactionMode": "default",
                    "modelSelection": { "instanceId": "t3mock", "model": "phone-model" },
                    "createdAt": "2026-08-12T00:03:01.000Z",
                },
                "headers": [],
            })
            .to_string(),
        ))
        .await
        .unwrap();
    let rejected = next_json(&mut socket).await;
    assert_eq!(rejected["exit"]["_tag"], "Failure");
    assert_eq!(
        rejected["exit"]["cause"][0]["error"]["_tag"],
        "OrchestrationDispatchCommandError"
    );

    let mut slow_snapshot = Value::Null;
    for _ in 0..80 {
        let (_, current) = http(
            addr,
            "GET",
            &format!("/api/orchestration/threads/{thread_id}"),
            None,
            Some(bearer),
            "",
        )
        .await;
        let ready = current["thread"]["session"]["status"] == "ready";
        slow_snapshot = current;
        if ready {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let slow_messages = slow_snapshot["thread"]["messages"].as_array().unwrap();
    assert!(slow_messages
        .iter()
        .any(|message| message["role"] == "user" && message["text"] == "hold open from phone"));
    assert!(!slow_messages.iter().any(|message| {
        message["role"] == "user" && message["text"] == "must be rejected while busy"
    }));

    if std::env::var_os("CODETWO_DUMP_T3_FIXTURES").is_some() {
        eprintln!("T3_THREAD_FIXTURE={long_snapshot}");
    }

    drop(socket);
    server.abort();
    let _ = server.await;

    // Simulate both adapter crash boundaries: SQLite accepted the session before its public alias
    // landed, and accepted prompts before the adapter's secondary command-receipt cache landed.
    // The two SQLite receipts must repair the alias and suppress every duplicate on replay.
    let compatibility_path = temp.join("t3-compatibility.json");
    let mut compatibility: Value =
        serde_json::from_slice(&std::fs::read(&compatibility_path).unwrap()).unwrap();
    compatibility["commandReceipts"] = json!([]);
    compatibility["aliases"] = json!({});
    compatibility["interactionModes"] = json!({thread_id: "plan"});
    std::fs::write(
        &compatibility_path,
        serde_json::to_vec_pretty(&compatibility).unwrap(),
    )
    .unwrap();

    // T3 mobile owns this public id and keeps it as its cache key. Reload around the same durable
    // C2 store and confirm the id survives while retired persisted Plan state is ignored.
    let (restart_engine, restart_rx) = Engine::with_store(
        vec![provider],
        SkillLibrary::new(builtin_skills()),
        store.clone(),
    );
    let restart_auth = Arc::new(AuthState::load(Some(auth_path)));
    let (restart_addr, restart_server) = bind_and_serve(
        Arc::new(restart_engine),
        fanout(restart_rx),
        "127.0.0.1:0".parse().unwrap(),
        restart_auth.clone(),
    )
    .await
    .unwrap();
    let (status, restart_ticket_reply) = http(
        restart_addr,
        "POST",
        "/api/auth/websocket-ticket",
        None,
        Some(bearer),
        "",
    )
    .await;
    assert_eq!(status, 200);
    assert!(restart_ticket_reply["ticket"].is_string());

    let (status, recovered_create) = http(
        restart_addr,
        "POST",
        "/api/orchestration/dispatch",
        Some("application/json"),
        Some(bearer),
        &create_command.to_string(),
    )
    .await;
    assert_eq!(
        status, 200,
        "durable session receipt did not repair the public alias: {recovered_create}"
    );
    assert_eq!(store.list_sessions().unwrap().len(), 1);

    let (status, restarted_shell) = http(
        restart_addr,
        "GET",
        "/api/orchestration/shell",
        None,
        Some(bearer),
        "",
    )
    .await;
    assert_eq!(status, 200);
    assert!(restarted_shell["threads"]
        .as_array()
        .unwrap()
        .iter()
        .any(|thread| thread["id"] == thread_id));
    let (status, restarted_detail) = http(
        restart_addr,
        "GET",
        &format!("/api/orchestration/threads/{thread_id}"),
        None,
        Some(bearer),
        "",
    )
    .await;
    assert_eq!(
        status, 200,
        "reloaded thread snapshot failed: {restarted_detail}"
    );
    assert_eq!(restarted_detail["thread"]["id"], thread_id);
    assert_eq!(restarted_detail["thread"]["interactionMode"], "default");
    assert_eq!(
        restarted_detail["thread"]["messages"]
            .as_array()
            .unwrap()
            .len(),
        116
    );

    // The persisted receipt survives a restart. Retrying the exact command id returns its prior
    // success without appending a duplicate prompt or creating another core session.
    let (status, replay_receipt) = http(
        restart_addr,
        "POST",
        "/api/orchestration/dispatch",
        Some("application/json"),
        Some(bearer),
        &long_command.to_string(),
    )
    .await;
    assert_eq!(
        status, 200,
        "persisted receipt replay failed: {replay_receipt}"
    );
    assert!(replay_receipt["sequence"].is_u64());
    let (_, after_replay) = http(
        restart_addr,
        "GET",
        &format!("/api/orchestration/threads/{thread_id}"),
        None,
        Some(bearer),
        "",
    )
    .await;
    assert_eq!(
        after_replay["thread"]["messages"].as_array().unwrap().len(),
        116
    );
    assert_eq!(restarted_shell["threads"].as_array().unwrap().len(), 1);

    // Revoke closes already-open sockets and a ticket redeemed after revocation is closed during
    // handler startup, covering both sides of the redeem-to-subscribe race.
    let (_, live_ticket_reply) = http(
        restart_addr,
        "POST",
        "/api/auth/websocket-ticket",
        None,
        Some(bearer),
        "",
    )
    .await;
    let (_, stale_ticket_reply) = http(
        restart_addr,
        "POST",
        "/api/auth/websocket-ticket",
        None,
        Some(bearer),
        "",
    )
    .await;
    let (mut live_socket, _) = tokio_tungstenite::connect_async(format!(
        "ws://{restart_addr}/ws?wsTicket={}",
        live_ticket_reply["ticket"].as_str().unwrap()
    ))
    .await
    .unwrap();
    let device_id = restart_auth.list_devices()[0].id.clone();
    assert!(restart_auth.revoke_device(&device_id));
    assert_socket_closes(&mut live_socket).await;

    let stale_connection = tokio_tungstenite::connect_async(format!(
        "ws://{restart_addr}/ws?wsTicket={}",
        stale_ticket_reply["ticket"].as_str().unwrap()
    ))
    .await
    .expect_err("revoked device ticket must not upgrade");
    assert!(matches!(
        stale_connection,
        tokio_tungstenite::tungstenite::Error::Http(response)
            if response.status() == tokio_tungstenite::tungstenite::http::StatusCode::UNAUTHORIZED
    ));

    restart_server.abort();
    let _ = restart_server.await;
    let _ = std::fs::remove_dir_all(temp);
}

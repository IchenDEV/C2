//! HTTP integration tests for `POST /external-mcp`.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use codetwo_core::external_mcp::clients::{ExternalScope, ProjectScope, TtlChoice};
use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
use codetwo_core::skill::SkillLibrary;
use codetwo_core::Engine;
use codetwo_server::{bind_and_serve, fanout, AuthState};
use serde_json::json;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn http_raw(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (u16, String) {
    let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let has_host = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("host"));
    let mut header_lines = format!("{method} {path} HTTP/1.1\r\n");
    if !has_host {
        header_lines.push_str(&format!("Host: 127.0.0.1:{}\r\n", addr.port()));
    }
    header_lines.push_str(&format!(
        "Connection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    ));
    for (name, value) in headers {
        header_lines.push_str(&format!("{name}: {value}\r\n"));
    }
    header_lines.push('\n');
    let request = format!("{header_lines}{body}");
    let (mut reader, mut writer) = stream.into_split();
    let read = async {
        let mut raw = Vec::new();
        if let Err(error) = reader.read_to_end(&mut raw).await {
            assert!(matches!(error.kind(), std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe), "{error}");
        }
        raw
    };
    let write = async {
        // Read concurrently so an early rejection isn't lost while the peer closes its reader.
        let _ = writer.write_all(request.as_bytes()).await;
    };
    let (raw, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(read, write)
    }).await.expect("bounded HTTP response");
    let text = String::from_utf8_lossy(&raw).to_string();
    let status: u16 = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

#[tokio::test]
async fn external_mcp_http_serves_tools_when_enabled() {
    let dir = TempDir::new().unwrap();
    let (engine, events_rx) = Engine::new(
        vec![Provider {
            id: ProviderId::ClaudeCode,
            display_name: "Claude".into(),
            needs_node: false,
            launch: LaunchSpec {
                command: "false".into(),
                args: Vec::new(),
                env: Vec::new(),
                cwd: None,
            },
        }],
        SkillLibrary::default(),
    );
    engine.external_mcp_state().set_enabled(true);
    engine
        .external_mcp_state()
        .configure_data_dir(dir.path())
        .unwrap();
    let token = engine
        .external_mcp_state()
        .with_registry(|reg| {
            reg.create(
                "http-test",
                vec![ExternalScope::Read],
                ProjectScope::All,
                TtlChoice::Default30Days,
            )
            .unwrap()
            .1
        })
        .unwrap();
    let engine = Arc::new(engine);
    let events = fanout(events_rx);
    let auth = Arc::new(AuthState::load(None));
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let (bound, _handle) = bind_and_serve(engine.clone(), events, addr, auth)
        .await
        .unwrap();

    let (status, _) = http_raw(bound, "POST", "/external-mcp", &[], "{}").await;
    assert_eq!(status, 401);

    let auth_header = format!("Bearer {token}");
    let list_body = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}).to_string();
    let (status, body) = http_raw(
        bound,
        "POST",
        "/external-mcp",
        &[("Authorization", &auth_header)],
        &list_body,
    )
    .await;
    assert_eq!(status, 200);
    assert!(body.contains("codetwo_capabilities"));

    let call_body = json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"codetwo_capabilities","arguments":{}}
    })
    .to_string();
    let (status, body) = http_raw(
        bound,
        "POST",
        "/external-mcp",
        &[("Authorization", &auth_header)],
        &call_body,
    )
    .await;
    assert_eq!(status, 200);
    assert!(body.contains("external_mcp"));

    // Exercise the real route, not a test router that calls the streaming helper directly.
    let listen = json!({"jsonrpc":"2.0","id":77,"method":"subscriptions/listen","params":{
        "_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"},
        "notifications":{"resourceSubscriptions":["codetwo://events"]}
    }})
    .to_string();
    let mut stream = tokio::net::TcpStream::connect(bound).await.unwrap();
    let request = format!("POST /external-mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: {auth_header}\r\nAccept: text/event-stream\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{listen}", bound.port(), listen.len());
    stream.write_all(request.as_bytes()).await.unwrap();
    let received = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let mut seen = String::new();
        loop {
            let mut bytes = [0; 4096];
            let count = stream.read(&mut bytes).await.unwrap();
            assert!(count > 0, "stream closed before acknowledgment: {seen}");
            seen.push_str(&String::from_utf8_lossy(&bytes[..count]));
            if seen.contains("notifications/subscriptions/acknowledged") {
                break seen;
            }
        }
    })
    .await
    .expect("real POST SSE acknowledgment");
    assert!(received.starts_with("HTTP/1.1 200"));
    assert!(received.to_lowercase().contains("text/event-stream"));
    assert!(received.contains("subscriptionId\":77"));
}

#[tokio::test]
async fn external_mcp_rejects_bad_host_and_oversize_body() {
    let dir = TempDir::new().unwrap();
    let (engine, events_rx) = Engine::new(
        vec![Provider {
            id: ProviderId::ClaudeCode,
            display_name: "Claude".into(),
            needs_node: false,
            launch: LaunchSpec {
                command: "false".into(),
                args: Vec::new(),
                env: Vec::new(),
                cwd: None,
            },
        }],
        SkillLibrary::default(),
    );
    engine.external_mcp_state().set_enabled(true);
    engine
        .external_mcp_state()
        .configure_data_dir(dir.path())
        .unwrap();
    let token = engine
        .external_mcp_state()
        .with_registry(|reg| {
            reg.create(
                "http-test",
                vec![ExternalScope::Read],
                ProjectScope::All,
                TtlChoice::Default30Days,
            )
            .unwrap()
            .1
        })
        .unwrap();
    let engine = Arc::new(engine);
    let events = fanout(events_rx);
    let auth = Arc::new(AuthState::load(None));
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let (bound, _handle) = bind_and_serve(engine, events, addr, auth).await.unwrap();

    let (status, _) = http_raw(
        bound,
        "POST",
        "/external-mcp",
        &[
            ("Host", "evil.example"),
            ("Authorization", &format!("Bearer {token}")),
        ],
        "{}",
    )
    .await;
    assert_eq!(status, 403);

    let huge = "x".repeat(300 * 1024);
    let (status, _) = http_raw(
        bound,
        "POST",
        "/external-mcp",
        &[("Authorization", &format!("Bearer {token}"))],
        &huge,
    )
    .await;
    assert_eq!(status, 413);
}

#[tokio::test]
async fn external_mcp_disabled_returns_404() {
    let (engine, events_rx) = Engine::new(
        vec![Provider {
            id: ProviderId::ClaudeCode,
            display_name: "Claude".into(),
            needs_node: false,
            launch: LaunchSpec {
                command: "false".into(),
                args: Vec::new(),
                env: Vec::new(),
                cwd: None,
            },
        }],
        SkillLibrary::default(),
    );
    let engine = Arc::new(engine);
    let events = fanout(events_rx);
    let auth = Arc::new(AuthState::load(None));
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let (bound, _handle) = bind_and_serve(engine, events, addr, auth).await.unwrap();
    let (status, _) = http_raw(
        bound,
        "POST",
        "/external-mcp",
        &[("Authorization", "Bearer ctmcp_test")],
        "{}",
    )
    .await;
    assert_eq!(status, 404);
}

async fn http_status_one_keepalive(
    addr: SocketAddr,
    stream: &mut tokio::net::TcpStream,
    auth: &str,
    body: &str,
) -> u16 {
    let request = format!(
        "POST /external-mcp HTTP/1.1\r\n\
         Host: 127.0.0.1:{}\r\n\
         Connection: keep-alive\r\n\
         Authorization: {auth}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\r\n{body}",
        addr.port(),
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf[..n]);
    text.split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

#[tokio::test]
async fn external_mcp_auth_failures_rate_limited() {
    let dir = TempDir::new().unwrap();
    let (engine, events_rx) = Engine::new(
        vec![Provider {
            id: ProviderId::ClaudeCode,
            display_name: "Claude".into(),
            needs_node: false,
            launch: LaunchSpec {
                command: "false".into(),
                args: Vec::new(),
                env: Vec::new(),
                cwd: None,
            },
        }],
        SkillLibrary::default(),
    );
    engine.external_mcp_state().set_enabled(true);
    engine
        .external_mcp_state()
        .configure_data_dir(dir.path())
        .unwrap();
    let engine = Arc::new(engine);
    let events = fanout(events_rx);
    let auth = Arc::new(AuthState::load(None));
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let (bound, _handle) = bind_and_serve(engine, events, addr, auth).await.unwrap();
    let bad = "Bearer ctmcp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    for _ in 0..20 {
        let mut stream = tokio::net::TcpStream::connect(bound).await.unwrap();
        let status = http_status_one_keepalive(bound, &mut stream, bad, "{}").await;
        assert!(status == 401 || status == 200);
    }
    let mut stream = tokio::net::TcpStream::connect(bound).await.unwrap();
    let status = http_status_one_keepalive(bound, &mut stream, bad, "{}").await;
    assert_eq!(status, 429);
}

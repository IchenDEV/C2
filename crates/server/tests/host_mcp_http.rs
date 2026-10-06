//! HTTP integration tests for loopback `POST /mcp`.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use codetwo_core::host_mcp::{HostMcpCapability, HostMcpScope, PROTOCOL_VERSION};
use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
use codetwo_core::skill::SkillLibrary;
use codetwo_core::Engine;
use codetwo_server::{bind_and_serve, fanout, host_mcp, AuthState};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn http_raw(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (u16, String) {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
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
    stream
        .write_all(format!("{header_lines}{body}").as_bytes())
        .await
        .unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
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
async fn host_mcp_http_authenticates_and_serves_json_rpc() {
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
    engine.set_host_mcp_enabled(true);
    let engine = Arc::new(engine);
    let events = fanout(events_rx);
    let auth = Arc::new(AuthState::load(None));
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let (bound, _handle) = bind_and_serve(engine.clone(), events, addr, auth)
        .await
        .unwrap();
    assert_eq!(
        engine.host_mcp_http_endpoint().unwrap(),
        format!("http://127.0.0.1:{}/mcp", bound.port())
    );

    let (status, _) = http_raw(bound, "POST", "/mcp", &[], "{}").await;
    assert_eq!(status, 401);

    let (_id, token) = engine
        .host_mcp_state()
        .registry()
        .lock()
        .unwrap()
        .issue(HostMcpScope {
            session_id: "sess-http".into(),
            provider_id: "claude_code".into(),
            capabilities: HostMcpCapability::default_read_only_set(),
        })
        .unwrap();

    let body = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion": PROTOCOL_VERSION}}).to_string();
    let (status, response) = http_raw(
        bound,
        "POST",
        "/mcp",
        &[("Authorization", &format!("Bearer {token}"))],
        &body,
    )
    .await;
    assert_eq!(status, 200);
    assert!(response.contains(PROTOCOL_VERSION));

    let oversize = "x".repeat(host_mcp::MAX_HOST_MCP_BODY_BYTES + 1);
    let (status, _) = http_raw(
        bound,
        "POST",
        "/mcp",
        &[("Authorization", &format!("Bearer {token}"))],
        &oversize,
    )
    .await;
    assert_eq!(status, 413);

    let (status, _) = http_raw(
        bound,
        "POST",
        "/mcp",
        &[
            ("Authorization", &format!("Bearer {token}")),
            ("Host", "evil.test"),
        ],
        &body,
    )
    .await;
    assert_eq!(status, 403);

    // A browser-style opaque or foreign Origin is refused even from loopback.
    for origin in ["null", "https://evil.test", "chrome-extension://abc"] {
        let (status, _) = http_raw(
            bound,
            "POST",
            "/mcp",
            &[
                ("Authorization", &format!("Bearer {token}")),
                ("Origin", origin),
            ],
            &body,
        )
        .await;
        assert_eq!(status, 403, "origin {origin}");
    }
}

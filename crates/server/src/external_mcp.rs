//! `POST /external-mcp`: external MCP endpoint (default off).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use codetwo_core::external_mcp::state::host_allowed;
use codetwo_core::Engine;
use serde_json::Value;

pub const MAX_EXTERNAL_MCP_BODY_BYTES: usize = 256 * 1024;

const AUTH_FAIL_LIMIT: u32 = 20;
const AUTH_FAIL_WINDOW: Duration = Duration::from_secs(60);

#[derive(Default)]
struct PeerAuthFailures {
    count: u32,
    window_start: Option<Instant>,
}

static AUTH_FAILURES: std::sync::LazyLock<Mutex<HashMap<String, PeerAuthFailures>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
static DISPATCH_LIMIT: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(64)));
static CLIENT_CALLS: std::sync::LazyLock<Mutex<HashMap<String, usize>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

struct ClientCall(String);
impl Drop for ClientCall {
    fn drop(&mut self) {
        let mut calls = CLIENT_CALLS.lock().unwrap();
        if let Some(count) = calls.get_mut(&self.0) {
            *count -= 1;
            if *count == 0 {
                calls.remove(&self.0);
            }
        }
    }
}

pub struct ExternalMcpGuard;

impl ExternalMcpGuard {
    pub fn set_allowed_hosts(hosts: Vec<String>) {
        codetwo_core::external_mcp::state::set_allowed_hosts(hosts);
    }
}

pub(crate) fn record_auth_failure(key: String) -> bool {
    let mut map = AUTH_FAILURES.lock().expect("auth failure map");
    let now = Instant::now();
    map.retain(|_, entry| {
        entry
            .window_start
            .is_some_and(|start| now.duration_since(start) < AUTH_FAIL_WINDOW)
    });
    if map.len() >= 1024 && !map.contains_key(&key) {
        return true;
    }
    let entry = map.entry(key).or_default();
    if entry
        .window_start
        .is_none_or(|start| now.duration_since(start) > AUTH_FAIL_WINDOW)
    {
        entry.count = 0;
        entry.window_start = Some(now);
    }
    entry.count += 1;
    entry.count > AUTH_FAIL_LIMIT
}

pub(crate) fn clear_auth_failures(key: &str) {
    AUTH_FAILURES.lock().expect("auth failure map").remove(key);
}

fn parse_bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

fn reject_transport(
    headers: &HeaderMap,
    peer: SocketAddr,
    _remote_allowed: bool,
) -> Option<Response> {
    if !peer.ip().is_loopback() {
        return Some((StatusCode::FORBIDDEN, "loopback only").into_response());
    }
    if let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) {
        if !host_allowed(host) {
            return Some((StatusCode::FORBIDDEN, "invalid Host").into_response());
        }
    }
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let allowed = origin
            .strip_prefix("http://")
            .or_else(|| origin.strip_prefix("https://"))
            .map(|rest| rest.split('/').next().unwrap_or(rest))
            .is_some_and(host_allowed);
        if !allowed {
            return Some((StatusCode::FORBIDDEN, "invalid Origin").into_response());
        }
    }
    None
}

pub async fn post_external_mcp(
    State(engine): State<Arc<Engine>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !engine.external_mcp_state().is_enabled() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let remote_allowed = crate::remote::remote_enabled();
    if let Some(response) = reject_transport(&headers, peer, remote_allowed) {
        return response;
    }
    if body.len() > MAX_EXTERNAL_MCP_BODY_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "body too large").into_response();
    }
    let bearer = match parse_bearer(&headers) {
        Some(token) => token,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Bearer")],
                "bearer required",
            )
                .into_response();
        }
    };
    let source_key = format!(
        "{:p}:{}",
        Arc::as_ptr(&engine),
        crate::remote::auth_limiter_source_key(&headers, peer)
    );
    let resolving_engine = engine.clone();
    let resolving_bearer = bearer.clone();
    let resolved = tokio::task::spawn_blocking(move || {
        resolving_engine.resolve_external_client(&resolving_bearer)
    })
    .await;
    let client = match resolved {
        Ok(Ok(client)) => {
            clear_auth_failures(&source_key);
            client
        }
        _ => {
            return if record_auth_failure(source_key) {
                (StatusCode::TOO_MANY_REQUESTS, "too many auth failures").into_response()
            } else {
                (StatusCode::UNAUTHORIZED, "invalid credential").into_response()
            }
        }
    };
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid json").into_response(),
    };
    if crate::external_mcp_stream::is_post_stream_request(&payload) {
        if payload.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return (StatusCode::BAD_REQUEST, "invalid jsonrpc version").into_response();
        }
        return crate::external_mcp_stream::post_events_stream(engine, &headers, bearer, payload)
            .await;
    }
    let permit = match DISPATCH_LIMIT.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return (StatusCode::TOO_MANY_REQUESTS, "dispatch capacity reached").into_response()
        }
    };
    let client_call = {
        let mut calls = CLIENT_CALLS.lock().unwrap();
        let count = calls.entry(client.record.id.clone()).or_default();
        if *count >= 8 {
            return (StatusCode::TOO_MANY_REQUESTS, "client concurrency reached").into_response();
        }
        *count += 1;
        ClientCall(client.record.id)
    };
    let notification = payload.get("id").is_none()
        && payload
            .get("method")
            .and_then(Value::as_str)
            .is_some_and(|method| method.starts_with("notifications/"));
    // Tool dispatch can wait or drive async Engine operations. Keep it off Tokio workers.
    let response = match tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let _client_call = client_call;
        engine.handle_external_mcp_json(&bearer, &payload)
    })
    .await
    {
        Ok(response) => response,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "MCP dispatch failed").into_response()
        }
    };
    if notification {
        return StatusCode::ACCEPTED.into_response();
    }
    (StatusCode::OK, Json(response)).into_response()
}

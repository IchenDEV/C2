//! Loopback-only Streamable HTTP MCP entry (`POST /mcp`).

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use codetwo_core::Engine;
use serde_json::Value;

pub const MAX_HOST_MCP_BODY_BYTES: usize = 256 * 1024;

pub fn is_loopback_host(value: &str) -> bool {
    let host = value.split(':').next().unwrap_or(value).trim();
    host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "::1"
        || host == "[::1]"
}

fn reject_non_loopback(headers: &HeaderMap, peer: SocketAddr) -> Option<Response> {
    if !peer.ip().is_loopback() {
        return Some((StatusCode::FORBIDDEN, "loopback only").into_response());
    }
    if let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) {
        if !is_loopback_host(host) {
            return Some((StatusCode::FORBIDDEN, "invalid Host").into_response());
        }
    }
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        // Native MCP clients send no Origin. A browser always does, so anything that is not an
        // http(s) loopback origin (including `null` and other schemes) is rejected.
        let allowed = origin
            .strip_prefix("http://")
            .or_else(|| origin.strip_prefix("https://"))
            .map(|rest| rest.split('/').next().unwrap_or(rest))
            .is_some_and(is_loopback_host);
        if !allowed {
            return Some((StatusCode::FORBIDDEN, "invalid Origin").into_response());
        }
    }
    None
}

fn parse_bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

pub async fn post_host_mcp(
    State(engine): State<Arc<Engine>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(response) = reject_non_loopback(&headers, peer) {
        return response;
    }
    if body.len() > MAX_HOST_MCP_BODY_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "body too large").into_response();
    }
    let bearer = match parse_bearer(&headers) {
        Some(token) => token,
        None => return (StatusCode::UNAUTHORIZED, "bearer required").into_response(),
    };
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid json").into_response(),
    };
    let response = engine.handle_host_mcp_json(&bearer, &payload);
    (StatusCode::OK, Json(response)).into_response()
}

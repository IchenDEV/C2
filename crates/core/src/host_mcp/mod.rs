//! Built-in host MCP surface (`codetwo` server).
//!
//! Providers receive an HTTP MCP entry (Streamable HTTP, protocol `2025-06-18`) authenticated with
//! a per-session bearer. This module implements credential issuance, JSON-RPC handling, and
//! read-only tools. Responses are plain JSON bodies; SSE streaming is not required for the current
//! tool set (full-duplex streaming may be added later for long-running tools).

mod protocol;
mod registry;
pub(crate) mod tools;

pub use protocol::{handle_request, HostMcpJsonRpcError, McpToolHost, PROTOCOL_VERSION};
pub use registry::{
    HostMcpCapability, HostMcpRegistry, HostMcpScope, ResolvedHostMcpCredential, CREDENTIAL_TTL,
    HOST_MCP_SERVER_NAME,
};
pub use tools::{tool_catalog, TOOL_CAPABILITIES, TOOL_SESSION_LIST, TOOL_SESSION_READ};

use crate::host_mcp::protocol::SERVER_NAME;
use crate::skill::{McpServer, McpTransport};
use crate::Engine;
use serde_json::Value;
use std::sync::{Arc, Mutex, RwLock};

const HOST_MCP_ENV: &str = "CODETWO_HOST_MCP";

/// Minimal provenance record for host MCP invocations (no secrets, no tool arguments).
#[derive(Debug, Clone)]
pub struct HostMcpAuditRecord {
    pub session_id: String,
    pub provider_id: String,
    pub tool: String,
    pub allowed: bool,
    pub reason: String,
}

#[derive(Clone)]
pub struct HostMcpDispatch {
    engine: Engine,
    registry: Arc<Mutex<HostMcpRegistry>>,
    bearer: String,
}

impl protocol::McpToolHost for HostMcpDispatch {
    fn server_name(&self) -> &str {
        SERVER_NAME
    }

    fn catalog(&self) -> Vec<Value> {
        tool_catalog()
    }

    fn call_tool(&self, tool: &str, arguments: &Value) -> Result<Value, String> {
        HostMcpDispatch::call_tool(self, tool, arguments)
    }
}

impl HostMcpDispatch {
    pub fn new(engine: Engine, registry: Arc<Mutex<HostMcpRegistry>>, bearer: String) -> Self {
        Self {
            engine,
            registry,
            bearer,
        }
    }

    pub fn call_tool(&self, tool: &str, arguments: &Value) -> Result<Value, String> {
        let mut registry = self.registry.lock().map_err(|_| "registry lock poisoned")?;
        let resolved = registry
            .resolve(&self.bearer)
            .ok_or_else(|| "invalid or expired credential".to_string())?;
        drop(registry);
        self.engine
            .authorize_host_mcp_call(&resolved, tool, arguments)
    }
}

#[derive(Clone, Default)]
pub struct HostMcpState {
    registry: Arc<Mutex<HostMcpRegistry>>,
    enabled: Arc<std::sync::atomic::AtomicBool>,
    endpoint: Arc<RwLock<Option<String>>>,
}

impl HostMcpState {
    pub fn new() -> Self {
        let enabled = std::env::var(HOST_MCP_ENV)
            .ok()
            .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
        Self {
            registry: Arc::new(Mutex::new(HostMcpRegistry::new())),
            enabled: Arc::new(std::sync::atomic::AtomicBool::new(enabled)),
            endpoint: Arc::new(RwLock::new(None)),
        }
    }

    pub fn registry(&self) -> Arc<Mutex<HostMcpRegistry>> {
        self.registry.clone()
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled
            .store(enabled, std::sync::atomic::Ordering::Release);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn set_endpoint(&self, url: Option<String>) {
        *self.endpoint.write().unwrap() = url;
    }

    pub fn endpoint(&self) -> Option<String> {
        self.endpoint.read().unwrap().clone()
    }

    pub fn injection_ready(&self) -> bool {
        self.is_enabled() && self.endpoint().is_some()
    }

    pub fn handle_http_json(&self, engine: &Engine, bearer: &str, body: &Value) -> Value {
        let dispatch =
            HostMcpDispatch::new(engine.clone(), self.registry.clone(), bearer.to_string());
        handle_request(&dispatch, body)
    }

    pub fn build_server(&self, bearer: &str, url: &str) -> McpServer {
        McpServer {
            name: HOST_MCP_SERVER_NAME.into(),
            cwd: None,
            transport: McpTransport::Http {
                url: url.to_string(),
                headers: vec![("Authorization".into(), format!("Bearer {bearer}"))],
            },
        }
    }
}

pub fn host_mcp_enabled_from_env() -> bool {
    std::env::var(HOST_MCP_ENV)
        .ok()
        .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

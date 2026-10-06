//! JSON-RPC dispatch for external MCP (`McpToolHost` + non-tool MCP methods).

use crate::external_mcp::catalog::catalog_for_client;
use crate::external_mcp::clients::ResolveError;
use crate::external_mcp::ctx::{ToolError, ToolErrorKind};
use crate::external_mcp::gate::{public_error, tool_error_message, tool_error_to_rpc_code, SERVER_NAME};
use crate::external_mcp::mcp_events;
use crate::host_mcp::McpToolHost;

use crate::Engine;
use serde_json::{json, Value};

pub struct ExternalDispatch<'a> {
    engine: &'a Engine,
    client: Option<crate::external_mcp::clients::ResolvedClient>,
}

impl ExternalDispatch<'_> {
    pub fn new<'a>(engine: &'a Engine, bearer: &'a str) -> ExternalDispatch<'a> {
        let client = engine.resolve_external_client(bearer).ok();
        ExternalDispatch {
            engine,
            client,
        }
    }
}

impl McpToolHost for ExternalDispatch<'_> {
    fn server_name(&self) -> &str {
        SERVER_NAME
    }

    fn catalog(&self) -> Vec<Value> {
        let Some(client) = &self.client else {
            return Vec::new();
        };
        catalog_for_client(&client.record.scopes)
    }

    fn call_tool(&self, tool: &str, arguments: &Value) -> Result<Value, String> {
        let client = self.client.as_ref().ok_or_else(|| {
            tool_error_message(&ToolError::denied("invalid or expired credential"))
        })?;
        match self
            .engine
            .authorize_external_mcp_call(client, tool, arguments)
        {
            Ok(value) => Ok(value),
            Err(error) if matches!(error.kind, ToolErrorKind::Internal | ToolErrorKind::NotFound) => {
                Err(tool_error_message(&error))
            }
            Err(error) => Err(tool_error_message(&error)),
        }
    }

    fn server_capabilities(&self) -> Value {
        let mut caps = json!({
            "tools": { "listChanged": false }
        });
        merge_capabilities(&mut caps, mcp_events::capabilities());
        caps
    }
}

fn merge_capabilities(target: &mut Value, extra: Value) {
    if let (Some(obj), Value::Object(add)) = (target.as_object_mut(), extra) {
        for (key, value) in add {
            obj.insert(key, value);
        }
    }
}

pub fn handle_external_mcp_json(engine: &Engine, bearer: &str, body: &Value) -> Value {
    if body.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return error_response(body.get("id").cloned(), -32600, "invalid jsonrpc version");
    }
    let method = body.get("method").and_then(Value::as_str);
    let id = body.get("id").cloned();
    let params = body.get("params").cloned().unwrap_or(Value::Null);

    if !engine.external_mcp_state().is_enabled() {
        return error_response(id, -32003, "denied: external MCP surface is disabled");
    }

    if engine.resolve_external_client(bearer).is_err() {
        return error_response(id, -32003, "denied: invalid or expired credential");
    }
    match method {
        Some("initialize" | "ping" | "tools/list" | "notifications/initialized" | "initialized") => {
            crate::host_mcp::handle_request(&ExternalDispatch::new(engine, bearer), body)
        }
        Some("tools/call") => match tools_call(engine, bearer, &params) {
            Ok(result) => success_response(id, result),
            Err(error) => error_response(id, error.code, &error.message),
        },
        Some(other) if other.starts_with("tools/") => {
            error_response(id, -32601, &format!("method not found: {other}"))
        }
        Some(other) if other.starts_with("notifications/") => {
            if id.is_some() {
                return error_response(id, -32601, "notification must not include id");
            }
            json!({"jsonrpc": "2.0"})
        }
        Some(method) => match handle_non_tool_method(engine, bearer, method, &params) {
            Some(Ok(result)) => success_response(id, result),
            Some(Err(error)) => {
                let error = public_error(error);
                error_response(id, tool_error_to_rpc_code(error.kind), &error.message)
            }
            None => error_response(id, -32601, &format!("method not found: {method}")),
        },
        None => error_response(id, -32600, "method required"),
    }
}

fn handle_non_tool_method(
    engine: &Engine,
    bearer: &str,
    method: &str,
    params: &Value,
) -> Option<Result<Value, ToolError>> {
    let client = match engine.resolve_external_client(bearer) {
        Ok(client) => client,
        Err(ResolveError::NotExternalToken) | Err(ResolveError::Unknown) => {
            return Some(Err(ToolError::denied("invalid or expired credential")));
        }
        Err(ResolveError::Expired) => {
            return Some(Err(ToolError::denied("credential expired")));
        }
        Err(ResolveError::Revoked) => {
            return Some(Err(ToolError::denied("credential revoked")));
        }
    };
    mcp_events::handle_method(engine, &client, method, params)
}

struct RpcError {
    code: i64,
    message: String,
}

fn tools_call(engine: &Engine, bearer: &str, params: &Value) -> Result<Value, RpcError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError {
            code: -32602,
            message: "params.name required".into(),
        })?;
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
    if !arguments.is_object() {
        return Err(RpcError {
            code: -32602,
            message: "params.arguments must be an object".into(),
        });
    }
    let client = match engine.resolve_external_client(bearer) {
        Ok(client) => client,
        Err(ResolveError::NotExternalToken) => {
            return Err(RpcError {
                code: -32003,
                message: "denied: not an external MCP credential".into(),
            });
        }
        Err(ResolveError::Expired) => {
            return Err(RpcError {
                code: -32003,
                message: "denied: credential expired".into(),
            });
        }
        Err(ResolveError::Revoked) => {
            return Err(RpcError {
                code: -32003,
                message: "denied: credential revoked".into(),
            });
        }
        Err(ResolveError::Unknown) => {
            return Err(RpcError {
                code: -32003,
                message: "denied: invalid credential".into(),
            });
        }
    };
    match engine.authorize_external_mcp_call(&client, name, &arguments) {
        Ok(content) => Ok(tool_success_payload(&content)),
        Err(error) => {
            let code = tool_error_to_rpc_code(error.kind);
            if matches!(
                error.kind,
                ToolErrorKind::Denied
                    | ToolErrorKind::InvalidParams
                    | ToolErrorKind::NotFound
                    | ToolErrorKind::RateLimited
                    | ToolErrorKind::Conflict
            ) {
                return Err(RpcError {
                    code,
                    message: tool_error_message(&error),
                });
            }
            Ok(tool_error_payload(&error))
        }
    }
}

fn tool_success_payload(content: &Value) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(content).unwrap_or_else(|_| "{}".into())
        }],
        "isError": false
    })
}

fn tool_error_payload(error: &ToolError) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": error.message.clone()
        }],
        "isError": true
    })
}

fn success_response(id: Option<Value>, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "result": result,
    })
}

fn error_response(id: Option<Value>, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "error": {
            "code": code,
            "message": message,
        }
    })
}

impl Engine {
    pub fn handle_external_mcp_json(&self, bearer: &str, body: &Value) -> Value {
        handle_external_mcp_json(self, bearer, body)
    }
}

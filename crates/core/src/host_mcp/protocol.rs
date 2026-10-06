//! Transport-agnostic MCP JSON-RPC (protocol version 2025-06-18).
//!
//! Streamable HTTP may also use SSE; this host surface responds with a single JSON body per POST.

use serde_json::{json, Value};

pub const PROTOCOL_VERSION: &str = "2025-06-18";
pub const SERVER_NAME: &str = "codetwo-host-mcp";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Shared JSON-RPC handler surface for internal host MCP and external MCP.
pub trait McpToolHost {
    fn server_name(&self) -> &str;
    fn catalog(&self) -> Vec<Value>;
    fn call_tool(&self, tool: &str, arguments: &Value) -> Result<Value, String>;

    /// MCP `initialize` capabilities; default disables tool list change notifications.
    fn server_capabilities(&self) -> Value {
        json!({
            "tools": { "listChanged": false }
        })
    }
}

#[derive(Debug, Clone)]
pub struct HostMcpJsonRpcError {
    pub code: i64,
    pub message: String,
}

pub fn handle_request(host: &dyn McpToolHost, body: &Value) -> Value {
    if body.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return error_response(body.get("id").cloned(), -32600, "invalid jsonrpc version");
    }
    let method = body.get("method").and_then(Value::as_str);
    let id = body.get("id").cloned();
    let params = body.get("params").cloned().unwrap_or(Value::Null);
    match method {
        Some("initialize") => success_response(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": host.server_capabilities(),
                "serverInfo": {
                    "name": host.server_name(),
                    "version": SERVER_VERSION,
                }
            }),
        ),
        Some("notifications/initialized") | Some("initialized") => {
            if id.is_some() {
                return error_response(id, -32601, "notification must not include id");
            }
            json!({"jsonrpc": "2.0"})
        }
        Some("ping") => success_response(id, json!({})),
        Some("tools/list") => success_response(id, json!({ "tools": host.catalog() })),
        Some("tools/call") => match tools_call(host, &params) {
            Ok(result) => success_response(id, result),
            Err(error) => error_response(id, error.code, &error.message),
        },
        Some(other) if other.starts_with("notifications/") => {
            if id.is_some() {
                return error_response(id, -32601, "notification must not include id");
            }
            json!({"jsonrpc": "2.0"})
        }
        Some(unknown) => error_response(id, -32601, &format!("method not found: {unknown}")),
        None => error_response(id, -32600, "method required"),
    }
}

fn tools_call(host: &dyn McpToolHost, params: &Value) -> Result<Value, HostMcpJsonRpcError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| HostMcpJsonRpcError {
            code: -32602,
            message: "params.name required".into(),
        })?;
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
    if !arguments.is_object() {
        return Err(HostMcpJsonRpcError {
            code: -32602,
            message: "params.arguments must be an object".into(),
        });
    }
    let content = host
        .call_tool(name, &arguments)
        .map_err(|message| HostMcpJsonRpcError {
            code: json_rpc_tool_error_code(&message),
            message,
        })?;
    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&content).unwrap_or_else(|_| "{}".into())
        }],
        "isError": false
    }))
}

/// Maps tool-layer errors to JSON-RPC codes. `rate limited` uses `-32005` (documented extension).
fn json_rpc_tool_error_code(message: &str) -> i64 {
    if message.contains("denied") {
        -32003
    } else if message.contains("rate limited") {
        -32005
    } else {
        -32603
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeHost {
        name: &'static str,
        tools: Vec<Value>,
        handler: fn(&str, &Value) -> Result<Value, String>,
    }

    impl McpToolHost for FakeHost {
        fn server_name(&self) -> &str {
            self.name
        }

        fn catalog(&self) -> Vec<Value> {
            self.tools.clone()
        }

        fn call_tool(&self, tool: &str, arguments: &Value) -> Result<Value, String> {
            (self.handler)(tool, arguments)
        }
    }

    #[test]
    fn generic_host_serves_initialize_and_catalog() {
        let host = FakeHost {
            name: "fake-external-mcp",
            tools: vec![json!({"name": "codetwo_capabilities"})],
            handler: |_, _| Ok(json!({"ok": true})),
        };
        let init = handle_request(
            &host,
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        );
        assert_eq!(init["result"]["serverInfo"]["name"], "fake-external-mcp");
        assert_eq!(
            init["result"]["capabilities"]["tools"]["listChanged"],
            false
        );

        let list = handle_request(
            &host,
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
        );
        assert_eq!(list["result"]["tools"][0]["name"], "codetwo_capabilities");
    }

    #[test]
    fn rate_limited_maps_to_32005() {
        let host = FakeHost {
            name: "fake",
            tools: vec![],
            handler: |_, _| Err("rate limited".into()),
        };
        let resp = handle_request(
            &host,
            &json!({
                "jsonrpc":"2.0","id":3,"method":"tools/call",
                "params":{"name":"x","arguments":{}}
            }),
        );
        assert_eq!(resp["error"]["code"], -32005);
    }

    #[test]
    fn denied_maps_to_32003() {
        let host = FakeHost {
            name: "fake",
            tools: vec![],
            handler: |_, _| Err("cross-session read denied".into()),
        };
        let resp = handle_request(
            &host,
            &json!({
                "jsonrpc":"2.0","id":4,"method":"tools/call",
                "params":{"name":"x","arguments":{}}
            }),
        );
        assert_eq!(resp["error"]["code"], -32003);
    }
}

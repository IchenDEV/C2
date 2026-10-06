//! Built-in read-only host MCP tools (`codetwo` server).

use crate::host_mcp::registry::HostMcpCapability;
use crate::session::Session;
use crate::Engine;
use serde_json::{json, Value};

pub const TOOL_CAPABILITIES: &str = "codetwo_capabilities";
pub const TOOL_SESSION_LIST: &str = "codetwo_session_list";
pub const TOOL_SESSION_READ: &str = "codetwo_session_read";

pub const SESSION_LIST_LIMIT: usize = 50;

pub fn tool_catalog() -> Vec<Value> {
    vec![
        tool_schema(
            TOOL_CAPABILITIES,
            "List CodeTwo host capabilities, native backend opt-in state, and registered providers.",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            true,
            false,
        ),
        tool_schema(
            TOOL_SESSION_LIST,
            "List sessions visible to the caller (bounded; same project cwd when available).",
            json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": SESSION_LIST_LIMIT,
                        "description": "Maximum sessions to return."
                    }
                },
                "additionalProperties": false
            }),
            true,
            false,
        ),
        tool_schema(
            TOOL_SESSION_READ,
            "Read metadata for one session. Defaults to the caller's own session only.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Target session id; must match caller unless cross-session capability is held."
                    }
                },
                "required": ["session_id"],
                "additionalProperties": false
            }),
            true,
            false,
        ),
    ]
}

fn tool_schema(
    name: &str,
    description: &str,
    input_schema: Value,
    read_only: bool,
    destructive: bool,
) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": destructive,
        }
    })
}

pub fn required_capability(tool: &str) -> Option<HostMcpCapability> {
    match tool {
        TOOL_CAPABILITIES => Some(HostMcpCapability::Capabilities),
        TOOL_SESSION_LIST => Some(HostMcpCapability::SessionList),
        TOOL_SESSION_READ => Some(HostMcpCapability::SessionReadOwn),
        _ => None,
    }
}

/// Extension point: delegate_task, schedule_task, preview_*, device_*, request_secret — not implemented.
pub fn is_reserved_future_tool(tool: &str) -> bool {
    matches!(
        tool,
        "delegate_task"
            | "task_status"
            | "schedule_task"
            | "request_secret"
            | "preview_open"
            | "device_list"
    )
}

pub fn dispatch(
    engine: &Engine,
    caller_session: &str,
    caller_project: Option<&str>,
    capabilities: &[HostMcpCapability],
    tool: &str,
    arguments: &Value,
) -> Result<Value, String> {
    if is_reserved_future_tool(tool) {
        return Err(format!(
            "tool '{tool}' is not available on this host MCP surface"
        ));
    }
    match tool {
        TOOL_CAPABILITIES => Ok(capabilities_payload(engine)),
        TOOL_SESSION_LIST => session_list(
            engine,
            caller_session,
            caller_project,
            capabilities,
            arguments,
        ),
        TOOL_SESSION_READ => session_read(engine, caller_session, capabilities, arguments),
        _ => Err(format!("unknown tool: {tool}")),
    }
}

fn capabilities_payload(engine: &Engine) -> Value {
    let providers: Vec<_> = engine
        .registered_providers()
        .iter()
        .map(|provider| {
            json!({
                "id": provider.id.as_str(),
                "display_name": provider.display_name,
            })
        })
        .collect();
    let native = engine.native_backends_snapshot();
    json!({
        "host": {
            "mcp_protocol": "2025-06-18",
            "transport": "streamable-http-json",
            "note": "This endpoint returns complete JSON-RPC responses; SSE streaming is not required for these tools.",
        },
        "providers": providers,
        "native_backends": {
            "opt_in": native.enabled_native_kind_names(),
            "sidecar_root_configured": native.sidecar_root().is_some(),
        },
        "tools_implemented": [TOOL_CAPABILITIES, TOOL_SESSION_LIST, TOOL_SESSION_READ],
        "tools_extension_point": [
            "delegate_task",
            "task_status",
            "schedule_task",
            "request_secret",
            "preview_*",
            "device_*"
        ],
    })
}

fn session_list(
    engine: &Engine,
    caller_session: &str,
    caller_project: Option<&str>,
    _capabilities: &[HostMcpCapability],
    arguments: &Value,
) -> Result<Value, String> {
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(SESSION_LIST_LIMIT)
        .clamp(1, SESSION_LIST_LIMIT);
    let sessions = engine.list_sessions().map_err(|error| error.to_string())?;
    let caller_project = caller_project.map(str::to_string);
    let filtered: Vec<&Session> = sessions
        .iter()
        .filter(|session| {
            if session.transient {
                return false;
            }
            if let Some(project) = &caller_project {
                let session_project = session
                    .project_path
                    .as_deref()
                    .filter(|path| !path.is_empty())
                    .unwrap_or(session.cwd.as_str());
                if session_project != project.as_str() {
                    return false;
                }
            }
            true
        })
        .take(limit)
        .collect();
    Ok(json!({
        "caller_session_id": caller_session,
        "sessions": filtered
            .iter()
            .map(|session| session_summary(session))
            .collect::<Vec<_>>(),
    }))
}

fn session_read(
    engine: &Engine,
    caller_session: &str,
    capabilities: &[HostMcpCapability],
    arguments: &Value,
) -> Result<Value, String> {
    let target = arguments
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "session_id is required".to_string())?;
    let cross = capabilities.contains(&HostMcpCapability::SessionReadCross);
    if target != caller_session && !cross {
        return Err("cross-session read denied".into());
    }
    let sessions = engine.list_sessions().map_err(|error| error.to_string())?;
    let session = sessions
        .iter()
        .find(|candidate| candidate.id == target)
        .ok_or_else(|| "session not found".to_string())?;
    Ok(json!({
        "session": session_summary(session),
        "protocol": engine.provider_protocol_compatibility(target),
    }))
}

fn session_summary(session: &Session) -> Value {
    json!({
        "id": session.id,
        "provider": session.provider.as_str(),
        "cwd": session.cwd,
        "title": session.title,
        "pinned": session.pinned,
        "project_path": session.project_path,
        "permission_mode": session.permission_mode,
        "sandbox_policy": session.sandbox_policy,
    })
}

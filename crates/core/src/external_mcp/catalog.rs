//! External MCP tool registry (single definition per tool: class, scope, annotations).

use crate::external_mcp::clients::ExternalScope;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolClass {
    R,
    W,
    X,
    D,
}

#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: &'static str,
    pub domain: &'static str,
    pub verb: &'static str,
    pub class: ToolClass,
    pub scope: ExternalScope,
    pub description: &'static str,
    pub input_schema: Value,
    pub wave: u8,
    pub deferred: bool,
    pub idempotent: bool,
}

const VERBS: &[&str] = &[
    "list", "read", "search", "create", "update", "delete", "send", "stop", "wait", "respond",
    "run", "set",
];

fn empty_object_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

fn destructive_path_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "minLength": 1, "maxLength": 4096 },
            "confirm": { "type": "boolean", "const": true },
            "expected_id": { "type": "string", "minLength": 1, "maxLength": 4096 }
        },
        "required": ["path", "confirm", "expected_id"],
        "additionalProperties": false
    })
}

fn destructive_session_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": { "type": "string", "minLength": 1, "maxLength": 128 },
            "confirm": { "type": "boolean", "const": true },
            "expected_id": { "type": "string", "minLength": 1, "maxLength": 128 }
        },
        "required": ["session_id", "confirm", "expected_id"],
        "additionalProperties": false
    })
}

fn destructive_id_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": { "type": "string", "minLength": 1, "maxLength": 128 },
            "confirm": { "type": "boolean", "const": true },
            "expected_id": { "type": "string", "minLength": 1, "maxLength": 128 }
        },
        "required": ["id", "confirm", "expected_id"],
        "additionalProperties": false
    })
}

fn destructive_worktree_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "project_path": { "type": "string", "minLength": 1, "maxLength": 4096 },
            "id": { "type": "string", "minLength": 1, "maxLength": 4096 },
            "confirm": { "type": "boolean", "const": true },
            "expected_id": { "type": "string", "minLength": 1, "maxLength": 4096 }
        },
        "required": ["project_path", "id", "confirm", "expected_id"],
        "additionalProperties": false
    })
}

fn bounded_id_schema(required: bool) -> Value {
    let mut schema = json!({
        "type": "object",
        "properties": {
            "id": {
                "type": "string",
                "minLength": 1,
                "maxLength": 128
            }
        },
        "additionalProperties": false
    });
    if required {
        schema["required"] = json!(["id"]);
    }
    schema
}

fn list_limit_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 50
            },
            "cursor": {
                "type": "string",
                "maxLength": 256
            }
        },
        "additionalProperties": false
    })
}

fn session_id_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": {
                "type": "string",
                "minLength": 1,
                "maxLength": 128
            }
        },
        "required": ["session_id"],
        "additionalProperties": false
    })
}

fn project_path_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "minLength": 1,
                "maxLength": 4096
            }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

fn turn_send_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": { "type": "string", "minLength": 1, "maxLength": 128 },
            "mode": { "type": "string", "enum": ["prompt", "queue", "steer"] },
            "text": { "type": "string", "maxLength": 65536 }
        },
        "required": ["session_id", "mode", "text"],
        "additionalProperties": false
    })
}

fn wait_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": { "type": "string", "minLength": 1, "maxLength": 128 },
            "timeout_ms": { "type": "integer", "minimum": 1, "maximum": 55000 }
        },
        "required": ["session_id"],
        "additionalProperties": false
    })
}

fn events_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "cursor": { "type": "string", "maxLength": 256 },
            "limit": { "type": "integer", "minimum": 1, "maximum": 50 },
            "types": {
                "type": "array",
                "maxItems": 32,
                "items": { "type": "string", "maxLength": 64 }
            },
            "session_id": { "type": "string", "maxLength": 128 }
        },
        "additionalProperties": false
    })
}

fn events_wait_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "cursor": { "type": "string", "maxLength": 256 },
            "limit": { "type": "integer", "minimum": 1, "maximum": 50 },
            "timeout_ms": { "type": "integer", "minimum": 1, "maximum": 55000 },
            "types": {
                "type": "array",
                "maxItems": 32,
                "items": { "type": "string", "maxLength": 64 }
            },
            "session_id": { "type": "string", "maxLength": 128 }
        },
        "additionalProperties": false
    })
}

fn workspace_read_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "minLength": 1, "maxLength": 4096 },
            "max_bytes": { "type": "integer", "minimum": 1, "maximum": 262144 }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

fn workspace_search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "project_path": { "type": "string", "minLength": 1, "maxLength": 4096 },
            "query": { "type": "string", "minLength": 1, "maxLength": 512 },
            "limit": { "type": "integer", "minimum": 1, "maximum": 50 }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

fn tool(
    domain: &'static str,
    verb: &'static str,
    class: ToolClass,
    scope: ExternalScope,
    description: &'static str,
    input_schema: Value,
    wave: u8,
    deferred: bool,
    idempotent: bool,
) -> ToolSpec {
    let name = if domain == "capabilities" {
        "codetwo_capabilities".to_string()
    } else {
        format!("codetwo_{domain}_{verb}")
    };
    // Leak the name for 'static lifetime — catalog is process-lifetime static.
    let name: &'static str = Box::leak(name.into_boxed_str());
    ToolSpec {
        name,
        domain,
        verb,
        class,
        scope,
        description,
        input_schema,
        wave,
        deferred,
        idempotent,
    }
}

fn build_catalog() -> Vec<ToolSpec> {
    vec![
        tool(
            "capabilities",
            "capabilities",
            ToolClass::R,
            ExternalScope::Read,
            "External MCP capabilities, scopes, waves, and experimental flags.",
            empty_object_schema(),
            1,
            false,
            true,
        ),
        tool(
            "project",
            "list",
            ToolClass::R,
            ExternalScope::Read,
            "List projects visible to the caller.",
            list_limit_schema(),
            1,
            false,
            true,
        ),
        tool(
            "project",
            "read",
            ToolClass::R,
            ExternalScope::Read,
            "Read one project by path or id.",
            project_path_schema(),
            1,
            false,
            true,
        ),
        tool(
            "project",
            "create",
            ToolClass::W,
            ExternalScope::Operate,
            "Create a project at a path.",
            project_path_schema(),
            2,
            false,
            false,
        ),
        tool(
            "project",
            "update",
            ToolClass::W,
            ExternalScope::Operate,
            "Update project metadata (rename, worktree mode, agent defaults).",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "minLength": 1, "maxLength": 4096 },
                    "name": { "type": "string", "maxLength": 256 },
                    "worktree_mode": { "type": "string", "enum": ["local", "current", "origin_default"] },
                    "provider": { "type": "string", "maxLength": 64 },
                    "model": { "type": "string", "maxLength": 256 },
                    "reasoning_effort": { "type": "string", "maxLength": 32 }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
            2,
            false,
            false,
        ),
        tool(
            "project",
            "delete",
            ToolClass::D,
            ExternalScope::Admin,
            "[DESTRUCTIVE] Unregister a project (never deletes user files).",
            destructive_path_schema(),
            2,
            false,
            false,
        ),
        tool(
            "session",
            "list",
            ToolClass::R,
            ExternalScope::Read,
            "List sessions filtered to allowed projects.",
            list_limit_schema(),
            1,
            false,
            true,
        ),
        tool(
            "session",
            "read",
            ToolClass::R,
            ExternalScope::Read,
            "Read session metadata.",
            session_id_schema(),
            1,
            false,
            true,
        ),
        tool(
            "session",
            "search",
            ToolClass::R,
            ExternalScope::Read,
            "Search sessions by title or id fragment.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "minLength": 1, "maxLength": 256 },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50 }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
            1,
            false,
            true,
        ),
        tool(
            "session",
            "create",
            ToolClass::W,
            ExternalScope::Operate,
            "Create a session in a project.",
            json!({
                "type": "object",
                "properties": {
                    "project_path": { "type": "string", "minLength": 1, "maxLength": 4096 },
                    "title": { "type": "string", "maxLength": 256 },
                    "provider": { "type": "string", "maxLength": 64 },
                    "model": { "type": "string", "maxLength": 256 },
                    "permission_mode": { "type": "string", "enum": ["ask", "accept_edits", "yolo"] },
                    "sandbox_policy": { "type": "string", "enum": ["read_only", "workspace_write", "danger_full_access"] },
                    "use_worktree": { "type": "boolean" }
                },
                "required": ["project_path"],
                "additionalProperties": false
            }),
            2,
            false,
            false,
        ),
        tool(
            "session",
            "update",
            ToolClass::W,
            ExternalScope::Operate,
            "Update session rename, pin, or archive flags.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "minLength": 1, "maxLength": 128 },
                    "title": { "type": "string", "maxLength": 256 },
                    "pinned": { "type": "boolean" },
                    "archived": { "type": "boolean" }
                },
                "required": ["session_id"],
                "additionalProperties": false
            }),
            2,
            false,
            false,
        ),
        tool(
            "session",
            "delete",
            ToolClass::D,
            ExternalScope::Admin,
            "[DESTRUCTIVE] Delete a session record.",
            destructive_session_schema(),
            2,
            false,
            false,
        ),
        tool(
            "transcript",
            "read",
            ToolClass::R,
            ExternalScope::Read,
            "Read a bounded transcript page.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "minLength": 1, "maxLength": 128 },
                    "cursor": { "type": "string", "maxLength": 256 },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50 }
                },
                "required": ["session_id"],
                "additionalProperties": false
            }),
            1,
            false,
            true,
        ),
        tool(
            "turn",
            "send",
            ToolClass::X,
            ExternalScope::Operate,
            "Send, queue, or steer a turn.",
            turn_send_schema(),
            2,
            false,
            false,
        ),
        tool(
            "turn",
            "stop",
            ToolClass::X,
            ExternalScope::Operate,
            "Stop the active turn for a session.",
            session_id_schema(),
            2,
            false,
            false,
        ),
        tool(
            "turn",
            "wait",
            ToolClass::R,
            ExternalScope::Read,
            "Wait for turn completion with bounded timeout.",
            wait_schema(),
            2,
            false,
            true,
        ),
        tool(
            "approval",
            "list",
            ToolClass::R,
            ExternalScope::Read,
            "List pending approval requests.",
            list_limit_schema(),
            1,
            false,
            true,
        ),
        tool(
            "approval",
            "respond",
            ToolClass::X,
            ExternalScope::Approve,
            "Respond to a pending approval request.",
            json!({
                "type": "object",
                "properties": {
                    "request_id": { "type": "string", "minLength": 1, "maxLength": 128 },
                    "allow": { "type": "boolean" }
                },
                "required": ["request_id", "allow"],
                "additionalProperties": false
            }),
            2,
            false,
            false,
        ),
        tool(
            "question",
            "list",
            ToolClass::R,
            ExternalScope::Read,
            "List pending user questions.",
            list_limit_schema(),
            1,
            false,
            true,
        ),
        tool(
            "question",
            "respond",
            ToolClass::X,
            ExternalScope::Operate,
            "Answer a pending question.",
            json!({
                "type": "object",
                "properties": {
                    "request_id": { "type": "string", "minLength": 1, "maxLength": 128 },
                    "answer": { "type": "string", "maxLength": 8192 }
                },
                "required": ["request_id", "answer"],
                "additionalProperties": false
            }),
            2,
            false,
            false,
        ),
        tool(
            "model",
            "list",
            ToolClass::R,
            ExternalScope::Read,
            "List selectable models for a session.",
            session_id_schema(),
            1,
            false,
            true,
        ),
        tool(
            "model",
            "set",
            ToolClass::W,
            ExternalScope::Operate,
            "Set the model for a session.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "minLength": 1, "maxLength": 128 },
                    "model_id": { "type": "string", "minLength": 1, "maxLength": 256 }
                },
                "required": ["session_id", "model_id"],
                "additionalProperties": false
            }),
            2,
            false,
            false,
        ),
        tool(
            "policy",
            "read",
            ToolClass::R,
            ExternalScope::Read,
            "Read execution policy for a session.",
            session_id_schema(),
            1,
            false,
            true,
        ),
        tool(
            "policy",
            "set",
            ToolClass::W,
            ExternalScope::Operate,
            "Tighten execution policy (never loosen).",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "minLength": 1, "maxLength": 128 },
                    "mode": { "type": "string", "enum": ["ask", "accept_edits", "yolo"] },
                    "sandbox_policy": { "type": "string", "enum": ["read_only", "workspace_write", "danger_full_access"] }
                },
                "required": ["session_id"],
                "additionalProperties": false
            }),
            2,
            false,
            false,
        ),
        tool(
            "worktree",
            "list",
            ToolClass::R,
            ExternalScope::Read,
            "List worktrees for a project.",
            project_path_schema(),
            1,
            false,
            true,
        ),
        tool(
            "worktree",
            "delete",
            ToolClass::D,
            ExternalScope::Admin,
            "[DESTRUCTIVE] Discard an orphan session worktree checkout.",
            destructive_worktree_schema(),
            2,
            false,
            false,
        ),
        tool(
            "git",
            "status",
            ToolClass::R,
            ExternalScope::Read,
            "Git status for a project worktree.",
            project_path_schema(),
            1,
            false,
            true,
        ),
        tool(
            "git",
            "diff",
            ToolClass::R,
            ExternalScope::Read,
            "Bounded git diff for a project worktree.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "minLength": 1, "maxLength": 4096 },
                    "max_bytes": { "type": "integer", "minimum": 1, "maximum": 262144 }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
            1,
            false,
            true,
        ),
        tool(
            "workspace",
            "file_read",
            ToolClass::R,
            ExternalScope::Read,
            "Read a bounded workspace file within the project.",
            workspace_read_schema(),
            1,
            false,
            true,
        ),
        tool(
            "workspace",
            "search",
            ToolClass::R,
            ExternalScope::Read,
            "Search workspace files within the project.",
            workspace_search_schema(),
            1,
            false,
            true,
        ),
        tool(
            "automation",
            "list",
            ToolClass::R,
            ExternalScope::Read,
            "List automations.",
            list_limit_schema(),
            1,
            false,
            true,
        ),
        tool(
            "automation",
            "run",
            ToolClass::X,
            ExternalScope::Operate,
            "Run an automation once.",
            bounded_id_schema(true),
            2,
            false,
            false,
        ),
        tool(
            "automation",
            "create",
            ToolClass::D,
            ExternalScope::Admin,
            "[DESTRUCTIVE] Create a scheduled automation.",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "minLength": 1, "maxLength": 256 },
                    "prompt": { "type": "string", "minLength": 1, "maxLength": 4096 },
                    "project_path": { "type": "string", "minLength": 1, "maxLength": 4096 },
                    "provider": { "type": "string", "minLength": 1, "maxLength": 64 },
                    "cron": { "type": "string", "minLength": 1, "maxLength": 128 },
                    "timezone": { "type": "string", "minLength": 1, "maxLength": 64 },
                    "confirm": { "type": "boolean", "const": true },
                    "expected_id": { "type": "string", "minLength": 1, "maxLength": 128 }
                },
                "required": ["name", "prompt", "project_path", "provider", "cron", "timezone", "confirm", "expected_id"],
                "additionalProperties": false
            }),
            2,
            false,
            false,
        ),
        tool(
            "automation",
            "update",
            ToolClass::D,
            ExternalScope::Admin,
            "[DESTRUCTIVE] Update a scheduled automation.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "minLength": 1, "maxLength": 128 },
                    "name": { "type": "string", "minLength": 1, "maxLength": 256 },
                    "prompt": { "type": "string", "minLength": 1, "maxLength": 4096 },
                    "project_path": { "type": "string", "minLength": 1, "maxLength": 4096 },
                    "provider": { "type": "string", "minLength": 1, "maxLength": 64 },
                    "cron": { "type": "string", "minLength": 1, "maxLength": 128 },
                    "timezone": { "type": "string", "minLength": 1, "maxLength": 64 },
                    "enabled": { "type": "boolean" },
                    "confirm": { "type": "boolean", "const": true },
                    "expected_id": { "type": "string", "minLength": 1, "maxLength": 128 }
                },
                "required": ["id", "confirm", "expected_id"],
                "additionalProperties": false
            }),
            2,
            false,
            false,
        ),
        tool(
            "automation",
            "delete",
            ToolClass::D,
            ExternalScope::Admin,
            "[DESTRUCTIVE] Delete a scheduled automation.",
            destructive_id_schema(),
            2,
            false,
            false,
        ),
        tool(
            "scene",
            "list",
            ToolClass::R,
            ExternalScope::Read,
            "List scenes.",
            list_limit_schema(),
            1,
            false,
            true,
        ),
        tool(
            "scene",
            "run",
            ToolClass::D,
            ExternalScope::Admin,
            "[DESTRUCTIVE] Run a scene against a session.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "minLength": 1, "maxLength": 128 },
                    "session_id": { "type": "string", "minLength": 1, "maxLength": 128 },
                    "project_path": { "type": "string", "minLength": 1, "maxLength": 4096 },
                    "confirm": { "type": "boolean", "const": true },
                    "expected_id": { "type": "string", "minLength": 1, "maxLength": 128 }
                },
                "required": ["id", "session_id", "confirm", "expected_id"],
                "additionalProperties": false
            }),
            2,
            false,
            false,
        ),
        tool(
            "pipeline",
            "list",
            ToolClass::R,
            ExternalScope::Read,
            "List pipelines.",
            list_limit_schema(),
            1,
            false,
            true,
        ),
        tool(
            "pipeline",
            "run",
            ToolClass::D,
            ExternalScope::Admin,
            "[DESTRUCTIVE] Start a pipeline instance.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "minLength": 1, "maxLength": 128 },
                    "session_id": { "type": "string", "minLength": 1, "maxLength": 128 },
                    "project_path": { "type": "string", "minLength": 1, "maxLength": 4096 },
                    "confirm": { "type": "boolean", "const": true },
                    "expected_id": { "type": "string", "minLength": 1, "maxLength": 128 }
                },
                "required": ["id", "project_path", "confirm", "expected_id"],
                "additionalProperties": false
            }),
            2,
            false,
            false,
        ),
        tool(
            "events",
            "poll",
            ToolClass::R,
            ExternalScope::Read,
            "Poll events since a cursor.",
            events_schema(),
            1,
            false,
            true,
        ),
        tool(
            "events",
            "wait",
            ToolClass::R,
            ExternalScope::Read,
            "Wait for events with bounded timeout.",
            events_wait_schema(),
            2,
            false,
            true,
        ),
        tool(
            "subagent",
            "list",
            ToolClass::R,
            ExternalScope::Read,
            "List subagent runs for a session.",
            session_id_schema(),
            1,
            false,
            true,
        ),
    ]
}

static CATALOG: std::sync::OnceLock<Vec<ToolSpec>> = std::sync::OnceLock::new();

pub fn external_tool_catalog() -> &'static [ToolSpec] {
    CATALOG.get_or_init(build_catalog)
}

pub fn default_catalog() -> Vec<&'static ToolSpec> {
    external_tool_catalog()
        .iter()
        .filter(|spec| !spec.deferred && spec.class != ToolClass::D)
        .collect()
}

pub fn annotations(spec: &ToolSpec) -> Value {
    let (read_only, destructive, open_world) = match spec.class {
        ToolClass::R => (true, false, false),
        ToolClass::W => (false, false, false),
        ToolClass::X => (false, false, true),
        ToolClass::D => (false, true, false),
    };
    let mut ann = json!({
        "readOnlyHint": read_only,
        "destructiveHint": destructive,
        "openWorldHint": open_world,
    });
    if spec.idempotent && matches!(spec.class, ToolClass::W | ToolClass::X) {
        ann["idempotentHint"] = json!(true);
    }
    ann
}

pub fn mcp_tool_json(spec: &ToolSpec) -> Value {
    let mut value = json!({
        "name": spec.name,
        "description": spec.description,
        "inputSchema": spec.input_schema,
        "annotations": annotations(spec),
    });
    if spec.class == ToolClass::D {
        value["_meta"] = json!({ "codetwo/risk": "destructive" });
    }
    value
}

pub fn validate_catalog() -> Result<(), String> {
    let catalog = external_tool_catalog();
    let mut names = std::collections::HashSet::new();
    for spec in catalog {
        if spec.name == "codetwo_capabilities" {
            if spec.domain != "capabilities" {
                return Err("capabilities tool domain mismatch".into());
            }
            if spec.scope != ExternalScope::Read || spec.class != ToolClass::R {
                return Err("capabilities must be R/read".into());
            }
        } else if !verb_allowed(spec.domain, spec.verb) {
            return Err(format!("invalid verb for {}: {}", spec.name, spec.verb));
        } else {
            let expected = format!("codetwo_{}_{}", spec.domain, spec.verb);
            if spec.name != expected {
                return Err(format!("name/domain/verb mismatch: {}", spec.name));
            }
        }
        if !names.insert(spec.name) {
            return Err(format!("duplicate tool name: {}", spec.name));
        }
        if spec.class == ToolClass::R && spec.scope != ExternalScope::Read {
            return Err(format!("R tool {} must require Read scope", spec.name));
        }
        if matches!(spec.class, ToolClass::W | ToolClass::X) && spec.scope == ExternalScope::Read {
            return Err(format!("W/X tool {} cannot require only Read", spec.name));
        }
        if spec.name == "codetwo_approval_respond" && spec.scope != ExternalScope::Approve {
            return Err("approval_respond must require Approve scope".into());
        }
        let ann = annotations(spec);
        if ann["readOnlyHint"] != json!(spec.class == ToolClass::R) {
            return Err(format!("readOnlyHint mismatch for {}", spec.name));
        }
        if ann["destructiveHint"] != json!(spec.class == ToolClass::D) {
            return Err(format!("destructiveHint mismatch for {}", spec.name));
        }
        if spec.class == ToolClass::X && ann["openWorldHint"] != json!(true) {
            return Err(format!("openWorldHint required for X tool {}", spec.name));
        }
        if spec.class == ToolClass::D || spec.deferred {
            if default_catalog().iter().any(|d| d.name == spec.name) {
                return Err(format!("deferred/D tool {} in default catalog", spec.name));
            }
        }
        validate_input_schema(&spec.input_schema, spec.name)?;
    }
    Ok(())
}

fn verb_allowed(domain: &str, verb: &str) -> bool {
    if VERBS.contains(&verb) {
        return true;
    }
    matches!(
        (domain, verb),
        ("git", "status")
            | ("git", "diff")
            | ("workspace", "file_read")
            | ("events", "poll")
    )
}

fn validate_input_schema(schema: &Value, tool: &str) -> Result<(), String> {
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err(format!("{tool}: input schema must be an object"));
    }
    if schema.get("additionalProperties") != Some(&json!(false)) {
        return Err(format!("{tool}: additionalProperties must be false"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_validation_passes() {
        validate_catalog().expect("catalog must be consistent");
    }

    #[test]
    fn default_catalog_excludes_deferred_and_class_d() {
        let names: Vec<_> = default_catalog().into_iter().map(|s| s.name).collect();
        assert!(!names.contains(&"codetwo_project_delete"));
        assert!(!names.contains(&"codetwo_automation_create"));
        assert!(names.contains(&"codetwo_session_list"));
    }

    #[test]
    fn approval_respond_requires_approve_scope() {
        let spec = external_tool_catalog()
            .iter()
            .find(|s| s.name == "codetwo_approval_respond")
            .unwrap();
        assert_eq!(spec.scope, ExternalScope::Approve);
        assert_eq!(spec.class, ToolClass::X);
    }
}

pub fn find_tool(name: &str) -> Option<&'static ToolSpec> {
    external_tool_catalog().iter().find(|spec| spec.name == name)
}

fn client_has_scope(scopes: &[ExternalScope], required: ExternalScope) -> bool {
    scopes.contains(&required)
}

/// MCP tool list for one resolved credential (scope, admin, and deferred rules applied).
pub fn catalog_for_client(scopes: &[ExternalScope]) -> Vec<Value> {
    let has_admin = scopes.contains(&ExternalScope::Admin);
    external_tool_catalog()
        .iter()
        .filter(|spec| {
            if spec.deferred {
                return false;
            }
            if spec.class == ToolClass::D && !has_admin {
                return false;
            }
            if spec.scope == ExternalScope::Admin && !has_admin {
                return false;
            }
            client_has_scope(scopes, spec.scope)
        })
        .map(mcp_tool_json)
        .collect()
}

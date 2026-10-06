//! Authorize gate and tool dispatch for external MCP.

use crate::external_mcp::audit::{build_audit_record, reason_code, AuditRecord};
use crate::external_mcp::catalog::{find_tool, ToolClass};
use crate::external_mcp::clients::{looks_like_external_token, ResolveError, ResolvedClient};
use crate::external_mcp::ctx::{CallContext, ToolError, ToolErrorKind, ToolResult};
use crate::external_mcp::ops_admin;
use crate::external_mcp::ops_events;
use crate::external_mcp::ops_read;
use crate::external_mcp::ops_write;
use crate::Engine;
use serde_json::{json, Value};
use std::time::Instant;

pub const SERVER_NAME: &str = "codetwo-external-mcp";
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;

pub fn tool_error_to_rpc_code(kind: ToolErrorKind) -> i64 {
    match kind {
        ToolErrorKind::Denied => -32003,
        ToolErrorKind::InvalidParams => -32602,
        ToolErrorKind::NotFound => -32004,
        ToolErrorKind::RateLimited => -32005,
        ToolErrorKind::Conflict => -32009,
        ToolErrorKind::Internal => -32603,
    }
}

/// Internal errors carry raw store, filesystem, or engine text; callers only get a fixed message.
pub fn public_error(error: ToolError) -> ToolError {
    if error.kind == ToolErrorKind::Internal {
        return ToolError::internal("operation failed");
    }
    error
}

pub fn tool_error_message(error: &ToolError) -> String {
    match error.kind {
        ToolErrorKind::Denied => format!("denied: {}", error.message),
        ToolErrorKind::RateLimited => format!("rate limited: {}", error.message),
        ToolErrorKind::NotFound => format!("not found: {}", error.message),
        ToolErrorKind::InvalidParams => error.message.clone(),
        ToolErrorKind::Conflict => format!("conflict: {}", error.message),
        ToolErrorKind::Internal => format!("internal: {}", error.message),
    }
}

fn destructive_target_id(tool: &str, args: &Value) -> Option<String> {
    if tool == "codetwo_automation_create" {
        return Some("new".into());
    }
    let key = match tool {
        "codetwo_project_delete" => "path",
        "codetwo_session_delete"
        | "codetwo_turn_send"
        | "codetwo_turn_stop"
        | "codetwo_turn_wait"
        | "codetwo_transcript_read"
        | "codetwo_subagent_list" => "session_id",
        "codetwo_worktree_delete" => "id",
        "codetwo_automation_delete" | "codetwo_automation_run" | "codetwo_automation_update" => {
            "id"
        }
        "codetwo_scene_run" | "codetwo_pipeline_run" => "id",
        _ => return None,
    };
    args.get(key).and_then(Value::as_str).map(str::to_string)
}

fn validate_destructive_confirm(tool: &str, args: &Value) -> Result<(), ToolError> {
    let confirm = args.get("confirm").and_then(Value::as_bool) == Some(true);
    if !confirm {
        return Err(ToolError::new(
            ToolErrorKind::Denied,
            "destructive call requires confirm:true",
        ));
    }
    let expected = args
        .get("expected_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("expected_id required for destructive tools"))?;
    let target = destructive_target_id(tool, args).ok_or_else(|| {
        ToolError::invalid("could not resolve target id for destructive confirmation")
    })?;
    if expected != target {
        return Err(ToolError::new(
            ToolErrorKind::Denied,
            "expected_id does not match target",
        ));
    }
    Ok(())
}

fn write_audit(engine: &Engine, record: AuditRecord, fail_closed: bool) -> Result<(), ToolError> {
    if engine.external_mcp_state().audit().record(&record).is_err() && fail_closed {
        return Err(ToolError::new(ToolErrorKind::Internal, "audit sink failed"));
    }
    Ok(())
}

/// Keep the operation's own error (kind and message) and add a fixed, generic note when its audit
/// record could not be written. The note never carries sink, path or argument text.
fn with_audit_outcome(mut error: ToolError, audited: Result<(), ToolError>) -> ToolError {
    if audited.is_err() {
        tracing::error!("external MCP audit record could not be written after a tool error");
        error.message.push_str("; audit record could not be written");
    }
    error
}

fn is_write_class(class: ToolClass) -> bool {
    matches!(class, ToolClass::W | ToolClass::X | ToolClass::D)
}

// The catalog uses this bounded subset of JSON Schema; validate it at the shared boundary.
fn validate_arguments(schema: &Value, value: &Value) -> Result<(), ToolError> {
    let invalid = || ToolError::invalid("arguments do not match the tool schema");
    if let Some(kind) = schema.get("type").and_then(Value::as_str) {
        let valid = match kind {
            "object" => value.is_object(),
            "string" => value.is_string(),
            "integer" => value.is_i64() || value.is_u64(),
            "boolean" => value.is_boolean(),
            "array" => value.is_array(),
            "number" => value.is_number(),
            _ => false,
        };
        if !valid {
            return Err(invalid());
        }
    }
    if schema
        .get("const")
        .is_some_and(|expected| expected != value)
        || schema
            .get("enum")
            .and_then(Value::as_array)
            .is_some_and(|choices| !choices.contains(value))
    {
        return Err(invalid());
    }
    if let Some(text) = value.as_str() {
        let count = text.chars().count() as u64;
        if schema
            .get("minLength")
            .and_then(Value::as_u64)
            .is_some_and(|min| count < min)
            || schema
                .get("maxLength")
                .and_then(Value::as_u64)
                .is_some_and(|max| count > max)
        {
            return Err(invalid());
        }
    }
    if let Some(number) = value.as_f64() {
        if schema
            .get("minimum")
            .and_then(Value::as_f64)
            .is_some_and(|min| number < min)
            || schema
                .get("maximum")
                .and_then(Value::as_f64)
                .is_some_and(|max| number > max)
        {
            return Err(invalid());
        }
    }
    if let Some(object) = value.as_object() {
        if schema
            .get("required")
            .and_then(Value::as_array)
            .is_some_and(|required| {
                required
                    .iter()
                    .any(|key| !key.as_str().is_some_and(|key| object.contains_key(key)))
            })
        {
            return Err(invalid());
        }
        for (key, item) in object {
            if let Some(property) = schema
                .get("properties")
                .and_then(|properties| properties.get(key))
            {
                validate_arguments(property, item)?;
            } else if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                return Err(invalid());
            }
        }
    }
    if let (Some(items), Some(item_schema)) = (value.as_array(), schema.get("items")) {
        if items.len() > 100
            || schema
                .get("maxItems")
                .and_then(Value::as_u64)
                .is_some_and(|max| items.len() as u64 > max)
            || schema
                .get("minItems")
                .and_then(Value::as_u64)
                .is_some_and(|min| (items.len() as u64) < min)
        {
            return Err(invalid());
        }
        for item in items {
            validate_arguments(item_schema, item)?;
        }
    }
    Ok(())
}

impl Engine {
    pub fn authorize_external_mcp_call(
        &self,
        resolved: &ResolvedClient,
        tool: &str,
        args: &Value,
    ) -> ToolResult {
        let started = Instant::now();
        let client_id = resolved.record.id.clone();
        // Every call, known tool or not, spends budget before any synchronous audit write.
        let spec = find_tool(tool);
        let is_write = spec.is_some_and(|spec| is_write_class(spec.class));
        if !self
            .external_mcp_state()
            .limiter()
            .try_consume(&client_id, is_write)
        {
            // Over-budget calls do not generate unbounded synchronous audit writes.
            return Err(ToolError::new(
                ToolErrorKind::RateLimited,
                "client rate limit exceeded",
            ));
        }
        // Audit only stored target identities; never copy caller-supplied text into metadata.
        let stored_session = args
            .get("session_id")
            .and_then(Value::as_str)
            .and_then(|id| self.store()?.get_session(id).ok().flatten());
        let target_session = stored_session.as_ref().map(|session| session.id.clone());
        let target_project = stored_session
            .as_ref()
            .and_then(session_project_path)
            .or_else(|| {
                let requested = args
                    .get("project_path")
                    .or_else(|| args.get("path"))?
                    .as_str()?;
                let canonical = std::path::Path::new(requested).canonicalize().ok()?;
                self.store()?
                    .list_projects()
                    .ok()?
                    .into_iter()
                    .find(|project| {
                        std::path::Path::new(&project.path)
                            .canonicalize()
                            .ok()
                            .as_ref()
                            == Some(&canonical)
                    })
                    .map(|project| project.path)
            });
        let audit = |allowed: bool,
                     reason: &str,
                     class: ToolClass,
                     session: Option<String>,
                     project: Option<String>| {
            build_audit_record(
                &client_id,
                tool,
                class,
                allowed,
                reason,
                session.or_else(|| target_session.clone()),
                project.or_else(|| target_project.clone()),
                Some(args),
                started.elapsed().as_millis() as u64,
            )
        };

        if !self.external_mcp_state().is_enabled() {
            let record = audit(false, "surface_disabled", ToolClass::R, None, None);
            let _ = write_audit(self, record, false);
            return Err(ToolError::denied("external MCP surface is disabled"));
        }
        let Some(spec) = spec else {
            let record = audit(false, reason_code::TOOL_UNKNOWN, ToolClass::R, None, None);
            let _ = write_audit(self, record, false);
            return Err(ToolError::not_found("unknown tool"));
        };
        if !resolved.record.scopes.contains(&spec.scope) {
            let record = audit(false, reason_code::SCOPE_DENIED, spec.class, None, None);
            let _ = write_audit(self, record, is_write_class(spec.class));
            return Err(ToolError::denied("scope not granted"));
        }
        if spec.class == ToolClass::D {
            if let Err(error) = validate_destructive_confirm(tool, args) {
                write_audit(
                    self,
                    audit(false, "confirmation_denied", spec.class, None, None),
                    is_write,
                )?;
                return Err(error);
            }
        }
        if let Err(error) = validate_arguments(&spec.input_schema, args) {
            write_audit(
                self,
                audit(false, "invalid_arguments", spec.class, None, None),
                is_write,
            )?;
            return Err(error);
        }
        let record = audit(true, reason_code::ALLOWED, spec.class, None, None);
        write_audit(self, record, is_write)?;

        let ctx = CallContext {
            client: resolved,
            spec,
        };
        let result = ops_read::call(self, &ctx, tool, args)
            .or_else(|| ops_write::call(self, &ctx, tool, args))
            .or_else(|| ops_admin::call(self, &ctx, tool, args))
            .or_else(|| ops_events::call(self, &ctx, tool, args))
            .unwrap_or_else(|| Err(ToolError::not_found("unknown tool")))
            .map_err(public_error);
        // The operation has already run: its own error is the outcome the caller must see (a
        // "do not retry" conflict, say). A failed audit write is reported next to it, never in
        // place of it.
        let result = match result {
            Err(error) => {
                let reason = match error.kind {
                    ToolErrorKind::Denied => reason_code::POLICY_DENIED,
                    ToolErrorKind::NotFound => "target_unavailable",
                    ToolErrorKind::InvalidParams => "invalid_arguments",
                    ToolErrorKind::Conflict => "conflict",
                    ToolErrorKind::RateLimited => reason_code::RATE_LIMITED,
                    ToolErrorKind::Internal => "operation_failed",
                };
                let audited =
                    write_audit(self, audit(false, reason, spec.class, None, None), is_write);
                Err(with_audit_outcome(error, audited))
            }
            ok => ok,
        };
        cap_response(result)
    }

    pub fn resolve_external_client(&self, bearer: &str) -> Result<ResolvedClient, ResolveError> {
        if !looks_like_external_token(bearer) {
            return Err(ResolveError::NotExternalToken);
        }
        if !self.external_mcp_state().is_enabled() {
            return Err(ResolveError::Unknown);
        }
        self.external_mcp_state()
            .with_registry(|registry| registry.resolve(bearer))
            .ok_or(ResolveError::Unknown)?
    }
}

pub fn cap_response(result: ToolResult) -> ToolResult {
    result.map(|value| truncate_value(value, MAX_RESPONSE_BYTES))
}

fn truncate_value(value: Value, max_bytes: usize) -> Value {
    let encoded = serde_json::to_string(&value).unwrap_or_else(|_| "{}".into());
    if encoded.len() <= max_bytes {
        return value;
    }
    json!({
        "truncated": true,
        "preview": encoded.chars().scan(0usize, |size, ch| {
            *size += ch.len_utf8(); Some((*size, ch))
        }).take_while(|(size, _)| *size <= max_bytes.saturating_sub(64) / 6).map(|(_, ch)| ch).collect::<String>(),
    })
}

pub fn resolve_session_project(engine: &Engine, session_id: &str) -> Option<String> {
    if let Ok(sessions) = engine.list_sessions() {
        if let Some(session) = sessions.iter().find(|s| s.id == session_id) {
            return session_project_path(session);
        }
    }
    engine
        .store()?
        .get_session(session_id)
        .ok()
        .flatten()
        .and_then(|session| session_project_path(&session))
}

pub fn session_project_path(session: &crate::session::Session) -> Option<String> {
    session
        .project_path
        .clone()
        .filter(|p| !p.is_empty())
        .or_else(|| {
            if session.cwd.is_empty() {
                None
            } else {
                Some(session.cwd.clone())
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_failure_never_replaces_the_operation_error() {
        let failed = || Err(ToolError::new(ToolErrorKind::Internal, "disk /secret/path is full"));
        let original =
            || ToolError::conflict("session creation outcome is unknown; do not retry automatically");

        let kept = with_audit_outcome(original(), Ok(()));
        assert_eq!(kept.kind, ToolErrorKind::Conflict);
        assert_eq!(kept.message, original().message);

        let reported = with_audit_outcome(original(), failed());
        assert_eq!(reported.kind, ToolErrorKind::Conflict, "the operation's kind survives");
        assert!(reported.message.starts_with(&original().message), "{}", reported.message);
        assert!(reported.message.ends_with("audit record could not be written"));
        assert!(!reported.message.contains("/secret/path"), "sink text must not leak");
    }
}

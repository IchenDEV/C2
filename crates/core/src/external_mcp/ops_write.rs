//! External MCP tools: write. Implements `pub fn call` (see `ctx::OpsFn`).

use crate::elicitation::ElicitationAnswer;
use crate::engine::PolicyCommit;
use crate::event::Op;
use crate::external_mcp::ctx::{CallContext, ToolError, ToolResult};
use crate::permission::{ExecutionPolicy, PermissionMode, SandboxPolicy};
use crate::project::ProjectWorktreeMode;
use crate::provider::ProviderId;
use crate::session::{PendingInputKind, Session, SessionRunState};
use crate::skill::DocBlock;
use crate::store::Store;
use crate::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAX_TEXT_FIELD: usize = 4096;
const MAX_PROMPT_TEXT: usize = 65536;

pub fn call(engine: &Engine, ctx: &CallContext<'_>, tool: &str, args: &Value) -> Option<ToolResult> {
    Some(match tool {
        "codetwo_project_create" => project_create(engine, ctx, args),
        "codetwo_project_update" => project_update(engine, ctx, args),
        "codetwo_session_create" => run_async(session_create(engine, ctx, args)),
        "codetwo_session_update" => session_update(engine, ctx, args),
        "codetwo_turn_send" => run_async(turn_send(engine, ctx, args)),
        "codetwo_turn_stop" => run_async(turn_stop(engine, ctx, args)),
        "codetwo_approval_respond" => approval_respond(engine, ctx, args),
        "codetwo_question_respond" => question_respond(engine, ctx, args),
        "codetwo_model_set" => run_async(model_set(engine, ctx, args)),
        "codetwo_policy_set" => policy_set(engine, ctx, args),
        "codetwo_automation_run" => automation_run(engine, ctx, args),
        _ => return None,
    })
}

pub(crate) fn run_async<F: Future<Output = ToolResult>>(future: F) -> ToolResult {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.block_on(future)
    } else {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime for external MCP write tools")
            .block_on(future)
    }
}

pub(crate) fn policy_tightens(current: &ExecutionPolicy, next: &ExecutionPolicy) -> bool {
    let mode = |mode| match mode { PermissionMode::Ask => 0, PermissionMode::AcceptEdits => 1, PermissionMode::Yolo => 2 };
    let sandbox = |sandbox| match sandbox { SandboxPolicy::ReadOnly => 0, SandboxPolicy::WorkspaceWrite => 1, SandboxPolicy::DangerFullAccess => 2 };
    mode(next.mode) <= mode(current.mode) && sandbox(next.sandbox) <= sandbox(current.sandbox)
}

pub(crate) fn require_store(engine: &Engine) -> Result<Arc<Store>, ToolError> {
    engine
        .store()
        .ok_or_else(|| ToolError::internal("persistence is unavailable"))
}

fn sanitize_bounded_text(value: &str, max: usize, field: &str) -> Result<String, ToolError> {
    if value.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return Err(ToolError::invalid(format!("{field} contains control characters")));
    }
    if value.len() > max {
        return Err(ToolError::invalid(format!("{field} exceeds maximum length")));
    }
    Ok(value.to_string())
}

pub(crate) fn require_id(value: &str, field: &str) -> Result<String, ToolError> {
    let id = sanitize_bounded_text(value, 128, field)?;
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
        return Err(ToolError::invalid(format!("invalid {field}")));
    }
    Ok(id)
}

pub(crate) fn canonical_existing_dir(path: &str) -> Result<PathBuf, ToolError> {
    let raw = sanitize_bounded_text(path, MAX_TEXT_FIELD, "path")?;
    let resolved = Path::new(&raw)
        .canonicalize()
        .map_err(|_| ToolError::not_found("project not found"))?;
    if !resolved.is_dir() {
        return Err(ToolError::invalid("path must be an existing directory"));
    }
    Ok(resolved)
}

pub(crate) fn resolve_allowed_path(
    engine: &Engine,
    ctx: &CallContext<'_>,
    path: &str,
    must_be_registered: bool,
) -> Result<String, ToolError> {
    let canonical = canonical_existing_dir(path)?;
    if !ctx.project_allowed(&canonical) {
        return Err(ToolError::not_found("project not found"));
    }
    let text = canonical.to_string_lossy().into_owned();
    if must_be_registered {
        let store = require_store(engine)?;
        if !store.project_exists(&text).map_err(store_err)? {
            return Err(ToolError::not_found("project not found"));
        }
    }
    Ok(text)
}

pub(crate) fn store_err(error: crate::store::StoreError) -> ToolError {
    ToolError::internal(error.to_string())
}

pub(crate) fn session_project_path(session: &Session) -> &str {
    session
        .project_path
        .as_deref()
        .filter(|path| !path.is_empty())
        .unwrap_or(session.cwd.as_str())
}

pub(crate) fn require_session(
    engine: &Engine,
    ctx: &CallContext<'_>,
    session_id: &str,
) -> Result<Session, ToolError> {
    let id = require_id(session_id, "session_id")?;
    let store = require_store(engine)?;
    let session = store
        .get_session(&id)
        .map_err(store_err)?
        .ok_or_else(|| ToolError::not_found("session not found"))?;
    if !ctx.project_allowed(Path::new(session_project_path(&session))) {
        return Err(ToolError::not_found("session not found"));
    }
    Ok(session)
}

fn find_pending_input(engine: &Engine, request_id: &str) -> Result<(Session, crate::session::PendingInput), ToolError> {
    let id = require_id(request_id, "request_id")?;
    let sessions = engine.list_sessions().map_err(store_err)?;
    for session in sessions {
        if let SessionRunState::AwaitingInput { pending, .. } = &session.activity.state {
            if let Some(input) = pending.iter().find(|item| item.input_id == id) {
                return Ok((session.clone(), input.clone()));
            }
        }
    }
    Err(ToolError::not_found("request not found"))
}

fn project_create(_engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("path is required"))?;
    let canonical = canonical_existing_dir(path)?;
    if !ctx.project_allowed(&canonical) {
        return Err(ToolError::not_found("project not found"));
    }
    let store = require_store(_engine)?;
    let path_text = canonical.to_string_lossy().into_owned();
    store
        .add_project(&path_text, None, crate::session::now_millis())
        .map_err(|error| match error {
            crate::store::StoreError::InvalidProject(message) => ToolError::invalid(message),
            other => store_err(other),
        })?;
    Ok(json!({ "path": path_text }))
}

#[derive(Deserialize)]
struct ProjectUpdateArgs {
    path: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    worktree_mode: Option<ProjectWorktreeMode>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<String>,
}

fn project_update(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let parsed: ProjectUpdateArgs = serde_json::from_value(args.clone())
        .map_err(|_| ToolError::invalid("invalid project update arguments"))?;
    let path = resolve_allowed_path(engine, ctx, &parsed.path, true)?;
    let store = require_store(engine)?;
    if let Some(name) = parsed.name.as_deref() {
        let name = sanitize_bounded_text(name, 256, "name")?;
        store.rename_project(&path, &name).map_err(store_err)?;
    }
    if let Some(mode) = parsed.worktree_mode {
        store
            .set_project_worktree_mode(&path, Some(mode))
            .map_err(store_err)?;
    }
    if parsed.provider.is_some() || parsed.model.is_some() || parsed.reasoning_effort.is_some() {
        store
            .set_project_agent_defaults(
                &path,
                parsed.provider.as_deref(),
                parsed.model.as_deref(),
                parsed.reasoning_effort.as_deref(),
            )
            .map_err(|error| match error {
                crate::store::StoreError::InvalidProject(message) => ToolError::invalid(message),
                other => store_err(other),
            })?;
    }
    Ok(json!({ "path": path, "updated": true }))
}

#[derive(Deserialize)]
struct SessionCreateArgs {
    project_path: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    permission_mode: Option<PermissionMode>,
    #[serde(default)]
    sandbox_policy: Option<SandboxPolicy>,
    #[serde(default)]
    use_worktree: Option<bool>,
}

async fn session_create(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let parsed: SessionCreateArgs = serde_json::from_value(args.clone())
        .map_err(|_| ToolError::invalid("invalid session create arguments"))?;
    let project_path = resolve_allowed_path(engine, ctx, &parsed.project_path, true)?;
    let provider = parsed
        .provider
        .as_deref()
        .unwrap_or("claude_code");
    let provider: ProviderId = serde_json::from_value(json!(provider))
        .map_err(|_| ToolError::invalid("unsupported provider"))?;
    // Submit reports an unregistered provider only as an event, after a long provider wait:
    // reject it, and bad titles, before anything is created.
    if !engine.registered_providers().iter().any(|registered| registered.id == provider) {
        return Err(ToolError::invalid("unsupported provider"));
    }
    let title = parsed.title.as_deref().map(|title| sanitize_bounded_text(title, 256, "title")).transpose()?;
    let initial_policy = match (parsed.permission_mode, parsed.sandbox_policy) {
        (Some(mode), Some(sandbox)) => Some(ExecutionPolicy { mode, sandbox }),
        (Some(mode), None) => Some(ExecutionPolicy {
            mode,
            sandbox: SandboxPolicy::default(),
        }),
        (None, Some(sandbox)) => Some(ExecutionPolicy {
            mode: PermissionMode::Ask,
            sandbox,
        }),
        (None, None) => None,
    };
    if initial_policy.as_ref().is_some_and(|next| !policy_tightens(&ExecutionPolicy::default(), next)) {
        return Err(ToolError::denied("loosening is not available"));
    }
    let command_id = uuid::Uuid::new_v4().to_string();
    let request_id = format!("c2-external-create:{}", json!([command_id, command_id]));
    engine
        .create_session(
            provider,
            project_path.clone(),
            parsed.use_worktree.unwrap_or(false),
            None,
            None,
            Some(request_id.clone()),
            parsed.model.clone(),
            initial_policy,
            false,
            None,
        )
        .await
        .map_err(|error| ToolError::internal(error.to_string()))?;
    // `create_session` also returns Ok when creation failed and only an event says so. No durable
    // receipt means the outcome is unknown: report it, never a success without a session.
    let session_id = require_store(engine)?.command_receipt("c2-external-create", &command_id)
        .map_err(store_err)?.map(|(id, _, _)| id)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| ToolError::conflict("session creation outcome is unknown; do not retry automatically"))?;
    if let Some(title) = title {
        engine.rename_session(&session_id, &title);
    }
    Ok(json!({
        "request_id": request_id,
        "session_id": session_id,
        "project_path": project_path,
    }))
}

#[derive(Deserialize)]
struct SessionUpdateArgs {
    session_id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    pinned: Option<bool>,
    #[serde(default)]
    archived: Option<bool>,
}

fn session_update(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let parsed: SessionUpdateArgs = serde_json::from_value(args.clone())
        .map_err(|_| ToolError::invalid("invalid session update arguments"))?;
    let session = require_session(engine, ctx, &parsed.session_id)?;
    if let Some(title) = parsed.title.as_deref() {
        let title = sanitize_bounded_text(title, 256, "title")?;
        engine.rename_session(&session.id, &title);
    }
    if let Some(pinned) = parsed.pinned {
        engine.set_pinned(&session.id, pinned);
    }
    if let Some(archived) = parsed.archived {
        engine.set_archived(&session.id, archived);
    }
    Ok(json!({ "session_id": session.id, "updated": true }))
}

async fn turn_send(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let session_id = args
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("session_id is required"))?;
    let mode = args
        .get("mode")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("mode is required"))?;
    if !matches!(mode, "prompt" | "queue" | "steer") {
        return Err(ToolError::invalid("mode must be prompt, queue, or steer"));
    }
    let text = args
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("text is required"))?;
    let text = sanitize_bounded_text(text, MAX_PROMPT_TEXT, "text")?;
    let session = require_session(engine, ctx, session_id)?;
    // Everything that can fail without side effects happens before anything is dispatched.
    let store = require_store(engine)?;
    let delivery_id = uuid::Uuid::new_v4().to_string();
    // From the first dispatch on, the prompt may already be durable or running. A failure after
    // that point must keep the receipt id and say "unknown, do not retry": a bare error would
    // hide the id and invite the client to send the same prompt again.
    let unknown = || -> ToolResult { Ok(receipt(&delivery_id, &session.id, None, "unknown")) };
    if mode == "prompt" {
        // A direct prompt leaves no delivery row to inspect, so any failure of `submit` is unknown.
        if engine
            .submit(Op::Prompt {
                session: session.id.clone(),
                doc: vec![DocBlock::Text { text }],
                request_id: Some(delivery_id.clone()),
            })
            .await
            .is_err()
        {
            return unknown();
        }
    } else {
        // Enqueue and drain are separate: the row is durable once the enqueue commits, even when a
        // later step of the enqueue fails or the drain errors; the next drain reconciles it.
        if let Err(error) = engine.enqueue_prompt(
            &session.id,
            vec![DocBlock::Text { text }],
            mode,
            Some(delivery_id.clone()),
            None,
        ) {
            match store.prompt_delivery(&delivery_id) {
                // Never persisted: nothing exists that a retry could duplicate.
                Ok(None) => return Err(ToolError::internal(error)),
                Ok(Some(_)) => {}
                Err(_) => return unknown(),
            }
        }
        if let Err(error) = engine.drain_prompt_deliveries().await {
            tracing::warn!(%error, "external MCP prompt delivery drain failed after enqueue");
        }
    }
    // This is a receipt, never a final result: the turn runs asynchronously, and its outcome is
    // only reported by events / `codetwo_turn_wait` for `delivery_id`. Report what is durable now.
    let (state, turn_id) = match store.prompt_delivery(&delivery_id) {
        Err(_) => return unknown(),
        Ok(Some(delivery)) => match delivery.state.as_str() {
            "failed" => return Err(ToolError::conflict("prompt was not delivered")),
            "unknown" => return unknown(),
            state => (state.to_string(), None),
        },
        // A queued or steered prompt always has its row; losing it is an unknown outcome.
        Ok(None) if mode != "prompt" => return unknown(),
        // A direct prompt has no delivery row: it is accepted only once the stored activity
        // carries this request id. A rejected or already-finished prompt stays merely submitted.
        Ok(None) => match store.get_session(&session.id) {
            Err(_) => return unknown(),
            Ok(stored) => match stored.map(|stored| stored.activity.state) {
                Some(SessionRunState::Running { turn_id, prompt_request_id: Some(id) })
                | Some(SessionRunState::AwaitingInput { turn_id, prompt_request_id: Some(id), .. })
                    if id == delivery_id => ("accepted".to_string(), Some(turn_id)),
                _ => ("submitted".to_string(), None),
            },
        },
    };
    Ok(receipt(&delivery_id, &session.id, turn_id, &state))
}

/// A non-final turn receipt. It is never a reason to send the prompt again: the client waits on
/// `delivery_id` instead, which is why `no_retry` is always set.
fn receipt(delivery_id: &str, session_id: &str, turn_id: Option<String>, state: &str) -> Value {
    json!({
        "delivery_id": delivery_id,
        "session_id": session_id,
        "turn_id": turn_id,
        "state": state,
        "final": false,
        "no_retry": true,
    })
}

async fn turn_stop(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let session_id = args
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("session_id is required"))?;
    let session = require_session(engine, ctx, session_id)?;
    // Cancel queued input first so the stopped turn cannot be followed by the next queued prompt.
    let pending_cancelled = engine.cancel_pending_prompts(&session.id);
    // The active turn is stopped even if the queue could not be cancelled: stopping is the safe
    // direction, but it must not be reported as a complete stop.
    engine
        .submit(Op::Cancel {
            session: session.id.clone(),
        })
        .await
        .map_err(|error| ToolError::internal(error.to_string()))?;
    if pending_cancelled.is_err() {
        return Err(ToolError::conflict(
            "stop was requested for the active turn, but queued prompts could not be cancelled and may still start",
        ));
    }
    Ok(json!({ "session_id": session.id, "stopped": true }))
}

fn approval_respond(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let request_id = args
        .get("request_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("request_id is required"))?;
    let allow = args
        .get("allow")
        .and_then(Value::as_bool)
        .ok_or_else(|| ToolError::invalid("allow is required"))?;
    let (session, pending) = find_pending_input(engine, request_id)?;
    if !ctx.project_allowed(Path::new(session_project_path(&session))) {
        return Err(ToolError::not_found("request not found"));
    }
    if pending.kind != PendingInputKind::Permission {
        return Err(ToolError::conflict("request is not a permission approval"));
    }
    let option_id = if allow {
        Some(pending.options.iter().find(|(id, _)| pending.option_kinds.get(id).is_some_and(|kind| kind == "allow_once"))
            .map(|(id, _)| id.clone()).ok_or_else(|| ToolError::conflict("permission request has no allow-once option"))?)
    } else { None };
    let accepted = engine.answer_permission(
        &session.id,
        &pending.input_id,
        option_id.as_deref(),
    );
    if !accepted {
        return Err(ToolError::conflict("permission request is stale or already resolved"));
    }
    Ok(json!({
        "request_id": pending.input_id,
        "session_id": session.id,
        "kind": "permission",
        "tool": pending.context.tool,
        "context_kind": pending.context.kind,
        "resolved": true,
    }))
}

fn question_respond(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let request_id = args
        .get("request_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("request_id is required"))?;
    let answer_text = args
        .get("answer")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("answer is required"))?;
    let answer_text = sanitize_bounded_text(answer_text, 8192, "answer")?;
    let (session, pending) = find_pending_input(engine, request_id)?;
    if !ctx.project_allowed(Path::new(session_project_path(&session))) {
        return Err(ToolError::not_found("request not found"));
    }
    if pending.kind != PendingInputKind::Elicitation {
        return Err(ToolError::conflict("request is not a structured question"));
    }
    let answer = ElicitationAnswer::Accept {
        content: serde_json::Map::from_iter([(
            "answer".to_string(),
            Value::String(answer_text),
        )]),
    };
    let accepted = engine.answer_elicitation(&session.id, &pending.input_id, answer);
    if !accepted {
        return Err(ToolError::conflict("question is stale or already resolved"));
    }
    Ok(json!({
        "request_id": pending.input_id,
        "session_id": session.id,
        "kind": "question",
        "resolved": true,
    }))
}

async fn model_set(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let session_id = args
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("session_id is required"))?;
    let model_id = args
        .get("model_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("model_id is required"))?;
    let model_id = sanitize_bounded_text(model_id, 256, "model_id")?;
    let session = require_session(engine, ctx, session_id)?;
    let session_id = session.id.clone();
    engine
        .submit(Op::SetModel {
            session: session_id.clone(),
            model: model_id.clone(),
        })
        .await
        .map_err(|error| ToolError::internal(error.to_string()))?;
    // `submit` is Ok even when the engine rejected the change (busy turn, agent refusal, persist
    // error); the stored session is the only truth.
    let stored = require_session(engine, ctx, &session_id)?;
    if stored.model.as_deref() != Some(model_id.as_str()) {
        return Err(ToolError::conflict("model change was not applied"));
    }
    Ok(json!({ "session_id": session_id, "model_id": model_id }))
}

#[derive(Deserialize)]
struct PolicySetArgs {
    session_id: String,
    #[serde(default)]
    mode: Option<PermissionMode>,
    #[serde(default)]
    sandbox_policy: Option<SandboxPolicy>,
}

fn policy_set(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let parsed: PolicySetArgs = serde_json::from_value(args.clone())
        .map_err(|_| ToolError::invalid("invalid policy arguments"))?;
    if parsed.mode.is_none() && parsed.sandbox_policy.is_none() {
        return Err(ToolError::invalid("mode or sandbox_policy is required"));
    }
    // Scope check only: the policy in force is read, compared and written by the Engine in one
    // critical section shared with every native policy writer. Reading it here and submitting a
    // full pair would let a concurrent tightening be overwritten (and loosened) by a stale read.
    let session = require_session(engine, ctx, &parsed.session_id)?;
    match engine.tighten_execution_policy(&session.id, parsed.mode, parsed.sandbox_policy) {
        Ok(PolicyCommit::Applied(applied)) => Ok(json!({
            "session_id": session.id,
            "mode": applied.mode,
            "sandbox_policy": applied.sandbox,
        })),
        Ok(PolicyCommit::Loosens(_)) => Err(ToolError::denied("loosening is not available")),
        Ok(PolicyCommit::NoSession) => Err(ToolError::not_found("session not found")),
        // Nothing was written: the durable and live policy both stay on the previous pair.
        Err(_) => Err(ToolError::conflict("policy change was not applied")),
    }
}

fn automation_run(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("id is required"))?;
    let id = require_id(id, "id")?;
    let store = require_store(engine)?;
    let automation = store
        .automation(&id)
        .map_err(store_err)?
        .ok_or_else(|| ToolError::not_found("automation not found"))?;
    if !ctx.project_allowed(Path::new(&automation.project_path)) {
        return Err(ToolError::not_found("automation not found"));
    }
    let now = crate::session::now_millis();
    let (_, run) = store
        .create_manual_automation_run(&id, now)
        .map_err(store_err)?
        .ok_or_else(|| ToolError::conflict("automation not found or already has an active run"))?;
    Ok(json!({
        "automation_id": id,
        "run_id": run.id,
        "status": run.status,
    }))
}

//! External MCP tools: admin (destructive). Implements `pub fn call` (see `ctx::OpsFn`).

use crate::automation::AutomationInput;
#[cfg(test)]
use crate::event::Op;
use crate::external_mcp::ctx::{CallContext, ToolError, ToolResult};
use crate::external_mcp::ops_write::{
    canonical_existing_dir, policy_tightens, require_id, require_session, require_store,
    resolve_allowed_path, run_async, store_err,
};
use crate::permission::{ExecutionPolicy, PermissionMode, SandboxPolicy};
use crate::provider::ProviderId;
use crate::scene::{session_mode_policy, HookActionKind, HookEvent, Scene, SceneLibrary};
use crate::session::{Session, SessionRunState};
use crate::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

pub fn call(
    engine: &Engine,
    ctx: &CallContext<'_>,
    tool: &str,
    args: &Value,
) -> Option<ToolResult> {
    Some(match tool {
        "codetwo_project_delete" => project_delete(engine, ctx, args),
        "codetwo_session_delete" => session_delete(engine, ctx, args),
        "codetwo_worktree_delete" => run_async(worktree_delete(engine, ctx, args)),
        "codetwo_automation_create" => automation_create(engine, ctx, args),
        "codetwo_automation_update" => automation_update(engine, ctx, args),
        "codetwo_automation_delete" => automation_delete(engine, ctx, args),
        "codetwo_scene_run" => scene_run(engine, ctx, args),
        "codetwo_pipeline_run" => pipeline_run(engine, ctx, args),
        _ => return None,
    })
}

fn policy_of(session: &Session) -> ExecutionPolicy {
    ExecutionPolicy {
        mode: session.permission_mode,
        sandbox: session.sandbox_policy,
    }
}

/// A scene may only tighten each policy axis on its own (`scenes::apply_execution` ranks one
/// combined mode and lets e.g. (Ask, DangerFullAccess) -> (AcceptEdits, WorkspaceWrite) through).
/// `entered` scenes are activated now, so an enter `run_macro` hook would start a provider turn
/// that needs the very session slot we hold: refuse instead of dropping it silently.
fn scene_safe(base: &ExecutionPolicy, scene: &Scene, entered: bool) -> Result<(), ToolError> {
    if let Some(mode) = scene
        .execution
        .as_ref()
        .and_then(|execution| execution.session_mode)
    {
        if !policy_tightens(base, &session_mode_policy(mode)) {
            return Err(ToolError::denied("scene would loosen the execution policy"));
        }
    }
    let macro_on_enter = scene.hooks.iter().any(|hook| {
        hook.on == HookEvent::Enter && matches!(hook.action.kind, HookActionKind::RunMacro)
    });
    if entered && macro_on_enter {
        return Err(ToolError::denied(
            "scene starts a provider turn when entered",
        ));
    }
    Ok(())
}

/// Run `apply` while holding the session's turn slot, so no prompt can start meanwhile. The policy
/// is read and checked under the reservation; if a concurrent library reload still made `apply`
/// loosen it, restore the previous pair (still reserved, so no turn ran under the looser one).
fn under_reservation(
    engine: &Engine,
    ctx: &CallContext<'_>,
    session_id: &str,
    check: impl FnOnce(&ExecutionPolicy) -> Result<(), ToolError>,
    apply: impl FnOnce() -> ToolResult,
) -> ToolResult {
    let _slot = engine
        .reserve_idle_session(session_id)
        .map_err(ToolError::conflict)?;
    let before = policy_of(&require_session(engine, ctx, session_id)?);
    check(&before)?;
    let result = apply();
    let after = policy_of(&require_session(engine, ctx, session_id)?);
    if !policy_tightens(&before, &after) {
        // Restore each ceiling monotonically against the current locked value. A concurrent
        // human tightening of one axis must never be overwritten by restoring the old pair.
        for (mode, sandbox) in [(Some(before.mode), None), (None, Some(before.sandbox))] {
            match engine.tighten_execution_policy(session_id, mode, sandbox) {
                Ok(
                    crate::engine::PolicyCommit::Applied(_)
                    | crate::engine::PolicyCommit::Loosens(_),
                ) => {}
                _ => return Err(ToolError::internal("couldn't restore the execution policy")),
            }
        }
        let restored = policy_of(&require_session(engine, ctx, session_id)?);
        if !policy_tightens(&before, &restored) {
            return Err(ToolError::internal("couldn't restore the execution policy"));
        }
        return Err(ToolError::denied("scene would loosen the execution policy"));
    }
    result
}

fn scene_run(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("id required"))?;
    let session_id = args
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("session_id required"))?;
    let session = require_session(engine, ctx, session_id)?;
    let library = engine.scenes();
    let entry = library
        .resolve(id)
        .ok_or_else(|| ToolError::not_found("scene not found"))?;
    let (reference, scene) = (SceneLibrary::reference_for(entry), &entry.scene);
    under_reservation(
        engine,
        ctx,
        &session.id,
        |base| scene_safe(base, scene, true),
        || {
            engine
                .call_scene_command(
                    "scenes.apply",
                    json!({
                        "session": session.id, "reference": reference, "confirm_escalation": false,
                    }),
                )
                .map_err(ToolError::internal)
        },
    )
}

fn pipeline_run(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("id required"))?;
    let path = args
        .get("project_path")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("project_path required"))?;
    let project = resolve_allowed_path(engine, ctx, path, true)?;
    let session = if let Some(id) = args.get("session_id").and_then(Value::as_str) {
        let session = require_session(engine, ctx, id)?;
        if crate::external_mcp::ops_write::session_project_path(&session) != project {
            return Err(ToolError::not_found("session not found in project"));
        }
        Some(session.id)
    } else {
        None
    };
    let library = engine.scenes();
    let entry = library
        .resolve_pipeline(id)
        .ok_or_else(|| ToolError::not_found("pipeline not found"))?;
    let reference = SceneLibrary::pipeline_reference_for(entry);
    let stages = &entry.pipeline.stages;
    let entry_stage = entry
        .pipeline
        .entry
        .as_deref()
        .or(stages.first().map(|stage| stage.id.as_str()));
    let entry_index = stages
        .iter()
        .position(|stage| Some(stage.id.as_str()) == entry_stage)
        .ok_or_else(|| ToolError::invalid("pipeline has no entry stage"))?;
    // Every stage's scene must be safe, not just the entry: later stages are applied to this
    // session (or to sessions spawned from the plan) under the same ceiling.
    let scenes = stages
        .iter()
        .map(|stage| {
            library
                .resolve(&stage.scene)
                .map(|resolved| &resolved.scene)
                .ok_or_else(|| ToolError::invalid("pipeline references an unknown scene"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let check = |base: &ExecutionPolicy| {
        scenes.iter().enumerate().try_for_each(|(index, scene)| {
            scene_safe(base, scene, session.is_some() && index == entry_index)
        })
    };
    let start = || {
        engine
            .call_scene_command(
                "pipelines.start",
                json!({
                    "reference": reference, "project_path": project, "session": session,
                }),
            )
            .map_err(ToolError::internal)
    };
    match session.as_deref() {
        Some(session_id) => under_reservation(engine, ctx, session_id, check, start),
        // No session to bound against: the default policy is the ceiling for every stage.
        None => check(&ExecutionPolicy::default()).and_then(|_| start()),
    }
}

fn project_delete(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("path is required"))?;
    let path = resolve_allowed_path(engine, ctx, path, true)?;
    let store = require_store(engine)?;
    store.remove_project(&path).map_err(store_err)?;
    Ok(json!({ "path": path, "unregistered": true }))
}

fn session_delete(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let session_id = args
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("session_id is required"))?;
    let session = require_session(engine, ctx, session_id)?;
    let removed = engine
        .delete_idle_session(&session.id)
        .map_err(ToolError::conflict)?;
    Ok(json!({ "session_id": session.id, "deleted": removed }))
}

async fn worktree_delete(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let project_path = args
        .get("project_path")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("project_path is required"))?;
    let worktree_id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("id is required"))?;
    let project_path = resolve_allowed_path(engine, ctx, project_path, true)?;
    let worktree_path = canonical_existing_dir(worktree_id)?;
    let sessions = engine.list_sessions().map_err(store_err)?;
    for session in &sessions {
        if session.worktree_path.as_deref() == Some(worktree_path.to_string_lossy().as_ref()) {
            if matches!(
                session.activity.state,
                SessionRunState::Running { .. } | SessionRunState::AwaitingInput { .. }
            ) {
                return Err(ToolError::conflict(
                    "refusing to delete the worktree of a running session",
                ));
            }
        }
    }
    engine
        .discard_orphan_worktree(&project_path, worktree_path.to_string_lossy().as_ref())
        .await
        .map_err(|error| ToolError::internal(error))?;
    Ok(json!({
        "project_path": project_path,
        "worktree_path": worktree_path.to_string_lossy(),
        "discarded": true,
    }))
}

#[derive(Deserialize)]
struct AutomationCreateArgs {
    name: String,
    prompt: String,
    project_path: String,
    provider: String,
    cron: String,
    timezone: String,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    use_worktree: bool,
    #[serde(default)]
    permission_mode: Option<PermissionMode>,
    #[serde(default)]
    sandbox_policy: Option<SandboxPolicy>,
}

fn automation_create(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let parsed: AutomationCreateArgs = serde_json::from_value(args.clone())
        .map_err(|_| ToolError::invalid("invalid automation create arguments"))?;
    let project_path = resolve_allowed_path(engine, ctx, &parsed.project_path, true)?;
    let provider: ProviderId = serde_json::from_value(json!(parsed.provider))
        .map_err(|_| ToolError::invalid("unsupported provider"))?;
    let policy = crate::permission::ExecutionPolicy {
        mode: parsed.permission_mode.unwrap_or(PermissionMode::Ask),
        sandbox: parsed
            .sandbox_policy
            .unwrap_or(SandboxPolicy::WorkspaceWrite),
    };
    if !crate::external_mcp::ops_write::policy_tightens(&Default::default(), &policy) {
        return Err(ToolError::denied("loosening is not available"));
    }
    let store = require_store(engine)?;
    let automation = store
        .create_automation(
            AutomationInput {
                name: parsed.name,
                prompt: parsed.prompt,
                project_path,
                provider,
                cron: parsed.cron,
                timezone: parsed.timezone,
                enabled: parsed.enabled,
                use_worktree: parsed.use_worktree,
                permission_mode: parsed.permission_mode.unwrap_or(PermissionMode::Ask),
                sandbox_policy: parsed
                    .sandbox_policy
                    .unwrap_or(SandboxPolicy::WorkspaceWrite),
            },
            crate::session::now_millis(),
        )
        .map_err(store_err)?;
    Ok(json!({ "id": automation.id, "created": true }))
}

#[derive(Deserialize)]
struct AutomationUpdateArgs {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    project_path: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    cron: Option<String>,
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    use_worktree: Option<bool>,
    #[serde(default)]
    permission_mode: Option<PermissionMode>,
    #[serde(default)]
    sandbox_policy: Option<SandboxPolicy>,
}

fn automation_update(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let parsed: AutomationUpdateArgs = serde_json::from_value(args.clone())
        .map_err(|_| ToolError::invalid("invalid automation update arguments"))?;
    let id = require_id(&parsed.id, "id")?;
    let store = require_store(engine)?;
    let current = store
        .automation(&id)
        .map_err(store_err)?
        .ok_or_else(|| ToolError::not_found("automation not found"))?;
    if !ctx.project_allowed(Path::new(&current.project_path)) {
        return Err(ToolError::not_found("automation not found"));
    }
    let project_path = match parsed.project_path.as_deref() {
        Some(path) => resolve_allowed_path(engine, ctx, path, true)?,
        None => current.project_path.clone(),
    };
    let provider = match parsed.provider.as_deref() {
        Some(provider) => serde_json::from_value(json!(provider))
            .map_err(|_| ToolError::invalid("unsupported provider"))?,
        None => current.provider.clone(),
    };
    let policy = crate::permission::ExecutionPolicy {
        mode: parsed.permission_mode.unwrap_or(current.permission_mode),
        sandbox: parsed.sandbox_policy.unwrap_or(current.sandbox_policy),
    };
    if !crate::external_mcp::ops_write::policy_tightens(
        &crate::permission::ExecutionPolicy {
            mode: current.permission_mode,
            sandbox: current.sandbox_policy,
        },
        &policy,
    ) {
        return Err(ToolError::denied("loosening is not available"));
    }
    let updated = store
        .update_automation(
            &id,
            AutomationInput {
                name: parsed.name.unwrap_or(current.name),
                prompt: parsed.prompt.unwrap_or(current.prompt),
                project_path,
                provider,
                cron: parsed.cron.unwrap_or(current.cron),
                timezone: parsed.timezone.unwrap_or(current.timezone),
                enabled: parsed.enabled.unwrap_or(current.enabled),
                use_worktree: parsed.use_worktree.unwrap_or(current.use_worktree),
                permission_mode: parsed.permission_mode.unwrap_or(current.permission_mode),
                sandbox_policy: parsed.sandbox_policy.unwrap_or(current.sandbox_policy),
            },
            crate::session::now_millis(),
        )
        .map_err(store_err)?
        .ok_or_else(|| ToolError::not_found("automation not found"))?;
    Ok(json!({ "id": updated.id, "updated": true }))
}

fn automation_delete(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
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
    let deleted = store.delete_automation(&id).map_err(|error| match error {
        crate::store::StoreError::InvalidAutomation(message) => ToolError::conflict(message),
        other => store_err(other),
    })?;
    Ok(json!({ "id": id, "deleted": deleted }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_mcp::catalog::external_tool_catalog;
    use crate::external_mcp::clients::{
        ExternalClientRecord, ExternalScope, ProjectScope, ResolvedClient,
    };
    use crate::external_mcp::ctx::ToolErrorKind;
    use crate::provider::{LaunchSpec, Provider};
    use crate::skill::SkillLibrary;
    use crate::Store;

    /// `apply` loosens the policy (as a concurrent library reload could); `freeze` then decides
    /// whether the engine can still write when the restore is attempted.
    fn loosened_then_restore(
        freeze: bool,
        stronger_sandbox: bool,
    ) -> (ToolError, (PermissionMode, SandboxPolicy)) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("c2.db").to_string_lossy().into_owned();
        let store = std::sync::Arc::new(Store::open(&db).unwrap());
        let (engine, _events) = Engine::with_store(
            vec![Provider {
                id: ProviderId::ClaudeCode,
                display_name: "Test".into(),
                needs_node: false,
                launch: LaunchSpec {
                    command: "false".into(),
                    args: Vec::new(),
                    env: Vec::new(),
                    cwd: None,
                },
            }],
            SkillLibrary::default(),
            store.clone(),
        );
        let mut session = Session::new(
            ProviderId::ClaudeCode,
            dir.path().to_string_lossy().into_owned(),
        );
        session.permission_mode = PermissionMode::Ask;
        session.sandbox_policy = if stronger_sandbox {
            SandboxPolicy::WorkspaceWrite
        } else {
            SandboxPolicy::ReadOnly
        };
        store.upsert_session(&session).unwrap();
        let client = ResolvedClient {
            record: ExternalClientRecord {
                id: "c".into(),
                name: "c".into(),
                scopes: vec![ExternalScope::Admin],
                projects: ProjectScope::All,
                created_at: chrono::Utc::now(),
                expires_at: None,
                last_used_at: None,
                revoked_at: None,
            },
        };
        let spec = external_tool_catalog()
            .iter()
            .find(|spec| spec.name == "codetwo_scene_run")
            .unwrap();
        let ctx = CallContext {
            client: &client,
            spec,
        };
        let error = under_reservation(&engine, &ctx, &session.id, |_| Ok(()), || {
            run_async(async {
                engine
                    .submit(Op::SetExecutionPolicy {
                        session: session.id.clone(),
                        mode: PermissionMode::Yolo,
                        sandbox: if stronger_sandbox { SandboxPolicy::ReadOnly } else { SandboxPolicy::DangerFullAccess },
                        request_id: None,
                    })
                    .await
                    .map(|_| Value::Null)
                    .map_err(|e| ToolError::internal(e.to_string()))
            })?;
            if freeze {
                rusqlite::Connection::open(&db)
                    .unwrap()
                    .execute_batch("CREATE TRIGGER freeze BEFORE UPDATE ON sessions BEGIN SELECT RAISE(ABORT,'frozen'); END;")
                    .unwrap();
            }
            Ok(Value::Null)
        })
        .unwrap_err();
        let stored = store.get_session(&session.id).unwrap().unwrap();
        (error, (stored.permission_mode, stored.sandbox_policy))
    }

    #[test]
    fn restore_preserves_a_concurrently_tightened_axis() {
        let (error, policy) = loosened_then_restore(false, true);
        assert_eq!(error.kind, ToolErrorKind::Denied);
        assert_eq!(policy, (PermissionMode::Ask, SandboxPolicy::ReadOnly));
    }

    #[test]
    fn a_loosened_policy_is_restored_and_a_failed_restore_is_not_hidden() {
        let (error, policy) = loosened_then_restore(false, false);
        assert_eq!(error.kind, ToolErrorKind::Denied);
        assert_eq!(policy, (PermissionMode::Ask, SandboxPolicy::ReadOnly));
        // The engine accepted the restore op but could not persist it: still loosened, so Internal.
        let (error, policy) = loosened_then_restore(true, false);
        assert_eq!(error.kind, ToolErrorKind::Internal);
        assert_eq!(
            policy,
            (PermissionMode::Yolo, SandboxPolicy::DangerFullAccess)
        );
    }
}

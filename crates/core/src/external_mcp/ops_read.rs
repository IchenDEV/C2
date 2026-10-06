//! External MCP read-class tools.

use crate::external_mcp::ctx::{CallContext, ToolError, ToolResult};
use crate::external_mcp::events::{sanitize_text, DEFAULT_TEXT_MAX_CHARS};
use crate::external_mcp::gate::session_project_path;
use crate::external_mcp::state::{PendingExternalRequest, EXTERNAL_MCP_ENV};
use crate::git::{self, DiffScope};
use crate::scene::SceneLibrary;
use crate::session::PendingInput;
use crate::session::{PendingInputKind, Session, SessionRunState};
use crate::Engine;
use chrono::Utc;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const MAX_FILE_BYTES: usize = 128 * 1024;
const LIST_LIMIT: usize = 50;
const SEARCH_BUDGET: std::time::Duration = std::time::Duration::from_secs(10);

pub fn call(
    engine: &Engine,
    ctx: &CallContext<'_>,
    tool: &str,
    args: &Value,
) -> Option<ToolResult> {
    match tool {
        "codetwo_capabilities" => Some(capabilities(engine, ctx)),
        "codetwo_project_list" => Some(project_list(engine, ctx, args)),
        "codetwo_project_read" => Some(project_read(engine, ctx, args)),
        "codetwo_session_list" => Some(session_list(engine, ctx, args)),
        "codetwo_session_read" => Some(session_read(engine, ctx, args)),
        "codetwo_session_search" => Some(session_search(engine, ctx, args)),
        "codetwo_transcript_read" => Some(transcript_read(engine, ctx, args)),
        "codetwo_approval_list" => Some(approval_list(engine, ctx, args)),
        "codetwo_question_list" => Some(question_list(engine, ctx, args)),
        "codetwo_model_list" => Some(model_list(engine, ctx)),
        "codetwo_policy_read" => Some(policy_read(engine, ctx, args)),
        "codetwo_worktree_list" => Some(worktree_list(engine, ctx, args)),
        "codetwo_git_status" => Some(git_status(engine, ctx, args)),
        "codetwo_git_diff" => Some(git_diff(engine, ctx, args)),
        "codetwo_workspace_file_read" => Some(workspace_file_read(engine, ctx, args)),
        "codetwo_workspace_search" => Some(workspace_search(engine, ctx, args)),
        "codetwo_automation_list" => Some(automation_list(engine, ctx, args)),
        "codetwo_scene_list" => Some(scene_list(engine, ctx, args)),
        "codetwo_pipeline_list" => Some(pipeline_list(engine, ctx, args)),
        "codetwo_subagent_list" => Some(subagent_list(engine, ctx, args)),
        "codetwo_events_poll" | "codetwo_events_wait" | "codetwo_turn_wait" => None,
        _ => None,
    }
}

fn limit_from_args(args: &Value) -> usize {
    args.get("limit")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(LIST_LIMIT)
        .clamp(1, LIST_LIMIT)
}

fn capabilities(engine: &Engine, ctx: &CallContext<'_>) -> ToolResult {
    let tools: Vec<_> = crate::external_mcp::catalog::catalog_for_client(&ctx.client.record.scopes)
        .into_iter()
        .filter_map(|entry| {
            entry
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();
    Ok(json!({
        "external_mcp": {
            "enabled": engine.external_mcp_state().is_enabled(),
            "env": EXTERNAL_MCP_ENV,
            "protocol": "2025-06-18",
            "events_spec": { "status": "draft", "label": "experimental-ext-triggers-events" },
            "mcp_events_subscriptions": { "status": "experimental", "version": "2026-07-28" },
        },
        "client_scopes": ctx.client.record.scopes,
        "tools_for_client": tools,
        "providers": engine.registered_providers().iter().map(|p| json!({
            "id": p.id.as_str(),
            "display_name": p.display_name,
        })).collect::<Vec<_>>(),
    }))
}

fn project_list(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let store = engine
        .store()
        .ok_or_else(|| ToolError::internal("no store"))?;
    let limit = limit_from_args(args);
    let projects = store
        .list_projects()
        .map_err(|e| ToolError::internal(e.to_string()))?;
    let items: Vec<_> = projects
        .into_iter()
        .filter(|p| ctx.project_allowed(Path::new(&p.path)))
        .take(limit)
        .map(|p| json!({ "path": p.path, "name": p.name }))
        .collect();
    Ok(json!({ "projects": items }))
}

fn project_read(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("path required"))?;
    if !ctx.project_allowed(Path::new(path)) {
        return Err(ToolError::not_found("project not found"));
    }
    let store = engine
        .store()
        .ok_or_else(|| ToolError::internal("no store"))?;
    let projects = store
        .list_projects()
        .map_err(|e| ToolError::internal(e.to_string()))?;
    let Some(project) = projects.into_iter().find(|p| p.path == path) else {
        return Err(ToolError::not_found("project not found"));
    };
    Ok(json!({
        "path": project.path,
        "name": project.name,
        "default_provider": project.default_provider,
        "default_model": project.default_model,
    }))
}

fn session_list(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let limit = limit_from_args(args);
    let sessions = engine
        .list_sessions()
        .map_err(|e| ToolError::internal(e.to_string()))?;
    let items: Vec<_> = sessions
        .into_iter()
        .filter(|s| !s.transient)
        .filter(|s| session_visible(ctx, s))
        .take(limit)
        .map(|s| session_summary(&s))
        .collect();
    Ok(json!({ "sessions": items }))
}

fn session_read(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let id = args
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("session_id required"))?;
    let session = load_session(engine, id)?;
    if !session_visible(ctx, &session) {
        return Err(ToolError::not_found("session not found"));
    }
    Ok(session_summary(&session))
}

fn session_search(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("query required"))?;
    let limit = limit_from_args(args);
    let hits = engine
        .search_sessions(query, limit)
        .map_err(|e| ToolError::internal(e.to_string()))?;
    let sessions = engine
        .list_sessions()
        .map_err(|e| ToolError::internal(e.to_string()))?;
    let by_id: std::collections::HashMap<_, _> =
        sessions.into_iter().map(|s| (s.id.clone(), s)).collect();
    let items: Vec<_> = hits
        .into_iter()
        .filter_map(|hit| by_id.get(&hit.session_id))
        .filter(|s| session_visible(ctx, s))
        .take(limit)
        .map(|s| session_summary(&s))
        .collect();
    Ok(json!({ "sessions": items }))
}

fn transcript_read(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let id = args
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("session_id required"))?;
    let session = load_session(engine, id)?;
    if !session_visible(ctx, &session) {
        return Err(ToolError::not_found("session not found"));
    }
    let limit = limit_from_args(args);
    let before = args
        .get("cursor")
        .and_then(Value::as_str)
        .map(|cursor| {
            cursor
                .parse::<i64>()
                .map(crate::session::TranscriptCursor)
                .map_err(|_| ToolError::invalid("invalid transcript cursor"))
        })
        .transpose()?;
    let page = engine
        .transcript_page(id, before, limit)
        .map_err(|e| ToolError::internal(e.to_string()))?;
    Ok(json!({
        "session_id": id,
        "entries": page.entries,
        "next_before": page.next_before,
        "snapshot_through": page.snapshot_through,
    }))
}

fn approval_list(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let limit = limit_from_args(args);
    let items = pending_for_kind(engine, ctx, PendingInputKind::Permission, limit);
    Ok(json!({ "approvals": items }))
}

fn question_list(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let limit = limit_from_args(args);
    let items = pending_for_kind(engine, ctx, PendingInputKind::Elicitation, limit);
    Ok(json!({ "questions": items }))
}

fn pending_for_kind(
    engine: &Engine,
    ctx: &CallContext<'_>,
    kind: PendingInputKind,
    limit: usize,
) -> Vec<Value> {
    let mut out = Vec::new();
    for record in engine.external_mcp_state().list_pending() {
        if record.kind != kind {
            continue;
        }
        let Ok(session) = load_session(engine, &record.session_id) else {
            continue;
        };
        if !session_visible(ctx, &session) {
            continue;
        }
        out.push(json!({
            "request_id": record.request_id,
            "session_id": record.session_id,
            "title": record.title,
            "tool_name": record.tool_name,
        }));
        if out.len() >= limit {
            break;
        }
    }
    if out.is_empty() {
        if let Ok(sessions) = engine.list_sessions() {
            for session in sessions {
                if !session_visible(ctx, &session) {
                    continue;
                }
                let SessionRunState::AwaitingInput { pending, .. } = &session.activity.state else {
                    continue;
                };
                for item in pending
                    .iter()
                    .filter(|p| p.kind == kind)
                    .take(limit.saturating_sub(out.len()))
                {
                    out.push(pending_item_json(&session.id, item));
                    if out.len() >= limit {
                        break;
                    }
                }
            }
        }
    }
    out
}

fn pending_item_json(session_id: &str, item: &PendingInput) -> Value {
    json!({
        "request_id": item.input_id,
        "session_id": session_id,
        "title": sanitize_text(&item.title, DEFAULT_TEXT_MAX_CHARS),
        "tool_name": item.context.tool.as_deref().map(|t| sanitize_text(t, DEFAULT_TEXT_MAX_CHARS)),
    })
}

fn model_list(engine: &Engine, _ctx: &CallContext<'_>) -> ToolResult {
    let catalog: Vec<_> = engine
        .provider_catalog()
        .into_iter()
        .map(|(provider, models)| {
            json!({
                "provider": provider.id.as_str(),
                "models": models.iter().map(|m| &m.id).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({ "providers": catalog }))
}

fn policy_read(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let id = args
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("session_id required"))?;
    let session = load_session(engine, id)?;
    if !session_visible(ctx, &session) {
        return Err(ToolError::not_found("session not found"));
    }
    Ok(json!({
        "session_id": id,
        "permission_mode": session.permission_mode,
        "sandbox_policy": session.sandbox_policy,
    }))
}

fn worktree_list(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let path = registered_project_path(engine, ctx, args)?;
    let entries = block_on(engine.list_project_worktrees(&path)).map_err(ToolError::internal)?;
    Ok(json!({"worktrees": entries}))
}

fn git_status(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let path = registered_project_path(engine, ctx, args)?;
    let (_, prefix) = repo_location(&path);
    let mut status = block_on(git::status(Path::new(&path)));
    // Git reports the whole repository; a project may be only a subdirectory of it.
    status.files = std::mem::take(&mut status.files)
        .into_iter()
        .filter_map(|mut file| {
            file.path = within_project(&prefix, &file.path)?;
            if let Some(original) = file.original_path.take() {
                file.original_path = Some(within_project(&prefix, &original)?);
            }
            Some(file)
        })
        .collect();
    Ok(serde_json::to_value(status).unwrap_or(json!({})))
}

/// The git work tree root containing `project`, and the project's directory relative to it
/// (`""` or `"sub/dir/"`). Not a repository: the project itself and `""`.
fn repo_location(project: &str) -> (PathBuf, String) {
    std::process::Command::new("git")
        .arg("-C")
        .arg(project)
        .args(["rev-parse", "--show-toplevel", "--show-prefix"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| {
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            let (root, prefix) = text.split_once('\n')?;
            Some((
                PathBuf::from(root),
                prefix.trim_end_matches('\n').to_string(),
            ))
        })
        .unwrap_or_else(|| (PathBuf::from(project), String::new()))
}

/// `repo_path` made relative to the project, or `None` when it lies outside the project.
fn within_project(prefix: &str, repo_path: &str) -> Option<String> {
    repo_path
        .strip_prefix(prefix)
        .filter(|rest| !rest.is_empty())
        .map(str::to_string)
}

fn git_diff(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let path = registered_project_path(engine, ctx, args)?;
    // `git::diff` pathspecs are repository-relative, so it only works from the work tree root;
    // run it there and keep what lies inside the project.
    let (root, prefix) = repo_location(&path);
    let diff = block_on(git::diff(&root, None, DiffScope::Unstaged))
        .map_err(|_| ToolError::internal("git diff failed"))?;
    let max = args
        .get("max_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(32_768)
        .min(128 * 1024) as usize;
    let mut text = String::new();
    let mut truncated = diff.truncated;
    for file in diff.file_diffs {
        let safe = |name: &str| {
            !is_secret_path(Path::new(name))
                && !Path::new(name).is_absolute()
                && !Path::new(name)
                    .components()
                    .any(|part| part == std::path::Component::ParentDir)
        };
        // Both sides must be inside the project (a rename may cross its boundary) and safe.
        let new_path = within_project(&prefix, &file.path);
        let old_path = file
            .old_path
            .as_deref()
            .map(|old| within_project(&prefix, old));
        let (Some(new_path), true) = (new_path, old_path.as_ref().is_none_or(Option::is_some))
        else {
            continue;
        };
        let old_path = old_path.flatten();
        if !safe(&new_path) || old_path.as_deref().is_some_and(|old| !safe(old)) {
            continue;
        }
        text.push_str(&format!(
            "--- a/{}\n+++ b/{}\n",
            old_path.as_ref().unwrap_or(&new_path),
            new_path
        ));
        for hunk in file.hunks {
            text.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
            ));
            for line in hunk.lines {
                text.push(match line.kind {
                    git::DiffLineKind::Context => ' ',
                    git::DiffLineKind::Added => '+',
                    git::DiffLineKind::Removed => '-',
                });
                text.push_str(&line.text);
                text.push('\n');
            }
        }
        if text.len() >= max {
            truncated = true;
            break;
        }
    }
    if text.len() > max {
        let mut boundary = max;
        while !text.is_char_boundary(boundary) {
            boundary -= 1;
        }
        text.truncate(boundary);
    }
    Ok(json!({"diff": text, "truncated": truncated}))
}

fn workspace_file_read(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let file = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("path required"))?;
    let target = std::fs::canonicalize(file).map_err(|_| ToolError::not_found("file not found"))?;
    if !ctx.project_allowed(&target) || !in_registered_project(engine, &target) {
        return Err(ToolError::not_found("file not found"));
    }
    if is_secret_path(&target) {
        return Err(ToolError::denied("path denied"));
    }
    let max = args
        .get("max_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(MAX_FILE_BYTES as u64)
        .min(MAX_FILE_BYTES as u64) as usize;
    use std::io::Read;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
    }
    let file = options
        .open(&target)
        .map_err(|_| ToolError::not_found("file not found"))?;
    let metadata = file
        .metadata()
        .map_err(|_| ToolError::not_found("file not found"))?;
    if !metadata.is_file() {
        return Err(ToolError::denied("only regular files can be read"));
    }
    let size = metadata.len();
    let mut bytes = Vec::new();
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ToolError::not_found("file not found"))?;
    if bytes.len() > max {
        return Ok(json!({
            "truncated": true,
            "size": size,
            "preview": String::from_utf8_lossy(&bytes[..max]),
        }));
    }
    Ok(json!({
        "path": target.display().to_string(),
        "content": String::from_utf8_lossy(&bytes),
    }))
}

fn workspace_search(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("query required"))?;
    let requested = args.get("project_path").and_then(Value::as_str);
    let store = engine
        .store()
        .ok_or_else(|| ToolError::internal("no store"))?;
    let roots: Vec<_> = store
        .list_projects()
        .map_err(|_| ToolError::internal("project lookup failed"))?
        .into_iter()
        .filter_map(|p| PathBuf::from(p.path).canonicalize().ok())
        .filter(|root| ctx.project_allowed(root))
        .filter(|root| {
            requested.is_none_or(|path| Path::new(path).canonicalize().ok().as_ref() == Some(root))
        })
        .take(LIST_LIMIT)
        .collect();
    if roots.is_empty() {
        return Err(ToolError::not_found("project not found"));
    }
    let limit = limit_from_args(args);
    let deadline = std::time::Instant::now() + SEARCH_BUDGET;
    search_roots(roots, limit, deadline, |root, remaining, deadline| {
        ripgrep_sync(root, query, remaining, deadline)
    })
}

fn search_roots(
    roots: Vec<PathBuf>,
    limit: usize,
    deadline: std::time::Instant,
    mut search: impl FnMut(&Path, usize, std::time::Instant) -> Result<(Vec<Value>, bool), ToolError>,
) -> ToolResult {
    let mut matches = Vec::new();
    let mut incomplete = false;
    for root in roots {
        if std::time::Instant::now() >= deadline {
            incomplete = true;
            break;
        }
        let (found, unfinished) = search(&root, limit - matches.len(), deadline)?;
        matches.extend(found);
        if unfinished {
            incomplete = true;
            break;
        }
        if matches.len() >= limit {
            break;
        }
    }
    Ok(json!({"matches": matches, "incomplete": incomplete}))
}

fn automation_list(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let _limit = limit_from_args(args);
    let store = engine
        .store()
        .ok_or_else(|| ToolError::internal("no store"))?;
    let automations = store
        .list_automations()
        .map_err(|e| ToolError::internal(e.to_string()))?;
    let items: Vec<_> = automations
        .into_iter()
        .filter(|a| ctx.project_allowed(Path::new(&a.project_path)))
        .map(|a| {
            json!({
                "id": a.id,
                "name": a.name,
                "enabled": a.enabled,
                "project_path": a.project_path,
            })
        })
        .collect();
    Ok(json!({ "automations": items }))
}

fn scene_list(engine: &Engine, _ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let limit = limit_from_args(args);
    let lib = engine.scenes();
    let scenes: Vec<_> = lib
        .scenes()
        .iter()
        .take(limit)
        .map(|entry| {
            json!({
                "reference": SceneLibrary::reference_for(entry),
                "name": entry.scene.name,
            })
        })
        .collect();
    Ok(json!({ "scenes": scenes }))
}

fn pipeline_list(engine: &Engine, _ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let limit = limit_from_args(args);
    let lib = engine.scenes();
    let pipelines: Vec<_> = lib
        .pipelines()
        .iter()
        .take(limit)
        .map(|entry| json!({ "name": entry.pipeline.name, "reference": SceneLibrary::pipeline_reference_for(entry) }))
        .collect();
    Ok(json!({ "pipelines": pipelines }))
}

fn subagent_list(engine: &Engine, ctx: &CallContext<'_>, args: &Value) -> ToolResult {
    let id = args
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("session_id required"))?;
    crate::external_mcp::ops_write::require_session(engine, ctx, id)?;
    let runs: Vec<_> = engine
        .subagent_runs(id)
        .into_iter()
        .take(LIST_LIMIT)
        .map(|run| {
            json!({
                "id": run.id, "parent_tool_call_id": run.parent_tool_call_id,
                "provider": run.provider, "origin": run.origin, "status": run.status,
                "model": run.model, "started_at": run.started_at, "completed_at": run.completed_at,
            })
        })
        .collect();
    Ok(json!({"subagents": runs}))
}

fn session_visible(ctx: &CallContext<'_>, session: &Session) -> bool {
    session_project_path(session)
        .map(|path| ctx.project_allowed(Path::new(&path)))
        .unwrap_or(false)
}

fn session_summary(session: &Session) -> Value {
    json!({
        "session_id": session.id,
        "title": session.title,
        "provider": session.provider,
        "project_path": session_project_path(session),
        "pinned": session.pinned,
    })
}

fn load_session(engine: &Engine, id: &str) -> Result<Session, ToolError> {
    if let Ok(sessions) = engine.list_sessions() {
        if let Some(session) = sessions.into_iter().find(|s| s.id == id) {
            return Ok(session);
        }
    }
    engine
        .store()
        .and_then(|store| store.get_session(id).ok().flatten())
        .ok_or_else(|| ToolError::not_found("session not found"))
}

fn project_path_arg(ctx: &CallContext<'_>, args: &Value) -> Result<String, ToolError> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid("path required"))?;
    if !ctx.project_allowed(Path::new(path)) {
        return Err(ToolError::not_found("project not found"));
    }
    Ok(path.to_string())
}

fn is_secret_path(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.starts_with(".env") {
        return true;
    }
    if name.ends_with(".pem")
        || name.ends_with(".key")
        || name.starts_with("id_")
        || matches!(
            name,
            ".npmrc" | ".netrc" | ".git-credentials" | "credentials"
        )
    {
        return true;
    }
    path.components().any(|part| {
        matches!(
            part.as_os_str().to_str(),
            Some(".git" | ".ssh" | ".aws" | ".gnupg")
        )
    })
}

fn in_registered_project(engine: &Engine, path: &Path) -> bool {
    engine
        .store()
        .and_then(|store| store.list_projects().ok())
        .is_some_and(|projects| {
            projects.iter().any(|project| {
                Path::new(&project.path)
                    .canonicalize()
                    .ok()
                    .is_some_and(|root| path.starts_with(root))
            })
        })
}

fn registered_project_path(
    engine: &Engine,
    ctx: &CallContext<'_>,
    args: &Value,
) -> Result<String, ToolError> {
    let path = PathBuf::from(project_path_arg(ctx, args)?)
        .canonicalize()
        .map_err(|_| ToolError::not_found("project not found"))?;
    if !in_registered_project(engine, &path) {
        return Err(ToolError::not_found("project not found"));
    }
    Ok(path.to_string_lossy().into_owned())
}

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        return tokio::task::block_in_place(|| handle.block_on(future));
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(future)
}

fn ripgrep_sync(
    root: &Path,
    query: &str,
    limit: usize,
    deadline: std::time::Instant,
) -> Result<(Vec<Value>, bool), ToolError> {
    use std::process::{Command, Stdio};
    // Only filenames leave the subprocess; both memory and runtime are bounded.
    let child = Command::new("rg")
        .arg("--no-config")
        .args(["--files-with-matches", "--null", "--max-count", "1"])
        .args([
            "--glob",
            "!.env*",
            "--glob",
            "!*.pem",
            "--glob",
            "!*.key",
            "--glob",
            "!id_*",
            "--glob",
            "!credentials",
            "--glob",
            "!.npmrc",
            "--glob",
            "!.netrc",
            "--glob",
            "!.git-credentials",
        ])
        .arg("-e")
        .arg(query)
        .arg("--")
        .arg(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ToolError::internal("workspace search requires ripgrep"))?;
    let (bytes, incomplete) = read_search_output(child, deadline)?;
    let matches = bytes
        .split(|byte| *byte == 0)
        .filter_map(|raw| std::str::from_utf8(raw).ok())
        .filter(|path| !path.is_empty() && !is_secret_path(Path::new(path)))
        .filter(|path| {
            Path::new(path)
                .canonicalize()
                .ok()
                .is_some_and(|path| path.starts_with(root))
        })
        .take(limit)
        .map(|path| json!({"path": path}))
        .collect();
    Ok((matches, incomplete))
}

fn read_search_output(
    mut child: std::process::Child,
    deadline: std::time::Instant,
) -> Result<(Vec<u8>, bool), ToolError> {
    use std::io::Read;
    use std::sync::mpsc::RecvTimeoutError;
    let stdout = child.stdout.take().expect("piped stdout");
    let (tx, rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(512 * 1024)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = tx.send(result);
    });
    let received = rx.recv_timeout(
        std::time::Duration::from_secs(5)
            .min(deadline.saturating_duration_since(std::time::Instant::now())),
    );
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();
    match received {
        Ok(Ok(bytes)) => {
            let incomplete = bytes.len() >= 512 * 1024;
            Ok((bytes, incomplete))
        }
        Err(RecvTimeoutError::Timeout) => Ok((Vec::new(), true)),
        _ => Err(ToolError::internal("workspace search failed")),
    }
}

pub fn track_request_from_event(engine: &Engine, event: &crate::event::Event) {
    use crate::event::Event;
    use crate::external_mcp::gate::resolve_session_project;
    match event {
        Event::PermissionRequest {
            session,
            request_id,
            title,
            context,
            ..
        } => {
            engine
                .external_mcp_state()
                .track_pending(PendingExternalRequest {
                    request_id: request_id.clone(),
                    session_id: session.clone(),
                    project_id: resolve_session_project(engine, session),
                    kind: PendingInputKind::Permission,
                    title: sanitize_text(title, DEFAULT_TEXT_MAX_CHARS),
                    tool_name: context.tool.clone(),
                    created_at: Utc::now(),
                });
        }
        Event::ElicitationRequest {
            session,
            request_id,
            form,
        } => {
            let title = form
                .questions()
                .next()
                .and_then(|q| q.description.as_deref().or(q.title.as_deref()))
                .map(|t| sanitize_text(t, DEFAULT_TEXT_MAX_CHARS))
                .unwrap_or_else(|| sanitize_text(&form.message, DEFAULT_TEXT_MAX_CHARS));
            engine
                .external_mcp_state()
                .track_pending(PendingExternalRequest {
                    request_id: request_id.clone(),
                    session_id: session.clone(),
                    project_id: resolve_session_project(engine, session),
                    kind: PendingInputKind::Elicitation,
                    title,
                    tool_name: None,
                    created_at: Utc::now(),
                });
        }
        _ => {}
    }
}

#[cfg(test)]
mod search_budget_tests {
    use super::*;

    #[test]
    fn a_timed_out_root_preserves_prior_matches_and_stops_the_scan() {
        let roots = vec![
            PathBuf::from("first"),
            PathBuf::from("slow"),
            PathBuf::from("unused"),
        ];
        let mut visits = 0;
        let result = search_roots(
            roots,
            50,
            std::time::Instant::now() + std::time::Duration::from_secs(10),
            |root, _, _| {
                visits += 1;
                if root == Path::new("first") {
                    Ok((vec![json!({"path":"first/result"})], false))
                } else {
                    Ok((Vec::new(), true))
                }
            },
        )
        .unwrap();
        assert_eq!(visits, 2);
        assert_eq!(result["incomplete"], true);
        assert_eq!(result["matches"], json!([{"path":"first/result"}]));
    }

    #[test]
    fn an_expired_budget_starts_no_more_subprocesses() {
        let result = search_roots(
            vec![PathBuf::from("unused")],
            50,
            std::time::Instant::now(),
            |_, _, _| panic!("budget exhausted"),
        )
        .unwrap();
        assert_eq!(result["incomplete"], true);
    }

    #[cfg(unix)]
    #[test]
    fn search_timeout_kills_and_reaps_the_owned_process() {
        let child = std::process::Command::new("sh")
            .args(["-c", "exec sleep 5"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        let (bytes, incomplete) =
            read_search_output(child, started + std::time::Duration::from_millis(20)).unwrap();
        assert!(incomplete && bytes.is_empty());
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }
}

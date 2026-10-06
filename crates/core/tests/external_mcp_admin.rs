//! External MCP admin (destructive) tool tests.

use chrono::{Duration as ChronoDuration, Utc};
use codetwo_core::external_mcp::catalog::external_tool_catalog;
use codetwo_core::external_mcp::clients::{
    ExternalClientRecord, ExternalScope, ProjectScope, ResolvedClient,
};
use codetwo_core::external_mcp::ctx::{CallContext, ToolErrorKind};
use codetwo_core::external_mcp::ops_admin;
use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
use codetwo_core::session::Session;
use codetwo_core::skill::SkillLibrary;
use codetwo_core::{Engine, Store};
use serde_json::json;
use std::sync::Arc;

fn spec(name: &str) -> &'static codetwo_core::external_mcp::catalog::ToolSpec {
    external_tool_catalog()
        .iter()
        .find(|entry| entry.name == name)
        .unwrap_or_else(|| panic!("missing tool {name}"))
}

fn admin_client(projects: ProjectScope) -> ResolvedClient {
    ResolvedClient {
        record: ExternalClientRecord {
            id: "admin-client".into(),
            name: "admin".into(),
            scopes: vec![ExternalScope::Admin],
            projects,
            created_at: Utc::now(),
            expires_at: Some(Utc::now() + ChronoDuration::days(30)),
            last_used_at: None,
            revoked_at: None,
        },
    }
}

fn engine(store: Arc<Store>) -> Engine {
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
        store,
    );
    engine
}

#[test]
fn project_delete_unregisters_without_removing_files() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("keep.txt"), b"stay").unwrap();
    let canonical = project.canonicalize().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    store
        .add_project(
            &canonical.to_string_lossy(),
            None,
            codetwo_core::session::now_millis(),
        )
        .unwrap();
    let engine = engine(store);
    let client = admin_client(ProjectScope::All);
    let ctx = CallContext {
        client: &client,
        spec: spec("codetwo_project_delete"),
    };
    ops_admin::call(
        &engine,
        &ctx,
        "codetwo_project_delete",
        &json!({
            "path": canonical.to_string_lossy(),
            "confirm": true,
            "expected_id": canonical.to_string_lossy()
        }),
    )
    .unwrap()
    .unwrap();
    assert!(canonical.join("keep.txt").is_file());
}

#[test]
fn session_delete_supports_idle_transient_sessions() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().canonicalize().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let mut transient = Session::new(ProviderId::ClaudeCode, project.to_string_lossy().to_string());
    transient.transient = true;
    store.upsert_session(&transient).unwrap();
    let engine = engine(store.clone());
    let client = admin_client(ProjectScope::All);
    let ctx = CallContext {
        client: &client,
        spec: spec("codetwo_session_delete"),
    };
    ops_admin::call(
        &engine,
        &ctx,
        "codetwo_session_delete",
        &json!({
            "session_id": transient.id,
            "confirm": true,
            "expected_id": transient.id
        }),
    )
    .unwrap()
    .unwrap();
    assert!(store.get_session(&transient.id).unwrap().is_none());
}

#[test]
fn automation_crud_respects_project_confinement() {
    let root = tempfile::tempdir().unwrap();
    let allowed = root.path().join("allowed");
    std::fs::create_dir_all(&allowed).unwrap();
    let allowed = allowed.canonicalize().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    store
        .add_project(
            &allowed.to_string_lossy(),
            None,
            codetwo_core::session::now_millis(),
        )
        .unwrap();
    let engine = engine(store.clone());
    let client = admin_client(ProjectScope::Paths(vec![allowed.clone()]));
    let ctx = CallContext {
        client: &client,
        spec: spec("codetwo_automation_create"),
    };
    let created = ops_admin::call(
        &engine,
        &ctx,
        "codetwo_automation_create",
        &json!({
            "name": "nightly",
            "prompt": "check status",
            "project_path": allowed.to_string_lossy(),
            "provider": "claude_code",
            "cron": "0 9 * * *",
            "timezone": "UTC",
            "confirm": true,
            "expected_id": "new"
        }),
    )
    .unwrap()
    .unwrap();
    let id = created["id"].as_str().unwrap();
    let delete_ctx = CallContext {
        client: &client,
        spec: spec("codetwo_automation_delete"),
    };
    ops_admin::call(
        &engine,
        &delete_ctx,
        "codetwo_automation_delete",
        &json!({ "id": id, "confirm": true, "expected_id": id }),
    )
    .unwrap()
    .unwrap();
    assert!(store.automation(id).unwrap().is_none());
}

#[test]
fn durable_session_delete_removes_records_and_keeps_workspace() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().canonicalize().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let session = Session::new(ProviderId::ClaudeCode, project.to_string_lossy().to_string());
    store.upsert_session(&session).unwrap();
    let engine = engine(store.clone());
    let client = admin_client(ProjectScope::All);
    let ctx = CallContext {
        client: &client,
        spec: spec("codetwo_session_delete"),
    };
    let result = ops_admin::call(
        &engine,
        &ctx,
        "codetwo_session_delete",
        &json!({
            "session_id": session.id,
            "confirm": true,
            "expected_id": session.id
        }),
    )
    .unwrap()
    .unwrap();
    assert_eq!(result["deleted"], true);
    assert!(store.get_session(&session.id).unwrap().is_none());
    assert!(project.is_dir());
}

#[test]
fn scene_and_pipeline_run_require_scoped_targets() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let engine = engine(store);
    let client = admin_client(ProjectScope::All);
    for tool in ["codetwo_scene_run", "codetwo_pipeline_run"] {
        let ctx = CallContext {
            client: &client,
            spec: spec(tool),
        };
        let err = ops_admin::call(
            &engine,
            &ctx,
            tool,
            &json!({ "id": "x", "confirm": true, "expected_id": "x" }),
        )
        .unwrap()
        .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidParams);
    }
}

// ---------------------------------------------------------------------------
// Regression tests over a real booted CoreApp (scene bridge) and an on-disk Store.
// ---------------------------------------------------------------------------

use codetwo_core::external_mcp::ctx::ToolError;
use codetwo_core::permission::{PermissionMode, SandboxPolicy};
use codetwo_core::plugins::{AppConfig, CoreApp, EngineService};
use codetwo_core::session::{SessionActivity, SessionRunState};
use serde_json::Value;

struct Booted {
    app: CoreApp,
    engine: Arc<Engine>,
    store: Arc<Store>,
    project: std::path::PathBuf,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

async fn boot() -> Booted {
    let data = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let project = project_dir.path().canonicalize().unwrap();
    let app = CoreApp::boot(AppConfig::new(data.path())).await.expect("boot");
    let engine = app.service::<EngineService>().expect("engine plugin").0.clone();
    let store = engine.store().expect("durable store");
    store
        .add_project(&project.to_string_lossy(), None, codetwo_core::session::now_millis())
        .unwrap();
    Booted { app, engine, store, project, _dirs: (data, project_dir) }
}

fn session_with(project: &std::path::Path, mode: PermissionMode, sandbox: SandboxPolicy) -> Session {
    let mut session = Session::new(ProviderId::ClaudeCode, project.to_string_lossy().to_string());
    session.permission_mode = mode;
    session.sandbox_policy = sandbox;
    session
}

fn running() -> SessionActivity {
    SessionActivity {
        revision: 1,
        state: SessionRunState::Running { turn_id: "turn".into(), prompt_request_id: None },
    }
}

/// Run one admin tool on a blocking thread, the way the HTTP dispatcher does.
async fn admin(
    engine: &Arc<Engine>,
    tool: &'static str,
    args: serde_json::Value,
) -> Result<Value, ToolError> {
    let engine = engine.clone();
    tokio::task::spawn_blocking(move || {
        let client = admin_client(ProjectScope::All);
        let ctx = CallContext { client: &client, spec: spec(tool) };
        ops_admin::call(&engine, &ctx, tool, &args).unwrap()
    })
    .await
    .unwrap()
}

fn policy(store: &Store, id: &str) -> (PermissionMode, SandboxPolicy) {
    let session = store.get_session(id).unwrap().unwrap();
    (session.permission_mode, session.sandbox_policy)
}

#[tokio::test(flavor = "multi_thread")]
async fn scene_run_cannot_loosen_one_axis_while_the_combined_mode_tightens() {
    let boot = boot().await;
    // (Ask, DangerFullAccess) ranks as FullAccess, so builtin:fix (AcceptEdits, WorkspaceWrite)
    // looks like a tightening to the combined-mode check but loosens `mode`.
    let session = session_with(&boot.project, PermissionMode::Ask, SandboxPolicy::DangerFullAccess);
    boot.store.upsert_session(&session).unwrap();
    let error = admin(&boot.engine, "codetwo_scene_run", json!({
        "id": "builtin:fix", "session_id": session.id, "confirm": true, "expected_id": "builtin:fix",
    })).await.unwrap_err();
    assert_eq!(error.kind, ToolErrorKind::Denied);
    assert_eq!(policy(&boot.store, &session.id), (PermissionMode::Ask, SandboxPolicy::DangerFullAccess));
    assert!(boot.store.session_scene(&session.id).unwrap().is_none());
    boot.app.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn scene_run_applies_a_tightening_scene_through_the_real_bridge() {
    let boot = boot().await;
    let session = session_with(&boot.project, PermissionMode::Ask, SandboxPolicy::WorkspaceWrite);
    boot.store.upsert_session(&session).unwrap();
    admin(&boot.engine, "codetwo_scene_run", json!({
        "id": "builtin:research", "session_id": session.id, "confirm": true, "expected_id": "builtin:research",
    })).await.unwrap();
    assert_eq!(policy(&boot.store, &session.id), (PermissionMode::Ask, SandboxPolicy::ReadOnly));
    assert_eq!(boot.store.session_scene(&session.id).unwrap().unwrap().0, "builtin:research");
    // The reservation is released afterwards: the turn slot can be claimed again.
    drop(boot.engine.reserve_idle_session(&session.id).unwrap());
    boot.app.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn scene_run_is_busy_for_persisted_running_and_for_a_held_reservation() {
    let boot = boot().await;
    let args = |id: &str| json!({
        "id": "builtin:research", "session_id": id, "confirm": true, "expected_id": "builtin:research",
    });
    // Durable Running snapshot the tracker has never seen.
    let mut persisted = session_with(&boot.project, PermissionMode::Ask, SandboxPolicy::WorkspaceWrite);
    persisted.activity = running();
    boot.store.upsert_session(&persisted).unwrap();
    let error = admin(&boot.engine, "codetwo_scene_run", args(&persisted.id)).await.unwrap_err();
    assert_eq!(error.kind, ToolErrorKind::Conflict);
    assert_eq!(policy(&boot.store, &persisted.id), (PermissionMode::Ask, SandboxPolicy::WorkspaceWrite));

    // Another writer holds the slot (a prompt about to start): the scene must not interleave.
    let idle = session_with(&boot.project, PermissionMode::Ask, SandboxPolicy::WorkspaceWrite);
    boot.store.upsert_session(&idle).unwrap();
    let slot = boot.engine.reserve_idle_session(&idle.id).unwrap();
    let error = admin(&boot.engine, "codetwo_scene_run", args(&idle.id)).await.unwrap_err();
    assert_eq!(error.kind, ToolErrorKind::Conflict);
    drop(slot);
    admin(&boot.engine, "codetwo_scene_run", args(&idle.id)).await.unwrap();
    boot.app.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_scene_and_pipeline_name_the_missing_target() {
    let boot = boot().await;
    let session = session_with(&boot.project, PermissionMode::Ask, SandboxPolicy::WorkspaceWrite);
    boot.store.upsert_session(&session).unwrap();
    let scene = admin(&boot.engine, "codetwo_scene_run", json!({
        "id": "builtin:nope", "session_id": session.id, "confirm": true, "expected_id": "builtin:nope",
    })).await.unwrap_err();
    assert_eq!((scene.kind, scene.message.as_str()), (ToolErrorKind::NotFound, "scene not found"));
    let pipeline = admin(&boot.engine, "codetwo_pipeline_run", json!({
        "id": "builtin:nope", "project_path": boot.project, "confirm": true, "expected_id": "builtin:nope",
    })).await.unwrap_err();
    assert_eq!((pipeline.kind, pipeline.message.as_str()), (ToolErrorKind::NotFound, "pipeline not found"));
    boot.app.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pipeline_run_requires_every_stage_to_fit_the_ceiling() {
    let boot = boot().await;
    let args = |session: Option<&str>| json!({
        "id": "builtin:rnd-lifecycle", "project_path": boot.project, "session_id": session,
        "confirm": true, "expected_id": "builtin:rnd-lifecycle",
    });
    // Entry (research) is read-only, but later stages are auto_edit: not bounded by the default
    // ceiling, so a session-less run (which may later spawn sessions) is denied before anything starts.
    let error = admin(&boot.engine, "codetwo_pipeline_run", args(None)).await.unwrap_err();
    assert_eq!(error.kind, ToolErrorKind::Denied);
    assert!(boot.store.list_pipeline_instances(&boot.project.to_string_lossy()).unwrap().is_empty());

    // Bound to a session whose mode axis is already tighter than a later stage: denied.
    let tight = session_with(&boot.project, PermissionMode::Ask, SandboxPolicy::DangerFullAccess);
    boot.store.upsert_session(&tight).unwrap();
    let error = admin(&boot.engine, "codetwo_pipeline_run", args(Some(&tight.id))).await.unwrap_err();
    assert_eq!(error.kind, ToolErrorKind::Denied);
    assert!(boot.store.list_pipeline_instances(&boot.project.to_string_lossy()).unwrap().is_empty());
    assert_eq!(policy(&boot.store, &tight.id), (PermissionMode::Ask, SandboxPolicy::DangerFullAccess));

    // Every stage fits under a session with headroom on both axes: runs and binds the entry scene.
    let roomy = session_with(&boot.project, PermissionMode::Yolo, SandboxPolicy::DangerFullAccess);
    boot.store.upsert_session(&roomy).unwrap();
    admin(&boot.engine, "codetwo_pipeline_run", args(Some(&roomy.id))).await.unwrap();
    assert_eq!(boot.store.list_pipeline_instances(&boot.project.to_string_lossy()).unwrap().len(), 1);
    assert_eq!(policy(&boot.store, &roomy.id), (PermissionMode::Ask, SandboxPolicy::ReadOnly));
    boot.app.stop().await;
}

// --- session_delete ---------------------------------------------------------

fn on_disk_engine() -> (tempfile::TempDir, String, Arc<Store>, Engine, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("c2.db").to_string_lossy().to_string();
    let store = Arc::new(Store::open(&db).unwrap());
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let project = project.canonicalize().unwrap();
    let engine = engine(store.clone());
    (dir, db, store, engine, project)
}

fn delete(engine: &Engine, id: &str) -> Result<serde_json::Value, ToolError> {
    let client = admin_client(ProjectScope::All);
    let ctx = CallContext { client: &client, spec: spec("codetwo_session_delete") };
    ops_admin::call(engine, &ctx, "codetwo_session_delete", &json!({
        "session_id": id, "confirm": true, "expected_id": id,
    })).unwrap()
}

#[test]
fn session_delete_blocks_stale_upsert_and_refuses_persisted_running() {
    let (_dir, _db, store, engine, project) = on_disk_engine();
    let idle = Session::new(ProviderId::ClaudeCode, project.to_string_lossy().to_string());
    store.upsert_session(&idle).unwrap();
    delete(&engine, &idle.id).unwrap();
    assert!(store.get_session(&idle.id).unwrap().is_none());
    // A late runtime/callback write with the old snapshot must not resurrect the row.
    assert!(store.upsert_session(&idle).is_err());
    assert!(store.append_part(&idle.id, codetwo_core::session::Role::Agent,
        &codetwo_core::session::Part::Text { text: "late callback".into() }).is_err());
    assert!(store.get_session(&idle.id).unwrap().is_none());
    assert!(store.list_sessions().unwrap().is_empty());

    let mut busy = Session::new(ProviderId::ClaudeCode, project.to_string_lossy().to_string());
    busy.activity = running();
    store.upsert_session(&busy).unwrap();
    let error = delete(&engine, &busy.id).unwrap_err();
    assert_eq!(error.kind, ToolErrorKind::Conflict);
    assert!(store.get_session(&busy.id).unwrap().is_some());
}

#[test]
fn session_delete_removes_owned_rows_and_keeps_shared_history() {
    let (_dir, db, store, engine, project) = on_disk_engine();
    let path = project.to_string_lossy().to_string();
    let doomed = Session::new(ProviderId::ClaudeCode, path.clone());
    let other = Session::new(ProviderId::ClaudeCode, path.clone());
    store.upsert_session(&doomed).unwrap();
    store.upsert_session(&other).unwrap();
    let raw = rusqlite::Connection::open(&db).unwrap();
    // Seed rows without their parents (automation, task); deletion itself runs with FKs enforced.
    raw.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    for id in [&doomed.id, &other.id] {
        raw.execute("INSERT INTO memory_turns(session_id,user_part_seq,project_path,provenance_json,capture_status,created_at) VALUES(?1,1,?2,'{}','captured',0)", rusqlite::params![id, path]).unwrap();
        raw.execute("INSERT INTO memory_candidates(id,project_path,session_id,user_part_seq,category,content,confidence,sources_json,created_at,eligible_at) VALUES(?3,?2,?1,1,'fact','c',0.9,'[]',0,0)", rusqlite::params![id, path, format!("cand-{id}")]).unwrap();
        raw.execute("INSERT INTO assistant_worker_receipts(session_id,command_id,operation,response) VALUES(?1,'cmd','op','{}')", [id]).unwrap();
        raw.execute("INSERT INTO prompt_deliveries(id,session_id,mode,doc,state) VALUES(?2,?1,'prompt','[]','submitted')", rusqlite::params![id, format!("delivery-{id}")]).unwrap();
        raw.execute("INSERT INTO automation_runs(id,automation_id,session_id,status,scheduled_for,started_at) VALUES(?2,'auto',?1,'finished',0,0)", rusqlite::params![id, format!("run-{id}")]).unwrap();
    }
    // Task ownership (active lease) and task history (released lease) both block deletion.
    let leased = Session::new(ProviderId::ClaudeCode, path.clone());
    store.upsert_session(&leased).unwrap();
    raw.execute("INSERT INTO task_session_leases_v2(task_id,session_id,agent_id,role,compatibility_identity,leased_at_ms,released_at_ms) VALUES('task',?1,'agent','executor','compat',0,NULL)", [&leased.id]).unwrap();
    for released in [None, Some(1)] {
        raw.execute("UPDATE task_session_leases_v2 SET released_at_ms=?2 WHERE session_id=?1", rusqlite::params![&leased.id, released]).unwrap();
        let error = delete(&engine, &leased.id).unwrap_err();
        assert_eq!(error.kind, ToolErrorKind::Conflict);
        assert!(store.get_session(&leased.id).unwrap().is_some());
    }

    delete(&engine, &doomed.id).unwrap();
    let count = |table: &str, id: &str| -> i64 {
        raw.query_row(&format!("SELECT COUNT(*) FROM {table} WHERE session_id=?1"), [id], |row| row.get(0)).unwrap()
    };
    for table in ["memory_turns", "memory_candidates", "assistant_worker_receipts", "prompt_deliveries"] {
        assert_eq!(count(table, &doomed.id), 0, "{table} should be removed");
        assert_eq!(count(table, &other.id), 1, "{table} of another session must stay");
    }
    // Run history stays with its automation, minus the dangling session reference.
    let runs: i64 = raw.query_row("SELECT COUNT(*) FROM automation_runs WHERE id=?1 AND session_id IS NULL", [format!("run-{}", doomed.id)], |row| row.get(0)).unwrap();
    assert_eq!(runs, 1);
    assert!(store.get_session(&other.id).unwrap().is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn session_create_rejects_unregistered_provider_and_bad_title_before_creating() {
    let boot = boot().await;
    let tool = "codetwo_session_create";
    let base = |provider: &str, title: &str| json!({
        "project_path": boot.project, "provider": provider, "title": title,
    });
    let engine = boot.engine.clone();
    let write = move |args: serde_json::Value| {
        let engine = engine.clone();
        async move {
            tokio::task::spawn_blocking(move || {
                let client = admin_client(ProjectScope::All);
                let ctx = CallContext { client: &client, spec: spec(tool) };
                codetwo_core::external_mcp::ops_write::call(&engine, &ctx, tool, &args).unwrap()
            })
            .await
            .unwrap()
        }
    };
    let started = std::time::Instant::now();
    let error = write(base("not_registered", "ok")).await.unwrap_err();
    assert_eq!(error.kind, ToolErrorKind::InvalidParams);
    let error = write(base("claude_code", "bad\u{7}title")).await.unwrap_err();
    assert_eq!(error.kind, ToolErrorKind::InvalidParams);
    assert!(started.elapsed() < std::time::Duration::from_secs(5), "must fail fast, not wait on a provider");
    assert!(boot.store.list_sessions().unwrap().is_empty());
    boot.app.stop().await;
}

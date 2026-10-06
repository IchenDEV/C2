//! External MCP write-tool tests (direct `ops_write::call`).

use chrono::{Duration as ChronoDuration, Utc};
use codetwo_core::external_mcp::catalog::external_tool_catalog;
use codetwo_core::external_mcp::clients::{
    ExternalClientRecord, ExternalScope, ProjectScope, ResolvedClient,
};
use codetwo_core::external_mcp::ctx::{CallContext, ToolErrorKind};
use codetwo_core::external_mcp::ops_write;
use codetwo_core::permission::{PermissionMode, SandboxPolicy};
use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
use codetwo_core::permission::PermissionContext;
use codetwo_core::session::{
    PendingInput, PendingInputKind, Session, SessionActivity, SessionRunState,
};
use codetwo_core::skill::SkillLibrary;
use codetwo_core::{Engine, Store};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;

fn spec(name: &str) -> &'static codetwo_core::external_mcp::catalog::ToolSpec {
    external_tool_catalog()
        .iter()
        .find(|entry| entry.name == name)
        .unwrap_or_else(|| panic!("missing tool {name}"))
}

fn engine_with_store(store: Arc<Store>) -> Engine {
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

fn client(projects: ProjectScope) -> ResolvedClient {
    ResolvedClient {
        record: ExternalClientRecord {
            id: "test-client".into(),
            name: "test".into(),
            scopes: vec![
                ExternalScope::Read,
                ExternalScope::Operate,
                ExternalScope::Approve,
            ],
            projects,
            created_at: Utc::now(),
            expires_at: Some(Utc::now() + ChronoDuration::days(30)),
            last_used_at: None,
            revoked_at: None,
        },
    }
}

#[test]
fn project_create_registers_allowed_directory() {
    let root = tempfile::tempdir().unwrap();
    let allowed = root.path().join("allowed");
    std::fs::create_dir_all(&allowed).unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let engine = engine_with_store(store);
    let client = client(ProjectScope::Paths(vec![allowed.clone()]));
    let ctx = CallContext {
        client: &client,
        spec: spec("codetwo_project_create"),
    };
    let result = ops_write::call(
        &engine,
        &ctx,
        "codetwo_project_create",
        &json!({ "path": allowed.to_string_lossy() }),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        result["path"].as_str().unwrap(),
        allowed.canonicalize().unwrap().to_string_lossy()
    );
}

#[test]
fn project_create_denies_out_of_scope_path() {
    let root = tempfile::tempdir().unwrap();
    let allowed = root.path().join("allowed");
    let outside = root.path().join("outside");
    std::fs::create_dir_all(&allowed).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let engine = engine_with_store(store);
    let client = client(ProjectScope::Paths(vec![allowed]));
    let ctx = CallContext {
        client: &client,
        spec: spec("codetwo_project_create"),
    };
    let err = ops_write::call(
        &engine,
        &ctx,
        "codetwo_project_create",
        &json!({ "path": outside.to_string_lossy() }),
    )
    .unwrap()
    .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
}

#[test]
fn project_update_rejects_invalid_args() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let engine = engine_with_store(store);
    let client = client(ProjectScope::All);
    let ctx = CallContext {
        client: &client,
        spec: spec("codetwo_project_update"),
    };
    let err = ops_write::call(
        &engine,
        &ctx,
        "codetwo_project_update",
        &json!({ "path": 1 }),
    )
    .unwrap()
    .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::InvalidParams);
}

#[test]
fn policy_set_refuses_loosening() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().canonicalize().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    store
        .add_project(
            &project.to_string_lossy(),
            None,
            codetwo_core::session::now_millis(),
        )
        .unwrap();
    let session = Session::new(ProviderId::ClaudeCode, project.to_string_lossy().to_string());
    let mut session = session;
    session.permission_mode = PermissionMode::Ask;
    session.sandbox_policy = SandboxPolicy::ReadOnly;
    store.upsert_session(&session).unwrap();
    let engine = engine_with_store(store);
    let client = client(ProjectScope::All);
    let ctx = CallContext {
        client: &client,
        spec: spec("codetwo_policy_set"),
    };
    let err = ops_write::call(
        &engine,
        &ctx,
        "codetwo_policy_set",
        &json!({
            "session_id": session.id,
            "mode": "yolo",
            "sandbox_policy": "danger_full_access"
        }),
    )
    .unwrap()
    .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Denied);
    assert!(err.message.contains("loosening"));
}

#[test]
fn approval_respond_is_stale_without_pending_input() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().canonicalize().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let session = Session::new(ProviderId::ClaudeCode, project.to_string_lossy().to_string());
    store.upsert_session(&session).unwrap();
    let engine = engine_with_store(store);
    let client = client(ProjectScope::All);
    let ctx = CallContext {
        client: &client,
        spec: spec("codetwo_approval_respond"),
    };
    let err = ops_write::call(
        &engine,
        &ctx,
        "codetwo_approval_respond",
        &json!({ "request_id": "missing-request", "allow": true }),
    )
    .unwrap()
    .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
}

#[test]
fn project_update_renames_registered_project() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("proj");
    std::fs::create_dir_all(&project).unwrap();
    let canonical = project.canonicalize().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    store
        .add_project(
            &canonical.to_string_lossy(),
            None,
            codetwo_core::session::now_millis(),
        )
        .unwrap();
    let engine = engine_with_store(store.clone());
    let client = client(ProjectScope::All);
    let ctx = CallContext {
        client: &client,
        spec: spec("codetwo_project_update"),
    };
    ops_write::call(
        &engine,
        &ctx,
        "codetwo_project_update",
        &json!({ "path": canonical.to_string_lossy(), "name": "Renamed" }),
    )
    .unwrap()
    .unwrap();
    let store = engine.store().unwrap();
    let name = store
        .list_projects()
        .unwrap()
        .into_iter()
        .find(|p| p.path == canonical.to_string_lossy())
        .unwrap()
        .name;
    assert_eq!(name, "Renamed");
}

#[test]
fn session_update_renames_in_scope_session() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().canonicalize().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let session = Session::new(ProviderId::ClaudeCode, project.to_string_lossy().to_string());
    store.upsert_session(&session).unwrap();
    let engine = engine_with_store(store);
    let client = client(ProjectScope::All);
    let ctx = CallContext {
        client: &client,
        spec: spec("codetwo_session_update"),
    };
    ops_write::call(
        &engine,
        &ctx,
        "codetwo_session_update",
        &json!({ "session_id": session.id, "title": "External title" }),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        engine
            .list_sessions()
            .unwrap()
            .into_iter()
            .find(|s| s.id == session.id)
            .unwrap()
            .title,
        "External title"
    );
}

#[test]
fn turn_send_rejects_invalid_mode() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().canonicalize().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let session = Session::new(ProviderId::ClaudeCode, project.to_string_lossy().to_string());
    store.upsert_session(&session).unwrap();
    let engine = engine_with_store(store);
    let client = client(ProjectScope::All);
    let ctx = CallContext {
        client: &client,
        spec: spec("codetwo_turn_send"),
    };
    let err = ops_write::call(
        &engine,
        &ctx,
        "codetwo_turn_send",
        &json!({ "session_id": session.id, "mode": "invalid", "text": "hi" }),
    )
    .unwrap()
    .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::InvalidParams);
}

#[test]
fn approval_respond_denies_out_of_scope_pending() {
    let root = tempfile::tempdir().unwrap();
    let allowed = root.path().join("allowed");
    let secret = root.path().join("secret");
    std::fs::create_dir_all(&allowed).unwrap();
    std::fs::create_dir_all(&secret).unwrap();
    let secret = secret.canonicalize().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let mut session = Session::new(ProviderId::ClaudeCode, secret.to_string_lossy().to_string());
    session.activity = SessionActivity {
        revision: 1,
        state: SessionRunState::AwaitingInput {
            turn_id: "turn-1".into(),
            prompt_request_id: None,
            pending: vec![PendingInput {
                input_id: "perm-1".into(),
                kind: PendingInputKind::Permission,
                title: "Run".into(),
                options: vec![("allow".into(), "Allow".into())],
                option_kinds: BTreeMap::from([("allow".into(), "allow_once".into())]),
                sequence: 1,
                context: PermissionContext {
                    tool: Some("bash".into()),
                    ..Default::default()
                },
                form: None,
            }],
        },
    };
    store.upsert_session(&session).unwrap();
    let engine = engine_with_store(store);
    let client = client(ProjectScope::Paths(vec![allowed.canonicalize().unwrap()]));
    let ctx = CallContext {
        client: &client,
        spec: spec("codetwo_approval_respond"),
    };
    let err = ops_write::call(
        &engine,
        &ctx,
        "codetwo_approval_respond",
        &json!({ "request_id": "perm-1", "allow": true }),
    )
    .unwrap()
    .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
    assert!(!err.message.contains("bash"));
}

#[test]
fn errors_do_not_echo_secrets() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let engine = engine_with_store(store);
    let client = client(ProjectScope::All);
    let ctx = CallContext {
        client: &client,
        spec: spec("codetwo_turn_send"),
    };
    let secret = "ctmcp_supersecret_token_value";
    let err = ops_write::call(
        &engine,
        &ctx,
        "codetwo_turn_send",
        &json!({ "session_id": "nope", "mode": "prompt", "text": secret }),
    )
    .unwrap()
    .unwrap_err();
    assert!(!err.message.contains(secret));
    assert!(!err.message.contains("ctmcp_"));
}

#[test]
fn policy_set_rejects_each_dimension_even_when_combined_mode_is_unchanged() {
    for (sandbox, next) in [(SandboxPolicy::ReadOnly, "read_only"), (SandboxPolicy::DangerFullAccess, "danger_full_access")] {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open_in_memory().unwrap());
        let mut session = Session::new(ProviderId::ClaudeCode, root.path().to_string_lossy().into_owned());
        session.permission_mode = PermissionMode::Ask;
        session.sandbox_policy = sandbox;
        store.upsert_session(&session).unwrap();
        let engine = engine_with_store(store.clone());
        let client = client(ProjectScope::All);
        let ctx = CallContext { client: &client, spec: spec("codetwo_policy_set") };
        let err = ops_write::call(&engine, &ctx, "codetwo_policy_set", &json!({
            "session_id": session.id, "mode": "yolo", "sandbox_policy": next,
        })).unwrap().unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Denied);
        assert_eq!(store.get_session(&session.id).unwrap().unwrap().permission_mode, PermissionMode::Ask);
    }
}

#[test]
fn session_creation_cannot_bypass_default_execution_policy() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().canonicalize().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    store.add_project(&path.to_string_lossy(), None, codetwo_core::session::now_millis()).unwrap();
    let engine = engine_with_store(store.clone());
    let client = client(ProjectScope::All);
    let ctx = CallContext { client: &client, spec: spec("codetwo_session_create") };
    let err = ops_write::call(&engine, &ctx, "codetwo_session_create", &json!({
        "project_path": path, "permission_mode": "yolo", "sandbox_policy": "danger_full_access",
    })).unwrap().unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Denied);
    assert!(store.list_sessions().unwrap().is_empty());
}

// --- engine rejection must not look like success -----------------------------------------------

/// An on-disk engine plus a way to make the engine's own session writes fail afterwards.
fn on_disk() -> (tempfile::TempDir, String, Arc<Store>, Engine, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("c2.db").to_string_lossy().into_owned();
    let store = Arc::new(Store::open(&db).unwrap());
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let engine = engine_with_store(store.clone());
    (dir, db, store, engine, project.canonicalize().unwrap())
}

fn block_session_updates(db: &str) {
    rusqlite::Connection::open(db)
        .unwrap()
        .execute_batch("CREATE TRIGGER block_updates BEFORE UPDATE ON sessions BEGIN SELECT RAISE(ABORT,'blocked'); END;")
        .unwrap();
}

fn write(engine: &Engine, tool: &str, args: serde_json::Value) -> Result<serde_json::Value, codetwo_core::external_mcp::ctx::ToolError> {
    let client = client(ProjectScope::All);
    let ctx = CallContext { client: &client, spec: spec(tool) };
    ops_write::call(engine, &ctx, tool, &args).unwrap()
}

#[test]
fn policy_and_model_set_report_the_persisted_result_and_conflict_when_the_engine_rejected() {
    let (_dir, db, store, engine, project) = on_disk();
    let mut session = Session::new(ProviderId::ClaudeCode, project.to_string_lossy().into_owned());
    session.sandbox_policy = SandboxPolicy::DangerFullAccess;
    store.upsert_session(&session).unwrap();
    let id = session.id.clone();

    let applied = write(&engine, "codetwo_policy_set", json!({"session_id": id, "sandbox_policy": "workspace_write"})).unwrap();
    assert_eq!(applied["sandbox_policy"], "workspace_write");
    assert_eq!(store.get_session(&id).unwrap().unwrap().sandbox_policy, SandboxPolicy::WorkspaceWrite);
    let applied = write(&engine, "codetwo_model_set", json!({"session_id": id, "model_id": "m1"})).unwrap();
    assert_eq!(applied["model_id"], "m1");
    assert_eq!(store.get_session(&id).unwrap().unwrap().model.as_deref(), Some("m1"));

    // `submit` still returns Ok when the engine can't persist: the tool must not claim success.
    block_session_updates(&db);
    let err = write(&engine, "codetwo_policy_set", json!({"session_id": id, "sandbox_policy": "read_only"})).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    let err = write(&engine, "codetwo_model_set", json!({"session_id": id, "model_id": "m2"})).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    let stored = store.get_session(&id).unwrap().unwrap();
    assert_eq!((stored.sandbox_policy, stored.model.as_deref()), (SandboxPolicy::WorkspaceWrite, Some("m1")));
}

#[test]
fn turn_send_returns_an_honest_non_final_receipt() {
    let (_dir, _db, store, engine, project) = on_disk();

    // Hold the session's turn slot so the engine refuses to start a turn for it.
    let held = Session::new(ProviderId::ClaudeCode, project.to_string_lossy().into_owned());
    store.upsert_session(&held).unwrap();
    let _slot = engine.reserve_idle_session(&held.id).unwrap();

    // A queued prompt is a durable, asynchronous receipt: never accepted or final on return.
    let queued = write(&engine, "codetwo_turn_send", json!({"session_id": held.id, "mode": "queue", "text": "later"})).unwrap();
    assert!(matches!(queued["state"].as_str(), Some("queued" | "submitting")), "{queued}");
    assert_eq!(queued["final"], false);
    assert!(queued["turn_id"].is_null());
    assert!(store.prompt_delivery(queued["delivery_id"].as_str().unwrap()).unwrap().is_some());

    // A direct prompt the engine refuses (the slot is taken) is only "submitted": never
    // "accepted", and it carries no turn id.
    let prompted = write(&engine, "codetwo_turn_send", json!({"session_id": held.id, "mode": "prompt", "text": "now"})).unwrap();
    assert_eq!((prompted["state"].as_str(), prompted["final"].as_bool()), (Some("submitted"), Some(false)));
    assert!(prompted["turn_id"].is_null());

    // Steering with nothing to steer is settled `failed` by the engine before the call returns:
    // report it instead of a stale "queued".
    let idle = Session::new(ProviderId::ClaudeCode, project.to_string_lossy().into_owned());
    store.upsert_session(&idle).unwrap();
    let err = write(&engine, "codetwo_turn_send", json!({"session_id": idle.id, "mode": "steer", "text": "go"})).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
}

// --- policy_set is one atomic tighten-only commit ----------------------------------------------

fn native_policy_op(engine: &Engine, runtime: &tokio::runtime::Runtime, op: codetwo_core::Op) {
    runtime.block_on(engine.submit(op)).unwrap();
}

/// Run `external` against a native change made at the same moment, many times over. Whatever the
/// interleaving, the human's tightening must survive and the external caller must never loosen.
fn race_policy_set(
    start: (PermissionMode, SandboxPolicy),
    external: serde_json::Value,
    human: impl Fn(&str) -> codetwo_core::Op + Send + Sync + 'static,
    expected: (PermissionMode, SandboxPolicy),
) {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let mut session = Session::new(ProviderId::ClaudeCode, root.path().to_string_lossy().into_owned());
    (session.permission_mode, session.sandbox_policy) = start;
    store.upsert_session(&session).unwrap();
    let engine = engine_with_store(store.clone());
    let human = Arc::new(human);
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let id = session.id.clone();
    for round in 0..400 {
        // A native writer may loosen: put the session back on the starting pair.
        native_policy_op(&engine, &runtime, codetwo_core::Op::SetExecutionPolicy {
            session: id.clone(), mode: start.0, sandbox: start.1, request_id: None,
        });
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let racer = {
            let (engine, barrier, human, id) = (engine.clone(), barrier.clone(), human.clone(), id.clone());
            std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
                barrier.wait();
                runtime.block_on(engine.submit(human(&id))).unwrap();
            })
        };
        barrier.wait();
        let mut args = external.clone();
        args["session_id"] = json!(id);
        match write(&engine, "codetwo_policy_set", args) {
            Ok(_) => {}
            Err(error) => assert_eq!(error.kind, ToolErrorKind::Denied, "round {round}: {error:?}"),
        }
        racer.join().unwrap();
        let stored = store.get_session(&id).unwrap().unwrap();
        assert_eq!((stored.permission_mode, stored.sandbox_policy), expected, "round {round}");
    }
}

#[test]
fn policy_set_mode_only_keeps_a_concurrent_sandbox_tightening() {
    // The request names only `mode`; the sandbox it leaves alone is whatever is in force when it
    // commits, not the value it read earlier (the old read-compute-submit overwrote it).
    race_policy_set(
        (PermissionMode::AcceptEdits, SandboxPolicy::WorkspaceWrite),
        json!({"mode": "ask"}),
        |id| codetwo_core::Op::SetSandbox { session: id.to_string(), sandbox: SandboxPolicy::ReadOnly },
        (PermissionMode::Ask, SandboxPolicy::ReadOnly),
    );
}

#[test]
fn policy_set_explicit_axis_cannot_loosen_a_concurrent_tightening() {
    // Allowed before the human's change, a loosening after it: it is refused, never applied.
    race_policy_set(
        (PermissionMode::AcceptEdits, SandboxPolicy::DangerFullAccess),
        json!({"sandbox_policy": "workspace_write"}),
        |id| codetwo_core::Op::SetSandbox { session: id.to_string(), sandbox: SandboxPolicy::ReadOnly },
        (PermissionMode::AcceptEdits, SandboxPolicy::ReadOnly),
    );
}

#[test]
fn policy_set_refuses_each_axis_against_the_committed_state_and_still_allows_native_loosening() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let mut session = Session::new(ProviderId::ClaudeCode, root.path().to_string_lossy().into_owned());
    session.permission_mode = PermissionMode::AcceptEdits;
    session.sandbox_policy = SandboxPolicy::WorkspaceWrite;
    store.upsert_session(&session).unwrap();
    let engine = engine_with_store(store.clone());
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let id = session.id.clone();
    let stored = || {
        let s = store.get_session(&id).unwrap().unwrap();
        (s.permission_mode, s.sandbox_policy)
    };

    // A native tightening lands first: a mode-only request keeps it.
    native_policy_op(&engine, &runtime, codetwo_core::Op::SetSandbox { session: id.clone(), sandbox: SandboxPolicy::ReadOnly });
    let applied = write(&engine, "codetwo_policy_set", json!({"session_id": id, "mode": "ask"})).unwrap();
    assert_eq!((applied["mode"].as_str(), applied["sandbox_policy"].as_str()), (Some("ask"), Some("read_only")));
    assert_eq!(stored(), (PermissionMode::Ask, SandboxPolicy::ReadOnly));

    // Each axis is checked on its own against what is stored; a refusal writes nothing.
    for args in [json!({"mode": "accept_edits"}), json!({"sandbox_policy": "workspace_write"})] {
        let mut args = args;
        args["session_id"] = json!(id);
        let err = write(&engine, "codetwo_policy_set", args).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Denied);
        assert_eq!(stored(), (PermissionMode::Ask, SandboxPolicy::ReadOnly));
    }

    // Explicit native loosening is still authorized: only the external surface is tighten-only.
    native_policy_op(&engine, &runtime, codetwo_core::Op::SetExecutionPolicy {
        session: id.clone(), mode: PermissionMode::AcceptEdits, sandbox: SandboxPolicy::WorkspaceWrite, request_id: None,
    });
    assert_eq!(stored(), (PermissionMode::AcceptEdits, SandboxPolicy::WorkspaceWrite));

    let err = write(&engine, "codetwo_policy_set", json!({"session_id": "no-such-session", "mode": "ask"})).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::NotFound);
}

// --- failures after dispatch keep the receipt and never invite a replay ------------------------

fn trigger(db: &str, sql: &str) {
    rusqlite::Connection::open(db).unwrap().execute_batch(sql).unwrap();
}

#[test]
fn turn_send_keeps_the_receipt_when_the_drain_fails_after_the_enqueue_persisted() {
    let (_dir, db, store, engine, project) = on_disk();
    let idle = Session::new(ProviderId::ClaudeCode, project.to_string_lossy().into_owned());
    store.upsert_session(&idle).unwrap();
    // The enqueue (an INSERT) commits; the drain's claim (an UPDATE) fails.
    trigger(&db, "CREATE TRIGGER block_claim BEFORE UPDATE ON prompt_deliveries BEGIN SELECT RAISE(ABORT,'blocked'); END;");

    let receipt = write(&engine, "codetwo_turn_send", json!({"session_id": idle.id, "mode": "queue", "text": "later"})).unwrap();
    let id = receipt["delivery_id"].as_str().unwrap();
    assert_eq!((receipt["state"].as_str(), receipt["final"].as_bool(), receipt["no_retry"].as_bool()), (Some("queued"), Some(false), Some(true)));
    assert_eq!(store.prompt_delivery(id).unwrap().unwrap().state, "queued");
}

#[test]
fn turn_send_reports_an_unknown_delivery_as_a_receipt_with_no_retry() {
    let (_dir, db, store, engine, project) = on_disk();
    let idle = Session::new(ProviderId::ClaudeCode, project.to_string_lossy().into_owned());
    store.upsert_session(&idle).unwrap();
    trigger(&db, "CREATE TRIGGER mark_unknown AFTER INSERT ON prompt_deliveries BEGIN UPDATE prompt_deliveries SET state='unknown' WHERE id=NEW.id; END;");

    // Not an error: an error would hide the delivery id and a client could resend the prompt.
    let receipt = write(&engine, "codetwo_turn_send", json!({"session_id": idle.id, "mode": "queue", "text": "maybe"})).unwrap();
    assert_eq!((receipt["state"].as_str(), receipt["no_retry"].as_bool(), receipt["final"].as_bool()), (Some("unknown"), Some(true), Some(false)));
    let id = receipt["delivery_id"].as_str().unwrap();
    assert_eq!(store.prompt_delivery(id).unwrap().unwrap().state, "unknown");
}

#[test]
fn turn_stop_does_not_report_stopped_when_queued_prompts_could_not_be_cancelled() {
    let (_dir, db, store, engine, project) = on_disk();
    let session = Session::new(ProviderId::ClaudeCode, project.to_string_lossy().into_owned());
    store.upsert_session(&session).unwrap();

    let stopped = write(&engine, "codetwo_turn_stop", json!({"session_id": session.id})).unwrap();
    assert_eq!(stopped["stopped"], true);

    let queued = codetwo_core::prompt_delivery::PromptDelivery {
        id: "queued-1".into(), session_id: session.id.clone(), mode: "queue".into(),
        doc: vec![codetwo_core::skill::DocBlock::Text { text: "later".into() }],
        state: "queued".into(), outcome: String::new(),
        goal_id: None, contract_revision: None, assignment_id: None, expected_turn: None,
    };
    store.enqueue_delivery(&queued).unwrap();
    trigger(&db, "CREATE TRIGGER block_cancel BEFORE UPDATE ON prompt_deliveries BEGIN SELECT RAISE(ABORT,'blocked'); END;");
    let err = write(&engine, "codetwo_turn_stop", json!({"session_id": session.id})).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Conflict);
    assert!(err.message.contains("queued prompts"), "{}", err.message);
    assert_eq!(store.prompt_delivery("queued-1").unwrap().unwrap().state, "queued");
}

// --- audit failure ----------------------------------------------------------------------------

#[test]
fn a_write_whose_pre_dispatch_audit_fails_is_refused_and_never_runs() {
    let dir = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let session = Session::new(ProviderId::ClaudeCode, root.path().to_string_lossy().into_owned());
    store.upsert_session(&session).unwrap();
    let engine = engine_with_store(store.clone());
    engine.external_mcp_state().set_enabled(true);
    engine.external_mcp_state().configure_data_dir(dir.path()).unwrap();
    let client = client(ProjectScope::All);
    let args = json!({"session_id": session.id, "title": "Renamed"});

    // The sink works: the call runs.
    engine.authorize_external_mcp_call(&client, "codetwo_session_update", &args).unwrap();
    assert_eq!(store.get_session(&session.id).unwrap().unwrap().title, "Renamed");

    // The sink breaks: the allow record cannot be written, so the write is refused before it runs.
    std::fs::remove_file(dir.path().join(codetwo_core::external_mcp::state::AUDIT_FILE)).unwrap();
    let args = json!({"session_id": session.id, "title": "Second"});
    let err = engine.authorize_external_mcp_call(&client, "codetwo_session_update", &args).unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::Internal);
    assert_eq!(store.get_session(&session.id).unwrap().unwrap().title, "Renamed");
}

//! External MCP gate and credential integration tests.

use codetwo_core::external_mcp::clients::{
    ExternalScope, ProjectScope, ResolveError, TtlChoice, TOKEN_PREFIX,
};
use codetwo_core::external_mcp::ctx::ToolErrorKind;
use codetwo_core::host_mcp::{HostMcpCapability, HostMcpRegistry, HostMcpScope};
use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
use codetwo_core::skill::SkillLibrary;
use codetwo_core::{Engine, Store};
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

fn engine_with_store(store: Arc<Store>) -> Engine {
    Engine::with_store(
        vec![Provider {
            id: ProviderId::ClaudeCode,
            display_name: "Claude".into(),
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
    )
    .0
}

fn enable_external(engine: &Engine, dir: &TempDir) -> String {
    engine.external_mcp_state().set_enabled(true);
    engine
        .external_mcp_state()
        .configure_data_dir(dir.path())
        .unwrap();
    engine
        .external_mcp_state()
        .with_registry(|reg| {
            reg.create(
                "test-client",
                vec![ExternalScope::Read, ExternalScope::Admin],
                ProjectScope::Paths(vec![dir.path().to_path_buf()]),
                TtlChoice::Default30Days,
            )
            .unwrap()
            .1
        })
        .unwrap()
}

#[test]
fn scope_denial_and_audit_before_dispatch() {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let engine = engine_with_store(store);
    let token = enable_external(&engine, &dir);
    let client = engine.resolve_external_client(&token).unwrap();
    let err = engine
        .authorize_external_mcp_call(
            &client,
            "codetwo_project_create",
            &json!({ "path": dir.path().join("new").display().to_string() }),
        )
        .unwrap_err();
    assert_eq!(err.kind, codetwo_core::external_mcp::ctx::ToolErrorKind::Denied);
    let audit_path = dir.path().join("external-mcp-audit.jsonl");
    let audit_text = std::fs::read_to_string(audit_path).unwrap_or_default();
    assert!(!audit_text.contains(&token));
}

#[test]
fn project_confinement_and_unknown_token() {
    let dir = TempDir::new().unwrap();
    let allowed = dir.path().join("allowed");
    std::fs::create_dir_all(&allowed).unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let engine = engine_with_store(store);
    engine.external_mcp_state().set_enabled(true);
    engine
        .external_mcp_state()
        .configure_data_dir(dir.path())
        .unwrap();
    let token = engine
        .external_mcp_state()
        .with_registry(|reg| {
            reg.create(
                "scoped",
                vec![ExternalScope::Read],
                ProjectScope::Paths(vec![allowed.clone()]),
                TtlChoice::Default30Days,
            )
            .unwrap()
            .1
        })
        .unwrap();
    let client = engine.resolve_external_client(&token).unwrap();
    let err = engine
        .authorize_external_mcp_call(
            &client,
            "codetwo_project_read",
            &json!({ "path": "/outside/scope" }),
        )
        .unwrap_err();
    assert!(matches!(
        err.kind,
        codetwo_core::external_mcp::ctx::ToolErrorKind::NotFound
    ));
    assert!(matches!(
        engine.resolve_external_client("ctmcp_not_a_real_token_value_here_xxxxxxxxxxxx"),
        Err(ResolveError::Unknown)
    ));
}

#[test]
fn host_session_token_rejected_on_external_gate() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let engine = engine_with_store(store);
    engine.external_mcp_state().set_enabled(true);
    let mut host = HostMcpRegistry::new();
    let (_id, host_token) = host
        .issue(HostMcpScope {
            session_id: "s1".into(),
            provider_id: "claude_code".into(),
            capabilities: HostMcpCapability::default_read_only_set(),
        })
        .unwrap();
    assert!(!host_token.starts_with(TOKEN_PREFIX));
    assert!(matches!(
        engine.resolve_external_client(&host_token),
        Err(ResolveError::NotExternalToken)
    ));
}

#[test]
fn destructive_tool_requires_confirm() {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let engine = engine_with_store(store);
    let token = enable_external(&engine, &dir);
    let client = engine.resolve_external_client(&token).unwrap();
    let err = engine
        .authorize_external_mcp_call(
            &client,
            "codetwo_project_delete",
            &json!({ "path": dir.path().display().to_string() }),
        )
        .unwrap_err();
    assert_eq!(err.kind, codetwo_core::external_mcp::ctx::ToolErrorKind::Denied);
}

#[test]
fn token_not_present_in_audit_or_error_strings() {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let engine = engine_with_store(store);
    let token = enable_external(&engine, &dir);
    let resp = engine.handle_external_mcp_json(
        &token,
        &json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"codetwo_capabilities","arguments":{}}
        }),
    );
    let encoded = resp.to_string();
    assert!(!encoded.contains(&token));
    let bad = engine.handle_external_mcp_json(
        "ctmcp_invalidtokenvalue",
        &json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    );
    assert!(!bad.to_string().contains("ctmcp_invalidtokenvalue"));
}

#[test]
fn expired_and_revoked_credentials_rejected() {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let engine = engine_with_store(store);
    engine.external_mcp_state().set_enabled(true);
    engine
        .external_mcp_state()
        .configure_data_dir(dir.path())
        .unwrap();
    let (record, token) = engine
        .external_mcp_state()
        .with_registry(|reg| {
            reg.create(
                "ttl",
                vec![ExternalScope::Read],
                ProjectScope::All,
                TtlChoice::Default30Days,
            )
            .unwrap()
        })
        .unwrap();
    engine
        .external_mcp_state()
        .with_registry(|reg| {
            reg.expire_client_for_test(&record.id);
        })
        .unwrap();
    assert!(matches!(
        engine.resolve_external_client(&token),
        Err(ResolveError::Expired)
    ));
    let (record2, token2) = engine
        .external_mcp_state()
        .with_registry(|reg| {
            reg.create(
                "rev",
                vec![ExternalScope::Read],
                ProjectScope::All,
                TtlChoice::Default30Days,
            )
            .unwrap()
        })
        .unwrap();
    engine
        .external_mcp_state()
        .with_registry(|reg| reg.revoke(&record2.id).map(|_| ()))
        .unwrap().unwrap();
    assert!(matches!(
        engine.resolve_external_client(&token2),
        Err(ResolveError::Revoked)
    ));
}

#[test]
fn read_rate_limit_enforced_at_gate() {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let engine = engine_with_store(store);
    let token = enable_external(&engine, &dir);
    let client = engine.resolve_external_client(&token).unwrap();
    let client_id = client.record.id.clone();
    let limiter = engine.external_mcp_state().limiter();
    for _ in 0..120 {
        assert!(limiter.try_consume(&client_id, false));
    }
    let err = engine
        .authorize_external_mcp_call(&client, "codetwo_capabilities", &json!({}))
        .unwrap_err();
    assert_eq!(err.kind, ToolErrorKind::RateLimited);
}

#[test]
fn successful_call_writes_audit_before_result() {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let engine = engine_with_store(store);
    let token = enable_external(&engine, &dir);
    let client = engine.resolve_external_client(&token).unwrap();
    engine
        .authorize_external_mcp_call(&client, "codetwo_capabilities", &json!({}))
        .unwrap();
    let audit_path = dir.path().join("external-mcp-audit.jsonl");
    let audit_text = std::fs::read_to_string(audit_path).unwrap();
    assert!(audit_text.contains("codetwo_capabilities"));
    assert!(audit_text.contains("\"allowed\":true"));
}

#[test]
fn destructive_automation_create_uses_the_gate_and_audits_confirmation_failures() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().canonicalize().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    store.add_project(&path.to_string_lossy(), None, codetwo_core::session::now_millis()).unwrap();
    let engine = engine_with_store(store.clone());
    let token = enable_external(&engine, &dir);
    let client = engine.resolve_external_client(&token).unwrap();
    let mut args = json!({"name":"daily", "prompt":"check", "project_path":path,
        "provider":"claude_code", "cron":"0 9 * * *", "timezone":"UTC",
        "confirm":true, "expected_id":"new"});
    let created = engine.authorize_external_mcp_call(&client,"codetwo_automation_create", &args).unwrap();
    assert!(created["id"].is_string());
    args["confirm"] = json!(false);
    assert!(engine.authorize_external_mcp_call(&client,"codetwo_automation_create", &args).is_err());
    let audit = std::fs::read_to_string(dir.path().join("external-mcp-audit.jsonl")).unwrap();
    assert!(audit.contains("confirmation_denied"));
    args["confirm"] = json!(true);
    args["permission_mode"] = json!("yolo");
    assert!(engine.authorize_external_mcp_call(&client,"codetwo_automation_create", &args).is_err());
}

#[test]
fn workspace_reads_require_registered_projects_and_search_treats_query_as_data() {
    let dir = TempDir::new().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("visible.txt"), "--help needle").unwrap();
    std::fs::write(project.join(".env.local"), "--help secret").unwrap();
    let outside = dir.path().join("outside.txt");
    std::fs::write(&outside, "--help private").unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    store.add_project(&project.to_string_lossy(), None, codetwo_core::session::now_millis()).unwrap();
    let engine = engine_with_store(store);
    let token = enable_external(&engine, &dir);
    let client = engine.resolve_external_client(&token).unwrap();
    assert!(engine.authorize_external_mcp_call(&client, "codetwo_workspace_file_read", &json!({"path": outside})).is_err());
    assert!(engine.authorize_external_mcp_call(&client, "codetwo_workspace_file_read", &json!({"path": project.join(".env.local")})).is_err());
    let result = engine.authorize_external_mcp_call(&client, "codetwo_workspace_search", &json!({"query": "--help"})).unwrap();
    let matches = result["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    assert!(matches[0]["path"].as_str().unwrap().ends_with("visible.txt"));
}

#[test]
fn response_cap_handles_unicode_and_json_escaping() {
    use codetwo_core::external_mcp::gate::{cap_response, MAX_RESPONSE_BYTES};
    for content in ["中".repeat(MAX_RESPONSE_BYTES), "\n\"\\".repeat(MAX_RESPONSE_BYTES)] {
        let value = cap_response(Ok(json!({"content": content}))).unwrap();
        assert_eq!(value["truncated"], true);
        assert!(serde_json::to_vec(&value).unwrap().len() <= MAX_RESPONSE_BYTES);
    }
}

#[cfg(unix)]
#[test]
fn workspace_file_read_refuses_fifo_without_waiting_for_a_writer() {
    let dir = TempDir::new().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let fifo = project.join("pipe");
    let name = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let store = Arc::new(Store::open_in_memory().unwrap());
    store.add_project(&project.to_string_lossy(), None, codetwo_core::session::now_millis()).unwrap();
    let engine = engine_with_store(store);
    let token = enable_external(&engine, &dir);
    let client = engine.resolve_external_client(&token).unwrap();
    assert!(engine.authorize_external_mcp_call(&client, "codetwo_workspace_file_read", &json!({"path": fifo})).is_err());
}

#[test]
fn schema_array_limits_are_enforced_at_the_gate() {
    let dir = TempDir::new().unwrap();
    let engine = engine_with_store(Arc::new(Store::open_in_memory().unwrap()));
    let token = enable_external(&engine, &dir);
    let client = engine.resolve_external_client(&token).unwrap();
    let result = engine.authorize_external_mcp_call(&client, "codetwo_events_poll", &json!({"types": vec!["turn.completed"; 33]}));
    assert_eq!(result.unwrap_err().kind, ToolErrorKind::InvalidParams);
}

#[test]
fn transcript_cursor_returns_an_earlier_page() {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let session = codetwo_core::session::Session::new(ProviderId::ClaudeCode, dir.path().to_string_lossy().to_string());
    store.upsert_session(&session).unwrap();
    for text in ["first", "second", "third"] {
        store.append_part(&session.id, codetwo_core::session::Role::User,
            &codetwo_core::session::Part::Prompt { text: text.into(), display: text.into() }).unwrap();
    }
    let engine = engine_with_store(store);
    let token = enable_external(&engine, &dir);
    let client = engine.resolve_external_client(&token).unwrap();
    let first = engine.authorize_external_mcp_call(&client, "codetwo_transcript_read", &json!({"session_id":session.id, "limit":1})).unwrap();
    let cursor = first["next_before"].as_i64().unwrap().to_string();
    let earlier = engine.authorize_external_mcp_call(&client, "codetwo_transcript_read", &json!({"session_id":session.id, "limit":1, "cursor":cursor})).unwrap();
    assert_eq!(first["entries"].as_array().unwrap().len(), 1);
    assert_ne!(earlier["entries"], first["entries"]);
}

#[test]
fn unknown_tools_spend_the_rate_limit_before_any_audit_write() {
    let dir = TempDir::new().unwrap();
    let engine = engine_with_store(Arc::new(Store::open_in_memory().unwrap()));
    let token = enable_external(&engine, &dir);
    let client = engine.resolve_external_client(&token).unwrap();
    let mut limited = 0;
    for _ in 0..200 {
        let error = engine.authorize_external_mcp_call(&client, "codetwo_no_such_tool", &json!({})).unwrap_err();
        limited += (error.kind == ToolErrorKind::RateLimited) as usize;
    }
    assert!(limited >= 70, "an unknown-tool flood must hit the read budget (limited {limited})");
    let audited = std::fs::read_to_string(dir.path().join("external-mcp-audit.jsonl")).unwrap().lines().count();
    assert!(audited <= 130, "over-budget unknown calls must not be audited ({audited} records)");
}

#[test]
fn internal_errors_never_expose_raw_text() {
    let dir = TempDir::new().unwrap();
    let engine = Engine::new(Vec::new(), SkillLibrary::default()).0; // no store
    let token = enable_external(&engine, &dir);
    let client = engine.resolve_external_client(&token).unwrap();
    let error = engine.authorize_external_mcp_call(&client, "codetwo_project_list", &json!({})).unwrap_err();
    assert_eq!((error.kind, error.message.as_str()), (ToolErrorKind::Internal, "operation failed"));
    let rpc = engine.handle_external_mcp_json(&token, &json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "codetwo_project_list", "arguments": {}},
    }));
    assert_eq!(rpc["result"]["content"][0]["text"], "operation failed");
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .arg("-C").arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

#[test]
fn git_reads_of_a_subdirectory_project_do_not_leak_the_rest_of_the_repository() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    let app = repo.join("app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::create_dir_all(repo.join("other")).unwrap();
    for file in ["app/a.txt", "app/.env", "other/b.txt", "other/c.txt"] {
        std::fs::write(repo.join(file), "one\n").unwrap();
    }
    let meta = TempDir::new().unwrap(); // keeps the git dir outside the work tree
    git(&repo, &["init", "-q", "--template=", &format!("--separate-git-dir={}", meta.path().join("git").display())]);
    git(&repo, &["add", "-A", "-f"]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    for file in ["app/a.txt", "app/.env", "other/b.txt"] {
        std::fs::write(repo.join(file), "two\n").unwrap();
    }
    std::fs::write(repo.join("other/untracked.txt"), "x\n").unwrap();
    git(&repo, &["mv", "other/c.txt", "other/d.txt"]); // staged rename entirely outside the project
    let store = Arc::new(Store::open_in_memory().unwrap());
    store.add_project(&app.to_string_lossy(), None, codetwo_core::session::now_millis()).unwrap();
    let engine = engine_with_store(store);
    let token = enable_external(&engine, &dir);
    let client = engine.resolve_external_client(&token).unwrap();
    let args = json!({"path": app});

    let status = engine.authorize_external_mcp_call(&client, "codetwo_git_status", &args).unwrap();
    let paths: Vec<_> = status["files"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert!(paths.iter().all(|path| !path.contains("other")), "status leaked {paths:?}");
    assert!(paths.contains(&"a.txt"), "paths are relative to the project: {paths:?}");

    let diff = engine.authorize_external_mcp_call(&client, "codetwo_git_diff", &args).unwrap();
    let text = diff["diff"].as_str().unwrap();
    assert!(text.contains("+++ b/a.txt"), "{diff} {status}");
    assert!(!text.contains("other") && !text.contains("b.txt"), "diff leaked {text}");
    assert!(!text.contains(".env"), "secret exclusion must still apply: {text}");
}

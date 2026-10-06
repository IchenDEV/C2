//! Integration tests for the `codetwo` headless CLI.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use codetwo_core::external_mcp::clients::{ExternalScope, ProjectScope, TtlChoice};
use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
use codetwo_core::session::{PendingInput, PendingInputKind, Session, SessionRunState};
use codetwo_core::skill::SkillLibrary;
use codetwo_core::store::Store;
use codetwo_core::Engine;
use codetwo_server::cli::{
    admin::{self, McpClientAction},
    client::{self, SessionAction},
    config::{write_token_file, ClientConfig, GlobalOpts},
    output::ExitCode,
    run_mcp_client, run_session, run_status, run_stop, run_wait,
};
use codetwo_server::{bind_and_serve, fanout, AuthState};
use serde_json::Value;
use tempfile::TempDir;

struct TestServer {
    addr: SocketAddr,
    token: String,
    engine: Arc<Engine>,
    store: Arc<Store>,
}

async fn start_server(dir: &TempDir, external_mcp: bool) -> TestServer {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (engine, events_rx) = Engine::with_store(
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
        store.clone(),
    );
    engine.external_mcp_state().set_enabled(external_mcp);
    engine
        .external_mcp_state()
        .configure_data_dir(dir.path())
        .unwrap();
    let token = engine
        .external_mcp_state()
        .with_registry(|reg| {
            reg.create(
                "cli-test",
                vec![
                    ExternalScope::Read,
                    ExternalScope::Operate,
                    ExternalScope::Approve,
                ],
                ProjectScope::All,
                TtlChoice::Default30Days,
            )
            .unwrap()
            .1
        })
        .unwrap();
    let engine = Arc::new(engine);
    let events = fanout(events_rx);
    let auth = Arc::new(AuthState::load(None));
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let (bound, _handle) = bind_and_serve(engine.clone(), events, addr, auth)
        .await
        .unwrap();
    TestServer {
        addr: bound,
        token,
        engine,
        store,
    }
}

impl TestServer {
    fn config(&self) -> ClientConfig {
        ClientConfig {
            base_url: format!("http://{}", self.addr),
            token: self.token.clone(),
        }
    }

    fn add_session(&self, dir: &Path, state: SessionRunState) -> String {
        let mut session = Session::new(ProviderId::ClaudeCode, dir.to_string_lossy());
        session.project_path = Some(dir.to_string_lossy().into_owned());
        session.activity.state = state;
        self.store.upsert_session(&session).unwrap();
        session.id.to_string()
    }
}

fn globals(dir: &Path) -> GlobalOpts {
    GlobalOpts {
        json: true,
        data_dir: Some(dir.to_path_buf()),
        ..Default::default()
    }
}

fn codetwo() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_codetwo"));
    // Never inherit a real credential, server or data directory from the developer shell.
    command
        .env_remove("CODETWO_TOKEN")
        .env_remove("CODETWO_URL")
        .env_remove("CODETWO_DATA_DIR")
        .env_remove("CODETWO_EXTERNAL_MCP");
    command
}

#[tokio::test]
async fn cli_status_and_session_list_via_library() {
    let dir = TempDir::new().unwrap();
    let server = start_server(&dir, true).await;
    let config = server.config();
    let status = tokio::task::spawn_blocking({
        let config = config.clone();
        move || {
            (
                run_status(&config),
                run_session(&config, SessionAction::List),
            )
        }
    })
    .await
    .unwrap();
    assert!(status.0.envelope.ok, "{:?}", status.0);
    assert!(status.1.envelope.ok, "{:?}", status.1);
}

#[tokio::test]
async fn admin_create_client_and_cli_auth() {
    let dir = TempDir::new().unwrap();
    let server = start_server(&dir, true).await;
    let token_path = dir.path().join("new-cli.token");
    let created = run_mcp_client(
        &globals(dir.path()),
        McpClientAction::Create {
            name: "cli-admin".into(),
            scopes: admin::parse_scopes(None).unwrap(),
            projects: ProjectScope::All,
            ttl: TtlChoice::Default30Days,
            token_out: Some(token_path.clone()),
        },
    );
    assert!(created.envelope.ok, "{created:?}");
    let raw = std::fs::read_to_string(&token_path).unwrap();
    assert!(raw.trim().starts_with("ctmcp_"));
    assert!(!serde_json::to_string(&created.envelope)
        .unwrap()
        .contains("ctmcp_"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&token_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    // The running server picks the new credential up from the shared file.
    let config = ClientConfig {
        base_url: format!("http://{}", server.addr),
        token: raw.trim().to_string(),
    };
    let outcome = tokio::task::spawn_blocking(move || run_status(&config))
        .await
        .unwrap();
    assert!(outcome.envelope.ok, "{outcome:?}");
}

#[test]
fn json_create_without_token_out_creates_no_credential() {
    let dir = TempDir::new().unwrap();
    let outcome = run_mcp_client(
        &globals(dir.path()),
        McpClientAction::Create {
            name: "orphan".into(),
            scopes: admin::parse_scopes(None).unwrap(),
            projects: ProjectScope::All,
            ttl: TtlChoice::Default30Days,
            token_out: None,
        },
    );
    assert_eq!(outcome.exit, ExitCode::Error);
    let listed = run_mcp_client(&globals(dir.path()), McpClientAction::List);
    assert_eq!(
        listed.envelope.result.unwrap()["clients"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn failed_token_out_revokes_the_new_credential() {
    let dir = TempDir::new().unwrap();
    let outcome = run_mcp_client(
        &globals(dir.path()),
        McpClientAction::Create {
            name: "lost".into(),
            scopes: admin::parse_scopes(None).unwrap(),
            projects: ProjectScope::All,
            ttl: TtlChoice::Default30Days,
            token_out: Some(dir.path().join("missing-dir").join("t")),
        },
    );
    assert_eq!(outcome.exit, ExitCode::Error);
    let listed = run_mcp_client(&globals(dir.path()), McpClientAction::List);
    let listed = listed.envelope.result.unwrap();
    for client in listed["clients"].as_array().unwrap() {
        assert!(client["revoked_at"].is_string(), "{client}");
    }
}

#[test]
fn ttl_parsing() {
    assert_eq!(admin::parse_ttl(None).unwrap(), TtlChoice::Default30Days);
    assert_eq!(admin::parse_ttl(Some("7d")).unwrap(), TtlChoice::Days(7));
    assert_eq!(
        admin::parse_ttl(Some("permanent")).unwrap(),
        TtlChoice::Permanent
    );
    for bad in ["0d", "d", "forever", "90", "-1d"] {
        assert!(admin::parse_ttl(Some(bad)).is_err(), "{bad}");
    }
}

#[tokio::test]
async fn revoked_token_is_refused_with_exit_1() {
    let dir = TempDir::new().unwrap();
    let server = start_server(&dir, true).await;
    let id = server
        .engine
        .external_mcp_state()
        .with_registry(|reg| reg.list().unwrap()[0].id.clone())
        .unwrap();
    run_mcp_client(&globals(dir.path()), McpClientAction::Revoke { id });
    let config = server.config();
    let outcome = tokio::task::spawn_blocking(move || run_status(&config))
        .await
        .unwrap();
    assert_eq!(outcome.exit, ExitCode::Error);
    assert!(!outcome.envelope.ok);
}

#[tokio::test]
async fn disabled_surface_reports_not_enabled() {
    let dir = TempDir::new().unwrap();
    let server = start_server(&dir, false).await;
    let config = server.config();
    let outcome = tokio::task::spawn_blocking(move || run_status(&config))
        .await
        .unwrap();
    assert_eq!(outcome.exit, ExitCode::Error);
    assert_eq!(outcome.envelope.error.unwrap().code, "not_enabled");
}

#[test]
fn plain_http_to_a_non_loopback_host_is_refused_before_connecting() {
    let config = ClientConfig {
        base_url: "http://192.0.2.1:4599".into(),
        token: "ctmcp_never_sent".into(),
    };
    let outcome = run_status(&config);
    assert_eq!(outcome.envelope.error.unwrap().code, "insecure_url");
}

#[tokio::test]
async fn wait_exit_codes() {
    let dir = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let server = start_server(&dir, true).await;
    let running = server.add_session(
        project.path(),
        SessionRunState::Running {
            turn_id: "t1".into(),
            prompt_request_id: None,
        },
    );
    let idle = server.add_session(project.path(), SessionRunState::Idle);
    let asking = server.add_session(
        project.path(),
        SessionRunState::AwaitingInput {
            turn_id: "t2".into(),
            prompt_request_id: None,
            pending: vec![PendingInput {
                input_id: "p1".into(),
                kind: PendingInputKind::Permission,
                title: "Run tool".into(),
                options: vec![("allow".into(), "Allow".into())],
                option_kinds: Default::default(),
                sequence: 1,
                context: Default::default(),
                form: None,
            }],
        },
    );
    let config = server.config();
    let exits = tokio::task::spawn_blocking(move || {
        let wait = |session_id: &str, timeout_secs| {
            run_wait(
                &config,
                client::WaitOpts {
                    session_id: session_id.to_string(),
                    timeout_secs,
                },
            )
        };
        let started = Instant::now();
        let attention = wait(&asking, 30);
        // Reported from the first slice, not after the 30 s deadline.
        assert!(started.elapsed() < Duration::from_secs(10));
        (
            wait(&idle, 5).exit,
            attention.exit,
            wait(&running, 1).exit,
            wait("no-such-session", 1).exit,
        )
    })
    .await
    .unwrap();
    assert_eq!(
        exits,
        (
            ExitCode::Ok,
            ExitCode::NeedsAttention,
            ExitCode::Timeout,
            ExitCode::Error
        )
    );
}

#[tokio::test]
async fn stop_unknown_session_returns_error() {
    let dir = TempDir::new().unwrap();
    let server = start_server(&dir, true).await;
    let config = server.config();
    let outcome = tokio::task::spawn_blocking(move || run_stop(&config, "missing"))
        .await
        .unwrap();
    assert_eq!(outcome.exit, ExitCode::Error);
}

#[test]
fn cli_token_insecure_permissions_refused() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cli.token");
    std::fs::write(&path, "ctmcp_testtoken\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    let globals = GlobalOpts {
        data_dir: Some(dir.path().to_path_buf()),
        url: Some("http://127.0.0.1:1".into()),
        ..Default::default()
    };
    let error = globals.resolve_client().unwrap_err();
    assert_eq!(error.envelope.error.unwrap().code, "insecure_token_file");
}

#[test]
fn binary_rejects_token_argv_and_json_shape() {
    for flag in [&["--token", "ctmcp_bad"][..], &["--token=ctmcp_bad"]] {
        let output = codetwo()
            .args(flag)
            .args(["status", "--json"])
            .output()
            .expect("run codetwo");
        assert_eq!(output.status.code(), Some(ExitCode::Error.as_i32()));
        let stdout = String::from_utf8_lossy(&output.stdout);
        let parsed: Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(parsed["ok"], Value::Bool(false));
        assert!(!stdout.contains("ctmcp_"));
    }
}

#[test]
fn binary_usage_errors_exit_1_and_help_exits_0() {
    let usage = codetwo().args(["send", "only-id"]).output().unwrap();
    assert_eq!(usage.status.code(), Some(1));
    let unknown = codetwo().args(["serve", "--bogus"]).output().unwrap();
    assert_eq!(unknown.status.code(), Some(1));
    for args in [&["--help"][..], &["serve", "--help"], &["status", "-h"]] {
        let help = codetwo().args(args).output().unwrap();
        assert_eq!(help.status.code(), Some(0), "{args:?}");
        assert!(!help.stdout.is_empty());
    }
}

#[test]
fn binary_mcp_client_lifecycle_in_a_temp_data_dir() {
    let dir = TempDir::new().unwrap();
    let data = dir.path().to_str().unwrap();
    let token_out = dir.path().join("t.token");
    let create = codetwo()
        .args([
            "mcp",
            "client",
            "create",
            "--name",
            "e2e",
            "--ttl",
            "7d",
            "--json",
            "--data-dir",
            data,
        ])
        .args(["--token-out", token_out.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(
        create.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&create.stderr)
    );
    let created: Value = serde_json::from_slice(&create.stdout).unwrap();
    assert!(!String::from_utf8_lossy(&create.stdout).contains("ctmcp_"));
    let id = created["result"]["id"].as_str().unwrap().to_string();

    let list = codetwo()
        .args(["mcp", "client", "list", "--json", "--data-dir", data])
        .output()
        .unwrap();
    assert_eq!(list.status.code(), Some(0));
    let revoke = codetwo()
        .args(["mcp", "client", "revoke", &id, "--json", "--data-dir", data])
        .output()
        .unwrap();
    assert_eq!(revoke.status.code(), Some(0));
    let again = codetwo()
        .args([
            "mcp",
            "client",
            "revoke",
            "no-such-id",
            "--json",
            "--data-dir",
            data,
        ])
        .output()
        .unwrap();
    assert_eq!(again.status.code(), Some(1));
}

#[tokio::test]
async fn binary_pending_via_json() {
    let dir = TempDir::new().unwrap();
    let server = start_server(&dir, true).await;
    let token_file = dir.path().join("cli.token");
    write_token_file(&token_file, &server.token).unwrap();
    let url = format!("http://{}", server.addr);
    let output = tokio::task::spawn_blocking(move || {
        codetwo()
            .args(["pending", "--json", "--url", &url, "--token-file"])
            .arg(&token_file)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"approvals\""));
    assert!(!stdout.contains("ctmcp_"));
}

/// `codetwo pair --json` with the credential piped on stdin. Returns (exit code, stdout+stderr).
fn pair_via_stdin(url: &str, data: &Path, token: &str) -> (Option<i32>, String) {
    use std::io::Write;
    let mut child = codetwo()
        .args(["pair", "--json", "--stdin", "--url", url, "--data-dir"])
        .arg(data)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{token}\n").as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code(), text)
}

fn pair_via_file(url: &str, data: &Path, token_file: &Path) -> (Option<i32>, String) {
    let out = codetwo()
        .args(["pair", "--json", "--url", url, "--data-dir"])
        .arg(data)
        .arg("--token-file")
        .arg(token_file)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code(), text)
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[tokio::test]
async fn pair_validates_then_saves_and_later_commands_use_it() {
    let server_dir = TempDir::new().unwrap();
    let server = start_server(&server_dir, true).await;
    let url = format!("http://{}", server.addr);
    let secret = server.token.clone();
    let cli_dir = TempDir::new().unwrap();
    let source = TempDir::new().unwrap();
    let source_file = source.path().join("imported.token");
    write_token_file(&source_file, &secret).unwrap();

    let (stdin_result, file_result, status) = tokio::task::spawn_blocking({
        let (url, cli, source_file) = (url.clone(), cli_dir.path().to_path_buf(), source_file);
        move || {
            let stdin_result = pair_via_stdin(&url, &cli, &secret);
            // Re-pairing from a file replaces the saved credential.
            std::fs::remove_file(cli.join("cli.token")).unwrap();
            let file_result = pair_via_file(&url, &cli, &source_file);
            // No --url/--token-file: cli.url and cli.token carry the connection.
            let status = codetwo()
                .args(["status", "--json", "--data-dir"])
                .arg(&cli)
                .output()
                .unwrap();
            (stdin_result, file_result, status)
        }
    })
    .await
    .unwrap();

    for (code, text) in [&stdin_result, &file_result] {
        assert_eq!(*code, Some(0), "{text}");
        assert!(!text.contains("ctmcp_"), "token echoed: {text}");
        let parsed: Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(parsed["result"]["paired"], Value::Bool(true));
        assert_eq!(parsed["result"]["url"], Value::String(url.clone()));
        assert!(parsed["result"]["client_scopes"].is_array());
    }
    let saved = std::fs::read_to_string(cli_dir.path().join("cli.token")).unwrap();
    assert_eq!(saved.trim(), server.token);
    assert_eq!(
        std::fs::read_to_string(cli_dir.path().join("cli.url"))
            .unwrap()
            .trim(),
        url
    );
    #[cfg(unix)]
    assert_eq!(mode_of(&cli_dir.path().join("cli.token")), 0o600);
    assert_eq!(status.status.code(), Some(0), "{status:?}");
    assert!(!String::from_utf8_lossy(&status.stdout).contains("ctmcp_"));
}

#[tokio::test]
async fn pair_rejects_without_saving_anything() {
    let server_dir = TempDir::new().unwrap();
    let server = start_server(&server_dir, true).await;
    let url = format!("http://{}", server.addr);
    // Right shape, never issued: the server must say no and the CLI must keep nothing.
    let unknown = "ctmcp_neverissued0123456789abcdef".to_string();
    let revoked_id = server
        .engine
        .external_mcp_state()
        .with_registry(|reg| reg.list().unwrap()[0].id.clone())
        .unwrap();
    let revoked = server.token.clone();
    run_mcp_client(
        &globals(server_dir.path()),
        McpClientAction::Revoke { id: revoked_id },
    );
    let cli_dir = TempDir::new().unwrap();

    let results = tokio::task::spawn_blocking({
        let (url, cli) = (url.clone(), cli_dir.path().to_path_buf());
        let unknown = unknown.clone();
        move || {
            (
                pair_via_stdin(&url, &cli, &unknown),
                pair_via_stdin(&url, &cli, &revoked),
                // A Web UI device Bearer or session token is not an external credential.
                pair_via_stdin(&url, &cli, "web-device-bearer-0123456789"),
                pair_via_stdin(&url, &cli, "ctmcp_two words"),
            )
        }
    })
    .await
    .unwrap();

    let expected = [
        "auth_failed",
        "auth_failed",
        "invalid_token",
        "invalid_token",
    ];
    for ((code, text), want) in [&results.0, &results.1, &results.2, &results.3]
        .into_iter()
        .zip(expected)
    {
        assert_eq!(*code, Some(1), "{text}");
        let parsed: Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(parsed["error"]["code"], want, "{text}");
        for secret in [
            unknown.as_str(),
            server.token.as_str(),
            "web-device-bearer",
            "two words",
        ] {
            assert!(!text.contains(secret), "echoed {secret}: {text}");
        }
    }
    assert!(!cli_dir.path().join("cli.token").exists());
    assert!(!cli_dir.path().join("cli.url").exists());
}

#[tokio::test]
async fn pair_reports_a_disabled_surface_and_keeps_nothing() {
    let server_dir = TempDir::new().unwrap();
    let server = start_server(&server_dir, false).await;
    let url = format!("http://{}", server.addr);
    let cli_dir = TempDir::new().unwrap();
    let (code, text) = tokio::task::spawn_blocking({
        let (cli, token) = (cli_dir.path().to_path_buf(), server.token.clone());
        move || pair_via_stdin(&url, &cli, &token)
    })
    .await
    .unwrap();
    assert_eq!(code, Some(1));
    assert!(text.contains("not_enabled"), "{text}");
    assert!(!text.contains("ctmcp_"));
    assert!(!cli_dir.path().join("cli.token").exists());
}

#[test]
fn pair_refuses_unsafe_urls_before_reading_or_connecting() {
    let cli_dir = TempDir::new().unwrap();
    for url in [
        "http://192.0.2.1:4599",
        "http://localhost:80@evil.example",
        "https://user:pw@h.example",
        "https://h.example/some/path",
        "ftp://h.example",
        "not a url",
    ] {
        let (code, text) = pair_via_stdin(url, cli_dir.path(), "ctmcp_neversent0123456789");
        assert_eq!(code, Some(1), "{url}: {text}");
        assert!(!text.contains("ctmcp_"), "{url}: {text}");
        assert!(!text.contains("pw@"), "{url}: {text}");
    }
    assert!(!cli_dir.path().join("cli.token").exists());
}

#[test]
fn pair_usage_needs_url_and_one_source_and_never_a_token_argv() {
    let dir = TempDir::new().unwrap();
    let data = dir.path();
    let run = |args: &[&str]| {
        codetwo()
            .args(["pair", "--json", "--data-dir"])
            .arg(data)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    for args in [
        &["--stdin"][..],
        &["--url", "https://h.example"],
        &["--url", "https://h.example", "--stdin", "--token-file", "f"],
        &["--url", "https://h.example", "ctmcp_positional0123456789"],
        &[
            "--url",
            "https://h.example",
            "--token",
            "ctmcp_argv0123456789",
        ],
    ] {
        let out = run(args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(!String::from_utf8_lossy(&out.stdout).contains("ctmcp_"));
    }
    // With a closed stdin the credential is simply missing: an error, not a hang.
    let out = run(&["--url", "https://h.example", "--stdin"]);
    assert_eq!(out.status.code(), Some(1));
}

#[cfg(unix)]
#[test]
fn pair_token_file_must_be_private() {
    let dir = TempDir::new().unwrap();
    let token_file = dir.path().join("open.token");
    std::fs::write(&token_file, "ctmcp_loosemode0123456789\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&token_file, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    let (code, text) = pair_via_file("https://h.example", dir.path(), &token_file);
    assert_eq!(code, Some(1));
    assert!(text.contains("insecure_token_file"), "{text}");
    assert!(!text.contains("loosemode"), "{text}");
}

/// A symlink at the destination is never written through: the target keeps its content, the
/// attempt fails, and a failed pair also leaves the previous `cli.url` as it was.
#[cfg(unix)]
#[tokio::test]
async fn writes_never_follow_a_symlink_at_the_destination() {
    let outside = TempDir::new().unwrap();
    let target = outside.path().join("victim");
    std::fs::write(&target, "keep me\n").unwrap();

    let dir = TempDir::new().unwrap();
    let link = dir.path().join("link.token");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let error = write_token_file(&link, "ctmcp_secret0123456789").unwrap_err();
    assert!(error.contains("symlink"), "{error}");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep me\n");

    // `--token-out` goes through the same writer and revokes the credential it could not save.
    let out = run_mcp_client(
        &globals(dir.path()),
        McpClientAction::Create {
            name: "linked".into(),
            scopes: admin::parse_scopes(None).unwrap(),
            projects: ProjectScope::All,
            ttl: TtlChoice::Default30Days,
            token_out: Some(link.clone()),
        },
    );
    assert_eq!(out.exit, ExitCode::Error);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep me\n");

    // A pair whose cli.token is a symlink fails after validation and restores cli.url.
    let server_dir = TempDir::new().unwrap();
    let server = start_server(&server_dir, true).await;
    let url = format!("http://{}", server.addr);
    let cli_dir = TempDir::new().unwrap();
    std::os::unix::fs::symlink(&target, cli_dir.path().join("cli.token")).unwrap();
    std::fs::write(cli_dir.path().join("cli.url"), "https://old.example\n").unwrap();
    let (code, text) = tokio::task::spawn_blocking({
        let (cli, token) = (cli_dir.path().to_path_buf(), server.token.clone());
        move || pair_via_stdin(&url, &cli, &token)
    })
    .await
    .unwrap();
    assert_eq!(code, Some(1), "{text}");
    assert!(text.contains("pair_save"), "{text}");
    assert!(!text.contains("ctmcp_"));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep me\n");
    assert_eq!(
        std::fs::read_to_string(cli_dir.path().join("cli.url")).unwrap(),
        "https://old.example\n"
    );
}

#[cfg(unix)]
#[test]
fn write_token_file_replaces_a_loose_regular_file_with_a_private_one() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("t.token");
    std::fs::write(&path, "old\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
    write_token_file(&path, "ctmcp_new0123456789").unwrap();
    assert_eq!(mode_of(&path), 0o600);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "ctmcp_new0123456789\n"
    );
    // No temp file is left next to it.
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn help_documents_pair_and_error_json_never_carries_a_token() {
    let help = codetwo().arg("--help").output().unwrap();
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(text.contains("codetwo pair --url URL"));
    // The help may name the `ctmcp_...` prefix, but never anything shaped like a token.
    for (at, _) in text.match_indices("ctmcp_") {
        let next = text[at + 6..].chars().next().unwrap_or(' ');
        assert!(!next.is_ascii_alphanumeric(), "token-shaped text in help");
    }
    // A token typed into an unknown scope is redacted from the error envelope.
    let out = codetwo()
        .args([
            "mcp",
            "client",
            "create",
            "--name",
            "x",
            "--scopes",
            "ctmcp_leaky0123456789",
            "--json",
            "--data-dir",
        ])
        .arg(TempDir::new().unwrap().path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&out.stdout).contains("leaky"));
}

struct ServeChild(Child);

impl Drop for ServeChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn serve_child(data: &Path, home: &Path) -> ServeChild {
    let mut command = codetwo();
    command
        .args([
            "serve",
            "--bind",
            "127.0.0.1:0",
            "--external-mcp",
            "--data-dir",
        ])
        .arg(data)
        // Core boot must not read or write the developer's real home.
        .env("HOME", home)
        .env_remove("CODETWO_HOST")
        .stdin(Stdio::null())
        // The pairing link on stdout is a one-time secret; never capture or print it.
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    ServeChild(command.spawn().expect("spawn codetwo serve"))
}

/// One Core owner per data directory, a manifest while serving, and cleanup on SIGTERM.
#[cfg(unix)]
#[test]
fn serve_enforces_one_owner_writes_manifest_and_cleans_up() {
    let data = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let manifest = data.path().join("server.json");

    let mut first = serve_child(data.path(), home.path());
    let deadline = Instant::now() + Duration::from_secs(60);
    while !manifest.is_file() {
        assert!(Instant::now() < deadline, "server.json never appeared");
        if let Some(status) = first.0.try_wait().unwrap() {
            panic!("first serve exited early: {status}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let written: Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
    assert_eq!(written["pid"].as_u64(), Some(u64::from(first.0.id())));
    assert_eq!(written["external_mcp"], Value::Bool(true));
    assert!(written["port"].as_u64().unwrap() > 0);
    assert!(written.get("token").is_none());
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&manifest).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    // A second owner is refused and must not disturb the first one's manifest.
    let mut second = serve_child(data.path(), home.path());
    let status = second.0.wait().unwrap();
    assert_eq!(status.code(), Some(1));
    let after: Value = serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
    assert_eq!(after["pid"], written["pid"]);

    // SIGTERM stops serving gracefully: exit 0 and the manifest is removed.
    unsafe { libc::kill(first.0.id() as i32, libc::SIGTERM) };
    let status = first.0.wait().unwrap();
    assert_eq!(status.code(), Some(0));
    assert!(!manifest.exists());
}

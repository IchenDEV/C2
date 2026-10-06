use std::fs;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static SUPERVISOR_TEST_LOCK: Mutex<()> = Mutex::new(());

use codetwo_core::provider::default_registry;
use codetwo_core::skill::{builtin_skills, SkillLibrary};
use codetwo_core::Engine;
use codetwo_server::remote::{
    auth_limiter_source_key, read_tunnel_token, remote_enabled, remote_public_path_allowed,
    set_remote_enabled, set_remote_hosts, validate_public_host, RemoteConfig, RemoteMode,
    RemoteSupervisor, SupervisorState, SupervisorTiming, TokenSource,
};
use codetwo_server::{bind_and_serve, fanout, AuthState, DEFAULT_PAIRING_TTL, REMOTE_PAIRING_TTL};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn fixture_cloudflared() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_cloudflared.sh");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
    }
    path
}

/// Serialize tests that mutate the fake's process env; clear every knob first.
fn supervisor_env() -> std::sync::MutexGuard<'static, ()> {
    let guard = SUPERVISOR_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for key in [
        "FAKE_CLOUDFLARED_MODE",
        "FAKE_CLOUDFLARED_DUMP",
        "FAKE_CLOUDFLARED_EXIT_AFTER",
        "FAKE_CLOUDFLARED_PIDFILE",
        "FAKE_CLOUDFLARED_STDOUT",
        "FAKE_CLOUDFLARED_STDERR",
        "FAKE_CLOUDFLARED_DELAY",
    ] {
        std::env::remove_var(key);
    }
    set_remote_hosts(Vec::new());
    set_remote_enabled(false);
    guard
}

fn token_file(dir: &Path) -> PathBuf {
    let path = dir.join("token");
    fs::write(&path, "token-for-supervisor").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    path
}

fn spawned_pids(pidfile: &Path) -> Vec<u32> {
    fs::read_to_string(pidfile)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

/// `kill -0` succeeds for live processes and for unreaped zombies, so this also proves reaping.
fn pid_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn live_count(pidfile: &Path) -> usize {
    spawned_pids(pidfile)
        .into_iter()
        .filter(|pid| pid_alive(*pid))
        .count()
}

fn named_config(token_file: &Path, host: &str, port: u16) -> RemoteConfig {
    RemoteConfig {
        mode: RemoteMode::Named,
        cloudflared_path: Some(fixture_cloudflared()),
        remote_host: Some(host.to_string()),
        token_source: Some(TokenSource::File(token_file.to_path_buf())),
        allow_quick_tunnel: false,
        local_port: port,
    }
}

#[test]
fn config_refuses_unsafe_named_settings() {
    let cfg = RemoteConfig {
        mode: RemoteMode::Named,
        cloudflared_path: None,
        remote_host: None,
        token_source: None,
        allow_quick_tunnel: false,
        local_port: 4599,
    };
    assert!(cfg.validate().unwrap_err().contains("remote-host"));

    let cfg = RemoteConfig {
        mode: RemoteMode::Named,
        cloudflared_path: None,
        remote_host: Some("remote.example.com".into()),
        token_source: None,
        allow_quick_tunnel: false,
        local_port: 4599,
    };
    assert!(cfg.validate().unwrap_err().contains("tunnel token"));

    let cfg = RemoteConfig {
        mode: RemoteMode::Quick,
        cloudflared_path: None,
        remote_host: None,
        token_source: None,
        allow_quick_tunnel: false,
        local_port: 4599,
    };
    assert!(cfg.validate().unwrap_err().contains("allow-quick-tunnel"));

    assert!(validate_public_host("bad/host").is_err());
}

#[cfg(unix)]
#[test]
fn config_refuses_world_readable_token_file() {
    let dir = tempfile::tempdir().unwrap();
    let token_path = dir.path().join("token");
    fs::write(&token_path, "secret-token-value").unwrap();
    let mut perms = fs::metadata(&token_path).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o644);
    fs::set_permissions(&token_path, perms).unwrap();
    let cfg = named_config(&token_path, "remote.example.com", 4599);
    assert!(cfg.validate().unwrap_err().contains("0600"));
}

#[tokio::test]
async fn supervisor_spawn_uses_env_not_argv_for_token() {
    let _lock = supervisor_env();
    let dir = tempfile::tempdir().unwrap();
    let token_path = dir.path().join("token");
    fs::write(&token_path, "super-secret-tunnel-token").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&token_path).unwrap().permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&token_path, perms).unwrap();
    }
    let dump = dir.path().join("spawn-dump.txt");
    std::env::set_var("FAKE_CLOUDFLARED_DUMP", dump.display().to_string());
    std::env::remove_var("FAKE_CLOUDFLARED_EXIT_AFTER");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let config = named_config(&token_path, "remote.example.com", port);
    read_tunnel_token(config.token_source.as_ref().unwrap()).unwrap();
    let supervisor = Arc::new(
        RemoteSupervisor::new(
            config,
            SupervisorTiming {
                min_backoff: Duration::from_millis(50),
                max_backoff: Duration::from_millis(200),
                max_restarts_per_hour: 30,
            },
        )
        .unwrap(),
    );
    supervisor.start().await.unwrap();
    let dump_text = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if dump.is_file() {
                if let Ok(text) = fs::read_to_string(&dump) {
                    if text.contains("argv:") {
                        return text;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("fake cloudflared did not write spawn dump");
    assert!(dump_text.contains("has_tunnel_token:yes"));
    assert!(!dump_text.contains("super-secret-tunnel-token"));
    let argv_line = dump_text
        .lines()
        .find(|line| line.starts_with("argv:"))
        .unwrap();
    assert!(!argv_line.contains("super-secret-tunnel-token"));
    assert!(argv_line.contains("tunnel"));
    assert!(argv_line.contains("run"));

    supervisor.shutdown().await;
    std::env::remove_var("FAKE_CLOUDFLARED_DUMP");
}

#[tokio::test]
async fn supervisor_quick_mode_discovers_public_url_without_leaking_token() {
    let _lock = supervisor_env();
    let dir = tempfile::tempdir().unwrap();
    let config = RemoteConfig {
        mode: RemoteMode::Quick,
        cloudflared_path: Some(fixture_cloudflared()),
        remote_host: None,
        token_source: None,
        allow_quick_tunnel: true,
        local_port: 4599,
    };
    let supervisor = Arc::new(RemoteSupervisor::new(config, SupervisorTiming::default()).unwrap());
    supervisor.start().await.unwrap();
    let observed = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let status = supervisor.status().await;
            if status.public_url.is_some() {
                break status.public_url;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    supervisor.shutdown().await;
    assert_eq!(
        observed
            .expect("quick tunnel URL was not discovered")
            .as_deref(),
        Some("https://fake-quick.trycloudflare.com")
    );
    let _ = dir;
}

#[tokio::test]
async fn supervisor_restarts_with_backoff_and_respects_hourly_cap() {
    let _lock = supervisor_env();
    std::env::set_var("FAKE_CLOUDFLARED_MODE", "exit_immediately");
    let dir = tempfile::tempdir().unwrap();
    let token_path = dir.path().join("token");
    fs::write(&token_path, "token-for-restart").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&token_path).unwrap().permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&token_path, perms).unwrap();
    }
    let config = named_config(&token_path, "remote.example.com", 4599);
    let supervisor = Arc::new(
        RemoteSupervisor::new(
            config,
            SupervisorTiming {
                min_backoff: Duration::from_millis(30),
                max_backoff: Duration::from_millis(120),
                max_restarts_per_hour: 2,
            },
        )
        .unwrap(),
    );
    let pidfile = dir.path().join("pids");
    std::env::set_var("FAKE_CLOUDFLARED_PIDFILE", &pidfile);
    supervisor.start().await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    let status = supervisor.status().await;
    // initial spawn + 2 allowed restarts, then the hourly cap stops the supervisor for good.
    assert_eq!(status.restarts, 2);
    assert_eq!(status.state, SupervisorState::Stopped);
    assert_eq!(spawned_pids(&pidfile).len(), 3);
    supervisor.shutdown().await;
    assert_eq!(live_count(&pidfile), 0);
}

fn timing(min_ms: u64, max_ms: u64) -> SupervisorTiming {
    SupervisorTiming {
        min_backoff: Duration::from_millis(min_ms),
        max_backoff: Duration::from_millis(max_ms),
        max_restarts_per_hour: 1000,
    }
}

#[tokio::test]
async fn supervisor_keeps_exactly_one_live_child_and_shutdown_reaps_it() {
    let _lock = supervisor_env();
    let dir = tempfile::tempdir().unwrap();
    let pidfile = dir.path().join("pids");
    std::env::set_var("FAKE_CLOUDFLARED_PIDFILE", &pidfile);
    let config = named_config(&token_file(dir.path()), "remote.example.com", 4599);
    let supervisor = Arc::new(RemoteSupervisor::new(config, timing(20, 40)).unwrap());
    supervisor.start().await.unwrap();
    // Backoff elapses many times over; a healthy child must never be duplicated.
    for _ in 0..15 {
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(live_count(&pidfile), 1);
    }
    assert_eq!(
        spawned_pids(&pidfile).len(),
        1,
        "healthy child was respawned"
    );
    assert_eq!(supervisor.status().await.state, SupervisorState::Running);

    supervisor.shutdown().await;
    assert_eq!(live_count(&pidfile), 0, "child still alive after shutdown");
    assert_eq!(supervisor.status().await.state, SupervisorState::Stopped);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(spawned_pids(&pidfile).len(), 1, "respawned after shutdown");
    assert!(
        supervisor.start().await.is_err(),
        "restart after shutdown must be refused"
    );
}

#[tokio::test]
async fn supervisor_restarts_after_exit_with_single_live_child() {
    let _lock = supervisor_env();
    let dir = tempfile::tempdir().unwrap();
    let pidfile = dir.path().join("pids");
    std::env::set_var("FAKE_CLOUDFLARED_PIDFILE", &pidfile);
    std::env::set_var("FAKE_CLOUDFLARED_EXIT_AFTER", "0.15");
    let config = named_config(&token_file(dir.path()), "remote.example.com", 4599);
    let supervisor = Arc::new(RemoteSupervisor::new(config, timing(20, 40)).unwrap());
    supervisor.start().await.unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    while spawned_pids(&pidfile).len() < 3 {
        assert!(
            Instant::now() < deadline,
            "supervisor did not restart the child"
        );
        assert!(live_count(&pidfile) <= 1, "more than one live child");
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    assert!(supervisor.status().await.restarts >= 2);

    supervisor.shutdown().await;
    assert_eq!(live_count(&pidfile), 0);
    let spawned = spawned_pids(&pidfile).len();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        spawned_pids(&pidfile).len(),
        spawned,
        "respawned after shutdown"
    );
}

#[tokio::test]
async fn shutdown_during_backoff_stops_without_respawn() {
    let _lock = supervisor_env();
    let dir = tempfile::tempdir().unwrap();
    let pidfile = dir.path().join("pids");
    std::env::set_var("FAKE_CLOUDFLARED_PIDFILE", &pidfile);
    std::env::set_var("FAKE_CLOUDFLARED_MODE", "exit_immediately");
    let config = named_config(&token_file(dir.path()), "remote.example.com", 4599);
    let supervisor = Arc::new(RemoteSupervisor::new(config, timing(5_000, 5_000)).unwrap());
    supervisor.start().await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(supervisor.status().await.state, SupervisorState::Backoff);
    tokio::time::timeout(Duration::from_secs(2), supervisor.shutdown())
        .await
        .expect("shutdown must interrupt backoff promptly");
    assert_eq!(spawned_pids(&pidfile).len(), 1);
    assert_eq!(live_count(&pidfile), 0);
}

#[test]
fn limiter_key_trusts_cf_header_only_from_loopback_tunnel_peer() {
    let _lock = supervisor_env();
    set_remote_hosts(vec!["remote.example.com".into()]);
    let mut tunnel = axum::http::HeaderMap::new();
    tunnel.insert("host", "remote.example.com".parse().unwrap());
    tunnel.insert("cf-connecting-ip", "203.0.113.10".parse().unwrap());
    let loopback: SocketAddr = "127.0.0.1:50000".parse().unwrap();
    let lan: SocketAddr = "192.168.1.50:50000".parse().unwrap();

    assert_eq!(
        auth_limiter_source_key(&tunnel, loopback),
        "cf:203.0.113.10"
    );
    // A LAN client spoofing the tunnel Host and CF header must be keyed by its real address.
    assert_eq!(auth_limiter_source_key(&tunnel, lan), "peer:192.168.1.50");

    let mut local_host = tunnel.clone();
    local_host.insert("host", "localhost".parse().unwrap());
    assert_eq!(
        auth_limiter_source_key(&local_host, loopback),
        "peer:127.0.0.1"
    );
    let mut other_host = tunnel.clone();
    other_host.insert("host", "evil.example.com".parse().unwrap());
    assert_eq!(
        auth_limiter_source_key(&other_host, loopback),
        "peer:127.0.0.1"
    );
    let mut junk = tunnel.clone();
    junk.insert("cf-connecting-ip", "not-an-ip".parse().unwrap());
    assert_eq!(auth_limiter_source_key(&junk, loopback), "peer:127.0.0.1");

    set_remote_hosts(Vec::new());
}

async fn raw_request(
    addr: SocketAddr,
    host: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
    extra_headers: &str,
) -> (u16, String) {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let payload = body.unwrap_or("");
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n{extra_headers}Content-Length: {}\r\n\r\n{payload}",
        payload.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or_default();
    (status, text)
}

#[tokio::test]
async fn remote_host_middleware_enforces_allowlist() {
    let _lock = supervisor_env();
    set_remote_hosts(vec!["remote.example.com".into()]);
    set_remote_enabled(true);
    let auth = Arc::new(AuthState::load(None));
    let (engine, rx) = Engine::new(default_registry(), SkillLibrary::new(builtin_skills()));
    let (addr, handle) = bind_and_serve(
        Arc::new(engine),
        fanout(rx),
        "127.0.0.1:0".parse().unwrap(),
        auth,
    )
    .await
    .unwrap();

    let (status, _) = raw_request(addr, "remote.example.com", "GET", "/health", None, "").await;
    assert_eq!(status, 200);

    let (status, _) = raw_request(
        addr,
        "remote.example.com",
        "GET",
        "/api/team/v1/tasks",
        None,
        "",
    )
    .await;
    assert_eq!(status, 404);

    let client = reqwest::Client::new();
    for path in ["/api/pair", "/api/ws-ticket"] {
        let response = client
            .request(reqwest::Method::OPTIONS, format!("http://{addr}{path}"))
            .header("host", "remote.example.com")
            .header("origin", "https://client.example.com")
            .header("access-control-request-method", "POST")
            .header(
                "access-control-request-headers",
                "authorization,content-type",
            )
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.headers()["access-control-allow-origin"], "*");
    }
    let rejected = client
        .get(format!("http://{addr}/api/devices"))
        .header("host", "remote.example.com")
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), reqwest::StatusCode::NOT_FOUND);
    assert!(rejected.headers()["cache-control"]
        .to_str()
        .unwrap()
        .contains("no-store"));

    let (status, _) = raw_request(addr, "evil.example.com", "GET", "/health", None, "").await;
    assert!(status == 421 || status == 403);

    set_remote_hosts(Vec::new());
    set_remote_enabled(false);
    handle.abort();
}

#[tokio::test]
async fn remote_auth_limiter_locks_out_pairing_failures() {
    let _lock = supervisor_env();
    set_remote_hosts(vec!["remote.example.com".into()]);
    set_remote_enabled(true);
    let auth = Arc::new(AuthState::load(None));
    auth.set_remote_hardening(true);
    let (engine, rx) = Engine::new(default_registry(), SkillLibrary::new(builtin_skills()));
    let (addr, handle) = bind_and_serve(
        Arc::new(engine),
        fanout(rx),
        "127.0.0.1:0".parse().unwrap(),
        auth,
    )
    .await
    .unwrap();

    let body = r#"{"token":"bad","device_name":"x"}"#;
    let mut saw_429 = false;
    for _ in 0..12 {
        let (status, response) = raw_request(
            addr,
            "remote.example.com",
            "POST",
            "/api/pair",
            Some(body),
            "Content-Type: application/json\r\nCF-Connecting-IP: 203.0.113.10\r\n",
        )
        .await;
        if status == 429 {
            saw_429 = true;
            assert!(response.to_ascii_lowercase().contains("retry-after"));
            break;
        }
    }
    assert!(saw_429, "expected lockout after repeated pairing failures");

    set_remote_hosts(Vec::new());
    set_remote_enabled(false);
    handle.abort();
}

#[test]
fn remote_pairing_ttl_shortens_when_hardening_enabled() {
    let auth = AuthState::load(None);
    assert_eq!(
        auth.effective_pairing_ttl(DEFAULT_PAIRING_TTL),
        DEFAULT_PAIRING_TTL
    );
    auth.set_remote_hardening(true);
    assert_eq!(
        auth.effective_pairing_ttl(DEFAULT_PAIRING_TTL),
        REMOTE_PAIRING_TTL
    );
}

#[test]
fn path_allowlist_matches_specified_routes() {
    assert!(remote_public_path_allowed("POST", "/external-mcp"));
    assert!(remote_public_path_allowed("OPTIONS", "/api/pair"));
    assert!(remote_public_path_allowed("OPTIONS", "/api/ws-ticket"));
    assert!(!remote_public_path_allowed("GET", "/api/pair"));
    assert!(!remote_public_path_allowed("OPTIONS", "/api/devices"));
    assert!(!remote_public_path_allowed("POST", "/mcp"));
    assert!(!remote_public_path_allowed("GET", "/terminal"));
}

async fn serve_for_guard() -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let auth = Arc::new(AuthState::load(None));
    let (engine, rx) = Engine::new(default_registry(), SkillLibrary::new(builtin_skills()));
    bind_and_serve(
        Arc::new(engine),
        fanout(rx),
        "127.0.0.1:0".parse().unwrap(),
        auth,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn quick_tunnel_denies_unknown_host_until_stderr_url_then_ignores_overrides() {
    let _lock = supervisor_env();
    let (addr, server) = serve_for_guard().await;
    // Guard is off before remote access starts: today's loopback behaviour is unchanged.
    assert!(!remote_enabled());
    let (status, _) =
        raw_request(addr, "random.trycloudflare.com", "GET", "/health", None, "").await;
    assert_eq!(status, 200);

    // cloudflared logs to stderr; the URL appears late, after cloudflared's own API URL, and a
    // second "tunnel" URL follows that must not replace the first.
    std::env::set_var("FAKE_CLOUDFLARED_DELAY", "0.8");
    std::env::set_var(
        "FAKE_CLOUDFLARED_STDERR",
        "INF Requesting new quick Tunnel on https://api.trycloudflare.com/tunnel\n\
         |  https://calm-river-0a1b.trycloudflare.com  |\n\
         INF later line https://evil-override.trycloudflare.com",
    );
    let config = RemoteConfig {
        mode: RemoteMode::Quick,
        cloudflared_path: Some(fixture_cloudflared()),
        remote_host: None,
        token_source: None,
        allow_quick_tunnel: true,
        local_port: addr.port(),
    };
    let supervisor = Arc::new(RemoteSupervisor::new(config, SupervisorTiming::default()).unwrap());
    supervisor.start().await.unwrap();
    assert!(remote_enabled(), "guard must be on once the tunnel spawns");

    // Hostname unknown yet: every non-loopback Host is denied, loopback keeps working.
    assert!(supervisor.status().await.public_url.is_none());
    for host in [
        "calm-river-0a1b.trycloudflare.com",
        "evil.example.com",
        "api.trycloudflare.com",
    ] {
        let (status, _) = raw_request(addr, host, "GET", "/health", None, "").await;
        assert_eq!(
            status, 421,
            "{host} must be denied while the tunnel host is unknown"
        );
    }
    let (status, _) = raw_request(addr, "127.0.0.1", "GET", "/health", None, "").await;
    assert_eq!(status, 200);

    let deadline = Instant::now() + Duration::from_secs(5);
    while supervisor.status().await.public_url.is_none() {
        assert!(Instant::now() < deadline, "stderr URL was never parsed");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await; // let the later lines drain
    assert_eq!(
        supervisor.status().await.public_url.as_deref(),
        Some("https://calm-river-0a1b.trycloudflare.com")
    );
    let (status, _) = raw_request(
        addr,
        "calm-river-0a1b.trycloudflare.com",
        "GET",
        "/health",
        None,
        "",
    )
    .await;
    assert_eq!(status, 200);
    for host in ["evil-override.trycloudflare.com", "api.trycloudflare.com"] {
        let (status, _) = raw_request(addr, host, "GET", "/health", None, "").await;
        assert_eq!(status, 421, "{host} must not be registered");
    }

    supervisor.shutdown().await;
    assert!(!remote_enabled());
    server.abort();
}

#[tokio::test]
async fn quick_mode_does_not_override_configured_named_hosts() {
    let _lock = supervisor_env();
    let dir = tempfile::tempdir().unwrap();
    let config = named_config(&token_file(dir.path()), "remote.example.com", 4599);
    let supervisor = Arc::new(RemoteSupervisor::new(config, timing(20, 40)).unwrap());
    supervisor
        .ingest_line_for_tests("https://override.trycloudflare.com")
        .await;
    assert!(supervisor.status().await.public_url.is_none());
    assert_eq!(
        codetwo_server::remote::allowed_remote_hosts(),
        vec!["remote.example.com".to_string()]
    );
    set_remote_hosts(Vec::new());
}

#[tokio::test]
async fn tunnel_host_cannot_reach_dotted_api_or_metadata_routes() {
    let _lock = supervisor_env();
    set_remote_hosts(vec!["remote.example.com".into()]);
    set_remote_enabled(true);
    let (addr, server) = serve_for_guard().await;

    for path in [
        "/.well-known/t3/environment",
        "/api/auth/session.json",
        "/api/orchestration/shell",
        "/api/v1.0/x.js",
        "/oauth/token.js",
        "/./favicon.ico",
        "/assets/../.well-known/t3/environment",
        "/%2e%2e/.well-known/t3/environment",
        "/.well-known/anything.js",
    ] {
        let (status, body) = raw_request(addr, "remote.example.com", "GET", path, None, "").await;
        assert_eq!(status, 404, "{path} leaked through the tunnel: {body}");
    }
    // A local client (loopback Host, no edge headers) still reaches the metadata route.
    let (status, _) = raw_request(
        addr,
        "127.0.0.1",
        "GET",
        "/.well-known/t3/environment",
        None,
        "",
    )
    .await;
    assert_ne!(status, 404);

    set_remote_hosts(Vec::new());
    set_remote_enabled(false);
    server.abort();
}

#[tokio::test]
async fn cloudflare_edge_headers_with_localhost_host_get_public_paths_only() {
    let _lock = supervisor_env();
    set_remote_hosts(vec!["remote.example.com".into()]);
    set_remote_enabled(true);
    let (addr, server) = serve_for_guard().await;

    // httpHostHeader=localhost ingress: Host looks local, but the CF edge headers give it away.
    for header in [
        "CF-Ray: 8a1b2c3d4e5f-AMS\r\n",
        "CF-Connecting-IP: 203.0.113.9\r\n",
    ] {
        let (status, _) = raw_request(
            addr,
            "localhost",
            "GET",
            "/.well-known/t3/environment",
            None,
            header,
        )
        .await;
        assert_eq!(status, 404, "{header}");
        let (status, _) =
            raw_request(addr, "localhost", "GET", "/api/team/v1/tasks", None, header).await;
        assert_eq!(status, 404, "{header}");
        let (status, _) = raw_request(addr, "localhost", "GET", "/health", None, header).await;
        assert_eq!(status, 200, "{header}");
    }

    set_remote_hosts(Vec::new());
    set_remote_enabled(false);
    server.abort();
}

#[cfg(unix)]
#[test]
fn token_file_rejects_non_regular_oversized_and_empty_without_leaking() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let secure =
        |path: &Path| fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();

    let fifo = dir.path().join("fifo");
    assert!(std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    secure(&fifo);
    let error = read_tunnel_token(&TokenSource::File(fifo)).unwrap_err();
    assert!(error.contains("regular file"), "{error}");

    let big = dir.path().join("big");
    fs::write(&big, "secret-".repeat(4096)).unwrap();
    secure(&big);
    let error = read_tunnel_token(&TokenSource::File(big)).unwrap_err();
    assert!(
        error.contains("too large") && !error.contains("secret-"),
        "{error}"
    );

    let empty = dir.path().join("empty");
    fs::write(&empty, " \n").unwrap();
    secure(&empty);
    assert!(read_tunnel_token(&TokenSource::File(empty)).is_err());

    let missing = dir.path().join("missing");
    assert!(read_tunnel_token(&TokenSource::File(missing)).is_err());

    // Validation and read use the same checked handle: the happy path still works and trims.
    let good = dir.path().join("good");
    fs::write(&good, "abc123\n").unwrap();
    secure(&good);
    assert_eq!(
        read_tunnel_token(&TokenSource::File(good)).unwrap(),
        "abc123"
    );
}

fn serve_config(data_dir: &Path, remote_args: Vec<String>) -> codetwo_server::serve::ServeConfig {
    codetwo_server::serve::ServeConfig {
        surface: codetwo_server::serve::ServeSurface::Compact,
        ui_dir: None,
        data_dir: Some(data_dir.to_path_buf()),
        open_browser: false,
        host: "127.0.0.1".into(),
        port: 0,
        external_mcp: false,
        remote_args,
    }
}

/// `serve::run` boots Core, which reads HOME-relative user state: point HOME at a temp dir.
struct HomeGuard(Option<std::ffi::OsString>);
impl HomeGuard {
    fn set(home: &Path) -> Self {
        let previous = std::env::var_os("HOME");
        std::env::set_var("HOME", home);
        Self(previous)
    }
}
impl Drop for HomeGuard {
    fn drop(&mut self) {
        match self.0.take() {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn serve_supervisor_setup_failure_leaves_no_manifest_and_guard_off() {
    let _lock = supervisor_env();
    let root = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(root.path());
    let data = root.path().join("data");
    let token = token_file(root.path());
    // A regular but non-executable "cloudflared": resolves, then fails to spawn.
    let not_exec = root.path().join("cloudflared");
    fs::write(&not_exec, "#!/bin/sh\n").unwrap();

    let error = codetwo_server::serve::run(serve_config(
        &data,
        vec![
            "--remote".into(),
            "cloudflared".into(),
            "--remote-host".into(),
            "remote.example.com".into(),
            "--tunnel-token-file".into(),
            token.display().to_string(),
            "--cloudflared-path".into(),
            not_exec.display().to_string(),
        ],
    ))
    .await
    .unwrap_err();
    assert!(error.contains("spawn"), "{error}");
    assert!(!data.join("server.json").exists(), "manifest leaked");
    assert!(!remote_enabled());
    assert!(codetwo_server::remote::allowed_remote_hosts().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn serve_abort_removes_manifest_and_stops_only_its_tunnel_child() {
    let _lock = supervisor_env();
    let root = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(root.path());
    let data = root.path().join("data");
    let pidfile = root.path().join("pids");
    std::env::set_var("FAKE_CLOUDFLARED_PIDFILE", &pidfile);
    std::env::set_var(
        "FAKE_CLOUDFLARED_STDERR",
        "|  https://abort-test.trycloudflare.com  |",
    );

    let args = vec![
        "--allow-quick-tunnel".to_string(),
        "--cloudflared-path".into(),
        fixture_cloudflared().display().to_string(),
    ];
    let task = tokio::spawn(codetwo_server::serve::run(serve_config(&data, args)));

    let manifest = data.join("server.json");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !manifest.is_file() || live_count(&pidfile) != 1 {
        assert!(
            Instant::now() < deadline,
            "serve did not publish manifest and tunnel"
        );
        assert!(!task.is_finished(), "serve exited early");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(remote_enabled());

    // Simulates every non-graceful exit: the future is dropped, RAII must undo everything.
    task.abort();
    let _ = task.await;
    assert!(!manifest.exists(), "manifest leaked after abort");
    assert!(!remote_enabled());
    let deadline = Instant::now() + Duration::from_secs(5);
    while live_count(&pidfile) != 0 {
        assert!(Instant::now() < deadline, "tunnel child outlived serve");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[test]
fn quick_config_validates_optional_remote_host_like_named() {
    let quick = |host: &str| RemoteConfig {
        mode: RemoteMode::Quick,
        cloudflared_path: None,
        remote_host: Some(host.to_string()),
        token_source: None,
        allow_quick_tunnel: true,
        local_port: 4599,
    };
    for bad in [
        "bad/host",
        "host:443",
        "user@host",
        "",
        "sp ace.example.com",
    ] {
        assert!(quick(bad).validate().is_err(), "{bad}");
    }
    assert!(quick("remote.example.com").validate().is_ok());
}

#[tokio::test]
async fn quick_tunnel_does_not_inherit_tunnel_token_from_our_env() {
    let _lock = supervisor_env();
    let dir = tempfile::tempdir().unwrap();
    let dump = dir.path().join("spawn-dump.txt");
    std::env::set_var("FAKE_CLOUDFLARED_DUMP", &dump);
    std::env::set_var("TUNNEL_TOKEN", "inherited-secret-token");
    let config = RemoteConfig {
        mode: RemoteMode::Quick,
        cloudflared_path: Some(fixture_cloudflared()),
        remote_host: None,
        token_source: None,
        allow_quick_tunnel: true,
        local_port: 4599,
    };
    let supervisor = Arc::new(RemoteSupervisor::new(config, SupervisorTiming::default()).unwrap());
    supervisor.start().await.unwrap();
    let text = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(text) = fs::read_to_string(&dump) {
                if text.contains("has_tunnel_token:") {
                    return text;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    supervisor.shutdown().await;
    std::env::remove_var("TUNNEL_TOKEN");
    let text = text.expect("fake cloudflared did not write spawn dump");
    assert!(text.contains("has_tunnel_token:no"), "{text}");
    assert!(!text.contains("inherited-secret-token"));
}

#[cfg(unix)]
#[tokio::test]
async fn serve_sighup_cleans_up_manifest_tunnel_and_guard() {
    use std::os::unix::fs::PermissionsExt;
    let _lock = supervisor_env();
    let root = tempfile::tempdir().unwrap();
    let _home = HomeGuard::set(root.path());
    let data = root.path().join("data");
    let pidfile = root.path().join("pids");
    std::env::set_var("FAKE_CLOUDFLARED_PIDFILE", &pidfile);
    std::env::set_var(
        "FAKE_CLOUDFLARED_STDERR",
        "|  https://hup-test.trycloudflare.com  |",
    );
    let args = vec![
        "--allow-quick-tunnel".to_string(),
        "--cloudflared-path".into(),
        fixture_cloudflared().display().to_string(),
    ];
    let task = tokio::spawn(codetwo_server::serve::run(serve_config(&data, args)));

    let manifest = data.join("server.json");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !manifest.is_file() || live_count(&pidfile) != 1 {
        assert!(Instant::now() < deadline, "serve did not start");
        assert!(!task.is_finished(), "serve exited early");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // Created private, not chmod-ed afterwards.
    assert_eq!(
        fs::metadata(&manifest).unwrap().permissions().mode() & 0o777,
        0o600
    );

    // Handlers are installed before the tunnel spawned, so this is a handled signal, not a kill.
    // SAFETY: plain kill(2) on our own pid.
    assert_eq!(unsafe { libc::kill(libc::getpid(), libc::SIGHUP) }, 0);
    let result = tokio::time::timeout(Duration::from_secs(15), task)
        .await
        .expect("serve ignored SIGHUP")
        .unwrap();
    assert!(result.is_ok(), "{result:?}");
    assert!(!manifest.exists(), "manifest leaked after SIGHUP");
    assert!(!remote_enabled());
    let deadline = Instant::now() + Duration::from_secs(5);
    while live_count(&pidfile) != 0 {
        assert!(Instant::now() < deadline, "tunnel child outlived serve");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

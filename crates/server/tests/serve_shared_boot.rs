//! `codetwo-server` and `codetwo serve` share one Core boot path: one owner per data directory,
//! `codetwo-server pair` reaches either owner, graceful shutdown releases the directory, and the
//! compact no-argument server keeps its baseline (no data-directory claim).
#![cfg(unix)]

use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Isolate a launched binary from the developer's home, data directory, network and tunnel
/// settings. None of these tests may start a real tunnel: no `--remote` flag is ever passed.
fn isolate(command: &mut Command, home: &Path) {
    for name in [
        "CODETWO_HOST",
        "CODETWO_PORT",
        "CODETWO_PUBLIC_URL",
        "CODETWO_DATA_DIR",
        "CODETWO_WEB_UI_DIR",
        "CODETWO_EXTERNAL_MCP",
        "CODETWO_PAIR_TTL",
        "CODETWO_URL",
        "CODETWO_TOKEN",
        "CODETWO_CLOUDFLARED_PATH",
        "CODETWO_TUNNEL_TOKEN_FILE",
    ] {
        command.env_remove(name);
    }
    command.env("HOME", home).stdin(Stdio::null());
}

fn server_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_codetwo-server"))
}

fn codetwo_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_codetwo"))
}

/// The pairing link on stdout is a one-time secret: never capture or print it.
fn spawn_quiet(mut command: Command) -> Running {
    command.stdout(Stdio::null()).stderr(Stdio::null());
    Running(command.spawn().expect("spawn server binary"))
}

fn wait_for_manifest(data: &Path, child: &mut Running) -> Value {
    let manifest = data.join("server.json");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(text) = std::fs::read_to_string(&manifest) {
            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                return value;
            }
        }
        assert!(Instant::now() < deadline, "server.json never appeared");
        if let Some(status) = child.0.try_wait().unwrap() {
            panic!("server exited early: {status}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn pid_file(data: &Path) -> Option<u32> {
    std::fs::read_to_string(data.join("server.pid"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Ask the owner of `data` for a fresh link; only its shape is returned to the assertions.
fn request_link(data: &Path, home: &Path) -> Result<String, String> {
    let mut command = server_binary();
    isolate(&mut command, home);
    let output = command
        .arg("pair")
        .arg("--data-dir")
        .arg(data)
        .output()
        .unwrap();
    if output.status.success() {
        Ok(String::from_utf8(output.stdout).unwrap().trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

fn terminate_and_wait(child: &mut Running) -> Option<i32> {
    unsafe { libc::kill(child.0.id() as i32, libc::SIGTERM) };
    child.0.wait().unwrap().code()
}

/// A second owner is refused with the ownership message and leaves the first one's files alone.
fn assert_refused(mut command: Command, data: &Path, home: &Path) {
    isolate(&mut command, home);
    let output = command.stdout(Stdio::null()).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("already owned"), "{stderr}");
    assert!(data.join("server.json").is_file());
}

#[test]
fn daemon_and_codetwo_serve_share_one_owner_per_data_directory() {
    let data = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let mut daemon = server_binary();
    isolate(&mut daemon, home.path());
    daemon
        .args(["serve", "--port", "0", "--data-dir"])
        .arg(data.path())
        // External MCP is enabled by the same shared path the unified CLI uses.
        .env("CODETWO_EXTERNAL_MCP", "1");
    // A remote daemon launched with nohup must retain its inherited SIGHUP ignore policy.
    use std::os::unix::process::CommandExt;
    unsafe {
        daemon.pre_exec(|| {
            if libc::signal(libc::SIGHUP, libc::SIG_IGN) == libc::SIG_ERR {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut daemon = spawn_quiet(daemon);
    let manifest = wait_for_manifest(data.path(), &mut daemon);

    let port = manifest["port"].as_u64().unwrap() as u16;
    assert!(port > 0);
    assert_eq!(manifest["pid"].as_u64(), Some(u64::from(daemon.0.id())));
    assert_eq!(manifest["external_mcp"], Value::Bool(true));
    assert_eq!(pid_file(data.path()), Some(daemon.0.id()));
    // Loopback is the daemon default.
    TcpStream::connect(("127.0.0.1", port)).expect("daemon listens on loopback");

    // Pairing IPC and the loopback default advertised address survive the shared boot path.
    let link = request_link(data.path(), home.path()).expect("daemon answers pair");
    assert!(
        link.starts_with(&format!("http://127.0.0.1:{port}")),
        "unexpected pairing base"
    );

    // Neither launcher can take the directory while the daemon owns it.
    let mut second_daemon = server_binary();
    second_daemon
        .args(["serve", "--port", "0", "--data-dir"])
        .arg(data.path());
    assert_refused(second_daemon, data.path(), home.path());
    let mut unified = codetwo_binary();
    unified
        .args(["serve", "--bind", "127.0.0.1:0", "--data-dir"])
        .arg(data.path());
    assert_refused(unified, data.path(), home.path());
    assert_eq!(pid_file(data.path()), Some(daemon.0.id()));
    let after: Value =
        serde_json::from_str(&std::fs::read_to_string(data.path().join("server.json")).unwrap())
            .unwrap();
    assert_eq!(after["pid"], manifest["pid"]);

    unsafe {
        assert_eq!(libc::kill(daemon.0.id() as i32, libc::SIGHUP), 0);
    }
    // Successful pairing after SIGHUP proves the inherited ignore policy survived boot.
    request_link(data.path(), home.path()).expect("nohup daemon remains available after SIGHUP");

    // Graceful shutdown releases the claim and the manifest.
    assert_eq!(terminate_and_wait(&mut daemon), Some(0));
    assert!(!data.path().join("server.json").exists());
    assert!(!data.path().join("server.pid").exists());
}

#[test]
fn codetwo_serve_holds_the_claim_and_answers_pair_requests() {
    let data = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let mut unified = codetwo_binary();
    isolate(&mut unified, home.path());
    unified
        .args([
            "serve",
            "--bind",
            "127.0.0.1:0",
            "--external-mcp",
            "--data-dir",
        ])
        .arg(data.path());
    let mut unified = spawn_quiet(unified);
    let manifest = wait_for_manifest(data.path(), &mut unified);
    assert_eq!(manifest["external_mcp"], Value::Bool(true));
    assert_eq!(pid_file(data.path()), Some(unified.0.id()));

    // An unhandled SIGUSR1 would kill the process; it must answer with a link instead.
    let port = manifest["port"].as_u64().unwrap();
    let link = request_link(data.path(), home.path()).expect("codetwo serve answers pair");
    assert!(link.starts_with(&format!("http://127.0.0.1:{port}")));
    assert!(unified.0.try_wait().unwrap().is_none());

    assert_eq!(terminate_and_wait(&mut unified), Some(0));
    assert!(!data.path().join("server.json").exists());
    assert!(!data.path().join("server.pid").exists());
}

#[test]
fn compact_server_keeps_its_baseline_and_claims_nothing() {
    let data = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let mut compact = server_binary();
    isolate(&mut compact, home.path());
    compact
        .env("CODETWO_HOST", "127.0.0.1")
        .env("CODETWO_PORT", "0")
        .env("CODETWO_DATA_DIR", data.path());
    let mut compact = spawn_quiet(compact);
    let manifest = wait_for_manifest(data.path(), &mut compact);

    // No data-directory claim, no external MCP unless asked, nothing to pair through.
    assert_eq!(manifest["external_mcp"], Value::Bool(false));
    assert_eq!(pid_file(data.path()), None);
    let error = request_link(data.path(), home.path()).unwrap_err();
    assert!(error.contains("no running C2 server"), "{error}");
    assert!(compact.0.try_wait().unwrap().is_none());

    assert_eq!(terminate_and_wait(&mut compact), Some(0));
    assert!(!data.path().join("server.json").exists());
}

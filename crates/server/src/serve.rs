//! Shared headless server startup for `codetwo-server` and `codetwo serve`.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use codetwo_core::external_mcp::state::EXTERNAL_MCP_ENV;
use codetwo_core::plugins::{
    AppConfig, CanvasService, CoreApp, EngineService, EventBus, PluginManager, StoreService,
};
use serde::{Deserialize, Serialize};

use crate::cli::config::write_private_file;
use crate::remote::{RemoteConfig, RemoteMode, RemoteSupervisor, SupervisorTiming};
use crate::{
    bind_and_serve_with_canvas, bind_and_serve_with_web_ui, pairing_endpoints,
    pairing_url_for_endpoint, print_pairing, remote, AuthState, KernelWebUiCommands,
    DEFAULT_PAIRING_TTL,
};

const SERVER_MANIFEST: &str = "server.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeSurface {
    Compact,
    WebUi,
}

#[derive(Debug, Clone)]
pub struct ServeConfig {
    pub surface: ServeSurface,
    pub ui_dir: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
    pub open_browser: bool,
    pub host: String,
    pub port: u16,
    /// When true, enable external MCP before boot (also honors `CODETWO_EXTERNAL_MCP`).
    pub external_mcp: bool,
    /// Extra argv (after subcommand) for [`RemoteConfig::from_env_and_args`].
    pub remote_args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerManifest {
    pub pid: u32,
    pub port: u16,
    pub version: String,
    pub external_mcp: bool,
}

pub fn default_data_dir() -> PathBuf {
    let home = codetwo_core::provider::home_dir().unwrap_or_else(std::env::temp_dir);
    home.join(".codetwo")
}

pub fn resolve_data_dir(explicit: Option<PathBuf>) -> PathBuf {
    explicit
        .or_else(|| std::env::var_os("CODETWO_DATA_DIR").map(PathBuf::from))
        .unwrap_or_else(default_data_dir)
}

pub fn resolve_ui_dir(
    explicit: Option<PathBuf>,
    configured: Option<PathBuf>,
    executable: &Path,
) -> Result<PathBuf, String> {
    let candidate = explicit
        .or(configured)
        .or_else(|| executable.parent().map(|parent| parent.join("web-ui")))
        .ok_or_else(|| "cannot resolve the C2 Web UI directory".to_string())?;
    if !candidate.join("index.html").is_file() {
        return Err(format!(
            "C2 Web UI assets are missing at {}. Run ./script/build/hosts.sh release or pass --ui-dir <path>.",
            candidate.display()
        ));
    }
    candidate.canonicalize().map_err(|error| {
        format!(
            "cannot resolve C2 Web UI assets at {}: {error}",
            candidate.display()
        )
    })
}

fn local_pairing_url(port: u16, pairing_token: &str) -> String {
    let endpoints = pairing_endpoints(port);
    let endpoint = endpoints
        .iter()
        .find(|endpoint| endpoint.id == "loopback")
        .or_else(|| endpoints.first())
        .expect("pairing endpoints include loopback");
    pairing_url_for_endpoint(&endpoint.url, pairing_token)
}

fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    command.spawn().map(|_| ())
}

/// Enable the external MCP surface (flag or `CODETWO_EXTERNAL_MCP`). Returns whether it is
/// actually serving: a credential-store failure leaves it off (fail closed).
fn wire_external_mcp(engine: &codetwo_core::Engine, data_dir: &Path, flag_enabled: bool) -> bool {
    let env_enabled = std::env::var(EXTERNAL_MCP_ENV)
        .ok()
        .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
    if !flag_enabled && !env_enabled {
        return false;
    }
    engine.external_mcp_state().set_enabled(true);
    match engine.external_mcp_state().configure_data_dir(data_dir) {
        Ok(()) => {
            println!("external MCP enabled (POST /external-mcp on this server)");
            true
        }
        Err(error) => {
            engine.external_mcp_state().set_enabled(false);
            eprintln!("external MCP not started: {error}");
            false
        }
    }
}

fn write_server_manifest(data_dir: &Path, port: u16, external_mcp: bool) -> Result<(), String> {
    let manifest = ServerManifest {
        pid: std::process::id(),
        port,
        version: env!("CARGO_PKG_VERSION").to_string(),
        external_mcp,
    };
    let path = data_dir.join(SERVER_MANIFEST);
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    // Private temp file (0600 from creation) renamed into place: never world-readable, never partial.
    write_private_file(&path, &json).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Remove `server.json` only if this process wrote it (never another server's manifest).
fn remove_server_manifest(data_dir: &Path) {
    let path = data_dir.join(SERVER_MANIFEST);
    let ours = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<ServerManifest>(&text).ok())
        .is_some_and(|manifest| manifest.pid == std::process::id());
    if ours {
        let _ = std::fs::remove_file(path);
    }
}

/// Everything `run` publishes or spawns, undone on every exit path (error, signal, panic).
#[derive(Default)]
struct RunCleanup {
    manifest_dir: Option<PathBuf>,
    remote: bool,
    server: Option<tokio::task::AbortHandle>,
    supervisor: Option<Arc<RemoteSupervisor>>,
}

impl RunCleanup {
    /// Async part: stop and reap the tunnel child through its owner.
    async fn shutdown(&mut self) {
        if let Some(supervisor) = self.supervisor.take() {
            supervisor.shutdown().await;
        }
    }
}

impl Drop for RunCleanup {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            server.abort();
        }
        if let Some(supervisor) = self.supervisor.take() {
            supervisor.request_stop();
        }
        if self.remote {
            remote::set_remote_hosts(Vec::new());
            remote::set_remote_enabled(false);
        }
        if let Some(dir) = self.manifest_dir.take() {
            remove_server_manifest(&dir);
        }
    }
}

pub async fn run(config: ServeConfig) -> Result<(), String> {
    let mut cleanup = RunCleanup::default();
    let result = run_inner(config, &mut cleanup).await;
    cleanup.shutdown().await;
    result
}

async fn run_inner(config: ServeConfig, cleanup: &mut RunCleanup) -> Result<(), String> {
    let web_ui_dir = if config.surface == ServeSurface::WebUi {
        let executable = std::env::current_exe()
            .map_err(|error| format!("cannot resolve server executable: {error}"))?;
        Some(resolve_ui_dir(
            config.ui_dir,
            std::env::var_os("CODETWO_WEB_UI_DIR").map(PathBuf::from),
            &executable,
        )?)
    } else {
        None
    };

    let data_dir = resolve_data_dir(config.data_dir);
    std::fs::create_dir_all(&data_dir).map_err(|error| {
        format!(
            "cannot create data directory {}: {error}",
            data_dir.display()
        )
    })?;
    let core = Arc::new(
        CoreApp::boot(AppConfig::new(&data_dir))
            .await
            .map_err(|error| error.to_string())?,
    );
    let engine = core
        .service::<EngineService>()
        .ok_or_else(|| "engine plugin did not load".to_string())?
        .0
        .clone();
    let external_mcp_on = wire_external_mcp(&engine, &data_dir, config.external_mcp);
    let store = core
        .service::<StoreService>()
        .ok_or_else(|| "store plugin did not load".to_string())?
        .0
        .clone();
    let events = core
        .service::<EventBus>()
        .ok_or_else(|| "bus plugin did not load".to_string())?
        .0
        .clone();
    let canvas_gate = core
        .service::<CanvasService>()
        .ok_or_else(|| "canvas service did not load".to_string())?
        .gate;

    let auth = Arc::new(AuthState::load(Some(data_dir.join("remote-devices.json"))));

    // Before the tunnel spawns or anything is published, so a signal can never hit the default
    // action (instant exit, tunnel child orphaned, `server.json` left behind) mid-startup.
    let mut signals = shutdown_signals();

    let mut remote_config =
        RemoteConfig::from_env_and_args(config.remote_args).map_err(|error| error.to_string())?;
    remote_config.local_port = config.port;
    if remote_config.mode != RemoteMode::Off {
        remote_config
            .validate()
            .map_err(|error| error.to_string())?;
        auth.set_remote_hardening(true);
        // Guard on before anything binds or spawns: unknown/foreign Hosts are denied from the start.
        cleanup.remote = true;
        remote::set_remote_enabled(true);
        if let Some(host) = &remote_config.remote_host {
            remote::set_remote_hosts(vec![host.clone()]);
        }
        if remote_config.mode == RemoteMode::Quick {
            eprintln!("warning: quick tunnel is experimental and unstable");
        }
    }

    let pair_ttl = std::env::var("CODETWO_PAIR_TTL")
        .ok()
        .and_then(|value| value.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_PAIRING_TTL);
    let pairing_token = auth.issue_pairing_token(auth.effective_pairing_ttl(pair_ttl));
    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .map_err(|error| {
            format!(
                "invalid bind address {}:{}: {error}",
                config.host, config.port
            )
        })?;

    let (local, handle) = if let Some(web_ui_dir) = web_ui_dir {
        let plugin_manager = core
            .service::<PluginManager>()
            .ok_or_else(|| "plugin manager did not load".to_string())?;
        bind_and_serve_with_web_ui(
            engine,
            events,
            addr,
            auth.clone(),
            store,
            canvas_gate,
            None,
            Some(Arc::new(KernelWebUiCommands::new(plugin_manager))),
            Some(web_ui_dir),
        )
        .await
        .map_err(|error| error.to_string())?
    } else {
        bind_and_serve_with_canvas(engine, events, addr, auth.clone(), store, canvas_gate)
            .await
            .map_err(|error| error.to_string())?
    };

    cleanup.server = Some(handle.abort_handle());

    // Fallible tunnel setup comes before anything is published; the manifest is written last, so
    // a failure here leaves no `server.json` (and `cleanup` covers every later exit).
    if remote_config.mode != RemoteMode::Off {
        remote_config.local_port = local.port();
        let supervisor = Arc::new(
            RemoteSupervisor::new(remote_config.clone(), SupervisorTiming::default())
                .map_err(|error| error.to_string())?,
        );
        cleanup.supervisor = Some(supervisor.clone());
        supervisor
            .start()
            .await
            .map_err(|error| error.to_string())?;
    }

    print_pairing(local.port(), &pairing_token);
    let paired = auth.list_devices().len();
    if paired > 0 {
        println!("  {paired} previously paired device(s) can reconnect without a new link.\n");
    }
    println!("  listening on {local}");
    println!("  data directory: {}\n", data_dir.display());
    if let Some(host) = &remote_config.remote_host {
        println!("  remote hostname: https://{host}/");
    }

    match write_server_manifest(&data_dir, local.port(), external_mcp_on) {
        Ok(()) => cleanup.manifest_dir = Some(data_dir.clone()),
        Err(error) => eprintln!("  could not write {}: {error}", SERVER_MANIFEST),
    }

    if config.surface == ServeSurface::WebUi && config.open_browser {
        let url = local_pairing_url(local.port(), &pairing_token);
        if let Err(error) = open_browser(&url) {
            eprintln!("  could not open the browser: {error}");
            eprintln!("  open this URL manually: {url}\n");
        }
    }

    let _core = core;
    tokio::select! {
        result = handle => result.map_err(|error| error.to_string()),
        _ = wait_for_shutdown(&mut signals) => Ok(()),
    }
}

#[cfg(unix)]
type Signals = Vec<tokio::signal::unix::Signal>;
#[cfg(not(unix))]
type Signals = ();

/// Ctrl-C, SIGTERM or SIGHUP (terminal closed): lets `run` stop the tunnel child and remove
/// `server.json`. Registering here (not lazily at the wait) closes the startup window.
fn shutdown_signals() -> Signals {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        [
            SignalKind::interrupt(),
            SignalKind::terminate(),
            SignalKind::hangup(),
        ]
        .into_iter()
        .filter_map(|kind| signal(kind).ok())
        .collect()
    }
}

async fn wait_for_shutdown(signals: &mut Signals) {
    #[cfg(unix)]
    if !signals.is_empty() {
        futures_util::future::select_all(signals.iter_mut().map(|s| Box::pin(s.recv()))).await;
        return;
    }
    let _ = signals;
    let _ = tokio::signal::ctrl_c().await;
}

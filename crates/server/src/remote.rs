//! Optional Cloudflare tunnel supervisor and remote Host guards.
//!
//! Default off. Never logs or argv-embeds connector tokens.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{watch, Mutex};
use tokio::task::JoinHandle;
use tracing::{info, warn};

static ALLOWED_REMOTE_HOSTS: OnceLock<RwLock<Vec<String>>> = OnceLock::new();
/// Explicit "remote access is on" state. An empty host list alone cannot mean "off": a quick
/// tunnel's hostname is unknown until cloudflared prints it, and the guard must deny meanwhile.
static REMOTE_ENABLED: AtomicBool = AtomicBool::new(false);

fn hosts_lock() -> &'static RwLock<Vec<String>> {
    ALLOWED_REMOTE_HOSTS.get_or_init(|| RwLock::new(Vec::new()))
}

/// Turn the remote Host guard on/off. Must be on before the listener binds / the tunnel spawns.
pub fn set_remote_enabled(enabled: bool) {
    REMOTE_ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn remote_enabled() -> bool {
    REMOTE_ENABLED.load(Ordering::SeqCst)
}

/// Hostnames that may reach the server through a supervised tunnel (shared with external MCP guard).
pub fn allowed_remote_hosts() -> Vec<String> {
    hosts_lock()
        .read()
        .map(|hosts| hosts.clone())
        .unwrap_or_default()
}

/// Install the configured public hostname(s) when remote access starts.
pub fn set_remote_hosts(hosts: Vec<String>) {
    // The external MCP transport guard must accept exactly the same tunnel hostnames.
    codetwo_core::external_mcp::state::set_allowed_hosts(hosts.clone());
    if let Ok(mut guard) = hosts_lock().write() {
        *guard = hosts;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteMode {
    Off,
    Named,
    Quick,
}

#[derive(Debug, Clone)]
pub enum TokenSource {
    File(PathBuf),
    Env,
}

#[derive(Clone)]
pub struct RemoteConfig {
    pub mode: RemoteMode,
    pub cloudflared_path: Option<PathBuf>,
    pub remote_host: Option<String>,
    pub token_source: Option<TokenSource>,
    pub allow_quick_tunnel: bool,
    pub local_port: u16,
}

impl std::fmt::Debug for RemoteConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteConfig")
            .field("mode", &self.mode)
            .field("cloudflared_path", &self.cloudflared_path)
            .field("remote_host", &self.remote_host)
            .field(
                "token_source",
                &self.token_source.as_ref().map(|_| "<redacted>"),
            )
            .field("allow_quick_tunnel", &self.allow_quick_tunnel)
            .field("local_port", &self.local_port)
            .finish()
    }
}

impl RemoteConfig {
    pub fn from_env_and_args(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut mode = RemoteMode::Off;
        let mut remote_host = None;
        let mut cloudflared_path = std::env::var_os("CODETWO_CLOUDFLARED_PATH").map(PathBuf::from);
        let mut token_file = std::env::var_os("CODETWO_TUNNEL_TOKEN_FILE").map(PathBuf::from);
        let mut allow_quick_tunnel = false;
        let mut local_port = std::env::var("CODETWO_PORT")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(4599);

        let mut arguments = args.into_iter().peekable();
        while let Some(arg) = arguments.next() {
            match arg.as_str() {
                "--remote" => {
                    let value = arguments
                        .next()
                        .ok_or_else(|| "--remote requires a value".to_string())?;
                    if value != "cloudflared" {
                        return Err(format!("unsupported --remote value: {value}"));
                    }
                    mode = RemoteMode::Named;
                }
                "--remote-host" => {
                    remote_host = Some(
                        arguments
                            .next()
                            .ok_or_else(|| "--remote-host requires a value".to_string())?,
                    );
                }
                "--tunnel-token-file" => {
                    token_file =
                        Some(PathBuf::from(arguments.next().ok_or_else(|| {
                            "--tunnel-token-file requires a path".to_string()
                        })?));
                }
                "--cloudflared-path" => {
                    cloudflared_path =
                        Some(PathBuf::from(arguments.next().ok_or_else(|| {
                            "--cloudflared-path requires a path".to_string()
                        })?));
                }
                "--allow-quick-tunnel" => {
                    allow_quick_tunnel = true;
                    if mode == RemoteMode::Off {
                        mode = RemoteMode::Quick;
                    }
                }
                "--port" | "-p" => {
                    local_port = arguments
                        .next()
                        .ok_or_else(|| format!("{arg} requires a port"))?
                        .parse()
                        .map_err(|_| format!("invalid port for {arg}"))?;
                }
                _ => {}
            }
        }

        if allow_quick_tunnel && mode == RemoteMode::Off {
            mode = RemoteMode::Quick;
        }

        let token_source = if mode == RemoteMode::Named {
            token_file
                .map(TokenSource::File)
                .or_else(|| std::env::var_os("TUNNEL_TOKEN").map(|_| TokenSource::Env))
        } else {
            None
        };

        Ok(Self {
            mode,
            cloudflared_path,
            remote_host,
            token_source,
            allow_quick_tunnel,
            local_port,
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        match self.mode {
            RemoteMode::Off => Ok(()),
            RemoteMode::Named => {
                let host = self
                    .remote_host
                    .as_deref()
                    .ok_or_else(|| "named remote access requires --remote-host".to_string())?;
                validate_public_host(host)?;
                if self.token_source.is_none() {
                    return Err(
                        "named remote access requires a tunnel token (file or TUNNEL_TOKEN env)"
                            .into(),
                    );
                }
                if let Some(TokenSource::File(path)) = &self.token_source {
                    validate_token_file_permissions(path)?;
                }
                Ok(())
            }
            RemoteMode::Quick => {
                if !self.allow_quick_tunnel {
                    return Err("quick tunnel requires --allow-quick-tunnel (experimental)".into());
                }
                // Optional here, but when given it becomes an allowed Host: same strict rules.
                if let Some(host) = self.remote_host.as_deref() {
                    validate_public_host(host)?;
                }
                Ok(())
            }
        }
    }
}

pub fn validate_public_host(host: &str) -> Result<(), String> {
    if host.is_empty() || host.len() > 253 {
        return Err("remote host is invalid".into());
    }
    if host.contains(':') || host.contains('/') || host.contains('\\') || host.contains('@') {
        return Err("remote host must be a plain hostname".into());
    }
    if !host
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')
    {
        return Err("remote host contains unsupported characters".into());
    }
    Ok(())
}

const MAX_TOKEN_FILE_BYTES: u64 = 16 * 1024;

/// Open the token file once and validate that same handle (no metadata/read race): regular file,
/// owned by us, no group/world access, bounded size. Messages never include token contents.
fn open_validated_token_file(path: &Path) -> Result<std::fs::File, String> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Never block on a FIFO planted at the path; the regular-file check below rejects it.
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|error| format!("cannot read tunnel token file: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot read tunnel token file: {error}"))?;
    if !metadata.is_file() {
        return Err("tunnel token file must be a regular file".into());
    }
    if metadata.len() > MAX_TOKEN_FILE_BYTES {
        return Err("tunnel token file is too large".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(
                "tunnel token file must not be group- or world-readable (chmod 0600)".into(),
            );
        }
        // SAFETY: geteuid has no preconditions and cannot fail.
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err("tunnel token file must be owned by the current user".into());
        }
    }
    Ok(file)
}

pub fn validate_token_file_permissions(path: &Path) -> Result<(), String> {
    open_validated_token_file(path).map(|_| ())
}

pub fn read_tunnel_token(source: &TokenSource) -> Result<String, String> {
    use std::io::Read;
    match source {
        TokenSource::Env => {
            std::env::var("TUNNEL_TOKEN").map_err(|_| "TUNNEL_TOKEN is not set".to_string())
        }
        TokenSource::File(path) => {
            let file = open_validated_token_file(path)?;
            let mut raw = String::new();
            file.take(MAX_TOKEN_FILE_BYTES + 1)
                .read_to_string(&mut raw)
                .map_err(|_| "tunnel token file is not valid text".to_string())?;
            let token = raw.trim();
            if token.is_empty() || raw.len() as u64 > MAX_TOKEN_FILE_BYTES {
                return Err("tunnel token file is empty or too large".into());
            }
            Ok(token.to_string())
        }
    }
}

pub fn resolve_cloudflared_binary(config: &RemoteConfig) -> Result<PathBuf, String> {
    if let Some(path) = &config.cloudflared_path {
        if path.is_file() {
            return Ok(path.clone());
        }
        return Err(format!(
            "cloudflared binary not found at {}",
            path.display()
        ));
    }
    which_cloudflared()
}

fn which_cloudflared() -> Result<PathBuf, String> {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    for directory in std::env::split_paths(&path_var) {
        let candidate = directory.join("cloudflared");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err("cloudflared not found on PATH (install externally or pass --cloudflared-path)".into())
}

fn host_header_value(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            let value = value.trim();
            let host = match value.strip_prefix('[') {
                Some(rest) => rest.split(']').next().map(|ip| format!("[{ip}]")),
                None => None,
            };
            host.unwrap_or_else(|| value.split(':').next().unwrap_or(value).to_string())
                .to_ascii_lowercase()
        })
        .filter(|value| !value.is_empty())
}

/// Headers the Cloudflare edge always sets (and a remote client cannot remove). A request that
/// carries them came through the tunnel even if `httpHostHeader` rewrote Host to localhost.
fn has_edge_headers(headers: &HeaderMap) -> bool {
    ["cf-ray", "cf-connecting-ip", "cf-visitor", "cf-ipcountry"]
        .iter()
        .any(|name| headers.contains_key(*name))
}

pub fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

pub fn request_came_via_tunnel(headers: &HeaderMap) -> bool {
    let Some(host) = host_header_value(headers) else {
        return false;
    };
    if is_loopback_host(&host) {
        return false;
    }
    allowed_remote_hosts()
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(&host))
}

/// Rate-limit key. `CF-Connecting-IP` is client-controlled unless the request provably came
/// through our own tunnel: the peer must be loopback (cloudflared runs locally) AND the Host must
/// be a configured tunnel hostname. Anything else (e.g. a LAN peer) is keyed on the real peer.
pub fn auth_limiter_source_key(headers: &HeaderMap, peer: SocketAddr) -> String {
    if peer.ip().is_loopback() && request_came_via_tunnel(headers) {
        if let Some(ip) = headers
            .get("cf-connecting-ip")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<std::net::IpAddr>().ok())
        {
            return format!("cf:{ip}");
        }
    }
    format!("peer:{}", peer.ip())
}

fn host_allowed_for_request(host: &str) -> bool {
    if is_loopback_host(host) {
        return true;
    }
    allowed_remote_hosts()
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(host))
}

pub fn remote_public_path_allowed(method: &str, path: &str) -> bool {
    let path = path.split('?').next().unwrap_or(path);
    if path == "/health" {
        return true;
    }
    if path == "/api/pair" || path == "/api/ws-ticket" {
        // Preflight carries no credential and performs no pairing or ticket issuance.
        return method.eq_ignore_ascii_case("POST") || method.eq_ignore_ascii_case("OPTIONS");
    }
    if path == "/ws" || path == "/api/web-ui/call" || path == "/external-mcp" {
        return true;
    }
    if path == "/" || path == "/pair" {
        return method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("HEAD");
    }
    (method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("HEAD"))
        && is_static_asset_path(path)
}

/// Explicit static-file rule for the Web UI bundle: a plain path whose last segment has a known
/// asset extension. Never an API namespace, dot-segment, hidden file or encoded path.
fn is_static_asset_path(path: &str) -> bool {
    const EXTENSIONS: &[&str] = &[
        "js",
        "mjs",
        "css",
        "map",
        "html",
        "ico",
        "png",
        "jpg",
        "jpeg",
        "gif",
        "webp",
        "svg",
        "woff",
        "woff2",
        "ttf",
        "otf",
        "webmanifest",
        "txt",
        "wasm",
    ];
    const BLOCKED_NAMESPACES: &[&str] = &[
        "api",
        "oauth",
        "mcp",
        "external-mcp",
        "terminal",
        "term",
        "ws",
        "health",
        "pair",
    ];
    let Some(rest) = path.strip_prefix('/') else {
        return false;
    };
    if rest.contains(['%', '\\', '\0']) {
        return false;
    }
    let segments: Vec<&str> = rest.split('/').collect();
    if segments
        .iter()
        .any(|segment| segment.is_empty() || segment.starts_with('.'))
    {
        return false;
    }
    if BLOCKED_NAMESPACES
        .iter()
        .any(|namespace| segments[0].eq_ignore_ascii_case(namespace))
    {
        return false;
    }
    segments
        .last()
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(_, ext)| {
            EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

pub async fn remote_access_guard(request: Request, next: Next) -> Response {
    if !remote_enabled() {
        return next.run(request).await;
    }
    let host = host_header_value(request.headers());
    let via_edge = has_edge_headers(request.headers());
    let loopback = host.as_deref().is_some_and(is_loopback_host);
    if loopback && !via_edge {
        return next.run(request).await;
    }
    // Unknown (quick tunnel not announced yet) or foreign Host is denied, never passed through.
    if !loopback && !host.as_deref().is_some_and(host_allowed_for_request) {
        return (
            StatusCode::MISDIRECTED_REQUEST,
            "host is not configured for remote access",
        )
            .into_response();
    }
    let path = request.uri().path();
    if !remote_public_path_allowed(request.method().as_str(), path) {
        return StatusCode::NOT_FOUND.into_response();
    }
    next.run(request).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorState {
    Stopped,
    Running,
    Backoff,
}

#[derive(Debug, Clone)]
pub struct RemoteStatus {
    pub state: SupervisorState,
    pub restarts: u32,
    pub public_url: Option<String>,
}

#[derive(Clone, Copy)]
pub struct SupervisorTiming {
    pub min_backoff: Duration,
    pub max_backoff: Duration,
    pub max_restarts_per_hour: u32,
}

impl Default for SupervisorTiming {
    fn default() -> Self {
        Self {
            min_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60),
            max_restarts_per_hour: 12,
        }
    }
}

struct Inner {
    state: SupervisorState,
    restarts: u32,
    restart_times: VecDeque<Instant>,
    public_url: Option<String>,
}

/// One owner task holds the only `Child`: it races process exit against shutdown, so there is
/// never more than one live tunnel process and shutdown always kills and reaps it.
pub struct RemoteSupervisor {
    config: RemoteConfig,
    timing: SupervisorTiming,
    token: Option<String>,
    binary: PathBuf,
    inner: Mutex<Inner>,
    stop: watch::Sender<bool>,
    owner: Mutex<Option<JoinHandle<()>>>,
}

async fn stopped(rx: &mut watch::Receiver<bool>) {
    let _ = rx.wait_for(|stop| *stop).await;
}

impl RemoteSupervisor {
    pub fn new(config: RemoteConfig, timing: SupervisorTiming) -> Result<Self, String> {
        config.validate()?;
        if config.mode == RemoteMode::Off {
            return Err("remote supervisor requires an active remote mode".into());
        }
        let binary = resolve_cloudflared_binary(&config)?;
        let token = match (&config.mode, &config.token_source) {
            (RemoteMode::Named, Some(source)) => Some(read_tunnel_token(source)?),
            _ => None,
        };
        if config.mode == RemoteMode::Named {
            if let Some(host) = &config.remote_host {
                set_remote_hosts(vec![host.clone()]);
            }
        }
        Ok(Self {
            config,
            timing,
            token,
            binary,
            inner: Mutex::new(Inner {
                state: SupervisorState::Stopped,
                restarts: 0,
                restart_times: VecDeque::new(),
                public_url: None,
            }),
            stop: watch::channel(false).0,
            owner: Mutex::new(None),
        })
    }

    pub async fn start(self: &Arc<Self>) -> Result<(), String> {
        let mut owner = self.owner.lock().await;
        if owner.is_some() || *self.stop.borrow() {
            return Err("remote supervisor already started or shut down".into());
        }
        // Guard first: from the first spawn on, unknown or foreign Hosts are denied.
        set_remote_enabled(true);
        let child = self.spawn_child()?;
        let supervisor = Arc::clone(self);
        *owner = Some(tokio::spawn(
            async move { supervisor.own_child(child).await },
        ));
        Ok(())
    }

    pub async fn status(&self) -> RemoteStatus {
        let inner = self.inner.lock().await;
        RemoteStatus {
            state: inner.state,
            restarts: inner.restarts,
            public_url: inner.public_url.clone(),
        }
    }

    /// Sync stop request for drop paths that cannot await: the owner task kills and reaps the child.
    pub fn request_stop(&self) {
        self.stop.send_replace(true);
    }

    /// Stop restarting, kill and reap the child, and drop the remote host allowlist.
    pub async fn shutdown(&self) {
        self.stop.send_replace(true);
        let owner = self.owner.lock().await.take();
        if let Some(owner) = owner {
            let _ = owner.await;
        }
        self.inner.lock().await.state = SupervisorState::Stopped;
        set_remote_hosts(Vec::new());
        set_remote_enabled(false);
    }

    async fn own_child(self: Arc<Self>, mut child: Child) {
        let mut stop = self.stop.subscribe();
        let mut backoff = self.timing.min_backoff;
        loop {
            let started = Instant::now();
            self.inner.lock().await.state = SupervisorState::Running;
            tokio::select! {
                _ = stopped(&mut stop) => {
                    let _ = child.kill().await;
                    self.inner.lock().await.state = SupervisorState::Stopped;
                    return;
                }
                status = child.wait() => {
                    if let Ok(status) = status {
                        warn!(
                            target: "remote.tunnel",
                            "cloudflared exited with {}",
                            status.code().unwrap_or(-1)
                        );
                    }
                }
            }
            if started.elapsed() >= self.timing.max_backoff {
                backoff = self.timing.min_backoff;
            }
            if self.config.mode == RemoteMode::Quick {
                // The exited quick tunnel's random hostname is dead; the next one gets a new one.
                set_remote_hosts(Vec::new());
                self.inner.lock().await.public_url = None;
            }
            child = loop {
                if !self.record_restart().await {
                    return;
                }
                tokio::select! {
                    _ = stopped(&mut stop) => {
                        self.inner.lock().await.state = SupervisorState::Stopped;
                        return;
                    }
                    _ = tokio::time::sleep(backoff) => {}
                }
                backoff = (backoff * 2).min(self.timing.max_backoff);
                match self.spawn_child() {
                    Ok(child) => break child,
                    Err(error) => {
                        warn!(target: "remote.tunnel", "cloudflared spawn failed: {error}")
                    }
                }
            };
        }
    }

    /// Count a restart; false (state Stopped) once the hourly cap is reached.
    async fn record_restart(&self) -> bool {
        let mut inner = self.inner.lock().await;
        let now = Instant::now();
        inner
            .restart_times
            .retain(|instant| now.duration_since(*instant) < Duration::from_secs(3600));
        if inner.restart_times.len() >= self.timing.max_restarts_per_hour as usize {
            warn!(target: "remote.tunnel", "restart cap reached; supervisor stopping");
            inner.state = SupervisorState::Stopped;
            return false;
        }
        inner.restart_times.push_back(now);
        inner.restarts = inner.restarts.saturating_add(1);
        inner.state = SupervisorState::Backoff;
        true
    }

    fn spawn_child(self: &Arc<Self>) -> Result<Child, String> {
        let mut command = Command::new(&self.binary);
        command
            .arg("tunnel")
            .arg("--no-autoupdate")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        match self.config.mode {
            RemoteMode::Named => {
                command.arg("run");
            }
            RemoteMode::Quick => {
                command
                    .arg("--url")
                    .arg(format!("http://127.0.0.1:{}", self.config.local_port));
            }
            RemoteMode::Off => return Err("remote mode is off".into()),
        }
        // Never inherit a connector credential from our own environment: a quick tunnel must stay
        // anonymous and a named one gets exactly the token we validated.
        command
            .env_remove("TUNNEL_TOKEN")
            .env_remove("TUNNEL_TOKEN_FILE");
        if let Some(token) = &self.token {
            command.env("TUNNEL_TOKEN", token);
        }
        let redact = Redactor::new(self.token.clone());
        let mut child = command
            .spawn()
            .map_err(|error| format!("failed to spawn {}: {error}", self.binary.display()))?;

        // Quick tunnels print their URL on stdout or stderr (cloudflared logs to stderr); the first
        // valid one per child wins, so later log lines can never replace the hostname.
        let quick = (self.config.mode == RemoteMode::Quick)
            .then(|| (Arc::downgrade(self), Arc::new(AtomicBool::new(false))));
        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(pump_output(stdout, false, redact.clone(), quick.clone()));
        }
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(pump_output(stderr, true, redact, quick));
        }
        Ok(child)
    }

    async fn register_quick_host(&self, host: String) {
        if *self.stop.borrow() {
            return;
        }
        set_remote_hosts(vec![host.clone()]);
        if *self.stop.borrow() {
            set_remote_hosts(Vec::new());
            return;
        }
        self.inner.lock().await.public_url = Some(format!("https://{host}"));
    }

    /// Feed one output line as if cloudflared printed it (quick mode only).
    pub async fn ingest_line_for_tests(&self, line: &str) {
        if self.config.mode != RemoteMode::Quick {
            return;
        }
        if let Some(host) = parse_quick_tunnel_host(line) {
            self.register_quick_host(host).await;
        }
    }
}

async fn pump_output<R: AsyncRead + Unpin>(
    reader: R,
    is_stderr: bool,
    redact: Redactor,
    quick: Option<(std::sync::Weak<RemoteSupervisor>, Arc<AtomicBool>)>,
) {
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if let Some((supervisor, found)) = &quick {
            if let Some(host) = parse_quick_tunnel_host(&line) {
                if !found.swap(true, Ordering::SeqCst) {
                    if let Some(supervisor) = supervisor.upgrade() {
                        supervisor.register_quick_host(host).await;
                    }
                }
            }
        }
        if is_stderr {
            warn!(target: "remote.tunnel", "{}", redact.redact(&line));
        } else {
            info!(target: "remote.tunnel", "{}", redact.redact(&line));
        }
    }
}

/// Strict quick-tunnel URL: a whole whitespace token `https://<random-label>.trycloudflare.com`
/// (optional trailing `/`, `.` or `,`). No path, port, userinfo or query; the single-label rule
/// rejects `a.b.trycloudflare.com`, and `api` is cloudflared's own endpoint, not a tunnel.
fn parse_quick_tunnel_host(line: &str) -> Option<String> {
    line.split_whitespace().find_map(|token| {
        let token = token.trim_end_matches(['.', ',']).trim_end_matches('/');
        let label = token
            .strip_prefix("https://")?
            .strip_suffix(".trycloudflare.com")?;
        let valid = !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        (valid && label != "api").then(|| format!("{label}.trycloudflare.com"))
    })
}

#[derive(Clone)]
struct Redactor {
    token: Option<String>,
}

impl Redactor {
    fn new(token: Option<String>) -> Self {
        Self { token }
    }

    fn redact(&self, line: &str) -> String {
        let mut output = line.to_string();
        if let Some(token) = &self.token {
            if !token.is_empty() {
                output = output.replace(token, "<redacted>");
            }
        }
        redact_jwt_shapes(&mut output);
        redact_url_queries(&mut output);
        output
    }
}

fn redact_jwt_shapes(line: &mut String) {
    let mut rebuilt = String::new();
    let mut rest = line.as_str();
    while let Some(index) = rest.find("eyJ").or_else(|| rest.find("EyJ")) {
        rebuilt.push_str(&rest[..index]);
        let tail = &rest[index..];
        let end = tail
            .char_indices()
            .find(|(_, ch)| ch.is_whitespace())
            .map(|(offset, _)| offset)
            .unwrap_or(tail.len());
        let candidate = &tail[..end];
        if candidate.matches('.').count() >= 2 {
            rebuilt.push_str("<redacted>");
        } else {
            rebuilt.push_str(candidate);
        }
        rest = &tail[end..];
    }
    rebuilt.push_str(rest);
    *line = rebuilt;
}

fn redact_url_queries(line: &mut String) {
    while let Some(index) = line.find('?') {
        let end = line[index..]
            .find(|ch: char| ch.is_whitespace())
            .map(|offset| index + offset)
            .unwrap_or(line.len());
        line.replace_range(index..end, "<query-redacted>");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_host_validation_rejects_unsafe_values() {
        assert!(validate_public_host("remote.example.com").is_ok());
        assert!(validate_public_host("bad/host").is_err());
        assert!(validate_public_host("host:443").is_err());
    }

    #[test]
    fn tunnel_path_allowlist_blocks_sensitive_routes() {
        assert!(remote_public_path_allowed("GET", "/health"));
        assert!(remote_public_path_allowed("POST", "/api/pair"));
        assert!(!remote_public_path_allowed("GET", "/api/team/v1/tasks"));
        assert!(!remote_public_path_allowed("POST", "/mcp"));
        assert!(remote_public_path_allowed("GET", "/assets/app.js"));
    }

    #[test]
    fn static_rule_excludes_dotted_api_and_metadata_paths() {
        for path in [
            "/.well-known/t3/environment",
            "/api/auth/session.json",
            "/api/v1.2/x.js",
            "/oauth/token.js",
            "/./app.js",
            "/assets/../x.js",
            "/assets/.hidden.js",
            "/%2e%2e/secret.js",
            "/mcp/x.js",
            "/term/x.js",
            "/app",
            "/x.exe",
        ] {
            assert!(!remote_public_path_allowed("GET", path), "{path}");
        }
        assert!(remote_public_path_allowed("GET", "/favicon.ico"));
        assert!(remote_public_path_allowed(
            "HEAD",
            "/canvas/canvas-island.js"
        ));
        assert!(!remote_public_path_allowed("POST", "/favicon.ico"));
    }

    #[test]
    fn quick_url_parser_is_strict() {
        let host = |line: &str| parse_quick_tunnel_host(line);
        assert_eq!(
            host("|  https://calm-river-0a1b.trycloudflare.com  |").as_deref(),
            Some("calm-river-0a1b.trycloudflare.com")
        );
        assert_eq!(
            host("Visit https://calm-river.trycloudflare.com.").as_deref(),
            Some("calm-river.trycloudflare.com")
        );
        for line in [
            "https://api.trycloudflare.com/tunnel",
            "https://api.trycloudflare.com",
            "https://evil.com/x.trycloudflare.com",
            "https://a.b.trycloudflare.com",
            "https://x.trycloudflare.com/path",
            "https://x.trycloudflare.com:8443",
            "https://user@x.trycloudflare.com",
            "http://x.trycloudflare.com",
            "https://-x.trycloudflare.com",
            "https://X_Y.trycloudflare.com",
            "url=https://x.trycloudflare.com",
            "https://.trycloudflare.com",
        ] {
            assert_eq!(host(line), None, "{line}");
        }
    }

    #[test]
    fn host_header_parsing_handles_ports_and_ipv6() {
        let parse = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert("host", value.parse().unwrap());
            host_header_value(&headers)
        };
        assert_eq!(
            parse("Remote.Example.com:443").as_deref(),
            Some("remote.example.com")
        );
        assert_eq!(parse("[::1]:4599").as_deref(), Some("[::1]"));
        assert!(is_loopback_host("[::1]"));
    }

    #[test]
    fn redactor_strips_jwt_and_query() {
        let redactor = Redactor::new(Some("secret-token".into()));
        let line = "visit https://x.test/?token=abc eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.e30.sig secret-token";
        let redacted = redactor.redact(line);
        assert!(!redacted.contains("secret-token"));
        assert!(!redacted.contains("eyJ"));
        assert!(redacted.contains("<query-redacted>"));
    }
}

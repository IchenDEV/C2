//! Server discovery and credential loading for CLI client commands.

use std::fs;
use std::io::Read;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use codetwo_core::external_mcp::clients::looks_like_external_token;
use reqwest::Url;

use crate::serve::{resolve_data_dir, ServerManifest};

use super::output::{CliOutcome, ExitCode};

const CLI_TOKEN_FILE: &str = "cli.token";
/// Remote server URL saved by `codetwo pair` (not a secret).
const CLI_URL_FILE: &str = "cli.url";
const SERVER_MANIFEST: &str = "server.json";
/// A credential is a short single line; anything bigger is not one.
const MAX_TOKEN_BYTES: u64 = 4096;

#[derive(Debug, Clone, Default)]
pub struct GlobalOpts {
    pub json: bool,
    pub url: Option<String>,
    pub token_file: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub base_url: String,
    pub token: String,
}

impl GlobalOpts {
    pub fn resolve_client(&self) -> Result<ClientConfig, CliOutcome> {
        let data_dir = resolve_data_dir(self.data_dir.clone());
        let base_url = self
            .url
            .clone()
            .or_else(|| std::env::var("CODETWO_URL").ok())
            .or_else(|| read_paired_url(&data_dir))
            .or_else(|| read_manifest_url(&data_dir))
            .ok_or_else(|| {
                CliOutcome::err(
                    ExitCode::Error,
                    "no_server",
                    "no server URL: pass --url, set CODETWO_URL, run `codetwo pair`, or run `codetwo serve`",
                )
            })?;
        let token = load_token(&data_dir, self.token_file.as_deref())?;
        Ok(ClientConfig { base_url, token })
    }
}

fn read_manifest_url(data_dir: &Path) -> Option<String> {
    let manifest = read_server_manifest(data_dir)?;
    #[cfg(unix)]
    if manifest.pid == 0 || unsafe { libc::kill(manifest.pid.try_into().ok()?, 0) } != 0 {
        return None;
    }
    Some(format!("http://127.0.0.1:{}", manifest.port))
}

fn read_paired_url(data_dir: &Path) -> Option<String> {
    let raw = fs::read_to_string(cli_url_path(data_dir)).ok()?;
    Some(raw.trim().to_string()).filter(|url| !url.is_empty())
}

pub fn data_dir_for_admin(opts: &GlobalOpts) -> PathBuf {
    resolve_data_dir(opts.data_dir.clone())
}

pub fn cli_token_path(data_dir: &Path) -> PathBuf {
    data_dir.join(CLI_TOKEN_FILE)
}

pub fn cli_url_path(data_dir: &Path) -> PathBuf {
    data_dir.join(CLI_URL_FILE)
}

pub fn read_server_manifest(data_dir: &Path) -> Option<ServerManifest> {
    let path = data_dir.join(SERVER_MANIFEST);
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// The bearer token may only go to https, or to plain http on this machine. Parsing (instead of
/// string matching) keeps `http://localhost:80@evil.example` from passing as loopback.
pub fn check_transport(raw: &str) -> Result<(), CliOutcome> {
    parse_server_url(raw).map(|_| ())
}

/// A bare `scheme://host[:port]` origin that passes [`check_transport`]; what `pair` saves.
pub fn normalize_server_url(raw: &str) -> Result<String, CliOutcome> {
    let url = parse_server_url(raw)?;
    if url.query().is_some() || url.fragment().is_some() || !matches!(url.path(), "" | "/") {
        return Err(CliOutcome::err(
            ExitCode::Error,
            "invalid_url",
            "server URL must be a bare origin like https://host[:port]",
        ));
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

// The URL is never echoed in errors: it may carry userinfo.
fn parse_server_url(raw: &str) -> Result<Url, CliOutcome> {
    let invalid = || {
        CliOutcome::err(
            ExitCode::Error,
            "invalid_url",
            "server URL must start with http:// (loopback only) or https://",
        )
    };
    let url = Url::parse(raw.trim()).map_err(|_| invalid())?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(CliOutcome::err(
            ExitCode::Error,
            "invalid_url",
            "server URL must not contain credentials",
        ));
    }
    let host = url.host_str().ok_or_else(invalid)?;
    match url.scheme() {
        "https" => Ok(url),
        "http" if is_loopback_host(host) => Ok(url),
        "http" => Err(CliOutcome::err(
            ExitCode::Error,
            "insecure_url",
            "refusing to send the credential over plain http to a non-loopback host; use https",
        )),
        _ => Err(invalid()),
    }
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn load_token(data_dir: &Path, token_file: Option<&Path>) -> Result<String, CliOutcome> {
    if let Some(path) = token_file {
        return read_token_file(path);
    }
    if let Ok(value) = std::env::var("CODETWO_TOKEN") {
        if value.is_empty() {
            return Err(CliOutcome::err(
                ExitCode::Error,
                "invalid_token",
                "CODETWO_TOKEN is empty",
            ));
        }
        if !looks_like_external_token(&value) {
            return Err(CliOutcome::err(
                ExitCode::Error,
                "invalid_token",
                "CODETWO_TOKEN is not an external MCP credential",
            ));
        }
        return Ok(value);
    }
    read_token_file(&cli_token_path(data_dir))
}

/// Read a credential from a protected file (0600 on unix, owned by us, no symlink). Type, mode
/// and owner are checked on the opened handle, so the file cannot change between check and read.
pub fn read_token_file(path: &Path) -> Result<String, CliOutcome> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Never hang on a FIFO planted at the path (the regular-file check rejects it) and never
        // follow a symlink in the last component.
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
    }
    let mut file = options.open(path).map_err(|error| {
        #[cfg(unix)]
        if error.raw_os_error() == Some(libc::ELOOP) {
            return CliOutcome::err(
                ExitCode::Error,
                "insecure_token_file",
                format!("refusing to follow a symlink: {}", path.display()),
            );
        }
        let _ = error;
        CliOutcome::err(
            ExitCode::Error,
            "missing_token",
            format!("credential file not found: {}", path.display()),
        )
    })?;
    let meta = file.metadata().map_err(|_| {
        CliOutcome::err(
            ExitCode::Error,
            "missing_token",
            format!("cannot read credential file: {}", path.display()),
        )
    })?;
    if !meta.is_file() {
        return Err(CliOutcome::err(
            ExitCode::Error,
            "invalid_token",
            format!("not a regular file: {}", path.display()),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(CliOutcome::err(
                ExitCode::Error,
                "insecure_token_file",
                format!(
                    "refusing to read {}: permissions must be 0600 (got {:o})",
                    path.display(),
                    mode
                ),
            ));
        }
        // SAFETY: geteuid has no preconditions and cannot fail.
        if meta.uid() != unsafe { libc::geteuid() } {
            return Err(CliOutcome::err(
                ExitCode::Error,
                "insecure_token_file",
                format!(
                    "refusing to read {}: not owned by the current user",
                    path.display()
                ),
            ));
        }
    }
    let mut raw = String::new();
    (&mut file)
        .take(MAX_TOKEN_BYTES + 1)
        .read_to_string(&mut raw)
        .map_err(|_| {
            CliOutcome::err(
                ExitCode::Error,
                "invalid_token",
                "credential file is not valid text",
            )
        })?;
    parse_token(&raw, "credential file")
}

/// Read a credential piped on stdin. An interactive terminal is refused: pasting would echo it.
pub fn read_token_stdin() -> Result<String, CliOutcome> {
    use std::io::IsTerminal;
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err(CliOutcome::err(
            ExitCode::Error,
            "stdin_is_terminal",
            "pipe the credential on stdin (it would be echoed in a terminal)",
        ));
    }
    let mut raw = String::new();
    stdin
        .lock()
        .take(MAX_TOKEN_BYTES + 1)
        .read_to_string(&mut raw)
        .map_err(|_| {
            CliOutcome::err(ExitCode::Error, "stdin", "stdin did not contain valid text")
        })?;
    parse_token(&raw, "stdin")
}

// Errors never quote the content: a mistaken paste may itself be a secret.
fn parse_token(raw: &str, source: &str) -> Result<String, CliOutcome> {
    let token = raw.trim();
    if raw.len() as u64 > MAX_TOKEN_BYTES || token.contains(char::is_whitespace) {
        return Err(CliOutcome::err(
            ExitCode::Error,
            "invalid_token",
            format!("{source} must contain only the single-line credential"),
        ));
    }
    if !looks_like_external_token(token) {
        return Err(CliOutcome::err(
            ExitCode::Error,
            "invalid_token",
            format!(
                "{source} does not contain an external MCP token (Web UI device and session tokens are not accepted)"
            ),
        ));
    }
    Ok(token.to_string())
}

/// Create or replace a credential file; it is 0600 from the first byte.
///
/// The secret is written to a private temp file next to `path` and renamed over it, so an
/// existing file is never partly written and a symlink is never written through. A path that
/// is already a symlink (or any non-regular file) is refused.
pub fn write_token_file(path: &Path, token: &str) -> Result<(), String> {
    write_private_file(path, &format!("{token}\n"))
}

/// Same guarantees as [`write_token_file`] for the non-secret `cli.url`.
pub fn write_private_file(path: &Path, contents: &str) -> Result<(), String> {
    use std::io::Write;
    match fs::symlink_metadata(path) {
        Ok(meta) if !meta.file_type().is_file() => {
            return Err(format!(
                "refusing to overwrite {}: not a regular file (symlinks are not followed)",
                path.display()
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    // Created 0600 by `tempfile`; removed on drop if anything below fails.
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
    tmp.write_all(contents.as_bytes())
        .and_then(|()| tmp.flush())
        .map_err(|e| e.to_string())?;
    tmp.persist(path).map_err(|e| e.error.to_string())?;
    Ok(())
}

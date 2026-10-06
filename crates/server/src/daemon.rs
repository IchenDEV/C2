//! Process ownership and operator pairing for the headless `codetwo-server serve` daemon.
//!
//! One data directory has exactly one live Core owner, so the daemon holds a pid file there and a
//! second `serve` refuses to start instead of racing the first for the database. A running daemon
//! never exposes a network endpoint that mints credentials. An operator on the same machine asks
//! for a new pairing link with `codetwo-server pair`, which signals the pid in the pid file; the
//! daemon answers through a private file in the data directory that the CLI reads and deletes.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The URL a second device should open, and whether it is reachable from another device.
///
/// An explicit public URL (reverse proxy, TLS tunnel, tailnet name) always wins. A server bound to
/// the loopback is only reachable from this machine; one bound to all interfaces advertises its
/// best LAN or tailnet address.
pub fn pairing_base_url(host: &str, port: u16, public_url: Option<&str>) -> (String, bool) {
    if let Some(public_url) = public_url.map(str::trim).filter(|url| !url.is_empty()) {
        return (public_url.trim_end_matches('/').to_string(), true);
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(address) if address.is_unspecified() => {
            let endpoints = crate::pairing_endpoints(port);
            let endpoint = crate::select_pairing_endpoint(&endpoints, None)
                .expect("pairing endpoints always include loopback");
            (
                endpoint.url.trim_end_matches('/').to_string(),
                endpoint.qr_shareable,
            )
        }
        Ok(address) if address.is_loopback() => (format!("http://{address}:{port}"), false),
        Ok(std::net::IpAddr::V6(address)) => (format!("http://[{address}]:{port}"), true),
        Ok(address) => (format!("http://{address}:{port}"), true),
        Err(_) => (format!("http://{host}:{port}"), true),
    }
}

const PID_FILE: &str = "server.pid";
const PAIRING_FILE: &str = "pairing.url";

/// Exclusive claim on a C2 data directory for the lifetime of the daemon.
#[derive(Debug)]
pub struct InstanceLock {
    path: PathBuf,
    pid: u32,
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // Signal 0 performs the existence and permission check without delivering anything. EPERM
    // means the process exists but belongs to another user, which still owns the directory.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    false
}

fn read_pid(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

impl InstanceLock {
    /// Claim `data_dir`, replacing a pid file whose process no longer exists.
    pub fn acquire(data_dir: &Path) -> Result<Self, String> {
        let path = data_dir.join(PID_FILE);
        let pid = std::process::id();
        for _ in 0..2 {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    file.write_all(format!("{pid}\n").as_bytes())
                        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
                    return Ok(Self { path, pid });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    match read_pid(&path) {
                        Some(owner) if owner != pid && process_alive(owner) => {
                            return Err(format!(
                                "data directory {} is already owned by C2 server pid {owner}",
                                data_dir.display()
                            ));
                        }
                        _ => {
                            let _ = std::fs::remove_file(&path);
                        }
                    }
                }
                Err(error) => {
                    return Err(format!("cannot claim {}: {error}", path.display()));
                }
            }
        }
        Err(format!(
            "cannot claim data directory {}",
            data_dir.display()
        ))
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        // Never remove a successor's claim.
        if read_pid(&self.path) == Some(self.pid) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Publish a fresh pairing link for the local operator. The file is private to the owner and is
/// replaced atomically so the CLI never reads a partial link.
pub fn write_pairing_file(data_dir: &Path, url: &str) -> Result<(), String> {
    let temporary = data_dir.join(format!("{PAIRING_FILE}.tmp"));
    let target = data_dir.join(PAIRING_FILE);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| format!("cannot write pairing link: {error}"))?;
    file.write_all(url.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("cannot write pairing link: {error}"))?;
    std::fs::rename(&temporary, &target)
        .map_err(|error| format!("cannot publish pairing link: {error}"))
}

/// Ask the daemon that owns `data_dir` for a new pairing link and return it.
#[cfg(unix)]
pub fn request_pairing_link(data_dir: &Path, timeout: Duration) -> Result<String, String> {
    let pid_path = data_dir.join(PID_FILE);
    let owner = read_pid(&pid_path)
        .filter(|pid| process_alive(*pid))
        .ok_or_else(|| {
            format!(
                "no running C2 server owns {} (start one with `codetwo-server serve`)",
                data_dir.display()
            )
        })?;
    let target = data_dir.join(PAIRING_FILE);
    let _ = std::fs::remove_file(&target);
    let owner_pid = i32::try_from(owner).map_err(|_| "invalid server pid".to_string())?;
    if unsafe { libc::kill(owner_pid, libc::SIGUSR1) } != 0 {
        return Err(format!(
            "cannot signal C2 server pid {owner}: {}",
            std::io::Error::last_os_error()
        ));
    }
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Ok(link) = std::fs::read_to_string(&target) {
            if !link.trim().is_empty() {
                let _ = std::fs::remove_file(&target);
                return Ok(link.trim().to_string());
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(format!("C2 server pid {owner} did not answer in time"))
}

#[cfg(not(unix))]
pub fn request_pairing_link(_data_dir: &Path, _timeout: Duration) -> Result<String, String> {
    Err("`codetwo-server pair` is only available on Unix hosts".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_owner_is_refused_until_the_first_releases() {
        let dir = tempfile::tempdir().unwrap();
        let first = InstanceLock::acquire(dir.path()).unwrap();
        // Our own pid is treated as re-entrant (same process), so simulate another live owner.
        std::fs::write(dir.path().join(PID_FILE), "1\n").unwrap();
        let error = InstanceLock::acquire(dir.path()).unwrap_err();
        assert!(error.contains("already owned"), "{error}");
        std::fs::write(dir.path().join(PID_FILE), format!("{}\n", first.pid)).unwrap();
        drop(first);
        assert!(!dir.path().join(PID_FILE).exists());
        InstanceLock::acquire(dir.path()).unwrap();
    }

    #[test]
    fn a_stale_pid_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        // pid_max never exceeds 2^22 on supported hosts, so this process cannot exist.
        std::fs::write(dir.path().join(PID_FILE), "2147483000\n").unwrap();
        let lock = InstanceLock::acquire(dir.path()).unwrap();
        assert_eq!(read_pid(&dir.path().join(PID_FILE)), Some(lock.pid));
    }

    #[test]
    fn pairing_base_reflects_how_reachable_the_bind_address_is() {
        assert_eq!(
            pairing_base_url("127.0.0.1", 4599, None),
            ("http://127.0.0.1:4599".into(), false)
        );
        assert_eq!(
            pairing_base_url("100.101.102.103", 4599, None),
            ("http://100.101.102.103:4599".into(), true)
        );
        assert_eq!(
            pairing_base_url("::", 4599, Some(" https://c2.example.com/ ")),
            ("https://c2.example.com".into(), true)
        );
        assert_eq!(
            pairing_base_url("fd00::1", 80, None),
            ("http://[fd00::1]:80".into(), true)
        );
        let (url, _) = pairing_base_url("0.0.0.0", 4599, None);
        assert!(url.starts_with("http://") && url.ends_with(":4599"));
    }

    #[cfg(unix)]
    #[test]
    fn the_pairing_file_is_private_and_atomic() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        write_pairing_file(dir.path(), "http://127.0.0.1:4599/pair#token=abc").unwrap();
        let target = dir.path().join(PAIRING_FILE);
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "http://127.0.0.1:4599/pair#token=abc"
        );
        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(!dir.path().join("pairing.url.tmp").exists());
    }

    #[cfg(unix)]
    #[test]
    fn pairing_request_fails_without_a_running_owner() {
        let dir = tempfile::tempdir().unwrap();
        let error = request_pairing_link(dir.path(), Duration::from_millis(50)).unwrap_err();
        assert!(error.contains("no running C2 server"), "{error}");
    }
}

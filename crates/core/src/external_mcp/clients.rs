//! File-backed registry of external MCP client credentials (hash-only, revocable, project-scoped).

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use uuid::Uuid;

mod rfc3339_ts {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(dt: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        dt.to_rfc3339().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        DateTime::parse_from_rfc3339(&raw)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(serde::de::Error::custom)
    }
}

mod rfc3339_ts_opt {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(dt: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match dt {
            Some(value) => serializer.serialize_some(&value.to_rfc3339()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw: Option<String> = Option::deserialize(deserializer)?;
        raw.map(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(serde::de::Error::custom)
        })
        .transpose()
    }
}

pub const TOKEN_PREFIX: &str = "ctmcp_";
pub const DEFAULT_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
pub const MAX_ACTIVE_CLIENTS: usize = 64;

/// Credential lifetime chosen at issuance time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtlChoice {
    Default30Days,
    Days(u32),
    Permanent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalScope {
    Read,
    Operate,
    Approve,
    Admin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectScope {
    All,
    Paths(Vec<PathBuf>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalClientRecord {
    pub id: String,
    pub name: String,
    pub scopes: Vec<ExternalScope>,
    pub projects: ProjectScope,
    #[serde(with = "rfc3339_ts")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "rfc3339_ts_opt", default)]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(with = "rfc3339_ts_opt")]
    pub last_used_at: Option<DateTime<Utc>>,
    #[serde(with = "rfc3339_ts_opt")]
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct ResolvedClient {
    pub record: ExternalClientRecord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    Unknown,
    Expired,
    Revoked,
    NotExternalToken,
}

#[derive(Debug, Clone)]
pub enum RegistryError {
    Io(String),
    Json(String),
    Disabled(String),
    Validation(String),
    LimitReached,
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(m) => write!(f, "io error: {m}"),
            Self::Json(m) => write!(f, "json error: {m}"),
            Self::Disabled(m) => write!(f, "registry disabled: {m}"),
            Self::Validation(m) => write!(f, "{m}"),
            Self::LimitReached => write!(f, "external MCP client limit reached"),
        }
    }
}

impl std::error::Error for RegistryError {}

#[derive(Serialize, Deserialize)]
struct PersistedFile {
    version: u32,
    clients: Vec<PersistedClient>,
}

#[derive(Serialize, Deserialize)]
struct PersistedClient {
    #[serde(flatten)]
    record: ExternalClientRecord,
    token_hash_hex: String,
}

struct StoredClient {
    record: ExternalClientRecord,
    hash: [u8; 32],
}

pub struct ExternalClientRegistry {
    path: PathBuf,
    clients: HashMap<String, StoredClient>,
    disabled: Option<String>,
    last_mtime: Option<std::time::SystemTime>,
    last_used_dirty: bool,
    last_used_flush_at: Instant,
}

impl fmt::Debug for ExternalClientRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExternalClientRegistry")
            .field("path", &self.path)
            .field("client_count", &self.clients.len())
            .field("disabled", &self.disabled.is_some())
            .finish()
    }
}

pub fn looks_like_external_token(token: &str) -> bool {
    token.starts_with(TOKEN_PREFIX) && token.len() > TOKEN_PREFIX.len()
}

impl ExternalClientRegistry {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RegistryError> {
        let path = path.as_ref().to_path_buf();
        let mut registry = Self {
            path,
            clients: HashMap::new(),
            disabled: None,
            last_mtime: None,
            last_used_dirty: false,
            last_used_flush_at: Instant::now(),
        };
        registry.reload_if_needed(true)?;
        Ok(registry)
    }

    pub fn create(
        &mut self,
        name: &str,
        mut scopes: Vec<ExternalScope>,
        projects: ProjectScope,
        ttl: TtlChoice,
    ) -> Result<(ExternalClientRecord, String), RegistryError> {
        let _file_lock = self.lock_store()?;
        self.reload_if_needed(true)?;
        self.ensure_enabled()?;
        validate_name(name)?;
        if scopes.is_empty() {
            scopes = vec![
                ExternalScope::Read,
                ExternalScope::Operate,
                ExternalScope::Approve,
            ];
        }
        let active = self
            .clients
            .values()
            .filter(|c| c.record.revoked_at.is_none())
            .count();
        if active >= MAX_ACTIVE_CLIENTS {
            return Err(RegistryError::LimitReached);
        }
        let projects = canonicalize_project_scope(projects)?;
        let now = Utc::now();
        let expires_at = match ttl {
            TtlChoice::Permanent => None,
            TtlChoice::Default30Days => Some(
                now + ChronoDuration::from_std(DEFAULT_TTL).unwrap_or(ChronoDuration::days(30)),
            ),
            TtlChoice::Days(days) => {
                if days == 0 {
                    return Err(RegistryError::Validation(
                        "TTL days must be at least 1 or use permanent".into(),
                    ));
                }
                Some(
                    now.checked_add_signed(ChronoDuration::days(i64::from(days)))
                        .ok_or_else(|| {
                            RegistryError::Validation("TTL exceeds the supported date range".into())
                        })?,
                )
            }
        };
        let token = generate_token();
        let hash = hash_token(&token);
        let id = Uuid::new_v4().to_string();
        let record = ExternalClientRecord {
            id: id.clone(),
            name: name.to_string(),
            scopes,
            projects,
            created_at: now,
            expires_at,
            last_used_at: None,
            revoked_at: None,
        };
        self.clients.insert(
            id,
            StoredClient {
                record: record.clone(),
                hash,
            },
        );
        self.persist()?;
        Ok((record, token))
    }

    pub fn revoke(&mut self, id: &str) -> Result<(), RegistryError> {
        let _file_lock = self.lock_store()?;
        self.reload_if_needed(true)?;
        self.ensure_enabled()?;
        let stored = self
            .clients
            .get_mut(id)
            .ok_or_else(|| RegistryError::Validation("client not found".into()))?;
        if stored.record.revoked_at.is_none() {
            stored.record.revoked_at = Some(Utc::now());
            self.persist()?;
        }
        Ok(())
    }

    pub fn list(&mut self) -> Result<Vec<ExternalClientRecord>, RegistryError> {
        self.reload_if_needed(true)?;
        self.ensure_enabled()?;
        Ok(self.clients.values().map(|c| c.record.clone()).collect())
    }

    pub fn resolve(&mut self, token: &str) -> Result<ResolvedClient, ResolveError> {
        if !looks_like_external_token(token) {
            return Err(ResolveError::NotExternalToken);
        }
        // Fail closed: a failed reload leaves no clients and `disabled` set.
        let _ = self.reload_if_needed(false);
        if self.disabled.is_some() {
            return Err(ResolveError::Unknown);
        }
        let now = Utc::now();
        let candidate_hash = hash_token(token);
        let mut matched_id: Option<String> = None;
        for (id, stored) in &self.clients {
            if constant_time_eq(&stored.hash, &candidate_hash) {
                matched_id = Some(id.clone());
            }
        }
        let id = matched_id.ok_or(ResolveError::Unknown)?;
        let stored = self.clients.get_mut(&id).expect("matched id");
        if stored.record.revoked_at.is_some() {
            return Err(ResolveError::Revoked);
        }
        if stored
            .record
            .expires_at
            .is_some_and(|expires| expires <= now)
        {
            return Err(ResolveError::Expired);
        }
        stored.record.last_used_at = Some(now);
        let record = stored.record.clone();
        self.last_used_dirty = true;
        self.maybe_flush_last_used();
        Ok(ResolvedClient { record })
    }

    pub fn project_allowed(client: &ResolvedClient, path: &Path) -> bool {
        if path
            .components()
            .any(|c| c == std::path::Component::ParentDir)
        {
            return false;
        }
        let Ok(canonical_target) = std::fs::canonicalize(path) else {
            return false;
        };
        match &client.record.projects {
            ProjectScope::All => true,
            ProjectScope::Paths(allowed) => allowed.iter().any(|root| {
                std::fs::canonicalize(root)
                    .ok()
                    .is_some_and(|canonical_root| {
                        path_is_prefix(&canonical_root, &canonical_target)
                    })
            }),
        }
    }

    fn ensure_enabled(&self) -> Result<(), RegistryError> {
        if let Some(reason) = &self.disabled {
            return Err(RegistryError::Disabled(reason.clone()));
        }
        Ok(())
    }

    /// Re-read the store when its mtime changed. Fail closed: any problem (missing, insecure,
    /// unreadable, malformed) clears every client and disables the surface, and the mtime is
    /// only remembered after a successful parse so the next call retries and recovers.
    fn reload_if_needed(&mut self, force: bool) -> Result<(), RegistryError> {
        let mtime = std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok();
        // Atomic replacements can share timestamps: always read the bounded file so revocation
        // never depends on filesystem mtime precision. Callers run this on blocking threads.
        let _ = force;
        match self.read_store() {
            Ok(clients) => {
                self.clients = clients;
                self.disabled = None;
                self.last_mtime = mtime;
                Ok(())
            }
            Err(error) => {
                self.clients.clear();
                self.disabled = Some(match &error {
                    RegistryError::Disabled(reason) => reason.clone(),
                    other => other.to_string(),
                });
                match error {
                    RegistryError::Disabled(_) => Ok(()),
                    other => Err(other),
                }
            }
        }
    }

    fn read_store(&self) -> Result<HashMap<String, StoredClient>, RegistryError> {
        use std::io::Read;
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
        }
        let file = match options.open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
            Err(_) => {
                return Err(RegistryError::Io(
                    "credential file could not be opened".into(),
                ))
            }
        };
        let meta = file
            .metadata()
            .map_err(|_| RegistryError::Io("credential metadata failed".into()))?;
        if !meta.is_file() || meta.len() > 1024 * 1024 {
            return Err(RegistryError::Disabled(
                "credential file must be a bounded regular file".into(),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o077 != 0 {
                return Err(RegistryError::Disabled(
                    "credential file is group or world accessible".into(),
                ));
            }
        }
        let mut bytes = Vec::new();
        file.take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| RegistryError::Io("credential file read failed".into()))?;
        if bytes.len() > 1024 * 1024 {
            return Err(RegistryError::Disabled(
                "credential file exceeds size limit".into(),
            ));
        }
        let file: PersistedFile = serde_json::from_slice(&bytes)
            .map_err(|e| RegistryError::Json(format!("parse credential file: {e}")))?;
        let mut clients = HashMap::new();
        for entry in file.clients {
            let hash = decode_hash_hex(&entry.token_hash_hex).ok_or_else(|| {
                RegistryError::Json("invalid token hash in credential file".into())
            })?;
            clients.insert(
                entry.record.id.clone(),
                StoredClient {
                    record: entry.record,
                    hash,
                },
            );
        }
        Ok(clients)
    }

    fn persist(&mut self) -> Result<(), RegistryError> {
        self.ensure_enabled()?;
        let persisted = PersistedFile {
            version: 1,
            clients: self
                .clients
                .values()
                .map(|c| PersistedClient {
                    record: c.record.clone(),
                    token_hash_hex: hex_encode(&c.hash),
                })
                .collect(),
        };
        let json = serde_json::to_vec_pretty(&persisted)
            .map_err(|e| RegistryError::Json(e.to_string()))?;
        atomic_write(&self.path, &json)?;
        self.last_mtime = std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok();
        self.last_used_dirty = false;
        self.last_used_flush_at = Instant::now();
        Ok(())
    }

    fn lock_store(&self) -> Result<std::fs::File, RegistryError> {
        use fs2::FileExt;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| RegistryError::Io("credential file has no parent".into()))?;
        std::fs::create_dir_all(parent)
            .map_err(|_| RegistryError::Io("credential directory failed".into()))?;
        let path = self.path.with_extension("lock");
        if std::fs::symlink_metadata(&path).is_ok_and(|meta| !meta.is_file()) {
            return Err(RegistryError::Disabled(
                "credential lock is not a regular file".into(),
            ));
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = options
            .open(path)
            .map_err(|_| RegistryError::Io("credential lock open failed".into()))?;
        file.lock_exclusive()
            .map_err(|_| RegistryError::Io("credential lock failed".into()))?;
        Ok(file)
    }

    fn maybe_flush_last_used(&mut self) {
        if !self.last_used_dirty || self.last_used_flush_at.elapsed() < Duration::from_secs(60) {
            return;
        }
        let Ok(_file_lock) = self.lock_store() else {
            return;
        };
        let observations: Vec<_> = self
            .clients
            .iter()
            .map(|(id, client)| (id.clone(), client.record.last_used_at))
            .collect();
        if self.reload_if_needed(true).is_err() || self.disabled.is_some() {
            return;
        }
        // Merge observations into the latest credential snapshot, preserving every revoke.
        for (id, observed) in observations {
            if let Some(client) = self.clients.get_mut(&id) {
                client.record.last_used_at = client.record.last_used_at.max(observed);
            }
        }
        let _ = self.persist();
    }

    #[cfg(test)]
    pub fn is_disabled(&self) -> bool {
        self.disabled.is_some()
    }

    /// Test helper: mark a client expired without waiting for TTL.
    #[doc(hidden)]
    pub fn expire_client_for_test(&mut self, id: &str) {
        if let Some(stored) = self.clients.get_mut(id) {
            stored.record.expires_at = Some(Utc::now() - ChronoDuration::seconds(1));
        }
        self.persist().expect("persist test expiry");
    }
}

fn generate_token() -> String {
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let mut token_bytes = [0u8; 32];
    token_bytes[..16].copy_from_slice(a.as_bytes());
    token_bytes[16..].copy_from_slice(b.as_bytes());
    let encoded = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        token_bytes,
    );
    format!("{TOKEN_PREFIX}{encoded}")
}

fn hash_token(token: &str) -> [u8; 32] {
    let digest = Sha256::digest(token.as_bytes());
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn constant_time_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

fn hex_encode(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_hash_hex(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        if chunk.len() != 2 {
            return None;
        }
        let s = std::str::from_utf8(chunk).ok()?;
        out[i] = u8::from_str_radix(s, 16).ok()?;
    }
    Some(out)
}

fn validate_name(name: &str) -> Result<(), RegistryError> {
    if name.is_empty() || name.len() > 64 {
        return Err(RegistryError::Validation(
            "client name must be 1..=64 characters".into(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '.' | '-'))
    {
        return Err(RegistryError::Validation(
            "client name contains invalid characters".into(),
        ));
    }
    Ok(())
}

fn canonicalize_project_scope(scope: ProjectScope) -> Result<ProjectScope, RegistryError> {
    match scope {
        ProjectScope::All => Ok(ProjectScope::All),
        ProjectScope::Paths(paths) => {
            let mut out = Vec::with_capacity(paths.len());
            for path in paths {
                let canonical = std::fs::canonicalize(&path).map_err(|e| {
                    RegistryError::Validation(format!(
                        "project path {} not found: {e}",
                        path.display()
                    ))
                })?;
                out.push(canonical);
            }
            Ok(ProjectScope::Paths(out))
        }
    }
}

fn path_is_prefix(root: &Path, target: &Path) -> bool {
    target.starts_with(root)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), RegistryError> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| RegistryError::Io("credential path has no parent".into()))?;
    std::fs::create_dir_all(parent)
        .map_err(|_| RegistryError::Io("credential directory failed".into()))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| RegistryError::Io("credential temporary file failed".into()))?;
    temp.write_all(bytes)
        .and_then(|()| temp.as_file().sync_all())
        .map_err(|_| RegistryError::Io("credential temporary write failed".into()))?;
    temp.persist(path)
        .map_err(|_| RegistryError::Io("credential replacement failed".into()))?;
    Ok(())
}

/// Per-client token bucket rate limits (read vs write classes).
pub struct RateLimiter<C: RateLimiterClock = SystemClock> {
    clock: C,
    read_capacity: f64,
    write_capacity: f64,
    read_refill_per_sec: f64,
    write_refill_per_sec: f64,
    buckets: Mutex<HashMap<String, ClientBucket>>,
}

#[derive(Clone, Copy)]
struct ClientBucket {
    read_tokens: f64,
    write_tokens: f64,
    last_update: Instant,
}

pub trait RateLimiterClock {
    fn now(&self) -> Instant;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl RateLimiterClock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

impl Default for RateLimiter<SystemClock> {
    fn default() -> Self {
        Self::new(SystemClock)
    }
}

impl<C: RateLimiterClock> RateLimiter<C> {
    pub fn new(clock: C) -> Self {
        Self {
            clock,
            read_capacity: 120.0,
            write_capacity: 30.0,
            read_refill_per_sec: 120.0 / 60.0,
            write_refill_per_sec: 30.0 / 60.0,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    pub fn try_consume(&self, client_id: &str, is_write: bool) -> bool {
        let mut buckets = self.buckets.lock().unwrap();
        let now = self.clock.now();
        let bucket = buckets
            .entry(client_id.to_string())
            .or_insert_with(|| ClientBucket {
                read_tokens: self.read_capacity,
                write_tokens: self.write_capacity,
                last_update: now,
            });
        let elapsed = now.duration_since(bucket.last_update).as_secs_f64();
        bucket.read_tokens =
            (bucket.read_tokens + elapsed * self.read_refill_per_sec).min(self.read_capacity);
        bucket.write_tokens =
            (bucket.write_tokens + elapsed * self.write_refill_per_sec).min(self.write_capacity);
        bucket.last_update = now;
        if is_write {
            if bucket.write_tokens >= 1.0 {
                bucket.write_tokens -= 1.0;
                true
            } else {
                false
            }
        } else if bucket.read_tokens >= 1.0 {
            bucket.read_tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

impl fmt::Debug for RateLimiter<SystemClock> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RateLimiter").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use tempfile::TempDir;

    struct FakeClock {
        millis: AtomicU64,
    }

    impl FakeClock {
        fn new() -> Self {
            Self {
                millis: AtomicU64::new(0),
            }
        }

        fn advance_ms(&self, ms: u64) {
            self.millis.fetch_add(ms, Ordering::SeqCst);
        }
    }

    impl RateLimiterClock for Arc<FakeClock> {
        fn now(&self) -> Instant {
            Instant::now() + Duration::from_millis(self.millis.load(Ordering::SeqCst))
        }
    }

    fn registry_path(dir: &TempDir) -> PathBuf {
        dir.path().join("external-mcp-clients.json")
    }

    #[test]
    fn create_resolve_revoke_and_never_debug_token() {
        let dir = TempDir::new().unwrap();
        let path = registry_path(&dir);
        let mut reg = ExternalClientRegistry::open(&path).unwrap();
        let (record, token) = reg
            .create(
                "cli-bot",
                vec![ExternalScope::Read],
                ProjectScope::All,
                TtlChoice::Default30Days,
            )
            .unwrap();
        assert!(looks_like_external_token(&token));
        let debug = format!("{reg:?}");
        assert!(!debug.contains(&token));
        let resolved = reg.resolve(&token).unwrap();
        assert_eq!(resolved.record.id, record.id);
        reg.revoke(&record.id).unwrap();
        assert!(matches!(reg.resolve(&token), Err(ResolveError::Revoked)));
    }

    #[test]
    fn wrong_prefix_and_unknown_token() {
        let dir = TempDir::new().unwrap();
        let mut reg = ExternalClientRegistry::open(registry_path(&dir)).unwrap();
        assert!(matches!(
            reg.resolve("not-external"),
            Err(ResolveError::NotExternalToken)
        ));
        reg.create(
            "a",
            vec![ExternalScope::Read],
            ProjectScope::All,
            TtlChoice::Default30Days,
        )
        .unwrap();
        assert!(matches!(
            reg.resolve("ctmcp_invalid"),
            Err(ResolveError::Unknown)
        ));
    }

    #[test]
    fn expired_client_rejected() {
        let dir = TempDir::new().unwrap();
        let path = registry_path(&dir);
        let mut reg = ExternalClientRegistry::open(&path).unwrap();
        let (record, token) = reg
            .create(
                "exp",
                vec![ExternalScope::Read],
                ProjectScope::All,
                TtlChoice::Default30Days,
            )
            .unwrap();
        reg.expire_client_for_test(&record.id);
        assert!(matches!(reg.resolve(&token), Err(ResolveError::Expired)));
        let _ = record;
    }

    #[test]
    fn permanent_credential_never_expires() {
        let dir = TempDir::new().unwrap();
        let mut reg = ExternalClientRegistry::open(registry_path(&dir)).unwrap();
        let (_record, token) = reg
            .create(
                "perm",
                vec![ExternalScope::Read],
                ProjectScope::All,
                TtlChoice::Permanent,
            )
            .unwrap();
        assert!(reg.resolve(&token).is_ok());
    }

    #[test]
    fn default_scopes_include_approve_when_empty() {
        let dir = TempDir::new().unwrap();
        let mut reg = ExternalClientRegistry::open(registry_path(&dir)).unwrap();
        let (record, _token) = reg
            .create(
                "defaults",
                vec![],
                ProjectScope::All,
                TtlChoice::Default30Days,
            )
            .unwrap();
        assert!(record.scopes.contains(&ExternalScope::Approve));
        assert!(record.scopes.contains(&ExternalScope::Read));
        assert!(record.scopes.contains(&ExternalScope::Operate));
    }

    #[test]
    fn max_clients_enforced() {
        let dir = TempDir::new().unwrap();
        let path = registry_path(&dir);
        let mut reg = ExternalClientRegistry::open(&path).unwrap();
        for i in 0..MAX_ACTIVE_CLIENTS {
            reg.create(
                &format!("client-{i:02}"),
                vec![ExternalScope::Read],
                ProjectScope::All,
                TtlChoice::Default30Days,
            )
            .unwrap();
        }
        assert!(matches!(
            reg.create(
                "overflow",
                vec![ExternalScope::Read],
                ProjectScope::All,
                TtlChoice::Default30Days,
            ),
            Err(RegistryError::LimitReached)
        ));
    }

    #[test]
    fn list_never_contains_hash_or_token() {
        let dir = TempDir::new().unwrap();
        let path = registry_path(&dir);
        let mut reg = ExternalClientRegistry::open(&path).unwrap();
        let (_record, token) = reg
            .create(
                "listed",
                vec![ExternalScope::Read],
                ProjectScope::All,
                TtlChoice::Default30Days,
            )
            .unwrap();
        let listed = reg.list().unwrap();
        let json = serde_json::to_string(&listed).unwrap();
        assert!(!json.contains(&token));
        assert!(!json.contains("token_hash"));
    }

    #[cfg(unix)]
    #[test]
    fn group_readable_file_disables_registry() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let path = registry_path(&dir);
        {
            let mut reg = ExternalClientRegistry::open(&path).unwrap();
            reg.create(
                "x",
                vec![ExternalScope::Read],
                ProjectScope::All,
                TtlChoice::Default30Days,
            )
            .unwrap();
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let mut reg = ExternalClientRegistry::open(&path).unwrap();
        assert!(reg.is_disabled());
        assert!(matches!(
            reg.resolve("ctmcp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            Err(ResolveError::Unknown)
        ));
    }

    #[test]
    fn project_path_escape_denied() {
        let dir = TempDir::new().unwrap();
        let allowed = dir.path().join("allowed");
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(allowed.join("nested")).unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let mut reg = ExternalClientRegistry::open(registry_path(&dir)).unwrap();
        let (_record, token) = reg
            .create(
                "scoped",
                vec![ExternalScope::Read],
                ProjectScope::Paths(vec![allowed.clone()]),
                TtlChoice::Default30Days,
            )
            .unwrap();
        let client = reg.resolve(&token).unwrap();
        assert!(ExternalClientRegistry::project_allowed(
            &client,
            &allowed.join("nested")
        ));
        assert!(!ExternalClientRegistry::project_allowed(&client, &outside));
        assert!(!ExternalClientRegistry::project_allowed(
            &client,
            Path::new("/this/path/should/not/exist/for/sure")
        ));
        assert!(!ExternalClientRegistry::project_allowed(
            &client,
            Path::new("../escape")
        ));
    }

    #[test]
    fn rate_limiter_blocks_burst_writes() {
        let clock = Arc::new(FakeClock::new());
        let limiter = RateLimiter::new(clock.clone());
        for _ in 0..30 {
            assert!(limiter.try_consume("c1", true));
        }
        assert!(!limiter.try_consume("c1", true));
        clock.advance_ms(60_000);
        assert!(limiter.try_consume("c1", true));
    }

    #[test]
    fn resolve_reloads_on_mtime_change() {
        let dir = TempDir::new().unwrap();
        let path = registry_path(&dir);
        let mut reg_a = ExternalClientRegistry::open(&path).unwrap();
        let (_record, token) = reg_a
            .create(
                "reload",
                vec![ExternalScope::Read],
                ProjectScope::All,
                TtlChoice::Default30Days,
            )
            .unwrap();
        let mut reg_b = ExternalClientRegistry::open(&path).unwrap();
        reg_b.revoke(&_record.id).unwrap();
        assert!(matches!(reg_a.resolve(&token), Err(ResolveError::Revoked)));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_fails_closed_on_corrupt_missing_or_insecure_store_then_recovers() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let path = registry_path(&dir);
        let mut reg_a = ExternalClientRegistry::open(&path).unwrap();
        let (record, token) = reg_a
            .create(
                "revoked-later",
                vec![ExternalScope::Read],
                ProjectScope::All,
                TtlChoice::Default30Days,
            )
            .unwrap();
        assert!(reg_a.resolve(&token).is_ok());
        // Revoke through a second handle (the CLI path); keep the revoked bytes for later.
        ExternalClientRegistry::open(&path)
            .unwrap()
            .revoke(&record.id)
            .unwrap();
        let revoked_bytes = std::fs::read(&path).unwrap();
        // Distinct mtimes make each rewrite observable regardless of clock granularity.
        let bump = |secs: u64| {
            let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            file.set_modified(std::time::SystemTime::now() + Duration::from_secs(secs))
                .unwrap();
        };

        // Malformed: clients cleared, and it stays denied (no stale cache after mtime moved on).
        std::fs::write(&path, b"{ not json").unwrap();
        bump(10);
        assert!(matches!(reg_a.resolve(&token), Err(ResolveError::Unknown)));
        assert!(reg_a.is_disabled());
        assert!(matches!(reg_a.resolve(&token), Err(ResolveError::Unknown)));

        // Repaired store (token revoked) recovers and the revocation is honored.
        std::fs::write(&path, &revoked_bytes).unwrap();
        bump(20);
        assert!(matches!(reg_a.resolve(&token), Err(ResolveError::Revoked)));
        assert!(!reg_a.is_disabled());

        // Missing store: nobody authenticates.
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(reg_a.resolve(&token), Err(ResolveError::Unknown)));

        // Insecure permissions: denied, and fixing chmod alone (mtime unchanged) recovers.
        std::fs::write(&path, &revoked_bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(reg_a.resolve(&token), Err(ResolveError::Unknown)));
        assert!(reg_a.is_disabled());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(reg_a.resolve(&token), Err(ResolveError::Revoked)));
    }
    #[test]
    fn excessive_ttl_returns_error_without_poisoning_or_issuing() {
        let dir = TempDir::new().unwrap();
        let mut reg = ExternalClientRegistry::open(registry_path(&dir)).unwrap();
        assert!(reg.create("too-long", vec![ExternalScope::Read], ProjectScope::All, TtlChoice::Days(u32::MAX)).is_err());
        assert!(reg.list().unwrap().is_empty());
        assert!(reg.create("valid", vec![ExternalScope::Read], ProjectScope::All, TtlChoice::Default30Days).is_ok());
    }

    #[test]
    fn separate_registry_writers_preserve_revocation_and_new_clients() {
        let dir = TempDir::new().unwrap();
        let path = registry_path(&dir);
        let mut first = ExternalClientRegistry::open(&path).unwrap();
        let (old, token) = first.create("old", vec![ExternalScope::Read], ProjectScope::All, TtlChoice::Default30Days).unwrap();
        first.resolve(&token).unwrap();
        let mut second = ExternalClientRegistry::open(&path).unwrap();
        second.revoke(&old.id).unwrap();
        second.create("second", vec![ExternalScope::Read], ProjectScope::All, TtlChoice::Default30Days).unwrap();
        first.last_used_flush_at = Instant::now() - Duration::from_secs(61);
        first.maybe_flush_last_used();
        first.create("first", vec![ExternalScope::Read], ProjectScope::All, TtlChoice::Default30Days).unwrap();
        assert!(matches!(second.resolve(&token), Err(ResolveError::Revoked)));
        assert_eq!(second.list().unwrap().len(), 3);
    }

}

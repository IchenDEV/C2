//! Append-only audit sink for external MCP tool invocations (no secrets, no argument bodies).

use crate::external_mcp::catalog::ToolClass;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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

pub mod reason_code {
    pub const ALLOWED: &str = "allowed";
    pub const SCOPE_DENIED: &str = "scope_denied";
    pub const PROJECT_DENIED: &str = "project_denied";
    pub const RATE_LIMITED: &str = "rate_limited";
    pub const TOOL_UNKNOWN: &str = "tool_unknown";
    pub const CREDENTIAL_EXPIRED: &str = "credential_expired";
    pub const CREDENTIAL_REVOKED: &str = "credential_revoked";
    pub const POLICY_DENIED: &str = "policy_denied";
    pub const AUDIT_SINK_FAILED: &str = "audit_sink_failed";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    #[serde(with = "rfc3339_ts")]
    pub ts: DateTime<Utc>,
    pub client_id: String,
    pub tool: String,
    pub class: ToolClassWire,
    pub allowed: bool,
    pub reason: String,
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    pub args_hash: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ToolClassWire {
    R,
    W,
    X,
    D,
}

impl From<ToolClass> for ToolClassWire {
    fn from(value: ToolClass) -> Self {
        match value {
            ToolClass::R => Self::R,
            ToolClass::W => Self::W,
            ToolClass::X => Self::X,
            ToolClass::D => Self::D,
        }
    }
}

#[derive(Debug)]
pub enum AuditError {
    Io(String),
    Serialize(String),
}

impl std::fmt::Display for AuditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(m) => write!(f, "{m}"),
            Self::Serialize(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for AuditError {}

pub trait AuditSink: Send + Sync {
    fn record(&self, record: &AuditRecord) -> Result<(), AuditError>;
}

pub struct MemoryAuditSink {
    records: Mutex<Vec<AuditRecord>>,
}

impl Default for MemoryAuditSink {
    fn default() -> Self {
        Self {
            records: Mutex::new(Vec::new()),
        }
    }
}

impl MemoryAuditSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn records(&self) -> Vec<AuditRecord> {
        self.records.lock().unwrap().clone()
    }
}

impl AuditSink for MemoryAuditSink {
    fn record(&self, record: &AuditRecord) -> Result<(), AuditError> {
        self.records.lock().unwrap().push(record.clone());
        Ok(())
    }
}

const MAX_AUDIT_BYTES: u64 = 5 * 1024 * 1024;
const MAX_ROTATED_FILES: usize = 3;

pub struct JsonlAuditSink {
    path: PathBuf,
    mutex: Mutex<()>,
}

impl JsonlAuditSink {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AuditError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AuditError::Io(format!("create audit dir: {e}")))?;
        }
        if !path.exists() {
            write_new_file(&path, &[])?;
        }
        Ok(Self {
            path,
            mutex: Mutex::new(()),
        })
    }
}

impl AuditSink for JsonlAuditSink {
    fn record(&self, record: &AuditRecord) -> Result<(), AuditError> {
        let _guard = self.mutex.lock().unwrap();
        maybe_rotate(&self.path)?;
        let line = serde_json::to_string(record)
            .map_err(|e| AuditError::Serialize(format!("audit record: {e}")))?
            + "\n";
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| AuditError::Io(format!("open audit log: {e}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
        }
        file.write_all(line.as_bytes())
            .map_err(|e| AuditError::Io(format!("append audit log: {e}")))?;
        file.sync_all()
            .map_err(|e| AuditError::Io(format!("fsync audit log: {e}")))?;
        Ok(())
    }
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), AuditError> {
    let mut file =
        File::create(path).map_err(|e| AuditError::Io(format!("create audit file: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| AuditError::Io(format!("chmod audit file: {e}")))?;
    }
    if !bytes.is_empty() {
        file.write_all(bytes)
            .map_err(|e| AuditError::Io(format!("write audit file: {e}")))?;
    }
    file.sync_all()
        .map_err(|e| AuditError::Io(format!("fsync audit file: {e}")))?;
    Ok(())
}

fn maybe_rotate(path: &Path) -> Result<(), AuditError> {
    let meta = std::fs::metadata(path).map_err(|e| AuditError::Io(e.to_string()))?;
    if meta.len() < MAX_AUDIT_BYTES {
        return Ok(());
    }
    let rotated = format!("{}.{}", path.display(), MAX_ROTATED_FILES - 1);
    let _ = std::fs::remove_file(&rotated);
    for idx in (0..MAX_ROTATED_FILES - 1).rev() {
        let from = if idx == 0 {
            path.to_path_buf()
        } else {
            PathBuf::from(format!("{}.{}", path.display(), idx))
        };
        let to = PathBuf::from(format!("{}.{}", path.display(), idx + 1));
        if from.exists() {
            std::fs::rename(&from, &to)
                .map_err(|e| AuditError::Io(format!("rotate audit log: {e}")))?;
        }
    }
    write_new_file(path, &[])?;
    Ok(())
}

/// SHA-256 hex digest of canonical JSON for tool arguments (never log raw args).
pub fn hash_arguments(arguments: &serde_json::Value) -> String {
    let canonical = canonical_json_string(arguments);
    let digest = Sha256::digest(canonical.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn canonical_json_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into()),
        serde_json::Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(canonical_json_string).collect();
            format!("[{}]", parts.join(","))
        }
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(k).unwrap(),
                        canonical_json_string(&map[k])
                    )
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
    }
}

pub fn build_audit_record(
    client_id: &str,
    tool: &str,
    class: ToolClass,
    allowed: bool,
    reason: &str,
    session_id: Option<String>,
    project_id: Option<String>,
    arguments: Option<&serde_json::Value>,
    duration_ms: u64,
) -> AuditRecord {
    AuditRecord {
        ts: Utc::now(),
        client_id: client_id.to_string(),
        tool: tool.to_string(),
        class: class.into(),
        allowed,
        reason: reason.to_string(),
        session_id,
        project_id,
        args_hash: arguments.map(hash_arguments),
        duration_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_mcp::catalog::ToolClass;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn jsonl_sink_never_writes_token_or_arguments() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("external-mcp-audit.jsonl");
        let sink = JsonlAuditSink::open(&path).unwrap();
        let secret_token = "ctmcp_super_secret_token_value";
        let args = json!({"prompt": "do not log this prompt text", "token": secret_token});
        let record = build_audit_record(
            "client-1",
            "codetwo_turn_send",
            ToolClass::X,
            true,
            reason_code::ALLOWED,
            Some("session-1".into()),
            None,
            Some(&args),
            12,
        );
        sink.record(&record).unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(!contents.contains(secret_token));
        assert!(!contents.contains("do not log"));
        assert!(contents.contains("args_hash"));
        assert!(record.args_hash.as_ref().is_some_and(|h| h.len() == 64));
    }

    #[test]
    fn memory_sink_collects_records() {
        let sink = MemoryAuditSink::new();
        let record = build_audit_record(
            "c",
            "codetwo_session_list",
            ToolClass::R,
            false,
            reason_code::SCOPE_DENIED,
            None,
            None,
            None,
            1,
        );
        sink.record(&record).unwrap();
        assert_eq!(sink.records().len(), 1);
    }

    #[test]
    fn hash_arguments_is_stable() {
        let a = json!({"b": 2, "a": 1});
        let b = json!({"a": 1, "b": 2});
        assert_eq!(hash_arguments(&a), hash_arguments(&b));
    }
}

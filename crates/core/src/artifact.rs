//! Durable, provider-neutral tool artifacts.
//!
//! The transcript stores only opaque artifact ids. Files live under C2's app-data directory,
//! are content-addressed internally for deduplication, and can only be read after a matching SQLite
//! row is found. This keeps provider-returned paths out of the frontend and out of transcript JSON.

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use image::{GenericImageView, ImageFormat, ImageReader};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::store::Store;

pub const MAX_ARTIFACT_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_ARTIFACT_PIXELS: u64 = 100_000_000;
const MAX_TEXT_OUTPUT_CHARS: usize = 262_144;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub id: String,
    pub mime_type: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolOutput {
    Text {
        text: String,
    },
    Image {
        artifact: ArtifactRef,
    },
    ResourceLink {
        name: String,
        uri: String,
        mime_type: Option<String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolSource {
    pub image_generation: bool,
    pub server: Option<String>,
    pub tool: Option<String>,
}

impl ToolSource {
    fn trusts_raw_output(&self) -> bool {
        self.server
            .as_deref()
            .is_some_and(|server| matches!(server, "codetwo_browser" | "node_repl"))
    }

    fn trusts_local_uri(&self) -> bool {
        self.image_generation || self.trusts_raw_output()
    }

    fn is_sites(&self) -> bool {
        self.server
            .as_deref()
            .is_some_and(|server| server.to_ascii_lowercase().contains("sites"))
            || self.tool.as_deref().is_some_and(|tool| {
                let tool = tool.to_ascii_lowercase();
                tool.starts_with("sites_") || tool.contains("__sites_")
            })
    }
}

#[derive(Debug, Clone, Default)]
pub struct NormalizedToolOutput {
    pub outputs: Vec<ToolOutput>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ArtifactError {
    #[error("artifact storage is unavailable")]
    Unavailable,
    #[error("artifact not found")]
    NotFound,
    #[error("artifact is too large ({0} bytes; maximum is 20 MiB)")]
    TooLarge(usize),
    #[error("artifact text preview exceeds the {0} byte limit")]
    PreviewTooLarge(usize),
    #[error("unsupported image format")]
    UnsupportedFormat,
    #[error("image dimensions exceed 100 megapixels")]
    TooManyPixels,
    #[error("invalid image: {0}")]
    InvalidImage(String),
    #[error("invalid image data: {0}")]
    InvalidData(String),
    #[error("artifact I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("artifact database: {0}")]
    Database(#[from] rusqlite::Error),
}

#[derive(Clone)]
pub struct ArtifactStore {
    root: Arc<PathBuf>,
    store: Arc<Store>,
}

impl ArtifactStore {
    pub fn from_store(store: Arc<Store>) -> Option<Self> {
        let root = store.artifact_root()?.to_path_buf();
        Some(Self {
            root: Arc::new(root),
            store,
        })
    }

    pub fn save_image(
        &self,
        bytes: &[u8],
        display_name: Option<&str>,
        session_id: &str,
        tool_call_id: &str,
    ) -> Result<ArtifactRef, ArtifactError> {
        let verified = verify_image(bytes)?;
        fs::create_dir_all(self.root.as_ref())?;
        let digest = blake3::hash(bytes).to_hex().to_string();
        let storage_name = format!("{digest}.{}", verified.extension);
        let path = self.root.join(&storage_name);
        if !path.is_file() {
            let temporary = self
                .root
                .join(format!(".{}.{}.tmp", digest, uuid::Uuid::new_v4()));
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            if let Err(error) = fs::rename(&temporary, &path) {
                let _ = fs::remove_file(&temporary);
                if !path.is_file() {
                    return Err(error.into());
                }
            }
        }

        let display_name = safe_display_name(
            display_name,
            &format!("generated-image.{}", verified.extension),
        );
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(i64::MAX as u128) as i64;
        let conn = self.store.conn.lock().unwrap();
        let existing: Option<(String, String)> = conn
            .query_row(
                "SELECT id,display_name FROM artifacts WHERE digest=?1",
                [&digest],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (id, effective_name) = match existing {
            Some(existing) => existing,
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                conn.execute(
                    "INSERT INTO artifacts
                     (id,digest,mime_type,byte_count,width,height,display_name,storage_name,created_at)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    rusqlite::params![
                        id,
                        digest,
                        verified.mime_type,
                        bytes.len() as i64,
                        verified.width as i64,
                        verified.height as i64,
                        display_name,
                        storage_name,
                        created_at,
                    ],
                )?;
                (id, display_name)
            }
        };
        conn.execute(
            "INSERT OR IGNORE INTO artifact_refs (session_id,tool_call_id,artifact_id)
             VALUES (?1,?2,?3)",
            rusqlite::params![session_id, tool_call_id, id],
        )?;
        Ok(ArtifactRef {
            id,
            mime_type: verified.mime_type.into(),
            bytes: bytes.len() as u64,
            width: verified.width,
            height: verified.height,
            display_name: effective_name,
        })
    }

    /// Store a UTF-8 text document (a scene artifact's content) through the same content-addressed
    /// blob layer as images. `width`/`height` are 0 — the documented "not an image" sentinel — so
    /// the wire shape stays compatible without a migration. `tool_call_id` is synthetic for scene
    /// captures (`scene:<artifact_key>`).
    pub fn save_document(
        &self,
        text: &str,
        mime_type: &str,
        display_name: Option<&str>,
        session_id: &str,
        tool_call_id: &str,
    ) -> Result<ArtifactRef, ArtifactError> {
        let extension = match mime_type {
            "text/markdown" => "md",
            "text/plain" => "txt",
            "text/html" => "html",
            "image/svg+xml" => "svg",
            "application/json" => "json",
            "text/csv" => "csv",
            "text/yaml" => "yaml",
            "application/xml" => "xml",
            _ => return Err(ArtifactError::UnsupportedFormat),
        };
        let bytes = text.as_bytes();
        if bytes.len() > MAX_ARTIFACT_BYTES {
            return Err(ArtifactError::TooLarge(bytes.len()));
        }
        fs::create_dir_all(self.root.as_ref())?;
        let digest = blake3::hash(bytes).to_hex().to_string();
        let storage_name = format!("{digest}.{extension}");
        let path = self.root.join(&storage_name);
        if !path.is_file() {
            let temporary = self
                .root
                .join(format!(".{}.{}.tmp", digest, uuid::Uuid::new_v4()));
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            if let Err(error) = fs::rename(&temporary, &path) {
                let _ = fs::remove_file(&temporary);
                if !path.is_file() {
                    return Err(error.into());
                }
            }
        }

        let display_name = safe_display_name(display_name, &format!("document.{extension}"));
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(i64::MAX as u128) as i64;
        let conn = self.store.conn.lock().unwrap();
        let existing: Option<(String, String)> = conn
            .query_row(
                "SELECT id,display_name FROM artifacts WHERE digest=?1",
                [&digest],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (id, effective_name) = match existing {
            Some(existing) => existing,
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                conn.execute(
                    "INSERT INTO artifacts
                     (id,digest,mime_type,byte_count,width,height,display_name,storage_name,created_at)
                     VALUES (?1,?2,?3,?4,0,0,?5,?6,?7)",
                    rusqlite::params![
                        id,
                        digest,
                        mime_type,
                        bytes.len() as i64,
                        display_name,
                        storage_name,
                        created_at,
                    ],
                )?;
                (id, display_name)
            }
        };
        conn.execute(
            "INSERT OR IGNORE INTO artifact_refs (session_id,tool_call_id,artifact_id)
             VALUES (?1,?2,?3)",
            rusqlite::params![session_id, tool_call_id, id],
        )?;
        Ok(ArtifactRef {
            id,
            mime_type: mime_type.into(),
            bytes: bytes.len() as u64,
            width: 0,
            height: 0,
            display_name: effective_name,
        })
    }

    pub fn get(&self, id: &str) -> Result<Vec<u8>, ArtifactError> {
        if id.len() > 128 || id.is_empty() {
            return Err(ArtifactError::NotFound);
        }
        let conn = self.store.conn.lock().unwrap();
        let row: Option<(String, i64)> = conn
            .query_row(
                "SELECT storage_name,byte_count FROM artifacts WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        drop(conn);
        let (storage_name, expected) = row.ok_or(ArtifactError::NotFound)?;
        if storage_name.contains('/')
            || storage_name.contains('\\')
            || storage_name.starts_with('.')
        {
            return Err(ArtifactError::NotFound);
        }
        let data = fs::read(self.root.join(storage_name))?;
        if data.len() as i64 != expected || data.len() > MAX_ARTIFACT_BYTES {
            return Err(ArtifactError::InvalidData(
                "stored byte count mismatch".into(),
            ));
        }
        Ok(data)
    }

    /// Metadata for one stored artifact, without reading its bytes.
    pub fn metadata(&self, id: &str) -> Result<ArtifactRef, ArtifactError> {
        if id.len() > 128 || id.is_empty() {
            return Err(ArtifactError::NotFound);
        }
        let conn = self.store.conn.lock().unwrap();
        let reference = conn
            .query_row(
                "SELECT id,mime_type,byte_count,width,height,display_name FROM artifacts WHERE id=?1",
                [id],
                |row| {
                    Ok(ArtifactRef {
                        id: row.get(0)?,
                        mime_type: row.get(1)?,
                        bytes: row.get::<_, i64>(2)?.max(0) as u64,
                        width: row.get::<_, i64>(3)?.max(0) as u32,
                        height: row.get::<_, i64>(4)?.max(0) as u32,
                        display_name: row.get(5)?,
                    })
                },
            )
            .optional()?;
        reference.ok_or(ArtifactError::NotFound)
    }

    pub fn path_for_reveal(&self, id: &str) -> Result<PathBuf, ArtifactError> {
        let conn = self.store.conn.lock().unwrap();
        let storage_name: Option<String> = conn
            .query_row(
                "SELECT storage_name FROM artifacts WHERE id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        let storage_name = storage_name.ok_or(ArtifactError::NotFound)?;
        if storage_name.contains('/')
            || storage_name.contains('\\')
            || storage_name.starts_with('.')
        {
            return Err(ArtifactError::NotFound);
        }
        Ok(self.root.join(storage_name))
    }

    pub fn save_as(&self, id: &str, destination: &Path) -> Result<(), ArtifactError> {
        let data = self.get(id)?;
        fs::write(destination, data)?;
        Ok(())
    }

    /// A bounded UTF-8 view of a text artifact for preview. Binary bodies are rejected rather than
    /// lossily decoded, and an oversized body is rejected rather than silently truncated.
    pub fn read_text(&self, id: &str, max_bytes: usize) -> Result<String, ArtifactError> {
        let data = self.get(id)?;
        if data.len() > max_bytes {
            return Err(ArtifactError::PreviewTooLarge(max_bytes));
        }
        String::from_utf8(data)
            .map_err(|_| ArtifactError::InvalidData("artifact is not UTF-8 text".into()))
    }

    /// Sessions that referenced this blob (the only app-level association of an artifact to task
    /// data), used to apply the share policy to an id presented without its session.
    pub fn referencing_sessions(&self, artifact_id: &str) -> Result<Vec<String>, ArtifactError> {
        let conn = self.store.conn.lock().unwrap();
        let mut statement =
            conn.prepare("SELECT DISTINCT session_id FROM artifact_refs WHERE artifact_id=?1")?;
        let rows = statement.query_map([artifact_id], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(ArtifactError::from)
    }

    /// The artifacts a session produced, newest first. Joined through `artifact_refs`, so a blob
    /// shared with another session is only returned for sessions that actually referenced it.
    pub fn list_for_session(&self, session_id: &str) -> Result<Vec<ArtifactRef>, ArtifactError> {
        let conn = self.store.conn.lock().unwrap();
        let mut statement = conn.prepare(
            "SELECT a.id,a.mime_type,a.byte_count,a.width,a.height,a.display_name
             FROM artifact_refs r JOIN artifacts a ON a.id=r.artifact_id
             WHERE r.session_id=?1
             ORDER BY r.rowid DESC",
        )?;
        let rows = statement.query_map([session_id], |row| {
            Ok(ArtifactRef {
                id: row.get(0)?,
                mime_type: row.get(1)?,
                bytes: row.get::<_, i64>(2)?.max(0) as u64,
                width: row.get::<_, i64>(3)?.max(0) as u32,
                height: row.get::<_, i64>(4)?.max(0) as u32,
                display_name: row.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(ArtifactError::from)
    }
}

/// Default preview bound for [`ArtifactStore::read_text`].
pub const MAX_ARTIFACT_PREVIEW_BYTES: usize = 1024 * 1024;

pub struct ToolOutputNormalizer {
    artifacts: Option<ArtifactStore>,
}

impl ToolOutputNormalizer {
    pub fn new(artifacts: Option<ArtifactStore>) -> Self {
        Self { artifacts }
    }

    pub fn normalize(
        &self,
        content: Option<&Value>,
        raw_output: Option<&Value>,
        source: &ToolSource,
        session_id: &str,
        tool_call_id: &str,
    ) -> NormalizedToolOutput {
        let mut result = NormalizedToolOutput::default();
        if let Some(content) = content {
            if source.is_sites() {
                collect_sites_resource_links(content, 0, &mut result.outputs);
            }
            self.visit(
                content,
                source,
                session_id,
                tool_call_id,
                !source.trusts_raw_output() && !source.is_sites(),
                &mut result,
            );
        }
        if source.is_sites() {
            if let Some(raw_output) = raw_output {
                collect_sites_resource_links(raw_output, 0, &mut result.outputs);
            }
        }
        if source.trusts_raw_output() {
            if let Some(content) = raw_output.and_then(|value| value.pointer("/result/content")) {
                // Raw MCP output is a compatibility escape hatch for images and small facets, not
                // a second transcript. In particular, never persist DOM, JavaScript, cookies, or
                // arbitrary tool text from this path.
                self.visit(
                    content,
                    source,
                    session_id,
                    tool_call_id,
                    false,
                    &mut result,
                );
            }
        }
        deduplicate_outputs(&mut result.outputs);
        result
    }

    fn visit(
        &self,
        value: &Value,
        source: &ToolSource,
        session_id: &str,
        tool_call_id: &str,
        retain_text: bool,
        result: &mut NormalizedToolOutput,
    ) {
        match value {
            Value::Array(values) => {
                for value in values {
                    self.visit(value, source, session_id, tool_call_id, retain_text, result);
                }
            }
            Value::Object(object) => {
                if object.get("type").and_then(Value::as_str) == Some("content") {
                    if let Some(content) = object.get("content") {
                        self.visit(
                            content,
                            source,
                            session_id,
                            tool_call_id,
                            retain_text,
                            result,
                        );
                    }
                    return;
                }
                match object.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if retain_text {
                            if let Some(text) = object.get("text").and_then(Value::as_str) {
                                result.outputs.push(ToolOutput::Text {
                                    text: text.chars().take(MAX_TEXT_OUTPUT_CHARS).collect(),
                                });
                            }
                        }
                    }
                    Some("image") => match image_bytes(object, source.trusts_local_uri()) {
                        Ok((bytes, name)) => match &self.artifacts {
                            Some(store) => match store.save_image(
                                &bytes,
                                name.as_deref(),
                                session_id,
                                tool_call_id,
                            ) {
                                Ok(artifact) => result.outputs.push(ToolOutput::Image { artifact }),
                                Err(error) => result.warnings.push(error.to_string()),
                            },
                            None => result.warnings.push(ArtifactError::Unavailable.to_string()),
                        },
                        Err(error) => result.warnings.push(error.to_string()),
                    },
                    Some("resource_link") | Some("resourceLink") => {
                        let uri = object.get("uri").and_then(Value::as_str);
                        let name = object.get("name").and_then(Value::as_str);
                        if let (Some(uri), Some(name)) = (uri, name) {
                            result.outputs.push(ToolOutput::ResourceLink {
                                name: name.chars().take(512).collect(),
                                uri: uri.chars().take(8_192).collect(),
                                mime_type: object
                                    .get("mimeType")
                                    .or_else(|| object.get("mime_type"))
                                    .and_then(Value::as_str)
                                    .map(|value| value.chars().take(128).collect()),
                            });
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

struct VerifiedImage {
    mime_type: &'static str,
    extension: &'static str,
    width: u32,
    height: u32,
}

fn verify_image(bytes: &[u8]) -> Result<VerifiedImage, ArtifactError> {
    if bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(ArtifactError::TooLarge(bytes.len()));
    }
    let format = image::guess_format(bytes).map_err(|_| ArtifactError::UnsupportedFormat)?;
    let (mime_type, extension) = match format {
        ImageFormat::Png => ("image/png", "png"),
        ImageFormat::Jpeg => ("image/jpeg", "jpg"),
        ImageFormat::WebP => ("image/webp", "webp"),
        ImageFormat::Gif => ("image/gif", "gif"),
        _ => return Err(ArtifactError::UnsupportedFormat),
    };
    let reader = ImageReader::with_format(Cursor::new(bytes), format);
    let (width, height) = reader
        .into_dimensions()
        .map_err(|error| ArtifactError::InvalidImage(error.to_string()))?;
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(ArtifactError::TooManyPixels)?;
    if pixels > MAX_ARTIFACT_PIXELS {
        return Err(ArtifactError::TooManyPixels);
    }
    let decoded = image::load_from_memory_with_format(bytes, format)
        .map_err(|error| ArtifactError::InvalidImage(error.to_string()))?;
    if decoded.dimensions() != (width, height) {
        return Err(ArtifactError::InvalidImage(
            "inconsistent dimensions".into(),
        ));
    }
    Ok(VerifiedImage {
        mime_type,
        extension,
        width,
        height,
    })
}

fn image_bytes(
    object: &serde_json::Map<String, Value>,
    allow_uri: bool,
) -> Result<(Vec<u8>, Option<String>), ArtifactError> {
    if let Some(data) = object.get("data").and_then(Value::as_str) {
        let encoded = data
            .split_once(',')
            .filter(|(prefix, _)| prefix.starts_with("data:") && prefix.ends_with(";base64"))
            .map(|(_, encoded)| encoded)
            .unwrap_or(data);
        let estimate = encoded.len().saturating_mul(3) / 4;
        if estimate > MAX_ARTIFACT_BYTES {
            return Err(ArtifactError::TooLarge(estimate));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| ArtifactError::InvalidData(error.to_string()))?;
        return Ok((bytes, None));
    }
    if allow_uri {
        if let Some(uri) = object.get("uri").and_then(Value::as_str) {
            let path = if let Some(path) = uri.strip_prefix("file://") {
                PathBuf::from(path)
            } else if !uri.contains("://") {
                PathBuf::from(uri)
            } else {
                return Err(ArtifactError::InvalidData(
                    "only local image URIs are accepted".into(),
                ));
            };
            let metadata = fs::metadata(&path)?;
            if metadata.len() > MAX_ARTIFACT_BYTES as u64 {
                return Err(ArtifactError::TooLarge(metadata.len() as usize));
            }
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .map(str::to_string);
            return Ok((fs::read(path)?, name));
        }
    }
    Err(ArtifactError::InvalidData(
        "image has no usable data".into(),
    ))
}

fn safe_display_name(name: Option<&str>, fallback: &str) -> String {
    let Some(name) = name else {
        return fallback.to_string();
    };
    let name = Path::new(name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let filtered: String = name
        .chars()
        .filter(|ch| !ch.is_control())
        .take(128)
        .collect();
    if filtered.trim().is_empty() {
        fallback.to_string()
    } else {
        filtered
    }
}

fn deduplicate_outputs(outputs: &mut Vec<ToolOutput>) {
    let mut image_ids = std::collections::HashSet::new();
    let mut resource_links = std::collections::HashSet::new();
    outputs.retain(|output| match output {
        ToolOutput::Image { artifact } => image_ids.insert(artifact.id.clone()),
        ToolOutput::ResourceLink { name, uri, .. } => {
            resource_links.insert((name.clone(), uri.clone()))
        }
        _ => true,
    });
}

fn collect_sites_resource_links(value: &Value, depth: usize, outputs: &mut Vec<ToolOutput>) {
    if depth > 8 {
        return;
    }
    match value {
        Value::Array(values) => {
            for value in values {
                collect_sites_resource_links(value, depth + 1, outputs);
            }
        }
        Value::Object(object) => {
            for (key, value) in object {
                let label = match key.as_str() {
                    "url" => Some("Sites production deployment"),
                    "current_live_url" => Some("Live site"),
                    "current_preview_url" => Some("Saved version preview"),
                    _ => None,
                };
                if let (Some(label), Some(uri)) = (label, value.as_str()) {
                    if is_safe_sites_url(uri) {
                        outputs.push(ToolOutput::ResourceLink {
                            name: label.into(),
                            uri: uri.chars().take(8_192).collect(),
                            mime_type: Some("text/html".into()),
                        });
                    }
                }
                collect_sites_resource_links(value, depth + 1, outputs);
            }
        }
        Value::String(text) => {
            if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                collect_sites_resource_links(&parsed, depth + 1, outputs);
            }
        }
        _ => {}
    }
}

fn is_safe_sites_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn png() -> Vec<u8> {
        let image = image::DynamicImage::new_rgba8(2, 3);
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn image_output_is_validated_deduplicated_and_recoverable() {
        let dir = tempdir().unwrap();
        let store = Arc::new(Store::open(dir.path().join("codetwo.db").to_str().unwrap()).unwrap());
        let artifacts = ArtifactStore::from_store(store).unwrap();
        let normalizer = ToolOutputNormalizer::new(Some(artifacts.clone()));
        let data = base64::engine::general_purpose::STANDARD.encode(png());
        let content = json!([
            {"type":"content","content":{"type":"image","data":data,"mimeType":"image/png"}},
            {"type":"content","content":{"type":"image","data":data,"mimeType":"image/png"}}
        ]);
        let result = normalizer.normalize(
            Some(&content),
            None,
            &ToolSource {
                image_generation: true,
                ..Default::default()
            },
            "session-1",
            "tool-1",
        );
        assert!(result.warnings.is_empty());
        let [ToolOutput::Image { artifact }] = result.outputs.as_slice() else {
            panic!("expected one image")
        };
        assert_eq!((artifact.width, artifact.height), (2, 3));
        assert_eq!(artifacts.get(&artifact.id).unwrap(), png());
    }

    #[test]
    fn sites_output_keeps_only_safe_hosted_links_and_drops_credentials() {
        let normalizer = ToolOutputNormalizer::new(None);
        let content = json!([{
            "type": "content",
            "content": {
                "type": "text",
                "text": r#"{"result":{"url":"https://example.sites.openai.com","token":"do-not-store","remote_url":"https://git.example.test/repo"}}"#
            }
        }]);
        let raw_output = json!({
            "result": {
                "current_live_url": "https://live.example.test",
                "current_preview_url": "javascript:alert(1)",
                "source_repository_credential": { "token": "also-do-not-store" }
            }
        });
        let source = ToolSource {
            server: Some("codex_apps".into()),
            tool: Some("sites_deploy_site_version".into()),
            ..Default::default()
        };

        let result = normalizer.normalize(
            Some(&content),
            Some(&raw_output),
            &source,
            "session",
            "tool",
        );

        assert_eq!(
            result.outputs,
            vec![
                ToolOutput::ResourceLink {
                    name: "Sites production deployment".into(),
                    uri: "https://example.sites.openai.com".into(),
                    mime_type: Some("text/html".into()),
                },
                ToolOutput::ResourceLink {
                    name: "Live site".into(),
                    uri: "https://live.example.test".into(),
                    mime_type: Some("text/html".into()),
                },
            ]
        );
        let serialized = serde_json::to_string(&result.outputs).unwrap();
        assert!(!serialized.contains("do-not-store"));
        assert!(!serialized.contains("remote_url"));
        assert!(!serialized.contains("javascript:"));
    }

    #[test]
    fn untrusted_raw_output_is_not_retained() {
        let normalizer = ToolOutputNormalizer::new(None);
        let raw = json!({"result":{"content":[{"type":"text","text":"cookie=secret"}]}});
        let result = normalizer.normalize(
            None,
            Some(&raw),
            &ToolSource {
                server: Some("arbitrary_mcp".into()),
                ..Default::default()
            },
            "session-1",
            "tool-1",
        );
        assert!(result.outputs.is_empty());
    }

    #[test]
    fn trusted_raw_output_still_does_not_persist_text() {
        let normalizer = ToolOutputNormalizer::new(None);
        let raw = json!({"result":{"content":[{"type":"text","text":"DOM and cookie material"}]}});
        let result = normalizer.normalize(
            None,
            Some(&raw),
            &ToolSource {
                server: Some("codetwo_browser".into()),
                ..Default::default()
            },
            "session-1",
            "tool-1",
        );
        assert!(result.outputs.is_empty());
    }

    #[test]
    fn save_document_dedupes_and_uses_zero_dimension_sentinel() {
        let dir = tempdir().unwrap();
        let store = Arc::new(Store::open(dir.path().join("codetwo.db").to_str().unwrap()).unwrap());
        let artifacts = ArtifactStore::from_store(store).unwrap();

        let first = artifacts
            .save_document("# Plan\n\n- [ ] step", "text/markdown", None, "s1", "scene:plan")
            .unwrap();
        assert_eq!((first.width, first.height), (0, 0));
        assert_eq!(first.mime_type, "text/markdown");
        assert_eq!(first.display_name, "document.md");
        assert_eq!(
            artifacts.get(&first.id).unwrap(),
            b"# Plan\n\n- [ ] step".to_vec()
        );

        // Same content again — content addressing must return the same artifact id.
        let second = artifacts
            .save_document("# Plan\n\n- [ ] step", "text/markdown", None, "s1", "scene:plan")
            .unwrap();
        assert_eq!(second.id, first.id);
    }

    #[test]
    fn save_document_accepts_the_widened_allow_list_and_rejects_the_rest() {
        let dir = tempdir().unwrap();
        let store = Arc::new(Store::open(dir.path().join("codetwo.db").to_str().unwrap()).unwrap());
        let artifacts = ArtifactStore::from_store(store).unwrap();

        for (mime, extension) in [
            ("text/html", "html"),
            ("image/svg+xml", "svg"),
            ("application/json", "json"),
            ("text/csv", "csv"),
            ("text/yaml", "yaml"),
            ("application/xml", "xml"),
        ] {
            let body = format!("<{mime}>");
            let saved = artifacts
                .save_document(&body, mime, None, "s1", &format!("t-{extension}"))
                .unwrap();
            assert_eq!(saved.mime_type, mime);
            assert_eq!(saved.display_name, format!("document.{extension}"));
            assert_eq!(artifacts.read_text(&saved.id, 4096).unwrap(), body);
        }

        assert!(matches!(
            artifacts.save_document("%PDF", "application/pdf", None, "s1", "t1"),
            Err(ArtifactError::UnsupportedFormat)
        ));
    }

    #[test]
    fn read_text_rejects_binary_and_oversized_bodies() {
        let dir = tempdir().unwrap();
        let store = Arc::new(Store::open(dir.path().join("codetwo.db").to_str().unwrap()).unwrap());
        let artifacts = ArtifactStore::from_store(store).unwrap();

        let text = artifacts
            .save_document("hello", "text/plain", None, "s1", "t1")
            .unwrap();
        assert_eq!(artifacts.read_text(&text.id, 4096).unwrap(), "hello");
        assert!(matches!(
            artifacts.read_text(&text.id, 3),
            Err(ArtifactError::PreviewTooLarge(3))
        ));

        // A raster image is stored as binary; a text preview must refuse it, not lossily decode.
        let image = artifacts
            .save_image(&png(), Some("pixel.png"), "s1", "t2")
            .unwrap();
        assert!(matches!(
            artifacts.read_text(&image.id, MAX_ARTIFACT_PREVIEW_BYTES),
            Err(ArtifactError::InvalidData(_))
        ));
    }

    #[test]
    fn list_for_session_returns_only_that_sessions_artifacts() {
        let dir = tempdir().unwrap();
        let store = Arc::new(Store::open(dir.path().join("codetwo.db").to_str().unwrap()).unwrap());
        let artifacts = ArtifactStore::from_store(store).unwrap();

        artifacts
            .save_document("one", "text/plain", Some("one.txt"), "s1", "t1")
            .unwrap();
        artifacts
            .save_document("two", "text/markdown", Some("two.md"), "s1", "t2")
            .unwrap();
        artifacts
            .save_document("other", "text/plain", Some("other.txt"), "s2", "t3")
            .unwrap();

        let listed = artifacts.list_for_session("s1").unwrap();
        assert_eq!(listed.len(), 2);
        let names: Vec<&str> = listed.iter().map(|a| a.display_name.as_str()).collect();
        assert!(names.contains(&"one.txt") && names.contains(&"two.md"));
        assert!(artifacts.list_for_session("nobody").unwrap().is_empty());
    }

    #[test]
    fn malformed_image_is_a_warning_not_a_text_copy() {
        let normalizer = ToolOutputNormalizer::new(None);
        let result = normalizer.normalize(
            Some(&json!([{"type":"image","data":"not-base64"}])),
            None,
            &ToolSource::default(),
            "session-1",
            "tool-1",
        );
        assert!(result.outputs.is_empty());
        assert_eq!(result.warnings.len(), 1);
    }
}

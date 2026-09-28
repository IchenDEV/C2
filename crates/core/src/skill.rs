//! Skills — the product differentiator.
//!
//! A "skill" is a reusable building block the user inserts into the document editor with the `/`
//! picker and combines inline. Per the locked decision, skills span four kinds:
//!   - `Fragment`   — a reusable markdown snippet (a role, constraints, an output format).
//!   - `AgentSkill` — a reference to a provider-native Agent Skill (Claude Code's SKILL.md), with an
//!                    optional inline fallback for providers that don't support native skills.
//!   - `Mcp`        — an MCP server to attach to the session (config, not prompt text).
//!   - `Macro`      — a saved, parameterized prompt template with `{{slot}}` variables.
//!
//! The [`compile`] function walks a document ([`DocBlock`]s) and lowers it into a [`CompiledPrompt`]:
//! the markdown prompt to send in ACP `session/prompt`, plus the MCP servers and agent-skills to
//! configure on `session/new`. Keeping the compiler in the Rust core means the TUI reuses it verbatim.

use base64::Engine as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;

use crate::canvas::{
    resolve_prompt_payload, CanvasFeatureGate, CanvasPixelPolicy, CanvasPromptPayload,
    CanvasProviderImageCapability, CanvasRef,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillKind {
    Fragment,
    AgentSkill,
    Subagent,
    Mcp,
    Macro,
}

/// An MCP server definition attached by an `Mcp` skill and passed through in `session/new`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServer {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(flatten)]
    pub transport: McpTransport,
}

/// `untagged` preserves the legacy saved stdio shape while adding remote HTTP/SSE servers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpTransport {
    Stdio {
        command: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
    Http {
        url: String,
        headers: Vec<(String, String)>,
    },
    Sse {
        url: String,
        headers: Vec<(String, String)>,
    },
}

impl Serialize for McpTransport {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct Stdio<'a> {
            command: &'a str,
            args: &'a [String],
            env: &'a [(String, String)],
        }
        #[derive(Serialize)]
        struct Remote<'a> {
            #[serde(rename = "type")]
            transport: &'a str,
            url: &'a str,
            headers: &'a [(String, String)],
        }

        match self {
            McpTransport::Stdio { command, args, env } => {
                Stdio { command, args, env }.serialize(serializer)
            }
            McpTransport::Http { url, headers } => Remote {
                transport: "http",
                url,
                headers,
            }
            .serialize(serializer),
            McpTransport::Sse { url, headers } => Remote {
                transport: "sse",
                url,
                headers,
            }
            .serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for McpTransport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Stored {
            Stdio {
                command: String,
                #[serde(default)]
                args: Vec<String>,
                #[serde(default)]
                env: Vec<(String, String)>,
            },
            Remote {
                #[serde(rename = "type", default = "default_http_transport")]
                transport: String,
                url: String,
                #[serde(default)]
                headers: Vec<(String, String)>,
            },
        }

        match Stored::deserialize(deserializer)? {
            Stored::Stdio { command, args, env } => Ok(McpTransport::Stdio { command, args, env }),
            Stored::Remote {
                transport,
                url,
                headers,
            } => match transport.as_str() {
                "http" | "streamable-http" => Ok(McpTransport::Http { url, headers }),
                "sse" => Ok(McpTransport::Sse { url, headers }),
                other => Err(serde::de::Error::custom(format!(
                    "unsupported MCP transport {other}"
                ))),
            },
        }
    }
}

fn default_http_transport() -> String {
    "http".into()
}

impl McpServer {
    /// Shape this server the way ACP `session/new` expects (`mcpServers[]`).
    pub fn to_acp_json(&self) -> serde_json::Value {
        let mut value = match &self.transport {
            McpTransport::Stdio { command, args, env } => serde_json::json!({
                "name": self.name,
                "command": command,
                "args": args,
                "env": env.iter()
                    .map(|(k, v)| serde_json::json!({ "name": k, "value": v }))
                    .collect::<Vec<_>>(),
            }),
            McpTransport::Http { url, headers } => serde_json::json!({
                "name": self.name,
                "type": "http",
                "url": url,
                "headers": headers.iter()
                    .map(|(k, v)| serde_json::json!({ "name": k, "value": v }))
                    .collect::<Vec<_>>(),
            }),
            McpTransport::Sse { url, headers } => serde_json::json!({
                "name": self.name,
                "type": "sse",
                "url": url,
                "headers": headers.iter()
                    .map(|(k, v)| serde_json::json!({ "name": k, "value": v }))
                    .collect::<Vec<_>>(),
            }),
        };
        if let (Some(cwd), Some(object)) = (&self.cwd, value.as_object_mut()) {
            object.insert("cwd".into(), serde_json::Value::String(cwd.clone()));
        }
        value
    }
}

/// One typed fill-in slot — the shared vocabulary between Macro skills and scene `brief.slots`
/// (Agent Scenes 1.0.0 slot object; see `crates/core/schemas/agent-scenes/1.0.0/scene.schema.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotDef {
    pub id: String,
    /// Empty → UI falls back to the id (legacy macros); scene validation requires non-empty.
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub kind: SlotKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    #[default]
    Text,
    Multiline,
    Select,
    File,
    Artifact,
}

impl SlotDef {
    /// A plain text slot with no label — what a legacy `slots: ["style"]` entry migrates to.
    pub fn text(id: impl Into<String>) -> Self {
        SlotDef {
            id: id.into(),
            label: String::new(),
            kind: SlotKind::Text,
            options: Vec::new(),
            required: false,
            default: None,
        }
    }
}

/// Legacy macro definitions saved `slots` as bare id strings; keep those constructions compiling.
impl From<String> for SlotDef {
    fn from(id: String) -> Self {
        SlotDef::text(id)
    }
}

impl From<&str> for SlotDef {
    fn from(id: &str) -> Self {
        SlotDef::text(id)
    }
}

/// Backward-compatible `Macro.slots` deserializer: an on-disk entry may be a bare id string
/// (legacy) or a full slot object (current). Serialization always writes the object shape.
fn deserialize_slots<'de, D>(deserializer: D) -> Result<Vec<SlotDef>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SlotDefOrName {
        Def(SlotDef),
        Name(String),
    }

    let raw = Vec::<SlotDefOrName>::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .map(|entry| match entry {
            SlotDefOrName::Def(def) => def,
            SlotDefOrName::Name(id) => SlotDef::text(id),
        })
        .collect())
}

/// True for a whitespace-free token that looks like a workspace file path: contains a `/`, is not
/// a URL, and its last segment carries a short alphanumeric extension (`src/main.rs`, `a/b.tsx`).
fn path_like(value: &str) -> bool {
    if value.contains("://") || value.contains(char::is_whitespace) || !value.contains('/') {
        return false;
    }
    let name = value.rsplit('/').next().unwrap_or("");
    match name.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty()
                && (1..=8).contains(&ext.len())
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
        }
        None => false,
    }
}

/// Sentence punctuation that should not ride along when a bare token becomes a slot value.
fn is_trailing_punct(c: char) -> bool {
    matches!(c, ',' | '.' | ';' | ':' | ')' | ']' | '}' | '!' | '?')
}

/// Whitespace-collapsed, length-capped label for a proposed slot.
fn proposed_label(value: &str) -> String {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut label: String = collapsed.chars().take(32).collect();
    if collapsed.chars().count() > 32 {
        label.push('…');
    }
    label
}

/// Heuristic "save as template" proposal (R2). Quoted strings, inline code spans, and
/// path-looking tokens become `{{slot-N}}` slots (kind `file` for paths, `text` otherwise; the
/// original value is kept as the slot default). Identical values dedupe to one slot; at most six
/// distinct slots are proposed — later candidates are left verbatim. No model call: this is the
/// v1 chokepoint the frontend degrades over identically when the command is missing.
pub fn propose_macro_slots(text: &str) -> (String, Vec<SlotDef>) {
    const MAX_SLOTS: usize = 6;
    const MAX_VALUE_LEN: usize = 120;
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut slots: Vec<SlotDef> = Vec::new();

    // Existing slot for the value, or a fresh `slot-N` while under the cap. None → leave as-is.
    let token_for = |value: &str, slots: &mut Vec<SlotDef>| -> Option<String> {
        if let Some(existing) = slots.iter().find(|s| s.default.as_deref() == Some(value)) {
            return Some(format!("{{{{{}}}}}", existing.id));
        }
        if slots.len() >= MAX_SLOTS {
            return None;
        }
        let id = format!("slot-{}", slots.len() + 1);
        let kind = if path_like(value) {
            SlotKind::File
        } else {
            SlotKind::Text
        };
        slots.push(SlotDef {
            id: id.clone(),
            label: proposed_label(value),
            kind,
            options: Vec::new(),
            required: false,
            default: Some(value.to_string()),
        });
        Some(format!("{{{{{id}}}}}"))
    };

    // ASCII delimiters never occur inside multi-byte UTF-8 sequences, so byte scanning with
    // slicing at delimiter positions stays on char boundaries.
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'`' || b == b'"' || b == b'\'' {
            // A `'` only opens a quote at a word boundary — apostrophes stay prose.
            let opens = b != b'\'' || i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            let close = if opens {
                (i + 1..bytes.len())
                    .take_while(|&j| bytes[j] != b'\n')
                    .find(|&j| bytes[j] == b)
            } else {
                None
            };
            if let Some(end) = close {
                let inner = &text[i + 1..end];
                let closes = b != b'\''
                    || bytes
                        .get(end + 1)
                        .is_none_or(|c| !c.is_ascii_alphanumeric());
                if closes && !inner.trim().is_empty() && inner.len() <= MAX_VALUE_LEN {
                    if let Some(token) = token_for(inner, &mut slots) {
                        // Keep the delimiters: the template reads as the prompt did.
                        out.push(b as char);
                        out.push_str(&token);
                        out.push(b as char);
                        i = end + 1;
                        continue;
                    }
                }
            }
            out.push(b as char);
            i += 1;
            continue;
        }
        if b.is_ascii_whitespace() {
            out.push(b as char);
            i += 1;
            continue;
        }
        // Bare token; stop at quote/code delimiters so `path:"a/b.rs"` still finds its quote.
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && !matches!(bytes[i], b'`' | b'"')
        {
            i += 1;
        }
        let token = &text[start..i];
        let trimmed = token.trim_end_matches(is_trailing_punct);
        if path_like(trimmed) && trimmed.len() <= MAX_VALUE_LEN {
            if let Some(slot_token) = token_for(trimmed, &mut slots) {
                out.push_str(&slot_token);
                out.push_str(&token[trimmed.len()..]);
                continue;
            }
        }
        out.push_str(token);
    }

    (out, slots)
}

/// A reusable specialist supplied by a plugin. Execution belongs to the provider's native
/// subagent capability; C2 only supplies the contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentDefinition {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
}

/// Kind-specific payload for a skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillPayload {
    Fragment {
        text: String,
    },
    AgentSkill {
        skill_ref: String,
        inline_text: Option<String>,
    },
    Subagent {
        agent: SubagentDefinition,
    },
    Mcp {
        server: McpServer,
    },
    Macro {
        template: String,
        #[serde(default, deserialize_with = "deserialize_slots")]
        slots: Vec<SlotDef>,
    },
}

impl SkillPayload {
    pub fn kind(&self) -> SkillKind {
        match self {
            SkillPayload::Fragment { .. } => SkillKind::Fragment,
            SkillPayload::AgentSkill { .. } => SkillKind::AgentSkill,
            SkillPayload::Subagent { .. } => SkillKind::Subagent,
            SkillPayload::Mcp { .. } => SkillKind::Mcp,
            SkillPayload::Macro { .. } => SkillKind::Macro,
        }
    }
}

/// A library entry. Stored on disk (`~/.config/codetwo/skills/`) in M1; in-memory here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: Option<String>,
    /// Where this skill came from (for example `GitHub · owner/repo`). Old on-disk skills omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub payload: SkillPayload,
}

impl Skill {
    pub fn kind(&self) -> SkillKind {
        self.payload.kind()
    }
}

/// In-memory skill library with id lookup. Backed by the on-disk store in M1.
#[derive(Debug, Clone, Default)]
pub struct SkillLibrary {
    by_id: HashMap<String, Skill>,
}

impl SkillLibrary {
    pub fn new(skills: impl IntoIterator<Item = Skill>) -> Self {
        let mut by_id = HashMap::new();
        for s in skills {
            by_id.insert(s.id.clone(), s);
        }
        Self { by_id }
    }
    pub fn get(&self, id: &str) -> Option<&Skill> {
        self.by_id.get(id)
    }
    pub fn all(&self) -> impl Iterator<Item = &Skill> {
        self.by_id.values()
    }

    /// Load every `*.json` file in `dir` as a [`Skill`]. A missing directory yields an empty library
    /// (first run); a malformed file is logged and skipped rather than failing the whole load.
    pub fn load_dir(dir: &std::path::Path) -> std::io::Result<SkillLibrary> {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SkillLibrary::default())
            }
            Err(e) => return Err(e),
        };
        let mut skills = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let data = std::fs::read_to_string(&path)?;
                match serde_json::from_str::<Skill>(&data) {
                    Ok(skill) => skills.push(skill),
                    Err(e) => tracing::warn!("skill {path:?}: {e}"),
                }
            }
        }
        Ok(SkillLibrary::new(skills))
    }

    /// Write a skill to `dir/<id>.json` (creating `dir`). Used by the library-management UI.
    pub fn save_to_dir(dir: &std::path::Path, skill: &Skill) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let json = serde_json::to_string_pretty(skill)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(dir.join(format!("{}.json", skill.id)), json)
    }

    /// Remove `dir/<id>.json`. Missing file is not an error.
    pub fn delete_from_dir(dir: &std::path::Path, id: &str) -> std::io::Result<()> {
        match std::fs::remove_file(dir.join(format!("{id}.json"))) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

/// One block of the document the user composed. This is the neutral shape the BlockNote editor
/// serializes into (text blocks + skill blocks) and the shape the TUI composer produces too.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DocBlock {
    Text {
        text: String,
    },
    Skill {
        skill_id: String,
        #[serde(default)]
        params: HashMap<String, String>,
    },
    /// An `@`-mentioned workspace file; its contents are inlined as context at compile time.
    File {
        path: String,
    },
    /// An attached image; sent to the agent as an ACP image content block.
    Image {
        path: String,
    },
    /// A private desktop capture addressed only by its opaque app-owned UUID. The image and
    /// accessibility snapshot are resolved from the application data directory at compile time.
    Appshot {
        id: String,
        #[serde(default)]
        title: Option<String>,
    },
    /// A user-selected or pasted image stored in the private attachment directory. Only the
    /// opaque id is authoritative; `name` is display text and is re-read from trusted metadata.
    Attachment {
        id: String,
        #[serde(default)]
        name: Option<String>,
    },
    /// A frozen app-owned Canvas revision.  Only the immutable reference and explicit image
    /// policy cross the document boundary; scene JSON and pixels remain in the core store.
    Canvas {
        id: String,
        frozen_revision: u64,
        #[serde(default)]
        pixel_policy: CanvasPixelPolicy,
    },
    /// An `@`-mentioned past chat; its transcript is inlined as context at compile time, so a
    /// planning conversation can be referenced from the document that implements it.
    Session {
        session_id: String,
        /// Optional user-part sequence that caps the referenced chat at one completed turn.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        through_seq: Option<i64>,
    },
    /// A stored scene-artifact version; its content is inlined as labeled context at compile time.
    Artifact {
        record_id: i64,
    },
    /// A referenced issue-tracker item (delegation provenance). The snapshot is embedded at
    /// insert time — the composer already fetched it — so compiling is offline-safe and an issue
    /// mention never lands in `unresolved`.
    Issue {
        /// `github` | `linear`.
        source: String,
        id: String,
        #[serde(default)]
        title: String,
        #[serde(default)]
        url: String,
        #[serde(default)]
        body: String,
    },
}

/// Plain, user-authored representation of a composed document.
///
/// This is the durable/searchable prompt record. It intentionally excludes project rules, skill
/// expansion, file contents and referenced-chat contents that are added only while compiling for
/// the agent. Keeping the two forms separate prevents search results and transcript replay from
/// surfacing hidden context as if the user had typed it.
pub fn canonical_doc_text(doc: &[DocBlock]) -> String {
    doc.iter()
        .map(|block| match block {
            DocBlock::Text { text } => text.clone(),
            DocBlock::Skill { skill_id, .. } => format!("[skill:{skill_id}]"),
            DocBlock::File { path } => format!("[@{path}]"),
            DocBlock::Image { path } => format!("[img:{path}]"),
            DocBlock::Appshot { id, .. } => format!("[appshot:{id}]"),
            DocBlock::Attachment { id, .. } => format!("[attachment:{id}]"),
            DocBlock::Canvas {
                id,
                frozen_revision,
                ..
            } => format!("[canvas:{id}@{frozen_revision}]"),
            DocBlock::Session { session_id, .. } => {
                format!("[chat:{}]", session_id.chars().take(8).collect::<String>())
            }
            DocBlock::Artifact { record_id } => format!("[artifact:{record_id}]"),
            DocBlock::Issue { source, id, .. } => format!("[issue:{source}#{id}]"),
        })
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The result of compiling a document: what to send and how to configure the session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CompiledPrompt {
    /// Markdown prompt for ACP `session/prompt` (as a single text content block).
    pub prompt: String,
    /// MCP servers to attach in `session/new` (from `Mcp` skills).
    pub mcp_servers: Vec<McpServer>,
    /// Provider-native Agent Skills referenced (from `AgentSkill` skills).
    pub agent_skills: Vec<String>,
    /// Plugin-supplied specialists requested by this document.
    #[serde(default)]
    pub subagents: Vec<String>,
    /// Workspace files inlined via `@`-mentions.
    pub files: Vec<String>,
    /// Attached image paths, sent as ACP image content blocks alongside the prompt.
    pub images: Vec<String>,
    /// App-private images already resolved to bytes. These never masquerade as workspace paths.
    #[serde(default)]
    pub appshots: Vec<CompiledAppshot>,
    /// User-selected or pasted private images already resolved to bytes.
    #[serde(default)]
    pub attachments: Vec<CompiledAttachment>,
    /// Resolved immutable Canvas payloads.  This is intentionally separate from workspace image
    /// paths so a provider cannot mistake an app-private asset for a workspace file.
    #[serde(default)]
    pub canvases: Vec<CompiledCanvas>,
    /// Past chats inlined via `@`-mentions (session ids).
    #[serde(default)]
    pub sessions: Vec<String>,
    /// Skill ids (or `file:<path>`) that could not be resolved — surfaced to the user as warnings.
    pub unresolved: Vec<String>,
    /// Scene-artifact record ids inlined via artifact mentions.
    #[serde(default)]
    pub artifacts: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledCanvas {
    pub reference: CanvasRef,
    pub payload: CanvasPromptPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledAppshot {
    pub id: String,
    pub data: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledAttachment {
    pub id: String,
    pub name: String,
    pub data: String,
    pub mime_type: String,
}

struct ResolvedAppshot {
    image: CompiledAppshot,
    context: String,
}

struct ResolvedAttachment {
    image: CompiledAttachment,
    context: String,
}

/// Lower a document into a [`CompiledPrompt`]. Provider-agnostic: `AgentSkill` blocks contribute a
/// reference (and inline fallback text when present) so a provider without native skills still gets
/// the intent as prose.
pub fn compile(doc: &[DocBlock], library: &SkillLibrary) -> CompiledPrompt {
    compile_with_context(doc, library, None)
}

/// Like [`compile`], but with a workspace: project rules (AGENTS.md / .cursorrules / CLAUDE.md …)
/// are prepended and `@`-mentioned files are inlined from disk.
pub fn compile_with_context(
    doc: &[DocBlock],
    library: &SkillLibrary,
    cwd: Option<&std::path::Path>,
) -> CompiledPrompt {
    compile_with_sessions(doc, library, cwd, None)
}

/// Compile with an explicit Canvas resolver, feature gate, and provider image capability. A
/// known-unsupported provider fails closed; unknown capability still attempts every ordered
/// image and surfaces a provider failure. Callers must choose `StructureOnly` when a summary-only
/// send is intended.
pub fn compile_with_canvas(
    doc: &[DocBlock],
    library: &SkillLibrary,
    cwd: Option<&std::path::Path>,
    resolve_session: Option<&dyn Fn(&str, Option<i64>) -> Option<String>>,
    gate: CanvasFeatureGate,
    capability: CanvasProviderImageCapability,
    resolve_canvas: &dyn Fn(&str, u64) -> Result<CanvasPromptPayload, crate::canvas::CanvasError>,
) -> Result<CompiledPrompt, crate::canvas::CanvasError> {
    append_canvases(
        compile_with_sessions(doc, library, cwd, resolve_session),
        doc,
        gate,
        capability,
        resolve_canvas,
    )
}

/// Desktop compilation path for private Appshots and prompt attachments. Only this explicit
/// application-data root can resolve opaque IDs; normal workspace compilation leaves them
/// unresolved.
pub fn compile_with_appshots(
    doc: &[DocBlock],
    library: &SkillLibrary,
    cwd: Option<&std::path::Path>,
    resolve_session: Option<&dyn Fn(&str, Option<i64>) -> Option<String>>,
    data_dir: &std::path::Path,
) -> Result<CompiledPrompt, String> {
    let appshot_resolver = |id: &str| load_appshot(data_dir, id);
    let attachment_resolver = |id: &str| load_attachment(data_dir, id);
    compile_full_resolving(
        doc,
        library,
        cwd,
        resolve_session,
        None,
        Some(&appshot_resolver),
        Some(&attachment_resolver),
    )
}

/// Desktop compilation path for documents that contain Canvas and private image blocks.
pub fn compile_with_canvas_and_appshots(
    doc: &[DocBlock],
    library: &SkillLibrary,
    cwd: Option<&std::path::Path>,
    resolve_session: Option<&dyn Fn(&str, Option<i64>) -> Option<String>>,
    data_dir: &std::path::Path,
    gate: CanvasFeatureGate,
    capability: CanvasProviderImageCapability,
    resolve_canvas: &dyn Fn(&str, u64) -> Result<CanvasPromptPayload, crate::canvas::CanvasError>,
) -> Result<CompiledPrompt, String> {
    let compiled = compile_with_appshots(doc, library, cwd, resolve_session, data_dir)?;
    append_canvases(compiled, doc, gate, capability, resolve_canvas)
        .map_err(|error| error.to_string())
}

fn append_canvases(
    mut out: CompiledPrompt,
    doc: &[DocBlock],
    gate: CanvasFeatureGate,
    capability: CanvasProviderImageCapability,
    resolve_canvas: &dyn Fn(&str, u64) -> Result<CanvasPromptPayload, crate::canvas::CanvasError>,
) -> Result<CompiledPrompt, crate::canvas::CanvasError> {
    for block in doc {
        let DocBlock::Canvas {
            id,
            frozen_revision,
            pixel_policy,
        } = block
        else {
            continue;
        };
        let reference = CanvasRef {
            id: id.clone(),
            frozen_revision: *frozen_revision,
            pixel_policy: *pixel_policy,
        };
        let marker = format!("canvas:{id}@{frozen_revision}");
        out.unresolved.retain(|item| item != &marker);
        let payload =
            resolve_prompt_payload(&reference, gate, capability, |canvas_id, revision| {
                resolve_canvas(canvas_id, revision)
            })?;
        out.prompt.push_str("\n\n");
        out.prompt.push_str(&format!(
            "**Canvas {} (revision {}) structural summary:**\n{}",
            payload.title, payload.revision, payload.summary
        ));
        out.canvases.push(CompiledCanvas { reference, payload });
    }
    Ok(out)
}

/// Like [`compile_with_context`], but able to resolve `@`-mentioned past chats: `resolve_session`
/// maps a session id to its rendered transcript context (`None` for an unknown id). The compiler
/// stays store-agnostic — callers with a [`crate::store::Store`] pass a closure over it.
pub fn compile_with_sessions(
    doc: &[DocBlock],
    library: &SkillLibrary,
    cwd: Option<&std::path::Path>,
    resolve_session: Option<&dyn Fn(&str, Option<i64>) -> Option<String>>,
) -> CompiledPrompt {
    compile_full(doc, library, cwd, resolve_session, None)
}

/// Like [`compile_with_sessions`], but also able to resolve scene-artifact mentions:
/// `resolve_artifact` maps a record id to a `(label, content)` pair (`None` for an unknown id),
/// which is inlined as a labeled context block. The compiler stays store-agnostic — callers with
/// a [`crate::scene_artifact::SceneArtifactStore`] pass a closure over it.
pub fn compile_full(
    doc: &[DocBlock],
    library: &SkillLibrary,
    cwd: Option<&std::path::Path>,
    resolve_session: Option<&dyn Fn(&str, Option<i64>) -> Option<String>>,
    resolve_artifact: Option<&dyn Fn(i64) -> Option<(String, String)>>,
) -> CompiledPrompt {
    compile_full_resolving(
        doc,
        library,
        cwd,
        resolve_session,
        resolve_artifact,
        None,
        None,
    )
    .expect("document compilation without private resolvers cannot fail")
}

fn compile_full_resolving(
    doc: &[DocBlock],
    library: &SkillLibrary,
    cwd: Option<&std::path::Path>,
    resolve_session: Option<&dyn Fn(&str, Option<i64>) -> Option<String>>,
    resolve_artifact: Option<&dyn Fn(i64) -> Option<(String, String)>>,
    resolve_appshot: Option<&dyn Fn(&str) -> Result<ResolvedAppshot, String>>,
    resolve_attachment: Option<&dyn Fn(&str) -> Result<ResolvedAttachment, String>>,
) -> Result<CompiledPrompt, String> {
    let mut out = CompiledPrompt::default();
    let mut parts: Vec<String> = Vec::new();

    // Project rules travel with every prompt, regardless of provider.
    if let Some(dir) = cwd {
        let ctx = crate::rules::to_context(&crate::rules::load(dir));
        if !ctx.is_empty() {
            parts.push(ctx);
        }
    }

    for block in doc {
        match block {
            DocBlock::Image { path } => {
                // Validate now so a bad path surfaces in the preview rather than mid-turn.
                match cwd.map(|c| crate::workspace::read_image_base64(c, path)) {
                    Some(Ok(_)) => {
                        out.images.push(path.clone());
                        // The image block lets the model see the pixels. The path hint lets an
                        // agent use file tools to copy, edit, or include that same image in work.
                        // JSON quoting keeps unusual filenames on one unambiguous prompt line.
                        let quoted = serde_json::to_string(path)
                            .unwrap_or_else(|_| "\"unavailable\"".to_string());
                        parts.push(format!("**Attached image workspace path:** {quoted}"));
                    }
                    _ => out.unresolved.push(format!("image:{path}")),
                }
            }
            DocBlock::Appshot { id, .. } => match resolve_appshot {
                Some(resolve) => {
                    let resolved = resolve(id)?;
                    parts.push(resolved.context);
                    out.appshots.push(resolved.image);
                }
                None => out.unresolved.push(format!("appshot:{id}")),
            },
            DocBlock::Attachment { id, .. } => match resolve_attachment {
                Some(resolve) => {
                    let resolved = resolve(id)?;
                    parts.push(resolved.context);
                    out.attachments.push(resolved.image);
                }
                None => out.unresolved.push(format!("attachment:{id}")),
            },
            DocBlock::Canvas {
                id,
                frozen_revision,
                ..
            } => out
                .unresolved
                .push(format!("canvas:{id}@{frozen_revision}")),
            DocBlock::File { path } => {
                out.files.push(path.clone());
                match cwd.map(|c| crate::workspace::read_file(c, path)) {
                    Some(Ok(content)) => {
                        let lang = crate::workspace::lang_for(path);
                        parts.push(format!(
                            "**File** `{path}`\n\n```{lang}\n{}\n```",
                            content.trim_end()
                        ));
                    }
                    _ => out.unresolved.push(format!("file:{path}")),
                }
            }
            DocBlock::Session {
                session_id,
                through_seq,
            } => match resolve_session.and_then(|resolve| resolve(session_id, *through_seq)) {
                Some(ctx) if !ctx.trim().is_empty() => {
                    out.sessions.push(session_id.clone());
                    parts.push(ctx);
                }
                _ => out.unresolved.push(format!("session:{session_id}")),
            },
            DocBlock::Artifact { record_id } => {
                match resolve_artifact.and_then(|resolve| resolve(*record_id)) {
                    Some((label, content)) => {
                        out.artifacts.push(*record_id);
                        parts.push(format!("**Artifact: {label}**\n\n{}", content.trim_end()));
                    }
                    None => out.unresolved.push(format!("artifact:{record_id}")),
                }
            }
            DocBlock::Issue {
                source,
                id,
                title,
                url,
                body,
            } => {
                // The snapshot was embedded at insert time, so this renders offline with the
                // exact byte shape of `issues::Issue::to_context` — never `unresolved`.
                let issue = crate::issues::Issue {
                    id: id.clone(),
                    title: title.clone(),
                    state: "open".into(),
                    url: url.clone(),
                    body: body.clone(),
                    source: source.clone(),
                };
                parts.push(issue.to_context());
            }
            DocBlock::Text { text } => {
                let t = text.trim_end();
                if !t.is_empty() {
                    parts.push(t.to_string());
                }
            }
            DocBlock::Skill { skill_id, params } => {
                let Some(skill) = library.get(skill_id) else {
                    out.unresolved.push(skill_id.clone());
                    continue;
                };
                match &skill.payload {
                    SkillPayload::Fragment { text } => parts.push(text.trim().to_string()),
                    SkillPayload::Macro { template, slots } => {
                        // Effective params = slot defaults overlaid by what the user filled in.
                        // A slot with neither keeps the leave-`{{slot}}`-as-is behavior.
                        let mut effective: HashMap<String, String> = HashMap::new();
                        for slot in slots {
                            if let Some(default) = &slot.default {
                                effective.insert(slot.id.clone(), default.clone());
                            }
                        }
                        for (key, value) in params {
                            effective.insert(key.clone(), value.clone());
                        }
                        parts.push(substitute(template, &effective).trim().to_string())
                    }
                    SkillPayload::AgentSkill {
                        skill_ref,
                        inline_text,
                    } => {
                        out.agent_skills.push(skill_ref.clone());
                        match inline_text {
                            Some(t) => parts.push(t.trim().to_string()),
                            None => parts.push(format!("Use the **{}** skill.", skill.name)),
                        }
                    }
                    SkillPayload::Subagent { agent } => {
                        out.subagents.push(agent.name.clone());
                        let mut contract = format!(
                            "## Provider-native subagent: {}\n\nUse the provider's native subagent capability to delegate this focused task. If native delegation is unavailable, report that it is unsupported; do not execute these specialist instructions inline.\n\n{}",
                            agent.name,
                            agent.prompt.trim()
                        );
                        if !agent.tools.is_empty() {
                            contract.push_str(&format!(
                                "\n\nAllowed tools: {}",
                                agent.tools.join(", ")
                            ));
                        }
                        if let Some(model) = &agent.model {
                            contract.push_str(&format!("\nPreferred model: {model}"));
                        }
                        parts.push(contract);
                    }
                    SkillPayload::Mcp { server } => {
                        if !out.mcp_servers.iter().any(|m| m.name == server.name) {
                            out.mcp_servers.push(server.clone());
                        }
                    }
                }
            }
        }
    }

    out.prompt = parts.join("\n\n");
    Ok(out)
}

const MAX_APPSHOT_IMAGE_BYTES: u64 = 25 * 1024 * 1024;
const MAX_APPSHOT_METADATA_BYTES: u64 = 1024 * 1024;

fn load_appshot(data_dir: &std::path::Path, id: &str) -> Result<ResolvedAppshot, String> {
    validate_appshot_id(id)?;
    let requested_root = data_dir.join("appshots");
    let root_metadata = std::fs::symlink_metadata(&requested_root)
        .map_err(|_| "appshot capture is unavailable".to_string())?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err("appshot capture directory is invalid".into());
    }
    let root = requested_root
        .canonicalize()
        .map_err(|_| "appshot capture directory is invalid".to_string())?;
    let image_path = private_appshot_file(&root, &format!("{id}.png"), "image")?;
    let metadata_path = private_appshot_file(&root, &format!("{id}.json"), "metadata")?;

    let image_metadata = std::fs::metadata(&image_path).map_err(|error| error.to_string())?;
    if image_metadata.len() == 0 || image_metadata.len() > MAX_APPSHOT_IMAGE_BYTES {
        return Err("appshot image is too large or empty".into());
    }
    let image = std::fs::read(&image_path).map_err(|error| error.to_string())?;
    if !image.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("appshot image is not a PNG".into());
    }

    let metadata_metadata = std::fs::metadata(&metadata_path).map_err(|error| error.to_string())?;
    if metadata_metadata.len() == 0 || metadata_metadata.len() > MAX_APPSHOT_METADATA_BYTES {
        return Err("appshot metadata is too large or empty".into());
    }
    let metadata_bytes = std::fs::read(&metadata_path).map_err(|error| error.to_string())?;
    let metadata: serde_json::Value = serde_json::from_slice(&metadata_bytes)
        .map_err(|_| "appshot metadata is invalid".to_string())?;
    if metadata.get("id").and_then(serde_json::Value::as_str) != Some(id) {
        return Err("appshot metadata does not match the capture".into());
    }

    let one_line = |key: &str, fallback: &str| {
        metadata
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(fallback)
            .replace(['\r', '\n'], " ")
    };
    let app_name = one_line("app_name", "Application");
    let title = one_line("window_title", "Window");
    let captured_at = one_line("captured_at", "unknown time");
    let accessible_text = metadata
        .get("text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let accessibility = if accessible_text.is_empty() {
        "No accessible window text was available; use the attached image.".to_string()
    } else {
        format!(
            "Accessible window text (may include content outside the visible scroll area):\n\n{accessible_text}"
        )
    };

    Ok(ResolvedAppshot {
        image: CompiledAppshot {
            id: id.to_string(),
            data: base64::engine::general_purpose::STANDARD.encode(image),
            mime_type: "image/png".into(),
        },
        context: format!(
            "**Appshot — {app_name}: {title}**\n\nCaptured: {captured_at}\n\n{accessibility}"
        ),
    })
}

fn load_attachment(data_dir: &std::path::Path, id: &str) -> Result<ResolvedAttachment, String> {
    use base64::Engine as _;

    let attachment = crate::attachment::load_prompt_attachment(data_dir, id)?;
    let quoted_name =
        serde_json::to_string(&attachment.name).unwrap_or_else(|_| "\"Image.png\"".to_string());
    Ok(ResolvedAttachment {
        image: CompiledAttachment {
            id: attachment.id,
            name: attachment.name,
            data: base64::engine::general_purpose::STANDARD.encode(attachment.bytes),
            mime_type: attachment.mime_type,
        },
        context: format!("**Attached image:** {quoted_name}"),
    })
}

fn validate_appshot_id(id: &str) -> Result<(), String> {
    let valid_shape = id.len() == 36
        && [8, 13, 18, 23]
            .into_iter()
            .all(|index| id.as_bytes().get(index) == Some(&b'-'));
    let parsed = valid_shape
        .then(|| uuid::Uuid::parse_str(id).ok())
        .flatten()
        .filter(|uuid| uuid.hyphenated().to_string().eq_ignore_ascii_case(id));
    let version = id.as_bytes().get(14).copied();
    let variant = id
        .as_bytes()
        .get(19)
        .copied()
        .map(|value| value.to_ascii_lowercase());
    if parsed.is_none()
        || !matches!(version, Some(b'1'..=b'8'))
        || !matches!(variant, Some(b'8' | b'9' | b'a' | b'b'))
    {
        return Err("appshot id is invalid".into());
    }
    Ok(())
}

fn private_appshot_file(
    root: &std::path::Path,
    name: &str,
    label: &str,
) -> Result<std::path::PathBuf, String> {
    let requested = root.join(name);
    let requested_metadata = std::fs::symlink_metadata(&requested)
        .map_err(|_| format!("appshot {label} is unavailable"))?;
    if requested_metadata.file_type().is_symlink() || !requested_metadata.is_file() {
        return Err(format!("appshot {label} path is invalid"));
    }
    let canonical = requested
        .canonicalize()
        .map_err(|_| format!("appshot {label} path is invalid"))?;
    if canonical.parent() != Some(root) {
        return Err(format!("appshot {label} path is invalid"));
    }
    Ok(canonical)
}

/// A few built-in skills so the `/` picker has resolvable content on first run. Shared by the GUI
/// and the TUI; merged with any user skills loaded from disk.
pub fn builtin_skills() -> Vec<Skill> {
    vec![
        Skill {
            id: "reviewer".into(),
            name: "Code Reviewer".into(),
            description: "Meticulous senior reviewer persona".into(),
            icon: None,
            source: None,
            payload: SkillPayload::Fragment {
                text: "Act as a meticulous senior code reviewer. Flag bugs, unsafe patterns, and \
                       missing tests. Explain the risk of each finding."
                    .into(),
            },
        },
        Skill {
            id: "test-writer".into(),
            name: "Test Writer".into(),
            description: "Write thorough tests".into(),
            icon: None,
            source: None,
            payload: SkillPayload::Fragment {
                text: "Write thorough, isolated unit tests covering happy paths and edge cases. \
                       Prefer deterministic tests with no network."
                    .into(),
            },
        },
        Skill {
            id: "security-audit".into(),
            name: "Security Audit".into(),
            description: "Audit for vulnerabilities".into(),
            icon: None,
            source: None,
            payload: SkillPayload::Fragment {
                text: "Audit the code for security vulnerabilities: injection, auth gaps, unsafe \
                       deserialization, secrets in code. Rank findings by severity."
                    .into(),
            },
        },
        Skill {
            id: "commit-macro".into(),
            name: "Commit Message".into(),
            description: "Parameterized commit message".into(),
            icon: None,
            source: None,
            payload: SkillPayload::Macro {
                template: "Write a {{style}} commit message for changes to {{scope}}.".into(),
                slots: vec![
                    SlotDef {
                        id: "style".into(),
                        label: "Style".into(),
                        kind: SlotKind::Select,
                        options: vec!["conventional".into(), "descriptive".into()],
                        required: true,
                        default: None,
                    },
                    SlotDef {
                        id: "scope".into(),
                        label: "Scope".into(),
                        kind: SlotKind::Text,
                        options: Vec::new(),
                        required: false,
                        default: None,
                    },
                ],
            },
        },
    ]
}

/// Replace `{{slot}}` occurrences in a macro template with provided params (missing → left as-is).
fn substitute(template: &str, params: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in params {
        result = result.replace(&format!("{{{{{key}}}}}"), value);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_library() -> SkillLibrary {
        SkillLibrary::new([
            Skill {
                id: "reviewer".into(),
                name: "Code Reviewer".into(),
                description: "Careful review persona".into(),
                icon: None,
                source: None,
                payload: SkillPayload::Fragment {
                    text: "Act as a meticulous code reviewer. Flag bugs and unsafe patterns."
                        .into(),
                },
            },
            Skill {
                id: "commit".into(),
                name: "Commit Macro".into(),
                description: String::new(),
                icon: None,
                source: None,
                payload: SkillPayload::Macro {
                    template: "Write a commit message for {{scope}} in {{style}} style.".into(),
                    slots: vec!["scope".into(), "style".into()],
                },
            },
            Skill {
                id: "fs-mcp".into(),
                name: "Filesystem MCP".into(),
                description: String::new(),
                icon: None,
                source: None,
                payload: SkillPayload::Mcp {
                    server: McpServer {
                        name: "filesystem".into(),
                        cwd: None,
                        transport: McpTransport::Stdio {
                            command: "mcp-fs".into(),
                            args: vec![],
                            env: vec![],
                        },
                    },
                },
            },
        ])
    }

    #[test]
    fn compiles_text_and_fragment() {
        let lib = sample_library();
        let doc = vec![
            DocBlock::Text {
                text: "# Refactor auth".into(),
            },
            DocBlock::Skill {
                skill_id: "reviewer".into(),
                params: HashMap::new(),
            },
            DocBlock::Text {
                text: "Focus on the token path.".into(),
            },
        ];
        let compiled = compile(&doc, &lib);
        assert!(compiled.prompt.contains("# Refactor auth"));
        assert!(compiled.prompt.contains("meticulous code reviewer"));
        assert!(compiled.prompt.contains("token path"));
        assert!(compiled.unresolved.is_empty());
    }

    #[test]
    fn macro_substitutes_params() {
        let lib = sample_library();
        let mut params = HashMap::new();
        params.insert("scope".into(), "auth module".into());
        params.insert("style".into(), "conventional".into());
        let doc = vec![DocBlock::Skill {
            skill_id: "commit".into(),
            params,
        }];
        let compiled = compile(&doc, &lib);
        assert_eq!(
            compiled.prompt,
            "Write a commit message for auth module in conventional style."
        );
    }

    #[test]
    fn mcp_skill_adds_server_not_prompt_text() {
        let lib = sample_library();
        let doc = vec![DocBlock::Skill {
            skill_id: "fs-mcp".into(),
            params: HashMap::new(),
        }];
        let compiled = compile(&doc, &lib);
        assert_eq!(compiled.mcp_servers.len(), 1);
        assert_eq!(compiled.mcp_servers[0].name, "filesystem");
        assert!(compiled.prompt.is_empty());
    }

    #[test]
    fn subagent_compiles_to_delegation_contract_and_preview_metadata() {
        let lib = SkillLibrary::new([Skill {
            id: "researcher".into(),
            name: "Researcher".into(),
            description: "Find primary evidence".into(),
            icon: None,
            source: Some("Plugin · Research".into()),
            payload: SkillPayload::Subagent {
                agent: SubagentDefinition {
                    name: "Researcher".into(),
                    description: "Find primary evidence".into(),
                    prompt: "Use primary sources and separate facts from inference.".into(),
                    model: Some("fast".into()),
                    tools: vec!["web".into(), "files".into()],
                },
            },
        }]);
        let compiled = compile(
            &[DocBlock::Skill {
                skill_id: "researcher".into(),
                params: HashMap::new(),
            }],
            &lib,
        );
        assert_eq!(compiled.subagents, vec!["Researcher"]);
        assert!(compiled
            .prompt
            .contains("## Provider-native subagent: Researcher"));
        assert!(compiled
            .prompt
            .contains("do not execute these specialist instructions inline"));
        assert!(compiled.prompt.contains("Allowed tools: web, files"));
        assert!(compiled.prompt.contains("Preferred model: fast"));
    }

    #[test]
    fn unknown_skill_is_reported() {
        let lib = sample_library();
        let doc = vec![DocBlock::Skill {
            skill_id: "nope".into(),
            params: HashMap::new(),
        }];
        let compiled = compile(&doc, &lib);
        assert_eq!(compiled.unresolved, vec!["nope".to_string()]);
    }

    #[test]
    fn session_mention_inlines_resolved_transcript() {
        let lib = sample_library();
        let doc = vec![
            DocBlock::Session {
                session_id: "abc".into(),
                through_seq: Some(7),
            },
            DocBlock::Text {
                text: "Implement what we planned.".into(),
            },
        ];
        let resolve = |id: &str, through_seq: Option<i64>| -> Option<String> {
            (id == "abc" && through_seq == Some(7))
                .then(|| "**Referenced chat** — Plan\n\n**User:**\nhello".to_string())
        };
        let compiled = compile_with_sessions(&doc, &lib, None, Some(&resolve));
        assert_eq!(compiled.sessions, vec!["abc".to_string()]);
        assert!(compiled.prompt.contains("Referenced chat"));
        assert!(compiled.prompt.contains("Implement what we planned."));
        assert!(compiled.unresolved.is_empty());
    }

    #[test]
    fn session_mention_without_resolver_is_unresolved() {
        let lib = sample_library();
        let doc = vec![DocBlock::Session {
            session_id: "ghost".into(),
            through_seq: None,
        }];
        let compiled = compile(&doc, &lib);
        assert!(compiled.sessions.is_empty());
        assert_eq!(compiled.unresolved, vec!["session:ghost".to_string()]);
    }

    #[test]
    fn artifact_mention_inlines_resolved_content_as_labeled_block() {
        let lib = sample_library();
        let doc = vec![
            DocBlock::Artifact { record_id: 7 },
            DocBlock::Text {
                text: "Execute this plan.".into(),
            },
        ];
        let resolve = |id: i64| -> Option<(String, String)> {
            (id == 7).then(|| ("Plan v2".to_string(), "- [ ] step one\n".to_string()))
        };
        let compiled = compile_full(&doc, &lib, None, None, Some(&resolve));
        assert_eq!(compiled.artifacts, vec![7]);
        assert!(compiled.prompt.contains("**Artifact: Plan v2**"));
        assert!(compiled.prompt.contains("- [ ] step one"));
        assert!(compiled.prompt.contains("Execute this plan."));
        assert!(compiled.unresolved.is_empty());
        assert_eq!(
            canonical_doc_text(&doc),
            "[artifact:7]\n\nExecute this plan."
        );
    }

    #[test]
    fn artifact_mention_without_resolver_is_unresolved() {
        let lib = sample_library();
        let doc = vec![DocBlock::Artifact { record_id: 9 }];
        // Existing entry points delegate with no artifact resolver.
        let compiled = compile(&doc, &lib);
        assert!(compiled.artifacts.is_empty());
        assert_eq!(compiled.unresolved, vec!["artifact:9".to_string()]);

        let resolve = |_: i64| -> Option<(String, String)> { None };
        let compiled = compile_full(&doc, &lib, None, None, Some(&resolve));
        assert_eq!(compiled.unresolved, vec!["artifact:9".to_string()]);
    }

    #[test]
    fn issue_mention_canonical_text() {
        let doc = vec![
            DocBlock::Issue {
                source: "github".into(),
                id: "42".into(),
                title: "Fix login".into(),
                url: "https://github.com/o/r/issues/42".into(),
                body: String::new(),
            },
            DocBlock::Text {
                text: "Please handle this.".into(),
            },
        ];
        assert_eq!(
            canonical_doc_text(&doc),
            "[issue:github#42]\n\nPlease handle this."
        );
    }

    #[test]
    fn issue_mention_compiles_title_url_body_and_stays_resolved() {
        let lib = sample_library();
        let doc = vec![DocBlock::Issue {
            source: "linear".into(),
            id: "ENG-9".into(),
            title: "Speed up CI".into(),
            url: "https://linear.app/t/ENG-9".into(),
            body: "Cache the toolchain.".into(),
        }];
        // Plain `compile` (no resolvers, no cwd) is enough: the snapshot is embedded.
        let compiled = compile(&doc, &lib);
        assert!(compiled.prompt.contains("linear #ENG-9"));
        assert!(compiled.prompt.contains("Speed up CI"));
        assert!(compiled.prompt.contains("https://linear.app/t/ENG-9"));
        assert!(compiled.prompt.contains("Cache the toolchain."));
        assert!(compiled.unresolved.is_empty());
    }

    #[test]
    fn compiled_prompt_without_artifacts_field_still_deserializes() {
        let stored: CompiledPrompt = serde_json::from_str(
            r#"{"prompt":"p","mcp_servers":[],"agent_skills":[],"files":[],"images":[],"unresolved":[]}"#,
        )
        .unwrap();
        assert!(stored.artifacts.is_empty());
    }

    #[test]
    fn canonical_doc_keeps_user_text_but_not_expanded_context() {
        let doc = vec![
            DocBlock::Text {
                text: "First line\n  indented line".into(),
            },
            DocBlock::Skill {
                skill_id: "review".into(),
                params: HashMap::from([("private".into(), "not persisted".into())]),
            },
            DocBlock::File {
                path: "src/main.rs".into(),
            },
            DocBlock::Session {
                session_id: "1234567890abcdef".into(),
                through_seq: None,
            },
        ];

        assert_eq!(
            canonical_doc_text(&doc),
            "First line\n  indented line\n\n[skill:review]\n\n[@src/main.rs]\n\n[chat:12345678]",
        );
    }

    #[test]
    fn load_dir_reads_json_skills() {
        let dir = std::env::temp_dir().join(format!("codetwo-skills-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let skill = Skill {
            id: "planner".into(),
            name: "Planner".into(),
            description: "Plan first".into(),
            icon: None,
            source: None,
            payload: SkillPayload::Fragment {
                text: "Make a plan before coding.".into(),
            },
        };
        std::fs::write(
            dir.join("planner.json"),
            serde_json::to_string(&skill).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.join("notes.txt"), "ignored").unwrap();

        let lib = SkillLibrary::load_dir(&dir).unwrap();
        assert!(lib.get("planner").is_some());
        assert_eq!(lib.all().count(), 1);

        // Missing dir → empty, not an error.
        let empty = SkillLibrary::load_dir(&dir.join("nope")).unwrap();
        assert_eq!(empty.all().count(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_and_delete_round_trip() {
        let dir = std::env::temp_dir().join(format!("codetwo-skills-rw-{}", uuid::Uuid::new_v4()));
        let skill = Skill {
            id: "mine".into(),
            name: "Mine".into(),
            description: String::new(),
            icon: None,
            source: None,
            payload: SkillPayload::Fragment { text: "x".into() },
        };
        SkillLibrary::save_to_dir(&dir, &skill).unwrap();
        assert!(SkillLibrary::load_dir(&dir).unwrap().get("mine").is_some());

        SkillLibrary::delete_from_dir(&dir, "mine").unwrap();
        assert!(SkillLibrary::load_dir(&dir).unwrap().get("mine").is_none());
        // Deleting again is a no-op.
        SkillLibrary::delete_from_dir(&dir, "mine").unwrap();

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compile_with_context_inlines_rules_and_files() {
        let dir = std::env::temp_dir().join(format!("codetwo-ctx-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "Use tabs.").unwrap();
        std::fs::write(dir.join("src/a.rs"), "fn a() {}").unwrap();
        let lib = sample_library();

        let doc = vec![
            DocBlock::Text {
                text: "Fix the bug.".into(),
            },
            DocBlock::File {
                path: "src/a.rs".into(),
            },
        ];
        let c = compile_with_context(&doc, &lib, Some(&dir));
        assert!(
            c.prompt.contains("## Project rules"),
            "rules prepended: {}",
            c.prompt
        );
        assert!(c.prompt.contains("Use tabs."));
        assert!(c.prompt.contains("**File** `src/a.rs`"));
        assert!(c.prompt.contains("fn a() {}"));
        assert!(c.prompt.contains("```rust"));
        assert_eq!(c.files, vec!["src/a.rs".to_string()]);
        assert!(c.unresolved.is_empty());

        // A missing file is reported, not silently dropped.
        let c2 = compile_with_context(
            &[DocBlock::File {
                path: "nope.rs".into(),
            }],
            &lib,
            Some(&dir),
        );
        assert_eq!(c2.unresolved, vec!["file:nope.rs".to_string()]);

        // Without a workspace, no rules and files can't be read.
        let c3 = compile(&doc, &lib);
        assert!(!c3.prompt.contains("Project rules"));
        assert_eq!(c3.unresolved, vec!["file:src/a.rs".to_string()]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn image_context_keeps_pixels_and_an_actionable_workspace_path_together() {
        let dir =
            std::env::temp_dir().join(format!("codetwo-image-context-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("screens")).unwrap();
        let tiny_png = [
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00,
            0x00, 0xb5, 0x1c, 0x0c, 0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78,
            0xda, 0x63, 0x64, 0xf8, 0x0f, 0x00, 0x01, 0x05, 0x01, 0x01, 0x27, 0x18, 0xe3, 0x66,
            0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        std::fs::write(dir.join("screens/result.png"), tiny_png).unwrap();
        let doc = vec![
            DocBlock::Text {
                text: "Include this result in the report.".into(),
            },
            DocBlock::Image {
                path: "screens/result.png".into(),
            },
        ];

        let compiled = compile_with_context(&doc, &sample_library(), Some(&dir));
        assert_eq!(compiled.images, vec!["screens/result.png".to_string()]);
        assert!(compiled
            .prompt
            .contains("**Attached image workspace path:** \"screens/result.png\""));
        assert!(compiled.unresolved.is_empty());
        assert_eq!(
            canonical_doc_text(&doc),
            "Include this result in the report.\n\n[img:screens/result.png]",
            "provider-only path context must not replace the user's canonical document"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_string_slots_deserialize_as_text_slots() {
        let skill: Skill = serde_json::from_str(
            r#"{"id":"commit","name":"Commit","payload":{"kind":"macro",
                "template":"Write a {{style}} message for {{scope}}.",
                "slots":["style","scope"]}}"#,
        )
        .unwrap();
        let SkillPayload::Macro { slots, .. } = &skill.payload else {
            panic!("expected macro payload");
        };
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0], SlotDef::text("style"));
        assert_eq!(slots[1].id, "scope");
        assert_eq!(slots[1].kind, SlotKind::Text);
        assert!(slots[1].label.is_empty());
        assert!(!slots[1].required);
    }

    #[test]
    fn typed_slots_round_trip_and_serialize_as_objects() {
        let skill = Skill {
            id: "commit".into(),
            name: "Commit".into(),
            description: String::new(),
            icon: None,
            source: None,
            payload: SkillPayload::Macro {
                template: "Write a {{style}} message.".into(),
                slots: vec![SlotDef {
                    id: "style".into(),
                    label: "Style".into(),
                    kind: SlotKind::Select,
                    options: vec!["conventional".into(), "descriptive".into()],
                    required: true,
                    default: Some("conventional".into()),
                }],
            },
        };
        let json = serde_json::to_value(&skill).unwrap();
        // Serialization always writes the object shape — never the legacy bare string.
        assert!(json["payload"]["slots"][0].is_object());
        assert_eq!(json["payload"]["slots"][0]["kind"], "select");
        assert_eq!(json["payload"]["slots"][0]["options"][1], "descriptive");
        let back: Skill = serde_json::from_value(json).unwrap();
        assert_eq!(back.payload, skill.payload);
    }

    #[test]
    fn mixed_slot_array_deserializes() {
        let skill: Skill = serde_json::from_str(
            r#"{"id":"m","name":"M","payload":{"kind":"macro","template":"{{a}} {{b}}",
                "slots":["a",{"id":"b","label":"B","kind":"select","options":["x"],"required":true}]}}"#,
        )
        .unwrap();
        let SkillPayload::Macro { slots, .. } = &skill.payload else {
            panic!("expected macro payload");
        };
        assert_eq!(slots[0], SlotDef::text("a"));
        assert_eq!(slots[1].kind, SlotKind::Select);
        assert_eq!(slots[1].options, vec!["x".to_string()]);
        assert!(slots[1].required);
    }

    #[test]
    fn macro_compile_applies_slot_defaults_and_user_overrides() {
        let lib = SkillLibrary::new([Skill {
            id: "commit".into(),
            name: "Commit".into(),
            description: String::new(),
            icon: None,
            source: None,
            payload: SkillPayload::Macro {
                template: "Write a {{style}} message for {{scope}}.".into(),
                slots: vec![
                    SlotDef {
                        default: Some("conventional".into()),
                        ..SlotDef::text("style")
                    },
                    SlotDef::text("scope"),
                ],
            },
        }]);

        // No user params: the default fills its slot; a slot with no default keeps the
        // placeholder (today's behavior).
        let compiled = compile(
            &[DocBlock::Skill {
                skill_id: "commit".into(),
                params: HashMap::new(),
            }],
            &lib,
        );
        assert_eq!(
            compiled.prompt,
            "Write a conventional message for {{scope}}."
        );

        // A user param overrides the default.
        let compiled = compile(
            &[DocBlock::Skill {
                skill_id: "commit".into(),
                params: HashMap::from([
                    ("style".into(), "descriptive".into()),
                    ("scope".into(), "auth".into()),
                ]),
            }],
            &lib,
        );
        assert_eq!(compiled.prompt, "Write a descriptive message for auth.");
    }

    #[test]
    fn builtin_commit_macro_has_typed_slots() {
        let skills = builtin_skills();
        let commit = skills.iter().find(|s| s.id == "commit-macro").unwrap();
        let SkillPayload::Macro { slots, .. } = &commit.payload else {
            panic!("expected macro payload");
        };
        let style = slots.iter().find(|s| s.id == "style").unwrap();
        assert_eq!(style.kind, SlotKind::Select);
        assert_eq!(
            style.options,
            vec!["conventional".to_string(), "descriptive".to_string()]
        );
        assert!(style.required);
        let scope = slots.iter().find(|s| s.id == "scope").unwrap();
        assert_eq!(scope.kind, SlotKind::Text);
    }

    #[test]
    fn builtin_skills_leave_icons_to_the_host_design_system() {
        assert!(builtin_skills().iter().all(|skill| skill.icon.is_none()));
    }

    fn write_appshot_fixture(data_dir: &std::path::Path, id: &str) -> Vec<u8> {
        let captures = data_dir.join("appshots");
        std::fs::create_dir_all(&captures).unwrap();
        let image = b"\x89PNG\r\n\x1a\nprivate-pixels".to_vec();
        std::fs::write(captures.join(format!("{id}.png")), &image).unwrap();
        std::fs::write(
            captures.join(format!("{id}.json")),
            serde_json::to_vec(&serde_json::json!({
                "id": id,
                "app_name": "Notes",
                "window_title": "Private draft",
                "captured_at": "2026-08-24T10:00:00Z",
                "text": "Visible and accessible content"
            }))
            .unwrap(),
        )
        .unwrap();
        image
    }

    #[test]
    fn appshots_resolve_private_pixels_and_accessibility_text_in_document_order() {
        use base64::Engine as _;

        let data = tempfile::tempdir().unwrap();
        let id = "550e8400-e29b-41d4-a716-446655440000";
        let image = write_appshot_fixture(data.path(), id);
        let doc = vec![
            DocBlock::Text {
                text: "Before".into(),
            },
            DocBlock::Appshot {
                id: id.into(),
                title: Some("Renderer hint is not trusted metadata".into()),
            },
            DocBlock::Text {
                text: "After".into(),
            },
        ];

        let compiled =
            compile_with_appshots(&doc, &sample_library(), None, None, data.path()).unwrap();
        assert!(compiled.unresolved.is_empty());
        assert_eq!(compiled.appshots.len(), 1);
        assert_eq!(compiled.appshots[0].id, id);
        assert_eq!(compiled.appshots[0].mime_type, "image/png");
        assert_eq!(
            compiled.appshots[0].data,
            base64::engine::general_purpose::STANDARD.encode(image)
        );
        let before = compiled.prompt.find("Before").unwrap();
        let appshot = compiled
            .prompt
            .find("**Appshot — Notes: Private draft**")
            .unwrap();
        let after = compiled.prompt.find("After").unwrap();
        assert!(before < appshot && appshot < after);
        assert!(compiled.prompt.contains("Accessible window text"));
        assert_eq!(
            canonical_doc_text(&doc),
            format!("Before\n\n[appshot:{id}]\n\nAfter")
        );
    }

    #[test]
    fn prompt_attachments_resolve_normalized_private_pixels_in_document_order() {
        use base64::Engine as _;

        let data = tempfile::tempdir().unwrap();
        let input = base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
            .unwrap();
        let stored = crate::attachment::import_prompt_attachment(
            data.path(),
            "diagram.webp",
            Some("image/png"),
            &input,
            42,
        )
        .unwrap();
        let doc = vec![
            DocBlock::Text {
                text: "Before".into(),
            },
            DocBlock::Attachment {
                id: stored.id.clone(),
                name: Some("untrusted renderer label".into()),
            },
            DocBlock::Text {
                text: "After".into(),
            },
        ];

        let compiled =
            compile_with_appshots(&doc, &sample_library(), None, None, data.path()).unwrap();
        assert!(compiled.unresolved.is_empty());
        assert_eq!(compiled.attachments.len(), 1);
        assert_eq!(compiled.attachments[0].id, stored.id);
        assert_eq!(compiled.attachments[0].name, "diagram.webp");
        assert_eq!(compiled.attachments[0].mime_type, "image/png");
        assert_eq!(
            compiled.attachments[0].data,
            base64::engine::general_purpose::STANDARD.encode(stored.bytes)
        );
        let before = compiled.prompt.find("Before").unwrap();
        let attachment = compiled
            .prompt
            .find("**Attached image:** \"diagram.webp\"")
            .unwrap();
        let after = compiled.prompt.find("After").unwrap();
        assert!(before < attachment && attachment < after);
        assert_eq!(
            canonical_doc_text(&doc),
            format!("Before\n\n[attachment:{}]\n\nAfter", stored.id)
        );
    }

    #[test]
    fn appshots_reject_invalid_ids_and_mismatched_metadata() {
        let data = tempfile::tempdir().unwrap();
        let invalid = vec![DocBlock::Appshot {
            id: "../../secret".into(),
            title: None,
        }];
        assert_eq!(
            compile_with_appshots(&invalid, &sample_library(), None, None, data.path())
                .unwrap_err(),
            "appshot id is invalid"
        );

        let id = "550e8400-e29b-41d4-a716-446655440001";
        write_appshot_fixture(data.path(), id);
        std::fs::write(
            data.path().join("appshots").join(format!("{id}.json")),
            br#"{"id":"550e8400-e29b-41d4-a716-446655440002"}"#,
        )
        .unwrap();
        let mismatched = vec![DocBlock::Appshot {
            id: id.into(),
            title: None,
        }];
        assert_eq!(
            compile_with_appshots(&mismatched, &sample_library(), None, None, data.path())
                .unwrap_err(),
            "appshot metadata does not match the capture"
        );
    }

    #[cfg(unix)]
    #[test]
    fn appshots_reject_symlinked_private_files() {
        use std::os::unix::fs::symlink;

        let data = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let id = "550e8400-e29b-41d4-a716-446655440003";
        write_appshot_fixture(data.path(), id);
        let image = data.path().join("appshots").join(format!("{id}.png"));
        std::fs::remove_file(&image).unwrap();
        let escaped = outside.path().join("capture.png");
        std::fs::write(&escaped, b"\x89PNG\r\n\x1a\nprivate-pixels").unwrap();
        symlink(&escaped, &image).unwrap();

        let doc = vec![DocBlock::Appshot {
            id: id.into(),
            title: None,
        }];
        assert!(
            compile_with_appshots(&doc, &sample_library(), None, None, data.path())
                .unwrap_err()
                .contains("image path is invalid")
        );
    }

    #[test]
    fn mcp_to_acp_json_shape() {
        let server = McpServer {
            name: "fs".into(),
            cwd: None,
            transport: McpTransport::Stdio {
                command: "mcp-fs".into(),
                args: vec!["--root".into(), "/tmp".into()],
                env: vec![("TOKEN".into(), "abc".into())],
            },
        };
        let v = server.to_acp_json();
        assert_eq!(v["name"], "fs");
        assert_eq!(v["command"], "mcp-fs");
        assert_eq!(v["args"][0], "--root");
        assert_eq!(v["env"][0]["name"], "TOKEN");
        assert_eq!(v["env"][0]["value"], "abc");

        let remote = McpServer {
            name: "remote".into(),
            cwd: None,
            transport: McpTransport::Http {
                url: "https://mcp.example.test".into(),
                headers: vec![("Authorization".into(), "Bearer token".into())],
            },
        };
        let v = remote.to_acp_json();
        assert_eq!(v["type"], "http");
        assert_eq!(v["url"], "https://mcp.example.test");
        assert_eq!(v["headers"][0]["name"], "Authorization");

        let events = McpServer {
            name: "events".into(),
            cwd: None,
            transport: McpTransport::Sse {
                url: "https://mcp.example.test/sse".into(),
                headers: Vec::new(),
            },
        };
        let v = events.to_acp_json();
        assert_eq!(v["type"], "sse");
        assert_eq!(v["url"], "https://mcp.example.test/sse");

        let stored = serde_json::to_string(&events).unwrap();
        assert!(stored.contains(r#""type":"sse""#));
        let round_trip: McpServer = serde_json::from_str(&stored).unwrap();
        assert!(matches!(round_trip.transport, McpTransport::Sse { .. }));

        // Records written before remote transports were tagged remain readable as HTTP.
        let legacy: McpServer = serde_json::from_str(
            r#"{"name":"legacy","url":"https://mcp.example.test","headers":[]}"#,
        )
        .unwrap();
        assert!(matches!(legacy.transport, McpTransport::Http { .. }));
    }

    #[test]
    fn propose_macro_slots_extracts_quotes_paths_and_code_spans() {
        let (template, slots) = propose_macro_slots(
            r#"Refactor `parse_input` in src/lib/parser.rs and rename "old name" to "new name"."#,
        );
        assert_eq!(
            template,
            "Refactor `{{slot-1}}` in {{slot-2}} and rename \"{{slot-3}}\" to \"{{slot-4}}\"."
        );
        assert_eq!(slots.len(), 4);
        assert_eq!(slots[0].kind, SlotKind::Text);
        assert_eq!(slots[0].default.as_deref(), Some("parse_input"));
        assert_eq!(slots[1].kind, SlotKind::File);
        assert_eq!(slots[1].default.as_deref(), Some("src/lib/parser.rs"));
        assert_eq!(slots[1].label, "src/lib/parser.rs");
        assert_eq!(slots[3].id, "slot-4");
    }

    #[test]
    fn propose_macro_slots_dedupes_and_caps_at_six() {
        let (template, slots) = propose_macro_slots(r#""a" "b" "c" "d" "e" "f" "g" "a""#);
        assert_eq!(slots.len(), 6, "seventh distinct value stays verbatim");
        assert!(
            template.ends_with(r#""g" "{{slot-1}}""#),
            "template was {template}"
        );
        assert_eq!(template.matches("{{slot-1}}").count(), 2);
    }

    #[test]
    fn propose_macro_slots_leaves_prose_alone() {
        let text =
            "Don't touch anything here; it's plain prose with a trailing URL https://a.b/c.html";
        let (template, slots) = propose_macro_slots(text);
        assert_eq!(template, text);
        assert!(
            slots.is_empty(),
            "apostrophes and URLs are not slot candidates"
        );
    }
}

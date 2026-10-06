//! Installable C2 Plugin Standard bundles.
//!
//! Installation is data-only. C2 validates and stores plugin files but never runs repository
//! scripts during install. MCP processes start only when the user composes that MCP component into
//! a session; scaffolds are applied explicitly to a selected workspace without overwriting files.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::github_skills::GitHubCheckout;
use crate::harness::parse_frontmatter;
use crate::skill::{
    McpServer, McpTransport, Skill, SkillKind, SkillPayload, SubagentDefinition,
};

const RECORD_FILE: &str = "installed-plugin.json";
const BUNDLE_DIR: &str = "bundle";
const MAX_COMPONENTS: usize = 100;
const MAX_FILES: usize = 5_000;
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_BUNDLE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_DEPTH: usize = 12;
const C2_EXTENSION_NAMESPACE: &str = "dev.codetwo";
const C2_PLUGIN_STANDARD_VERSION: &str = "1.2.0";
const CONNECTOR_CAPABILITIES: &[&str] = &[
    "connection",
    "conversations",
    "documents",
    "tables",
    "messaging",
    "turn_notifications",
    "issues",
    "observations",
];

/// Connector ids whose manifest declares the host-owned `observations` capability.
pub fn observation_connector_ids(connectors: &[PluginConnectorContribution]) -> Vec<String> {
    connectors
        .iter()
        .filter(|c| {
            c.capabilities
                .iter()
                .any(|capability| capability == "observations")
        })
        .map(|c| c.id.clone())
        .collect()
}
const AGENT_PLUGIN_SCHEMA_JSON: &str =
    include_str!("schemas/agent-plugins/1.0.0/plugin.schema.json");
const AGENT_MCP_SCHEMA_JSON: &str = include_str!("schemas/agent-plugins/1.0.0/mcp.schema.json");
static AGENT_PLUGIN_SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(AGENT_PLUGIN_SCHEMA_JSON)
        .expect("bundled Agent Plugins plugin schema must be valid JSON")
});
static AGENT_MCP_SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(AGENT_MCP_SCHEMA_JSON)
        .expect("bundled Agent Plugins MCP schema must be valid JSON")
});

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginCounts {
    pub skills: usize,
    pub subagents: usize,
    pub mcp_servers: usize,
    pub scaffolds: usize,
    pub commands: usize,
    /// Commands implemented by a C2 process runtime and declared statically in plugin.json.
    /// Agent Plugins prompt commands remain in `commands` above.
    pub runtime_commands: usize,
    pub hooks: usize,
    pub lsp_servers: usize,
    pub monitors: usize,
    pub apps: usize,
    pub ui: usize,
    pub connectors: usize,
    /// Agent Scenes 1.0.0 components (`scenes/*.scene.json`).
    pub scenes: usize,
    pub pipelines: usize,
    /// 1 when the bundle ships a C2 runtime declaration — a process C2 speaks the protocol to.
    /// Counted so a code-only plugin is a legitimate bundle rather than "no components".
    pub runtime: usize,
}

impl PluginCounts {
    pub fn total(&self) -> usize {
        self.skills
            + self.subagents
            + self.mcp_servers
            + self.scaffolds
            + self.commands
            + self.runtime_commands
            + self.hooks
            + self.lsp_servers
            + self.monitors
            + self.apps
            + self.ui
            + self.connectors
            + self.scenes
            + self.pipelines
            + self.runtime
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginInstallScope {
    #[default]
    User,
    Project,
    Local,
    Managed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginDiagnosticLevel {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginDiagnostic {
    pub level: PluginDiagnosticLevel,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginExtensionComponent {
    pub kind: String,
    pub name: String,
    pub path: String,
    /// `ready`, `requires_trust`, `requires_auth`, or `unsupported`.
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginLspServer {
    pub id: String,
    pub languages: Vec<String>,
    pub command: String,
    pub args: Vec<String>,
    pub env: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginUiContribution {
    pub id: String,
    pub slot: String,
    pub label: String,
    pub description: String,
    pub command: String,
    pub input: Value,
    pub order: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginConnectorContribution {
    pub id: String,
    pub provider: String,
    pub command: String,
    pub capabilities: Vec<String>,
}

/// A host-readable command contribution implemented by the bundle's process runtime.
///
/// The manifest remains authoritative: the runtime may confirm these commands during initialize,
/// but it cannot add to or rewrite this surface dynamically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginRuntimeCommand {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_schema: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginScaffold {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Relative to the preserved plugin bundle.
    pub path: PathBuf,
    pub files: usize,
}

/// A bundle that ships **code** through `extensions.dev.codetwo.runtime`: a process C2 speaks the
/// plugin protocol to (`docs/reference/plugin-protocol.md`).
///
/// Installing one still executes nothing. Enablement and trust make
/// its host adapter eligible; the first declared command starts the process. See
/// [`crate::plugins::app::protocol`], which is the only place that spawns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRuntimeSpec {
    /// Protocol version the plugin implements. Major must match the host's.
    #[serde(default = "default_plugin_protocol_version")]
    pub protocol: String,
    /// Finite host deadline for commands and callbacks; omitted means 60 seconds.
    #[serde(
        default,
        rename = "commandTimeoutMs",
        deserialize_with = "deserialize_command_timeout",
        skip_serializing_if = "Option::is_none"
    )]
    pub command_timeout_ms: Option<u64>,
    /// Executable to run, resolved against `PATH` or the bundle directory.
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment for the child. C2 passes nothing else of its own.
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    /// Services that must exist before the process is started — the same reactive contract a Rust
    /// plugin gets, declared in the manifest because the host needs it before `initialize`.
    #[serde(default)]
    pub inject: Vec<String>,
    /// Services the plugin can use if present; their arrival or departure restarts it.
    #[serde(default, rename = "optionalInject")]
    pub optional_inject: Vec<String>,
    /// Configuration scopes this process supports. Project instances are created only when the
    /// bundle opts in explicitly.
    #[serde(default = "default_runtime_scope_support", rename = "scopeSupport")]
    pub scope_support: Vec<crate::kernel::PluginScopeSupport>,
}

fn deserialize_command_timeout<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    let value = u64::deserialize(deserializer)?;
    if !(1..=3_600_000).contains(&value) {
        return Err(serde::de::Error::custom(
            "commandTimeoutMs must be between 1 and 3600000",
        ));
    }
    Ok(Some(value))
}

fn default_runtime_scope_support() -> Vec<crate::kernel::PluginScopeSupport> {
    vec![crate::kernel::PluginScopeSupport::User]
}

fn default_plugin_protocol_version() -> String {
    "1.0.0".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledPlugin {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub source: String,
    pub repository: String,
    pub standard_version: String,
    pub enabled: bool,
    pub trusted: bool,
    pub scope: PluginInstallScope,
    pub counts: PluginCounts,
    pub components: Vec<Skill>,
    pub scaffolds: Vec<PluginScaffold>,
    pub extension_components: Vec<PluginExtensionComponent>,
    pub runtime_commands: Vec<PluginRuntimeCommand>,
    pub ui_contributions: Vec<PluginUiContribution>,
    pub connector_contributions: Vec<PluginConnectorContribution>,
    pub lsp_servers: Vec<PluginLspServer>,
    pub diagnostics: Vec<PluginDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<PluginRuntimeSpec>,
}

#[derive(Debug)]
pub struct PluginBundle {
    pub plugin: InstalledPlugin,
    files: Vec<PluginFile>,
}

#[derive(Debug)]
struct PluginFile {
    path: PathBuf,
    bytes: Vec<u8>,
    mode: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScaffoldInstallResult {
    pub plugin: String,
    pub scaffold: String,
    pub destination: String,
    pub files: usize,
}

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("{0}")]
    Invalid(String),
    #[error("Could not read the plugin: {0}")]
    Io(#[from] std::io::Error),
    #[error("Plugin JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("The repository contains multiple plugins; import a specific /tree/ path")]
    MultiplePlugins,
    #[error("The plugin does not contain a supported C2 component")]
    NoComponents,
    #[error("Plugin contains more than {0} components")]
    TooManyComponents(usize),
    #[error("Plugin contains more than {0} files")]
    TooManyFiles(usize),
    #[error("Plugin bundle exceeds the {0} MiB safety limit")]
    BundleTooLarge(u64),
    #[error("Plugin file is larger than the {0} MiB safety limit: {1}")]
    FileTooLarge(u64, String),
    #[error("Plugin path is unsafe: {0}")]
    UnsafePath(String),
    #[error("Unknown plugin: {0}")]
    UnknownPlugin(String),
    #[error("Unknown scaffold: {0}")]
    UnknownScaffold(String),
    #[error("Scaffold would overwrite an existing file: {0}")]
    ScaffoldConflict(String),
    #[error("Scaffold destination path is unsafe: {0}")]
    UnsafeScaffoldPath(String),
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawManifest {
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: Value,
    #[serde(default)]
    repository: String,
    /// C2 Plugin Standard data lives only in this Agent Plugins extension map.
    #[serde(default)]
    extensions: Value,
}

fn semantic_version_major(value: &str) -> Option<u64> {
    let core = value.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    parts.next()?.parse::<u64>().ok()?;
    parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(major)
}

fn c2_extension(
    manifest: &RawManifest,
) -> Result<(&serde_json::Map<String, Value>, &str), PluginError> {
    let extension = manifest
        .extensions
        .as_object()
        .and_then(|extensions| extensions.get(C2_EXTENSION_NAMESPACE))
        .and_then(Value::as_object)
        .ok_or_else(|| {
            PluginError::Invalid(format!(
                "C2 plugins require extensions.{C2_EXTENSION_NAMESPACE}"
            ))
        })?;
    let standard_version = extension
        .get("standardVersion")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if standard_version != C2_PLUGIN_STANDARD_VERSION {
        return Err(PluginError::Invalid(format!(
            "Unsupported C2 plugin standard: {}",
            if standard_version.is_empty() {
                "missing"
            } else {
                standard_version
            }
        )));
    }
    let unknown = extension
        .keys()
        .filter(|key| {
            !matches!(
                key.as_str(),
                "standardVersion"
                    | "runtime"
                    | "commands"
                    | "ui"
                    | "connectors"
                    | "languageServers"
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        return Err(PluginError::Invalid(format!(
            "Unknown C2 plugin fields: {}",
            unknown.join(", ")
        )));
    }
    Ok((extension, standard_version))
}

fn parse_runtime(
    root: &Path,
    raw: Option<&Value>,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Result<Option<PluginRuntimeSpec>, PluginError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let spec: PluginRuntimeSpec = serde_json::from_value(raw.clone())
        .map_err(|error| PluginError::Invalid(format!("Invalid C2 runtime: {error}")))?;
    let command = spec.command.trim();
    if command.is_empty() {
        return Err(PluginError::Invalid(
            "Invalid C2 runtime: `command` is empty".into(),
        ));
    }
    validate_c2_command(root, command, "runtime", "runtime")
        .map_err(|error| PluginError::Invalid(format!("Invalid C2 runtime: {error}")))?;
    diagnostics.push(PluginDiagnostic {
        level: PluginDiagnosticLevel::Warning,
        code: "runtime.requires_trust".into(),
        message: "this plugin ships a process. It runs only after you mark the plugin trusted."
            .into(),
        component: None,
    });
    Ok(Some(spec))
}

#[derive(Debug, Default)]
struct ManifestSet {
    primary: RawManifest,
    standard_version: String,
    runtime: Option<Value>,
    extension_components: Vec<PluginExtensionComponent>,
    runtime_commands: Vec<PluginRuntimeCommand>,
    ui_contributions: Vec<PluginUiContribution>,
    connector_contributions: Vec<PluginConnectorContribution>,
    lsp_servers: Vec<PluginLspServer>,
    diagnostics: Vec<PluginDiagnostic>,
}

#[derive(Debug, Default, Deserialize)]
struct RawScaffold {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
}

#[derive(Debug)]
struct SkillCandidate {
    path: PathBuf,
    strict_agent_skill: bool,
}

/// Build a complete plugin bundle from a verified GitHub checkout.
pub fn from_github(checkout: &GitHubCheckout) -> Result<PluginBundle, PluginError> {
    let selected = checkout
        .selected_root()
        .map_err(|error| PluginError::Invalid(error.to_string()))?;
    let plugin_root = locate_plugin_root(&selected)?;
    let mut manifest_set = load_manifest(&plugin_root)?;
    let manifest = manifest_set.primary.clone();

    let manifest_name = manifest.name.trim().to_string();
    if semantic_version_major(manifest.version.trim()).is_none() {
        return Err(PluginError::Invalid(
            "C2 plugins require a semantic version".into(),
        ));
    }
    let version = manifest.version.trim().to_string();
    let checkout_root = checkout.root.canonicalize()?;
    let root_relative = plugin_root
        .strip_prefix(&checkout_root)
        .unwrap_or(&plugin_root)
        .to_string_lossy();
    let normalized_name = slug(&manifest_name);
    let normalized_name = if normalized_name.is_empty() {
        "plugin"
    } else {
        normalized_name.as_str()
    };
    let plugin_id = format!(
        "{}-{:08x}",
        normalized_name.chars().take(52).collect::<String>(),
        fnv1a(
            format!(
                "{}/{}:{root_relative}",
                checkout.spec.owner, checkout.spec.repo
            )
            .as_bytes()
        )
    );

    let source = format!("Plugin · {manifest_name}");
    let skill_files = discover_skill_files(&plugin_root, &mut manifest_set.diagnostics)?;
    let mut components = parse_skills(
        &plugin_root,
        &plugin_id,
        &source,
        skill_files,
        &mut manifest_set.diagnostics,
    )?;
    components.extend(parse_command_components(
        &plugin_root,
        &plugin_id,
        &source,
        &manifest_set.extension_components,
        &mut manifest_set.diagnostics,
    )?);
    components.extend(parse_subagents(
        &plugin_root,
        &plugin_id,
        &source,
        &mut manifest_set.diagnostics,
    )?);
    components.extend(parse_mcp_servers(&plugin_root, &plugin_id, &source)?);
    if components.len() > MAX_COMPONENTS {
        return Err(PluginError::TooManyComponents(MAX_COMPONENTS));
    }

    let scaffolds = discover_scaffolds(&plugin_root)?;
    let (scene_count, pipeline_count) = count_scene_components(&plugin_root);
    let runtime = parse_runtime(
        &plugin_root,
        manifest_set.runtime.as_ref(),
        &mut manifest_set.diagnostics,
    )?;
    let counts = PluginCounts {
        skills: components
            .iter()
            .filter(|skill| {
                skill.kind() == SkillKind::AgentSkill && !skill.id.contains(":command:")
            })
            .count(),
        subagents: components
            .iter()
            .filter(|skill| skill.kind() == SkillKind::Subagent)
            .count(),
        mcp_servers: components
            .iter()
            .filter(|skill| skill.kind() == SkillKind::Mcp)
            .count(),
        scaffolds: scaffolds.len(),
        commands: manifest_set
            .extension_components
            .iter()
            .filter(|item| item.kind == "command")
            .count(),
        runtime_commands: manifest_set.runtime_commands.len(),
        hooks: manifest_set
            .extension_components
            .iter()
            .filter(|item| item.kind == "hook")
            .count(),
        lsp_servers: manifest_set
            .extension_components
            .iter()
            .filter(|item| item.kind == "lsp")
            .count(),
        monitors: manifest_set
            .extension_components
            .iter()
            .filter(|item| item.kind == "monitor")
            .count(),
        apps: manifest_set
            .extension_components
            .iter()
            .filter(|item| item.kind == "app")
            .count(),
        ui: manifest_set.ui_contributions.len(),
        connectors: manifest_set.connector_contributions.len(),
        scenes: scene_count,
        pipelines: pipeline_count,
        runtime: usize::from(runtime.is_some()),
    };
    if counts.total() == 0 {
        return Err(PluginError::NoComponents);
    }

    let files = collect_bundle_files(&plugin_root)?;
    let repository = if manifest.repository.trim().is_empty() {
        format!(
            "https://github.com/{}/{}",
            checkout.spec.owner, checkout.spec.repo
        )
    } else {
        manifest.repository.trim().to_string()
    };
    Ok(PluginBundle {
        plugin: InstalledPlugin {
            schema_version: 3,
            id: plugin_id,
            name: manifest_name,
            version,
            description: truncate(manifest.description.trim(), 500),
            author: author_name(&manifest.author),
            source: checkout.spec.source(),
            repository,
            standard_version: manifest_set.standard_version,
            enabled: true,
            trusted: false,
            scope: PluginInstallScope::User,
            counts,
            components,
            scaffolds,
            extension_components: manifest_set.extension_components,
            runtime_commands: manifest_set.runtime_commands,
            ui_contributions: manifest_set.ui_contributions,
            connector_contributions: manifest_set.connector_contributions,
            lsp_servers: manifest_set.lsp_servers,
            diagnostics: manifest_set.diagnostics,
            runtime,
        },
        files,
    })
}

/// Build a plugin from an already available directory without ever executing its contents.
/// The directory is copied through the same bounded, symlink-skipping bundle collector used by
/// GitHub installs, so later parsing cannot escape into sibling marketplace files.
pub fn from_local(
    root: &Path,
    source_label: &str,
    source_identity: &str,
) -> Result<PluginBundle, PluginError> {
    let canonical = root.canonicalize()?;
    if !canonical.is_dir() {
        return Err(PluginError::Invalid(
            "Local plugin source must be a directory".into(),
        ));
    }
    let fallback = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .map(slug)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "plugin".into());
    let temporary =
        std::env::temp_dir().join(format!("codetwo-local-plugin-{}", uuid::Uuid::new_v4()));
    let selected = temporary.join(&fallback);
    std::fs::create_dir_all(&selected)?;
    let files = collect_bundle_files(&canonical)?;
    let copy_result = (|| -> Result<(), PluginError> {
        for file in &files {
            let target = selected.join(safe_relative(&file.path)?);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            write_file(&target, file)?;
        }
        Ok(())
    })();
    if let Err(error) = copy_result {
        let _ = std::fs::remove_dir_all(&temporary);
        return Err(error);
    }
    let checkout = GitHubCheckout {
        root: temporary,
        spec: crate::github_skills::GitHubRepoSpec {
            owner: "local".into(),
            repo: format!("source-{:08x}", fnv1a(source_identity.as_bytes())),
            reference: None,
            subpath: Some(PathBuf::from(&fallback)),
        },
    };
    let mut bundle = from_github(&checkout)?;
    bundle.plugin.source = source_label.into();
    bundle.plugin.repository = canonical.display().to_string();
    Ok(bundle)
}

/// Atomically replace one installed plugin directory with the validated bundle.
pub fn install(
    plugins_dir: &Path,
    mut bundle: PluginBundle,
) -> Result<InstalledPlugin, PluginError> {
    std::fs::create_dir_all(plugins_dir)?;
    let id = require_safe_id(&bundle.plugin.id)?;
    std::fs::create_dir_all(plugin_data_dir(plugins_dir, id))?;
    let final_dir = plugins_dir.join(id);
    if let Ok(previous) = std::fs::read_to_string(final_dir.join(RECORD_FILE)).and_then(|text| {
        serde_json::from_str::<InstalledPlugin>(&text)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    }) {
        if previous.schema_version == 3 && previous.standard_version == C2_PLUGIN_STANDARD_VERSION {
            bundle.plugin.enabled = previous.enabled;
            bundle.plugin.trusted = previous.trusted;
            bundle.plugin.scope = previous.scope;
        }
    }
    let stage = plugins_dir.join(format!(".{id}.stage-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(stage.join(BUNDLE_DIR))?;

    let result = (|| -> Result<(), PluginError> {
        for file in &bundle.files {
            let relative = safe_relative(&file.path)?;
            let target = stage.join(BUNDLE_DIR).join(relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            write_file(&target, file)?;
        }
        let record = serde_json::to_vec_pretty(&bundle.plugin)?;
        std::fs::write(stage.join(RECORD_FILE), record)?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(error);
    }

    let backup = plugins_dir.join(format!(".{id}.backup-{}", uuid::Uuid::new_v4()));
    let had_existing = final_dir.exists();
    if had_existing {
        std::fs::rename(&final_dir, &backup)?;
    }
    if let Err(error) = std::fs::rename(&stage, &final_dir) {
        if had_existing {
            let _ = std::fs::rename(&backup, &final_dir);
        }
        let _ = std::fs::remove_dir_all(&stage);
        return Err(error.into());
    }
    if had_existing {
        let _ = std::fs::remove_dir_all(backup);
    }
    Ok(bundle.plugin)
}

/// Load every installed plugin. Malformed records are logged and skipped so one bad plugin cannot
/// hide the rest of the library.
pub fn load_dir(plugins_dir: &Path) -> Result<Vec<InstalledPlugin>, PluginError> {
    let entries = match std::fs::read_dir(plugins_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut plugins = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() || entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let plugin_dir = entry.path();
        let text = match std::fs::read_to_string(plugin_dir.join(RECORD_FILE)) {
            Ok(text) => text,
            Err(error) => {
                tracing::warn!("plugin {:?}: {error}", plugin_dir);
                continue;
            }
        };
        match serde_json::from_str::<InstalledPlugin>(&text) {
            Ok(mut plugin)
                if plugin.schema_version == 3
                    && plugin.standard_version == C2_PLUGIN_STANDARD_VERSION
                    && valid_installed_runtime_contract(&plugin) =>
            {
                let data_dir = plugin_data_dir(plugins_dir, &plugin.id);
                resolve_relative_mcp_commands(&mut plugin, &plugin_dir, &data_dir);
                plugins.push(plugin);
            }
            Ok(_) => tracing::warn!("plugin {:?}: unsupported installed record", plugin_dir),
            Err(error) => tracing::warn!("plugin {:?}: {error}", plugin_dir),
        }
    }
    plugins.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(plugins)
}

fn valid_installed_runtime_contract(plugin: &InstalledPlugin) -> bool {
    if plugin.runtime.is_some() == plugin.runtime_commands.is_empty() {
        return false;
    }
    validate_connector_contributions(&plugin.connector_contributions).is_ok()
        && validate_runtime_commands(&plugin.runtime_commands).is_ok()
        && validate_runtime_command_ownership(
            &plugin.runtime_commands,
            &plugin.ui_contributions,
            &plugin.connector_contributions,
        )
        .is_ok()
}

pub fn uninstall(plugins_dir: &Path, id: &str) -> Result<(), PluginError> {
    uninstall_with_options(plugins_dir, id, false)
}

pub fn uninstall_with_options(
    plugins_dir: &Path,
    id: &str,
    keep_data: bool,
) -> Result<(), PluginError> {
    let id = require_safe_id(id)?;
    let target = plugins_dir.join(id);
    match std::fs::remove_dir_all(&target) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    if !keep_data {
        let data = plugin_data_dir(plugins_dir, id);
        match std::fs::remove_dir_all(data) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub fn set_enabled(
    plugins_dir: &Path,
    id: &str,
    enabled: bool,
) -> Result<InstalledPlugin, PluginError> {
    update_plugin_record(plugins_dir, id, |plugin| plugin.enabled = enabled)
}

pub fn set_trusted(
    plugins_dir: &Path,
    id: &str,
    trusted: bool,
) -> Result<InstalledPlugin, PluginError> {
    update_plugin_record(plugins_dir, id, |plugin| plugin.trusted = trusted)
}

fn update_plugin_record(
    plugins_dir: &Path,
    id: &str,
    update: impl FnOnce(&mut InstalledPlugin),
) -> Result<InstalledPlugin, PluginError> {
    let id = require_safe_id(id)?;
    let plugin_dir = plugins_dir.join(id);
    let record_path = plugin_dir.join(RECORD_FILE);
    let mut plugin: InstalledPlugin =
        serde_json::from_str(&std::fs::read_to_string(&record_path)?)?;
    update(&mut plugin);
    let temporary = plugin_dir.join(format!(".{RECORD_FILE}.{}", uuid::Uuid::new_v4()));
    std::fs::write(&temporary, serde_json::to_vec_pretty(&plugin)?)?;
    std::fs::rename(temporary, record_path)?;
    resolve_relative_mcp_commands(&mut plugin, &plugin_dir, &plugin_data_dir(plugins_dir, id));
    Ok(plugin)
}

/// Copy a stored scaffold into `destination` after a complete conflict preflight.
pub fn apply_scaffold(
    plugins_dir: &Path,
    plugin_id: &str,
    scaffold_id: &str,
    destination: &Path,
) -> Result<ScaffoldInstallResult, PluginError> {
    let plugin_id = require_safe_id(plugin_id)?;
    let scaffold_id = require_safe_id(scaffold_id)?;
    let plugin = load_dir(plugins_dir)?
        .into_iter()
        .find(|plugin| plugin.id == plugin_id)
        .ok_or_else(|| PluginError::UnknownPlugin(plugin_id.to_string()))?;
    let scaffold = plugin
        .scaffolds
        .iter()
        .find(|scaffold| scaffold.id == scaffold_id)
        .ok_or_else(|| PluginError::UnknownScaffold(scaffold_id.to_string()))?;
    let destination = destination.canonicalize()?;
    if !destination.is_dir() {
        return Err(PluginError::Invalid(
            "Scaffold destination must be a directory".into(),
        ));
    }
    let plugin_dir = plugins_dir.join(plugin_id).canonicalize()?;
    let scaffold_root = plugin_dir
        .join(BUNDLE_DIR)
        .join(safe_relative(&scaffold.path)?);
    let scaffold_root = scaffold_root.canonicalize()?;
    if !scaffold_root.starts_with(&plugin_dir) {
        return Err(PluginError::UnsafePath(scaffold.path.display().to_string()));
    }
    let files = collect_apply_files(&scaffold_root)?;
    let targets = files
        .iter()
        .map(|file| scaffold_target(&destination, &file.path))
        .collect::<Result<Vec<_>, _>>()?;
    for (file, target) in files.iter().zip(targets) {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_file(&target, file)?;
    }
    Ok(ScaffoldInstallResult {
        plugin: plugin.name,
        scaffold: scaffold.name.clone(),
        destination: destination.display().to_string(),
        files: files.len(),
    })
}

fn load_manifest(root: &Path) -> Result<ManifestSet, PluginError> {
    let mut diagnostics = Vec::new();
    let manifest_path = root.join("plugin.json");
    ensure_plugin_path(root, &manifest_path)?;
    if !manifest_path.is_file() {
        return Err(PluginError::Invalid(
            "C2 plugin root must contain plugin.json".into(),
        ));
    }
    let value: Value = serde_json::from_str(&std::fs::read_to_string(&manifest_path)?)?;
    validate_agent_manifest(&value)?;
    let primary: RawManifest = serde_json::from_value(value)?;
    let (extension, standard_version) = c2_extension(&primary)?;
    let standard_version = standard_version.to_string();
    let runtime = extension.get("runtime").cloned();
    let runtime_commands = discover_runtime_commands(extension.get("commands"))?;
    let lsp_servers = discover_lsp_servers(root, extension.get("languageServers"))?;
    let mut extension_components =
        discover_extension_components(root, &lsp_servers, &mut diagnostics)?;
    let ui_contributions = discover_ui_contributions(extension.get("ui"))?;
    let connector_contributions = discover_connector_contributions(extension.get("connectors"))?;
    if !ui_contributions.is_empty() && runtime.is_none() {
        return Err(PluginError::Invalid(
            "UI action contributions require extensions.dev.codetwo.runtime".into(),
        ));
    }
    if !connector_contributions.is_empty() && runtime.is_none() {
        return Err(PluginError::Invalid(
            "Connector contributions require extensions.dev.codetwo.runtime".into(),
        ));
    }
    if runtime.is_some() && runtime_commands.is_empty() {
        return Err(PluginError::Invalid(
            "C2 process runtimes require at least one extensions.dev.codetwo.commands declaration"
                .into(),
        ));
    }
    if !runtime_commands.is_empty() && runtime.is_none() {
        return Err(PluginError::Invalid(
            "Runtime command contributions require extensions.dev.codetwo.runtime".into(),
        ));
    }
    if !runtime_commands.is_empty() {
        validate_runtime_command_ownership(
            &runtime_commands,
            &ui_contributions,
            &connector_contributions,
        )?;
    }
    extension_components.extend(ui_contributions.iter().map(|contribution| {
        PluginExtensionComponent {
            kind: "ui".into(),
            name: contribution.label.clone(),
            path: format!(
                "plugin.json#extensions.{C2_EXTENSION_NAMESPACE}.ui.{}.{}",
                contribution.id, contribution.slot
            ),
            status: "requires_trust".into(),
        }
    }));
    extension_components.extend(connector_contributions.iter().map(|contribution| {
        PluginExtensionComponent {
            kind: "connector".into(),
            name: contribution.provider.clone(),
            path: format!(
                "plugin.json#extensions.{C2_EXTENSION_NAMESPACE}.connectors.{}.{}",
                contribution.id, contribution.provider
            ),
            status: "requires_trust".into(),
        }
    }));
    Ok(ManifestSet {
        primary,
        standard_version,
        runtime,
        extension_components,
        runtime_commands,
        ui_contributions,
        connector_contributions,
        lsp_servers,
        diagnostics,
    })
}

fn valid_runtime_command_id(command: &str) -> bool {
    if command.is_empty() || command.len() > 128 {
        return false;
    }
    let mut segments = command.split('.');
    let Some(first) = segments.next() else {
        return false;
    };
    let valid_segment = |segment: &str| {
        !segment.is_empty()
            && segment
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    };
    valid_segment(first) && segments.clone().next().is_some() && segments.all(valid_segment)
}

fn discover_runtime_commands(
    value: Option<&Value>,
) -> Result<Vec<PluginRuntimeCommand>, PluginError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let entries = value.as_array().ok_or_else(|| {
        PluginError::Invalid("extensions.dev.codetwo.commands must be an array".into())
    })?;
    let mut commands = Vec::new();
    for (index, value) in entries.iter().enumerate() {
        let command: PluginRuntimeCommand =
            serde_json::from_value(value.clone()).map_err(|error| {
                PluginError::Invalid(format!(
                    "extensions.dev.codetwo.commands[{index}] is invalid: {error}"
                ))
            })?;
        commands.push(command);
    }
    validate_runtime_commands(&commands)?;
    for command in &mut commands {
        command.title = command.title.trim().to_string();
        command.description = command.description.trim().to_string();
    }
    Ok(commands)
}

fn validate_runtime_commands(commands: &[PluginRuntimeCommand]) -> Result<(), PluginError> {
    if commands.len() > MAX_COMPONENTS {
        return Err(PluginError::Invalid(format!(
            "extensions.dev.codetwo.commands cannot contain more than {MAX_COMPONENTS} entries"
        )));
    }
    let mut ids = HashSet::new();
    for (index, command) in commands.iter().enumerate() {
        if !valid_runtime_command_id(&command.id) {
            return Err(PluginError::Invalid(format!(
                "extensions.dev.codetwo.commands[{index}] requires a namespaced id"
            )));
        }
        if !ids.insert(command.id.as_str()) {
            return Err(PluginError::Invalid(format!(
                "Duplicate C2 runtime command id: {}",
                command.id
            )));
        }
        if command.title.trim().is_empty() || command.title.len() > 80 {
            return Err(PluginError::Invalid(format!(
                "extensions.dev.codetwo.commands[{index}] requires a title"
            )));
        }
        if command.description.len() > 300 {
            return Err(PluginError::Invalid(format!(
                "extensions.dev.codetwo.commands[{index}] has an invalid description"
            )));
        }
        if command
            .args_schema
            .as_ref()
            .is_some_and(|schema| !schema.is_object())
        {
            return Err(PluginError::Invalid(format!(
                "extensions.dev.codetwo.commands[{index}].argsSchema must be an object"
            )));
        }
    }
    Ok(())
}

fn validate_runtime_command_ownership(
    commands: &[PluginRuntimeCommand],
    ui_contributions: &[PluginUiContribution],
    connector_contributions: &[PluginConnectorContribution],
) -> Result<(), PluginError> {
    let declared = commands
        .iter()
        .map(|command| command.id.as_str())
        .collect::<HashSet<_>>();
    if let Some(contribution) = ui_contributions
        .iter()
        .find(|contribution| !declared.contains(contribution.command.as_str()))
    {
        return Err(PluginError::Invalid(format!(
            "UI action `{}` references undeclared runtime command `{}`",
            contribution.id, contribution.command
        )));
    }
    if let Some(contribution) = connector_contributions
        .iter()
        .find(|contribution| !declared.contains(contribution.command.as_str()))
    {
        return Err(PluginError::Invalid(format!(
            "Connector `{}` references undeclared runtime command `{}`",
            contribution.id, contribution.command
        )));
    }
    Ok(())
}

fn validate_connector_contributions(
    contributions: &[PluginConnectorContribution],
) -> Result<(), PluginError> {
    if contributions.len() > MAX_COMPONENTS {
        return Err(PluginError::Invalid(format!(
            "extensions.dev.codetwo.connectors cannot contain more than {MAX_COMPONENTS} entries"
        )));
    }
    let mut ids = HashSet::new();
    for (index, contribution) in contributions.iter().enumerate() {
        if require_safe_id(&contribution.id).is_err() || !ids.insert(contribution.id.as_str()) {
            return Err(PluginError::Invalid(format!(
                "extensions.dev.codetwo.connectors[{index}] has an invalid or duplicate id"
            )));
        }
        if require_safe_id(&contribution.provider).is_err()
            || !valid_runtime_command_id(&contribution.command)
            || contribution.capabilities.is_empty()
            || contribution
                .capabilities
                .iter()
                .any(|capability| !CONNECTOR_CAPABILITIES.contains(&capability.as_str()))
        {
            return Err(PluginError::Invalid(format!(
                "extensions.dev.codetwo.connectors[{index}] is invalid"
            )));
        }
    }
    Ok(())
}

fn locate_plugin_root(selected: &Path) -> Result<PathBuf, PluginError> {
    let root = selected.canonicalize()?;
    if !root.is_dir() || !root.join("plugin.json").is_file() {
        return Err(PluginError::Invalid(
            "C2 plugin root must contain plugin.json; select the bundle directory explicitly"
                .into(),
        ));
    }
    Ok(root)
}

fn validate_agent_manifest(value: &Value) -> Result<(), PluginError> {
    let object = value.as_object().ok_or_else(|| {
        PluginError::Invalid("Agent Plugins plugin.json must be an object".into())
    })?;
    let schema = object
        .get("$schema")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let supported_schema = AGENT_PLUGIN_SCHEMA
        .get("$id")
        .and_then(Value::as_str)
        .expect("bundled Agent Plugins plugin schema must declare $id");
    if schema != supported_schema {
        return Err(PluginError::Invalid(format!(
            "Unsupported Agent Plugins schema: {schema}"
        )));
    }
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !valid_agent_plugin_name(name) {
        return Err(PluginError::Invalid(
            "Agent Plugins name must match the 1.0.0 naming rules".into(),
        ));
    }
    let allowed = [
        "$schema",
        "name",
        "version",
        "description",
        "author",
        "homepage",
        "repository",
        "license",
        "keywords",
        "extensions",
    ];
    let unknown = object
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        return Err(PluginError::Invalid(format!(
            "Unknown Agent Plugins manifest fields: {}",
            unknown.join(", ")
        )));
    }
    if let Some(extensions) = object.get("extensions") {
        match extensions.as_object() {
            Some(extensions) => {
                if let Some((namespace, _)) =
                    extensions.iter().find(|(_, value)| !value.is_object())
                {
                    return Err(PluginError::Invalid(format!(
                        "Agent Plugins extension {namespace} must be an object"
                    )));
                }
            }
            None => {
                return Err(PluginError::Invalid(
                    "Agent Plugins extensions must be an object".into(),
                ));
            }
        }
    }
    for key in [
        "version",
        "description",
        "homepage",
        "repository",
        "license",
    ] {
        if object.get(key).is_some_and(|value| !value.is_string()) {
            return Err(PluginError::Invalid(format!(
                "Agent Plugins manifest field {key} must be a string"
            )));
        }
    }
    if let Some(author) = object.get("author") {
        let author = author.as_object().ok_or_else(|| {
            PluginError::Invalid("Agent Plugins manifest field author must be an object".into())
        })?;
        if author
            .keys()
            .any(|key| !matches!(key.as_str(), "name" | "email" | "url"))
            || author.values().any(|value| !value.is_string())
        {
            return Err(PluginError::Invalid(
                "Agent Plugins manifest field author must match the 1.0.0 schema".into(),
            ));
        }
    }
    if object.get("keywords").is_some_and(|value| {
        value
            .as_array()
            .is_none_or(|items| items.iter().any(|item| !item.is_string()))
    }) {
        return Err(PluginError::Invalid(
            "Agent Plugins manifest field keywords must be an array of strings".into(),
        ));
    }
    Ok(())
}

fn valid_agent_plugin_name(value: &str) -> bool {
    let len = value.chars().count();
    (1..=64).contains(&len)
        && !value.contains("--")
        && !value.contains("..")
        && value
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '.' | '-'))
        && value
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
        && value
            .chars()
            .last()
            .is_some_and(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
}

fn discover_lsp_servers(
    root: &Path,
    value: Option<&Value>,
) -> Result<Vec<PluginLspServer>, PluginError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let entries = value.as_array().ok_or_else(|| {
        PluginError::Invalid("extensions.dev.codetwo.languageServers must be an array".into())
    })?;
    let mut servers = Vec::new();
    let mut ids = HashSet::new();
    for (index, value) in entries.iter().enumerate() {
        let server = parse_lsp_server(root, value).map_err(|message| {
            PluginError::Invalid(format!(
                "extensions.dev.codetwo.languageServers[{index}] is invalid: {message}"
            ))
        })?;
        if !ids.insert(server.id.clone()) {
            return Err(PluginError::Invalid(format!(
                "Duplicate C2 language server id: {}",
                server.id
            )));
        }
        servers.push(server);
    }
    Ok(servers)
}

fn discover_ui_contributions(
    value: Option<&Value>,
) -> Result<Vec<PluginUiContribution>, PluginError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let entries = value
        .as_array()
        .ok_or_else(|| PluginError::Invalid("extensions.dev.codetwo.ui must be an array".into()))?;
    let slots = [
        "rail.features",
        "session.header",
        "transcript.before",
        "composer.above",
        "composer.toolbar",
        "host.actions",
    ];
    let mut ids = HashSet::new();
    let mut contributions = Vec::new();
    for (index, value) in entries.iter().enumerate() {
        let item = value.as_object().ok_or_else(|| {
            PluginError::Invalid(format!(
                "extensions.dev.codetwo.ui[{index}] must be an object"
            ))
        })?;
        let unknown = item
            .keys()
            .filter(|key| {
                !matches!(
                    key.as_str(),
                    "id" | "slot" | "label" | "description" | "command" | "input" | "order"
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        if !unknown.is_empty() {
            return Err(PluginError::Invalid(format!(
                "extensions.dev.codetwo.ui[{index}] has unknown fields: {}",
                unknown.join(", ")
            )));
        }
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| require_safe_id(id).is_ok())
            .ok_or_else(|| {
                PluginError::Invalid(format!(
                    "extensions.dev.codetwo.ui[{index}] requires a safe id"
                ))
            })?;
        if !ids.insert(id.to_string()) {
            return Err(PluginError::Invalid(format!(
                "Duplicate C2 UI contribution id: {id}"
            )));
        }
        let slot = item
            .get("slot")
            .and_then(Value::as_str)
            .filter(|slot| slots.contains(slot))
            .ok_or_else(|| {
                PluginError::Invalid(format!(
                    "extensions.dev.codetwo.ui[{index}] has an invalid slot"
                ))
            })?;
        let label = item
            .get("label")
            .and_then(Value::as_str)
            .filter(|label| !label.trim().is_empty() && label.len() <= 80)
            .ok_or_else(|| {
                PluginError::Invalid(format!(
                    "extensions.dev.codetwo.ui[{index}] requires a label"
                ))
            })?;
        let description = match item.get("description") {
            None => "",
            Some(Value::String(description)) if description.len() <= 300 => description.trim(),
            Some(_) => {
                return Err(PluginError::Invalid(format!(
                    "extensions.dev.codetwo.ui[{index}] has an invalid description"
                )))
            }
        };
        let command = item
            .get("command")
            .and_then(Value::as_str)
            .filter(|command| valid_runtime_command_id(command))
            .ok_or_else(|| {
                PluginError::Invalid(format!(
                    "extensions.dev.codetwo.ui[{index}] requires a namespaced command"
                ))
            })?;
        let order = match item.get("order") {
            None => 0,
            Some(Value::Number(order)) => order
                .as_i64()
                .filter(|order| (-100..=100).contains(order))
                .ok_or_else(|| {
                    PluginError::Invalid(format!(
                        "extensions.dev.codetwo.ui[{index}] has an invalid order"
                    ))
                })? as i32,
            Some(_) => {
                return Err(PluginError::Invalid(format!(
                    "extensions.dev.codetwo.ui[{index}] has an invalid order"
                )))
            }
        };
        contributions.push(PluginUiContribution {
            id: id.into(),
            slot: slot.to_string(),
            label: label.trim().into(),
            description: description.into(),
            command: command.into(),
            input: item.get("input").cloned().unwrap_or(Value::Null),
            order,
        });
    }
    Ok(contributions)
}

fn discover_connector_contributions(
    value: Option<&Value>,
) -> Result<Vec<PluginConnectorContribution>, PluginError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let entries = value.as_array().ok_or_else(|| {
        PluginError::Invalid("extensions.dev.codetwo.connectors must be an array".into())
    })?;
    if entries.len() > MAX_COMPONENTS {
        return Err(PluginError::Invalid(format!(
            "extensions.dev.codetwo.connectors cannot contain more than {MAX_COMPONENTS} entries"
        )));
    }
    let mut ids = HashSet::new();
    let mut contributions = Vec::new();
    for (index, value) in entries.iter().enumerate() {
        let item = value.as_object().ok_or_else(|| {
            PluginError::Invalid(format!(
                "extensions.dev.codetwo.connectors[{index}] must be an object"
            ))
        })?;
        let unknown = item
            .keys()
            .filter(|key| !matches!(key.as_str(), "id" | "provider" | "command" | "capabilities"))
            .cloned()
            .collect::<Vec<_>>();
        if !unknown.is_empty() {
            return Err(PluginError::Invalid(format!(
                "extensions.dev.codetwo.connectors[{index}] has unknown fields: {}",
                unknown.join(", ")
            )));
        }
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| require_safe_id(id).is_ok())
            .ok_or_else(|| {
                PluginError::Invalid(format!(
                    "extensions.dev.codetwo.connectors[{index}] requires a safe id"
                ))
            })?;
        if !ids.insert(id.to_string()) {
            return Err(PluginError::Invalid(format!(
                "Duplicate C2 connector contribution id: {id}"
            )));
        }
        let provider = item
            .get("provider")
            .and_then(Value::as_str)
            .filter(|provider| require_safe_id(provider).is_ok())
            .ok_or_else(|| {
                PluginError::Invalid(format!(
                    "extensions.dev.codetwo.connectors[{index}] requires a safe provider"
                ))
            })?;
        let command = item
            .get("command")
            .and_then(Value::as_str)
            .filter(|command| valid_runtime_command_id(command))
            .ok_or_else(|| {
                PluginError::Invalid(format!(
                    "extensions.dev.codetwo.connectors[{index}] requires a namespaced command"
                ))
            })?;
        let raw_capabilities = item
            .get("capabilities")
            .and_then(Value::as_array)
            .filter(|items| !items.is_empty())
            .ok_or_else(|| {
                PluginError::Invalid(format!(
                    "extensions.dev.codetwo.connectors[{index}] requires capabilities"
                ))
            })?;
        let mut seen_capabilities = HashSet::new();
        let mut capabilities = Vec::new();
        for capability in raw_capabilities {
            let capability = capability
                .as_str()
                .filter(|capability| CONNECTOR_CAPABILITIES.contains(capability))
                .ok_or_else(|| {
                    PluginError::Invalid(format!(
                        "extensions.dev.codetwo.connectors[{index}] has an invalid capability"
                    ))
                })?;
            if seen_capabilities.insert(capability) {
                capabilities.push(capability.to_string());
            }
        }
        contributions.push(PluginConnectorContribution {
            id: id.into(),
            provider: provider.into(),
            command: command.into(),
            capabilities,
        });
    }
    validate_connector_contributions(&contributions)?;
    Ok(contributions)
}

fn parse_lsp_server(root: &Path, value: &Value) -> Result<PluginLspServer, String> {
    let config = value
        .as_object()
        .ok_or_else(|| "language server must be an object".to_string())?;
    let unknown = config
        .keys()
        .filter(|key| {
            !matches!(
                key.as_str(),
                "id" | "languages" | "command" | "args" | "env"
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        return Err(format!("unknown fields: {}", unknown.join(", ")));
    }
    let id = config
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| require_safe_id(id).is_ok())
        .ok_or_else(|| "language server requires a safe id".to_string())?;
    let command = config
        .get("command")
        .and_then(Value::as_str)
        .filter(|command| !command.trim().is_empty())
        .ok_or_else(|| format!("language server {id} requires command"))?;
    validate_c2_command(root, command, id, "language server")?;
    let args = string_array(config.get("args"), &format!("language server {id} args"))
        .map_err(|error| error.to_string())?;
    let env = string_map(config.get("env"), &format!("language server {id} env"))
        .map_err(|error| error.to_string())?;
    let languages = string_array(
        config.get("languages"),
        &format!("language server {id} languages"),
    )
    .map_err(|error| error.to_string())?
    .into_iter()
    .map(|language| language.to_ascii_lowercase())
    .collect::<HashSet<_>>();
    if languages.is_empty()
        || languages.len() > 16
        || languages.iter().any(|language| {
            !language
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || "+_.-".contains(ch))
        })
    {
        return Err(format!(
            "language server {id} requires one to sixteen valid language ids"
        ));
    }
    Ok(PluginLspServer {
        id: id.into(),
        languages: languages.into_iter().collect(),
        command: command.into(),
        args,
        env: env.into_iter().collect(),
    })
}

fn validate_c2_command(root: &Path, command: &str, name: &str, kind: &str) -> Result<(), String> {
    if command.contains('/') || command.contains('\\') {
        if Path::new(command).is_absolute() || command.contains("..") || command.contains("${") {
            return Err(format!(
                "{kind} {name} command must be a bare executable or bundle-relative path"
            ));
        }
        let path = root.join(safe_relative(Path::new(command)).map_err(|error| error.to_string())?);
        if !path.exists() {
            return Err(format!(
                "{kind} {name} command does not exist in the bundle"
            ));
        }
        ensure_plugin_path(root, &path).map_err(|error| error.to_string())?;
        return Ok(());
    }
    if command.contains("${") || command.contains("..") {
        return Err(format!(
            "{kind} {name} command must be a bare executable or bundle-relative path"
        ));
    }
    Ok(())
}

fn discover_extension_components(
    root: &Path,
    lsp_servers: &[PluginLspServer],
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Result<Vec<PluginExtensionComponent>, PluginError> {
    let mut components = Vec::new();
    let command_paths = root
        .join("commands")
        .is_dir()
        .then(|| vec!["./commands".to_string()])
        .unwrap_or_default();

    discover_markdown_components(
        root,
        &command_paths,
        "command",
        "ready",
        &mut components,
        diagnostics,
    )?;
    components.extend(lsp_servers.iter().map(|server| PluginExtensionComponent {
        kind: "lsp".into(),
        name: server.id.clone(),
        path: format!("plugin.json#extensions.{C2_EXTENSION_NAMESPACE}.languageServers"),
        status: "requires_trust".into(),
    }));
    for (relative, kind) in [
        ("hooks/hooks.json", "hook"),
        ("monitors/monitors.json", "monitor"),
    ] {
        let path = root.join(relative);
        if path.is_file() {
            ensure_plugin_path(root, &path)?;
            components.push(PluginExtensionComponent {
                kind: kind.into(),
                name: kind.into(),
                path: relative.into(),
                status: "unsupported".into(),
            });
        }
    }

    components.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.path.cmp(&right.path))
    });
    components.dedup_by(|left, right| {
        left.kind == right.kind && left.name == right.name && left.path == right.path
    });

    for component in &components {
        if component.status == "unsupported" {
            diagnostics.push(PluginDiagnostic {
                level: PluginDiagnosticLevel::Warning,
                code: "component_unsupported".into(),
                message: format!(
                    "{} is preserved but has no C2 runtime adapter yet",
                    component.kind
                ),
                component: Some(component.path.clone()),
            });
        }
    }
    Ok(components)
}

fn discover_markdown_components(
    root: &Path,
    configured: &[String],
    kind: &str,
    status: &str,
    output: &mut Vec<PluginExtensionComponent>,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Result<(), PluginError> {
    for relative in configured {
        let path = root.join(safe_relative(Path::new(relative))?);
        if !path.exists() {
            diagnostics.push(missing_component_diagnostic(kind, relative));
            continue;
        }
        ensure_plugin_path(root, &path)?;
        let mut files = Vec::new();
        if path.is_file() {
            files.push(path);
        } else {
            collect_extension(&path, "md", 0, &mut files)?;
        }
        for file in files {
            let relative = file.strip_prefix(root).unwrap_or(&file).to_string_lossy();
            output.push(PluginExtensionComponent {
                kind: kind.into(),
                name: file
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or(kind)
                    .to_string(),
                path: relative.into_owned(),
                status: status.into(),
            });
        }
    }
    Ok(())
}

fn missing_component_diagnostic(kind: &str, relative: &str) -> PluginDiagnostic {
    PluginDiagnostic {
        level: PluginDiagnosticLevel::Warning,
        code: format!("missing_{kind}_path"),
        message: format!("Configured {kind} path does not exist: {relative}"),
        component: Some(relative.into()),
    }
}

fn discover_skill_files(
    root: &Path,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Result<Vec<SkillCandidate>, PluginError> {
    let mut files = Vec::new();
    let skills_root = root.join("skills");
    match std::fs::read_dir(&skills_root) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                let metadata = std::fs::symlink_metadata(entry.path())?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    continue;
                }
                let manifest = entry.path().join("SKILL.md");
                if manifest.is_file() {
                    ensure_plugin_path(root, &manifest)?;
                    files.push(SkillCandidate {
                        path: manifest,
                        strict_agent_skill: true,
                    });
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            diagnostics.push(PluginDiagnostic {
                level: PluginDiagnosticLevel::Error,
                code: "agent_skills_location_invalid".into(),
                message: format!("Could not read Agent Plugins skills directory: {error}"),
                component: Some("skills".into()),
            });
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn parse_skills(
    root: &Path,
    plugin_id: &str,
    source: &str,
    files: Vec<SkillCandidate>,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Result<Vec<Skill>, PluginError> {
    let mut skills = Vec::new();
    for candidate in files {
        let path = candidate.path;
        let text = read_small_text(&path)?;
        let prompt = markdown_body(&text).trim();
        if prompt.is_empty() {
            diagnostics.push(PluginDiagnostic {
                level: PluginDiagnosticLevel::Warning,
                code: "empty_skill".into(),
                message: "Skipped a Skill with no instructions".into(),
                component: Some(
                    path.strip_prefix(root)
                        .unwrap_or(&path)
                        .display()
                        .to_string(),
                ),
            });
            continue;
        }
        let fallback = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or("Skill");
        let (name, description) = parse_frontmatter(&text);
        if candidate.strict_agent_skill
            && (name
                .as_deref()
                .is_none_or(|name| !valid_agent_skill_name(name))
                || description
                    .as_deref()
                    .is_none_or(|description| description.trim().is_empty()))
        {
            diagnostics.push(PluginDiagnostic {
                level: PluginDiagnosticLevel::Warning,
                code: "invalid_agent_skill".into(),
                message: "Skipped Agent Skill: frontmatter requires a valid name and non-empty description".into(),
                component: Some(path.strip_prefix(root).unwrap_or(&path).display().to_string()),
            });
            continue;
        }
        let name = name.unwrap_or_else(|| fallback.to_string());
        let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();
        skills.push(Skill {
            id: format!("{plugin_id}:skill:{}", slug_with_hash(&relative)),
            name: name.clone(),
            description: truncate(description.unwrap_or_default().trim(), 280),
            icon: None,
            source: Some(source.to_string()),
            payload: SkillPayload::AgentSkill {
                skill_ref: name,
                inline_text: Some(prompt.to_string()),
            },
        });
    }
    Ok(skills)
}

fn valid_agent_skill_name(value: &str) -> bool {
    let len = value.chars().count();
    (1..=64).contains(&len)
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

fn parse_command_components(
    root: &Path,
    plugin_id: &str,
    source: &str,
    inventory: &[PluginExtensionComponent],
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Result<Vec<Skill>, PluginError> {
    let mut commands = Vec::new();
    for item in inventory.iter().filter(|item| item.kind == "command") {
        let path = root.join(safe_relative(Path::new(&item.path))?);
        ensure_plugin_path(root, &path)?;
        let text = read_small_text(&path)?;
        let prompt = markdown_body(&text).trim();
        if prompt.is_empty() {
            diagnostics.push(PluginDiagnostic {
                level: PluginDiagnosticLevel::Warning,
                code: "empty_command".into(),
                message: "Skipped a command with no instructions".into(),
                component: Some(item.path.clone()),
            });
            continue;
        }
        let (frontmatter_name, description) = parse_frontmatter(&text);
        let name = frontmatter_name.unwrap_or_else(|| item.name.clone());
        commands.push(Skill {
            id: format!("{plugin_id}:command:{}", slug_with_hash(&item.path)),
            name: name.clone(),
            description: truncate(description.unwrap_or_default().trim(), 280),
            icon: None,
            source: Some(source.to_string()),
            payload: SkillPayload::AgentSkill {
                skill_ref: name,
                inline_text: Some(prompt.to_string()),
            },
        });
    }
    Ok(commands)
}

fn parse_subagents(
    root: &Path,
    plugin_id: &str,
    source: &str,
    _diagnostics: &mut Vec<PluginDiagnostic>,
) -> Result<Vec<Skill>, PluginError> {
    let mut files = Vec::new();
    collect_extension(&root.join("agents"), "md", 0, &mut files)?;
    files.sort();
    files.dedup();
    let mut agents = Vec::new();
    for path in files {
        let text = read_small_text(&path)?;
        let prompt = markdown_body(&text).trim();
        if prompt.is_empty() {
            continue;
        }
        let fallback = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("Subagent");
        let (name, description) = parse_frontmatter(&text);
        let name = name.unwrap_or_else(|| fallback.to_string());
        let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();
        agents.push(Skill {
            id: format!("{plugin_id}:agent:{}", slug_with_hash(&relative)),
            name: name.clone(),
            description: truncate(description.unwrap_or_default().trim(), 280),
            icon: None,
            source: Some(source.to_string()),
            payload: SkillPayload::Subagent {
                agent: SubagentDefinition {
                    name,
                    description: frontmatter_scalar(&text, "description").unwrap_or_default(),
                    prompt: prompt.to_string(),
                    model: frontmatter_scalar(&text, "model"),
                    tools: frontmatter_list(&text, "tools"),
                },
            },
        });
    }
    Ok(agents)
}

fn parse_mcp_servers(
    root: &Path,
    plugin_id: &str,
    source: &str,
) -> Result<Vec<Skill>, PluginError> {
    let path = root.join("mcp.json");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    ensure_plugin_path(root, &path)?;
    let value = serde_json::from_str::<Value>(&std::fs::read_to_string(&path)?)?;
    let servers = mcp_entries(&value).map_err(PluginError::Invalid)?;

    let mut components = Vec::new();
    for (name, config) in servers {
        let server = parse_mcp_server(root, name, config).map_err(PluginError::Invalid)?;
        components.push(Skill {
            id: format!("{plugin_id}:mcp:{}", slug_with_hash(name)),
            name: name.to_string(),
            description: format!("MCP server from plugin {source}"),
            icon: None,
            source: Some(source.to_string()),
            payload: SkillPayload::Mcp { server },
        });
    }
    Ok(components)
}

fn mcp_entries(value: &Value) -> Result<&serde_json::Map<String, Value>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "MCP configuration must be a JSON object".to_string())?;
    let schema = object
        .get("$schema")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let supported_schema = AGENT_MCP_SCHEMA
        .get("$id")
        .and_then(Value::as_str)
        .expect("bundled Agent Plugins MCP schema must declare $id");
    if schema != supported_schema {
        return Err(format!("Unsupported Agent Plugins MCP schema: {schema}"));
    }
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "$schema" | "mcpServers"))
    {
        return Err("Agent Plugins mcp.json contains unknown top-level fields".into());
    }
    object
        .get("mcpServers")
        .and_then(Value::as_object)
        .ok_or_else(|| "Agent Plugins mcp.json requires an mcpServers object".into())
}

fn parse_mcp_server(root: &Path, name: &str, value: &Value) -> Result<McpServer, String> {
    let config = value
        .as_object()
        .ok_or_else(|| format!("MCP server {name} must be an object"))?;
    let transport_name = config
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("MCP server {name} requires a type"))?;

    match transport_name {
        "stdio" => {
            reject_unknown_fields(config, &["type", "command", "args", "env", "cwd"], name)?;
            let command = config
                .get("command")
                .and_then(Value::as_str)
                .filter(|command| !command.is_empty())
                .ok_or_else(|| format!("MCP server {name} requires a non-empty command"))?;
            validate_stdio_command(root, command, name)?;
            let args = string_array(config.get("args"), &format!("MCP server {name} args"))
                .map_err(|error| error.to_string())?;
            let env = string_map(config.get("env"), &format!("MCP server {name} env"))
                .map_err(|error| error.to_string())?;
            if env
                .iter()
                .any(|(key, _)| matches!(key.as_str(), "PLUGIN_ROOT" | "PLUGIN_DATA"))
            {
                return Err(format!(
                    "MCP server {name} cannot set reserved plugin environment variables"
                ));
            }
            let cwd = config
                .get("cwd")
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| format!("MCP server {name} cwd must be a string"))
                        .and_then(|cwd| validate_mcp_cwd(root, cwd, name))
                })
                .transpose()?;
            Ok(McpServer {
                name: name.into(),
                cwd,
                transport: McpTransport::Stdio {
                    command: command.into(),
                    args,
                    env,
                },
            })
        }
        "http" | "streamable-http" | "sse" => {
            reject_unknown_fields(config, &["type", "url", "headers"], name)?;
            if transport_name == "http" {
                return Err(format!(
                    "MCP server {name} must declare streamable-http, not http, in Agent Plugins"
                ));
            }
            let url = config
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("MCP server {name} requires a URL"))?;
            validate_remote_url(url, name)?;
            let headers = string_map(config.get("headers"), &format!("MCP server {name} headers"))
                .map_err(|error| error.to_string())?;
            validate_headers(&headers, name)?;
            let transport = if transport_name == "sse" {
                McpTransport::Sse {
                    url: url.into(),
                    headers,
                }
            } else {
                McpTransport::Http {
                    url: url.into(),
                    headers,
                }
            };
            Ok(McpServer {
                name: name.into(),
                cwd: None,
                transport,
            })
        }
        other => Err(format!(
            "MCP server {name} has unsupported transport {other}"
        )),
    }
}

fn reject_unknown_fields(
    config: &serde_json::Map<String, Value>,
    allowed: &[&str],
    name: &str,
) -> Result<(), String> {
    if let Some(field) = config
        .keys()
        .find(|field| !allowed.contains(&field.as_str()))
    {
        return Err(format!("MCP server {name} contains unknown field {field}"));
    }
    Ok(())
}

fn validate_stdio_command(root: &Path, command: &str, name: &str) -> Result<(), String> {
    if command.contains("${") {
        return Err(format!(
            "MCP server {name} command cannot contain placeholders"
        ));
    }
    if command.starts_with("./") {
        let path = root.join(safe_relative(Path::new(command)).map_err(|error| error.to_string())?);
        if !path.is_file() {
            return Err(format!(
                "MCP server {name} command does not exist in the bundle"
            ));
        }
        ensure_plugin_path(root, &path).map_err(|error| error.to_string())?;
    } else if command.contains('/') || command.contains('\\') {
        return Err(format!(
            "MCP server {name} command must be a bare executable or start with ./"
        ));
    }
    Ok(())
}

fn validate_mcp_cwd(root: &Path, cwd: &str, name: &str) -> Result<String, String> {
    let relative = if let Some(relative) = cwd.strip_prefix("./") {
        Some(relative)
    } else if let Some(relative) = cwd.strip_prefix("${PLUGIN_ROOT}") {
        Some(relative.trim_start_matches('/'))
    } else if cwd == "${PLUGIN_DATA}" || cwd.starts_with("${PLUGIN_DATA}/") {
        None
    } else {
        return Err(format!("MCP server {name} cwd has an unsupported form"));
    };
    if let Some(relative) = relative.filter(|value| !value.is_empty()) {
        let path =
            root.join(safe_relative(Path::new(relative)).map_err(|error| error.to_string())?);
        if !path.is_dir() {
            return Err(format!(
                "MCP server {name} cwd does not exist in the bundle"
            ));
        }
        ensure_plugin_path(root, &path).map_err(|error| error.to_string())?;
    }
    Ok(cwd.into())
}

fn validate_remote_url(value: &str, name: &str) -> Result<(), String> {
    if value.contains("${") {
        return Err(format!("MCP server {name} URL cannot contain placeholders"));
    }
    let url =
        url::Url::parse(value).map_err(|_| format!("MCP server {name} has an invalid URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(format!("MCP server {name} has an unsafe URL"));
    }
    if url.scheme() == "http" {
        let host = url.host_str().unwrap_or_default();
        let loopback = host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback());
        if !loopback {
            return Err(format!("MCP server {name} must use HTTPS outside loopback"));
        }
    }
    Ok(())
}

fn validate_headers(headers: &[(String, String)], name: &str) -> Result<(), String> {
    let mut names = HashSet::new();
    for (header, value) in headers {
        if header.is_empty()
            || !header.chars().all(|ch| {
                ch.is_ascii_alphanumeric()
                    || matches!(
                        ch,
                        '!' | '#'
                            | '$'
                            | '%'
                            | '&'
                            | '\''
                            | '*'
                            | '+'
                            | '-'
                            | '.'
                            | '^'
                            | '_'
                            | '`'
                            | '|'
                            | '~'
                    )
            })
            || value
                .chars()
                .any(|ch| ch == '\r' || ch == '\n' || ch.is_control())
            || value.contains("${")
        {
            return Err(format!(
                "MCP server {name} contains an invalid literal header"
            ));
        }
        if !names.insert(header.to_ascii_lowercase()) {
            return Err(format!("MCP server {name} contains duplicate header names"));
        }
    }
    Ok(())
}

fn discover_scaffolds(root: &Path) -> Result<Vec<PluginScaffold>, PluginError> {
    let mut scaffolds = Vec::new();
    for container_name in ["scaffolds", "templates"] {
        let container = root.join(container_name);
        let entries = match std::fs::read_dir(&container) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let entry = entry?;
            let metadata = std::fs::symlink_metadata(entry.path())?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                continue;
            }
            let fallback = entry.file_name().to_string_lossy().to_string();
            let metadata_path = entry.path().join("scaffold.json");
            let raw = if metadata_path.is_file() {
                ensure_plugin_path(root, &metadata_path)?;
                serde_json::from_str::<RawScaffold>(&std::fs::read_to_string(metadata_path)?)?
            } else {
                RawScaffold::default()
            };
            let id = slug_with_hash(&format!("{container_name}/{fallback}"));
            let files = count_scaffold_files(&entry.path())?;
            if files == 0 {
                continue;
            }
            scaffolds.push(PluginScaffold {
                id,
                name: if raw.name.trim().is_empty() {
                    title_case(&fallback)
                } else {
                    raw.name.trim().to_string()
                },
                description: truncate(raw.description.trim(), 280),
                path: PathBuf::from(container_name).join(&fallback),
                files,
            });
        }
    }
    scaffolds.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(scaffolds)
}

/// Count the plugin's Agent Scenes components: `scenes/*.scene.json` and `scenes/*.pipeline.json`
/// documents, serde-parsed and validated against the 1.0.0 schemas at install/parse time. Invalid
/// or unreadable files are warned about and skipped — never fatal to the install (docs/reference/scenes.md:
/// same posture as `SkillLibrary::load_dir`). The bundle collector already preserves the
/// directory verbatim; hosts read the installed copy back through [`plugin_scenes_dir`].
fn count_scene_components(root: &Path) -> (usize, usize) {
    let dir = root.join("scenes");
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("plugin scenes dir {dir:?}: {error}");
            }
            return (0, 0);
        }
    };
    let mut scenes = 0;
    let mut pipelines = 0;
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.ends_with(".scene.json") {
            let parsed = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|data| {
                    serde_json::from_str::<crate::scene::Scene>(&data)
                        .map_err(|e| e.to_string())
                })
                .and_then(|scene| crate::scene::validate_scene(&scene).map(|()| scene));
            match parsed {
                Ok(_) => scenes += 1,
                Err(error) => tracing::warn!("plugin scene {path:?}: {error} (skipped)"),
            }
        } else if name.ends_with(".pipeline.json") {
            let parsed = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|data| {
                    serde_json::from_str::<crate::scene::Pipeline>(&data)
                        .map_err(|e| e.to_string())
                })
                .and_then(|pipeline| {
                    crate::scene::validate_pipeline(&pipeline).map(|()| pipeline)
                });
            match parsed {
                Ok(_) => pipelines += 1,
                Err(error) => tracing::warn!("plugin pipeline {path:?}: {error} (skipped)"),
            }
        }
    }
    (scenes, pipelines)
}

/// Where an installed plugin's scene components live on disk. [`install`] preserves the verified
/// bundle under `<plugins_dir>/<id>/bundle/`, so the scene loader must read
/// `<plugins_dir>/<id>/bundle/scenes` — this helper keeps that layout knowledge in one place.
pub fn plugin_scenes_dir(plugins_dir: &Path, id: &str) -> PathBuf {
    plugins_dir.join(id).join(BUNDLE_DIR).join("scenes")
}

fn collect_bundle_files(root: &Path) -> Result<Vec<PluginFile>, PluginError> {
    let mut files = Vec::new();
    let mut bytes = 0;
    collect_files(root, root, 0, &mut bytes, &mut files, false)?;
    Ok(files)
}

fn collect_apply_files(root: &Path) -> Result<Vec<PluginFile>, PluginError> {
    let mut files = Vec::new();
    let mut bytes = 0;
    collect_files(root, root, 0, &mut bytes, &mut files, true)?;
    Ok(files)
}

fn collect_files(
    root: &Path,
    dir: &Path,
    depth: usize,
    bytes: &mut u64,
    files: &mut Vec<PluginFile>,
    skip_scaffold_metadata: bool,
) -> Result<(), PluginError> {
    if depth > MAX_DEPTH {
        return Err(PluginError::Invalid(
            "Plugin directory is nested too deeply".into(),
        ));
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if metadata.is_dir() {
            if matches!(name.as_ref(), ".git" | "node_modules" | "target" | ".venv") {
                continue;
            }
            collect_files(root, &path, depth + 1, bytes, files, skip_scaffold_metadata)?;
            continue;
        }
        if !metadata.is_file() || (skip_scaffold_metadata && name == "scaffold.json") {
            continue;
        }
        if metadata.len() > MAX_FILE_BYTES {
            return Err(PluginError::FileTooLarge(
                MAX_FILE_BYTES / 1024 / 1024,
                path.display().to_string(),
            ));
        }
        *bytes += metadata.len();
        if *bytes > MAX_BUNDLE_BYTES {
            return Err(PluginError::BundleTooLarge(MAX_BUNDLE_BYTES / 1024 / 1024));
        }
        if files.len() >= MAX_FILES {
            return Err(PluginError::TooManyFiles(MAX_FILES));
        }
        let relative = safe_relative(path.strip_prefix(root).unwrap_or(&path))?.to_path_buf();
        files.push(PluginFile {
            path: relative,
            bytes: std::fs::read(path)?,
            mode: file_mode(&metadata),
        });
    }
    Ok(())
}

fn collect_extension(
    dir: &Path,
    extension: &str,
    depth: usize,
    out: &mut Vec<PathBuf>,
) -> Result<(), PluginError> {
    collect_matching(dir, depth, out, &|path| {
        path.extension().and_then(|value| value.to_str()) == Some(extension)
    })
}

fn collect_matching(
    dir: &Path,
    depth: usize,
    out: &mut Vec<PathBuf>,
    matches: &dyn Fn(&Path) -> bool,
) -> Result<(), PluginError> {
    if depth > MAX_DEPTH || !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_file() && matches(&path) {
            out.push(path);
            if out.len() > MAX_COMPONENTS {
                return Err(PluginError::TooManyComponents(MAX_COMPONENTS));
            }
        } else if metadata.is_dir() {
            collect_matching(&path, depth + 1, out, matches)?;
        }
    }
    Ok(())
}

fn count_scaffold_files(root: &Path) -> Result<usize, PluginError> {
    Ok(collect_apply_files(root)?.len())
}

fn read_small_text(path: &Path) -> Result<String, PluginError> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > MAX_FILE_BYTES {
        return Err(PluginError::FileTooLarge(
            MAX_FILE_BYTES / 1024 / 1024,
            path.display().to_string(),
        ));
    }
    std::fs::read_to_string(path).map_err(PluginError::Io)
}

fn write_file(target: &Path, file: &PluginFile) -> Result<(), PluginError> {
    std::fs::write(target, &file.bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(target, std::fs::Permissions::from_mode(file.mode))?;
    }
    Ok(())
}

#[cfg(unix)]
fn file_mode(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o777
}

#[cfg(not(unix))]
fn file_mode(_metadata: &std::fs::Metadata) -> u32 {
    0
}

fn plugin_data_dir(plugins_dir: &Path, id: &str) -> PathBuf {
    plugins_dir.join(".data").join(id)
}

fn resolve_relative_mcp_commands(plugin: &mut InstalledPlugin, plugin_dir: &Path, data_dir: &Path) {
    let bundle_root = plugin_dir.join(BUNDLE_DIR);
    for component in &mut plugin.components {
        let SkillPayload::Mcp { server } = &mut component.payload else {
            continue;
        };
        let McpTransport::Stdio { command, args, env } = &mut server.transport else {
            continue;
        };
        if let Some(relative) = command.strip_prefix("./") {
            *command = bundle_root.join(relative).display().to_string();
        }
        for arg in args {
            *arg = expand_plugin_variables(arg, &bundle_root, data_dir);
        }
        for (_, value) in env.iter_mut() {
            *value = expand_plugin_variables(value, &bundle_root, data_dir);
        }
        env.retain(|(name, _)| {
            !["PLUGIN_ROOT", "PLUGIN_DATA"]
                .iter()
                .any(|reserved| name.eq_ignore_ascii_case(reserved))
        });
        env.push(("PLUGIN_ROOT".into(), bundle_root.display().to_string()));
        env.push(("PLUGIN_DATA".into(), data_dir.display().to_string()));
        if let Some(cwd) = &mut server.cwd {
            if let Some(relative) = cwd.strip_prefix("./") {
                *cwd = bundle_root.join(relative).display().to_string();
            } else {
                *cwd = expand_plugin_variables(cwd, &bundle_root, data_dir);
            }
        }
    }
    for server in &mut plugin.lsp_servers {
        if let Some(relative) = server.command.strip_prefix("./") {
            server.command = bundle_root.join(relative).display().to_string();
        }
        for arg in &mut server.args {
            *arg = expand_plugin_variables(arg, &bundle_root, data_dir);
        }
        for value in server.env.values_mut() {
            *value = expand_plugin_variables(value, &bundle_root, data_dir);
        }
        server.env.retain(|name, _| {
            !["PLUGIN_ROOT", "PLUGIN_DATA"]
                .iter()
                .any(|reserved| name.eq_ignore_ascii_case(reserved))
        });
        server
            .env
            .insert("PLUGIN_ROOT".into(), bundle_root.display().to_string());
        server
            .env
            .insert("PLUGIN_DATA".into(), data_dir.display().to_string());
    }
}

fn expand_plugin_variables(value: &str, root: &Path, data: &Path) -> String {
    let root = root.display().to_string();
    let data = data.display().to_string();
    value
        .replace("${PLUGIN_ROOT}", &root)
        .replace("${PLUGIN_DATA}", &data)
}

fn string_array(value: Option<&Value>, label: &str) -> Result<Vec<String>, PluginError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or_else(|| PluginError::Invalid(format!("{label} must be an array")))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| PluginError::Invalid(format!("{label} must contain strings")))
        })
        .collect()
}

fn string_map(value: Option<&Value>, label: &str) -> Result<Vec<(String, String)>, PluginError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let object = value
        .as_object()
        .ok_or_else(|| PluginError::Invalid(format!("{label} must be an object")))?;
    object
        .iter()
        .map(|(key, value)| {
            value
                .as_str()
                .map(|value| (key.clone(), value.to_string()))
                .ok_or_else(|| PluginError::Invalid(format!("{label} values must be strings")))
        })
        .collect()
}

fn author_name(value: &Value) -> String {
    value
        .as_str()
        .or_else(|| value.get("name").and_then(Value::as_str))
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn markdown_body(text: &str) -> &str {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return text;
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        offset += line.len();
        if line.trim_end_matches(['\r', '\n']) == "---" {
            return &rest[offset..];
        }
    }
    text
}

fn frontmatter_scalar(text: &str, key: &str) -> Option<String> {
    let frontmatter = frontmatter(text)?;
    frontmatter.lines().find_map(|line| {
        let (candidate, value) = line.split_once(':')?;
        (candidate.trim() == key)
            .then(|| value.trim().trim_matches(['\'', '"']).to_string())
            .filter(|value| !value.is_empty())
    })
}

fn frontmatter_list(text: &str, key: &str) -> Vec<String> {
    let Some(value) = frontmatter_scalar(text, key) else {
        return Vec::new();
    };
    value
        .trim_matches(['[', ']'])
        .split(',')
        .map(|item| item.trim().trim_matches(['\'', '"']).to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn frontmatter(text: &str) -> Option<&str> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

/// Confirm that a repository path stays inside the plugin root and does not traverse symlinks.
/// Git preserves symlinks, so lexical `..` checks alone are not sufficient for untrusted repos.
fn ensure_plugin_path(root: &Path, path: &Path) -> Result<(), PluginError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| PluginError::UnsafePath(path.display().to_string()))?;
    safe_relative(relative)?;

    let canonical_root = root.canonicalize()?;
    let canonical_path = path.canonicalize()?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(PluginError::UnsafePath(path.display().to_string()));
    }

    let mut current = root.to_path_buf();
    for component in relative.components() {
        if component == Component::CurDir {
            continue;
        }
        current.push(component.as_os_str());
        if std::fs::symlink_metadata(&current)?
            .file_type()
            .is_symlink()
        {
            return Err(PluginError::UnsafePath(path.display().to_string()));
        }
    }
    Ok(())
}

/// Resolve a not-yet-created scaffold target while rejecting existing files and symlinked parents.
fn scaffold_target(destination: &Path, relative: &Path) -> Result<PathBuf, PluginError> {
    let relative = safe_relative(relative)?;
    let target = destination.join(relative);
    let mut current = destination.to_path_buf();
    for component in relative.components() {
        if component == Component::CurDir {
            continue;
        }
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(PluginError::UnsafeScaffoldPath(
                    relative.display().to_string(),
                ));
            }
            Ok(metadata) if current == target || !metadata.is_dir() => {
                return Err(PluginError::ScaffoldConflict(
                    relative.display().to_string(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(target)
}

fn safe_relative(path: &Path) -> Result<&Path, PluginError> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(PluginError::UnsafePath(path.display().to_string()));
    }
    for component in path.components() {
        if !matches!(component, Component::Normal(_) | Component::CurDir) {
            return Err(PluginError::UnsafePath(path.display().to_string()));
        }
    }
    Ok(path)
}

fn require_safe_id(value: &str) -> Result<&str, PluginError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(PluginError::UnsafePath(value.to_string()));
    }
    Ok(value)
}

fn slug_with_hash(value: &str) -> String {
    format!(
        "{}-{:08x}",
        slug(value).chars().take(52).collect::<String>(),
        fnv1a(value.as_bytes())
    )
}

fn slug(value: &str) -> String {
    let mut output = String::new();
    let mut dash = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            output.push(character.to_ascii_lowercase());
            dash = false;
        } else if !dash && !output.is_empty() {
            output.push('-');
            dash = true;
        }
    }
    output.trim_end_matches('-').to_string()
}

fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c9dc5, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x01000193)
    })
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value.to_string()
    } else {
        value.chars().take(max_chars).collect::<String>() + "…"
    }
}

fn title_case(value: &str) -> String {
    value
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_scope_support_defaults_to_user_and_accepts_explicit_project() {
        let default: PluginRuntimeSpec =
            serde_json::from_value(serde_json::json!({ "command": "server" })).unwrap();
        assert_eq!(
            default.scope_support,
            [crate::kernel::PluginScopeSupport::User]
        );

        let project: PluginRuntimeSpec = serde_json::from_value(serde_json::json!({
            "command": "server",
            "scopeSupport": ["user", "project"]
        }))
        .unwrap();
        assert_eq!(
            project.scope_support,
            [
                crate::kernel::PluginScopeSupport::User,
                crate::kernel::PluginScopeSupport::Project,
            ]
        );
        assert_eq!(
            serde_json::to_value(project).unwrap()["scopeSupport"],
            serde_json::json!(["user", "project"])
        );
    }

    #[test]
    fn loads_c2_runtime_from_agent_plugins_namespace() {
        let root = std::env::temp_dir().join(format!(
            "codetwo-namespaced-runtime-{}",
            uuid::Uuid::new_v4()
        ));
        write(
            &root.join("plugin.json"),
            r#"{
              "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
              "name":"c2-runtime","version":"1.0.0",
              "extensions":{"dev.codetwo":{
                "standardVersion":"1.2.0",
                "commands":[{"id":"review.run","title":"Review workspace"}],
                "runtime":{"protocol":"1.0.0","command":"node","args":["plugin.js"],"scopeSupport":["user","project"]},
                "ui":[{"id":"review","slot":"composer.toolbar","label":"Review","description":"Review this project.","command":"review.run","input":{"mode":"project"},"order":10}]
              }}
            }"#,
        );
        write(&root.join("plugin.js"), "process.exit(0);\n");

        let bundle = from_github(&checkout(root)).unwrap();
        let runtime = bundle.plugin.runtime.expect("C2 runtime must be loaded");
        assert_eq!(runtime.command, "node");
        assert_eq!(runtime.args, ["plugin.js"]);
        assert_eq!(
            runtime.scope_support,
            [
                crate::kernel::PluginScopeSupport::User,
                crate::kernel::PluginScopeSupport::Project,
            ]
        );
        assert_eq!(bundle.plugin.counts.runtime, 1);
        assert_eq!(bundle.plugin.standard_version, "1.2.0");
        assert_eq!(bundle.plugin.runtime_commands[0].id, "review.run");
        assert_eq!(bundle.plugin.counts.ui, 1);
        assert_eq!(bundle.plugin.ui_contributions[0].command, "review.run");
        assert_eq!(bundle.plugin.ui_contributions[0].order, 10);
    }

    #[test]
    fn loads_static_runtime_commands_from_c2_standard_1_2() {
        let root =
            std::env::temp_dir().join(format!("codetwo-static-runtime-{}", uuid::Uuid::new_v4()));
        write(
            &root.join("plugin.json"),
            r#"{
              "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
              "name":"static-runtime","version":"1.0.0",
              "extensions":{"dev.codetwo":{
                "standardVersion":"1.2.0",
                "commands":[{"id":"review.run","title":"Review workspace","description":"Review this project.","argsSchema":{"type":"object","additionalProperties":false}}],
                "runtime":{"protocol":"1.0.0","command":"node","args":["plugin.js"]},
                "ui":[{"id":"review","slot":"composer.toolbar","label":"Review","command":"review.run"}]
              }}
            }"#,
        );
        write(&root.join("plugin.js"), "process.exit(0);\n");

        let bundle = from_github(&checkout(root)).unwrap();
        assert_eq!(bundle.plugin.standard_version, "1.2.0");
        assert_eq!(bundle.plugin.counts.runtime_commands, 1);
        let commands = bundle.plugin.runtime_commands;
        assert_eq!(commands[0].id, "review.run");
        assert_eq!(commands[0].title, "Review workspace");
        assert_eq!(
            commands[0].args_schema,
            Some(serde_json::json!({
                "type": "object",
                "additionalProperties": false
            }))
        );
    }

    #[test]
    fn loads_owned_connector_from_c2_standard_1_2() {
        let root = std::env::temp_dir().join(format!(
            "codetwo-collaboration-connector-{}",
            uuid::Uuid::new_v4()
        ));
        write(
            &root.join("plugin.json"),
            r#"{
              "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
              "name":"collaboration-connector","version":"1.0.0",
              "extensions":{"dev.codetwo":{
                "standardVersion":"1.2.0",
                "commands":[{"id":"chat.connector","title":"Invoke chat connector"}],
                "runtime":{"command":"node","args":["plugin.js"]},
                "connectors":[{
                  "id":"workspace","provider":"test-chat",
                  "command":"chat.connector","capabilities":["connection","conversations","messaging"]
                }]
              }}
            }"#,
        );
        write(&root.join("plugin.js"), "process.exit(0);\n");

        let bundle = from_github(&checkout(root)).unwrap();
        assert_eq!(bundle.plugin.standard_version, "1.2.0");
        assert_eq!(bundle.plugin.counts.connectors, 1);
        assert_eq!(bundle.plugin.connector_contributions.len(), 1);
        assert_eq!(
            bundle.plugin.connector_contributions[0].capabilities,
            ["connection", "conversations", "messaging"]
        );
        assert!(bundle
            .plugin
            .extension_components
            .iter()
            .any(|component| component.kind == "connector"));
    }

    #[test]
    fn closes_static_runtime_command_ownership_and_schema() {
        let missing = std::env::temp_dir().join(format!(
            "codetwo-missing-static-commands-{}",
            uuid::Uuid::new_v4()
        ));
        write(
            &missing.join("plugin.json"),
            r#"{
              "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
              "name":"missing-static","version":"1.0.0",
              "extensions":{"dev.codetwo":{"standardVersion":"1.2.0","runtime":{"command":"node"}}}
            }"#,
        );
        assert!(matches!(
            from_github(&checkout(missing)),
            Err(PluginError::Invalid(message)) if message.contains("require at least one")
        ));

        let unknown_ui = std::env::temp_dir().join(format!(
            "codetwo-unknown-ui-command-{}",
            uuid::Uuid::new_v4()
        ));
        write(
            &unknown_ui.join("plugin.json"),
            r#"{
              "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
              "name":"unknown-ui-command","version":"1.0.0",
              "extensions":{"dev.codetwo":{
                "standardVersion":"1.2.0",
                "commands":[{"id":"review.run","title":"Review"}],
                "runtime":{"command":"node"},
                "ui":[{"id":"missing","slot":"composer.toolbar","label":"Missing","command":"review.missing"}]
              }}
            }"#,
        );
        assert!(matches!(
            from_github(&checkout(unknown_ui)),
            Err(PluginError::Invalid(message)) if message.contains("references undeclared runtime command")
        ));

        let unknown_connector = std::env::temp_dir().join(format!(
            "codetwo-unknown-connector-command-{}",
            uuid::Uuid::new_v4()
        ));
        write(
            &unknown_connector.join("plugin.json"),
            r#"{
              "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
              "name":"unknown-connector-command","version":"1.0.0",
              "extensions":{"dev.codetwo":{
                "standardVersion":"1.2.0",
                "commands":[{"id":"chat.run","title":"Chat"}],
                "runtime":{"command":"node"},
                "connectors":[{
                  "id":"workspace","provider":"test-chat",
                  "command":"chat.missing","capabilities":["conversations"]
                }]
              }}
            }"#,
        );
        assert!(matches!(
            from_github(&checkout(unknown_connector)),
            Err(PluginError::Invalid(message)) if message.contains("references undeclared runtime command")
        ));
    }

    #[test]
    fn rejects_unsupported_c2_standard() {
        for version in ["1.0.0", "1.1.0"] {
            let root = std::env::temp_dir().join(format!(
                "codetwo-old-extension-{version}-{}",
                uuid::Uuid::new_v4()
            ));
            write(
                &root.join("plugin.json"),
                &format!(
                    r#"{{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"old-runtime","version":"1.0.0","extensions":{{"dev.codetwo":{{"standardVersion":"{version}"}}}}}}"#
                ),
            );
            assert!(matches!(
                from_github(&checkout(root)),
                Err(PluginError::Invalid(message)) if message.contains("Unsupported C2 plugin standard")
            ));
        }

        let root =
            std::env::temp_dir().join(format!("codetwo-future-extension-{}", uuid::Uuid::new_v4()));
        write(
            &root.join("plugin.json"),
            r#"{
              "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
              "name":"future-runtime",
              "extensions":{"dev.codetwo":{
                "standardVersion":"2.0.0",
                "runtime":{"command":"node"}
              }}
            }"#,
        );
        write(
            &root.join("skills/review/SKILL.md"),
            "---\nname: review\ndescription: Review a change\n---\nReview the current change.",
        );

        assert!(matches!(
            from_github(&checkout(root)),
            Err(PluginError::Invalid(message)) if message.contains("Unsupported C2 plugin standard")
        ));
    }

    #[test]
    fn rejects_non_object_agent_plugin_extension_member() {
        let root = std::env::temp_dir().join(format!(
            "codetwo-invalid-extension-{}",
            uuid::Uuid::new_v4()
        ));
        write(
            &root.join("plugin.json"),
            r#"{
              "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
              "name":"invalid-extension",
              "extensions":{"dev.codetwo":"invalid"}
            }"#,
        );
        assert!(matches!(
            from_github(&checkout(root)),
            Err(PluginError::Invalid(message)) if message.contains("extension dev.codetwo must be an object")
        ));
    }

    #[test]
    fn checked_in_hello_runtime_conforms_to_the_c2_namespace() {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/hello-runtime");
        let root =
            std::env::temp_dir().join(format!("codetwo-hello-runtime-{}", uuid::Uuid::new_v4()));
        let plugin_root = root.join("packs/hello-runtime");
        std::fs::create_dir_all(&plugin_root).unwrap();
        std::fs::copy(source.join("plugin.json"), plugin_root.join("plugin.json")).unwrap();
        std::fs::copy(source.join("plugin.js"), plugin_root.join("plugin.js")).unwrap();

        // GitHubCheckout owns and removes its root on drop, so it must only receive this temp copy.
        let bundle = from_github(&GitHubCheckout {
            root,
            spec: crate::github_skills::GitHubRepoSpec {
                owner: "IchenDEV".into(),
                repo: "codeTwo".into(),
                reference: None,
                subpath: Some(PathBuf::from("packs/hello-runtime")),
            },
        })
        .unwrap();

        assert_eq!(bundle.plugin.standard_version, "1.2.0");
        assert_eq!(bundle.plugin.counts.runtime, 1);
        assert_eq!(bundle.plugin.counts.runtime_commands, 1);
        assert_eq!(bundle.plugin.runtime_commands[0].id, "hello.dirty");
        assert_eq!(bundle.plugin.runtime.unwrap().command, "node");
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn write_manifest(root: &Path, name: &str, version: &str, c2_fields: &str) {
        let text = [
            r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":""#,
            name,
            r#"","version":""#,
            version,
            r#"","extensions":{"dev.codetwo":{"standardVersion":"1.2.0""#,
            c2_fields,
            "}}}",
        ]
        .concat();
        write(&root.join("plugin.json"), &text);
    }

    fn checkout(root: PathBuf) -> GitHubCheckout {
        GitHubCheckout {
            root,
            spec: crate::github_skills::GitHubRepoSpec {
                owner: "acme".into(),
                repo: "developer-kit".into(),
                reference: None,
                subpath: None,
            },
        }
    }

    #[test]
    fn loads_canonical_agent_plugin_components_and_isolates_invalid_skills() {
        let root =
            std::env::temp_dir().join(format!("codetwo-agent-plugin-{}", uuid::Uuid::new_v4()));
        write(
            &root.join("plugin.json"),
            r#"{
              "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
              "name":"c2-tools","version":"1.0.0","description":"C2 tools",
              "extensions":{"dev.codetwo":{"standardVersion":"1.2.0"}}
            }"#,
        );
        write(
            &root.join("skills/review/SKILL.md"),
            "---\nname: review\ndescription: Review a change\n---\nReview the current change.",
        );
        write(
            &root.join("skills/broken/SKILL.md"),
            "---\nname: Broken Skill\n---\nThis must be skipped.",
        );
        write(
            &root.join("skills/nested/deeper/SKILL.md"),
            "---\nname: hidden\ndescription: Nested too deeply\n---\nDo not discover this.",
        );
        write(
            &root.join("agents/reviewer.md"),
            "---\nname: reviewer\ndescription: Review specialist\n---\nReview the requested change.",
        );
        write(&root.join("bin/server"), "#!/bin/sh\nexit 0\n");
        write(
            &root.join("mcp.json"),
            r#"{
              "$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
              "mcpServers":{
                "local":{"type":"stdio","command":"./bin/server","args":["${PLUGIN_ROOT}/config.json"],"env":{"DATA":"${PLUGIN_DATA}/cache"},"cwd":"${PLUGIN_DATA}"}
              }
            }"#,
        );

        let bundle = from_github(&checkout(root)).unwrap();
        assert_eq!(bundle.plugin.standard_version, "1.2.0");
        assert_eq!(bundle.plugin.counts.skills, 1);
        assert_eq!(bundle.plugin.counts.subagents, 1);
        assert_eq!(bundle.plugin.counts.mcp_servers, 1);
        assert!(bundle
            .plugin
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "invalid_agent_skill"));
    }

    #[test]
    fn rejects_a_malformed_declared_mcp_server() {
        let root =
            std::env::temp_dir().join(format!("codetwo-invalid-mcp-{}", uuid::Uuid::new_v4()));
        write_manifest(&root, "invalid-mcp", "1.0.0", "");
        write(
            &root.join("mcp.json"),
            r#"{
              "$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
              "mcpServers":{"broken":{"type":"stdio","command":"tool","shell":true}}
            }"#,
        );
        assert!(matches!(
            from_github(&checkout(root)),
            Err(PluginError::Invalid(message)) if message.contains("unknown field shell")
        ));
    }

    #[test]
    fn rejects_unsupported_agent_plugins_schema() {
        let root = std::env::temp_dir().join(format!(
            "codetwo-agent-plugin-version-{}",
            uuid::Uuid::new_v4()
        ));
        write(
            &root.join("plugin.json"),
            r#"{"$schema":"https://agent-plugins.org/schemas/2.0.0/plugin.schema.json","name":"future"}"#,
        );
        write(
            &root.join("skills/example/SKILL.md"),
            "---\nname: example\ndescription: Example\n---\nExample.",
        );
        assert!(matches!(
            from_github(&checkout(root)),
            Err(PluginError::Invalid(message)) if message.contains("Unsupported Agent Plugins schema")
        ));
    }

    #[test]
    fn bundled_agent_plugin_schemas_are_the_selected_1_0_0_documents() {
        assert_eq!(
            AGENT_PLUGIN_SCHEMA.get("$id").and_then(Value::as_str),
            Some("https://agent-plugins.org/schemas/1.0.0/plugin.schema.json")
        );
        assert_eq!(
            AGENT_MCP_SCHEMA.get("$id").and_then(Value::as_str),
            Some("https://agent-plugins.org/schemas/1.0.0/mcp.schema.json")
        );
    }

    #[test]
    fn parses_and_resolves_c2_language_servers() {
        let root =
            std::env::temp_dir().join(format!("codetwo-c2-lsp-plugin-{}", uuid::Uuid::new_v4()));
        write(
            &root.join("plugin.json"),
            r#"{
              "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
              "name":"c2-lsp","version":"1.0.0",
              "extensions":{"dev.codetwo":{"standardVersion":"1.2.0","languageServers":[
                {"id":"inline","languages":["rust"],"command":"./bin/lsp","args":["--stdio","${PLUGIN_DATA}/cache"],"env":{"ROOT":"${PLUGIN_ROOT}"}}
              ]}}
            }"#,
        );
        write(
            &root.join("skills/example/SKILL.md"),
            "---\nname: example\ndescription: Example\n---\nExample.",
        );
        write(&root.join("bin/lsp"), "#!/bin/sh\nexit 0\n");
        let data =
            std::env::temp_dir().join(format!("codetwo-c2-lsp-store-{}", uuid::Uuid::new_v4()));
        let installed = install(&data, from_github(&checkout(root)).unwrap()).unwrap();
        assert_eq!(installed.counts.lsp_servers, 1);

        let loaded = load_dir(&data).unwrap();
        let inline = loaded[0]
            .lsp_servers
            .iter()
            .find(|server| server.id == "inline")
            .unwrap();
        assert!(Path::new(&inline.command).is_absolute());
        assert!(inline.args[1].ends_with("/cache"));
        assert!(inline
            .env
            .iter()
            .any(|(name, value)| name == "PLUGIN_ROOT" && Path::new(value).is_absolute()));
        assert!(loaded[0].extension_components.iter().any(|component| {
            component.name == "inline" && component.status == "requires_trust"
        }));
        let _ = std::fs::remove_dir_all(data);
    }

    #[test]
    fn installed_agent_mcp_receives_persistent_plugin_environment() {
        let root =
            std::env::temp_dir().join(format!("codetwo-agent-plugin-env-{}", uuid::Uuid::new_v4()));
        write(
            &root.join("plugin.json"),
            r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"c2-env","version":"1.0.0","extensions":{"dev.codetwo":{"standardVersion":"1.2.0"}}}"#,
        );
        write(
            &root.join("skills/example/SKILL.md"),
            "---\nname: example\ndescription: Example\n---\nExample.",
        );
        write(&root.join("bin/server"), "#!/bin/sh\nexit 0\n");
        write(
            &root.join("mcp.json"),
            r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{"local":{"type":"stdio","command":"./bin/server","args":["${PLUGIN_ROOT}/config"],"env":{"CACHE":"${PLUGIN_DATA}/cache"},"cwd":"${PLUGIN_DATA}"}}}"#,
        );
        let data = std::env::temp_dir().join(format!(
            "codetwo-agent-plugin-store-{}",
            uuid::Uuid::new_v4()
        ));
        let installed = install(&data, from_github(&checkout(root)).unwrap()).unwrap();
        let loaded = load_dir(&data).unwrap();
        let plugin = &loaded[0];
        let server = plugin
            .components
            .iter()
            .find_map(|component| match &component.payload {
                SkillPayload::Mcp { server } => Some(server),
                _ => None,
            })
            .unwrap();
        let McpTransport::Stdio { command, args, env } = &server.transport else {
            panic!("expected stdio");
        };
        assert!(Path::new(command).is_absolute());
        assert!(Path::new(&args[0].trim_end_matches("/config")).is_absolute());
        assert!(server
            .cwd
            .as_deref()
            .is_some_and(|cwd| Path::new(cwd).is_absolute()));
        assert!(env
            .iter()
            .any(|(name, value)| name == "PLUGIN_ROOT" && Path::new(value).is_absolute()));
        assert!(env
            .iter()
            .any(|(name, value)| name == "PLUGIN_DATA" && Path::new(value).is_absolute()));
        assert!(plugin_data_dir(&data, &installed.id).is_dir());
        uninstall_with_options(&data, &installed.id, true).unwrap();
        assert!(plugin_data_dir(&data, &installed.id).is_dir());
        let _ = std::fs::remove_dir_all(data);
    }

    #[test]
    fn parses_complete_manifest_and_all_component_types() {
        let root = std::env::temp_dir().join(format!("codetwo-plugin-{}", uuid::Uuid::new_v4()));
        write(
            &root.join("plugin.json"),
            r#"{
              "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
              "name":"developer-kit","version":"1.2.0","description":"Developer workflow",
              "author":{"name":"Acme"},
              "extensions":{"dev.codetwo":{"standardVersion":"1.2.0"}}
            }"#,
        );
        write(
            &root.join("skills/review/SKILL.md"),
            "---\nname: review\ndescription: Review changes\n---\nCheck the diff carefully.",
        );
        write(
            &root.join("agents/researcher.md"),
            "---\nname: Researcher\ndescription: Find evidence\nmodel: fast\ntools: [web, files]\n---\nResearch before answering.",
        );
        write(
            &root.join("mcp.json"),
            r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{"local":{"type":"stdio","command":"./bin/server","args":["--stdio"],"env":{"MODE":"safe"}},"remote":{"type":"streamable-http","url":"https://mcp.example.test","headers":{"X-Key":"value"}},"events":{"type":"sse","url":"https://mcp.example.test/sse"}}}"#,
        );
        write(&root.join("bin/server"), "#!/bin/sh\nexit 0\n");
        write(
            &root.join("scaffolds/react/scaffold.json"),
            r#"{"name":"React App","description":"Minimal app"}"#,
        );
        write(
            &root.join("scaffolds/react/src/main.tsx"),
            "export default 1;",
        );

        let bundle = from_github(&checkout(root)).unwrap();
        assert_eq!(bundle.plugin.name, "developer-kit");
        assert_eq!(bundle.plugin.version, "1.2.0");
        assert_eq!(bundle.plugin.counts.skills, 1);
        assert_eq!(bundle.plugin.counts.subagents, 1);
        assert_eq!(bundle.plugin.counts.mcp_servers, 3);
        assert_eq!(bundle.plugin.counts.scaffolds, 1);
        assert!(bundle
            .plugin
            .components
            .iter()
            .any(|component| component.kind() == SkillKind::Subagent));
        assert!(bundle.plugin.components.iter().any(|component| matches!(
            &component.payload,
            SkillPayload::Mcp {
                server: McpServer {
                    transport: McpTransport::Sse { .. },
                    ..
                }
            }
        )));
    }

    #[test]
    fn installs_loads_uninstalls_and_applies_scaffold_without_overwrite() {
        let root =
            std::env::temp_dir().join(format!("codetwo-plugin-src-{}", uuid::Uuid::new_v4()));
        write_manifest(&root, "helper", "1.0.0", "");
        write(
            &root.join("skills/helper/SKILL.md"),
            "---\nname: helper\ndescription: Help carefully\n---\nHelp carefully.",
        );
        write(&root.join("scaffolds/basic/README.md"), "hello");
        write(&root.join("bin/tool"), "#!/bin/sh\nexit 0\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                root.join("bin/tool"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        let bundle = from_github(&checkout(root)).unwrap();
        let plugin_id = bundle.plugin.id.clone();
        let scaffold_id = bundle.plugin.scaffolds[0].id.clone();
        let data =
            std::env::temp_dir().join(format!("codetwo-plugin-data-{}", uuid::Uuid::new_v4()));
        let project =
            std::env::temp_dir().join(format!("codetwo-plugin-project-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&project).unwrap();

        install(&data, bundle).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(data.join(&plugin_id).join("bundle/bin/tool"))
                .unwrap()
                .permissions()
                .mode();
            assert_ne!(mode & 0o111, 0);
        }
        let plugins = load_dir(&data).unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].counts.skills, 1);
        let result = apply_scaffold(&data, &plugin_id, &scaffold_id, &project).unwrap();
        assert_eq!(result.files, 1);
        assert_eq!(
            std::fs::read_to_string(project.join("README.md")).unwrap(),
            "hello"
        );
        assert!(matches!(
            apply_scaffold(&data, &plugin_id, &scaffold_id, &project),
            Err(PluginError::ScaffoldConflict(_))
        ));
        uninstall(&data, &plugin_id).unwrap();
        assert!(load_dir(&data).unwrap().is_empty());

        let _ = std::fs::remove_dir_all(data);
        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn local_install_keeps_source_and_update_preserves_user_state() {
        let source = std::env::temp_dir().join(format!(
            "codetwo-local-plugin-source-{}",
            uuid::Uuid::new_v4()
        ));
        write_manifest(&source, "local-state", "1.0.0", "");
        write(
            &source.join("skills/example/SKILL.md"),
            "---\nname: example\ndescription: Example\n---\nExample.",
        );
        let data = std::env::temp_dir().join(format!(
            "codetwo-local-plugin-store-{}",
            uuid::Uuid::new_v4()
        ));

        let first = install(
            &data,
            from_local(&source, "Local marketplace", "catalog:local-state").unwrap(),
        )
        .unwrap();
        set_enabled(&data, &first.id, false).unwrap();
        set_trusted(&data, &first.id, true).unwrap();
        write(
            &source.join("plugin.json"),
            r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"local-state","version":"2.0.0","extensions":{"dev.codetwo":{"standardVersion":"1.2.0"}}}"#,
        );

        let updated = install(
            &data,
            from_local(&source, "Local marketplace", "catalog:local-state").unwrap(),
        )
        .unwrap();
        assert_eq!(updated.version, "2.0.0");
        assert_eq!(updated.standard_version, "1.2.0");
        assert!(updated.runtime_commands.is_empty());
        assert!(!updated.enabled);
        assert!(updated.trusted);
        assert!(source.join("skills/example/SKILL.md").is_file());
        assert_eq!(updated.source, "Local marketplace");
        assert_eq!(
            updated.repository,
            source.canonicalize().unwrap().display().to_string()
        );

        let _ = std::fs::remove_dir_all(data);
        let _ = std::fs::remove_dir_all(source);
    }

    #[test]
    fn requires_the_bundle_root_to_be_selected_explicitly() {
        let root =
            std::env::temp_dir().join(format!("codetwo-plugin-bad-{}", uuid::Uuid::new_v4()));
        write(
            &root.join("packages/tool/plugin.json"),
            r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"nested","version":"1.0.0","extensions":{"dev.codetwo":{"standardVersion":"1.2.0"}}}"#,
        );
        assert!(matches!(
            from_github(&checkout(root)),
            Err(PluginError::Invalid(message)) if message.contains("select the bundle directory explicitly")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_component_paths_outside_plugin() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("codetwo-plugin-link-{}", uuid::Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("codetwo-plugin-outside-{}", uuid::Uuid::new_v4()));
        write(&outside.join("tool"), "#!/bin/sh\nexit 0\n");
        write_manifest(
            &root,
            "bad-link",
            "1.0.0",
            r#","commands":[{"id":"link.run","title":"Run"}],"runtime":{"protocol":"1.0.0","command":"linked/tool"}"#,
        );
        symlink(&outside, root.join("linked")).unwrap();

        assert!(matches!(
            from_github(&checkout(root)),
            Err(PluginError::Invalid(message)) if message.contains("Plugin path is unsafe")
        ));
        let _ = std::fs::remove_dir_all(outside);
    }

    #[cfg(unix)]
    #[test]
    fn scaffold_rejects_symlinked_destination_parent() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("codetwo-plugin-src-{}", uuid::Uuid::new_v4()));
        write_manifest(&root, "scaffold-safety", "1.0.0", "");
        write(
            &root.join("skills/helper/SKILL.md"),
            "---\nname: helper\ndescription: Help carefully\n---\nHelp carefully.",
        );
        write(&root.join("scaffolds/basic/nested/file.txt"), "safe");
        let bundle = from_github(&checkout(root)).unwrap();
        let plugin_id = bundle.plugin.id.clone();
        let scaffold_id = bundle.plugin.scaffolds[0].id.clone();
        let data =
            std::env::temp_dir().join(format!("codetwo-plugin-data-{}", uuid::Uuid::new_v4()));
        let project =
            std::env::temp_dir().join(format!("codetwo-plugin-project-{}", uuid::Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("codetwo-plugin-target-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        symlink(&outside, project.join("nested")).unwrap();
        install(&data, bundle).unwrap();

        assert!(matches!(
            apply_scaffold(&data, &plugin_id, &scaffold_id, &project),
            Err(PluginError::UnsafeScaffoldPath(_))
        ));
        assert!(!outside.join("file.txt").exists());

        let _ = std::fs::remove_dir_all(data);
        let _ = std::fs::remove_dir_all(project);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn counts_scene_components_and_skips_invalid_files() {
        let root =
            std::env::temp_dir().join(format!("codetwo-plugin-scenes-{}", uuid::Uuid::new_v4()));
        write(
            &root.join("plugin.json"),
            r#"{
              "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
              "name":"scene-pack","version":"1.0.0","description":"Scenes only",
              "extensions":{"dev.codetwo":{"standardVersion":"1.2.0"}}
            }"#,
        );
        write(
            &root.join("scenes/valid.scene.json"),
            &format!(
                r#"{{"$schema":"{}","name":"valid","title":"Valid scene"}}"#,
                crate::scene::SCENE_SCHEMA_ID
            ),
        );
        // Malformed JSON: must be warned about and skipped, never failing the install.
        write(&root.join("scenes/broken.scene.json"), "{not json");
        write(
            &root.join("scenes/flow.pipeline.json"),
            &format!(
                r#"{{"$schema":"{}","name":"flow","title":"Flow","stages":[{{"id":"a","scene":"valid"}}]}}"#,
                crate::scene::PIPELINE_SCHEMA_ID
            ),
        );

        let bundle = from_github(&checkout(root.clone())).unwrap();
        assert_eq!(
            bundle.plugin.counts.scenes, 1,
            "invalid scene must be skipped"
        );
        assert_eq!(bundle.plugin.counts.pipelines, 1);
        // A scenes-only pack has no other components: scenes must keep it installable.
        assert_eq!(bundle.plugin.counts.total(), 2);

        // The install layout keeps the bundle verbatim; the scene loader path must point at it.
        let data =
            std::env::temp_dir().join(format!("codetwo-plugin-data-{}", uuid::Uuid::new_v4()));
        let plugin_id = bundle.plugin.id.clone();
        install(&data, bundle).unwrap();
        let scenes_dir = plugin_scenes_dir(&data, &plugin_id);
        assert!(scenes_dir.join("valid.scene.json").is_file());

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(data);
    }
}

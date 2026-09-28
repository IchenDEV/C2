//! Agent Scenes 1.0.0 — the scenario layer above skills.
//!
//! A scene is a pure-data bundle configuring a session for one stage of work: execution posture,
//! pinned skills, a task-brief skeleton, expected artifacts, exit criteria, and declarative hooks.
//! A pipeline chains scenes into a lifecycle. See `docs/reference/scenes.md` for the normative spec; the
//! serde types below mirror the frozen JSON Schemas at `schemas/agent-scenes/1.0.0/` exactly.
//!
//! Two invariants live here and nowhere else:
//! - [`apply_execution`] is the single escalation chokepoint: a scene may tighten permissions
//!   silently but can never loosen them without explicit user confirmation.
//! - Loading is never fatal: malformed or invalid files are warned about and skipped, and an
//!   unresolved reference degrades to `None` for callers to surface as a warning.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::permission::{ExecutionPolicy, PermissionMode, SandboxPolicy};
use crate::session::MemoryAccess;
use crate::skill::SlotDef;
use crate::worktree::WorktreeBaseline;

pub const SCENE_SCHEMA_ID: &str = "https://agent-scenes.org/schemas/1.0.0/scene.schema.json";
pub const PIPELINE_SCHEMA_ID: &str = "https://agent-scenes.org/schemas/1.0.0/pipeline.schema.json";

// ---------------------------------------------------------------------------
// Scene document model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    #[serde(rename = "$schema")]
    pub schema: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<SceneAuthor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub localizations: HashMap<String, SceneLocalization>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<SceneExecution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<SceneSkills>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief: Option<SceneBrief>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<SceneArtifactSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<SceneExit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hooks: Vec<SceneHook>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraints: Option<SceneConstraints>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extensions: HashMap<String, serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneAuthor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneLocalization {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SceneExecution {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_mode: Option<SceneSessionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_preset: Option<SceneMemoryPreset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<SceneWorktree>,
    /// Decode old scene files without enabling or re-exporting the retired feature.
    #[serde(default, rename = "plan_first", skip_serializing)]
    pub _legacy_plan_first: Option<bool>,
}

/// Declaration order IS the loosening order; `Ord` on it drives the escalation rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneSessionMode {
    ReadOnly,
    Ask,
    AutoEdit,
    FullAccess,
}

impl SceneSessionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Ask => "ask",
            Self::AutoEdit => "auto_edit",
            Self::FullAccess => "full_access",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneMemoryPreset {
    Standard,
    ReadOnly,
    Private,
    LearnOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneWorktree {
    Off,
    Current,
    OriginDefault,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SceneSkills {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pinned: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inline: Vec<SceneInlineFragment>,
    #[serde(default)]
    pub suppress_unpinned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneInlineFragment {
    pub name: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneBrief {
    pub template: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<SlotDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clarify: Option<BriefClarify>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BriefClarify {
    MultiChoice,
    FreeForm,
    Off,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneArtifactSpec {
    pub id: String,
    pub title: String,
    pub kind: SceneArtifactKind,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneArtifactKind {
    Document,
    Plan,
    Report,
    TestReport,
    Checklist,
    Diff,
    Link,
    Custom,
}

impl SceneArtifactKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Plan => "plan",
            Self::Report => "report",
            Self::TestReport => "test_report",
            Self::Checklist => "checklist",
            Self::Diff => "diff",
            Self::Link => "link",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SceneExit {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub criteria: Vec<ExitCriterion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next: Vec<NextSuggestion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExitCriterion {
    pub kind: ExitCriterionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitCriterionKind {
    RequiredArtifacts,
    ChecklistComplete,
    TestsPass,
    UserConfirm,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NextSuggestion {
    pub scene: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub carry: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneHook {
    pub on: HookEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    pub action: HookAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    Enter,
    TurnEnd,
    ArtifactProduced,
    ExitCriteriaMet,
    TestsFailed,
    Schedule,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookAction {
    pub kind: HookActionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<String>,
    #[serde(default, rename = "macro", skip_serializing_if = "Option::is_none")]
    pub macro_ref: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub args: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookActionKind {
    SuggestScene,
    SuggestNext,
    RunMacro,
    Notify,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SceneConstraints {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guardrails: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolHints>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ToolHints {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<String>,
}

impl Scene {
    /// Spec default when `exit.criteria` is absent or empty: required artifacts, then the user.
    pub fn effective_criteria(&self) -> Vec<ExitCriterion> {
        match &self.exit {
            Some(exit) if !exit.criteria.is_empty() => exit.criteria.clone(),
            _ => vec![
                ExitCriterion {
                    kind: ExitCriterionKind::RequiredArtifacts,
                    artifact: None,
                    description: None,
                },
                ExitCriterion {
                    kind: ExitCriterionKind::UserConfirm,
                    artifact: None,
                    description: None,
                },
            ],
        }
    }

    pub fn localized_title(&self, locale: &str) -> &str {
        self.localizations
            .get(locale)
            .and_then(|l| l.title.as_deref())
            .unwrap_or(&self.title)
    }
}

// ---------------------------------------------------------------------------
// Pipeline document model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pipeline {
    #[serde(rename = "$schema")]
    pub schema: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<SceneAuthor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub localizations: HashMap<String, SceneLocalization>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    pub stages: Vec<PipelineStage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transitions: Vec<PipelineTransition>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extensions: HashMap<String, serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineStage {
    pub id: String,
    pub scene: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<Gate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub carry: Vec<CarrySpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    Suggest,
    Confirm,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarrySpec {
    pub from: String,
    pub artifact: String,
    #[serde(default, rename = "as", skip_serializing_if = "Option::is_none")]
    pub as_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineTransition {
    pub from: String,
    pub to: String,
    pub when: TransitionTrigger,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<Gate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionTrigger {
    ExitCriteriaMet,
    TestsFailed,
    UserRequest,
}

// ---------------------------------------------------------------------------
// Validation — the parts JSON Schema conditionals express that serde cannot
// ---------------------------------------------------------------------------

/// Slug per the schema: 1–64 of `[a-z0-9.-]`, alphanumeric at both ends, no `--` or `..`.
pub fn is_slug(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return false;
    }
    let alnum = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    if !alnum(bytes[0]) || !alnum(bytes[bytes.len() - 1]) {
        return false;
    }
    if !bytes.iter().all(|&b| alnum(b) || b == b'.' || b == b'-') {
        return false;
    }
    !(s.contains("--") || s.contains(".."))
}

/// Artifact id per the schema: 1–64 of `[a-z0-9-]`, alphanumeric at both ends.
/// Unlike slugs, internal `--` is allowed (the schema pattern has no lookahead here).
pub fn is_artifact_id(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return false;
    }
    let alnum = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    if !alnum(bytes[0]) || !alnum(bytes[bytes.len() - 1]) {
        return false;
    }
    bytes.iter().all(|&b| alnum(b) || b == b'-')
}

pub fn validate_scene(scene: &Scene) -> Result<(), String> {
    if scene.schema != SCENE_SCHEMA_ID {
        return Err(format!("unsupported $schema `{}`", scene.schema));
    }
    if !is_slug(&scene.name) {
        return Err(format!("invalid scene name `{}`", scene.name));
    }
    if scene.title.trim().is_empty() {
        return Err("title must be non-empty".into());
    }
    if let Some(brief) = &scene.brief {
        if brief.template.is_empty() {
            return Err("brief.template must be non-empty".into());
        }
        for slot in &brief.slots {
            if !is_slug(&slot.id) {
                return Err(format!("invalid brief slot id `{}`", slot.id));
            }
            if slot.label.trim().is_empty() {
                return Err(format!("brief slot `{}` requires a label", slot.id));
            }
            if slot.kind == crate::skill::SlotKind::Select && slot.options.is_empty() {
                return Err(format!("select slot `{}` requires options", slot.id));
            }
        }
    }
    for artifact in &scene.artifacts {
        if !is_artifact_id(&artifact.id) {
            return Err(format!("invalid artifact id `{}`", artifact.id));
        }
        if artifact.title.trim().is_empty() {
            return Err(format!("artifact `{}` requires a title", artifact.id));
        }
    }
    if let Some(exit) = &scene.exit {
        for criterion in &exit.criteria {
            match criterion.kind {
                ExitCriterionKind::ChecklistComplete if criterion.artifact.is_none() => {
                    return Err("checklist_complete criterion requires `artifact`".into());
                }
                ExitCriterionKind::Custom if criterion.description.is_none() => {
                    return Err("custom criterion requires `description`".into());
                }
                _ => {}
            }
        }
        for next in &exit.next {
            if next.scene.is_empty() {
                return Err("exit.next entry requires `scene`".into());
            }
        }
    }
    for hook in &scene.hooks {
        match hook.action.kind {
            HookActionKind::SuggestScene if hook.action.scene.is_none() => {
                return Err("suggest_scene action requires `scene`".into());
            }
            HookActionKind::RunMacro if hook.action.macro_ref.is_none() => {
                return Err("run_macro action requires `macro`".into());
            }
            HookActionKind::Notify if hook.action.message.is_none() => {
                return Err("notify action requires `message`".into());
            }
            _ => {}
        }
        if hook.on == HookEvent::Schedule && hook.schedule.is_none() {
            return Err("schedule hook requires `schedule`".into());
        }
        if hook.on != HookEvent::Schedule && hook.schedule.is_some() {
            return Err("`schedule` is only meaningful on schedule hooks".into());
        }
    }
    Ok(())
}

pub fn validate_pipeline(pipeline: &Pipeline) -> Result<(), String> {
    if pipeline.schema != PIPELINE_SCHEMA_ID {
        return Err(format!("unsupported $schema `{}`", pipeline.schema));
    }
    if !is_slug(&pipeline.name) {
        return Err(format!("invalid pipeline name `{}`", pipeline.name));
    }
    if pipeline.title.trim().is_empty() {
        return Err("title must be non-empty".into());
    }
    if pipeline.stages.is_empty() {
        return Err("pipeline requires at least one stage".into());
    }
    let mut ids = std::collections::HashSet::new();
    for stage in &pipeline.stages {
        if !is_slug(&stage.id) {
            return Err(format!("invalid stage id `{}`", stage.id));
        }
        if !ids.insert(stage.id.as_str()) {
            return Err(format!("duplicate stage id `{}`", stage.id));
        }
    }
    let declared = |id: &str| pipeline.stages.iter().any(|s| s.id == id);
    if let Some(entry) = &pipeline.entry {
        if !declared(entry) {
            return Err(format!("entry `{entry}` names no declared stage"));
        }
    }
    for stage in &pipeline.stages {
        for carry in &stage.carry {
            if !declared(&carry.from) {
                return Err(format!(
                    "stage `{}` carry from `{}` names no declared stage",
                    stage.id, carry.from
                ));
            }
            if !is_artifact_id(&carry.artifact) {
                return Err(format!("invalid carry artifact id `{}`", carry.artifact));
            }
        }
    }
    for transition in &pipeline.transitions {
        if !declared(&transition.from) || !declared(&transition.to) {
            return Err(format!(
                "transition `{}` -> `{}` names an undeclared stage",
                transition.from, transition.to
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Library — four-source precedence
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneSource {
    Project,
    User,
    Plugin { plugin_id: String },
    Builtin,
}

impl SceneSource {
    pub fn source_label(&self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::User => "user",
            Self::Plugin { .. } => "plugin",
            Self::Builtin => "builtin",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedScene {
    pub scene: Scene,
    pub source: SceneSource,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ResolvedPipeline {
    pub pipeline: Pipeline,
    pub source: SceneSource,
    pub path: Option<PathBuf>,
}

/// Scenes and pipelines in precedence order: project > user > plugin > builtin.
#[derive(Debug, Clone, Default)]
pub struct SceneLibrary {
    scenes: Vec<ResolvedScene>,
    pipelines: Vec<ResolvedPipeline>,
}

static BUILTIN: LazyLock<SceneLibrary> = LazyLock::new(|| {
    let mut lib = SceneLibrary::default();
    let fixtures: [(&str, &str); 5] = [
        (
            "research",
            include_str!("../schemas/agent-scenes/1.0.0/examples/research.scene.json"),
        ),
        (
            "develop",
            include_str!("../schemas/agent-scenes/1.0.0/examples/develop.scene.json"),
        ),
        (
            "test",
            include_str!("../schemas/agent-scenes/1.0.0/examples/test.scene.json"),
        ),
        (
            "fix",
            include_str!("../schemas/agent-scenes/1.0.0/examples/fix.scene.json"),
        ),
        (
            "acceptance",
            include_str!("../schemas/agent-scenes/1.0.0/examples/acceptance.scene.json"),
        ),
    ];
    for (name, json) in fixtures {
        match serde_json::from_str::<Scene>(json).map_err(|e| e.to_string()) {
            Ok(scene) => match validate_scene(&scene) {
                Ok(()) => lib.scenes.push(ResolvedScene {
                    scene,
                    source: SceneSource::Builtin,
                    path: None,
                }),
                Err(e) => tracing::warn!("builtin scene {name}: {e}"),
            },
            Err(e) => tracing::warn!("builtin scene {name}: {e}"),
        }
    }
    let pipeline_json =
        include_str!("../schemas/agent-scenes/1.0.0/examples/rnd-lifecycle.pipeline.json");
    match serde_json::from_str::<Pipeline>(pipeline_json).map_err(|e| e.to_string()) {
        Ok(pipeline) => match validate_pipeline(&pipeline) {
            Ok(()) => lib.pipelines.push(ResolvedPipeline {
                pipeline,
                source: SceneSource::Builtin,
                path: None,
            }),
            Err(e) => tracing::warn!("builtin pipeline rnd-lifecycle: {e}"),
        },
        Err(e) => tracing::warn!("builtin pipeline rnd-lifecycle: {e}"),
    }
    lib
});

impl SceneLibrary {
    /// The compiled-in scenes and pipeline (the six conformance fixtures).
    pub fn builtin() -> SceneLibrary {
        BUILTIN.clone()
    }

    /// Load all sources in precedence order. Missing directories yield nothing; malformed or
    /// schema-invalid files are warned about and skipped — loading is never fatal.
    pub fn load(
        project_dir: Option<&Path>,
        user_dir: Option<&Path>,
        plugins: &[(String, PathBuf)],
    ) -> SceneLibrary {
        let mut lib = SceneLibrary::default();
        if let Some(dir) = project_dir {
            Self::load_dir(dir, SceneSource::Project, &mut lib);
        }
        if let Some(dir) = user_dir {
            Self::load_dir(dir, SceneSource::User, &mut lib);
        }
        for (plugin_id, dir) in plugins {
            Self::load_dir(
                dir,
                SceneSource::Plugin {
                    plugin_id: plugin_id.clone(),
                },
                &mut lib,
            );
        }
        let builtin = Self::builtin();
        lib.scenes.extend(builtin.scenes);
        lib.pipelines.extend(builtin.pipelines);
        lib
    }

    /// Validate and persist one editable scene under `dir/<name>.scene.json`.
    ///
    /// The filename is derived only from the schema-validated slug, so callers cannot escape the
    /// selected user/project scene directory. A changed name removes the previous file only after
    /// the replacement has been written successfully.
    pub fn save_to_dir(
        dir: &Path,
        scene: &Scene,
        previous_name: Option<&str>,
    ) -> Result<PathBuf, String> {
        validate_scene(scene)?;
        if let Some(previous) = previous_name {
            if !is_slug(previous) {
                return Err(format!("invalid previous scene name `{previous}`"));
            }
        }

        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let target = dir.join(format!("{}.scene.json", scene.name));
        let temp = dir.join(format!(".{}.scene.json.tmp", scene.name));
        let json = serde_json::to_string_pretty(scene).map_err(|e| e.to_string())?;
        std::fs::write(&temp, format!("{json}\n")).map_err(|e| e.to_string())?;
        std::fs::rename(&temp, &target).map_err(|e| {
            let _ = std::fs::remove_file(&temp);
            e.to_string()
        })?;

        if let Some(previous) = previous_name.filter(|previous| *previous != scene.name) {
            let old = dir.join(format!("{previous}.scene.json"));
            match std::fs::remove_file(old) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(target)
    }

    /// Delete one editable scene. Missing files are already in the desired state.
    pub fn delete_from_dir(dir: &Path, name: &str) -> Result<(), String> {
        if !is_slug(name) {
            return Err(format!("invalid scene name `{name}`"));
        }
        match std::fs::remove_file(dir.join(format!("{name}.scene.json"))) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn load_dir(dir: &Path, source: SceneSource, out: &mut SceneLibrary) {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                tracing::warn!("scene dir {dir:?}: {e}");
                return;
            }
        };
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        for path in paths {
            let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if let Some(stem) = file_name.strip_suffix(".scene.json") {
                let data = match std::fs::read_to_string(&path) {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::warn!("scene {path:?}: {e}");
                        continue;
                    }
                };
                match serde_json::from_str::<Scene>(&data) {
                    Ok(scene) => match validate_scene(&scene) {
                        Ok(()) => {
                            if scene.name != stem {
                                tracing::warn!(
                                    "scene {path:?}: filename stem `{stem}` != name `{}`",
                                    scene.name
                                );
                            }
                            out.scenes.push(ResolvedScene {
                                scene,
                                source: source.clone(),
                                path: Some(path),
                            });
                        }
                        Err(e) => tracing::warn!("scene {path:?}: {e}"),
                    },
                    Err(e) => tracing::warn!("scene {path:?}: {e}"),
                }
            } else if let Some(stem) = file_name.strip_suffix(".pipeline.json") {
                let data = match std::fs::read_to_string(&path) {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::warn!("pipeline {path:?}: {e}");
                        continue;
                    }
                };
                match serde_json::from_str::<Pipeline>(&data) {
                    Ok(pipeline) => match validate_pipeline(&pipeline) {
                        Ok(()) => {
                            if pipeline.name != stem {
                                tracing::warn!(
                                    "pipeline {path:?}: filename stem `{stem}` != name `{}`",
                                    pipeline.name
                                );
                            }
                            out.pipelines.push(ResolvedPipeline {
                                pipeline,
                                source: source.clone(),
                                path: Some(path),
                            });
                        }
                        Err(e) => tracing::warn!("pipeline {path:?}: {e}"),
                    },
                    Err(e) => tracing::warn!("pipeline {path:?}: {e}"),
                }
            }
        }
    }

    pub fn scenes(&self) -> &[ResolvedScene] {
        &self.scenes
    }

    pub fn pipelines(&self) -> &[ResolvedPipeline] {
        &self.pipelines
    }

    /// Resolve a scene reference: `builtin:`/`user:`/`project:` pin a source,
    /// `<plugin-id>:scene:<name>` pins a plugin, a bare name walks precedence order.
    pub fn resolve(&self, reference: &str) -> Option<&ResolvedScene> {
        if let Some(name) = reference.strip_prefix("builtin:") {
            return self
                .scenes
                .iter()
                .find(|s| s.source == SceneSource::Builtin && s.scene.name == name);
        }
        if let Some(name) = reference.strip_prefix("user:") {
            return self
                .scenes
                .iter()
                .find(|s| s.source == SceneSource::User && s.scene.name == name);
        }
        if let Some(name) = reference.strip_prefix("project:") {
            return self
                .scenes
                .iter()
                .find(|s| s.source == SceneSource::Project && s.scene.name == name);
        }
        if let Some((plugin_id, name)) = reference.split_once(":scene:") {
            return self.scenes.iter().find(|s| {
                matches!(&s.source, SceneSource::Plugin { plugin_id: id } if id == plugin_id)
                    && s.scene.name == name
            });
        }
        self.scenes.iter().find(|s| s.scene.name == reference)
    }

    pub fn resolve_pipeline(&self, reference: &str) -> Option<&ResolvedPipeline> {
        if let Some(name) = reference.strip_prefix("builtin:") {
            return self
                .pipelines
                .iter()
                .find(|p| p.source == SceneSource::Builtin && p.pipeline.name == name);
        }
        if let Some(name) = reference.strip_prefix("user:") {
            return self
                .pipelines
                .iter()
                .find(|p| p.source == SceneSource::User && p.pipeline.name == name);
        }
        if let Some(name) = reference.strip_prefix("project:") {
            return self
                .pipelines
                .iter()
                .find(|p| p.source == SceneSource::Project && p.pipeline.name == name);
        }
        if let Some((plugin_id, name)) = reference.split_once(":pipeline:") {
            return self.pipelines.iter().find(|p| {
                matches!(&p.source, SceneSource::Plugin { plugin_id: id } if id == plugin_id)
                    && p.pipeline.name == name
            });
        }
        self.pipelines.iter().find(|p| p.pipeline.name == reference)
    }

    /// Canonical pinned reference for a resolved scene.
    pub fn reference_for(entry: &ResolvedScene) -> String {
        match &entry.source {
            SceneSource::Project => format!("project:{}", entry.scene.name),
            SceneSource::User => format!("user:{}", entry.scene.name),
            SceneSource::Builtin => format!("builtin:{}", entry.scene.name),
            SceneSource::Plugin { plugin_id } => {
                format!("{plugin_id}:scene:{}", entry.scene.name)
            }
        }
    }

    /// Canonical pinned reference for a resolved pipeline (`:pipeline:` analog of scenes).
    pub fn pipeline_reference_for(entry: &ResolvedPipeline) -> String {
        match &entry.source {
            SceneSource::Project => format!("project:{}", entry.pipeline.name),
            SceneSource::User => format!("user:{}", entry.pipeline.name),
            SceneSource::Builtin => format!("builtin:{}", entry.pipeline.name),
            SceneSource::Plugin { plugin_id } => {
                format!("{plugin_id}:pipeline:{}", entry.pipeline.name)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Posture mapping + the escalation chokepoint
// ---------------------------------------------------------------------------

pub fn session_mode_policy(mode: SceneSessionMode) -> ExecutionPolicy {
    match mode {
        SceneSessionMode::ReadOnly => ExecutionPolicy {
            mode: PermissionMode::Ask,
            sandbox: SandboxPolicy::ReadOnly,
        },
        SceneSessionMode::Ask => ExecutionPolicy {
            mode: PermissionMode::Ask,
            sandbox: SandboxPolicy::WorkspaceWrite,
        },
        SceneSessionMode::AutoEdit => ExecutionPolicy {
            mode: PermissionMode::AcceptEdits,
            sandbox: SandboxPolicy::WorkspaceWrite,
        },
        SceneSessionMode::FullAccess => ExecutionPolicy {
            mode: PermissionMode::Yolo,
            sandbox: SandboxPolicy::DangerFullAccess,
        },
    }
}

/// Conservative inverse: unmatched axis combinations map to the loosest mode they imply, so the
/// escalation comparison can never under-report the session's current looseness.
pub fn policy_session_mode(policy: &ExecutionPolicy) -> SceneSessionMode {
    match policy.sandbox {
        SandboxPolicy::ReadOnly => SceneSessionMode::ReadOnly,
        SandboxPolicy::DangerFullAccess => SceneSessionMode::FullAccess,
        SandboxPolicy::WorkspaceWrite => match policy.mode {
            PermissionMode::Ask => SceneSessionMode::Ask,
            PermissionMode::AcceptEdits => SceneSessionMode::AutoEdit,
            PermissionMode::Yolo => SceneSessionMode::FullAccess,
        },
    }
}

pub fn memory_preset_policy(preset: SceneMemoryPreset) -> (MemoryAccess, MemoryAccess) {
    match preset {
        SceneMemoryPreset::Standard => (MemoryAccess::Inherit, MemoryAccess::Inherit),
        SceneMemoryPreset::ReadOnly => (MemoryAccess::Allow, MemoryAccess::Deny),
        SceneMemoryPreset::Private => (MemoryAccess::Deny, MemoryAccess::Deny),
        SceneMemoryPreset::LearnOnly => (MemoryAccess::Deny, MemoryAccess::Allow),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EscalationRequired {
    pub from: SceneSessionMode,
    pub to: SceneSessionMode,
}

/// The ONLY function allowed to turn a scene's `session_mode` into an [`ExecutionPolicy`] change.
///
/// Tightening or no-op always succeeds; loosening (target ranks above the current mode per
/// [`SceneSessionMode`]'s ordering) without `user_confirmed` returns [`EscalationRequired`] and
/// the caller must apply NOTHING. Hooks and pipeline auto-gates call this with
/// `user_confirmed = false` and therefore can never loosen.
pub fn apply_execution(
    current: &ExecutionPolicy,
    target: Option<SceneSessionMode>,
    user_confirmed: bool,
) -> Result<Option<ExecutionPolicy>, EscalationRequired> {
    let Some(target) = target else {
        return Ok(None);
    };
    let current_mode = policy_session_mode(current);
    if target > current_mode && !user_confirmed {
        return Err(EscalationRequired {
            from: current_mode,
            to: target,
        });
    }
    let policy = session_mode_policy(target);
    if policy == *current {
        Ok(None)
    } else {
        Ok(Some(policy))
    }
}

// ---------------------------------------------------------------------------
// Apply planning — the soft/full binding matrix
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyStrength {
    Soft,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingField {
    Providers,
    Model,
    ReasoningEffort,
    Worktree,
}

#[derive(Debug, Clone, Serialize)]
pub struct SceneSessionParams {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    /// `None` = inherit the project default; `Some(false)` = the scene pins worktree off.
    pub use_worktree: Option<bool>,
    pub worktree_base: Option<WorktreeBaseline>,
    pub initial_policy: Option<ExecutionPolicy>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SceneApplyPlan {
    pub scene_ref: String,
    pub execution: Option<ExecutionPolicy>,
    pub memory: Option<(MemoryAccess, MemoryAccess)>,
    pub pending: Vec<PendingField>,
    /// Set ⇒ NOTHING was applied; the caller re-calls with `confirm_escalation = true`.
    pub escalation: Option<EscalationRequired>,
    pub new_session: Option<SceneSessionParams>,
}

pub fn plan_apply(
    current: &ExecutionPolicy,
    scene: &Scene,
    scene_ref: &str,
    strength: ApplyStrength,
    confirm_escalation: bool,
) -> SceneApplyPlan {
    let execution = scene.execution.clone().unwrap_or_default();
    let target = execution.session_mode;
    let applied = match apply_execution(current, target, confirm_escalation) {
        Ok(policy) => policy,
        Err(escalation) => {
            return SceneApplyPlan {
                scene_ref: scene_ref.to_string(),
                execution: None,
                memory: None,
                pending: Vec::new(),
                escalation: Some(escalation),
                new_session: None,
            };
        }
    };
    let memory = execution.memory_preset.map(memory_preset_policy);
    match strength {
        ApplyStrength::Soft => {
            let mut pending = Vec::new();
            if !execution.providers.is_empty() {
                pending.push(PendingField::Providers);
            }
            if execution.model.is_some() {
                pending.push(PendingField::Model);
            }
            if execution.reasoning_effort.is_some() {
                pending.push(PendingField::ReasoningEffort);
            }
            if execution.worktree.is_some() {
                pending.push(PendingField::Worktree);
            }
            SceneApplyPlan {
                scene_ref: scene_ref.to_string(),
                execution: applied,
                memory,

                pending,
                escalation: None,
                new_session: None,
            }
        }
        ApplyStrength::Full => {
            let (use_worktree, worktree_base) = match execution.worktree {
                None => (None, None),
                Some(SceneWorktree::Off) => (Some(false), None),
                Some(SceneWorktree::Current) => (Some(true), Some(WorktreeBaseline::Current)),
                Some(SceneWorktree::OriginDefault) => {
                    (Some(true), Some(WorktreeBaseline::OriginDefault))
                }
            };
            SceneApplyPlan {
                scene_ref: scene_ref.to_string(),
                execution: applied,
                memory,

                pending: Vec::new(),
                escalation: None,
                new_session: Some(SceneSessionParams {
                    provider: execution.providers.first().cloned(),
                    model: execution.model.clone(),
                    reasoning_effort: execution.reasoning_effort.clone(),
                    use_worktree,
                    worktree_base,
                    initial_policy: applied.or(Some(*current)),
                }),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Prompt preamble
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarriedArtifact {
    pub label: String,
    pub from_stage: Option<String>,
    pub version: i64,
    pub content: String,
}

/// Everything a scene injects at compile time, in the normative order: guardrails → inline
/// fragments → artifact-capture instruction → clarify instruction → carried artifacts. The engine
/// prepends this AFTER project rules and BEFORE the user's document. An empty scene yields "".
pub fn prompt_preamble(scene: &Scene, carried: &[CarriedArtifact]) -> String {
    let mut sections: Vec<String> = Vec::new();

    if let Some(constraints) = &scene.constraints {
        if !constraints.guardrails.is_empty() {
            let mut s = String::from("## Scene guardrails\n");
            for rule in &constraints.guardrails {
                s.push_str("- ");
                s.push_str(rule);
                s.push('\n');
            }
            sections.push(s);
        }
    }

    if let Some(skills) = &scene.skills {
        for fragment in &skills.inline {
            sections.push(format!("## {}\n\n{}\n", fragment.name, fragment.text));
        }
    }

    if !scene.artifacts.is_empty() {
        let mut s = String::from(
            "## Scene artifacts\n\nThis stage is expected to produce the artifacts below. Emit \
             each one as a single fenced markdown code block whose info string is \
             `artifact:<id>` (for example ```` ```artifact:research-report ````). Re-emitting an \
             id replaces the previous version.\n",
        );
        for artifact in &scene.artifacts {
            s.push_str(&format!(
                "- `{}` — {}{}\n",
                artifact.id,
                artifact.title,
                if artifact.required { " (required)" } else { "" }
            ));
            if let Some(template) = &artifact.template {
                s.push_str("  Follow this skeleton:\n\n");
                for line in template.lines() {
                    s.push_str("  ");
                    s.push_str(line);
                    s.push('\n');
                }
            }
        }
        sections.push(s);
    }

    if let Some(brief) = &scene.brief {
        match brief.clarify.unwrap_or(BriefClarify::MultiChoice) {
            BriefClarify::MultiChoice => sections.push(
                "## Clarifying questions\n\nIf the brief is underspecified, ask structured \
                 multiple-choice questions (2–4 options each) before starting; do not guess.\n"
                    .to_string(),
            ),
            BriefClarify::FreeForm => sections.push(
                "## Clarifying questions\n\nIf the brief is underspecified, ask concise free-form \
                 questions before starting; do not guess.\n"
                    .to_string(),
            ),
            BriefClarify::Off => {}
        }
    }

    for artifact in carried {
        let from = artifact
            .from_stage
            .as_deref()
            .map(|s| format!(" (from {s}, v{})", artifact.version))
            .unwrap_or_else(|| format!(" (v{})", artifact.version));
        sections.push(format!(
            "## Carried artifact: {}{from}\n\n{}\n",
            artifact.label, artifact.content
        ));
    }

    sections.join("\n")
}

// ---------------------------------------------------------------------------
// Pipeline transition evaluation (R9 append — scene.rs is otherwise frozen)
// ---------------------------------------------------------------------------

/// One effective outgoing edge of a pipeline stage, with the trigger and gate resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveTransition {
    pub to: String,
    pub when: TransitionTrigger,
    pub gate: Gate,
}

/// The effective outgoing edges of `stage_id` (docs/reference/scenes.md §Pipelines): listed transitions
/// with `from == stage_id` REPLACE the default edge entirely; absent any, the default is the next
/// stage in array order with `when = exit_criteria_met` and the target stage's gate (or
/// [`Gate::Suggest`]). The last stage with no listed transitions has no edges — the pipeline
/// completes there. An unknown stage id yields no edges (degrade, never error).
pub fn outgoing_edges(pipeline: &Pipeline, stage_id: &str) -> Vec<EffectiveTransition> {
    let stage_gate = |id: &str| {
        pipeline
            .stages
            .iter()
            .find(|stage| stage.id == id)
            .and_then(|stage| stage.gate)
            .unwrap_or(Gate::Suggest)
    };
    let listed: Vec<EffectiveTransition> = pipeline
        .transitions
        .iter()
        .filter(|transition| transition.from == stage_id)
        .map(|transition| EffectiveTransition {
            to: transition.to.clone(),
            when: transition.when,
            gate: transition.gate.unwrap_or_else(|| stage_gate(&transition.to)),
        })
        .collect();
    if !listed.is_empty() {
        return listed;
    }
    let Some(index) = pipeline.stages.iter().position(|stage| stage.id == stage_id) else {
        return Vec::new();
    };
    match pipeline.stages.get(index + 1) {
        Some(next) => vec![EffectiveTransition {
            to: next.id.clone(),
            when: TransitionTrigger::ExitCriteriaMet,
            gate: next.gate.unwrap_or(Gate::Suggest),
        }],
        None => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// SKILL.md export (R14 append — scene.rs is otherwise frozen)
// ---------------------------------------------------------------------------

/// Double-quoted YAML scalar, safe for arbitrary titles/descriptions in frontmatter.
fn yaml_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A code fence longer than any backtick run inside `text` (at least the standard three), so the
/// brief template can itself contain fenced blocks without breaking the export.
fn fence_for(text: &str) -> String {
    let mut longest = 0usize;
    let mut run = 0usize;
    for c in text.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat((longest + 1).max(3))
}

/// Export a scene as a SKILL.md document for hosts that only speak Agent Skills
/// (docs/reference/scenes.md §Interop). Deliberately lossy: only the prompt-shaped parts survive —
/// guardrails, inline fragments, and the brief template as a suggested prompt skeleton. The
/// exporter must say what was lost, so the document ends with a note naming the execution
/// posture, artifacts, exit criteria, and hooks that did not survive.
pub fn export_skill_md(scene: &Scene) -> String {
    let description = if scene.description.trim().is_empty() {
        &scene.title
    } else {
        &scene.description
    };
    let mut out = format!(
        "---\nname: {}\ndescription: {}\n---\n\n# {}\n",
        yaml_quote(&scene.name),
        yaml_quote(description),
        scene.title,
    );

    if let Some(constraints) = &scene.constraints {
        if !constraints.guardrails.is_empty() {
            out.push_str("\n## Guardrails\n\n");
            for rule in &constraints.guardrails {
                out.push_str("- ");
                out.push_str(rule);
                out.push('\n');
            }
        }
    }

    if let Some(skills) = &scene.skills {
        for fragment in &skills.inline {
            out.push_str(&format!("\n## {}\n\n{}\n", fragment.name, fragment.text));
        }
    }

    if let Some(brief) = &scene.brief {
        let fence = fence_for(&brief.template);
        out.push_str(&format!(
            "\n## Suggested prompt skeleton\n\n{fence}\n{}\n{fence}\n",
            brief.template.trim_end_matches('\n'),
        ));
    }

    out.push_str(&format!(
        "\n> Lossy export: the execution posture, artifacts, exit criteria, and hooks of scene \
         `{}` do not survive SKILL.md and were omitted.\n",
        scene.name
    ));
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill::SlotKind;

    #[test]
    fn legacy_plan_first_is_readable_but_not_reexported() {
        let execution: SceneExecution = serde_json::from_value(serde_json::json!({
            "plan_first": true, "model": "account-model"
        }))
        .unwrap();
        let value = serde_json::to_value(execution).unwrap();
        assert!(value.get("plan_first").is_none());
        assert_eq!(value["model"], "account-model");
    }

    fn scene_json(name: &str, extra: &str) -> String {
        format!(r#"{{"$schema":"{SCENE_SCHEMA_ID}","name":"{name}","title":"T"{extra}}}"#)
    }

    fn minimal_scene(name: &str) -> Scene {
        serde_json::from_str(&scene_json(name, "")).unwrap()
    }

    #[test]
    fn slug_validation() {
        for ok in ["a", "research", "a.b-c", "x1"] {
            assert!(is_slug(ok), "{ok}");
        }
        for bad in ["", "-a", "a-", "a--b", "a..b", "A", "a_b", ".a"] {
            assert!(!is_slug(bad), "{bad}");
        }
        // Artifact ids allow internal double dashes (schema pattern has no lookahead).
        assert!(is_artifact_id("a--b"));
        assert!(!is_artifact_id("a.b"));
        assert!(!is_artifact_id("-a"));
    }

    #[test]
    fn unknown_fields_rejected() {
        let json = scene_json("x", r#","bogus":1"#);
        assert!(serde_json::from_str::<Scene>(&json).is_err());
    }

    #[test]
    fn wrong_schema_id_invalid() {
        let mut scene = minimal_scene("x");
        scene.schema = "https://example.com/other.json".into();
        assert!(validate_scene(&scene).is_err());
    }

    #[test]
    fn conditional_requirements() {
        let mut scene = minimal_scene("x");
        scene.brief = Some(SceneBrief {
            template: "t {{a}}".into(),
            slots: vec![SlotDef {
                id: "a".into(),
                label: "A".into(),
                kind: SlotKind::Select,
                options: Vec::new(),
                required: false,
                default: None,
            }],
            clarify: None,
        });
        assert!(validate_scene(&scene).is_err(), "select without options");

        let mut scene = minimal_scene("x");
        scene.hooks = vec![SceneHook {
            on: HookEvent::TurnEnd,
            artifact: None,
            schedule: None,
            action: HookAction {
                kind: HookActionKind::Notify,
                scene: None,
                macro_ref: None,
                args: HashMap::new(),
                message: None,
            },
        }];
        assert!(validate_scene(&scene).is_err(), "notify without message");

        let mut scene = minimal_scene("x");
        scene.hooks = vec![SceneHook {
            on: HookEvent::Schedule,
            artifact: None,
            schedule: None,
            action: HookAction {
                kind: HookActionKind::Notify,
                scene: None,
                macro_ref: None,
                args: HashMap::new(),
                message: Some("m".into()),
            },
        }];
        assert!(validate_scene(&scene).is_err(), "schedule without cron");
    }

    #[test]
    fn effective_criteria_default() {
        let scene = minimal_scene("x");
        let criteria = scene.effective_criteria();
        assert_eq!(criteria.len(), 2);
        assert_eq!(criteria[0].kind, ExitCriterionKind::RequiredArtifacts);
        assert_eq!(criteria[1].kind, ExitCriterionKind::UserConfirm);
    }

    #[test]
    fn builtin_library_loads_fixtures() {
        let lib = SceneLibrary::builtin();
        assert_eq!(lib.scenes().len(), 5);
        assert_eq!(lib.pipelines().len(), 1);
        assert!(lib.resolve("develop").is_some());
        assert!(lib.resolve("builtin:develop").is_some());
        assert!(lib.resolve_pipeline("rnd-lifecycle").is_some());
    }

    #[test]
    fn precedence_and_pinned_references() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        let user = tmp.path().join("user");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&user).unwrap();
        // Shadow the builtin `develop` from both project and user sources.
        std::fs::write(
            project.join("develop.scene.json"),
            format!(
                r#"{{"$schema":"{SCENE_SCHEMA_ID}","name":"develop","title":"Project develop"}}"#
            ),
        )
        .unwrap();
        std::fs::write(
            user.join("develop.scene.json"),
            format!(r#"{{"$schema":"{SCENE_SCHEMA_ID}","name":"develop","title":"User develop"}}"#),
        )
        .unwrap();
        // A malformed sibling must not poison the load.
        std::fs::write(user.join("broken.scene.json"), "{not json").unwrap();

        let lib = SceneLibrary::load(Some(&project), Some(&user), &[]);
        let bare = lib.resolve("develop").unwrap();
        assert_eq!(bare.scene.title, "Project develop");
        assert_eq!(bare.source, SceneSource::Project);
        assert_eq!(SceneLibrary::reference_for(bare), "project:develop");
        assert_eq!(
            lib.resolve("user:develop").unwrap().scene.title,
            "User develop"
        );
        assert_eq!(
            lib.resolve("builtin:develop").unwrap().scene.title,
            "Develop"
        );
        assert!(lib.resolve("broken").is_none());
    }

    #[test]
    fn missing_dirs_yield_builtins_only() {
        let lib = SceneLibrary::load(
            Some(Path::new("/nonexistent/a")),
            Some(Path::new("/nonexistent/b")),
            &[],
        );
        assert_eq!(lib.scenes().len(), 5);
    }

    #[test]
    fn apply_execution_matrix() {
        use SceneSessionMode::*;
        let modes = [ReadOnly, Ask, AutoEdit, FullAccess];
        for current_mode in modes {
            let current = session_mode_policy(current_mode);
            for target in modes {
                for confirmed in [false, true] {
                    let result = apply_execution(&current, Some(target), confirmed);
                    if target > current_mode && !confirmed {
                        let err = result.expect_err("loosening without confirm must fail");
                        assert_eq!(err.from, current_mode);
                        assert_eq!(err.to, target);
                    } else {
                        let ok = result.expect("tighten/equal/confirmed must succeed");
                        if target == current_mode {
                            assert_eq!(ok, None, "no-op change");
                        } else {
                            assert_eq!(ok, Some(session_mode_policy(target)));
                        }
                    }
                }
            }
            // No target = inherit = never a change, never an escalation.
            assert_eq!(apply_execution(&current, None, false).unwrap(), None);
        }
    }

    #[test]
    fn plan_apply_soft_vs_full() {
        let mut scene = minimal_scene("x");
        scene.execution = Some(SceneExecution {
            _legacy_plan_first: None,
            providers: vec!["claude_code".into()],
            model: Some("m".into()),
            reasoning_effort: None,
            session_mode: Some(SceneSessionMode::ReadOnly),
            memory_preset: Some(SceneMemoryPreset::Private),
            worktree: Some(SceneWorktree::Current),
        });
        let current = ExecutionPolicy::default(); // ask / workspace_write

        let soft = plan_apply(&current, &scene, "builtin:x", ApplyStrength::Soft, false);
        assert!(soft.escalation.is_none());
        assert_eq!(
            soft.execution,
            Some(session_mode_policy(SceneSessionMode::ReadOnly))
        );
        assert_eq!(soft.memory, Some((MemoryAccess::Deny, MemoryAccess::Deny)));
        assert_eq!(
            soft.pending,
            vec![
                PendingField::Providers,
                PendingField::Model,
                PendingField::Worktree
            ]
        );
        assert!(soft.new_session.is_none());

        let full = plan_apply(&current, &scene, "builtin:x", ApplyStrength::Full, false);
        assert!(full.pending.is_empty());
        let params = full.new_session.unwrap();
        assert_eq!(params.provider.as_deref(), Some("claude_code"));
        assert_eq!(params.use_worktree, Some(true));
        assert_eq!(params.worktree_base, Some(WorktreeBaseline::Current));
    }

    #[test]
    fn plan_apply_escalation_applies_nothing() {
        let mut scene = minimal_scene("x");
        scene.execution = Some(SceneExecution {
            session_mode: Some(SceneSessionMode::FullAccess),
            memory_preset: Some(SceneMemoryPreset::Private),
            ..Default::default()
        });
        let current = ExecutionPolicy::default();
        let plan = plan_apply(&current, &scene, "builtin:x", ApplyStrength::Soft, false);
        let escalation = plan.escalation.expect("must escalate");
        assert_eq!(escalation.from, SceneSessionMode::Ask);
        assert_eq!(escalation.to, SceneSessionMode::FullAccess);
        assert!(plan.execution.is_none());
        assert!(plan.memory.is_none());
        assert!(plan.new_session.is_none());

        let confirmed = plan_apply(&current, &scene, "builtin:x", ApplyStrength::Soft, true);
        assert!(confirmed.escalation.is_none());
        assert!(confirmed.execution.is_some());
    }

    #[test]
    fn preamble_ordering() {
        let mut scene = minimal_scene("x");
        scene.constraints = Some(SceneConstraints {
            guardrails: vec!["Do not modify files.".into()],
            tools: None,
        });
        scene.skills = Some(SceneSkills {
            pinned: Vec::new(),
            inline: vec![SceneInlineFragment {
                name: "Researcher".into(),
                text: "Survey first.".into(),
                icon: None,
            }],
            suppress_unpinned: false,
        });
        scene.artifacts = vec![SceneArtifactSpec {
            id: "report".into(),
            title: "Report".into(),
            kind: SceneArtifactKind::Report,
            required: true,
            template: None,
            description: None,
        }];
        scene.brief = Some(SceneBrief {
            template: "t".into(),
            slots: Vec::new(),
            clarify: None,
        });
        let carried = [CarriedArtifact {
            label: "Research report".into(),
            from_stage: Some("research".into()),
            version: 2,
            content: "findings".into(),
        }];
        let preamble = prompt_preamble(&scene, &carried);
        let guardrail = preamble.find("Do not modify files.").unwrap();
        let fragment = preamble.find("Survey first.").unwrap();
        let capture = preamble.find("artifact:<id>").unwrap();
        let clarify = preamble.find("multiple-choice").unwrap();
        let carried_at = preamble.find("Carried artifact: Research report").unwrap();
        assert!(guardrail < fragment && fragment < capture && capture < clarify);
        assert!(clarify < carried_at);
        assert!(preamble.contains("(from research, v2)"));

        assert_eq!(prompt_preamble(&minimal_scene("y"), &[]), "");
    }

    #[test]
    fn outgoing_edges_against_the_rnd_lifecycle_fixture() {
        let lib = SceneLibrary::builtin();
        let pipeline = &lib.resolve_pipeline("rnd-lifecycle").unwrap().pipeline;

        // No listed transitions from research/develop: the default next-in-array edge, firing on
        // exit_criteria_met with the target stage's gate.
        assert_eq!(
            outgoing_edges(pipeline, "research"),
            vec![EffectiveTransition {
                to: "develop".into(),
                when: TransitionTrigger::ExitCriteriaMet,
                gate: Gate::Suggest,
            }]
        );

        // The test stage's listed edges REPLACE the default (which would have been fix): a
        // tests_failed loop into fix and a confirm-gated advance to acceptance.
        let test_edges = outgoing_edges(pipeline, "test");
        assert_eq!(
            test_edges,
            vec![
                EffectiveTransition {
                    to: "fix".into(),
                    when: TransitionTrigger::TestsFailed,
                    gate: Gate::Suggest,
                },
                EffectiveTransition {
                    to: "acceptance".into(),
                    when: TransitionTrigger::ExitCriteriaMet,
                    gate: Gate::Confirm,
                },
            ]
        );

        // fix → test loop on exit_criteria_met.
        assert_eq!(
            outgoing_edges(pipeline, "fix"),
            vec![EffectiveTransition {
                to: "test".into(),
                when: TransitionTrigger::ExitCriteriaMet,
                gate: Gate::Suggest,
            }]
        );

        // The last stage's only listed edge is the user_request rework loop; without it the
        // stage would have no edges at all.
        assert_eq!(
            outgoing_edges(pipeline, "acceptance"),
            vec![EffectiveTransition {
                to: "develop".into(),
                when: TransitionTrigger::UserRequest,
                gate: Gate::Confirm,
            }]
        );

        assert!(outgoing_edges(pipeline, "missing").is_empty());
    }

    #[test]
    fn outgoing_edges_last_stage_without_listed_transitions_is_empty() {
        let pipeline: Pipeline = serde_json::from_str(&format!(
            r#"{{"$schema":"{PIPELINE_SCHEMA_ID}","name":"two-step","title":"T",
                "stages":[{{"id":"a","scene":"research"}},{{"id":"b","scene":"develop","gate":"confirm"}}]}}"#
        ))
        .unwrap();
        // Default edge inherits the TARGET stage's gate.
        assert_eq!(
            outgoing_edges(&pipeline, "a"),
            vec![EffectiveTransition {
                to: "b".into(),
                when: TransitionTrigger::ExitCriteriaMet,
                gate: Gate::Confirm,
            }]
        );
        assert!(outgoing_edges(&pipeline, "b").is_empty());
    }

    #[test]
    fn export_skill_md_structure_and_lossy_note() {
        let mut scene = minimal_scene("review");
        scene.title = "Design review".into();
        scene.description = "Evidence-based review".into();
        scene.constraints = Some(SceneConstraints {
            guardrails: vec!["Cite evidence.".into(), "Do not fix in this scene.".into()],
            tools: None,
        });
        scene.skills = Some(SceneSkills {
            pinned: vec!["some:skill".into()],
            inline: vec![SceneInlineFragment {
                name: "Reviewer".into(),
                text: "Read before judging.".into(),
                icon: None,
            }],
            suppress_unpinned: false,
        });
        scene.brief = Some(SceneBrief {
            template: "## Focus\n\n{{focus}}\n".into(),
            slots: Vec::new(),
            clarify: None,
        });

        let md = export_skill_md(&scene);
        assert!(md.starts_with("---\nname: \"review\"\ndescription: \"Evidence-based review\"\n---\n"));
        let guardrails = md.find("## Guardrails").unwrap();
        assert!(md.contains("- Cite evidence.\n- Do not fix in this scene.\n"));
        let fragment = md.find("## Reviewer").unwrap();
        assert!(md.contains("Read before judging."));
        let skeleton = md.find("## Suggested prompt skeleton").unwrap();
        assert!(md.contains("```\n## Focus\n\n{{focus}}\n```"));
        let lossy = md.find("> Lossy export:").unwrap();
        assert!(guardrails < fragment && fragment < skeleton && skeleton < lossy);
        // §Interop requires the exporter to name exactly what did not survive.
        assert!(md.contains("execution posture, artifacts, exit criteria, and hooks"));
        assert!(md.contains("`review`"));
    }

    #[test]
    fn export_skill_md_minimal_scene_is_still_valid() {
        let scene = minimal_scene("bare");
        let md = export_skill_md(&scene);
        // Nothing exportable: valid frontmatter (description falls back to the title), the title
        // heading, and the lossy note — nothing else.
        assert!(md.starts_with("---\nname: \"bare\"\ndescription: \"T\"\n---\n\n# T\n"));
        assert!(!md.contains("## Guardrails"));
        assert!(!md.contains("## Suggested prompt skeleton"));
        assert!(md.trim_end().ends_with("were omitted."));
    }

    #[test]
    fn export_skill_md_fences_grow_past_template_backticks() {
        let mut scene = minimal_scene("fenced");
        scene.brief = Some(SceneBrief {
            template: "```sh\necho hi\n```\n".into(),
            slots: Vec::new(),
            clarify: None,
        });
        let md = export_skill_md(&scene);
        assert!(md.contains("````\n```sh\necho hi\n```\n````"));
    }

    #[test]
    fn editable_scene_save_rename_and_delete_stay_inside_the_selected_dir() {
        let dir = tempfile::tempdir().unwrap();
        let mut scene = minimal_scene("my-scene");
        let first = SceneLibrary::save_to_dir(dir.path(), &scene, None).unwrap();
        assert_eq!(first, dir.path().join("my-scene.scene.json"));
        assert_eq!(
            serde_json::from_str::<Scene>(&std::fs::read_to_string(&first).unwrap()).unwrap(),
            scene
        );

        scene.name = "renamed-scene".into();
        let renamed = SceneLibrary::save_to_dir(dir.path(), &scene, Some("my-scene")).unwrap();
        assert!(renamed.is_file());
        assert!(!first.exists());

        SceneLibrary::delete_from_dir(dir.path(), "renamed-scene").unwrap();
        assert!(!renamed.exists());
        SceneLibrary::delete_from_dir(dir.path(), "renamed-scene").unwrap();
        assert!(SceneLibrary::delete_from_dir(dir.path(), "../escape").is_err());
    }
}

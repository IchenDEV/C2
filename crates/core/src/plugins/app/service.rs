//! The core's services — every capability C2 has, published under a name.
//!
//! Before this module, these were fields on a `AppState` struct built by a 200-line `setup()` in
//! the old desktop bridge: twenty subsystems constructed in a fixed order, each one reachable only by
//! whoever held the struct. Now each is a [`Service`] published by the plugin that owns it, and
//! reached by anything that declares it in `inject`. The wiring is no longer a place in the code.

use crate::plugins::app::{PluginConfigStore, PluginScope};
use crate::canvas::CanvasFeatureGate;
use crate::engine::Engine;
use crate::event::Event;
use crate::host_tools::HostToolDiscovery;
use crate::keymap::Keymap;
use crate::memory::MemoryCapability;
use crate::models::available_models;
use crate::provider::{
    Provider, ProviderCapability, ProviderToolset, CODEX_ACP_PACKAGE, CODEX_ACP_VERSION,
};
use crate::provider_lifecycle::{
    ProviderLaunchMode, ProviderLifecycleManager, ProviderLifecycleStatus,
    ProviderRuntimeConfiguration,
};
use crate::scene::SceneLibrary;
use crate::scene_artifact::SceneArtifactStore;
use crate::skill::{builtin_skills, Skill, SkillLibrary};
use crate::store::Store;
use crate::kernel::Service;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::{broadcast, Mutex as AsyncMutex};

/// The loader itself, published so the Core management plane can inspect the runtime-module graph.
///
/// The manager uses the same scoped lifecycle as other Core modules, while its product role and
/// command visibility keep it outside user extension policy.
pub struct LoaderService(pub Arc<Mutex<crate::kernel::Loader>>);

impl Service for LoaderService {
    const NAME: &'static str = "loader";
}

/// Durable user and project policy for managed built-in features and installed extensions.
///
/// This is rooted beside the loader, not provided by `paths`: disabling the paths plugin must not
/// remove the one interface capable of recovering the graph.
pub struct PluginConfigService(pub Arc<Mutex<PluginConfigStore>>);

impl Service for PluginConfigService {
    const NAME: &'static str = "plugin-config";
}

/// Where everything lives on disk. Every other plugin asks this instead of recomputing paths.
pub struct Paths {
    pub data_dir: PathBuf,
}

impl Service for Paths {
    const NAME: &'static str = "paths";
}

impl Paths {
    pub fn new(data_dir: impl Into<PathBuf>) -> Paths {
        Paths {
            data_dir: data_dir.into(),
        }
    }

    pub fn db(&self) -> PathBuf {
        self.data_dir.join("codetwo.db")
    }

    pub fn skills(&self) -> PathBuf {
        self.data_dir.join("skills")
    }

    pub fn plugins(&self) -> PathBuf {
        self.data_dir.join("plugins")
    }

    pub fn scenes(&self) -> PathBuf {
        self.data_dir.join("scenes")
    }

    pub fn keymap(&self) -> PathBuf {
        self.data_dir.join("keymap.json")
    }
}

/// Persistent storage (SQLite): sessions, transcripts, memory, artifacts, usage.
pub struct StoreService(pub Arc<Store>);

impl Service for StoreService {
    const NAME: &'static str = "store";
}

/// Revocable product memory published through the plugin kernel without coupling Core to it.
#[derive(Clone)]
pub struct MemoryService(pub MemoryCapability);

impl Service for MemoryService {
    const NAME: &'static str = "memory";
}

/// Canvas persistence policy and owner identity. The gate remains closed in production builds;
/// publishing it as a service keeps every canvas command behind the same check.
pub struct CanvasService {
    pub gate: CanvasFeatureGate,
    pub owner: String,
    pub store: Arc<Store>,
}

impl Service for CanvasService {
    const NAME: &'static str = "canvas";
}

impl std::ops::Deref for StoreService {
    type Target = Store;
    fn deref(&self) -> &Store {
        &self.0
    }
}

/// The engine's [`Event`] fan-out. Anything that wants to watch the agent loop subscribes here
/// instead of being handed a receiver at construction time.
pub struct EventBus(pub broadcast::Sender<Event>);

impl Service for EventBus {
    const NAME: &'static str = "bus";
}

#[derive(Debug, Clone)]
pub enum TerminalEvent {
    Data { id: String, data: String },
    Title { id: String, title: String },
    Exit { id: String },
}

/// Live terminal emulators and their host-facing event stream.
pub struct TerminalService {
    pub(crate) terminals: Mutex<HashMap<String, crate::term::TerminalHandle>>,
    events: broadcast::Sender<TerminalEvent>,
}

impl Service for TerminalService {
    const NAME: &'static str = "terminal";
}

impl TerminalService {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(512);
        Self {
            terminals: Mutex::new(HashMap::new()),
            events,
        }
    }

    pub fn publish(&self, event: TerminalEvent) {
        let _ = self.events.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TerminalEvent> {
        self.events.subscribe()
    }
}

impl EventBus {
    pub fn new(capacity: usize) -> EventBus {
        let (tx, _) = broadcast::channel(capacity);
        EventBus(tx)
    }

    pub fn publish(&self, event: Event) {
        let _ = self.0.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.0.subscribe()
    }
}

/// What the UI needs to know about one provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSummary {
    pub id: String,
    pub display_name: String,
    pub custom: bool,
    pub available: bool,
    pub enabled: bool,
    pub needs_node: bool,
    pub models: Vec<crate::event::ModelChoice>,
    pub capabilities: Vec<ProviderCapability>,
    pub management: ProviderLifecycleStatus,
    pub configuration: ProviderRuntimeConfiguration,
}

/// Provider installation facts safe for the default support bundle. Launch commands, paths,
/// environment values, capability failure text, and update-check errors are omitted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RedactedProviderDiagnostics {
    pub provider: String,
    pub enabled: bool,
    pub installed: bool,
    pub installed_version: Option<String>,
    pub launch_mode: ProviderLaunchMode,
    pub configured_adapter_package: Option<String>,
    pub configured_adapter_version: Option<String>,
}

/// The provider registry plus the live host-tool projection shared with the engine.
pub struct ProviderService {
    pub providers: Vec<Provider>,
    lifecycle: ProviderLifecycleManager,
    host_tools: RwLock<HostToolDiscovery>,
    provider_tools: Arc<RwLock<HashMap<String, ProviderToolset>>>,
}

impl Service for ProviderService {
    const NAME: &'static str = "providers";
}

impl ProviderService {
    pub fn new(
        providers: Vec<Provider>,
        host_tools: HostToolDiscovery,
        lifecycle: ProviderLifecycleManager,
    ) -> Self {
        let provider_tools = providers
            .iter()
            .map(|provider| {
                (
                    provider.id.as_str().to_string(),
                    host_tools.toolset(&provider.id),
                )
            })
            .collect();
        Self {
            providers,
            lifecycle,
            host_tools: RwLock::new(host_tools),
            provider_tools: Arc::new(RwLock::new(provider_tools)),
        }
    }

    pub fn runtime_providers(&self) -> Vec<Provider> {
        self.providers
            .iter()
            .filter(|provider| {
                self.lifecycle
                    .enabled(provider.id.as_str())
                    .unwrap_or(false)
            })
            .cloned()
            .collect()
    }

    pub fn lifecycle(&self) -> ProviderLifecycleManager {
        self.lifecycle.clone()
    }

    pub fn toolsets(&self) -> HashMap<String, ProviderToolset> {
        self.provider_tools.read().unwrap().clone()
    }

    pub fn shared_toolsets(&self) -> Arc<RwLock<HashMap<String, ProviderToolset>>> {
        self.provider_tools.clone()
    }

    pub fn computer_use_settings(&self) -> crate::host_tools::ComputerUseSettings {
        self.host_tools.read().unwrap().computer_use_settings()
    }

    pub fn browser_use_settings(&self) -> crate::host_tools::BrowserUseSettings {
        self.host_tools.read().unwrap().browser_use_settings()
    }

    pub fn refresh_host_tools(&self, host_tools: HostToolDiscovery) {
        let provider_tools = self
            .providers
            .iter()
            .map(|provider| {
                (
                    provider.id.as_str().to_string(),
                    host_tools.toolset(&provider.id),
                )
            })
            .collect();
        *self.provider_tools.write().unwrap() = provider_tools;
        *self.host_tools.write().unwrap() = host_tools;
    }

    pub async fn summaries(&self) -> Vec<ProviderSummary> {
        self.summaries_with_updates(false).await
    }

    pub async fn summaries_with_updates(&self, check_updates: bool) -> Vec<ProviderSummary> {
        let toolsets = self.toolsets();
        let mut tasks = tokio::task::JoinSet::new();
        for (index, provider) in self.providers.iter().cloned().enumerate() {
            let lifecycle = self.lifecycle.clone();
            let capabilities = toolsets
                .get(provider.id.as_str())
                .map(|toolset| toolset.capabilities.clone())
                .unwrap_or_default();
            tasks.spawn(async move {
                let enabled = lifecycle.enabled(provider.id.as_str()).unwrap_or(false);
                let management = lifecycle.status(&provider, check_updates).await;
                let configuration = lifecycle
                    .runtime_configuration(&provider)
                    .expect("registered provider has lifecycle configuration");
                let summary = ProviderSummary {
                    id: provider.id.as_str().to_string(),
                    display_name: provider.display_name.clone(),
                    custom: matches!(&provider.id, crate::provider::ProviderId::Custom(_)),
                    available: enabled && management.launch_mode != ProviderLaunchMode::Unavailable,
                    enabled,
                    needs_node: provider.needs_node,
                    models: available_models(&provider).await,
                    capabilities,
                    management,
                    configuration,
                };
                (index, summary)
            });
        }
        let mut summaries = Vec::with_capacity(self.providers.len());
        while let Some(result) = tasks.join_next().await {
            if let Ok(summary) = result {
                summaries.push(summary);
            }
        }
        summaries.sort_by_key(|(index, _)| *index);
        summaries.into_iter().map(|(_, summary)| summary).collect()
    }

    pub async fn redacted_diagnostics(&self) -> Vec<RedactedProviderDiagnostics> {
        let mut tasks = tokio::task::JoinSet::new();
        for (index, provider) in self.providers.iter().cloned().enumerate() {
            let lifecycle = self.lifecycle.clone();
            tasks.spawn(async move {
                let enabled = lifecycle.enabled(provider.id.as_str()).unwrap_or(false);
                let status = lifecycle.status(&provider, false).await;
                let codex = (provider.id.as_str() == "codex").then_some(());
                (
                    index,
                    RedactedProviderDiagnostics {
                        provider: provider.id.as_str().to_string(),
                        enabled,
                        installed: status.installed,
                        installed_version: status.version,
                        launch_mode: status.launch_mode,
                        configured_adapter_package: codex.map(|_| CODEX_ACP_PACKAGE.to_string()),
                        configured_adapter_version: codex.map(|_| CODEX_ACP_VERSION.to_string()),
                    },
                )
            });
        }
        let mut reports = Vec::with_capacity(self.providers.len());
        while let Some(result) = tasks.join_next().await {
            if let Ok(report) = result {
                reports.push(report);
            }
        }
        reports.sort_by_key(|(index, _)| *index);
        reports.into_iter().map(|(_, report)| report).collect()
    }
}

/// The live skill library: built-ins + user skills on disk + installed plugin components +
/// skills discovered from the harness directories of the current workspace.
pub struct SkillService {
    paths: Arc<Paths>,
    catalog: Mutex<SkillLibrary>,
    library: Mutex<SkillLibrary>,
    cwd: Mutex<Option<PathBuf>>,
    plugin_config: Option<Arc<Mutex<PluginConfigStore>>>,
}

impl Service for SkillService {
    const NAME: &'static str = "skills";
}

impl SkillService {
    pub fn new(paths: Arc<Paths>) -> SkillService {
        Self::with_plugin_config(paths, None)
    }

    pub fn with_plugin_config(
        paths: Arc<Paths>,
        plugin_config: Option<Arc<Mutex<PluginConfigStore>>>,
    ) -> SkillService {
        let service = SkillService {
            paths,
            catalog: Mutex::new(SkillLibrary::default()),
            library: Mutex::new(SkillLibrary::default()),
            cwd: Mutex::new(None),
            plugin_config,
        };
        service.reload(None);
        service
    }

    /// A snapshot of the library. Callers get a clone: the engine holds its own copy and is
    /// refreshed through [`crate::plugins::app::events::SkillsChanged`], not by sharing a lock.
    pub fn library(&self) -> SkillLibrary {
        self.library.lock().unwrap().clone()
    }

    pub fn list(&self) -> Vec<Skill> {
        self.catalog.lock().unwrap().all().cloned().collect()
    }

    /// Rebuild from every source. `cwd` selects the workspace whose project-level harness
    /// directories are scanned; `None` keeps the last one.
    pub fn reload(&self, cwd: Option<&Path>) {
        if let Some(cwd) = cwd {
            *self.cwd.lock().unwrap() = Some(cwd.to_path_buf());
        }
        let cwd = self.cwd.lock().unwrap().clone();

        let mut skills = builtin_skills();
        let mut source_enabled = skills
            .iter()
            .map(|skill| (skill.id.clone(), true))
            .collect::<HashMap<_, _>>();
        if let Ok(loaded) = SkillLibrary::load_dir(&self.paths.skills()) {
            for skill in loaded.all().cloned() {
                source_enabled.insert(skill.id.clone(), true);
                skills.push(skill);
            }
        }
        if let Ok(plugins) = crate::plugins::bundle::load_dir(&self.paths.plugins()) {
            for plugin in plugins {
                for skill in plugin.components {
                    source_enabled.insert(skill.id.clone(), plugin.enabled);
                    skills.push(skill);
                }
            }
        }
        for skill in crate::harness::discover(cwd.as_deref()) {
            source_enabled.insert(skill.id.clone(), true);
            skills.push(skill);
        }
        let catalog = SkillLibrary::new(skills);
        let scope = cwd
            .as_deref()
            .map(PluginScope::project)
            .unwrap_or(PluginScope::User);
        let config = self
            .plugin_config
            .as_ref()
            .map(|config| config.lock().unwrap());
        let enabled = catalog
            .all()
            .filter(|skill| {
                source_enabled.get(&skill.id).copied().unwrap_or(true)
                    && config.as_ref().map_or(true, |config| {
                        config.effective_component_enabled(
                            &scope,
                            "skills",
                            &format!("skill:{}", skill.id),
                            true,
                        )
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        *self.catalog.lock().unwrap() = catalog;
        *self.library.lock().unwrap() = SkillLibrary::new(enabled);
    }

    pub fn save(&self, skill: &Skill) -> std::io::Result<()> {
        SkillLibrary::save_to_dir(&self.paths.skills(), skill)?;
        self.reload(None);
        Ok(())
    }

    pub fn delete(&self, id: &str) -> std::io::Result<()> {
        SkillLibrary::delete_from_dir(&self.paths.skills(), id)?;
        self.reload(None);
        Ok(())
    }
}

/// The resolved scene/pipeline library and the artifact captures that go with it.
pub struct SceneService {
    library: Mutex<Arc<SceneLibrary>>,
    cwd: Mutex<Option<PathBuf>>,
    /// `None` when the store has no blob root (an in-memory database). Scene *resolution* is pure
    /// data and works regardless; only capturing artifacts needs somewhere to put them, and losing
    /// captures is not a reason to take the whole scene layer down.
    pub artifacts: Option<SceneArtifactStore>,
}

impl Service for SceneService {
    const NAME: &'static str = "scenes";
}

impl SceneService {
    pub fn new(artifacts: Option<SceneArtifactStore>) -> SceneService {
        SceneService {
            library: Mutex::new(Arc::new(SceneLibrary::builtin())),
            cwd: Mutex::new(None),
            artifacts,
        }
    }

    pub fn library(&self) -> Arc<SceneLibrary> {
        self.library.lock().unwrap().clone()
    }

    pub fn set_library(&self, library: Arc<SceneLibrary>) {
        *self.library.lock().unwrap() = library;
    }

    pub fn cwd(&self) -> Option<PathBuf> {
        self.cwd.lock().unwrap().clone()
    }

    /// Re-resolve from every source, in precedence order: the workspace, the user's config
    /// directory, enabled plugin bundles, then the built-ins.
    ///
    /// Callers announce the result with [`crate::plugins::app::events::ScenesChanged`]; the engine and the
    /// hook runtime follow that rather than being updated by hand.
    pub fn reload(&self, cwd: Option<&Path>, hub: Option<&PluginHub>) {
        if let Some(cwd) = cwd {
            *self.cwd.lock().unwrap() = Some(cwd.to_path_buf());
        }
        let cwd = self.cwd.lock().unwrap().clone();
        let project_dir = cwd.map(|cwd| cwd.join(".codetwo/scenes"));
        let user_dir =
            crate::provider::home_dir().map(|home| home.join(".config/codetwo/scenes"));
        let plugins = hub.map(PluginHub::scene_dirs).unwrap_or_default();
        self.set_library(Arc::new(SceneLibrary::load(
            project_dir.as_deref(),
            user_dir.as_deref(),
            &plugins,
        )));
    }
}

/// The Agent Scenes hook dispatcher.
pub struct SceneRuntimeService(pub Arc<crate::scene_runtime::SceneRuntime>);

impl Service for SceneRuntimeService {
    const NAME: &'static str = "scene-runtime";
}

impl std::ops::Deref for SceneRuntimeService {
    type Target = crate::scene_runtime::SceneRuntime;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Per-session token and cost accounting.
pub struct CostService(pub Arc<crate::cost::SessionCostTracker>);

impl Service for CostService {
    const NAME: &'static str = "cost";
}

impl std::ops::Deref for CostService {
    type Target = crate::cost::SessionCostTracker;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// The agent loop.
pub struct EngineService(pub Arc<Engine>);

impl Service for EngineService {
    const NAME: &'static str = "engine";
}

impl std::ops::Deref for EngineService {
    type Target = Engine;
    fn deref(&self) -> &Engine {
        &self.0
    }
}

/// Durable source/target fencing plus portable workspace transfer.
pub struct HandoffService(pub Arc<crate::handoff::TaskHandoffManager>);

impl Service for HandoffService {
    const NAME: &'static str = "handoff";
}

impl std::ops::Deref for HandoffService {
    type Target = crate::handoff::TaskHandoffManager;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Editable key bindings, persisted next to the store.
pub struct KeymapService {
    path: PathBuf,
    keymap: Mutex<Keymap>,
}

impl Service for KeymapService {
    const NAME: &'static str = "keymap";
}

impl KeymapService {
    pub fn load(path: PathBuf) -> KeymapService {
        let keymap = Keymap::load(&path);
        KeymapService {
            path,
            keymap: Mutex::new(keymap),
        }
    }

    pub fn snapshot(&self) -> Keymap {
        self.keymap.lock().unwrap().clone()
    }

    pub fn set(
        &self,
        action: crate::keymap::Action,
        key: String,
    ) -> std::io::Result<Keymap> {
        let mut keymap = self.keymap.lock().unwrap();
        keymap.set(action, key);
        keymap.save(&self.path)?;
        Ok(keymap.clone())
    }
}

/// Installed plugin *bundles* — the data-only packages (skills, subagents, MCP servers, scenes,
/// scaffolds) users install from GitHub.
///
/// A [`crate::kernel::Plugin`] is an internal runtime module; a
/// [`crate::plugins::bundle::InstalledPlugin`] is a separately managed extension Bundle. This Core service
/// bridges the two without making the kernel trait a public extension API.
pub struct PluginHub {
    pub dir: PathBuf,
    /// Serializes installed-bundle mutations with the runtime factory snapshot they publish.
    pub(crate) inventory: AsyncMutex<()>,
}

impl Service for PluginHub {
    const NAME: &'static str = "plugin-hub";
}

impl PluginHub {
    pub fn installed(&self) -> Vec<crate::plugins::bundle::InstalledPlugin> {
        crate::plugins::bundle::load_dir(&self.dir).unwrap_or_default()
    }

    /// `(id, scenes dir)` for every enabled plugin that ships scenes — what the scene loader reads.
    pub fn scene_dirs(&self) -> Vec<(String, PathBuf)> {
        self.installed()
            .into_iter()
            .filter(|plugin| plugin.enabled)
            .map(|plugin| {
                let dir = crate::plugins::bundle::plugin_scenes_dir(&self.dir, &plugin.id);
                (plugin.id, dir)
            })
            .filter(|(_, dir)| dir.is_dir())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::app::{PluginOverride, PluginPolicy};
    use std::collections::BTreeMap;

    #[test]
    fn skill_catalog_keeps_disabled_entries_while_runtime_library_filters_them() {
        let data = tempfile::tempdir().unwrap();
        let project = data.path().join("project");
        let mut store = PluginConfigStore::ephemeral();
        store
            .set_policy(
                PluginScope::User,
                "skills",
                PluginPolicy {
                    components: BTreeMap::from([(
                        "skill:test-writer".into(),
                        PluginOverride::Disabled,
                    )]),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .set_policy(
                PluginScope::project(&project),
                "skills",
                PluginPolicy {
                    components: BTreeMap::from([(
                        "skill:reviewer".into(),
                        PluginOverride::Disabled,
                    )]),
                    ..Default::default()
                },
            )
            .unwrap();

        let service = SkillService::with_plugin_config(
            Arc::new(Paths::new(data.path())),
            Some(Arc::new(Mutex::new(store))),
        );
        assert!(service.list().iter().any(|skill| skill.id == "test-writer"));
        assert!(service.library().get("test-writer").is_none());
        assert!(service.library().get("reviewer").is_some());

        service.reload(Some(&project));
        assert!(service.list().iter().any(|skill| skill.id == "reviewer"));
        assert!(service.library().get("reviewer").is_none());
    }
}

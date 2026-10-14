//! C2 as an internal runtime-module graph with a separate extension boundary.
//!
//! # What changed
//!
//! C2 used to *be* a program with extension points: a `setup()` that constructed subsystems in
//! a fixed order into one `AppState` struct, plus a large hand-written command-wrapper layer that
//! reached into it. That wrapper layer has been removed: adding or removing a feature now changes
//! the runtime-module graph instead of the middle of the application.
//!
//! Following [cordis](https://github.com/cordiverse/cordis), it is now a graph. Each subsystem uses
//! the internal [`Plugin`](crate::kernel::Plugin) lifecycle to publish a
//! [`Service`](crate::kernel::Service) and contribute commands. Each declares what it needs; the
//! kernel decides the order, waits for dependencies, and tears the module's whole world down when
//! it unloads. The boot sequence is a config file:
//!
//! ```text
//! paths ──┬─→ store ──┬─→ scenes ──────┬─→ scene-commands
//!         │           ├─→ engine ──────┤
//!         │           ├─→ memory       ├─→ scene-runtime
//!         │           ├─→ projects     └─→ cost
//!         │           ├─→ canvas ────────→ document
//!         │           └─→ (artifacts, workspace-search, issues)
//!         ├─→ plugin-hub ─→ (skills, extensions)
//!         └─→ keymap
//! providers ─→ engine       skills ─→ (engine, market, document)
//! (git, workspace, usage, voice, terminal: no dependencies)
//! ```
//!
//! # What that buys
//!
//! - **Deletability.** Turn `store` off and everything downstream goes pending rather than
//!   half-working. That configuration is now a real one, exercised by tests.
//! - **Reloadability.** Reconfigure `store` and the engine — which was built against it — is torn
//!   down and rebuilt automatically. Nothing holds a stale handle, because nothing is asked to
//!   handle its dependencies changing.
//! - **One host transport.** The desktop bridge exposes only [`CoreApp::call`]; in-process hosts can
//!   additionally consume typed services for streaming protocols without constructing a second
//!   copy of the subsystem. Kernel commands remain internal unless explicitly exported through the
//!   versioned Extension API.
//! - **Extensions that are not ours.** [`protocol`] lets an installed extension run as a process in
//!   any language. It shares lifecycle plumbing with Core runtime modules without inheriting Core's
//!   private API.
//!
//! # Booting
//!
//! ```no_run
//! # use codetwo_core::plugins::{AppConfig, CoreApp};
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! let app = CoreApp::boot(AppConfig::new("/home/me/.codetwo")).await?;
//! let status = app.call("git.status", serde_json::json!({ "cwd": "/repo" })).await?;
//! # Ok(())
//! # }
//! ```

mod bundle_runtime;
mod data_dir_lock;
pub mod events;
mod plugin_config;
mod plugin_manager;
pub mod plugins;
pub mod protocol;
mod service;

pub use events::TerminalOutputEvent;
pub use plugin_config::{
    normalize_project_path, PluginConfigDocument, PluginConfigError, PluginConfigStore,
    PluginOverride, PluginPolicy, PluginRecoveryState, PluginScope,
};
pub use plugin_manager::{
    PluginActiveResource, PluginCatalog, PluginCatalogEntry, PluginChangePlan, PluginChangeRequest,
    PluginChangeResult, PluginManager, PluginManagerError, ProjectActivityLease,
};

pub use service::{
    CanvasService, CostService, EngineService, EventBus, HandoffService, KeymapService,
    LoaderService, MemoryService, Paths, PluginConfigService, PluginHub, ProviderService,
    ProviderSummary, SceneRuntimeService, SceneService, SkillService, StoreService, TerminalEvent,
    TerminalService,
};

use crate::kernel::{
    App, CommandInfo, CommandRealm, Context, KernelError, Loader, LoaderConfig, PluginEntry,
    PluginError, PluginRole, ScopeInfo, ServiceInfo, Status,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Deserialize a command's arguments. A missing payload is an empty object, so commands that take
/// only optional fields can be called with no arguments at all.
pub(crate) fn take_args<T: DeserializeOwned>(value: Value) -> Result<T, PluginError> {
    let value = if value.is_null() {
        Value::Object(Default::default())
    } else {
        value
    };
    serde_json::from_value(value)
        .map_err(|error| PluginError::new(format!("bad arguments: {error}")))
}

/// Serialize a command's result.
pub(crate) fn json<T: Serialize>(value: T) -> Result<Value, PluginError> {
    serde_json::to_value(value).map_err(PluginError::new)
}

/// What to boot: where the data lives, and which plugins to run with what config.
#[derive(Debug)]
pub struct AppConfig {
    pub plugins: LoaderConfig,
    data_dir: Option<PathBuf>,
    data_dir_lock: Option<Arc<data_dir_lock::DataDirLock>>,
}

impl Clone for AppConfig {
    fn clone(&self) -> Self {
        Self {
            plugins: self.plugins.clone(),
            data_dir: self.data_dir.clone(),
            data_dir_lock: None,
        }
    }
}

impl AppConfig {
    /// Acquire before a host touches state outside the plugin graph (for example its socket).
    /// Boot also calls this, so all CoreApp hosts obey the same ownership boundary.
    pub fn acquire_data_dir_lock(&mut self) -> Result<(), KernelError> {
        if self.data_dir_lock.is_none() {
            if let Some(path) = &self.data_dir {
                self.data_dir_lock = Some(Arc::new(
                    data_dir_lock::DataDirLock::acquire(path).map_err(|message| {
                        KernelError::Config {
                            name: "core-ownership".into(),
                            message,
                        }
                    })?,
                ));
            }
        }
        Ok(())
    }

    /// Every built-in, storing under `data_dir`.
    pub fn new(data_dir: impl Into<PathBuf>) -> AppConfig {
        let data_dir = data_dir.into();
        let plugins = LoaderConfig::default()
            .enable(plugins::BUILTIN.iter().copied())
            .with(
                "paths",
                PluginEntry::with_config(
                    serde_json::json!({ "data_dir": data_dir.to_string_lossy() }),
                ),
            );
        AppConfig {
            plugins,
            data_dir: Some(data_dir),
            data_dir_lock: None,
        }
    }

    /// The same graph over an ephemeral database.
    ///
    /// Note what this genuinely cannot do: an in-memory store has no blob root, so scene artifact
    /// capture is unavailable and `scene-runtime` refuses to load. That is the graph reporting a
    /// real limitation rather than half-working, but it means this is not the config to use when
    /// you want every plugin running.
    pub fn in_memory(data_dir: impl Into<PathBuf>) -> AppConfig {
        let mut config = AppConfig::new(data_dir);
        config.plugins.plugins.insert(
            "store".into(),
            PluginEntry::with_config(serde_json::json!({ "path": ":memory:" })),
        );
        config
    }

    /// Start from nothing and pick. Useful for a headless host that wants `git` and `market` but
    /// no agent loop.
    pub fn bare() -> AppConfig {
        AppConfig {
            plugins: LoaderConfig::default(),
            data_dir: None,
            data_dir_lock: None,
        }
    }

    /// Start from an empty graph while keeping durable app configuration under `data_dir`.
    /// Useful for small hosts that still want plugin policy to survive restarts.
    pub fn bare_in(data_dir: impl Into<PathBuf>) -> AppConfig {
        AppConfig {
            plugins: LoaderConfig::default(),
            data_dir: Some(data_dir.into()),
            data_dir_lock: None,
        }
    }

    pub fn with(mut self, name: impl Into<String>, entry: PluginEntry) -> AppConfig {
        self.plugins = self.plugins.with(name, entry);
        self
    }

    /// Drop a plugin from the boot set.
    pub fn without(mut self, name: &str) -> AppConfig {
        self.plugins.plugins.remove(name);
        self
    }
}

/// A booted C2: the kernel, the loader, and the root context.
///
/// Frontends hold one of these instead of an `AppState`. What they can do is whatever the loaded
/// plugins registered — discoverable at runtime through [`CoreApp::commands`].
pub struct CoreApp {
    app: App,
    loader: Arc<Mutex<Loader>>,
    plugin_config: Arc<Mutex<PluginConfigStore>>,
    plugin_manager: Arc<PluginManager>,
    _data_dir_lock: Option<Arc<data_dir_lock::DataDirLock>>,
}

impl CoreApp {
    /// Boot the graph and wait for it to settle.
    ///
    /// Config entries naming plugins nobody registered are logged and skipped: one stale line in a
    /// user's config must not stop the app from starting.
    pub async fn boot(config: AppConfig) -> Result<CoreApp, KernelError> {
        CoreApp::boot_with(config, plugins::builtin_registry()).await
    }

    /// Boot with a customized registry — a host replaces a built-in by registering its own plugin
    /// under the same name, or adds plugins of its own that the config can then enable.
    pub async fn boot_with(
        mut config: AppConfig,
        registry: crate::kernel::PluginRegistry,
    ) -> Result<CoreApp, KernelError> {
        config.acquire_data_dir_lock()?;
        let app = App::new();
        let plugin_config = Arc::new(Mutex::new(match &config.data_dir {
            Some(data_dir) => {
                PluginConfigStore::open(data_dir).map_err(|error| KernelError::Config {
                    name: "plugin-manager".into(),
                    message: error.to_string(),
                })?
            }
            None => PluginConfigStore::ephemeral(),
        }));

        let default_plugins = config.plugins.clone();
        let essential = safe_mode_entries(&registry, &default_plugins);
        apply_persisted_user_policy(
            &mut config.plugins,
            &registry,
            &plugin_config.lock().unwrap(),
        );
        if matches!(
            plugin_config.lock().unwrap().recovery(),
            PluginRecoveryState::SafeMode { .. }
        ) {
            config
                .plugins
                .plugins
                .retain(|name, _| essential.iter().any(|(essential, _)| essential == name));
            for (name, entry) in &essential {
                config.plugins.plugins.insert(name.clone(), entry.clone());
            }
        }

        let loader = Arc::new(Mutex::new(Loader::new(app.ctx(), registry)));
        app.ctx().provide(Arc::new(LoaderService(loader.clone())))?;
        app.ctx()
            .provide(Arc::new(PluginConfigService(plugin_config.clone())))?;
        let plugin_manager = Arc::new(PluginManager::new(
            loader.clone(),
            plugin_config.clone(),
            default_plugins,
            app.ctx().weak(),
        ));
        plugin_manager.start_reaper();
        app.ctx().provide(plugin_manager.clone())?;

        let errors = loader.lock().unwrap().apply(config.plugins);
        for error in errors {
            tracing::warn!("plugin config: {error}");
        }
        app.flush().await;

        for scope in app.runtime().scopes() {
            if let Some(error) = &scope.error {
                tracing::warn!(plugin = %scope.plugin, "plugin failed: {error}");
            }
        }
        let scopes = app.runtime().scopes();
        let configured = loader.lock().unwrap().config().clone();
        let unhealthy = configured
            .plugins
            .iter()
            .filter(|(_, entry)| entry.enabled)
            .filter_map(|(name, _)| {
                let instance = scopes.iter().find(|scope| {
                    scope.plugin == *name && scope.command_realm == CommandRealm::Global
                });
                (!instance.is_some_and(|scope| scope.status == Status::Active)).then(|| {
                    let detail = instance
                        .and_then(|scope| scope.error.clone())
                        .unwrap_or_else(|| {
                            instance
                                .map(|scope| format!("status is {:?}", scope.status))
                                .unwrap_or_else(|| "no runtime instance was created".into())
                        });
                    (name.clone(), detail)
                })
            })
            .collect::<Vec<_>>();
        let recovery = plugin_config.lock().unwrap().recovery().clone();
        if unhealthy.is_empty() && matches!(recovery, PluginRecoveryState::Normal) {
            if let Err(error) = plugin_config.lock().unwrap().mark_last_good() {
                tracing::warn!("could not save last known-good plugin config: {error}");
            }
        } else if unhealthy.is_empty() {
            tracing::warn!(
                recovery = ?recovery,
                "plugin config is still in recovery mode; preserving the existing last-known-good snapshot until an explicit reset"
            );
        } else {
            for (plugin, error) in unhealthy {
                tracing::warn!(
                    plugin = %plugin,
                    "plugin did not become active at startup; preserving the previous last-known-good config: {error}"
                );
            }
        }
        Ok(CoreApp {
            _data_dir_lock: config.data_dir_lock,
            app,
            loader,
            plugin_config,
            plugin_manager,
        })
    }

    /// Invoke a command through the trusted host transport.
    ///
    /// This includes internal commands and is deliberately broader than the public Extension API.
    pub async fn call(&self, name: &str, args: Value) -> Result<Value, KernelError> {
        self.plugin_manager.call(name, args).await
    }

    /// The one share context of this Core (same Arc the remote adapter uses).
    pub fn share_store(&self) -> Option<Arc<crate::share::ShareStore>> {
        self.service::<service::EngineService>()
            .and_then(|engine| engine.share_store())
    }

    /// Ordinary native local-UI command: same creator-only policy as remote participants, with
    /// the trusted stable local actor (`local-owner`). Original unshared tasks stay local-only;
    /// guests' private tasks are denied to the host owner.
    pub async fn call_local(&self, name: &str, args: Value) -> Result<Value, KernelError> {
        self.call_local_scoped(None, name, args).await
    }

    pub async fn call_local_in_project(
        &self,
        project_path: impl AsRef<std::path::Path>,
        name: &str,
        args: Value,
    ) -> Result<Value, KernelError> {
        self.call_local_scoped(Some(project_path.as_ref().to_path_buf()), name, args)
            .await
    }

    async fn call_local_scoped(
        &self,
        project: Option<PathBuf>,
        name: &str,
        args: Value,
    ) -> Result<Value, KernelError> {
        let Some(share) = self.share_store() else {
            return match project {
                Some(path) => self.plugin_manager.call_in_project(path, name, args).await,
                None => self.plugin_manager.call(name, args).await,
            };
        };
        let manager = self.plugin_manager.clone();
        let command = name.to_string();
        // Errors from the policy are plain strings; keep the kernel error type at the boundary.
        let outcome = crate::share_policy::shared_web_ui_call(&share, "", name, args, move |args| async move {
            match project {
                Some(path) => manager.call_in_project(path, &command, args).await,
                None => manager.call(&command, args).await,
            }
            .map_err(|error| error.to_string())
        })
        .await;
        outcome.map_err(|message| KernelError::Config {
            name: name.to_string(),
            message,
        })
    }

    /// Native local event exit policy for one event (host owner is not exempt from private).
    pub fn local_event_visible(&self, event: &crate::Event) -> bool {
        match self.share_store() {
            Some(share) => crate::share_policy::share_event_visible(&share, "", event),
            None => true,
        }
    }

    /// Invoke through one project's command realm, falling back to global commands when the
    /// project has no override. Project child loaders populate the realm lazily.
    pub async fn call_in_project(
        &self,
        project_path: impl AsRef<std::path::Path>,
        name: &str,
        args: Value,
    ) -> Result<Value, KernelError> {
        self.plugin_manager
            .call_in_project(project_path, name, args)
            .await
    }

    /// Look a service up by type — for in-process callers (the TUI, tests) that would rather have
    /// the real thing than a JSON round trip.
    pub fn service<S: crate::kernel::Service>(&self) -> Option<Arc<S>> {
        self.app.ctx().get::<S>()
    }

    /// Wait for the graph to settle after a change.
    pub async fn flush(&self) {
        self.app.flush().await;
    }

    /// Every plugin instance and its status.
    pub fn scopes(&self) -> Vec<ScopeInfo> {
        self.app.runtime().scopes()
    }

    /// Every live service.
    pub fn services(&self) -> Vec<ServiceInfo> {
        self.app.runtime().services()
    }

    /// Every command the running graph offers.
    pub fn commands(&self) -> Vec<CommandInfo> {
        self.app.runtime().commands()
    }

    /// Unload everything, in reverse.
    pub async fn stop(&self) {
        self.plugin_manager.shutdown_projects();
        self.app.stop().await;
    }
}

/// Internal handles used by integration tests that exercise graph mutation and recovery.
///
/// Keeping them behind an explicitly imported testing trait prevents the normal `CoreApp`
/// interface from exposing Kernel implementation objects to hosts.
#[doc(hidden)]
pub mod testing {
    use super::*;

    pub trait CoreAppTestExt {
        fn ctx(&self) -> Context;
        fn loader(&self) -> &Arc<Mutex<Loader>>;
        fn plugin_config(&self) -> &Arc<Mutex<PluginConfigStore>>;
        fn plugin_manager(&self) -> &Arc<PluginManager>;
    }

    impl CoreAppTestExt for CoreApp {
        fn ctx(&self) -> Context {
            self.app.ctx()
        }

        fn loader(&self) -> &Arc<Mutex<Loader>> {
            &self.loader
        }

        fn plugin_config(&self) -> &Arc<Mutex<PluginConfigStore>> {
            &self.plugin_config
        }

        fn plugin_manager(&self) -> &Arc<PluginManager> {
            &self.plugin_manager
        }
    }
}

/// The smallest graph that can keep every management-plane plugin alive in safe mode.
///
/// Required injection names that also name a registered factory are plugin dependencies and are
/// followed transitively. Names with no factory are root/host-provided services. Optional
/// injections and unrelated default-enabled plugins deliberately stay outside safe mode.
fn safe_mode_entries(
    registry: &crate::kernel::PluginRegistry,
    defaults: &LoaderConfig,
) -> Vec<(String, PluginEntry)> {
    let mut included = registry
        .factories()
        .filter(|factory| factory.metadata.essential || factory.name == "kernel")
        .map(|factory| factory.name.clone())
        .collect::<BTreeSet<_>>();

    loop {
        let required = included
            .iter()
            .filter_map(|name| registry.get(name))
            .flat_map(|factory| factory.dependencies.required.iter())
            .filter(|name| registry.get(name).is_some())
            .cloned()
            .collect::<Vec<_>>();
        let before = included.len();
        included.extend(required);
        if included.len() == before {
            break;
        }
    }

    included
        .into_iter()
        .map(|name| {
            let mut entry = defaults.plugins.get(&name).cloned().unwrap_or_default();
            entry.enabled = true;
            (name, entry)
        })
        .collect()
}

fn apply_persisted_user_policy(
    config: &mut LoaderConfig,
    registry: &crate::kernel::PluginRegistry,
    store: &PluginConfigStore,
) {
    for factory in registry.factories() {
        if factory.metadata.role == PluginRole::Core {
            continue;
        }
        let base = config
            .plugins
            .get(&factory.name)
            .map(|entry| entry.enabled)
            .unwrap_or(false);
        let policy = store.policy(&PluginScope::User, &factory.name);
        let enabled = policy.state.resolve(base);
        if base || policy.state != PluginOverride::Inherit || policy.config.is_some() {
            let entry = config.plugins.entry(factory.name.clone()).or_default();
            entry.enabled = if factory.metadata.essential {
                true
            } else {
                enabled
            };
            if let Some(plugin_config) = policy.config {
                entry.config = plugin_config;
            }
        }
    }
}

#[cfg(test)]
mod ownership_tests {
    use super::*;

    /// A concurrent `fork`/`exec` elsewhere in the test process can briefly keep the released
    /// data-dir `flock` alive through an inherited file description. Ownership is still exclusive;
    /// this only tolerates the transient hand-off so the test asserts eventual restart.
    async fn boot_within(data_dir: &std::path::Path, timeout: std::time::Duration) -> CoreApp {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match CoreApp::boot(AppConfig::bare_in(data_dir)).await {
                Ok(app) => return app,
                Err(error) if std::time::Instant::now() < deadline => {
                    tracing::debug!("retrying boot after transient ownership: {error}");
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                Err(error) => panic!("boot did not succeed within the release window: {error}"),
            }
        }
    }

    #[tokio::test]
    async fn cloning_configuration_does_not_clone_core_ownership() {
        let data = tempfile::tempdir().unwrap();
        let mut config = AppConfig::bare_in(data.path());
        config.acquire_data_dir_lock().unwrap();
        let duplicate = config.clone();
        let first = CoreApp::boot(config).await.unwrap();
        assert!(CoreApp::boot(duplicate).await.is_err());
        first.stop().await;
        drop(first);
        let restarted = boot_within(data.path(), std::time::Duration::from_secs(5)).await;
        restarted.stop().await;
    }
}

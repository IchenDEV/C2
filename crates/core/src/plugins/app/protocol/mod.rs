//! The C2 Plugin Protocol — plugins that are not written in Rust, and do not run in this
//! process.
//!
//! # Why this exists
//!
//! [`crate::app`] made C2 an internal runtime-module graph, but a Rust host can only load modules it
//! was compiled with. An external extension is a process; C2 speaks JSON-RPC to it over stdio; and
//! static Manifest commands and initialized event subscriptions land in the same scoped registries
//! as built-in modules.
//!
//! Its commands appear in `kernel.commands`, are callable from the host through `call()`, and
//! disappear the instant it unloads. The reverse direction is narrower: an extension process may
//! discover and invoke only commands explicitly registered as `extension_public`; ordinary Core
//! and frontend commands are internal by default.
//!
//! # Trust
//!
//! Installing a bundle still executes nothing — that property of the Plugin Hub is not weakened
//! here. Enablement and trust make a static adapter eligible; its process starts only on first
//! command activation. Trust is a deliberate user action on an install already shown to ship code.
//! [`ExtensionsPlugin`] is the only host that admits these adapters, and it refuses untrusted
//! bundles by default.
//!
//! # Shape
//!
//! ```text
//!  host                                  plugin process
//!    │  (Manifest commands registered; process dormant)
//!    │  first command: initialize ─────────▶
//!    │  ◀──────────── { commands, events }  (host verifies commands, registers events)
//!    │  command/invoke ───────────────────▶      ← frontend called `call("foo.bar")`
//!    │  event/emit ───────────────────────▶      ← something happened in the host
//!    │  ◀─────────────────── command/call        ← plugin calls an extension-public command
//!    │  ◀─────────────────── event/emit, log
//!    │  (unload: process killed, registrations gone)
//! ```

#[cfg(unix)]
use crate::unix_process_group;

mod peer;
mod wire;

pub use peer::{HostHandler, Peer, ProtocolError, DEFAULT_COMMAND_TIMEOUT};
pub use wire::{
    version_is_compatible, CommandSpec, EventParams, HostInfo, InitializeParams, InitializeResult,
    InvokeParams, LogParams, PROTOCOL_VERSION,
};

use crate::plugins::app::events::ConnectorEvent;
use crate::plugins::bundle::{PluginRuntimeCommand, PluginRuntimeSpec};
use crate::kernel::{
    async_trait, CommandRealm, Context, Event, Injection, Plugin, PluginError, PluginResult,
    WeakContext,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::OnceCell;

/// Observations only: the existing scope and peer still own every lifecycle transition.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum ProcessObservation {
    Dormant,
    Starting,
    Running,
    Failed { error: String },
}

pub(crate) struct ProcessObserver {
    state: Mutex<ProcessObservation>,
    context: WeakContext,
}

impl ProcessObserver {
    fn new(ctx: &Context) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(ProcessObservation::Dormant),
            context: ctx.weak(),
        })
    }

    pub(crate) fn snapshot(&self) -> ProcessObservation {
        self.state.lock().unwrap().clone()
    }

    fn set(&self, state: ProcessObservation) {
        {
            let mut current = self.state.lock().unwrap();
            // A response followed immediately by EOF must not turn a terminal peer back into
            // a running process when activation commits on another task.
            if matches!(state, ProcessObservation::Running)
                && matches!(*current, ProcessObservation::Failed { .. })
            {
                return;
            }
            *current = state;
        }
        if let Some(ctx) = self.context.upgrade() {
            let weak = ctx.weak();
            ctx.spawn(async move {
                if let Some(ctx) = weak.upgrade() {
                    ctx.emit(crate::plugins::app::events::PluginRuntimeChanged).await;
                }
            });
        }
    }
}

/// A started plugin: the two streams to talk over, and how to stop it.
pub struct Channel {
    pub reader: Box<dyn AsyncRead + Unpin + Send>,
    pub writer: Box<dyn AsyncWrite + Unpin + Send>,
    /// Called when the plugin's scope unloads. It must not return until owned resources have
    /// stopped; kernel `flush()` waits for this callback to finish.
    pub shutdown: Box<dyn FnOnce() + Send>,
}

/// How a plugin is started. A trait rather than a concrete process spawn so the protocol is
/// testable over an in-memory duplex — the same trick [`crate::acp`] uses to test the prompt turn
/// without a provider binary.
#[async_trait]
pub trait Transport: Send + Sync + 'static {
    async fn start(&self) -> Result<Channel, PluginError>;
}

/// Start a plugin as a child process with piped stdio.
pub struct ProcessTransport {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    /// Used only in log lines.
    pub label: String,
}

impl ProcessTransport {
    /// Build from an installed bundle's `runtime` block, rooted at that bundle's directory.
    pub fn from_spec(spec: &PluginRuntimeSpec, cwd: PathBuf, label: String) -> ProcessTransport {
        let command_path = PathBuf::from(&spec.command);
        let bundled_command = cwd.join(&command_path);
        let command = if command_path.components().count() == 1
            && is_executable_bundle_command(&bundled_command)
        {
            bundled_command.to_string_lossy().into_owned()
        } else {
            spec.command.clone()
        };
        ProcessTransport {
            command,
            args: spec.args.clone(),
            env: spec
                .env
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            cwd: Some(cwd),
            label,
        }
    }
}

fn is_executable_bundle_command(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[async_trait]
impl Transport for ProcessTransport {
    async fn start(&self) -> Result<Channel, PluginError> {
        let executable = crate::provider::which(&self.command)
            .unwrap_or_else(|| self.command.clone().into());
        let mut command = tokio::process::Command::new(executable);
        command
            .args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            // The child dies with us even if shutdown never runs — a plugin process must not
            // outlive the app that started it.
            .kill_on_drop(true);
        for (key, value) in &self.env {
            command.env(key, value);
        }
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        // A separate group lets teardown signal the plugin and ordinary descendants together.
        // Windows still gets direct-child kill-and-wait below; process-tree ownership there needs
        // a Job Object and is intentionally not claimed by this Unix mechanism.
        #[cfg(unix)]
        command.process_group(0);

        let mut child = command.spawn().map_err(|error| {
            PluginError::new(format!("couldn't start `{}`: {error}", self.command))
        })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| PluginError::new("no stdout"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| PluginError::new("no stdin"))?;

        // A plugin's stderr is its diagnostics channel; route it where every other diagnostic goes
        // rather than letting it vanish into a closed pipe.
        if let Some(stderr) = child.stderr.take() {
            let label = self.label.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let mut lines = tokio::io::BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::warn!(plugin = %label, "{line}");
                }
            });
        }

        let label = self.label.clone();
        let shutdown =
            Box::new(move || terminate_child_process(child, &label)) as Box<dyn FnOnce() + Send>;
        Ok(Channel {
            reader: Box::new(stdout),
            writer: Box::new(stdin),
            shutdown,
        })
    }
}

/// Force a process down and synchronously reap the direct child. Scope disposal runs this callback
/// on the kernel driver; returning only after `try_wait` observes an exit makes the following
/// `flush()` a real lifecycle barrier instead of merely a signal-delivery barrier.
fn terminate_child_process(mut child: tokio::process::Child, label: &str) {
    #[cfg(unix)]
    let process_group = child.id().and_then(|pid| i32::try_from(pid).ok());

    #[cfg(unix)]
    if let Some(process_group) = process_group {
        if let Err(error) = unix_process_group::kill(process_group) {
            if !unix_process_group::is_missing(&error) {
                tracing::warn!(plugin = %label, %error, "could not kill plugin process group");
            }
        }
    }

    if let Err(error) = child.start_kill() {
        tracing::warn!(plugin = %label, %error, "could not kill plugin process");
    }
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                tracing::debug!(plugin = %label, %status, "plugin process exited");
                break;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(1)),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                tracing::error!(plugin = %label, %error, "could not wait for plugin process exit");
                break;
            }
        }
    }
}

/// A plugin that lives in another process.
///
/// Everything the kernel knows about its command surface comes from the Manifest. The handshake
/// confirms the implementation and subscribes to events. From the graph's point of view the ready
/// adapter is an ordinary plugin even while its child process is dormant.
pub struct ProtocolPlugin {
    name: String,
    description: Option<String>,
    inject: Injection,
    transport: Arc<dyn Transport>,
    data_dir: Option<PathBuf>,
    handshake_timeout: std::time::Duration,
    command_timeout: std::time::Duration,
    /// `None` is the 1.0 compatibility path where initialize contributes commands dynamically.
    /// `Some` is the 1.1 static contract: handlers exist before the process and activate it once.
    declared_commands: Option<Vec<PluginRuntimeCommand>>,
}

/// How long to wait for `initialize` before giving up on a plugin.
///
/// This one has teeth: loading runs on the kernel's single driver task, so a plugin that starts
/// and then says nothing would stall the *entire* graph, not just itself. A bounded wait turns
/// "the app hangs on startup" into "one plugin failed, here is its name".
pub const DEFAULT_HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

impl ProtocolPlugin {
    pub fn with_command_timeout(mut self, timeout: std::time::Duration) -> ProtocolPlugin {
        self.command_timeout = timeout;
        self
    }

    pub fn new(name: impl Into<String>, transport: Arc<dyn Transport>) -> ProtocolPlugin {
        ProtocolPlugin {
            name: name.into(),
            description: None,
            inject: Injection::default(),
            transport,
            data_dir: None,
            handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
            command_timeout: DEFAULT_COMMAND_TIMEOUT,
            declared_commands: None,
        }
    }

    pub fn with_handshake_timeout(mut self, timeout: std::time::Duration) -> ProtocolPlugin {
        self.handshake_timeout = timeout;
        self
    }

    /// Build from an installed bundle's `runtime` block.
    pub fn from_spec(
        id: &str,
        spec: &PluginRuntimeSpec,
        bundle_dir: PathBuf,
        data_dir: PathBuf,
    ) -> ProtocolPlugin {
        let transport = ProcessTransport::from_spec(spec, bundle_dir, format!("plugin:{id}"));
        ProtocolPlugin {
            name: id.to_string(),
            description: None,
            inject: Injection {
                required: spec.inject.clone(),
                optional: spec.optional_inject.clone(),
            },
            transport: Arc::new(transport),
            data_dir: Some(data_dir),
            handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
            command_timeout: spec
                .command_timeout_ms
                .map(std::time::Duration::from_millis)
                .unwrap_or(DEFAULT_COMMAND_TIMEOUT),
            declared_commands: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> ProtocolPlugin {
        self.description = Some(description.into());
        self
    }

    pub fn with_inject(mut self, inject: Injection) -> ProtocolPlugin {
        self.inject = inject;
        self
    }

    pub fn with_declared_commands(mut self, commands: Vec<PluginRuntimeCommand>) -> ProtocolPlugin {
        self.declared_commands = Some(commands);
        self
    }
}

type Shutdown = Box<dyn FnOnce() + Send>;

/// A process teardown callback shared by the activation guard and the committed scope effect.
/// Taking the callback makes cancellation, failure, unload, and their races idempotent.
struct ShutdownOnce(Mutex<Option<Shutdown>>);

impl ShutdownOnce {
    fn new(shutdown: Shutdown) -> Arc<Self> {
        Arc::new(Self(Mutex::new(Some(shutdown))))
    }

    fn run(&self) {
        let shutdown = self.0.lock().unwrap().take();
        if let Some(shutdown) = shutdown {
            shutdown();
        }
    }
}

/// Owns a newly started process until initialize and contribution validation have completed.
/// Dropping an uncommitted guard synchronously stops the child, including when the caller cancels.
struct ActivationGuard {
    shutdown: Arc<ShutdownOnce>,
    committed: bool,
}

impl ActivationGuard {
    fn new(shutdown: Shutdown) -> Self {
        Self {
            shutdown: ShutdownOnce::new(shutdown),
            committed: false,
        }
    }

    fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for ActivationGuard {
    fn drop(&mut self) {
        if !self.committed {
            self.shutdown.run();
        }
    }
}

struct PendingActivation {
    peer: Arc<Peer>,
    result: InitializeResult,
    guard: ActivationGuard,
}

struct ProtocolSession {
    observer: Arc<ProcessObserver>,
    plugin: String,
    transport: Arc<dyn Transport>,
    data_dir: Option<PathBuf>,
    handshake_timeout: std::time::Duration,
    command_timeout: std::time::Duration,
}

impl ProtocolSession {
    async fn initialize(
        &self,
        ctx: &Context,
        config: Value,
    ) -> Result<PendingActivation, PluginError> {
        if let Some(dir) = &self.data_dir {
            std::fs::create_dir_all(dir)?;
        }

        self.observer.set(ProcessObservation::Starting);
        let channel = self.transport.start().await?;
        let guard = ActivationGuard::new(channel.shutdown);
        let shutdown = guard.shutdown.clone();
        if !ctx.effect(move || shutdown.run()) {
            return Err(PluginError::new("plugin runtime is unavailable"));
        }
        let command_realm = ctx.command_realm().clone();
        let project_path = match &command_realm {
            CommandRealm::Global => None,
            CommandRealm::Project(path) => Some(path.clone()),
        };
        let host = Arc::new(KernelHost {
            ctx: ctx.weak(),
            plugin: self.plugin.clone(),
        });
        let shutdown = guard.shutdown.clone();
        let observer = self.observer.clone();
        let peer = Peer::managed(
            channel.reader,
            channel.writer,
            host,
            self.command_timeout,
            Box::new(move |error| {
                shutdown.run();
                observer.set(ProcessObservation::Failed { error });
            }),
        );
        let owned = peer.clone();
        if !ctx.effect(move || owned.close("plugin runtime unloaded".into())) {
            peer.close("plugin runtime unavailable".into());
            return Err(PluginError::new("plugin runtime unavailable"));
        }
        let params = InitializeParams {
            protocol_version: PROTOCOL_VERSION.to_string(),
            host: HostInfo {
                name: "code2".into(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                commands: ctx
                    .extension_public_commands()
                    .into_iter()
                    .map(|command| command.name)
                    .collect(),
            },
            config,
            data_dir: self
                .data_dir
                .as_ref()
                .map(|dir| dir.to_string_lossy().into_owned()),
            project_path,
        };
        let result: InitializeResult = tokio::time::timeout(
            self.handshake_timeout,
            peer.request_with_timeout("initialize", params, self.handshake_timeout),
        )
        .await
        .map_err(|_| {
            PluginError::new(format!(
                "did not answer `initialize` within {:?}",
                self.handshake_timeout
            ))
        })?
        .map_err(|error| PluginError::new(format!("handshake failed: {error}")))?;

        if !version_is_compatible(&result.protocol_version) {
            return Err(PluginError::new(format!(
                "speaks plugin protocol {} — this host speaks {PROTOCOL_VERSION}",
                result.protocol_version
            )));
        }
        if !ctx.is_current() {
            return Err(PluginError::new("plugin runtime is unavailable"));
        }
        Ok(PendingActivation {
            peer,
            result,
            guard,
        })
    }
}

fn ensure_command_available(ctx: &Context, plugin: &str, name: &str) -> PluginResult {
    if let Some(existing) = ctx.runtime().commands().into_iter().find(|command| {
        command.realm == CommandRealm::Global && command.name == name && command.plugin != plugin
    }) {
        return Err(PluginError::new(format!(
            "extension command `{name}` conflicts with global command owned by `{}`",
            existing.plugin
        )));
    }
    Ok(())
}

fn register_events(ctx: &Context, peer: &Arc<Peer>, events: &[String]) -> PluginResult {
    for event in events {
        let peer = peer.clone();
        let name = event.clone();
        if !ctx.on_json(event.clone(), move |payload| {
            let peer = peer.clone();
            let name = name.clone();
            async move {
                peer.notify(
                    "event/emit",
                    EventParams {
                        name,
                        payload: (*payload).clone(),
                    },
                );
                None
            }
        }) {
            return Err(PluginError::new("plugin runtime is unavailable"));
        }
    }
    Ok(())
}

fn validate_declared_commands(
    declared: &[PluginRuntimeCommand],
    result: &InitializeResult,
) -> PluginResult {
    let mut actual = BTreeMap::new();
    for command in &result.commands {
        if actual.insert(command.name.as_str(), command).is_some() {
            return Err(PluginError::new(format!(
                "initialize returned duplicate command `{}`",
                command.name
            )));
        }
    }
    let expected_ids = declared
        .iter()
        .map(|command| command.id.as_str())
        .collect::<BTreeSet<_>>();
    let actual_ids = actual.keys().copied().collect::<BTreeSet<_>>();
    if expected_ids != actual_ids {
        let missing = expected_ids
            .difference(&actual_ids)
            .copied()
            .collect::<Vec<_>>();
        let extra = actual_ids
            .difference(&expected_ids)
            .copied()
            .collect::<Vec<_>>();
        return Err(PluginError::new(format!(
            "initialize commands do not match the manifest (missing: {}; extra: {})",
            missing.join(", "),
            extra.join(", ")
        )));
    }
    for command in declared {
        let implemented = actual
            .get(command.id.as_str())
            .expect("equal command sets must contain every declared id");
        if implemented.schema != command.args_schema {
            return Err(PluginError::new(format!(
                "initialize schema for `{}` does not match the manifest argsSchema",
                command.id
            )));
        }
    }
    Ok(())
}

struct LazyProtocolRuntime {
    session: Arc<ProtocolSession>,
    context: WeakContext,
    config: Value,
    commands: Arc<Vec<PluginRuntimeCommand>>,
    activation: OnceCell<Result<Arc<Peer>, String>>,
}

impl LazyProtocolRuntime {
    async fn activate(&self) -> Result<Arc<Peer>, PluginError> {
        let result = self
            .activation
            .get_or_init(|| async {
                self.activate_once().await.map_err(|error| {
                    self.session.observer.set(ProcessObservation::Failed { error: error.to_string() });
                    format!(
                        "plugin activation failed: {error}; reload or disable and re-enable the plugin to retry"
                    )
                })
            })
            .await;
        match result {
            Ok(peer) => Ok(peer.clone()),
            Err(message) => Err(PluginError::new(message.clone())),
        }
    }

    async fn activate_once(&self) -> Result<Arc<Peer>, PluginError> {
        let ctx = self
            .context
            .upgrade()
            .ok_or_else(|| PluginError::new("plugin runtime is unavailable"))?;
        let PendingActivation {
            peer,
            result,
            guard,
        } = self.session.initialize(&ctx, self.config.clone()).await?;
        validate_declared_commands(&self.commands, &result)?;
        register_events(&ctx, &peer, &result.events)?;
        guard.commit();
        self.session.observer.set(ProcessObservation::Running);
        Ok(peer)
    }
}

#[async_trait]
impl Plugin for ProtocolPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    fn inject(&self) -> Injection {
        self.inject.clone()
    }

    async fn apply(&self, ctx: Context, config: Value) -> PluginResult {
        let observer = ProcessObserver::new(&ctx);
        if let Some(manager) = ctx.get::<crate::plugins::app::PluginManager>() {
            manager.observe_process(&self.name, ctx.command_realm().clone(), &observer);
        }
        let session = Arc::new(ProtocolSession {
            observer,
            plugin: self.name.clone(),
            transport: self.transport.clone(),
            data_dir: self.data_dir.clone(),
            handshake_timeout: self.handshake_timeout,
            command_timeout: self.command_timeout,
        });

        let Some(declared) = &self.declared_commands else {
            let PendingActivation {
                peer,
                result,
                guard,
            } = session.initialize(&ctx, config).await?;
            for spec in &result.commands {
                ensure_command_available(&ctx, &self.name, &spec.name)?;
                let peer = peer.clone();
                let name = spec.name.clone();
                ctx.command_described(
                    spec.name.clone(),
                    spec.description.as_deref(),
                    move |args| {
                        let peer = peer.clone();
                        let name = name.clone();
                        async move {
                            peer.request::<_, Value>(
                                "command/invoke",
                                InvokeParams {
                                    name: name.clone(),
                                    args,
                                },
                            )
                            .await
                            .map_err(|error| PluginError::new(error.to_string()))
                        }
                    },
                )?;
            }
            register_events(&ctx, &peer, &result.events)?;
            guard.commit();
            session.observer.set(ProcessObservation::Running);
            ctx.effect(move || drop(peer));
            return Ok(());
        };

        for command in declared {
            ensure_command_available(&ctx, &self.name, &command.id)?;
        }
        let runtime = Arc::new(LazyProtocolRuntime {
            session,
            context: ctx.weak(),
            config,
            commands: Arc::new(declared.clone()),
            activation: OnceCell::new(),
        });
        for command in declared {
            let runtime = runtime.clone();
            let name = command.id.clone();
            let description = if command.description.is_empty() {
                command.title.as_str()
            } else {
                command.description.as_str()
            };
            ctx.command_described(command.id.clone(), Some(description), move |args| {
                let runtime = runtime.clone();
                let name = name.clone();
                async move {
                    let peer = runtime.activate().await?;
                    peer.request::<_, Value>("command/invoke", InvokeParams { name, args })
                        .await
                        .map_err(|error| PluginError::new(error.to_string()))
                }
            })?;
        }
        Ok(())
    }
}

/// The host side of the protocol: what an extension process is allowed to ask for.
struct KernelHost {
    ctx: WeakContext,
    plugin: String,
}

fn connector_owner_id(plugin: &str) -> String {
    plugin.strip_prefix("bundle:").unwrap_or(plugin).to_string()
}

#[async_trait]
impl HostHandler for KernelHost {
    async fn call(&self, name: &str, args: Value) -> Result<Value, String> {
        let Some(ctx) = self.ctx.upgrade() else {
            return Err("the host is shutting down".into());
        };
        ctx.call_extension_public(name, args)
            .await
            .map_err(|error| error.to_string())
    }

    async fn emit(&self, name: &str, payload: Value) {
        if let Some(ctx) = self.ctx.upgrade() {
            if name == ConnectorEvent::NAME {
                ctx.emit(ConnectorEvent {
                    plugin_id: connector_owner_id(&self.plugin),
                    event: payload,
                })
                .await;
            } else {
                ctx.emit_json(name, payload).await;
            }
        }
    }

    fn log(&self, level: &str, message: &str) {
        match level {
            "error" => tracing::error!(plugin = %self.plugin, "{message}"),
            "warn" => tracing::warn!(plugin = %self.plugin, "{message}"),
            "debug" | "trace" => tracing::debug!(plugin = %self.plugin, "{message}"),
            _ => tracing::info!(plugin = %self.plugin, "{message}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connector_events_expose_the_installed_bundle_id_not_the_internal_runtime_name() {
        assert_eq!(
            connector_owner_id("bundle:feishu-collaboration"),
            "feishu-collaboration"
        );
        assert_eq!(connector_owner_id("mock"), "mock");
    }

    #[test]
    fn process_transport_prefers_a_bundle_local_bare_command_then_falls_back_to_path() {
        let bundle = tempfile::tempdir().unwrap();
        let spec: PluginRuntimeSpec =
            serde_json::from_value(serde_json::json!({ "command": "fixture-server" })).unwrap();

        let path_fallback =
            ProcessTransport::from_spec(&spec, bundle.path().to_path_buf(), "fixture".into());
        assert_eq!(path_fallback.command, "fixture-server");

        let local = bundle.path().join("fixture-server");
        std::fs::write(&local, "fixture").unwrap();
        #[cfg(unix)]
        {
            let non_executable =
                ProcessTransport::from_spec(&spec, bundle.path().to_path_buf(), "fixture".into());
            assert_eq!(non_executable.command, "fixture-server");

            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&local, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let local_transport =
            ProcessTransport::from_spec(&spec, bundle.path().to_path_buf(), "fixture".into());
        assert_eq!(local_transport.command, local.to_string_lossy());
    }
}

//! C2's native desktop sidecar.
//!
//! Electrobun owns the application window and native desktop APIs. This process owns the existing
//! Rust plugin graph and exposes exactly one JSON-lines command bridge over stdin/stdout. Plugin
//! events use the same connection in the opposite direction, so the renderer still receives the
//! engine, terminal, automation, and language-server streams without coupling the core to a GUI
//! framework.

mod automation;
mod device_sync;
mod github;
mod host_events;
mod issue_delivery;
mod lsp;
mod remote;
mod scene_mcp;

use std::path::PathBuf;
use std::sync::Arc;

use codetwo_core::{CanvasFeatureGate, DesktopMcpConfig, Engine};
use codetwo_core::kernel::{
    PluginCategory, PluginEntry, PluginMetadata, PluginOrigin, PluginRole, PluginScopeSupport,
};
use codetwo_core::plugins::builtins::{EngineInputs, EnginePlugin};
use codetwo_core::plugins::{AppConfig, CoreApp};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

/// Cloneable output channel shared by host plugins.
///
/// Nothing except the protocol writer touches stdout. That invariant matters because a single log
/// line mixed into JSON would corrupt every pending renderer request.
#[derive(Clone)]
pub struct EventSink {
    output: mpsc::UnboundedSender<Output>,
}

impl EventSink {
    fn new(output: mpsc::UnboundedSender<Output>) -> Self {
        Self { output }
    }

    pub fn emit<T: Serialize>(&self, name: &str, payload: T) -> Result<(), String> {
        let payload = serde_json::to_value(payload).map_err(|error| error.to_string())?;
        self.output
            .send(Output::Message(serde_json::json!({
                "method": "event",
                "params": { "name": name, "payload": payload }
            })))
            .map_err(|_| "desktop protocol output is closed".to_string())
    }
}

enum Output {
    Message(Value),
    Finish,
}

#[derive(Debug, Deserialize)]
struct Request {
    id: u64,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize)]
struct CallParams {
    name: String,
    #[serde(default)]
    args: Value,
    #[serde(default)]
    project_path: Option<String>,
}

fn data_dir_from_args() -> Result<PathBuf, String> {
    let mut args = std::env::args_os().skip(1);
    while let Some(argument) = args.next() {
        if argument == "--data-dir" {
            return args
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| "--data-dir requires a path".to_string());
        }
    }
    Err("missing required --data-dir <path> argument".to_string())
}

fn response(id: u64, result: Result<Value, String>) -> Value {
    match result {
        Ok(result) => serde_json::json!({ "id": id, "result": result }),
        Err(error) => serde_json::json!({ "id": id, "error": error }),
    }
}

async fn write_output(mut output: mpsc::UnboundedReceiver<Output>) -> Result<(), String> {
    let mut stdout = tokio::io::stdout();
    while let Some(item) = output.recv().await {
        let Output::Message(message) = item else {
            break;
        };
        let mut encoded = serde_json::to_vec(&message).map_err(|error| error.to_string())?;
        encoded.push(b'\n');
        stdout
            .write_all(&encoded)
            .await
            .map_err(|error| format!("write desktop stdout: {error}"))?;
        stdout
            .flush()
            .await
            .map_err(|error| format!("flush desktop stdout: {error}"))?;
    }
    Ok(())
}

fn now_millis() -> i64 {
    codetwo_core::session::now_millis()
}

/// Tokio's stdio adapters require blocking descriptors. A spawning runtime can finish
/// configuring inherited pipes after exec, so the input loop also repairs WouldBlock.
pub fn configure_stdio() -> Result<(), String> {
    #[cfg(unix)]
    for fd in [libc::STDIN_FILENO, libc::STDOUT_FILENO] {
        // SAFETY: fcntl changes only flags on the inherited standard descriptors.
        let result = unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags < 0 {
                -1
            } else {
                libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK)
            }
        };
        if result < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    Ok(())
}

/// Boot the core plugin graph and serve the desktop protocol until stdin closes or the host sends
/// `shutdown`. The renderer-facing Browser MCP is intentionally not registered here: Electrobun's
/// stable BrowserView API cannot yet return evaluated JavaScript values or capture screenshots,
/// both of which are required for the existing agent-browser contract.
pub async fn run() -> Result<(), String> {
    codetwo_core::provider::augment_search_path();

    let data_dir = data_dir_from_args()?;
    let mut config = AppConfig::new(&data_dir);
    config
        .acquire_data_dir_lock()
        .map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&data_dir).map_err(|error| {
        format!(
            "could not create desktop data directory {}: {error}",
            data_dir.display()
        )
    })?;

    let (output_tx, output_rx) = mpsc::unbounded_channel();
    let writer = tokio::spawn(write_output(output_rx));
    let events = EventSink::new(output_tx.clone());

    #[cfg(unix)]
    let scene_socket_path = std::env::var_os("CODETWO_SCENE_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir.join("codetwo-scenes.sock"));
    #[cfg(unix)]
    if let Some(parent) = scene_socket_path.parent() {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .map_err(|error| error.to_string())?;
    }
    #[cfg(unix)]
    let scene_master_key = uuid::Uuid::new_v4().to_string();
    #[cfg(unix)]
    let (scene_listener, _scene_ownership) = scene_mcp::bind_broker(&scene_socket_path)?;
    #[cfg(unix)]
    let desktop_mcp = DesktopMcpConfig {
        command: std::env::current_exe()
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .into_owned(),
        socket_path: scene_socket_path.to_string_lossy().into_owned(),
        master_key: scene_master_key.clone(),
        browser_enabled: false,
    };

    let mut registry = codetwo_core::plugins::builtins::builtin_registry();
    #[cfg(unix)]
    let engine_metadata = registry
        .get("engine")
        .expect("built-in engine must be registered")
        .metadata
        .clone();
    #[cfg(unix)]
    registry.register_arc(Box::new(move || {
        let desktop_mcp = desktop_mcp.clone();
        Arc::new(EnginePlugin::with_builder(Arc::new(
            move |inputs: EngineInputs| {
                Engine::with_store_canvas_and_desktop_mcp(
                    inputs.providers,
                    inputs.skills,
                    inputs.store,
                    inputs.memory,
                    CanvasFeatureGate::default(),
                    desktop_mcp.clone(),
                    inputs.provider_tools,
                )
            },
        )))
    }));
    #[cfg(unix)]
    registry
        .set_metadata("engine", engine_metadata)
        .expect("desktop engine replacement must preserve Core metadata");
    let host = events.clone();
    registry.register(move || automation::AutomationPlugin::new(host.clone()));
    registry.register(|| github::GitHubPlugin);
    registry.register(|| issue_delivery::IssueDeliveryPlugin);
    let host = events.clone();
    registry.register(move || lsp::LspPlugin::new(host.clone()));
    let host = events.clone();
    registry.register(move || host_events::HostEventsPlugin::new(host.clone()));
    let device_sync_dir = data_dir.clone();
    let host = events.clone();
    registry.register(move || {
        device_sync::DeviceSyncPlugin::new(device_sync_dir.clone(), host.clone())
    });
    let remote_auth_path = data_dir.join("remote-devices.json");
    registry.register(move || remote::RemotePlugin::new(remote_auth_path.clone()));

    for (name, category, essential, project_scoped) in [
        ("automation", PluginCategory::Automation, false, false),
        ("github", PluginCategory::Integration, false, false),
        ("issue-delivery", PluginCategory::Integration, false, false),
        ("lsp", PluginCategory::DeveloperTools, false, true),
        ("desktop-events", PluginCategory::Foundation, true, false),
        ("device-sync", PluginCategory::Integration, false, false),
        ("remote", PluginCategory::Integration, false, false),
    ] {
        let scope_support = if project_scoped {
            vec![PluginScopeSupport::User, PluginScopeSupport::Project]
        } else {
            vec![PluginScopeSupport::User]
        };
        registry
            .set_metadata(
                name,
                PluginMetadata {
                    origin: PluginOrigin::Host,
                    role: if name == "desktop-events" {
                        PluginRole::Core
                    } else {
                        PluginRole::BuiltIn
                    },
                    category,
                    scope_support,
                    essential,
                    default_enabled: true,
                },
            )
            .expect("host metadata must refer to a registered plugin");
    }

    let config = config
        .with("automation", PluginEntry::default())
        .with("github", PluginEntry::default())
        .with("issue-delivery", PluginEntry::default())
        .with("desktop-events", PluginEntry::default())
        .with("lsp", PluginEntry::default())
        .with("device-sync", PluginEntry::default())
        .with("remote", PluginEntry::default());
    let core = Arc::new(
        CoreApp::boot_with(config, registry)
            .await
            .map_err(|error| error.to_string())?,
    );

    #[cfg(unix)]
    let scene_broker = tokio::spawn(scene_mcp::serve_broker(
        scene_listener,
        core.clone(),
        events.clone(),
        scene_master_key,
    ));

    events.emit(
        "host-ready",
        serde_json::json!({
            "protocol_version": 1,
            "commands": core
                .commands()
                .into_iter()
                .map(|command| command.name)
                .collect::<Vec<_>>()
        }),
    )?;

    let stdin = tokio::io::stdin();
    let mut input = BufReader::new(stdin);
    let mut line = Vec::new();
    let mut calls = tokio::task::JoinSet::new();
    loop {
        match input.read_until(b'\n', &mut line).await {
            Ok(0) if line.is_empty() => break,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                configure_stdio()?;
                tokio::task::yield_now().await;
                continue;
            }
            Err(error) => return Err(format!("read desktop stdin: {error}")),
        }
        // Keep partial bytes across WouldBlock, including split UTF-8 code points.
        let request = serde_json::from_slice::<Request>(&line);
        line.clear();
        let request = match request {
            Ok(request) => request,
            Err(error) => {
                let _ = events.emit("protocol-error", error.to_string());
                continue;
            }
        };

        if request.method == "call" {
            let id = request.id;
            let params = serde_json::from_value::<CallParams>(request.params)
                .map_err(|error| format!("invalid call parameters: {error}"));
            let core = Arc::clone(&core);
            let output = output_tx.clone();
            calls.spawn(async move {
                let result = match params {
                    Ok(params) => match params.project_path {
                        Some(project_path) => {
                            core.call_local_in_project(project_path, &params.name, params.args)
                                .await
                        }
                        None => core.call_local(&params.name, params.args).await,
                    }
                    .map_err(|error| error.to_string()),
                    Err(error) => Err(error),
                };
                let _ = output.send(Output::Message(response(id, result)));
            });
            continue;
        }

        let should_stop = request.method == "shutdown";
        let result = match request.method.as_str() {
            "ping" => Ok(Value::String("pong".into())),
            "shutdown" => Ok(Value::Null),
            method => Err(format!("unknown desktop host method `{method}`")),
        };
        output_tx
            .send(Output::Message(response(request.id, result)))
            .map_err(|_| "desktop protocol output is closed".to_string())?;
        if should_stop {
            break;
        }
    }

    calls.abort_all();
    while calls.join_next().await.is_some() {}
    #[cfg(unix)]
    {
        scene_broker.abort();
        let _ = scene_broker.await;
        let _ = std::fs::remove_file(scene_socket_path);
    }
    core.stop().await;
    drop(core);
    drop(events);
    let _ = output_tx.send(Output::Finish);
    writer
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

/// Private entrypoint used by the scene MCP configuration attached to provider sessions.
pub fn run_scene_mcp() -> Result<(), String> {
    scene_mcp::run_stdio()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_success_and_error_responses_without_protocol_noise() {
        assert_eq!(
            response(7, Ok(serde_json::json!({ "ok": true }))),
            serde_json::json!({ "id": 7, "result": { "ok": true } })
        );
        assert_eq!(
            response(8, Err("nope".into())),
            serde_json::json!({ "id": 8, "error": "nope" })
        );
    }

    #[test]
    fn deserializes_project_scoped_calls() {
        let params: CallParams = serde_json::from_value(serde_json::json!({
            "name": "terminal.write",
            "args": { "id": "shell", "data": "pwd\n" },
            "project_path": "/tmp/project"
        }))
        .unwrap();
        assert_eq!(params.name, "terminal.write");
        assert_eq!(params.project_path.as_deref(), Some("/tmp/project"));
    }

    #[tokio::test]
    async fn event_sink_uses_the_shared_json_lines_envelope() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        EventSink::new(tx).emit("ready", 2).unwrap();
        let Output::Message(message) = rx.recv().await.unwrap() else {
            panic!("event sink finished instead of sending the event");
        };
        assert_eq!(
            message,
            serde_json::json!({
                "method": "event",
                "params": { "name": "ready", "payload": 2 }
            })
        );
    }
}

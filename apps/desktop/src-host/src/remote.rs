//! Remote-control server as a desktop-host plugin.
//!
//! The listener, authentication state and command surface share one plugin scope. Disabling or
//! reloading the plugin therefore aborts the listener and unregisters every `remote.*` command.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use codetwo_core::{Engine, Event, Member, MemberId, Store, WorkspaceId, WorkspaceRole};
use codetwo_core::kernel::{async_trait, Context, Injection, Plugin, PluginError, PluginResult};
use codetwo_core::plugins::{CanvasService, EngineService, EventBus, PluginManager, StoreService};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;

use crate::device_sync::DeviceSyncRuntime;

pub struct RemotePlugin {
    auth_path: PathBuf,
}

impl RemotePlugin {
    pub fn new(auth_path: PathBuf) -> Self {
        Self { auth_path }
    }
}

#[derive(Debug, Serialize, Clone)]
struct RemoteStatus {
    port: u16,
    endpoints: Vec<codetwo_server::PairingEndpoint>,
    protocols: Vec<&'static str>,
}

#[derive(Serialize)]
struct RemotePairingLink {
    endpoint_id: String,
    url: String,
    token: String,
    expires_in: u64,
    qr_svg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    member_id: Option<String>,
}

#[derive(Serialize)]
struct TeamInvite {
    member: Member,
    pairing: RemotePairingLink,
}

struct RemoteHandle {
    port: u16,
    auth: Arc<codetwo_server::AuthState>,
    task: tokio::task::JoinHandle<()>,
    device_sync: bool,
}

impl RemoteHandle {
    fn status(&self) -> RemoteStatus {
        RemoteStatus {
            port: self.port,
            endpoints: RemoteRuntime::endpoints(self.port),
            protocols: if self.device_sync {
                vec!["c2", "t3", "legacy"]
            } else {
                vec!["t3", "legacy"]
            },
        }
    }

    #[cfg(test)]
    fn for_test(port: u16, task: tokio::task::JoinHandle<()>) -> Self {
        Self {
            port,
            auth: Arc::new(codetwo_server::AuthState::load(None)),
            task,
            device_sync: false,
        }
    }
}

impl Drop for RemoteHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

enum RemotePhase {
    Idle,
    Starting(u64),
    Running(RemoteHandle),
    Closed,
}

struct RemoteLifecycle {
    phase: RemotePhase,
    next_ticket: u64,
}

impl Default for RemoteLifecycle {
    fn default() -> Self {
        Self {
            phase: RemotePhase::Idle,
            next_ticket: 0,
        }
    }
}

#[derive(Debug)]
enum RemoteStart {
    Begin(u64),
    Running(RemoteStatus),
}

impl RemoteLifecycle {
    fn begin_start(&mut self) -> Result<RemoteStart, String> {
        match &self.phase {
            RemotePhase::Idle => {}
            RemotePhase::Starting(_) => return Err("remote server is already starting".into()),
            RemotePhase::Running(handle) => {
                return Ok(RemoteStart::Running(handle.status()));
            }
            RemotePhase::Closed => return Err("remote plugin is unloading".into()),
        }

        self.next_ticket = self
            .next_ticket
            .checked_add(1)
            .ok_or_else(|| "remote start generation exhausted".to_string())?;
        let ticket = self.next_ticket;
        self.phase = RemotePhase::Starting(ticket);
        Ok(RemoteStart::Begin(ticket))
    }

    fn finish_start(&mut self, ticket: u64, handle: RemoteHandle) -> Result<RemoteStatus, String> {
        match &self.phase {
            RemotePhase::Starting(current) if *current == ticket => {
                let status = handle.status();
                self.phase = RemotePhase::Running(handle);
                Ok(status)
            }
            RemotePhase::Closed => Err("remote plugin is unloading".into()),
            _ => Err("remote server start was cancelled".into()),
        }
    }

    fn fail_start(&mut self, ticket: u64) {
        if matches!(&self.phase, RemotePhase::Starting(current) if *current == ticket) {
            self.phase = RemotePhase::Idle;
        }
    }

    fn stop(&mut self) {
        if !matches!(&self.phase, RemotePhase::Closed) {
            self.phase = RemotePhase::Idle;
        }
    }

    fn shutdown(&mut self) {
        self.phase = RemotePhase::Closed;
    }

    fn status(&self) -> Option<RemoteStatus> {
        match &self.phase {
            RemotePhase::Running(handle) => Some(handle.status()),
            _ => None,
        }
    }

    fn running_access(&self) -> Option<(u16, Arc<codetwo_server::AuthState>)> {
        match &self.phase {
            RemotePhase::Running(handle) => Some((handle.port, handle.auth.clone())),
            _ => None,
        }
    }

    #[cfg(test)]
    fn starting_ticket(&self) -> Option<u64> {
        match &self.phase {
            RemotePhase::Starting(ticket) => Some(*ticket),
            _ => None,
        }
    }
}

struct RemoteRuntime {
    engine: Arc<Engine>,
    store: Arc<Store>,
    events: broadcast::Sender<Event>,
    canvas_gate: codetwo_core::CanvasFeatureGate,
    device_sync: Option<Arc<DeviceSyncRuntime>>,
    plugin_manager: Arc<PluginManager>,
    share_store: Arc<codetwo_core::share::ShareStore>,
    auth_path: PathBuf,
    lifecycle: Mutex<RemoteLifecycle>,
}

impl RemoteRuntime {
    fn endpoints(port: u16) -> Vec<codetwo_server::PairingEndpoint> {
        codetwo_server::pairing_endpoints(port)
    }

    fn auth(&self) -> Arc<codetwo_server::AuthState> {
        if let Some((_, auth)) = self.lifecycle.lock().unwrap().running_access() {
            return auth;
        }
        Arc::new(codetwo_server::AuthState::load(Some(
            self.auth_path.clone(),
        )))
    }

    fn stop(&self) {
        self.lifecycle.lock().unwrap().stop();
    }

    fn shutdown(&self) {
        self.lifecycle.lock().unwrap().shutdown();
    }

    fn member_pairing_link(
        &self,
        member_id: &MemberId,
        ttl: std::time::Duration,
        endpoint_id: Option<&str>,
    ) -> Result<RemotePairingLink, String> {
        let (port, auth) = self
            .lifecycle
            .lock()
            .unwrap()
            .running_access()
            .ok_or_else(|| "turn on network access first".to_string())?;
        let endpoints = Self::endpoints(port);
        let endpoint = codetwo_server::select_pairing_endpoint(&endpoints, endpoint_id)?;
        let token = auth.issue_member_pairing_token(member_id.as_str(), ttl);
        let url = codetwo_server::pairing_url_for_endpoint(&endpoint.url, &token);
        let qr_svg = if endpoint.qr_shareable {
            codetwo_server::pairing_qr_svg(&url).unwrap_or_default()
        } else {
            String::new()
        };
        Ok(RemotePairingLink {
            endpoint_id: endpoint.id.clone(),
            url,
            token,
            expires_in: ttl.as_secs(),
            qr_svg,
            member_id: Some(member_id.as_str().to_string()),
        })
    }
}

fn take_args<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, PluginError> {
    let value = if value.is_null() {
        Value::Object(Default::default())
    } else {
        value
    };
    serde_json::from_value(value)
        .map_err(|error| PluginError::new(format!("bad arguments: {error}")))
}

fn json<T: Serialize>(value: T) -> Result<Value, PluginError> {
    serde_json::to_value(value).map_err(PluginError::new)
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[async_trait]
impl Plugin for RemotePlugin {
    fn name(&self) -> &str {
        "remote"
    }

    fn inject(&self) -> Injection {
        Injection::required(["engine", "store", "bus", "canvas", "plugin-manager"])
            .with_optional(["device-sync"])
    }

    fn description(&self) -> Option<&str> {
        Some("Authenticated remote access to the live desktop engine.")
    }

    async fn apply(&self, ctx: Context, _config: Value) -> PluginResult {
        // Share metadata lives next to the device auth file in the same data directory.
        // Same Arc the native local adapter uses (engine-attached), never a second store.
        let share_store = match ctx
            .get::<codetwo_core::plugins::EngineService>()
            .and_then(|engine| engine.share_store())
        {
            Some(store) => store,
            None => codetwo_core::share::shared_store(
                self.auth_path.with_file_name("session-shares.json"),
            )
            .map_err(|error| PluginError::new(format!("session share store: {error}")))?,
        };
        let plugin_manager = ctx
            .get::<PluginManager>()
            .ok_or_else(|| PluginError::new("plugin manager is unavailable"))?;
        let runtime = Arc::new(RemoteRuntime {
            engine: ctx
                .get::<EngineService>()
                .ok_or_else(|| PluginError::new("engine service is unavailable"))?
                .0
                .clone(),
            store: ctx
                .get::<StoreService>()
                .ok_or_else(|| PluginError::new("store service is unavailable"))?
                .0
                .clone(),
            events: ctx
                .get::<EventBus>()
                .ok_or_else(|| PluginError::new("event bus is unavailable"))?
                .0
                .clone(),
            canvas_gate: ctx
                .get::<CanvasService>()
                .ok_or_else(|| PluginError::new("canvas service is unavailable"))?
                .gate,
            device_sync: ctx.get::<DeviceSyncRuntime>(),
            plugin_manager,
            share_store,
            auth_path: self.auth_path.clone(),
            lifecycle: Mutex::new(RemoteLifecycle::default()),
        });
        let cleanup = runtime.clone();
        ctx.effect(move || cleanup.shutdown());

        #[derive(Deserialize)]
        struct StartArgs {
            #[serde(default)]
            port: Option<u16>,
        }
        let service = runtime.clone();
        ctx.command("remote.start", move |args| {
            let service = service.clone();
            async move {
                let args: StartArgs = take_args(args)?;
                let ticket = match service
                    .lifecycle
                    .lock()
                    .unwrap()
                    .begin_start()
                    .map_err(PluginError::new)?
                {
                    RemoteStart::Begin(ticket) => ticket,
                    RemoteStart::Running(status) => return json(status),
                };
                let port = args.port.unwrap_or(4599);
                let addr: std::net::SocketAddr =
                    format!("0.0.0.0:{port}").parse().map_err(|error| {
                        service.lifecycle.lock().unwrap().fail_start(ticket);
                        PluginError::new(error)
                    })?;
                let auth = Arc::new(codetwo_server::AuthState::load(Some(
                    service.auth_path.clone(),
                )));
                let device_sync_http = service
                    .device_sync
                    .clone()
                    .map(|device_sync| device_sync as Arc<dyn codetwo_server::DeviceSyncHttp>);
                // Same engine/share store as native; pairing links mint from this exact auth Arc.
                let web_ui_commands = Arc::new(
                    codetwo_server::KernelWebUiCommands::with_share_store(
                        service.plugin_manager.clone(),
                        service.share_store.clone(),
                    )
                    .with_pairing(auth.clone()),
                );
                let bound = codetwo_server::bind_and_serve_with_web_ui(
                    service.engine.clone(),
                    service.events.clone(),
                    addr,
                    auth.clone(),
                    service.store.clone(),
                    service.canvas_gate,
                    device_sync_http,
                    Some(web_ui_commands),
                    None,
                )
                .await;
                let (local, task) = match bound {
                    Ok(bound) => bound,
                    Err(error) => {
                        service.lifecycle.lock().unwrap().fail_start(ticket);
                        return Err(PluginError::new(error));
                    }
                };
                let status = service
                    .lifecycle
                    .lock()
                    .unwrap()
                    .finish_start(
                        ticket,
                        RemoteHandle {
                            port: local.port(),
                            auth,
                            task,
                            device_sync: service.device_sync.is_some(),
                        },
                    )
                    .map_err(PluginError::new)?;
                json(status)
            }
        })?;

        let service = runtime.clone();
        ctx.command("remote.stop", move |_| {
            let service = service.clone();
            async move {
                service.stop();
                Ok(Value::Null)
            }
        })?;

        let service = runtime.clone();
        ctx.command("remote.status", move |_| {
            let service = service.clone();
            async move {
                let status = service.lifecycle.lock().unwrap().status();
                json(status)
            }
        })?;

        #[derive(Deserialize)]
        struct PairingArgs {
            #[serde(default)]
            ttl_secs: Option<u64>,
            #[serde(default)]
            endpoint_id: Option<String>,
            #[serde(default)]
            client_protocol: Option<String>,
        }
        let service = runtime.clone();
        ctx.command("remote.pairing_link", move |args| {
            let service = service.clone();
            async move {
                let args: PairingArgs = take_args(args)?;
                let (port, auth) = service
                    .lifecycle
                    .lock()
                    .unwrap()
                    .running_access()
                    .ok_or_else(|| PluginError::new("turn on network access first"))?;
                let endpoints = RemoteRuntime::endpoints(port);
                let endpoint = codetwo_server::select_pairing_endpoint(
                    &endpoints,
                    args.endpoint_id.as_deref(),
                )
                .map_err(PluginError::new)?;
                let ttl = std::time::Duration::from_secs(
                    args.ttl_secs
                        .unwrap_or(codetwo_server::DEFAULT_PAIRING_TTL.as_secs()),
                );
                let token = match args.client_protocol.as_deref().unwrap_or("c2") {
                    "c2" if service.device_sync.is_some() => auth.issue_c2_pairing_token(ttl),
                    "c2" => return Err(PluginError::new("C2 device sync is unavailable")),
                    "t3" => auth.issue_t3_pairing_token(ttl),
                    "legacy" => auth.issue_pairing_token(ttl),
                    protocol => {
                        return Err(PluginError::new(format!(
                            "unsupported remote client protocol: {protocol}"
                        )))
                    }
                };
                let url = codetwo_server::pairing_url_for_endpoint(&endpoint.url, &token);
                let qr_svg = if endpoint.qr_shareable {
                    codetwo_server::pairing_qr_svg(&url).unwrap_or_default()
                } else {
                    String::new()
                };
                json(RemotePairingLink {
                    endpoint_id: endpoint.id.clone(),
                    url,
                    token,
                    expires_in: ttl.as_secs(),
                    qr_svg,
                    member_id: None,
                })
            }
        })?;

        #[derive(Deserialize)]
        struct TeamBootstrapArgs {
            workspace_name: String,
            admin_name: String,
            #[serde(default)]
            ttl_secs: Option<u64>,
            #[serde(default)]
            endpoint_id: Option<String>,
        }
        let service = runtime.clone();
        ctx.command("remote.team_bootstrap", move |args| {
            let service = service.clone();
            async move {
                let args: TeamBootstrapArgs = take_args(args)?;
                let now_ms = now_millis();
                let member = match service.store.workspace().map_err(PluginError::new)? {
                    Some(_) => service
                        .store
                        .list_members()
                        .map_err(PluginError::new)?
                        .into_iter()
                        .find(|member| member.active && member.role == WorkspaceRole::Admin)
                        .ok_or_else(|| PluginError::new("Workspace has no active Admin"))?,
                    None => {
                        let workspace_id =
                            WorkspaceId::new(format!("workspace-{}", uuid::Uuid::new_v4()));
                        service
                            .store
                            .create_workspace(workspace_id.clone(), &args.workspace_name, now_ms)
                            .map_err(PluginError::new)?;
                        service
                            .store
                            .create_member(
                                &workspace_id,
                                MemberId::new(format!("member-{}", uuid::Uuid::new_v4())),
                                &args.admin_name,
                                WorkspaceRole::Admin,
                                now_ms,
                            )
                            .map_err(PluginError::new)?
                    }
                };
                let ttl = std::time::Duration::from_secs(
                    args.ttl_secs
                        .unwrap_or(codetwo_server::DEFAULT_PAIRING_TTL.as_secs()),
                );
                let pairing = service
                    .member_pairing_link(&member.id, ttl, args.endpoint_id.as_deref())
                    .map_err(PluginError::new)?;
                json(TeamInvite { member, pairing })
            }
        })?;

        #[derive(Deserialize)]
        struct TeamInviteArgs {
            display_name: String,
            #[serde(default)]
            ttl_secs: Option<u64>,
            #[serde(default)]
            endpoint_id: Option<String>,
        }
        let service = runtime.clone();
        ctx.command("remote.team_invite", move |args| {
            let service = service.clone();
            async move {
                let args: TeamInviteArgs = take_args(args)?;
                let workspace = service
                    .store
                    .workspace()
                    .map_err(PluginError::new)?
                    .ok_or_else(|| PluginError::new("bootstrap the Workspace first"))?;
                let member = service
                    .store
                    .create_member(
                        &workspace.id,
                        MemberId::new(format!("member-{}", uuid::Uuid::new_v4())),
                        &args.display_name,
                        WorkspaceRole::Member,
                        now_millis(),
                    )
                    .map_err(PluginError::new)?;
                let ttl = std::time::Duration::from_secs(
                    args.ttl_secs
                        .unwrap_or(codetwo_server::DEFAULT_PAIRING_TTL.as_secs()),
                );
                let pairing = service
                    .member_pairing_link(&member.id, ttl, args.endpoint_id.as_deref())
                    .map_err(PluginError::new)?;
                json(TeamInvite { member, pairing })
            }
        })?;

        let service = runtime.clone();
        ctx.command("remote.devices", move |_| {
            let service = service.clone();
            async move {
                let mut devices = Vec::new();
                for device in service.auth().list_devices() {
                    let mut value = serde_json::to_value(device).map_err(PluginError::new)?;
                    if let Some(object) = value.as_object_mut() {
                        if let Some(id) = object.get("id").and_then(Value::as_str) {
                            object.insert("id".into(), Value::String(format!("in:{id}")));
                        }
                    }
                    devices.push(value);
                }
                if let Some(device_sync) = &service.device_sync {
                    for device in device_sync.devices() {
                        devices.push(serde_json::to_value(device).map_err(PluginError::new)?);
                    }
                }
                Ok(Value::Array(devices))
            }
        })?;

        #[derive(Deserialize)]
        struct PairDeviceArgs {
            url: String,
            #[serde(default)]
            device_name: Option<String>,
        }
        let service = runtime.clone();
        ctx.command("remote.pair_device", move |args| {
            let service = service.clone();
            async move {
                let args: PairDeviceArgs = take_args(args)?;
                let device_sync = service
                    .device_sync
                    .as_ref()
                    .ok_or_else(|| PluginError::new("C2 device sync is unavailable"))?;
                let result = device_sync
                    .pair_device(&args.url, args.device_name.as_deref())
                    .await
                    .map_err(PluginError::new)?;
                let sync = device_sync.set_enabled(true).await;
                json(serde_json::json!({ "device": result.device, "sync": sync }))
            }
        })?;

        #[derive(Deserialize)]
        struct IdArgs {
            id: String,
        }
        ctx.command("remote.revoke_device", move |args| {
            let service = runtime.clone();
            async move {
                let args: IdArgs = take_args(args)?;
                let revoked = if let Some(id) = args.id.strip_prefix("out:") {
                    match &service.device_sync {
                        Some(sync) => sync.revoke_device(id).map_err(PluginError::new)?,
                        None => false,
                    }
                } else if let Some(id) = args.id.strip_prefix("in:") {
                    service
                        .auth()
                        .try_revoke_device(id)
                        .map_err(PluginError::new)?
                } else if service
                    .auth()
                    .try_revoke_device(&args.id)
                    .map_err(PluginError::new)?
                {
                    true
                } else {
                    match &service.device_sync {
                        Some(sync) => sync.revoke_device(&args.id).map_err(PluginError::new)?,
                        None => false,
                    }
                };
                json(revoked)
            }
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{RemoteHandle, RemoteLifecycle, RemoteStart};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    struct Dropped(Arc<AtomicBool>);

    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    fn start_ticket(lifecycle: &mut RemoteLifecycle) -> u64 {
        match lifecycle.begin_start().unwrap() {
            RemoteStart::Begin(ticket) => ticket,
            RemoteStart::Running(_) => panic!("test lifecycle unexpectedly running"),
        }
    }

    #[tokio::test]
    async fn remote_start_is_single_flight_and_shutdown_rejects_late_server() {
        let mut lifecycle = RemoteLifecycle::default();
        let ticket = start_ticket(&mut lifecycle);
        assert_eq!(
            lifecycle.begin_start().unwrap_err(),
            "remote server is already starting"
        );

        lifecycle.shutdown();
        let dropped = Arc::new(AtomicBool::new(false));
        let guard = Dropped(dropped.clone());
        let task = tokio::spawn(async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        });
        tokio::task::yield_now().await;
        let late = RemoteHandle::for_test(4599, task);

        assert_eq!(
            lifecycle.finish_start(ticket, late).unwrap_err(),
            "remote plugin is unloading"
        );
        tokio::time::timeout(Duration::from_secs(1), async {
            while !dropped.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("late remote server task should be aborted");
        assert_eq!(
            lifecycle.begin_start().unwrap_err(),
            "remote plugin is unloading"
        );
    }

    #[tokio::test]
    async fn stopped_start_cannot_complete_into_a_new_start_slot() {
        let mut lifecycle = RemoteLifecycle::default();
        let first = start_ticket(&mut lifecycle);
        lifecycle.stop();
        let second = start_ticket(&mut lifecycle);
        let task = tokio::spawn(std::future::pending::<()>());
        let late = RemoteHandle::for_test(4599, task);

        assert_ne!(first, second);
        assert_eq!(
            lifecycle.finish_start(first, late).unwrap_err(),
            "remote server start was cancelled"
        );
        assert_eq!(lifecycle.starting_ticket(), Some(second));
    }
}

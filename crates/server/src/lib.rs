//! C2 remote-control server.
//!
//! Exposes the shared [`Engine`] over WebSocket so another device can drive it: clients send `Op`
//! JSON, and the server streams `Event` JSON back. The same engine can be shared with the desktop
//! app (via the broadcast sender), so remote and local see one set of sessions.
//!
//! Access is gated by protocol-bound credentials (see [`auth`]): one-time pairing tokens are
//! exchanged for per-device bearers, which buy short-lived single-use WebSocket tickets. Legacy
//! C2 browser clients use `/api/pair`, `/api/ws-ticket`, and `/ws?ticket=…`; T3 Code clients use
//! `/oauth/token`, `/api/auth/websocket-ticket`, and `/ws?wsTicket=…`. Only those single-use tickets
//! ever appear in a query string.
//!
//! Wire protocol (over the socket):
//! - client → server: a raw [`Op`] object, e.g. `{"op":"prompt","session":"…","doc":[…]}`, or a
//!   request like `{"req":"transcript","session":"…"}`.
//! - server → client: `{"kind":"sessions",…}` once on connect, `{"kind":"event",…}` per engine
//!   event, `{"kind":"transcript",…}` in reply to a transcript request, and `{"kind":"lagged",…}`
//!   if the client fell behind and events were dropped. Snapshot failures are explicit
//!   `{"kind":"sessions_error",…}` frames, never successful empty lists.

mod auth;
pub mod cli;
pub mod serve;
pub mod remote;
pub mod external_mcp;
pub mod external_mcp_stream;
pub mod host_mcp;
pub mod daemon;
pub mod t3_compat;
pub mod terminal;

pub use auth::{
    AuthRateLimit, AuthState, Device, DeviceInfo, Paired, DEFAULT_PAIRING_TTL, REMOTE_PAIRING_TTL,
    WS_TICKET_TTL,
};

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};

use axum::extract::ConnectInfo;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{any, get, get_service, post};
use axum::{extract::DefaultBodyLimit, Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

use codetwo_core::device_sync::DeviceSyncDocument;
use codetwo_core::plugins::PluginManager;
use codetwo_core::worktree::WorktreeBaseline;
use codetwo_core::{
    CanvasDraft, CanvasDraftUpdate, CanvasError, CanvasFeatureGate, CanvasFreezeInput,
    CanvasRevision, CanvasStaticAsset, DocBlock, Engine, Event, MemberId, Op, ParallelTaskCreation,
    PortableTaskHandoff, ProviderConfiguration, ProviderId, ResultContract, Session, SessionId,
    SharedTaskSnapshot, Store, StoreError, SuggestionApprovalReceipt, SuggestionId, Task,
    TaskBudget, TaskHandoffManager, TaskId, TaskStatus, TranscriptCursor, TranscriptEntry,
    DEFAULT_TRANSCRIPT_TURNS,
};

const MAX_HANDOFF_BODY_BYTES: usize = 384 * 1024 * 1024;
const MAX_DEVICE_SYNC_BODY_BYTES: usize = 64 * 1024 * 1024;
const MAX_WEB_UI_CALL_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Transport-neutral device-sync surface supplied by a host plugin.
///
/// The HTTP server only authenticates and frames requests. Snapshot ownership and merge policy
/// remain with the host's Core-backed device-sync plugin.
pub trait DeviceSyncHttp: Send + Sync + 'static {
    fn identity(&self) -> DeviceSyncIdentity;
    fn snapshot(&self) -> Result<DeviceSyncReplica, String>;
    fn write_snapshot(
        &self,
        document: &DeviceSyncDocument,
        expected_version: &str,
    ) -> Result<DeviceSyncWriteResult, String>;
}

/// Host-owned command seam for the full React renderer running in a paired browser.
///
/// The server authenticates and frames the request; the supplying host owns which commands are
/// exposed and dispatches them through the same Kernel context as its native renderer.
#[async_trait::async_trait]
pub trait WebUiCommandCaller: Send + Sync + 'static {
    async fn call(
        &self,
        device_id: &str,
        name: &str,
        args: serde_json::Value,
        project_path: Option<String>,
    ) -> Result<serde_json::Value, String>;
}

/// Shared host adapter for the paired full React renderer.
///
/// Desktop Remote and the standalone server both use this adapter so browser capability policy
/// and root/project Kernel dispatch cannot drift between launch surfaces.
pub struct KernelWebUiCommands {
    plugin_manager: Arc<PluginManager>,
}

impl KernelWebUiCommands {
    pub fn new(plugin_manager: Arc<PluginManager>) -> Self {
        Self { plugin_manager }
    }
}

fn web_ui_command_allowed(name: &str) -> bool {
    matches!(
        name,
        "providers.list"
            | "projects.list"
            | "assistant.snapshot"
            | "assistant.edit"
            | "memory.add"
            | "memory.update"
            | "memory.set_active"
            | "workspace.default_cwd"
            | "sessions.list"
            | "sessions.archived"
            | "sessions.previews"
            | "sessions.transcript"
            | "sessions.rename"
            | "sessions.set_archived"
            | "sessions.set_pinned"
            | "engine.new_session"
            | "engine.new_parallel_task"
            | "engine.close_transient_session"
            | "engine.prompt"
            | "engine.prepare_session"
            | "engine.queue"
            | "engine.steer"
            | "engine.answer_permission"
            | "engine.answer_elicitation"
            | "engine.set_permission_mode"
            | "engine.set_execution_policy"
            | "engine.set_sandbox"
            | "engine.set_model"
            | "engine.set_config_option"
            | "engine.cancel"
            // Workspace file and Git panels of a paired desktop. Every command takes the
            // session's `cwd` explicitly; scripts, GitHub and LSP stay unavailable.
            | "workspace.list_dir"
            | "workspace.list_files"
            | "workspace.read_text"
            | "workspace.read_binary"
            | "workspace.write_text"
            | "workspace.create_file"
            | "workspace.create_dir"
            | "workspace.rename"
            | "workspace.copy"
            | "workspace.delete"
            | "workspace.search"
            | "workspace.cancel_search"
            | "workspace.rules"
            | "workspace.source_control"
            | "git.status"
            | "git.diff"
            | "git.diff_stat"
            | "git.diff_since"
            | "git.stage"
            | "git.unstage"
            | "git.commit"
            | "git.push"
            | "git.revert"
            | "git.checkpoint"
            | "git.checkpoints"
            | "git.suggest_message"
    )
}

#[async_trait::async_trait]
impl WebUiCommandCaller for KernelWebUiCommands {
    async fn call(
        &self,
        _device_id: &str,
        name: &str,
        args: serde_json::Value,
        project_path: Option<String>,
    ) -> Result<serde_json::Value, String> {
        if !web_ui_command_allowed(name) {
            return Err(format!(
                "command is unavailable in the browser renderer: {name}"
            ));
        }

        if let Some(project_path) = project_path {
            self.plugin_manager
                .call_in_project(project_path, name, args)
                .await
                .map_err(|error| error.to_string())
        } else {
            self.plugin_manager
                .call(name, args)
                .await
                .map_err(|error| error.to_string())
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceSyncIdentity {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSyncReplica {
    pub id: String,
    pub document: DeviceSyncDocument,
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceSyncWriteState {
    Written,
    Conflict,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceSyncWriteResult {
    pub state: DeviceSyncWriteState,
    pub version: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Outbound {
    Sessions {
        sessions: Vec<Session>,
    },
    SessionsError {
        message: String,
    },
    Event {
        event: Event,
    },
    Transcript {
        session: SessionId,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        entries: Vec<TranscriptEntry>,
        next_before: Option<TranscriptCursor>,
        snapshot_through: Option<TranscriptCursor>,
    },
    TranscriptError {
        session: SessionId,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        message: String,
    },
    /// The event stream fell behind and `missed` events were dropped for this client. Re-request
    /// the transcript of the session you're watching to resync.
    Lagged {
        missed: u64,
    },
}

/// What a client may send besides a bare [`Op`].
#[derive(Deserialize)]
#[serde(tag = "req", rename_all = "snake_case")]
enum Req {
    /// Ask for a session's persisted transcript (replayed as one `transcript` frame).
    Transcript {
        session: SessionId,
        #[serde(default)]
        before: Option<TranscriptCursor>,
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        request_id: Option<String>,
    },
    /// Ask for a fresh session-list snapshot.
    Sessions,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Inbound {
    Op(Op),
    Req(Req),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CanvasFeatureState {
    feature: &'static str,
    enabled: bool,
    status: &'static str,
}

#[derive(Debug, Deserialize)]
struct CanvasCreateBody {
    #[serde(default)]
    title: String,
}

/// JSON requests accept the frontend's camelCase spelling and the core's canonical snake_case
/// spelling.  Conversion is explicit so scene data stays opaque and no caller can smuggle a live
/// asset URL into persistence.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CanvasAssetBody {
    id: String,
    #[serde(alias = "mime_type")]
    mime_type: String,
    width: u32,
    height: u32,
    bytes: Vec<u8>,
}

impl From<CanvasAssetBody> for CanvasStaticAsset {
    fn from(value: CanvasAssetBody) -> Self {
        Self {
            id: value.id,
            mime_type: value.mime_type,
            width: value.width,
            height: value.height,
            bytes: value.bytes,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CanvasAssetRefBody {
    id: String,
    #[serde(alias = "mime_type")]
    mime_type: String,
    width: u32,
    height: u32,
    #[serde(default, alias = "source_name")]
    source_name: Option<String>,
}

impl From<CanvasAssetRefBody> for codetwo_core::CanvasAssetRef {
    fn from(value: CanvasAssetRefBody) -> Self {
        Self {
            id: value.id,
            mime_type: value.mime_type,
            width: value.width,
            height: value.height,
            source_name: value.source_name,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CanvasEnvelopeBody {
    engine: String,
    #[serde(alias = "engine_version")]
    engine_version: String,
    #[serde(alias = "schema_version")]
    schema_version: u32,
    revision: CanvasRevision,
    theme: codetwo_core::CanvasTheme,
    #[serde(default)]
    assets: Vec<CanvasAssetRefBody>,
    scene: serde_json::Value,
}

impl From<CanvasEnvelopeBody> for codetwo_core::CanvasSceneEnvelope {
    fn from(value: CanvasEnvelopeBody) -> Self {
        Self {
            engine: value.engine,
            engine_version: value.engine_version,
            schema_version: value.schema_version,
            revision: value.revision,
            theme: value.theme,
            assets: value.assets.into_iter().map(Into::into).collect(),
            scene: value.scene,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CanvasObjectBody {
    id: String,
    kind: codetwo_core::CanvasObjectKind,
    #[serde(default, alias = "original_text")]
    original_text: String,
    bounds: codetwo_core::CanvasRect,
    layer: i64,
    #[serde(default, alias = "arrow_start")]
    arrow_start: Option<codetwo_core::CanvasPoint>,
    #[serde(default, alias = "arrow_end")]
    arrow_end: Option<codetwo_core::CanvasPoint>,
    #[serde(default, alias = "asset_id")]
    asset_id: Option<String>,
}

impl From<CanvasObjectBody> for codetwo_core::CanvasObject {
    fn from(value: CanvasObjectBody) -> Self {
        Self {
            id: value.id,
            kind: value.kind,
            original_text: value.original_text,
            bounds: value.bounds,
            layer: value.layer,
            arrow_start: value.arrow_start,
            arrow_end: value.arrow_end,
            asset_id: value.asset_id,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CanvasManifestBody {
    objects: Vec<CanvasObjectBody>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CanvasExportBody {
    id: String,
    kind: codetwo_core::CanvasExportKind,
    #[serde(default, alias = "index")]
    index: Option<u32>,
    #[serde(alias = "mime_type")]
    mime_type: String,
    width: u32,
    height: u32,
    bytes: Vec<u8>,
}

impl From<CanvasExportBody> for codetwo_core::CanvasExport {
    fn from(value: CanvasExportBody) -> Self {
        Self {
            id: value.id,
            kind: value.kind,
            index: value.index,
            mime_type: value.mime_type,
            width: value.width,
            height: value.height,
            bytes: value.bytes,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CanvasUpdateBody {
    title: String,
    theme: codetwo_core::CanvasTheme,
    envelope: CanvasEnvelopeBody,
    manifest: CanvasManifestBody,
    #[serde(default)]
    assets: Vec<CanvasAssetBody>,
    #[serde(alias = "expected_revision")]
    expected_revision: CanvasRevision,
}

impl CanvasUpdateBody {
    fn into_core(self) -> CanvasDraftUpdate {
        CanvasDraftUpdate {
            title: self.title,
            theme: self.theme,
            envelope: self.envelope.into(),
            manifest: codetwo_core::CanvasManifest::new(
                self.manifest.objects.into_iter().map(Into::into).collect(),
            ),
            assets: self.assets.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CanvasFreezeBody {
    title: String,
    theme: codetwo_core::CanvasTheme,
    envelope: CanvasEnvelopeBody,
    manifest: CanvasManifestBody,
    #[serde(default)]
    assets: Vec<CanvasAssetBody>,
    #[serde(default)]
    exports: Vec<CanvasExportBody>,
    #[serde(alias = "expected_revision")]
    expected_revision: CanvasRevision,
}

impl CanvasFreezeBody {
    fn into_core(self, now: i64) -> CanvasFreezeInput {
        CanvasFreezeInput {
            title: self.title,
            theme: self.theme,
            envelope: self.envelope.into(),
            manifest: codetwo_core::CanvasManifest::new(
                self.manifest.objects.into_iter().map(Into::into).collect(),
            ),
            assets: self.assets.into_iter().map(Into::into).collect(),
            exports: self.exports.into_iter().map(Into::into).collect(),
            now,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CanvasMediaBody {
    bytes: Vec<u8>,
    #[serde(default, alias = "declared_mime")]
    declared_mime: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CanvasBytesReply {
    id: String,
    mime_type: String,
    width: u32,
    height: u32,
    bytes: Vec<u8>,
}

impl From<CanvasStaticAsset> for CanvasBytesReply {
    fn from(value: CanvasStaticAsset) -> Self {
        Self {
            id: value.id,
            mime_type: value.mime_type,
            width: value.width,
            height: value.height,
            bytes: value.bytes,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CanvasDraftResponse {
    id: String,
    owner: String,
    revision: CanvasRevision,
    title: String,
    theme: codetwo_core::CanvasTheme,
    envelope: CanvasEnvelopeBody,
    manifest: CanvasManifestBody,
    assets: Vec<CanvasAssetBody>,
    created_at: i64,
    updated_at: i64,
    tombstoned_at: Option<i64>,
}

impl From<CanvasDraft> for CanvasDraftResponse {
    fn from(value: CanvasDraft) -> Self {
        Self {
            id: value.id,
            owner: value.owner,
            revision: value.revision,
            title: value.title,
            theme: value.theme,
            envelope: value.envelope.into(),
            manifest: value.manifest.into(),
            assets: value.assets.into_iter().map(Into::into).collect(),
            created_at: value.created_at,
            updated_at: value.updated_at,
            tombstoned_at: value.tombstoned_at,
        }
    }
}

impl From<codetwo_core::CanvasSceneEnvelope> for CanvasEnvelopeBody {
    fn from(value: codetwo_core::CanvasSceneEnvelope) -> Self {
        Self {
            engine: value.engine,
            engine_version: value.engine_version,
            schema_version: value.schema_version,
            revision: value.revision,
            theme: value.theme,
            assets: value.assets.into_iter().map(Into::into).collect(),
            scene: value.scene,
        }
    }
}

impl From<codetwo_core::CanvasAssetRef> for CanvasAssetRefBody {
    fn from(value: codetwo_core::CanvasAssetRef) -> Self {
        Self {
            id: value.id,
            mime_type: value.mime_type,
            width: value.width,
            height: value.height,
            source_name: value.source_name,
        }
    }
}

impl From<CanvasStaticAsset> for CanvasAssetBody {
    fn from(value: CanvasStaticAsset) -> Self {
        Self {
            id: value.id,
            mime_type: value.mime_type,
            width: value.width,
            height: value.height,
            bytes: value.bytes,
        }
    }
}

impl From<codetwo_core::CanvasObject> for CanvasObjectBody {
    fn from(value: codetwo_core::CanvasObject) -> Self {
        Self {
            id: value.id,
            kind: value.kind,
            original_text: value.original_text,
            bounds: value.bounds,
            layer: value.layer,
            arrow_start: value.arrow_start,
            arrow_end: value.arrow_end,
            asset_id: value.asset_id,
        }
    }
}

impl From<codetwo_core::CanvasManifest> for CanvasManifestBody {
    fn from(value: codetwo_core::CanvasManifest) -> Self {
        Self {
            objects: value.objects.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<codetwo_core::CanvasExport> for CanvasExportBody {
    fn from(value: codetwo_core::CanvasExport) -> Self {
        Self {
            id: value.id,
            kind: value.kind,
            index: value.index,
            mime_type: value.mime_type,
            width: value.width,
            height: value.height,
            bytes: value.bytes,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CanvasSnapshotResponse {
    id: String,
    revision: CanvasRevision,
    title: String,
    theme: codetwo_core::CanvasTheme,
    created_at: i64,
    frozen_at: i64,
    object_count: usize,
    envelope: CanvasEnvelopeBody,
    manifest: CanvasManifestBody,
    assets: Vec<CanvasAssetBody>,
    summary: String,
    exports: Vec<CanvasExportBody>,
}

impl From<codetwo_core::CanvasSnapshot> for CanvasSnapshotResponse {
    fn from(value: codetwo_core::CanvasSnapshot) -> Self {
        Self {
            id: value.id,
            revision: value.revision,
            title: value.title,
            theme: value.theme,
            created_at: value.created_at,
            frozen_at: value.frozen_at,
            object_count: value.object_count,
            envelope: value.envelope.into(),
            manifest: value.manifest.into(),
            assets: value.assets.into_iter().map(Into::into).collect(),
            summary: value.summary,
            exports: value.exports.into_iter().map(Into::into).collect(),
        }
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

struct ServerState {
    engine: Arc<Engine>,
    events: broadcast::Sender<Event>,
    auth: Arc<AuthState>,
    store: Arc<Store>,
    canvas_gate: CanvasFeatureGate,
    t3: Arc<t3_compat::T3CompatState>,
    terminals: Arc<terminal::TerminalRegistry>,
    handoff: Arc<TaskHandoffManager>,
    device_sync: Option<Arc<dyn DeviceSyncHttp>>,
    web_ui_commands: Option<Arc<dyn WebUiCommandCaller>>,
}

/// Forward the engine's single event receiver into a broadcast channel so multiple clients (and the
/// desktop) can each subscribe. Returns the broadcast sender.
pub fn fanout(mut rx: mpsc::UnboundedReceiver<Event>) -> broadcast::Sender<Event> {
    let (tx, _) = broadcast::channel::<Event>(1024);
    let out = tx.clone();
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let _ = out.send(ev);
        }
    });
    tx
}

/// Bind the server to `addr` (use port 0 for an ephemeral port) and start serving. Returns the bound
/// address and the serving task handle. `auth` carries pairing/session state; share one instance
/// with the host app so it can mint pairing tokens and manage devices while the server runs.
pub async fn bind_and_serve(
    engine: Arc<Engine>,
    events: broadcast::Sender<Event>,
    addr: SocketAddr,
    auth: Arc<AuthState>,
) -> std::io::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
    let store = Arc::new(Store::open_in_memory().expect("in-memory canvas store"));
    bind_and_serve_with_canvas(
        engine,
        events,
        addr,
        auth,
        store,
        CanvasFeatureGate::default(),
    )
    .await
}

/// Bind the server with the same Store and Canvas gate owned by the host runtime.  The gate is
/// closed for every normal process; only trusted tests may inject `enabled_for_tests()`.
pub async fn bind_and_serve_with_canvas(
    engine: Arc<Engine>,
    events: broadcast::Sender<Event>,
    addr: SocketAddr,
    auth: Arc<AuthState>,
    store: Arc<Store>,
    canvas_gate: CanvasFeatureGate,
) -> std::io::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
    bind_and_serve_with_services(engine, events, addr, auth, store, canvas_gate, None).await
}

/// Bind the server with optional host-owned services. Desktop uses this entry point so the Remote
/// plugin can expose device sync without moving that business capability into the server crate.
pub async fn bind_and_serve_with_services(
    engine: Arc<Engine>,
    events: broadcast::Sender<Event>,
    addr: SocketAddr,
    auth: Arc<AuthState>,
    store: Arc<Store>,
    canvas_gate: CanvasFeatureGate,
    device_sync: Option<Arc<dyn DeviceSyncHttp>>,
) -> std::io::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
    bind_and_serve_with_web_ui(
        engine,
        events,
        addr,
        auth,
        store,
        canvas_gate,
        device_sync,
        None,
        None,
    )
    .await
}

/// Bind the paired server with the host's optional full-renderer command adapter.
///
/// The compact remote protocol remains available without this adapter. Supplying commands adds the
/// authenticated generic command route. Supplying a Web asset directory also replaces the compact
/// root with the existing React SPA while leaving protocol, terminal, and Canvas routes intact.
pub async fn bind_and_serve_with_web_ui(
    engine: Arc<Engine>,
    events: broadcast::Sender<Event>,
    addr: SocketAddr,
    auth: Arc<AuthState>,
    store: Arc<Store>,
    canvas_gate: CanvasFeatureGate,
    device_sync: Option<Arc<dyn DeviceSyncHttp>>,
    web_ui_commands: Option<Arc<dyn WebUiCommandCaller>>,
    web_ui_dir: Option<PathBuf>,
) -> std::io::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
    let t3 = Arc::new(
        t3_compat::T3CompatState::new(engine.clone(), events.clone(), auth.clone())
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
    );
    let handoff = Arc::new(
        TaskHandoffManager::new(store.clone(), engine.clone(), Some(events.clone()))
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
    );
    let state = Arc::new(ServerState {
        engine,
        events,
        auth,
        store,
        canvas_gate,
        t3: t3.clone(),
        terminals: Arc::new(terminal::TerminalRegistry::default()),
        handoff,
        device_sync,
        web_ui_commands,
    });
    let handoff_routes = Router::new()
        .route("/api/codetwo/handoffs", post(accept_handoff))
        .route("/api/codetwo/handoffs/:id/activate", post(activate_handoff))
        .route("/api/codetwo/handoffs/:id/rollback", post(rollback_handoff))
        .with_state(state.clone())
        .layer(DefaultBodyLimit::max(MAX_HANDOFF_BODY_BYTES));
    let device_sync_pair_route = Router::new()
        .route("/api/device-sync/v1/pair", post(pair_c2_device))
        .with_state(state.clone())
        .layer(DefaultBodyLimit::max(16 * 1024));
    let device_sync_snapshot_routes = Router::new()
        .route(
            "/api/device-sync/v1/snapshot",
            get(device_sync_snapshot).put(write_device_sync_snapshot),
        )
        .with_state(state.clone())
        .layer(DefaultBodyLimit::max(MAX_DEVICE_SYNC_BODY_BYTES));
    let web_ui_routes = Router::new()
        .route("/api/web-ui/call", post(web_ui_call))
        .with_state(state.clone())
        .layer(DefaultBodyLimit::max(MAX_WEB_UI_CALL_BODY_BYTES));
    let external_mcp_routes = Router::new()
        .route(
            "/external-mcp",
            post(external_mcp::post_external_mcp)
                .get(external_mcp_stream::get_external_mcp_stream)
                .with_state(state.engine.clone()),
        )
        .layer(DefaultBodyLimit::max(external_mcp::MAX_EXTERNAL_MCP_BODY_BYTES));
    let host_mcp_routes = Router::new()
        .route(
            "/mcp",
            post(host_mcp::post_host_mcp).with_state(state.engine.clone()),
        )
        .layer(DefaultBodyLimit::max(host_mcp::MAX_HOST_MCP_BODY_BYTES));
    let app = Router::new()
        .route("/terminal", get(index))
        .route("/health", get(|| async { "ok" }))
        .route("/api/pair", post(pair))
        .route("/api/ws-ticket", post(ws_ticket))
        .route("/api/devices", get(list_paired_devices))
        .route("/api/devices/:id/revoke", post(revoke_paired_device))
        .route("/api/team/v1/workspace", get(team_workspace))
        .route("/api/team/v1/attention", get(team_attention))
        .route("/api/team/v1/tasks", get(team_tasks).post(team_create_task))
        .route("/api/team/v1/tasks/:id", get(team_task))
        .route("/api/team/v1/tasks/:id/comments", post(team_add_comment))
        .route(
            "/api/team/v1/tasks/:id/suggestions",
            post(team_create_suggestion),
        )
        .route(
            "/api/team/v1/tasks/:id/suggestions/:suggestion_id/approve",
            post(team_approve_suggestion),
        )
        .route("/api/artifacts/:id", get(artifact_download))
        .route("/api/terminals", get(list_terminals))
        .route("/api/terminals/:id/kill", post(kill_terminal))
        .route("/term/*path", get(term_asset))
        .route("/ws/terminal", any(terminal_ws_handler))
        .route("/api/canvas/feature", get(canvas_feature))
        .route("/api/canvas/media/normalize", post(canvas_normalize_media))
        .route("/api/canvas/drafts", post(canvas_create_draft))
        .route(
            "/api/canvas/drafts/:id",
            get(canvas_get_draft).put(canvas_update_draft),
        )
        .route(
            "/api/canvas/drafts/:id/normalize",
            post(canvas_normalize_draft_media),
        )
        .route("/api/canvas/drafts/:id/freeze", post(canvas_freeze_draft))
        .route("/api/canvas/drafts/:id/tombstone", post(canvas_tombstone))
        .route("/api/canvas/drafts/:id/restore", post(canvas_restore))
        .route("/api/canvas/drafts/:id/purge", post(canvas_purge))
        .route(
            "/api/canvas/:id/revisions/:revision",
            get(canvas_get_snapshot),
        )
        .route(
            "/api/canvas/:id/revisions/:revision/duplicate",
            post(canvas_duplicate),
        )
        .route(
            "/api/canvas/:id/revisions/:revision/assets/:asset_id",
            get(canvas_get_asset),
        )
        .route(
            "/api/canvas/:id/revisions/:revision/exports/:export_id",
            get(canvas_get_export),
        )
        // `/canvas` is the same vanilla Remote shell as `/`; the Canvas island itself is still
        // fetched lazily from the stable `/canvas/canvas-island.js` route after the feature gate
        // check and an explicit user invocation.
        .route("/canvas", get(canvas_page))
        .route("/canvas/", get(canvas_page))
        // Stable aliases keep the shell free of Vite's hashed filenames. The generated manifest
        // remains the source of truth for the actual embedded asset bytes.
        .route("/canvas/canvas-island.js", get(canvas_entry))
        .route("/canvas/canvas.css", get(canvas_css))
        .route("/canvas/styles.css", get(canvas_css))
        // Axum 0.7/matchit 0.7 uses `/*path` for a safe terminal catch-all.
        .route("/canvas/*path", get(canvas_asset))
        .route("/ws", any(ws_handler))
        .with_state(state.clone())
        .merge(t3_compat::router(t3))
        .layer(DefaultBodyLimit::max(
            codetwo_core::canvas::MAX_CANVAS_TOTAL_BYTES + 4_000_000,
        ))
        .merge(handoff_routes)
        .merge(device_sync_pair_route)
        .merge(device_sync_snapshot_routes)
        .merge(web_ui_routes)
        .merge(host_mcp_routes)
        .merge(external_mcp_routes);
    let app = if let Some(web_ui_dir) = web_ui_dir {
        let index = web_ui_dir.join("index.html");
        let spa_index = ServeFile::new(index.clone());
        app.route("/", get_service(spa_index.clone()))
            .route("/pair", get_service(spa_index))
            .fallback_service(ServeDir::new(web_ui_dir).not_found_service(ServeFile::new(index)))
    } else {
        app.route("/", get(index)).route("/pair", get(index))
    }
    .layer(axum::middleware::map_response(no_store_headers))
    .layer(remote_client_cors())
    .layer(axum::middleware::from_fn(remote::remote_access_guard));

    let listener = TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    state.engine.set_host_mcp_http_endpoint(Some(format!(
        "http://127.0.0.1:{}/mcp",
        local.port()
    )));
    let handle = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    Ok((local, handle))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WebUiCallBody {
    name: String,
    #[serde(default)]
    args: serde_json::Value,
    #[serde(default)]
    project_path: Option<String>,
}

async fn web_ui_call(
    State(st): State<Arc<ServerState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<WebUiCallBody>,
) -> Response {
    let Some(commands) = &st.web_ui_commands else {
        return (StatusCode::NOT_FOUND, "C2 Web UI commands are unavailable").into_response();
    };
    let device_id = match require_device(&st, &headers, Some(peer)) {
        Ok(device_id) => device_id,
        Err(response) => return response,
    };
    if body.name.is_empty() || body.name.len() > 160 {
        return (StatusCode::BAD_REQUEST, "C2 Web UI command name is invalid").into_response();
    }
    match commands
        .call(&device_id, &body.name, body.args, body.project_path)
        .await
    {
        Ok(result) => Json(serde_json::json!({ "result": result })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": error })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct PairC2DeviceBody {
    token: String,
    device_name: String,
    device_id: String,
}

#[derive(Serialize)]
struct PairC2DeviceReply {
    server_id: String,
    server_name: String,
    device_id: String,
    bearer: String,
}

async fn pair_c2_device(
    State(st): State<Arc<ServerState>>,
    Json(body): Json<PairC2DeviceBody>,
) -> Response {
    let Some(device_sync) = &st.device_sync else {
        return (StatusCode::NOT_FOUND, "C2 device sync is unavailable").into_response();
    };
    let paired = match st
        .auth
        .try_pair_c2(&body.token, &body.device_name, &body.device_id)
    {
        Ok(Some(paired)) => paired,
        Ok(None) => {
            return (StatusCode::UNAUTHORIZED, "invalid or expired pairing token").into_response()
        }
        Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, error).into_response(),
    };
    let identity = device_sync.identity();
    Json(PairC2DeviceReply {
        server_id: identity.id,
        server_name: identity.name,
        device_id: paired.device_id,
        bearer: paired.bearer,
    })
    .into_response()
}

fn require_c2_device(st: &ServerState, headers: &HeaderMap) -> Result<String, Response> {
    bearer_from(headers)
        .and_then(|bearer| st.auth.authorize_c2_bearer(bearer))
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                "invalid or revoked device credential",
            )
                .into_response()
        })
}

#[derive(Serialize)]
struct DeviceSyncSnapshotReply {
    replica: DeviceSyncReplica,
}

async fn device_sync_snapshot(State(st): State<Arc<ServerState>>, headers: HeaderMap) -> Response {
    let Some(device_sync) = &st.device_sync else {
        return (StatusCode::NOT_FOUND, "C2 device sync is unavailable").into_response();
    };
    if let Err(response) = require_c2_device(&st, &headers) {
        return response;
    }
    match device_sync.snapshot() {
        Ok(replica) => Json(DeviceSyncSnapshotReply { replica }).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error).into_response(),
    }
}

#[derive(Deserialize)]
struct WriteDeviceSyncSnapshotBody {
    document: DeviceSyncDocument,
    expected_version: String,
}

async fn write_device_sync_snapshot(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(body): Json<WriteDeviceSyncSnapshotBody>,
) -> Response {
    let Some(device_sync) = &st.device_sync else {
        return (StatusCode::NOT_FOUND, "C2 device sync is unavailable").into_response();
    };
    if let Err(response) = require_c2_device(&st, &headers) {
        return response;
    }
    match device_sync.write_snapshot(&body.document, &body.expected_version) {
        Ok(result) => {
            let status = if result.state == DeviceSyncWriteState::Conflict {
                StatusCode::CONFLICT
            } else {
                StatusCode::OK
            };
            (status, Json(result)).into_response()
        }
        Err(error) => (StatusCode::BAD_REQUEST, error).into_response(),
    }
}

/// Cross-origin policy for another C2 client (the desktop renderer's own origin is not this
/// server's). Every credential travels in the `Authorization` header or a single-use ticket,
/// never in a cookie, so a foreign page can neither read nor drive an endpoint without already
/// holding a bearer. Allowing any origin therefore adds no ambient authority.
fn remote_client_cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
        .max_age(std::time::Duration::from_secs(600))
}

async fn no_store_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store, max-age=0"),
    );
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

fn auth_rate_limit_response(limit: auth::AuthRateLimit) -> Response {
    let mut response = (
        StatusCode::TOO_MANY_REQUESTS,
        "too many authentication attempts",
    )
        .into_response();
    if let Ok(value) = HeaderValue::from_str(&limit.retry_after_secs.to_string()) {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, value);
    }
    response
}

fn require_device(
    st: &ServerState,
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
) -> Result<String, Response> {
    let limit_key = peer.map(|peer| remote::auth_limiter_source_key(headers, peer));
    if let Some(ref key) = limit_key {
        if let Err(limit) = st.auth.check_auth_rate_limit(key) {
            return Err(auth_rate_limit_response(limit));
        }
    }
    let Some(device_id) = bearer_from(headers).and_then(|bearer| st.auth.authorize_bearer(bearer))
    else {
        if let Some(key) = limit_key {
            if let Some(limit) = st.auth.record_auth_failure(&key) {
                return Err(auth_rate_limit_response(limit));
            }
        }
        return Err((StatusCode::UNAUTHORIZED, "invalid bearer").into_response());
    };
    if let Some(key) = limit_key {
        st.auth.record_auth_success(&key);
    }
    if st.auth.member_for_device(&device_id).is_some() {
        return Err((
            StatusCode::FORBIDDEN,
            "team members must use the shared Task surface",
        )
            .into_response());
    }
    Ok(device_id)
}

fn require_t3_scope(
    st: &ServerState,
    headers: &HeaderMap,
    required_scope: &str,
) -> Result<auth::BearerAuthorization, Response> {
    let authorization = bearer_from(headers)
        .and_then(|bearer| st.auth.authorize_bearer_profile(bearer))
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, "invalid bearer").into_response())?;
    if !authorization
        .scopes
        .iter()
        .any(|scope| scope == required_scope)
    {
        return Err((StatusCode::FORBIDDEN, "missing required scope").into_response());
    }
    Ok(authorization)
}

#[derive(Deserialize)]
struct AcceptHandoffBody {
    handoff: PortableTaskHandoff,
    destination: String,
}

async fn accept_handoff(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(body): Json<AcceptHandoffBody>,
) -> Response {
    if let Err(response) = require_t3_scope(&st, &headers, "orchestration:operate") {
        return response;
    }
    match st.handoff.accept(&body.handoff, &body.destination) {
        Ok(result) => Json(result).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error).into_response(),
    }
}

#[derive(Deserialize)]
struct ActivateHandoffBody {
    session: String,
    epoch: u64,
}

async fn activate_handoff(
    State(st): State<Arc<ServerState>>,
    Path(handoff_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ActivateHandoffBody>,
) -> Response {
    let authorization = match require_t3_scope(&st, &headers, "orchestration:operate") {
        Ok(authorization) => authorization,
        Err(response) => return response,
    };
    match st.handoff.activate(&body.session, &handoff_id, body.epoch) {
        Ok(()) => {
            if authorization.ephemeral_handoff {
                if let Err(error) = st.auth.try_revoke_device(&authorization.device_id) {
                    tracing::warn!("revoke consumed handoff credential failed: {error}");
                }
            }
            Json(serde_json::json!({ "state": "active" })).into_response()
        }
        Err(error) => (StatusCode::CONFLICT, error).into_response(),
    }
}

#[derive(Deserialize)]
struct RollbackHandoffBody {
    session: String,
    epoch: u64,
    destination: String,
}

async fn rollback_handoff(
    State(st): State<Arc<ServerState>>,
    Path(handoff_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<RollbackHandoffBody>,
) -> Response {
    if let Err(response) = require_t3_scope(&st, &headers, "orchestration:operate") {
        return response;
    }
    match st
        .handoff
        .rollback_target(&body.session, &handoff_id, body.epoch, &body.destination)
    {
        Ok(()) => Json(serde_json::json!({ "state": "rolled_back" })).into_response(),
        Err(error) => (StatusCode::CONFLICT, error).into_response(),
    }
}

fn canvas_error(error: CanvasError) -> Response {
    let status = match &error {
        CanvasError::GateDisabled => StatusCode::FORBIDDEN,
        CanvasError::OwnerMismatch | CanvasError::NotFound(_) => StatusCode::NOT_FOUND,
        CanvasError::StaleRevision { .. } => StatusCode::CONFLICT,
        CanvasError::Tombstoned(_) | CanvasError::Immutable(_) => StatusCode::CONFLICT,
        CanvasError::InvalidEnvelope(_)
        | CanvasError::InvalidManifest(_)
        | CanvasError::InvalidAssets(_)
        | CanvasError::InvalidExports(_)
        | CanvasError::ExportOverBudget(_)
        | CanvasError::InvalidGeometry(_)
        | CanvasError::UnsafeMedia(_)
        | CanvasError::ProviderImageUnsupported { .. } => StatusCode::BAD_REQUEST,
        CanvasError::Sqlite(_) | CanvasError::Serde(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    // Mutable owner mismatches deliberately share the same body as an unknown id.  A 404 alone
    // is not enough if the text says "owner mismatch" and therefore confirms another device's
    // draft exists.
    let message = match &error {
        CanvasError::OwnerMismatch | CanvasError::NotFound(_) => "canvas not found".to_string(),
        _ => error.to_string(),
    };
    (status, message).into_response()
}

fn parse_revision(value: &str) -> Result<CanvasRevision, Response> {
    value
        .parse::<CanvasRevision>()
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid canvas revision").into_response())
}

async fn canvas_feature(State(st): State<Arc<ServerState>>, headers: HeaderMap) -> Response {
    if let Err(response) = require_device(&st, &headers, None) {
        return response;
    }
    Json(CanvasFeatureState {
        feature: codetwo_core::CANVAS_FEATURE_GATE,
        enabled: st.canvas_gate.is_enabled(),
        status: "not production-enabled",
    })
    .into_response()
}

async fn canvas_create_draft(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(body): Json<CanvasCreateBody>,
) -> Response {
    let owner = match require_device(&st, &headers, None) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    match st
        .store
        .create_canvas_draft_with_gate(st.canvas_gate, &owner, &body.title, now_millis())
    {
        Ok(draft) => Json(CanvasDraftResponse::from(draft)).into_response(),
        Err(error) => canvas_error(error),
    }
}

async fn canvas_get_draft(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let owner = match require_device(&st, &headers, None) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    match st.store.get_canvas_draft(&id, &owner) {
        Ok(Some(draft)) => Json(CanvasDraftResponse::from(draft)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "canvas draft not found").into_response(),
        Err(error) => canvas_error(error),
    }
}

async fn canvas_update_draft(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<CanvasUpdateBody>,
) -> Response {
    let owner = match require_device(&st, &headers, None) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    match st.store.update_canvas_draft_cas(
        &id,
        &owner,
        body.expected_revision,
        body.into_core(),
        now_millis(),
    ) {
        Ok(draft) => Json(CanvasDraftResponse::from(draft)).into_response(),
        Err(error) => canvas_error(error),
    }
}

async fn canvas_normalize_media(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(body): Json<CanvasMediaBody>,
) -> Response {
    if let Err(response) = require_device(&st, &headers, None) {
        return response;
    }
    if let Err(error) = st.canvas_gate.require() {
        return canvas_error(error);
    }
    match codetwo_core::normalize_media(&body.bytes, body.declared_mime.as_deref()) {
        Ok(asset) => Json(CanvasBytesReply::from(asset)).into_response(),
        Err(error) => canvas_error(error),
    }
}

async fn canvas_normalize_draft_media(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<CanvasMediaBody>,
) -> Response {
    let owner = match require_device(&st, &headers, None) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    if let Err(error) = st.canvas_gate.require() {
        return canvas_error(error);
    }
    match st.store.get_canvas_draft(&id, &owner) {
        Ok(Some(_)) => {
            match codetwo_core::normalize_media(&body.bytes, body.declared_mime.as_deref()) {
                Ok(asset) => Json(CanvasBytesReply::from(asset)).into_response(),
                Err(error) => canvas_error(error),
            }
        }
        Ok(None) => (StatusCode::NOT_FOUND, "canvas draft not found").into_response(),
        Err(error) => canvas_error(error),
    }
}

async fn canvas_freeze_draft(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<CanvasFreezeBody>,
) -> Response {
    let owner = match require_device(&st, &headers, None) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    match st.store.freeze_canvas_with_gate(
        st.canvas_gate,
        &id,
        &owner,
        body.expected_revision,
        body.into_core(now_millis()),
    ) {
        Ok(snapshot) => Json(CanvasSnapshotResponse::from(snapshot)).into_response(),
        Err(error) => canvas_error(error),
    }
}

async fn canvas_get_snapshot(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path((id, revision)): Path<(String, String)>,
) -> Response {
    if let Err(response) = require_device(&st, &headers, None) {
        return response;
    }
    let revision = match parse_revision(&revision) {
        Ok(revision) => revision,
        Err(response) => return response,
    };
    match st.store.get_canvas_snapshot_frozen(&id, revision) {
        Ok(Some(snapshot)) => Json(CanvasSnapshotResponse::from(snapshot)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "canvas snapshot not found").into_response(),
        Err(error) => canvas_error(error),
    }
}

/// Download one stored artifact by its opaque id. Device-authenticated like the canvas byte routes;
/// the filename is sanitized so a provider-chosen display name cannot inject headers.
async fn artifact_download(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = require_device(&st, &headers, None) {
        return response;
    }
    let Some(artifacts) = codetwo_core::ArtifactStore::from_store(st.store.clone()) else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "artifact storage is unavailable",
        )
            .into_response();
    };
    let reference = match artifacts.metadata(&id) {
        Ok(reference) => reference,
        Err(codetwo_core::artifact::ArtifactError::NotFound) => {
            return (StatusCode::NOT_FOUND, "artifact not found").into_response()
        }
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "artifact unavailable").into_response()
        }
    };
    let bytes = match artifacts.get(&id) {
        Ok(bytes) => bytes,
        Err(codetwo_core::artifact::ArtifactError::NotFound) => {
            return (StatusCode::NOT_FOUND, "artifact not found").into_response()
        }
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "artifact unavailable").into_response()
        }
    };
    let mime = HeaderValue::from_str(&reference.mime_type)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    let mut response = bytes.into_response();
    response.headers_mut().insert(header::CONTENT_TYPE, mime);
    if let Ok(disposition) = HeaderValue::from_str(&format!(
        "attachment; filename=\"{}\"",
        artifact_download_filename(&reference.display_name)
    )) {
        response
            .headers_mut()
            .insert(header::CONTENT_DISPOSITION, disposition);
    }
    response
}

/// Keep only ASCII alphanumerics and `.`, `-`, `_`; every other character (quotes, slashes,
/// newlines, non-ASCII) becomes `_`. The stem is capped and a name is always produced.
fn artifact_download_filename(display_name: &str) -> String {
    let mut cleaned: String = display_name
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() || matches!(value, '.' | '-' | '_') {
                value
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.len() > 80 {
        cleaned.truncate(80);
    }
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "artifact.bin".to_string()
    } else {
        cleaned
    }
}

async fn canvas_get_asset(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path((id, revision, asset_id)): Path<(String, String, String)>,
) -> Response {
    if let Err(response) = require_device(&st, &headers, None) {
        return response;
    }
    let revision = match parse_revision(&revision) {
        Ok(revision) => revision,
        Err(response) => return response,
    };
    match st.store.get_canvas_snapshot_frozen(&id, revision) {
        Ok(Some(snapshot)) => match snapshot
            .assets
            .into_iter()
            .find(|asset| asset.id == asset_id)
        {
            Some(asset) => {
                let mime = HeaderValue::from_str(&asset.mime_type)
                    .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
                let mut response = asset.bytes.into_response();
                response.headers_mut().insert(header::CONTENT_TYPE, mime);
                response
            }
            None => (StatusCode::NOT_FOUND, "canvas asset not found").into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "canvas snapshot not found").into_response(),
        Err(error) => canvas_error(error),
    }
}

async fn canvas_get_export(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path((id, revision, export_id)): Path<(String, String, String)>,
) -> Response {
    if let Err(response) = require_device(&st, &headers, None) {
        return response;
    }
    let revision = match parse_revision(&revision) {
        Ok(revision) => revision,
        Err(response) => return response,
    };
    match st.store.get_canvas_snapshot_frozen(&id, revision) {
        Ok(Some(snapshot)) => match snapshot
            .exports
            .into_iter()
            .find(|export| export.id == export_id)
        {
            Some(export) => {
                let mut response = export.bytes.into_response();
                response
                    .headers_mut()
                    .insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
                response
            }
            None => (StatusCode::NOT_FOUND, "canvas export not found").into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "canvas snapshot not found").into_response(),
        Err(error) => canvas_error(error),
    }
}

async fn canvas_duplicate(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path((id, revision)): Path<(String, String)>,
) -> Response {
    let owner = match require_device(&st, &headers, None) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    let revision = match parse_revision(&revision) {
        Ok(revision) => revision,
        Err(response) => return response,
    };
    match st.store.duplicate_canvas_to_owner_with_gate(
        st.canvas_gate,
        &id,
        revision,
        &owner,
        now_millis(),
    ) {
        Ok(draft) => Json(CanvasDraftResponse::from(draft)).into_response(),
        Err(error) => canvas_error(error),
    }
}

async fn canvas_tombstone(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let owner = match require_device(&st, &headers, None) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    match st.store.tombstone_canvas(&id, &owner, now_millis()) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => canvas_error(error),
    }
}

async fn canvas_restore(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let owner = match require_device(&st, &headers, None) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    match st.store.restore_canvas(&id, &owner, now_millis()) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => canvas_error(error),
    }
}

async fn canvas_purge(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let owner = match require_device(&st, &headers, None) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    match st.store.purge_canvas(&id, &owner, now_millis()) {
        Ok(purged) => Json(serde_json::json!({ "purged": purged })).into_response(),
        Err(error) => canvas_error(error),
    }
}

#[derive(Deserialize)]
struct PairBody {
    token: String,
    #[serde(default)]
    device_name: String,
}

/// Exchange a one-time pairing token for a per-device bearer.
async fn pair(
    State(st): State<Arc<ServerState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<PairBody>,
) -> Response {
    let key = remote::auth_limiter_source_key(&headers, peer);
    if let Err(limit) = st.auth.check_auth_rate_limit(&key) {
        return auth_rate_limit_response(limit);
    }
    match st.auth.try_pair(&body.token, &body.device_name) {
        Ok(Some(paired)) => {
            st.auth.record_auth_success(&key);
            Json(paired).into_response()
        }
        Ok(None) => {
            if let Some(limit) = st.auth.record_auth_failure(&key) {
                return auth_rate_limit_response(limit);
            }
            (StatusCode::UNAUTHORIZED, "invalid or expired pairing token").into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("could not persist the paired device: {error}"),
        )
            .into_response(),
    }
}

#[derive(Serialize)]
struct TeamWorkspaceReply {
    workspace: codetwo_core::Workspace,
    me: codetwo_core::Member,
    members: Vec<codetwo_core::Member>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TeamCreateTaskBody {
    task_id: String,
    goal: String,
    cwd: String,
    provider: ProviderId,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    collaborator_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TeamTextMutationBody {
    expected_revision: u64,
    body: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TeamApproveSuggestionBody {
    command_id: String,
    expected_revision: u64,
    #[serde(default)]
    worktree_base_sha: Option<String>,
}

#[derive(Serialize)]
struct TeamApproveSuggestionReply {
    receipt: SuggestionApprovalReceipt,
    snapshot: SharedTaskSnapshot,
}

fn team_actor(st: &ServerState, headers: &HeaderMap) -> Result<MemberId, Response> {
    let Some(authorization) =
        bearer_from(headers).and_then(|bearer| st.auth.authorize_member_bearer(bearer))
    else {
        return Err((StatusCode::UNAUTHORIZED, "member-bound bearer required").into_response());
    };
    let member_id = MemberId::new(authorization.member_id);
    match st.store.member(&member_id) {
        Ok(Some(member)) if member.active => Ok(member_id),
        Ok(_) => Err((StatusCode::FORBIDDEN, "Member is inactive or unknown").into_response()),
        Err(error) => Err(team_store_error(error)),
    }
}

fn team_store_error(error: StoreError) -> Response {
    let status = match error {
        StoreError::TaskNotFound { .. } | StoreError::SuggestionNotFound { .. } => {
            StatusCode::NOT_FOUND
        }
        StoreError::MemberUnauthorized { .. } => StatusCode::FORBIDDEN,
        StoreError::TaskRevisionConflict { .. }
        | StoreError::CommandReceiptConflict { .. }
        | StoreError::CollaborationConflict(_)
        | StoreError::TaskExecutorBusy { .. } => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(serde_json::json!({ "error": error.to_string() })),
    )
        .into_response()
}

fn publish_task_snapshot(st: &ServerState, task_id: &TaskId, revision: u64) {
    let _ = st.events.send(Event::TaskSnapshotChanged {
        session: None,
        task_id: task_id.clone(),
        revision,
    });
}

async fn team_workspace(State(st): State<Arc<ServerState>>, headers: HeaderMap) -> Response {
    let actor_id = match team_actor(&st, &headers) {
        Ok(actor_id) => actor_id,
        Err(response) => return response,
    };
    let workspace = match st.store.workspace() {
        Ok(Some(workspace)) => workspace,
        Ok(None) => return (StatusCode::NOT_FOUND, "Workspace is not configured").into_response(),
        Err(error) => return team_store_error(error),
    };
    let me = match st.store.member(&actor_id) {
        Ok(Some(member)) => member,
        Ok(None) => return (StatusCode::FORBIDDEN, "Member is unknown").into_response(),
        Err(error) => return team_store_error(error),
    };
    let members = match st.store.list_members() {
        Ok(members) => members,
        Err(error) => return team_store_error(error),
    };
    Json(TeamWorkspaceReply {
        workspace,
        me,
        members,
    })
    .into_response()
}

async fn team_tasks(State(st): State<Arc<ServerState>>, headers: HeaderMap) -> Response {
    let actor_id = match team_actor(&st, &headers) {
        Ok(actor_id) => actor_id,
        Err(response) => return response,
    };
    match st.store.list_shared_tasks(&actor_id) {
        Ok(tasks) => Json(tasks).into_response(),
        Err(error) => team_store_error(error),
    }
}

async fn team_attention(State(st): State<Arc<ServerState>>, headers: HeaderMap) -> Response {
    let actor_id = match team_actor(&st, &headers) {
        Ok(actor_id) => actor_id,
        Err(response) => return response,
    };
    match st.store.list_attention_items(&actor_id) {
        Ok(items) => Json(items).into_response(),
        Err(error) => team_store_error(error),
    }
}

async fn team_task(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let actor_id = match team_actor(&st, &headers) {
        Ok(actor_id) => actor_id,
        Err(response) => return response,
    };
    match st.store.shared_task_snapshot(&TaskId::new(id), &actor_id) {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(error) => team_store_error(error),
    }
}

async fn team_create_task(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(body): Json<TeamCreateTaskBody>,
) -> Response {
    let actor_id = match team_actor(&st, &headers) {
        Ok(actor_id) => actor_id,
        Err(response) => return response,
    };
    let workspace = match st.store.workspace() {
        Ok(Some(workspace)) => workspace,
        Ok(None) => return (StatusCode::CONFLICT, "Workspace is not configured").into_response(),
        Err(error) => return team_store_error(error),
    };
    let task_id = TaskId::new(body.task_id);
    let task = Task {
        id: task_id.clone(),
        status: TaskStatus::Active,
        result_contract: ResultContract {
            goal: body.goal.trim().to_string(),
            required_deliverables: Vec::new(),
            completion_conditions: Vec::new(),
            boundaries: Vec::new(),
            known_risks: Vec::new(),
            unresolved_facts: Vec::new(),
        },
        provider_configuration: ProviderConfiguration {
            provider: body.provider,
            model: body.model,
            reasoning_effort: body.reasoning_effort,
        },
        budget: TaskBudget {
            max_cost_microusd: None,
            max_tokens: None,
            max_duration_seconds: None,
        },
    };
    let collaborators = body
        .collaborator_ids
        .into_iter()
        .map(MemberId::new)
        .collect::<Vec<_>>();
    match st.store.create_shared_task(
        &task,
        &workspace.id,
        &actor_id,
        &collaborators,
        &body.cwd,
        now_millis(),
    ) {
        Ok(snapshot) => {
            publish_task_snapshot(&st, &task_id, snapshot.collaboration.revision);
            (StatusCode::CREATED, Json(snapshot)).into_response()
        }
        Err(error) => team_store_error(error),
    }
}

async fn team_add_comment(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<TeamTextMutationBody>,
) -> Response {
    let actor_id = match team_actor(&st, &headers) {
        Ok(actor_id) => actor_id,
        Err(response) => return response,
    };
    let task_id = TaskId::new(id);
    match st.store.add_task_comment(
        &task_id,
        &actor_id,
        body.expected_revision,
        &body.body,
        now_millis(),
    ) {
        Ok(snapshot) => {
            publish_task_snapshot(&st, &task_id, snapshot.revision);
            Json(snapshot).into_response()
        }
        Err(error) => team_store_error(error),
    }
}

async fn team_create_suggestion(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<TeamTextMutationBody>,
) -> Response {
    let actor_id = match team_actor(&st, &headers) {
        Ok(actor_id) => actor_id,
        Err(response) => return response,
    };
    let task_id = TaskId::new(id);
    match st.store.create_task_suggestion(
        &task_id,
        &actor_id,
        body.expected_revision,
        &body.body,
        now_millis(),
    ) {
        Ok(snapshot) => {
            publish_task_snapshot(&st, &task_id, snapshot.revision);
            (StatusCode::CREATED, Json(snapshot)).into_response()
        }
        Err(error) => team_store_error(error),
    }
}

async fn team_approve_suggestion(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path((id, suggestion_id)): Path<(String, String)>,
    Json(body): Json<TeamApproveSuggestionBody>,
) -> Response {
    let actor_id = match team_actor(&st, &headers) {
        Ok(actor_id) => actor_id,
        Err(response) => return response,
    };
    let task_id = TaskId::new(id);
    let suggestion_id = SuggestionId::new(suggestion_id);
    let approval = match st.store.approve_task_suggestion(
        &task_id,
        &suggestion_id,
        &actor_id,
        &body.command_id,
        body.expected_revision,
        now_millis(),
    ) {
        Ok(approval) => approval,
        Err(error) => return team_store_error(error),
    };
    publish_task_snapshot(&st, &task_id, approval.receipt.revision);

    if !approval.replayed && approval.receipt.execution_claimed {
        let before = match st.store.shared_task_snapshot(&task_id, &actor_id) {
            Ok(snapshot) => snapshot,
            Err(error) => return team_store_error(error),
        };
        let suggestion = match before
            .collaboration
            .suggestions
            .iter()
            .find(|suggestion| suggestion.id == suggestion_id)
        {
            Some(suggestion) => suggestion.clone(),
            None => {
                return team_store_error(StoreError::SuggestionNotFound {
                    task_id: task_id.as_str().to_string(),
                    suggestion_id: suggestion_id.as_str().to_string(),
                })
            }
        };
        let creation = ParallelTaskCreation {
            provider: before.runtime.provider_configuration.provider.clone(),
            cwd: before.collaboration.cwd.clone(),
            worktree_base: WorktreeBaseline::Current,
            worktree_base_sha: body.worktree_base_sha,
            request_id: format!("team-approve:{}", body.command_id),
            model: before.runtime.provider_configuration.model.clone(),
            initial_policy: None,
            reasoning_effort: before
                .runtime
                .provider_configuration
                .reasoning_effort
                .clone(),
            task_id: task_id.clone(),
            goal: suggestion.body.clone(),
        };
        if let Err(error) = st.engine.attach_parallel_task_session(creation).await {
            let message = error.to_string();
            if let Ok(revision) = st.store.fail_suggestion_execution(
                &task_id,
                &suggestion_id,
                &actor_id,
                &message,
                now_millis(),
            ) {
                publish_task_snapshot(&st, &task_id, revision);
            }
            return (StatusCode::INTERNAL_SERVER_ERROR, message).into_response();
        }
        let runtime = match st.store.task_snapshot(&task_id) {
            Ok(runtime) => runtime,
            Err(error) => return team_store_error(error),
        };
        let session_id = runtime
            .session_leases
            .iter()
            .filter(|lease| lease.released_at_ms.is_none())
            .max_by_key(|lease| lease.lease_id)
            .map(|lease| lease.session_id.clone());
        let Some(session_id) = session_id else {
            let message = "Core did not attach an execution Session";
            if let Ok(revision) = st.store.fail_suggestion_execution(
                &task_id,
                &suggestion_id,
                &actor_id,
                message,
                now_millis(),
            ) {
                publish_task_snapshot(&st, &task_id, revision);
            }
            return (StatusCode::INTERNAL_SERVER_ERROR, message).into_response();
        };
        let revision = match st.store.link_suggestion_execution(
            &task_id,
            &suggestion_id,
            &actor_id,
            &session_id,
            now_millis(),
        ) {
            Ok(revision) => revision,
            Err(error) => return team_store_error(error),
        };
        publish_task_snapshot(&st, &task_id, revision);
        if let Err(error) = st
            .engine
            .submit(Op::Prompt {
                session: session_id,
                doc: vec![DocBlock::Text {
                    text: suggestion.body,
                }],
                request_id: Some(format!("team-execute:{}", body.command_id)),
            })
            .await
        {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Session was attached but the Suggestion could not start: {error}"),
            )
                .into_response();
        }
    }

    match st.store.shared_task_snapshot(&task_id, &actor_id) {
        Ok(snapshot) => Json(TeamApproveSuggestionReply {
            receipt: approval.receipt,
            snapshot,
        })
        .into_response(),
        Err(error) => team_store_error(error),
    }
}

fn bearer_from(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
}

#[derive(Serialize)]
struct WsTicketReply {
    ticket: String,
    expires_in: u64,
}

/// Mint a short-lived single-use WebSocket ticket for a paired device (bearer in the
/// `Authorization` header).
async fn ws_ticket(
    State(st): State<Arc<ServerState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let key = remote::auth_limiter_source_key(&headers, peer);
    if let Err(limit) = st.auth.check_auth_rate_limit(&key) {
        return auth_rate_limit_response(limit);
    }
    let Some(device_id) = bearer_from(&headers).and_then(|b| st.auth.authorize_bearer(b)) else {
        if let Some(limit) = st.auth.record_auth_failure(&key) {
            return auth_rate_limit_response(limit);
        }
        return (StatusCode::UNAUTHORIZED, "invalid bearer").into_response();
    };
    st.auth.record_auth_success(&key);
    let ticket = st.auth.issue_ws_ticket(&device_id);
    Json(WsTicketReply {
        ticket,
        expires_in: WS_TICKET_TTL.as_secs(),
    })
    .into_response()
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(q): Query<HashMap<String, String>>,
    State(st): State<Arc<ServerState>>,
) -> Response {
    if let Some(ticket) = q.get("wsTicket") {
        let Some(authorization) = st.auth.take_ws_ticket_profile(ticket) else {
            return (StatusCode::UNAUTHORIZED, "invalid or expired ticket").into_response();
        };
        let t3 = st.t3.clone();
        return ws.on_upgrade(move |socket| {
            t3_compat::handle_socket(socket, t3, authorization.device_id, authorization.scopes)
        });
    }

    let Some(device_id) = q
        .get("ticket")
        .and_then(|ticket| st.auth.take_ws_ticket(ticket))
    else {
        return (StatusCode::UNAUTHORIZED, "invalid or expired ticket").into_response();
    };
    ws.on_upgrade(move |socket| handle_socket(socket, st, device_id))
}

#[derive(Serialize)]
struct PairedDeviceEntry {
    #[serde(flatten)]
    info: auth::DeviceInfo,
    /// The device that made this request, so a client can warn before it signs itself out.
    current: bool,
}

/// The devices paired with this server, for a paired owner device to review and revoke.
async fn list_paired_devices(State(st): State<Arc<ServerState>>, headers: HeaderMap) -> Response {
    let me = match require_device(&st, &headers, None) {
        Ok(device_id) => device_id,
        Err(response) => return response,
    };
    let devices: Vec<PairedDeviceEntry> = st
        .auth
        .list_devices()
        .into_iter()
        .map(|info| PairedDeviceEntry {
            current: info.id == me,
            info,
        })
        .collect();
    Json(devices).into_response()
}

/// Revoke a paired device; its bearer and live sockets stop working at once. Revoking the caller's
/// own device is allowed and reported, which is how a client signs itself out.
async fn revoke_paired_device(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let me = match require_device(&st, &headers, None) {
        Ok(device_id) => device_id,
        Err(response) => return response,
    };
    match st.auth.try_revoke_device(&id) {
        Ok(true) => {
            Json(serde_json::json!({ "revoked": true, "was_current": id == me })).into_response()
        }
        Ok(false) => (StatusCode::NOT_FOUND, "no such device").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error).into_response(),
    }
}

/// List live terminals so a reconnecting browser can reattach instead of respawning.
async fn list_terminals(State(st): State<Arc<ServerState>>, headers: HeaderMap) -> Response {
    if let Err(response) = require_device(&st, &headers, None) {
        return response;
    }
    Json(st.terminals.list()).into_response()
}

/// Kill a terminal outright (the WS `kill` op works too; this covers viewers that only hold the
/// listing).
async fn kill_terminal(
    State(st): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = require_device(&st, &headers, None) {
        return response;
    }
    if st.terminals.kill(&id) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (StatusCode::NOT_FOUND, "no such terminal").into_response()
    }
}

/// Upgrade a terminal socket. Accepts the legacy single-use `ticket`, or a T3 `wsTicket` whose
/// profile carries the `terminal:operate` scope.
async fn terminal_ws_handler(
    ws: WebSocketUpgrade,
    Query(q): Query<HashMap<String, String>>,
    State(st): State<Arc<ServerState>>,
) -> Response {
    let device_id = if let Some(ticket) = q.get("wsTicket") {
        match st.auth.take_ws_ticket_profile(ticket) {
            Some(authorization)
                if authorization
                    .scopes
                    .iter()
                    .any(|scope| scope == "terminal:operate") =>
            {
                authorization.device_id
            }
            Some(_) => {
                return (StatusCode::FORBIDDEN, "missing terminal:operate scope").into_response()
            }
            None => return (StatusCode::UNAUTHORIZED, "invalid or expired ticket").into_response(),
        }
    } else {
        match q
            .get("ticket")
            .and_then(|ticket| st.auth.take_ws_ticket(ticket))
        {
            Some(device_id) => device_id,
            None => return (StatusCode::UNAUTHORIZED, "invalid or expired ticket").into_response(),
        }
    };
    if st.auth.member_for_device(&device_id).is_some() {
        return (
            StatusCode::FORBIDDEN,
            "team members cannot operate the shared terminal directly",
        )
            .into_response();
    }
    let registry = st.terminals.clone();
    ws.on_upgrade(move |socket| terminal::handle_socket(socket, registry, device_id))
}

fn validate_canvas_op(st: &ServerState, _device_id: &str, op: &Op) -> Result<(), CanvasError> {
    let Op::Prompt { doc, .. } = op else {
        return Ok(());
    };
    let has_canvas = doc
        .iter()
        .any(|block| matches!(block, DocBlock::Canvas { .. }));
    if !has_canvas {
        return Ok(());
    }
    st.canvas_gate.require()?;
    for block in doc {
        let DocBlock::Canvas {
            id,
            frozen_revision,
            ..
        } = block
        else {
            continue;
        };
        // WS prompts carry only immutable references.  The lookup is global after ticket auth;
        // mutable draft ownership is enforced by the REST update/freeze routes before a ref can
        // exist, and arbitrary ids never reach Engine.
        st.store
            .resolve_canvas_prompt_frozen(id, *frozen_revision)?;
    }
    Ok(())
}

fn op_session_id(op: &Op) -> Option<&str> {
    match op {
        Op::NewSession { .. } => None,
        Op::Prompt { session, .. }
        | Op::Cancel { session }
        | Op::AnswerPermission { session, .. }
        | Op::AnswerElicitation { session, .. }
        | Op::SetPermissionMode { session, .. }
        | Op::SetSandbox { session, .. }
        | Op::SetExecutionPolicy { session, .. }
        | Op::SetModel { session, .. }
        | Op::SetConfigOption { session, .. } => Some(session),
    }
}

fn event_session_id(event: &Event) -> Option<String> {
    serde_json::to_value(event)
        .ok()?
        .get("session")?
        .as_str()
        .map(str::to_string)
}

async fn handle_socket(socket: WebSocket, st: Arc<ServerState>, device_id: String) {
    let (mut sender, mut receiver) = socket.split();
    let team_member_id = st.auth.member_for_device(&device_id).map(MemberId::new);
    let mut revocations = st.auth.subscribe_revocations();

    // One outbound lane per client: engine events and request replies both go through it, so the
    // socket sender lives in exactly one task.
    let (out_tx, mut out_rx) = mpsc::channel::<Outbound>(256);

    // Welcome: a snapshot of sessions.
    let welcome = match st.engine.list_sessions() {
        Ok(mut sessions) => {
            if let Some(member_id) = &team_member_id {
                sessions.retain(|session| {
                    st.store
                        .member_can_access_session(member_id, &session.id)
                        .unwrap_or(false)
                });
            } else {
                sessions.retain(|session| {
                    !st.store
                        .session_is_team_managed(&session.id)
                        .unwrap_or(true)
                });
            }
            Outbound::Sessions { sessions }
        }
        Err(error) => Outbound::SessionsError {
            message: error.to_string(),
        },
    };
    let _ = out_tx.send(welcome).await;

    // Forward engine events into the outbound lane. A slow client that overflows the broadcast
    // buffer is told what it missed and stays connected, instead of being silently dropped.
    let mut ev_rx = st.events.subscribe();
    let ev_out = out_tx.clone();
    let event_store = st.store.clone();
    let event_member_id = team_member_id.clone();
    let event_task = tokio::spawn(async move {
        loop {
            match ev_rx.recv().await {
                Ok(ev) => {
                    if let Some(member_id) = &event_member_id {
                        let visible = match &ev {
                            Event::TaskSnapshotChanged { task_id, .. } => event_store
                                .member_can_access_task(member_id, task_id)
                                .unwrap_or(false),
                            _ => event_session_id(&ev).is_some_and(|session_id| {
                                event_store
                                    .member_can_access_session(member_id, &session_id)
                                    .unwrap_or(false)
                            }),
                        };
                        if !visible {
                            continue;
                        }
                    } else {
                        let team_event = match &ev {
                            Event::TaskSnapshotChanged { .. } => true,
                            _ => event_session_id(&ev).is_some_and(|session_id| {
                                event_store
                                    .session_is_team_managed(&session_id)
                                    .unwrap_or(true)
                            }),
                        };
                        if team_event {
                            // Team collaboration state and its execution stream are visible only
                            // to member-bound devices. Personal clients retain personal Sessions.
                            continue;
                        }
                    }
                    if ev_out.send(Outbound::Event { event: ev }).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    let _ = ev_out.send(Outbound::Lagged { missed }).await;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Drain the outbound lane into the socket.
    let mut send_task = tokio::spawn(async move {
        while let Some(out) = out_rx.recv().await {
            let msg = serde_json::to_string(&out).unwrap_or_default();
            if sender.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    // Keep accepted inbound work independent of the socket's lifetime. `Engine::submit(Prompt)`
    // publishes TurnStarted before it may have to revive/create the provider session; cancelling
    // that future on disconnect would release the core turn lease without a terminal event and
    // strand every other connected frontend in a false running state. A bounded, ordered worker
    // preserves per-client message ordering and finishes already accepted work after disconnect.
    let (in_tx, mut in_rx) = mpsc::channel::<Inbound>(16);
    let work_engine = st.engine.clone();
    let work_out = out_tx.clone();
    let work_member_id = team_member_id.clone();
    let work_device_id = device_id.clone();
    let _work_task = tokio::spawn(async move {
        while let Some(inbound) = in_rx.recv().await {
            match inbound {
                Inbound::Op(op) => {
                    let (session, request_id) = match &op {
                        Op::NewSession { request_id, .. } => (None, request_id.clone()),
                        Op::Prompt {
                            session,
                            request_id,
                            ..
                        } => (Some(session.clone()), request_id.clone()),
                        _ => (None, None),
                    };
                    if let Some(member_id) = &work_member_id {
                        let allowed = op_session_id(&op).is_some_and(|session_id| {
                            st.store
                                .member_controls_session(member_id, session_id)
                                .unwrap_or(false)
                        });
                        if !allowed {
                            let _ = work_out
                                .send(Outbound::Event {
                                    event: Event::Error {
                                        session,
                                        message:
                                            "Task owner approval is required for this operation"
                                                .into(),
                                        terminal: true,
                                        request_id,
                                    },
                                })
                                .await;
                            continue;
                        }
                    } else if op_session_id(&op).is_some_and(|session_id| {
                        st.store.session_is_team_managed(session_id).unwrap_or(true)
                    }) {
                        let _ = work_out
                            .send(Outbound::Event {
                                event: Event::Error {
                                    session,
                                    message:
                                        "A member-bound device is required for shared Task Sessions"
                                            .into(),
                                    terminal: true,
                                    request_id,
                                },
                            })
                            .await;
                        continue;
                    }
                    if let Err(error) = validate_canvas_op(&st, &work_device_id, &op) {
                        let _ = work_out
                            .send(Outbound::Event {
                                event: Event::Error {
                                    session,
                                    message: error.to_string(),
                                    terminal: true,
                                    request_id,
                                },
                            })
                            .await;
                        continue;
                    }
                    // A submit error (for example a missing provider binary) does not necessarily
                    // reach the engine event stream. Give the initiating remote client the same
                    // command-result feedback as Desktop while it remains connected.
                    if let Err(error) = work_engine.submit(op).await {
                        let _ = work_out
                            .send(Outbound::Event {
                                event: Event::Error {
                                    session,
                                    message: error.to_string(),
                                    terminal: true,
                                    request_id,
                                },
                            })
                            .await;
                    }
                }
                Inbound::Req(Req::Transcript {
                    session,
                    before,
                    limit,
                    request_id,
                }) => {
                    if let Some(member_id) = &work_member_id {
                        if !st
                            .store
                            .member_can_access_session(member_id, &session)
                            .unwrap_or(false)
                        {
                            let _ = work_out
                                .send(Outbound::TranscriptError {
                                    session,
                                    request_id,
                                    message: "Session is not visible to this Member".into(),
                                })
                                .await;
                            continue;
                        }
                    } else if st.store.session_is_team_managed(&session).unwrap_or(true) {
                        let _ = work_out
                            .send(Outbound::TranscriptError {
                                session,
                                request_id,
                                message:
                                    "A member-bound device is required for shared Task Sessions"
                                        .into(),
                            })
                            .await;
                        continue;
                    }
                    match work_engine.transcript_page(
                        &session,
                        before,
                        limit.unwrap_or(DEFAULT_TRANSCRIPT_TURNS),
                    ) {
                        Ok(page) => {
                            let _ = work_out
                                .send(Outbound::Transcript {
                                    session,
                                    request_id,
                                    entries: page.entries,
                                    next_before: page.next_before,
                                    snapshot_through: page.snapshot_through,
                                })
                                .await;
                        }
                        Err(error) => {
                            let _ = work_out
                                .send(Outbound::TranscriptError {
                                    session,
                                    request_id,
                                    message: error.to_string(),
                                })
                                .await;
                        }
                    }
                }
                Inbound::Req(Req::Sessions) => {
                    let reply = match work_engine.list_sessions() {
                        Ok(mut sessions) => {
                            if let Some(member_id) = &work_member_id {
                                sessions.retain(|session| {
                                    st.store
                                        .member_can_access_session(member_id, &session.id)
                                        .unwrap_or(false)
                                });
                            } else {
                                sessions.retain(|session| {
                                    !st.store
                                        .session_is_team_managed(&session.id)
                                        .unwrap_or(true)
                                });
                            }
                            Outbound::Sessions { sessions }
                        }
                        Err(error) => Outbound::SessionsError {
                            message: error.to_string(),
                        },
                    };
                    let _ = work_out.send(reply).await;
                }
            }
        }
    });

    // Client → ordered worker: parsing remains tied to the socket, accepted work does not.
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                match serde_json::from_str::<Inbound>(&text) {
                    Ok(inbound) => {
                        if in_tx.send(inbound).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => tracing::debug!("remote: bad message: {e}"),
                }
            }
        }
    });

    // If either side ends, tear down the rest.
    let revoked_device_id = device_id.clone();
    let revocation_task = async move {
        loop {
            match revocations.recv().await {
                Ok(device_id) if device_id == revoked_device_id => break,
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => std::future::pending::<()>().await,
            }
        }
    };
    tokio::pin!(revocation_task);
    tokio::select! {
        _ = &mut send_task => { recv_task.abort(); event_task.abort(); }
        _ = &mut recv_task => { send_task.abort(); event_task.abort(); }
        _ = &mut revocation_task => {
            // Disconnect immediately so the revoked device cannot submit or read anything else.
            // Work already accepted into the ordered lane keeps the same disconnect semantics.
            send_task.abort();
            recv_task.abort();
            event_task.abort();
        }
    }
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

include!(concat!(env!("OUT_DIR"), "/canvas_assets.rs"));
include!(concat!(env!("OUT_DIR"), "/term_assets.rs"));

/// Embedded xterm.js bundle for the remote terminal view. Same traversal rules as the Canvas
/// island: only compile-time embedded names resolve, nothing touches the filesystem at runtime.
async fn term_asset(Path(path): Path<String>) -> Response {
    if path.is_empty() || path.contains('/') || path.contains('\\') || path.starts_with('.') {
        return (StatusCode::NOT_FOUND, "terminal asset not found").into_response();
    }
    let Some((_, bytes)) = TERM_ASSETS.iter().find(|(name, _)| *name == path) else {
        return (StatusCode::NOT_FOUND, "terminal asset not found").into_response();
    };
    let mut response = (*bytes).to_vec().into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(canvas_asset_mime(&path)),
    );
    response
}

/// Compile-time embedded Canvas island paths, exposed only for integration probes and packaged
/// host diagnostics.  The returned names are generated from `assets/canvas`, so callers never
/// need to hard-code Vite's content hashes.
#[doc(hidden)]
pub fn embedded_canvas_asset_paths() -> impl Iterator<Item = &'static str> {
    CANVAS_ASSETS.iter().map(|(path, _)| *path)
}

fn canvas_asset_mime(path: &str) -> &'static str {
    if path.ends_with(".js") {
        "text/javascript; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".woff2") {
        "font/woff2"
    } else {
        "application/octet-stream"
    }
}

async fn canvas_page() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn canvas_entry() -> Response {
    canvas_asset_response("canvas-island.js")
}

async fn canvas_css() -> Response {
    let path = CANVAS_ASSETS
        .iter()
        .map(|(path, _)| *path)
        .find(|path| path.ends_with(".css"));
    match path {
        Some(path) => canvas_asset_response(path),
        None => (StatusCode::NOT_FOUND, "canvas stylesheet not found").into_response(),
    }
}

async fn canvas_asset(Path(path): Path<String>) -> Response {
    // Reject traversal before lookup; only compile-time embedded paths are eligible.
    if path.is_empty()
        || path.starts_with('/')
        || path.split('/').any(|part| part == ".." || part.is_empty())
        || path.contains('\\')
    {
        return (StatusCode::NOT_FOUND, "canvas asset not found").into_response();
    }
    canvas_asset_response(&path)
}

fn canvas_asset_response(path: &str) -> Response {
    let Some((_, bytes)) = CANVAS_ASSETS.iter().find(|(name, _)| *name == path) else {
        return (StatusCode::NOT_FOUND, "canvas asset not found").into_response();
    };
    let mut response = (*bytes).to_vec().into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(canvas_asset_mime(path)),
    );
    response
}

fn is_pairable_lan_address(interface: &str, ip: Ipv4Addr) -> bool {
    let interface = interface.to_ascii_lowercase();
    let physical_interface = [
        "en", "eth", "em", "wlan", "wlp", "wlx", "bond", "team", "usb", "rndis",
    ]
    .iter()
    .any(|prefix| interface.starts_with(prefix));
    let octets = ip.octets();
    physical_interface
        && !ip.is_loopback()
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && !ip.is_link_local()
        && ip != Ipv4Addr::BROADCAST
        && !is_tailnet_address(ip)
        // RFC 2544 benchmarking space is commonly used by packet-tunnel VPNs (including the
        // 198.18.0.1 address seen on macOS utun devices), and is not a phone-reachable LAN URL.
        && !(octets[0] == 198 && matches!(octets[1], 18 | 19))
}

/// Tailscale assigns stable node IPv4 addresses from RFC 6598's 100.64.0.0/10 range. The range is
/// shared CGNAT space rather than proof of Tailscale, so the UI presents matches as candidates and
/// never silently prefers them over a physical LAN address.
fn is_tailnet_address(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1]) && ip != Ipv4Addr::new(100, 100, 100, 100)
}

#[derive(Clone)]
struct LocalInterfaceAddress {
    interface: String,
    ip: Ipv4Addr,
    point_to_point: bool,
}

#[cfg(unix)]
fn local_interface_addresses() -> Vec<LocalInterfaceAddress> {
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: `head` is initialized for getifaddrs, every pointer is checked before dereferencing,
    // only AF_INET records are cast to sockaddr_in, and the list is freed exactly once.
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return Vec::new();
    }

    let mut addresses = Vec::new();
    let mut cursor = head;
    while !cursor.is_null() {
        // SAFETY: cursor belongs to the live getifaddrs list and is non-null for this iteration.
        let entry = unsafe { &*cursor };
        let flags = entry.ifa_flags as libc::c_int;
        let active = flags & libc::IFF_UP != 0 && flags & libc::IFF_RUNNING != 0;
        let point_to_point = flags & libc::IFF_POINTOPOINT != 0;
        let loopback = flags & libc::IFF_LOOPBACK != 0;
        if active && !loopback && !entry.ifa_addr.is_null() {
            // SAFETY: ifa_name is a NUL-terminated name owned by the live getifaddrs list.
            let interface = unsafe { std::ffi::CStr::from_ptr(entry.ifa_name) }
                .to_string_lossy()
                .into_owned();
            // SAFETY: the family check makes the sockaddr_in cast valid for this record.
            if unsafe { (*entry.ifa_addr).sa_family as libc::c_int } == libc::AF_INET {
                let address = unsafe { &*(entry.ifa_addr as *const libc::sockaddr_in) };
                let ip = Ipv4Addr::from(address.sin_addr.s_addr.to_ne_bytes());
                if !addresses
                    .iter()
                    .any(|address: &LocalInterfaceAddress| address.ip == ip)
                {
                    addresses.push(LocalInterfaceAddress {
                        interface,
                        ip,
                        point_to_point,
                    });
                }
            }
        }
        cursor = entry.ifa_next;
    }
    // SAFETY: head is the unchanged pointer returned by a successful getifaddrs call.
    unsafe { libc::freeifaddrs(head) };
    addresses.sort_by(|left, right| {
        left.interface
            .cmp(&right.interface)
            .then(left.ip.cmp(&right.ip))
    });
    addresses
}

#[cfg(not(unix))]
fn local_interface_addresses() -> Vec<LocalInterfaceAddress> {
    // A loopback-only result is safer than turning a VPN/default-route guess into a pairing QR.
    Vec::new()
}

fn local_lan_addresses() -> Vec<(String, Ipv4Addr)> {
    local_interface_addresses()
        .into_iter()
        .filter(|address| {
            !address.point_to_point && is_pairable_lan_address(&address.interface, address.ip)
        })
        .map(|address| (address.interface, address.ip))
        .collect()
}

fn local_tailnet_addresses() -> Vec<(String, Ipv4Addr)> {
    local_interface_addresses()
        .into_iter()
        .filter(|address| is_tailnet_address(address.ip))
        .map(|address| (address.interface, address.ip))
        .collect()
}

/// Best-effort phone-reachable LAN IP from an active physical network interface.
pub fn local_ip() -> Option<std::net::IpAddr> {
    local_lan_addresses()
        .into_iter()
        .next()
        .map(|(_, ip)| ip.into())
}

/// One address the desktop can advertise for remote pairing. IDs are stable across refreshes so
/// the frontend can keep a selection without treating the URL itself as identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PairingEndpoint {
    pub id: String,
    pub label: String,
    pub url: String,
    /// False for loopback: copying it for another browser on this machine is valid, but a phone
    /// scanning that URL would connect to itself.
    pub qr_shareable: bool,
}

/// Addresses advertised by a server bound on all interfaces. Physical LAN addresses are followed
/// by 100.64/10 tailnet candidates; loopback remains available for same-machine use.
pub fn pairing_endpoints(port: u16) -> Vec<PairingEndpoint> {
    pairing_endpoints_for_addresses(port, local_lan_addresses(), local_tailnet_addresses())
}

fn pairing_endpoints_for_addresses(
    port: u16,
    lan_addresses: Vec<(String, Ipv4Addr)>,
    tailnet_addresses: Vec<(String, Ipv4Addr)>,
) -> Vec<PairingEndpoint> {
    let mut endpoints: Vec<_> = lan_addresses
        .into_iter()
        .map(|(interface, ip)| PairingEndpoint {
            id: format!("lan-{interface}-{}", ip.to_string().replace('.', "-")),
            label: format!("LAN ({interface}: {ip})"),
            url: format!("http://{ip}:{port}/"),
            qr_shareable: true,
        })
        .collect();
    if endpoints.len() == 1 {
        // Keep the familiar compact label when there is no address choice to disambiguate.
        endpoints[0].label = "LAN".into();
    }
    if endpoints.is_empty() {
        tracing::warn!("remote: no active physical LAN address is available for phone pairing");
    }
    let single_tailnet = tailnet_addresses.len() == 1;
    endpoints.extend(
        tailnet_addresses
            .into_iter()
            .map(|(interface, ip)| PairingEndpoint {
                id: format!("tailnet-{interface}-{}", ip.to_string().replace('.', "-")),
                label: if single_tailnet {
                    format!("Tailnet candidate ({ip})")
                } else {
                    format!("Tailnet candidate ({interface}: {ip})")
                },
                url: format!("http://{ip}:{port}/"),
                qr_shareable: true,
            }),
    );
    endpoints.push(PairingEndpoint {
        id: "loopback".into(),
        label: "Loopback".into(),
        url: format!("http://127.0.0.1:{port}/"),
        qr_shareable: false,
    });
    endpoints
}

/// Resolve an explicitly requested endpoint, or prefer the first address that is meaningful to a
/// second device. Validation happens before a pairing token is issued by callers.
pub fn select_pairing_endpoint<'a>(
    endpoints: &'a [PairingEndpoint],
    requested_id: Option<&str>,
) -> Result<&'a PairingEndpoint, String> {
    if let Some(id) = requested_id {
        return endpoints
            .iter()
            .find(|endpoint| endpoint.id == id)
            .ok_or_else(|| format!("unknown pairing endpoint: {id}"));
    }
    endpoints
        .iter()
        .find(|endpoint| endpoint.qr_shareable)
        .or_else(|| endpoints.first())
        .ok_or_else(|| "no pairing endpoints are available".to_string())
}

/// Attach the one-time token as a fragment, never as a query or request-path credential.
pub fn pairing_url_for_endpoint(endpoint_url: &str, pairing_token: &str) -> String {
    format!(
        "{}/pair#token={pairing_token}",
        endpoint_url.trim_end_matches('/')
    )
}

/// The pairing URL a device opens to connect. The one-time token rides in the fragment so it never
/// appears in a request line or server log.
pub fn pairing_url(port: u16, pairing_token: &str) -> String {
    let endpoints = pairing_endpoints(port);
    let endpoint = select_pairing_endpoint(&endpoints, None)
        .expect("pairing_endpoints always includes the loopback fallback");
    pairing_url_for_endpoint(&endpoint.url, pairing_token)
}

/// Print a pairing panel (URL, token, and a scannable QR) to the terminal, t3code-style.
pub fn print_pairing(port: u16, pairing_token: &str) {
    let endpoints = pairing_endpoints(port);
    let endpoint = select_pairing_endpoint(&endpoints, None)
        .expect("pairing_endpoints always includes the loopback fallback");
    let url = pairing_url_for_endpoint(&endpoint.url, pairing_token);
    println!("\n  C2 remote is live.\n");
    if endpoint.qr_shareable {
        println!("  Open on another device (link is one-time, expires in 15 minutes):");
    } else {
        println!(
            "  Open in another browser on this machine (link is one-time, expires in 15 minutes):"
        );
    }
    println!("    {url}\n");
    println!("  Pairing token: {pairing_token}\n");
    if endpoint.qr_shareable {
        if let Ok(code) = qrcode::QrCode::new(url.as_bytes()) {
            let img = code
                .render::<qrcode::render::unicode::Dense1x2>()
                .quiet_zone(true)
                .build();
            println!("{img}\n");
        }
    } else {
        println!("  Loopback is not reachable from another device, so no QR code is shown.\n");
    }
}

/// Print an already-resolved pairing link (headless daemon). A QR code is shown only when another
/// device could actually reach the address.
pub fn print_pairing_link(url: &str, shareable: bool, ttl: std::time::Duration) {
    println!(
        "\n  Pair a device (one-time link, expires in {} minutes):\n    {url}\n",
        (ttl.as_secs() / 60).max(1)
    );
    if shareable {
        if let Ok(code) = qrcode::QrCode::new(url.as_bytes()) {
            let img = code
                .render::<qrcode::render::unicode::Dense1x2>()
                .quiet_zone(true)
                .build();
            println!("{img}\n");
        }
    }
}

/// A pairing QR code as an SVG document, for embedding in a UI.
pub fn pairing_qr_svg(url: &str) -> Option<String> {
    let code = qrcode::QrCode::new(url.as_bytes()).ok()?;
    Some(
        code.render::<qrcode::render::svg::Color>()
            .min_dimensions(192, 192)
            .quiet_zone(true)
            .build(),
    )
}

const INDEX_HTML: &str = include_str!("client.html");

#[cfg(test)]
mod tests {
    use super::{
        web_ui_command_allowed, DeviceSyncHttp, DeviceSyncIdentity, DeviceSyncReplica,
        DeviceSyncWriteResult, DeviceSyncWriteState, Outbound, PairingEndpoint, TranscriptCursor,
        TranscriptEntry,
    };
    use codetwo_core::device_sync::{device_sync_snapshot_version, DeviceSyncDocument};
    use codetwo_core::provider::ProviderId;
    use codetwo_core::skill::SkillLibrary;
    use codetwo_core::{Engine, Part, Role, Session, Store, TaskHandoffManager};
    use std::net::Ipv4Addr;
    use std::process::Command;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn artifact_download_filenames_are_sanitized() {
        let name = super::artifact_download_filename;
        assert_eq!(name("report.md"), "report.md");
        assert_eq!(name("../../etc/passwd"), ".._.._etc_passwd");
        assert_eq!(name("a\"b\nc d"), "a_b_c_d");
        assert_eq!(name(""), "artifact.bin");
        assert_eq!(name("."), "artifact.bin");
        assert_eq!(name("  "), "__");
        assert_eq!(name(&"x".repeat(200)).len(), 80);
    }

    fn endpoint(id: &str, qr_shareable: bool) -> PairingEndpoint {
        PairingEndpoint {
            id: id.into(),
            label: id.into(),
            url: format!("http://{id}.example/"),
            qr_shareable,
        }
    }

    #[test]
    fn browser_renderer_has_one_bounded_core_capability_set() {
        assert!(web_ui_command_allowed("sessions.list"));
        for command in [
            "assistant.snapshot",
            "assistant.edit",
            "memory.add",
            "memory.update",
            "memory.set_active",
        ] {
            assert!(web_ui_command_allowed(command));
        }
        assert!(!web_ui_command_allowed("memory.delete"));
        assert!(web_ui_command_allowed("engine.prompt"));
        assert!(web_ui_command_allowed("workspace.default_cwd"));
        assert!(!web_ui_command_allowed("plugins.set_trusted"));
        assert!(!web_ui_command_allowed("remote.stop"));
        // Workspace and Git panels are reachable, but running project scripts, GitHub operations,
        // LSP processes and the raw terminal command surface are not.
        for command in [
            "workspace.read_text",
            "workspace.write_text",
            "workspace.delete",
            "git.status",
            "git.commit",
        ] {
            assert!(web_ui_command_allowed(command), "{command}");
        }
        for command in [
            "workspace.run_script",
            "workspace.save_script",
            "git.create_pr",
            "github.merge_pr",
            "lsp.start",
            "terminal.spawn",
        ] {
            assert!(!web_ui_command_allowed(command), "{command}");
        }
    }

    fn git(cwd: &std::path::Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    struct TestDeviceSync {
        store: Arc<Store>,
        id: String,
    }

    impl DeviceSyncHttp for TestDeviceSync {
        fn identity(&self) -> DeviceSyncIdentity {
            DeviceSyncIdentity {
                id: self.id.clone(),
                name: "Server C2".into(),
            }
        }

        fn snapshot(&self) -> Result<DeviceSyncReplica, String> {
            let document = self
                .store
                .device_sync_snapshot(&self.id)
                .map_err(|error| error.to_string())?;
            Ok(DeviceSyncReplica {
                id: format!("paired:{}", self.id),
                version: device_sync_snapshot_version(&document),
                document,
            })
        }

        fn write_snapshot(
            &self,
            document: &DeviceSyncDocument,
            expected_version: &str,
        ) -> Result<DeviceSyncWriteResult, String> {
            let current = self.snapshot()?;
            if current.version != expected_version {
                return Ok(DeviceSyncWriteResult {
                    state: DeviceSyncWriteState::Conflict,
                    version: current.version,
                });
            }
            self.store
                .import_device_sync_document(document)
                .map_err(|error| error.to_string())?;
            let written = self.snapshot()?;
            Ok(DeviceSyncWriteResult {
                state: DeviceSyncWriteState::Written,
                version: written.version,
            })
        }
    }

    #[test]
    fn pairing_url_puts_token_in_fragment() {
        let u = super::pairing_url(4599, "abc");
        assert!(u.contains(":4599/pair#token=abc"), "got: {u}");
    }

    #[test]
    fn pairing_url_for_endpoint_normalizes_the_slash_before_the_fragment() {
        assert_eq!(
            super::pairing_url_for_endpoint("http://device.example///", "abc"),
            "http://device.example/pair#token=abc"
        );
    }

    #[test]
    fn endpoint_selection_validates_requests_and_prefers_qr_shareable_addresses() {
        let endpoints = vec![endpoint("loopback", false), endpoint("lan", true)];
        assert_eq!(
            super::select_pairing_endpoint(&endpoints, None).unwrap().id,
            "lan"
        );
        assert_eq!(
            super::select_pairing_endpoint(&endpoints, Some("loopback"))
                .unwrap()
                .id,
            "loopback"
        );
        assert!(super::select_pairing_endpoint(&endpoints, Some("missing")).is_err());
    }

    #[test]
    fn endpoint_selection_falls_back_when_only_loopback_exists() {
        let endpoints = vec![endpoint("loopback", false)];
        assert_eq!(
            super::select_pairing_endpoint(&endpoints, None).unwrap().id,
            "loopback"
        );
    }

    #[test]
    fn phone_pairing_only_advertises_physical_lan_addresses() {
        assert!(super::is_pairable_lan_address(
            "en0",
            Ipv4Addr::new(192, 168, 31, 102)
        ));
        assert!(super::is_pairable_lan_address(
            "wlan0",
            Ipv4Addr::new(10, 0, 0, 8)
        ));
        assert!(!super::is_pairable_lan_address(
            "utun4",
            Ipv4Addr::new(198, 18, 0, 1)
        ));
        assert!(!super::is_pairable_lan_address(
            "en0",
            Ipv4Addr::new(198, 18, 0, 1)
        ));
        assert!(!super::is_pairable_lan_address(
            "en0",
            Ipv4Addr::new(127, 0, 0, 1)
        ));
        assert!(!super::is_pairable_lan_address(
            "en0",
            Ipv4Addr::new(100, 99, 10, 8)
        ));
    }

    #[test]
    fn tailscale_addresses_are_explicit_tailnet_endpoints() {
        assert!(super::is_tailnet_address(Ipv4Addr::new(100, 64, 0, 1)));
        assert!(super::is_tailnet_address(Ipv4Addr::new(100, 127, 255, 254)));
        assert!(!super::is_tailnet_address(Ipv4Addr::new(100, 63, 255, 255)));
        assert!(!super::is_tailnet_address(Ipv4Addr::new(100, 128, 0, 1)));
        assert!(!super::is_tailnet_address(Ipv4Addr::new(
            100, 100, 100, 100
        )));

        let endpoints = super::pairing_endpoints_for_addresses(
            4599,
            vec![("en0".into(), Ipv4Addr::new(192, 168, 31, 102))],
            vec![("utun7".into(), Ipv4Addr::new(100, 99, 10, 8))],
        );
        assert_eq!(endpoints[0].id, "lan-en0-192-168-31-102");
        assert_eq!(endpoints[1].id, "tailnet-utun7-100-99-10-8");
        assert_eq!(endpoints[1].label, "Tailnet candidate (100.99.10.8)");
        assert_eq!(endpoints[1].url, "http://100.99.10.8:4599/");
        assert!(endpoints[1].qr_shareable);
        assert_eq!(endpoints[2].id, "loopback");
    }

    #[test]
    fn qr_svg_renders() {
        let svg = super::pairing_qr_svg("http://192.168.1.2:4599/pair#token=abc").unwrap();
        assert!(
            svg.starts_with("<?xml") || svg.starts_with("<svg"),
            "got: {}",
            &svg[..20.min(svg.len())]
        );
    }

    #[test]
    fn transcript_frame_preserves_the_canonical_prompt_variant() {
        let frame = Outbound::Transcript {
            session: "session-1".into(),
            request_id: Some("page-1".into()),
            entries: vec![TranscriptEntry {
                seq: 7,
                role: Role::User,
                part: Part::Prompt {
                    text: "full\n  prompt".into(),
                    display: "full prompt".into(),
                },
                created_at: 123,
                started_at: Some(123),
            }],
            next_before: Some(TranscriptCursor(7)),
            snapshot_through: Some(TranscriptCursor(7)),
        };
        let value = serde_json::to_value(frame).unwrap();
        assert_eq!(value["kind"], "transcript");
        assert_eq!(value["request_id"], "page-1");
        assert_eq!(value["entries"][0]["seq"], 7);
        assert_eq!(value["entries"][0]["part"]["kind"], "prompt");
        assert_eq!(value["entries"][0]["part"]["text"], "full\n  prompt");
        assert_eq!(value["next_before"], 7);
    }

    #[test]
    fn embedded_canvas_manifest_is_generated_and_wire_fields_are_camel_case() {
        assert!(super::CANVAS_ASSETS
            .iter()
            .any(|(path, _)| *path == "canvas-island.js"));
        assert!(super::CANVAS_ASSETS
            .iter()
            .any(|(path, _)| path.ends_with(".woff2")));
        let asset = super::CanvasAssetBody {
            id: "a".into(),
            mime_type: "image/png".into(),
            width: 1,
            height: 1,
            bytes: vec![1, 2],
        };
        let value = serde_json::to_value(asset).unwrap();
        assert_eq!(value["mimeType"], "image/png");
        assert!(value.get("mime_type").is_none());
    }

    #[tokio::test]
    async fn c2_device_sync_routes_pair_conflict_and_revoke_independently() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let (engine, events) =
            Engine::with_store(Vec::new(), SkillLibrary::new(Vec::new()), store.clone());
        let events = super::fanout(events);
        let auth = Arc::new(super::AuthState::load(None));
        let token = auth.issue_c2_pairing_token(Duration::from_secs(60));
        let sync: Arc<dyn DeviceSyncHttp> = Arc::new(TestDeviceSync {
            store: store.clone(),
            id: "server-id".into(),
        });
        let (address, server) = super::bind_and_serve_with_services(
            Arc::new(engine),
            events,
            "127.0.0.1:0".parse().unwrap(),
            auth.clone(),
            store.clone(),
            codetwo_core::CanvasFeatureGate::default(),
            Some(sync),
        )
        .await
        .unwrap();
        let base = format!("http://{address}/api/device-sync/v1");
        let client = reqwest::Client::new();

        let pair = client
            .post(format!("{base}/pair"))
            .json(&serde_json::json!({
                "token": token,
                "device_name": "Client C2",
                "device_id": "stable-client-id",
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(pair.status(), reqwest::StatusCode::OK);
        let paired: serde_json::Value = pair.json().await.unwrap();
        assert_eq!(paired["server_id"], "server-id");
        let device_id = paired["device_id"].as_str().unwrap().to_string();
        let bearer = paired["bearer"].as_str().unwrap().to_string();

        let replay = client
            .post(format!("{base}/pair"))
            .json(&serde_json::json!({
                "token": token,
                "device_name": "Replay",
                "device_id": "stable-client-id",
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(replay.status(), reqwest::StatusCode::UNAUTHORIZED);

        let unauthorized = client.get(format!("{base}/snapshot")).send().await.unwrap();
        assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

        let snapshot = client
            .get(format!("{base}/snapshot"))
            .bearer_auth(&bearer)
            .send()
            .await
            .unwrap();
        assert_eq!(snapshot.status(), reqwest::StatusCode::OK);
        let stale: serde_json::Value = snapshot.json().await.unwrap();
        store
            .add_project("/raced", Some("Raced"), codetwo_core::session::now_millis())
            .unwrap();

        let conflict = client
            .put(format!("{base}/snapshot"))
            .bearer_auth(&bearer)
            .json(&serde_json::json!({
                "document": stale["replica"]["document"],
                "expected_version": stale["replica"]["version"],
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(conflict.status(), reqwest::StatusCode::CONFLICT);
        assert_eq!(
            conflict.json::<serde_json::Value>().await.unwrap()["state"],
            "conflict"
        );

        let fresh = client
            .get(format!("{base}/snapshot"))
            .bearer_auth(&bearer)
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap();
        let written = client
            .put(format!("{base}/snapshot"))
            .bearer_auth(&bearer)
            .json(&serde_json::json!({
                "document": fresh["replica"]["document"],
                "expected_version": fresh["replica"]["version"],
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(written.status(), reqwest::StatusCode::OK);
        assert_eq!(
            written.json::<serde_json::Value>().await.unwrap()["state"],
            "written"
        );

        assert!(auth.revoke_device(&device_id));
        let revoked = client
            .get(format!("{base}/snapshot"))
            .bearer_auth(&bearer)
            .send()
            .await
            .unwrap();
        assert_eq!(revoked.status(), reqwest::StatusCode::UNAUTHORIZED);
        server.abort();
    }

    #[tokio::test]
    async fn authenticated_handoff_routes_move_one_writable_task() {
        let source_workspace = tempfile::tempdir().unwrap();
        git(source_workspace.path(), &["init", "-q"]);
        git(
            source_workspace.path(),
            &["config", "user.email", "test@codetwo.local"],
        );
        git(source_workspace.path(), &["config", "user.name", "C2 Test"]);
        std::fs::write(source_workspace.path().join("task.txt"), "before\n").unwrap();
        git(source_workspace.path(), &["add", "task.txt"]);
        git(source_workspace.path(), &["commit", "-qm", "baseline"]);
        std::fs::write(source_workspace.path().join("task.txt"), "after\n").unwrap();

        let source_data = tempfile::tempdir().unwrap();
        let target_data = tempfile::tempdir().unwrap();
        let source_store =
            Arc::new(Store::open(source_data.path().join("codetwo.db").to_str().unwrap()).unwrap());
        let target_store =
            Arc::new(Store::open(target_data.path().join("codetwo.db").to_str().unwrap()).unwrap());
        let session = Session::new(
            ProviderId::Codex,
            source_workspace.path().to_string_lossy().into_owned(),
        );
        source_store.upsert_session(&session).unwrap();
        let (source_engine, _) = Engine::with_store(
            Vec::new(),
            SkillLibrary::new(Vec::new()),
            source_store.clone(),
        );
        let (target_engine, target_events) = Engine::with_store(
            Vec::new(),
            SkillLibrary::new(Vec::new()),
            target_store.clone(),
        );
        let target_events = super::fanout(target_events);
        let auth = Arc::new(super::AuthState::load(None));
        let pairing = auth.issue_t3_pairing_token(Duration::from_secs(60));
        let paired = auth
            .try_pair_with_profile_options(
                &pairing,
                "task transfer",
                Some(Duration::from_secs(60)),
                vec!["orchestration:operate".into()],
                true,
            )
            .unwrap()
            .unwrap();
        let (address, server) = super::bind_and_serve_with_canvas(
            Arc::new(target_engine),
            target_events,
            "127.0.0.1:0".parse().unwrap(),
            auth.clone(),
            target_store.clone(),
            codetwo_core::CanvasFeatureGate::default(),
        )
        .await
        .unwrap();
        let source =
            TaskHandoffManager::new(source_store.clone(), Arc::new(source_engine), None).unwrap();
        let target_parent = tempfile::tempdir().unwrap();
        let destination = target_parent.path().join("moved-task");
        let result = source
            .transfer(
                &session.id,
                &format!("http://{address}"),
                &paired.bearer,
                destination.to_str().unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(result.state, "transferred");
        assert!(source_store.assert_session_active(&session.id).is_err());
        assert!(target_store.assert_session_active(&session.id).is_ok());
        assert_eq!(
            std::fs::read_to_string(destination.join("task.txt")).unwrap(),
            "after\n"
        );
        assert!(
            auth.authorize_bearer_profile(&paired.bearer).is_none(),
            "activation consumes the task-transfer credential"
        );
        server.abort();
    }
}

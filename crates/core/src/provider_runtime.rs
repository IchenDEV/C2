//! Provider-neutral runtime boundary.
//!
//! Engine owns sessions, turns, permissions, persistence and delivery. It reaches a provider
//! backend only through [`ProviderRuntime`], so the wire protocol (ACP today; official native
//! connectors later) is an implementation detail of one backend. This module owns neutral types
//! only, with legacy public error conversions at the compatibility seam.
//!
//! Honesty rules the types encode:
//! - A turn is never reported as *accepted* by this boundary on local evidence. [`TurnOutcome`]
//!   distinguishes "provably not transmitted", "provider rejected", "provider terminal" and
//!   "transmitted but result unknown". Callers must not replay `Unknown`.
//! - Capabilities are tri-state where a backend cannot know ([`Support::Unverified`]), and
//!   stop/steer/resume carry the strength of the guarantee, not a bare boolean.
//! - A stop request is not a terminal: [`StopSupport::RequestOnly`] means the writer is only known
//!   to be released when the matching turn reports its own terminal.
//!
//! Foundation limitations (deliberately not claimed as done):
//! - [`ProviderRuntime::send_turn`] awaits the provider terminal and has no early *Accepted*
//!   event. The spec's accepted/terminal split needs a backend that proves acceptance (a native
//!   run id); ACP cannot, so only terminal/not-sent/rejected/unknown are reported.
//! - [`RuntimeError`] converts to/from `AcpError` only to keep the existing public `submit`
//!   surface compatible; native adapters must not depend on `AcpError`.
//!
//! Still ACP-shaped (documented seam, deferred): the agent→client callback surface is the ACP
//! `ClientHandler` implemented by Engine's `SessionHandler`, which `connectors::acp::launch`
//! accepts. A native connector will need its own event ingress mapping into the same Engine
//! `Event`s; that is I1–I4 work, not part of this foundation.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AcpError, RpcError};
use crate::event::{ConfigOptionInfo, ModelChoice};
use crate::permission::ExecutionPolicy;
use crate::skill::McpServer;

/// Shared, dynamically dispatched backend handle held by Engine sessions.
pub type RuntimeHandle = Arc<dyn ProviderRuntime>;

/// Which kind of backend drives a session. Persisted by a later slice together with the
/// contract version; native variants are added only when a verified adapter exists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RuntimeBackendKind {
    /// Agent Client Protocol subprocess (third-party providers and current builtin sessions).
    #[default]
    Acp,
    /// Official Codex `app-server` JSON-RPC over stdio, driven directly from Rust.
    CodexAppServer,
    /// Official Claude Agent SDK behind a supervised C2 sidecar.
    ClaudeAgentSdk,
    /// Official `@cursor/sdk` (TypeScript) behind a supervised C2 sidecar.
    CursorSdk,
    /// OpenCode V1 line (`@opencode-ai/sdk`, HTTP server and SSE events).
    OpenCodeV1,
    /// OpenCode V2 line (`@opencode/sdk`, embedded host). A distinct API from V1.
    OpenCodeV2,
}

impl RuntimeBackendKind {
    /// Stable persisted name. Never reuse a name for a different backend.
    pub fn as_str(self) -> &'static str {
        match self {
            RuntimeBackendKind::Acp => "acp",
            RuntimeBackendKind::CodexAppServer => "codex-app-server",
            RuntimeBackendKind::ClaudeAgentSdk => "claude-agent-sdk",
            RuntimeBackendKind::CursorSdk => "cursor-sdk",
            RuntimeBackendKind::OpenCodeV1 => "opencode-v1",
            RuntimeBackendKind::OpenCodeV2 => "opencode-v2",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "acp" => RuntimeBackendKind::Acp,
            "codex-app-server" => RuntimeBackendKind::CodexAppServer,
            "claude-agent-sdk" => RuntimeBackendKind::ClaudeAgentSdk,
            "cursor-sdk" => RuntimeBackendKind::CursorSdk,
            "opencode-v1" => RuntimeBackendKind::OpenCodeV1,
            "opencode-v2" => RuntimeBackendKind::OpenCodeV2,
            _ => return None,
        })
    }

    pub fn is_native(self) -> bool {
        self != RuntimeBackendKind::Acp
    }
}

/// Backend identity. The C2 session/turn identities stay Engine-owned; `backend_session_id` is
/// not part of this struct because it is per-session and lives in [`RuntimeSessionState`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeIdentity {
    pub backend: RuntimeBackendKind,
    /// Version of this neutral contract the backend implements.
    pub contract_version: u32,
    /// Backend wire/protocol revision (ACP protocol version for ACP).
    pub protocol_version: i64,
    pub adapter_name: Option<String>,
    pub adapter_version: Option<String>,
}

pub const RUNTIME_CONTRACT_VERSION: u32 = 1;

/// Tri-state support: a backend must not claim what it cannot know.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Support {
    Supported,
    #[default]
    Unsupported,
    /// Not advertised either way; attempting it may or may not work and failures are surfaced.
    Unverified,
}

impl Support {
    pub fn is_supported(self) -> bool {
        self == Support::Supported
    }
}

/// How a stopped turn becomes verifiable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopSupport {
    #[default]
    Unsupported,
    /// The request itself carries no acknowledgement. The writer is released only when the
    /// matching turn later reports a terminal (or the backend is terminated).
    RequestOnly,
    /// The backend acknowledges stop with a terminal for the matching turn.
    VerifiedTerminal,
}

/// Live steering (adding input to a running turn) support.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SteerSupport {
    #[default]
    Unsupported,
    Native,
}

/// Provider session restore strength, least lossy first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeSupport {
    /// Re-attach without replaying history (the transcript is already C2-owned).
    pub resume: bool,
    /// Re-attach by replaying history (callers must mute event ingestion during replay).
    pub load: bool,
}

impl RuntimeCapabilities {
    /// Reject MCP servers whose transport this backend did not advertise, before any provider
    /// session is created or mutated.
    pub fn validate_mcp(&self, servers: &[McpServer]) -> Result<(), String> {
        use crate::skill::McpTransport;
        for server in servers {
            let (supported, transport) = match &server.transport {
                McpTransport::Stdio { .. } => (self.mcp_stdio, "stdio"),
                McpTransport::Http { .. } => (self.mcp_http, "HTTP"),
                McpTransport::Sse { .. } => (self.mcp_sse, "SSE"),
            };
            if !supported {
                return Err(format!(
                    "MCP server '{}' needs {transport} transport, but this agent did not advertise that capability",
                    server.name
                ));
            }
        }
        Ok(())
    }
}

impl ResumeSupport {
    pub fn any(self) -> bool {
        self.resume || self.load
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeCapabilities {
    pub resume: ResumeSupport,
    pub steering: SteerSupport,
    pub stop: StopSupport,
    pub mcp_stdio: bool,
    pub mcp_http: bool,
    pub mcp_sse: bool,
    pub image_input: Support,
    pub tool_approval: Support,
    /// Switching model/mode/config after creation. Optional and unstable in ACP, so adapters
    /// report `Unverified` rather than guessing; errors are surfaced to the user.
    pub model_options: Support,
}

/// Identity and capabilities learned when a backend is initialized.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeInit {
    pub identity: RuntimeIdentity,
    pub capabilities: RuntimeCapabilities,
}

/// One provider-neutral prompt content block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeContent {
    Text(String),
    Image { data: String, mime_type: String },
}

impl RuntimeContent {
    pub fn text(text: impl Into<String>) -> Self {
        RuntimeContent::Text(text.into())
    }
}

/// Failure of a backend operation, independent of the wire protocol.
#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeError {
    /// The backend connection is closed.
    Closed,
    CommandNotFound(String),
    Spawn(String),
    Decode(String),
    /// The backend answered with an explicit error.
    Provider {
        code: i64,
        message: String,
        data: Option<Value>,
    },
    /// The operation needs a backend capability that was not advertised.
    Unsupported(String),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&AcpError::from(self.clone()), f)
    }
}

impl std::error::Error for RuntimeError {}

/// Public-API compatibility: Engine's submit surface and napi/server consumers still see
/// `AcpError`.
impl From<RuntimeError> for AcpError {
    fn from(error: RuntimeError) -> Self {
        use serde::de::Error as _;
        match error {
            RuntimeError::Closed => AcpError::Closed,
            RuntimeError::CommandNotFound(command) => AcpError::CommandNotFound(command),
            RuntimeError::Spawn(message) => AcpError::Spawn(std::io::Error::other(message)),
            RuntimeError::Decode(message) => AcpError::Decode(serde_json::Error::custom(message)),
            RuntimeError::Provider {
                code,
                message,
                data,
            } => AcpError::Rpc(RpcError {
                code,
                message,
                data,
            }),
            RuntimeError::Unsupported(message) => AcpError::Rpc(RpcError::new(-32601, message)),
        }
    }
}

impl From<AcpError> for RuntimeError {
    fn from(error: AcpError) -> Self {
        match error {
            AcpError::Closed => RuntimeError::Closed,
            AcpError::CommandNotFound(command) => RuntimeError::CommandNotFound(command),
            AcpError::Spawn(error) => RuntimeError::Spawn(error.to_string()),
            AcpError::Decode(error) => RuntimeError::Decode(error.to_string()),
            AcpError::Rpc(error) => RuntimeError::Provider {
                code: error.code,
                message: error.message,
                data: error.data,
            },
        }
    }
}

/// Inputs to creating a backend session. MCP servers are neutral; each backend encodes them for
/// its own protocol and rejects transports it did not advertise.
#[derive(Debug, Clone, Default)]
pub struct RuntimeSessionStart {
    pub cwd: String,
    pub mcp_servers: Vec<McpServer>,
    /// The session's current permission posture. ACP ignores it (Engine mediates every request);
    /// native backends use it to pick the provider's own sandbox ceiling.
    pub execution: ExecutionPolicy,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeSessionRestore {
    pub backend_session_id: String,
    pub cwd: String,
    pub mcp_servers: Vec<McpServer>,
    pub execution: ExecutionPolicy,
}

/// Model catalogue the backend reported when the session was created or restored.
#[derive(Debug, Clone, Default)]
pub struct RuntimeModels {
    pub available: Vec<ModelChoice>,
    pub current: String,
}

/// What a backend reported about a created/restored session. `backend_session_id` is the
/// provider's own identifier; it never replaces the C2 session id.
#[derive(Debug, Clone)]
pub struct RuntimeSessionState {
    pub backend_session_id: String,
    pub models: Option<RuntimeModels>,
    pub config_options: Vec<ConfigOptionInfo>,
}

/// Why a provider ended a turn that it did complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnTerminal {
    EndTurn,
    MaxTokens,
    MaxTurnRequests,
    Refusal,
    Cancelled,
    /// The provider reported a terminal reason this contract does not name.
    Other,
}

impl TurnTerminal {
    /// A cancelled turn is intentionally incomplete and must not be memorialized as an answer.
    pub fn is_cancelled(self) -> bool {
        self == TurnTerminal::Cancelled
    }

    pub fn label(self) -> &'static str {
        match self {
            TurnTerminal::EndTurn => "EndTurn",
            TurnTerminal::MaxTokens => "MaxTokens",
            TurnTerminal::MaxTurnRequests => "MaxTurnRequests",
            TurnTerminal::Refusal => "Refusal",
            TurnTerminal::Cancelled => "Cancelled",
            TurnTerminal::Other => "Unknown",
        }
    }
}

/// Result of sending one turn. The variants are the only honest outcomes a transport can prove.
#[derive(Debug, Clone, PartialEq)]
pub enum TurnOutcome {
    /// The provider reported a terminal for the turn.
    Terminal(TurnTerminal),
    /// The request provably never left this process; a fresh send is safe.
    NotSent(RuntimeError),
    /// The provider received the request and answered with an error; the turn did not run.
    Rejected(RuntimeError),
    /// The request may have been transmitted but no terminal evidence arrived (connection lost,
    /// undecodable answer). The turn may have partially or fully run. Must not be replayed
    /// blindly; reconcile through persisted attempt state.
    Unknown(RuntimeError),
}

/// Result of a steering request.
#[derive(Debug, Clone, PartialEq)]
pub enum SteerOutcome {
    /// The backend's transport confirmed delivery of the input (`outcome` is its own label).
    /// This is a delivery receipt only: it does not prove the worker adopted the input into its
    /// work. Adoption stays the existing coordination-confirm step owned by Engine/delivery.
    Delivered {
        outcome: String,
    },
    /// The backend answered but reported it did not take the input.
    Declined {
        outcome: String,
    },
    NotSent(RuntimeError),
    Rejected(RuntimeError),
    /// Transmission may have happened; the input may or may not have been applied.
    Unknown(RuntimeError),
}

/// Content-free lifecycle state, independent of the backend wire protocol.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeProcessDiagnostics {
    pub started_at_unix_ms: i64,
    pub closed_at_unix_ms: Option<i64>,
    pub termination_requested: bool,
}

/// One bounded, content-free backend protocol anomaly category.
#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct RuntimeProtocolAnomaly {
    pub category: String,
    pub count: u64,
}

/// Decode/notification anomalies observed on one provider connection.
///
/// Only operation and event discriminators are retained. Raw JSON, prompt text, tool
/// payloads, paths, environment variables, and error strings are deliberately excluded so this
/// snapshot is safe to feed into the default diagnostics export.
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct RuntimeProtocolDiagnostics {
    pub outbound_requests: u64,
    pub outbound_request_methods: Vec<RuntimeProtocolAnomaly>,
    pub outbound_notifications: u64,
    pub outbound_notification_methods: Vec<RuntimeProtocolAnomaly>,
    pub outbound_rpc_errors: u64,
    pub outbound_rpc_error_codes: Vec<RuntimeProtocolAnomaly>,
    pub recent_outbound_methods: Vec<String>,
    pub malformed_json_lines: u64,
    pub unhandled_session_updates: u64,
    pub unhandled_session_update_kinds: Vec<RuntimeProtocolAnomaly>,
    pub ignored_notifications: u64,
    pub ignored_notification_methods: Vec<RuntimeProtocolAnomaly>,
}

/// Content-free process and protocol health for support diagnostics.
#[derive(Debug, Clone)]
pub struct RuntimeDiagnostics {
    pub process: RuntimeProcessDiagnostics,
    pub protocol: RuntimeProtocolDiagnostics,
}

/// The operations Engine consumes from a provider backend. Slow calls (`start_session`,
/// `restore_session`, `send_turn`, `steer`, model/config changes) are awaited by Engine outside
/// its shared gates; `request_stop` and `terminate` are synchronous and never wait on a provider
/// so Stop is never queued behind an unrelated slow call.
#[async_trait]
pub trait ProviderRuntime: Send + Sync + 'static {
    fn kind(&self) -> RuntimeBackendKind;

    /// Identity and capabilities as of the last [`ProviderRuntime::initialize`]; conservative
    /// defaults (nothing supported) before that.
    fn negotiated(&self) -> RuntimeInit;

    /// Negotiate with the backend. Must be called once before sessions are created.
    async fn initialize(&self) -> Result<RuntimeInit, RuntimeError>;

    async fn start_session(
        &self,
        request: RuntimeSessionStart,
    ) -> Result<RuntimeSessionState, RuntimeError>;

    /// Re-attach a previous backend session using the least lossy advertised method. While a
    /// history-replaying method runs, `replaying` is set so Engine's handler ignores the replay.
    async fn restore_session(
        &self,
        request: RuntimeSessionRestore,
        replaying: &AtomicBool,
    ) -> Result<RuntimeSessionState, RuntimeError>;

    /// Send one turn and await the provider's terminal. The returned outcome never claims more
    /// than the transport proved; see [`TurnOutcome`].
    async fn send_turn(
        &self,
        backend_session_id: &str,
        content: Vec<RuntimeContent>,
    ) -> TurnOutcome;

    /// Add input to the running turn. Only valid when `capabilities().steering` is `Native`.
    async fn steer(&self, backend_session_id: &str, content: Vec<RuntimeContent>) -> SteerOutcome;

    /// Request a stop of the running turn. Success means the request was handed to the
    /// transport, not that the provider stopped (see [`StopSupport`]).
    fn request_stop(&self, backend_session_id: &str) -> Result<(), RuntimeError>;

    async fn set_model(&self, backend_session_id: &str, model_id: &str)
        -> Result<(), RuntimeError>;

    async fn set_mode(&self, backend_session_id: &str, mode_id: &str) -> Result<(), RuntimeError>;

    async fn set_config_option(
        &self,
        backend_session_id: &str,
        config_id: &str,
        value: &str,
    ) -> Result<Vec<ConfigOptionInfo>, RuntimeError>;

    /// Update the permission posture applied to later turns. Backends that only ever ask the
    /// host (ACP) have nothing to change.
    fn set_execution_policy(&self, _backend_session_id: &str, _policy: ExecutionPolicy) {}

    fn diagnostics(&self) -> RuntimeDiagnostics;

    /// Release the backend's process/connection without waiting. Pending sends fail and are
    /// reported as `Unknown` (if already queued) rather than silently dropped.
    fn terminate(&self);
}


// ---- backend → host callbacks ---------------------------------------------------------------

/// One tool invocation as a backend reports it. Used for both the initial announcement and later
/// partial updates; an update leaves unchanged fields `None`.
///
/// Vocabulary (owned by Engine's tool projection, not by any wire protocol):
/// - `kind`: `read | edit | delete | move | search | execute | think | fetch | other`.
/// - `status`: `pending | in_progress | completed | failed`.
/// - `raw_input`: the provider's own argument object. MCP calls carry `server` and `tool` keys so
///   Engine can recognise host-brokered tools.
/// - `content`: an array of `{"type":"content","content":{"type":"text","text":..}}`,
///   `{"type":"diff","path":..,"oldText":..,"newText":..}` or `{"type":"terminal",..}` entries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuntimeToolCall {
    pub id: String,
    pub title: Option<String>,
    pub kind: Option<String>,
    pub status: Option<String>,
    pub content: Option<Value>,
    pub raw_input: Option<Value>,
    pub raw_output: Option<Value>,
}

/// A streamed fact from a backend session. Engine turns these into domain events and persisted
/// transcript parts; backends never write either themselves.
#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    AgentText(String),
    AgentThought(String),
    ToolCall(RuntimeToolCall),
    ToolUpdate(RuntimeToolCall),
    /// Full replacement set of selectors (model effort etc.).
    ConfigOptions(Vec<ConfigOptionInfo>),
    /// Full replacement set of provider-native slash command names.
    Commands(Vec<String>),
    /// Context window usage. `cost_usd` is only forwarded when the provider reports plain USD.
    Usage {
        used: u64,
        size: u64,
        cost_usd: Option<f64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePermissionOption {
    pub id: String,
    pub name: String,
    /// `allow_once | allow_always | reject_once | reject_always`.
    pub kind: String,
}

/// A parked decision: the backend is blocked until the host answers. Never auto-answered here.
#[derive(Debug, Clone)]
pub struct RuntimePermissionRequest {
    /// Descriptor of the tool call being gated (`toolCallId`, `kind`, `title`, optional `content`).
    pub tool_call: Value,
    pub options: Vec<RuntimePermissionOption>,
    /// Optional context (`is_mcp_tool_approval`, `origin`, `risk`, `application`).
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimePermissionOutcome {
    Selected(String),
    /// Dismissed, expired or unreachable. Backends must treat this as a denial, never an approval.
    Cancelled,
}

/// A structured question to the user (form elicitation).
#[derive(Debug, Clone)]
pub struct RuntimeQuestionRequest {
    pub message: String,
    pub tool_call_id: Option<String>,
    /// JSON Schema (restricted form subset) describing the answer.
    pub schema: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeQuestionOutcome {
    Answered(serde_json::Map<String, Value>),
    /// The user skipped; the backend continues knowing nothing was chosen.
    Declined,
    /// Abort the thing being asked about.
    Cancelled,
}

/// Engine-owned sink a native backend reports into. Slow answers (approvals) park the backend's
/// own request, so implementations may await user input.
#[async_trait]
pub trait RuntimeCallbacks: Send + Sync + 'static {
    async fn event(&self, backend_session_id: &str, event: RuntimeEvent);

    async fn request_permission(
        &self,
        _backend_session_id: &str,
        _request: RuntimePermissionRequest,
    ) -> RuntimePermissionOutcome {
        RuntimePermissionOutcome::Cancelled
    }

    async fn ask_question(
        &self,
        _backend_session_id: &str,
        _request: RuntimeQuestionRequest,
    ) -> RuntimeQuestionOutcome {
        RuntimeQuestionOutcome::Declined
    }
}

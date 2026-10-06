//! Supervised-sidecar connector for official SDKs that only exist as JavaScript packages
//! (Claude Agent SDK, Cursor TypeScript SDK, OpenCode V1/V2).
//!
//! The sidecar is a thin translator under `script/provider-sidecars/<backend>/`: it calls the
//! official SDK, maps SDK events into the C2 sidecar protocol below, and bridges approvals back to
//! the host. It owns no session, scheduling, memory or permission state of its own: Engine owns the
//! C2 session/turn/attempt identities and every decision; the SDK's own store is provider context.
//!
//! # C2 sidecar protocol (version [`SIDECAR_PROTOCOL_VERSION`])
//!
//! JSON-RPC 2.0 lines over the child's stdio, one frame per line (see [`rpc::MAX_FRAME_BYTES`]).
//! The sidecar exits when its stdin closes, and the host kills its process group on terminate.
//!
//! Host → sidecar requests: `initialize`, `session/start`, `session/restore`, `turn/send`,
//! `turn/steer`, `turn/stop`, `session/set_model`, `session/set_mode`, `session/set_config_option`.
//! Notifications: `session/set_execution_policy`, `shutdown`.
//!
//! Sidecar → host: notification `session/event` (`{sessionId, event: {type, ..}}`); requests `host/permission`, `host/question`.
//!
//! `turn/send` is answered only when the SDK reports the turn's terminal:
//! `{"terminal": "end_turn|max_tokens|max_turn_requests|refusal|cancelled|other"}` or
//! `{"failed": {"message": ..}}`. A sidecar error carrying `data.phase == "pre_dispatch"` proves the
//! SDK never saw the prompt ([`TurnOutcome::Rejected`]); every other failure, and a closed pipe
//! after the request was queued, is [`TurnOutcome::Unknown`] and is never replayed here.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock, Weak};

use async_trait::async_trait;
use serde_json::{json, Map, Value};

use crate::connectors::rpc::{spawn_child, ChildGuard, Dialect, RpcFault, RpcInbound, RpcPeer};
use crate::error::RpcError;
use crate::event::{ConfigOptionInfo, ModelChoice};
use crate::permission::{ExecutionPolicy, PermissionMode, SandboxPolicy};
use crate::provider_runtime::{
    ProviderRuntime, ResumeSupport, RuntimeBackendKind, RuntimeCallbacks, RuntimeCapabilities,
    RuntimeContent, RuntimeDiagnostics, RuntimeError, RuntimeEvent, RuntimeIdentity, RuntimeInit,
    RuntimeModels, RuntimePermissionOption, RuntimePermissionOutcome, RuntimePermissionRequest,
    RuntimeProcessDiagnostics, RuntimeQuestionOutcome, RuntimeQuestionRequest,
    RuntimeSessionRestore, RuntimeSessionStart, RuntimeSessionState, RuntimeToolCall,
    SteerOutcome, SteerSupport, StopSupport, Support, TurnOutcome, TurnTerminal,
    RUNTIME_CONTRACT_VERSION,
};
use crate::skill::{McpServer, McpTransport};

pub const SIDECAR_PROTOCOL_VERSION: u32 = 1;

/// How to start one sidecar.
#[derive(Debug, Clone)]
pub struct SidecarLaunch {
    pub backend: RuntimeBackendKind,
    /// JavaScript runtime (`node` or `bun`).
    pub runtime: PathBuf,
    /// Script and its arguments.
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    pub client_version: String,
}

struct SessionState {
    turn_active: bool,
}

struct Shared {
    callbacks: Arc<dyn RuntimeCallbacks>,
    sessions: Mutex<std::collections::HashMap<String, SessionState>>,
    negotiated: RwLock<RuntimeInit>,
    peer: Mutex<Weak<RpcPeer>>,
}

pub struct SidecarRuntime {
    backend: RuntimeBackendKind,
    peer: Arc<RpcPeer>,
    shared: Arc<Shared>,
    child: Option<ChildGuard>,
    client_version: String,
}

pub async fn launch(
    spec: &SidecarLaunch,
    callbacks: Arc<dyn RuntimeCallbacks>,
) -> Result<Arc<SidecarRuntime>, RuntimeError> {
    let spawned = spawn_child(
        &spec.runtime,
        &spec.args,
        &spec.env,
        spec.cwd.as_deref(),
        "provider-sidecar",
    )?;
    Ok(connect(
        spec.backend,
        spawned.stdout,
        spawned.stdin,
        Some(spawned.guard),
        callbacks,
        spec.client_version.clone(),
    ))
}

pub fn connect<R, W>(
    backend: RuntimeBackendKind,
    reader: R,
    writer: W,
    child: Option<ChildGuard>,
    callbacks: Arc<dyn RuntimeCallbacks>,
    client_version: String,
) -> Arc<SidecarRuntime>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let shared = Arc::new(Shared {
        callbacks,
        sessions: Mutex::new(Default::default()),
        negotiated: RwLock::new(RuntimeInit {
            identity: RuntimeIdentity {
                backend,
                contract_version: RUNTIME_CONTRACT_VERSION,
                protocol_version: i64::from(SIDECAR_PROTOCOL_VERSION),
                ..RuntimeIdentity::default()
            },
            capabilities: RuntimeCapabilities::default(),
        }),
        peer: Mutex::new(Weak::new()),
    });
    let peer = RpcPeer::new(
        reader,
        writer,
        Arc::new(Inbound {
            shared: shared.clone(),
        }),
        Dialect::Versioned,
    );
    *shared.peer.lock().unwrap() = Arc::downgrade(&peer);
    Arc::new(SidecarRuntime {
        backend,
        peer,
        shared,
        child,
        client_version,
    })
}

// ---- encoding ----------------------------------------------------------------------------------

pub fn encode_content(content: Vec<RuntimeContent>) -> Vec<Value> {
    content
        .into_iter()
        .map(|block| match block {
            RuntimeContent::Text(text) => json!({"type": "text", "text": text}),
            RuntimeContent::Image { data, mime_type } => {
                json!({"type": "image", "data": data, "mimeType": mime_type})
            }
        })
        .collect()
}

pub fn encode_mcp_servers(servers: &[McpServer]) -> Vec<Value> {
    servers
        .iter()
        .map(|server| {
            let mut entry = Map::new();
            entry.insert("name".into(), json!(server.name));
            match &server.transport {
                McpTransport::Stdio { command, args, env } => {
                    entry.insert("type".into(), json!("stdio"));
                    entry.insert("command".into(), json!(command));
                    entry.insert("args".into(), json!(args));
                    entry.insert(
                        "env".into(),
                        Value::Object(env.iter().map(|(k, v)| (k.clone(), json!(v))).collect()),
                    );
                    if let Some(cwd) = &server.cwd {
                        entry.insert("cwd".into(), json!(cwd));
                    }
                }
                McpTransport::Http { url, headers } | McpTransport::Sse { url, headers } => {
                    let kind = if matches!(server.transport, McpTransport::Sse { .. }) {
                        "sse"
                    } else {
                        "http"
                    };
                    entry.insert("type".into(), json!(kind));
                    entry.insert("url".into(), json!(url));
                    entry.insert(
                        "headers".into(),
                        Value::Object(
                            headers.iter().map(|(k, v)| (k.clone(), json!(v))).collect(),
                        ),
                    );
                }
            }
            Value::Object(entry)
        })
        .collect()
}

pub fn encode_execution(policy: ExecutionPolicy) -> Value {
    json!({
        "mode": match policy.mode {
            PermissionMode::Ask => "ask",
            PermissionMode::AcceptEdits => "accept_edits",
            PermissionMode::Yolo => "yolo",
        },
        "sandbox": match policy.sandbox {
            SandboxPolicy::ReadOnly => "read_only",
            SandboxPolicy::WorkspaceWrite => "workspace_write",
            SandboxPolicy::DangerFullAccess => "danger_full_access",
        },
    })
}

fn support(value: Option<&Value>) -> Support {
    match value.and_then(Value::as_str) {
        Some("supported") => Support::Supported,
        Some("unverified") => Support::Unverified,
        _ => Support::Unsupported,
    }
}

pub fn decode_capabilities(value: &Value) -> RuntimeCapabilities {
    let flag = |key: &str| value.get(key).and_then(Value::as_bool).unwrap_or(false);
    RuntimeCapabilities {
        resume: ResumeSupport {
            resume: flag("resume"),
            load: false,
        },
        steering: if flag("steering") {
            SteerSupport::Native
        } else {
            SteerSupport::Unsupported
        },
        stop: match value.get("stop").and_then(Value::as_str) {
            Some("verified_terminal") => StopSupport::VerifiedTerminal,
            Some("request_only") => StopSupport::RequestOnly,
            _ => StopSupport::Unsupported,
        },
        mcp_stdio: flag("mcpStdio"),
        mcp_http: flag("mcpHttp"),
        mcp_sse: flag("mcpSse"),
        image_input: support(value.get("imageInput")),
        tool_approval: support(value.get("toolApproval")),
        model_options: support(value.get("modelOptions")),
    }
}

fn decode_config_options(value: Option<&Value>) -> Vec<ConfigOptionInfo> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| {
            Some(ConfigOptionInfo {
                id: option.get("id")?.as_str()?.to_string(),
                name: option
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                category: option
                    .get("category")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                current: option
                    .get("current")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                choices: decode_choices(option.get("choices")),
            })
        })
        .collect()
}

fn decode_choices(value: Option<&Value>) -> Vec<ModelChoice> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|choice| {
            let id = choice.get("id")?.as_str()?.to_string();
            Some(ModelChoice {
                name: choice
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_string(),
                description: choice
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                id,
            })
        })
        .collect()
}

fn decode_session_state(value: &Value, fallback_id: Option<&str>) -> Result<RuntimeSessionState, RuntimeError> {
    let backend_session_id = value
        .get("sessionId")
        .and_then(Value::as_str)
        .or(fallback_id)
        .ok_or_else(|| RuntimeError::Decode("sidecar returned no session id".into()))?
        .to_string();
    let models = value.get("models").filter(|m| m.is_object()).map(|models| RuntimeModels {
        available: decode_choices(models.get("available")),
        current: models
            .get("current")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    });
    Ok(RuntimeSessionState {
        backend_session_id,
        models,
        config_options: decode_config_options(value.get("configOptions")),
    })
}

fn decode_tool_call(value: &Value) -> Option<RuntimeToolCall> {
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
    let present = |key: &str| value.get(key).filter(|v| !v.is_null()).cloned();
    Some(RuntimeToolCall {
        id: text("id").filter(|id| !id.is_empty())?,
        title: text("title"),
        kind: text("kind"),
        status: text("status"),
        content: present("content"),
        raw_input: present("rawInput"),
        raw_output: present("rawOutput"),
    })
}

/// Events are discriminated by `type`; `kind` is reserved for the tool kind inside tool events.
pub fn decode_event(event: &Value) -> Result<RuntimeEvent, String> {
    let kind = event
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing".to_string())?;
    let text = || event.get("text").and_then(Value::as_str).map(str::to_string);
    match kind {
        "agent_text" => text().map(RuntimeEvent::AgentText).ok_or_else(|| kind.into()),
        "agent_thought" => text().map(RuntimeEvent::AgentThought).ok_or_else(|| kind.into()),
        "tool_call" => decode_tool_call(event)
            .map(RuntimeEvent::ToolCall)
            .ok_or_else(|| kind.into()),
        "tool_update" => decode_tool_call(event)
            .map(RuntimeEvent::ToolUpdate)
            .ok_or_else(|| kind.into()),
        "config_options" => Ok(RuntimeEvent::ConfigOptions(decode_config_options(
            event.get("options"),
        ))),
        "commands" => Ok(RuntimeEvent::Commands(
            event
                .get("names")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|n| n.as_str().map(str::to_string))
                .collect(),
        )),
        "usage" => {
            let used = event.get("used").and_then(Value::as_u64);
            let size = event.get("size").and_then(Value::as_u64);
            match (used, size) {
                (Some(used), Some(size)) => Ok(RuntimeEvent::Usage {
                    used,
                    size,
                    cost_usd: event.get("costUsd").and_then(Value::as_f64),
                }),
                _ => Err(kind.into()),
            }
        }
        other => Err(other.to_string()),
    }
}

fn terminal_from(value: &str) -> TurnTerminal {
    match value {
        "end_turn" => TurnTerminal::EndTurn,
        "max_tokens" => TurnTerminal::MaxTokens,
        "max_turn_requests" => TurnTerminal::MaxTurnRequests,
        "refusal" => TurnTerminal::Refusal,
        "cancelled" => TurnTerminal::Cancelled,
        _ => TurnTerminal::Other,
    }
}

fn pre_dispatch(error: &RpcError) -> bool {
    error
        .data
        .as_ref()
        .and_then(|data| data.get("phase"))
        .and_then(Value::as_str)
        == Some("pre_dispatch")
}

// ---- inbound -----------------------------------------------------------------------------------

struct Inbound {
    shared: Arc<Shared>,
}

#[async_trait]
impl RpcInbound for Inbound {
    async fn notification(&self, method: &str, params: Value) {
        if method != "session/event" {
            if let Some(peer) = self.shared.peer.lock().unwrap().upgrade() {
                peer.record_ignored(method);
            }
            return;
        }
        let Some(session) = params.get("sessionId").and_then(Value::as_str) else {
            return;
        };
        if !self.shared.sessions.lock().unwrap().contains_key(session) {
            return;
        }
        let Some(event) = params.get("event") else {
            return;
        };
        match decode_event(event) {
            Ok(event) => self.shared.callbacks.event(session, event).await,
            Err(category) => {
                if let Some(peer) = self.shared.peer.lock().unwrap().upgrade() {
                    peer.record_ignored(&format!("session/event.{category}"));
                }
            }
        }
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let session = params
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let known = self.shared.sessions.lock().unwrap().contains_key(&session);
        match method {
            "host/permission" => {
                if !known {
                    return Ok(json!({"outcome": "cancelled"}));
                }
                let options = params
                    .get("options")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|option| {
                        Some(RuntimePermissionOption {
                            id: option.get("id")?.as_str()?.to_string(),
                            name: option.get("name")?.as_str()?.to_string(),
                            kind: option.get("kind")?.as_str()?.to_string(),
                        })
                    })
                    .collect::<Vec<_>>();
                let offered = options.iter().map(|o| o.id.clone()).collect::<Vec<_>>();
                let outcome = self
                    .shared
                    .callbacks
                    .request_permission(
                        &session,
                        RuntimePermissionRequest {
                            tool_call: params.get("toolCall").cloned().unwrap_or(Value::Null),
                            options,
                            meta: params.get("meta").filter(|m| !m.is_null()).cloned(),
                        },
                    )
                    .await;
                Ok(match outcome {
                    // Only an option the sidecar actually offered can be selected.
                    RuntimePermissionOutcome::Selected(id) if offered.contains(&id) => {
                        json!({"outcome": "selected", "optionId": id})
                    }
                    _ => json!({"outcome": "cancelled"}),
                })
            }
            "host/question" => {
                if !known {
                    return Ok(json!({"action": "cancel"}));
                }
                let answer = self
                    .shared
                    .callbacks
                    .ask_question(
                        &session,
                        RuntimeQuestionRequest {
                            message: params
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            tool_call_id: params
                                .get("toolCallId")
                                .and_then(Value::as_str)
                                .map(str::to_string),
                            schema: params.get("schema").cloned().unwrap_or(Value::Null),
                        },
                    )
                    .await;
                Ok(match answer {
                    RuntimeQuestionOutcome::Answered(content) => {
                        json!({"action": "accept", "content": content})
                    }
                    RuntimeQuestionOutcome::Declined => json!({"action": "decline"}),
                    RuntimeQuestionOutcome::Cancelled => json!({"action": "cancel"}),
                })
            }
            other => Err(RpcError::method_not_found(other)),
        }
    }
}

// ---- runtime -----------------------------------------------------------------------------------

impl SidecarRuntime {
    async fn call(&self, method: &str, params: Value) -> Result<Value, RuntimeError> {
        self.peer
            .request(method, params)
            .await
            .map_err(RpcFault::into_runtime_error)
    }

    fn register(&self, id: &str) {
        self.shared
            .sessions
            .lock()
            .unwrap()
            .insert(id.to_string(), SessionState { turn_active: false });
    }

    fn clear_turn(&self, id: &str) {
        if let Some(state) = self.shared.sessions.lock().unwrap().get_mut(id) {
            state.turn_active = false;
        }
    }

    fn session_params(
        &self,
        cwd: &str,
        servers: &[McpServer],
        execution: ExecutionPolicy,
    ) -> Result<Map<String, Value>, RuntimeError> {
        self.negotiated()
            .capabilities
            .validate_mcp(servers)
            .map_err(RuntimeError::Unsupported)?;
        let mut params = Map::new();
        params.insert("cwd".into(), json!(cwd));
        params.insert("mcpServers".into(), Value::Array(encode_mcp_servers(servers)));
        params.insert("execution".into(), encode_execution(execution));
        Ok(params)
    }
}

#[async_trait]
impl ProviderRuntime for SidecarRuntime {
    fn kind(&self) -> RuntimeBackendKind {
        self.backend
    }

    fn negotiated(&self) -> RuntimeInit {
        self.shared.negotiated.read().unwrap().clone()
    }

    async fn initialize(&self) -> Result<RuntimeInit, RuntimeError> {
        let response = self
            .call(
                "initialize",
                json!({
                    "protocol": SIDECAR_PROTOCOL_VERSION,
                    "backend": self.backend.as_str(),
                    "clientVersion": self.client_version,
                }),
            )
            .await?;
        let protocol = response.get("protocol").and_then(Value::as_u64);
        if protocol != Some(u64::from(SIDECAR_PROTOCOL_VERSION)) {
            return Err(RuntimeError::Unsupported(format!(
                "sidecar speaks protocol {protocol:?}, expected {SIDECAR_PROTOCOL_VERSION}"
            )));
        }
        if response.get("backend").and_then(Value::as_str) != Some(self.backend.as_str()) {
            return Err(RuntimeError::Unsupported(
                "sidecar serves a different backend than requested".into(),
            ));
        }
        let adapter = |key: &str| {
            response
                .pointer(key)
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        let init = RuntimeInit {
            identity: RuntimeIdentity {
                backend: self.backend,
                contract_version: RUNTIME_CONTRACT_VERSION,
                protocol_version: i64::from(SIDECAR_PROTOCOL_VERSION),
                adapter_name: adapter("/sdk/name"),
                adapter_version: adapter("/sdk/version"),
            },
            capabilities: decode_capabilities(response.get("capabilities").unwrap_or(&Value::Null)),
        };
        *self.shared.negotiated.write().unwrap() = init.clone();
        Ok(init)
    }

    async fn start_session(
        &self,
        request: RuntimeSessionStart,
    ) -> Result<RuntimeSessionState, RuntimeError> {
        let params = self.session_params(&request.cwd, &request.mcp_servers, request.execution)?;
        let response = self.call("session/start", Value::Object(params)).await?;
        let state = decode_session_state(&response, None)?;
        self.register(&state.backend_session_id);
        Ok(state)
    }

    async fn restore_session(
        &self,
        request: RuntimeSessionRestore,
        _replaying: &AtomicBool,
    ) -> Result<RuntimeSessionState, RuntimeError> {
        if !self.negotiated().capabilities.resume.resume {
            return Err(RuntimeError::Unsupported(
                "this SDK connector does not support resuming a session".into(),
            ));
        }
        let mut params =
            self.session_params(&request.cwd, &request.mcp_servers, request.execution)?;
        params.insert("sessionId".into(), json!(request.backend_session_id));
        let response = self.call("session/restore", Value::Object(params)).await?;
        let state = decode_session_state(&response, Some(&request.backend_session_id))?;
        if state.backend_session_id != request.backend_session_id {
            return Err(RuntimeError::Decode(
                "sidecar restored a different session than requested".into(),
            ));
        }
        self.register(&state.backend_session_id);
        Ok(state)
    }

    async fn send_turn(
        &self,
        backend_session_id: &str,
        content: Vec<RuntimeContent>,
    ) -> TurnOutcome {
        {
            let mut sessions = self.shared.sessions.lock().unwrap();
            let Some(state) = sessions.get_mut(backend_session_id) else {
                return TurnOutcome::NotSent(RuntimeError::Unsupported("unknown session".into()));
            };
            if state.turn_active {
                return TurnOutcome::NotSent(RuntimeError::Unsupported(
                    "a turn is already running on this session".into(),
                ));
            }
            state.turn_active = true;
        }
        let result = self
            .peer
            .request(
                "turn/send",
                json!({"sessionId": backend_session_id, "content": encode_content(content)}),
            )
            .await;
        self.clear_turn(backend_session_id);
        match result {
            Err(RpcFault::NotQueued) => TurnOutcome::NotSent(RuntimeError::Closed),
            Err(fault @ RpcFault::Unresolved) => TurnOutcome::Unknown(fault.into_runtime_error()),
            Err(RpcFault::Provider(error)) => {
                let proven = pre_dispatch(&error);
                let error = RpcFault::Provider(error).into_runtime_error();
                if proven {
                    TurnOutcome::Rejected(error)
                } else {
                    TurnOutcome::Unknown(error)
                }
            }
            Ok(response) => {
                if let Some(failed) = response.get("failed") {
                    let message = failed
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("provider turn failed")
                        .to_string();
                    return TurnOutcome::Failed(RuntimeError::Provider {
                        code: -1,
                        message,
                        data: None,
                    });
                }
                match response.get("terminal").and_then(Value::as_str) {
                    Some(terminal) => TurnOutcome::Terminal(terminal_from(terminal)),
                    None => TurnOutcome::Unknown(RuntimeError::Decode(
                        "turn/send answered without a terminal".into(),
                    )),
                }
            }
        }
    }

    async fn steer(&self, backend_session_id: &str, content: Vec<RuntimeContent>) -> SteerOutcome {
        if self.negotiated().capabilities.steering != SteerSupport::Native {
            return SteerOutcome::NotSent(RuntimeError::Unsupported(
                "this SDK connector does not support live steering".into(),
            ));
        }
        let running = self
            .shared
            .sessions
            .lock()
            .unwrap()
            .get(backend_session_id)
            .is_some_and(|state| state.turn_active);
        if !running {
            return SteerOutcome::NotSent(RuntimeError::Unsupported(
                "no running turn to steer".into(),
            ));
        }
        match self
            .peer
            .request(
                "turn/steer",
                json!({"sessionId": backend_session_id, "content": encode_content(content)}),
            )
            .await
        {
            Ok(response) => match response.get("receipt").and_then(Value::as_str) {
                Some("delivered") => SteerOutcome::Delivered {
                    outcome: "delivered".into(),
                },
                Some("declined") => SteerOutcome::Declined {
                    outcome: "declined".into(),
                },
                // `queued`, `unconfirmed`, or anything else the SDK cannot prove was applied.
                _ => SteerOutcome::Unknown(RuntimeError::Decode(
                    "steering returned no positive delivery receipt".into(),
                )),
            },
            Err(RpcFault::NotQueued) => SteerOutcome::NotSent(RuntimeError::Closed),
            Err(RpcFault::Unresolved) => SteerOutcome::Unknown(RuntimeError::Closed),
            Err(RpcFault::Provider(error)) => {
                let proven = pre_dispatch(&error);
                let error = RpcFault::Provider(error).into_runtime_error();
                if proven {
                    SteerOutcome::Rejected(error)
                } else {
                    SteerOutcome::Unknown(error)
                }
            }
        }
    }

    fn request_stop(&self, backend_session_id: &str) -> Result<(), RuntimeError> {
        if self.peer.is_closed() {
            return Err(RuntimeError::Closed);
        }
        let running = self
            .shared
            .sessions
            .lock()
            .unwrap()
            .get(backend_session_id)
            .is_some_and(|state| state.turn_active);
        if !running {
            return Ok(());
        }
        let peer = self.peer.clone();
        let session = backend_session_id.to_string();
        tokio::spawn(async move {
            // The terminal is the answer to `turn/send`; this acknowledgement proves nothing.
            if let Err(fault) = peer.request("turn/stop", json!({"sessionId": session})).await {
                tracing::debug!(?fault, "sidecar turn/stop was not acknowledged");
            }
        });
        Ok(())
    }

    async fn set_model(
        &self,
        backend_session_id: &str,
        model_id: &str,
    ) -> Result<(), RuntimeError> {
        self.call(
            "session/set_model",
            json!({"sessionId": backend_session_id, "modelId": model_id}),
        )
        .await
        .map(|_| ())
    }

    async fn set_mode(&self, backend_session_id: &str, mode_id: &str) -> Result<(), RuntimeError> {
        self.call(
            "session/set_mode",
            json!({"sessionId": backend_session_id, "modeId": mode_id}),
        )
        .await
        .map(|_| ())
    }

    async fn set_config_option(
        &self,
        backend_session_id: &str,
        config_id: &str,
        value: &str,
    ) -> Result<Vec<ConfigOptionInfo>, RuntimeError> {
        let response = self
            .call(
                "session/set_config_option",
                json!({"sessionId": backend_session_id, "configId": config_id, "value": value}),
            )
            .await?;
        Ok(decode_config_options(response.get("configOptions")))
    }

    fn set_execution_policy(&self, backend_session_id: &str, policy: ExecutionPolicy) {
        let _ = self.peer.notify(
            "session/set_execution_policy",
            json!({"sessionId": backend_session_id, "execution": encode_execution(policy)}),
        );
    }

    fn diagnostics(&self) -> RuntimeDiagnostics {
        RuntimeDiagnostics {
            process: match &self.child {
                Some(child) => child.process_diagnostics(&self.peer),
                None => RuntimeProcessDiagnostics {
                    started_at_unix_ms: 0,
                    closed_at_unix_ms: self.peer.closed_at_unix_ms(),
                    termination_requested: false,
                },
            },
            protocol: self.peer.protocol_diagnostics(),
        }
    }

    fn terminate(&self) {
        let _ = self.peer.notify("shutdown", Value::Null);
        if let Some(child) = &self.child {
            child.terminate();
        }
        self.peer.close();
    }
}

impl Drop for SidecarRuntime {
    fn drop(&mut self) {
        self.terminate();
    }
}

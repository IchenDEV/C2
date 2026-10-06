//! ACP backend for the provider-neutral runtime.
//!
//! Wraps an [`AcpClient`] and translates between neutral runtime types and ACP wire types. All
//! ACP DTO knowledge (session/new, session/load, session/resume, session/prompt, the
//! `_session/steering` extension, content blocks, stop reasons, config options) lives here.
//!
//! Transmission honesty: `session/prompt` and `_session/steering` use
//! [`Connection::request_phased`], so a failure keeps whether the request was ever queued. A
//! request that was never queued is `NotSent`; a queued request whose connection closed is
//! `Unknown`; a generic JSON-RPC error also remains `Unknown` because it cannot prove no execution. ACP has no separate "accepted" signal for
//! a prompt: the response *is* the terminal, so this adapter exposes no early acceptance event.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use serde_json::Value;

use crate::acp::wire::{
    AgentCaps, ContentBlock, LoadSessionResponse, PromptRequest, PromptResponse,
    SessionConfigOption, SessionModelState, StopReason,
};
use crate::acp::{self, AcpClient, ClientHandler, RequestFault};
use crate::event::{ConfigOptionInfo, ModelChoice};
use crate::provider::LaunchSpec;
use crate::provider_runtime::{
    ProviderRuntime, ResumeSupport, RuntimeBackendKind, RuntimeCapabilities, RuntimeContent,
    RuntimeDiagnostics, RuntimeError, RuntimeIdentity, RuntimeInit, RuntimeModels,
    RuntimeProcessDiagnostics, RuntimeProtocolAnomaly, RuntimeProtocolDiagnostics,
    RuntimeSessionRestore, RuntimeSessionStart, RuntimeSessionState, SteerOutcome, SteerSupport,
    StopSupport, Support, TurnOutcome, TurnTerminal, RUNTIME_CONTRACT_VERSION,
};
use crate::skill::{McpServer, McpTransport};

/// Capabilities C2 advertises to ACP agents.
///
/// Only form elicitation so far. It matters more than it looks: the Claude adapter routes its
/// built-in `AskUserQuestion` tool through `elicitation/create` *only* when this is advertised,
/// and otherwise degrades the whole question into an allow/reject permission prompt that shows the
/// tool's name and none of its questions. URL elicitation stays unadvertised — we have nowhere
/// honest to send the user — and `fs` remains unclaimed, so agents keep doing their own file I/O.
pub(crate) fn client_capabilities() -> Value {
    serde_json::json!({"elicitation": {"form": {}}})
}

#[derive(Clone, Copy, Default)]
struct Negotiated {
    init_caps: AgentCaps,
}

/// One live ACP provider process behind the neutral runtime interface.
pub struct AcpRuntime {
    client: AcpClient,
    negotiated: RwLock<(RuntimeInit, Negotiated)>,
}

/// Spawn an ACP provider subprocess. [`ProviderRuntime::initialize`] must still be called.
/// `handler` is the agent→client callback seam (still ACP-shaped; see module docs of
/// `provider_runtime`).
pub async fn launch(
    spec: &LaunchSpec,
    handler: Arc<dyn ClientHandler>,
) -> Result<Arc<AcpRuntime>, RuntimeError> {
    let client = acp::spawn(spec, handler).await?;
    Ok(Arc::new(AcpRuntime::new(client)))
}

impl AcpRuntime {
    pub fn new(client: AcpClient) -> Self {
        Self {
            client,
            negotiated: RwLock::new((
                RuntimeInit {
                    identity: RuntimeIdentity {
                        backend: RuntimeBackendKind::Acp,
                        contract_version: RUNTIME_CONTRACT_VERSION,
                        ..RuntimeIdentity::default()
                    },
                    capabilities: RuntimeCapabilities::default(),
                },
                Negotiated::default(),
            )),
        }
    }

    fn caps(&self) -> AgentCaps {
        self.negotiated.read().unwrap().1.init_caps
    }
}

/// Neutral content to ACP content. Total: every neutral block has an ACP encoding.
fn to_wire(content: Vec<RuntimeContent>) -> Vec<ContentBlock> {
    content
        .into_iter()
        .map(|block| match block {
            RuntimeContent::Text(text) => ContentBlock::Text { text },
            RuntimeContent::Image { data, mime_type } => ContentBlock::Image { data, mime_type },
        })
        .collect()
}

/// ACP content to neutral content. ACP `resource` blocks have no neutral form and are dropped;
/// Engine never produces them.
pub fn content_from_wire(blocks: Vec<ContentBlock>) -> Vec<RuntimeContent> {
    blocks
        .into_iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(RuntimeContent::Text(text)),
            ContentBlock::Image { data, mime_type } => {
                Some(RuntimeContent::Image { data, mime_type })
            }
            ContentBlock::Resource { .. } => None,
        })
        .collect()
}

fn terminal_from_wire(stop: StopReason) -> TurnTerminal {
    match stop {
        StopReason::EndTurn => TurnTerminal::EndTurn,
        StopReason::MaxTokens => TurnTerminal::MaxTokens,
        StopReason::MaxTurnRequests => TurnTerminal::MaxTurnRequests,
        StopReason::Refusal => TurnTerminal::Refusal,
        StopReason::Cancelled => TurnTerminal::Cancelled,
        StopReason::Unknown => TurnTerminal::Other,
    }
}

fn fault_error(fault: &RequestFault) -> RuntimeError {
    match fault {
        RequestFault::NotQueued | RequestFault::Unresolved => RuntimeError::Closed,
        RequestFault::Provider(error) => RuntimeError::Provider {
            code: error.code,
            message: error.message.clone(),
            data: error.data.clone(),
        },
        RequestFault::Decode(error) => RuntimeError::Decode(error.to_string()),
    }
}

/// Validate and encode MCP servers for `session/new|load|resume`. Stdio is the ACP baseline;
/// remote transports need the matching advertised capability.
pub(crate) fn encode_mcp_servers(
    servers: &[McpServer],
    caps: AgentCaps,
) -> Result<Vec<Value>, String> {
    servers
        .iter()
        .map(|server| {
            let supported = match &server.transport {
                McpTransport::Stdio { .. } => true,
                McpTransport::Http { .. } => caps.mcp_http,
                McpTransport::Sse { .. } => caps.mcp_sse,
            };
            if supported {
                Ok(server.to_acp_json())
            } else {
                let transport = match &server.transport {
                    McpTransport::Http { .. } => "HTTP",
                    McpTransport::Sse { .. } => "SSE",
                    McpTransport::Stdio { .. } => unreachable!(),
                };
                Err(format!(
                    "MCP server '{}' needs {transport} transport, but this agent did not advertise that ACP capability",
                    server.name
                ))
            }
        })
        .collect()
}

fn session_state(
    backend_session_id: String,
    models: Option<SessionModelState>,
    options: Option<Vec<SessionConfigOption>>,
) -> RuntimeSessionState {
    let mut config_options = options
        .as_deref()
        .map(config_option_infos)
        .unwrap_or_default();
    if !config_options
        .iter()
        .any(|option| option.category.as_deref() == Some("thought_level"))
    {
        if let Some(option) = reasoning_option_from_models(models.as_ref()) {
            config_options.push(option);
        }
    }
    RuntimeSessionState {
        backend_session_id,
        models: models.map(|state| RuntimeModels {
            available: state
                .available_models
                .iter()
                .map(|model| ModelChoice {
                    id: model.model_id.clone(),
                    name: model.name.clone(),
                    description: model.description.clone(),
                })
                .collect(),
            current: state.current_model_id,
        }),
        config_options,
    }
}

struct ReplayGuard<'a>(&'a AtomicBool);

impl Drop for ReplayGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[async_trait]
impl ProviderRuntime for AcpRuntime {
    fn kind(&self) -> RuntimeBackendKind {
        RuntimeBackendKind::Acp
    }

    fn negotiated(&self) -> RuntimeInit {
        self.negotiated.read().unwrap().0.clone()
    }

    async fn initialize(&self) -> Result<RuntimeInit, RuntimeError> {
        let response = self.client.initialize(client_capabilities()).await?;
        let caps = response.caps();
        let agent = |value: &str| (!value.is_empty()).then(|| value.to_string());
        let image_input = match response
            .agent_capabilities
            .pointer("/promptCapabilities/image")
            .and_then(Value::as_bool)
        {
            Some(true) => Support::Supported,
            Some(false) => Support::Unsupported,
            None => Support::Unverified,
        };
        let init = RuntimeInit {
            identity: RuntimeIdentity {
                backend: RuntimeBackendKind::Acp,
                contract_version: RUNTIME_CONTRACT_VERSION,
                protocol_version: response.protocol_version,
                adapter_name: response.agent_info.as_ref().and_then(|i| agent(&i.name)),
                adapter_version: response.agent_info.as_ref().and_then(|i| agent(&i.version)),
            },
            capabilities: RuntimeCapabilities {
                resume: ResumeSupport {
                    resume: caps.resume_session,
                    load: caps.load_session,
                },
                steering: if response.interaction_capabilities().steering {
                    SteerSupport::Native
                } else {
                    SteerSupport::Unsupported
                },
                // `session/cancel` is a notification: no acknowledgement, terminal arrives as
                // the prompt's `cancelled` stop reason.
                stop: StopSupport::RequestOnly,
                mcp_stdio: true,
                mcp_http: caps.mcp_http,
                mcp_sse: caps.mcp_sse,
                image_input,
                // Permission requests are part of the ACP baseline.
                tool_approval: Support::Supported,
                // set_model / set_mode / set_config_option are optional UNSTABLE methods.
                model_options: Support::Unverified,
            },
        };
        *self.negotiated.write().unwrap() = (init.clone(), Negotiated { init_caps: caps });
        Ok(init)
    }

    async fn start_session(
        &self,
        request: RuntimeSessionStart,
    ) -> Result<RuntimeSessionState, RuntimeError> {
        let mcp = encode_mcp_servers(&request.mcp_servers, self.caps())
            .map_err(RuntimeError::Unsupported)?;
        let response = self.client.new_session_full(request.cwd, mcp).await?;
        Ok(session_state(
            response.session_id,
            response.models,
            response.config_options,
        ))
    }

    /// Prefer `session/resume` (no history replay); fall back to `session/load` with replay
    /// suppression. A resume failure with no usable fallback reports both causes.
    async fn restore_session(
        &self,
        request: RuntimeSessionRestore,
        replaying: &AtomicBool,
    ) -> Result<RuntimeSessionState, RuntimeError> {
        let caps = self.caps();
        let mcp =
            encode_mcp_servers(&request.mcp_servers, caps).map_err(RuntimeError::Unsupported)?;
        let id = request.backend_session_id.as_str();
        let mut resume_error = None;
        let mut restored: Option<LoadSessionResponse> = None;
        if caps.resume_session {
            match self
                .client
                .resume_session(id, request.cwd.as_str(), mcp.clone())
                .await
            {
                Ok(response) => restored = Some(response),
                Err(error) => resume_error = Some(error.to_string()),
            }
        }
        if restored.is_none() && caps.load_session {
            replaying.store(true, Ordering::SeqCst);
            let guard = ReplayGuard(replaying);
            let loaded = self
                .client
                .load_session(id, request.cwd.as_str(), mcp)
                .await;
            drop(guard);
            match loaded {
                Ok(response) => restored = Some(response),
                Err(load_error) => {
                    return Err(RuntimeError::Unsupported(match resume_error {
                        Some(resume_error) => format!(
                        "session/resume failed: {resume_error}; session/load failed: {load_error}"
                    ),
                        None => format!("session/load failed: {load_error}"),
                    }))
                }
            }
        }
        match restored {
            Some(response) => Ok(session_state(
                request.backend_session_id,
                response.models,
                response.config_options,
            )),
            None => Err(RuntimeError::Unsupported(match resume_error {
                Some(error) => format!("session/resume failed: {error}"),
                None => "provider advertised no session restore capability".into(),
            })),
        }
    }

    async fn send_turn(
        &self,
        backend_session_id: &str,
        content: Vec<RuntimeContent>,
    ) -> TurnOutcome {
        let request = PromptRequest {
            session_id: backend_session_id.to_string(),
            prompt: to_wire(content),
        };
        match self
            .client
            .connection()
            .request_phased::<_, PromptResponse>("session/prompt", request)
            .await
        {
            Ok(response) => TurnOutcome::Terminal(terminal_from_wire(response.stop_reason)),
            Err(fault @ RequestFault::NotQueued) => TurnOutcome::NotSent(fault_error(&fault)),
            Err(fault) => TurnOutcome::Unknown(fault_error(&fault)),
        }
    }

    async fn steer(&self, backend_session_id: &str, content: Vec<RuntimeContent>) -> SteerOutcome {
        if self.negotiated().capabilities.steering != SteerSupport::Native {
            return SteerOutcome::NotSent(RuntimeError::Unsupported(
                "the provider did not advertise native steering".into(),
            ));
        }
        match self
            .client
            .connection()
            .request_phased::<_, Value>(
                "_session/steering",
                serde_json::json!({ "sessionId": backend_session_id, "prompt": to_wire(content) }),
            )
            .await
        {
            Ok(response) => {
                match response.get("outcome").and_then(Value::as_str) {
                    Some(outcome @ ("injected" | "startedNewTurn")) => SteerOutcome::Delivered {
                        outcome: outcome.into(),
                    },
                    // This private extension has no verified negative receipt contract. A
                    // missing or unfamiliar result cannot prove the input was never applied.
                    _ => SteerOutcome::Unknown(RuntimeError::Decode(
                        "ACP steering returned no verified delivery receipt".into(),
                    )),
                }
            }
            Err(fault @ RequestFault::NotQueued) => SteerOutcome::NotSent(fault_error(&fault)),
            Err(fault) => SteerOutcome::Unknown(fault_error(&fault)),
        }
    }

    fn request_stop(&self, backend_session_id: &str) -> Result<(), RuntimeError> {
        Ok(self.client.cancel(backend_session_id)?)
    }

    async fn set_model(
        &self,
        backend_session_id: &str,
        model_id: &str,
    ) -> Result<(), RuntimeError> {
        Ok(self.client.set_model(backend_session_id, model_id).await?)
    }

    async fn set_mode(&self, backend_session_id: &str, mode_id: &str) -> Result<(), RuntimeError> {
        Ok(self.client.set_mode(backend_session_id, mode_id).await?)
    }

    async fn set_config_option(
        &self,
        backend_session_id: &str,
        config_id: &str,
        value: &str,
    ) -> Result<Vec<ConfigOptionInfo>, RuntimeError> {
        let options = self
            .client
            .set_config_option(backend_session_id, config_id, value)
            .await?;
        Ok(config_option_infos(&options))
    }

    fn diagnostics(&self) -> RuntimeDiagnostics {
        RuntimeDiagnostics {
            process: {
                let d = self.client.process_diagnostics();
                RuntimeProcessDiagnostics {
                    started_at_unix_ms: d.started_at_unix_ms,
                    closed_at_unix_ms: d.closed_at_unix_ms,
                    termination_requested: d.termination_requested,
                }
            },
            protocol: {
                let d = self.client.protocol_diagnostics();
                RuntimeProtocolDiagnostics {
                    outbound_requests: d.outbound_requests,
                    outbound_request_methods: diagnostics_categories(d.outbound_request_methods),
                    outbound_notifications: d.outbound_notifications,
                    outbound_notification_methods: diagnostics_categories(
                        d.outbound_notification_methods,
                    ),
                    outbound_rpc_errors: d.outbound_rpc_errors,
                    outbound_rpc_error_codes: diagnostics_categories(d.outbound_rpc_error_codes),
                    recent_outbound_methods: d.recent_outbound_methods,
                    malformed_json_lines: d.malformed_json_lines,
                    unhandled_session_updates: d.unhandled_session_updates,
                    unhandled_session_update_kinds: diagnostics_categories(
                        d.unhandled_session_update_kinds,
                    ),
                    ignored_notifications: d.ignored_notifications,
                    ignored_notification_methods: diagnostics_categories(
                        d.ignored_notification_methods,
                    ),
                }
            },
        }
    }

    fn terminate(&self) {
        self.client.terminate();
    }
}

/// Flatten ACP config options into the frontend shape. Non-select options (booleans we never
/// advertise support for, future types) are dropped — the UI can only render selectors.
pub(crate) fn config_option_infos(
    options: &[crate::acp::wire::SessionConfigOption],
) -> Vec<ConfigOptionInfo> {
    options
        .iter()
        .filter(|o| o.option_type.as_deref().unwrap_or("select") == "select")
        .filter(|o| {
            o.id != "collaboration_mode" && o.category.as_deref() != Some("collaboration_mode")
        })
        .map(|o| ConfigOptionInfo {
            id: o.id.clone(),
            name: o.name.clone(),
            category: o.category.clone(),
            current: o.current().unwrap_or_default(),
            choices: o
                .choices()
                .into_iter()
                .map(|c| ModelChoice {
                    id: c.value,
                    name: c.name,
                    description: c.description,
                })
                .collect(),
        })
        .collect()
}

/// Some agents expose a model-specific effort ladder before ACP's config-options surface. Grok's
/// current ACP server puts it in `ModelInfo._meta.reasoningEfforts` and switches it through the
/// legacy `session/set_mode` method. Turn that provider-owned metadata into the same frontend shape
/// without inventing levels or applying one provider's matrix to another.
pub(crate) fn reasoning_option_from_models(
    models: Option<&crate::acp::wire::SessionModelState>,
) -> Option<ConfigOptionInfo> {
    let models = models?;
    let model = models
        .available_models
        .iter()
        .find(|model| model.model_id == models.current_model_id)
        .or_else(|| models.available_models.first())?;
    let raw = model.meta.get("reasoningEfforts")?.as_array()?;
    let mut choices: Vec<ModelChoice> = raw
        .iter()
        .filter_map(|effort| {
            let id = effort
                .get("value")
                .or_else(|| effort.get("id"))?
                .as_str()?
                .to_string();
            let name = effort
                .get("label")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&id)
                .to_string();
            let description = effort
                .get("description")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            Some(ModelChoice {
                id,
                name,
                description,
            })
        })
        .collect();
    if choices.len() < 2 {
        return None;
    }
    const ORDER: [&str; 9] = [
        "off", "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
    ];
    choices.sort_by_key(|choice| {
        ORDER
            .iter()
            .position(|effort| *effort == choice.id)
            .unwrap_or(ORDER.len())
    });
    let current = model
        .meta
        .get("reasoningEffort")
        .and_then(serde_json::Value::as_str)
        .filter(|current| choices.iter().any(|choice| choice.id == *current))
        .map(str::to_string)
        .or_else(|| {
            raw.iter().find_map(|effort| {
                effort
                    .get("default")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
                    .then(|| {
                        effort
                            .get("value")
                            .or_else(|| effort.get("id"))
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string)
                    })
                    .flatten()
            })
        })
        .unwrap_or_else(|| choices[0].id.clone());
    Some(ConfigOptionInfo {
        id: "reasoning_effort".into(),
        name: "Reasoning Effort".into(),
        category: Some("thought_level".into()),
        current,
        choices,
    })
}

fn diagnostics_categories(
    categories: Vec<crate::acp::AcpProtocolAnomaly>,
) -> Vec<RuntimeProtocolAnomaly> {
    categories
        .into_iter()
        .map(|d| RuntimeProtocolAnomaly {
            category: d.category,
            count: d.count,
        })
        .collect()
}

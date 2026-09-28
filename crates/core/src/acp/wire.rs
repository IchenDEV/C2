//! ACP wire types (the subset we drive today).
//!
//! ACP is JSON-RPC 2.0, newline-delimited over stdio, with user-facing text as Markdown and a data
//! model that reuses MCP's content shapes. We model what the client needs for a prompt turn and stay
//! lenient about the rest (unknown `sessionUpdate` variants are dropped, not fatal) — the plan's
//! "code to the common denominator, feature-detect the rest" guidance.
//!
//! These hand-written serde types keep us independent of any single adapter's version churn; the
//! official `agent-client-protocol` crate can be swapped in later behind [`super::client::AcpClient`].

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// ACP stable protocol version.
pub const PROTOCOL_VERSION: i64 = 1;

// ---- initialize ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeRequest {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: i64,
    #[serde(rename = "clientCapabilities", skip_serializing_if = "Option::is_none")]
    pub client_capabilities: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResponse {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: i64,
    /// The ACP endpoint's own identity. Kept optional for older/custom adapters that predate the
    /// stable field; current Codex ACP reports its package name and exact adapter version here.
    #[serde(rename = "agentInfo", default)]
    pub agent_info: Option<AgentInfo>,
    #[serde(rename = "agentCapabilities", default)]
    pub agent_capabilities: Value,
    #[serde(rename = "authMethods", default)]
    pub auth_methods: Value,
    /// Provider-owned optional extensions such as native steering.
    #[serde(rename = "_meta", default)]
    pub meta: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InteractionCapabilities {
    pub steering: bool,
}

/// The agent capabilities we act on, lifted out of the raw `agentCapabilities` object. Everything
/// defaults to "not supported" — an agent earns a capability by advertising it, never by omission.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgentCaps {
    /// `session/load`: the agent can re-attach a previous session, replaying its history and
    /// restoring the conversation context. This is the provider-native resume cursor — the only
    /// way an agent's context survives its process.
    pub load_session: bool,
    /// `session/resume`: re-attach without replaying history. Prefer this when both sides support
    /// it because C2 already owns the durable transcript and does not need provider history decoded
    /// a second time.
    pub resume_session: bool,
    /// Remote MCP transports are optional; stdio is the ACP baseline.
    pub mcp_http: bool,
    pub mcp_sse: bool,
}

impl InitializeResponse {
    pub fn caps(&self) -> AgentCaps {
        AgentCaps {
            load_session: self
                .agent_capabilities
                .get("loadSession")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            resume_session: self
                .agent_capabilities
                .pointer("/sessionCapabilities/resume")
                .is_some_and(|value| value.is_object() || value.as_bool() == Some(true)),
            mcp_http: self
                .agent_capabilities
                .pointer("/mcpCapabilities/http")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            mcp_sse: self
                .agent_capabilities
                .pointer("/mcpCapabilities/sse")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }
    }

    pub fn interaction_capabilities(&self) -> InteractionCapabilities {
        let steering = self
            .meta
            .pointer("/steering/supported")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        InteractionCapabilities { steering }
    }
}

// ---- session/new ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewSessionRequest {
    pub cwd: String,
    #[serde(rename = "mcpServers", default)]
    pub mcp_servers: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewSessionResponse {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    /// The agent's selectable models, when it reports any. Marked UNSTABLE in the ACP spec and
    /// absent from most adapters today, so this stays optional and its absence is a normal state —
    /// the engine then offers the installed CLI catalogue for the provider instead.
    #[serde(default)]
    pub models: Option<SessionModelState>,
    /// Session config options (UNSTABLE) — where current adapters report the model selector and
    /// the thought/reasoning level. Newer than (and superseding) the `models` field above; both
    /// are read so either generation of adapter works.
    #[serde(rename = "configOptions", default)]
    pub config_options: Option<Vec<SessionConfigOption>>,
}

// ---- session/load --------------------------------------------------------------------------

/// Re-attach a previous session (`session/load`). Gated on [`AgentCaps::load_session`]. The agent
/// replays the whole conversation as `session/update` notifications before answering, then the
/// session accepts prompts with its context intact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadSessionRequest {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub cwd: String,
    #[serde(rename = "mcpServers", default)]
    pub mcp_servers: Vec<Value>,
}

/// `session/load` reports models/config the same way `session/new` does; agents that predate those
/// (UNSTABLE) fields answer with nothing at all, so every field — and the response itself — is
/// optional at the call site.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoadSessionResponse {
    #[serde(default)]
    pub models: Option<SessionModelState>,
    #[serde(rename = "configOptions", default)]
    pub config_options: Option<Vec<SessionConfigOption>>,
}

// ---- session/resume (ACP: UNSTABLE) --------------------------------------------------------

/// Re-attach without replaying history. The request is deliberately separate from
/// [`LoadSessionRequest`] so method-specific schema changes remain visible at the wire boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeSessionRequest {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub cwd: String,
    #[serde(rename = "mcpServers", default)]
    pub mcp_servers: Vec<Value>,
}

/// Current adapters return the same optional model/config projection for load and resume.
pub type ResumeSessionResponse = LoadSessionResponse;

// ---- models (ACP: UNSTABLE) ------------------------------------------------------------------

/// One model the agent can be switched to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    #[serde(rename = "modelId")]
    pub model_id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Provider extensions such as Grok's per-model `reasoningEfforts`. Kept raw because ACP has
    /// not standardized this metadata shape; consumers must feature-detect every field.
    #[serde(rename = "_meta", default)]
    pub meta: Value,
}

/// The models an agent offers, and which one is live.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionModelState {
    #[serde(rename = "availableModels", default)]
    pub available_models: Vec<ModelInfo>,
    #[serde(rename = "currentModelId", default)]
    pub current_model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetModelRequest {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "modelId")]
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetModeRequest {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "modeId")]
    pub mode_id: String,
}

// ---- session config options (ACP: UNSTABLE) --------------------------------------------------

/// One selectable value of a config option.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSelectChoice {
    pub value: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// A session configuration selector (`category` distinguishes model / mode / thought_level).
///
/// Only `type: "select"` carries choices we can render; anything else still parses (lenient, like
/// the rest of this module) but exposes an empty choice list. `options` stays a raw `Value`
/// because the spec allows both a flat list and a grouped one — [`Self::choices`] flattens either.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfigOption {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(rename = "type", default)]
    pub option_type: Option<String>,
    #[serde(rename = "currentValue", default)]
    pub current_value: Option<Value>,
    #[serde(default)]
    pub options: Option<Value>,
}

impl SessionConfigOption {
    /// The currently selected value id, when this is a select option.
    pub fn current(&self) -> Option<String> {
        self.current_value
            .as_ref()
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }

    /// Flatten the select choices, accepting both wire shapes: a flat option list and a list of
    /// `{group, name, options: […]}` groups.
    pub fn choices(&self) -> Vec<ConfigSelectChoice> {
        let Some(Value::Array(items)) = &self.options else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for item in items {
            if let Some(Value::Array(grouped)) = item.get("options") {
                for o in grouped {
                    if let Ok(c) = serde_json::from_value::<ConfigSelectChoice>(o.clone()) {
                        out.push(c);
                    }
                }
            } else if let Ok(c) = serde_json::from_value::<ConfigSelectChoice>(item.clone()) {
                out.push(c);
            }
        }
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetConfigOptionRequest {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "configId")]
    pub config_id: String,
    /// A `SessionConfigValueId` — the "value_id" request variant (no `type` field on the wire).
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetConfigOptionResponse {
    #[serde(rename = "configOptions", default)]
    pub config_options: Vec<SessionConfigOption>,
}

// ---- content blocks (MCP-shaped) -----------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    Resource {
        resource: Value,
    },
}

impl ContentBlock {
    pub fn text(s: impl Into<String>) -> Self {
        ContentBlock::Text { text: s.into() }
    }
}

// ---- session/prompt ------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptRequest {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub prompt: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptResponse {
    #[serde(rename = "stopReason")]
    pub stop_reason: StopReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    MaxTurnRequests,
    Refusal,
    Cancelled,
    #[serde(other)]
    Unknown,
}

// ---- session/cancel ------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelNotification {
    #[serde(rename = "sessionId")]
    pub session_id: String,
}

// ---- session/update (agent → client notifications) -----------------------------------------

/// One streamed update within a prompt turn. Constructed by the reader from the raw `update` object.
#[derive(Debug, Clone)]
pub struct SessionNotification {
    pub session_id: String,
    pub update: SessionUpdate,
}

/// One provider-native slash command advertised for the live ACP session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableCommand {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub input: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "sessionUpdate", rename_all = "snake_case")]
pub enum SessionUpdate {
    UserMessageChunk {
        content: ContentBlock,
    },
    AgentMessageChunk {
        content: ContentBlock,
    },
    AgentThoughtChunk {
        content: ContentBlock,
    },
    ToolCall(ToolCall),
    ToolCallUpdate(ToolCallUpdate),
    /// Retired provider feature: accept the notification but discard its payload.
    Plan {},
    /// The agent's config options changed (model switched, effort adjusted, …). Carries the full
    /// replacement set, same as `session/new` and `session/set_config_option` responses.
    ConfigOptionUpdate {
        #[serde(rename = "configOptions", default)]
        config_options: Vec<SessionConfigOption>,
    },
    /// Full replacement set of provider-native slash commands for this session. Consumers must
    /// feature-detect commands from this update rather than infer them from provider identity.
    AvailableCommandsUpdate {
        #[serde(rename = "availableCommands", default)]
        available_commands: Vec<AvailableCommand>,
    },
    /// Provider-owned session metadata, including turn status outside `session/prompt`.
    SessionInfoUpdate {
        #[serde(default, rename = "_meta")]
        meta: Value,
    },
    /// Authoritative provider context usage/capacity for the active session. `cost` is an
    /// adapter-specific optional payload; retain it as an opaque value so newer providers remain
    /// parse-compatible without making cost part of C2's context-window contract.
    UsageUpdate {
        used: u64,
        size: u64,
        #[serde(default)]
        cost: Option<Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    #[serde(rename = "toolCallId")]
    pub tool_call_id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub content: Option<Value>,
    #[serde(default, rename = "rawInput")]
    pub raw_input: Option<Value>,
    #[serde(default, rename = "rawOutput")]
    pub raw_output: Option<Value>,
    #[serde(default, rename = "_meta")]
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallUpdate {
    #[serde(rename = "toolCallId")]
    pub tool_call_id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub content: Option<Value>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default, rename = "rawInput")]
    pub raw_input: Option<Value>,
    #[serde(default, rename = "rawOutput")]
    pub raw_output: Option<Value>,
    #[serde(default, rename = "_meta")]
    pub meta: Option<Value>,
}

// ---- session/request_permission (agent → client request) -----------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestPermissionRequest {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "toolCall", default)]
    pub tool_call: Value,
    pub options: Vec<PermissionOption>,
    #[serde(default, rename = "_meta")]
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionOption {
    #[serde(rename = "optionId")]
    pub option_id: String,
    pub name: String,
    /// One of: allow_once | allow_always | reject_once | reject_always.
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestPermissionResponse {
    pub outcome: PermissionOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum PermissionOutcome {
    Selected {
        #[serde(rename = "optionId")]
        option_id: String,
    },
    Cancelled,
}

// ---- elicitation/create (agent → client request; ACP: UNSTABLE) ----------------------------

/// The agent asks the client to collect structured input. `mode` is `form` (render
/// `requestedSchema`) or `url` (send the user to `url`); we only advertise — and only answer —
/// form mode, so every other field stays optional and unopinionated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateElicitationRequest {
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(rename = "sessionId", default)]
    pub session_id: Option<String>,
    #[serde(rename = "toolCallId", default)]
    pub tool_call_id: Option<String>,
    pub message: String,
    #[serde(rename = "requestedSchema", default)]
    pub requested_schema: Option<Value>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(rename = "elicitationId", default)]
    pub elicitation_id: Option<String>,
    #[serde(default, rename = "_meta")]
    pub meta: Option<Value>,
}

impl CreateElicitationRequest {
    /// Form mode is the default when the agent omits `mode`, matching the ACP schema.
    pub fn is_form(&self) -> bool {
        self.mode.as_deref().unwrap_or("form") == "form"
    }
}

/// `decline` means the user skipped (the agent carries on knowing nothing was chosen); `cancel`
/// aborts what the agent was asking about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum CreateElicitationResponse {
    Accept {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<serde_json::Map<String, Value>>,
    },
    Decline,
    Cancel,
}

// ---- fs/* (agent → client requests) --------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadTextFileRequest {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub path: String,
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadTextFileResponse {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteTextFileRequest {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub path: String,
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn caps_default_to_unsupported() {
        // An agent earns a capability by advertising it — absence, null, and non-bool all mean no.
        let none: InitializeResponse =
            serde_json::from_value(json!({"protocolVersion": 1})).unwrap();
        assert!(!none.caps().load_session);
        assert!(!none.caps().resume_session);
        assert!(!none.caps().mcp_http);
        assert!(!none.caps().mcp_sse);
        let odd: InitializeResponse = serde_json::from_value(
            json!({"protocolVersion": 1, "agentCapabilities": {"loadSession": "yes"}}),
        )
        .unwrap();
        assert!(!odd.caps().load_session);
        assert!(!odd.caps().resume_session);
    }

    #[test]
    fn caps_read_load_session() {
        let r: InitializeResponse =
            serde_json::from_value(json!({"protocolVersion": 1, "agentCapabilities": {
                "loadSession": true,
                "sessionCapabilities": {"resume": {}},
                "mcpCapabilities": {"http": true, "sse": true}
            }}))
            .unwrap();
        assert!(r.caps().load_session);
        assert!(r.caps().resume_session);
        assert!(r.caps().mcp_http);
        assert!(r.caps().mcp_sse);
    }

    #[test]
    fn usage_update_parses_authoritative_window_and_unknown_metadata() {
        let update: SessionUpdate = serde_json::from_value(json!({
            "sessionUpdate": "usage_update",
            "used": 53_000,
            "size": 200_000,
            "cost": {"input": 0.01},
            "_meta": {"provider": "codex"}
        }))
        .expect("usage_update is a supported ACP session update");

        match update {
            SessionUpdate::UsageUpdate { used, size, cost } => {
                assert_eq!(used, 53_000);
                assert_eq!(size, 200_000);
                assert_eq!(cost, Some(json!({"input": 0.01})));
            }
            other => panic!("unexpected update: {other:?}"),
        }
    }

    #[test]
    fn session_info_update_retains_provider_metadata() {
        let update: SessionUpdate = serde_json::from_value(json!({
            "sessionUpdate": "session_info_update",
            "_meta": {
                "goal": {"objective": "Ship it", "status": "active"},
                "codex": {"threadStatus": {"type": "active"}}
            }
        }))
        .expect("session_info_update is a supported ACP session update");

        match update {
            SessionUpdate::SessionInfoUpdate { meta } => {
                assert_eq!(meta["goal"]["objective"], "Ship it");
                assert_eq!(meta["codex"]["threadStatus"]["type"], "active");
            }
            other => panic!("unexpected update: {other:?}"),
        }
    }
}

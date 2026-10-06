//! The SQ/EQ interface between the core and any frontend (codex's Submission-Queue / Event-Queue
//! pattern). Frontends push [`Op`]s and consume a stream of [`Event`]s. The Electrobun desktop
//! forwards both through its Rust sidecar; the ratatui TUI calls the same core API in-process. One
//! agent loop, two renderers.
//!
//! The engine that turns `Op`s into `Event`s (by driving [`crate::acp`]) lands in M1; these are the
//! stable types both sides code against.

use crate::memory::MemoryReceipt;
use crate::permission::ExecutionPolicy;
use crate::provider::ProviderId;
use crate::session::{SessionActivity, SessionId};
use crate::skill::DocBlock;
use crate::task::TaskId;
use crate::worktree::ResolvedWorktreeBaseline;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Submissions: what a frontend asks the core to do.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    NewSession {
        provider: ProviderId,
        cwd: String,
        use_worktree: bool,
        /// Explicit local commit source for the isolated checkout. `None` keeps old clients
        /// compatible: when `use_worktree` is true it means the current checkout.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        worktree_base: Option<crate::worktree::WorktreeBaseline>,
        /// SHA shown by a baseline picker. The core resolves the selected local ref again and
        /// rejects creation if it moved. Older clients may omit this to keep click-time resolution.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        worktree_base_sha: Option<String>,
        /// Correlates a broadcast `session_created`/terminal error with the initiating client.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        /// Model selected before the session exists. The engine persists it with the session and
        /// applies it when ACP creates the provider-side session on the first prompt.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// Optional draft policy chosen before the session exists. The core persists and installs
        /// it before emitting `session_created`, so it mediates permission requests from the very
        /// first prompt.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        initial_policy: Option<ExecutionPolicy>,
    },
    /// Submit the composed document as one prompt turn. The core compiles it (see [`crate::skill`]).
    Prompt {
        session: SessionId,
        doc: Vec<DocBlock>,
        /// Correlates prompt acceptance or rejection with the initiating client.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    Cancel {
        session: SessionId,
    },
    /// Answer an outstanding permission request. `option_id = None` means "cancelled".
    AnswerPermission {
        session: SessionId,
        request_id: String,
        option_id: Option<String>,
    },
    /// Answer an outstanding structured question. `accept` carries form content keyed by field;
    /// `decline` means the user skipped it, `cancel` aborts what the agent was asking about.
    AnswerElicitation {
        session: SessionId,
        request_id: String,
        answer: crate::elicitation::ElicitationAnswer,
    },
    SetPermissionMode {
        session: SessionId,
        mode: crate::permission::PermissionMode,
    },
    /// Change the ACP tool-kind ceiling used by permission mediation. This does not install an
    /// operating-system or container sandbox.
    SetSandbox {
        session: SessionId,
        sandbox: crate::permission::SandboxPolicy,
    },
    /// Change both execution-policy axes as one logical operation.
    SetExecutionPolicy {
        session: SessionId,
        mode: crate::permission::PermissionMode,
        sandbox: crate::permission::SandboxPolicy,
        /// Correlates the authoritative success event or a rejection with the initiating client.
        /// Older clients may omit it and still receive the broadcast projection.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    SetModel {
        session: SessionId,
        model: String,
    },
    /// Set an agent-reported session config option (model, reasoning effort, …) by id.
    SetConfigOption {
        session: SessionId,
        config_id: String,
        value: String,
    },
}

/// Events: what the core streams back for the UI to render.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    AssistantAlert {
        session: Option<SessionId>,
        id: String,
        goal_id: String,
        title: String,
        body: String,
    },
    /// A member-visible collaboration snapshot changed. Clients refetch by Task id; the event does
    /// not duplicate comments or Suggestions into the execution stream.
    TaskSnapshotChanged {
        /// Kept null so existing session-event consumers can safely treat this as a global event.
        session: Option<SessionId>,
        task_id: TaskId,
        revision: u64,
    },
    SessionCreated {
        session: SessionId,
        /// Durable provider working directory, including the selected subdirectory inside a newly
        /// created worktree. Frontends can switch atomically without depending on a second list RPC.
        #[serde(default)]
        cwd: String,
        /// Source project provenance for the durable session. This receipt lets a frontend create
        /// another session safely even if its best-effort session-list refresh fails.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project_path: Option<String>,
        /// Isolated checkout root, when this session owns one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        worktree_path: Option<String>,
        /// Immutable local baseline used to create `worktree_path`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        worktree_baseline: Option<ResolvedWorktreeBaseline>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    /// Exact, inspectable record of the recalled items injected into this turn. Emitted only after
    /// the matching user part is persisted, and never represented as user-authored transcript.
    MemoryContext {
        session: SessionId,
        receipt: MemoryReceipt,
    },
    /// Durable automatic-title update; frontends can refresh a shell without reloading sessions.
    SessionTitleChanged { session: SessionId, title: String },
    /// Authoritative durable provider replacement for an existing conversation. Provider-owned
    /// model/config/capability projections are reset and republished separately after this event.
    ProviderChanged {
        session: SessionId,
        provider: ProviderId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    /// Authoritative, revisioned session lifecycle projection.
    SessionActivityChanged {
        session: SessionId,
        activity: SessionActivity,
    },
    /// The core accepted a prompt and now owns the session's single live turn slot.
    TurnStarted {
        session: SessionId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        /// Sequence of the canonical persisted prompt, absent for non-persistent runs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transcript_seq: Option<i64>,
    },
    AgentText {
        session: SessionId,
        message_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transcript_seq: Option<i64>,
    },
    AgentThought {
        session: SessionId,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transcript_seq: Option<i64>,
    },
    ToolCall {
        session: SessionId,
        id: String,
        title: String,
        status: String,
        /// Provider-neutral ACP category when the adapter supplied one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        /// Bounded launch metadata powers capability-detected subagent observability.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_input: Option<Value>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        outputs: Vec<crate::artifact::ToolOutput>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transcript_seq: Option<i64>,
    },
    /// A permission decision is needed from the user. `options` are `(option_id, label)` pairs.
    PermissionRequest {
        session: SessionId,
        request_id: String,
        title: String,
        options: Vec<(String, String)>,
        #[serde(default)]
        context: crate::permission::PermissionContext,
    },
    /// The agent asked the user a structured question (ACP `elicitation/create`) — Claude Code's
    /// `AskUserQuestion`, an MCP form elicitation. Answered with [`Op::AnswerElicitation`].
    ElicitationRequest {
        session: SessionId,
        request_id: String,
        form: crate::elicitation::ElicitationForm,
    },
    Usage {
        session: SessionId,
        input_tokens: u64,
        output_tokens: u64,
    },
    /// Authoritative provider-reported context usage/capacity for the active session. This is
    /// deliberately separate from [`Event::Usage`], which remains the legacy rolling quota shape.
    ContextWindow {
        session: SessionId,
        used_tokens: u64,
        context_window: u64,
        /// Provider-reported cumulative cost in USD, forwarded when the ACP usage update carries a
        /// numeric cost. Optional-field append: older payloads without it still deserialize.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost_usd: Option<f64>,
    },
    /// The models this session can run on: the agent's own list (reported at `session/new` and
    /// echoed after a switch), or the installed CLI catalogue for its provider — emitted as
    /// soon as the session exists — when the agent doesn't implement the (UNSTABLE) ACP model API.
    /// `current` is empty when nothing has been chosen yet.
    Models {
        session: SessionId,
        available: Vec<ModelChoice>,
        current: String,
    },
    /// The agent's session config options (model selector, thought level, …), reported at
    /// `session/new`, echoed after every `set_config_option`, and pushed on agent-side changes.
    /// This is the newer ACP surface that superseded `Models`; sessions may emit either or both.
    ConfigOptions {
        session: SessionId,
        options: Vec<ConfigOptionInfo>,
    },
    SessionCapabilities {
        session: SessionId,
        steering: bool,
        /// True only after this live ACP session advertises the native `/compact` command.
        #[serde(default)]
        compact_context: bool,
    },
    PromptQueued {
        session: SessionId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        position: usize,
    },
    SteerAccepted {
        session: SessionId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transcript_seq: Option<i64>,
        outcome: String,
    },
    /// Authoritative execution policy after the durable store and live permission handler have
    /// both committed the same pair. A rejected write emits a correlated [`Event::Error`] instead
    /// and never emits this success receipt.
    ExecutionPolicyChanged {
        session: SessionId,
        policy: ExecutionPolicy,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    TurnEnded {
        session: SessionId,
        stop_reason: String,
    },
    /// Terminal receipt of the explicit worktree discard flow: the session's isolated checkout
    /// and `codetwo/…` branch are permanently gone, while the session itself remains readable
    /// history that can no longer run prompts.
    WorktreeDiscarded {
        session: SessionId,
        worktree_path: String,
        /// `false` when the checkout was already gone and only stale registration/branch state
        /// needed cleanup.
        #[serde(default)]
        removed_checkout: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deleted_branch: Option<String>,
    },
    /// A terminal tool call the engine's conservative heuristic recognized as a test run
    /// ([`crate::testsignal`]). Feeds scene `tests_failed` hooks and the `tests_pass` exit
    /// criterion; consumers must tolerate it never firing.
    TestSignal {
        session: SessionId,
        tool_call_id: String,
        /// The classified command, bounded to 256 chars.
        command: String,
        passed: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
    },
    /// One captured version of a declared scene artifact (auto-capture at turn end, or a manual
    /// pin). `record_id` resolves content via the scene-artifact store.
    ArtifactProduced {
        session: SessionId,
        scene_ref: String,
        artifact_key: String,
        kind: String,
        version: i64,
        record_id: i64,
    },
    /// All machine-checkable exit criteria of the session's active scene hold. `state_key` is the
    /// debounce identity: the completion banner never re-fires for the same key.
    ExitCriteriaMet {
        session: SessionId,
        scene_ref: String,
        satisfied: Vec<String>,
        unverified: Vec<String>,
        state_key: String,
    },
    /// Render-only outcome of a scene hook's suggest/notify action. The frontend renders it; it
    /// never acts on the session by itself.
    HookSuggestion {
        session: SessionId,
        scene_ref: String,
        /// The hook event that fired ("tests_failed", "exit_criteria_met", …).
        on: String,
        /// The action kind ("suggest_scene" | "suggest_next" | "notify").
        kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_scene: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        carry: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pipeline_instance: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to_stage: Option<String>,
        state_key: String,
    },
    /// A hook-initiated `run_macro` turn was submitted — the transcript attribution for automatic
    /// prompts (at most one in flight per session).
    HookTurnStarted {
        session: SessionId,
        scene_ref: String,
        macro_id: String,
    },
    /// Per-session cost projection (R7 tracker). `priced: false` means no price table entry
    /// matched and only token counts are meaningful.
    SessionCost {
        session: SessionId,
        input_tokens: u64,
        output_tokens: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost_usd: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        burn_rate_usd_per_hour: Option<f64>,
        priced: bool,
    },
    Error {
        session: Option<SessionId>,
        message: String,
        /// Warnings use `false`; only terminal errors end a running turn or creation request.
        #[serde(default)]
        terminal: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
}

/// One selectable model, flattened from ACP's `ModelInfo` for the frontends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelChoice {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

/// One session config selector, flattened from ACP's `SessionConfigOption` for the frontends.
/// `category` is the spec's semantic hint ("model", "mode", "thought_level", …); `choices` reuses
/// [`ModelChoice`] since the shape (id, name, description) is identical.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigOptionInfo {
    pub id: String,
    pub name: String,
    pub category: Option<String>,
    pub current: String,
    pub choices: Vec<ModelChoice>,
}

#[cfg(test)]
mod tests {
    use super::{Event, Op};
    use crate::permission::{ExecutionPolicy, PermissionMode, SandboxPolicy};
    use crate::session::{PendingInput, PendingInputKind, SessionActivity, SessionRunState};

    #[test]
    fn turn_started_uses_the_public_wire_shape() {
        let value = serde_json::to_value(Event::TurnStarted {
            session: "session-1".into(),
            request_id: Some("prompt-1".into()),
            transcript_seq: Some(42),
        })
        .unwrap();

        assert_eq!(value["event"], "turn_started");
        assert_eq!(value["session"], "session-1");
        assert_eq!(value["request_id"], "prompt-1");
        assert_eq!(value["transcript_seq"], 42);
    }

    #[test]
    fn prompt_request_ids_remain_optional_on_the_wire() {
        let prompt: Op = serde_json::from_value(serde_json::json!({
            "op": "prompt",
            "session": "session-1",
            "doc": []
        }))
        .unwrap();
        assert!(matches!(
            prompt,
            Op::Prompt {
                request_id: None,
                ..
            }
        ));

        let event = serde_json::to_value(Event::TurnStarted {
            session: "session-1".into(),
            request_id: None,
            transcript_seq: None,
        })
        .unwrap();
        assert!(event.get("request_id").is_none());
        assert!(event.get("transcript_seq").is_none());
    }

    #[test]
    fn usage_update_event_serializes_context_window_fields() {
        let value = serde_json::to_value(Event::ContextWindow {
            session: "session-1".into(),
            used_tokens: 53_000,
            context_window: 200_000,
            cost_usd: None,
        })
        .unwrap();

        assert_eq!(value["event"], "context_window");
        assert_eq!(value["session"], "session-1");
        assert_eq!(value["used_tokens"], 53_000);
        assert_eq!(value["context_window"], 200_000);
        assert!(value.get("cost_usd").is_none());
    }

    #[test]
    fn execution_policy_change_has_a_correlated_authoritative_wire_shape() {
        let op = serde_json::to_value(Op::SetExecutionPolicy {
            session: "session-1".into(),
            mode: PermissionMode::Yolo,
            sandbox: SandboxPolicy::DangerFullAccess,
            request_id: Some("policy-1".into()),
        })
        .unwrap();
        assert_eq!(op["op"], "set_execution_policy");
        assert_eq!(op["request_id"], "policy-1");

        let event = serde_json::to_value(Event::ExecutionPolicyChanged {
            session: "session-1".into(),
            policy: ExecutionPolicy {
                mode: PermissionMode::Yolo,
                sandbox: SandboxPolicy::DangerFullAccess,
            },
            request_id: Some("policy-1".into()),
        })
        .unwrap();
        assert_eq!(event["event"], "execution_policy_changed");
        assert_eq!(event["policy"]["mode"], "yolo");
        assert_eq!(event["policy"]["sandbox"], "danger_full_access");
        assert_eq!(event["request_id"], "policy-1");
    }

    #[test]
    fn legacy_policy_payloads_remain_compatible_but_invalid_enums_do_not_fallback() {
        let legacy: Op = serde_json::from_value(serde_json::json!({
            "op": "set_execution_policy",
            "session": "session-1",
            "mode": "ask",
            "sandbox": "workspace_write"
        }))
        .unwrap();
        assert!(matches!(
            legacy,
            Op::SetExecutionPolicy {
                request_id: None,
                ..
            }
        ));

        let legacy_create: Op = serde_json::from_value(serde_json::json!({
            "op": "new_session",
            "provider": "grok",
            "cwd": "/work",
            "use_worktree": false
        }))
        .unwrap();
        assert!(matches!(
            legacy_create,
            Op::NewSession {
                model: None,
                initial_policy: None,
                ..
            }
        ));

        for (field, value) in [("mode", "future_mode"), ("sandbox", "future_sandbox")] {
            let mut payload = serde_json::json!({
                "op": "set_execution_policy",
                "session": "session-1",
                "mode": "ask",
                "sandbox": "workspace_write"
            });
            payload[field] = value.into();
            assert!(serde_json::from_value::<Op>(payload).is_err());
        }
    }

    #[test]
    fn session_activity_changed_uses_the_public_wire_shape() {
        let value = serde_json::to_value(Event::SessionActivityChanged {
            session: "session-1".into(),
            activity: SessionActivity {
                revision: 2,
                state: SessionRunState::AwaitingInput {
                    turn_id: "turn-1".into(),
                    prompt_request_id: Some("prompt-1".into()),
                    pending: vec![PendingInput {
                        input_id: "permission-1".into(),
                        kind: PendingInputKind::Permission,
                        title: "Run command".into(),
                        options: vec![("allow".into(), "Allow".into())],
                        option_kinds: Default::default(),
                        sequence: 1,
                        context: Default::default(),
                        form: None,
                    }],
                },
            },
        })
        .unwrap();

        assert_eq!(value["event"], "session_activity_changed");
        assert_eq!(value["session"], "session-1");
        assert_eq!(value["activity"]["revision"], 2);
        assert_eq!(value["activity"]["state"]["kind"], "awaiting_input");
        assert_eq!(
            value["activity"]["state"]["pending"][0]["input_id"],
            "permission-1"
        );
    }

    #[test]
    fn context_window_without_cost_still_deserializes() {
        let event: Event = serde_json::from_value(serde_json::json!({
            "event": "context_window",
            "session": "session-1",
            "used_tokens": 1_000,
            "context_window": 200_000
        }))
        .unwrap();
        assert!(matches!(event, Event::ContextWindow { cost_usd: None, .. }));

        let value = serde_json::to_value(Event::ContextWindow {
            session: "session-1".into(),
            used_tokens: 1_000,
            context_window: 200_000,
            cost_usd: Some(0.42),
        })
        .unwrap();
        assert_eq!(value["cost_usd"], 0.42);
    }

    #[test]
    fn test_signal_uses_the_public_wire_shape() {
        let value = serde_json::to_value(Event::TestSignal {
            session: "session-1".into(),
            tool_call_id: "tool-1".into(),
            command: "cargo test".into(),
            passed: false,
            exit_code: Some(101),
        })
        .unwrap();
        assert_eq!(value["event"], "test_signal");
        assert_eq!(value["session"], "session-1");
        assert_eq!(value["tool_call_id"], "tool-1");
        assert_eq!(value["command"], "cargo test");
        assert_eq!(value["passed"], false);
        assert_eq!(value["exit_code"], 101);

        let value = serde_json::to_value(Event::TestSignal {
            session: "session-1".into(),
            tool_call_id: "tool-1".into(),
            command: "cargo test".into(),
            passed: true,
            exit_code: None,
        })
        .unwrap();
        assert!(value.get("exit_code").is_none());
    }

    #[test]
    fn artifact_produced_uses_the_public_wire_shape() {
        let value = serde_json::to_value(Event::ArtifactProduced {
            session: "session-1".into(),
            scene_ref: "builtin:test".into(),
            artifact_key: "test-report".into(),
            kind: "test_report".into(),
            version: 2,
            record_id: 7,
        })
        .unwrap();
        assert_eq!(value["event"], "artifact_produced");
        assert_eq!(value["session"], "session-1");
        assert_eq!(value["scene_ref"], "builtin:test");
        assert_eq!(value["artifact_key"], "test-report");
        assert_eq!(value["kind"], "test_report");
        assert_eq!(value["version"], 2);
        assert_eq!(value["record_id"], 7);
    }

    #[test]
    fn exit_criteria_met_uses_the_public_wire_shape() {
        let value = serde_json::to_value(Event::ExitCriteriaMet {
            session: "session-1".into(),
            scene_ref: "builtin:test".into(),
            satisfied: vec!["required_artifacts".into()],
            unverified: vec!["manual review".into()],
            state_key: "required_artifacts,user_confirm".into(),
        })
        .unwrap();
        assert_eq!(value["event"], "exit_criteria_met");
        assert_eq!(value["session"], "session-1");
        assert_eq!(value["scene_ref"], "builtin:test");
        assert_eq!(value["satisfied"][0], "required_artifacts");
        assert_eq!(value["unverified"][0], "manual review");
        assert_eq!(value["state_key"], "required_artifacts,user_confirm");
    }

    #[test]
    fn hook_suggestion_uses_the_public_wire_shape() {
        let value = serde_json::to_value(Event::HookSuggestion {
            session: "session-1".into(),
            scene_ref: "builtin:test".into(),
            on: "tests_failed".into(),
            kind: "suggest_scene".into(),
            target_scene: Some("fix".into()),
            carry: vec!["test-report".into()],
            message: None,
            pipeline_instance: None,
            to_stage: None,
            state_key: "tool-1".into(),
        })
        .unwrap();
        assert_eq!(value["event"], "hook_suggestion");
        assert_eq!(value["session"], "session-1");
        assert_eq!(value["scene_ref"], "builtin:test");
        assert_eq!(value["on"], "tests_failed");
        assert_eq!(value["kind"], "suggest_scene");
        assert_eq!(value["target_scene"], "fix");
        assert_eq!(value["carry"][0], "test-report");
        assert_eq!(value["state_key"], "tool-1");
        assert!(value.get("message").is_none());
        assert!(value.get("pipeline_instance").is_none());
        assert!(value.get("to_stage").is_none());

        let value = serde_json::to_value(Event::HookSuggestion {
            session: "session-1".into(),
            scene_ref: "builtin:test".into(),
            on: "turn_end".into(),
            kind: "notify".into(),
            target_scene: None,
            carry: Vec::new(),
            message: Some("done".into()),
            pipeline_instance: None,
            to_stage: None,
            state_key: "1".into(),
        })
        .unwrap();
        assert_eq!(value["message"], "done");
        assert!(value.get("target_scene").is_none());
        assert!(value.get("carry").is_none());
    }

    #[test]
    fn hook_turn_started_uses_the_public_wire_shape() {
        let value = serde_json::to_value(Event::HookTurnStarted {
            session: "session-1".into(),
            scene_ref: "builtin:test".into(),
            macro_id: "commit-macro".into(),
        })
        .unwrap();
        assert_eq!(value["event"], "hook_turn_started");
        assert_eq!(value["session"], "session-1");
        assert_eq!(value["scene_ref"], "builtin:test");
        assert_eq!(value["macro_id"], "commit-macro");
    }

    #[test]
    fn session_cost_uses_the_public_wire_shape() {
        let value = serde_json::to_value(Event::SessionCost {
            session: "session-1".into(),
            input_tokens: 1_000,
            output_tokens: 250,
            cost_usd: Some(0.12),
            burn_rate_usd_per_hour: None,
            priced: true,
        })
        .unwrap();
        assert_eq!(value["event"], "session_cost");
        assert_eq!(value["session"], "session-1");
        assert_eq!(value["input_tokens"], 1_000);
        assert_eq!(value["output_tokens"], 250);
        assert_eq!(value["cost_usd"], 0.12);
        assert_eq!(value["priced"], true);
        assert!(value.get("burn_rate_usd_per_hour").is_none());
    }
}

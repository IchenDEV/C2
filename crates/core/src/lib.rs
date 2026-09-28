//! C2 core — the unified product engine.
//!
//! Contains the reactive plugin kernel, product domain, execution capabilities, built-in adapters,
//! extension bundle management, and plugin protocol. Every host (desktop, server) depends on this
//! single crate.
//!
//! Module map:
//! - [`kernel`] — Cordis-style reactive plugin runtime: contexts, services, injections, commands.
//! - [`acp`] — Agent Client Protocol client (JSON-RPC over stdio) used to drive provider CLIs.
//! - [`provider`] — registry of provider launch specs (Claude Code / Codex / Grok).
//! - [`models`] — model catalogues discovered from installed provider runtimes.
//! - [`session`] — session / message / part model.
//! - [`skill`] — skill library + the document → prompt compiler (the product differentiator).
//! - [`permission`] — ask/allow/deny engine and permission modes (incl. YOLO).
//! - [`event`] — the Op/Event types exchanged with frontends.
//! - [`error`] — shared error types.

#[cfg(unix)]
mod unix_process_group;

pub mod kernel;
pub mod plugins;
pub mod acp;
pub mod activity;
pub mod agent_skill_v2;
pub mod artifact;
pub mod attachment;
pub mod automation;
pub mod brief;
pub mod browser;
pub mod canvas;
pub mod capability_v2;
pub mod codex_runtime;
pub mod context;
pub mod cost;
pub mod device_sync;
pub mod elicitation;
pub mod engine;
pub mod error;
pub mod event;
pub mod git;
pub mod github_skills;
pub mod handoff;
pub mod harness;
pub mod host_tools;
pub mod issue_delivery;
pub mod issues;
pub mod keymap;
pub mod market;
pub mod memory;
pub mod models;
pub mod orchestrator;
pub mod permission;
pub mod project;
pub mod provider;
pub mod provider_lifecycle;
pub mod pty;
pub mod risk_v2;
pub mod rules;
pub mod scene;
pub mod scene_artifact;
pub mod scene_runtime;
pub mod scene_v2;
pub mod session;
pub mod session_import;
pub mod skill;
pub mod source_control;
pub mod store;
pub mod task;
pub mod task_capsule;
pub mod task_store;
#[cfg(feature = "terminal")]
pub mod term;
pub mod testsignal;
pub mod tmux;
pub mod usage;
pub mod voice;
pub mod workspace;
pub mod workspace_search;
pub mod worktree;

pub use activity::{ActivityTracker, TurnLease};
pub use agent_skill_v2::{AgentSkillContribution, AgentSkillResolver, ResolvedAgentSkill};
pub use artifact::{ArtifactRef, ArtifactStore, ToolOutput, ToolOutputNormalizer};
pub use automation::{
    next_automation_run_after, Automation, AutomationInput, AutomationRun, AutomationRunStatus,
};
pub use browser::Annotation;
pub use canvas::{
    canvas_search_projection, deterministic_summary, encode_canvas_history_marker, normalize_media,
    parse_canvas_history_marker, resolve_prompt_payload, CanvasAssetRef, CanvasDraft,
    CanvasDraftUpdate, CanvasError, CanvasExport, CanvasExportBudget, CanvasExportKind,
    CanvasFeatureGate, CanvasFreezeInput, CanvasHistoryMarker, CanvasId, CanvasManifest,
    CanvasObject, CanvasObjectKind, CanvasPixelPolicy, CanvasPoint, CanvasPromptPayload,
    CanvasProviderImageCapability, CanvasRect, CanvasRef, CanvasRevision, CanvasSceneEnvelope,
    CanvasSnapshot, CanvasStaticAsset, CanvasTheme, CanvasTool, CANVAS_FEATURE_GATE,
    CANVAS_SCHEMA_VERSION, EXCALIDRAW_ENGINE, EXCALIDRAW_ENGINE_VERSION,
};
pub use capability_v2::{
    propose_plugin_installations, resolve_work_item_capabilities, CapabilityAdapter,
    CapabilityAdapterState, CapabilityReadiness, CapabilityReadinessStatus,
    CapabilityResolutionError, ConcreteEffect, PluginInstallCandidate, PluginInstallProposal,
};
pub use context::{estimate_tokens, ContextUsage};
pub use engine::{
    lower_canvas_prompt_payload, DesktopMcpConfig, Engine, ParallelTaskCreation, PermissionRouter,
    ProviderProtocolCompatibility, RedactedEngineDiagnostics, RedactedSessionDiagnostics,
    SessionHandler,
};
pub use error::{AcpError, RpcError};
pub use event::{Event, Op};
pub use git::{Checkpoint, GitFile, GitStatus};
pub use handoff::{PortableTaskHandoff, TaskHandoffManager, TaskHandoffResult};
pub use host_tools::{
    BrowserUseBackendOption, BrowserUseSettings, ComputerUseBackendOption, ComputerUseSettings,
    HostToolDiscovery,
};
pub use issues::Issue;
pub use keymap::{Action as KeyAction, Keymap};
pub use market::MarketEntry;
pub use memory::{
    MemoryCanvasRef, MemoryContext, MemoryReceipt, MemoryReceiptItem, MemoryRecord, MemorySettings,
    MemorySourceRef, MemoryStats, MemoryTurnAudit, MemoryTurnProvenance,
};
pub use orchestrator::{
    apply_orchestration_patch, ExecutionPreparation, ExecutionRequest, ExecutionStep,
    ExecutorAssignment, ExecutorOutcome, ExecutorPort, GraphOperation, InMemoryExecutor,
    InMemoryPlanner, OrchestrationPatch, OrchestrationValidationError, Orchestrator,
    OrchestratorError, PlannerInput, PlannerPort,
};
pub use permission::{
    Action, ExecutionPolicy, PermissionContext, PermissionContextKind, PermissionMode,
    PermissionPolicy, Rule, SandboxPolicy,
};
pub use project::{ProjectConfig, ProjectScript};
pub use provider::{
    default_registry, registry_with_codex_runtime, CapabilityState, LaunchSpec, Provider,
    ProviderCapability, ProviderCapabilityId, ProviderId, ProviderToolset,
};
pub use pty::PtySession;
pub use risk_v2::{effect_requires_risk_gate, RiskGateDecision, RiskGateReceipt, UserRiskDecision};
pub use scene::{
    apply_execution, is_artifact_id, is_slug, memory_preset_policy, outgoing_edges, plan_apply,
    policy_session_mode, prompt_preamble, session_mode_policy, validate_pipeline, validate_scene,
    ApplyStrength, BriefClarify, CarriedArtifact, CarrySpec, EffectiveTransition,
    EscalationRequired, ExitCriterion, ExitCriterionKind, Gate, HookAction, HookActionKind,
    HookEvent, NextSuggestion, PendingField, Pipeline, PipelineStage, PipelineTransition,
    ResolvedPipeline, ResolvedScene, Scene, SceneApplyPlan, SceneArtifactKind, SceneArtifactSpec,
    SceneBrief, SceneConstraints, SceneExecution, SceneExit, SceneHook, SceneInlineFragment,
    SceneLibrary, SceneLocalization, SceneMemoryPreset, SceneSessionMode, SceneSessionParams,
    SceneSkills, SceneSource, SceneWorktree, ToolHints, TransitionTrigger, PIPELINE_SCHEMA_ID,
    SCENE_SCHEMA_ID,
};
pub use scene_artifact::{
    extract_artifact_blocks, SceneArtifactRecord, SceneArtifactStore, MAX_CARRY_CONTENT_BYTES,
};
pub use scene_runtime::{evaluate_exit, ExitEvaluation, SceneRuntime};
pub use scene_v2::{
    parse_scene_v2, validate_scene_v2, ResolvedSceneV2, SceneCatalogDiagnostic, SceneCatalogV2,
    SceneDefinitionV2, SceneV2Author, SceneV2Error, SceneV2Localization, SceneV2Origin,
    SCENE_V2_SCHEMA_ID,
};
pub use session::{
    HandoffState, MemoryAccess, Message, Part, PendingInput, PendingInputKind, Role,
    RunFailureReason, Session, SessionActivity, SessionId, SessionRunState, SessionTitleOrigin,
    TranscriptCursor, TranscriptEntry, TranscriptPage, DEFAULT_TRANSCRIPT_TURNS,
    MAX_TRANSCRIPT_TURNS,
};
pub use skill::{
    canonical_doc_text, compile, compile_full, compile_with_canvas, CompiledCanvas, CompiledPrompt,
    DocBlock, McpServer, McpTransport, Skill, SkillKind, SkillLibrary, SkillPayload, SlotDef,
    SlotKind, SubagentDefinition,
};
pub use source_control::{SourceControlInfo, SourceControlProviderKind};
pub use store::{
    IssueDelegation, PipelineInstance, PipelineTransitionRecord, SessionSearchHit, Store,
    StoreError,
};
pub use task::{
    AgentAssignment, AgentId, AgentRole, AgentSkillOrigin, AgentSkillRef, AgentStatus,
    ArtifactProvenance, AttentionItem, IdempotentCommand, LoopCeilings, LoopGuardState,
    MaterialGoalChangeReceipt, Member, MemberId, OrchestrationEvent, OrchestrationEventKind,
    ProviderCacheMetrics, ProviderConfiguration, ResultContract, ResultContractRefinement,
    RunSnapshot, SceneOrigin, SceneRef, SharedTaskSnapshot, StructuralPromptReuse,
    SuggestionApprovalReceipt, SuggestionId, SuggestionStatus, Task, TaskActivityEvent,
    TaskActivityKind, TaskArtifactStatus, TaskBudget, TaskBudgetState, TaskCacheReceipt,
    TaskCollaborationSnapshot, TaskComment, TaskCommentId, TaskCompletionEvaluation, TaskGraph,
    TaskId, TaskSessionLease, TaskStatus, TaskSuggestion, TaskUsageObservation, WorkItem,
    WorkItemAttempt, WorkItemAttemptStatus, WorkItemEdge, WorkItemId, WorkItemStatus, Workspace,
    WorkspaceId, WorkspaceRole,
};
pub use task_capsule::{
    compile_task_capsule, session_compatibility_key, CapabilityManifestEntry, CompiledTaskCapsule,
    SessionCompatibilityKey, StablePromptLayer, StablePromptLayerKind, TaskCapsuleContext,
    TaskCapsuleError,
};
pub use task_store::TaskRecord;
#[cfg(feature = "terminal")]
pub use term::{Scope, TerminalConfig, TerminalHandle, TerminalOutput};
pub use testsignal::{classify_test_command, test_outcome, TestOutcome};
pub use workspace_search::{WorkspaceContentMatch, WorkspaceSearchOptions, WorkspaceSearchResult};

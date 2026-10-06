//! The agent loop, as a plugin.
//!
//! This is the one that proves the model. The engine is the biggest thing in the codebase and the
//! most connected — it needs storage, providers, the skill library, the scene library, and an
//! event sink. Under the old shape that meant a construction site in `setup()` that had to run in
//! exactly the right order and could never be undone.
//!
//! Here it declares five injections and gets torn down and rebuilt whenever any of them changes.
//! The event pump, the skill subscription, and the session commands all belong to its scope, so
//! "reload the engine" is a real operation rather than a restart.

use crate::engine::{Engine, ParallelTaskCreation};
use crate::event::Op;
use crate::kernel::{async_trait, Context, Injection, Plugin, PluginError, PluginResult};
use crate::permission::{ExecutionPolicy, PermissionMode, SandboxPolicy};
use crate::plugins::app::events::{EngineEvent, ScenesChanged, SkillsChanged};
use crate::plugins::app::service::{
    EngineService, EventBus, Paths, ProviderService, SceneService, SkillService, StoreService,
};
use crate::plugins::app::{json, take_args};
use crate::provider::ProviderId;
use crate::session::TranscriptCursor;
use crate::task::TaskId;
use crate::worktree::WorktreeBaseline;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// What the engine is built from. A host that needs a different construction gets these and
/// returns an engine without forking the plugin graph.
pub struct EngineInputs {
    pub providers: Vec<crate::provider::Provider>,
    pub provider_tools:
        Arc<std::sync::RwLock<std::collections::HashMap<String, crate::provider::ProviderToolset>>>,
    pub skills: crate::skill::SkillLibrary,
    pub store: Arc<crate::store::Store>,
    pub memory: Option<crate::memory::MemoryCapability>,
}

/// Replaces how the engine is constructed, without replacing what it is wired to.
pub type EngineBuilder = Arc<
    dyn Fn(
            EngineInputs,
        ) -> (
            Engine,
            tokio::sync::mpsc::UnboundedReceiver<crate::event::Event>,
        ) + Send
        + Sync,
>;

#[derive(Default)]
pub struct EnginePlugin {
    builder: Option<EngineBuilder>,
    extra_required: Vec<String>,
}

impl EnginePlugin {
    pub fn new() -> EnginePlugin {
        EnginePlugin::default()
    }

    /// Swap the construction step. Everything else — the injections, the event pump, the skill
    /// subscription, the session commands — stays exactly as it is, which is the point: a host
    /// customizes one seam instead of forking the boot sequence.
    pub fn with_builder(builder: EngineBuilder) -> EnginePlugin {
        EnginePlugin {
            builder: Some(builder),
            extra_required: Vec::new(),
        }
    }

    /// A host-specific engine can require a host service as part of the same reactive graph. The
    /// desktop uses this for its browser MCP: disabling the browser takes the engine down instead
    /// of leaving a tool configured against a dead socket.
    pub fn with_builder_and_required<I, S>(builder: EngineBuilder, required: I) -> EnginePlugin
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        EnginePlugin {
            builder: Some(builder),
            extra_required: required.into_iter().map(Into::into).collect(),
        }
    }
}

#[async_trait]
impl Plugin for EnginePlugin {
    fn name(&self) -> &str {
        "engine"
    }

    fn description(&self) -> Option<&str> {
        Some("The ACP agent loop: sessions, prompts, permissions.")
    }

    fn inject(&self) -> Injection {
        let mut injection = Injection::required(["store", "providers", "skills", "bus", "paths"])
            .with_optional(["scenes", "memory"]);
        injection
            .required
            .extend(self.extra_required.iter().cloned());
        injection
    }

    async fn apply(&self, ctx: Context, _config: Value) -> PluginResult {
        let store = ctx.expect::<StoreService>()?;
        let providers = ctx.expect::<ProviderService>()?;
        let skills = ctx.expect::<SkillService>()?;
        let bus = ctx.expect::<EventBus>()?;
        let paths = ctx.expect::<Paths>()?;

        let inputs = EngineInputs {
            providers: providers.runtime_providers(),
            provider_tools: providers.shared_toolsets(),
            skills: skills.library(),
            store: store.0.clone(),
            memory: ctx
                .get::<crate::plugins::app::service::MemoryService>()
                .map(|memory| memory.0.clone()),
        };
        let (engine, mut rx) = match &self.builder {
            Some(build) => build(inputs),
            None => Engine::with_store_memory_and_shared_provider_tools(
                inputs.providers,
                inputs.skills,
                inputs.store,
                inputs.memory,
                inputs.provider_tools,
            ),
        };
        engine.set_private_data_dir(paths.data_dir.clone());
        let worktree_settings =
            crate::worktree::load_settings(&paths.data_dir).unwrap_or_else(|error| {
                tracing::warn!("could not load worktree settings: {error}");
                crate::worktree::WorktreeSettings::default()
            });
        engine.set_worktree_root(worktree_settings.root.map(std::path::PathBuf::from));
        let engine = Arc::new(engine);

        // Scenes are optional: without them the engine compiles prompts against the default
        // (empty) library, exactly as it does in the TUI today.
        if let Some(scenes) = ctx.get::<SceneService>() {
            engine.set_scenes(scenes.library());
            if let Some(artifacts) = scenes.artifacts.clone() {
                engine.set_scene_artifacts(artifacts);
            }
        }

        // One pump, two audiences: the broadcast bus for subscribers that want a receiver, and the
        // kernel bus for plugins that want a scope-owned listener.
        let publisher = bus.clone();
        let weak = ctx.weak();
        ctx.spawn(async move {
            while let Some(event) = rx.recv().await {
                publisher.publish(event.clone());
                match weak.upgrade() {
                    Some(ctx) => ctx.emit(EngineEvent(event)).await,
                    None => break,
                }
            }
        });

        // The engine holds a compiled copy of both libraries; this is how they stay current
        // without the skills or scenes plugins knowing an engine exists.
        let refreshed = engine.clone();
        let source = skills.clone();
        ctx.on::<SkillsChanged, _>(move |_| {
            refreshed.set_skills(source.library());
            None
        });
        if let Some(scenes) = ctx.get::<SceneService>() {
            let refreshed = engine.clone();
            ctx.on::<ScenesChanged, _>(move |_| {
                refreshed.set_scenes(scenes.library());
                None
            });
        }

        ctx.provide(Arc::new(EngineService(engine.clone())))?;
        register_commands(&ctx, engine.clone(), store, providers, bus, paths)?;
        ctx.effect(move || engine.shutdown());
        Ok(())
    }
}

fn register_commands(
    ctx: &Context,
    engine: Arc<Engine>,
    store: Arc<StoreService>,
    providers: Arc<ProviderService>,
    bus: Arc<EventBus>,
    paths: Arc<Paths>,
) -> Result<(), PluginError> {
    #[derive(Serialize)]
    struct SessionSummary {
        id: String,
        title: String,
        last_active_at: i64,
        activity_revision: u64,
        activity_state: &'static str,
    }
    #[derive(Deserialize)]
    struct SubmitArgs {
        op: Op,
    }
    let submitting = engine.clone();
    ctx.command_described(
        "engine.submit",
        Some("Push one Op into the agent loop."),
        move |args| {
            let engine = submitting.clone();
            async move {
                let args: SubmitArgs = take_args(args)?;
                engine.submit(args.op).await.map_err(PluginError::new)?;
                Ok(Value::Bool(true))
            }
        },
    )?;

    let listing = engine.clone();
    ctx.command("sessions.list", move |_| {
        let engine = listing.clone();
        async move { json(engine.list_sessions().map_err(PluginError::new)?) }
    })?;

    let summarizing = engine.clone();
    ctx.command_extension_public("sessions.summary", move |_| {
        let engine = summarizing.clone();
        async move {
            let sessions = engine
                .list_sessions()
                .map_err(PluginError::new)?
                .into_iter()
                .filter(|session| !session.transient)
                .take(100)
                .map(|session| SessionSummary {
                    id: session.id,
                    title: session.title,
                    last_active_at: session.last_active_at,
                    activity_revision: session.activity.revision,
                    activity_state: match session.activity.state {
                        crate::session::SessionRunState::Idle => "idle",
                        crate::session::SessionRunState::Running { .. } => "running",
                        crate::session::SessionRunState::AwaitingInput { .. } => "awaiting_input",
                        crate::session::SessionRunState::Failed { .. } => "failed",
                    },
                })
                .collect::<Vec<_>>();
            json(sessions)
        }
    })?;

    let diagnostics_engine = engine.clone();
    let diagnostics_providers = providers.clone();
    ctx.command_described(
        "diagnostics.redacted_snapshot",
        Some("Create a content-free provider and process support snapshot."),
        move |_| {
            let engine = diagnostics_engine.clone();
            let providers = diagnostics_providers.clone();
            async move {
                let sessions = engine.redacted_diagnostics(100).map_err(PluginError::new)?;
                let providers = providers.redacted_diagnostics().await;
                Ok(serde_json::json!({
                    "schema_version": 1,
                    "generated_at": chrono::Utc::now().to_rfc3339(),
                    "redaction": {
                        "level": "default",
                        "excluded": [
                            "account_data",
                            "environment_values",
                            "paths",
                            "prompts",
                            "raw_errors",
                            "session_identifiers",
                            "token_usage",
                            "tool_input",
                            "tool_output"
                        ]
                    },
                    "core": {
                        "version": env!("CARGO_PKG_VERSION"),
                        "os": std::env::consts::OS,
                        "arch": std::env::consts::ARCH
                    },
                    "providers": providers,
                    "engine": sessions
                }))
            }
        },
    )?;

    let archived = engine.clone();
    ctx.command("sessions.archived", move |_| {
        let engine = archived.clone();
        async move { json(engine.list_archived().map_err(PluginError::new)?) }
    })?;

    #[derive(Deserialize)]
    struct SearchArgs {
        query: String,
        #[serde(default)]
        limit: Option<usize>,
    }
    let searching = engine.clone();
    ctx.command("sessions.search", move |args| {
        let engine = searching.clone();
        async move {
            let args: SearchArgs = take_args(args)?;
            json(
                engine
                    .search_sessions(&args.query, args.limit.unwrap_or(50).min(200))
                    .map_err(PluginError::new)?,
            )
        }
    })?;

    #[derive(Deserialize)]
    struct TranscriptArgs {
        session: String,
        #[serde(default)]
        before: Option<TranscriptCursor>,
        #[serde(default)]
        limit: Option<usize>,
    }
    let transcript = engine.clone();
    ctx.command("sessions.transcript", move |args| {
        let engine = transcript.clone();
        async move {
            let args: TranscriptArgs = take_args(args)?;
            json(
                engine
                    .transcript_page(
                        &args.session,
                        args.before,
                        args.limit
                            .unwrap_or(crate::session::DEFAULT_TRANSCRIPT_TURNS),
                    )
                    .map_err(PluginError::new)?,
            )
        }
    })?;

    #[derive(Deserialize)]
    struct RenameArgs {
        session: String,
        title: String,
    }
    let renaming = engine.clone();
    ctx.command("sessions.rename", move |args| {
        let engine = renaming.clone();
        async move {
            let args: RenameArgs = take_args(args)?;
            engine.rename_session(&args.session, &args.title);
            Ok(Value::Bool(true))
        }
    })?;

    #[derive(Deserialize)]
    struct FlagArgs {
        session: String,
        value: bool,
    }
    let archiving = engine.clone();
    ctx.command("sessions.set_archived", move |args| {
        let engine = archiving.clone();
        async move {
            let args: FlagArgs = take_args(args)?;
            engine.set_archived(&args.session, args.value);
            Ok(Value::Bool(true))
        }
    })?;

    let pinning = engine.clone();
    ctx.command("sessions.set_pinned", move |args| {
        let engine = pinning.clone();
        async move {
            let args: FlagArgs = take_args(args)?;
            engine.set_pinned(&args.session, args.value);
            Ok(Value::Bool(true))
        }
    })?;

    let previews = store.clone();
    ctx.command("sessions.previews", move |_| {
        let store = previews.clone();
        async move { json(store.last_texts().map_err(PluginError::new)?) }
    })?;

    #[derive(Deserialize)]
    struct ImportSessionsArgs {
        paths: Vec<String>,
        fallback_cwd: String,
    }
    let importing = store.clone();
    let import_bus = bus.clone();
    ctx.command("sessions.import", move |args| {
        let store = importing.clone();
        let bus = import_bus.clone();
        async move {
            let args: ImportSessionsArgs = take_args(args)?;
            if args.paths.is_empty() {
                return Err(PluginError::new("select at least one session file"));
            }
            if args.paths.len() > 100 {
                return Err(PluginError::new(
                    "select no more than 100 session files at once",
                ));
            }
            let report = crate::session_import::import_session_files(
                &store,
                &args.paths,
                &args.fallback_cwd,
            );
            for session in report.sessions.iter().filter(|session| session.imported) {
                bus.publish(crate::event::Event::SessionCreated {
                    session: session.id.clone(),
                    cwd: session.cwd.clone(),
                    project_path: session.project_path.clone(),
                    worktree_path: None,
                    worktree_baseline: None,
                    request_id: None,
                });
            }
            json(report)
        }
    })?;

    #[derive(Deserialize)]
    struct SessionArgs {
        session: String,
    }
    let diff_store = store.clone();
    ctx.command("sessions.diff_stat", move |args| {
        let store = diff_store.clone();
        async move {
            let args: SessionArgs = take_args(args)?;
            let Some(cwd) = store
                .get_session(&args.session)
                .map_err(PluginError::new)?
                .map(|session| session.cwd)
            else {
                return Ok(Value::Null);
            };
            json(
                crate::git::diff_stat(std::path::Path::new(&cwd))
                    .await
                    .map_err(PluginError::new)?,
            )
        }
    })?;

    #[derive(Deserialize)]
    struct NewSessionArgs {
        provider: String,
        cwd: String,
        #[serde(default)]
        use_worktree: bool,
        #[serde(default)]
        worktree_base: Option<WorktreeBaseline>,
        #[serde(default)]
        worktree_base_sha: Option<String>,
        #[serde(default)]
        request_id: Option<String>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        initial_policy: Option<ExecutionPolicy>,
        #[serde(default)]
        transient: bool,
        #[serde(default)]
        reasoning_effort: Option<String>,
    }
    let new_session = engine.clone();
    let worktree_settings_dir = paths.data_dir.clone();
    ctx.command("engine.new_session", move |args| {
        let engine = new_session.clone();
        let settings_dir = worktree_settings_dir.clone();
        async move {
            let mut args: NewSessionArgs = take_args(args)?;
            let worktree_settings =
                crate::worktree::load_settings(&settings_dir).map_err(PluginError::new)?;
            engine.set_worktree_root(
                worktree_settings
                    .root
                    .as_deref()
                    .map(std::path::PathBuf::from),
            );
            if args.use_worktree && worktree_settings.fetch_upstream {
                let source = std::path::Path::new(&args.cwd);
                crate::worktree::fetch_upstream(source)
                    .await
                    .map_err(PluginError::new)?;
                let baseline = args.worktree_base.unwrap_or(WorktreeBaseline::Current);
                args.worktree_base_sha = Some(
                    crate::worktree::resolve_baseline(source, baseline)
                        .await
                        .map_err(PluginError::new)?
                        .sha,
                );
            }
            engine
                .create_session(
                    parse_provider(&args.provider),
                    args.cwd,
                    args.use_worktree,
                    args.worktree_base,
                    args.worktree_base_sha,
                    args.request_id,
                    args.model,
                    args.initial_policy,
                    args.transient,
                    args.reasoning_effort,
                )
                .await
                .map_err(PluginError::new)?;
            if args.use_worktree && worktree_settings.auto_delete {
                let (_, errors) = engine
                    .cleanup_old_worktrees(worktree_settings.auto_delete_limit)
                    .await;
                for error in errors {
                    tracing::warn!("automatic worktree cleanup failed: {error}");
                }
            }
            Ok(Value::Bool(true))
        }
    })?;

    #[derive(Deserialize)]
    struct NewParallelTaskArgs {
        provider: String,
        cwd: String,
        #[serde(default)]
        worktree_base: Option<WorktreeBaseline>,
        #[serde(default)]
        worktree_base_sha: Option<String>,
        request_id: String,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        initial_policy: Option<ExecutionPolicy>,
        #[serde(default)]
        reasoning_effort: Option<String>,
        task_id: String,
        goal: String,
    }
    let new_parallel_task = engine.clone();
    let parallel_worktree_settings_dir = paths.data_dir.clone();
    ctx.command("engine.new_parallel_task", move |args| {
        let engine = new_parallel_task.clone();
        let settings_dir = parallel_worktree_settings_dir.clone();
        async move {
            let mut args: NewParallelTaskArgs = take_args(args)?;
            let worktree_settings =
                crate::worktree::load_settings(&settings_dir).map_err(PluginError::new)?;
            engine.set_worktree_root(
                worktree_settings
                    .root
                    .as_deref()
                    .map(std::path::PathBuf::from),
            );
            let baseline = args.worktree_base.unwrap_or(WorktreeBaseline::Current);
            if worktree_settings.fetch_upstream {
                let source = std::path::Path::new(&args.cwd);
                crate::worktree::fetch_upstream(source)
                    .await
                    .map_err(PluginError::new)?;
                args.worktree_base_sha = Some(
                    crate::worktree::resolve_baseline(source, baseline)
                        .await
                        .map_err(PluginError::new)?
                        .sha,
                );
            }
            engine
                .create_parallel_task_session(ParallelTaskCreation {
                    provider: parse_provider(&args.provider),
                    cwd: args.cwd,
                    worktree_base: baseline,
                    worktree_base_sha: args.worktree_base_sha,
                    request_id: args.request_id,
                    model: args.model,
                    initial_policy: args.initial_policy,
                    reasoning_effort: args.reasoning_effort,
                    task_id: TaskId::new(args.task_id),
                    goal: args.goal,
                })
                .await
                .map_err(PluginError::new)?;
            if worktree_settings.auto_delete {
                let (_, errors) = engine
                    .cleanup_old_worktrees(worktree_settings.auto_delete_limit)
                    .await;
                for error in errors {
                    tracing::warn!("automatic worktree cleanup failed: {error}");
                }
            }
            Ok(Value::Bool(true))
        }
    })?;

    #[derive(Deserialize)]
    struct PromptArgs {
        session: String,
        doc: Vec<crate::skill::DocBlock>,
        #[serde(default)]
        request_id: Option<String>,
    }
    let prompt = engine.clone();
    ctx.command("engine.prompt", move |args| {
        let engine = prompt.clone();
        async move {
            let args: PromptArgs = take_args(args)?;
            engine
                .submit(Op::Prompt {
                    session: args.session,
                    doc: args.doc,
                    request_id: args.request_id,
                })
                .await
                .map_err(PluginError::new)?;
            Ok(Value::Bool(true))
        }
    })?;

    let closing = engine.clone();
    ctx.command("engine.close_transient_session", move |args| {
        let engine = closing.clone();
        async move {
            let args: SessionArgs = take_args(args)?;
            let closed = engine
                .close_transient_session(&args.session)
                .map_err(PluginError::new)?;
            if closed {
                engine
                    .cancel_pending_prompts(&args.session)
                    .map_err(PluginError::new)?;
            }
            json(closed)
        }
    })?;

    let preparing = engine.clone();
    ctx.command("engine.prepare_session", move |args| {
        let engine = preparing.clone();
        async move {
            let args: SessionArgs = take_args(args)?;
            engine
                .prepare_session(&args.session)
                .await
                .map_err(PluginError::new)?;
            Ok(Value::Bool(true))
        }
    })?;

    let queueing = engine.clone();
    ctx.command("engine.queue", move |args| {
        let engine = queueing.clone();
        async move {
            let args: PromptArgs = take_args(args)?;
            let delivery = engine
                .deliver_prompt(&args.session, args.doc, "queue", args.request_id, None)
                .await
                .map_err(PluginError::new)?;
            let position = engine
                .delivery_position(&delivery.id)
                .map_err(PluginError::new)?;
            json(serde_json::json!({ "position": position, "delivery": delivery }))
        }
    })?;
    let steering = engine.clone();
    ctx.command("engine.steer", move |args| {
        let engine = steering.clone();
        async move {
            let args: PromptArgs = take_args(args)?;
            let delivery = engine
                .deliver_prompt(&args.session, args.doc, "steer", args.request_id, None)
                .await
                .map_err(PluginError::new)?;
            if delivery.state != "accepted" {
                return Err(PluginError::new(delivery.outcome));
            }
            json(serde_json::json!({ "outcome": delivery.outcome, "delivery": delivery }))
        }
    })?;
    let draining = engine.clone();
    let mut delivery_events = bus.subscribe();
    ctx.spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        loop {
            tokio::select! {
                _ = interval.tick() => {},
                event = delivery_events.recv() => {
                    if matches!(event, Err(tokio::sync::broadcast::error::RecvError::Closed)) { break; }
                }
            }
            if let Err(error) = draining.drain_prompt_deliveries().await { tracing::warn!(%error, "prompt delivery reconciliation failed"); }
        }
    });

    #[derive(Deserialize)]
    struct PermissionArgs {
        session: String,
        request_id: String,
        #[serde(default)]
        option_id: Option<String>,
    }
    let permission = engine.clone();
    ctx.command("engine.answer_permission", move |args| {
        let engine = permission.clone();
        async move {
            let args: PermissionArgs = take_args(args)?;
            Ok(Value::Bool(engine.answer_permission(
                &args.session,
                &args.request_id,
                args.option_id.as_deref(),
            )))
        }
    })?;

    #[derive(Deserialize)]
    struct ElicitationArgs {
        session: String,
        request_id: String,
        answer: crate::elicitation::ElicitationAnswer,
    }
    let elicitation = engine.clone();
    ctx.command("engine.answer_elicitation", move |args| {
        let engine = elicitation.clone();
        async move {
            let args: ElicitationArgs = take_args(args)?;
            Ok(Value::Bool(engine.answer_elicitation(
                &args.session,
                &args.request_id,
                args.answer,
            )))
        }
    })?;

    #[derive(Deserialize)]
    struct ModeArgs {
        session: String,
        mode: PermissionMode,
    }
    let mode = engine.clone();
    ctx.command("engine.set_permission_mode", move |args| {
        let engine = mode.clone();
        async move {
            let args: ModeArgs = take_args(args)?;
            engine
                .submit(Op::SetPermissionMode {
                    session: args.session,
                    mode: args.mode,
                })
                .await
                .map_err(PluginError::new)?;
            Ok(Value::Bool(true))
        }
    })?;

    #[derive(Deserialize)]
    struct SandboxArgs {
        session: String,
        sandbox: SandboxPolicy,
    }
    let sandbox = engine.clone();
    ctx.command("engine.set_sandbox", move |args| {
        let engine = sandbox.clone();
        async move {
            let args: SandboxArgs = take_args(args)?;
            engine
                .submit(Op::SetSandbox {
                    session: args.session,
                    sandbox: args.sandbox,
                })
                .await
                .map_err(PluginError::new)?;
            Ok(Value::Bool(true))
        }
    })?;

    #[derive(Deserialize)]
    struct ExecutionArgs {
        session: String,
        mode: PermissionMode,
        sandbox: SandboxPolicy,
        #[serde(default)]
        request_id: Option<String>,
    }
    let execution = engine.clone();
    ctx.command("engine.set_execution_policy", move |args| {
        let engine = execution.clone();
        async move {
            let args: ExecutionArgs = take_args(args)?;
            engine
                .submit(Op::SetExecutionPolicy {
                    session: args.session,
                    mode: args.mode,
                    sandbox: args.sandbox,
                    request_id: args.request_id,
                })
                .await
                .map_err(PluginError::new)?;
            Ok(Value::Bool(true))
        }
    })?;

    #[derive(Deserialize)]
    struct ProviderSwitchArgs {
        session: String,
        provider: String,
        #[serde(default)]
        model: Option<String>,
    }
    let provider_switch = engine.clone();
    ctx.command("engine.switch_provider", move |args| {
        let engine = provider_switch.clone();
        async move {
            let args: ProviderSwitchArgs = take_args(args)?;
            let session = engine
                .switch_provider(&args.session, parse_provider(&args.provider), args.model)
                .await
                .map_err(PluginError::new)?;
            json(session)
        }
    })?;

    #[derive(Deserialize)]
    struct ModelArgs {
        session: String,
        model: String,
    }
    let model = engine.clone();
    ctx.command("engine.set_model", move |args| {
        let engine = model.clone();
        async move {
            let args: ModelArgs = take_args(args)?;
            engine
                .submit(Op::SetModel {
                    session: args.session,
                    model: args.model,
                })
                .await
                .map_err(PluginError::new)?;
            Ok(Value::Bool(true))
        }
    })?;

    #[derive(Deserialize)]
    struct ConfigArgs {
        session: String,
        config_id: String,
        value: String,
    }
    let config = engine.clone();
    ctx.command("engine.set_config_option", move |args| {
        let engine = config.clone();
        async move {
            let args: ConfigArgs = take_args(args)?;
            engine
                .submit(Op::SetConfigOption {
                    session: args.session,
                    config_id: args.config_id,
                    value: args.value,
                })
                .await
                .map_err(PluginError::new)?;
            Ok(Value::Bool(true))
        }
    })?;

    let cancelling = engine.clone();
    ctx.command("engine.cancel", move |args| {
        let engine = cancelling.clone();
        async move {
            let args: SessionArgs = take_args(args)?;
            engine
                .cancel_pending_prompts(&args.session)
                .map_err(PluginError::new)?;
            engine
                .submit(Op::Cancel {
                    session: args.session,
                })
                .await
                .map_err(PluginError::new)?;
            Ok(Value::Bool(true))
        }
    })?;

    let discard = engine.clone();
    ctx.command("worktrees.discard_session", move |args| {
        let engine = discard.clone();
        async move {
            let args: SessionArgs = take_args(args)?;
            json(
                engine
                    .discard_session_worktree(&args.session)
                    .await
                    .map_err(PluginError::new)?,
            )
        }
    })?;

    let settings_dir = paths.data_dir.clone();
    ctx.command("worktrees.settings", move |_| {
        let settings_dir = settings_dir.clone();
        async move {
            json(crate::worktree::load_settings(&settings_dir).map_err(PluginError::new)?)
        }
    })?;

    #[derive(Deserialize)]
    struct WorktreeSettingsArgs {
        settings: crate::worktree::WorktreeSettings,
    }
    let settings_dir = paths.data_dir.clone();
    let settings_engine = engine.clone();
    ctx.command("worktrees.set_settings", move |args| {
        let settings_dir = settings_dir.clone();
        let engine = settings_engine.clone();
        async move {
            let args: WorktreeSettingsArgs = take_args(args)?;
            let settings = crate::worktree::save_settings(&settings_dir, args.settings)
                .map_err(PluginError::new)?;
            engine.set_worktree_root(settings.root.as_deref().map(std::path::PathBuf::from));
            if settings.auto_delete {
                let (_, errors) = engine
                    .cleanup_old_worktrees(settings.auto_delete_limit)
                    .await;
                for error in errors {
                    tracing::warn!("automatic worktree cleanup failed: {error}");
                }
            }
            json(settings)
        }
    })?;

    #[derive(Deserialize)]
    struct ProjectArgs {
        project_path: String,
    }
    let worktrees = engine.clone();
    ctx.command("worktrees.list", move |args| {
        let engine = worktrees.clone();
        async move {
            let args: ProjectArgs = take_args(args)?;
            json(
                engine
                    .list_project_worktrees(&args.project_path)
                    .await
                    .map_err(PluginError::new)?,
            )
        }
    })?;

    #[derive(Deserialize)]
    struct OrphanArgs {
        project_path: String,
        worktree_path: String,
    }
    ctx.command("worktrees.discard_orphan", move |args| {
        let engine = engine.clone();
        async move {
            let args: OrphanArgs = take_args(args)?;
            json(
                engine
                    .discard_orphan_worktree(&args.project_path, &args.worktree_path)
                    .await
                    .map_err(PluginError::new)?,
            )
        }
    })?;
    Ok(())
}

fn parse_provider(value: &str) -> ProviderId {
    match value {
        "claude_code" => ProviderId::ClaudeCode,
        "codex" => ProviderId::Codex,
        "grok" => ProviderId::Grok,
        "cursor" => ProviderId::Cursor,
        "opencode" => ProviderId::OpenCode,
        "opencode2" => ProviderId::OpenCode2,
        "pi" => ProviderId::Pi,
        "kimi" => ProviderId::Kimi,
        "zcode" => ProviderId::ZCode,
        "amp" => ProviderId::Amp,
        "droid" => ProviderId::Droid,
        other => ProviderId::Custom(other.to_string()),
    }
}

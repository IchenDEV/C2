//! Runtime scene application, artifact capture, scheduling, and pipeline instances.

use crate::plugins::app::service::{
    EngineService, ProviderService, SceneRuntimeService, SceneService, StoreService,
};
use crate::plugins::app::{json, take_args};
use crate::event::Op;
use crate::permission::ExecutionPolicy;
use crate::scene::{
    self, ApplyStrength, SceneArtifactKind, SceneArtifactSpec, SceneLibrary,
};
use crate::scene_artifact::{SceneArtifactRecord, SceneArtifactStore};
use crate::store::{PipelineInstance, PipelineTransitionRecord};
use crate::kernel::{async_trait, Context, Injection, Plugin, PluginError, PluginResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone)]
struct SceneInputs {
    engine: Arc<EngineService>,
    store: Arc<StoreService>,
    scenes: Arc<SceneService>,
    runtime: Arc<SceneRuntimeService>,
    providers: Arc<ProviderService>,
    artifacts: Option<SceneArtifactStore>,
}

#[derive(Deserialize)]
struct AdvanceArgs {
    instance_id: String,
    to_stage: String,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    confirm: bool,
}

pub struct SceneCommandsPlugin;

#[async_trait]
impl Plugin for SceneCommandsPlugin {
    fn name(&self) -> &str {
        "scene-commands"
    }

    fn description(&self) -> Option<&str> {
        Some("Apply scenes to sessions and manage artifacts, hooks, and pipeline instances.")
    }

    fn inject(&self) -> Injection {
        Injection::required(["engine", "store", "scenes", "scene-runtime", "providers"])
    }

    async fn apply(&self, ctx: Context, _config: Value) -> PluginResult {
        let scenes = ctx.expect::<SceneService>()?;
        let inputs = SceneInputs {
            engine: ctx.expect::<EngineService>()?,
            store: ctx.expect::<StoreService>()?,
            runtime: ctx.expect::<SceneRuntimeService>()?,
            providers: ctx.expect::<ProviderService>()?,
            artifacts: scenes.artifacts.clone(),
            scenes,
        };
        register_scene_commands(&ctx, inputs.clone())?;
        register_pipeline_commands(&ctx, inputs)?;
        Ok(())
    }
}

fn register_scene_commands(ctx: &Context, inputs: SceneInputs) -> PluginResult {
    #[derive(Deserialize)]
    struct ApplyArgs {
        session: String,
        reference: String,
        #[serde(default)]
        confirm_escalation: bool,
    }
    let applying = inputs.clone();
    ctx.command("scenes.apply", move |args| {
        let inputs = applying.clone();
        async move {
            let args: ApplyArgs = take_args(args)?;
            json(
                apply_scene_to(
                    &inputs,
                    &args.session,
                    &args.reference,
                    args.confirm_escalation,
                )
                .await?,
            )
        }
    })?;

    #[derive(Deserialize)]
    struct PlanArgs {
        reference: String,
        #[serde(default)]
        confirm_escalation: bool,
    }
    let planning = inputs.clone();
    ctx.command("scenes.session_plan", move |args| {
        let inputs = planning.clone();
        async move {
            let args: PlanArgs = take_args(args)?;
            json(scene_session_plan_for(&inputs, &args.reference, args.confirm_escalation).await?)
        }
    })?;

    #[derive(Deserialize)]
    struct SetSceneArgs {
        session: String,
        #[serde(default)]
        reference: Option<String>,
        #[serde(default)]
        customized: bool,
    }
    let setting = inputs.clone();
    ctx.command("scenes.set_session", move |args| {
        let inputs = setting.clone();
        async move {
            let args: SetSceneArgs = take_args(args)?;
            inputs
                .store
                .set_session_scene(&args.session, args.reference.as_deref(), args.customized)
                .map_err(PluginError::new)?;
            inputs
                .runtime
                .scene_activated(&args.session, args.reference.as_deref());
            Ok(Value::Bool(true))
        }
    })?;

    #[derive(Deserialize)]
    struct SessionArgs {
        session: String,
    }
    let session_scene = inputs.clone();
    ctx.command("scenes.session", move |args| {
        let inputs = session_scene.clone();
        async move {
            let args: SessionArgs = take_args(args)?;
            let stored = inputs
                .store
                .session_scene(&args.session)
                .map_err(PluginError::new)?;
            let library = inputs.scenes.library();
            json(stored.map(|(reference, customized)| SessionSceneState {
                resolved: library.resolve(&reference).is_some(),
                reference,
                customized,
            }))
        }
    })?;

    #[derive(Deserialize)]
    struct AutoArgs {
        session: String,
        enabled: bool,
    }
    let auto = inputs.clone();
    ctx.command("scenes.set_auto", move |args| {
        let inputs = auto.clone();
        async move {
            let args: AutoArgs = take_args(args)?;
            inputs
                .store
                .set_session_auto_scene(&args.session, args.enabled)
                .map_err(PluginError::new)?;
            Ok(Value::Bool(true))
        }
    })?;

    let get_auto = inputs.clone();
    ctx.command("scenes.auto", move |args| {
        let inputs = get_auto.clone();
        async move {
            let args: SessionArgs = take_args(args)?;
            json(
                inputs
                    .store
                    .session_auto_scene(&args.session)
                    .map_err(PluginError::new)?,
            )
        }
    })?;

    let artifacts = inputs.clone();
    ctx.command("scene_artifacts.list", move |args| {
        let inputs = artifacts.clone();
        async move {
            let args: SessionArgs = take_args(args)?;
            json(
                artifact_store(&inputs)?
                    .list_for_session(&args.session)
                    .map_err(PluginError::new)?,
            )
        }
    })?;

    #[derive(Deserialize)]
    struct RecordIdArgs {
        record_id: i64,
    }
    let content = inputs.clone();
    ctx.command("scene_artifacts.content", move |args| {
        let inputs = content.clone();
        async move {
            let args: RecordIdArgs = take_args(args)?;
            json(
                artifact_store(&inputs)?
                    .content(args.record_id)
                    .map_err(PluginError::new)?,
            )
        }
    })?;

    #[derive(Deserialize)]
    struct RecordArgs {
        session: String,
        artifact_key: String,
        content: String,
    }
    let recording = inputs.clone();
    ctx.command("scene_artifacts.record", move |args| {
        let inputs = recording.clone();
        async move {
            let args: RecordArgs = take_args(args)?;
            let scene_ref = inputs
                .store
                .session_scene(&args.session)
                .map_err(PluginError::new)?
                .map(|(reference, _)| reference)
                .unwrap_or_default();
            let spec = {
                let library = inputs.scenes.library();
                library
                    .resolve(&scene_ref)
                    .and_then(|entry| {
                        entry
                            .scene
                            .artifacts
                            .iter()
                            .find(|artifact| artifact.id == args.artifact_key)
                            .cloned()
                    })
                    .unwrap_or_else(|| SceneArtifactSpec {
                        id: args.artifact_key.clone(),
                        title: args.artifact_key.clone(),
                        kind: SceneArtifactKind::Custom,
                        required: false,
                        template: None,
                        description: None,
                    })
            };
            json(
                artifact_store(&inputs)?
                    .record(&scene_ref, &spec, &args.session, None, &args.content)
                    .map_err(PluginError::new)?,
            )
        }
    })?;

    #[derive(Deserialize)]
    struct PinArgs {
        session: String,
        artifact_key: String,
        #[serde(default)]
        version: Option<i64>,
    }
    let pinning = inputs.clone();
    ctx.command("scene_artifacts.pin", move |args| {
        let inputs = pinning.clone();
        async move {
            let args: PinArgs = take_args(args)?;
            artifact_store(&inputs)?
                .pin(&args.session, &args.artifact_key, args.version)
                .map_err(PluginError::new)?;
            Ok(Value::Bool(true))
        }
    })?;

    #[derive(Deserialize)]
    struct BannerArgs {
        session: String,
        state_key: String,
    }
    let banner = inputs.clone();
    ctx.command("scenes.dismiss_banner", move |args| {
        let inputs = banner.clone();
        async move {
            let args: BannerArgs = take_args(args)?;
            inputs
                .runtime
                .dismiss_banner(&args.session, &args.state_key);
            Ok(Value::Bool(true))
        }
    })?;

    #[derive(Deserialize)]
    struct ScheduleArgs {
        path: String,
        enabled: bool,
    }
    let scheduling = inputs.clone();
    ctx.command("scenes.set_scheduling", move |args| {
        let inputs = scheduling.clone();
        async move {
            let args: ScheduleArgs = take_args(args)?;
            inputs.runtime.set_scheduling(&args.path, args.enabled);
            Ok(Value::Bool(true))
        }
    })?;

    #[derive(Deserialize)]
    struct PathArgs {
        path: String,
    }
    ctx.command("scenes.scheduling", move |args| {
        let inputs = inputs.clone();
        async move {
            let args: PathArgs = take_args(args)?;
            json(
                inputs
                    .store
                    .project_scheduling(&args.path)
                    .map_err(PluginError::new)?,
            )
        }
    })?;
    Ok(())
}

async fn apply_scene_to(
    inputs: &SceneInputs,
    session: &str,
    reference: &str,
    confirm_escalation: bool,
) -> Result<SceneApplyOutcome, PluginError> {
    let (scene_ref, scene, plan) = {
        let library = inputs.scenes.library();
        let entry = library
            .resolve(reference)
            .ok_or_else(|| PluginError::new(format!("unknown scene `{reference}`")))?;
        let current = inputs
            .store
            .get_session(session)
            .map_err(PluginError::new)?
            .map(|session| ExecutionPolicy {
                mode: session.permission_mode,
                sandbox: session.sandbox_policy,
            })
            .unwrap_or_default();
        let scene_ref = SceneLibrary::reference_for(entry);
        let plan = scene::plan_apply(
            &current,
            &entry.scene,
            &scene_ref,
            ApplyStrength::Soft,
            confirm_escalation,
        );
        (scene_ref, entry.scene.clone(), plan)
    };

    if let Some(escalation) = plan.escalation {
        return Ok(SceneApplyOutcome {
            applied: Vec::new(),
            pending: Vec::new(),
            escalation: Some(EscalationOut::from_core(escalation)),
            suppress_unpinned: false,
            pinned_skills: Vec::new(),
        });
    }

    let mut applied = Vec::new();
    if let Some(policy) = plan.execution {
        inputs
            .engine
            .submit(Op::SetExecutionPolicy {
                session: session.to_string(),
                mode: policy.mode,
                sandbox: policy.sandbox,
                request_id: None,
            })
            .await
            .map_err(PluginError::new)?;
        applied.push("session_mode");
    }
    if let Some((read, write)) = plan.memory {
        inputs
            .store
            .set_session_memory_policy(session, read, write)
            .map_err(PluginError::new)?;
        applied.push("memory_preset");
    }
    inputs
        .store
        .set_session_scene(session, Some(&scene_ref), false)
        .map_err(PluginError::new)?;
    inputs.runtime.scene_activated(session, Some(&scene_ref));

    let skills = scene.skills.unwrap_or_default();
    Ok(SceneApplyOutcome {
        applied,
        pending: plan.pending.into_iter().map(pending_field_str).collect(),
        escalation: None,
        suppress_unpinned: skills.suppress_unpinned,
        pinned_skills: skills.pinned,
    })
}

async fn scene_session_plan_for(
    inputs: &SceneInputs,
    reference: &str,
    confirm_escalation: bool,
) -> Result<SceneSessionPlanOutcome, PluginError> {
    let library = inputs.scenes.library();
    let entry = library
        .resolve(reference)
        .ok_or_else(|| PluginError::new(format!("unknown scene `{reference}`")))?;
    let scene_ref = SceneLibrary::reference_for(entry);
    let plan = scene::plan_apply(
        &ExecutionPolicy::default(),
        &entry.scene,
        &scene_ref,
        ApplyStrength::Full,
        confirm_escalation,
    );
    if let Some(escalation) = plan.escalation {
        return Ok(SceneSessionPlanOutcome {
            params: None,
            escalation: Some(EscalationOut::from_core(escalation)),
        });
    }
    let mut params = plan.new_session;
    let providers = inputs.providers.summaries().await;
    if let Some(params) = params.as_mut() {
        if let Some(wanted) = &params.provider {
            let available = providers
                .iter()
                .any(|provider| provider.available && provider.id == *wanted);
            if !available {
                params.provider = entry
                    .scene
                    .execution
                    .as_ref()
                    .map(|execution| execution.providers.clone())
                    .unwrap_or_default()
                    .into_iter()
                    .find(|candidate| {
                        providers
                            .iter()
                            .any(|provider| provider.available && provider.id == *candidate)
                    });
            }
        }
    }
    Ok(SceneSessionPlanOutcome {
        params,
        escalation: None,
    })
}

fn artifact_store(inputs: &SceneInputs) -> Result<&SceneArtifactStore, PluginError> {
    inputs
        .artifacts
        .as_ref()
        .ok_or_else(|| PluginError::new("scene artifact storage is unavailable"))
}

#[derive(Serialize)]
struct EscalationOut {
    from: String,
    to: String,
}

impl EscalationOut {
    fn from_core(escalation: scene::EscalationRequired) -> Self {
        Self {
            from: escalation.from.as_str().to_string(),
            to: escalation.to.as_str().to_string(),
        }
    }
}

#[derive(Serialize)]
struct SceneApplyOutcome {
    applied: Vec<&'static str>,
    pending: Vec<&'static str>,
    escalation: Option<EscalationOut>,
    suppress_unpinned: bool,
    pinned_skills: Vec<String>,
}

#[derive(Serialize)]
struct SceneSessionPlanOutcome {
    params: Option<crate::scene::SceneSessionParams>,
    escalation: Option<EscalationOut>,
}

#[derive(Serialize)]
struct SessionSceneState {
    reference: String,
    customized: bool,
    resolved: bool,
}

fn pending_field_str(field: scene::PendingField) -> &'static str {
    match field {
        scene::PendingField::Providers => "providers",
        scene::PendingField::Model => "model",
        scene::PendingField::ReasoningEffort => "reasoning_effort",
        scene::PendingField::Worktree => "worktree",
    }
}

fn register_pipeline_commands(ctx: &Context, inputs: SceneInputs) -> PluginResult {
    #[derive(Deserialize)]
    struct StartArgs {
        reference: String,
        project_path: String,
        #[serde(default)]
        session: Option<String>,
    }
    let starting = inputs.clone();
    ctx.command("pipelines.start", move |args| {
        let inputs = starting.clone();
        async move {
            let args: StartArgs = take_args(args)?;
            let (pipeline_ref, entry_stage, entry_scene, entry_gate) = {
                let library = inputs.scenes.library();
                let entry = library.resolve_pipeline(&args.reference).ok_or_else(|| {
                    PluginError::new(format!("unknown pipeline `{}`", args.reference))
                })?;
                let pipeline = &entry.pipeline;
                let stage_id = pipeline
                    .entry
                    .clone()
                    .unwrap_or_else(|| pipeline.stages[0].id.clone());
                let stage = pipeline
                    .stages
                    .iter()
                    .find(|stage| stage.id == stage_id)
                    .ok_or_else(|| {
                        PluginError::new(format!("pipeline entry `{stage_id}` names no stage"))
                    })?;
                (
                    SceneLibrary::pipeline_reference_for(entry),
                    stage_id,
                    stage.scene.clone(),
                    stage.gate.unwrap_or(scene::Gate::Suggest),
                )
            };
            let instance = inputs
                .store
                .create_pipeline_instance(&pipeline_ref, &args.project_path, &entry_stage)
                .map_err(PluginError::new)?;
            inputs
                .store
                .record_pipeline_transition(
                    &instance.id,
                    None,
                    &entry_stage,
                    "entry",
                    gate_str(entry_gate),
                    args.session.as_deref(),
                )
                .map_err(PluginError::new)?;
            let mut applied_scene = None;
            if let Some(session) = args.session.as_deref() {
                inputs
                    .store
                    .bind_session_to_stage(session, Some((&instance.id, &entry_stage)))
                    .map_err(PluginError::new)?;
                applied_scene = Some(apply_scene_to(&inputs, session, &entry_scene, false).await?);
            }
            let instance = inputs
                .store
                .get_pipeline_instance(&instance.id)
                .map_err(PluginError::new)?
                .ok_or_else(|| PluginError::new("pipeline instance vanished"))?;
            json(PipelineStartOutcome {
                detail: pipeline_detail(&inputs, instance)?,
                applied_scene,
            })
        }
    })?;

    let advancing = inputs.clone();
    ctx.command("pipelines.advance", move |args| {
        let inputs = advancing.clone();
        async move {
            let args: AdvanceArgs = take_args(args)?;
            json(advance_pipeline(&inputs, args).await?)
        }
    })?;

    #[derive(Deserialize)]
    struct BindArgs {
        instance_id: String,
        stage_id: String,
        session: String,
    }
    let binding = inputs.clone();
    ctx.command("pipelines.bind_session", move |args| {
        let inputs = binding.clone();
        async move {
            let args: BindArgs = take_args(args)?;
            inputs
                .store
                .bind_session_to_stage(&args.session, Some((&args.instance_id, &args.stage_id)))
                .map_err(PluginError::new)?;
            let scene_ref = inputs
                .store
                .get_pipeline_instance(&args.instance_id)
                .ok()
                .flatten()
                .and_then(|instance| {
                    let library = inputs.scenes.library();
                    let resolved = library.resolve_pipeline(&instance.pipeline_ref)?;
                    resolved
                        .pipeline
                        .stages
                        .iter()
                        .find(|stage| stage.id == args.stage_id)
                        .map(|stage| stage.scene.clone())
                });
            if let Some(scene_ref) = scene_ref {
                inputs
                    .store
                    .set_session_scene(&args.session, Some(&scene_ref), false)
                    .map_err(PluginError::new)?;
                inputs
                    .runtime
                    .scene_activated(&args.session, Some(&scene_ref));
            }
            Ok(Value::Bool(true))
        }
    })?;

    #[derive(Deserialize)]
    struct InstanceArgs {
        instance_id: String,
    }
    let detail = inputs.clone();
    ctx.command("pipelines.instance", move |args| {
        let inputs = detail.clone();
        async move {
            let args: InstanceArgs = take_args(args)?;
            let instance = inputs
                .store
                .get_pipeline_instance(&args.instance_id)
                .map_err(PluginError::new)?
                .ok_or_else(|| {
                    PluginError::new(format!("unknown pipeline instance `{}`", args.instance_id))
                })?;
            json(pipeline_detail(&inputs, instance)?)
        }
    })?;

    #[derive(Deserialize)]
    struct ProjectArgs {
        project_path: String,
    }
    let listing = inputs.clone();
    ctx.command("pipelines.instances", move |args| {
        let inputs = listing.clone();
        async move {
            let args: ProjectArgs = take_args(args)?;
            json(
                inputs
                    .store
                    .list_pipeline_instances(&args.project_path)
                    .map_err(PluginError::new)?,
            )
        }
    })?;

    #[derive(Deserialize)]
    struct SessionArgs {
        session: String,
    }
    ctx.command("pipelines.session", move |args| {
        let inputs = inputs.clone();
        async move {
            let args: SessionArgs = take_args(args)?;
            json(
                inputs
                    .store
                    .session_pipeline(&args.session)
                    .map_err(PluginError::new)?
                    .map(|(instance_id, stage_id)| SessionPipelineOut {
                        instance_id,
                        stage_id,
                    }),
            )
        }
    })?;
    Ok(())
}

async fn advance_pipeline(
    inputs: &SceneInputs,
    args: AdvanceArgs,
) -> Result<PipelineAdvanceOutcome, PluginError> {
    let instance = inputs
        .store
        .get_pipeline_instance(&args.instance_id)
        .map_err(PluginError::new)?
        .ok_or_else(|| {
            PluginError::new(format!("unknown pipeline instance `{}`", args.instance_id))
        })?;
    let (stage, trigger, gate) = {
        let library = inputs.scenes.library();
        let resolved = library
            .resolve_pipeline(&instance.pipeline_ref)
            .ok_or_else(|| {
                PluginError::new(format!("unknown pipeline `{}`", instance.pipeline_ref))
            })?;
        let stage = resolved
            .pipeline
            .stages
            .iter()
            .find(|stage| stage.id == args.to_stage)
            .ok_or_else(|| PluginError::new(format!("unknown stage `{}`", args.to_stage)))?
            .clone();
        let edge = scene::outgoing_edges(&resolved.pipeline, &instance.current_stage)
            .into_iter()
            .find(|edge| edge.to == args.to_stage);
        match edge {
            Some(edge) => (stage, trigger_str(edge.when), gate_str(edge.gate)),
            None => (
                stage,
                "user_request",
                if args.confirm { "confirm" } else { "suggest" },
            ),
        }
    };

    let mut applied_scene = None;
    let mut session_plan = None;
    match args.session.as_deref() {
        Some(session) => {
            let outcome = apply_scene_to(inputs, session, &stage.scene, args.confirm).await?;
            if outcome.escalation.is_some() {
                return Ok(PipelineAdvanceOutcome {
                    instance,
                    applied_scene: Some(outcome),
                    session_plan: None,
                    escalation: None,
                    carried: Vec::new(),
                });
            }
            applied_scene = Some(outcome);
        }
        None => {
            let plan = scene_session_plan_for(inputs, &stage.scene, args.confirm).await?;
            if let Some(escalation) = plan.escalation {
                return Ok(PipelineAdvanceOutcome {
                    instance,
                    applied_scene: None,
                    session_plan: None,
                    escalation: Some(escalation),
                    carried: Vec::new(),
                });
            }
            session_plan = plan.params;
        }
    }

    inputs
        .store
        .record_pipeline_transition(
            &args.instance_id,
            Some(&instance.current_stage),
            &args.to_stage,
            trigger,
            gate,
            args.session.as_deref(),
        )
        .map_err(PluginError::new)?;
    if let Some(session) = args.session.as_deref() {
        inputs
            .store
            .bind_session_to_stage(session, Some((&args.instance_id, &args.to_stage)))
            .map_err(PluginError::new)?;
    }
    let carried = artifact_store(inputs)?
        .resolve_carry(&args.instance_id, &stage)
        .into_iter()
        .map(|artifact| artifact.label)
        .collect();
    let instance = inputs
        .store
        .get_pipeline_instance(&args.instance_id)
        .map_err(PluginError::new)?
        .ok_or_else(|| PluginError::new("pipeline instance vanished"))?;
    Ok(PipelineAdvanceOutcome {
        instance,
        applied_scene,
        session_plan,
        escalation: None,
        carried,
    })
}

fn pipeline_detail(
    inputs: &SceneInputs,
    instance: PipelineInstance,
) -> Result<PipelineInstanceDetail, PluginError> {
    let transitions = inputs
        .store
        .list_pipeline_transitions(&instance.id)
        .map_err(PluginError::new)?;
    let bound = inputs
        .store
        .sessions_for_pipeline(&instance.id)
        .map_err(PluginError::new)?;
    let artifacts = inputs
        .artifacts
        .as_ref()
        .and_then(|artifacts| artifacts.list_for_instance(&instance.id).ok())
        .unwrap_or_default();
    let library = inputs.scenes.library();
    let stages = library
        .resolve_pipeline(&instance.pipeline_ref)
        .map(|entry| {
            entry
                .pipeline
                .stages
                .iter()
                .map(|stage| {
                    let loop_count = transitions
                        .iter()
                        .filter(|transition| transition.to_stage == stage.id)
                        .count();
                    let state = if instance.current_stage == stage.id {
                        "current"
                    } else if loop_count > 0 {
                        "done"
                    } else {
                        "pending"
                    };
                    let title = stage
                        .title
                        .clone()
                        .or_else(|| {
                            library
                                .resolve(&stage.scene)
                                .map(|scene| scene.scene.title.clone())
                        })
                        .unwrap_or_else(|| stage.id.clone());
                    let mut sessions: Vec<String> = bound
                        .iter()
                        .filter(|(_, bound_stage)| *bound_stage == stage.id)
                        .map(|(session, _)| session.clone())
                        .collect();
                    for transition in &transitions {
                        if transition.to_stage == stage.id {
                            if let Some(session) = &transition.session_id {
                                if !sessions.contains(session) {
                                    sessions.push(session.clone());
                                }
                            }
                        }
                    }
                    StageStatus {
                        id: stage.id.clone(),
                        scene_ref: stage.scene.clone(),
                        title,
                        state,
                        gate: gate_str(stage.gate.unwrap_or(scene::Gate::Suggest)),
                        loop_count,
                        sessions,
                        artifacts: artifacts
                            .iter()
                            .filter(|record| record.stage_id.as_deref() == Some(stage.id.as_str()))
                            .cloned()
                            .collect(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(PipelineInstanceDetail {
        instance,
        transitions,
        stages,
    })
}

fn gate_str(gate: scene::Gate) -> &'static str {
    match gate {
        scene::Gate::Suggest => "suggest",
        scene::Gate::Confirm => "confirm",
        scene::Gate::Auto => "auto",
    }
}

fn trigger_str(trigger: scene::TransitionTrigger) -> &'static str {
    match trigger {
        scene::TransitionTrigger::ExitCriteriaMet => "exit_criteria_met",
        scene::TransitionTrigger::TestsFailed => "tests_failed",
        scene::TransitionTrigger::UserRequest => "user_request",
    }
}

#[derive(Serialize)]
struct StageStatus {
    id: String,
    scene_ref: String,
    title: String,
    state: &'static str,
    gate: &'static str,
    loop_count: usize,
    sessions: Vec<String>,
    artifacts: Vec<SceneArtifactRecord>,
}

#[derive(Serialize)]
struct PipelineInstanceDetail {
    instance: PipelineInstance,
    transitions: Vec<PipelineTransitionRecord>,
    stages: Vec<StageStatus>,
}

#[derive(Serialize)]
struct PipelineStartOutcome {
    detail: PipelineInstanceDetail,
    applied_scene: Option<SceneApplyOutcome>,
}

#[derive(Serialize)]
struct PipelineAdvanceOutcome {
    instance: PipelineInstance,
    applied_scene: Option<SceneApplyOutcome>,
    session_plan: Option<crate::scene::SceneSessionParams>,
    escalation: Option<EscalationOut>,
    carried: Vec<String>,
}

#[derive(Serialize)]
struct SessionPipelineOut {
    instance_id: String,
    stage_id: String,
}

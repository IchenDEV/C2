//! Durable chief-of-staff goals. Sessions and project memory keep their existing owners.
//! One bounded, revisioned document is sufficient for a personal workspace; no shadow run table.
use crate::{
    memory::MemoryRecord,
    provider::ProviderId,
    session::{RunFailureReason, Session, SessionRunState},
    store::{Store, StoreError},
};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[path = "assistant_conversation.rs"]
mod assistant_conversation;
pub use assistant_conversation::*;
#[path = "assistant_project_hierarchy.rs"]
mod assistant_project_hierarchy;
pub use assistant_project_hierarchy::*;

pub const GLOBAL_MEMORY: &str = "codetwo://chief-of-staff";

pub(crate) fn install(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS assistant_state (singleton INTEGER PRIMARY KEY CHECK(singleton=1), revision INTEGER NOT NULL, body TEXT NOT NULL); CREATE TABLE IF NOT EXISTS assistant_worker_receipts(session_id TEXT NOT NULL,command_id TEXT NOT NULL,operation TEXT NOT NULL,response TEXT NOT NULL,PRIMARY KEY(session_id,command_id));")?;
    // Persisted writer guard: even a binary which predates the hierarchy cannot overwrite or
    // delete a document that has one, because the rule lives in the database file itself.
    conn.execute_batch(assistant_project_hierarchy::HIERARCHY_GUARD_SQL)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantSettings {
    pub enabled: bool,
    pub projects: Vec<String>,
    pub provider: ProviderId,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub concurrency: usize,
    pub turn_limit: u32,
    pub dispatch_limit: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssistantState {
    pub revision: u64,
    pub settings: Option<AssistantSettings>,
    pub goals: Vec<AssistantGoal>,
    /// Read-only migration input. New scheduling uses scoped_runs exclusively.
    pub run: Option<AssistantRun>,
    #[serde(default)]
    pub scoped_runs: Vec<ScopedReview>,
    #[serde(default)]
    pub attempts: std::collections::BTreeMap<String, AssistantAttempt>,
    pub observed: String,
    pub turns: u32,
    pub dispatches: u32,
    pub summary: String,
    pub attention: Option<String>,
    #[serde(default)]
    pub messages: Vec<CoordinationMessage>,
    #[serde(default)]
    pub questions: Vec<CoordinationQuestion>,
    #[serde(default)]
    pub changes: Vec<RequirementChange>,
    #[serde(default)]
    pub notifications: Vec<AssistantNotification>,
    #[serde(default)]
    pub requests: Vec<UserRequest>,
    #[serde(default)]
    pub reviews: Vec<ReviewRecord>,
    /// Durable user/assistant conversation. Execution facts stay in goals and receipts.
    #[serde(default)]
    pub conversation: Vec<ConversationTurn>,
    #[serde(default)]
    pub memory_proposals: Vec<MemoryProposal>,
    /// Client turn id -> argument hash, for same-id idempotency and different-argument rejection.
    #[serde(default)]
    pub turn_args: std::collections::BTreeMap<String, String>,
    /// Count of explicit user operations per goal; later user decisions outrank older analysis.
    #[serde(default)]
    pub user_gen: std::collections::BTreeMap<String, u64>,
    /// Versions each running intake saw, for the goals it may touch.
    #[serde(default)]
    pub intake_stamps: std::collections::BTreeMap<String, Vec<GoalStamp>>,
    /// User-owned external event sources. Inputs/receipts live in Store side tables.
    #[serde(default)]
    pub sources: Vec<crate::assistant_observation::SourceBinding>,
    /// Versioned user decision raising the global observation history quota.
    #[serde(default)]
    pub source_history_global: Option<u32>,
    /// Optional business Project hierarchy. Absent (never serialized) until a user enables it, so
    /// legacy bytes and fingerprints are unchanged. See `assistant_project_hierarchy`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hierarchy: Option<ProjectHierarchy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantGoal {
    pub id: String,
    pub project_path: String,
    pub title: String,
    pub acceptance: String,
    pub priority: u8,
    pub status: GoalStatus,
    pub next_step: String,
    pub blocker: String,
    pub assignments: Vec<Assignment>,
    pub verdict: Option<Acceptance>,
    #[serde(default = "initial_revision")]
    pub contract_revision: u64,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Paused,
    NeedsAttention,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assignment {
    pub id: String,
    pub instruction: String,
    pub session_id: Option<String>,
    /// Set before submitting. An unknown prompt outcome is never blindly replayed.
    pub submitted: bool,
    pub taken_over: bool,
    #[serde(default)]
    pub owned: bool,
    #[serde(default = "initial_revision")]
    pub contract_revision: u64,
    #[serde(default = "initial_revision")]
    pub confirmed_revision: u64,
    #[serde(default)]
    pub protocol: u8,
    #[serde(default)]
    pub stop_requested: bool,
    #[serde(default)]
    pub stop_sent: bool,
    #[serde(default)]
    pub result: Option<SubmittedResult>,
    #[serde(default)]
    pub inputs: Vec<DependencyVersion>,
    /// User-authorized input versions to adopt on resume, cleared only by worker confirmation.
    #[serde(default)]
    pub pending_inputs: Option<Vec<DependencyVersion>>,
    #[serde(default)]
    pub prior_results: Vec<SubmittedResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Acceptance {
    pub session_id: String,
    pub activity_revision: u64,
    pub evidence: String,
    pub artifacts: Vec<ArtifactStamp>,
    #[serde(default = "initial_revision")]
    pub contract_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactStamp {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyVersion {
    pub goal_id: String,
    pub contract_revision: u64,
    pub artifacts: Vec<ArtifactStamp>,
}
pub fn dependency_versions(
    state: &AssistantState,
    goal: &AssistantGoal,
) -> Option<Vec<DependencyVersion>> {
    goal.dependencies
        .iter()
        .map(|id| {
            state
                .goals
                .iter()
                .find(|g| {
                    &g.id == id
                        && g.status == GoalStatus::Completed
                        && state
                            .settings
                            .as_ref()
                            .is_some_and(|s| s.projects.contains(&g.project_path))
                })
                .and_then(|g| {
                    g.verdict
                        .as_ref()
                        .filter(|v| v.contract_revision == g.contract_revision)
                        .map(|v| DependencyVersion {
                            goal_id: id.clone(),
                            contract_revision: g.contract_revision,
                            artifacts: v.artifacts.clone(),
                        })
                })
        })
        .collect()
}
pub fn dependencies_valid(state: &AssistantState, goal: &AssistantGoal) -> bool {
    dependency_versions(state, goal)
        .is_some_and(|inputs| goal.assignments.last().is_none_or(|a| a.inputs == inputs))
}
pub(crate) fn dependency_resume_authorized(state: &AssistantState, goal: &AssistantGoal) -> bool {
    dependency_versions(state, goal).is_some_and(|inputs| {
        goal.assignments
            .last()
            .is_some_and(|a| a.pending_inputs.as_ref() == Some(&inputs))
    })
}
pub(crate) fn verify_dependencies(
    state: &AssistantState,
    goal: &AssistantGoal,
    sessions: &[Session],
) -> Result<(), StoreError> {
    for id in &goal.dependencies {
        let upstream = state
            .goals
            .iter()
            .find(|g| &g.id == id && g.status == GoalStatus::Completed)
            .ok_or_else(|| invalid("input goal is not accepted"))?;
        let verdict = upstream
            .verdict
            .as_ref()
            .ok_or_else(|| invalid("input has no accepted version"))?;
        if verdict.contract_revision != upstream.contract_revision {
            return Err(invalid("input acceptance belongs to an older requirement"));
        }
        let session = session_for(sessions, &verdict.session_id)?;
        if !matches!(session.activity.state, SessionRunState::Idle)
            || session.activity.revision != verdict.activity_revision
        {
            return Err(invalid("input session version changed"));
        }
        verify_artifacts(&session.cwd, &verdict.artifacts)?;
    }
    Ok(())
}
fn initial_revision() -> u64 {
    1
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationMessage {
    #[serde(default)]
    pub created_at: String,
    pub id: String,
    pub goal_id: String,
    pub assignment_id: String,
    pub contract_revision: u64,
    pub author: String,
    pub content: String,
    pub mode: String,
    pub state: String,
    pub outcome: String,
    pub reply_to: Option<String>,
    pub delivery_id: Option<String>,
    pub confirmed: bool,
    #[serde(default)]
    pub expected_turn: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationQuestion {
    #[serde(default)]
    pub created_at: String,
    pub id: String,
    pub goal_id: String,
    pub assignment_id: String,
    pub contract_revision: u64,
    pub author: String,
    pub title: String,
    pub context: String,
    pub options: Vec<String>,
    pub blocking: bool,
    pub state: String,
    pub answer: Option<String>,
    pub answered_by: Option<String>,
    pub source_input: Option<crate::session::PendingInput>,
    pub request_id: Option<String>,
    pub answer_delivery_id: Option<String>,
    #[serde(default)]
    pub factual: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementChange {
    #[serde(default)]
    pub created_at: String,
    pub id: String,
    pub goal_id: String,
    pub from_revision: u64,
    pub to_revision: u64,
    pub before_title: String,
    pub before_acceptance: String,
    pub title: String,
    pub acceptance: String,
    pub reason: String,
    pub state: String,
    pub author: String,
    pub delivery_id: Option<String>,
    pub proposed: bool,
    pub memory_id: Option<String>,
    #[serde(default)]
    pub remember: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantNotification {
    #[serde(default)]
    pub created_at: String,
    pub id: String,
    pub goal_id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub session_id: Option<String>,
    pub reference_id: String,
    pub read: bool,
    pub desktop: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmittedResult {
    #[serde(default)]
    pub created_at: String,
    pub id: String,
    pub contract_revision: u64,
    pub activity_revision: u64,
    pub evidence: String,
    pub artifacts: Vec<ArtifactStamp>,
    pub state: String,
    #[serde(default)]
    pub inputs: Vec<DependencyVersion>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRequest {
    #[serde(default)]
    pub created_at: String,
    pub id: String,
    pub project_path: String,
    pub content: String,
    pub handled: bool,
    /// The conversation message which routed this request, if any.
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "crate::assistant::is_zero_u64")]
    pub route_revision: u64,
}

pub(crate) fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRecord {
    pub id: String,
    pub goal_id: String,
    pub assignment_id: String,
    pub contract_revision: u64,
    pub verdict: String,
    pub evidence: String,
    pub artifacts: Vec<ArtifactStamp>,
    pub created_at: String,
}
fn retain_result(assignment: &mut Assignment) {
    if let Some(result) = assignment.result.take() {
        assignment.prior_results.push(result);
    }
}
fn record_review(
    state: &mut AssistantState,
    index: usize,
    verdict: &str,
    evidence: &str,
    artifacts: Vec<ArtifactStamp>,
) {
    let goal = &state.goals[index];
    state.reviews.push(ReviewRecord {
        id: uuid::Uuid::new_v4().to_string(),
        goal_id: goal.id.clone(),
        assignment_id: goal.assignments.last().unwrap().id.clone(),
        contract_revision: goal.contract_revision,
        verdict: verdict.into(),
        evidence: evidence.into(),
        artifacts,
        created_at: chrono::Utc::now().to_rfc3339(),
    });
}
pub(crate) fn question_unresolved(q: &CoordinationQuestion) -> bool {
    q.blocking
        && (matches!(q.state.as_str(), "open" | "delivery_unknown" | "expired")
            || q.answer_delivery_id.as_deref() == Some("native:claimed"))
}
fn pending_instructions(state: &AssistantState, goal: &AssistantGoal) -> bool {
    goal.assignments.last().is_some_and(|a| {
        state.messages.iter().any(|m| {
            m.assignment_id == a.id
                && m.contract_revision == goal.contract_revision
                && m.author != "worker"
                && !m.confirmed
                && matches!(
                    m.state.as_str(),
                    "recorded" | "queued" | "submitting" | "accepted" | "unknown" | "failed"
                )
        })
    })
}
pub fn notify(
    state: &mut AssistantState,
    goal_id: &str,
    kind: &str,
    reference_id: &str,
    title: &str,
    body: &str,
    session_id: Option<String>,
) {
    let id = format!("{kind}:{reference_id}");
    if state.notifications.iter().any(|n| n.id == id) {
        return;
    }
    state.notifications.push(AssistantNotification {
        created_at: chrono::Utc::now().to_rfc3339(),
        id,
        goal_id: goal_id.into(),
        kind: kind.into(),
        title: title.into(),
        body: body.into(),
        session_id,
        reference_id: reference_id.into(),
        read: false,
        desktop: "pending".into(),
    });
}
fn latest_assignment<'a>(
    state: &'a AssistantState,
    goal_id: &str,
) -> Result<(usize, &'a Assignment), StoreError> {
    let index = state
        .goals
        .iter()
        .position(|g| g.id == goal_id)
        .ok_or_else(|| invalid("unknown goal"))?;
    let goal = &state.goals[index];
    if !state
        .settings
        .as_ref()
        .is_some_and(|s| s.projects.contains(&goal.project_path))
    {
        return Err(invalid("goal outside the selected projects"));
    }
    let assignment = goal
        .assignments
        .last()
        .ok_or_else(|| invalid("goal has no assignment"))?;
    Ok((index, assignment))
}
fn can_control(goal: &AssistantGoal, assignment: &Assignment) -> bool {
    assignment.owned
        && !assignment.taken_over
        && !matches!(goal.status, GoalStatus::Cancelled | GoalStatus::Completed)
}

pub fn native_answer_valid(state: &AssistantState, question: &CoordinationQuestion) -> bool {
    state
        .goals
        .iter()
        .find(|g| g.id == question.goal_id)
        .is_some_and(|g| {
            g.status == GoalStatus::Active
                && dependencies_valid(state, g)
                && g.contract_revision == question.contract_revision
                && state.settings.as_ref().is_some_and(|s| {
                    s.projects.contains(&g.project_path)
                        && (s.enabled || question.answered_by.as_deref() == Some("user"))
                })
                && g.assignments.last().is_some_and(|a| {
                    a.id == question.assignment_id && a.owned && !a.taken_over && !a.stop_requested
                })
        })
}
pub fn add_message(
    state: &mut AssistantState,
    goal_id: &str,
    content: String,
    mode: &str,
    author: &str,
    reply_to: Option<String>,
) -> Result<String, StoreError> {
    bounded(&content, 16000)?;
    if !matches!(mode, "auto" | "queue") {
        return Err(invalid("invalid communication mode"));
    }
    let (index, assignment) = latest_assignment(state, goal_id)?;
    if !can_control(&state.goals[index], assignment) {
        return Err(invalid("assignment is observation-only or taken over"));
    }
    let assignment_id = assignment.id.clone();
    retain_result(state.goals[index].assignments.last_mut().unwrap());
    let id = uuid::Uuid::new_v4().to_string();
    state.messages.push(CoordinationMessage {
        created_at: chrono::Utc::now().to_rfc3339(),
        id: id.clone(),
        goal_id: goal_id.into(),
        assignment_id,
        contract_revision: state.goals[index].contract_revision,
        author: author.into(),
        content,
        mode: mode.into(),
        state: "recorded".into(),
        outcome: String::new(),
        reply_to,
        delivery_id: None,
        confirmed: false,
        expected_turn: None,
    });
    Ok(id)
}

pub fn open_question(
    state: &mut AssistantState,
    goal_id: &str,
    title: String,
    context: String,
    options: Vec<String>,
    blocking: bool,
    author: &str,
) -> Result<String, StoreError> {
    bounded(&title, 4000)?;
    if context.len() > 8000 || options.len() > 8 || options.iter().any(|o| o.len() > 2000) {
        return Err(invalid("question exceeds limits"));
    }
    let (index, assignment) = latest_assignment(state, goal_id)?;
    let id = uuid::Uuid::new_v4().to_string();
    let assignment_id = assignment.id.clone();
    let session = assignment.session_id.clone();
    state.questions.push(CoordinationQuestion {
        created_at: chrono::Utc::now().to_rfc3339(),
        id: id.clone(),
        goal_id: goal_id.into(),
        assignment_id,
        contract_revision: state.goals[index].contract_revision,
        author: author.into(),
        title: title.clone(),
        context: context.clone(),
        options,
        blocking,
        state: "open".into(),
        answer: None,
        answered_by: None,
        source_input: None,
        request_id: None,
        answer_delivery_id: None,
        factual: false,
    });
    notify(state, goal_id, "question", &id, &title, &context, session);
    Ok(id)
}

pub fn answer_question(
    state: &mut AssistantState,
    id: &str,
    answer: String,
    author: &str,
) -> Result<(), StoreError> {
    bounded(&answer, 8000)?;
    let index = state
        .questions
        .iter()
        .position(|q| q.id == id)
        .ok_or_else(|| invalid("unknown question"))?;
    let q = state.questions[index].clone();
    if q.state != "open" {
        return Err(invalid("question already answered or expired"));
    }
    state.questions[index].answer_delivery_id = None;
    if let Some(input) = &q.source_input {
        if input.kind == crate::session::PendingInputKind::Permission
            && (author != "user"
                || (answer != "cancel" && !input.options.iter().any(|(id, _)| id == &answer)))
        {
            return Err(invalid("choose a concrete permission option or cancel"));
        }
    }
    if let Some(request_id) = &q.request_id {
        if !state
            .requests
            .iter()
            .any(|r| &r.id == request_id && !r.handled)
        {
            return Err(invalid("request is no longer pending"));
        }
    } else if q.author.starts_with("observation:") {
        // Observer questions are answered locally on the exact goal/contract. The answer is
        // never forwarded to a worker: external feedback needs a separate explicit user
        // execution directive through the normal intake path.
        let goal = state
            .goals
            .iter()
            .find(|g| g.id == q.goal_id)
            .ok_or_else(|| invalid("unknown goal"))?;
        if author != "user"
            || q.source_input.is_some()
            || q.contract_revision != goal.contract_revision
            || matches!(goal.status, GoalStatus::Cancelled | GoalStatus::Completed)
            || !state
                .settings
                .as_ref()
                .is_some_and(|s| s.projects.contains(&goal.project_path))
        {
            return Err(invalid("question version or control changed"));
        }
        state.questions[index].answer_delivery_id = Some("local:observation".into());
    } else {
        let (g, a) = latest_assignment(state, &q.goal_id)?;
        if q.contract_revision != state.goals[g].contract_revision
            || q.assignment_id != a.id
            || !can_control(&state.goals[g], a)
        {
            return Err(invalid("question version or control changed"));
        }
        if q.source_input.is_none() {
            let message = add_message(
                state,
                &q.goal_id,
                format!("Answer to question {}: {}", q.title, answer),
                "auto",
                author,
                Some(id.into()),
            )?;
            state.questions[index].answer_delivery_id = Some(message);
        }
    }
    state.questions[index].answer = Some(answer);
    state.questions[index].answered_by = Some(author.into());
    state.questions[index].state = "answered".into();
    for n in &mut state.notifications {
        if n.reference_id == id {
            n.read = true;
        }
    }
    Ok(())
}

pub fn verify_artifacts(cwd: &str, artifacts: &[ArtifactStamp]) -> Result<(), StoreError> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    if artifacts.is_empty() || artifacts.len() > 16 {
        return Err(invalid("acceptance needs 1–16 versioned files"));
    }
    let root =
        std::fs::canonicalize(cwd).map_err(|_| invalid("artifact workspace is unavailable"))?;
    let mut total = 0usize;
    for artifact in artifacts {
        let relative = std::path::Path::new(&artifact.path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(invalid(
                "artifact path must be relative to the execution workspace",
            ));
        }
        let path = root
            .join(relative)
            .canonicalize()
            .map_err(|_| invalid("artifact is unavailable"))?;
        if !path.starts_with(&root) {
            return Err(invalid("artifact escapes the execution workspace"));
        }
        if !std::fs::metadata(&path)
            .map_err(|_| invalid("artifact metadata is unavailable"))?
            .is_file()
        {
            return Err(invalid("artifact must be a regular file"));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .map_err(|_| invalid("artifact cannot be read"))?
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid("artifact cannot be read"))?;
        total += bytes.len();
        if bytes.len() > 16 * 1024 * 1024 || total > 64 * 1024 * 1024 {
            return Err(invalid("artifact verification size limit reached"));
        }
        if format!("{:x}", Sha256::digest(&bytes)) != artifact.sha256 {
            return Err(invalid("artifact version changed"));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantRun {
    pub id: String,
    pub session_id: Option<String>,
    pub submitted: bool,
    pub base_revision: u64,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewCredential {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default)]
    pub project_version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_id: Option<String>,
    #[serde(default)]
    pub binding_version: u64,
    #[serde(default)]
    pub owner_epoch: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_hash: Option<String>,
    #[serde(default)]
    pub instruction_revision: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memory_refs: Vec<(String, String)>,
}

/// Review history is owned by the same document as its goal/request. One active run per scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopedReview {
    pub scope: String,
    /// Only a known local commit rejection can wait for worker capacity.
    #[serde(default)]
    pub retry_on_capacity: Option<bool>,
    pub run: AssistantRun,
    pub observed: String,
    pub state: String,
    pub summary: String,
    pub error: Option<String>,
    /// Set only for an observation review (kind=observation); legacy runs are kind=goal.
    #[serde(default)]
    pub observation: Option<crate::assistant_observation::ObservationRun>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<ReviewCredential>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantAttempt {
    pub state: String,
    pub outcome: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssistantSnapshot {
    pub state: AssistantState,
    pub sessions: Vec<Session>,
    pub memories: Vec<MemoryRecord>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AssistantEdit {
    Settings {
        settings: AssistantSettings,
    },
    Goal {
        id: Option<String>,
        project_path: String,
        title: String,
        acceptance: String,
        priority: u8,
    },
    Status {
        id: String,
        status: GoalStatus,
    },
    Takeover {
        id: String,
    },
    Wake,
    Message {
        goal_id: String,
        content: String,
        #[serde(default)]
        queue: bool,
    },
    Answer {
        question_id: String,
        answer: String,
    },
    RetryQuestion {
        question_id: String,
    },
    Control {
        goal_id: String,
        operation: String,
    },
    Acknowledge {
        notification_id: String,
    },
    Request {
        project_path: String,
        content: String,
    },
    Dependencies {
        goal_id: String,
        dependencies: Vec<String>,
    },
    RememberChange {
        change_id: String,
    },
    ApplyChange {
        change_id: String,
        #[serde(default)]
        remember: bool,
    },
    RejectChange {
        change_id: String,
    },
    Review {
        goal_id: String,
        verdict: String,
        evidence: String,
    },
    Say {
        turn_id: String,
        content: String,
        #[serde(default)]
        project_paths: Vec<String>,
        #[serde(default)]
        answers_question: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        actor: Option<String>,
    },
    ConfirmMemory {
        proposal_id: String,
        content_hash: String,
    },
    RejectMemory {
        proposal_id: String,
        content_hash: String,
    },
    /// Create/update/revoke (enabled=false) a source binding at its exact version.
    Source {
        binding: crate::assistant_observation::SourceBindingInput,
    },
    /// Versioned raise of source/global observation history quota.
    SourceCapacity {
        source_id: String,
        binding_version: u64,
        #[serde(default)]
        source_history_limit: Option<u32>,
        #[serde(default)]
        global_history_limit: Option<u32>,
    },
    /// User-only approval of one exact cursor reset. The adapter can later present only the
    /// returned reference; it cannot assert approval itself.
    SourceReset {
        source_id: String,
        binding_version: u64,
        stream_id: String,
        #[serde(default)]
        checkpoint_before: Option<String>,
        #[serde(default)]
        checkpoint_after: Option<String>,
        reason: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantDecision {
    pub summary: String,
    pub actions: Vec<AssistantAction>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AssistantAction {
    Dispatch {
        goal_id: String,
        instruction: String,
    },
    Link {
        goal_id: String,
        session_id: String,
    },
    Update {
        goal_id: String,
        next_step: String,
        blocker: String,
    },
    Communicate {
        goal_id: String,
        content: String,
    },
    Ask {
        goal_id: String,
        title: String,
        context: String,
        #[serde(default)]
        options: Vec<String>,
        #[serde(default)]
        blocking: bool,
    },
    Answer {
        goal_id: String,
        question_id: String,
        answer: String,
        memory_ids: Vec<String>,
    },
    ProposeChange {
        goal_id: String,
        title: String,
        acceptance: String,
        reason: String,
    },
    CreateGoal {
        request_id: String,
        title: String,
        acceptance: String,
        priority: u8,
    },
    ClarifyRequest {
        request_id: String,
        title: String,
        options: Vec<String>,
    },
    Accept {
        goal_id: String,
        session_id: String,
        activity_revision: u64,
        evidence: String,
        artifacts: Vec<ArtifactStamp>,
    },
}

fn invalid(text: &str) -> StoreError {
    StoreError::InvalidAssistant(text.into())
}
fn bounded(text: &str, max: usize) -> Result<(), StoreError> {
    if text.trim().is_empty() || text.len() > max {
        return Err(invalid("text is empty or too long"));
    }
    Ok(())
}

pub fn revise_goal(
    state: &mut AssistantState,
    index: usize,
    title: String,
    acceptance: String,
    reason: String,
    author: &str,
    remember: bool,
) -> Result<(), StoreError> {
    bounded(&title, 4000)?;
    bounded(&acceptance, 8000)?;
    bounded(&reason, 4000)?;
    let old = state.goals[index].clone();
    let next = old.contract_revision + 1;
    let id = uuid::Uuid::new_v4().to_string();
    let goal = &mut state.goals[index];
    goal.title = title.clone();
    goal.acceptance = acceptance.clone();
    goal.contract_revision = next;
    goal.verdict = None;
    if goal.status == GoalStatus::Completed {
        goal.status = GoalStatus::Paused;
    }
    if let Some(a) = goal.assignments.last_mut() {
        if a.owned && !a.taken_over {
            a.stop_requested = a.submitted;
            retain_result(a);
            a.confirmed_revision = 0;
        }
    }
    for q in &mut state.questions {
        if q.goal_id == old.id && q.state == "open" {
            q.state = "expired".into();
        }
    }
    for m in &mut state.messages {
        if m.goal_id == old.id && matches!(m.state.as_str(), "recorded" | "queued") {
            m.state = "cancelled".into();
        }
    }
    for c in &mut state.changes {
        if c.goal_id == old.id && !matches!(c.state.as_str(), "confirmed" | "rejected" | "applied")
        {
            c.state = "superseded".into();
        }
    }
    let has_worker = old
        .assignments
        .last()
        .is_some_and(|a| a.owned && !a.taken_over);
    state.changes.push(RequirementChange {
        created_at: chrono::Utc::now().to_rfc3339(),
        id: id.clone(),
        goal_id: old.id.clone(),
        from_revision: old.contract_revision,
        to_revision: next,
        before_title: old.title,
        before_acceptance: old.acceptance,
        title,
        acceptance,
        reason: reason.clone(),
        state: if has_worker { "recorded" } else { "confirmed" }.into(),
        author: author.into(),
        delivery_id: None,
        proposed: false,
        memory_id: None,
        remember,
    });
    notify(
        state,
        &old.id,
        "change",
        &id,
        "Requirements changed",
        &reason,
        old.assignments.last().and_then(|a| a.session_id.clone()),
    );
    Ok(())
}

pub fn accept_result(
    state: &mut AssistantState,
    index: usize,
    session: &Session,
    sessions: &[Session],
    activity_revision: u64,
    evidence: String,
    artifacts: Vec<ArtifactStamp>,
) -> Result<(), StoreError> {
    bounded(&evidence, 8000)?;
    verify_dependencies(state, &state.goals[index], sessions)?;
    let goal = &state.goals[index];
    if goal.status != GoalStatus::Active
        || pending_instructions(state, goal)
        || state.questions.iter().any(|q| {
            q.goal_id == goal.id
                && q.contract_revision == goal.contract_revision
                && question_unresolved(q)
        })
        || !dependencies_valid(state, goal)
    {
        return Err(invalid(
            "resolve current questions and dependency versions before acceptance",
        ));
    }
    let a = goal
        .assignments
        .last()
        .ok_or_else(|| invalid("no current assignment"))?;
    if !matches!(session.activity.state, SessionRunState::Idle)
        || session.activity.revision != activity_revision
        || a.session_id.as_deref() != Some(&session.id)
        || a.contract_revision != goal.contract_revision
        || a.confirmed_revision != goal.contract_revision
        || a.stop_requested
    {
        return Err(invalid(
            "acceptance must reference the confirmed current idle assignment version",
        ));
    }
    if a.protocol > 0 {
        let result = a
            .result
            .as_ref()
            .filter(|r| {
                r.state == "submitted"
                    && r.contract_revision == goal.contract_revision
                    && r.artifacts == artifacts
                    && r.inputs == a.inputs
                    && (r.activity_revision == activity_revision
                        || r.activity_revision + 1 == activity_revision)
            })
            .ok_or_else(|| invalid("acceptance needs the current submitted deliverable"))?;
        if evidence.trim().is_empty() || result.evidence.trim().is_empty() {
            return Err(invalid("verification evidence missing"));
        }
    }
    verify_artifacts(&session.cwd, &artifacts)?;
    record_review(state, index, "accepted", &evidence, artifacts.clone());
    let goal = &state.goals[index];
    let id = format!(
        "{}:{}:{}",
        goal.id, goal.contract_revision, activity_revision
    );
    let goal_id = goal.id.clone();
    let title = goal.title.clone();
    let goal = &mut state.goals[index];
    goal.verdict = Some(Acceptance {
        session_id: session.id.clone(),
        activity_revision,
        evidence: evidence.clone(),
        artifacts,
        contract_revision: goal.contract_revision,
    });
    goal.status = GoalStatus::Completed;
    goal.next_step.clear();
    goal.blocker.clear();
    if let Some(result) = goal.assignments.last_mut().and_then(|a| a.result.as_mut()) {
        result.state = "accepted".into();
    }
    notify(
        state,
        &goal_id,
        "accepted",
        &id,
        &title,
        &evidence,
        Some(session.id.clone()),
    );
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkerOperation {
    Context,
    Progress {
        content: String,
    },
    Ask {
        title: String,
        context: String,
        #[serde(default)]
        options: Vec<String>,
        #[serde(default)]
        blocking: bool,
        #[serde(default)]
        factual: bool,
    },
    ProposeChange {
        title: String,
        acceptance: String,
        reason: String,
    },
    Confirm {
        contract_revision: u64,
        #[serde(default)]
        message_ids: Vec<String>,
    },
    Submit {
        contract_revision: u64,
        evidence: String,
        artifacts: Vec<ArtifactStamp>,
    },
}

impl Store {
    pub(crate) fn require_assistant_prompt(
        &self,
        id: &str,
        session: &str,
    ) -> Result<(), StoreError> {
        require_prompt_on(&self.conn.lock().unwrap(), id, session)
    }

    pub fn assistant_worker(
        &self,
        session: &str,
        operation: WorkerOperation,
    ) -> Result<serde_json::Value, StoreError> {
        self.assistant_worker_command(session, &uuid::Uuid::new_v4().to_string(), operation)
    }
    pub fn assistant_worker_command(
        &self,
        session: &str,
        command_id: &str,
        operation: WorkerOperation,
    ) -> Result<serde_json::Value, StoreError> {
        bounded(command_id, 200)?;
        let payload = serde_json::to_string(&operation)?;
        let existing:Option<(String,String)>=self.conn.lock().unwrap().query_row("SELECT operation,response FROM assistant_worker_receipts WHERE session_id=?1 AND command_id=?2",rusqlite::params![session,command_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((original, response)) = existing {
            if original != payload {
                return Err(invalid("worker command id content changed"));
            }
            return Ok(serde_json::from_str(&response)?);
        }
        let mut state = self.assistant_state()?;
        let base = state.revision;
        let index = state
            .goals
            .iter()
            .position(|g| {
                g.assignments
                    .last()
                    .is_some_and(|a| a.session_id.as_deref() == Some(session))
            })
            .ok_or_else(|| invalid("session has no current assignment"))?;
        let goal = state.goals[index].clone();
        let assignment = goal.assignments.last().unwrap().clone();
        if !state
            .settings
            .as_ref()
            .is_some_and(|s| s.projects.contains(&goal.project_path))
            || !can_control(&goal, &assignment)
        {
            return Err(invalid("assignment outside scope or taken over"));
        }
        if matches!(operation, WorkerOperation::Context) {
            let (memory_block, shared_block) = if let Some(h) = state.hierarchy.as_ref() {
                if h.goal_owners.contains_key(&goal.id) {
                    let actor = HostActor::executor(&state, &goal.id)?;
                    let grant = memory_grant(&state, &actor)?;
                    let mut mem_blocks = Vec::new();
                    for scope_path in grant.scopes() {
                        let recalled = self.memory_context_with_receipt(
                            scope_path,
                            session,
                            &format!("{} {}", goal.title, goal.acceptance),
                        )?;
                        if !recalled.block.is_empty() {
                            mem_blocks.push(recalled.block);
                        }
                    }
                    (mem_blocks.join("\n\n"), String::new())
                } else {
                    let memory = self.memory_context_with_receipt(
                        &goal.project_path,
                        session,
                        &format!("{} {}", goal.title, goal.acceptance),
                    )?;
                    let shared =
                        self.memory_context_with_receipt(GLOBAL_MEMORY, session, &goal.title)?;
                    (memory.block, shared.block)
                }
            } else {
                let memory = self.memory_context_with_receipt(
                    &goal.project_path,
                    session,
                    &format!("{} {}", goal.title, goal.acceptance),
                )?;
                let shared = self.memory_context_with_receipt(GLOBAL_MEMORY, session, &goal.title)?;
                (memory.block, shared.block)
            };
            let inputs:Vec<_>=goal.dependencies.iter().filter_map(|id|state.goals.iter().find(|g|&g.id==id && state.settings.as_ref().is_some_and(|s|s.projects.contains(&g.project_path)))).filter_map(|g|g.verdict.as_ref().and_then(|v|self.get_session(&v.session_id).ok().flatten().map(|s|serde_json::json!({"goal_id":g.id,"title":g.title,"contract_revision":g.contract_revision,"workspace":s.cwd,"artifacts":v.artifacts})))).collect();
            return Ok(
                serde_json::json!({"goal":goal,"assignment_id":assignment.id,"messages":state.messages.iter().filter(|m|m.assignment_id==assignment.id).collect::<Vec<_>>(),"questions":state.questions.iter().filter(|q|q.assignment_id==assignment.id).collect::<Vec<_>>(),"memory":memory_block,"shared_memory":shared_block,"inputs":inputs}),
            );
        }
        if !matches!(&operation, WorkerOperation::Confirm { .. })
            && (assignment.stop_requested
                || assignment.confirmed_revision != goal.contract_revision)
        {
            return Err(invalid(
                "confirm current requirements before reporting or asking",
            ));
        }
        let output = match operation {
            WorkerOperation::Progress { content } => {
                bounded(&content, 8000)?;
                state.goals[index].next_step = content.clone();
                let id = uuid::Uuid::new_v4().to_string();
                state.messages.push(CoordinationMessage {
                    created_at: chrono::Utc::now().to_rfc3339(),
                    id: id.clone(),
                    goal_id: goal.id.clone(),
                    assignment_id: assignment.id.clone(),
                    contract_revision: assignment.contract_revision,
                    author: "worker".into(),
                    content,
                    mode: "report".into(),
                    state: "received".into(),
                    outcome: String::new(),
                    reply_to: None,
                    delivery_id: None,
                    confirmed: true,
                    expected_turn: None,
                });
                serde_json::json!({"id":id})
            }
            WorkerOperation::Ask {
                title,
                context,
                options,
                blocking,
                factual,
            } => {
                let id = open_question(
                    &mut state, &goal.id, title, context, options, blocking, "worker",
                )?;
                state.questions.last_mut().unwrap().factual = factual;
                serde_json::json!({"question_id":id,"state":"open"})
            }
            WorkerOperation::ProposeChange {
                title,
                acceptance,
                reason,
            } => {
                bounded(&title, 4000)?;
                bounded(&acceptance, 8000)?;
                bounded(&reason, 4000)?;
                let id = uuid::Uuid::new_v4().to_string();
                state.changes.push(RequirementChange {
                    created_at: chrono::Utc::now().to_rfc3339(),
                    id: id.clone(),
                    goal_id: goal.id.clone(),
                    from_revision: goal.contract_revision,
                    to_revision: goal.contract_revision + 1,
                    before_title: goal.title.clone(),
                    before_acceptance: goal.acceptance.clone(),
                    title,
                    acceptance,
                    reason: reason.clone(),
                    state: "proposed".into(),
                    author: "worker".into(),
                    delivery_id: None,
                    proposed: true,
                    memory_id: None,
                    remember: false,
                });
                notify(
                    &mut state,
                    &goal.id,
                    "proposal",
                    &id,
                    "Worker proposed a requirement change",
                    &reason,
                    Some(session.into()),
                );
                serde_json::json!({"change_id":id})
            }
            WorkerOperation::Confirm {
                contract_revision,
                message_ids,
            } => {
                if contract_revision != goal.contract_revision
                    || assignment.stop_requested
                    || message_ids.len() > 50
                {
                    return Err(invalid("requirement version is not ready for confirmation"));
                }
                for id in &message_ids {
                    let message = state
                        .messages
                        .iter_mut()
                        .find(|m| {
                            &m.id == id
                                && m.assignment_id == assignment.id
                                && m.contract_revision == contract_revision
                        })
                        .ok_or_else(|| invalid("message is not for this assignment"))?;
                    let accepted = self
                        .prompt_delivery(id)?
                        .is_some_and(|d| d.state == "accepted")
                        || self.command_receipt("c2-delivery-prompt", id)?.is_some();
                    if !accepted && message.state != "accepted" {
                        return Err(invalid("message is not delivered for this assignment"));
                    }
                    message.state = "accepted".into();
                }
                if assignment.contract_revision != contract_revision
                    && state.changes.iter().any(|c| {
                        c.goal_id == goal.id
                            && !c.proposed
                            && c.to_revision == contract_revision
                            && !matches!(c.state.as_str(), "confirmed" | "superseded")
                            && !c
                                .delivery_id
                                .as_ref()
                                .is_some_and(|id| message_ids.contains(id))
                    })
                {
                    return Err(invalid("confirm the delivered requirement-change message"));
                }
                verify_dependencies(&state, &goal, &self.list_sessions()?)?;
                let inputs = dependency_versions(&state, &goal)
                    .ok_or_else(|| invalid("input goals are not accepted"))?;
                let a = state.goals[index].assignments.last_mut().unwrap();
                if a.inputs != inputs {
                    retain_result(a);
                }
                a.contract_revision = contract_revision;
                a.confirmed_revision = contract_revision;
                a.inputs = inputs;
                a.pending_inputs = None;
                for m in &mut state.messages {
                    if message_ids.contains(&m.id) {
                        m.confirmed = true;
                    }
                }
                for c in &mut state.changes {
                    if c.goal_id == goal.id
                        && c.to_revision == contract_revision
                        && !c.proposed
                        && c.state == "awaiting_confirmation"
                    {
                        c.state = "confirmed".into();
                    }
                }
                serde_json::json!({"confirmed_revision":contract_revision})
            }
            WorkerOperation::Submit {
                contract_revision,
                evidence,
                artifacts,
            } => {
                bounded(&evidence, 8000)?;
                if pending_instructions(&state, &goal)
                    || !dependencies_valid(&state, &goal)
                    || contract_revision != goal.contract_revision
                    || assignment.confirmed_revision != contract_revision
                    || assignment.stop_requested
                    || state.questions.iter().any(|q| {
                        q.goal_id == goal.id
                            && q.contract_revision == goal.contract_revision
                            && question_unresolved(q)
                    })
                {
                    return Err(invalid(
                        "resolve questions and confirm current requirements before submitting",
                    ));
                }
                verify_dependencies(&state, &goal, &self.list_sessions()?)?;
                let session = self
                    .get_session(session)?
                    .ok_or_else(|| invalid("session missing"))?;
                verify_artifacts(&session.cwd, &artifacts)?;
                retain_result(state.goals[index].assignments.last_mut().unwrap());
                let id = uuid::Uuid::new_v4().to_string();
                state.goals[index].assignments.last_mut().unwrap().result = Some(SubmittedResult {
                    created_at: chrono::Utc::now().to_rfc3339(),
                    id: id.clone(),
                    contract_revision,
                    activity_revision: session.activity.revision,
                    evidence: evidence.clone(),
                    artifacts,
                    state: "submitted".into(),
                    inputs: assignment.inputs.clone(),
                });
                notify(
                    &mut state,
                    &goal.id,
                    "deliverable",
                    &id,
                    &goal.title,
                    &evidence,
                    Some(session.id.clone()),
                );
                serde_json::json!({"result_id":id,"state":"submitted"})
            }
            WorkerOperation::Context => unreachable!(),
        };
        state.observed.clear();
        self.save_assistant_with_receipt(
            base,
            state,
            Some((
                session,
                command_id,
                &payload,
                &serde_json::to_string(&output)?,
            )),
        )?;
        Ok(output)
    }
}

/// The one implementation of task control. Manual edits and bound intake actions both use it.
/// `sessions` is only consulted by `resume`.
pub(crate) fn control_goal(
    state: &mut AssistantState,
    goal_id: &str,
    operation: &str,
    sessions: &[Session],
) -> Result<(), StoreError> {
    if !matches!(operation, "stop" | "cancel" | "resume") {
        return Err(invalid("unknown task control"));
    }
    let (index, a) = latest_assignment(state, goal_id)?;
    if !can_control(&state.goals[index], a) {
        return Err(invalid("assignment is not controlled by the assistant"));
    }
    if operation == "resume" {
        if a.stop_requested {
            return Err(invalid("wait for the stop receipt before resuming"));
        }
        verify_dependencies(state, &state.goals[index], sessions)?;
        let inputs = dependency_versions(state, &state.goals[index])
            .ok_or_else(|| invalid("input goals are not accepted"))?;
        state.goals[index]
            .assignments
            .last_mut()
            .unwrap()
            .pending_inputs = Some(inputs);
        state.goals[index].status = GoalStatus::Active;
        state.goals[index].blocker.clear();
        // A recorded requirement change is itself the resume instruction. Do not
        // start a turn ahead of it which cannot yet confirm the delivered change.
        let change_will_resume = state.changes.iter().any(|c| {
            c.goal_id == goal_id
                && !c.proposed
                && c.to_revision == state.goals[index].contract_revision
                && c.state == "recorded"
        });
        if !change_will_resume {
            add_message(state,goal_id,"Resume the current goal using its latest acceptance criteria and accepted dependency versions. Confirm the requirement version and delivered instructions before working.".into(),"queue","user",None)?;
        }
    } else {
        state.goals[index].status = if operation == "cancel" {
            GoalStatus::Cancelled
        } else {
            GoalStatus::Paused
        };
        state.goals[index]
            .assignments
            .last_mut()
            .unwrap()
            .stop_requested = true;
        for m in &mut state.messages {
            if m.goal_id == goal_id && m.delivery_id.is_none() {
                m.state = "cancelled".into();
            }
        }
    }
    Ok(())
}

/// Revision-CAS save inside a caller-owned transaction, so other Store tables (observation
/// receipts, outputs) commit or roll back with the assistant document.
pub(crate) fn save_assistant_in_tx(
    tx: &rusqlite::Transaction,
    expected: u64,
    mut state: AssistantState,
) -> Result<AssistantState, StoreError> {
    if state.messages.len() > 1000
        || state.questions.len() > 500
        || state.changes.len() > 500
        || state.requests.len() > 200
        || state.conversation.len() > MAX_CONVERSATION
        || state.memory_proposals.len() > MAX_PROPOSALS
    {
        return Err(invalid("coordination history limit reached"));
    }
    if state.notifications.len() > 500 {
        state.notifications.retain(|n| !n.read);
    }
    let current: u64 = tx
        .query_row(
            "SELECT revision FROM assistant_state WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0);
    if expected != current {
        return Err(invalid(
            "assistant changed; refresh before applying this decision",
        ));
    }
    guard_save(tx, &state)?;
    for index in 0..state.questions.len() {
        let q = &state.questions[index];
        if q.source_input.is_some()
            && q.state == "answered"
            && q.answer_delivery_id.is_none()
            && !native_answer_valid(&state, q)
        {
            state.questions[index].state = "expired".into();
            state.questions[index].answer_delivery_id = Some("control:revoked".into());
        }
    }
    crate::prompt_delivery::cancel_obsolete_on(tx, &state)?;
    state.revision = current + 1;
    tx.execute("INSERT INTO assistant_state(singleton,revision,body) VALUES(1,?1,?2) ON CONFLICT(singleton) DO UPDATE SET revision=excluded.revision,body=excluded.body",rusqlite::params![state.revision,serde_json::to_string(&state)?])?;
    Ok(state)
}

impl Store {
    pub fn assistant_state(&self) -> Result<AssistantState, StoreError> {
        let conn = self.conn.lock().unwrap();
        let body: Option<String> = conn
            .query_row(
                "SELECT body FROM assistant_state WHERE singleton=1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(body
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or_default())
    }

    pub fn save_assistant(
        &self,
        expected: u64,
        state: AssistantState,
    ) -> Result<AssistantState, StoreError> {
        self.save_assistant_with_receipt(expected, state, None)
    }
    fn save_assistant_with_receipt(
        &self,
        expected: u64,
        state: AssistantState,
        receipt: Option<(&str, &str, &str, &str)>,
    ) -> Result<AssistantState, StoreError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let state = save_assistant_in_tx(&tx, expected, state)?;
        if let Some((session, id, operation, response)) = receipt {
            tx.execute("INSERT INTO assistant_worker_receipts(session_id,command_id,operation,response) VALUES(?1,?2,?3,?4)",rusqlite::params![session,id,operation,response])?;
        }
        tx.commit()?;
        Ok(state)
    }

    fn edit_coordination(
        &self,
        expected: u64,
        edit: AssistantEdit,
    ) -> Result<AssistantState, StoreError> {
        let mut state = self.assistant_state()?;
        if state.revision != expected {
            return Err(invalid("assistant changed; refresh first"));
        }
        let target = edit_target(&state, &edit);
        match edit {
            AssistantEdit::Message {
                goal_id,
                content,
                queue,
            } => {
                add_message(
                    &mut state,
                    &goal_id,
                    content,
                    if queue { "queue" } else { "auto" },
                    "user",
                    None,
                )?;
            }
            AssistantEdit::Answer {
                question_id,
                answer,
            } => answer_question(&mut state, &question_id, answer, "user")?,
            AssistantEdit::RetryQuestion { question_id } => {
                let index = state
                    .questions
                    .iter()
                    .position(|q| q.id == question_id && q.state == "expired")
                    .ok_or_else(|| invalid("question is not expired"))?;
                let q = state.questions[index].clone();
                let (g, a) = latest_assignment(&state, &q.goal_id)?;
                if a.id != q.assignment_id
                    || state.goals[g].contract_revision != q.contract_revision
                {
                    return Err(invalid(
                        "question no longer belongs to current requirements",
                    ));
                }
                add_message(&mut state,&q.goal_id,format!("The original input request {} expired during recovery. Reopen this clarification with a fresh input id before continuing: {}. No answer or permission has been granted.",q.id,q.title),"queue","user",Some(q.id.clone()))?;
                state.questions[index].state = "recovery_requested".into();
                state.questions[index].answer_delivery_id =
                    Some("native:recovery_requested".into());
                state.goals[g].status = GoalStatus::Active;
            }
            AssistantEdit::Acknowledge { notification_id } => {
                let n = state
                    .notifications
                    .iter_mut()
                    .find(|n| n.id == notification_id)
                    .ok_or_else(|| invalid("unknown notification"))?;
                n.read = true;
            }
            AssistantEdit::Request {
                project_path,
                content,
            } => {
                bounded(&content, 16000)?;
                if !state
                    .settings
                    .as_ref()
                    .is_some_and(|s| s.projects.contains(&project_path))
                {
                    return Err(invalid("choose a managed project for this request"));
                }
                let mut managed_project_id = None;
                if let Some(h) = state.hierarchy.as_ref() {
                    if let Some(p) = h.projects.iter().find(|p| {
                        p.status == ProjectStatus::Active
                            && p.bindings.iter().any(|b| b.active && b.path == project_path)
                    }) {
                        managed_project_id = Some(p.id.clone());
                    }
                }
                state.requests.push(UserRequest {
                    created_at: chrono::Utc::now().to_rfc3339(),
                    id: uuid::Uuid::new_v4().to_string(),
                    project_path,
                    content,
                    handled: false,
                    turn_id: None,
                    managed_project_id,
                    parent_request_id: None,
                    route_revision: 0,
                });
            }
            AssistantEdit::Control { goal_id, operation } => {
                let sessions = if operation == "resume" {
                    self.list_sessions()?
                } else {
                    Vec::new()
                };
                control_goal(&mut state, &goal_id, &operation, &sessions)?;
            }
            AssistantEdit::Say { .. }
            | AssistantEdit::ConfirmMemory { .. }
            | AssistantEdit::RejectMemory { .. } => match edit {
                AssistantEdit::Say {
                    turn_id,
                    content,
                    project_paths,
                    answers_question,
                    actor,
                } => say(
                    &mut state,
                    turn_id,
                    content,
                    project_paths,
                    answers_question,
                    actor,
                )?,
                AssistantEdit::ConfirmMemory {
                    proposal_id,
                    content_hash,
                } => decide_memory(&mut state, &proposal_id, &content_hash, true)?,
                AssistantEdit::RejectMemory {
                    proposal_id,
                    content_hash,
                } => decide_memory(&mut state, &proposal_id, &content_hash, false)?,
                _ => unreachable!(),
            },
            AssistantEdit::Dependencies {
                goal_id,
                dependencies,
            } => {
                if dependencies.len() > 20
                    || dependencies
                        .iter()
                        .any(|d| d == &goal_id || !state.goals.iter().any(|g| &g.id == d))
                {
                    return Err(invalid("invalid dependencies"));
                }
                fn reaches(
                    goals: &[AssistantGoal],
                    from: &str,
                    to: &str,
                    seen: &mut Vec<String>,
                ) -> bool {
                    if from == to {
                        return true;
                    }
                    if seen.iter().any(|s| s == from) {
                        return false;
                    }
                    seen.push(from.into());
                    goals
                        .iter()
                        .find(|g| g.id == from)
                        .is_some_and(|g| g.dependencies.iter().any(|d| reaches(goals, d, to, seen)))
                }
                if dependencies
                    .iter()
                    .any(|d| reaches(&state.goals, d, &goal_id, &mut Vec::new()))
                {
                    return Err(invalid("cyclic goal dependency"));
                }
                let index = state
                    .goals
                    .iter()
                    .position(|g| g.id == goal_id)
                    .ok_or_else(|| invalid("unknown goal"))?;
                let goal = state.goals[index].clone();
                if goal.dependencies != dependencies {
                    revise_goal(
                        &mut state,
                        index,
                        goal.title,
                        goal.acceptance,
                        "User changed input dependencies".into(),
                        "user",
                        false,
                    )?;
                    state.goals[index].dependencies = dependencies;
                }
            }
            AssistantEdit::RememberChange { change_id } => {
                let c = state
                    .changes
                    .iter_mut()
                    .find(|c| c.id == change_id && !c.proposed)
                    .ok_or_else(|| invalid("unknown applied change"))?;
                c.remember = true;
            }
            AssistantEdit::ApplyChange {
                change_id,
                remember,
            } => {
                let c = state
                    .changes
                    .iter()
                    .find(|c| c.id == change_id && c.proposed && c.state == "proposed")
                    .cloned()
                    .ok_or_else(|| invalid("change is not pending"))?;
                let index = state
                    .goals
                    .iter()
                    .position(|g| g.id == c.goal_id)
                    .ok_or_else(|| invalid("unknown goal"))?;
                if state.goals[index].contract_revision != c.from_revision {
                    return Err(invalid("change is stale"));
                }
                revise_goal(
                    &mut state,
                    index,
                    c.title,
                    c.acceptance,
                    c.reason,
                    "user",
                    remember,
                )?;
                state
                    .changes
                    .iter_mut()
                    .find(|x| x.id == change_id)
                    .unwrap()
                    .state = "applied".into();
            }
            AssistantEdit::RejectChange { change_id } => {
                state
                    .changes
                    .iter_mut()
                    .find(|c| c.id == change_id && c.state == "proposed")
                    .ok_or_else(|| invalid("change is not pending"))?
                    .state = "rejected".into();
            }
            AssistantEdit::Review {
                goal_id,
                verdict,
                evidence,
            } => {
                bounded(&evidence, 8000)?;
                let (index, a) = latest_assignment(&state, &goal_id)?;
                let session = a
                    .session_id
                    .clone()
                    .ok_or_else(|| invalid("assignment has no session"))?;
                if verdict == "accepted" {
                    let result = a
                        .result
                        .clone()
                        .ok_or_else(|| invalid("no submitted deliverable"))?;
                    let session = self
                        .get_session(&session)?
                        .ok_or_else(|| invalid("session missing"))?;
                    accept_result(
                        &mut state,
                        index,
                        &session,
                        &self.list_sessions()?,
                        session.activity.revision,
                        evidence,
                        result.artifacts,
                    )?;
                } else if matches!(verdict.as_str(), "rework" | "unverifiable") {
                    let result = state.goals[index]
                        .assignments
                        .last_mut()
                        .unwrap()
                        .result
                        .as_mut()
                        .ok_or_else(|| invalid("no submitted deliverable"))?;
                    result.state = verdict.clone();
                    let artifacts = result.artifacts.clone();
                    record_review(&mut state, index, &verdict, &evidence, artifacts);
                    state.goals[index].verdict = None;
                    state.goals[index].status = if verdict == "rework" {
                        GoalStatus::Active
                    } else {
                        GoalStatus::NeedsAttention
                    };
                    state.goals[index].blocker = evidence.clone();
                    if verdict == "rework" {
                        add_message(
                            &mut state,
                            &goal_id,
                            format!("Review requires rework: {evidence}"),
                            "queue",
                            "user",
                            None,
                        )?;
                    }
                } else {
                    return Err(invalid("invalid review verdict"));
                }
            }
            _ => return Err(invalid("not a coordination edit")),
        }
        if let Some(goal_id) = target {
            bump_user_generation(&mut state, &goal_id);
        }
        state.observed.clear();
        self.save_assistant(expected, state)
    }

    pub fn edit_assistant(
        &self,
        expected: u64,
        edit: AssistantEdit,
    ) -> Result<AssistantState, StoreError> {
        if matches!(
            &edit,
            AssistantEdit::Source { .. }
                | AssistantEdit::SourceCapacity { .. }
                | AssistantEdit::SourceReset { .. }
        ) {
            return self.edit_source(expected, edit);
        }
        if let AssistantEdit::Say {
            turn_id,
            content,
            project_paths,
            answers_question,
            actor,
        } = &edit
        {
            // A lost response is retried with the same client id: replay, never duplicate.
            let current = self.assistant_state()?;
            if let Some(act) = actor.as_deref() {
                if let Some(h) = current.hierarchy.as_ref() {
                    if act != "chief" {
                        let pid = act
                            .strip_prefix("project:")
                            .ok_or_else(|| invalid("invalid actor format"))?;
                        h.project(pid)?;
                    }
                }
            }
            if let Some(previous) = current.turn_args.get(turn_id) {
                return if *previous
                    == turn_args_hash(
                        turn_id,
                        content,
                        project_paths,
                        answers_question.as_deref(),
                        actor.as_deref(),
                    )
                {
                    Ok(current)
                } else {
                    Err(invalid("turn id was already used with different content"))
                };
            }
        }
        if matches!(
            &edit,
            AssistantEdit::Message { .. }
                | AssistantEdit::Answer { .. }
                | AssistantEdit::RetryQuestion { .. }
                | AssistantEdit::Control { .. }
                | AssistantEdit::Acknowledge { .. }
                | AssistantEdit::Request { .. }
                | AssistantEdit::Dependencies { .. }
                | AssistantEdit::RememberChange { .. }
                | AssistantEdit::ApplyChange { .. }
                | AssistantEdit::RejectChange { .. }
                | AssistantEdit::Review { .. }
                | AssistantEdit::Say { .. }
                | AssistantEdit::ConfirmMemory { .. }
                | AssistantEdit::RejectMemory { .. }
        ) {
            return self.edit_coordination(expected, edit);
        }
        let mut state = self.assistant_state()?;
        if state.revision != expected {
            return Err(invalid("assistant changed; refresh first"));
        }
        let target = edit_target(&state, &edit);
        match edit {
            AssistantEdit::Settings { settings } => {
                if settings.projects.is_empty()
                    || settings.projects.len() > 30
                    || !(1..=4).contains(&settings.concurrency)
                    || !(1..=100).contains(&settings.turn_limit)
                    || !(1..=100).contains(&settings.dispatch_limit)
                {
                    return Err(invalid(
                        "select 1–30 projects, 1–4 concurrent goals and limits of 1–100",
                    ));
                }
                for path in &settings.projects {
                    if !self.project_exists(path)? {
                        return Err(invalid("unknown project"));
                    }
                }
                // Configuration changes invalidate pending decisions, but never cancel workers.
                state.settings = Some(settings);
                state.turns = 0;
                state.dispatches = 0;
                state.attention = None;
            }
            AssistantEdit::Goal {
                id,
                project_path,
                title,
                acceptance,
                priority,
            } => {
                bounded(&title, 4000)?;
                bounded(&acceptance, 8000)?;
                if priority > 3 || !self.project_exists(&project_path)? {
                    return Err(invalid("invalid project or priority"));
                }
                if let Some(id) = id {
                    let index = state
                        .goals
                        .iter()
                        .position(|g| g.id == id)
                        .ok_or_else(|| invalid("goal not found"))?;
                    if state.goals[index].project_path != project_path {
                        return Err(invalid("a goal cannot move projects"));
                    }
                    if state.goals[index].title != title
                        || state.goals[index].acceptance != acceptance
                    {
                        revise_goal(
                            &mut state,
                            index,
                            title,
                            acceptance,
                            "User edited the goal".into(),
                            "user",
                            false,
                        )?;
                    }
                    state.goals[index].priority = priority;
                } else {
                    if state.goals.len() >= 200 {
                        return Err(invalid("personal workspace limit: 200 goals"));
                    }
                    state.goals.push(AssistantGoal {
                        id: uuid::Uuid::new_v4().to_string(),
                        project_path,
                        title,
                        acceptance,
                        priority,
                        status: GoalStatus::Active,
                        next_step: String::new(),
                        blocker: String::new(),
                        assignments: vec![],
                        verdict: None,
                        contract_revision: 1,
                        dependencies: Vec::new(),
                    });
                }
            }
            AssistantEdit::Status { id, status } => {
                if status == GoalStatus::Completed {
                    return Err(invalid("completion needs versioned acceptance evidence"));
                }
                let goal = state
                    .goals
                    .iter_mut()
                    .find(|g| g.id == id)
                    .ok_or_else(|| invalid("goal not found"))?;
                goal.status = status;
                if status == GoalStatus::Active {
                    goal.blocker.clear();

                    for assignment in &mut goal.assignments {
                        assignment.taken_over = false;
                    }
                }
            }
            AssistantEdit::Takeover { id } => {
                let goal = state
                    .goals
                    .iter_mut()
                    .find(|g| g.id == id)
                    .ok_or_else(|| invalid("goal not found"))?;
                goal.status = GoalStatus::Paused;
                for assignment in &mut goal.assignments {
                    if assignment.session_id.is_none() {
                        assignment.session_id = self
                            .command_receipt("c2-assistant-create", &assignment.id)?
                            .map(|receipt| receipt.0);
                    }
                    assignment.taken_over = true;
                    assignment.owned = false;
                }
                // A cancelled intent without a creation receipt owns no execution slot.
                // Retain revoked intents too: creation may still return a receipt. A late
                // completion must attach to this identity without regranting control.
            }
            AssistantEdit::Wake => {
                state.attention = None;
                state.scoped_runs.retain(|r| r.state == "running");
                if let Some(id) = state.run.as_ref().and_then(|r| r.session_id.as_ref()) {
                    if self.get_session(id)?.is_some_and(|s| {
                        matches!(
                            s.activity.state,
                            SessionRunState::Idle | SessionRunState::Failed { .. }
                        )
                    }) {
                        state.run = None;
                    }
                }
            }
            _ => unreachable!("coordination edits routed above"),
        }
        if let Some(goal_id) = target {
            bump_user_generation(&mut state, &goal_id);
        }
        state.observed.clear();
        // Keep a running manager until it terminates; its old revision will discard the decision.
        self.save_assistant(expected, state)
    }

    pub fn assistant_snapshot(&self) -> Result<AssistantSnapshot, StoreError> {
        let state = self.assistant_state()?;
        let paths: Vec<&str> = state
            .settings
            .as_ref()
            .map(|s| s.projects.iter().map(String::as_str).collect())
            .unwrap_or_default();
        let sessions = self
            .list_sessions()?
            .into_iter()
            .filter(|s| paths.contains(&s.project_path.as_deref().unwrap_or(&s.cwd)))
            .collect();
        let mut memories = self.list_managed_memories(GLOBAL_MEMORY, 100)?;
        for path in paths {
            memories.extend(self.list_managed_memories(path, 100)?);
        }
        Ok(AssistantSnapshot {
            state,
            sessions,
            memories,
        })
    }
}

pub fn session_for<'a>(sessions: &'a [Session], id: &str) -> Result<&'a Session, StoreError> {
    sessions
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| invalid("session not in authorized projects"))
}
fn quiet(session: &Session) -> bool {
    matches!(
        session.activity.state,
        SessionRunState::Idle
            | SessionRunState::Failed {
                reason: RunFailureReason::ProviderError,
                ..
            }
    )
}

/// Proposals are untrusted. Validate the entire batch before writing any intent or changing a goal.
/// Initial managed prompts share the Engine's acceptance transaction. A delayed configuration
/// request cannot accept a prompt after its assignment was stopped, changed or taken over.
pub(crate) fn require_prompt_on(
    conn: &Connection,
    id: &str,
    session: &str,
) -> Result<(), StoreError> {
    let body: Option<String> = conn
        .query_row(
            "SELECT body FROM assistant_state WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let state: AssistantState = body
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_default();
    for goal in &state.goals {
        if let Some(a) = goal.assignments.iter().find(|a| a.id == id) {
            if goal.assignments.last().map(|a| a.id.as_str()) != Some(id)
                || a.session_id.as_deref() != Some(session)
                || !a.owned
                || a.taken_over
                || a.stop_requested
                || a.contract_revision != goal.contract_revision
                || goal.status != GoalStatus::Active
                || !state
                    .settings
                    .as_ref()
                    .is_some_and(|s| s.enabled && s.projects.contains(&goal.project_path))
            {
                return Err(invalid(
                    "initial prompt authority changed before Engine acceptance",
                ));
            }
        }
    }
    if let Some(r) = state.scoped_runs.iter().find(|r| r.run.id == id) {
        if r.state != "running"
            || r.run.session_id.as_deref() != Some(session)
            || !state.settings.as_ref().is_some_and(|s| s.enabled)
        {
            return Err(invalid("review run was revoked before Engine acceptance"));
        }
    }
    if state.attempts.contains_key(&format!("prompt:{id}"))
        && !state.scoped_runs.iter().any(|r| r.run.id == id)
        && !state
            .goals
            .iter()
            .any(|g| g.assignments.iter().any(|a| a.id == id))
    {
        return Err(invalid("managed prompt generation is no longer current"));
    }
    Ok(())
}

/// Same resource predicate for commit admission and waking a locally rejected review.
/// Paused/observed assignments still occupy capacity until their execution is quiet.
pub(crate) fn worker_capacity_available(state: &AssistantState, sessions: &[Session]) -> bool {
    let occupied = state
        .goals
        .iter()
        .filter(|g| {
            g.assignments.last().is_some_and(|a| {
                (a.owned || a.session_id.is_some())
                    && a.session_id
                        .as_ref()
                        .map(|id| session_for(sessions, id).map(|s| !quiet(s)).unwrap_or(true))
                        .unwrap_or(true)
            })
        })
        .count();
    state
        .settings
        .as_ref()
        .is_some_and(|s| occupied < s.concurrency)
}

pub fn apply_decision(
    state: &AssistantState,
    sessions: &[Session],
    decision: AssistantDecision,
) -> Result<AssistantState, StoreError> {
    let settings = state
        .settings
        .as_ref()
        .filter(|s| s.enabled)
        .ok_or_else(|| invalid("assistant is disabled"))?;
    bounded(&decision.summary, 12000)?;
    if decision.actions.len() > 8 {
        return Err(invalid("too many actions in one turn"));
    }
    let mut next = state.clone();
    next.summary = decision.summary;
    for action in decision.actions {
        match &action {
            AssistantAction::CreateGoal {
                request_id,
                title,
                acceptance,
                priority,
            } => {
                // The manager cannot dispatch a generated goal id in the same response.
                // Schedule the next bounded planning round without reacting to summary text.
                next.observed.clear();
                bounded(title, 4000)?;
                bounded(acceptance, 8000)?;
                let request = next
                    .requests
                    .iter_mut()
                    .find(|r| &r.id == request_id && !r.handled)
                    .ok_or_else(|| invalid("request is not pending"))?;
                if *priority > 3
                    || next.goals.len() >= 200
                    || !settings.projects.contains(&request.project_path)
                {
                    return Err(invalid("invalid request goal"));
                }
                request.handled = true;
                let turn_id = request.turn_id.clone();
                let goal_id = uuid::Uuid::new_v4().to_string();
                link_request_outcome(&mut next, turn_id.as_deref(), Some(&goal_id), None);
                let request = next
                    .requests
                    .iter()
                    .find(|r| &r.id == request_id)
                    .expect("request was just handled");
                let managed_project_id = request.managed_project_id.clone();
                let path = request.project_path.clone();
                next.goals.push(AssistantGoal {
                    id: goal_id.clone(),
                    project_path: path,
                    title: title.clone(),
                    acceptance: acceptance.clone(),
                    priority: *priority,
                    status: GoalStatus::Active,
                    next_step: String::new(),
                    blocker: String::new(),
                    assignments: Vec::new(),
                    verdict: None,
                    contract_revision: 1,
                    dependencies: Vec::new(),
                });
                if let Some(pid) = managed_project_id {
                    if next.hierarchy.is_some() {
                        let _ = assign_goal(&mut next, &goal_id, &pid, None);
                    }
                }
                continue;
            }
            AssistantAction::ClarifyRequest {
                request_id,
                title,
                options,
            } => {
                bounded(title, 4000)?;
                if options.len() > 8 || options.iter().any(|o| o.len() > 2000) {
                    return Err(invalid("invalid request question"));
                }
                if !next
                    .requests
                    .iter()
                    .any(|r| &r.id == request_id && !r.handled)
                {
                    return Err(invalid("request is not pending"));
                }
                if !next
                    .questions
                    .iter()
                    .any(|q| q.request_id.as_ref() == Some(request_id) && q.state == "open")
                {
                    let id = uuid::Uuid::new_v4().to_string();
                    let turn_id = next
                        .requests
                        .iter()
                        .find(|r| &r.id == request_id)
                        .and_then(|r| r.turn_id.clone());
                    link_request_outcome(&mut next, turn_id.as_deref(), None, Some(&id));
                    next.questions.push(CoordinationQuestion {
                        created_at: chrono::Utc::now().to_rfc3339(),
                        id: id.clone(),
                        goal_id: String::new(),
                        assignment_id: String::new(),
                        contract_revision: 0,
                        author: "assistant".into(),
                        title: title.clone(),
                        context: String::new(),
                        options: options.clone(),
                        blocking: true,
                        state: "open".into(),
                        answer: None,
                        answered_by: None,
                        source_input: None,
                        request_id: Some(request_id.clone()),
                        answer_delivery_id: None,
                        factual: false,
                    });
                    notify(
                        &mut next,
                        "",
                        "question",
                        &id,
                        title,
                        "Clarify the request",
                        None,
                    );
                }
                continue;
            }
            _ => {}
        }
        let id = match &action {
            AssistantAction::Dispatch { goal_id, .. }
            | AssistantAction::Link { goal_id, .. }
            | AssistantAction::Update { goal_id, .. }
            | AssistantAction::Accept { goal_id, .. }
            | AssistantAction::Communicate { goal_id, .. }
            | AssistantAction::Ask { goal_id, .. }
            | AssistantAction::Answer { goal_id, .. }
            | AssistantAction::ProposeChange { goal_id, .. } => goal_id,
            _ => unreachable!(),
        };
        let id = id.clone();
        let index = next
            .goals
            .iter()
            .position(|g| g.id == id)
            .ok_or_else(|| invalid("unknown goal"))?;
        if next.goals[index].status != GoalStatus::Active
            || !settings.projects.contains(&next.goals[index].project_path)
        {
            return Err(invalid("goal is paused or outside the authorized scope"));
        }
        match action {
            AssistantAction::Communicate { content, .. } => {
                add_message(
                    &mut next,
                    id.clone().as_str(),
                    content,
                    "auto",
                    "assistant",
                    None,
                )?;
            }
            AssistantAction::Ask {
                title,
                context,
                options,
                blocking,
                ..
            } => {
                open_question(
                    &mut next,
                    id.clone().as_str(),
                    title,
                    context,
                    options,
                    blocking,
                    "assistant",
                )?;
            }
            AssistantAction::Answer {
                question_id,
                answer,
                memory_ids,
                ..
            } => {
                if memory_ids.is_empty() || memory_ids.len() > 8 {
                    return Err(invalid("automatic answers need confirmed memory sources"));
                }
                // Source membership/content is checked by Runtime against the scoped recall used
                // for this round. The pure validator still fences the exact question and goal.
                let q = next
                    .questions
                    .iter()
                    .find(|q| q.id == question_id && q.goal_id == id)
                    .ok_or_else(|| invalid("question is not in this goal"))?;
                if !q.factual {
                    return Err(invalid("this question requires the user"));
                }
                if q.source_input
                    .as_ref()
                    .is_some_and(|p| p.kind == crate::session::PendingInputKind::Permission)
                {
                    return Err(invalid("permissions require the user"));
                }
                answer_question(&mut next, &question_id, answer, "assistant")?;
            }
            AssistantAction::ProposeChange {
                title,
                acceptance,
                reason,
                ..
            } => {
                bounded(&title, 4000)?;
                bounded(&acceptance, 8000)?;
                bounded(&reason, 4000)?;
                let goal = next.goals[index].clone();
                let cid = uuid::Uuid::new_v4().to_string();
                next.changes.push(RequirementChange {
                    created_at: chrono::Utc::now().to_rfc3339(),
                    id: cid.clone(),
                    goal_id: goal.id.clone(),
                    from_revision: goal.contract_revision,
                    to_revision: goal.contract_revision + 1,
                    before_title: goal.title.clone(),
                    before_acceptance: goal.acceptance.clone(),
                    title,
                    acceptance,
                    reason: reason.clone(),
                    state: "proposed".into(),
                    author: "assistant".into(),
                    delivery_id: None,
                    proposed: true,
                    memory_id: None,
                    remember: false,
                });
                notify(
                    &mut next,
                    &goal.id,
                    "proposal",
                    &cid,
                    "Requirement change needs a decision",
                    &reason,
                    goal.assignments.last().and_then(|a| a.session_id.clone()),
                );
            }
            AssistantAction::Dispatch { instruction, .. } => {
                bounded(&instruction, 16000)?;
                if next.questions.iter().any(|q| {
                    q.goal_id == id
                        && q.contract_revision == next.goals[index].contract_revision
                        && question_unresolved(q)
                }) || next.goals[index].dependencies.iter().any(|d| {
                    !next
                        .goals
                        .iter()
                        .any(|g| &g.id == d && g.status == GoalStatus::Completed)
                }) {
                    return Err(invalid("goal is waiting for a question or dependency"));
                }
                if next.dispatches >= settings.dispatch_limit {
                    return Err(invalid("dispatch allowance exhausted"));
                }
                if !worker_capacity_available(&next, sessions) {
                    return Err(StoreError::AssistantConcurrencyLimit);
                }
                let inputs = dependency_versions(&next, &next.goals[index])
                    .ok_or_else(|| invalid("unmet dependency"))?;
                let goal = &mut next.goals[index];
                if goal.assignments.last().is_some_and(|a| {
                    a.taken_over
                        || a.stop_requested
                        || a.session_id
                            .as_ref()
                            .map(|id| session_for(sessions, id).map(|s| !quiet(s)).unwrap_or(true))
                            .unwrap_or(a.owned)
                }) {
                    return Err(invalid("existing assignment still owns the goal"));
                }
                if goal.assignments.len() >= 20 {
                    return Err(invalid("goal retry limit reached"));
                }
                goal.assignments.push(Assignment {
                    id: uuid::Uuid::new_v4().to_string(),
                    instruction,
                    session_id: None,
                    submitted: false,
                    taken_over: false,
                    owned: true,
                    contract_revision: goal.contract_revision,
                    confirmed_revision: 0,
                    protocol: 1,
                    stop_requested: false,
                    stop_sent: false,
                    result: None,
                    inputs,
                    pending_inputs: None,
                    prior_results: vec![],
                });
                goal.verdict = None;
                next.dispatches += 1;
            }
            AssistantAction::Link { session_id, .. } => {
                let session = session_for(sessions, &session_id)?;
                if session.project_path.as_deref().unwrap_or(&session.cwd)
                    != next.goals[index].project_path
                {
                    return Err(invalid("session belongs to another project"));
                }
                if next.goals.iter().any(|g| {
                    g.assignments
                        .iter()
                        .any(|a| a.session_id.as_ref() == Some(&session_id))
                }) {
                    return Err(invalid("session already assigned"));
                }
                let goal = &mut next.goals[index];
                if !goal.assignments.is_empty() {
                    return Err(invalid("goal already has assignments"));
                }
                // Linking grants observation only. A later assignment must be explicitly resumed by the user.
                goal.assignments.push(Assignment {
                    id: uuid::Uuid::new_v4().to_string(),
                    instruction: "Observe existing work".into(),
                    session_id: Some(session_id),
                    submitted: true,
                    taken_over: true,
                    owned: false,
                    contract_revision: goal.contract_revision,
                    confirmed_revision: goal.contract_revision,
                    protocol: 0,
                    stop_requested: false,
                    stop_sent: false,
                    result: None,
                    inputs: Vec::new(),
                    pending_inputs: None,
                    prior_results: vec![],
                });
            }
            AssistantAction::Update {
                next_step, blocker, ..
            } => {
                if next_step.len() > 4000 || blocker.len() > 4000 {
                    return Err(invalid("update too long"));
                }
                next.goals[index].next_step = next_step;
                next.goals[index].blocker = blocker;
            }
            AssistantAction::Accept {
                session_id,
                activity_revision,
                evidence,
                artifacts,
                ..
            } => {
                let session = session_for(sessions, &session_id)?;
                accept_result(
                    &mut next,
                    index,
                    session,
                    sessions,
                    activity_revision,
                    evidence,
                    artifacts,
                )?;
            }
            _ => unreachable!(),
        }
    }
    Ok(next)
}

pub fn parse_decision(text: &str) -> Result<AssistantDecision, StoreError> {
    let text = text.trim();
    let json = if let Some(s) = text.strip_prefix("```json") {
        s.strip_suffix("```").unwrap_or(s).trim()
    } else {
        text
    };
    if json.len() > 64000 {
        return Err(invalid("decision exceeds size limit"));
    }
    Ok(serde_json::from_str(json)?)
}

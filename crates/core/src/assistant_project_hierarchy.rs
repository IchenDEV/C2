//! Business Project hierarchy: global manager -> Project manager -> executor.
//!
//! This is an optional extension of the single `AssistantState` document. Identity, workspace
//! bindings, versioned instructions, memory share references and goal owner epochs live here and
//! are saved by the same revision CAS as every other assistant fact. There is no second table.
//! When `hierarchy` is absent nothing is serialized, so legacy bytes and fingerprints are unchanged.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const HIERARCHY_SCHEMA: u32 = 1;
pub const MANAGED_PROJECT_MEMORY_PREFIX: &str = "codetwo://managed-project/";
const MAX_PROJECTS: usize = 100;
const MAX_BINDINGS: usize = 30;
const MAX_INSTRUCTION_REVISIONS: usize = 400;
const MAX_INSTRUCTION_PROPOSALS: usize = 200;
const MAX_SHARES: usize = 500;
const MAX_TEXT: usize = 8000;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
pub fn instruction_hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
/// Hash of the exact memory text plus the scope it was read from. A corrected memory changes it,
/// which silently invalidates every share reference to the old text.
pub fn memory_content_hash(source_scope: &str, content: &str) -> String {
    let body = serde_json::json!([source_scope, content]);
    format!("{:x}", Sha256::digest(body.to_string().as_bytes()))
}
pub fn project_memory_scope(project_id: &str) -> String {
    format!("{MANAGED_PROJECT_MEMORY_PREFIX}{project_id}")
}

/// Ordered by distance from the user: a reach includes every role at or above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Chief,
    ProjectManager,
    Executor,
}
pub type Reach = Role;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "project_id", rename_all = "snake_case")]
pub enum HierarchyScope {
    Global,
    Project(String),
}
fn scope_key(scope: &HierarchyScope) -> String {
    match scope {
        HierarchyScope::Global => "global".into(),
        HierarchyScope::Project(id) => format!("project:{id}"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Active,
    /// Stops new coordination only. Existing executions keep their resources and controls.
    Paused,
    /// Authority is revoked; unresolved writers keep a stop/attention state until released.
    Retired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceBinding {
    pub id: String,
    /// Normalized absolute path of a registered Store project (the execution workspace).
    pub path: String,
    pub version: u64,
    pub active: bool,
    pub source: String,
    #[serde(default)]
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedProject {
    /// Stable identity. Renaming never changes it; the path-based Store project is only a binding.
    pub id: String,
    pub name: String,
    pub status: ProjectStatus,
    pub version: u64,
    pub created_at: String,
    #[serde(default)]
    pub bindings: Vec<WorkspaceBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionRevision {
    pub scope: HierarchyScope,
    pub revision: u64,
    pub text: String,
    pub hash: String,
    pub reach: Reach,
    pub proposal_id: String,
    pub confirmed_by: String,
    pub confirmed_at: String,
    pub receipt: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HierarchyProposalState {
    Proposed,
    Confirmed,
    Rejected,
    Stale,
}

/// A model/manager can only create one of these. Exact text, hash and reach are what a human sees
/// and confirms; nothing becomes an active revision before that confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionProposal {
    pub id: String,
    pub scope: HierarchyScope,
    pub base_revision: u64,
    pub text: String,
    pub hash: String,
    pub reach: Reach,
    pub proposer: String,
    pub created_at: String,
    pub state: HierarchyProposalState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalOwner {
    pub project_id: String,
    pub binding_id: String,
    /// Fences stale commits: every ownership change bumps it.
    pub epoch: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareState {
    Proposed,
    Approved,
    Revoked,
}

/// An approved reference, never a copy: the memory stays in its source scope and is read through
/// this record only while its exact content hash still matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryShare {
    pub id: String,
    pub memory_id: String,
    pub source_scope: String,
    pub content_hash: String,
    pub target: HierarchyScope,
    pub reach: Reach,
    pub state: ShareState,
    pub proposer: String,
    pub created_at: String,
    #[serde(default)]
    pub decided_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectHierarchy {
    pub schema: u32,
    /// Bumped by every hierarchy change. Persisted guards reject a lower or unbumped change.
    pub revision: u64,
    #[serde(default)]
    pub projects: Vec<ManagedProject>,
    #[serde(default)]
    pub instructions: Vec<InstructionRevision>,
    #[serde(default)]
    pub proposals: Vec<InstructionProposal>,
    #[serde(default)]
    pub shares: Vec<MemoryShare>,
    /// goal id -> scoped manager owner. Legacy `AssistantGoal` literals stay untouched.
    #[serde(default)]
    pub goal_owners: BTreeMap<String, GoalOwner>,
}
impl Default for ProjectHierarchy {
    fn default() -> Self {
        Self {
            schema: HIERARCHY_SCHEMA,
            revision: 1,
            projects: vec![],
            instructions: vec![],
            proposals: vec![],
            shares: vec![],
            goal_owners: BTreeMap::new(),
        }
    }
}

impl ProjectHierarchy {
    pub fn project(&self, id: &str) -> Result<&ManagedProject, StoreError> {
        self.projects
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| invalid("unknown managed project"))
    }
    fn project_mut(&mut self, id: &str) -> Result<&mut ManagedProject, StoreError> {
        self.projects
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| invalid("unknown managed project"))
    }
    pub fn active_instruction(&self, scope: &HierarchyScope) -> Option<&InstructionRevision> {
        self.instructions
            .iter()
            .filter(|i| &i.scope == scope)
            .max_by_key(|i| i.revision)
    }
    pub(crate) fn usable_project(&self, id: &str) -> Result<&ManagedProject, StoreError> {
        let p = self.project(id)?;
        if p.status == ProjectStatus::Retired {
            return Err(invalid("managed project is retired"));
        }
        Ok(p)
    }
}

/// Host-created identity for a manager/executor call. It deliberately has no `Deserialize`
/// implementation and private fields: provider text can name an actor, but cannot become one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostActor {
    role: Role,
    project_id: Option<String>,
    goal_id: Option<String>,
}
impl HostActor {
    pub fn chief() -> Self {
        Self {
            role: Role::Chief,
            project_id: None,
            goal_id: None,
        }
    }
    pub fn project(state: &AssistantState, project_id: &str) -> Result<Self, StoreError> {
        let h = enabled(state)?;
        h.usable_project(project_id)?;
        Ok(Self {
            role: Role::ProjectManager,
            project_id: Some(project_id.into()),
            goal_id: None,
        })
    }
    /// Executor identity is derived from the goal's recorded owner, never from request text.
    pub fn executor(state: &AssistantState, goal_id: &str) -> Result<Self, StoreError> {
        let h = enabled(state)?;
        let owner = h
            .goal_owners
            .get(goal_id)
            .ok_or_else(|| invalid("goal has no managed project owner"))?;
        h.usable_project(&owner.project_id)?;
        Ok(Self {
            role: Role::Executor,
            project_id: Some(owner.project_id.clone()),
            goal_id: Some(goal_id.into()),
        })
    }
    pub fn role(&self) -> Role {
        self.role
    }
    pub fn id(&self) -> String {
        match (&self.role, &self.project_id) {
            (Role::Chief, _) => "chief".into(),
            (Role::ProjectManager, Some(p)) => format!("project:{p}"),
            (_, Some(p)) => format!("executor:{p}"),
            _ => "unknown".into(),
        }
    }
}

fn enabled(state: &AssistantState) -> Result<&ProjectHierarchy, StoreError> {
    state
        .hierarchy
        .as_ref()
        .ok_or_else(|| invalid("project hierarchy is not enabled"))
}
fn enabled_mut(state: &mut AssistantState) -> Result<&mut ProjectHierarchy, StoreError> {
    state
        .hierarchy
        .as_mut()
        .ok_or_else(|| invalid("project hierarchy is not enabled"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagerAction {
    RouteProject,
    Query,
    Report,
    ProposeCrossProject,
    ProposeInstruction,
    ProposeShare,
    CreateGoal,
    Dispatch,
    Communicate,
    Ask,
    Accept,
}

/// Core-enforced action allow-list. `claimed_epoch` is the owner epoch the caller claimed work
/// under; a changed owner invalidates it.
pub fn authorize(
    state: &AssistantState,
    actor: &HostActor,
    action: ManagerAction,
    goal_id: Option<&str>,
    claimed_epoch: Option<u64>,
) -> Result<(), StoreError> {
    use ManagerAction::*;
    let h = enabled(state)?;
    match actor.role {
        Role::Executor => Err(invalid("executors have no coordination authority")),
        Role::Chief => match action {
            RouteProject | Query | Report | ProposeCrossProject | ProposeInstruction
            | ProposeShare => Ok(()),
            _ => Err(invalid(
                "global manager may only route, query, report or propose",
            )),
        },
        Role::ProjectManager => {
            let pid = actor
                .project_id
                .as_deref()
                .ok_or_else(|| invalid("missing project identity"))?;
            let project = h.usable_project(pid)?;
            match action {
                Query | Report | ProposeInstruction | ProposeShare => Ok(()),
                RouteProject | ProposeCrossProject => {
                    Err(invalid("project manager cannot route across projects"))
                }
                CreateGoal => {
                    if project.status != ProjectStatus::Active
                        || !project.bindings.iter().any(|b| b.active)
                    {
                        return Err(invalid("project is paused or has no active workspace"));
                    }
                    Ok(())
                }
                Dispatch | Communicate | Ask | Accept => {
                    if project.status != ProjectStatus::Active {
                        return Err(invalid("project is paused"));
                    }
                    let goal = goal_id.ok_or_else(|| invalid("goal is required"))?;
                    let owner = h
                        .goal_owners
                        .get(goal)
                        .filter(|o| o.project_id == pid)
                        .ok_or_else(|| invalid("goal belongs to another project"))?;
                    if !project
                        .bindings
                        .iter()
                        .any(|b| b.id == owner.binding_id && b.active)
                    {
                        return Err(invalid("workspace binding is revoked"));
                    }
                    if claimed_epoch.is_some_and(|e| e != owner.epoch) {
                        return Err(invalid("stale owner epoch"));
                    }
                    Ok(())
                }
            }
        }
    }
}

/// A writer is unresolved while its prompt may be running or in flight. Revocation, control and
/// ownership changes must keep it visible until the host has verified a terminal receipt.
fn creation_unresolved<'a>(
    state: &'a AssistantState,
    assignment: &Assignment,
) -> Option<&'a AssistantAttempt> {
    state
        .attempts
        .get(&format!("create:{}", assignment.id))
        .filter(|attempt| !matches!(attempt.state.as_str(), "failed" | "cancelled"))
}

pub fn writer_unresolved(state: &AssistantState, goal: &AssistantGoal) -> bool {
    let Some(assignment) = goal.assignments.last() else {
        return false;
    };
    if creation_unresolved(state, assignment).is_some() {
        return true;
    }
    goal.status != GoalStatus::Completed
        && assignment.result.is_none()
        && (assignment.owned || assignment.submitted || assignment.session_id.is_some())
}

pub fn normalize_path(path: &str) -> Result<String, StoreError> {
    let p = path.trim();
    if !p.starts_with('/') || p.contains('\0') || p.len() > 1024 {
        return Err(invalid("workspace path must be absolute"));
    }
    let mut parts = Vec::new();
    for c in p.split('/') {
        match c {
            "" | "." => {}
            ".." => return Err(invalid("workspace path must not contain '..'")),
            other => parts.push(other),
        }
    }
    Ok(format!("/{}", parts.join("/")))
}

pub fn enable_hierarchy(state: &mut AssistantState) -> Result<(), StoreError> {
    if state.hierarchy.is_none() {
        state.hierarchy = Some(ProjectHierarchy::default());
    }
    Ok(())
}

pub fn create_project(state: &mut AssistantState, name: &str) -> Result<String, StoreError> {
    bounded(name, 200)?;
    let h = enabled_mut(state)?;
    if h.projects.len() >= MAX_PROJECTS {
        return Err(invalid("managed project limit reached"));
    }
    let name = name.trim();
    if h.projects
        .iter()
        .any(|p| p.status != ProjectStatus::Retired && p.name.eq_ignore_ascii_case(name))
    {
        return Err(invalid("a managed project already uses this name"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    h.projects.push(ManagedProject {
        id: id.clone(),
        name: name.into(),
        status: ProjectStatus::Active,
        version: 1,
        created_at: now(),
        bindings: vec![],
    });
    h.revision += 1;
    Ok(id)
}

pub fn rename_project(
    state: &mut AssistantState,
    project_id: &str,
    name: &str,
) -> Result<(), StoreError> {
    bounded(name, 200)?;
    let h = enabled_mut(state)?;
    h.usable_project(project_id)?;
    let project = h.project_mut(project_id)?;
    project.name = name.trim().into();
    project.version += 1;
    h.revision += 1;
    Ok(())
}

/// Pause/resume only. Pausing stops new coordination, never an existing execution.
pub fn set_project_paused(
    state: &mut AssistantState,
    project_id: &str,
    paused: bool,
) -> Result<(), StoreError> {
    let h = enabled_mut(state)?;
    h.usable_project(project_id)?;
    let project = h.project_mut(project_id)?;
    project.status = if paused {
        ProjectStatus::Paused
    } else {
        ProjectStatus::Active
    };
    project.version += 1;
    h.revision += 1;
    Ok(())
}

pub fn bind_workspace(
    state: &mut AssistantState,
    project_id: &str,
    path: &str,
) -> Result<String, StoreError> {
    let path = normalize_path(path)?;
    let h = enabled_mut(state)?;
    h.usable_project(project_id)?;
    let project = h.project_mut(project_id)?;
    if project.bindings.len() >= MAX_BINDINGS {
        return Err(invalid("workspace binding limit reached"));
    }
    if project.bindings.iter().any(|b| b.active && b.path == path) {
        return Err(invalid("workspace is already bound to this project"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    project.bindings.push(WorkspaceBinding {
        id: id.clone(),
        path,
        version: 1,
        active: true,
        source: "user".into(),
        revoked_at: None,
    });
    project.version += 1;
    h.revision += 1;
    Ok(id)
}

/// Make a goal unreachable for new coordination and keep any unresolved writer controllable.
fn halt_goal(state: &mut AssistantState, goal_id: &str, reason: &str, revision: u64) {
    let Some(index) = state.goals.iter().position(|g| g.id == goal_id) else {
        return;
    };
    if state.goals[index].status == GoalStatus::Completed {
        return;
    }
    let unresolved = writer_unresolved(state, &state.goals[index]);
    {
        let goal = &mut state.goals[index];
        if goal.status != GoalStatus::Cancelled {
            goal.status = GoalStatus::NeedsAttention;
        }
        goal.blocker = reason.into();
        if unresolved {
            if let Some(a) = goal.assignments.last_mut() {
                a.stop_requested = true;
            }
        }
    }
    for m in &mut state.messages {
        if m.goal_id == goal_id && m.delivery_id.is_none() {
            m.state = "cancelled".into();
        }
    }
    notify(
        state,
        goal_id,
        "attention",
        &format!("hierarchy:{goal_id}:{revision}"),
        "Project authority changed",
        reason,
        None,
    );
}

/// Revoke one binding. In the same commit new analysis/dispatch is denied, affected goals become
/// NeedsAttention and an unresolved writer gets `stop_requested` but is never dropped or replaced.
pub fn revoke_binding(
    state: &mut AssistantState,
    project_id: &str,
    binding_id: &str,
) -> Result<(), StoreError> {
    let affected: Vec<String> = enabled(state)?
        .goal_owners
        .iter()
        .filter(|(_, o)| o.project_id == project_id && o.binding_id == binding_id)
        .map(|(g, _)| g.clone())
        .collect();
    let revision = {
        let h = enabled_mut(state)?;
        let project = h.project_mut(project_id)?;
        let binding = project
            .bindings
            .iter_mut()
            .find(|b| b.id == binding_id)
            .ok_or_else(|| invalid("unknown workspace binding"))?;
        if !binding.active {
            return Err(invalid("workspace binding is already revoked"));
        }
        binding.active = false;
        binding.version += 1;
        binding.revoked_at = Some(now());
        project.version += 1;
        h.revision += 1;
        h.revision
    };
    for goal in affected {
        halt_goal(state, &goal, "workspace binding revoked", revision);
    }
    Ok(())
}

/// Retire a project: all bindings are revoked and every owned goal is halted in one commit.
pub fn retire_project(state: &mut AssistantState, project_id: &str) -> Result<(), StoreError> {
    let affected: Vec<String> = enabled(state)?
        .goal_owners
        .iter()
        .filter(|(_, o)| o.project_id == project_id)
        .map(|(g, _)| g.clone())
        .collect();
    let revision = {
        let h = enabled_mut(state)?;
        h.usable_project(project_id)?;
        let project = h.project_mut(project_id)?;
        for b in project.bindings.iter_mut().filter(|b| b.active) {
            b.active = false;
            b.version += 1;
            b.revoked_at = Some(now());
        }
        project.status = ProjectStatus::Retired;
        project.version += 1;
        h.revision += 1;
        h.revision
    };
    for goal in affected {
        halt_goal(state, &goal, "managed project retired", revision);
    }
    Ok(())
}

/// Projects whose active bindings cover the goal's workspace. More than one means: ask the user.
pub fn candidate_projects(state: &AssistantState, goal_id: &str) -> Vec<String> {
    let (Some(h), Some(goal)) = (
        state.hierarchy.as_ref(),
        state.goals.iter().find(|g| g.id == goal_id),
    ) else {
        return vec![];
    };
    h.projects
        .iter()
        .filter(|p| {
            p.status == ProjectStatus::Active
                && p.bindings
                    .iter()
                    .any(|b| b.active && b.path == goal.project_path)
        })
        .map(|p| p.id.clone())
        .collect()
}

fn choose_binding(
    h: &ProjectHierarchy,
    project_id: &str,
    binding_id: Option<&str>,
    goal_path: &str,
) -> Result<String, StoreError> {
    let project = h.usable_project(project_id)?;
    if project.status != ProjectStatus::Active {
        return Err(invalid("project is paused"));
    }
    let candidates: Vec<&WorkspaceBinding> = project
        .bindings
        .iter()
        .filter(|b| b.active && b.path == goal_path)
        .collect();
    match binding_id {
        Some(id) => candidates
            .iter()
            .find(|b| b.id == id)
            .map(|b| b.id.clone())
            .ok_or_else(|| invalid("binding is not an active workspace of this goal")),
        None if candidates.len() == 1 => Ok(candidates[0].id.clone()),
        None => Err(invalid("goal workspace is not bound to this project")),
    }
}

/// First ownership of a goal. One goal has exactly one managed project and one workspace.
pub fn assign_goal(
    state: &mut AssistantState,
    goal_id: &str,
    project_id: &str,
    binding_id: Option<&str>,
) -> Result<(), StoreError> {
    let goal = state
        .goals
        .iter()
        .find(|g| g.id == goal_id)
        .ok_or_else(|| invalid("unknown goal"))?;
    if writer_unresolved(state, goal) {
        return Err(invalid(
            "an active legacy writer cannot be mapped without a verified terminal receipt",
        ));
    }
    let path = goal.project_path.clone();
    let h = enabled_mut(state)?;
    if h.goal_owners.contains_key(goal_id) {
        return Err(invalid("goal already has an owner; transfer it explicitly"));
    }
    let binding = choose_binding(h, project_id, binding_id, &path)?;
    h.goal_owners.insert(
        goal_id.into(),
        GoalOwner {
            project_id: project_id.into(),
            binding_id: binding,
            epoch: 1,
        },
    );
    h.revision += 1;
    Ok(())
}

/// Owner change. An unresolved writer cannot be transferred. M2 must connect the existing
/// persisted Engine terminal/stop receipt before a writer can be released; a UI edit or Idle
/// snapshot is not that evidence. Epoch change fences old commits.
pub fn transfer_goal(
    state: &mut AssistantState,
    goal_id: &str,
    project_id: &str,
    binding_id: Option<&str>,
) -> Result<u64, StoreError> {
    let goal = state
        .goals
        .iter()
        .find(|g| g.id == goal_id)
        .ok_or_else(|| invalid("unknown goal"))?;
    if writer_unresolved(state, goal) {
        return Err(invalid(
            "stop the active writer and confirm its release before changing the owner",
        ));
    }
    let path = goal.project_path.clone();
    let h = enabled_mut(state)?;
    let epoch = h
        .goal_owners
        .get(goal_id)
        .ok_or_else(|| invalid("goal has no owner to transfer"))?
        .epoch
        + 1;
    let binding = choose_binding(h, project_id, binding_id, &path)?;
    h.goal_owners.insert(
        goal_id.into(),
        GoalOwner {
            project_id: project_id.into(),
            binding_id: binding,
            epoch,
        },
    );
    h.revision += 1;
    Ok(epoch)
}

/// Reject a commit carrying a stale owner epoch.
pub fn check_owner_epoch(
    state: &AssistantState,
    goal_id: &str,
    epoch: u64,
) -> Result<(), StoreError> {
    match enabled(state)?.goal_owners.get(goal_id) {
        Some(o) if o.epoch == epoch => Ok(()),
        _ => Err(invalid("stale owner epoch")),
    }
}

fn check_scope(h: &ProjectHierarchy, scope: &HierarchyScope) -> Result<(), StoreError> {
    if let HierarchyScope::Project(id) = scope {
        h.usable_project(id)?;
    }
    Ok(())
}

/// Create an instruction proposal. It is inert until `confirm_instruction`.
pub fn propose_instruction(
    state: &mut AssistantState,
    proposer: &str,
    scope: HierarchyScope,
    text: &str,
    reach: Reach,
) -> Result<String, StoreError> {
    bounded(text, MAX_TEXT)?;
    let h = enabled_mut(state)?;
    check_scope(h, &scope)?;
    if matches!(scope, HierarchyScope::Project(_)) && reach == Role::Chief {
        return Err(invalid(
            "project instructions must reach at least the project manager",
        ));
    }
    if h.proposals.len() >= MAX_INSTRUCTION_PROPOSALS {
        let old = h
            .proposals
            .iter()
            .position(|p| p.state != HierarchyProposalState::Proposed)
            .ok_or_else(|| invalid("too many pending instruction proposals"))?;
        h.proposals.remove(old);
    }
    let id = uuid::Uuid::new_v4().to_string();
    let base = h.active_instruction(&scope).map_or(0, |i| i.revision);
    h.proposals.push(InstructionProposal {
        id: id.clone(),
        scope,
        base_revision: base,
        hash: instruction_hash(text),
        text: text.into(),
        reach,
        proposer: proposer.into(),
        created_at: now(),
        state: HierarchyProposalState::Proposed,
    });
    h.revision += 1;
    Ok(id)
}

fn proposal_index(
    h: &ProjectHierarchy,
    proposal_id: &str,
    text_hash: &str,
) -> Result<usize, StoreError> {
    let index = h
        .proposals
        .iter()
        .position(|p| p.id == proposal_id)
        .ok_or_else(|| invalid("unknown instruction proposal"))?;
    let p = &h.proposals[index];
    if p.state != HierarchyProposalState::Proposed {
        return Err(invalid("instruction proposal is already decided"));
    }
    if p.hash != text_hash {
        return Err(invalid(
            "instruction text changed; confirm the exact current text",
        ));
    }
    Ok(index)
}

/// Explicit human confirmation. Returns the new active revision of the scope.
pub fn confirm_instruction(
    state: &mut AssistantState,
    proposal_id: &str,
    text_hash: &str,
) -> Result<u64, StoreError> {
    let h = enabled_mut(state)?;
    let index = proposal_index(h, proposal_id, text_hash)?;
    let proposal = h.proposals[index].clone();
    check_scope(h, &proposal.scope)?;
    let current = h
        .active_instruction(&proposal.scope)
        .map_or(0, |i| i.revision);
    if current != proposal.base_revision {
        h.proposals[index].state = HierarchyProposalState::Stale;
        h.revision += 1;
        return Err(invalid(
            "instruction changed since the proposal; propose again",
        ));
    }
    if h.instructions.len() >= MAX_INSTRUCTION_REVISIONS {
        return Err(invalid("instruction history limit reached"));
    }
    let revision = current + 1;
    h.instructions.push(InstructionRevision {
        scope: proposal.scope.clone(),
        revision,
        text: proposal.text,
        hash: proposal.hash,
        reach: proposal.reach,
        proposal_id: proposal.id,
        confirmed_by: "user".into(),
        confirmed_at: now(),
        receipt: uuid::Uuid::new_v4().to_string(),
    });
    h.proposals[index].state = HierarchyProposalState::Confirmed;
    for p in h
        .proposals
        .iter_mut()
        .filter(|p| p.scope == proposal.scope && p.state == HierarchyProposalState::Proposed)
    {
        p.state = HierarchyProposalState::Stale;
    }
    h.revision += 1;
    Ok(revision)
}

pub fn reject_instruction(
    state: &mut AssistantState,
    proposal_id: &str,
    text_hash: &str,
) -> Result<(), StoreError> {
    let h = enabled_mut(state)?;
    let index = proposal_index(h, proposal_id, text_hash)?;
    h.proposals[index].state = HierarchyProposalState::Rejected;
    h.revision += 1;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EffectiveInstruction {
    pub scope: HierarchyScope,
    pub revision: u64,
    pub hash: String,
    pub text: String,
}

/// Confirmed documents only, global first and the actor's project second. Lower documents never
/// widen what a higher one forbids because platform/host rules stay outside this composition.
pub fn effective_instructions(
    state: &AssistantState,
    actor: &HostActor,
) -> Vec<EffectiveInstruction> {
    let Some(h) = state.hierarchy.as_ref() else {
        return vec![];
    };
    let mut scopes = vec![HierarchyScope::Global];
    if let Some(id) = actor.project_id.as_ref() {
        if h.usable_project(id).is_err() {
            return vec![];
        }
        scopes.push(HierarchyScope::Project(id.clone()));
    }
    scopes
        .iter()
        .filter_map(|s| h.active_instruction(s))
        .filter(|i| actor.role <= i.reach)
        .map(|i| EffectiveInstruction {
            scope: i.scope.clone(),
            revision: i.revision,
            hash: i.hash.clone(),
            text: i.text.clone(),
        })
        .collect()
}

// ---- memory scope resolution -------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShareGrant {
    pub share_id: String,
    pub memory_id: String,
    pub content_hash: String,
    pub source_scope: String,
}

/// Everything an actor may read, as derived at one moment. `scopes` are whole Store memory scopes;
/// `shares` are exact, hash-bound references into other scopes. Anything not listed is denied.
///
/// A grant is a sealed, inert receipt: every field is private, it has no `Default` or
/// `Deserialize`, and only [`memory_grant`] builds one from a host-created [`HostActor`]. It is
/// never an authority by itself. `Store::scoped_memory_read` ignores the listed edges and
/// re-derives them from the current authoritative `AssistantState` through [`current_memory_grant`],
/// so a revoked share, a retired project, a moved binding or a changed goal owner takes effect on
/// the next read even for a grant that was cached before the change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemoryGrant {
    actor: String,
    #[serde(skip)]
    identity: HostActor,
    /// `(project_id, binding_id)` of the goal owner an executor grant was derived from.
    #[serde(skip)]
    owner: Option<(String, String, u64)>,
    scopes: Vec<String>,
    shares: Vec<ShareGrant>,
    /// Destination scopes whose own inject policy must also allow an incoming share.
    target_scopes: Vec<String>,
}

impl MemoryGrant {
    pub fn actor(&self) -> &str {
        &self.actor
    }
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }
    pub fn shares(&self) -> &[ShareGrant] {
        &self.shares
    }
    /// Destination scopes for this actor; a deny on any of them blocks all context injection.
    pub(crate) fn target_scopes(&self) -> &[String] {
        &self.target_scopes
    }
}

pub fn memory_grant(state: &AssistantState, actor: &HostActor) -> Result<MemoryGrant, StoreError> {
    let h = enabled(state)?;
    let mut grant = MemoryGrant {
        actor: actor.id(),
        identity: actor.clone(),
        owner: None,
        scopes: vec![],
        shares: vec![],
        target_scopes: vec![],
    };
    let target = match actor.role {
        Role::Chief => {
            grant.scopes.push(GLOBAL_MEMORY.into());
            grant.target_scopes.push(GLOBAL_MEMORY.into());
            HierarchyScope::Global
        }
        Role::ProjectManager => {
            let pid = actor
                .project_id
                .as_deref()
                .ok_or_else(|| invalid("missing project identity"))?;
            let project = h.usable_project(pid)?;
            grant.scopes.push(project_memory_scope(pid));
            grant.target_scopes.push(project_memory_scope(pid));
            for b in project.bindings.iter().filter(|b| b.active) {
                grant.scopes.push(b.path.clone());
            }
            HierarchyScope::Project(pid.into())
        }
        Role::Executor => {
            let goal_id = actor
                .goal_id
                .as_deref()
                .ok_or_else(|| invalid("missing goal identity"))?;
            let owner = h
                .goal_owners
                .get(goal_id)
                .ok_or_else(|| invalid("goal has no managed project owner"))?;
            let project = h.usable_project(&owner.project_id)?;
            if actor.project_id.as_deref() != Some(owner.project_id.as_str()) {
                return Err(invalid(
                    "executor identity no longer matches the goal owner",
                ));
            }
            grant.owner = Some((
                owner.project_id.clone(),
                owner.binding_id.clone(),
                owner.epoch,
            ));
            grant
                .target_scopes
                .push(project_memory_scope(&owner.project_id));
            if let Some(b) = project
                .bindings
                .iter()
                .find(|b| b.id == owner.binding_id && b.active)
            {
                grant.scopes.push(b.path.clone());
                grant.target_scopes.push(b.path.clone());
            }
            HierarchyScope::Project(owner.project_id.clone())
        }
    };
    for s in h
        .shares
        .iter()
        .filter(|s| s.state == ShareState::Approved && s.target == target && actor.role <= s.reach)
        .filter(|s| source_still_usable(h, &s.source_scope))
    {
        grant.shares.push(ShareGrant {
            share_id: s.id.clone(),
            memory_id: s.memory_id.clone(),
            content_hash: s.content_hash.clone(),
            source_scope: s.source_scope.clone(),
        });
    }
    grant.scopes.sort();
    grant.scopes.dedup();
    grant.target_scopes.sort();
    grant.target_scopes.dedup();
    Ok(grant)
}

/// A share out of a managed project's own memory scope dies with that project's authority.
fn source_still_usable(h: &ProjectHierarchy, source_scope: &str) -> bool {
    match source_scope.strip_prefix(MANAGED_PROJECT_MEMORY_PREFIX) {
        Some(project_id) => h.usable_project(project_id).is_ok(),
        None => true,
    }
}

/// Re-derive a grant from the current authoritative state. Only the sealed identity of the old
/// grant is used; its edges are discarded. An executor grant is additionally fenced to the owner
/// binding it was derived from, so a reassigned goal never inherits the old reader. Changes to
/// unrelated projects do not touch the result, so an equivalent grant stays equivalent.
pub(crate) fn current_memory_grant(
    state: &AssistantState,
    grant: &MemoryGrant,
) -> Result<MemoryGrant, StoreError> {
    let identity = &grant.identity;
    let actor = match identity.role {
        Role::Chief => HostActor::chief(),
        Role::ProjectManager => HostActor::project(
            state,
            identity
                .project_id
                .as_deref()
                .ok_or_else(|| invalid("missing project identity"))?,
        )?,
        Role::Executor => HostActor::executor(
            state,
            identity
                .goal_id
                .as_deref()
                .ok_or_else(|| invalid("missing goal identity"))?,
        )?,
    };
    if actor != *identity {
        return Err(invalid("memory grant identity is no longer current"));
    }
    let current = memory_grant(state, &actor)?;
    if current.owner != grant.owner {
        return Err(invalid("memory grant owner is no longer current"));
    }
    Ok(current)
}

/// Create a share proposal for exact memory text. Only `confirm_share` makes it readable.
pub fn propose_share(
    state: &mut AssistantState,
    proposer: &str,
    memory_id: &str,
    source_scope: &str,
    content: &str,
    target: HierarchyScope,
    reach: Reach,
) -> Result<String, StoreError> {
    let h = enabled_mut(state)?;
    check_scope(h, &target)?;
    if reach == Role::Chief && target != HierarchyScope::Global {
        return Err(invalid(
            "a project share must reach at least the project manager",
        ));
    }
    if h.shares.len() >= MAX_SHARES {
        return Err(invalid("memory share limit reached"));
    }
    let hash = memory_content_hash(source_scope, content);
    if h.shares.iter().any(|s| {
        s.memory_id == memory_id
            && s.content_hash == hash
            && s.target == target
            && s.state != ShareState::Revoked
    }) {
        return Err(invalid(
            "this exact memory is already shared with that target",
        ));
    }
    let id = uuid::Uuid::new_v4().to_string();
    h.shares.push(MemoryShare {
        id: id.clone(),
        memory_id: memory_id.into(),
        source_scope: source_scope.into(),
        content_hash: hash,
        target,
        reach,
        state: ShareState::Proposed,
        proposer: proposer.into(),
        created_at: now(),
        decided_at: None,
    });
    h.revision += 1;
    Ok(id)
}

pub fn confirm_share(
    state: &mut AssistantState,
    share_id: &str,
    content_hash: &str,
) -> Result<(), StoreError> {
    let h = enabled_mut(state)?;
    let share = h
        .shares
        .iter_mut()
        .find(|s| s.id == share_id)
        .ok_or_else(|| invalid("unknown memory share"))?;
    if share.state != ShareState::Proposed || share.content_hash != content_hash {
        return Err(invalid("share is decided or its exact text changed"));
    }
    share.state = ShareState::Approved;
    share.decided_at = Some(now());
    h.revision += 1;
    Ok(())
}

pub fn revoke_share(state: &mut AssistantState, share_id: &str) -> Result<(), StoreError> {
    let h = enabled_mut(state)?;
    let share = h
        .shares
        .iter_mut()
        .find(|s| s.id == share_id)
        .ok_or_else(|| invalid("unknown memory share"))?;
    if share.state == ShareState::Revoked {
        return Err(invalid("memory share is already revoked"));
    }
    share.state = ShareState::Revoked;
    share.decided_at = Some(now());
    h.revision += 1;
    Ok(())
}

// ---- invariants and persisted guard ------------------------------------------------------

pub fn validate_hierarchy(h: &ProjectHierarchy, state: &AssistantState) -> Result<(), StoreError> {
    if h.schema == 0 || h.schema > HIERARCHY_SCHEMA || h.revision == 0 {
        return Err(invalid("unsupported project hierarchy schema"));
    }
    if h.projects.len() > MAX_PROJECTS
        || h.instructions.len() > MAX_INSTRUCTION_REVISIONS
        || h.proposals.len() > MAX_INSTRUCTION_PROPOSALS
        || h.shares.len() > MAX_SHARES
    {
        return Err(invalid("project hierarchy limit reached"));
    }
    let mut project_ids = BTreeSet::new();
    let mut binding_ids = BTreeSet::new();
    for p in &h.projects {
        bounded(&p.name, 200)?;
        if uuid::Uuid::parse_str(&p.id).is_err() || !project_ids.insert(p.id.as_str()) {
            return Err(invalid("managed project id is invalid or duplicated"));
        }
        if p.bindings.len() > MAX_BINDINGS {
            return Err(invalid("workspace binding limit reached"));
        }
        let mut active_paths = BTreeSet::new();
        for b in &p.bindings {
            if !binding_ids.insert(b.id.as_str()) || normalize_path(&b.path)? != b.path {
                return Err(invalid("workspace binding is invalid or duplicated"));
            }
            if b.active && !active_paths.insert(b.path.as_str()) {
                return Err(invalid("workspace is bound twice in one project"));
            }
        }
        if p.status == ProjectStatus::Retired && p.bindings.iter().any(|b| b.active) {
            return Err(invalid("retired project keeps an active binding"));
        }
    }
    for (goal_id, owner) in &h.goal_owners {
        let goal = state
            .goals
            .iter()
            .find(|g| &g.id == goal_id)
            .ok_or_else(|| invalid("goal owner refers to an unknown goal"))?;
        let binding = h
            .project(&owner.project_id)?
            .bindings
            .iter()
            .find(|b| b.id == owner.binding_id)
            .ok_or_else(|| invalid("goal owner refers to an unknown binding"))?;
        if binding.path != goal.project_path || owner.epoch == 0 {
            return Err(invalid("goal owner does not match the goal workspace"));
        }
    }
    let mut last: BTreeMap<String, u64> = BTreeMap::new();
    for i in &h.instructions {
        if let HierarchyScope::Project(id) = &i.scope {
            h.project(id)?;
        }
        let key = scope_key(&i.scope);
        let expected = last.get(&key).copied().unwrap_or(0) + 1;
        if i.revision != expected || i.hash != instruction_hash(&i.text) || i.text.len() > MAX_TEXT
        {
            return Err(invalid(
                "instruction revisions are not consecutive or hashed",
            ));
        }
        last.insert(key, expected);
    }
    let mut proposal_ids = BTreeSet::new();
    for p in &h.proposals {
        if !proposal_ids.insert(p.id.as_str()) || p.hash != instruction_hash(&p.text) {
            return Err(invalid("instruction proposal is invalid or duplicated"));
        }
    }
    let mut share_ids = BTreeSet::new();
    for s in &h.shares {
        if !share_ids.insert(s.id.as_str()) {
            return Err(invalid("memory share is duplicated"));
        }
    }
    Ok(())
}

/// Persisted writer guard, called by the one CAS save path inside its transaction. It rejects:
/// * a document without hierarchy over one with it (stale/downgrade writer);
/// * a lower hierarchy revision or schema, or a changed hierarchy with an unbumped revision;
/// * a schema this binary does not understand;
/// * any change which removes/alters the owner of a goal whose writer is unresolved, or erases
///   that writer's assignment.
pub(crate) fn guard_save(
    tx: &rusqlite::Transaction,
    state: &AssistantState,
) -> Result<(), StoreError> {
    let body: Option<String> = tx
        .query_row(
            "SELECT body FROM assistant_state WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let current: Option<AssistantState> = body.map(|b| serde_json::from_str(&b)).transpose()?;
    let current_h = current.as_ref().and_then(|c| c.hierarchy.as_ref());
    if let Some(cur) = current_h {
        if cur.schema > HIERARCHY_SCHEMA {
            return Err(invalid("stored hierarchy schema is newer than this writer"));
        }
        let new = state
            .hierarchy
            .as_ref()
            .ok_or_else(|| invalid("stale writer would drop the project hierarchy"))?;
        if new.schema < cur.schema || new.revision < cur.revision {
            return Err(invalid("project hierarchy downgrade rejected"));
        }
        if new.revision == cur.revision && new != cur {
            return Err(invalid("project hierarchy changed without a new revision"));
        }
        let before = current.as_ref().unwrap();
        for (goal_id, owner) in &cur.goal_owners {
            let Some(goal) = before.goals.iter().find(|g| &g.id == goal_id) else {
                continue;
            };
            if !writer_unresolved(before, goal) {
                continue;
            }
            let kept_owner = new.goal_owners.get(goal_id).is_some_and(|o| {
                o.project_id == owner.project_id
                    && o.binding_id == owner.binding_id
                    && o.epoch == owner.epoch
            });
            let previous = goal.assignments.last().unwrap();
            let kept_writer = state
                .goals
                .iter()
                .find(|g| &g.id == goal_id)
                .is_some_and(|g| {
                    g.project_path == goal.project_path
                        && g.status != GoalStatus::Completed
                        && g.assignments.len() == goal.assignments.len()
                        && g.assignments.last().is_some_and(|a| {
                            a.id == previous.id
                                && a.session_id == previous.session_id
                                && a.submitted == previous.submitted
                                && a.result.is_none()
                        })
                });
            let kept_attempt = creation_unresolved(before, previous).is_none_or(|attempt| {
                state
                    .attempts
                    .get(&format!("create:{}", previous.id))
                    .is_some_and(|next| {
                        next.state == attempt.state && next.outcome == attempt.outcome
                    })
            });
            if !kept_owner || !kept_writer || !kept_attempt {
                return Err(invalid(
                    "an unresolved writer must keep its identity and owner until a verified terminal receipt; hierarchy release is not connected yet",
                ));
            }
        }
    }
    if let Some(new) = state.hierarchy.as_ref() {
        validate_hierarchy(new, state)?;
    }
    Ok(())
}

pub(crate) const HIERARCHY_GUARD_SQL: &str = "
CREATE TRIGGER IF NOT EXISTS assistant_hierarchy_no_downgrade BEFORE UPDATE ON assistant_state
WHEN json_extract(OLD.body,'$.hierarchy') IS NOT NULL AND (
    json_extract(NEW.body,'$.hierarchy') IS NULL
    OR json_extract(NEW.body,'$.hierarchy.revision') < json_extract(OLD.body,'$.hierarchy.revision')
    OR json_extract(NEW.body,'$.hierarchy.schema') < json_extract(OLD.body,'$.hierarchy.schema'))
BEGIN SELECT RAISE(ABORT,'assistant hierarchy downgrade rejected'); END;
CREATE TRIGGER IF NOT EXISTS assistant_hierarchy_no_delete BEFORE DELETE ON assistant_state
WHEN json_extract(OLD.body,'$.hierarchy') IS NOT NULL
BEGIN SELECT RAISE(ABORT,'assistant hierarchy delete rejected'); END;";

// ---- Store entry points ------------------------------------------------------------------

/// User-origin edits (host UI/CLI). Manager/model origin has no variant which confirms,
/// binds, revokes or transfers; it can only use `Store::propose_*`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HierarchyEdit {
    Enable {},
    CreateProject {
        name: String,
    },
    RenameProject {
        project_id: String,
        name: String,
    },
    PauseProject {
        project_id: String,
    },
    ResumeProject {
        project_id: String,
    },
    RetireProject {
        project_id: String,
    },
    BindWorkspace {
        project_id: String,
        path: String,
    },
    RevokeBinding {
        project_id: String,
        binding_id: String,
    },
    AssignGoal {
        goal_id: String,
        project_id: String,
        #[serde(default)]
        binding_id: Option<String>,
    },
    TransferGoal {
        goal_id: String,
        project_id: String,
        #[serde(default)]
        binding_id: Option<String>,
    },
    /// Human authored text: proposal and confirmation in one explicit user action.
    SetInstruction {
        scope: HierarchyScope,
        text: String,
        reach: Reach,
    },
    ConfirmInstruction {
        proposal_id: String,
        text_hash: String,
    },
    RejectInstruction {
        proposal_id: String,
        text_hash: String,
    },
    ConfirmShare {
        share_id: String,
        content_hash: String,
    },
    RevokeShare {
        share_id: String,
    },
}

impl Store {
    fn hierarchy_cas(
        &self,
        expected: u64,
        apply: impl FnOnce(&mut AssistantState) -> Result<(), StoreError>,
    ) -> Result<AssistantState, StoreError> {
        let mut state = self.assistant_state()?;
        if state.revision != expected {
            return Err(invalid("assistant changed; refresh first"));
        }
        apply(&mut state)?;
        self.save_assistant(expected, state)
    }

    pub fn edit_hierarchy(
        &self,
        expected: u64,
        edit: HierarchyEdit,
    ) -> Result<AssistantState, StoreError> {
        // Registered-directory check happens outside the document closure.
        if let HierarchyEdit::BindWorkspace { path, .. } = &edit {
            if !self.project_exists(&normalize_path(path)?)? {
                return Err(invalid("workspace is not a registered project"));
            }
        }
        self.hierarchy_cas(expected, |state| {
            match edit {
                HierarchyEdit::Enable {} => enable_hierarchy(state)?,
                HierarchyEdit::CreateProject { name } => {
                    create_project(state, &name)?;
                }
                HierarchyEdit::RenameProject { project_id, name } => {
                    rename_project(state, &project_id, &name)?
                }
                HierarchyEdit::PauseProject { project_id } => {
                    set_project_paused(state, &project_id, true)?
                }
                HierarchyEdit::ResumeProject { project_id } => {
                    set_project_paused(state, &project_id, false)?
                }
                HierarchyEdit::RetireProject { project_id } => retire_project(state, &project_id)?,
                HierarchyEdit::BindWorkspace { project_id, path } => {
                    bind_workspace(state, &project_id, &path)?;
                }
                HierarchyEdit::RevokeBinding {
                    project_id,
                    binding_id,
                } => revoke_binding(state, &project_id, &binding_id)?,
                HierarchyEdit::AssignGoal {
                    goal_id,
                    project_id,
                    binding_id,
                } => assign_goal(state, &goal_id, &project_id, binding_id.as_deref())?,
                HierarchyEdit::TransferGoal {
                    goal_id,
                    project_id,
                    binding_id,
                } => {
                    transfer_goal(state, &goal_id, &project_id, binding_id.as_deref())?;
                }
                HierarchyEdit::SetInstruction { scope, text, reach } => {
                    let id = propose_instruction(state, "user", scope, &text, reach)?;
                    let hash = instruction_hash(&text);
                    confirm_instruction(state, &id, &hash)?;
                }
                HierarchyEdit::ConfirmInstruction {
                    proposal_id,
                    text_hash,
                } => {
                    confirm_instruction(state, &proposal_id, &text_hash)?;
                }
                HierarchyEdit::RejectInstruction {
                    proposal_id,
                    text_hash,
                } => reject_instruction(state, &proposal_id, &text_hash)?,
                HierarchyEdit::ConfirmShare {
                    share_id,
                    content_hash,
                } => confirm_share(state, &share_id, &content_hash)?,
                HierarchyEdit::RevokeShare { share_id } => revoke_share(state, &share_id)?,
            }
            Ok(())
        })
    }

    /// Manager-origin proposal. The result is inert until a user confirms the exact hash.
    pub fn propose_instruction_as(
        &self,
        expected: u64,
        actor: &HostActor,
        scope: HierarchyScope,
        text: &str,
        reach: Reach,
    ) -> Result<(AssistantState, String), StoreError> {
        let mut proposal = String::new();
        let saved = self.hierarchy_cas(expected, |state| {
            authorize(state, actor, ManagerAction::ProposeInstruction, None, None)?;
            match (&actor.role, &scope) {
                (Role::Chief, HierarchyScope::Global) => {}
                (Role::ProjectManager, HierarchyScope::Project(p))
                    if actor.project_id.as_deref() == Some(p.as_str()) => {}
                _ => {
                    return Err(invalid(
                        "instruction scope is outside the actor's authority",
                    ))
                }
            }
            proposal = propose_instruction(state, &actor.id(), scope, text, reach)?;
            Ok(())
        })?;
        Ok((saved, proposal))
    }

    /// Manager-origin share proposal for one readable memory. Needs `confirm_share` to take effect.
    pub fn propose_memory_share_as(
        &self,
        expected: u64,
        actor: &HostActor,
        memory_id: &str,
        target: HierarchyScope,
        reach: Reach,
    ) -> Result<(AssistantState, String), StoreError> {
        let record = self
            .memory_by_id(memory_id)?
            .filter(|m| m.active && m.conflict_with_id.is_none())
            .ok_or_else(|| invalid("memory is not available for sharing"))?;
        let mut proposal = String::new();
        let saved = self.hierarchy_cas(expected, |state| {
            authorize(state, actor, ManagerAction::ProposeShare, None, None)?;
            if !memory_grant(state, actor)?
                .scopes()
                .contains(&record.project_path)
            {
                return Err(invalid("memory is outside the actor's readable scopes"));
            }
            proposal = propose_share(
                state,
                &actor.id(),
                &record.id,
                &record.project_path,
                &record.content,
                target,
                reach,
            )?;
            Ok(())
        })?;
        Ok((saved, proposal))
    }

    /// Hash a user must be shown (and echo back) to confirm sharing this exact memory text.
    pub fn memory_share_hash(&self, memory_id: &str) -> Result<String, StoreError> {
        let record = self
            .memory_by_id(memory_id)?
            .ok_or_else(|| invalid("unknown memory"))?;
        Ok(memory_content_hash(&record.project_path, &record.content))
    }
}

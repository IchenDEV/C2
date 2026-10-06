//! Persistent chief-of-staff conversation: intake records, bound actions and memory candidates.
//! The coordination document stays the single owner; execution facts remain in goals, receipts
//! and the Engine. Turns only record message handling and refer to those objects by id.
use super::*;
use sha2::{Digest, Sha256};

pub const MAX_CONVERSATION: usize = 400;
pub const MAX_OUTSTANDING_TURNS: usize = 20;
pub const MAX_PROPOSALS: usize = 200;
pub const STALE_INTAKE: &str = "stale intake decision";
const MEMORY_CATEGORIES: [&str; 6] = [
    "constraint",
    "preference",
    "fact",
    "relationship",
    "event",
    "episode",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnAuthor {
    User,
    Assistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    Recorded,
    Reviewing,
    Handled,
    Failed,
    Unknown,
}

/// Frontend contract: `state.conversation[]`. Status describes message handling only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTurn {
    pub id: String,
    #[serde(default)]
    pub created_at: String,
    pub author: TurnAuthor,
    pub content: String,
    #[serde(default)]
    pub reply_to: Option<String>,
    #[serde(default)]
    pub project_paths: Vec<String>,
    pub status: TurnStatus,
    #[serde(default)]
    pub goal_ids: Vec<String>,
    #[serde(default)]
    pub question_ids: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub source_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalState {
    Proposed,
    Confirmed,
    Rejected,
    Failed,
}

/// Frontend contract: `state.memory_proposals[]`. A candidate is not memory until confirmed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryProposal {
    pub id: String,
    pub turn_id: String,
    pub project_path: String,
    pub category: String,
    pub content: String,
    pub content_hash: String,
    pub state: ProposalState,
    #[serde(default)]
    pub memory_id: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// What an intake run saw for one goal. Commit compares only the goals an action touches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalStamp {
    pub goal_id: String,
    pub contract_revision: u64,
    pub status: GoalStatus,
    pub priority: u8,
    pub user_gen: u64,
    pub assignment: Option<String>,
    pub controlled: bool,
    pub stop_requested: bool,
}

pub fn goal_stamp(state: &AssistantState, goal: &AssistantGoal) -> GoalStamp {
    let a = goal.assignments.last();
    GoalStamp {
        goal_id: goal.id.clone(),
        contract_revision: goal.contract_revision,
        status: goal.status,
        priority: goal.priority,
        user_gen: state.user_gen.get(&goal.id).copied().unwrap_or(0),
        assignment: a.map(|a| a.id.clone()),
        controlled: a.is_some_and(|a| a.owned && !a.taken_over),
        stop_requested: a.is_some_and(|a| a.stop_requested),
    }
}

/// Authorized goals visible to one turn, in stable order.
pub fn turn_goals<'a>(
    state: &'a AssistantState,
    turn: &ConversationTurn,
) -> Vec<&'a AssistantGoal> {
    state
        .goals
        .iter()
        .filter(|g| allowed_project(state, turn, &g.project_path))
        .collect()
}

pub fn turn_stamps(state: &AssistantState, turn: &ConversationTurn) -> Vec<GoalStamp> {
    turn_goals(state, turn)
        .into_iter()
        .map(|g| goal_stamp(state, g))
        .collect()
}

fn allowed_project(state: &AssistantState, turn: &ConversationTurn, path: &str) -> bool {
    turn.project_paths.iter().any(|p| p == path)
        && state
            .settings
            .as_ref()
            .is_some_and(|s| s.projects.iter().any(|p| p == path))
}

pub fn bump_user_generation(state: &mut AssistantState, goal_id: &str) {
    if !goal_id.is_empty() {
        *state.user_gen.entry(goal_id.into()).or_insert(0) += 1;
    }
}

/// Goal whose user-controlled version a manual edit moves. Absent for settings/ack edits.
pub(crate) fn edit_target(state: &AssistantState, edit: &AssistantEdit) -> Option<String> {
    use AssistantEdit::*;
    match edit {
        Message { goal_id, .. }
        | Control { goal_id, .. }
        | Dependencies { goal_id, .. }
        | Review { goal_id, .. } => Some(goal_id.clone()),
        Status { id, .. } | Takeover { id } => Some(id.clone()),
        Goal { id: Some(id), .. } => Some(id.clone()),
        ApplyChange { change_id, .. }
        | RejectChange { change_id }
        | RememberChange { change_id } => state
            .changes
            .iter()
            .find(|c| &c.id == change_id)
            .map(|c| c.goal_id.clone()),
        Answer { question_id, .. } | RetryQuestion { question_id } => state
            .questions
            .iter()
            .find(|q| &q.id == question_id && !q.goal_id.is_empty())
            .map(|q| q.goal_id.clone()),
        _ => None,
    }
}

pub fn turn_args_hash(
    turn_id: &str,
    content: &str,
    project_paths: &[String],
    answers_question: Option<&str>,
    actor: Option<&str>,
) -> String {
    let body = match actor {
        Some(a) => serde_json::json!([turn_id, content, project_paths, answers_question, a]),
        None => serde_json::json!([turn_id, content, project_paths, answers_question]),
    };
    format!("{:x}", Sha256::digest(body.to_string().as_bytes()))
}

pub fn args_hash(
    turn_id: &str,
    content: &str,
    project_paths: &[String],
    answers_question: Option<&str>,
) -> String {
    turn_args_hash(turn_id, content, project_paths, answers_question, None)
}

pub fn memory_hash(project_path: &str, category: &str, content: &str) -> String {
    let body = serde_json::json!([project_path, category, content]);
    format!("{:x}", Sha256::digest(body.to_string().as_bytes()))
}

fn clean_turn_id(id: &str) -> Result<(), StoreError> {
    if id.trim().is_empty()
        || id.len() > 128
        || id.starts_with("reply:")
        || id.chars().any(char::is_whitespace)
    {
        return Err(invalid("turn id must be a stable client id without spaces"));
    }
    Ok(())
}

/// Record one user message. The caller has already handled same-id replays.
pub(crate) fn say(
    state: &mut AssistantState,
    turn_id: String,
    content: String,
    hint: Vec<String>,
    answers_question: Option<String>,
    actor: Option<String>,
) -> Result<(), StoreError> {
    clean_turn_id(&turn_id)?;
    bounded(&content, 16000)?;
    if hint.len() > 30 {
        return Err(invalid("too many project hints"));
    }
    let hash = turn_args_hash(&turn_id, &content, &hint, answers_question.as_deref(), actor.as_deref());
    let authorized = state
        .settings
        .as_ref()
        .map(|s| s.projects.clone())
        .filter(|p| !p.is_empty())
        .ok_or_else(|| invalid("select projects before talking to the assistant"))?;
    // Resolve once. Later settings changes can only narrow this record; never widen it.
    let paths = if hint.is_empty() {
        authorized
    } else {
        for p in &hint {
            if !authorized.contains(p) {
                return Err(invalid("project hint is outside the authorized projects"));
            }
        }
        let mut unique = Vec::new();
        for p in hint {
            if !unique.contains(&p) {
                unique.push(p);
            }
        }
        unique
    };
    let outstanding = state
        .conversation
        .iter()
        .filter(|t| {
            t.author == TurnAuthor::User
                && matches!(t.status, TurnStatus::Recorded | TurnStatus::Reviewing)
        })
        .count();
    if outstanding >= MAX_OUTSTANDING_TURNS
        || state.conversation.len() + 2 * (outstanding + 1) > MAX_CONVERSATION
    {
        return Err(invalid(
            "conversation limit reached; unhandled messages are kept",
        ));
    }
    let mut turn = ConversationTurn {
        id: turn_id.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        author: TurnAuthor::User,
        content: content.clone(),
        reply_to: None,
        project_paths: paths,
        status: TurnStatus::Recorded,
        goal_ids: vec![],
        question_ids: vec![],
        error: None,
        source_run_id: None,
        actor,
    };
    if let Some(question_id) = answers_question {
        // The reference is an explicit user decision; the original answer path validates it.
        let q = state
            .questions
            .iter()
            .find(|q| q.id == question_id)
            .ok_or_else(|| invalid("unknown question"))?;
        if q.source_input
            .as_ref()
            .is_some_and(|p| p.kind == crate::session::PendingInputKind::Permission)
        {
            return Err(invalid("choose a concrete permission option or cancel"));
        }
        let goal_id = q.goal_id.clone();
        answer_question(state, &question_id, content, "user")?;
        bump_user_generation(state, &goal_id);
        if !goal_id.is_empty() {
            turn.goal_ids.push(goal_id);
        }
        turn.question_ids.push(question_id);
        turn.status = TurnStatus::Handled;
    }
    state.turn_args.insert(turn_id, hash);
    state.conversation.push(turn);
    Ok(())
}

pub(crate) fn decide_memory(
    state: &mut AssistantState,
    proposal_id: &str,
    content_hash: &str,
    confirm: bool,
) -> Result<(), StoreError> {
    let key = format!("memory:{proposal_id}");
    let attempt = state.attempts.get(&key).cloned();
    let p = state
        .memory_proposals
        .iter_mut()
        .find(|p| p.id == proposal_id)
        .ok_or_else(|| invalid("unknown memory proposal"))?;
    // Repeating the same decision is harmless; a different decision never rewrites history.
    if p.content_hash != content_hash
        || memory_hash(&p.project_path, &p.category, &p.content) != p.content_hash
    {
        return Err(invalid("memory proposal changed; review the exact content"));
    }
    let mut reset_attempt = false;
    match (p.state, confirm) {
        (ProposalState::Proposed, true) => p.state = ProposalState::Confirmed,
        (ProposalState::Proposed, false) => p.state = ProposalState::Rejected,
        (ProposalState::Failed, true) => {
            // Only a known failure (the Memory Store returned an error, so nothing was written)
            // may be retried. An unknown outcome (attempt started, no result) could already be
            // stored and is never rewritten; the user checks the memory list instead.
            match attempt.as_ref().map(|a| a.state.as_str()) {
                None | Some("failed") => {
                    p.state = ProposalState::Confirmed;
                    p.error = None;
                    reset_attempt = attempt.is_some();
                }
                _ => return Err(invalid(
                    "memory write outcome is unknown; check the memory list instead of rewriting",
                )),
            }
        }
        (ProposalState::Confirmed, true) | (ProposalState::Rejected, false) => {}
        _ => return Err(invalid("memory proposal was already decided")),
    }
    if reset_attempt {
        state.attempts.remove(&key);
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeDecision {
    /// The natural-language reply recorded against the original message.
    pub summary: String,
    #[serde(default)]
    pub actions: Vec<IntakeAction>,
}

/// Actions are bound to the original message by `quote`: explicit user words which must appear in
/// that message. The model cannot name another message or object to widen authority.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum IntakeAction {
    Route {
        project_path: String,
        content: String,
    },
    SetPriority {
        goal_id: String,
        priority: u8,
        quote: String,
    },
    Control {
        goal_id: String,
        /// stop | cancel | pause_followup
        operation: String,
        quote: String,
    },
    Message {
        goal_id: String,
        content: String,
        quote: String,
    },
    Change {
        goal_id: String,
        title: String,
        acceptance: String,
        reason: String,
        quote: String,
    },
    ProposeMemory {
        project_path: String,
        category: String,
        content: String,
    },
}

pub fn parse_intake(text: &str) -> Result<IntakeDecision, StoreError> {
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

fn acknowledgement_only(text: &str) -> bool {
    let words: String = text
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    matches!(
        words.as_str(),
        "好" | "好的"
            | "可以"
            | "确认"
            | "同意"
            | "收到"
            | "行"
            | "ok"
            | "okay"
            | "yes"
            | "sure"
            | "approved"
            | "confirm"
    )
}

fn grounded(turn: &ConversationTurn, quote: &str) -> Result<(), StoreError> {
    let q = quote.trim();
    if q.chars().count() < 2 || !turn.content.contains(q) || acknowledgement_only(q) {
        return Err(invalid(
            "action needs an explicit quote from the original user message",
        ));
    }
    Ok(())
}

fn fresh<'a>(
    state: &'a AssistantState,
    turn: &ConversationTurn,
    stamps: &[GoalStamp],
    goal_id: &str,
) -> Result<usize, StoreError> {
    let index = state
        .goals
        .iter()
        .position(|g| g.id == goal_id)
        .ok_or_else(|| invalid("unknown goal"))?;
    let goal = &state.goals[index];
    if !allowed_project(state, turn, &goal.project_path) {
        return Err(invalid("goal is outside the message's authorized projects"));
    }
    // User order: a later message which already decided this goal wins over this earlier one,
    // however late this run started or captured its stamps.
    let position = state.conversation.iter().position(|t| t.id == turn.id);
    let superseded = position.is_some_and(|at| {
        state.conversation[at + 1..].iter().any(|t| {
            t.author == TurnAuthor::User
                && t.status == TurnStatus::Handled
                && t.goal_ids.iter().any(|g| g == goal_id)
        })
    });
    if superseded {
        return Err(invalid(&format!(
            "{STALE_INTAKE}: a later user message already decided this goal"
        )));
    }
    match stamps.iter().find(|s| s.goal_id == goal_id) {
        Some(seen) if *seen == goal_stamp(state, goal) => Ok(index),
        _ => Err(invalid(&format!(
            "{STALE_INTAKE}: goal changed after the message was interpreted"
        ))),
    }
}

/// Validate and commit an intake decision against the current document. The result is a complete
/// replacement candidate; the caller still saves it with the revision CAS.
pub fn apply_intake(
    state: &AssistantState,
    sessions: &[Session],
    run_id: &str,
    decision: IntakeDecision,
) -> Result<AssistantState, StoreError> {
    let settings = state
        .settings
        .as_ref()
        .filter(|s| s.enabled)
        .ok_or_else(|| invalid("assistant is disabled"))?;
    let turn_index = state
        .conversation
        .iter()
        .position(|t| {
            t.author == TurnAuthor::User
                && t.status == TurnStatus::Reviewing
                && t.source_run_id.as_deref() == Some(run_id)
        })
        .ok_or_else(|| invalid("message is no longer owned by this run"))?;
    let turn = state.conversation[turn_index].clone();
    bounded(&decision.summary, 12000)?;
    if decision.actions.len() > 8 {
        return Err(invalid("too many actions in one message"));
    }
    if acknowledgement_only(&turn.content) && !decision.actions.is_empty() {
        return Err(invalid(
            "an acknowledgement needs clarification before changing work",
        ));
    }
    // A revision supersedes old steering. Require its complete instructions in the change
    // rather than silently cancelling a supplement emitted by this same interpretation.
    for action in &decision.actions {
        if let IntakeAction::Change { goal_id, .. } = action {
            if decision
                .actions
                .iter()
                .any(|a| matches!(a, IntakeAction::Message { goal_id: id, .. } if id == goal_id))
            {
                return Err(invalid("include supplements in the direction change, not a separate message for the same goal"));
            }
        }
    }
    let stamps = state.intake_stamps.get(run_id).cloned().unwrap_or_default();
    let mut next = state.clone();
    let mut touched: Vec<String> = Vec::new();
    for action in decision.actions {
        match action {
            IntakeAction::Route {
                project_path,
                content,
            } => {
                bounded(&content, 16000)?;
                if !allowed_project(&next, &turn, &project_path)
                    || !settings.projects.contains(&project_path)
                {
                    return Err(invalid("route target is outside the authorized projects"));
                }
                if next.requests.len() >= 200 {
                    return Err(invalid("coordination history limit reached"));
                }
                let mut managed_project_id = None;
                if let Some(h) = next.hierarchy.as_ref() {
                    if let Some(p) = h.projects.iter().find(|p| {
                        p.status == ProjectStatus::Active
                            && p.bindings.iter().any(|b| b.active && b.path == project_path)
                    }) {
                        managed_project_id = Some(p.id.clone());
                    }
                }
                if next.requests.iter().any(|r| {
                    (r.turn_id.as_deref() == Some(&turn.id) && r.project_path == project_path)
                        || (managed_project_id.is_some()
                            && r.turn_id.as_deref() == Some(&turn.id)
                            && r.managed_project_id == managed_project_id)
                }) {
                    return Err(invalid(
                        "this message already has a request for that project",
                    ));
                }
                next.requests.push(UserRequest {
                    created_at: chrono::Utc::now().to_rfc3339(),
                    id: uuid::Uuid::new_v4().to_string(),
                    project_path,
                    content,
                    handled: false,
                    turn_id: Some(turn.id.clone()),
                    managed_project_id,
                    parent_request_id: Some(turn.id.clone()),
                    route_revision: 0,
                });
            }
            IntakeAction::SetPriority {
                goal_id,
                priority,
                quote,
            } => {
                grounded(&turn, &quote)?;
                let i = fresh(state, &turn, &stamps, &goal_id)?;
                if priority > 3 {
                    return Err(invalid("invalid priority"));
                }
                next.goals[i].priority = priority;
                touched.push(goal_id);
            }
            IntakeAction::Control {
                goal_id,
                operation,
                quote,
            } => {
                grounded(&turn, &quote)?;
                let i = fresh(state, &turn, &stamps, &goal_id)?;
                match operation.as_str() {
                    "stop" | "cancel" if next.goals[i].assignments.is_empty() => {
                        // Work which never started owns no execution slot; only status moves.
                        if matches!(
                            next.goals[i].status,
                            GoalStatus::Cancelled | GoalStatus::Completed
                        ) {
                            return Err(invalid("goal is already finished"));
                        }
                        next.goals[i].status = if operation == "cancel" {
                            GoalStatus::Cancelled
                        } else {
                            GoalStatus::Paused
                        };
                    }
                    "stop" | "cancel" => {
                        control_goal(&mut next, &goal_id, &operation, sessions)?;
                    }
                    "pause_followup" => {
                        // Stop issuing follow-ups; the running worker keeps its control and result.
                        if next.goals[i].status != GoalStatus::Active {
                            return Err(invalid("only an active goal can pause follow-up"));
                        }
                        next.goals[i].status = GoalStatus::Paused;
                    }
                    _ => return Err(invalid("unknown task control")),
                }
                touched.push(goal_id);
            }
            IntakeAction::Message {
                goal_id,
                content,
                quote,
            } => {
                grounded(&turn, &quote)?;
                fresh(state, &turn, &stamps, &goal_id)?;
                if let Some(h) = next.hierarchy.as_ref() {
                    if let Some(owner) = h.goal_owners.get(&goal_id) {
                        let is_chief = turn.actor.as_deref().unwrap_or("chief") == "chief";
                        if is_chief {
                            return Err(invalid("global manager may not message or revise a project-managed goal directly"));
                        }
                        if let Some(act) = turn.actor.as_deref() {
                            if let Some(pid) = act.strip_prefix("project:") {
                                if pid != owner.project_id {
                                    return Err(invalid("project manager cannot message goals of another project"));
                                }
                            }
                        }
                    }
                }
                add_message(&mut next, &goal_id, content, "auto", "assistant", None)?;
                touched.push(goal_id);
            }
            IntakeAction::Change {
                goal_id,
                title,
                acceptance,
                reason,
                quote,
            } => {
                grounded(&turn, &quote)?;
                let i = fresh(state, &turn, &stamps, &goal_id)?;
                if matches!(next.goals[i].status, GoalStatus::Cancelled) {
                    return Err(invalid("a cancelled goal cannot change direction"));
                }
                if let Some(h) = next.hierarchy.as_ref() {
                    if let Some(owner) = h.goal_owners.get(&goal_id) {
                        let is_chief = turn.actor.as_deref().unwrap_or("chief") == "chief";
                        if is_chief {
                            return Err(invalid("global manager may not message or revise a project-managed goal directly"));
                        }
                        if let Some(act) = turn.actor.as_deref() {
                            if let Some(pid) = act.strip_prefix("project:") {
                                if pid != owner.project_id {
                                    return Err(invalid("project manager cannot revise goals of another project"));
                                }
                            }
                        }
                    }
                }
                revise_goal(
                    &mut next,
                    i,
                    title,
                    acceptance,
                    format!("User message {}: {}", turn.id, reason),
                    "assistant",
                    false,
                )?;
                touched.push(goal_id);
            }
            IntakeAction::ProposeMemory {
                project_path,
                category,
                content,
            } => {
                let content = content.trim().to_string();
                bounded(&content, 2000)?;
                if !MEMORY_CATEGORIES.contains(&category.as_str()) {
                    return Err(invalid("invalid memory category"));
                }
                if !allowed_project(&next, &turn, &project_path) {
                    return Err(invalid("memory scope is outside the authorized projects"));
                }
                let hash = memory_hash(&project_path, &category, &content);
                let duplicate = next.memory_proposals.iter().any(|p| {
                    p.content_hash == hash
                        && matches!(p.state, ProposalState::Proposed | ProposalState::Confirmed)
                });
                if !duplicate {
                    if next.memory_proposals.len() >= MAX_PROPOSALS {
                        return Err(invalid("memory proposal limit reached"));
                    }
                    next.memory_proposals.push(MemoryProposal {
                        id: format!("{}:{}", turn.id, &hash[..12]),
                        turn_id: turn.id.clone(),
                        project_path,
                        category,
                        content,
                        content_hash: hash,
                        state: ProposalState::Proposed,
                        memory_id: None,
                        error: None,
                    });
                }
            }
        }
    }
    if next.conversation.len() >= MAX_CONVERSATION {
        return Err(invalid("conversation limit reached"));
    }
    touched.sort();
    touched.dedup();
    // The user's decision is a new control generation for exactly the goals it touched.
    for g in &touched {
        bump_user_generation(&mut next, g);
    }
    let reply_id = format!("reply:{}", turn.id);
    {
        let user = &mut next.conversation[turn_index];
        user.status = TurnStatus::Handled;
        user.error = None;
        for g in &touched {
            if !user.goal_ids.contains(g) {
                user.goal_ids.push(g.clone());
            }
        }
    }
    let user = next.conversation[turn_index].clone();
    if !next.conversation.iter().any(|t| t.id == reply_id) {
        next.conversation.push(ConversationTurn {
            id: reply_id,
            created_at: chrono::Utc::now().to_rfc3339(),
            author: TurnAuthor::Assistant,
            content: decision.summary,
            reply_to: Some(turn.id.clone()),
            project_paths: user.project_paths.clone(),
            status: TurnStatus::Handled,
            goal_ids: user.goal_ids.clone(),
            question_ids: user.question_ids.clone(),
            error: None,
            source_run_id: Some(run_id.into()),
            actor: turn.actor.clone(),
        });
    }
    next.intake_stamps.remove(run_id);
    Ok(next)
}

/// Link a pending request's outcome back to the message which routed it.
pub(crate) fn link_request_outcome(
    state: &mut AssistantState,
    turn_id: Option<&str>,
    goal_id: Option<&str>,
    question_id: Option<&str>,
) {
    let Some(turn_id) = turn_id else { return };
    let ids = [format!("reply:{turn_id}"), turn_id.to_string()];
    for t in &mut state.conversation {
        if ids.contains(&t.id) {
            if let Some(g) = goal_id {
                if !t.goal_ids.iter().any(|x| x == g) {
                    t.goal_ids.push(g.into());
                }
            }
            if let Some(q) = question_id {
                if !t.question_ids.iter().any(|x| x == q) {
                    t.question_ids.push(q.into());
                }
            }
        }
    }
}

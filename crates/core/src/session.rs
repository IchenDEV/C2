//! Session / message / part model.
//!
//! A session is one provider-neutral conversation shown as a row in the left session list. One
//! provider owns its live ACP runtime at a time, but an idle session may replace that runtime while
//! retaining the same durable transcript and workspace.
//! Persistence (SQLite) lands in M1; these are the in-memory shapes the store will mirror.

use crate::permission::{PermissionMode, SandboxPolicy};
use crate::provider::ProviderId;
use crate::worktree::{DirectoryIdentity, ResolvedWorktreeBaseline};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Our own session id (a UUID). Distinct from the provider's ACP session id, which we store
/// separately so we can resume via ACP `session/load`.
pub type SessionId = String;

/// Whether one side of long-term memory follows the global setting, is explicitly allowed, or is
/// denied for this session. The global master switch still wins; `Allow` expresses session intent
/// and does not bypass a disabled global feature.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryAccess {
    #[default]
    Inherit,
    Allow,
    Deny,
}

impl MemoryAccess {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Inherit => "inherit",
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }

    pub(crate) fn from_db(value: &str) -> Self {
        match value {
            "allow" => Self::Allow,
            "deny" => Self::Deny,
            _ => Self::Inherit,
        }
    }
}

/// Durable fencing state for moving one task between C2 runtimes. Only `Active` sessions may
/// accept provider work; `Prepared` fences the source, `Accepted` hides an uncommitted target,
/// and `Transferred` records the archived source after the target has accepted the payload.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffState {
    #[default]
    Active,
    Prepared,
    Accepted,
    Transferred,
}

impl HandoffState {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Prepared => "prepared",
            Self::Accepted => "accepted",
            Self::Transferred => "transferred",
        }
    }

    pub(crate) fn from_db(value: &str) -> Self {
        match value {
            "prepared" => Self::Prepared,
            "accepted" => Self::Accepted,
            "transferred" => Self::Transferred,
            _ => Self::Active,
        }
    }
}

pub const UNTITLED_SESSION_TITLE: &str = "Untitled session";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Agent,
}

/// Exclusive transcript-page boundary. The wrapped value is the sequence number of a persisted
/// user row, which keeps every page aligned to complete conversation turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TranscriptCursor(pub i64);

/// Default and hard maximum number of user turns returned by one transcript page.
pub const DEFAULT_TRANSCRIPT_TURNS: usize = 20;
pub const MAX_TRANSCRIPT_TURNS: usize = 50;

/// Who last chose a session title. Automatic naming may replace the placeholder (and, later, an
/// automatic title), but it must never overwrite a title the user entered deliberately.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionTitleOrigin {
    #[default]
    Default,
    Automatic,
    Manual,
}

/// Core-owned lifecycle state for one conversation. Frontends use the monotonic revision to merge
/// list snapshots with live events without letting a late snapshot roll state backwards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionActivity {
    pub revision: u64,
    pub state: SessionRunState,
}

impl Default for SessionActivity {
    fn default() -> Self {
        Self {
            revision: 0,
            state: SessionRunState::Idle,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionRunState {
    Idle,
    Running {
        turn_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_request_id: Option<String>,
    },
    AwaitingInput {
        turn_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_request_id: Option<String>,
        pending: Vec<PendingInput>,
    },
    Failed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        reason: RunFailureReason,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunFailureReason {
    Interrupted,
    ProviderError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingInput {
    pub input_id: String,
    pub kind: PendingInputKind,
    pub title: String,
    pub options: Vec<(String, String)>,
    /// ACP permission option kind keyed by option id. Kept separately so existing frontend option
    /// tuples retain their wire shape while protocol adapters can distinguish once/always/reject.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub option_kinds: BTreeMap<String, String>,
    pub sequence: u64,
    #[serde(default)]
    pub context: crate::permission::PermissionContext,
    /// The structured question behind an [`PendingInputKind::Elicitation`] input. Clients that
    /// understand it render the form; the rest fall back to `options`, which projects the common
    /// single-question shape onto the same allow/deny prompt they already draw.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<crate::elicitation::ElicitationForm>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingInputKind {
    Permission,
    /// A structured question from the agent (ACP `elicitation/create`), e.g. Claude Code's
    /// built-in `AskUserQuestion` tool. Answered with form content, not a permission option.
    Elicitation,
}

/// Read-only compatibility for plan rows written by older versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanEntry {
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

impl<'de> Deserialize<'de> for PlanEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum WireEntry {
            Legacy(String),
            Structured {
                content: String,
                #[serde(default)]
                priority: Option<String>,
                #[serde(default)]
                status: Option<String>,
            },
        }

        Ok(match WireEntry::deserialize(deserializer)? {
            WireEntry::Legacy(content) => Self {
                content,
                priority: None,
                status: None,
            },
            WireEntry::Structured {
                content,
                priority,
                status,
            } => Self {
                content,
                priority,
                status,
            },
        })
    }
}

impl From<&str> for PlanEntry {
    fn from(content: &str) -> Self {
        Self {
            content: content.into(),
            priority: None,
            status: None,
        }
    }
}

impl From<String> for PlanEntry {
    fn from(content: String) -> Self {
        Self {
            content,
            priority: None,
            status: None,
        }
    }
}

/// A rendered piece of a message. The transcript is a flat list of these per message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Part {
    Text {
        text: String,
    },
    /// The user's canonical document, kept separate from the compiled prompt sent to the agent.
    /// `display` is the chat-bubble projection; `text` is the canonical searchable/context form.
    Prompt {
        text: String,
        display: String,
    },
    Reasoning {
        text: String,
    },
    ToolCall {
        id: String,
        title: String,
        status: String,
        /// Provider-neutral ACP category when the adapter supplied one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_kind: Option<String>,
        /// Whitelisted agent/workflow launch fields; never the arbitrary raw tool payload.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_input: Option<Value>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        outputs: Vec<crate::artifact::ToolOutput>,
    },
    Plan {
        entries: Vec<PlanEntry>,
    },
}

pub(crate) fn tool_status_is_terminal(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "completed" | "failed" | "cancelled" | "canceled" | "rejected" | "denied"
    )
}

pub(crate) fn tool_status_is_in_flight(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "pending" | "in_progress" | "running"
    )
}

/// One durably ordered transcript part.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub seq: i64,
    pub role: Role,
    pub part: Part,
    /// Durable wall-clock time for this exact row. Older handoffs deserialize as unknown.
    #[serde(default)]
    pub created_at: i64,
    /// Earliest update folded into this row, used by projections such as tool timelines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
}

/// A turn-aligned slice of a transcript. `next_before` is the exclusive cursor for the next older
/// page. `snapshot_through` is the newest sequence visible to the read transaction that produced
/// this page, allowing clients to merge later live events without guessing the persistence edge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptPage {
    pub entries: Vec<TranscriptEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_before: Option<TranscriptCursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_through: Option<TranscriptCursor>,
}

impl TranscriptPage {
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
            next_before: None,
            snapshot_through: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub session_id: SessionId,
    pub role: Role,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub title: String,
    /// Source of the current title, used to protect manual renames from automatic naming.
    #[serde(default)]
    pub title_origin: SessionTitleOrigin,
    /// Pinned sessions stay above the recency-sorted active session list.
    ///
    /// Defaulting keeps sessions serialized by older C2 versions readable.
    #[serde(default)]
    pub pinned: bool,
    /// App-lifetime side chats are persisted only for crash-safe turn handling and never appear
    /// in the durable rail. Startup and the explicit close command purge them.
    #[serde(default)]
    pub transient: bool,
    /// Durable core lifecycle projection. Older serialized sessions deserialize as idle revision 0.
    #[serde(default)]
    pub activity: SessionActivity,
    pub provider: ProviderId,
    pub model: Option<String>,
    /// The source project directory selected by the user. Worktree sessions retain this identity
    /// while `cwd` moves to the corresponding directory inside the isolated checkout.
    ///
    /// Older serialized sessions did not retain this distinction.
    #[serde(default)]
    pub project_path: Option<String>,
    /// Working directory the provider runs in. For a worktree session this may be a subdirectory of
    /// `worktree_path`, mirroring the directory selected inside the source repository.
    pub cwd: String,
    /// Root of the isolated checkout, even when `cwd` points at one of its subdirectories.
    pub worktree_path: Option<String>,
    /// Stable filesystem identity of the isolated checkout root. This closes the gap left by Git
    /// metadata alone: a different directory can be placed at the same path with a copied `.git`
    /// marker. Older persisted sessions lack this receipt and therefore use degraded validation.
    #[serde(default)]
    pub worktree_identity: Option<DirectoryIdentity>,
    /// Canonical Git common directory shared by the source repository and all of its worktrees.
    /// This remains usable when the originally selected source was itself a linked worktree that
    /// has since been removed.
    #[serde(default)]
    pub worktree_common_dir: Option<String>,
    /// Canonical per-worktree Git administrative directory. This distinguishes two checkouts in
    /// the same common repository even if one is moved onto the other's former filesystem path.
    #[serde(default)]
    pub worktree_git_dir: Option<String>,
    /// Exact local ref and immutable commit used to create `worktree_path`.
    #[serde(default)]
    pub worktree_baseline: Option<ResolvedWorktreeBaseline>,
    /// Set once the explicit discard flow permanently removed this session's isolated checkout
    /// and branch. The session stays readable history; running a provider in it is refused.
    #[serde(default)]
    pub worktree_discarded: bool,
    pub permission_mode: PermissionMode,
    /// The sandbox axis is persisted independently from approval mode. Older sessions used the
    /// product default (`workspace_write`).
    #[serde(default)]
    pub sandbox_policy: SandboxPolicy,
    /// The provider's ACP session id, once `session/new` (or `session/load`) has run.
    pub acp_session_id: Option<String>,
    /// Per-session narrowing of prompt-time recall.
    pub memory_read: MemoryAccess,
    /// Per-session narrowing of completed-turn learning.
    pub memory_write: MemoryAccess,
    pub created_at: i64,
    /// Last high-value re-entry into active work. Token chunks and background updates never touch
    /// this timestamp; accepted user prompts and explicit unarchive do.
    #[serde(default)]
    pub last_active_at: i64,
}

impl Session {
    pub fn new(provider: ProviderId, cwd: impl Into<String>) -> Self {
        let cwd = cwd.into();
        let created_at = now_millis();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            title: UNTITLED_SESSION_TITLE.into(),
            title_origin: SessionTitleOrigin::Default,
            pinned: false,
            transient: false,
            activity: SessionActivity::default(),
            provider,
            model: None,
            project_path: Some(cwd.clone()),
            cwd,
            worktree_path: None,
            worktree_identity: None,
            worktree_common_dir: None,
            worktree_git_dir: None,
            worktree_baseline: None,
            worktree_discarded: false,
            permission_mode: PermissionMode::Ask,
            sandbox_policy: SandboxPolicy::default(),
            acp_session_id: None,
            memory_read: MemoryAccess::Inherit,
            memory_write: MemoryAccess::Inherit,
            created_at,
            last_active_at: created_at,
        }
    }
}

/// Make a compact, deterministic title from the user's own first text block.
///
/// This deliberately does not call the active agent: using the conversation's ACP session for a
/// side request would pollute its context, and a separate text-generation service does not exist
/// yet. The result stays in the user's language, stops at the first sentence, and is bounded to
/// eight words / forty characters (twenty-four for unspaced scripts such as Chinese).
pub fn initial_session_title(text: &str) -> Option<String> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("```"))?;
    let line = line.trim_start_matches(|c: char| {
        c.is_whitespace() || matches!(c, '#' | '>' | '*' | '-' | '+' | '`' | '"' | '\'')
    });

    let sentence = line
        .split(|c| matches!(c, '.' | '?' | '!' | '。' | '？' | '！' | ';' | '；'))
        .next()
        .unwrap_or(line)
        .trim();
    if sentence.is_empty() {
        return None;
    }

    let uses_unspaced_script = sentence.chars().any(|c| {
        matches!(
            c,
            '\u{2E80}'..='\u{2FDF}'
                | '\u{3040}'..='\u{30FF}'
                | '\u{3400}'..='\u{9FFF}'
                | '\u{F900}'..='\u{FAFF}'
                | '\u{AC00}'..='\u{D7AF}'
        )
    });
    let has_word_boundaries = !uses_unspaced_script && sentence.split_whitespace().count() > 1;
    let limit = if uses_unspaced_script { 24 } else { 40 };
    let mut title = String::new();
    if has_word_boundaries {
        for word in sentence.split_whitespace().take(8) {
            let needed = word.chars().count() + usize::from(!title.is_empty());
            if !title.is_empty() && title.chars().count() + needed > limit {
                break;
            }
            if !title.is_empty() {
                title.push(' ');
            }
            title.push_str(word);
        }
    } else {
        title = sentence.chars().take(limit).collect();
    }

    let title = title
        .trim()
        .trim_end_matches(|c: char| c.is_whitespace() || matches!(c, ',' | ':' | '，' | '：'))
        .to_string();
    (!title.is_empty()).then_some(title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_entries_keep_status_and_accept_legacy_strings() {
        let structured: Part = serde_json::from_value(serde_json::json!({
            "kind": "plan",
            "entries": [{
                "content": "Run renderer checks",
                "priority": "high",
                "status": "in_progress"
            }]
        }))
        .unwrap();
        let legacy: Part = serde_json::from_value(serde_json::json!({
            "kind": "plan",
            "entries": ["Inspect the workspace"]
        }))
        .unwrap();

        let Part::Plan { entries } = structured else {
            panic!("expected plan");
        };
        assert_eq!(entries[0].content, "Run renderer checks");
        assert_eq!(entries[0].priority.as_deref(), Some("high"));
        assert_eq!(entries[0].status.as_deref(), Some("in_progress"));

        let Part::Plan { entries } = legacy else {
            panic!("expected legacy plan");
        };
        assert_eq!(entries, vec![PlanEntry::from("Inspect the workspace")]);
    }

    #[test]
    fn older_serialized_sessions_default_to_unpinned() {
        let session = Session::new(ProviderId::Grok, "/work");
        let mut value = serde_json::to_value(&session).unwrap();
        value.as_object_mut().unwrap().remove("pinned");
        value.as_object_mut().unwrap().remove("activity");
        value.as_object_mut().unwrap().remove("worktree_baseline");
        value.as_object_mut().unwrap().remove("worktree_identity");
        value.as_object_mut().unwrap().remove("worktree_common_dir");
        value.as_object_mut().unwrap().remove("worktree_git_dir");
        value.as_object_mut().unwrap().remove("worktree_discarded");
        value.as_object_mut().unwrap().remove("sandbox_policy");

        let restored: Session = serde_json::from_value(value).unwrap();
        assert!(!restored.pinned);
        assert_eq!(restored.activity, SessionActivity::default());
        assert_eq!(restored.title_origin, SessionTitleOrigin::Default);
        assert_eq!(restored.project_path.as_deref(), Some("/work"));
        assert!(restored.worktree_baseline.is_none());
        assert!(restored.worktree_identity.is_none());
        assert!(restored.worktree_common_dir.is_none());
        assert!(restored.worktree_git_dir.is_none());
        assert!(!restored.worktree_discarded);
        assert_eq!(restored.sandbox_policy, SandboxPolicy::WorkspaceWrite);
    }

    #[test]
    fn session_activity_uses_a_stable_internally_tagged_wire_shape() {
        let activity = SessionActivity {
            revision: 3,
            state: SessionRunState::AwaitingInput {
                turn_id: "turn-1".into(),
                prompt_request_id: Some("prompt-1".into()),
                pending: vec![PendingInput {
                    input_id: "input-1".into(),
                    kind: PendingInputKind::Permission,
                    title: "Run tests".into(),
                    options: vec![("allow".into(), "Allow".into())],
                    option_kinds: BTreeMap::from([("allow".into(), "allow_once".into())]),
                    sequence: 7,
                    context: Default::default(),
                    form: None,
                }],
            },
        };

        let value = serde_json::to_value(&activity).unwrap();
        assert_eq!(value["revision"], 3);
        assert_eq!(value["state"]["kind"], "awaiting_input");
        assert_eq!(value["state"]["pending"][0]["kind"], "permission");
        assert_eq!(
            serde_json::from_value::<SessionActivity>(value).unwrap(),
            activity
        );
    }

    #[test]
    fn older_serialized_sessions_default_to_no_project_path() {
        let session = Session::new(ProviderId::Grok, "/work");
        let mut value = serde_json::to_value(&session).unwrap();
        value.as_object_mut().unwrap().remove("project_path");

        let restored: Session = serde_json::from_value(value).unwrap();
        assert!(restored.project_path.is_none());
    }

    #[test]
    fn initial_titles_are_compact_and_stay_in_the_users_language() {
        assert_eq!(
            initial_session_title("# Implement conversation search with useful snippets and loading states. More detail"),
            Some("Implement conversation search with".into()),
        );
        assert_eq!(
            initial_session_title("调研并吸纳 t3code 更早版本中的高价值功能，继续完善产品"),
            Some("调研并吸纳 t3code 更早版本中的高价值功能".into()),
        );
    }

    #[test]
    fn initial_titles_skip_code_fences_and_empty_input() {
        assert_eq!(
            initial_session_title("```rust\nfn main() {}\n```"),
            Some("fn main() {}".into())
        );
        assert_eq!(initial_session_title("  \n```\n```"), None);
    }
}

/// Render a stored transcript as markdown context for `@`-mentioning a past chat from a new
/// prompt. Only the conversation's words travel: reasoning and tool chatter are omitted, plans
/// ride along as bullet lists. Long chats keep their tail — the end of a planning conversation
/// is where the conclusions live.
pub fn transcript_context(title: &str, transcript: &[(Role, Part)]) -> String {
    transcript_context_with_omission(title, transcript, false)
}

/// Render transcript context while disclosing that the caller deliberately supplied only a recent
/// page. This keeps the public compatibility helper above useful for complete in-memory inputs and
/// lets paginated callers remain honest about the older turns they did not hydrate.
pub fn transcript_context_with_omission(
    title: &str,
    transcript: &[(Role, Part)],
    earlier_omitted: bool,
) -> String {
    /// Strict context budget for one referenced chat (~4k tokens), measured as Unicode scalar
    /// values rather than UTF-8 bytes so CJK text receives the same predictable allowance.
    const MAX_CONTEXT_CHARS: usize = 16_000;
    const OMISSION_MARKER: &str = "_(earlier messages omitted)_";

    // Collapse consecutive same-role parts into one turn, so a streamed answer that landed as
    // many parts reads as one "Agent:" block.
    let mut turns: Vec<(Role, String)> = Vec::new();
    for (role, part) in transcript {
        let text = match part {
            Part::Text { text } => text.trim().to_string(),
            Part::Prompt { text, .. } => text.trim().to_string(),
            Part::Reasoning { .. } | Part::ToolCall { .. } | Part::Plan { .. } => continue,
        };
        if text.is_empty() {
            continue;
        }
        match turns.last_mut() {
            Some((r, buf)) if *r == *role => {
                buf.push_str("\n\n");
                buf.push_str(&text);
            }
            _ => turns.push((*role, text)),
        }
    }

    let body: Vec<String> = turns
        .iter()
        .map(|(role, text)| {
            let who = match role {
                Role::User => "User",
                Role::Agent => "Agent",
            };
            format!("**{who}:**\n{text}")
        })
        .collect();
    // An empty chat contributes nothing; compile reports it as unresolved instead.
    if body.is_empty() {
        return String::new();
    }
    let title: String = title.chars().take(512).collect();
    let header = format!("**Referenced chat** — {title}");
    let render = |blocks: &[String], omitted: bool| {
        let mut out = header.clone();
        if omitted {
            out.push_str("\n\n");
            out.push_str(OMISSION_MARKER);
        }
        for block in blocks {
            out.push_str("\n\n");
            out.push_str(block);
        }
        out
    };

    let complete = render(&body, earlier_omitted);
    if complete.chars().count() <= MAX_CONTEXT_CHARS {
        return complete;
    }

    // Keep whole recent turns whenever possible. If even the latest turn is oversized, preserve
    // its speaker label and the most recent tail of its text rather than violating the budget.
    let mut kept: Vec<String> = Vec::new();
    for block in body.iter().rev() {
        let mut candidate = Vec::with_capacity(kept.len() + 1);
        candidate.push(block.clone());
        candidate.extend(kept.iter().cloned());
        if render(&candidate, true).chars().count() <= MAX_CONTEXT_CHARS {
            kept = candidate;
        } else {
            break;
        }
    }

    if kept.is_empty() {
        let newest = body.last().expect("non-empty body checked above");
        let label_end = newest.find('\n').map(|index| index + 1).unwrap_or(0);
        let label = &newest[..label_end];
        let text = &newest[label_end..];
        let fixed = render(&[label.to_string()], true).chars().count();
        let room = MAX_CONTEXT_CHARS.saturating_sub(fixed);
        let tail: String = text
            .chars()
            .rev()
            .take(room.saturating_sub(1))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        kept.push(format!("{label}…{tail}"));
    }

    let out = render(&kept, true);
    debug_assert!(out.chars().count() <= MAX_CONTEXT_CHARS);
    out
}

#[cfg(test)]
mod transcript_context_tests {
    use super::{transcript_context, Part, Role};

    #[test]
    fn context_omits_reasoning_tools_and_retired_plans() {
        let transcript = vec![
            (
                Role::User,
                Part::Prompt {
                    text: "ship the feature".into(),
                    display: "ship".into(),
                },
            ),
            (
                Role::Agent,
                Part::Reasoning {
                    text: "private chain".into(),
                },
            ),
            (
                Role::Agent,
                Part::ToolCall {
                    id: "secret".into(),
                    title: "dangerous payload".into(),
                    status: "completed".into(),
                    tool_kind: None,
                    agent_input: None,
                    outputs: Vec::new(),
                },
            ),
            (
                Role::Agent,
                Part::Plan {
                    entries: vec!["verify".into(), "release".into()],
                },
            ),
        ];

        let context = transcript_context("Release", &transcript);
        assert!(context.contains("ship the feature"));
        assert!(!context.contains("- verify\n- release"));
        assert!(!context.contains("private chain"));
        assert!(!context.contains("dangerous payload"));
    }

    #[test]
    fn oversized_latest_turn_keeps_its_unicode_tail_within_the_strict_budget() {
        let latest = format!("{}LATEST_END", "界".repeat(24_000));
        let transcript = vec![
            (
                Role::User,
                Part::Text {
                    text: "OLD_PREFIX".into(),
                },
            ),
            (Role::Agent, Part::Text { text: latest }),
        ];

        let context = transcript_context("Long chat", &transcript);
        assert!(context.chars().count() <= 16_000);
        assert!(context.contains("_(earlier messages omitted)_"));
        assert!(context.ends_with("LATEST_END"));
        assert!(!context.contains("OLD_PREFIX"));
    }
}

/// Unix time in milliseconds. Fine for ordering the session list.
pub fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

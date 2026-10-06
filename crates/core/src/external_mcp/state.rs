//! Engine-owned runtime state of the external MCP surface (default off).

use crate::elicitation::ElicitationForm;
use crate::event::Event;
use crate::external_mcp::audit::{AuditSink, JsonlAuditSink, MemoryAuditSink};
use crate::external_mcp::clients::{ExternalClientRegistry, RateLimiter};
use crate::external_mcp::events::{
    approval_requested, map_event_with, question_requested, sanitize_text, EventHub, EventKind,
    EventObserver, EventRing, MappedEvent, DEFAULT_RING_CAPACITY, DEFAULT_TEXT_MAX_CHARS,
};
use crate::external_mcp::subscriptions::SubscriptionRegistry;
use crate::session::{PendingInput, PendingInputKind, SessionActivity, SessionRunState};
use crate::store::Store;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

pub const EXTERNAL_MCP_ENV: &str = "CODETWO_EXTERNAL_MCP";
pub const CLIENTS_FILE: &str = "external-mcp-clients.json";
pub const AUDIT_FILE: &str = "external-mcp-audit.jsonl";

/// Entries kept by the tap's session index before it is rebuilt from events and the store.
const SESSION_INDEX_CAP: usize = 4096;

static ALLOWED_HOSTS: std::sync::OnceLock<RwLock<Vec<String>>> = std::sync::OnceLock::new();

fn allowed_hosts_lock() -> &'static RwLock<Vec<String>> {
    ALLOWED_HOSTS.get_or_init(|| RwLock::new(Vec::new()))
}

/// Remote tunnel hostnames permitted in `Host` / `Origin` (default empty = loopback only).
pub fn set_allowed_hosts(hosts: Vec<String>) {
    *allowed_hosts_lock().write().unwrap() = hosts;
}

pub fn allowed_hosts_snapshot() -> Vec<String> {
    allowed_hosts_lock().read().unwrap().clone()
}

pub fn host_allowed(host: &str) -> bool {
    if host.trim() != host || host.contains(['/', '?', '#', '@', '\\']) {
        return false;
    }
    let Ok(authority) = url::Url::parse(&format!("http://{host}")) else {
        return false;
    };
    let Some(host) = authority.host_str() else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "::1"
        || host == "[::1]"
    {
        return true;
    }
    allowed_hosts_lock()
        .read()
        .unwrap()
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(host))
}

#[derive(Debug, Clone)]
pub struct PendingExternalRequest {
    pub request_id: String,
    pub session_id: String,
    pub project_id: Option<String>,
    pub kind: PendingInputKind,
    pub title: String,
    pub tool_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Open requests kept at once. Closure is derived from the authoritative activity snapshots, so
/// this only bites when a session vanishes without a final snapshot; the oldest is then expired
/// (with a `resolved` envelope) instead of growing without bound.
const OPEN_PENDING_CAP: usize = 1024;
/// Closed ids remembered so a late request event never re-opens a request that already resolved.
const CLOSED_CAP: usize = 2048;
/// Rejected prompt receipts remembered for `codetwo_turn_wait` (direct prompts have no store row).
const REJECTION_CAP: usize = 256;

#[derive(Default)]
struct RecentIds {
    set: HashSet<String>,
    order: VecDeque<String>,
}

impl RecentIds {
    fn insert(&mut self, id: String) {
        if self.set.insert(id.clone()) {
            self.order.push_back(id);
            if self.order.len() > CLOSED_CAP {
                if let Some(old) = self.order.pop_front() {
                    self.set.remove(&old);
                }
            }
        }
    }

    fn contains(&self, id: &str) -> bool {
        self.set.contains(id)
    }
}

struct OpenRequest {
    record: PendingExternalRequest,
    /// `requested` was pushed, so closing it must push the matching `resolved`.
    announced: bool,
}

/// Pending approvals/questions as the event surface exposes them. Opened and closed only from the
/// ordered `SessionActivityChanged` snapshots (the engine's single pending authority); the
/// request events and the answer hooks are idempotent shortcuts onto the same transitions, so a
/// request is announced once and resolved once whichever signal arrives first or late.
#[derive(Default)]
struct Ledger {
    open: HashMap<String, OpenRequest>,
    closed: RecentIds,
    revisions: HashMap<String, u64>,
    /// Requests an engine answer is in flight for: their removal is `answered`, not `cancelled`.
    answering: HashSet<String>,
    /// Prompt receipts (`session`, `request_id`) refused before a turn started.
    rejected: VecDeque<(String, String)>,
}

type SharedLedger = Arc<Mutex<Ledger>>;

/// Session → project/turn lookups that never touch the engine's session lock: the tap runs on
/// every producer's call stack, some of which already hold engine locks.
#[derive(Default)]
struct SessionIndex {
    projects: HashMap<String, String>,
    /// Current (or last) turn of a session and the prompt request id that started it.
    turns: HashMap<String, (String, Option<String>)>,
}

/// The single event tap: maps each engine event into the ring and tracks pending requests.
pub struct EventTap {
    enabled: Arc<AtomicBool>,
    hub: EventHub,
    ledger: SharedLedger,
    index: Mutex<SessionIndex>,
    store: Option<Arc<Store>>,
}

fn project_of_paths(project_path: Option<&str>, cwd: &str) -> Option<String> {
    project_path
        .filter(|path| !path.is_empty())
        .or_else(|| (!cwd.is_empty()).then_some(cwd))
        .map(str::to_owned)
}

fn permission_record(
    session: &str,
    project: Option<String>,
    request_id: &str,
    title: &str,
    tool: Option<&str>,
) -> PendingExternalRequest {
    PendingExternalRequest {
        request_id: request_id.to_owned(),
        session_id: session.to_owned(),
        project_id: project,
        kind: PendingInputKind::Permission,
        title: sanitize_text(title, DEFAULT_TEXT_MAX_CHARS),
        tool_name: tool.map(str::to_owned),
        created_at: Utc::now(),
    }
}

fn question_record(
    session: &str,
    project: Option<String>,
    request_id: &str,
    form: &ElicitationForm,
) -> PendingExternalRequest {
    let title = form
        .questions()
        .next()
        .and_then(|q| q.description.as_deref().or(q.title.as_deref()))
        .map(|t| sanitize_text(t, DEFAULT_TEXT_MAX_CHARS))
        .unwrap_or_else(|| sanitize_text(&form.message, DEFAULT_TEXT_MAX_CHARS));
    PendingExternalRequest {
        request_id: request_id.to_owned(),
        session_id: session.to_owned(),
        project_id: project,
        kind: PendingInputKind::Elicitation,
        title,
        tool_name: None,
        created_at: Utc::now(),
    }
}

impl EventTap {
    /// Project of a session: the live index first, then one store lookup (cached).
    pub fn project_of(&self, session: &str) -> Option<String> {
        if let Some(project) = self.index.lock().unwrap().projects.get(session) {
            return Some(project.clone());
        }
        let found = self
            .store
            .as_ref()?
            .get_session(session)
            .ok()
            .flatten()
            .and_then(|s| project_of_paths(s.project_path.as_deref(), &s.cwd))?;
        let mut index = self.index.lock().unwrap();
        if index.projects.len() >= SESSION_INDEX_CAP {
            index.projects.clear();
        }
        index.projects.insert(session.to_string(), found.clone());
        Some(found)
    }

    /// The session's current turn id, or the last one it had.
    pub fn turn_of(&self, session: &str) -> Option<String> {
        self.index
            .lock()
            .unwrap()
            .turns
            .get(session)
            .map(|(turn, _)| turn.clone())
    }

    /// Whether the session's current (or last) turn was started by prompt `request_id`.
    fn owns_request(&self, session: &str, request_id: &str) -> bool {
        self.index
            .lock()
            .unwrap()
            .turns
            .get(session)
            .is_some_and(|(_, request)| request.as_deref() == Some(request_id))
    }

    /// A prompt receipt that was refused before it ever became a turn (never delivered).
    pub fn rejected_request(&self, session: &str, request_id: &str) -> bool {
        self.ledger
            .lock()
            .unwrap()
            .rejected
            .iter()
            .any(|(s, r)| s == session && r == request_id)
    }

    fn update_index(&self, event: &Event) {
        let mut index = self.index.lock().unwrap();
        match event {
            Event::SessionCreated {
                session,
                cwd,
                project_path,
                ..
            } => {
                if let Some(project) = project_of_paths(project_path.as_deref(), cwd) {
                    if index.projects.len() >= SESSION_INDEX_CAP {
                        index.projects.clear();
                    }
                    index.projects.insert(session.clone(), project);
                }
            }
            Event::SessionActivityChanged { session, activity } => {
                let turn = match &activity.state {
                    SessionRunState::Running {
                        turn_id,
                        prompt_request_id,
                    }
                    | SessionRunState::AwaitingInput {
                        turn_id,
                        prompt_request_id,
                        ..
                    } => Some((turn_id.clone(), prompt_request_id.clone())),
                    SessionRunState::Failed {
                        turn_id: Some(turn_id),
                        ..
                    } => Some((
                        turn_id.clone(),
                        index
                            .turns
                            .get(session)
                            .filter(|(known, _)| known == turn_id)
                            .and_then(|(_, request)| request.clone()),
                    )),
                    SessionRunState::Failed { .. } | SessionRunState::Idle => None,
                };
                if let Some(turn) = turn {
                    if index.turns.len() >= SESSION_INDEX_CAP {
                        index.turns.clear();
                    }
                    index.turns.insert(session.clone(), turn);
                }
            }
            _ => {}
        }
    }

    fn push_locked(&self, (kind, session, project, turn, data): MappedEvent) {
        self.hub
            .ring()
            .lock()
            .expect("event ring mutex poisoned")
            .push(kind, session, project, turn, data);
    }

    /// Open `record`, announcing it with `requested`. Caller holds the ledger lock, which makes
    /// "decide + push" one step: `requested` can never land after its own `resolved`.
    fn open_locked(
        &self,
        ledger: &mut Ledger,
        record: PendingExternalRequest,
        requested: MappedEvent,
    ) {
        if ledger.open.len() >= OPEN_PENDING_CAP {
            let oldest = ledger
                .open
                .values()
                .min_by_key(|open| open.record.created_at)
                .map(|open| open.record.request_id.clone());
            if let Some(oldest) = oldest {
                self.close_locked(ledger, &oldest, "expired");
            }
        }
        self.push_locked(requested);
        ledger.open.insert(
            record.request_id.clone(),
            OpenRequest {
                record,
                announced: true,
            },
        );
    }

    /// Close an open request exactly once with the resolved kind its own record dictates.
    fn close_locked(&self, ledger: &mut Ledger, request_id: &str, outcome: &str) {
        let Some(open) = ledger.open.remove(request_id) else {
            return;
        };
        ledger.closed.insert(request_id.to_owned());
        if !open.announced {
            return;
        }
        let record = open.record;
        let kind = match record.kind {
            PendingInputKind::Permission => EventKind::ApprovalResolved,
            PendingInputKind::Elicitation => EventKind::QuestionResolved,
        };
        let turn = self.turn_of(&record.session_id);
        self.push_locked((
            kind,
            Some(record.session_id),
            record.project_id,
            turn,
            serde_json::json!({ "request_id": request_id, "outcome": outcome }),
        ));
    }

    /// Reconcile the ledger with one activity snapshot: whatever it no longer lists was answered
    /// or cancelled (Stop, turn end, handoff), whatever it newly lists is announced.
    fn sync_pending(&self, session: &str, activity: &SessionActivity) {
        let snapshot: &[PendingInput] = match &activity.state {
            SessionRunState::AwaitingInput { pending, .. } => pending,
            _ => &[],
        };
        let project = (!snapshot.is_empty())
            .then(|| self.project_of(session))
            .flatten();
        let mut ledger = self.ledger.lock().unwrap();
        if ledger
            .revisions
            .get(session)
            .is_some_and(|seen| *seen > activity.revision)
        {
            return;
        }
        if ledger.revisions.len() >= SESSION_INDEX_CAP {
            ledger.revisions.clear();
        }
        ledger
            .revisions
            .insert(session.to_owned(), activity.revision);
        let gone: Vec<String> = ledger
            .open
            .values()
            .filter(|open| open.record.session_id == session)
            .filter(|open| {
                !snapshot
                    .iter()
                    .any(|p| p.input_id == open.record.request_id)
            })
            .map(|open| open.record.request_id.clone())
            .collect();
        for id in gone {
            let outcome = if ledger.answering.remove(&id) {
                "answered"
            } else {
                "cancelled"
            };
            self.close_locked(&mut ledger, &id, outcome);
        }
        for input in snapshot {
            if ledger.open.contains_key(&input.input_id) || ledger.closed.contains(&input.input_id)
            {
                continue;
            }
            let turn = self.turn_of(session);
            let (record, requested) = match (input.kind, &input.form) {
                (PendingInputKind::Elicitation, Some(form)) => (
                    question_record(session, project.clone(), &input.input_id, form),
                    question_requested(
                        session,
                        &input.input_id,
                        form.questions().count(),
                        project.clone(),
                        turn,
                    ),
                ),
                (PendingInputKind::Elicitation, None) => (
                    PendingExternalRequest {
                        kind: PendingInputKind::Elicitation,
                        ..permission_record(
                            session,
                            project.clone(),
                            &input.input_id,
                            &input.title,
                            None,
                        )
                    },
                    question_requested(session, &input.input_id, 0, project.clone(), turn),
                ),
                (PendingInputKind::Permission, _) => (
                    permission_record(
                        session,
                        project.clone(),
                        &input.input_id,
                        &input.title,
                        input.context.tool.as_deref(),
                    ),
                    approval_requested(
                        session,
                        &input.input_id,
                        &input.context,
                        project.clone(),
                        turn,
                    ),
                ),
            };
            self.open_locked(&mut ledger, record, requested);
        }
    }

    /// Request events are the late echo of what the snapshot already announced; they only open a
    /// request nobody has seen (no tracker) and are ignored once it is open or resolved.
    fn announce_from_event(&self, event: &Event) {
        let record = match event {
            Event::PermissionRequest {
                session,
                request_id,
                title,
                context,
                ..
            } => permission_record(
                session,
                self.project_of(session),
                request_id,
                title,
                context.tool.as_deref(),
            ),
            Event::ElicitationRequest {
                session,
                request_id,
                form,
            } => question_record(session, self.project_of(session), request_id, form),
            _ => return,
        };
        let mapped = map_event_with(event, &|s| self.project_of(s), &|s| self.turn_of(s));
        let Some(requested) = mapped.into_iter().next() else {
            return;
        };
        let mut ledger = self.ledger.lock().unwrap();
        if ledger.open.contains_key(&record.request_id)
            || ledger.closed.contains(&record.request_id)
        {
            return;
        }
        self.open_locked(&mut ledger, record, requested);
    }

    /// A prompt refused before it became a turn. It is recorded as a receipt outcome and, when
    /// terminal, surfaced as `turn.failed` **without** a turn id: it must not borrow the id of an
    /// older turn that merely happens to be the session's latest.
    fn reject(&self, session: &str, request_id: &str, terminal: bool) {
        let project = self.project_of(session);
        let mut ledger = self.ledger.lock().unwrap();
        if ledger
            .rejected
            .iter()
            .any(|(s, r)| s == session && r == request_id)
        {
            return;
        }
        if ledger.rejected.len() >= REJECTION_CAP {
            ledger.rejected.pop_front();
        }
        ledger
            .rejected
            .push_back((session.to_owned(), request_id.to_owned()));
        if terminal {
            self.push_locked((
                EventKind::TurnFailed,
                Some(session.to_owned()),
                project,
                None,
                serde_json::json!({
                    "terminal": true,
                    "turn_started": false,
                    "request_id": sanitize_text(request_id, DEFAULT_TEXT_MAX_CHARS),
                }),
            ));
        }
    }

    fn begin_answer(&self, request_id: &str) {
        self.ledger
            .lock()
            .unwrap()
            .answering
            .insert(request_id.to_owned());
    }

    fn finish_answer(&self, request_id: &str, accepted: bool) {
        let mut ledger = self.ledger.lock().unwrap();
        ledger.answering.remove(request_id);
        if accepted {
            self.close_locked(&mut ledger, request_id, "answered");
        }
    }
}

impl EventObserver for EventTap {
    fn observe(&self, event: &Event) {
        self.update_index(event);
        if !self.enabled.load(Ordering::Acquire) {
            return;
        }
        match event {
            Event::SessionActivityChanged { session, activity } => {
                self.sync_pending(session, activity)
            }
            Event::PermissionRequest { .. } | Event::ElicitationRequest { .. } => {
                self.announce_from_event(event)
            }
            Event::Error {
                session: Some(session),
                request_id: Some(request_id),
                terminal,
                ..
            } if !self.owns_request(session, request_id) => {
                self.reject(session, request_id, *terminal)
            }
            _ => {
                self.hub
                    .ingest_with(event, &|s| self.project_of(s), &|s| self.turn_of(s));
            }
        }
    }
}

pub struct ExternalMcpState {
    enabled: Arc<AtomicBool>,
    data_dir: RwLock<Option<PathBuf>>,
    registry: Mutex<Option<ExternalClientRegistry>>,
    hub: EventHub,
    audit: RwLock<Arc<dyn AuditSink>>,
    limiter: RateLimiter,
    ledger: SharedLedger,
    tap: Arc<EventTap>,
    subscriptions: Arc<SubscriptionRegistry>,
}

impl Default for ExternalMcpState {
    fn default() -> Self {
        Self::new()
    }
}

impl ExternalMcpState {
    pub fn new() -> Self {
        Self::with_store(None)
    }

    /// `store` lets the event tap resolve the project of sessions it has not seen created.
    pub fn with_store(store: Option<Arc<Store>>) -> Self {
        let enabled = Arc::new(AtomicBool::new(
            std::env::var(EXTERNAL_MCP_ENV)
                .ok()
                .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")),
        ));
        let hub = EventHub::new(EventRing::new(DEFAULT_RING_CAPACITY));
        let ledger = SharedLedger::default();
        let tap = Arc::new(EventTap {
            enabled: Arc::clone(&enabled),
            hub: hub.clone(),
            ledger: Arc::clone(&ledger),
            index: Mutex::new(SessionIndex::default()),
            store,
        });
        Self {
            enabled,
            data_dir: RwLock::new(None),
            registry: Mutex::new(None),
            hub,
            audit: RwLock::new(Arc::new(MemoryAuditSink::new())),
            limiter: RateLimiter::new(Default::default()),
            ledger,
            tap,
            subscriptions: Arc::new(SubscriptionRegistry::default()),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
    }

    pub fn hub(&self) -> &EventHub {
        &self.hub
    }

    /// The observer the engine installs on its event sender.
    pub fn tap(&self) -> Arc<EventTap> {
        Arc::clone(&self.tap)
    }

    pub fn subscriptions(&self) -> &Arc<SubscriptionRegistry> {
        &self.subscriptions
    }

    /// Project of a session without taking engine locks (see [`EventTap::project_of`]).
    pub fn session_project(&self, session: &str) -> Option<String> {
        self.tap.project_of(session)
    }

    pub fn limiter(&self) -> &RateLimiter {
        &self.limiter
    }

    pub fn audit(&self) -> Arc<dyn AuditSink> {
        self.audit.read().unwrap().clone()
    }

    pub fn data_dir(&self) -> Option<PathBuf> {
        self.data_dir.read().unwrap().clone()
    }

    /// Register a request without announcing it (no `requested`/`resolved` envelope).
    pub fn track_pending(&self, record: PendingExternalRequest) {
        self.ledger
            .lock()
            .unwrap()
            .open
            .entry(record.request_id.clone())
            .or_insert(OpenRequest {
                record,
                announced: false,
            });
    }

    pub fn remove_pending(&self, request_id: &str) -> Option<PendingExternalRequest> {
        self.ledger
            .lock()
            .unwrap()
            .open
            .remove(request_id)
            .map(|open| open.record)
    }

    pub fn list_pending(&self) -> Vec<PendingExternalRequest> {
        let mut pending: Vec<_> = self
            .ledger
            .lock()
            .unwrap()
            .open
            .values()
            .map(|open| open.record.clone())
            .collect();
        pending.sort_by_key(|record| record.created_at);
        pending
    }

    /// Engine hook, **before** it hands an answer to the activity tracker: the snapshot that
    /// removes the request is then labelled `answered` rather than `cancelled`.
    pub fn begin_answer(&self, request_id: &str) {
        if self.is_enabled() {
            self.tap.begin_answer(request_id);
        }
    }

    /// Engine hook, after the answer returned. An accepted answer closes the request if the
    /// snapshot did not already (idempotent); a refused one withdraws the label.
    pub fn finish_answer(&self, request_id: &str, accepted: bool) {
        if self.is_enabled() {
            self.tap.finish_answer(request_id, accepted);
        }
    }

    pub fn push_resolved(
        &self,
        kind: EventKind,
        session_id: String,
        project_id: Option<String>,
        request_id: &str,
    ) {
        let turn_id = self.tap.turn_of(&session_id);
        let _ = self.hub.ring().lock().unwrap().push_resolved(
            kind,
            Some(session_id),
            project_id,
            turn_id,
            serde_json::json!({ "request_id": request_id }),
        );
    }

    /// Point the surface at a data directory: opens (or creates) the credential file and the
    /// audit log there. Fails closed: on error the registry stays closed and no client resolves.
    pub fn configure_data_dir(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let registry = ExternalClientRegistry::open(dir.join(CLIENTS_FILE))
            .map_err(|e| format!("external MCP credential store: {e:?}"))?;
        let audit = JsonlAuditSink::open(dir.join(AUDIT_FILE)).map_err(|e| e.to_string())?;
        *self.registry.lock().unwrap() = Some(registry);
        *self.audit.write().unwrap() = Arc::new(audit);
        *self.data_dir.write().unwrap() = Some(dir.to_path_buf());
        Ok(())
    }

    /// Run `f` with the credential registry; `None` when no data directory is configured.
    pub fn with_registry<T>(&self, f: impl FnOnce(&mut ExternalClientRegistry) -> T) -> Option<T> {
        self.registry.lock().unwrap().as_mut().map(f)
    }
}

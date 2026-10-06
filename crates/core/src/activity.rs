//! Authoritative session lifecycle and pending-input routing.
//!
//! One tracker owns both the single live turn claim and every parked permission sender. The
//! revisioned [`SessionActivity`] projection is therefore derived from one source of truth rather
//! than reconstructed independently by each frontend.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use tokio::sync::oneshot;

use crate::acp::wire::{CreateElicitationResponse, PermissionOutcome};
use crate::elicitation::{ElicitationAnswer, ElicitationForm};
use crate::event::Event;
use crate::external_mcp::events::EventSender;
use crate::permission::PermissionContext;
use crate::session::{
    PendingInput, PendingInputKind, RunFailureReason, SessionActivity, SessionRunState,
};
use crate::store::Store;

const INTERRUPTED_MESSAGE: &str = "C2 stopped before the turn finished";

#[derive(Clone)]
pub struct ActivityTracker {
    inner: Arc<TrackerInner>,
}

struct TrackerInner {
    /// Covers a complete public transition (state mutation, durable CAS, then broadcast). The
    /// state mutex alone is not enough: releasing it before SQLite I/O would let two concurrent
    /// permission callbacks persist or broadcast revisions in the opposite order.
    transitions: Mutex<()>,
    state: Mutex<TrackerState>,
    events: EventSender,
    store: Option<Arc<Store>>,
    turn_terminal_hook: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

#[derive(Default)]
struct TrackerState {
    sessions: HashMap<String, TrackedSession>,
    next_sequence: u64,
}

struct TrackedSession {
    activity: SessionActivity,
    turn: Option<TrackedTurn>,
}

struct TrackedTurn {
    turn_id: String,
    prompt_request_id: Option<String>,
    accepted: bool,
    pending: BTreeMap<u64, PendingRoute>,
}

struct PendingRoute {
    input: PendingInput,
    reply: PendingReply,
}

/// The parked agent call one pending input answers. Two protocol shapes, one queue: a client
/// listing `SessionActivity::pending` sees both, and neither can be resolved with the other's
/// outcome type.
enum PendingReply {
    Permission(oneshot::Sender<PermissionOutcome>),
    Elicitation(oneshot::Sender<CreateElicitationResponse>),
}

impl PendingReply {
    /// Resolve the parked call the way an interruption should: nothing chosen, nothing allowed.
    fn cancel(self) {
        match self {
            PendingReply::Permission(sender) => {
                let _ = sender.send(PermissionOutcome::Cancelled);
            }
            PendingReply::Elicitation(sender) => {
                let _ = sender.send(CreateElicitationResponse::Cancel);
            }
        }
    }
}

fn elicitation_response(
    answer: ElicitationAnswer,
    form: &ElicitationForm,
) -> CreateElicitationResponse {
    match answer {
        ElicitationAnswer::Accept { content } => CreateElicitationResponse::Accept {
            content: Some(form.sanitize_content(&content)),
        },
        ElicitationAnswer::Decline => CreateElicitationResponse::Decline,
        ElicitationAnswer::Cancel => CreateElicitationResponse::Cancel,
    }
}

/// RAII ownership of one core turn. Dropping an accepted, unfinished lease is an interruption;
/// dropping a pre-acceptance claim is silent because the frontend never surrendered its draft.
pub struct TurnLease {
    tracker: ActivityTracker,
    session: String,
    turn_id: String,
}

impl ActivityTracker {
    pub fn new(events: impl Into<EventSender>, store: Option<Arc<Store>>) -> ActivityTracker {
        ActivityTracker {
            inner: Arc::new(TrackerInner {
                transitions: Mutex::new(()),
                state: Mutex::new(TrackerState::default()),
                events: events.into(),
                store,
                turn_terminal_hook: Mutex::new(None),
            }),
        }
    }

    pub(crate) fn set_turn_terminal_hook(&self, hook: Arc<dyn Fn() + Send + Sync>) {
        *self.inner.turn_terminal_hook.lock().unwrap() = Some(hook);
    }

    /// Register a durable snapshot without replacing an active or newer live projection.
    pub fn register(&self, session: &str, activity: SessionActivity) {
        let mut state = self.inner.state.lock().unwrap();
        match state.sessions.get_mut(session) {
            Some(tracked)
                if tracked.turn.is_none() && activity.revision > tracked.activity.revision =>
            {
                tracked.activity = activity;
            }
            Some(_) => {}
            None => {
                state.sessions.insert(
                    session.to_string(),
                    TrackedSession {
                        activity,
                        turn: None,
                    },
                );
            }
        }
    }

    pub fn activity(&self, session: &str) -> Option<SessionActivity> {
        self.inner
            .state
            .lock()
            .unwrap()
            .sessions
            .get(session)
            .map(|tracked| tracked.activity.clone())
    }

    /// Replace the live projection with a snapshot that was already committed by the store.
    ///
    /// Task handoff uses this after installing its durable fence. Clearing the turn first makes
    /// any late provider completion a no-op, while cancelling parked routes ensures no permission
    /// or elicitation remains actionable on the source device.
    pub fn install_durable_snapshot(&self, session: &str, activity: SessionActivity) {
        let _transition = self.inner.transitions.lock().unwrap();
        let routes = {
            let mut state = self.inner.state.lock().unwrap();
            let tracked = state
                .sessions
                .entry(session.to_string())
                .or_insert_with(|| TrackedSession {
                    activity: activity.clone(),
                    turn: None,
                });
            let routes = tracked
                .turn
                .take()
                .map(|turn| turn.pending.into_values().collect::<Vec<_>>())
                .unwrap_or_default();
            tracked.activity = activity.clone();
            routes
        };
        for route in routes {
            route.reply.cancel();
        }
        self.emit_activity(session, activity);
    }

    /// Reserve the session's one turn slot. Reservation is intentionally not a public state change;
    /// the Running revision is published only after durable prompt acceptance.
    pub fn claim(
        &self,
        session: &str,
        prompt_request_id: Option<String>,
        initial_activity: SessionActivity,
    ) -> Option<TurnLease> {
        let mut state = self.inner.state.lock().unwrap();
        let tracked = state
            .sessions
            .entry(session.to_string())
            .or_insert_with(|| TrackedSession {
                activity: initial_activity,
                turn: None,
            });
        if tracked.turn.is_some()
            || matches!(
                tracked.activity.state,
                SessionRunState::Running { .. } | SessionRunState::AwaitingInput { .. }
            )
        {
            return None;
        }
        let turn_id = uuid::Uuid::new_v4().to_string();
        tracked.turn = Some(TrackedTurn {
            turn_id: turn_id.clone(),
            prompt_request_id,
            accepted: false,
            pending: BTreeMap::new(),
        });
        Some(TurnLease {
            tracker: self.clone(),
            session: session.to_string(),
            turn_id,
        })
    }

    pub fn park_permission(
        &self,
        session: &str,
        title: String,
        options: Vec<(String, String)>,
        option_kinds: BTreeMap<String, String>,
        context: PermissionContext,
    ) -> Option<(String, oneshot::Receiver<PermissionOutcome>)> {
        let (sender, receiver) = oneshot::channel();
        let input_id = self.park(
            session,
            |input_id, sequence| PendingInput {
                input_id,
                kind: PendingInputKind::Permission,
                title,
                options,
                option_kinds,
                sequence,
                context,
                form: None,
            },
            PendingReply::Permission(sender),
        )?;
        Some((input_id, receiver))
    }

    /// Park a structured question (ACP `elicitation/create`) on the same queue permissions use, so
    /// one turn's outstanding asks stay ordered and every client sees them in one place.
    pub fn park_elicitation(
        &self,
        session: &str,
        form: ElicitationForm,
    ) -> Option<(String, oneshot::Receiver<CreateElicitationResponse>)> {
        let (sender, receiver) = oneshot::channel();
        let input_id = self.park(
            session,
            |input_id, sequence| PendingInput {
                input_id,
                kind: PendingInputKind::Elicitation,
                title: form.message.clone(),
                options: form.legacy_options(),
                option_kinds: BTreeMap::new(),
                sequence,
                context: PermissionContext::default(),
                form: Some(form),
            },
            PendingReply::Elicitation(sender),
        )?;
        Some((input_id, receiver))
    }

    /// Add one pending input to the live turn, publishing the revision that makes it visible.
    fn park(
        &self,
        session: &str,
        input: impl FnOnce(String, u64) -> PendingInput,
        reply: PendingReply,
    ) -> Option<String> {
        let _transition = self.inner.transitions.lock().unwrap();
        let input_id = uuid::Uuid::new_v4().to_string();
        let (expected_revision, activity) = {
            let mut state = self.inner.state.lock().unwrap();
            state.next_sequence = state.next_sequence.saturating_add(1);
            let sequence = state.next_sequence;
            let tracked = state.sessions.get_mut(session)?;
            let turn = tracked.turn.as_mut()?;
            if !turn.accepted {
                return None;
            }
            turn.pending.insert(
                sequence,
                PendingRoute {
                    input: input(input_id.clone(), sequence),
                    reply,
                },
            );
            let expected_revision = tracked.activity.revision;
            let activity = activity_for_turn(expected_revision.saturating_add(1), turn);
            tracked.activity = activity.clone();
            (expected_revision, activity)
        };
        self.persist_and_emit(session, expected_revision, &activity);
        Some(input_id)
    }

    /// Resolve exactly one permission. Wrong sessions, unknown/duplicate ids, and options the
    /// provider did not advertise are no-ops and never touch the parked sender.
    ///
    /// A pending *elicitation* is answerable this way too, through the permission-shaped options
    /// its form projects ([`ElicitationForm::legacy_options`]): a client that only knows how to
    /// pick an option can still answer a single-question form, and can always skip.
    pub fn answer_permission(
        &self,
        session: &str,
        input_id: &str,
        option_id: Option<&str>,
    ) -> bool {
        let Some(route) = self.take_pending(session, input_id, |input| {
            option_id.is_none_or(|option_id| input.options.iter().any(|(id, _)| id == option_id))
        }) else {
            return false;
        };
        match route.reply {
            PendingReply::Permission(sender) => {
                let outcome = match option_id {
                    Some(option_id) => PermissionOutcome::Selected {
                        option_id: option_id.to_string(),
                    },
                    None => PermissionOutcome::Cancelled,
                };
                sender.send(outcome).is_ok()
            }
            PendingReply::Elicitation(sender) => {
                // `form` is always present on an elicitation route; a missing one can only mean a
                // shape we didn't build, so skip rather than guess an answer.
                let response = match &route.input.form {
                    Some(form) => {
                        elicitation_response(form.answer_from_legacy_option(option_id), form)
                    }
                    None => CreateElicitationResponse::Decline,
                };
                sender.send(response).is_ok()
            }
        }
    }

    /// Answer a parked elicitation with form content. Content is sanitized against the form the
    /// agent sent, so no client can answer with a value that was never offered.
    pub fn answer_elicitation(
        &self,
        session: &str,
        input_id: &str,
        answer: ElicitationAnswer,
    ) -> bool {
        let Some(route) = self.take_pending(session, input_id, |input| {
            input.kind == PendingInputKind::Elicitation
        }) else {
            return false;
        };
        let PendingReply::Elicitation(sender) = route.reply else {
            return false;
        };
        let response = match &route.input.form {
            Some(form) => elicitation_response(answer, form),
            None => CreateElicitationResponse::Decline,
        };
        sender.send(response).is_ok()
    }

    /// Remove one pending input (publishing the revision that hides it) when `accept` agrees it is
    /// answerable. Rejected answers leave the queue — and the parked sender — untouched.
    fn take_pending(
        &self,
        session: &str,
        input_id: &str,
        accept: impl FnOnce(&PendingInput) -> bool,
    ) -> Option<PendingRoute> {
        let _transition = self.inner.transitions.lock().unwrap();
        let (route, expected_revision, activity) = {
            let mut state = self.inner.state.lock().unwrap();
            let tracked = state.sessions.get_mut(session)?;
            let turn = tracked.turn.as_mut().filter(|turn| turn.accepted)?;
            let sequence = turn.pending.iter().find_map(|(sequence, route)| {
                (route.input.input_id == input_id).then_some(*sequence)
            })?;
            if !accept(&turn.pending[&sequence].input) {
                return None;
            }
            let route = turn.pending.remove(&sequence).expect("route found above");
            let expected_revision = tracked.activity.revision;
            let activity = activity_for_turn(expected_revision.saturating_add(1), turn);
            tracked.activity = activity.clone();
            (route, expected_revision, activity)
        };
        self.persist_and_emit(session, expected_revision, &activity);
        Some(route)
    }

    /// Compatibility path for low-level tests and embedders that already hold the globally unique
    /// input id. Product operations use [`answer_permission`] with an explicit session.
    pub fn answer_any(&self, input_id: &str, outcome: PermissionOutcome) -> bool {
        let session = {
            let state = self.inner.state.lock().unwrap();
            state.sessions.iter().find_map(|(session, tracked)| {
                tracked
                    .turn
                    .as_ref()?
                    .pending
                    .values()
                    .any(|route| route.input.input_id == input_id)
                    .then(|| session.clone())
            })
        };
        let Some(session) = session else {
            return false;
        };
        match outcome {
            PermissionOutcome::Selected { option_id } => {
                self.answer_permission(&session, input_id, Some(&option_id))
            }
            PermissionOutcome::Cancelled => self.answer_permission(&session, input_id, None),
        }
    }

    /// Cancel every parked local request before notifying ACP. The turn remains Running until the
    /// provider supplies its terminal response.
    pub fn cancel_pending(&self, session: &str) -> bool {
        let _transition = self.inner.transitions.lock().unwrap();
        let (routes, expected_revision, activity) = {
            let mut state = self.inner.state.lock().unwrap();
            let Some(tracked) = state.sessions.get_mut(session) else {
                return false;
            };
            let Some(turn) = tracked.turn.as_mut().filter(|turn| turn.accepted) else {
                return false;
            };
            if turn.pending.is_empty() {
                return false;
            }
            let routes = std::mem::take(&mut turn.pending)
                .into_values()
                .collect::<Vec<_>>();
            let expected_revision = tracked.activity.revision;
            let activity = activity_for_turn(expected_revision.saturating_add(1), turn);
            tracked.activity = activity.clone();
            (routes, expected_revision, activity)
        };
        self.persist_and_emit(session, expected_revision, &activity);
        for route in routes {
            route.reply.cancel();
        }
        true
    }

    /// Fail only the captured provider turn. A natural completion or a newer turn makes this a
    /// no-op, so late cancellation errors cannot overwrite authoritative lifecycle progress.
    pub fn fail_provider_turn(
        &self,
        session: &str,
        turn_id: &str,
        message: impl Into<String>,
    ) -> bool {
        self.terminal(
            session,
            turn_id,
            Some((RunFailureReason::ProviderError, message.into())),
        )
    }

    fn commit_running(
        &self,
        session: &str,
        turn_id: &str,
        activity: SessionActivity,
        already_persisted: bool,
    ) -> bool {
        let _transition = self.inner.transitions.lock().unwrap();
        let expected_revision = activity.revision.saturating_sub(1);
        {
            let mut state = self.inner.state.lock().unwrap();
            let Some(tracked) = state.sessions.get_mut(session) else {
                return false;
            };
            let Some(turn) = tracked.turn.as_mut() else {
                return false;
            };
            if turn.turn_id != turn_id
                || turn.accepted
                || tracked.activity.revision != expected_revision
            {
                return false;
            }
            turn.accepted = true;
            tracked.activity = activity.clone();
        }
        if !already_persisted {
            self.persist(session, expected_revision, &activity);
        }
        self.emit_activity(session, activity);
        true
    }

    fn terminal(
        &self,
        session: &str,
        turn_id: &str,
        failed: Option<(RunFailureReason, String)>,
    ) -> bool {
        let _transition = self.inner.transitions.lock().unwrap();
        let (routes, expected_revision, activity) = {
            let mut state = self.inner.state.lock().unwrap();
            let Some(tracked) = state.sessions.get_mut(session) else {
                return false;
            };
            let Some(turn) = tracked.turn.as_ref() else {
                return false;
            };
            if turn.turn_id != turn_id || !turn.accepted {
                return false;
            }
            let turn = tracked.turn.take().expect("turn checked above");
            let routes = turn.pending.into_values().collect::<Vec<_>>();
            let expected_revision = tracked.activity.revision;
            let state = match failed {
                Some((reason, message)) => SessionRunState::Failed {
                    turn_id: Some(turn_id.to_string()),
                    reason,
                    message,
                },
                None => SessionRunState::Idle,
            };
            let activity = SessionActivity {
                revision: expected_revision.saturating_add(1),
                state,
            };
            tracked.activity = activity.clone();
            (routes, expected_revision, activity)
        };
        self.persist_and_emit(session, expected_revision, &activity);
        for route in routes {
            route.reply.cancel();
        }
        if let Some(hook) = self.inner.turn_terminal_hook.lock().unwrap().clone() {
            hook();
        }
        true
    }

    fn abandon(&self, session: &str, turn_id: &str) {
        let accepted_and_request = {
            let mut state = self.inner.state.lock().unwrap();
            let Some(tracked) = state.sessions.get_mut(session) else {
                return;
            };
            let Some(turn) = tracked.turn.as_ref() else {
                return;
            };
            if turn.turn_id != turn_id {
                return;
            }
            if turn.accepted {
                Some(turn.prompt_request_id.clone())
            } else {
                tracked.turn = None;
                None
            }
        };
        let Some(request_id) = accepted_and_request else {
            return;
        };
        if self.terminal(
            session,
            turn_id,
            Some((RunFailureReason::Interrupted, INTERRUPTED_MESSAGE.into())),
        ) {
            let _ = self.inner.events.send(Event::Error {
                session: Some(session.to_string()),
                message: INTERRUPTED_MESSAGE.into(),
                terminal: true,
                request_id,
            });
        }
    }

    fn persist_and_emit(&self, session: &str, expected_revision: u64, activity: &SessionActivity) {
        self.persist(session, expected_revision, activity);
        self.emit_activity(session, activity.clone());
    }

    fn persist(&self, session: &str, expected_revision: u64, activity: &SessionActivity) {
        let Some(store) = &self.inner.store else {
            return;
        };
        match store.update_session_activity(session, expected_revision, activity) {
            Ok(true) => {}
            Ok(false) => tracing::warn!(
                "session activity CAS rejected for {session} at revision {expected_revision}"
            ),
            Err(error) => tracing::warn!("persist session activity failed: {error}"),
        }
    }

    fn emit_activity(&self, session: &str, activity: SessionActivity) {
        let _ = self.inner.events.send(Event::SessionActivityChanged {
            session: session.to_string(),
            activity,
        });
    }
}

impl TurnLease {
    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }

    /// The exact revision that must commit beside the canonical prompt.
    pub fn prepare_running(&self) -> Option<(u64, SessionActivity)> {
        let state = self.tracker.inner.state.lock().unwrap();
        let tracked = state.sessions.get(&self.session)?;
        let turn = tracked.turn.as_ref()?;
        if turn.turn_id != self.turn_id || turn.accepted {
            return None;
        }
        let expected_revision = tracked.activity.revision;
        Some((
            expected_revision,
            SessionActivity {
                revision: expected_revision.saturating_add(1),
                state: SessionRunState::Running {
                    turn_id: self.turn_id.clone(),
                    prompt_request_id: turn.prompt_request_id.clone(),
                },
            },
        ))
    }

    pub fn commit_running(&self, activity: SessionActivity, already_persisted: bool) -> bool {
        self.tracker
            .commit_running(&self.session, &self.turn_id, activity, already_persisted)
    }

    pub fn finish_success(&self) -> bool {
        self.tracker.terminal(&self.session, &self.turn_id, None)
    }

    pub fn fail_provider(&self, message: impl Into<String>) -> bool {
        self.tracker.terminal(
            &self.session,
            &self.turn_id,
            Some((RunFailureReason::ProviderError, message.into())),
        )
    }
}

impl Drop for TurnLease {
    fn drop(&mut self) {
        self.tracker.abandon(&self.session, &self.turn_id);
    }
}

fn activity_for_turn(revision: u64, turn: &TrackedTurn) -> SessionActivity {
    let state = if turn.pending.is_empty() {
        SessionRunState::Running {
            turn_id: turn.turn_id.clone(),
            prompt_request_id: turn.prompt_request_id.clone(),
        }
    } else {
        SessionRunState::AwaitingInput {
            turn_id: turn.turn_id.clone(),
            prompt_request_id: turn.prompt_request_id.clone(),
            pending: turn
                .pending
                .values()
                .map(|route| route.input.clone())
                .collect(),
        }
    };
    SessionActivity { revision, state }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    fn running_tracker() -> (ActivityTracker, TurnLease, mpsc::UnboundedReceiver<Event>) {
        let (events, mut receiver) = mpsc::unbounded_channel();
        let tracker = ActivityTracker::new(events, None);
        let lease = tracker
            .claim("session", Some("prompt".into()), SessionActivity::default())
            .unwrap();
        let (_, activity) = lease.prepare_running().unwrap();
        assert!(lease.commit_running(activity, false));
        assert!(matches!(
            receiver.try_recv().unwrap(),
            Event::SessionActivityChanged { activity, .. }
                if activity.revision == 1
                    && matches!(activity.state, SessionRunState::Running { .. })
        ));
        (tracker, lease, receiver)
    }

    #[test]
    fn two_permissions_stay_awaiting_until_the_last_valid_answer() {
        let (tracker, lease, mut events) = running_tracker();
        let (first, first_rx) = tracker
            .park_permission(
                "session",
                "First".into(),
                vec![("allow-1".into(), "Allow".into())],
                BTreeMap::new(),
                PermissionContext::default(),
            )
            .unwrap();
        let (second, second_rx) = tracker
            .park_permission(
                "session",
                "Second".into(),
                vec![("allow-2".into(), "Allow".into())],
                BTreeMap::new(),
                PermissionContext::default(),
            )
            .unwrap();
        let awaiting = tracker.activity("session").unwrap();
        assert_eq!(awaiting.revision, 3);
        let SessionRunState::AwaitingInput { pending, .. } = awaiting.state else {
            panic!("expected awaiting input");
        };
        assert_eq!(pending.len(), 2);
        assert!(pending[0].sequence < pending[1].sequence);

        assert!(!tracker.answer_permission("wrong-session", &first, Some("allow-1")));
        assert!(!tracker.answer_permission("session", &first, Some("not-an-option")));
        assert_eq!(tracker.activity("session").unwrap().revision, 3);
        assert!(tracker.answer_permission("session", &first, Some("allow-1")));
        assert!(matches!(
            tracker.activity("session").unwrap().state,
            SessionRunState::AwaitingInput { ref pending, .. } if pending.len() == 1
        ));
        assert!(!tracker.answer_permission("session", &first, Some("allow-1")));
        assert!(tracker.answer_permission("session", &second, None));
        let running = tracker.activity("session").unwrap();
        assert_eq!(running.revision, 5);
        assert!(matches!(running.state, SessionRunState::Running { .. }));
        assert!(matches!(
            first_rx.blocking_recv().unwrap(),
            PermissionOutcome::Selected { option_id } if option_id == "allow-1"
        ));
        assert!(matches!(
            second_rx.blocking_recv().unwrap(),
            PermissionOutcome::Cancelled
        ));
        assert!(lease.finish_success());
        assert_eq!(tracker.activity("session").unwrap().revision, 6);
        assert!(matches!(
            tracker.activity("session").unwrap().state,
            SessionRunState::Idle
        ));

        let revisions = std::iter::from_fn(|| events.try_recv().ok())
            .filter_map(|event| match event {
                Event::SessionActivityChanged { activity, .. } => Some(activity.revision),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(revisions, [2, 3, 4, 5, 6]);
    }

    #[test]
    fn cancel_drains_all_pending_but_keeps_the_turn_running() {
        let (tracker, lease, _events) = running_tracker();
        let (_, first) = tracker
            .park_permission(
                "session",
                "One".into(),
                vec![],
                BTreeMap::new(),
                PermissionContext::default(),
            )
            .unwrap();
        let (_, second) = tracker
            .park_permission(
                "session",
                "Two".into(),
                vec![],
                BTreeMap::new(),
                PermissionContext::default(),
            )
            .unwrap();
        assert!(tracker.cancel_pending("session"));
        assert!(matches!(
            tracker.activity("session").unwrap().state,
            SessionRunState::Running { .. }
        ));
        assert!(matches!(
            first.blocking_recv().unwrap(),
            PermissionOutcome::Cancelled
        ));
        assert!(matches!(
            second.blocking_recv().unwrap(),
            PermissionOutcome::Cancelled
        ));
        assert!(lease.finish_success());
    }

    #[test]
    fn closed_pending_receivers_are_removed_without_reporting_success() {
        let (tracker, lease, _events) = running_tracker();
        let (permission_id, permission_rx) = tracker
            .park_permission(
                "session",
                "Permission".into(),
                vec![("allow".into(), "Allow".into())],
                BTreeMap::new(),
                PermissionContext::default(),
            )
            .unwrap();
        drop(permission_rx);

        assert!(!tracker.answer_permission("session", &permission_id, Some("allow")));
        assert!(matches!(
            tracker.activity("session").unwrap().state,
            SessionRunState::Running { .. }
        ));

        let (elicitation_id, elicitation_rx) = tracker
            .park_elicitation(
                "session",
                ElicitationForm {
                    message: "Question".into(),
                    tool_call_id: None,
                    fields: Vec::new(),
                },
            )
            .unwrap();
        drop(elicitation_rx);

        assert!(!tracker.answer_elicitation(
            "session",
            &elicitation_id,
            ElicitationAnswer::Decline,
        ));
        assert!(matches!(
            tracker.activity("session").unwrap().state,
            SessionRunState::Running { .. }
        ));
        assert!(lease.finish_success());
    }

    #[test]
    fn stale_terminal_cannot_end_a_new_generation() {
        let (tracker, first, _events) = running_tracker();
        assert!(first.finish_success());
        let second = tracker
            .claim(
                "session",
                Some("second".into()),
                tracker.activity("session").unwrap(),
            )
            .unwrap();
        let (_, running) = second.prepare_running().unwrap();
        assert!(second.commit_running(running, false));
        assert!(!first.finish_success());
        assert!(matches!(
            tracker.activity("session").unwrap().state,
            SessionRunState::Running { ref turn_id, .. } if turn_id == second.turn_id()
        ));
        assert!(second.fail_provider("provider closed"));
        assert!(matches!(
            tracker.activity("session").unwrap().state,
            SessionRunState::Failed {
                reason: RunFailureReason::ProviderError,
                ..
            }
        ));
    }

    #[test]
    fn late_cancel_failure_cannot_end_a_new_generation() {
        let (tracker, first, _events) = running_tracker();
        let first_turn_id = first.turn_id().to_string();
        assert!(first.finish_success());

        let second = tracker
            .claim(
                "session",
                Some("second".into()),
                tracker.activity("session").unwrap(),
            )
            .unwrap();
        let (_, running) = second.prepare_running().unwrap();
        assert!(second.commit_running(running, false));

        assert!(!tracker.fail_provider_turn("session", &first_turn_id, "cancel failed"));
        assert!(matches!(
            tracker.activity("session").unwrap().state,
            SessionRunState::Running { ref turn_id, .. } if turn_id == second.turn_id()
        ));
        assert!(tracker.fail_provider_turn("session", second.turn_id(), "cancel failed"));
        assert!(matches!(
            tracker.activity("session").unwrap().state,
            SessionRunState::Failed {
                ref turn_id,
                reason: RunFailureReason::ProviderError,
                ref message,
            } if turn_id.as_deref() == Some(second.turn_id()) && message == "cancel failed"
        ));
    }

    #[test]
    fn dropping_an_accepted_turn_marks_it_interrupted_and_emits_terminal_error() {
        let (tracker, lease, mut events) = running_tracker();
        drop(lease);
        let activity = tracker.activity("session").unwrap();
        assert_eq!(activity.revision, 2);
        assert!(matches!(
            activity.state,
            SessionRunState::Failed {
                reason: RunFailureReason::Interrupted,
                ..
            }
        ));
        assert!(matches!(
            events.try_recv().unwrap(),
            Event::SessionActivityChanged { .. }
        ));
        assert!(matches!(
            events.try_recv().unwrap(),
            Event::Error {
                terminal: true,
                request_id: Some(ref id),
                ..
            } if id == "prompt"
        ));
    }
}

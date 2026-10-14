//! Session sharing policy and persisted share metadata.
//!
//! One authoritative record per session: immutable creator, visibility, nonexclusive follow marks
//! and an attributed operation journal. Sessions without a record are legacy: they stay local
//! (invisible to every paired participant) and are never promoted automatically.
//!
//! Participant identity is always derived by the host from the paired device credential. Nicknames
//! are display labels only; no client-supplied actor is trusted.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Stable creator identity of the host's own local UI (normal context, not a privilege bypass).
pub const LOCAL_OWNER_ID: &str = "local-owner";
/// Bound on the retained attributed operation journal per session.
pub const MAX_JOURNAL: usize = 500;
const MAX_NICKNAME_CHARS: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    Private,
}

impl Default for Visibility {
    /// New tasks default to public (latest user decision).
    fn default() -> Self {
        Visibility::Public
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Participant {
    pub id: String,
    pub nickname: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShareOp {
    pub actor: String,
    pub nickname: String,
    pub kind: String,
    /// Delivery/request id of the dispatched command; binds the author to the real transcript
    /// entry (`TurnStarted`/`PromptQueued` carry the same id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// Durable author snapshot bound to one canonical transcript row (`seq` of the user message).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageAuthor {
    pub actor: String,
    pub nickname: String,
    /// Scoped request id the row was accepted under (stable association with the client's id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// Safe, metadata-free change notice; the engine turns it into `Event::SessionShareChanged`.
#[derive(Debug, Clone)]
pub struct ShareChange {
    pub session: String,
    /// `visibility` | `revoked` | `follow` | `message_author` | `nickname`.
    pub kind: &'static str,
    pub revision: u64,
}

pub type ShareNotifier = std::sync::Arc<dyn Fn(ShareChange) + Send + Sync>;

/// Deterministic actor-scoped id for a client-supplied request/create id from a paired device.
/// The host's local UI keeps its legacy unscoped ids. Never derived from payload identity.
pub fn scoped_request_id(actor: &Participant, client_id: &str) -> String {
    if actor.id == LOCAL_OWNER_ID {
        client_id.to_string()
    } else {
        format!("{}/{client_id}", actor.id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PendingAuthor {
    session: String,
    participant: Participant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShareRecord {
    pub creator: String,
    pub visibility: Visibility,
    /// Nonexclusive follow marks: a signature, never a lock.
    #[serde(default)]
    pub followers: BTreeSet<String>,
    #[serde(default)]
    pub journal: Vec<ShareOp>,
    /// Durable per-message authorship keyed by canonical transcript `seq`. Unbounded history
    /// (unlike the operation journal) so reopen/replay can always attach an author to its row.
    #[serde(default)]
    pub message_authors: BTreeMap<i64, MessageAuthor>,
    /// Monotonic revision for invalidation of `share.get`.
    #[serde(default)]
    pub revision: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct ShareFile {
    #[serde(default)]
    sessions: BTreeMap<String, ShareRecord>,
    #[serde(default)]
    nicknames: BTreeMap<String, String>,
    /// Accepted-but-not-yet-appended authorship intents (request id -> author); durable so a
    /// queued message keeps its author across restarts.
    #[serde(default)]
    pending_authors: BTreeMap<String, PendingAuthor>,
}

/// Single persisted source for share metadata. `path == None` keeps it in memory (tests).
pub struct ShareStore {
    path: Option<PathBuf>,
    state: Mutex<ShareFile>,
    /// Creation intents (request id -> creator) registered BEFORE a create is dispatched so the
    /// session metadata exists before its `SessionCreated` event becomes visible to anyone.
    pending: Mutex<BTreeMap<String, (Participant, Visibility)>>,
    notifier: Mutex<Option<ShareNotifier>>,
}

const MAX_PENDING_CREATES: usize = 1024;

/// One `ShareStore` per file path in this process, so the native local adapter and the remote
/// adapter at the same data directory share a single context instead of two divergent copies.
pub fn shared_store(path: impl AsRef<Path>) -> Result<std::sync::Arc<ShareStore>, String> {
    use std::collections::HashMap;
    use std::sync::{Arc, OnceLock};
    static REGISTRY: OnceLock<Mutex<HashMap<PathBuf, Arc<ShareStore>>>> = OnceLock::new();
    let path = path.as_ref().to_path_buf();
    let mut registry = REGISTRY.get_or_init(Default::default).lock().unwrap();
    if let Some(store) = registry.get(&path) {
        return Ok(store.clone());
    }
    let store = Arc::new(ShareStore::open(&path)?);
    registry.insert(path, store.clone());
    Ok(store)
}

pub fn participant_id_for_device(device_id: &str) -> String {
    if device_id.is_empty() {
        LOCAL_OWNER_ID.to_string()
    } else {
        format!("device:{device_id}")
    }
}

impl ShareStore {
    pub fn in_memory() -> Self {
        Self {
            path: None,
            state: Mutex::new(ShareFile::default()),
            pending: Mutex::new(BTreeMap::new()),
            notifier: Mutex::new(None),
        }
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let file = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| format!("share store is corrupt: {error}"))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => ShareFile::default(),
            Err(error) => return Err(format!("share store unreadable: {error}")),
        };
        Ok(Self {
            path: Some(path),
            state: Mutex::new(file),
            pending: Mutex::new(BTreeMap::new()),
            notifier: Mutex::new(None),
        })
    }

    fn persist(&self, file: &ShareFile) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let tmp = path.with_extension("tmp");
        let bytes = serde_json::to_vec_pretty(file).map_err(|error| error.to_string())?;
        std::fs::write(&tmp, bytes).map_err(|error| error.to_string())?;
        std::fs::rename(&tmp, path).map_err(|error| error.to_string())
    }

    /// Transaction boundary: every persistent mutation runs on a copy, is written to disk, and
    /// only then replaces the in-memory state. A failed mutation or write leaves memory, disk and
    /// revision exactly as before, and callers notify only after `Ok`.
    fn commit<R>(&self, mutate: impl FnOnce(&mut ShareFile) -> Result<R, String>) -> Result<R, String> {
        let mut state = self.state.lock().unwrap();
        let mut next = state.clone();
        let out = mutate(&mut next)?;
        self.persist(&next)?;
        *state = next;
        Ok(out)
    }

    /// Trusted participant for a host-derived device id. Nickname is a display label only.
    pub fn participant(&self, device_id: &str) -> Participant {
        let id = participant_id_for_device(device_id);
        let state = self.state.lock().unwrap();
        let nickname = state
            .nicknames
            .get(&id)
            .cloned()
            .unwrap_or_else(|| default_nickname(&id));
        Participant { id, nickname }
    }

    pub fn set_nickname(&self, device_id: &str, nickname: &str) -> Result<Participant, String> {
        let nickname: String = nickname.trim().chars().take(MAX_NICKNAME_CHARS).collect();
        if nickname.is_empty() {
            return Err("nickname must not be empty".into());
        }
        let id = participant_id_for_device(device_id);
        // Sessions this participant created or authored in get a revision bump so open views
        // refresh the (non-secret) display name; the notice itself carries no content and is
        // filtered per recipient by the ordinary event policy (public, or the creator's own).
        let touched = self.commit(|state| {
            state.nicknames.insert(id.clone(), nickname.clone());
            let mut touched = Vec::new();
            for (session, record) in state.sessions.iter_mut() {
                let involved = record.creator == id
                    || record.message_authors.values().any(|a| a.actor == id)
                    || record.journal.iter().any(|op| op.actor == id);
                if involved {
                    record.revision += 1;
                    touched.push((session.clone(), record.revision));
                }
            }
            Ok(touched)
        })?;
        for (session, revision) in touched {
            self.notify(&session, "nickname", revision);
        }
        Ok(Participant { id, nickname })
    }

    /// Record a new session. The creator is immutable: a second registration is ignored.
    pub fn register_session(
        &self,
        session: &str,
        creator: &Participant,
        visibility: Visibility,
    ) -> Result<ShareRecord, String> {
        self.commit(|state| {
            if let Some(existing) = state.sessions.get(session) {
                return Ok(existing.clone());
            }
            let record = ShareRecord {
                creator: creator.id.clone(),
                visibility,
                followers: BTreeSet::new(),
                journal: Vec::new(),
                message_authors: BTreeMap::new(),
                revision: 0,
            };
            state.sessions.insert(session.to_string(), record.clone());
            Ok(record)
        })
    }

    /// Declare a create before dispatch. First intent for a request id wins.
    pub fn expect_create(&self, request_id: &str, creator: &Participant, visibility: Visibility) {
        let mut pending = self.pending.lock().unwrap();
        if pending.len() >= MAX_PENDING_CREATES && !pending.contains_key(request_id) {
            if let Some(oldest) = pending.keys().next().cloned() {
                pending.remove(&oldest);
            }
        }
        pending
            .entry(request_id.to_string())
            .or_insert_with(|| (creator.clone(), visibility));
    }

    /// Register the session from a pending intent (idempotent; creator stays immutable).
    pub fn claim_created(&self, request_id: &str, session: &str) -> Result<(), String> {
        let intent = self.pending.lock().unwrap().get(request_id).cloned();
        if let Some((creator, visibility)) = intent {
            self.register_session(session, &creator, visibility)?;
            self.pending.lock().unwrap().remove(request_id);
        }
        Ok(())
    }

    pub fn has_pending(&self, request_id: &str) -> bool {
        self.pending.lock().unwrap().contains_key(request_id)
    }

    pub fn pending_creator(&self, request_id: &str) -> Option<String> {
        self.pending
            .lock()
            .unwrap()
            .get(request_id)
            .map(|(creator, _)| creator.id.clone())
    }

    pub fn drop_pending(&self, request_id: &str) {
        self.pending.lock().unwrap().remove(request_id);
    }

    /// One policy for list, direct read, command and event exits, local and remote alike.
    /// Legacy (no record) stays in the original local UI (`LOCAL_OWNER_ID`) and is never exposed
    /// to a paired device. Private is creator-only, including the host's local UI.
    pub fn can_view(&self, session: &str, actor: &Participant) -> bool {
        let state = self.state.lock().unwrap();
        match state.sessions.get(session) {
            None => actor.id == LOCAL_OWNER_ID,
            Some(record) => match record.visibility {
                Visibility::Public => true,
                Visibility::Private => record.creator == actor.id,
            },
        }
    }

    /// Canonical list-row metadata: visibility plus the creator's CURRENT nickname, read from the
    /// same store as `share.get`. `None` for legacy (unowned) sessions; callers never guess.
    /// Does not authorize: callers apply `can_view` first.
    pub fn list_summary(&self, session: &str) -> Option<(Visibility, String)> {
        let state = self.state.lock().unwrap();
        let record = state.sessions.get(session)?;
        let name = state
            .nicknames
            .get(&record.creator)
            .cloned()
            .unwrap_or_else(|| default_nickname(&record.creator));
        Some((record.visibility, name))
    }

    pub fn is_shared_session(&self, session: &str) -> bool {
        self.state.lock().unwrap().sessions.contains_key(session)
    }

    /// Read share metadata; `None` for unknown, legacy and denied sessions alike.
    pub fn read(&self, session: &str, actor: &Participant) -> Option<ShareRecord> {
        if !self.can_view(session, actor) {
            return None;
        }
        self.state.lock().unwrap().sessions.get(session).cloned()
    }

    /// Only the creator toggles visibility; this grants no other exclusive control.
    pub fn set_visibility(
        &self,
        session: &str,
        actor: &Participant,
        visibility: Visibility,
    ) -> Result<(), String> {
        let (revision, revoked) = self.commit(|state| match state.sessions.get_mut(session) {
            Some(record) if record.creator == actor.id => {
                let revoked =
                    record.visibility == Visibility::Public && visibility == Visibility::Private;
                record.visibility = visibility;
                record.revision += 1;
                Ok((record.revision, revoked))
            }
            _ => Err("session not found".to_string()),
        })?;
        self.notify(session, if revoked { "revoked" } else { "visibility" }, revision);
        Ok(())
    }

    pub fn set_notifier(&self, notifier: ShareNotifier) {
        *self.notifier.lock().unwrap() = Some(notifier);
    }

    fn notify(&self, session: &str, kind: &'static str, revision: u64) {
        let notifier = self.notifier.lock().unwrap().clone();
        if let Some(notifier) = notifier {
            notifier(ShareChange {
                session: session.to_string(),
                kind,
                revision,
            });
        }
    }

    /// Register the author of an accepted request BEFORE dispatch. Returns whether this call
    /// inserted the intent (only the inserter may roll it back); an existing intent is kept so a
    /// retry by the same actor is the same operation. Sessions without a record are not shared.
    pub fn expect_author(&self, session: &str, request_id: &str, actor: &Participant) -> bool {
        self.commit(|state| {
            if !state.sessions.contains_key(session) || state.pending_authors.contains_key(request_id)
            {
                return Err("not inserted".to_string());
            }
            state.pending_authors.insert(
                request_id.to_string(),
                PendingAuthor {
                    session: session.to_string(),
                    participant: actor.clone(),
                },
            );
            Ok(())
        })
        .is_ok()
    }

    /// Roll back an intent whose dispatch failed: no false author is left behind.
    pub fn drop_author(&self, request_id: &str) {
        let _ = self.commit(|state| {
            state
                .pending_authors
                .remove(request_id)
                .map(|_| ())
                .ok_or_else(|| "no intent".to_string())
        });
    }

    /// Bind the intent to the canonical transcript row that was actually appended. Idempotent
    /// per `seq`; called by the engine at the real append, never by text/row-index heuristics.
    pub fn record_message_author(
        &self,
        session: &str,
        seq: i64,
        request_id: &str,
    ) -> Result<(), String> {
        let revision = self.commit(|state| {
            let Some(pending) = state.pending_authors.get(request_id).cloned() else {
                return Ok(None);
            };
            if pending.session != session {
                return Ok(None);
            }
            let Some(record) = state.sessions.get_mut(session) else {
                return Ok(None);
            };
            record.message_authors.entry(seq).or_insert(MessageAuthor {
                actor: pending.participant.id.clone(),
                nickname: pending.participant.nickname.clone(),
                request_id: Some(request_id.to_string()),
            });
            record.revision += 1;
            let revision = record.revision;
            state.pending_authors.remove(request_id);
            Ok(Some(revision))
        })?;
        if let Some(revision) = revision {
            self.notify(session, "message_author", revision);
        }
        Ok(())
    }

    /// Nonexclusive follow: any number of viewers may follow; never blocks control.
    pub fn set_follow(
        &self,
        session: &str,
        actor: &Participant,
        follow: bool,
    ) -> Result<ShareRecord, String> {
        if !self.can_view(session, actor) {
            return Err("session not found".into());
        }
        let out = self.commit(|state| {
            let record = state
                .sessions
                .get_mut(session)
                .ok_or_else(|| "session not found".to_string())?;
            if follow {
                record.followers.insert(actor.id.clone());
            } else {
                record.followers.remove(&actor.id);
            }
            record.revision += 1;
            Ok(record.clone())
        })?;
        self.notify(session, "follow", out.revision);
        Ok(out)
    }

    /// Durable authors by transcript seq (policy callers must have checked `can_view`).
    pub fn message_authors(&self, session: &str) -> Option<BTreeMap<i64, MessageAuthor>> {
        let state = self.state.lock().unwrap();
        state
            .sessions
            .get(session)
            .map(|record| record.message_authors.clone())
    }

    /// Actor that already used `request_id` in this session, if journaled.
    pub fn request_owner(&self, session: &str, request_id: &str) -> Option<String> {
        let state = self.state.lock().unwrap();
        state
            .sessions
            .get(session)?
            .journal
            .iter()
            .find_map(|op| (op.request_id.as_deref() == Some(request_id)).then(|| op.actor.clone()))
    }

    /// Append a server-attributed operation (prompt, queue, steer, cancel, ...).
    pub fn record_op(
        &self,
        session: &str,
        actor: &Participant,
        kind: &str,
        request_id: Option<&str>,
    ) -> Result<(), String> {
        self.commit(|state| {
            let Some(record) = state.sessions.get_mut(session) else {
                return Ok(());
            };
            if let Some(id) = request_id {
                // A retry by the same participant is the same operation: no duplicate attribution.
                if record
                    .journal
                    .iter()
                    .any(|op| op.request_id.as_deref() == Some(id) && op.actor == actor.id)
                {
                    return Ok(());
                }
            }
            record.journal.push(ShareOp {
                actor: actor.id.clone(),
                nickname: actor.nickname.clone(),
                kind: kind.to_string(),
                request_id: request_id.map(str::to_string),
            });
            let excess = record.journal.len().saturating_sub(MAX_JOURNAL);
            record.journal.drain(..excess);
            Ok(())
        })
    }
}

fn default_nickname(id: &str) -> String {
    let tail: String = id
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if id == LOCAL_OWNER_ID {
        "Host".to_string()
    } else {
        format!("Guest-{tail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(device: &str) -> Participant {
        ShareStore::in_memory().participant(device)
    }

    #[test]
    fn default_public_creator_immutable_and_claims_nonexclusive() {
        let store = ShareStore::in_memory();
        let (a, b) = (p("a"), p("b"));
        let record = store
            .register_session("s1", &a, Visibility::default())
            .unwrap();
        assert_eq!(record.visibility, Visibility::Public);
        let again = store
            .register_session("s1", &b, Visibility::Private)
            .unwrap();
        assert_eq!(again.creator, a.id, "creator is immutable");
        store.set_follow("s1", &a, true).unwrap();
        store.set_follow("s1", &b, true).unwrap();
        let read = store.read("s1", &b).unwrap();
        assert_eq!(read.followers.len(), 2, "claims are nonexclusive");
        assert!(store.set_visibility("s1", &b, Visibility::Private).is_err());
    }

    #[test]
    fn private_is_creator_only_including_host_and_legacy_is_local() {
        let store = ShareStore::in_memory();
        let (guest, host) = (p("guest"), p(""));
        assert_eq!(host.id, LOCAL_OWNER_ID);
        store
            .register_session("priv", &guest, Visibility::Private)
            .unwrap();
        assert!(store.can_view("priv", &guest));
        assert!(!store.can_view("priv", &host), "host owner is filtered too");
        assert!(store.read("priv", &host).is_none());
        assert!(store.set_follow("priv", &host, true).is_err());
        assert!(!store.can_view("legacy-or-unknown", &guest));
        assert!(
            store.can_view("legacy-or-unknown", &host),
            "legacy stays visible in the original local UI"
        );
        assert!(
            store.read("legacy-or-unknown", &host).is_none(),
            "but is never shared"
        );
    }

    #[test]
    fn attribution_and_visibility_persist_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("share.json");
        let store = ShareStore::open(&path).unwrap();
        let a = store.set_nickname("a", "Ada").unwrap();
        store.register_session("s", &a, Visibility::Public).unwrap();
        store.record_op("s", &a, "prompt", None).unwrap();
        store.set_visibility("s", &a, Visibility::Private).unwrap();
        let reopened = ShareStore::open(&path).unwrap();
        let record = reopened.read("s", &reopened.participant("a")).unwrap();
        assert_eq!(record.visibility, Visibility::Private);
        assert_eq!(record.journal[0].nickname, "Ada");
        assert_eq!(record.journal[0].actor, "device:a");
        assert!(reopened.read("s", &reopened.participant("b")).is_none());
    }
}

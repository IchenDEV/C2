//! Unified, observation-only event ingress. Immutable inputs, stream checkpoints and per-goal
//! receipts live in side tables of the one Store; user-owned `SourceBinding`s live in
//! `AssistantState`. Recording never bumps `assistant_state.revision`.
use crate::{
    assistant::{AssistantEdit, AssistantState},
    store::{Store, StoreError},
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MAX_BATCH: usize = 16;
pub const MAX_BODY_BYTES: usize = 16 * 1024;
pub const MAX_STREAMS: usize = 8;
pub const MAX_FANOUT: usize = 30;
pub const MAX_UNFINISHED_GLOBAL: i64 = 256;
pub const MAX_UNFINISHED_SOURCE: i64 = 64;
pub const MAX_UNFINISHED_BINDING: i64 = 32;
pub const MAX_TARGET_RECEIPTS: i64 = 7_680;
pub const DEFAULT_HISTORY_GLOBAL: i64 = 10_000;
pub const DEFAULT_HISTORY_SOURCE: i64 = 2_000;
pub const MAX_RAISED_SOURCE: u32 = 20_000;
pub const MAX_RAISED_GLOBAL: u32 = 100_000;
pub const MAX_UNBOUND_META: i64 = 128;
pub const RETRY_AFTER_MS: u64 = 5_000;
const MAX_ID: usize = 256;

/// Terminal receipt states; everything else counts toward the unfinished quota.
pub const TERMINAL: &[&str] = &[
    "deferred_goal_state",
    "handled",
    "rejected",
    "filtered",
    "superseded",
    "revoked",
];
/// States a receipt may be moved to. `reviewing` is a projection of the original scoped run.
const SETTABLE: &[&str] = &[
    "recorded",
    "deferred_paused",
    "deferred_attention",
    "deferred_budget",
    "needs_configuration",
    "needs_attention",
    "invalidated",
    "unknown",
    "deferred_goal_state",
    "handled",
    "rejected",
    "superseded",
    "revoked",
];

pub(crate) fn install(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS obs_input(
            seq INTEGER PRIMARY KEY AUTOINCREMENT, observation_id TEXT NOT NULL UNIQUE,
            source_id TEXT NOT NULL, event_id TEXT NOT NULL, hash TEXT NOT NULL,
            stream_id TEXT NOT NULL, batch_id TEXT NOT NULL, status TEXT NOT NULL, reason TEXT,
            meta TEXT NOT NULL, content TEXT, binding_version INTEGER NOT NULL,
            received_at INTEGER NOT NULL, conflict_count INTEGER NOT NULL DEFAULT 0, conflict_hash TEXT,
            UNIQUE(source_id,event_id));
         CREATE TABLE IF NOT EXISTS obs_batch(
            source_id TEXT NOT NULL, stream_id TEXT NOT NULL, batch_id TEXT NOT NULL,
            req_hash TEXT NOT NULL, response TEXT NOT NULL, PRIMARY KEY(source_id,stream_id,batch_id));
         CREATE TABLE IF NOT EXISTS obs_stream(
            source_id TEXT NOT NULL, stream_id TEXT NOT NULL, checkpoint TEXT,
            PRIMARY KEY(source_id,stream_id));
         CREATE TABLE IF NOT EXISTS obs_reset(
            approval_ref TEXT PRIMARY KEY, source_id TEXT NOT NULL, binding_version INTEGER NOT NULL,
            stream_id TEXT NOT NULL, checkpoint_before TEXT, checkpoint_after TEXT,
            reason TEXT NOT NULL, approved_at INTEGER NOT NULL, consumed_at INTEGER);
         CREATE TABLE IF NOT EXISTS obs_target(
            observation_id TEXT NOT NULL, goal_id TEXT NOT NULL, source_id TEXT NOT NULL,
            binding_version INTEGER NOT NULL, project_path TEXT NOT NULL, state TEXT NOT NULL,
            run_id TEXT, output_id TEXT, updated_at INTEGER NOT NULL,
            PRIMARY KEY(observation_id,goal_id));
         CREATE INDEX IF NOT EXISTS obs_target_state ON obs_target(state);
         CREATE INDEX IF NOT EXISTS obs_target_source ON obs_target(source_id);
         CREATE TABLE IF NOT EXISTS obs_health(
            key TEXT PRIMARY KEY, kind TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'ok',
            accepted INTEGER NOT NULL DEFAULT 0, filtered INTEGER NOT NULL DEFAULT 0,
            duplicate INTEGER NOT NULL DEFAULT 0, conflicts INTEGER NOT NULL DEFAULT 0,
            rejected INTEGER NOT NULL DEFAULT 0, unbound INTEGER NOT NULL DEFAULT 0,
            gap_count INTEGER NOT NULL DEFAULT 0, gap_first TEXT, gap_last TEXT, detail TEXT);
         CREATE TABLE IF NOT EXISTS obs_unbound(
            principal_key TEXT NOT NULL, event_id TEXT NOT NULL, hash TEXT NOT NULL,
            reason TEXT NOT NULL, at INTEGER NOT NULL);",
    )?;
    Ok(())
}

fn invalid(text: &str) -> StoreError {
    StoreError::InvalidAssistant(text.into())
}

// ---------------------------------------------------------------- bindings

/// Host-verified identity: raw plugin name (keeps any `bundle:` prefix), command realm and the
/// connector id declared in the bundle manifest. Never taken from the payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    pub plugin: String,
    pub realm: String,
    pub connector_id: String,
}

impl Principal {
    pub fn key(&self) -> String {
        serde_json::json!([self.plugin, self.realm, self.connector_id]).to_string()
    }
}

/// `provider`/`account_scope` are adapter-reported and pinned by the user; they are not host
/// proof of a remote identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceBinding {
    pub source_id: String,
    pub principal: Principal,
    pub provider: String,
    pub account_scope: String,
    pub resource_filter: Vec<String>,
    pub actor_filter: Vec<String>,
    pub project_paths: Vec<String>,
    pub goal_ids: Vec<String>,
    pub streams: Vec<String>,
    pub version: u64,
    pub enabled: bool,
    /// Versioned user decision raising the per-source history quota.
    #[serde(default)]
    pub history_limit: Option<u32>,
}

/// `(source_id, version)` the Runtime pins into a claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceStamp {
    pub source_id: String,
    pub version: u64,
}

impl SourceBinding {
    pub fn stamp(&self) -> SourceStamp {
        SourceStamp {
            source_id: self.source_id.clone(),
            version: self.version,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceBindingInput {
    /// None creates (id derived from principal/provider/account); Some updates.
    pub source_id: Option<String>,
    /// Must equal the current version (0 to create). Stale edits are rejected.
    pub expected_version: u64,
    pub principal: Principal,
    pub provider: String,
    pub account_scope: String,
    #[serde(default)]
    pub resource_filter: Vec<String>,
    #[serde(default)]
    pub actor_filter: Vec<String>,
    pub project_paths: Vec<String>,
    #[serde(default)]
    pub goal_ids: Vec<String>,
    pub streams: Vec<String>,
    pub enabled: bool,
}

fn safe(text: &str) -> bool {
    !text.trim().is_empty() && text.len() <= MAX_ID && !text.chars().any(char::is_control)
}

pub(crate) fn apply_source_edit(
    state: &mut AssistantState,
    edit: AssistantEdit,
) -> Result<(), StoreError> {
    match edit {
        AssistantEdit::Source { binding: b } => {
            let settings = state
                .settings
                .as_ref()
                .ok_or_else(|| invalid("configure chief of staff projects first"))?;
            let list_ok = |v: &[String], max: usize| v.len() <= max && v.iter().all(|s| safe(s));
            if !safe(&b.provider)
                || !safe(&b.account_scope)
                || !safe(&b.principal.plugin)
                || !safe(&b.principal.realm)
                || !safe(&b.principal.connector_id)
                || b.project_paths.is_empty()
                || !list_ok(&b.project_paths, 30)
                || b.project_paths.iter().collect::<BTreeSet<_>>().len() != b.project_paths.len()
                || b.goal_ids.iter().collect::<BTreeSet<_>>().len() != b.goal_ids.len()
                || !list_ok(&b.goal_ids, MAX_FANOUT)
                || !list_ok(&b.resource_filter, 64)
                || !list_ok(&b.actor_filter, 64)
                || b.streams.is_empty()
                || !list_ok(&b.streams, MAX_STREAMS)
                || b.streams.iter().collect::<BTreeSet<_>>().len() != b.streams.len()
            {
                return Err(invalid("invalid source binding"));
            }
            if b.project_paths
                .iter()
                .any(|p| !settings.projects.contains(p))
            {
                return Err(invalid(
                    "source project is outside the chief-of-staff scope",
                ));
            }
            for id in &b.goal_ids {
                let goal = state.goals.iter().find(|g| &g.id == id);
                if !goal.is_some_and(|g| b.project_paths.contains(&g.project_path)) {
                    return Err(invalid("source goal is outside the bound projects"));
                }
            }
            let derived = format!(
                "src_{}",
                hex(&Sha256::digest(
                    serde_json::json!([b.principal, b.provider, b.account_scope])
                        .to_string()
                        .as_bytes()
                ))
            );
            let id = b.source_id.clone().unwrap_or_else(|| derived.clone());
            let existing = state.sources.iter().position(|s| s.source_id == id);
            match existing {
                None => {
                    if b.expected_version != 0 || id != derived {
                        return Err(invalid("source binding does not exist"));
                    }
                }
                Some(i) => {
                    let s = &state.sources[i];
                    if s.version != b.expected_version {
                        return Err(invalid("source binding changed; refresh first"));
                    }
                    // A different principal/account is a different source: it must be re-bound.
                    if s.principal != b.principal
                        || s.provider != b.provider
                        || s.account_scope != b.account_scope
                    {
                        return Err(invalid("principal or account changed; bind a new source"));
                    }
                }
            }
            let next = SourceBinding {
                source_id: id,
                principal: b.principal,
                provider: b.provider,
                account_scope: b.account_scope,
                resource_filter: b.resource_filter,
                actor_filter: b.actor_filter,
                project_paths: b.project_paths,
                goal_ids: b.goal_ids,
                streams: b.streams,
                version: b.expected_version + 1,
                enabled: b.enabled,
                history_limit: existing.and_then(|i| state.sources[i].history_limit),
            };
            match existing {
                Some(i) => state.sources[i] = next,
                None => {
                    if state.sources.len() >= 64 {
                        return Err(invalid("source limit reached"));
                    }
                    state.sources.push(next)
                }
            }
        }
        AssistantEdit::SourceCapacity {
            source_id,
            binding_version,
            source_history_limit,
            global_history_limit,
        } => {
            let b = state
                .sources
                .iter_mut()
                .find(|s| s.source_id == source_id)
                .ok_or_else(|| invalid("unknown source"))?;
            if b.version != binding_version {
                return Err(invalid("source binding changed; refresh first"));
            }
            if let Some(v) = source_history_limit {
                if v < DEFAULT_HISTORY_SOURCE as u32 || v > MAX_RAISED_SOURCE {
                    return Err(invalid("source history limit out of range"));
                }
                b.history_limit = Some(v);
                // Capacity does not enlarge authority or revoke already recorded inputs.
            }
            if let Some(v) = global_history_limit {
                if v < DEFAULT_HISTORY_GLOBAL as u32 || v > MAX_RAISED_GLOBAL {
                    return Err(invalid("global history limit out of range"));
                }
                state.source_history_global = Some(v);
            }
        }
        AssistantEdit::SourceReset {
            source_id,
            binding_version,
            stream_id,
            checkpoint_before,
            checkpoint_after,
            reason,
        } => {
            // Document-level validation only; the durable approval row is written by
            // `edit_source` in the same transaction as the revision-CAS save.
            let b = state
                .sources
                .iter()
                .find(|s| s.source_id == source_id)
                .ok_or_else(|| invalid("unknown source"))?;
            if b.version != binding_version || !b.enabled {
                return Err(invalid("source binding changed; refresh first"));
            }
            if !b.streams.contains(&stream_id)
                || !safe(&reason)
                || reason.len() > 512
                || checkpoint_before.as_deref().is_some_and(|c| c.len() > 4096)
                || checkpoint_after.as_deref().is_some_and(|c| c.len() > 4096)
            {
                return Err(invalid("invalid source reset"));
            }
        }
        _ => unreachable!("source edits routed by caller"),
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------- wire types

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Recovery {
    Resumable,
    LiveOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContentOrigin {
    #[default]
    Event,
    Snapshot,
    Reference,
}

impl ContentOrigin {
    fn as_str(self) -> &'static str {
        match self {
            Self::Event => "event",
            Self::Snapshot => "snapshot",
            Self::Reference => "reference",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationEvent {
    pub event_id: String,
    pub kind: String,
    pub occurred_at: String,
    pub actor_id: String,
    pub resource_id: String,
    pub object_id: String,
    #[serde(default)]
    pub object_version: Option<String>,
    #[serde(default)]
    pub reply_to: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub content_origin: ContentOrigin,
    #[serde(default)]
    pub reference: Option<String>,
}

/// Fixed field order; the SHA-256 of this serialization is the stable content hash. Host receive
/// time, transport headers and signatures are not part of it.
#[derive(Serialize)]
struct Canonical<'a> {
    event_id: &'a str,
    kind: &'a str,
    occurred_at: &'a str,
    actor_id: &'a str,
    resource_id: &'a str,
    object_id: &'a str,
    object_version: &'a Option<String>,
    reply_to: &'a Option<String>,
    content: &'a Option<String>,
    content_origin: &'a str,
    reference: &'a Option<String>,
}

impl ObservationEvent {
    pub fn canonical_json(&self) -> String {
        serde_json::to_string(&Canonical {
            event_id: &self.event_id,
            kind: &self.kind,
            occurred_at: &self.occurred_at,
            actor_id: &self.actor_id,
            resource_id: &self.resource_id,
            object_id: &self.object_id,
            object_version: &self.object_version,
            reply_to: &self.reply_to,
            content: &self.content,
            content_origin: self.content_origin.as_str(),
            reference: &self.reference,
        })
        .expect("canonical event serializes")
    }
    pub fn content_hash(&self) -> String {
        hex(&Sha256::digest(self.canonical_json().as_bytes()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetMark {
    pub approval_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordRequest {
    pub connector_id: String,
    pub account_scope: String,
    pub stream_id: String,
    pub batch_id: String,
    pub recovery: Recovery,
    #[serde(default)]
    pub checkpoint_before: Option<String>,
    #[serde(default)]
    pub checkpoint_after: Option<String>,
    pub events: Vec<ObservationEvent>,
    /// User-approved reset: still CAS on `checkpoint_before`, records the gap.
    #[serde(default)]
    pub reset: Option<ResetMark>,
    /// Provider says the cursor is invalid: no advance, health becomes needs_reset.
    #[serde(default)]
    pub cursor_invalid: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordStatus {
    Recorded,
    Noop,
    Backpressure,
    OutOfOrder,
    Unsupported,
    Rejected,
    NeedsReset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventReceipt {
    pub event_id: String,
    pub observation_id: Option<String>,
    /// recorded | duplicate | conflict | filtered | rejected
    pub status: String,
    pub reason: Option<String>,
    pub goal_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordResponse {
    pub status: RecordStatus,
    pub batch_id: String,
    pub current_checkpoint: Option<String>,
    pub event_receipts: Vec<EventReceipt>,
    pub retry_after_ms: Option<u64>,
    /// Additive machine-readable cause (needs_binding, needs_capacity, malformed, ...).
    pub reason: Option<String>,
}

impl RecordResponse {
    fn new(status: RecordStatus, req: &RecordRequest, cp: Option<String>, reason: &str) -> Self {
        Self {
            status,
            batch_id: req.batch_id.clone(),
            current_checkpoint: cp,
            event_receipts: vec![],
            retry_after_ms: (status == RecordStatus::Backpressure).then_some(RETRY_AFTER_MS),
            reason: (!reason.is_empty()).then(|| reason.to_string()),
        }
    }
}

// ---------------------------------------------------------------- read types

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub observation_id: String,
    pub seq: i64,
    pub source_id: String,
    pub stream_id: String,
    pub hash: String,
    pub binding_version: u64,
    pub received_at_ms: i64,
    pub status: String,
    pub event: ObservationEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetReceipt {
    pub observation_id: String,
    pub goal_id: String,
    pub source_id: String,
    pub binding_version: u64,
    pub project_path: String,
    pub state: String,
    pub run_id: Option<String>,
    pub output_id: Option<String>,
    pub seq: i64,
    /// Content hash of the immutable input this receipt addresses.
    pub hash: String,
    pub received_at_ms: i64,
}

/// Unused user approval of one exact cursor reset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResetApproval {
    pub approval_ref: String,
    pub source_id: String,
    pub binding_version: u64,
    pub stream_id: String,
    pub checkpoint_before: Option<String>,
    pub checkpoint_after: Option<String>,
    pub reason: String,
    pub approved_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamState {
    pub source_id: String,
    pub stream_id: String,
    pub checkpoint: Option<String>,
}

/// Body-free view of one input and all of its receipts (terminal included).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationInspection {
    pub observation_id: String,
    pub seq: i64,
    pub source_id: String,
    pub stream_id: String,
    pub hash: String,
    pub status: String,
    pub reason: Option<String>,
    pub received_at_ms: i64,
    pub conflict_count: i64,
    pub kind: String,
    pub actor_id: String,
    pub resource_id: String,
    pub object_id: String,
    pub targets: Vec<TargetReceipt>,
}

/// Pinned into `ScopedReview::observation` when a run claims inputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationStamp {
    pub observation_id: String,
    pub hash: String,
    pub source: SourceStamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationRun {
    /// Persisted on the original run; the commit allowlist follows this, never the model.
    pub external: bool,
    pub inputs: Vec<ObservationStamp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceHealth {
    pub key: String,
    pub kind: String,
    pub state: String,
    pub accepted: i64,
    pub filtered: i64,
    pub duplicate: i64,
    pub conflicts: i64,
    pub rejected: i64,
    pub unbound: i64,
    pub gap_count: i64,
    pub gap_first: Option<String>,
    pub gap_last: Option<String>,
    pub detail: Option<String>,
}

/// One receipt transition committed together with the assistant document and its outputs.
#[derive(Debug, Clone)]
pub struct ReceiptUpdate {
    pub observation_id: String,
    pub goal_id: String,
    /// Required current state (compare-and-set).
    pub from: String,
    pub to: String,
    pub run_id: Option<String>,
    /// Stable output id; retrying with the same id and target state is an idempotent replay.
    pub output_id: Option<String>,
}

// ---------------------------------------------------------------- store

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn observation_id(source: &str, event_id: &str) -> String {
    format!(
        "obs_{}",
        &hex(&Sha256::digest(format!("{source}\n{event_id}").as_bytes()))[..24]
    )
}

fn load_state(tx: &Transaction) -> Result<AssistantState, StoreError> {
    let body: Option<String> = tx
        .query_row(
            "SELECT body FROM assistant_state WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(body
        .map(|b| serde_json::from_str(&b))
        .transpose()?
        .unwrap_or_default())
}

/// Shrink-only. `unknown` (unverified attempt outcome) stays quarantined even when the
/// binding is revoked; terminal receipts and already-invalidated ones are untouched.
fn invalidate_in_tx(tx: &Transaction, state: &AssistantState) -> Result<usize, StoreError> {
    let rows: Vec<(String, String, String, i64)> = {
        let mut q = tx.prepare(
            "SELECT observation_id,goal_id,source_id,binding_version FROM obs_target
             WHERE state NOT IN ('deferred_goal_state','handled','rejected','filtered','superseded','revoked','invalidated','unknown')",
        )?;
        let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    let mut n = 0;
    for (obs, goal, source, version) in rows {
        let valid = state
            .sources
            .iter()
            .any(|s| s.source_id == source && s.enabled && s.version as i64 == version);
        if !valid {
            tx.execute(
                "UPDATE obs_target SET state='invalidated',updated_at=?3 WHERE observation_id=?1 AND goal_id=?2",
                params![obs, goal, now_ms()],
            )?;
            n += 1;
        }
    }
    Ok(n)
}

fn count(tx: &Transaction, sql: &str, p: impl rusqlite::Params) -> Result<i64, StoreError> {
    Ok(tx.query_row(sql, p, |r| r.get(0))?)
}

fn bump(tx: &Transaction, key: &str, kind: &str, column: &str, n: i64) -> Result<(), StoreError> {
    let column = match column {
        "accepted" | "filtered" | "duplicate" | "conflicts" | "rejected" | "unbound"
        | "gap_count" => column,
        _ => unreachable!(),
    };
    tx.execute(
        "INSERT OR IGNORE INTO obs_health(key,kind) VALUES(?1,?2)",
        params![key, kind],
    )?;
    tx.execute(
        &format!("UPDATE obs_health SET {column}={column}+?2 WHERE key=?1"),
        params![key, n],
    )?;
    Ok(())
}

fn set_health(tx: &Transaction, key: &str, state: &str, detail: &str) -> Result<(), StoreError> {
    tx.execute(
        "INSERT OR IGNORE INTO obs_health(key,kind) VALUES(?1,'source')",
        [key],
    )?;
    tx.execute(
        "UPDATE obs_health SET state=?2,detail=?3 WHERE key=?1",
        params![key, state, detail],
    )?;
    Ok(())
}

fn gap(tx: &Transaction, key: &str, n: i64, first: &str, last: &str) -> Result<(), StoreError> {
    bump(tx, key, "source", "gap_count", n)?;
    tx.execute(
        "UPDATE obs_health SET gap_first=COALESCE(gap_first,?2),gap_last=?3 WHERE key=?1",
        params![key, first, last],
    )?;
    Ok(())
}

enum Plan {
    New {
        status: &'static str,
        reason: Option<&'static str>,
    },
    Duplicate(String),
    Conflict(String),
}

impl Store {
    /// Host-only record. `principal` must come from the host (never from the payload).
    pub fn observation_record(
        &self,
        principal: &Principal,
        req: RecordRequest,
    ) -> Result<RecordResponse, StoreError> {
        let reject = |reason: &str| {
            Ok(RecordResponse::new(
                RecordStatus::Rejected,
                &req,
                None,
                reason,
            ))
        };
        if req.connector_id != principal.connector_id {
            return reject("principal_mismatch");
        }
        if !safe(&req.stream_id)
            || !safe(&req.batch_id)
            || !safe(&req.account_scope)
            || req
                .checkpoint_before
                .as_deref()
                .is_some_and(|c| c.len() > 4096)
            || req
                .checkpoint_after
                .as_deref()
                .is_some_and(|c| c.len() > 4096)
        {
            return reject("malformed");
        }
        if req.events.len() > MAX_BATCH {
            return reject("batch_too_large");
        }
        let mut ids = BTreeSet::new();
        // A reliable, bounded, unique event_id is the only batch-level requirement: a malformed
        // remainder becomes a terminal per-event `rejected` record so a poison event cannot
        // stall a resumable stream. Unbounded/duplicate ids reject the whole batch.
        for e in &req.events {
            if !safe(&e.event_id) || !ids.insert(&e.event_id) {
                return reject("malformed");
            }
        }
        if req.events.is_empty()
            && req.checkpoint_before == req.checkpoint_after
            && req.reset.is_none()
        {
            // Stateless empty poll: no row is written.
            let current = self.observation_checkpoint_of(principal, &req)?;
            return Ok(RecordResponse::new(RecordStatus::Noop, &req, current, ""));
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let response = record_in_tx(&tx, principal, &req)?;
        tx.commit()?;
        Ok(response)
    }

    fn observation_checkpoint_of(
        &self,
        principal: &Principal,
        req: &RecordRequest,
    ) -> Result<Option<String>, StoreError> {
        let state = self.assistant_state()?;
        let Some(b) = find_binding(&state, principal, &req.account_scope) else {
            return Ok(None);
        };
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT checkpoint FROM obs_stream WHERE source_id=?1 AND stream_id=?2",
                params![b.source_id, req.stream_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
    }

    /// Edit a binding, its capacity or approve one cursor reset with the exact-version rule.
    /// The document save (revision CAS), any durable reset approval and the stale-receipt
    /// shrink commit in ONE transaction.
    pub(crate) fn edit_source(
        &self,
        expected: u64,
        edit: AssistantEdit,
    ) -> Result<AssistantState, StoreError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut state = load_state(&tx)?;
        if state.revision != expected {
            return Err(invalid("assistant changed; refresh first"));
        }
        let reset = match &edit {
            AssistantEdit::SourceReset { .. } => Some(edit.clone()),
            _ => None,
        };
        apply_source_edit(&mut state, edit)?;
        let saved = crate::assistant::save_assistant_in_tx(&tx, expected, state)?;
        if let Some(AssistantEdit::SourceReset {
            source_id,
            binding_version,
            stream_id,
            checkpoint_before,
            checkpoint_after,
            reason,
        }) = reset
        {
            let current: Option<String> = tx
                .query_row(
                    "SELECT checkpoint FROM obs_stream WHERE source_id=?1 AND stream_id=?2",
                    params![source_id, stream_id],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            if current != checkpoint_before {
                return Err(invalid("reset must name the current checkpoint"));
            }
            let reference = format!(
                "rst_{}",
                &hex(&Sha256::digest(
                    serde_json::json!([
                        source_id,
                        binding_version,
                        stream_id,
                        checkpoint_before,
                        checkpoint_after,
                        reason,
                        saved.revision
                    ])
                    .to_string()
                    .as_bytes()
                ))[..24]
            );
            // One pending approval per stream; a newer decision replaces an unused older one.
            tx.execute(
                "DELETE FROM obs_reset WHERE source_id=?1 AND stream_id=?2 AND consumed_at IS NULL",
                params![source_id, stream_id],
            )?;
            let source_limit = saved
                .sources
                .iter()
                .find(|s| s.source_id == source_id)
                .and_then(|s| s.history_limit)
                .map_or(DEFAULT_HISTORY_SOURCE, i64::from);
            let global_limit = saved
                .source_history_global
                .map_or(DEFAULT_HISTORY_GLOBAL, i64::from);
            if count(&tx, "SELECT COUNT(*) FROM obs_reset", [])? >= global_limit
                || count(
                    &tx,
                    "SELECT COUNT(*) FROM obs_reset WHERE source_id=?1",
                    [&source_id],
                )? >= source_limit
            {
                return Err(invalid(
                    "reset history capacity reached; raise exact source capacity",
                ));
            }
            tx.execute(
                "INSERT INTO obs_reset(approval_ref,source_id,binding_version,stream_id,checkpoint_before,checkpoint_after,reason,approved_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![reference, source_id, binding_version as i64, stream_id, checkpoint_before, checkpoint_after, reason, now_ms()],
            )?;
        }
        invalidate_in_tx(&tx, &saved)?;
        tx.commit()?;
        Ok(saved)
    }

    /// Non-terminal receipts whose binding is gone, disabled or at another version become
    /// `invalidated`. Authority only ever shrinks. `unknown` is never downgraded: it is the
    /// quarantine for an attempt with an unverified outcome. Idempotent; reads state in-TX.
    pub fn observation_invalidate_stale(&self) -> Result<usize, StoreError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state = load_state(&tx)?;
        let n = invalidate_in_tx(&tx, &state)?;
        tx.commit()?;
        Ok(n)
    }

    /// Unused user-approved cursor resets (the reference an adapter must present).
    pub fn observation_resets(&self) -> Result<Vec<ResetApproval>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut q = conn.prepare(
            "SELECT approval_ref,source_id,binding_version,stream_id,checkpoint_before,checkpoint_after,reason,approved_at FROM obs_reset WHERE consumed_at IS NULL ORDER BY approved_at LIMIT 64",
        )?;
        let rows = q.query_map([], |r| {
            Ok(ResetApproval {
                approval_ref: r.get(0)?,
                source_id: r.get(1)?,
                binding_version: r.get::<_, i64>(2)? as u64,
                stream_id: r.get(3)?,
                checkpoint_before: r.get(4)?,
                checkpoint_after: r.get(5)?,
                reason: r.get(6)?,
                approved_at_ms: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Current checkpoint of every stream, for user-facing health.
    pub fn observation_streams(&self) -> Result<Vec<StreamState>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut q = conn.prepare(
            "SELECT source_id,stream_id,checkpoint FROM obs_stream ORDER BY source_id,stream_id LIMIT 1024",
        )?;
        let rows = q.query_map([], |r| {
            Ok(StreamState {
                source_id: r.get(0)?,
                stream_id: r.get(1)?,
                checkpoint: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Newest inputs first (bounded), with every receipt including terminal ones. Metadata
    /// only: bodies are never returned here.
    pub fn observation_inspection(
        &self,
        limit: usize,
    ) -> Result<Vec<ObservationInspection>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut q = conn.prepare(
            "SELECT observation_id,seq,source_id,stream_id,hash,status,reason,received_at,conflict_count,meta FROM obs_input ORDER BY seq DESC LIMIT ?1",
        )?;
        let rows: Vec<(
            String,
            i64,
            String,
            String,
            String,
            String,
            Option<String>,
            i64,
            i64,
            String,
        )> = q
            .query_map([limit.clamp(1, 200) as i64], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        let mut t = conn.prepare(
            "SELECT t.observation_id,t.goal_id,t.source_id,t.binding_version,t.project_path,t.state,t.run_id,t.output_id,i.seq,i.hash,i.received_at
             FROM obs_target t JOIN obs_input i ON i.observation_id=t.observation_id
             WHERE t.observation_id=?1 ORDER BY t.goal_id LIMIT 64",
        )?;
        let mut out = Vec::new();
        for (
            observation_id,
            seq,
            source_id,
            stream_id,
            hash,
            status,
            reason,
            received,
            conflicts,
            meta,
        ) in rows
        {
            let event: ObservationEvent = serde_json::from_str(&meta)?;
            let targets = t
                .query_map([observation_id.as_str()], target_row)?
                .collect::<Result<_, _>>()?;
            out.push(ObservationInspection {
                observation_id,
                seq,
                source_id,
                stream_id,
                hash,
                status,
                reason,
                received_at_ms: received,
                conflict_count: conflicts,
                kind: event.kind,
                actor_id: event.actor_id,
                resource_id: event.resource_id,
                object_id: event.object_id,
                targets,
            });
        }
        Ok(out)
    }

    /// Eligible receipts for the Runtime to claim, oldest first. The Runtime applies pause,
    /// attention and budget rules and may then call `observation_commit` to defer them.
    pub fn observation_pending(&self, limit: usize) -> Result<Vec<TargetReceipt>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut q = conn.prepare(&format!(
            "SELECT t.observation_id,t.goal_id,t.source_id,t.binding_version,t.project_path,t.state,t.run_id,t.output_id,i.seq,i.hash,i.received_at
             FROM obs_target t JOIN obs_input i ON i.observation_id=t.observation_id
             WHERE t.state NOT IN ('deferred_goal_state','handled','rejected','filtered','superseded','revoked')
             ORDER BY i.seq LIMIT {}",
            limit.min(1000)
        ))?;
        let rows = q.query_map([], target_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Only schedulable/deferred inputs. Unknown and attention quarantines cannot hide another
    /// source's ready work behind a bounded inspection page.
    pub fn observation_ready(&self) -> Result<Vec<TargetReceipt>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut q = conn.prepare("SELECT t.observation_id,t.goal_id,t.source_id,t.binding_version,t.project_path,t.state,t.run_id,t.output_id,i.seq,i.hash,i.received_at
            FROM obs_target t JOIN obs_input i ON i.observation_id=t.observation_id
            WHERE t.state IN ('recorded','deferred_paused','deferred_attention','deferred_budget','needs_configuration') ORDER BY i.seq LIMIT 7680")?;
        let rows = q.query_map([], target_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn observation_receipts(
        &self,
        observation_id: &str,
    ) -> Result<Vec<TargetReceipt>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut q = conn.prepare(
            "SELECT t.observation_id,t.goal_id,t.source_id,t.binding_version,t.project_path,t.state,t.run_id,t.output_id,i.seq,i.hash,i.received_at
             FROM obs_target t JOIN obs_input i ON i.observation_id=t.observation_id
             WHERE t.observation_id=?1 ORDER BY t.goal_id",
        )?;
        let rows = q.query_map([observation_id], target_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Scoped read: only inputs that have a receipt for `goal_id`, and only when the stored hash
    /// still equals the pinned stamp.
    pub fn observation_read(
        &self,
        goal_id: &str,
        stamps: &[ObservationStamp],
    ) -> Result<Vec<Observation>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut out = Vec::new();
        for stamp in stamps.iter().take(MAX_BATCH * 4) {
            let row = conn
                .query_row(
                    "SELECT i.seq,i.source_id,i.stream_id,i.hash,i.binding_version,i.received_at,i.status,i.meta,i.content
                     FROM obs_input i JOIN obs_target t ON t.observation_id=i.observation_id
                     WHERE i.observation_id=?1 AND t.goal_id=?2",
                    params![stamp.observation_id, goal_id],
                    |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, i64>(4)?,
                            r.get::<_, i64>(5)?,
                            r.get::<_, String>(6)?,
                            r.get::<_, String>(7)?,
                            r.get::<_, Option<String>>(8)?,
                        ))
                    },
                )
                .optional()?
                .ok_or_else(|| invalid("observation is not addressed to this goal"))?;
            if row.3 != stamp.hash {
                return Err(invalid("observation changed after it was pinned"));
            }
            let mut event: ObservationEvent = serde_json::from_str(&row.7)?;
            event.content = row.8;
            out.push(Observation {
                observation_id: stamp.observation_id.clone(),
                seq: row.0,
                source_id: row.1,
                stream_id: row.2,
                hash: row.3,
                binding_version: row.4 as u64,
                received_at_ms: row.5,
                status: row.6,
                event,
            });
        }
        Ok(out)
    }

    pub fn observation_health(&self) -> Result<Vec<SourceHealth>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut q = conn.prepare(
            "SELECT key,kind,state,accepted,filtered,duplicate,conflicts,rejected,unbound,gap_count,gap_first,gap_last,detail FROM obs_health ORDER BY key",
        )?;
        let rows = q.query_map([], |r| {
            Ok(SourceHealth {
                key: r.get(0)?,
                kind: r.get(1)?,
                state: r.get(2)?,
                accepted: r.get(3)?,
                filtered: r.get(4)?,
                duplicate: r.get(5)?,
                conflicts: r.get(6)?,
                rejected: r.get(7)?,
                unbound: r.get(8)?,
                gap_count: r.get(9)?,
                gap_first: r.get(10)?,
                gap_last: r.get(11)?,
                detail: r.get(12)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// One oldest reference per source/state for bounded user notices. This is a projection of
    /// receipts, not another queue, and includes quarantined inputs behind schedulable work.
    pub fn observation_notices(&self) -> Result<Vec<(String, String, String, i64)>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut q = conn.prepare(
            "SELECT t.source_id,t.state,i.observation_id,COUNT(*),MIN(i.seq)
             FROM obs_target t JOIN obs_input i ON i.observation_id=t.observation_id
             WHERE t.state IN ('needs_attention','needs_configuration','deferred_paused',
                 'deferred_attention','deferred_budget','deferred_goal_state','unknown')
             GROUP BY t.source_id,t.state ORDER BY t.source_id,t.state LIMIT 512",
        )?;
        let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The single atomic path: optionally save the assistant document (revision CAS) and move
    /// receipts in ONE transaction. Any rejected update rolls everything back. Moving a receipt
    /// to a non-shrinking state requires the saved state's binding to be enabled at the
    /// receipt's pinned version and the goal's project inside the binding and settings.
    pub fn observation_commit(
        &self,
        assistant: Option<(u64, AssistantState)>,
        updates: &[ReceiptUpdate],
    ) -> Result<Option<AssistantState>, StoreError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let saved = match assistant {
            Some((expected, state)) => Some(crate::assistant::save_assistant_in_tx(
                &tx, expected, state,
            )?),
            None => None,
        };
        let state = match &saved {
            Some(s) => s.clone(),
            None => {
                let body: Option<String> = tx
                    .query_row(
                        "SELECT body FROM assistant_state WHERE singleton=1",
                        [],
                        |r| r.get(0),
                    )
                    .optional()?;
                body.map(|b| serde_json::from_str(&b))
                    .transpose()?
                    .unwrap_or_default()
            }
        };
        for u in updates {
            if !SETTABLE.contains(&u.to.as_str()) {
                return Err(invalid("unknown receipt state"));
            }
            let row: Option<(String, Option<String>, String, i64, String, String)> = tx
                .query_row(
                    "SELECT state,output_id,source_id,binding_version,project_path,goal_id FROM obs_target WHERE observation_id=?1 AND goal_id=?2",
                    params![u.observation_id, u.goal_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
                )
                .optional()?;
            let (cur, out, source, version, project, goal_id_ok) =
                row.ok_or_else(|| invalid("unknown observation receipt"))?;
            if cur == u.to && out == u.output_id {
                continue; // retry of the same committed result
            }
            if cur == "unknown" && u.to != "unknown" {
                return Err(invalid("unknown result cannot be replayed or downgraded"));
            }
            if cur != u.from || TERMINAL.contains(&cur.as_str()) {
                return Err(invalid("receipt changed; reload before committing"));
            }
            // Only `recorded`/`handled` use authority. Every other state (deferred_*,
            // needs_*, invalidated, unknown, ...) narrows it and is accepted even when the
            // binding or project has since been removed.
            if matches!(u.to.as_str(), "recorded" | "handled") {
                let ok = state.sources.iter().any(|s| {
                    s.source_id == source
                        && s.enabled
                        && s.version as i64 == version
                        && s.project_paths.contains(&project)
                        && (s.goal_ids.is_empty() || s.goal_ids.contains(&u.goal_id))
                }) && state
                    .settings
                    .as_ref()
                    .is_some_and(|c| c.projects.contains(&project));
                if !ok {
                    return Err(invalid(
                        "source binding or scope changed; receipt invalidated",
                    ));
                }
                if !goal_id_ok.is_empty()
                    && !state
                        .goals
                        .iter()
                        .any(|g| g.id == goal_id_ok && g.project_path == project)
                {
                    return Err(invalid("goal is outside the pinned scope"));
                }
            }
            if u.to == "handled" {
                let actual_hash: String = tx.query_row(
                    "SELECT hash FROM obs_input WHERE observation_id=?1",
                    [&u.observation_id],
                    |r| r.get(0),
                )?;
                let pinned = state
                    .scoped_runs
                    .iter()
                    .find(|r| {
                        Some(&r.run.id) == u.run_id.as_ref()
                            && r.scope == u.goal_id
                            && r.state == "completed"
                    })
                    .and_then(|r| r.observation.as_ref())
                    .is_some_and(|o| {
                        o.inputs
                            .iter()
                            .any(|i| i.observation_id == u.observation_id && i.hash == actual_hash)
                    });
                if !pinned {
                    return Err(invalid(
                        "handled needs the original completed run and content hash",
                    ));
                }
                handled_is_backed(&state, u, &source, version)?;
            }
            tx.execute(
                "UPDATE obs_target SET state=?3,run_id=COALESCE(?4,run_id),output_id=COALESCE(?5,output_id),updated_at=?6 WHERE observation_id=?1 AND goal_id=?2",
                params![u.observation_id, u.goal_id, u.to, u.run_id, u.output_id, now_ms()],
            )?;
        }
        tx.commit()?;
        Ok(saved)
    }
}

/// A receipt may become `handled` only when the saved assistant document itself holds the
/// persisted external run, its pinned input ref and the stable output that reports it, so a
/// receipt can never be handled silently (for example when the conversation is full).
fn handled_is_backed(
    state: &AssistantState,
    u: &ReceiptUpdate,
    source: &str,
    version: i64,
) -> Result<(), StoreError> {
    let (Some(run_id), Some(output_id)) = (&u.run_id, &u.output_id) else {
        return Err(invalid("handled needs a run and a stable output"));
    };
    let run_ok = state.scoped_runs.iter().any(|r| {
        &r.run.id == run_id
            && r.observation.as_ref().is_some_and(|o| {
                o.external
                    && o.inputs.iter().any(|i| {
                        i.observation_id == u.observation_id
                            && i.source.source_id == source
                            && i.source.version as i64 == version
                    })
            })
    });
    let output_ok = output_id == &format!("obs-out:{run_id}")
        && state
            .conversation
            .iter()
            .any(|t| &t.id == output_id && t.source_run_id.as_ref() == Some(run_id));
    if !run_ok || !output_ok {
        return Err(invalid(
            "handled needs the persisted external run, its input refs and its output",
        ));
    }
    Ok(())
}

fn target_row(r: &rusqlite::Row) -> rusqlite::Result<TargetReceipt> {
    Ok(TargetReceipt {
        observation_id: r.get(0)?,
        goal_id: r.get(1)?,
        source_id: r.get(2)?,
        binding_version: r.get::<_, i64>(3)? as u64,
        project_path: r.get(4)?,
        state: r.get(5)?,
        run_id: r.get(6)?,
        output_id: r.get(7)?,
        seq: r.get(8)?,
        hash: r.get(9)?,
        received_at_ms: r.get(10)?,
    })
}

/// Fields other than `event_id` that make an event a terminal `rejected` poison record.
fn event_malformed(e: &ObservationEvent) -> bool {
    !safe(&e.object_id)
        || !safe(&e.kind)
        || e.occurred_at.len() > MAX_ID
        || e.actor_id.len() > MAX_ID
        || e.resource_id.len() > MAX_ID
        || e.reference.as_deref().is_some_and(|r| r.len() > 2048)
        || e.object_version
            .as_deref()
            .is_some_and(|r| r.len() > MAX_ID)
        || e.reply_to.as_deref().is_some_and(|r| r.len() > MAX_ID)
        || (e.content_origin == ContentOrigin::Reference && e.content.is_some())
}

/// Metadata kept for a malformed event: the reliable id only, never the oversize remainder.
fn poison_meta(e: &ObservationEvent) -> ObservationEvent {
    ObservationEvent {
        event_id: e.event_id.clone(),
        kind: String::new(),
        occurred_at: String::new(),
        actor_id: String::new(),
        resource_id: String::new(),
        object_id: String::new(),
        object_version: None,
        reply_to: None,
        content: None,
        content_origin: ContentOrigin::Event,
        reference: None,
    }
}

const TERMINAL_SQL: &str =
    "'deferred_goal_state','handled','rejected','filtered','superseded','revoked'";

fn target_goals(tx: &Transaction, observation_id: &str) -> Result<Vec<String>, StoreError> {
    let mut q =
        tx.prepare("SELECT goal_id FROM obs_target WHERE observation_id=?1 ORDER BY goal_id")?;
    let rows = q.query_map([observation_id], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Move unresolved targets of an input to `to`, never touching terminal or `unknown` ones.
fn move_unresolved(tx: &Transaction, observation_id: &str, to: &str) -> Result<usize, StoreError> {
    let extra = if to == "superseded" {
        ""
    } else {
        ",'needs_attention','invalidated','needs_configuration'"
    };
    Ok(tx.execute(
        &format!("UPDATE obs_target SET state=?2,updated_at=?3 WHERE observation_id=?1 AND state NOT IN ({TERMINAL_SQL},'unknown'{extra})"),
        params![observation_id, to, now_ms()],
    )?)
}

/// Same-object update/withdrawal. Only an explicit `reply_to` naming the exact prior event with
/// a known `message.updated`/`message.deleted` kind may supersede it. Any other relation is an
/// opaque version: never sorted lexically nor by receive time, everything unresolved needs
/// attention. Returns whether the new event's own targets must start as `needs_attention`.
fn relate_same_object(
    tx: &Transaction,
    source: &str,
    e: &ObservationEvent,
) -> Result<bool, StoreError> {
    let known_kind = matches!(e.kind.as_str(), "message.updated" | "message.deleted");
    if !known_kind && e.object_version.is_none() {
        return Ok(false);
    }
    let priors: Vec<(String, String)> = {
        let mut q = tx.prepare(
            "SELECT observation_id,event_id FROM obs_input WHERE source_id=?1 AND status='accepted' AND event_id<>?2 AND json_extract(meta,'$.object_id')=?3 AND json_extract(meta,'$.resource_id')=?4 LIMIT 64",
        )?;
        let rows = q.query_map(
            params![source, e.event_id, e.object_id, e.resource_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        rows.collect::<Result<_, _>>()?
    };
    if priors.is_empty() {
        return Ok(false);
    }
    let causal = known_kind
        && e.reply_to
            .as_deref()
            .is_some_and(|r| priors.iter().any(|(_, ev)| ev == r));
    let mut attention = !causal;
    for (obs, ev) in priors {
        if causal && Some(ev.as_str()) == e.reply_to.as_deref() {
            tx.execute("UPDATE obs_target SET state='superseded',updated_at=?2 WHERE observation_id=?1 AND state NOT IN ('unknown','rejected','filtered','revoked')", params![obs, now_ms()])?;
        } else if move_unresolved(tx, &obs, "needs_attention")? > 0 {
            attention = true;
        }
    }
    Ok(attention)
}

fn find_binding<'a>(
    state: &'a AssistantState,
    principal: &Principal,
    account_scope: &str,
) -> Option<&'a crate::assistant_observation::SourceBinding> {
    state
        .sources
        .iter()
        .find(|s| &s.principal == principal && s.account_scope == account_scope)
}

fn batch_hash(source: &str, req: &RecordRequest, hashes: &[String]) -> String {
    let v = serde_json::json!([
        source,
        req.stream_id,
        req.batch_id,
        req.recovery,
        req.checkpoint_before,
        req.checkpoint_after,
        req.reset.as_ref().map(|r| &r.approval_ref),
        req.cursor_invalid,
        hashes
    ]);
    hex(&Sha256::digest(v.to_string().as_bytes()))
}

fn record_in_tx(
    tx: &Transaction,
    principal: &Principal,
    req: &RecordRequest,
) -> Result<RecordResponse, StoreError> {
    let body: Option<String> = tx
        .query_row(
            "SELECT body FROM assistant_state WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let state: AssistantState = body
        .map(|b| serde_json::from_str(&b))
        .transpose()?
        .unwrap_or_default();
    let hashes: Vec<String> = req.events.iter().map(|e| e.content_hash()).collect();
    let binding = find_binding(&state, principal, &req.account_scope).cloned();
    let Some(binding) = binding.filter(|b| b.enabled) else {
        // Unbound or disabled: count, keep bounded id/hash/reason metadata, never the body.
        let key = format!("principal:{}", principal.key());
        bump(tx, &key, "principal", "unbound", req.events.len() as i64)?;
        set_health(
            tx,
            &key,
            "needs_binding",
            "bind this connector to project scope",
        )?;
        let mut kept = count(tx, "SELECT COUNT(*) FROM obs_unbound", [])?;
        for (e, h) in req.events.iter().zip(&hashes) {
            if kept >= MAX_UNBOUND_META {
                break;
            }
            tx.execute(
                "INSERT INTO obs_unbound(principal_key,event_id,hash,reason,at) VALUES(?1,?2,?3,'needs_binding',?4)",
                params![principal.key(), e.event_id, h, now_ms()],
            )?;
            kept += 1;
        }
        return Ok(RecordResponse::new(
            RecordStatus::Rejected,
            req,
            None,
            "needs_binding",
        ));
    };
    let source = binding.source_id.clone();
    if !binding.streams.contains(&req.stream_id) {
        return Ok(RecordResponse::new(
            RecordStatus::Rejected,
            req,
            None,
            "unknown_stream",
        ));
    }
    let current: Option<String> = tx
        .query_row(
            "SELECT checkpoint FROM obs_stream WHERE source_id=?1 AND stream_id=?2",
            params![source, req.stream_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    // Same batch id: replay the original result, even after the checkpoint moved on.
    let req_hash = batch_hash(&source, req, &hashes);
    let prior: Option<(String, String)> = tx
        .query_row(
            "SELECT req_hash,response FROM obs_batch WHERE source_id=?1 AND stream_id=?2 AND batch_id=?3",
            params![source, req.stream_id, req.batch_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((h, response)) = prior {
        if h != req_hash {
            return Ok(RecordResponse::new(
                RecordStatus::Rejected,
                req,
                current,
                "batch_conflict",
            ));
        }
        let mut r: RecordResponse = serde_json::from_str(&response)?;
        r.current_checkpoint = current;
        return Ok(r);
    }
    if let Some(reason) = req.cursor_invalid.as_ref().filter(|_| req.reset.is_none()) {
        set_health(tx, &source, "needs_reset", reason)?;
        return Ok(RecordResponse::new(
            RecordStatus::NeedsReset,
            req,
            current,
            reason,
        ));
    }
    if req.checkpoint_before != current {
        return Ok(RecordResponse::new(
            RecordStatus::OutOfOrder,
            req,
            current,
            "",
        ));
    }
    // A reset is valid only against a user-approved row for exactly this source, binding
    // version, stream and checkpoint pair. The adapter's own claim carries no authority.
    let mut reset_reason = None;
    if let Some(mark) = &req.reset {
        if !safe(&mark.approval_ref) {
            return Ok(RecordResponse::new(
                RecordStatus::Rejected,
                req,
                current,
                "malformed",
            ));
        }
        let approved: Option<String> = tx
            .query_row(
                "SELECT reason FROM obs_reset WHERE approval_ref=?1 AND source_id=?2 AND binding_version=?3 AND stream_id=?4 AND checkpoint_before IS ?5 AND checkpoint_after IS ?6 AND consumed_at IS NULL",
                params![mark.approval_ref, source, binding.version as i64, req.stream_id, req.checkpoint_before, req.checkpoint_after],
                |r| r.get(0),
            )
            .optional()?;
        match approved {
            Some(reason) => reset_reason = Some(reason),
            None => {
                return Ok(RecordResponse::new(
                    RecordStatus::Rejected,
                    req,
                    current,
                    "reset_not_approved",
                ))
            }
        }
    }

    // Classify first (reads only) so capacity is checked before any event write.
    let mut plans = Vec::new();
    for (e, h) in req.events.iter().zip(&hashes) {
        let stored: Option<String> = tx
            .query_row(
                "SELECT hash FROM obs_input WHERE source_id=?1 AND event_id=?2",
                params![source, e.event_id],
                |r| r.get(0),
            )
            .optional()?;
        plans.push(match stored {
            Some(s) if &s == h => Plan::Duplicate(s),
            Some(s) => Plan::Conflict(s),
            None if event_malformed(e) => Plan::New {
                status: "rejected",
                reason: Some("malformed"),
            },
            None if e.content.as_ref().is_some_and(|c| c.len() > MAX_BODY_BYTES) => Plan::New {
                status: "rejected",
                reason: Some("body_too_large"),
            },
            None if (!binding.resource_filter.is_empty()
                && !binding.resource_filter.contains(&e.resource_id))
                || (!binding.actor_filter.is_empty()
                    && !binding.actor_filter.contains(&e.actor_id)) =>
            {
                Plan::New {
                    status: "filtered",
                    reason: Some("filtered"),
                }
            }
            None => Plan::New {
                status: "accepted",
                reason: None,
            },
        });
    }
    let new_rows = plans
        .iter()
        .filter(|p| matches!(p, Plan::New { .. }))
        .count() as i64;
    let new_live = plans
        .iter()
        .filter(|p| {
            matches!(
                p,
                Plan::New {
                    status: "accepted",
                    ..
                }
            )
        })
        .count() as i64;
    // Targets per live event: bound goals that still exist (fan-out capped), else one
    // needs_attention placeholder so ambiguity is visible rather than dropped.
    let mut seen_goals = BTreeSet::new();
    let goals: Vec<(String, String)> = binding
        .goal_ids
        .iter()
        .filter(|id| seen_goals.insert(id.as_str()))
        .filter_map(|id| state.goals.iter().find(|g| &g.id == id))
        // Revalidate: the goal's project must still be inside the binding and the settings.
        .filter(|g| {
            binding.project_paths.contains(&g.project_path)
                && state
                    .settings
                    .as_ref()
                    .is_some_and(|s| s.projects.contains(&g.project_path))
        })
        .take(MAX_FANOUT)
        .map(|g| (g.id.clone(), g.project_path.clone()))
        .collect();
    let per_event = goals.len().max(1) as i64;

    let hist_global = state
        .source_history_global
        .map_or(DEFAULT_HISTORY_GLOBAL, i64::from);
    let hist_source = binding
        .history_limit
        .map_or(DEFAULT_HISTORY_SOURCE, i64::from);
    let inputs_global = count(tx, "SELECT COUNT(*) FROM obs_input", [])?;
    let inputs_source = count(
        tx,
        "SELECT COUNT(*) FROM obs_input WHERE source_id=?1",
        [&source],
    )?;
    let batches_global = count(tx, "SELECT COUNT(*) FROM obs_batch", [])?;
    let batches_source = count(
        tx,
        "SELECT COUNT(*) FROM obs_batch WHERE source_id=?1",
        [&source],
    )?;
    if inputs_global + new_rows > hist_global
        || inputs_source + new_rows > hist_source
        || batches_global + 1 > hist_global
        || batches_source + 1 > hist_source
    {
        let detail = format!(
            "source {source}: inputs {inputs_source}/{hist_source}, batches {batches_source}/{hist_source}; global inputs {inputs_global}/{hist_global}. Raise the limit with the current binding version {} to continue.",
            binding.version
        );
        if req.recovery == Recovery::LiveOnly {
            let first = req.events.first().map_or("", |e| e.event_id.as_str());
            let last = req.events.last().map_or("", |e| e.event_id.as_str());
            gap(tx, &source, req.events.len() as i64, first, last)?;
        }
        set_health(tx, &source, "needs_capacity", &detail)?;
        return Ok(RecordResponse::new(
            RecordStatus::Backpressure,
            req,
            current,
            "needs_capacity",
        ));
    }
    let unfinished = "SELECT COUNT(DISTINCT observation_id) FROM obs_target WHERE state NOT IN ('deferred_goal_state','handled','rejected','filtered','superseded','revoked')";
    let u_global = count(tx, unfinished, [])?;
    let u_source = count(tx, &format!("{unfinished} AND source_id=?1"), [&source])?;
    let u_binding = count(
        tx,
        &format!("{unfinished} AND source_id=?1 AND binding_version=?2"),
        params![source, binding.version as i64],
    )?;
    let targets_total = count(tx, "SELECT COUNT(*) FROM obs_target", [])?;
    if u_global + new_live > MAX_UNFINISHED_GLOBAL
        || u_source + new_live > MAX_UNFINISHED_SOURCE
        || u_binding + new_live > MAX_UNFINISHED_BINDING
        || targets_total + new_live * per_event > MAX_TARGET_RECEIPTS
    {
        if req.recovery == Recovery::LiveOnly {
            let first = req.events.first().map_or("", |e| e.event_id.as_str());
            let last = req.events.last().map_or("", |e| e.event_id.as_str());
            gap(tx, &source, req.events.len() as i64, first, last)?;
        }
        set_health(
            tx,
            &source,
            "backpressure",
            "unfinished observation quota reached",
        )?;
        return Ok(RecordResponse::new(
            RecordStatus::Backpressure,
            req,
            current,
            "unfinished_quota",
        ));
    }

    let at = now_ms();
    let mut receipts = Vec::new();
    let (mut accepted, mut filtered, mut duplicate, mut conflicts, mut rejected) = (0, 0, 0, 0, 0);
    for ((e, h), plan) in req.events.iter().zip(&hashes).zip(plans) {
        let id = observation_id(&source, &e.event_id);
        let mut receipt = EventReceipt {
            event_id: e.event_id.clone(),
            observation_id: Some(id.clone()),
            status: "recorded".into(),
            reason: None,
            goal_ids: vec![],
        };
        match plan {
            Plan::Duplicate(_) => {
                duplicate += 1;
                receipt.status = "duplicate".into();
                receipt.goal_ids = target_goals(tx, &id)?;
            }
            Plan::Conflict(_) => {
                conflicts += 1;
                tx.execute(
                    "UPDATE obs_input SET conflict_count=conflict_count+1,conflict_hash=?3 WHERE source_id=?1 AND event_id=?2",
                    params![source, e.event_id, h],
                )?;
                // The competing content is never adopted; the unresolved original is quarantined.
                move_unresolved(tx, &id, "needs_attention")?;
                receipt.goal_ids = target_goals(tx, &id)?;
                receipt.status = "conflict".into();
                receipt.reason = Some("different_content_for_same_event".into());
            }
            Plan::New { status, reason } => {
                let mut meta = if reason == Some("malformed") {
                    poison_meta(e)
                } else {
                    e.clone()
                };
                let keep = status == "accepted";
                let content = if keep { meta.content.take() } else { None };
                meta.content = None;
                tx.execute(
                    "INSERT INTO obs_input(observation_id,source_id,event_id,hash,stream_id,batch_id,status,reason,meta,content,binding_version,received_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                    params![id, source, e.event_id, h, req.stream_id, req.batch_id, status, reason, serde_json::to_string(&meta)?, content, binding.version as i64, at],
                )?;
                match status {
                    "filtered" => {
                        filtered += 1;
                        receipt.status = "filtered".into();
                    }
                    "rejected" => {
                        rejected += 1;
                        receipt.status = "rejected".into();
                    }
                    _ => {
                        accepted += 1;
                        let attention = relate_same_object(tx, &source, e)?;
                        let targets = if goals.is_empty() {
                            vec![(
                                String::new(),
                                binding.project_paths[0].clone(),
                                "needs_attention",
                            )]
                        } else {
                            goals
                                .iter()
                                .map(|(g, p)| {
                                    (
                                        g.clone(),
                                        p.clone(),
                                        if attention {
                                            "needs_attention"
                                        } else {
                                            "recorded"
                                        },
                                    )
                                })
                                .collect()
                        };
                        for (goal, project, st) in targets {
                            tx.execute(
                                "INSERT INTO obs_target(observation_id,goal_id,source_id,binding_version,project_path,state,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                                params![id, goal, source, binding.version as i64, project, st, at],
                            )?;
                            receipt.goal_ids.push(goal);
                        }
                    }
                }
                receipt.reason = reason.map(str::to_string);
            }
        }
        // The first receipt and later duplicate receipts use the same target order.
        receipt.goal_ids.sort();
        receipts.push(receipt);
    }
    for (column, n) in [
        ("accepted", accepted),
        ("filtered", filtered),
        ("duplicate", duplicate),
        ("conflicts", conflicts),
        ("rejected", rejected),
    ] {
        if n > 0 {
            bump(tx, &source, "source", column, n)?;
        }
    }
    if req.reset.is_some() {
        gap(
            tx,
            &source,
            1,
            req.checkpoint_before.as_deref().unwrap_or(""),
            req.checkpoint_after.as_deref().unwrap_or(""),
        )?;
    }
    set_health(tx, &source, "ok", reset_reason.as_deref().unwrap_or(""))?;
    if let Some(mark) = &req.reset {
        // Consumed in the same transaction as the cursor commit: one approval, one reset.
        tx.execute(
            "UPDATE obs_reset SET consumed_at=?2 WHERE approval_ref=?1",
            params![mark.approval_ref, now_ms()],
        )?;
    }
    tx.execute(
        "INSERT INTO obs_stream(source_id,stream_id,checkpoint) VALUES(?1,?2,?3) ON CONFLICT(source_id,stream_id) DO UPDATE SET checkpoint=excluded.checkpoint",
        params![source, req.stream_id, req.checkpoint_after],
    )?;
    let response = RecordResponse {
        status: RecordStatus::Recorded,
        batch_id: req.batch_id.clone(),
        current_checkpoint: req.checkpoint_after.clone(),
        event_receipts: receipts,
        retry_after_ms: None,
        reason: None,
    };
    tx.execute(
        "INSERT INTO obs_batch(source_id,stream_id,batch_id,req_hash,response) VALUES(?1,?2,?3,?4,?5)",
        params![source, req.stream_id, req.batch_id, req_hash, serde_json::to_string(&response)?],
    )?;
    Ok(response)
}

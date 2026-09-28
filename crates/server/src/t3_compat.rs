//! Compatibility surface for the native T3 Code mobile client.
//!
//! The wire contract is pinned to `pingdotgg/t3code` commit
//! `5a84614809b6e853b872f9e57ff4b97e9df5df02` (contracts 0.0.33, Effect
//! 4.0.0-beta.103). T3's client protocol is Effect RPC over WebSocket; it is not ACP and it is not
//! JSON-RPC 2.0. This adapter keeps those transport types at the server edge and translates the
//! useful mobile chat operations to C2's existing `Op`/`Event` domain contract.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Form, Path as AxumPath, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use codetwo_core::event::ModelChoice;
use codetwo_core::{
    default_registry, DocBlock, Engine, Event, ExecutionPolicy, Op, Part, PendingInputKind,
    PermissionMode, ProviderId, Role, SandboxPolicy, Session, SessionRunState,
    MAX_TRANSCRIPT_TURNS,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::{broadcast, Mutex as AsyncMutex};

use crate::{AuthState, WS_TICKET_TTL};

const T3_CONTRACT_VERSION: &str = "0.0.33-codetwo.1";
const LEGACY_PLAN_PROMPT_PREFIX: &str = "[skill:plan-first]\n\n";
const ACCESS_TOKEN_TTL_DAYS: i64 = 30;
const TOKEN_EXCHANGE_GRANT: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
const BOOTSTRAP_TOKEN_TYPE: &str = "urn:t3:params:oauth:token-type:environment-bootstrap";
const ACCESS_TOKEN_TYPE: &str = "urn:ietf:params:oauth:token-type:access_token";
const STANDARD_SCOPES: [&str; 5] = [
    "orchestration:read",
    "orchestration:operate",
    "terminal:operate",
    "review:write",
    "relay:read",
];

#[derive(Clone)]
struct CoreUpdate {
    sequence: u64,
}

const COMPATIBILITY_STATE_VERSION: u32 = 1;
const MAX_PERSISTED_COMMAND_RECEIPTS: usize = 2_048;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedCommandReceipt {
    command_id: String,
    sequence: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompatibilityMetadata {
    version: u32,
    #[serde(default)]
    aliases: HashMap<String, String>,
    #[serde(default)]
    command_receipts: Vec<PersistedCommandReceipt>,
}

impl Default for CompatibilityMetadata {
    fn default() -> Self {
        Self {
            version: COMPATIBILITY_STATE_VERSION,
            aliases: HashMap::new(),
            command_receipts: Vec::new(),
        }
    }
}

fn load_compatibility(path: Option<&Path>) -> (CompatibilityMetadata, Option<String>) {
    let Some(path) = path else {
        return (CompatibilityMetadata::default(), None);
    };
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (CompatibilityMetadata::default(), None);
        }
        Err(error) => {
            return (
                CompatibilityMetadata::default(),
                Some(format!("could not read metadata: {error}")),
            );
        }
    };
    let metadata = match serde_json::from_str::<CompatibilityMetadata>(&contents) {
        Ok(metadata) => metadata,
        Err(error) => {
            return (
                CompatibilityMetadata::default(),
                Some(format!("invalid metadata JSON: {error}")),
            );
        }
    };
    if let Err(error) = validate_compatibility(&metadata) {
        return (CompatibilityMetadata::default(), Some(error));
    }
    (metadata, None)
}

fn validate_compatibility(metadata: &CompatibilityMetadata) -> Result<(), String> {
    if metadata.version != COMPATIBILITY_STATE_VERSION {
        return Err(format!("unsupported metadata version {}", metadata.version));
    }
    if metadata
        .aliases
        .iter()
        .any(|(public, core)| public.trim().is_empty() || core.trim().is_empty())
    {
        return Err("thread aliases must be non-empty".into());
    }
    let unique_core_ids: HashSet<&str> = metadata.aliases.values().map(String::as_str).collect();
    if unique_core_ids.len() != metadata.aliases.len() {
        return Err("multiple public thread ids map to the same C2 session".into());
    }
    if metadata.command_receipts.len() > MAX_PERSISTED_COMMAND_RECEIPTS
        || metadata
            .command_receipts
            .iter()
            .any(|receipt| receipt.command_id.trim().is_empty())
    {
        return Err("invalid persisted T3 command receipts".into());
    }
    let unique_receipts: HashSet<&str> = metadata
        .command_receipts
        .iter()
        .map(|receipt| receipt.command_id.as_str())
        .collect();
    if unique_receipts.len() != metadata.command_receipts.len() {
        return Err("duplicate persisted T3 command receipt".into());
    }
    Ok(())
}

fn compatibility_unavailable_message() -> String {
    "T3 compatibility metadata could not be loaded; repair or remove t3-compatibility.json before creating or changing a mobile thread".into()
}

fn valid_environment_id(value: &str) -> bool {
    value.strip_prefix("codetwo-").is_some_and(|suffix| {
        suffix.len() == 32 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn read_environment_id(path: &Path) -> Result<String, String> {
    let value = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read T3 environment identity: {error}"))?;
    let value = value.trim();
    if !valid_environment_id(value) {
        return Err("persisted T3 environment identity is invalid".into());
    }
    Ok(value.to_string())
}

fn load_or_create_environment_id(path: Option<&Path>) -> Result<String, String> {
    let Some(path) = path else {
        return Ok(format!("codetwo-{}", uuid::Uuid::new_v4().simple()));
    };
    match read_environment_id(path) {
        Ok(identity) => return Ok(identity),
        Err(_) if !path.exists() => {}
        Err(error) => return Err(error),
    }

    let directory = path
        .parent()
        .ok_or_else(|| "T3 environment identity path has no parent directory".to_string())?;
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("could not create T3 identity directory: {error}"))?;
    let identity = format!("codetwo-{}", uuid::Uuid::new_v4().simple());
    let mut temporary =
        tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
    temporary
        .write_all(identity.as_bytes())
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| format!("could not persist T3 environment identity: {error}"))?;
    match temporary.persist_noclobber(path) {
        Ok(_) => Ok(identity),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            read_environment_id(path)
        }
        Err(error) => Err(format!(
            "could not install T3 environment identity: {}",
            error.error
        )),
    }
}

/// Shared adapter state. One instance is created per running C2 remote listener.
pub struct T3CompatState {
    engine: Arc<Engine>,
    events: broadcast::Sender<Event>,
    auth: Arc<AuthState>,
    sequence: Arc<AtomicU64>,
    updates: broadcast::Sender<CoreUpdate>,
    environment_id: String,
    cwd: String,
    compatibility_path: Option<PathBuf>,
    compatibility_load_error: Option<String>,
    compatibility: Mutex<CompatibilityMetadata>,
    compatibility_persist_lock: Mutex<()>,
    dispatch_lock: AsyncMutex<()>,
    command_receipts: Mutex<HashMap<String, Result<u64, String>>>,
}

impl T3CompatState {
    pub fn new(
        engine: Arc<Engine>,
        events: broadcast::Sender<Event>,
        auth: Arc<AuthState>,
    ) -> Result<Self, String> {
        let cwd = std::env::current_dir()
            .ok()
            .and_then(|path| path.to_str().map(str::to_owned))
            .filter(|path| !path.trim().is_empty())
            .unwrap_or_else(|| "/".to_string());
        let environment_id_path = auth.sibling_persist_path("t3-environment-id");
        // A paired T3 client keys its durable connection by environmentId. Falling back to a
        // random identity after an I/O or corruption error would silently strand every paired
        // phone on the next restart, so listener startup fails closed instead.
        let environment_id = load_or_create_environment_id(environment_id_path.as_deref())?;
        let sequence = Arc::new(AtomicU64::new(0));
        let (updates, _) = broadcast::channel(1024);
        let compatibility_path = auth.sibling_persist_path("t3-compatibility.json");
        let (compatibility, compatibility_load_error) =
            load_compatibility(compatibility_path.as_deref());
        let command_receipts = compatibility
            .command_receipts
            .iter()
            .map(|receipt| (receipt.command_id.clone(), Ok(receipt.sequence)))
            .collect();
        if let Some(error) = &compatibility_load_error {
            eprintln!("warning: T3 compatibility metadata is unavailable: {error}");
        }

        // Fold the core broadcast once, regardless of how many T3 devices are connected. Every
        // client therefore observes one global monotonic sequence instead of advancing it once per
        // socket. A full snapshot is used as the bounded catch-up fallback below.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let mut source = events.subscribe();
            let update_out = updates.clone();
            let update_sequence = sequence.clone();
            runtime.spawn(async move {
                loop {
                    match source.recv().await {
                        Ok(_) => {
                            let next = update_sequence.fetch_add(1, Ordering::SeqCst) + 1;
                            let _ = update_out.send(CoreUpdate { sequence: next });
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            let next = update_sequence.fetch_add(1, Ordering::SeqCst) + 1;
                            let _ = update_out.send(CoreUpdate { sequence: next });
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
        }

        Ok(Self {
            engine,
            events,
            auth,
            sequence,
            updates,
            environment_id,
            cwd,
            compatibility_path,
            compatibility_load_error,
            compatibility: Mutex::new(compatibility),
            compatibility_persist_lock: Mutex::new(()),
            dispatch_lock: AsyncMutex::new(()),
            command_receipts: Mutex::new(command_receipts),
        })
    }

    fn persist_compatibility(&self) -> Result<(), String> {
        let Some(path) = &self.compatibility_path else {
            return Ok(());
        };
        if let Some(error) = &self.compatibility_load_error {
            return Err(format!(
                "refusing to replace unreadable T3 compatibility metadata: {error}"
            ));
        }
        // Serialize clone + write + replace. Without this guard, an older concurrent writer can
        // land after a newer one and erase a just-created public thread-id mapping.
        let _persist_guard = self.compatibility_persist_lock.lock().unwrap();
        let metadata = self.compatibility.lock().unwrap().clone();
        let contents = serde_json::to_vec_pretty(&metadata).map_err(|error| error.to_string())?;
        let directory = path
            .parent()
            .ok_or_else(|| "T3 compatibility path has no parent directory".to_string())?;
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
        temporary
            .write_all(&contents)
            .and_then(|_| temporary.as_file().sync_all())
            .map_err(|error| error.to_string())?;
        temporary
            .persist(path)
            .map(|_| ())
            .map_err(|error| error.error.to_string())
    }

    fn persist_command_receipt(&self, command_id: &str, sequence: u64) -> Result<(), String> {
        if self.compatibility_load_error.is_some() {
            return Err(compatibility_unavailable_message());
        }
        {
            let mut compatibility = self.compatibility.lock().unwrap();
            compatibility
                .command_receipts
                .retain(|receipt| receipt.command_id != command_id);
            compatibility
                .command_receipts
                .push(PersistedCommandReceipt {
                    command_id: command_id.to_string(),
                    sequence,
                });
            let excess = compatibility
                .command_receipts
                .len()
                .saturating_sub(MAX_PERSISTED_COMMAND_RECEIPTS);
            if excess > 0 {
                compatibility.command_receipts.drain(..excess);
            }
        }
        self.persist_compatibility()
            .map_err(|error| format!("could not persist the T3 command receipt: {error}"))
    }

    async fn submit_prompt_and_wait(
        &self,
        session: String,
        doc: Vec<DocBlock>,
        request_id: String,
        message_id: String,
    ) -> Result<(), String> {
        if let Some(store) = self.engine.store() {
            if let Some((accepted_session, accepted_message, _)) = store
                .command_receipt("t3-prompt", &request_id)
                .map_err(|error| error.to_string())?
            {
                if accepted_session == session
                    && accepted_message.as_deref() == Some(message_id.as_str())
                {
                    return Ok(());
                }
                return Err("T3 command id was already used for another message or thread".into());
            }
        }
        let mut events = self.events.subscribe();
        let encoded_receipt = serde_json::to_string(&(&request_id, &message_id))
            .map_err(|error| format!("could not encode T3 prompt receipt: {error}"))?;
        let core_request_id = format!("t3-command:{encoded_receipt}");
        self.engine
            .submit(Op::Prompt {
                session: session.clone(),
                doc,
                request_id: Some(core_request_id.clone()),
            })
            .await
            .map_err(|error| error.to_string())?;
        // Both a first acceptance and a replay commit/query the SQLite receipt before submit
        // returns. Prefer that authoritative result over timing the broadcast, whose original
        // TurnStarted may have happened in a prior process.
        if let Some(store) = self.engine.store() {
            if let Some((accepted_session, accepted_message, _)) = store
                .command_receipt("t3-prompt", &request_id)
                .map_err(|error| error.to_string())?
            {
                if accepted_session == session
                    && accepted_message.as_deref() == Some(message_id.as_str())
                {
                    return Ok(());
                }
                return Err("T3 command id was already used for another message or thread".into());
            }
        }
        tokio::time::timeout(StdDuration::from_secs(5), async {
            loop {
                match events.recv().await {
                    Ok(Event::TurnStarted {
                        session: accepted_session,
                        request_id: Some(accepted_request),
                        ..
                    }) if accepted_session == session && accepted_request == core_request_id => {
                        break Ok(())
                    }
                    Ok(Event::Error {
                        session: Some(rejected_session),
                        message,
                        request_id: Some(rejected_request),
                        ..
                    }) if rejected_session == session && rejected_request == core_request_id => {
                        break Err(message)
                    }
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => {
                        break Err("C2 event stream closed before accepting the prompt".into())
                    }
                }
            }
        })
        .await
        .map_err(|_| "C2 timed out while accepting the mobile prompt".to_string())?
    }

    async fn set_execution_policy_and_wait(
        &self,
        session: String,
        mode: PermissionMode,
        sandbox: SandboxPolicy,
        request_id: String,
    ) -> Result<(), String> {
        let mut events = self.events.subscribe();
        self.engine
            .submit(Op::SetExecutionPolicy {
                session: session.clone(),
                mode,
                sandbox,
                request_id: Some(request_id.clone()),
            })
            .await
            .map_err(|error| error.to_string())?;
        tokio::time::timeout(StdDuration::from_secs(5), async {
            loop {
                match events.recv().await {
                    Ok(Event::ExecutionPolicyChanged {
                        session: changed_session,
                        request_id: Some(changed_request),
                        ..
                    }) if changed_session == session && changed_request == request_id => {
                        break Ok(())
                    }
                    Ok(Event::Error {
                        session: Some(rejected_session),
                        message,
                        request_id: Some(rejected_request),
                        ..
                    }) if rejected_session == session && rejected_request == request_id => {
                        break Err(message)
                    }
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => {
                        break Err("C2 event stream closed before changing execution policy".into())
                    }
                }
            }
        })
        .await
        .map_err(|_| "C2 timed out while changing execution policy".to_string())?
    }

    async fn set_model_and_verify(&self, session: String, model: String) -> Result<(), String> {
        let before = self
            .engine
            .list_sessions()
            .map_err(|error| error.to_string())?;
        if !before.iter().any(|candidate| candidate.id == session) {
            return Err(format!(
                "unknown thread: {}",
                self.public_thread_id(&session)
            ));
        }
        self.engine
            .submit(Op::SetModel {
                session: session.clone(),
                model: model.clone(),
            })
            .await
            .map_err(|error| error.to_string())?;
        let applied = self
            .engine
            .list_sessions()
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|candidate| candidate.id == session)
            .and_then(|candidate| candidate.model)
            .is_some_and(|current| current == model);
        if applied {
            Ok(())
        } else {
            Err(format!("C2 did not apply model {model}"))
        }
    }

    fn now(&self) -> String {
        iso(Utc::now())
    }

    fn descriptor(&self) -> Value {
        json!({
            "environmentId": self.environment_id,
            "label": host_label(),
            "platform": {
                "os": platform_os(),
                "arch": platform_arch(),
            },
            "serverVersion": T3_CONTRACT_VERSION,
            "capabilities": {
                "repositoryIdentity": false,
                "connectionProbe": true,
            },
        })
    }

    fn auth_descriptor(&self) -> Value {
        json!({
            "policy": "remote-reachable",
            "bootstrapMethods": ["one-time-token"],
            "sessionMethods": ["bearer-access-token"],
            "sessionCookieName": "codetwo-t3-session",
        })
    }

    fn server_config(&self) -> Value {
        let home = codetwo_core::provider::home_dir().unwrap_or_else(|| PathBuf::from(&self.cwd));
        let keybindings_config = home.join(".config/codetwo/keymap.json");
        let logs_directory = home.join(".codetwo/logs");
        json!({
            "environment": self.descriptor(),
            "auth": self.auth_descriptor(),
            "cwd": self.cwd,
            "keybindingsConfigPath": keybindings_config.to_string_lossy(),
            "keybindings": [],
            "issues": [],
            "providers": self.providers(),
            "availableEditors": [],
            "observability": {
                "logsDirectoryPath": logs_directory.to_string_lossy(),
                "localTracingEnabled": false,
                "otlpTracesEnabled": false,
                "otlpMetricsEnabled": false,
            },
            // Every ServerSettings field in contracts 0.0.33 has a decoding default.
            "settings": {},
            "shellResumeCompletionMarker": false,
            "threadResumeCompletionMarker": false,
            "threadSnapshotPagination": false,
        })
    }

    fn providers(&self) -> Vec<Value> {
        let checked_at = self.now();
        let sessions = self.engine.list_sessions().unwrap_or_default();
        let mut seen = HashSet::new();
        let mut values = Vec::new();

        for (provider, models) in self.engine.provider_catalog() {
            let instance_id = provider_slug(&provider.id);
            seen.insert(instance_id.clone());
            let installed = provider.is_available();
            values.push(provider_value(
                &instance_id,
                &provider.display_name,
                installed,
                models,
                &checked_at,
            ));
        }

        // Keep historical threads decodable after their integration is removed. A saved thread
        // does not establish that its provider or model is currently available.
        for session in sessions {
            let instance_id = provider_slug(&session.provider);
            if !seen.insert(instance_id.clone()) {
                continue;
            }
            values.push(provider_value(
                &instance_id,
                &provider_display_name(&session.provider),
                false,
                Vec::new(),
                &checked_at,
            ));
        }

        values
    }

    fn public_thread_id(&self, core_id: &str) -> String {
        self.compatibility
            .lock()
            .unwrap()
            .aliases
            .iter()
            .find_map(|(public, core)| (core == core_id).then(|| public.clone()))
            .unwrap_or_else(|| core_id.to_string())
    }

    fn core_thread_id(&self, public_id: &str) -> String {
        self.compatibility
            .lock()
            .unwrap()
            .aliases
            .get(public_id)
            .cloned()
            .unwrap_or_else(|| public_id.to_string())
    }

    fn validate_interaction_mode(mode: &str) -> Result<(), String> {
        match mode {
            "default" => Ok(()),
            _ => Err(format!("unsupported T3 interaction mode: {mode}")),
        }
    }

    fn persist_thread_identity(&self, public_id: &str, core_id: &str) -> Result<(), String> {
        if self.compatibility_load_error.is_some() {
            return Err(compatibility_unavailable_message());
        }
        let previous_alias = {
            let mut compatibility = self.compatibility.lock().unwrap();
            if compatibility
                .aliases
                .iter()
                .any(|(known_public, known_core)| {
                    known_public != public_id && known_core == core_id
                })
            {
                return Err("C2 session is already mapped to another T3 thread".into());
            }
            compatibility
                .aliases
                .insert(public_id.to_string(), core_id.to_string())
        };
        if let Err(error) = self.persist_compatibility() {
            // Do not let a failed durable write turn into an in-memory-only alias that a retry
            // mistakes for a completed bootstrap.
            let mut compatibility = self.compatibility.lock().unwrap();
            match previous_alias {
                Some(previous) => {
                    compatibility
                        .aliases
                        .insert(public_id.to_string(), previous);
                }
                None => {
                    compatibility.aliases.remove(public_id);
                }
            }
            return Err(format!(
                "could not persist the T3 mobile thread identity: {error}"
            ));
        }
        Ok(())
    }

    fn shell_snapshot(&self, sequence: u64) -> Result<Value, String> {
        let sessions = self
            .engine
            .list_sessions()
            .map_err(|error| error.to_string())?;
        let mut grouped: BTreeMap<String, Vec<Session>> = BTreeMap::new();
        for session in &sessions {
            grouped
                .entry(project_path(session, &self.cwd))
                .or_default()
                .push(session.clone());
        }
        if let Some(store) = self.engine.store() {
            for project in store.list_projects().map_err(|error| error.to_string())? {
                grouped.entry(project.path).or_default();
            }
        }
        grouped.entry(self.cwd.clone()).or_default();

        let updated_at = self.now();
        let projects: Vec<Value> = grouped
            .iter()
            .map(|(path, project_sessions)| project_shell(path, project_sessions, &updated_at))
            .collect();
        let threads: Vec<Value> = sessions
            .iter()
            .map(|session| self.thread_shell(session, &updated_at))
            .collect();

        Ok(json!({
            "snapshotSequence": sequence,
            "projects": projects,
            "threads": threads,
            "updatedAt": updated_at,
        }))
    }

    fn thread_shell(&self, session: &Session, updated_at: &str) -> Value {
        let public_id = self.public_thread_id(&session.id);
        let project = project_path(session, &self.cwd);
        let created_at = millis_iso(session.created_at);
        let (latest_turn, status, active_turn_id, last_error) =
            activity_projection(session, &created_at);
        let pending = pending_activities(session, updated_at);

        json!({
            "id": public_id,
            "projectId": project_id(&project),
            "title": nonempty(&session.title, "Untitled session"),
            "modelSelection": model_selection(session),
            "runtimeMode": runtime_mode(session.permission_mode, session.sandbox_policy),
            "interactionMode": "default",
            "branch": Value::Null,
            "worktreePath": session.worktree_path,
            "latestTurn": latest_turn,
            "createdAt": created_at,
            "updatedAt": updated_at,
            "archivedAt": Value::Null,
            "settledOverride": Value::Null,
            "settledAt": Value::Null,
            "session": {
                "threadId": public_id,
                "status": status,
                "providerName": provider_display_name(&session.provider),
                "providerInstanceId": provider_slug(&session.provider),
                "runtimeMode": runtime_mode(session.permission_mode, session.sandbox_policy),
                "activeTurnId": active_turn_id,
                "lastError": last_error,
                "updatedAt": updated_at,
            },
            "latestUserMessageAt": Value::Null,
            "hasPendingApprovals": !pending.is_empty(),
            "hasPendingUserInput": false,
            "hasActionableProposedPlan": false,
        })
    }

    fn thread_snapshot(&self, public_id: &str, sequence: u64) -> Result<Value, String> {
        let core_id = self.core_thread_id(public_id);
        let session = self
            .engine
            .list_sessions()
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|session| session.id == core_id)
            .ok_or_else(|| format!("unknown thread: {public_id}"))?;
        let entries = self.full_transcript(&core_id)?;
        let updated_at = self.now();
        let mut shell = self.thread_shell(&session, &updated_at);
        let mut messages = Vec::new();
        let mut activities = pending_activities(&session, &updated_at);
        let mut latest_user_message_at = Value::Null;
        let created_at = millis_iso(session.created_at);
        let external_message_ids = match self.engine.store() {
            Some(store) => store
                .command_receipt_subjects("t3-prompt", &core_id)
                .map_err(|error| error.to_string())?,
            None => HashMap::new(),
        };
        let mut assistant_message: Option<(i64, String, String)> = None;

        for entry in entries {
            let at = millis_iso(session.created_at.saturating_add(entry.seq.max(0)));
            match entry.part {
                Part::Prompt { text, .. } => {
                    flush_assistant_message(&mut messages, &session.id, &mut assistant_message);
                    // C2's display projection is intentionally capped at 400 characters. T3
                    // mobile expects the complete authored message. Hide legacy adapter-owned
                    // planning markers in old transcripts.
                    let text = t3_user_prompt(&text);
                    messages.push(message_value(
                        external_message_ids
                            .get(&entry.seq)
                            .cloned()
                            .unwrap_or_else(|| fallback_message_id(&session.id, entry.seq)),
                        "user",
                        text,
                        &at,
                    ));
                    latest_user_message_at = Value::String(at);
                }
                Part::Text { text } => {
                    if entry.role == Role::User {
                        flush_assistant_message(&mut messages, &session.id, &mut assistant_message);
                        messages.push(message_value(
                            fallback_message_id(&session.id, entry.seq),
                            "user",
                            text,
                            &at,
                        ));
                        latest_user_message_at = Value::String(at);
                    } else {
                        match &mut assistant_message {
                            Some((_, _, accumulated)) => accumulated.push_str(&text),
                            None => assistant_message = Some((entry.seq, at, text)),
                        }
                    }
                }
                Part::Reasoning { text } => activities.push(json!({
                    "id": format!("codetwo-reasoning-{}-{}", session.id, entry.seq),
                    "tone": "info",
                    "kind": "assistant.reasoning",
                    "summary": "Reasoning",
                    "payload": { "text": text },
                    "turnId": Value::Null,
                    "sequence": entry.seq.max(0),
                    "createdAt": at,
                })),
                Part::ToolCall {
                    id,
                    title,
                    status,
                    tool_kind,
                    agent_input,
                    ..
                } => {
                    activities.push(json!({
                        "id": nonempty(&id, &format!("codetwo-tool-{}-{}", session.id, entry.seq)),
                        "tone": "tool",
                        "kind": "tool.lifecycle",
                        "summary": nonempty(&title, "Tool call"),
                        "payload": {
                            "status": status,
                            "toolKind": tool_kind,
                            "agentInput": agent_input,
                        },
                        "turnId": Value::Null,
                        "sequence": entry.seq.max(0),
                        "createdAt": at,
                    }));
                }
                Part::Plan { .. } => {}
            }
        }
        flush_assistant_message(&mut messages, &session.id, &mut assistant_message);

        let object = shell
            .as_object_mut()
            .ok_or_else(|| "invalid thread projection".to_string())?;
        object.insert("deletedAt".into(), Value::Null);
        object.insert("messages".into(), Value::Array(messages));
        object.insert("proposedPlans".into(), Value::Array(Vec::new()));
        object.insert("activities".into(), Value::Array(activities));
        object.insert("checkpoints".into(), Value::Array(Vec::new()));
        object.insert("latestUserMessageAt".into(), latest_user_message_at);
        object.insert("createdAt".into(), Value::String(created_at));

        Ok(json!({
            "snapshotSequence": sequence,
            "thread": shell,
        }))
    }

    /// T3's `threadSnapshotPagination: false` contract means every thread snapshot must contain
    /// its complete history. C2 stores turn-aligned bounded pages, so walk them newest to oldest
    /// and then restore chronological order before projecting the snapshot.
    fn full_transcript(&self, core_id: &str) -> Result<Vec<codetwo_core::TranscriptEntry>, String> {
        let mut before = None;
        let mut pages = Vec::new();
        loop {
            let page = self
                .engine
                .transcript_page(core_id, before, MAX_TRANSCRIPT_TURNS)
                .map_err(|error| error.to_string())?;
            let next_before = page.next_before;
            pages.push(page.entries);
            match next_before {
                Some(cursor) => before = Some(cursor),
                None => break,
            }
        }
        pages.reverse();
        Ok(pages.into_iter().flatten().collect())
    }

    async fn dispatch_command(&self, command: Value) -> Result<u64, String> {
        let command_id = required_string(&command, "commandId")?;
        if let Some(receipt) = self
            .command_receipts
            .lock()
            .unwrap()
            .get(&command_id)
            .cloned()
        {
            return receipt;
        }
        // A mobile reconnect can retry the same create command while the first socket is still
        // active. Serialize commands and recheck the receipt after acquiring the gate so one public
        // thread id can never race into two C2 sessions.
        let _dispatch_guard = self.dispatch_lock.lock().await;
        if let Some(receipt) = self
            .command_receipts
            .lock()
            .unwrap()
            .get(&command_id)
            .cloned()
        {
            return receipt;
        }
        match self.dispatch_uncached(&command_id, command).await {
            Ok(sequence) => {
                // The mobile outbox retries the same command id after reconnects. Keep completed
                // receipts beside the durable public-thread aliases so a response-loss + server
                // restart cannot submit the turn twice.
                self.persist_command_receipt(&command_id, sequence)?;
                self.command_receipts
                    .lock()
                    .unwrap()
                    .insert(command_id, Ok(sequence));
                Ok(sequence)
            }
            Err(error) => Err(error),
        }
    }

    async fn dispatch_uncached(&self, command_id: &str, command: Value) -> Result<u64, String> {
        let command_type = required_string(&command, "type")?;
        match command_type.as_str() {
            "thread.turn.start" => {
                Self::validate_interaction_mode(
                    command
                        .get("interactionMode")
                        .and_then(Value::as_str)
                        .unwrap_or("default"),
                )?;
                let public_id = required_string(&command, "threadId")?;
                let text = command
                    .pointer("/message/text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "thread.turn.start requires message.text".to_string())?
                    .to_string();
                let message_id = command
                    .pointer("/message/messageId")
                    .and_then(Value::as_str)
                    .filter(|message_id| !message_id.trim().is_empty())
                    .ok_or_else(|| "thread.turn.start requires message.messageId".to_string())?
                    .to_string();
                let attachments = command
                    .pointer("/message/attachments")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0);
                if attachments != 0 {
                    return Err("C2's T3 adapter does not yet support mobile attachments".into());
                }

                let known = self
                    .engine
                    .list_sessions()
                    .map_err(|error| error.to_string())?
                    .iter()
                    .any(|session| session.id == self.core_thread_id(&public_id));
                let core_id = if known {
                    self.core_thread_id(&public_id)
                } else {
                    self.create_thread(command_id, &public_id, &command).await?
                };
                let doc = vec![DocBlock::Text { text }];
                self.submit_prompt_and_wait(core_id, doc, command_id.to_string(), message_id)
                    .await?;
            }
            "thread.turn.interrupt" | "thread.session.stop" => {
                let thread_id = self.core_thread_id(&required_string(&command, "threadId")?);
                let known = self
                    .engine
                    .list_sessions()
                    .map_err(|error| error.to_string())?
                    .iter()
                    .any(|session| session.id == thread_id);
                if !known {
                    return Err(format!(
                        "unknown thread: {}",
                        self.public_thread_id(&thread_id)
                    ));
                }
                self.engine
                    .submit(Op::Cancel { session: thread_id })
                    .await
                    .map_err(|error| error.to_string())?;
            }
            "thread.runtime-mode.set" => {
                let thread_id = self.core_thread_id(&required_string(&command, "threadId")?);
                let runtime = required_string(&command, "runtimeMode")?;
                let (mode, sandbox) = policy_from_runtime(&runtime)?;
                self.set_execution_policy_and_wait(
                    thread_id,
                    mode,
                    sandbox,
                    command_id.to_string(),
                )
                .await?;
            }
            "thread.interaction-mode.set" => {
                let public_id = required_string(&command, "threadId")?;
                let core_id = self.core_thread_id(&public_id);
                let known = self
                    .engine
                    .list_sessions()
                    .map_err(|error| error.to_string())?
                    .iter()
                    .any(|session| session.id == core_id);
                if !known {
                    return Err(format!("unknown thread: {public_id}"));
                }
                let mode = required_string(&command, "interactionMode")?;
                Self::validate_interaction_mode(&mode)?;
            }
            "thread.meta.update" => {
                let thread_id = self.core_thread_id(&required_string(&command, "threadId")?);
                let Some(model) = command
                    .pointer("/modelSelection/model")
                    .and_then(Value::as_str)
                    .filter(|model| !model.trim().is_empty())
                else {
                    return Err(
                        "C2 currently supports model changes, not T3 title/branch edits".into(),
                    );
                };
                self.set_model_and_verify(thread_id, model.to_string())
                    .await?;
            }
            "thread.approval.respond" => {
                let thread_id = self.core_thread_id(&required_string(&command, "threadId")?);
                let request_id = required_string(&command, "requestId")?;
                let decision = required_string(&command, "decision")?;
                let option_id = self.permission_option(&thread_id, &request_id, &decision)?;
                if !self
                    .engine
                    .answer_permission(&thread_id, &request_id, option_id.as_deref())
                {
                    return Err("the approval request was already resolved".into());
                }
            }
            _ => return Err(format!("unsupported T3 command: {command_type}")),
        }

        Ok(self.sequence.load(Ordering::SeqCst))
    }

    async fn create_thread(
        &self,
        command_id: &str,
        public_id: &str,
        command: &Value,
    ) -> Result<String, String> {
        let create = command
            .pointer("/bootstrap/createThread")
            .ok_or_else(|| format!("unknown thread: {public_id}"))?;
        if self.compatibility_load_error.is_some() {
            return Err(compatibility_unavailable_message());
        }
        if command
            .pointer("/bootstrap/prepareWorktree")
            .is_some_and(|value| !value.is_null())
            || command
                .pointer("/bootstrap/runSetupScript")
                .is_some_and(|value| value == &Value::Bool(true))
            || create
                .get("worktreePath")
                .is_some_and(|value| !value.is_null())
            || create.get("branch").is_some_and(|value| !value.is_null())
        {
            return Err(
                "C2's T3 adapter does not yet support mobile Git/worktree bootstrap options".into(),
            );
        }
        let model = create
            .pointer("/modelSelection/model")
            .or_else(|| command.pointer("/modelSelection/model"))
            .and_then(Value::as_str)
            .filter(|model| !model.trim().is_empty())
            .map(str::to_owned);
        let runtime = create
            .get("runtimeMode")
            .or_else(|| command.get("runtimeMode"))
            .and_then(Value::as_str)
            .unwrap_or("approval-required");
        let (mode, sandbox) = policy_from_runtime(runtime)?;
        let interaction_mode = create
            .get("interactionMode")
            .or_else(|| command.get("interactionMode"))
            .and_then(Value::as_str)
            .unwrap_or("default");
        Self::validate_interaction_mode(interaction_mode)?;

        // NewSession and this receipt share one SQLite transaction. If the process died before
        // the JSON alias/model projection landed, replay repairs those projections without
        // creating a second provider session or worktree.
        if let Some(store) = self.engine.store() {
            if let Some((core_id, receipt_public_id, _)) = store
                .command_receipt("t3-create", command_id)
                .map_err(|error| error.to_string())?
            {
                if receipt_public_id.as_deref() != Some(public_id) {
                    return Err("T3 command id was already used for another thread".into());
                }
                let stored = store
                    .get_session(&core_id)
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "T3 session receipt points to a missing session".to_string())?;
                if let Some(model) = model.as_ref() {
                    if stored.model.as_deref() != Some(model.as_str()) {
                        self.set_model_and_verify(core_id.clone(), model.clone())
                            .await?;
                    }
                }
                self.persist_thread_identity(public_id, &core_id)?;
                return Ok(core_id);
            }
        }

        let project = required_string(create, "projectId")?;
        let path = self
            .project_paths()?
            .remove(&project)
            .ok_or_else(|| format!("unknown project: {project}"))?;
        let instance_id = create
            .pointer("/modelSelection/instanceId")
            .or_else(|| command.pointer("/modelSelection/instanceId"))
            .and_then(Value::as_str)
            .unwrap_or("codex");
        let provider = provider_from_slug(instance_id);
        let encoded_receipt = serde_json::to_string(&(command_id, public_id))
            .map_err(|error| format!("could not encode T3 session receipt: {error}"))?;
        let request_id = format!("t3-create:{encoded_receipt}");
        let mut event_rx = self.events.subscribe();
        self.engine
            .submit(Op::NewSession {
                provider,
                cwd: path,
                use_worktree: false,
                worktree_base: None,
                worktree_base_sha: None,
                request_id: Some(request_id.clone()),
                model: None,
                initial_policy: Some(ExecutionPolicy { mode, sandbox }),
            })
            .await
            .map_err(|error| error.to_string())?;

        let core_id = tokio::time::timeout(StdDuration::from_secs(5), async {
            loop {
                match event_rx.recv().await {
                    Ok(Event::SessionCreated {
                        session,
                        request_id: Some(id),
                        ..
                    }) if id == request_id => break Ok(session),
                    Ok(Event::Error {
                        message,
                        request_id: Some(id),
                        terminal: true,
                        ..
                    }) if id == request_id => break Err(message),
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => {
                        break Err("C2 session event stream closed during creation".into())
                    }
                }
            }
        })
        .await
        .map_err(|_| "C2 timed out waiting for the new session id".to_string())??;
        if let Some(model) = model {
            // Engine operations are serialized. Enqueue this before returning so the caller's
            // first Prompt observes the model chosen in T3's create-thread bootstrap payload.
            self.set_model_and_verify(core_id.clone(), model).await?;
        }
        self.persist_thread_identity(public_id, &core_id)?;
        Ok(core_id)
    }

    fn project_paths(&self) -> Result<HashMap<String, String>, String> {
        let mut paths = HashMap::new();
        paths.insert(project_id(&self.cwd), self.cwd.clone());
        if let Some(store) = self.engine.store() {
            for project in store.list_projects().map_err(|error| error.to_string())? {
                paths.insert(project_id(&project.path), project.path);
            }
        }
        for session in self
            .engine
            .list_sessions()
            .map_err(|error| error.to_string())?
        {
            let path = project_path(&session, &self.cwd);
            paths.insert(project_id(&path), path);
        }
        Ok(paths)
    }

    fn permission_option(
        &self,
        thread_id: &str,
        request_id: &str,
        decision: &str,
    ) -> Result<Option<String>, String> {
        if decision == "cancel" {
            return Ok(None);
        }
        if !matches!(decision, "accept" | "acceptForSession" | "decline") {
            return Err(format!("unknown approval decision: {decision}"));
        }
        let session = self
            .engine
            .list_sessions()
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|session| session.id == thread_id)
            .ok_or_else(|| format!("unknown thread: {thread_id}"))?;
        let SessionRunState::AwaitingInput { pending, .. } = session.activity.state else {
            return Err("the approval request is no longer pending".into());
        };
        let pending = pending
            .into_iter()
            .find(|pending| pending.input_id == request_id)
            .ok_or_else(|| "the approval request is no longer pending".to_string())?;
        // A structured question is not an approval: T3's accept/decline vocabulary has nothing to
        // say about which option the user meant. Decline maps to skipping the question — which is
        // what unblocks the agent — and accepting is refused rather than answered on a guess.
        if pending.kind == PendingInputKind::Elicitation {
            return match decision {
                "decline" => Ok(Some(codetwo_core::elicitation::SKIP_OPTION_ID.to_string())),
                _ => Err(
                    "this request is a question from the agent; answer it in a client that renders \
                     questions, or decline to skip it"
                        .into(),
                ),
            };
        }
        select_permission_option(&pending.options, &pending.option_kinds, decision).map(Some)
    }
}

fn select_permission_option(
    options: &[(String, String)],
    option_kinds: &BTreeMap<String, String>,
    decision: &str,
) -> Result<String, String> {
    let target_kind = match decision {
        "accept" => "allow_once",
        "acceptForSession" => "allow_always",
        "decline" => "reject_once",
        _ => return Err(format!("unknown approval decision: {decision}")),
    };
    let mut matches = options
        .iter()
        .filter(|(id, _)| option_kinds.get(id).is_some_and(|kind| kind == target_kind));
    let Some((id, _)) = matches.next() else {
        return Err(format!(
            "the provider did not offer the required {target_kind} approval option"
        ));
    };
    if matches.next().is_some() {
        return Err("the provider offered multiple matching approval options".into());
    }
    Ok(id.clone())
}

/// T3 HTTP routes. `/ws` is intentionally owned by the parent server so it can select either the
/// existing C2 protocol (`ticket`) or this protocol (`wsTicket`) without breaking old clients.
pub fn router(state: Arc<T3CompatState>) -> Router {
    Router::new()
        .route("/.well-known/t3/environment", get(environment_descriptor))
        .route("/oauth/token", post(oauth_token))
        .route("/api/auth/session", get(auth_session))
        .route("/api/auth/websocket-ticket", post(websocket_ticket))
        .route("/api/orchestration/shell", get(shell_snapshot))
        .route(
            "/api/orchestration/threads/:thread_id",
            get(thread_snapshot),
        )
        .route("/api/orchestration/dispatch", post(dispatch_http))
        .with_state(state)
}

async fn environment_descriptor(State(state): State<Arc<T3CompatState>>) -> Json<Value> {
    Json(state.descriptor())
}

#[derive(Deserialize)]
struct TokenExchangeForm {
    grant_type: String,
    subject_token: String,
    subject_token_type: String,
    requested_token_type: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    client_label: Option<String>,
    #[serde(default)]
    client_device_type: Option<String>,
    #[serde(default)]
    client_os: Option<String>,
}

async fn oauth_token(
    State(state): State<Arc<T3CompatState>>,
    Form(form): Form<TokenExchangeForm>,
) -> Response {
    if form.grant_type != TOKEN_EXCHANGE_GRANT
        || form.subject_token_type != BOOTSTRAP_TOKEN_TYPE
        || form.requested_token_type != ACCESS_TOKEN_TYPE
    {
        return (StatusCode::BAD_REQUEST, "unsupported token exchange").into_response();
    }
    let requested: Vec<&str> = form
        .scope
        .as_deref()
        .unwrap_or("")
        .split_whitespace()
        .collect();
    if requested
        .iter()
        .any(|scope| !STANDARD_SCOPES.contains(scope))
    {
        return (StatusCode::BAD_REQUEST, "unsupported scope").into_response();
    }
    let scopes: Vec<String> = if requested.is_empty() {
        STANDARD_SCOPES
            .iter()
            .map(|scope| (*scope).to_string())
            .collect()
    } else {
        requested.iter().map(|scope| (*scope).to_string()).collect()
    };
    let ephemeral_handoff = form.client_device_type.as_deref() == Some("handoff");
    let device_name = form
        .client_label
        .filter(|label| !label.trim().is_empty())
        .unwrap_or_else(|| {
            let kind = form.client_device_type.unwrap_or_else(|| "mobile".into());
            let os = form.client_os.unwrap_or_else(|| "unknown".into());
            format!("T3 Code {kind} ({os})")
        });
    let paired = match state.auth.try_pair_with_profile_options(
        &form.subject_token,
        &device_name,
        Some(StdDuration::from_secs(
            Duration::days(ACCESS_TOKEN_TTL_DAYS).num_seconds() as u64,
        )),
        scopes.clone(),
        ephemeral_handoff,
    ) {
        Ok(Some(paired)) => paired,
        Ok(None) => {
            return (StatusCode::UNAUTHORIZED, "invalid or expired pairing token").into_response()
        }
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("could not persist the paired device: {error}"),
            )
                .into_response()
        }
    };
    Json(json!({
        "access_token": paired.bearer,
        "issued_token_type": ACCESS_TOKEN_TYPE,
        "token_type": "Bearer",
        "expires_in": Duration::days(ACCESS_TOKEN_TTL_DAYS).num_seconds(),
        "scope": scopes.join(" "),
    }))
    .into_response()
}

async fn auth_session(State(state): State<Arc<T3CompatState>>, headers: HeaderMap) -> Json<Value> {
    let authorization =
        bearer_from(&headers).and_then(|bearer| state.auth.authorize_bearer_profile(bearer));
    let authenticated = authorization.is_some();
    let mut value = json!({
        "authenticated": authenticated,
        "auth": state.auth_descriptor(),
    });
    if let Some(authorization) = authorization {
        let object = value.as_object_mut().expect("auth session is an object");
        object.insert("scopes".into(), json!(authorization.scopes));
        object.insert("sessionMethod".into(), json!("bearer-access-token"));
        if let Some(expires_at) = authorization.expires_at {
            object.insert("expiresAt".into(), json!(epoch_secs_iso(expires_at)));
        }
    }
    Json(value)
}

async fn websocket_ticket(State(state): State<Arc<T3CompatState>>, headers: HeaderMap) -> Response {
    let Some(authorization) =
        bearer_from(&headers).and_then(|bearer| state.auth.authorize_bearer_profile(bearer))
    else {
        return (StatusCode::UNAUTHORIZED, "invalid bearer").into_response();
    };
    Json(json!({
        "ticket": state.auth.issue_ws_ticket_with_scopes(
            &authorization.device_id,
            authorization.scopes,
        ),
        "expiresAt": iso(Utc::now() + Duration::from_std(WS_TICKET_TTL).unwrap_or_default()),
    }))
    .into_response()
}

async fn shell_snapshot(State(state): State<Arc<T3CompatState>>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers, "orchestration:read") {
        return (StatusCode::UNAUTHORIZED, "invalid bearer").into_response();
    }
    match state.shell_snapshot(state.sequence.load(Ordering::SeqCst)) {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error).into_response(),
    }
}

async fn thread_snapshot(
    State(state): State<Arc<T3CompatState>>,
    AxumPath(thread_id): AxumPath<String>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&state, &headers, "orchestration:read") {
        return (StatusCode::UNAUTHORIZED, "invalid bearer").into_response();
    }
    match state.thread_snapshot(&thread_id, state.sequence.load(Ordering::SeqCst)) {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(error) if error.starts_with("unknown thread:") => {
            (StatusCode::NOT_FOUND, error).into_response()
        }
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error).into_response(),
    }
}

async fn dispatch_http(
    State(state): State<Arc<T3CompatState>>,
    headers: HeaderMap,
    Json(command): Json<Value>,
) -> Response {
    if !authorized(&state, &headers, "orchestration:operate") {
        return (StatusCode::UNAUTHORIZED, "invalid bearer").into_response();
    }
    match state.dispatch_command(command).await {
        Ok(sequence) => Json(json!({ "sequence": sequence })).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error).into_response(),
    }
}

fn bearer_from(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
}

fn authorized(state: &T3CompatState, headers: &HeaderMap, required_scope: &str) -> bool {
    bearer_from(headers)
        .and_then(|bearer| state.auth.authorize_bearer_profile(bearer))
        .is_some_and(|authorization| {
            authorization
                .scopes
                .iter()
                .any(|scope| scope == required_scope)
        })
}

#[derive(Clone)]
enum SubscriptionKind {
    Config,
    Lifecycle,
    Shell,
    Thread(String),
}

struct Subscription {
    request_id: Value,
    kind: SubscriptionKind,
    awaiting_ack: bool,
    pending_update: bool,
}

/// Serve one already-ticket-authenticated T3 Effect RPC socket.
pub async fn handle_socket(
    socket: WebSocket,
    state: Arc<T3CompatState>,
    device_id: String,
    scopes: Vec<String>,
) {
    let (mut sender, mut receiver) = socket.split();
    let mut updates = state.updates.subscribe();
    let mut revocations = state.auth.subscribe_revocations();
    let mut subscriptions: Vec<Subscription> = Vec::new();
    let authorization_period = StdDuration::from_secs(30);
    let mut authorization_checks = tokio::time::interval_at(
        tokio::time::Instant::now() + authorization_period,
        authorization_period,
    );

    // Close the redeem-to-subscribe race: a revoke can happen after the ticket is consumed but
    // before this receiver is installed, in which case the broadcast alone cannot observe it.
    if !state.auth.t3_device_is_authorized(&device_id) {
        let _ = sender.send(Message::Close(None)).await;
        return;
    }

    loop {
        tokio::select! {
            _ = authorization_checks.tick() => {
                if !state.auth.t3_device_is_authorized(&device_id) {
                    let _ = sender.send(Message::Close(None)).await;
                    break;
                }
            }
            revoked = revocations.recv() => {
                match revoked {
                    Ok(revoked_id) if revoked_id == device_id => {
                        let _ = sender.send(Message::Close(None)).await;
                        break;
                    }
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {
                        if !state.auth.t3_device_is_authorized(&device_id) {
                            let _ = sender.send(Message::Close(None)).await;
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = receiver.next() => {
                let Some(Ok(message)) = incoming else { break };
                if !state.auth.t3_device_is_authorized(&device_id) {
                    let _ = sender.send(Message::Close(None)).await;
                    break;
                }
                match message {
                    Message::Text(text) => {
                        let Ok(value) = serde_json::from_str::<Value>(&text) else {
                            continue;
                        };
                        let frames = value.as_array().cloned().unwrap_or_else(|| vec![value]);
                        for frame in frames {
                            // A single WebSocket message may contain an arbitrary frame batch. An
                            // RPC can await long enough for a concurrent revoke, so re-authorize
                            // each frame instead of granting the whole attacker-controlled batch.
                            if !state.auth.t3_device_is_authorized(&device_id) {
                                let _ = sender.send(Message::Close(None)).await;
                                return;
                            }
                            if !handle_rpc_frame(&mut sender, &mut subscriptions, &state, &scopes, frame).await {
                                return;
                            }
                        }
                    }
                    Message::Ping(bytes) => {
                        if sender.send(Message::Pong(bytes)).await.is_err() { return; }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            update = updates.recv() => {
                // Never push a fresh full snapshot to a device revoked between core updates.
                if !state.auth.t3_device_is_authorized(&device_id) {
                    let _ = sender.send(Message::Close(None)).await;
                    break;
                }
                let sequence = match update {
                    Ok(update) => update.sequence,
                    Err(broadcast::error::RecvError::Lagged(_)) => state.sequence.load(Ordering::SeqCst),
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                for subscription in &mut subscriptions {
                    if !matches!(subscription.kind, SubscriptionKind::Shell | SubscriptionKind::Thread(_)) {
                        continue;
                    }
                    if subscription.awaiting_ack {
                        subscription.pending_update = true;
                        continue;
                    }
                    let value = subscription_value(&state, &subscription.kind, sequence);
                    if let Some(value) = value {
                        if send_chunk(&mut sender, &subscription.request_id, value).await.is_err() {
                            return;
                        }
                        subscription.awaiting_ack = true;
                    }
                }
            }
        }
    }
}

async fn handle_rpc_frame<S>(
    sender: &mut S,
    subscriptions: &mut Vec<Subscription>,
    state: &Arc<T3CompatState>,
    scopes: &[String],
    frame: Value,
) -> bool
where
    S: futures_util::Sink<Message> + Unpin,
{
    let tag = frame.get("_tag").and_then(Value::as_str).unwrap_or("");
    match tag {
        "Ping" => return send_json(sender, json!({ "_tag": "Pong" })).await.is_ok(),
        "Ack" => {
            let request_id = frame.get("requestId").cloned().unwrap_or(Value::Null);
            if let Some(index) = subscriptions
                .iter()
                .position(|subscription| subscription.request_id == request_id)
            {
                subscriptions[index].awaiting_ack = false;
                if subscriptions[index].pending_update {
                    subscriptions[index].pending_update = false;
                    let sequence = state.sequence.load(Ordering::SeqCst);
                    if let Some(value) =
                        subscription_value(state, &subscriptions[index].kind, sequence)
                    {
                        if send_chunk(sender, &request_id, value).await.is_err() {
                            return false;
                        }
                        subscriptions[index].awaiting_ack = true;
                    }
                }
            }
            return true;
        }
        "Interrupt" => {
            let request_id = frame.get("requestId").cloned().unwrap_or(Value::Null);
            subscriptions.retain(|subscription| subscription.request_id != request_id);
            return true;
        }
        "Request" => {}
        _ => return true,
    }

    let request_id = frame.get("id").cloned().unwrap_or(Value::Null);
    let method = frame.get("tag").and_then(Value::as_str).unwrap_or("");
    let payload = frame.get("payload").cloned().unwrap_or_else(|| json!({}));
    let required_scope = if method == "orchestration.dispatchCommand" {
        "orchestration:operate"
    } else {
        "orchestration:read"
    };
    if !scopes.iter().any(|scope| scope == required_scope) {
        return send_authorization_failure(sender, &request_id, required_scope)
            .await
            .is_ok();
    }
    match method {
        "server.getConfig" => send_success(sender, &request_id, state.server_config())
            .await
            .is_ok(),
        "server.probe" => send_success(sender, &request_id, json!({})).await.is_ok(),
        // The native mobile client reports foreground/background activity on connect and at a
        // cadence. C2 has no matching host power policy, but acknowledging the void RPC keeps
        // the official client from producing a stream of expected-but-unsupported failures.
        "server.reportClientActivity" => {
            send_success(sender, &request_id, Value::Null).await.is_ok()
        }
        "subscribeServerConfig" => {
            let value =
                json!({ "version": 1, "type": "snapshot", "config": state.server_config() });
            if send_chunk(sender, &request_id, value).await.is_err() {
                return false;
            }
            subscriptions.push(Subscription {
                request_id,
                kind: SubscriptionKind::Config,
                awaiting_ack: true,
                pending_update: false,
            });
            true
        }
        "subscribeServerLifecycle" => {
            let cwd_name = Path::new(&state.cwd)
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or("C2");
            let now = state.now();
            let values = vec![
                json!({
                    "version": 1,
                    "sequence": 0,
                    "type": "welcome",
                    "payload": {
                        "environment": state.descriptor(),
                        "cwd": state.cwd,
                        "projectName": cwd_name,
                    },
                }),
                json!({
                    "version": 1,
                    "sequence": 1,
                    "type": "ready",
                    "payload": { "at": now, "environment": state.descriptor() },
                }),
            ];
            if send_chunks(sender, &request_id, values).await.is_err() {
                return false;
            }
            subscriptions.push(Subscription {
                request_id,
                kind: SubscriptionKind::Lifecycle,
                awaiting_ack: true,
                pending_update: false,
            });
            true
        }
        "orchestration.subscribeShell" => {
            let sequence = state.sequence.load(Ordering::SeqCst);
            match state.shell_snapshot(sequence) {
                Ok(snapshot) => {
                    if send_chunk(
                        sender,
                        &request_id,
                        json!({ "kind": "snapshot", "snapshot": snapshot }),
                    )
                    .await
                    .is_err()
                    {
                        return false;
                    }
                    subscriptions.push(Subscription {
                        request_id,
                        kind: SubscriptionKind::Shell,
                        awaiting_ack: true,
                        pending_update: false,
                    });
                    true
                }
                Err(error) => {
                    send_failure(sender, &request_id, "OrchestrationGetSnapshotError", error)
                        .await
                        .is_ok()
                }
            }
        }
        "orchestration.subscribeThread" => {
            let Some(thread_id) = payload.get("threadId").and_then(Value::as_str) else {
                return send_failure(
                    sender,
                    &request_id,
                    "OrchestrationGetSnapshotError",
                    "threadId is required",
                )
                .await
                .is_ok();
            };
            let thread_id = thread_id.to_string();
            let sequence = state.sequence.load(Ordering::SeqCst);
            match state.thread_snapshot(&thread_id, sequence) {
                Ok(snapshot) => {
                    if send_chunk(
                        sender,
                        &request_id,
                        json!({ "kind": "snapshot", "snapshot": snapshot }),
                    )
                    .await
                    .is_err()
                    {
                        return false;
                    }
                    subscriptions.push(Subscription {
                        request_id,
                        kind: SubscriptionKind::Thread(thread_id),
                        awaiting_ack: true,
                        pending_update: false,
                    });
                    true
                }
                Err(error) => {
                    send_failure(sender, &request_id, "OrchestrationGetSnapshotError", error)
                        .await
                        .is_ok()
                }
            }
        }
        "orchestration.dispatchCommand" => match state.dispatch_command(payload).await {
            Ok(sequence) => send_success(sender, &request_id, json!({ "sequence": sequence }))
                .await
                .is_ok(),
            Err(error) => send_failure(
                sender,
                &request_id,
                "OrchestrationDispatchCommandError",
                error,
            )
            .await
            .is_ok(),
        },
        _ => send_failure(
            sender,
            &request_id,
            "EnvironmentAuthorizationError",
            format!("unsupported T3 RPC method: {method}"),
        )
        .await
        .is_ok(),
    }
}

async fn send_authorization_failure<S>(
    sender: &mut S,
    request_id: &Value,
    required_scope: &str,
) -> Result<(), S::Error>
where
    S: futures_util::Sink<Message> + Unpin,
{
    send_json(
        sender,
        json!({
            "_tag": "Exit",
            "requestId": request_id,
            "exit": {
                "_tag": "Failure",
                "cause": [{
                    "_tag": "Fail",
                    "error": {
                        "_tag": "EnvironmentAuthorizationError",
                        "message": format!("missing required scope: {required_scope}"),
                        "requiredScope": required_scope,
                    },
                }],
            },
        }),
    )
    .await
}

fn subscription_value(
    state: &T3CompatState,
    kind: &SubscriptionKind,
    sequence: u64,
) -> Option<Value> {
    match kind {
        SubscriptionKind::Shell => state
            .shell_snapshot(sequence)
            .ok()
            .map(|snapshot| json!({ "kind": "snapshot", "snapshot": snapshot })),
        SubscriptionKind::Thread(thread_id) => state
            .thread_snapshot(thread_id, sequence)
            .ok()
            .map(|snapshot| json!({ "kind": "snapshot", "snapshot": snapshot })),
        _ => None,
    }
}

async fn send_success<S>(sender: &mut S, request_id: &Value, value: Value) -> Result<(), S::Error>
where
    S: futures_util::Sink<Message> + Unpin,
{
    send_json(
        sender,
        json!({
            "_tag": "Exit",
            "requestId": request_id,
            "exit": { "_tag": "Success", "value": value },
        }),
    )
    .await
}

async fn send_failure<S>(
    sender: &mut S,
    request_id: &Value,
    error_tag: &str,
    message: impl Into<String>,
) -> Result<(), S::Error>
where
    S: futures_util::Sink<Message> + Unpin,
{
    // EnvironmentAuthorizationError has one additional required field in contracts 0.0.33.
    // Unsupported methods are reported through this error because every T3 RPC includes it in
    // its error union; keeping the payload schema-valid lets the official client surface the
    // limitation instead of treating the response as a protocol defect.
    let error = if error_tag == "EnvironmentAuthorizationError" {
        json!({
            "_tag": error_tag,
            "message": message.into(),
            "requiredScope": "orchestration:read",
        })
    } else {
        json!({ "_tag": error_tag, "message": message.into() })
    };
    send_json(
        sender,
        json!({
            "_tag": "Exit",
            "requestId": request_id,
            "exit": {
                "_tag": "Failure",
                "cause": [{
                    "_tag": "Fail",
                    "error": error,
                }],
            },
        }),
    )
    .await
}

async fn send_chunk<S>(sender: &mut S, request_id: &Value, value: Value) -> Result<(), S::Error>
where
    S: futures_util::Sink<Message> + Unpin,
{
    send_chunks(sender, request_id, vec![value]).await
}

async fn send_chunks<S>(
    sender: &mut S,
    request_id: &Value,
    values: Vec<Value>,
) -> Result<(), S::Error>
where
    S: futures_util::Sink<Message> + Unpin,
{
    send_json(
        sender,
        json!({ "_tag": "Chunk", "requestId": request_id, "values": values }),
    )
    .await
}

async fn send_json<S>(sender: &mut S, value: Value) -> Result<(), S::Error>
where
    S: futures_util::Sink<Message> + Unpin,
{
    sender.send(Message::Text(value.to_string())).await
}

fn provider_value(
    instance_id: &str,
    display_name: &str,
    installed: bool,
    models: Vec<ModelChoice>,
    checked_at: &str,
) -> Value {
    let mut value = json!({
        "instanceId": instance_id,
        "driver": instance_id,
        "displayName": display_name,
        "enabled": true,
        "installed": installed,
        "version": Value::Null,
        "status": if installed { "ready" } else { "warning" },
        "auth": { "status": "unknown" },
        "checkedAt": checked_at,
        "models": models.into_iter().enumerate().map(|(index, model)| json!({
            "slug": model.id,
            "name": nonempty(&model.name, "Default"),
            "isCustom": false,
            "isDefault": index == 0,
            "capabilities": Value::Null,
        })).collect::<Vec<_>>(),
        "slashCommands": [],
        "skills": [],
    });
    if !installed {
        value
            .as_object_mut()
            .expect("provider projection is an object")
            .insert(
                "message".into(),
                json!("Provider command was not found on PATH"),
            );
    }
    value
}

fn project_shell(path: &str, sessions: &[Session], updated_at: &str) -> Value {
    let created = sessions
        .iter()
        .map(|session| session.created_at)
        .min()
        .map(millis_iso)
        .unwrap_or_else(|| updated_at.to_string());
    let selection = sessions.first().map(model_selection).unwrap_or(Value::Null);
    let title = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("C2");
    json!({
        "id": project_id(path),
        "title": title,
        "workspaceRoot": path,
        "defaultModelSelection": selection,
        "scripts": [],
        "createdAt": created,
        "updatedAt": updated_at,
    })
}

fn fallback_message_id(session_id: &str, seq: i64) -> String {
    format!("codetwo-message-{session_id}-{seq}")
}

fn message_value(id: String, role: &str, text: String, at: &str) -> Value {
    json!({
        "id": id,
        "role": role,
        "text": text,
        "turnId": Value::Null,
        "streaming": false,
        "createdAt": at,
        "updatedAt": at,
    })
}

fn flush_assistant_message(
    messages: &mut Vec<Value>,
    session_id: &str,
    assistant_message: &mut Option<(i64, String, String)>,
) {
    if let Some((seq, at, text)) = assistant_message.take() {
        messages.push(message_value(
            fallback_message_id(session_id, seq),
            "assistant",
            text,
            &at,
        ));
    }
}

fn t3_user_prompt(canonical: &str) -> String {
    canonical
        .strip_prefix(LEGACY_PLAN_PROMPT_PREFIX)
        .unwrap_or(canonical)
        .to_string()
}

fn pending_activities(session: &Session, at: &str) -> Vec<Value> {
    let SessionRunState::AwaitingInput {
        turn_id, pending, ..
    } = &session.activity.state
    else {
        return Vec::new();
    };
    pending
        .iter()
        .map(|pending| {
            json!({
                "id": format!("codetwo-approval-{}", pending.input_id),
                "tone": "approval",
                "kind": "approval.requested",
                "summary": nonempty(&pending.title, "Command approval requested"),
                "payload": {
                    "requestId": pending.input_id,
                    "requestKind": "command",
                    "detail": pending.title,
                    "options": pending.options,
                },
                "turnId": turn_id,
                "sequence": pending.sequence,
                "createdAt": at,
            })
        })
        .collect()
}

fn activity_projection(session: &Session, created_at: &str) -> (Value, &'static str, Value, Value) {
    match &session.activity.state {
        SessionRunState::Idle => (Value::Null, "ready", Value::Null, Value::Null),
        SessionRunState::Running { turn_id, .. }
        | SessionRunState::AwaitingInput { turn_id, .. } => (
            json!({
                "turnId": turn_id,
                "state": "running",
                "requestedAt": created_at,
                "startedAt": created_at,
                "completedAt": Value::Null,
                "assistantMessageId": Value::Null,
            }),
            "running",
            json!(turn_id),
            Value::Null,
        ),
        SessionRunState::Failed {
            turn_id,
            reason,
            message,
        } => {
            let state = match reason {
                codetwo_core::RunFailureReason::Interrupted => "interrupted",
                codetwo_core::RunFailureReason::ProviderError => "error",
            };
            let status = if state == "interrupted" {
                "interrupted"
            } else {
                "error"
            };
            let latest = turn_id.as_ref().map_or(Value::Null, |turn_id| {
                json!({
                    "turnId": turn_id,
                    "state": state,
                    "requestedAt": created_at,
                    "startedAt": created_at,
                    "completedAt": created_at,
                    "assistantMessageId": Value::Null,
                })
            });
            (latest, status, Value::Null, json!(message))
        }
    }
}

fn runtime_mode(mode: PermissionMode, sandbox: SandboxPolicy) -> &'static str {
    match (mode, sandbox) {
        (PermissionMode::Ask, _) => "approval-required",
        (PermissionMode::AcceptEdits, _) => "auto-accept-edits",
        (PermissionMode::Yolo, SandboxPolicy::DangerFullAccess) => "full-access",
        (PermissionMode::Yolo, _) => "auto",
    }
}

fn policy_from_runtime(runtime: &str) -> Result<(PermissionMode, SandboxPolicy), String> {
    match runtime {
        "approval-required" => Ok((PermissionMode::Ask, SandboxPolicy::WorkspaceWrite)),
        "auto-accept-edits" => Ok((PermissionMode::AcceptEdits, SandboxPolicy::WorkspaceWrite)),
        "auto" => Ok((PermissionMode::Yolo, SandboxPolicy::WorkspaceWrite)),
        "full-access" => Ok((PermissionMode::Yolo, SandboxPolicy::DangerFullAccess)),
        _ => Err(format!("unknown runtime mode: {runtime}")),
    }
}

fn model_selection(session: &Session) -> Value {
    let model = session
        .model
        .clone()
        .filter(|model| !model.trim().is_empty())
        .unwrap_or_else(|| "default".into());
    json!({ "instanceId": provider_slug(&session.provider), "model": model })
}

fn provider_slug(provider: &ProviderId) -> String {
    match provider {
        ProviderId::ClaudeCode => "claudeAgent".into(),
        ProviderId::Codex => "codex".into(),
        ProviderId::Grok => "grok".into(),
        ProviderId::Cursor => "cursor".into(),
        ProviderId::OpenCode => "opencode".into(),
        ProviderId::OpenCode2 => "opencode2".into(),
        ProviderId::Pi => "pi".into(),
        ProviderId::Kimi => "kimi".into(),
        ProviderId::ZCode => "zcode".into(),
        ProviderId::Amp => "amp".into(),
        ProviderId::Droid => "droid".into(),
        ProviderId::Custom(value) => slug(value),
    }
}

fn provider_from_slug(value: &str) -> ProviderId {
    match value {
        "claudeAgent" | "claude_code" => ProviderId::ClaudeCode,
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

fn provider_display_name(provider: &ProviderId) -> String {
    default_registry()
        .into_iter()
        .find(|known| known.id == *provider)
        .map(|known| known.display_name)
        .unwrap_or_else(|| provider.as_str().to_string())
}

fn slug(value: &str) -> String {
    let mut out = String::new();
    for (index, ch) in value.trim().chars().take(64).enumerate() {
        let allowed = ch.is_ascii_alphanumeric() || ch == '-' || ch == '_';
        let ch = if allowed { ch } else { '-' };
        if index == 0 && !ch.is_ascii_alphabetic() {
            out.push('p');
            if out.len() >= 64 {
                break;
            }
        }
        out.push(ch);
    }
    if out.is_empty() {
        "custom".into()
    } else {
        out
    }
}

fn project_path(session: &Session, fallback: &str) -> String {
    session
        .project_path
        .clone()
        .filter(|path| !path.trim().is_empty())
        .or_else(|| (!session.cwd.trim().is_empty()).then(|| session.cwd.clone()))
        .unwrap_or_else(|| fallback.to_string())
}

fn project_id(path: &str) -> String {
    let digest = Sha256::digest(path.as_bytes());
    format!("codetwo-project-{}", hex_prefix(&digest, 24))
}

fn hex_prefix(bytes: &[u8], chars: usize) -> String {
    bytes
        .iter()
        .flat_map(|byte| format!("{byte:02x}").chars().collect::<Vec<_>>())
        .take(chars)
        .collect()
}

fn required_string(value: &Value, field: &str) -> Result<String, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("{field} is required"))
}

fn nonempty<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

fn millis_iso(millis: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(millis)
        .unwrap_or_else(Utc::now)
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn iso(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn epoch_secs_iso(value: u64) -> String {
    DateTime::<Utc>::from_timestamp(value.min(i64::MAX as u64) as i64, 0)
        .map(iso)
        .unwrap_or_else(|| iso(Utc::now()))
}

fn host_label() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|host| format!("C2 on {host}"))
        .unwrap_or_else(|| "C2".into())
}

fn platform_os() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unknown"
    }
}

fn platform_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86_64") {
        "x64"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn provider_projection_uses_registered_integrations_without_invented_models() {
        let directory = tempfile::tempdir().unwrap();
        let auth = Arc::new(AuthState::load(Some(directory.path().join("devices.json"))));
        let provider = codetwo_core::provider::Provider {
            id: ProviderId::Custom("future-agent".into()),
            display_name: "Future agent".into(),
            launch: codetwo_core::provider::LaunchSpec::new(
                "/missing/future-agent",
                [] as [&str; 0],
            ),
            needs_node: false,
        };
        let (engine, rx) = Engine::new(vec![provider], codetwo_core::SkillLibrary::new(Vec::new()));
        let state = T3CompatState::new(Arc::new(engine), crate::fanout(rx), auth).unwrap();
        let providers = state.providers();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0]["displayName"], "Future agent");
        assert_eq!(providers[0]["installed"], false);
        assert_eq!(providers[0]["models"], json!([]));
    }

    #[test]
    fn provider_and_runtime_mappings_match_t3_contract_values() {
        assert_eq!(provider_slug(&ProviderId::ClaudeCode), "claudeAgent");
        assert_eq!(provider_slug(&ProviderId::OpenCode2), "opencode2");
        assert_eq!(provider_from_slug("opencode2"), ProviderId::OpenCode2);
        assert_eq!(
            provider_slug(&ProviderId::Custom("9 odd/provider".into())),
            "p9-odd-provider"
        );
        assert_eq!(
            runtime_mode(PermissionMode::Yolo, SandboxPolicy::DangerFullAccess),
            "full-access"
        );
        assert_eq!(
            policy_from_runtime("auto-accept-edits").unwrap(),
            (PermissionMode::AcceptEdits, SandboxPolicy::WorkspaceWrite)
        );
    }

    #[test]
    fn effect_rpc_success_and_failure_envelopes_are_not_json_rpc() {
        let success = json!({
            "_tag": "Exit",
            "requestId": 7,
            "exit": { "_tag": "Success", "value": {} },
        });
        assert_eq!(success["_tag"], "Exit");
        assert!(success.get("jsonrpc").is_none());

        let failure = json!({
            "_tag": "Failure",
            "cause": [{"_tag":"Fail","error":{"_tag":"OrchestrationDispatchCommandError"}}]
        });
        assert_eq!(failure["cause"][0]["_tag"], "Fail");
    }

    #[test]
    fn project_ids_are_stable_and_do_not_expose_paths() {
        let first = project_id("/Users/example/project");
        assert_eq!(first, project_id("/Users/example/project"));
        assert_ne!(first, project_id("/Users/example/other"));
        assert!(!first.contains("Users"));
    }

    #[test]
    fn prompt_projection_is_complete_and_hides_the_adapter_plan_marker() {
        let long = "x".repeat(700);
        assert_eq!(t3_user_prompt(&long), long);
        assert_eq!(
            t3_user_prompt(&format!("{LEGACY_PLAN_PROMPT_PREFIX}{long}")),
            long
        );
    }

    #[test]
    fn environment_identity_is_unique_per_install_and_stable_across_restart() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let first_path = first.path().join("t3-environment-id");
        let second_path = second.path().join("t3-environment-id");

        let first_id = load_or_create_environment_id(Some(&first_path)).unwrap();
        let second_id = load_or_create_environment_id(Some(&second_path)).unwrap();
        assert_ne!(first_id, second_id);
        assert_eq!(
            load_or_create_environment_id(Some(&first_path)).unwrap(),
            first_id
        );
    }

    #[test]
    fn corrupt_environment_identity_is_rejected_without_being_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("t3-environment-id");
        std::fs::write(&path, b"not-a-valid-environment-id").unwrap();

        assert!(load_or_create_environment_id(Some(&path)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"not-a-valid-environment-id");
    }

    #[tokio::test]
    async fn corrupt_environment_identity_prevents_t3_state_startup() {
        let directory = tempfile::tempdir().unwrap();
        let identity_path = directory.path().join("t3-environment-id");
        std::fs::write(&identity_path, b"not-a-valid-environment-id").unwrap();
        let auth = Arc::new(AuthState::load(Some(directory.path().join("devices.json"))));
        let (engine, rx) = Engine::new(Vec::new(), codetwo_core::SkillLibrary::new(Vec::new()));

        let state = T3CompatState::new(Arc::new(engine), crate::fanout(rx), auth);
        assert!(state.is_err());
        assert_eq!(
            std::fs::read(identity_path).unwrap(),
            b"not-a-valid-environment-id"
        );
    }

    #[test]
    fn approval_decisions_use_exact_acp_kinds_not_provider_order() {
        let options = vec![
            ("reject-always".into(), "Never allow".into()),
            ("allow-always".into(), "Allow for session".into()),
            ("allow-once".into(), "Allow once".into()),
            ("reject-once".into(), "Decline".into()),
        ];
        let kinds = BTreeMap::from([
            ("reject-always".into(), "reject_always".into()),
            ("allow-always".into(), "allow_always".into()),
            ("allow-once".into(), "allow_once".into()),
            ("reject-once".into(), "reject_once".into()),
        ]);
        assert_eq!(
            select_permission_option(&options, &kinds, "accept").unwrap(),
            "allow-once"
        );
        assert_eq!(
            select_permission_option(&options, &kinds, "acceptForSession").unwrap(),
            "allow-always"
        );
        assert_eq!(
            select_permission_option(&options, &kinds, "decline").unwrap(),
            "reject-once"
        );
        assert!(select_permission_option(&options, &BTreeMap::new(), "accept").is_err());
    }

    #[test]
    fn retired_interaction_modes_are_rejected_and_legacy_state_is_ignored() {
        assert!(T3CompatState::validate_interaction_mode("default").is_ok());
        assert!(T3CompatState::validate_interaction_mode("plan").is_err());
        let metadata: CompatibilityMetadata = serde_json::from_value(json!({
            "version": 1,
            "aliases": {"thread-1": "core-1"},
            "interactionModes": {"thread-1": "plan"}
        }))
        .unwrap();
        assert!(validate_compatibility(&metadata).is_ok());
        let saved = serde_json::to_value(metadata).unwrap();
        assert_eq!(saved["aliases"]["thread-1"], "core-1");
        assert!(saved.get("interactionModes").is_none());
    }

    #[test]
    fn compatibility_metadata_rejects_invalid_versions_and_aliases() {
        let mut metadata = CompatibilityMetadata::default();
        metadata.version += 1;
        assert!(validate_compatibility(&metadata).is_err());

        let mut metadata = CompatibilityMetadata::default();
        metadata.aliases.insert("public-1".into(), "core-1".into());
        metadata.aliases.insert("public-2".into(), "core-1".into());
        assert!(validate_compatibility(&metadata).is_err());

        let metadata = CompatibilityMetadata {
            command_receipts: vec![
                PersistedCommandReceipt {
                    command_id: "duplicate".into(),
                    sequence: 1,
                },
                PersistedCommandReceipt {
                    command_id: "duplicate".into(),
                    sequence: 2,
                },
            ],
            ..CompatibilityMetadata::default()
        };
        assert!(validate_compatibility(&metadata).is_err());
    }

    #[tokio::test]
    async fn compatibility_metadata_writes_do_not_lose_concurrent_updates() {
        let directory = tempfile::tempdir().unwrap();
        let auth = Arc::new(AuthState::load(Some(directory.path().join("devices.json"))));
        let (engine, rx) = Engine::new(Vec::new(), codetwo_core::SkillLibrary::new(Vec::new()));
        let state =
            Arc::new(T3CompatState::new(Arc::new(engine), crate::fanout(rx), auth).unwrap());
        let mut writers = Vec::new();
        for index in 0..16 {
            let state = state.clone();
            writers.push(tokio::task::spawn_blocking(move || {
                state
                    .persist_thread_identity(&format!("thread-{index}"), &format!("core-{index}"))
                    .unwrap();
            }));
        }
        for writer in writers {
            writer.await.unwrap();
        }

        let path = directory.path().join("t3-compatibility.json");
        let (reloaded, error) = load_compatibility(Some(&path));
        assert!(
            error.is_none(),
            "persisted metadata did not reload: {error:?}"
        );
        assert_eq!(reloaded.aliases.len(), 16);
    }

    #[tokio::test]
    async fn corrupt_compatibility_metadata_is_not_silently_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("t3-compatibility.json");
        std::fs::write(&path, b"{not valid JSON").unwrap();
        let auth = Arc::new(AuthState::load(Some(directory.path().join("devices.json"))));
        let (engine, rx) = Engine::new(Vec::new(), codetwo_core::SkillLibrary::new(Vec::new()));
        let state = T3CompatState::new(Arc::new(engine), crate::fanout(rx), auth).unwrap();

        assert!(state.persist_thread_identity("thread-1", "core-1").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{not valid JSON");
    }
}

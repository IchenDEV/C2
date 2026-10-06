//! Protocol-neutral JSON-RPC peer and child-process owner shared by native connectors.
//!
//! Unlike [`crate::acp::Connection`] this peer knows nothing about ACP DTOs: it moves raw JSON
//! values and hands inbound notifications/requests to an [`RpcInbound`]. Codex `app-server` (which
//! omits the `"jsonrpc"` member) and C2's own sidecar IPC both ride it.
//!
//! Transmission honesty is the same contract as the ACP connection: a request that was never
//! queued for the writer is [`RpcFault::NotQueued`] (provably unsent); a queued request whose
//! connection closed is [`RpcFault::Unresolved`] (may have run); a peer error is
//! [`RpcFault::Provider`]. Transport loss is never reported as a provider rejection.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, watch};

use crate::error::RpcError;
use crate::provider_runtime::{
    RuntimeError, RuntimeProcessDiagnostics, RuntimeProtocolAnomaly, RuntimeProtocolDiagnostics,
};

const MAX_DIAGNOSTIC_CATEGORIES: usize = 16;
const MAX_DIAGNOSTIC_CATEGORY_CHARS: usize = 80;
const MAX_RECENT_METHODS: usize = 16;
/// A single inbound line larger than this closes the connection instead of exhausting memory.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// Why a request produced no usable response, with the transmission phase preserved.
#[derive(Debug, Clone)]
pub enum RpcFault {
    /// The writer was already gone; nothing was transmitted.
    NotQueued,
    /// Queued, but no answer arrived (connection closed). The peer may have run it.
    Unresolved,
    /// The peer answered with an error: definite evidence it received the request.
    Provider(RpcError),
}

impl RpcFault {
    pub fn into_runtime_error(self) -> RuntimeError {
        match self {
            RpcFault::NotQueued | RpcFault::Unresolved => RuntimeError::Closed,
            RpcFault::Provider(error) => RuntimeError::Provider {
                code: error.code,
                message: error.message,
                data: error.data,
            },
        }
    }
}

/// Inbound traffic from the peer.
#[async_trait]
pub trait RpcInbound: Send + Sync + 'static {
    /// Handled inline by the reader so ordering is preserved. Must not block on user input.
    async fn notification(&self, method: &str, params: Value);

    /// Handled on its own task, so a parked approval never stalls the read loop.
    async fn request(&self, method: &str, params: Value) -> Result<Value, RpcError>;
}

/// Wire dialect: whether frames carry `"jsonrpc":"2.0"`. Codex `app-server` omits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Versioned,
    Bare,
}

#[derive(Debug, Default)]
struct DiagnosticState {
    outbound_requests: u64,
    outbound_request_methods: BTreeMap<String, u64>,
    outbound_notifications: u64,
    outbound_notification_methods: BTreeMap<String, u64>,
    outbound_rpc_errors: u64,
    outbound_rpc_error_codes: BTreeMap<String, u64>,
    recent_outbound_methods: VecDeque<String>,
    malformed_json_lines: u64,
    ignored_notifications: u64,
    ignored_notification_methods: BTreeMap<String, u64>,
}

pub struct RpcPeer {
    tx_out: mpsc::UnboundedSender<String>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, RpcError>>>>,
    next_id: AtomicU64,
    closed_at_unix_ms: AtomicI64,
    closed: watch::Sender<bool>,
    dialect: Dialect,
    diagnostics: Mutex<DiagnosticState>,
}

impl RpcPeer {
    pub fn new<R, W>(
        reader: R,
        writer: W,
        inbound: Arc<dyn RpcInbound>,
        dialect: Dialect,
    ) -> Arc<RpcPeer>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (tx_out, rx_out) = mpsc::unbounded_channel::<String>();
        let peer = Arc::new(RpcPeer {
            tx_out,
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            closed_at_unix_ms: AtomicI64::new(0),
            closed: watch::channel(false).0,
            dialect,
            diagnostics: Mutex::new(DiagnosticState::default()),
        });
        tokio::spawn(writer_task(
            writer,
            rx_out,
            Arc::downgrade(&peer),
            peer.closed.subscribe(),
        ));
        tokio::spawn(reader_task(reader, peer.clone(), inbound));
        peer
    }

    fn frame(&self, mut object: serde_json::Map<String, Value>) -> String {
        if self.dialect == Dialect::Versioned {
            object.insert("jsonrpc".into(), json!("2.0"));
        }
        Value::Object(object).to_string()
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value, RpcFault> {
        self.record_outbound(method, true);
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        let mut object = serde_json::Map::new();
        object.insert("id".into(), json!(id));
        object.insert("method".into(), json!(method));
        if !params.is_null() {
            object.insert("params".into(), params);
        }
        let line = self.frame(object);
        {
            // Claiming/queueing is serialized with transport closure, never with network I/O.
            let mut pending = self.pending.lock().unwrap();
            if self.closed_at_unix_ms.load(Ordering::Acquire) != 0 {
                return Err(RpcFault::NotQueued);
            }
            pending.insert(id, tx);
            if self.tx_out.send(line).is_err() {
                pending.remove(&id);
                return Err(RpcFault::NotQueued);
            }
        }
        match rx.await.map_err(|_| RpcFault::Unresolved)? {
            Ok(value) => Ok(value),
            Err(error) => {
                self.record_rpc_error(error.code);
                Err(RpcFault::Provider(error))
            }
        }
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), RuntimeError> {
        self.record_outbound(method, false);
        let mut object = serde_json::Map::new();
        object.insert("method".into(), json!(method));
        if !params.is_null() {
            object.insert("params".into(), params);
        }
        let line = self.frame(object);
        let _pending = self.pending.lock().unwrap();
        if self.closed_at_unix_ms.load(Ordering::Acquire) != 0 {
            return Err(RuntimeError::Closed);
        }
        self.tx_out.send(line).map_err(|_| RuntimeError::Closed)
    }

    fn respond(&self, id: Value, result: Result<Value, RpcError>) {
        let mut object = serde_json::Map::new();
        object.insert("id".into(), id);
        match result {
            Ok(value) => {
                object.insert("result".into(), value);
            }
            Err(error) => {
                object.insert(
                    "error".into(),
                    json!({"code": error.code, "message": error.message, "data": error.data}),
                );
            }
        }
        let _ = self.tx_out.send(self.frame(object));
    }

    fn resolve(&self, id: u64, result: Result<Value, RpcError>) {
        if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
            let _ = tx.send(result);
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed_at_unix_ms.load(Ordering::Acquire) != 0
    }

    pub fn closed_at_unix_ms(&self) -> Option<i64> {
        match self.closed_at_unix_ms.load(Ordering::Acquire) {
            0 => None,
            timestamp => Some(timestamp),
        }
    }

    pub fn subscribe_closed(&self) -> watch::Receiver<bool> {
        self.closed.subscribe()
    }

    /// Close both directions. Pending requests resolve as [`RpcFault::Unresolved`].
    pub fn close(&self) {
        self.mark_closed();
    }

    fn mark_closed(&self) {
        let mut pending = self.pending.lock().unwrap();
        if self
            .closed_at_unix_ms
            .compare_exchange(
                0,
                unix_time_millis().max(1),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            // Dropping response channels: transport loss is not a provider rejection.
            pending.clear();
            self.closed.send_replace(true);
        }
    }

    pub fn protocol_diagnostics(&self) -> RuntimeProtocolDiagnostics {
        let state = self.diagnostics.lock().unwrap();
        RuntimeProtocolDiagnostics {
            outbound_requests: state.outbound_requests,
            outbound_request_methods: snapshot(&state.outbound_request_methods),
            outbound_notifications: state.outbound_notifications,
            outbound_notification_methods: snapshot(&state.outbound_notification_methods),
            outbound_rpc_errors: state.outbound_rpc_errors,
            outbound_rpc_error_codes: snapshot(&state.outbound_rpc_error_codes),
            recent_outbound_methods: state.recent_outbound_methods.iter().cloned().collect(),
            malformed_json_lines: state.malformed_json_lines,
            unhandled_session_updates: 0,
            unhandled_session_update_kinds: Vec::new(),
            ignored_notifications: state.ignored_notifications,
            ignored_notification_methods: snapshot(&state.ignored_notification_methods),
        }
    }

    /// Record an inbound notification the connector deliberately does not map (bounded category,
    /// no payload).
    pub fn record_ignored(&self, method: &str) {
        let mut state = self.diagnostics.lock().unwrap();
        state.ignored_notifications = state.ignored_notifications.saturating_add(1);
        record_category(&mut state.ignored_notification_methods, method);
    }

    fn record_outbound(&self, method: &str, request: bool) {
        let mut state = self.diagnostics.lock().unwrap();
        if request {
            state.outbound_requests = state.outbound_requests.saturating_add(1);
            record_category(&mut state.outbound_request_methods, method);
        } else {
            state.outbound_notifications = state.outbound_notifications.saturating_add(1);
            record_category(&mut state.outbound_notification_methods, method);
        }
        if state.recent_outbound_methods.len() == MAX_RECENT_METHODS {
            state.recent_outbound_methods.pop_front();
        }
        state
            .recent_outbound_methods
            .push_back(diagnostic_category(method));
    }

    fn record_rpc_error(&self, code: i64) {
        let mut state = self.diagnostics.lock().unwrap();
        state.outbound_rpc_errors = state.outbound_rpc_errors.saturating_add(1);
        record_category(&mut state.outbound_rpc_error_codes, &code.to_string());
    }

    fn record_malformed(&self) {
        let mut state = self.diagnostics.lock().unwrap();
        state.malformed_json_lines = state.malformed_json_lines.saturating_add(1);
    }
}

pub fn unix_time_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn diagnostic_category(value: &str) -> String {
    let category = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'))
        .take(MAX_DIAGNOSTIC_CATEGORY_CHARS)
        .collect::<String>();
    if category.is_empty() {
        "invalid".into()
    } else {
        category
    }
}

fn record_category(categories: &mut BTreeMap<String, u64>, value: &str) {
    let category = diagnostic_category(value);
    if let Some(count) = categories.get_mut(&category) {
        *count = count.saturating_add(1);
        return;
    }
    let category = if categories.len() < MAX_DIAGNOSTIC_CATEGORIES - 1 {
        category
    } else {
        "other".to_string()
    };
    let count = categories.entry(category).or_default();
    *count = count.saturating_add(1);
}

fn snapshot(categories: &BTreeMap<String, u64>) -> Vec<RuntimeProtocolAnomaly> {
    categories
        .iter()
        .map(|(category, count)| RuntimeProtocolAnomaly {
            category: category.clone(),
            count: *count,
        })
        .collect()
}

async fn writer_task<W>(
    mut writer: W,
    mut rx: mpsc::UnboundedReceiver<String>,
    peer: Weak<RpcPeer>,
    mut closed: watch::Receiver<bool>,
) where
    W: AsyncWrite + Unpin + Send + 'static,
{
    loop {
        if *closed.borrow() {
            break;
        }
        let line = tokio::select! {
            _ = closed.changed() => break,
            line = rx.recv() => line,
        };
        let Some(mut line) = line else { break };
        line.push('\n');
        let written = tokio::select! {
            _ = closed.changed() => break,
            result = async {
                writer.write_all(line.as_bytes()).await?;
                writer.flush().await
            } => result,
        };
        if written.is_err() {
            break;
        }
    }
    rx.close();
    if let Some(peer) = peer.upgrade() {
        peer.mark_closed();
    }
}

async fn reader_task<R>(reader: R, peer: Arc<RpcPeer>, inbound: Arc<dyn RpcInbound>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut reader = BufReader::new(reader);
    let mut closed = peer.closed.subscribe();
    let mut line = Vec::new();
    loop {
        if *closed.borrow() {
            break;
        }
        line.clear();
        let read = tokio::select! {
            _ = closed.changed() => break,
            read = read_frame(&mut reader, &mut line) => read,
        };
        match read {
            Ok(true) => {
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                match serde_json::from_slice::<Value>(&line) {
                    Ok(value) => dispatch(&peer, &inbound, value).await,
                    Err(error) => {
                        peer.record_malformed();
                        tracing::warn!("native connector: dropping non-JSON line: {error}");
                    }
                }
            }
            Ok(false) => break,
            Err(error) => {
                tracing::warn!("native connector: read error: {error}");
                break;
            }
        }
    }
    peer.mark_closed();
}

/// Read one newline-terminated frame, refusing frames above [`MAX_FRAME_BYTES`].
async fn read_frame<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    out: &mut Vec<u8>,
) -> std::io::Result<bool> {
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(!out.is_empty());
        }
        match available.iter().position(|b| *b == b'\n') {
            Some(index) => {
                out.extend_from_slice(&available[..index]);
                reader.consume(index + 1);
                return Ok(true);
            }
            None => {
                let length = available.len();
                out.extend_from_slice(available);
                reader.consume(length);
            }
        }
        if out.len() > MAX_FRAME_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "frame exceeds the connector size limit",
            ));
        }
    }
}

async fn dispatch(peer: &Arc<RpcPeer>, inbound: &Arc<dyn RpcInbound>, value: Value) {
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_string);
    let Some(method) = method else {
        // A response: id and result/error, no method.
        if let Some(id) = value.get("id").and_then(Value::as_u64) {
            if let Some(error) = value.get("error") {
                let rpc = serde_json::from_value(error.clone())
                    .unwrap_or_else(|_| RpcError::new(-1, "malformed error object"));
                peer.resolve(id, Err(rpc));
            } else {
                peer.resolve(id, Ok(value.get("result").cloned().unwrap_or(Value::Null)));
            }
        }
        return;
    };
    let params = value.get("params").cloned().unwrap_or(Value::Null);
    match value.get("id").cloned() {
        Some(id) => {
            let peer = peer.clone();
            let inbound = inbound.clone();
            tokio::spawn(async move {
                let result = inbound.request(&method, params).await;
                peer.respond(id, result);
            });
        }
        None => inbound.notification(&method, params).await,
    }
}

// ---- child process ownership ------------------------------------------------------------------

/// A spawned connector process (and, on Unix, its process group). Dropping it terminates both, so
/// Core shutdown never leaks a provider/sidecar it started. It only ever signals what it spawned.
pub struct ChildGuard {
    child: Mutex<Child>,
    #[cfg(unix)]
    process_group: Option<i32>,
    started_at_unix_ms: i64,
    terminated: AtomicBool,
}

pub struct SpawnedChild {
    pub guard: ChildGuard,
    pub stdin: tokio::process::ChildStdin,
    pub stdout: tokio::process::ChildStdout,
}

/// Spawn `command` with piped stdio in its own process group. stderr is forwarded to tracing at
/// debug level under `target`.
pub fn spawn_child(
    command: &std::path::Path,
    args: &[String],
    env: &[(String, String)],
    cwd: Option<&std::path::Path>,
    stderr_target: &'static str,
) -> Result<SpawnedChild, RuntimeError> {
    let mut cmd = Command::new(command);
    cmd.args(args);
    for (key, value) in env {
        cmd.env(key, value);
    }
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            RuntimeError::CommandNotFound(command.display().to_string())
        } else {
            RuntimeError::Spawn(error.to_string())
        }
    })?;
    let stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(target: "native.connector.stderr", connector = stderr_target, "{line}");
            }
        });
    }
    #[cfg(unix)]
    let process_group = child.id().and_then(|pid| i32::try_from(pid).ok());
    Ok(SpawnedChild {
        guard: ChildGuard {
            child: Mutex::new(child),
            #[cfg(unix)]
            process_group,
            started_at_unix_ms: unix_time_millis(),
            terminated: AtomicBool::new(false),
        },
        stdin,
        stdout,
    })
}

impl ChildGuard {
    /// Terminate without waiting, so synchronous shutdown never parks on `Child::wait`.
    pub fn terminate(&self) {
        if self.terminated.swap(true, Ordering::AcqRel) {
            return;
        }
        #[cfg(unix)]
        if let Some(group) = self.process_group {
            if let Err(error) = crate::unix_process_group::kill(group) {
                tracing::warn!(%error, "could not terminate native connector process group");
            }
        }
        if let Ok(mut child) = self.child.lock() {
            let _ = child.start_kill();
        }
    }

    pub fn process_diagnostics(&self, peer: &RpcPeer) -> RuntimeProcessDiagnostics {
        RuntimeProcessDiagnostics {
            started_at_unix_ms: self.started_at_unix_ms,
            closed_at_unix_ms: peer.closed_at_unix_ms(),
            termination_requested: self.terminated.load(Ordering::Acquire),
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.terminate();
    }
}

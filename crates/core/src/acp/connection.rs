//! A minimal JSON-RPC 2.0 peer over any async byte streams (child stdio in prod, an in-memory
//! duplex in tests). It multiplexes: outbound requests matched to responses by id, inbound
//! notifications and requests dispatched to a [`ClientHandler`].
//!
//! Generic over the reader/writer so the whole prompt-turn loop is testable offline against a mock
//! agent — no provider binary required.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, watch};

use super::handler::ClientHandler;
use super::wire::{SessionNotification, SessionUpdate};
use crate::error::{AcpError, RpcError};

const MAX_DIAGNOSTIC_CATEGORIES: usize = 16;
const MAX_DIAGNOSTIC_CATEGORY_CHARS: usize = 80;
const MAX_RECENT_RPC_METHODS: usize = 16;

/// One bounded, content-free ACP protocol anomaly category.
#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct AcpProtocolAnomaly {
    pub category: String,
    pub count: u64,
}

/// Decode/notification anomalies observed on one provider connection.
///
/// Only method and `sessionUpdate` discriminators are retained. Raw JSON, prompt text, tool
/// payloads, paths, environment variables, and error strings are deliberately excluded so this
/// snapshot is safe to feed into the default diagnostics export.
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct AcpProtocolDiagnostics {
    pub outbound_requests: u64,
    pub outbound_request_methods: Vec<AcpProtocolAnomaly>,
    pub outbound_notifications: u64,
    pub outbound_notification_methods: Vec<AcpProtocolAnomaly>,
    pub outbound_rpc_errors: u64,
    pub outbound_rpc_error_codes: Vec<AcpProtocolAnomaly>,
    pub recent_outbound_methods: Vec<String>,
    pub malformed_json_lines: u64,
    pub unhandled_session_updates: u64,
    pub unhandled_session_update_kinds: Vec<AcpProtocolAnomaly>,
    pub ignored_notifications: u64,
    pub ignored_notification_methods: Vec<AcpProtocolAnomaly>,
}

#[derive(Debug, Default)]
struct AcpProtocolDiagnosticState {
    outbound_requests: u64,
    outbound_request_methods: BTreeMap<String, u64>,
    outbound_notifications: u64,
    outbound_notification_methods: BTreeMap<String, u64>,
    outbound_rpc_errors: u64,
    outbound_rpc_error_codes: BTreeMap<String, u64>,
    recent_outbound_methods: VecDeque<String>,
    malformed_json_lines: u64,
    unhandled_session_updates: u64,
    unhandled_session_update_kinds: BTreeMap<String, u64>,
    ignored_notifications: u64,
    ignored_notification_methods: BTreeMap<String, u64>,
}

/// Why a request produced no usable response, with the transmission phase preserved.
#[derive(Debug)]
pub enum RequestFault {
    /// The writer was already gone, so the request was never queued and nothing was transmitted.
    NotQueued,
    /// The request was queued for the writer but no response arrived (the connection closed).
    /// Whether the peer saw it is unknown.
    Unresolved,
    /// The peer answered with a JSON-RPC error: definite evidence that it received the request.
    Provider(RpcError),
    /// The peer answered, but the result could not be decoded.
    Decode(serde_json::Error),
}

pub struct Connection {
    tx_out: mpsc::UnboundedSender<String>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, RpcError>>>>,
    next_id: AtomicU64,
    closed_at_unix_ms: AtomicI64,
    closed: watch::Sender<bool>,
    diagnostics: Mutex<AcpProtocolDiagnosticState>,
}

impl Connection {
    /// Wire a connection over the given byte streams, dispatching agent callbacks to `handler`.
    /// Spawns a reader task and a writer task; the returned handle sends requests/notifications.
    pub fn new<R, W>(reader: R, writer: W, handler: Arc<dyn ClientHandler>) -> Arc<Connection>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (tx_out, rx_out) = mpsc::unbounded_channel::<String>();
        let conn = Arc::new(Connection {
            tx_out,
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            closed_at_unix_ms: AtomicI64::new(0),
            closed: watch::channel(false).0,
            diagnostics: Mutex::new(AcpProtocolDiagnosticState::default()),
        });
        tokio::spawn(writer_task(
            writer,
            rx_out,
            Arc::downgrade(&conn),
            conn.closed.subscribe(),
        ));
        tokio::spawn(reader_task(reader, conn.clone(), handler));
        conn
    }

    /// Send a request and await the typed response.
    pub async fn request<P, R>(&self, method: &str, params: P) -> Result<R, AcpError>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        self.request_phased(method, params)
            .await
            .map_err(|fault| match fault {
                RequestFault::NotQueued | RequestFault::Unresolved => AcpError::Closed,
                RequestFault::Provider(error) => AcpError::Rpc(error),
                RequestFault::Decode(error) => AcpError::Decode(error),
            })
    }

    /// Like [`Connection::request`], but keeps the transmission phase of a failure. A request that
    /// was never queued for the writer is provably unsent; a queued request whose response channel
    /// closed may or may not have reached the peer and must not be replayed blindly.
    pub async fn request_phased<P, R>(&self, method: &str, params: P) -> Result<R, RequestFault>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        self.record_outbound_request(method);
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        {
            // Serialize claiming/queueing with transport closure, never with network I/O.
            let mut pending = self.pending.lock().unwrap();
            if self.closed_at_unix_ms.load(Ordering::Acquire) != 0 {
                return Err(RequestFault::NotQueued);
            }
            pending.insert(id, tx);
            if self.tx_out.send(msg.to_string()).is_err() {
                pending.remove(&id);
                return Err(RequestFault::NotQueued);
            }
        }
        match rx.await.map_err(|_| RequestFault::Unresolved)? {
            Ok(v) => serde_json::from_value(v).map_err(RequestFault::Decode),
            Err(e) => {
                self.record_outbound_rpc_error(e.code);
                Err(RequestFault::Provider(e))
            }
        }
    }

    /// Fire-and-forget notification.
    pub fn notify<P: Serialize>(&self, method: &str, params: P) -> Result<(), AcpError> {
        self.record_outbound_notification(method);
        let msg = json!({"jsonrpc": "2.0", "method": method, "params": params});
        let _pending = self.pending.lock().unwrap();
        if self.closed_at_unix_ms.load(Ordering::Acquire) != 0 {
            return Err(AcpError::Closed);
        }
        self.tx_out
            .send(msg.to_string())
            .map_err(|_| AcpError::Closed)
    }

    fn resolve(&self, id: u64, result: Result<Value, RpcError>) {
        if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
            let _ = tx.send(result);
        }
    }

    pub fn protocol_diagnostics(&self) -> AcpProtocolDiagnostics {
        let state = self.diagnostics.lock().unwrap();
        AcpProtocolDiagnostics {
            outbound_requests: state.outbound_requests,
            outbound_request_methods: anomaly_snapshot(&state.outbound_request_methods),
            outbound_notifications: state.outbound_notifications,
            outbound_notification_methods: anomaly_snapshot(&state.outbound_notification_methods),
            outbound_rpc_errors: state.outbound_rpc_errors,
            outbound_rpc_error_codes: anomaly_snapshot(&state.outbound_rpc_error_codes),
            recent_outbound_methods: state.recent_outbound_methods.iter().cloned().collect(),
            malformed_json_lines: state.malformed_json_lines,
            unhandled_session_updates: state.unhandled_session_updates,
            unhandled_session_update_kinds: anomaly_snapshot(&state.unhandled_session_update_kinds),
            ignored_notifications: state.ignored_notifications,
            ignored_notification_methods: anomaly_snapshot(&state.ignored_notification_methods),
        }
    }

    pub fn closed_at_unix_ms(&self) -> Option<i64> {
        match self.closed_at_unix_ms.load(Ordering::Acquire) {
            0 => None,
            timestamp => Some(timestamp),
        }
    }

    fn record_outbound_request(&self, method: &str) {
        let mut state = self.diagnostics.lock().unwrap();
        state.outbound_requests = state.outbound_requests.saturating_add(1);
        record_category(&mut state.outbound_request_methods, Some(method));
        if state.recent_outbound_methods.len() == MAX_RECENT_RPC_METHODS {
            state.recent_outbound_methods.pop_front();
        }
        state
            .recent_outbound_methods
            .push_back(diagnostic_category(Some(method)));
    }

    fn record_outbound_rpc_error(&self, code: i64) {
        let mut state = self.diagnostics.lock().unwrap();
        state.outbound_rpc_errors = state.outbound_rpc_errors.saturating_add(1);
        record_category(&mut state.outbound_rpc_error_codes, Some(&code.to_string()));
    }

    fn record_outbound_notification(&self, method: &str) {
        let mut state = self.diagnostics.lock().unwrap();
        state.outbound_notifications = state.outbound_notifications.saturating_add(1);
        record_category(&mut state.outbound_notification_methods, Some(method));
        if state.recent_outbound_methods.len() == MAX_RECENT_RPC_METHODS {
            state.recent_outbound_methods.pop_front();
        }
        state
            .recent_outbound_methods
            .push_back(diagnostic_category(Some(method)));
    }

    fn record_malformed_json_line(&self) {
        let mut state = self.diagnostics.lock().unwrap();
        state.malformed_json_lines = state.malformed_json_lines.saturating_add(1);
    }

    fn record_unhandled_session_update(&self, category: Option<&str>) {
        let mut state = self.diagnostics.lock().unwrap();
        state.unhandled_session_updates = state.unhandled_session_updates.saturating_add(1);
        record_category(&mut state.unhandled_session_update_kinds, category);
    }

    fn record_ignored_notification(&self, method: &str) {
        let mut state = self.diagnostics.lock().unwrap();
        state.ignored_notifications = state.ignored_notifications.saturating_add(1);
        record_category(&mut state.ignored_notification_methods, Some(method));
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
            // Drop response channels: local transport loss is not a provider RPC rejection.
            pending.clear();
            self.closed.send_replace(true);
        }
    }
}

fn unix_time_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn diagnostic_category(value: Option<&str>) -> String {
    let value = value.unwrap_or("missing");
    let category = value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '/')
        })
        .take(MAX_DIAGNOSTIC_CATEGORY_CHARS)
        .collect::<String>();
    if category.is_empty() {
        "invalid".to_string()
    } else {
        category
    }
}

fn record_category(categories: &mut BTreeMap<String, u64>, value: Option<&str>) {
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

fn anomaly_snapshot(categories: &BTreeMap<String, u64>) -> Vec<AcpProtocolAnomaly> {
    categories
        .iter()
        .map(|(category, count)| AcpProtocolAnomaly {
            category: category.clone(),
            count: *count,
        })
        .collect()
}

async fn writer_task<W>(
    mut writer: W,
    mut rx: mpsc::UnboundedReceiver<String>,
    connection: Weak<Connection>,
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
        let Some(mut line) = line else {
            break;
        };
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
    if let Some(connection) = connection.upgrade() {
        connection.mark_closed();
    }
}

async fn reader_task<R>(reader: R, conn: Arc<Connection>, handler: Arc<dyn ClientHandler>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut lines = BufReader::new(reader).lines();
    let mut closed = conn.closed.subscribe();
    loop {
        if *closed.borrow() {
            break;
        }
        let line = tokio::select! {
            _ = closed.changed() => break,
            line = lines.next_line() => line,
        };
        match line {
            Ok(Some(line)) => {
                if line.trim().is_empty() {
                    continue;
                }
                let val: Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(e) => {
                        conn.record_malformed_json_line();
                        tracing::warn!("acp: dropping non-JSON line: {e}");
                        continue;
                    }
                };
                dispatch(&conn, &handler, val).await;
            }
            Ok(None) => break,
            Err(e) => {
                tracing::warn!("acp: read error: {e}");
                break;
            }
        }
    }
    conn.mark_closed();
}

/// Route one decoded message. Responses resolve pending requests; notifications are handled inline
/// (to preserve update ordering); inbound requests are spawned (so a slow permission prompt doesn't
/// stall the read loop).
async fn dispatch(conn: &Arc<Connection>, handler: &Arc<dyn ClientHandler>, val: Value) {
    // A response has an id and no method.
    if val.get("method").is_none() {
        if let Some(id) = val.get("id").and_then(|v| v.as_u64()) {
            if let Some(err) = val.get("error") {
                let rpc = serde_json::from_value(err.clone())
                    .unwrap_or_else(|_| RpcError::new(-1, "malformed error object"));
                conn.resolve(id, Err(rpc));
            } else {
                conn.resolve(id, Ok(val.get("result").cloned().unwrap_or(Value::Null)));
            }
        }
        return;
    }

    let method = val
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    let params = val.get("params").cloned().unwrap_or(Value::Null);

    match val.get("id").cloned() {
        Some(id) => {
            // Inbound request: handle off-thread, reply with the matching id.
            let conn = conn.clone();
            let handler = handler.clone();
            tokio::spawn(async move {
                let resp = match handle_request(handler.as_ref(), &method, params).await {
                    Ok(v) => json!({"jsonrpc": "2.0", "id": id, "result": v}),
                    Err(e) => json!({"jsonrpc": "2.0", "id": id,
                        "error": {"code": e.code, "message": e.message, "data": e.data}}),
                };
                let _ = conn.tx_out.send(resp.to_string());
            });
        }
        None => handle_notification(conn.as_ref(), handler.as_ref(), &method, params).await,
    }
}

async fn handle_request(
    handler: &dyn ClientHandler,
    method: &str,
    params: Value,
) -> Result<Value, RpcError> {
    match method {
        "session/request_permission" => {
            let req = serde_json::from_value(params).map_err(RpcError::invalid_params)?;
            let resp = handler.request_permission(req).await;
            serde_json::to_value(resp).map_err(RpcError::invalid_params)
        }
        "elicitation/create" => {
            let req = serde_json::from_value(params).map_err(RpcError::invalid_params)?;
            let resp = handler.create_elicitation(req).await;
            serde_json::to_value(resp).map_err(RpcError::invalid_params)
        }
        "fs/read_text_file" => {
            let req = serde_json::from_value(params).map_err(RpcError::invalid_params)?;
            let resp = handler.read_text_file(req).await?;
            serde_json::to_value(resp).map_err(RpcError::invalid_params)
        }
        "fs/write_text_file" => {
            let req = serde_json::from_value(params).map_err(RpcError::invalid_params)?;
            handler.write_text_file(req).await?;
            Ok(Value::Null)
        }
        other => Err(RpcError::method_not_found(other)),
    }
}

async fn handle_notification(
    conn: &Connection,
    handler: &dyn ClientHandler,
    method: &str,
    params: Value,
) {
    match method {
        "session/update" => {
            let session_id = params
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let update_val = params.get("update").cloned().unwrap_or(Value::Null);
            match serde_json::from_value::<SessionUpdate>(update_val) {
                Ok(update) => {
                    handler
                        .session_update(SessionNotification { session_id, update })
                        .await
                }
                // Unknown update variant: log and continue (code to the common denominator).
                Err(e) => {
                    let category = params
                        .get("update")
                        .and_then(|update| update.get("sessionUpdate"))
                        .and_then(Value::as_str);
                    conn.record_unhandled_session_update(category);
                    tracing::debug!("acp: unhandled session update: {e}");
                }
            }
        }
        // Cursor exposes provider-owned subagents through its documented ACP extension instead
        // of a session/update variant. Project the completion into the ordinary tool-call path;
        // Cursor remains responsible for spawning, running, and resuming the child agent.
        "cursor/task" => {
            let Some(tool_call_id) = params
                .get("toolCallId")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
            else {
                tracing::debug!("acp: ignoring cursor/task without toolCallId");
                return;
            };
            let raw_input = ["subagentType", "description", "prompt", "model"]
                .into_iter()
                .filter_map(|key| {
                    params
                        .get(key)
                        .cloned()
                        .map(|value| (key.to_string(), value))
                })
                .collect();
            handler
                .session_update(SessionNotification {
                    session_id: params
                        .get("sessionId")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    update: SessionUpdate::ToolCall(super::wire::ToolCall {
                        tool_call_id: tool_call_id.to_string(),
                        title: Some("cursor/task".into()),
                        kind: Some("agent".into()),
                        status: Some("completed".into()),
                        content: None,
                        raw_input: Some(Value::Object(raw_input)),
                        raw_output: None,
                        meta: None,
                    }),
                })
                .await;
        }
        other => {
            conn.record_ignored_notification(other);
            tracing::debug!("acp: ignoring notification {other}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::RecordingHandler;

    fn test_connection() -> Arc<Connection> {
        let (tx_out, _rx_out) = mpsc::unbounded_channel();
        Arc::new(Connection {
            tx_out,
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            closed_at_unix_ms: AtomicI64::new(0),
            closed: watch::channel(false).0,
            diagnostics: Mutex::new(AcpProtocolDiagnosticState::default()),
        })
    }

    #[tokio::test]
    async fn unknown_protocol_categories_are_counted_without_retaining_payloads() {
        let conn = test_connection();
        let handler: Arc<dyn ClientHandler> = Arc::new(RecordingHandler::default());
        dispatch(
            &conn,
            &handler,
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": {
                    "sessionId": "private-session",
                    "update": {
                        "sessionUpdate": "sub_agent_completed",
                        "secret": "must-not-survive"
                    }
                }
            }),
        )
        .await;
        dispatch(
            &conn,
            &handler,
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "provider/private_notice",
                "params": {"token": "must-not-survive"}
            }),
        )
        .await;

        let snapshot = conn.protocol_diagnostics();
        assert_eq!(snapshot.unhandled_session_updates, 1);
        assert_eq!(
            snapshot.unhandled_session_update_kinds,
            vec![AcpProtocolAnomaly {
                category: "sub_agent_completed".into(),
                count: 1,
            }]
        );
        assert_eq!(snapshot.ignored_notifications, 1);
        assert_eq!(
            snapshot.ignored_notification_methods[0].category,
            "provider/private_notice"
        );
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(!json.contains("private-session"));
        assert!(!json.contains("must-not-survive"));
    }

    #[test]
    fn diagnostic_categories_are_bounded() {
        let conn = test_connection();
        for index in 0..100 {
            conn.record_unhandled_session_update(Some(&format!("future_update_{index}")));
        }
        let snapshot = conn.protocol_diagnostics();
        assert_eq!(snapshot.unhandled_session_updates, 100);
        assert_eq!(
            snapshot.unhandled_session_update_kinds.len(),
            MAX_DIAGNOSTIC_CATEGORIES
        );
        assert!(snapshot
            .unhandled_session_update_kinds
            .iter()
            .any(|item| item.category == "other" && item.count == 85));
    }

    #[test]
    fn outbound_rpc_diagnostics_keep_only_methods_and_error_codes() {
        let conn = test_connection();
        conn.record_outbound_request("session/prompt");
        conn.record_outbound_rpc_error(-32603);
        let _ = conn.notify(
            "session/cancel",
            serde_json::json!({"sessionId": "private-session", "token": "must-not-survive"}),
        );

        let snapshot = conn.protocol_diagnostics();
        assert_eq!(snapshot.outbound_requests, 1);
        assert_eq!(snapshot.outbound_rpc_errors, 1);
        assert_eq!(snapshot.outbound_notifications, 1);
        assert_eq!(
            snapshot.recent_outbound_methods,
            vec!["session/prompt", "session/cancel"]
        );
        assert_eq!(snapshot.outbound_rpc_error_codes[0].category, "-32603");
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(!json.contains("private-session"));
        assert!(!json.contains("must-not-survive"));
    }
}

#[cfg(test)]
mod cursor_tests {
    use serde_json::json;

    use super::*;
    use crate::acp::wire::ToolCall;

    #[derive(Default)]
    struct Handler(Mutex<Vec<SessionNotification>>);

    #[async_trait::async_trait]
    impl ClientHandler for Handler {
        async fn session_update(&self, note: SessionNotification) {
            self.0.lock().unwrap().push(note);
        }
    }

    #[tokio::test]
    async fn cursor_native_subagent_notification_uses_tool_call_path() {
        let (tx_out, _rx_out) = tokio::sync::mpsc::unbounded_channel();
        let conn = Connection {
            tx_out,
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            closed_at_unix_ms: AtomicI64::new(0),
            closed: watch::channel(false).0,
            diagnostics: Mutex::new(AcpProtocolDiagnosticState::default()),
        };
        let handler = Arc::new(Handler::default());
        handle_notification(
            &conn,
            handler.as_ref(),
            "cursor/task",
            json!({
                "toolCallId": "cursor-child-1",
                "description": "Explore authentication",
                "prompt": "Find the authentication entry points",
                "subagentType": "explore",
                "agentId": "private-provider-id",
                "durationMs": 42
            }),
        )
        .await;

        let notes = handler.0.lock().unwrap();
        let SessionUpdate::ToolCall(ToolCall {
            tool_call_id,
            kind,
            status,
            raw_input: Some(raw_input),
            ..
        }) = &notes[0].update
        else {
            panic!("cursor/task should become an agent tool call");
        };
        assert_eq!(tool_call_id, "cursor-child-1");
        assert_eq!(kind.as_deref(), Some("agent"));
        assert_eq!(status.as_deref(), Some("completed"));
        assert_eq!(raw_input["subagentType"], "explore");
        assert!(raw_input.get("agentId").is_none());
        assert!(raw_input.get("durationMs").is_none());
    }
}

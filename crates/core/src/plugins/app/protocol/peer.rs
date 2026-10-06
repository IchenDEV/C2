//! Bounded JSON-RPC transport shared by every process extension.

use crate::plugins::app::protocol::wire::{EventParams, InvokeParams, LogParams};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinSet;

pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
pub const MAX_PENDING_REQUESTS: usize = 64;
pub const MAX_OUTBOUND_MESSAGES: usize = 64;
pub const MAX_HOST_CALLBACKS: usize = 16;
pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

#[async_trait::async_trait]
pub trait HostHandler: Send + Sync + 'static {
    async fn call(&self, name: &str, args: Value) -> Result<Value, String>;
    async fn emit(&self, name: &str, payload: Value);
    /// Host-only `observation/record`. Not an extension-public command.
    async fn observe(&self, _params: Value) -> Result<Value, String> {
        Err("the host does not accept observations".into())
    }
    fn log(&self, level: &str, message: &str);
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("the plugin process is not responding")]
    Closed,
    #[error("{0}")]
    Remote(String),
    #[error("the plugin sent something unreadable: {0}")]
    Decode(String),
    #[error("plugin protocol capacity exceeded: {0}")]
    Overloaded(&'static str),
    #[error("plugin protocol message exceeds the 1 MiB limit")]
    MessageTooLarge,
    #[error("plugin request `{0}` timed out")]
    Timeout(String),
}

type Shutdown = Box<dyn FnOnce(String) + Send>;

pub struct Peer {
    outbound: mpsc::Sender<String>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>,
    next_id: AtomicU64,
    timeout: Duration,
    closed: watch::Sender<Option<String>>,
    shutdown: Mutex<Option<Shutdown>>,
}

/// A dropped caller must not leave an invisible operation running in the child. Cancellation
/// terminates this peer/realm, including concurrent calls, since protocol 1.0 has no cancel ack.
struct PendingRequest<'a> {
    peer: &'a Peer,
    id: u64,
    sent: bool,
}

impl Drop for PendingRequest<'_> {
    fn drop(&mut self) {
        let pending = self.peer.pending.lock().unwrap().remove(&self.id).is_some();
        if pending && self.sent {
            self.peer
                .close("plugin request cancelled; reload the plugin to retry".into());
        }
    }
}

impl Peer {
    pub fn new<R, W>(reader: R, writer: W, handler: Arc<dyn HostHandler>) -> Arc<Peer>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        Self::managed(
            reader,
            writer,
            handler,
            DEFAULT_COMMAND_TIMEOUT,
            Box::new(|_| {}),
        )
    }

    pub(super) fn managed<R, W>(
        reader: R,
        writer: W,
        handler: Arc<dyn HostHandler>,
        timeout: Duration,
        shutdown: Shutdown,
    ) -> Arc<Peer>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (outbound, rx) = mpsc::channel(MAX_OUTBOUND_MESSAGES);
        let peer = Arc::new(Peer {
            outbound,
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            timeout,
            closed: watch::channel(None).0,
            shutdown: Mutex::new(Some(shutdown)),
        });
        tokio::spawn(writer_task(writer, rx, peer.clone()));
        tokio::spawn(reader_task(reader, peer.clone(), handler));
        peer
    }

    pub(super) fn close(&self, reason: String) {
        // Only the first closer owns shutdown and the diagnostic. Never hold a lock while
        // invoking process teardown or failing callers.
        let shutdown = self.shutdown.lock().unwrap().take();
        if let Some(shutdown) = shutdown {
            self.closed.send_replace(Some(reason.clone()));
            let pending = std::mem::take(&mut *self.pending.lock().unwrap());
            shutdown(reason.clone());
            for (_, tx) in pending {
                let _ = tx.send(Err(reason.clone()));
            }
        }
    }

    fn send(&self, message: Value) -> Result<(), ProtocolError> {
        if let Some(reason) = self.closed.borrow().as_ref() {
            return Err(ProtocolError::Remote(reason.clone()));
        }
        let encoded = message.to_string();
        if encoded.len() + 1 > MAX_MESSAGE_BYTES {
            return Err(ProtocolError::MessageTooLarge);
        }
        self.outbound
            .try_send(encoded)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => ProtocolError::Overloaded("outbound queue"),
                mpsc::error::TrySendError::Closed(_) => ProtocolError::Closed,
            })
    }

    pub async fn request<P, R>(&self, method: &str, params: P) -> Result<R, ProtocolError>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        self.request_with_timeout(method, params, self.timeout)
            .await
    }

    pub(super) async fn request_with_timeout<P, R>(
        &self,
        method: &str,
        params: P,
        timeout: Duration,
    ) -> Result<R, ProtocolError>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().unwrap();
            if pending.len() >= MAX_PENDING_REQUESTS {
                return Err(ProtocolError::Overloaded("pending requests"));
            }
            pending.insert(id, tx);
        }
        let mut guard = PendingRequest {
            peer: self,
            id,
            sent: false,
        };
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        guard.sent = true;
        let result = tokio::time::timeout(timeout, rx).await;
        guard.sent = false;
        match result {
            Err(_) => {
                let error = ProtocolError::Timeout(method.into());
                self.close(format!("{error}; reload the plugin to retry"));
                Err(error)
            }
            Ok(result) => match result.map_err(|_| ProtocolError::Closed)? {
                Ok(value) => {
                    serde_json::from_value(value).map_err(|e| ProtocolError::Decode(e.to_string()))
                }
                Err(message) => Err(ProtocolError::Remote(message)),
            },
        }
    }

    pub fn notify<P: Serialize>(&self, method: &str, params: P) {
        if let Err(error) =
            self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
        {
            self.close(error.to_string());
        }
    }

    fn resolve(&self, id: u64, result: Result<Value, String>) {
        if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
            let _ = tx.send(result);
        }
    }
}

async fn writer_task<W>(mut writer: W, mut rx: mpsc::Receiver<String>, peer: Arc<Peer>)
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut closed = peer.closed.subscribe();
    loop {
        if closed.borrow().is_some() {
            break;
        }
        tokio::select! {
            biased;
            _ = closed.changed() => break,
            result = async {
                let mut line = rx.recv().await.ok_or(())?;
                line.push('\n');
                writer.write_all(line.as_bytes()).await.map_err(|_| ())?;
                writer.flush().await.map_err(|_| ())
            } => if result.is_err() {
                peer.close("plugin transport write failed".into());
                break;
            },
        }
    }
}

async fn reader_task<R>(reader: R, peer: Arc<Peer>, handler: Arc<dyn HostHandler>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut reader = BufReader::new(reader);
    let mut closed = peer.closed.subscribe();
    let mut callbacks = JoinSet::new();
    loop {
        if closed.borrow().is_some() {
            break;
        }
        // Take caps allocation even if a child never emits a newline.
        let mut frame = Vec::new();
        let mut bounded = (&mut reader).take((MAX_MESSAGE_BYTES + 1) as u64);
        let result = tokio::select! {
            biased;
            _ = closed.changed() => break,
            result = bounded.read_until(b'\n', &mut frame) => result,
        };
        match result {
            Ok(0) | Err(_) => {
                peer.close("the plugin process exited".into());
                break;
            }
            Ok(_) if frame.len() > MAX_MESSAGE_BYTES => {
                peer.close(ProtocolError::MessageTooLarge.to_string());
                break;
            }
            Ok(_) => {}
        }
        if frame.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let value = match serde_json::from_slice::<Value>(&frame) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!("plugin protocol: dropping non-JSON line: {error}");
                continue;
            }
        };
        while callbacks.try_join_next().is_some() {}
        if value.get("method").is_none() {
            if let Some(id) = value.get("id").and_then(Value::as_u64) {
                let result = match value.get("error") {
                    Some(error) => Err(error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("the plugin refused without a reason")
                        .to_string()),
                    None => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
                };
                peer.resolve(id, result);
            }
            continue;
        }
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let params = value.get("params").cloned().unwrap_or(Value::Null);
        if let Some(id) = value.get("id").cloned() {
            if callbacks.len() >= MAX_HOST_CALLBACKS {
                if method == "observation/record" {
                    peer.send(json!({"jsonrpc":"2.0","id":id,"result":{
                        "status":"backpressure","batch_id":params.get("batch_id").and_then(Value::as_str).unwrap_or_default(),
                        "current_checkpoint":null,"event_receipts":[],"retry_after_ms":1000,"reason":"host_callbacks"
                    }})).ok();
                    continue;
                }
                peer.close(ProtocolError::Overloaded("host callbacks").to_string());
                break;
            }
            let peer = peer.clone();
            let handler = handler.clone();
            callbacks.spawn(async move {
                let result = tokio::time::timeout(peer.timeout, handle_request(handler.as_ref(), &method, params)).await;
                let response = match result {
                    Ok(Ok(result)) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                    Ok(Err(message)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": message } }),
                    Err(_) => { peer.close("plugin host callback timed out".into()); return; }
                };
                if let Err(error) = peer.send(response) { peer.close(error.to_string()); }
            });
        } else {
            tokio::select! {
                biased;
                _ = closed.changed() => break,
                result = tokio::time::timeout(peer.timeout, handle_notification(handler.as_ref(), &method, params)) => {
                    if result.is_err() { peer.close("plugin notification handler timed out".into()); break; }
                }
            }
        }
    }
    callbacks.shutdown().await;
}

async fn handle_request(
    handler: &dyn HostHandler,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    match method {
        "command/call" => {
            let params: InvokeParams =
                serde_json::from_value(params).map_err(|error| error.to_string())?;
            handler.call(&params.name, params.args).await
        }
        "observation/record" => handler.observe(params).await,
        other => Err(format!("the host has no method `{other}`")),
    }
}

async fn handle_notification(handler: &dyn HostHandler, method: &str, params: Value) {
    match method {
        "event/emit" => {
            if let Ok(params) = serde_json::from_value::<EventParams>(params) {
                handler.emit(&params.name, params.payload).await;
            }
        }
        "log" => {
            if let Ok(params) = serde_json::from_value::<LogParams>(params) {
                handler.log(&params.level, &params.message);
            }
        }
        other => tracing::debug!("plugin protocol: ignoring notification {other}"),
    }
}

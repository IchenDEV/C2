//! The high-level ACP client: the small set of calls a frontend/engine makes to drive a prompt turn.
//! Wraps a [`Connection`] and (in production) owns the provider child process.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::process::Child;

use super::connection::{AcpProtocolDiagnostics, Connection};
use super::wire::*;
use crate::error::AcpError;

/// Content-free lifecycle state for the provider process behind one ACP connection.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct AcpProcessDiagnostics {
    pub started_at_unix_ms: i64,
    pub closed_at_unix_ms: Option<i64>,
    pub termination_requested: bool,
}

pub struct AcpClient {
    conn: Arc<Connection>,
    // Wrapped in a std Mutex so `AcpClient` is `Sync` (needed to live in desktop host state / the engine's
    // shared session map). We only touch it to kill the child on drop.
    child: Option<Mutex<Child>>,
    #[cfg(unix)]
    process_group: Option<i32>,
    started_at_unix_ms: i64,
    terminated: AtomicBool,
}

impl AcpClient {
    pub fn new(conn: Arc<Connection>, child: Option<Child>) -> Self {
        Self {
            conn,
            child: child.map(Mutex::new),
            #[cfg(unix)]
            process_group: None,
            started_at_unix_ms: unix_time_millis(),
            terminated: AtomicBool::new(false),
        }
    }

    /// Only `spawn` creates an isolated process group; callers supplying arbitrary children
    /// through `new` retain direct-child ownership.
    #[cfg(unix)]
    pub(super) fn with_process_group(mut self, process_group: Option<i32>) -> Self {
        self.process_group = process_group;
        self
    }

    /// The underlying connection, for advanced/unsupported calls.
    pub fn connection(&self) -> &Arc<Connection> {
        &self.conn
    }

    /// Content-free, bounded protocol anomalies observed on this connection.
    pub fn protocol_diagnostics(&self) -> AcpProtocolDiagnostics {
        self.conn.protocol_diagnostics()
    }

    pub fn process_diagnostics(&self) -> AcpProcessDiagnostics {
        AcpProcessDiagnostics {
            started_at_unix_ms: self.started_at_unix_ms,
            closed_at_unix_ms: self.conn.closed_at_unix_ms(),
            termination_requested: self.terminated.load(Ordering::Acquire),
        }
    }

    /// Negotiate protocol version and exchange capabilities.
    pub async fn initialize(
        &self,
        client_capabilities: Value,
    ) -> Result<InitializeResponse, AcpError> {
        self.conn
            .request(
                "initialize",
                InitializeRequest {
                    protocol_version: PROTOCOL_VERSION,
                    client_capabilities: Some(client_capabilities),
                },
            )
            .await
    }

    /// Create a new session and return the provider's ACP session id.
    pub async fn new_session(
        &self,
        cwd: impl Into<String>,
        mcp_servers: Vec<Value>,
    ) -> Result<String, AcpError> {
        Ok(self.new_session_full(cwd, mcp_servers).await?.session_id)
    }

    /// `session/new` with the whole response, including the models the agent offers (if any).
    pub async fn new_session_full(
        &self,
        cwd: impl Into<String>,
        mcp_servers: Vec<Value>,
    ) -> Result<NewSessionResponse, AcpError> {
        let request = NewSessionRequest {
            cwd: cwd.into(),
            mcp_servers,
        };
        let first = self.conn.request("session/new", request.clone()).await;
        match first {
            Err(AcpError::Rpc(error)) if error.code == -32603 => {
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                self.conn.request("session/new", request).await
            }
            result => result,
        }
    }

    /// Re-attach a previous session by id (`session/load`), restoring the agent's conversation
    /// context. Only meaningful when the agent advertised [`AgentCaps::load_session`] at
    /// `initialize`. The agent replays the history as `session/update` notifications before this
    /// resolves — callers that already hold the transcript should suppress their handler for the
    /// duration. A `null` result is a success with nothing reported (older adapters).
    pub async fn load_session(
        &self,
        session_id: &str,
        cwd: impl Into<String>,
        mcp_servers: Vec<Value>,
    ) -> Result<LoadSessionResponse, AcpError> {
        let r: Option<LoadSessionResponse> = self
            .conn
            .request(
                "session/load",
                LoadSessionRequest {
                    session_id: session_id.to_string(),
                    cwd: cwd.into(),
                    mcp_servers,
                },
            )
            .await?;
        Ok(r.unwrap_or_default())
    }

    /// Re-attach without replaying history (`session/resume`, UNSTABLE). Callers must gate this on
    /// the initialize response's `sessionCapabilities.resume` advertisement.
    pub async fn resume_session(
        &self,
        session_id: &str,
        cwd: impl Into<String>,
        mcp_servers: Vec<Value>,
    ) -> Result<ResumeSessionResponse, AcpError> {
        let r: Option<ResumeSessionResponse> = self
            .conn
            .request(
                "session/resume",
                ResumeSessionRequest {
                    session_id: session_id.to_string(),
                    cwd: cwd.into(),
                    mcp_servers,
                },
            )
            .await?;
        Ok(r.unwrap_or_default())
    }

    /// Switch the session's model. `session/set_model` is UNSTABLE in the ACP spec and adapters
    /// that don't implement it answer with a method-not-found error — the caller is expected to
    /// surface that rather than treat it as fatal.
    pub async fn set_model(&self, session_id: &str, model_id: &str) -> Result<(), AcpError> {
        let _: serde_json::Value = self
            .conn
            .request(
                "session/set_model",
                SetModelRequest {
                    session_id: session_id.to_string(),
                    model_id: model_id.to_string(),
                },
            )
            .await?;
        Ok(())
    }

    /// Switch an agent's legacy ACP session mode. Grok currently exposes its model-specific
    /// reasoning ladder through `ModelInfo._meta.reasoningEfforts` and accepts those values here,
    /// rather than through the newer `session/set_config_option` surface.
    pub async fn set_mode(&self, session_id: &str, mode_id: &str) -> Result<(), AcpError> {
        let _: serde_json::Value = self
            .conn
            .request(
                "session/set_mode",
                SetModeRequest {
                    session_id: session_id.to_string(),
                    mode_id: mode_id.to_string(),
                },
            )
            .await?;
        Ok(())
    }

    /// Set a session config option (`session/set_config_option`, UNSTABLE) — how current adapters
    /// switch model and reasoning effort. Answers with the full replacement option set. Adapters
    /// without the method reply method-not-found; the caller surfaces that, it isn't fatal.
    pub async fn set_config_option(
        &self,
        session_id: &str,
        config_id: &str,
        value: &str,
    ) -> Result<Vec<SessionConfigOption>, AcpError> {
        let r: SetConfigOptionResponse = self
            .conn
            .request(
                "session/set_config_option",
                SetConfigOptionRequest {
                    session_id: session_id.to_string(),
                    config_id: config_id.to_string(),
                    value: value.to_string(),
                },
            )
            .await?;
        Ok(r.config_options)
    }

    /// Run one prompt turn. Streamed updates arrive via the [`super::handler::ClientHandler`];
    /// this resolves with the turn's stop reason.
    pub async fn prompt(
        &self,
        session_id: &str,
        blocks: Vec<ContentBlock>,
    ) -> Result<StopReason, AcpError> {
        let r: PromptResponse = self
            .conn
            .request(
                "session/prompt",
                PromptRequest {
                    session_id: session_id.to_string(),
                    prompt: blocks,
                },
            )
            .await?;
        Ok(r.stop_reason)
    }

    /// Interrupt the current turn (fire-and-forget notification).
    pub fn cancel(&self, session_id: &str) -> Result<(), AcpError> {
        self.conn.notify(
            "session/cancel",
            CancelNotification {
                session_id: session_id.to_string(),
            },
        )
    }

    /// Terminate the owned provider process without waiting for it to exit.
    ///
    /// Plugin unload is synchronous, so it must never park on `Child::wait`. Closing the child is
    /// insufficient when an adapter wrapper leaves descendants holding stdio. On Unix, terminate
    /// the isolated group as well so pending requests close and turn leases can be released.
    /// Windows retains direct-child cleanup; process-tree ownership requires a Job Object.
    pub fn terminate(&self) {
        if self.terminated.swap(true, Ordering::AcqRel) {
            return;
        }
        #[cfg(unix)]
        if let Some(group) = self.process_group {
            if let Err(error) = crate::unix_process_group::kill(group) {
                tracing::warn!(%error, "could not terminate ACP process group");
            }
        }
        if let Some(child) = &self.child {
            if let Ok(mut child) = child.lock() {
                let _ = child.start_kill();
            }
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

impl Drop for AcpClient {
    fn drop(&mut self) {
        // Don't leak provider subprocesses when a client is dropped.
        self.terminate();
    }
}

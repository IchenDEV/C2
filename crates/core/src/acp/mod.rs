//! Agent Client Protocol client.
//!
//! We drive every provider (Claude Code, Codex, Grok) over ACP — JSON-RPC 2.0, newline-delimited
//! over stdio. The core spawns the provider (or its ACP adapter) as a subprocess and runs the
//! standard loop: `initialize` → `session/new` → `session/prompt` → consume `session/update` →
//! answer `session/request_permission` → read the `StopReason`.
//!
//! See [`spawn`] to launch a provider, [`AcpClient`] for the call surface, and [`ClientHandler`]
//! for the agent→client callbacks.

mod client;
mod connection;
mod handler;
pub mod wire;

pub use client::{AcpClient, AcpProcessDiagnostics};
pub use connection::{AcpProtocolAnomaly, AcpProtocolDiagnostics, Connection, RequestFault};
pub use handler::{ClientHandler, RecordingHandler};
pub use wire::*;

use std::process::Stdio;
use std::sync::Arc;

use tokio::process::Command;

use crate::error::AcpError;
use crate::provider::{which, LaunchSpec};

/// Spawn a provider CLI as an ACP agent subprocess and return a connected [`AcpClient`].
/// The caller must still run [`AcpClient::initialize`] before creating sessions.
pub async fn spawn(
    spec: &LaunchSpec,
    handler: Arc<dyn ClientHandler>,
) -> Result<AcpClient, AcpError> {
    let executable = which(&spec.command).unwrap_or_else(|| spec.command.clone().into());
    let mut cmd = Command::new(executable);
    cmd.args(&spec.args);
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // Wrapper CLIs (for example npx) may leave descendants holding ACP stdio open.
    #[cfg(unix)]
    cmd.process_group(0);

    let mut child = cmd.spawn().map_err(|e| {
        // "No such file or directory (os error 2)" is inscrutable; name the missing command and
        // point at the fix instead.
        if e.kind() == std::io::ErrorKind::NotFound {
            AcpError::CommandNotFound(spec.command.clone())
        } else {
            AcpError::Spawn(e)
        }
    })?;
    let stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(drain_stderr(stderr));
    }

    let conn = Connection::new(stdout, stdin, handler);
    #[cfg(unix)]
    let process_group = child.id().and_then(|pid| i32::try_from(pid).ok());
    let client = AcpClient::new(conn, Some(child));
    #[cfg(unix)]
    let client = client.with_process_group(process_group);
    Ok(client)
}

/// Forward a provider's stderr to tracing (adapters log diagnostics there).
async fn drain_stderr(stderr: tokio::process::ChildStderr) {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        tracing::debug!(target: "acp.agent.stderr", "{line}");
    }
}

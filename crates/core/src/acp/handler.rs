//! The client-side callback surface. When we drive a provider, the agent calls *back* into us for
//! streamed updates, permission decisions, and filesystem access. A [`ClientHandler`] implements
//! those callbacks. The M1 engine will implement this to translate ACP updates into domain
//! [`crate::event::Event`]s and route permission requests to the UI; tests use [`RecordingHandler`].

use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use super::wire::*;
use crate::error::RpcError;

#[async_trait]
pub trait ClientHandler: Send + Sync + 'static {
    /// A streamed `session/update` (assistant text, thoughts, tool calls).
    async fn session_update(&self, _note: SessionNotification) {}

    /// The agent needs a permission decision. Default: cancel (deny) — safe by default.
    async fn request_permission(
        &self,
        _req: RequestPermissionRequest,
    ) -> RequestPermissionResponse {
        RequestPermissionResponse {
            outcome: PermissionOutcome::Cancelled,
        }
    }

    /// The agent asks the user a structured question (`elicitation/create`, UNSTABLE). Default:
    /// decline — the agent learns nothing was chosen and keeps going, which is the right answer
    /// for a client that never advertised the capability but was asked anyway.
    async fn create_elicitation(
        &self,
        _req: CreateElicitationRequest,
    ) -> CreateElicitationResponse {
        CreateElicitationResponse::Decline
    }

    /// The agent asks the client to read a file (ACP delegates fs to the client). Default: unsupported.
    async fn read_text_file(
        &self,
        _req: ReadTextFileRequest,
    ) -> Result<ReadTextFileResponse, RpcError> {
        Err(RpcError::method_not_found("fs/read_text_file"))
    }

    /// The agent asks the client to write a file. Default: unsupported.
    async fn write_text_file(&self, _req: WriteTextFileRequest) -> Result<(), RpcError> {
        Err(RpcError::method_not_found("fs/write_text_file"))
    }
}

/// A test/demo handler: records streamed agent text and auto-approves permission requests by
/// selecting the first "allow*" option. Handy for the M0 spike and integration tests.
#[derive(Default)]
pub struct RecordingHandler {
    pub texts: Mutex<Vec<String>>,
    pub permissions: AtomicUsize,
}

impl RecordingHandler {
    pub fn text(&self) -> String {
        self.texts.lock().unwrap().join("")
    }
    pub fn permission_count(&self) -> usize {
        self.permissions.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ClientHandler for RecordingHandler {
    async fn session_update(&self, note: SessionNotification) {
        if let SessionUpdate::AgentMessageChunk {
            content: ContentBlock::Text { text },
        } = note.update
        {
            self.texts.lock().unwrap().push(text);
        }
    }

    async fn request_permission(&self, req: RequestPermissionRequest) -> RequestPermissionResponse {
        self.permissions.fetch_add(1, Ordering::SeqCst);
        let chosen = req
            .options
            .iter()
            .find(|o| o.kind.starts_with("allow"))
            .or_else(|| req.options.first());
        match chosen {
            Some(o) => RequestPermissionResponse {
                outcome: PermissionOutcome::Selected {
                    option_id: o.option_id.clone(),
                },
            },
            None => RequestPermissionResponse {
                outcome: PermissionOutcome::Cancelled,
            },
        }
    }
}

//! Shared contract between the authorize gate and the per-domain tool implementations.
//!
//! The gate (owner: gate lane) resolves the credential, enforces scope, project confinement,
//! rate limits and audit, then hands a `CallContext` to the `ops_*::call` functions. Each ops
//! module returns `None` when the tool name is not one of its own so the gate can try the next.

use crate::external_mcp::catalog::ToolSpec;
use crate::external_mcp::clients::{ExternalClientRegistry, ExternalScope, ResolvedClient};
use crate::Engine;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolErrorKind {
    Denied,
    InvalidParams,
    NotFound,
    Conflict,
    RateLimited,
    Internal,
}

#[derive(Debug, Clone)]
pub struct ToolError {
    pub kind: ToolErrorKind,
    pub message: String,
}

impl ToolError {
    pub fn new(kind: ToolErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
    pub fn denied(message: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::Denied, message)
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::InvalidParams, message)
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::NotFound, message)
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::Conflict, message)
    }
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::Internal, message)
    }
}

pub type ToolResult = Result<Value, ToolError>;

/// Signature every `ops_*` module exposes as `pub fn call`.
pub type OpsFn = fn(&Engine, &CallContext<'_>, &str, &Value) -> Option<ToolResult>;

pub struct CallContext<'a> {
    pub client: &'a ResolvedClient,
    pub spec: &'static ToolSpec,
}

impl CallContext<'_> {
    pub fn client_id(&self) -> &str {
        &self.client.record.id
    }

    pub fn has_scope(&self, scope: ExternalScope) -> bool {
        self.client.record.scopes.contains(&scope)
    }

    /// Project confinement: true when the canonical path is inside the client's allowed set.
    pub fn project_allowed(&self, path: &Path) -> bool {
        ExternalClientRegistry::project_allowed(self.client, path)
    }
}

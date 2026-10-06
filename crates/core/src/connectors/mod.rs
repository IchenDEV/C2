//! Backends for [`crate::provider_runtime::ProviderRuntime`].
//!
//! `acp` adapts any Agent Client Protocol subprocess. The native connectors drive the official
//! provider interfaces directly (`codex` over Codex App Server JSON-RPC) or through a supervised
//! sidecar that wraps the official SDK (`sidecar`: Claude Agent SDK, Cursor SDK, OpenCode V1/V2).
//! None of them depends on ACP types; they report into Engine only through
//! [`crate::provider_runtime::RuntimeCallbacks`]. Engine selects a backend per session; a native
//! backend is never silently replaced by ACP.

pub mod acp;
pub mod codex;
pub mod rpc;
pub mod sidecar;

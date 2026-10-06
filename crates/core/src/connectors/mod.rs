//! Backends for [`crate::provider_runtime::ProviderRuntime`].
//!
//! Only the ACP adapter exists in this foundation slice. Official native connectors (Codex App
//! Server, Claude Agent SDK, Cursor SDK, OpenCode) are separate adapters that must pass their own
//! verified capability contract before Engine selects them; none is claimed here.

pub mod acp;

//! External MCP surface (design: docs/sdlc/changes/2026-10-06-external-mcp-events-remote-headless).
//!
//! Separate from `host_mcp` (per-session, in-session agent channel): callers here are external
//! clients with their own scoped, revocable, persisted credentials. Nothing in this module is
//! routed or enabled by default.

pub mod audit;
pub mod catalog;
pub mod clients;
pub mod events;
pub mod ctx;
pub mod ops_admin;
pub mod ops_events;
pub mod ops_read;
pub mod ops_write;
pub mod state;
pub mod dispatch;
pub mod gate;
pub mod mcp_events;
pub mod subscriptions;

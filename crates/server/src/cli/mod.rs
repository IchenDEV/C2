//! Headless `codetwo` CLI: external-MCP client and local credential admin.

pub mod admin;
pub mod client;
pub mod config;
pub mod output;

pub use admin::{run_mcp_client, run_pair, McpClientAction, PairSource};
pub use client::{
    run_answer, run_approve, run_events, run_pending, run_project, run_send, run_session,
    run_status, run_stop, run_wait, ProjectAction, SessionAction,
};
pub use config::{ClientConfig, GlobalOpts};
pub use output::{CliError, CliOutcome, ExitCode};

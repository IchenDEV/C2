//! C2 NAPI native addon — loads Rust CoreApp directly into Bun/Node.
//!
//! Replaces the JSON-lines stdin/stdout IPC bridge with direct in-process NAPI calls.
//! Electrobun loads this addon via `require("codetwo.node")` and calls the exported functions.

use std::path::PathBuf;
use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi_derive::napi;
use serde_json::Value;
use tokio::sync::RwLock;

use codetwo_core::plugins::{AppConfig, CoreApp};

static CORE: std::sync::OnceLock<Arc<RwLock<Option<CoreApp>>>> = std::sync::OnceLock::new();

fn core_lock() -> &'static Arc<RwLock<Option<CoreApp>>> {
    CORE.get_or_init(|| Arc::new(RwLock::new(None)))
}

/// Boot the CoreApp with the given data directory.
/// Must be called once before any other core_* function.
#[napi]
pub async fn core_boot(data_dir: String) -> Result<()> {
    codetwo_core::provider::augment_search_path();

    let data_dir = PathBuf::from(&data_dir);
    let mut config = AppConfig::new(&data_dir);
    config
        .acquire_data_dir_lock()
        .map_err(|e| Error::from_reason(e.to_string()))?;

    std::fs::create_dir_all(&data_dir)
        .map_err(|e| Error::from_reason(format!("could not create data directory: {e}")))?;

    let app = CoreApp::boot(config)
        .await
        .map_err(|e| Error::from_reason(e.to_string()))?;

    let mut lock = core_lock().write().await;
    *lock = Some(app);
    Ok(())
}

/// Execute a named core command with JSON arguments.
/// Returns the JSON-serialized result.
#[napi]
pub async fn core_call(name: String, args: String) -> Result<String> {
    let lock = core_lock().read().await;
    let app = lock
        .as_ref()
        .ok_or_else(|| Error::from_reason("core not booted — call core_boot first"))?;

    let args: Value =
        serde_json::from_str(&args).map_err(|e| Error::from_reason(format!("invalid JSON: {e}")))?;

    // Ordinary native UI: creator-only policy with the trusted local actor.
    let result = app
        .call_local(&name, args)
        .await
        .map_err(|e| Error::from_reason(e.to_string()))?;

    serde_json::to_string(&result).map_err(|e| Error::from_reason(e.to_string()))
}

/// Execute a named core command scoped to a specific project.
/// Falls back to global commands when the project has no override.
#[napi]
pub async fn core_call_in_project(
    project_path: String,
    name: String,
    args: String,
) -> Result<String> {
    let lock = core_lock().read().await;
    let app = lock
        .as_ref()
        .ok_or_else(|| Error::from_reason("core not booted — call core_boot first"))?;

    let args: Value =
        serde_json::from_str(&args).map_err(|e| Error::from_reason(format!("invalid JSON: {e}")))?;

    let result = app
        .call_local_in_project(&project_path, &name, args)
        .await
        .map_err(|e| Error::from_reason(e.to_string()))?;

    serde_json::to_string(&result).map_err(|e| Error::from_reason(e.to_string()))
}

/// Shut down the CoreApp gracefully.
#[napi]
pub async fn core_shutdown() -> Result<()> {
    let mut lock = core_lock().write().await;
    if let Some(app) = lock.take() {
        drop(app);
    }
    Ok(())
}

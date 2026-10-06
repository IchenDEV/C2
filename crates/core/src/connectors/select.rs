//! Backend selection: which [`ProviderRuntime`] drives one C2 session.
//!
//! The decision is made once per session and persisted (see `Store::bind_session_runtime`); it is
//! never re-derived from the current provider registry. A native backend that cannot start is an
//! error shown to the user. It is never replaced by ACP, another model, or another account.
//!
//! Native backends are opt-in until the real-backend acceptance has run: nothing is enabled by
//! default, so existing installations keep their ACP behavior.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use crate::acp::ClientHandler;
use crate::connectors::codex::CodexLaunch;
use crate::connectors::sidecar::SidecarLaunch;
use crate::connectors::{acp, codex, sidecar};
use crate::provider::{LaunchSpec, ProviderId};
use crate::provider_runtime::{RuntimeBackendKind, RuntimeCallbacks, RuntimeError, RuntimeHandle};

/// Environment variable listing opted-in native providers (comma separated), for example
/// `codex,claude,cursor,opencode,opencode2`. Backend names such as `codex-app-server` also work.
pub const NATIVE_PROVIDERS_ENV: &str = "CODETWO_NATIVE_PROVIDERS";
/// Directory holding the `claude/`, `cursor/`, `opencode-v1/` and `opencode-v2/` sidecars.
pub const SIDECAR_ROOT_ENV: &str = "CODETWO_PROVIDER_SIDECARS_DIR";
/// JavaScript runtime used for sidecars (`node` by default).
pub const JS_RUNTIME_ENV: &str = "CODETWO_JS_RUNTIME";

/// Map a provider identity to the native backend that serves it, when one exists.
pub fn native_kind_for(provider: &ProviderId) -> Option<RuntimeBackendKind> {
    match provider {
        ProviderId::Codex => Some(RuntimeBackendKind::CodexAppServer),
        ProviderId::ClaudeCode => Some(RuntimeBackendKind::ClaudeAgentSdk),
        ProviderId::Cursor => Some(RuntimeBackendKind::CursorSdk),
        ProviderId::OpenCode => Some(RuntimeBackendKind::OpenCodeV1),
        ProviderId::OpenCode2 => Some(RuntimeBackendKind::OpenCodeV2),
        _ => None,
    }
}

fn parse_opt_in(name: &str) -> Option<RuntimeBackendKind> {
    match name.trim() {
        "codex" => Some(RuntimeBackendKind::CodexAppServer),
        "claude" | "claude_code" => Some(RuntimeBackendKind::ClaudeAgentSdk),
        "cursor" => Some(RuntimeBackendKind::CursorSdk),
        "opencode" => Some(RuntimeBackendKind::OpenCodeV1),
        "opencode2" => Some(RuntimeBackendKind::OpenCodeV2),
        other => RuntimeBackendKind::parse(other).filter(|kind| kind.is_native()),
    }
}

fn sidecar_dir(kind: RuntimeBackendKind) -> Option<&'static str> {
    match kind {
        RuntimeBackendKind::ClaudeAgentSdk => Some("claude"),
        RuntimeBackendKind::CursorSdk => Some("cursor"),
        RuntimeBackendKind::OpenCodeV1 => Some("opencode-v1"),
        RuntimeBackendKind::OpenCodeV2 => Some("opencode-v2"),
        _ => None,
    }
}

#[derive(Debug, Clone, Default)]
pub struct NativeBackends {
    enabled: HashSet<RuntimeBackendKind>,
    sidecar_root: Option<PathBuf>,
    js_runtime: Option<PathBuf>,
    data_dir: Option<PathBuf>,
}

impl NativeBackends {
    /// Read the opt-in set and sidecar location from the environment. Unknown names are ignored.
    pub fn from_env() -> Self {
        let mut config = Self::default();
        if let Ok(list) = std::env::var(NATIVE_PROVIDERS_ENV) {
            config.enabled = list.split(',').filter_map(parse_opt_in).collect();
        }
        config.sidecar_root = std::env::var_os(SIDECAR_ROOT_ENV).map(PathBuf::from);
        config.js_runtime = std::env::var_os(JS_RUNTIME_ENV).map(PathBuf::from);
        config
    }

    pub fn enable(mut self, kind: RuntimeBackendKind) -> Self {
        if kind.is_native() {
            self.enabled.insert(kind);
        }
        self
    }

    pub fn with_sidecar_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.sidecar_root = Some(root.into());
        self
    }

    pub fn with_js_runtime(mut self, runtime: impl Into<PathBuf>) -> Self {
        self.js_runtime = Some(runtime.into());
        self
    }

    /// Directory for per-backend durable state (the OpenCode V2 session database).
    pub fn with_data_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.data_dir = Some(dir.into());
        self
    }

    pub fn is_enabled(&self, kind: RuntimeBackendKind) -> bool {
        self.enabled.contains(&kind)
    }

    /// Backend for a session that has no persisted choice and no provider-side session yet.
    pub fn select_for_new_session(&self, provider: &ProviderId) -> RuntimeBackendKind {
        match native_kind_for(provider) {
            Some(kind) if self.is_enabled(kind) => kind,
            _ => RuntimeBackendKind::Acp,
        }
    }

    /// Start the backend. `acp_launch` is the provider's ACP spec: used as-is for ACP, and only
    /// as a source of environment and Codex config for native backends.
    pub async fn launch<H>(
        &self,
        kind: RuntimeBackendKind,
        acp_launch: &LaunchSpec,
        handler: Arc<H>,
    ) -> Result<RuntimeHandle, RuntimeError>
    where
        H: ClientHandler + RuntimeCallbacks + 'static,
    {
        match kind {
            RuntimeBackendKind::Acp => {
                let runtime = acp::launch(acp_launch, handler).await?;
                Ok(runtime)
            }
            RuntimeBackendKind::CodexAppServer => {
                let mut spec = CodexLaunch::discover().ok_or_else(|| {
                    RuntimeError::Unsupported(
                        "Codex is not installed, so the native Codex backend is unavailable".into(),
                    )
                })?;
                for (key, value) in &acp_launch.env {
                    if key == "CODEX_CONFIG" {
                        let config: serde_json::Map<String, serde_json::Value> =
                            serde_json::from_str(value).map_err(|error| {
                                RuntimeError::Decode(format!(
                                    "CODEX_CONFIG is not valid JSON: {error}"
                                ))
                            })?;
                        spec.config = config;
                    } else {
                        spec.env.push((key.clone(), value.clone()));
                    }
                }
                let runtime = codex::launch(&spec, handler).await?;
                Ok(runtime)
            }
            native => {
                let spec = self.sidecar_launch(native, acp_launch)?;
                let runtime = sidecar::launch(&spec, handler).await?;
                Ok(runtime)
            }
        }
    }

    fn sidecar_launch(
        &self,
        kind: RuntimeBackendKind,
        acp_launch: &LaunchSpec,
    ) -> Result<SidecarLaunch, RuntimeError> {
        let dir = sidecar_dir(kind).ok_or_else(|| {
            RuntimeError::Unsupported(format!("{} has no SDK sidecar", kind.as_str()))
        })?;
        let root = self.sidecar_root.as_ref().ok_or_else(|| {
            RuntimeError::Unsupported(format!(
                "the {} backend needs {SIDECAR_ROOT_ENV} to locate its SDK sidecar",
                kind.as_str()
            ))
        })?;
        let script = root.join(dir).join("sidecar.mjs");
        if !script.is_file() {
            return Err(RuntimeError::Unsupported(format!(
                "SDK sidecar for {} is missing: {}",
                kind.as_str(),
                script.display()
            )));
        }
        let mut env = acp_launch.env.clone();
        if kind == RuntimeBackendKind::OpenCodeV2 {
            if let Some(data_dir) = &self.data_dir {
                env.push((
                    "CODETWO_OPENCODE_V2_DB".into(),
                    data_dir
                        .join("opencode-v2.db")
                        .to_string_lossy()
                        .into_owned(),
                ));
            }
        }
        Ok(SidecarLaunch {
            backend: kind,
            runtime: self
                .js_runtime
                .clone()
                .unwrap_or_else(|| PathBuf::from("node")),
            args: vec![script.to_string_lossy().into_owned()],
            env,
            cwd: None,
            client_version: env!("CARGO_PKG_VERSION").to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_native_by_default() {
        let config = NativeBackends::default();
        for provider in [
            ProviderId::Codex,
            ProviderId::ClaudeCode,
            ProviderId::Cursor,
            ProviderId::OpenCode,
            ProviderId::OpenCode2,
            ProviderId::Grok,
            ProviderId::Custom("x".into()),
        ] {
            assert_eq!(
                config.select_for_new_session(&provider),
                RuntimeBackendKind::Acp
            );
        }
    }

    #[test]
    fn opt_in_is_per_backend_and_third_party_providers_stay_acp() {
        let config = NativeBackends::default()
            .enable(RuntimeBackendKind::CodexAppServer)
            .enable(RuntimeBackendKind::OpenCodeV2)
            .enable(RuntimeBackendKind::Acp);
        assert_eq!(
            config.select_for_new_session(&ProviderId::Codex),
            RuntimeBackendKind::CodexAppServer
        );
        // OpenCode V1 and V2 are separate identities: enabling one never enables the other.
        assert_eq!(
            config.select_for_new_session(&ProviderId::OpenCode),
            RuntimeBackendKind::Acp
        );
        assert_eq!(
            config.select_for_new_session(&ProviderId::OpenCode2),
            RuntimeBackendKind::OpenCodeV2
        );
        assert_eq!(
            config.select_for_new_session(&ProviderId::Grok),
            RuntimeBackendKind::Acp
        );
        assert!(!config.is_enabled(RuntimeBackendKind::Acp));
    }

    #[test]
    fn opt_in_names_parse() {
        assert_eq!(
            parse_opt_in("codex"),
            Some(RuntimeBackendKind::CodexAppServer)
        );
        assert_eq!(
            parse_opt_in(" claude "),
            Some(RuntimeBackendKind::ClaudeAgentSdk)
        );
        assert_eq!(
            parse_opt_in("opencode-v2"),
            Some(RuntimeBackendKind::OpenCodeV2)
        );
        assert_eq!(parse_opt_in("acp"), None);
        assert_eq!(parse_opt_in("bogus"), None);
    }

    #[test]
    fn sidecar_launch_fails_clearly_without_a_root_or_script() {
        let acp = LaunchSpec {
            command: "x".into(),
            args: vec![],
            env: vec![],
            cwd: None,
        };
        let error = NativeBackends::default()
            .sidecar_launch(RuntimeBackendKind::ClaudeAgentSdk, &acp)
            .unwrap_err();
        assert!(error.to_string().contains(SIDECAR_ROOT_ENV), "{error}");
        let error = NativeBackends::default()
            .with_sidecar_root("/nonexistent")
            .sidecar_launch(RuntimeBackendKind::CursorSdk, &acp)
            .unwrap_err();
        assert!(error.to_string().contains("missing"), "{error}");
    }

    #[test]
    fn opencode_v2_gets_a_persistent_database_only_when_a_data_dir_is_known() {
        let root = tempfile_root();
        let acp = LaunchSpec {
            command: "x".into(),
            args: vec![],
            env: vec![("A".into(), "1".into())],
            cwd: None,
        };
        let without = NativeBackends::default()
            .with_sidecar_root(&root)
            .sidecar_launch(RuntimeBackendKind::OpenCodeV2, &acp)
            .unwrap();
        assert!(!without
            .env
            .iter()
            .any(|(k, _)| k == "CODETWO_OPENCODE_V2_DB"));
        let with = NativeBackends::default()
            .with_sidecar_root(&root)
            .with_data_dir("/data")
            .sidecar_launch(RuntimeBackendKind::OpenCodeV2, &acp)
            .unwrap();
        assert!(with.env.contains(&(
            "CODETWO_OPENCODE_V2_DB".into(),
            "/data/opencode-v2.db".into()
        )));
        assert!(with.env.contains(&("A".into(), "1".into())));
        std::fs::remove_dir_all(root).ok();
    }

    fn tempfile_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("c2-select-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("opencode-v2")).unwrap();
        std::fs::write(root.join("opencode-v2/sidecar.mjs"), "").unwrap();
        root
    }
}

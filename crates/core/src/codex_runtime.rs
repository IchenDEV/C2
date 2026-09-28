//! Codex ACP launch discovery.
//!
//! Special-tool discovery and routing live in the Bun Tool Broker. Rust only locates an optional
//! Codex executable for the ACP provider adapter; it does not inspect Computer Use, Browser,
//! Image Generation, Sites, plugins, or `host-tools.json`.

#[cfg(target_os = "macos")]
use std::path::Path;
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;

pub const OPENAI_TEAM_ID: &str = "2DC432GLL2";
pub const CHATGPT_BUNDLE_ID: &str = "com.openai.codex";

#[derive(Debug, Clone, Default)]
pub struct CodexRuntimeDiscovery {
    pub codex_path: Option<PathBuf>,
}

impl CodexRuntimeDiscovery {
    pub fn detect() -> Self {
        if let Some(path) = std::env::var_os("CODEX_PATH").filter(|path| !path.is_empty()) {
            return Self {
                codex_path: Some(path.into()),
            };
        }
        #[cfg(target_os = "macos")]
        {
            let app = Path::new("/Applications/ChatGPT.app");
            let codex = app.join("Contents/Resources/codex");
            if codex.is_file() && is_verified_openai_app(app) {
                return Self {
                    codex_path: Some(codex),
                };
            }
        }

        // Pin ACP and catalogue discovery to the same installed CLI. Otherwise the adapter
        // silently uses its own bundled version, which can reject models discovered locally.
        Self {
            codex_path: crate::provider::which("codex"),
        }
    }
}

#[cfg(target_os = "macos")]
fn is_verified_openai_app(path: &Path) -> bool {
    let verified = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(path)
        .output()
        .is_ok_and(|output| output.status.success());
    if !verified {
        return false;
    }
    let details = match Command::new("/usr/bin/codesign")
        .args(["-dv", "--verbose=4"])
        .arg(path)
        .output()
    {
        Ok(output) => String::from_utf8_lossy(&output.stderr).into_owned(),
        Err(_) => return false,
    };
    details
        .lines()
        .any(|line| line.trim() == format!("Identifier={CHATGPT_BUNDLE_ID}"))
        && details
            .lines()
            .any(|line| line.trim() == format!("TeamIdentifier={OPENAI_TEAM_ID}"))
}

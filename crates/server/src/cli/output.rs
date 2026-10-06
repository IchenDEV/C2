//! JSON envelope and process exit codes for the headless CLI.

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Ok = 0,
    Error = 1,
    Timeout = 2,
    NeedsAttention = 3,
}

impl ExitCode {
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CliError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CliEnvelope {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CliError>,
}

#[derive(Debug, Clone)]
pub struct CliOutcome {
    pub exit: ExitCode,
    pub envelope: CliEnvelope,
}

impl CliOutcome {
    pub fn ok(result: Value) -> Self {
        Self {
            exit: ExitCode::Ok,
            envelope: CliEnvelope {
                ok: true,
                result: Some(result),
                error: None,
            },
        }
    }

    pub fn err(exit: ExitCode, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            exit,
            envelope: CliEnvelope {
                ok: false,
                result: None,
                error: Some(CliError {
                    code: code.into(),
                    message: sanitize_message(message.into()),
                }),
            },
        }
    }

    pub fn emit(&self, json_mode: bool) -> ExitCode {
        if json_mode {
            let line = serde_json::to_string(&self.envelope).unwrap_or_else(|_| {
                r#"{"ok":false,"error":{"code":"internal","message":"json encode failed"}}"#
                    .to_string()
            });
            println!("{line}");
        } else if self.envelope.ok {
            if let Some(result) = &self.envelope.result {
                if result.is_string() {
                    println!("{}", result.as_str().unwrap_or(""));
                } else {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(result).unwrap_or_default()
                    );
                }
            }
        } else if let Some(error) = &self.envelope.error {
            eprintln!("{}: {}", error.code, error.message);
        }
        self.exit
    }
}

/// Strip external MCP token shapes from user-visible text.
pub fn sanitize_message(message: String) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message.as_str();
    while let Some(at) = rest.find("ctmcp_") {
        out.push_str(&rest[..at]);
        out.push_str("[redacted]");
        let tail = &rest[at..];
        rest = tail
            .find(char::is_whitespace)
            .map_or("", |end| &tail[end..]);
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_ctmcp_tokens() {
        let msg = "failed with ctmcp_abc123secret trailing";
        assert!(!sanitize_message(msg.into()).contains("ctmcp_"));
    }

    #[test]
    fn sanitize_keeps_non_ascii_text() {
        let out = sanitize_message("错误 ctmcp_abc 之后 é".into());
        assert_eq!(out, "错误 [redacted] 之后 é");
    }
}

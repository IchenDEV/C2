//! Client commands via `POST /external-mcp` (`tools/call`).

use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use serde_json::{json, Value};

use super::config::ClientConfig;
use super::output::{CliOutcome, ExitCode};

/// `codetwo wait` asks the server in short slices so a pending approval/question is reported
/// (exit 3) right away instead of after the full timeout.
const WAIT_SLICE_MS: u64 = 2_000;

pub enum SessionAction {
    List,
    Read {
        session_id: String,
    },
    Create {
        project_path: String,
        provider: Option<String>,
    },
}

pub enum ProjectAction {
    List,
    Create { path: String },
}

pub struct SendOpts {
    pub session_id: String,
    pub text: String,
    pub mode: String,
}

pub struct WaitOpts {
    pub session_id: String,
    pub timeout_secs: u64,
}

pub struct EventsOpts {
    pub cursor: Option<String>,
    pub follow: bool,
}

struct McpClient {
    http: Client,
    base_url: String,
    token: String,
    next_id: u64,
}

impl McpClient {
    fn new(config: &ClientConfig) -> Result<Self, CliOutcome> {
        let base_url = config.base_url.trim_end_matches('/').to_string();
        ensure_transport_is_safe(&base_url)?;
        let mut builder = Client::builder();
        if reqwest::Url::parse(&base_url).ok().is_some_and(|url| {
            url.host_str().is_some_and(|host| {
                host == "localhost"
                    || host
                        .trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
            })
        }) {
            builder = builder.no_proxy();
        }
        let http = builder
            .timeout(Duration::from_secs(120))
            // A redirect could carry the credential somewhere the URL check never approved.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| CliOutcome::err(ExitCode::Error, "http_client", e.to_string()))?;
        Ok(Self {
            http,
            base_url,
            token: config.token.clone(),
            next_id: 1,
        })
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, CliOutcome> {
        let id = self.next_id;
        self.next_id += 1;
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        });
        let response = self
            .http
            .post(format!("{}/external-mcp", self.base_url))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .map_err(map_http_err)?;
        let status = response.status();
        let text = response
            .text()
            .map_err(|e| CliOutcome::err(ExitCode::Error, "http_read", e.to_string()))?;
        match status.as_u16() {
            401 | 403 => {
                return Err(CliOutcome::err(
                    ExitCode::Error,
                    "auth_failed",
                    "server rejected the credential",
                ))
            }
            404 => {
                return Err(CliOutcome::err(
                    ExitCode::Error,
                    "not_enabled",
                    "external MCP is not enabled on this server (start it with --external-mcp)",
                ))
            }
            429 => {
                return Err(CliOutcome::err(
                    ExitCode::Error,
                    "rate_limited",
                    "server rate limit reached; retry later",
                ))
            }
            _ if !status.is_success() => {
                return Err(CliOutcome::err(
                    ExitCode::Error,
                    "http_status",
                    format!("HTTP {}", status.as_u16()),
                ))
            }
            _ => {}
        }
        let payload: Value = serde_json::from_str(&text).map_err(|_| {
            CliOutcome::err(
                ExitCode::Error,
                "invalid_json",
                "server returned invalid JSON",
            )
        })?;
        if let Some(error) = payload.get("error") {
            let code = error.get("code").and_then(Value::as_i64).unwrap_or(-1);
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("rpc error");
            return Err(if is_timeout_message(message) {
                CliOutcome::err(ExitCode::Timeout, "timeout", message)
            } else if code == -32003 {
                CliOutcome::err(ExitCode::Error, "denied", message)
            } else {
                CliOutcome::err(ExitCode::Error, "rpc_error", message)
            });
        }
        let result = payload.get("result").cloned().unwrap_or(Value::Null);
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            let message = result
                .pointer("/content/0/text")
                .and_then(Value::as_str)
                .unwrap_or("tool error");
            return Err(CliOutcome::err(ExitCode::Error, "tool_error", message));
        }
        parse_tool_result(&result)
    }
}

/// The bearer token must not cross the network in clear text.
fn ensure_transport_is_safe(base_url: &str) -> Result<(), CliOutcome> {
    super::config::check_transport(base_url)
}

/// Read-only credential check for `codetwo pair`: one `codetwo_capabilities` call, no retry.
pub fn probe_capabilities(config: &ClientConfig) -> Result<Value, CliOutcome> {
    McpClient::new(config)?.call_tool("codetwo_capabilities", json!({}))
}

fn is_timeout_message(message: &str) -> bool {
    message == "timeout" || message.starts_with("timeout ")
}

fn map_http_err(error: reqwest::Error) -> CliOutcome {
    if error.is_timeout() {
        CliOutcome::err(ExitCode::Timeout, "timeout", "request timed out")
    } else if error.is_connect() {
        CliOutcome::err(
            ExitCode::Error,
            "connect_failed",
            "could not connect to the server",
        )
    } else {
        // The URL may carry userinfo or a query; keep it out of the message.
        CliOutcome::err(
            ExitCode::Error,
            "http_error",
            error.without_url().to_string(),
        )
    }
}

fn parse_tool_result(result: &Value) -> Result<Value, CliOutcome> {
    let text = result
        .pointer("/content/0/text")
        .and_then(Value::as_str)
        .ok_or_else(|| CliOutcome::err(ExitCode::Error, "tool_shape", "unexpected tool result"))?;
    serde_json::from_str(text)
        .map_err(|_| CliOutcome::err(ExitCode::Error, "tool_json", "tool returned non-JSON text"))
}

/// One tool call as a finished outcome (the shape most commands need).
fn call(config: &ClientConfig, tool: &str, args: Value) -> CliOutcome {
    McpClient::new(config)
        .and_then(|mut client| client.call_tool(tool, args))
        .map_or_else(|outcome| outcome, CliOutcome::ok)
}

pub fn run_status(config: &ClientConfig) -> CliOutcome {
    let mut client = match McpClient::new(config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let health = client
        .http
        .get(format!("{}/health", client.base_url))
        .send()
        .map(|r| r.status().is_success())
        .unwrap_or(false);
    match client.call_tool("codetwo_capabilities", json!({})) {
        Ok(caps) => CliOutcome::ok(json!({
            "health": health,
            "url": client.base_url,
            "capabilities": caps,
        })),
        Err(outcome) => outcome,
    }
}

pub fn run_session(config: &ClientConfig, action: SessionAction) -> CliOutcome {
    match action {
        SessionAction::List => call(config, "codetwo_session_list", json!({})),
        SessionAction::Read { session_id } => call(
            config,
            "codetwo_session_read",
            json!({ "session_id": session_id }),
        ),
        SessionAction::Create {
            project_path,
            provider,
        } => {
            let mut args = json!({ "project_path": project_path });
            if let Some(p) = provider {
                args["provider"] = json!(p);
            }
            call(config, "codetwo_session_create", args)
        }
    }
}

pub fn run_project(config: &ClientConfig, action: ProjectAction) -> CliOutcome {
    match action {
        ProjectAction::List => call(config, "codetwo_project_list", json!({})),
        ProjectAction::Create { path } => {
            call(config, "codetwo_project_create", json!({ "path": path }))
        }
    }
}

pub fn run_send(config: &ClientConfig, opts: SendOpts) -> CliOutcome {
    call(
        config,
        "codetwo_turn_send",
        json!({
            "session_id": opts.session_id,
            "mode": opts.mode,
            "text": opts.text,
        }),
    )
}

pub fn run_stop(config: &ClientConfig, session_id: &str) -> CliOutcome {
    call(
        config,
        "codetwo_turn_stop",
        json!({ "session_id": session_id }),
    )
}

fn needs_attention(status: &str) -> CliOutcome {
    CliOutcome::err(
        ExitCode::NeedsAttention,
        "needs_attention",
        format!("session is {status}"),
    )
}

fn classify_wait_status(value: Value) -> CliOutcome {
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    match status {
        "waiting_approval" | "waiting_question" => needs_attention(status),
        _ if value["timed_out"] == true => CliOutcome::err(
            ExitCode::Timeout,
            "timeout",
            "turn did not finish before the timeout",
        ),
        "completed" => CliOutcome::ok(value),
        _ => CliOutcome::err(
            ExitCode::Error,
            "turn_not_completed",
            format!("turn ended with status {status}"),
        ),
    }
}

pub fn run_wait(config: &ClientConfig, opts: WaitOpts) -> CliOutcome {
    let mut client = match McpClient::new(config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let deadline = Instant::now() + Duration::from_secs(opts.timeout_secs);
    let mut turn_id: Option<String> = None;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let slice_ms = (left.as_millis() as u64).min(WAIT_SLICE_MS);
        let mut args = json!({"session_id": opts.session_id, "timeout_ms": slice_ms});
        if let Some(turn) = &turn_id {
            args["turn_id"] = json!(turn);
        }
        let value = match client.call_tool("codetwo_turn_wait", args) {
            Ok(value) => value,
            Err(outcome) => return outcome,
        };
        // Keep waiting for the same turn across bounded read-only requests.
        if turn_id.is_none() {
            turn_id = value
                .get("turn_id")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        let outcome = classify_wait_status(value);
        if outcome.exit != ExitCode::Timeout || Instant::now() >= deadline {
            return outcome;
        }
    }
}

pub fn run_approve(config: &ClientConfig, request_id: &str, deny: bool) -> CliOutcome {
    call(
        config,
        "codetwo_approval_respond",
        json!({ "request_id": request_id, "allow": !deny }),
    )
}

pub fn run_answer(config: &ClientConfig, request_id: &str, text: &str) -> CliOutcome {
    call(
        config,
        "codetwo_question_respond",
        json!({ "request_id": request_id, "answer": text }),
    )
}

pub fn run_pending(config: &ClientConfig) -> CliOutcome {
    let mut client = match McpClient::new(config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let approvals = match client.call_tool("codetwo_approval_list", json!({})) {
        Ok(v) => v,
        Err(o) => return o,
    };
    let questions = match client.call_tool("codetwo_question_list", json!({})) {
        Ok(v) => v,
        Err(o) => return o,
    };
    CliOutcome::ok(json!({
        "approvals": approvals,
        "questions": questions,
    }))
}

pub fn run_events(config: &ClientConfig, opts: EventsOpts) -> CliOutcome {
    let mut client = match McpClient::new(config) {
        Ok(c) => c,
        Err(o) => return o,
    };
    let mut cursor = opts.cursor;
    loop {
        let mut args = json!({ "limit": 50 });
        if let Some(c) = &cursor {
            args["cursor"] = json!(c);
        }
        if opts.follow {
            args["timeout_ms"] = json!(30_000);
        }
        let tool = if opts.follow {
            "codetwo_events_wait"
        } else {
            "codetwo_events_poll"
        };
        match client.call_tool(tool, args) {
            Ok(value) if !opts.follow => return CliOutcome::ok(value),
            Ok(value) => {
                let quiet = value["events"].as_array().is_some_and(Vec::is_empty)
                    && value["reset"] != json!(true);
                if !quiet {
                    println!("{value}");
                }
                // Keep the previous cursor when the server has none yet (empty ring).
                if let Some(next) = value.get("next_cursor").and_then(Value::as_str) {
                    cursor = Some(next.to_string());
                }
            }
            // A bounded wait that saw nothing is the normal idle case when following.
            Err(outcome) if opts.follow && outcome.exit == ExitCode::Timeout => {}
            Err(outcome) => return outcome,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_status_maps_to_exit_codes() {
        assert_eq!(
            classify_wait_status(json!({"status": "running", "timed_out": true})).exit,
            ExitCode::Timeout
        );
        assert_eq!(
            classify_wait_status(json!({"status": "waiting_approval", "timed_out": true})).exit,
            ExitCode::NeedsAttention
        );
        assert_eq!(
            classify_wait_status(json!({"status": "completed"})).exit,
            ExitCode::Ok
        );
        assert_eq!(
            classify_wait_status(json!({"status": "waiting_question"})).exit,
            ExitCode::NeedsAttention
        );
        assert_eq!(
            classify_wait_status(json!({"status": "failed"})).exit,
            ExitCode::Error
        );
    }

    #[test]
    fn plain_http_is_limited_to_loopback() {
        for url in [
            "http://127.0.0.1:1",
            "http://localhost:2",
            "http://[::1]:3",
            "https://x.example",
        ] {
            assert!(ensure_transport_is_safe(url).is_ok(), "{url}");
        }
        for url in [
            "http://192.168.1.2:4599",
            "http://x.example",
            "http://localhost:80@evil.example",
            "http://127.0.0.1@evil.example",
            "ftp://x",
            "x",
        ] {
            assert!(ensure_transport_is_safe(url).is_err(), "{url}");
        }
    }
}

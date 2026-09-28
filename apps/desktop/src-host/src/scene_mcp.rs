//! Authenticated stdio MCP and owner-only broker for C2 Auto Scene.

use std::io::{BufRead, BufReader, Write};

use codetwo_core::plugins::{CoreApp, SceneService, StoreService};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::EventSink;

const SCENE_INSTRUCTIONS: &str = "Call scene_list at the start of each turn and use its returned `enabled` status. When disabled, continue without scene action. When enabled, search with a short task query, select only an exact returned reference, and follow scene_select's returned current-turn instructions.";

#[cfg(unix)]
struct Sidecar {
    socket_path: String,
    session: String,
    key: String,
    next_request_id: u64,
}

#[cfg(unix)]
impl Sidecar {
    fn from_env() -> Result<Self, String> {
        let read = |name: &str| {
            std::env::var(name).map_err(|_| format!("missing required environment: {name}"))
        };
        Ok(Self {
            socket_path: read("CODETWO_BROWSER_SOCKET")?,
            session: read("CODETWO_BROWSER_SESSION")?,
            key: read("CODETWO_BROWSER_SESSION_KEY")?,
            next_request_id: 1,
        })
    }

    fn broker(&self, method: &str, params: Value, approved: bool) -> Result<Value, String> {
        use std::os::unix::net::UnixStream;

        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|error| format!("C2 Scene broker is unavailable: {error}"))?;
        serde_json::to_writer(
            &mut stream,
            &json!({
                "session": self.session,
                "key": self.key,
                "method": method,
                "params": params,
                "approved": approved,
            }),
        )
        .map_err(|error| error.to_string())?;
        stream.write_all(b"\n").map_err(|error| error.to_string())?;
        stream.flush().map_err(|error| error.to_string())?;

        let mut response = String::new();
        BufReader::new(stream)
            .read_line(&mut response)
            .map_err(|error| error.to_string())?;
        serde_json::from_str(&response).map_err(|error| error.to_string())
    }

    fn call_tool<R: BufRead, W: Write>(
        &mut self,
        reader: &mut R,
        writer: &mut W,
        name: &str,
        params: Value,
    ) -> Result<Value, String> {
        let response = self.broker(name, params.clone(), false)?;
        let Some(approval) = response.get("approval") else {
            return broker_result(response);
        };
        let title = approval
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Confirm scene switch");
        let message = approval
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or(title);
        let elicitation_id = format!("codetwo-scene-{}", self.next_request_id);
        self.next_request_id += 1;
        write_json(
            writer,
            &json!({
                "jsonrpc": "2.0",
                "id": elicitation_id,
                "method": "elicitation/create",
                "params": {
                    "message": format!("{title}: {message}"),
                    "requestedSchema": {
                        "type": "object",
                        "properties": {
                            "decision": {
                                "type": "string",
                                "title": title,
                                "enum": ["allow_once", "deny"]
                            }
                        },
                        "required": ["decision"]
                    }
                }
            }),
        )?;

        let answer = loop {
            let mut line = String::new();
            if reader
                .read_line(&mut line)
                .map_err(|error| error.to_string())?
                == 0
            {
                return Err("scene approval channel closed".into());
            }
            let message: Value = serde_json::from_str(&line).map_err(|error| error.to_string())?;
            if message.get("method").and_then(Value::as_str) == Some("notifications/cancelled") {
                return Err("scene switch approval was cancelled".into());
            }
            if message.get("method").and_then(Value::as_str) == Some("ping") {
                if let Some(id) = message.get("id").cloned() {
                    write_json(writer, &json!({ "jsonrpc": "2.0", "id": id, "result": {} }))?;
                }
                continue;
            }
            if message.get("id") == Some(&Value::String(elicitation_id.clone())) {
                break message;
            }
        };
        if !approval_accepted(&answer) {
            return Err("scene switch was not approved".into());
        }
        broker_result(self.broker(name, params, true)?)
    }
}

fn approval_accepted(answer: &Value) -> bool {
    let payload = answer.get("result").unwrap_or(answer);
    let accepted = payload.get("action").and_then(Value::as_str) == Some("accept")
        || (payload.get("outcome").and_then(Value::as_str) == Some("selected")
            && payload
                .get("optionId")
                .and_then(Value::as_str)
                .is_some_and(|option| option.starts_with("allow") || option == "accept"));
    accepted
        && payload
            .pointer("/content/decision")
            .and_then(Value::as_str)
            .is_none_or(|decision| decision == "allow_once")
}

#[cfg(unix)]
fn broker_result(response: Value) -> Result<Value, String> {
    if response.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    } else {
        Err(response
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("C2 Scene request failed")
            .to_string())
    }
}

fn tools() -> Value {
    json!([
        {
            "name": "scene_list",
            "description": "Search installed C2 scenes on demand for the current Auto Scene session. Returns only bounded routing metadata, not scene instructions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "maxLength": 240,
                        "description": "Short description of the current task. Omit to list installed scenes."
                    },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 12 }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "scene_select",
            "description": "Select an installed C2 scene for this Auto Scene session. Returns its instructions for the current turn. Permission increases require user approval.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "reference": {
                        "type": "string",
                        "description": "Exact scene reference returned by scene_list."
                    },
                    "reason": {
                        "type": "string",
                        "maxLength": 240,
                        "description": "Short user-visible reason this scene best fits the task."
                    }
                },
                "required": ["reference", "reason"],
                "additionalProperties": false
            }
        }
    ])
}

fn tool_content(result: Value) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&result).unwrap_or_else(|_| "null".into())
        }],
        "structuredContent": result
    })
}

fn write_json<W: Write>(writer: &mut W, value: &Value) -> Result<(), String> {
    serde_json::to_writer(&mut *writer, value).map_err(|error| error.to_string())?;
    writer.write_all(b"\n").map_err(|error| error.to_string())?;
    writer.flush().map_err(|error| error.to_string())
}

#[cfg(unix)]
pub fn run_stdio() -> Result<(), String> {
    let mut sidecar = Sidecar::from_env()?;
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = stdout.lock();
    loop {
        let mut line = String::new();
        if reader
            .read_line(&mut line)
            .map_err(|error| error.to_string())?
            == 0
        {
            return Ok(());
        }
        let message: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            continue;
        };
        let Some(id) = message.get("id").cloned() else {
            continue;
        };
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": message
                    .pointer("/params/protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or("2025-06-18"),
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "codetwo_scenes", "version": env!("CARGO_PKG_VERSION") },
                "instructions": SCENE_INSTRUCTIONS
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => {
                let name = message
                    .pointer("/params/name")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let arguments = message
                    .pointer("/params/arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                sidecar
                    .call_tool(&mut reader, &mut writer, name, arguments)
                    .map(tool_content)
            }
            _ => Err(format!("unsupported MCP method: {method}")),
        };
        let response = match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(message) => {
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": message } })
            }
        };
        write_json(&mut writer, &response)?;
    }
}

#[cfg(not(unix))]
pub fn run_stdio() -> Result<(), String> {
    Err("C2 Scene MCP is not implemented on this platform".into())
}

#[derive(Debug, Deserialize)]
struct BrokerRequest {
    session: String,
    key: String,
    method: String,
    #[serde(default)]
    params: Value,
    #[serde(default)]
    approved: bool,
}

#[derive(Debug, Serialize)]
struct BrokerApproval {
    title: String,
    message: String,
}

#[derive(Debug, Serialize)]
struct BrokerResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    approval: Option<BrokerApproval>,
}

impl BrokerResponse {
    fn result(value: Value) -> Self {
        Self {
            ok: true,
            result: Some(value),
            error: None,
            approval: None,
        }
    }

    fn error(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(error.into()),
            approval: None,
        }
    }

    fn approval(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            result: None,
            error: None,
            approval: Some(BrokerApproval {
                title: title.into(),
                message: message.into(),
            }),
        }
    }
}

fn expected_session_key(master_key: &str, session: &str) -> String {
    blake3::hash(format!("codetwo-browser\0{master_key}\0{session}").as_bytes())
        .to_hex()
        .to_string()
}

fn secure_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.as_bytes()
        .iter()
        .zip(right.as_bytes())
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

fn scene_results(
    scenes: &codetwo_core::SceneLibrary,
    query: Option<&str>,
    limit: usize,
    current: Option<Value>,
) -> Value {
    let query = query.unwrap_or("").trim().to_lowercase();
    let terms = query.split_whitespace().collect::<Vec<_>>();
    let mut matches = scenes
        .scenes()
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let description = entry
                .scene
                .description
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let description: String = description.chars().take(240).collect();
            let reference = codetwo_core::SceneLibrary::reference_for(entry);
            let haystack = format!(
                "{} {} {} {}",
                reference,
                entry.scene.title,
                description,
                entry.scene.keywords.join(" ")
            )
            .to_lowercase();
            let score = if terms.is_empty() {
                1
            } else {
                terms.iter().filter(|term| haystack.contains(*term)).count()
            };
            (score > 0).then(|| {
                (
                    score,
                    index,
                    json!({
                        "reference": reference,
                        "title": entry.scene.title,
                        "description": description,
                    }),
                )
            })
        })
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let matched = matches.len();
    let scenes = matches
        .into_iter()
        .take(limit)
        .map(|(_, _, scene)| scene)
        .collect::<Vec<_>>();
    let returned = scenes.len();
    json!({
        "enabled": true,
        "current": current,
        "query": query,
        "scenes": scenes,
        "matched": matched,
        "truncated": matched > returned,
    })
}

fn disabled_scene_results(query: Option<&str>) -> Value {
    json!({
        "enabled": false,
        "current": Value::Null,
        "query": query.unwrap_or("").trim().to_lowercase(),
        "scenes": [],
        "matched": 0,
        "truncated": false,
    })
}

async fn dispatch_broker(
    core: &CoreApp,
    events: &EventSink,
    master_key: &str,
    request: BrokerRequest,
) -> BrokerResponse {
    if request.session.is_empty()
        || !secure_eq(
            &request.key,
            &expected_session_key(master_key, &request.session),
        )
    {
        return BrokerResponse::error("scene broker authentication failed");
    }
    let Some(store) = core.service::<StoreService>() else {
        return BrokerResponse::error("store plugin is unavailable");
    };
    let auto_scene_enabled = match store.session_auto_scene(&request.session) {
        Ok(enabled) => enabled,
        Err(error) => return BrokerResponse::error(error.to_string()),
    };
    if !auto_scene_enabled {
        return if request.method == "scene_list" {
            let query = request.params.get("query").and_then(Value::as_str);
            if query.is_some_and(|query| query.chars().count() > 240) {
                BrokerResponse::error("query must be at most 240 characters")
            } else {
                BrokerResponse::result(disabled_scene_results(query))
            }
        } else {
            BrokerResponse::error("Auto Scene is not enabled for this session")
        };
    }

    let result: Result<Value, String> = async {
        match request.method.as_str() {
            "scene_list" => {
                let query = request.params.get("query").and_then(Value::as_str);
                if query.is_some_and(|query| query.chars().count() > 240) {
                    return Err("query must be at most 240 characters".into());
                }
                let limit = request
                    .params
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(12)
                    .clamp(1, 50) as usize;
                let scenes = core
                    .service::<SceneService>()
                    .ok_or_else(|| "scenes plugin is unavailable".to_string())?
                    .library();
                let current = store
                    .session_scene(&request.session)
                    .map_err(|error| error.to_string())?
                    .and_then(|(reference, _)| {
                        scenes.resolve(&reference).map(|entry| {
                            json!({
                                "reference": codetwo_core::SceneLibrary::reference_for(entry),
                                "title": entry.scene.title,
                            })
                        })
                    });
                Ok(scene_results(&scenes, query, limit, current))
            }
            "scene_select" => {
                let reference = required_string(&request.params, "reference")?;
                let reason = required_string(&request.params, "reason")?
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let reason: String = reason.chars().take(240).collect();
                let scenes = core
                    .service::<SceneService>()
                    .ok_or_else(|| "scenes plugin is unavailable".to_string())?
                    .library();
                let entry = scenes
                    .resolve(&reference)
                    .ok_or_else(|| format!("unknown scene `{reference}`"))?;
                let canonical = codetwo_core::SceneLibrary::reference_for(entry);
                let title = entry.scene.title.clone();
                let instructions = codetwo_core::scene::prompt_preamble(&entry.scene, &[]);
                let current = store
                    .session_scene(&request.session)
                    .map_err(|error| error.to_string())?
                    .and_then(|(reference, _)| {
                        scenes
                            .resolve(&reference)
                            .map(codetwo_core::SceneLibrary::reference_for)
                    });
                let (changed, pending) = if current.as_deref() == Some(&canonical) {
                    (false, Vec::new())
                } else {
                    let outcome = core
                        .call(
                            "scenes.apply",
                            json!({
                                "session": request.session,
                                "reference": canonical,
                                "confirm_escalation": request.approved,
                            }),
                        )
                        .await
                        .map_err(|error| error.to_string())?;
                    if let Some(escalation) =
                        outcome.get("escalation").filter(|value| !value.is_null())
                    {
                        let from = escalation
                            .get("from")
                            .and_then(Value::as_str)
                            .unwrap_or("current");
                        let to = escalation
                            .get("to")
                            .and_then(Value::as_str)
                            .unwrap_or("broader");
                        if !request.approved {
                            return Err(format!("approval:{from}:{to}:{title}"));
                        }
                        return Err("scene switch remained blocked after approval".into());
                    }
                    let pending = outcome
                        .get("pending")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect();
                    (true, pending)
                };
                let (memory_read, memory_write) = store
                    .session_memory_policy(&request.session)
                    .map_err(|error| error.to_string())?;
                if changed {
                    let _ = events.emit(
                        "auto-scene-changed",
                        json!({
                            "session": request.session,
                            "reference": canonical,
                            "title": title,
                            "reason": reason,
                            "pending": pending,
                            "memoryRead": memory_read,
                            "memoryWrite": memory_write,
                        }),
                    );
                }
                Ok(json!({
                    "selected": canonical,
                    "title": title,
                    "reason": reason,
                    "changed": changed,
                    "pending": pending,
                    "instructions": instructions,
                    "message": if changed {
                        "Scene applied. Follow `instructions` for the current turn."
                    } else {
                        "Scene already active. Continue following `instructions`."
                    }
                }))
            }
            _ => Err(format!("unknown scene tool `{}`", request.method)),
        }
    }
    .await;

    match result {
        Ok(value) => BrokerResponse::result(value),
        Err(error) if error.starts_with("approval:") => {
            let mut parts = error.splitn(4, ':');
            let _ = parts.next();
            let from = parts.next().unwrap_or("current");
            let to = parts.next().unwrap_or("broader");
            let title = parts.next().unwrap_or("scene");
            BrokerResponse::approval(
                "Allow scene switch",
                format!("Switch to {title}, which changes permissions from {from} to {to}?"),
            )
        }
        Err(error) => BrokerResponse::error(error),
    }
}

fn required_string(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty() && value.len() <= 8_192)
        .map(str::to_string)
        .ok_or_else(|| format!("{key} is required"))
}

#[cfg(unix)]
pub fn bind_broker(
    path: &std::path::Path,
) -> Result<(tokio::net::UnixListener, std::fs::File), String> {
    use fs2::FileExt;
    // Separate data directories can be explicitly pointed at the same socket. Protect that
    // resource too, before unlinking it, and retain the inode after exit for future contenders.
    let ownership = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open({
            let mut lock_path = path.as_os_str().to_os_string();
            lock_path.push(".lock");
            lock_path
        })
        .map_err(|error| error.to_string())?;
    ownership
        .try_lock_exclusive()
        .map_err(|error| format!("scene socket already owned: {} ({error})", path.display()))?;
    use std::os::unix::fs::PermissionsExt;

    if path.exists() {
        std::fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    let listener = tokio::net::UnixListener::bind(path).map_err(|error| error.to_string())?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;
    Ok((listener, ownership))
}

#[cfg(unix)]
pub async fn serve_broker(
    listener: tokio::net::UnixListener,
    core: std::sync::Arc<CoreApp>,
    events: EventSink,
    master_key: String,
) -> Result<(), String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    loop {
        let (stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
        let core = core.clone();
        let events = events.clone();
        let master_key = master_key.clone();
        tokio::spawn(async move {
            let (reader, mut writer) = stream.into_split();
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let response = match serde_json::from_str::<BrokerRequest>(&line) {
                    Ok(request) => dispatch_broker(&core, &events, &master_key, request).await,
                    Err(_) => BrokerResponse::error("invalid scene broker request"),
                };
                let Ok(mut encoded) = serde_json::to_vec(&response) else {
                    break;
                };
                encoded.push(b'\n');
                if writer.write_all(&encoded).await.is_err() {
                    break;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        approval_accepted, disabled_scene_results, scene_results, secure_eq, tools,
        SCENE_INSTRUCTIONS,
    };
    use serde_json::json;

    #[test]
    fn exposes_dynamic_scene_discovery_before_selection() {
        let available = tools();
        let names = available
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool.get("name").unwrap().as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(names, ["scene_list", "scene_select"]);
    }

    #[test]
    fn scene_instructions_use_the_status_probe_instead_of_a_prompt_marker() {
        assert!(SCENE_INSTRUCTIONS.contains("returned `enabled` status"));
        assert!(!SCENE_INSTRUCTIONS.contains("enabled in the prompt"));
    }

    #[test]
    fn scene_search_returns_bounded_routing_metadata() {
        let scenes = codetwo_core::SceneLibrary::builtin();
        let result = scene_results(&scenes, Some("fix"), 1, None);
        assert_eq!(result.pointer("/enabled").unwrap(), true);
        assert_eq!(
            result.pointer("/scenes/0/reference").unwrap(),
            "builtin:fix"
        );
        assert!(result.pointer("/scenes/0/title").is_some());
        assert!(result.pointer("/scenes/0/description").is_some());
        assert!(result.pointer("/scenes/0/instructions").is_none());
    }

    #[test]
    fn disabled_scene_status_is_bounded_and_non_failing() {
        let result = disabled_scene_results(Some("fix"));
        assert_eq!(result.pointer("/enabled").unwrap(), false);
        assert_eq!(result.pointer("/scenes").unwrap(), &json!([]));
        assert!(result.pointer("/current").unwrap().is_null());
    }

    #[test]
    fn approvals_and_session_keys_are_fail_closed() {
        assert!(approval_accepted(&json!({
            "result": { "action": "accept", "content": { "decision": "allow_once" } }
        })));
        assert!(!approval_accepted(&json!({
            "result": { "action": "accept", "content": { "decision": "deny" } }
        })));
        assert!(secure_eq("same", "same"));
        assert!(!secure_eq("same", "different"));
    }
}

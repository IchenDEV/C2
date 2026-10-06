//! A session-bound stdio MCP adapter. The loopback broker lives inside the owning Core.
//! Credentials are injected by Engine, never accepted as model tool arguments.
use crate::{
    assistant::WorkerOperation,
    skill::{McpServer, McpTransport},
};
use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::{mpsc, oneshot},
};

#[derive(Clone)]
pub struct AssistantBridge {
    pub address: String,
    pub executable: String,
    secret: String,
}
pub type WorkerCall = (
    String,
    String,
    WorkerOperation,
    oneshot::Sender<Result<Value, String>>,
);
impl AssistantBridge {
    pub async fn bind() -> Result<(Self, TcpListener), String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| e.to_string())?;
        Ok((
            Self {
                address: listener
                    .local_addr()
                    .map_err(|e| e.to_string())?
                    .to_string(),
                executable: std::env::current_exe()
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .into_owned(),
                secret: uuid::Uuid::new_v4().to_string(),
            },
            listener,
        ))
    }
    fn key(&self, session: &str) -> String {
        blake3::hash(format!("codetwo-assistant\0{}\0{session}", self.secret).as_bytes())
            .to_hex()
            .to_string()
    }
    pub fn server(&self, session: &str) -> McpServer {
        McpServer {
            name: "codetwo_coordination".into(),
            cwd: None,
            transport: McpTransport::Stdio {
                command: self.executable.clone(),
                args: vec!["--codetwo-assistant-mcp".into()],
                env: vec![
                    ("CODETWO_COORDINATION_ADDRESS".into(), self.address.clone()),
                    ("CODETWO_COORDINATION_SESSION".into(), session.into()),
                    ("CODETWO_COORDINATION_KEY".into(), self.key(session)),
                ],
            },
        }
    }
    fn authentic(&self, session: &str, key: &str) -> bool {
        let expected = self.key(session);
        expected.len() == key.len()
            && expected
                .bytes()
                .zip(key.bytes())
                .fold(0u8, |diff, (a, b)| diff | (a ^ b))
                == 0
    }
}

pub async fn serve(
    listener: TcpListener,
    config: AssistantBridge,
    calls: mpsc::Sender<WorkerCall>,
) {
    let permits = std::sync::Arc::new(tokio::sync::Semaphore::new(16));
    let mut connections = tokio::task::JoinSet::new();
    loop {
        let accepted = tokio::select! {accepted=listener.accept()=>accepted,_=connections.join_next(),if !connections.is_empty()=>continue};
        let Ok((stream, _)) = accepted else { break };
        let Ok(permit) = permits.clone().try_acquire_owned() else {
            continue;
        };
        let config = config.clone();
        let calls = calls.clone();
        connections.spawn(async move {
            let _permit = permit;
            let mut reader = BufReader::new(stream).take(65537);
            let mut bytes = Vec::new();
            let response = async {
                tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    reader.read_until(b'\n', &mut bytes),
                )
                .await
                .map_err(|_| "coordination read timeout".to_string())?
                .map_err(|e| e.to_string())?;
                if bytes.len() > 65536 {
                    return Err("coordination input too large".into());
                }
                let v: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                let session = v["session"].as_str().ok_or("session required")?;
                let key = v["key"].as_str().ok_or("credential required")?;
                if !config.authentic(session, key) {
                    return Err("invalid coordination credential".into());
                }
                let command_id = v["command_id"]
                    .as_str()
                    .ok_or("stable command_id required")?;
                let operation =
                    serde_json::from_value(v["operation"].clone()).map_err(|e| e.to_string())?;
                let (reply, receive) = oneshot::channel();
                calls
                    .send((session.into(), command_id.into(), operation, reply))
                    .await
                    .map_err(|_| "coordination unavailable".to_string())?;
                tokio::time::timeout(std::time::Duration::from_secs(30), receive)
                    .await
                    .map_err(|_| {
                        "coordination outcome unknown; inspect before retrying".to_string()
                    })?
                    .map_err(|_| "coordination stopped".to_string())?
            }
            .await;
            let value = match response {
                Ok(value) => json!({"result":value}),
                Err(error) => json!({"error":error}),
            };
            let mut stream = reader.into_inner().into_inner();
            let _ = stream.write_all(format!("{value}\n").as_bytes()).await;
        });
    }
}

fn schema(mut properties: Value, required: &[&str]) -> Value {
    let mut required = required.to_vec();
    if !properties.as_object().unwrap().is_empty() {
        required.push("command_id");
    }
    properties["command_id"] = json!({"type":"string","description":"Unique stable identifier for this operation. Reuse it only to retry identical arguments after an unknown outcome."});
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
pub fn tools() -> Value {
    let string = json!({"type":"string"});
    json!([
      {"name":"context","description":"Read your current goal, requirement version, messages, questions and relevant project memory.","inputSchema":schema(json!({}),&[])},
      {"name":"progress","description":"Report evidence-based progress and blockers to the chief of staff.","inputSchema":schema(json!({"content":string}),&["content"])},
      {"name":"ask","description":"Ask a project clarification. Stop dependent work when blocking; await an explicit answer. Does not authorize external actions.","inputSchema":schema(json!({"title":string,"context":string,"options":{"type":"array","items":{"type":"string"}},"blocking":{"type":"boolean"},"factual":{"type":"boolean","description":"True only for a fact question answerable from an already confirmed project decision; product choices and authorization remain false."}}),&["title","context"])},
      {"name":"propose_change","description":"Propose changed requirements with impact and reason. The user must decide before the new direction is adopted.","inputSchema":schema(json!({"title":string,"acceptance":string,"reason":string}),&["title","acceptance","reason"])},
      {"name":"confirm","description":"Explicitly acknowledge the current requirements and delivered message ids before working. Read context first.","inputSchema":schema(json!({"contract_revision":{"type":"integer","minimum":1},"message_ids":{"type":"array","items":{"type":"string"}}}),&["contract_revision"])},
      {"name":"submit","description":"Submit versioned files and actual checks for review. Submission is not acceptance.","inputSchema":schema(json!({"contract_revision":{"type":"integer","minimum":1},"evidence":string,"artifacts":{"type":"array","items":{"type":"object","properties":{"path":{"type":"string"},"sha256":{"type":"string"}},"required":["path","sha256"],"additionalProperties":false}}}),&["contract_revision","evidence","artifacts"])}
    ])
}

pub fn run_stdio() -> Result<(), String> {
    let address = std::env::var("CODETWO_COORDINATION_ADDRESS")
        .map_err(|_| "coordination address missing")?;
    let address: std::net::SocketAddr = address
        .parse()
        .map_err(|_| "invalid coordination address")?;
    if !address.ip().is_loopback() {
        return Err("coordination must use loopback".into());
    }
    let session = std::env::var("CODETWO_COORDINATION_SESSION")
        .map_err(|_| "coordination session missing")?;
    let key =
        std::env::var("CODETWO_COORDINATION_KEY").map_err(|_| "coordination credential missing")?;
    let input = std::io::stdin();
    let mut reader = input.lock();
    let mut out = std::io::stdout().lock();
    loop {
        let mut line = Vec::new();
        let count = reader
            .by_ref()
            .take(65537)
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        if line.len() > 65536 {
            return Err("MCP input too large".into());
        }
        let request: Value = serde_json::from_slice(&line).map_err(|e| e.to_string())?;
        let Some(id) = request.get("id") else {
            continue;
        };
        let result: Result<Value, String> = match request["method"].as_str().unwrap_or("") {
            "initialize" => Ok(
                json!({"protocolVersion":request["params"]["protocolVersion"].as_str().unwrap_or("2024-11-05"),"capabilities":{"tools":{}},"serverInfo":{"name":"codetwo_coordination","version":"1.0.0"}}),
            ),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools":tools()})),
            "tools/call" => (|| {
                let name = request["params"]["name"]
                    .as_str()
                    .ok_or("tool name required")?;
                if !tools()
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|t| t["name"] == name)
                {
                    return Err("unknown coordination tool".into());
                }
                let mut args = request["params"]["arguments"]
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                if args.contains_key("operation") {
                    return Err("operation is host-bound".into());
                }
                let command_id = args
                    .remove("command_id")
                    .and_then(|v| v.as_str().map(String::from))
                    .or_else(|| (name == "context").then(|| uuid::Uuid::new_v4().to_string()))
                    .ok_or("stable command_id required")?;
                args.insert("operation".into(), json!(name));
                let operation: WorkerOperation =
                    serde_json::from_value(Value::Object(args)).map_err(|e| e.to_string())?;
                let mut stream = std::net::TcpStream::connect_timeout(
                    &address,
                    std::time::Duration::from_secs(5),
                )
                .map_err(|e| e.to_string())?;
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(35)))
                    .map_err(|e| e.to_string())?;
                stream
                    .set_write_timeout(Some(std::time::Duration::from_secs(5)))
                    .map_err(|e| e.to_string())?;
                writeln!(stream,"{}",json!({"session":session,"key":key,"command_id":command_id,"operation":operation})).map_err(|e|e.to_string())?;
                let mut reply = String::new();
                std::io::BufReader::new(stream)
                    .take(65537)
                    .read_line(&mut reply)
                    .map_err(|e| e.to_string())?;
                if reply.len() > 65536 {
                    return Err("coordination reply too large".into());
                }
                let response: Value = serde_json::from_str(&reply).map_err(|e| e.to_string())?;
                Ok(
                    json!({"content":[{"type":"text","text":response.get("result").unwrap_or(&response).to_string()}],"isError":response.get("error").is_some()}),
                )
            })(),
            _ => Err("unsupported MCP method".into()),
        };
        let response = match result {
            Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
            Err(error) => json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":error}}),
        };
        writeln!(out, "{response}").map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}

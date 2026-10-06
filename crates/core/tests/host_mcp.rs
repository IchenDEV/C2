//! Host MCP registry and JSON-RPC protocol tests.

use codetwo_core::host_mcp::{
    handle_request, HostMcpCapability, HostMcpDispatch, HostMcpRegistry, HostMcpScope,
    PROTOCOL_VERSION, TOOL_CAPABILITIES,
};
use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
use codetwo_core::skill::SkillLibrary;
use codetwo_core::{Engine, Store};
use serde_json::json;
use std::sync::{Arc, Mutex};

fn engine() -> Engine {
    let (engine, _events) = Engine::new(
        vec![Provider {
            id: ProviderId::ClaudeCode,
            display_name: "Claude".into(),
            needs_node: false,
            launch: LaunchSpec {
                command: "false".into(),
                args: Vec::new(),
                env: Vec::new(),
                cwd: None,
            },
        }],
        SkillLibrary::default(),
    );
    engine
}

fn issue(registry: &mut HostMcpRegistry, session: &str) -> (String, String) {
    registry
        .issue(HostMcpScope {
            session_id: session.into(),
            provider_id: "claude_code".into(),
            capabilities: HostMcpCapability::default_read_only_set(),
        })
        .unwrap()
}

#[test]
fn protocol_serves_handshake_and_refuses_calls_for_sessions_that_are_not_live() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let session = codetwo_core::session::Session::new(
        ProviderId::ClaudeCode,
        std::env::temp_dir().to_string_lossy().into_owned(),
    );
    store.upsert_session(&session).unwrap();
    let (engine, _rx) = Engine::with_store(
        vec![Provider {
            id: ProviderId::ClaudeCode,
            display_name: "Claude".into(),
            needs_node: false,
            launch: LaunchSpec {
                command: "false".into(),
                args: Vec::new(),
                env: Vec::new(),
                cwd: None,
            },
        }],
        SkillLibrary::default(),
        store,
    );
    let mut registry = HostMcpRegistry::new();
    let (_id, token) = issue(&mut registry, &session.id);
    let registry = Arc::new(Mutex::new(registry));
    let dispatch = HostMcpDispatch::new(engine.clone(), registry, token);

    let init = handle_request(
        &dispatch,
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion": PROTOCOL_VERSION}}),
    );
    assert_eq!(init["result"]["protocolVersion"], PROTOCOL_VERSION);

    let tools = handle_request(
        &dispatch,
        &json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    );
    let names: Vec<_> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&TOOL_CAPABILITIES.to_string()));

    let call = handle_request(
        &dispatch,
        &json!({
            "jsonrpc":"2.0",
            "id":3,
            "method":"tools/call",
            "params":{"name": TOOL_CAPABILITIES, "arguments": {}}
        }),
    );
    // The stored session has no live runtime, so a valid credential is still refused (fail
    // closed). The live success path is covered by the Engine-level test.
    assert_eq!(call["error"]["message"], "session is not live");
}

#[test]
fn cross_session_read_denied() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let caller =
        codetwo_core::session::Session::new(ProviderId::ClaudeCode, "/tmp/project-a".to_string());
    let other =
        codetwo_core::session::Session::new(ProviderId::ClaudeCode, "/tmp/project-b".to_string());
    store.upsert_session(&caller).unwrap();
    store.upsert_session(&other).unwrap();
    let (engine, _rx) = Engine::with_store(
        vec![Provider {
            id: ProviderId::ClaudeCode,
            display_name: "Claude".into(),
            needs_node: false,
            launch: LaunchSpec {
                command: "false".into(),
                args: Vec::new(),
                env: Vec::new(),
                cwd: None,
            },
        }],
        SkillLibrary::default(),
        store,
    );
    let mut registry = HostMcpRegistry::new();
    let (_id, token) = issue(&mut registry, &caller.id);
    let registry = Arc::new(Mutex::new(registry));
    let dispatch = HostMcpDispatch::new(engine, registry, token);
    let response = handle_request(
        &dispatch,
        &json!({
            "jsonrpc":"2.0",
            "id":4,
            "method":"tools/call",
            "params":{
                "name":"codetwo_session_read",
                "arguments":{"session_id": other.id}
            }
        }),
    );
    assert_eq!(response["error"]["message"], "cross-session read denied");
}

#[test]
fn tools_call_rejects_invalid_params() {
    let engine = engine();
    let mut registry = HostMcpRegistry::new();
    let (_id, token) = issue(&mut registry, "s1");
    let dispatch = HostMcpDispatch::new(engine, Arc::new(Mutex::new(registry)), token);
    let missing_name = handle_request(
        &dispatch,
        &json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"arguments":{}}}),
    );
    assert_eq!(missing_name["error"]["code"], -32602);
}

#[test]
fn unknown_tool_returns_error() {
    let engine = engine();
    let mut registry = HostMcpRegistry::new();
    let (_id, token) = issue(&mut registry, "s1");
    let dispatch = HostMcpDispatch::new(engine, Arc::new(Mutex::new(registry)), token);
    let response = handle_request(
        &dispatch,
        &json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"nope","arguments":{}}}),
    );
    assert!(response.get("error").is_some());
}

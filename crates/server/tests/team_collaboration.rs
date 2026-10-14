use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
use codetwo_core::skill::SkillLibrary;
use codetwo_core::{CanvasFeatureGate, Engine, MemberId, Store, WorkspaceId, WorkspaceRole};
use codetwo_server::{bind_and_serve_with_canvas, fanout, AuthState, DEFAULT_PAIRING_TTL};
use futures_util::{SinkExt, StreamExt};
use reqwest::{Client, StatusCode};
use tokio_tungstenite::tungstenite::Message;

const TEAM_AGENT: &str = r#"
import json, sys

def send(message):
    print(json.dumps(message), flush=True)

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    message = json.loads(line)
    method = message.get("method")
    mid = message.get("id")
    if method == "initialize":
        send({"jsonrpc":"2.0","id":mid,"result":{"protocolVersion":1}})
    elif method == "session/new":
        send({"jsonrpc":"2.0","id":mid,"result":{"sessionId":"team-agent-session"}})
    elif method == "session/prompt":
        with open(sys.argv[1], "a", encoding="utf-8") as log:
            log.write(json.dumps(message.get("params", {}), sort_keys=True) + "\n")
        send({"jsonrpc":"2.0","id":mid,"result":{"stopReason":"end_turn"}})
"#;

fn git_project() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    for args in [
        vec!["init"],
        vec!["config", "user.email", "team-test@example.com"],
        vec!["config", "user.name", "Team Test"],
    ] {
        assert!(Command::new("git")
            .args(args)
            .current_dir(directory.path())
            .status()
            .unwrap()
            .success());
    }
    std::fs::write(directory.path().join("README.md"), "team test\n").unwrap();
    assert!(Command::new("git")
        .args(["add", "README.md"])
        .current_dir(directory.path())
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(directory.path())
        .status()
        .unwrap()
        .success());
    directory
}

async fn pair_member(
    client: &Client,
    base: &str,
    auth: &AuthState,
    member_id: &MemberId,
    device_name: &str,
) -> (String, String) {
    let token = auth.issue_member_pairing_token(member_id.as_str(), DEFAULT_PAIRING_TTL);
    let response = client
        .post(format!("{base}/api/pair"))
        .json(&serde_json::json!({ "token": token, "device_name": device_name }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["member_id"], member_id.as_str());
    (
        body["device_id"].as_str().unwrap().to_string(),
        body["bearer"].as_str().unwrap().to_string(),
    )
}

async fn json_response(response: reqwest::Response) -> (StatusCode, serde_json::Value) {
    let status = response.status();
    let bytes = response.bytes().await.unwrap();
    let value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| serde_json::json!({ "raw": String::from_utf8_lossy(&bytes) }));
    (status, value)
}

#[tokio::test]
async fn alice_and_bob_converge_and_one_approval_starts_one_execution() {
    let project = git_project();
    let prompt_log = project.path().join("prompt-log.jsonl");
    let provider_id = ProviderId::Custom("team-test".into());
    let mut launch = LaunchSpec::new("python3", ["-u", "-c", TEAM_AGENT]);
    launch.args.push(prompt_log.to_string_lossy().to_string());
    let provider = Provider {
        id: provider_id.clone(),
        display_name: "Team test provider".into(),
        launch,
        needs_node: false,
    };
    let store = Arc::new(Store::open_in_memory().unwrap());
    let workspace_id = WorkspaceId::new("workspace-team");
    let alice = MemberId::new("member-alice");
    let bob = MemberId::new("member-bob");
    store
        .create_workspace(workspace_id.clone(), "CodeTwo Team", 1)
        .unwrap();
    store
        .create_member(
            &workspace_id,
            alice.clone(),
            "Alice",
            WorkspaceRole::Admin,
            2,
        )
        .unwrap();
    store
        .create_member(&workspace_id, bob.clone(), "Bob", WorkspaceRole::Member, 3)
        .unwrap();

    let (engine, rx) = Engine::with_store(vec![provider], SkillLibrary::new(vec![]), store.clone());
    let engine = Arc::new(engine);
    let events = fanout(rx);
    let auth = Arc::new(AuthState::load(None));
    let (address, server) = bind_and_serve_with_canvas(
        engine,
        events,
        "127.0.0.1:0".parse().unwrap(),
        auth.clone(),
        store.clone(),
        CanvasFeatureGate::default(),
    )
    .await
    .unwrap();
    let base = format!("http://{address}");
    let client = Client::new();
    let (_alice_device, alice_bearer) =
        pair_member(&client, &base, &auth, &alice, "Alice Mac").await;
    let (bob_device, bob_bearer) = pair_member(&client, &base, &auth, &bob, "Bob Mac").await;
    let team_terminal_list = client
        .get(format!("{base}/api/terminals"))
        .bearer_auth(&bob_bearer)
        .send()
        .await
        .unwrap();
    assert_eq!(team_terminal_list.status(), StatusCode::FORBIDDEN);
    let team_canvas = client
        .get(format!("{base}/api/canvas/feature"))
        .bearer_auth(&bob_bearer)
        .send()
        .await
        .unwrap();
    assert_eq!(team_canvas.status(), StatusCode::FORBIDDEN);
    let team_terminal_ticket = auth.issue_ws_ticket(&bob_device);
    let terminal_error = tokio_tungstenite::connect_async(format!(
        "ws://{address}/ws/terminal?ticket={team_terminal_ticket}"
    ))
    .await
    .unwrap_err();
    assert!(matches!(
        terminal_error,
        tokio_tungstenite::tungstenite::Error::Http(response)
            if response.status() == StatusCode::FORBIDDEN
    ));
    let legacy_token = auth.issue_pairing_token(DEFAULT_PAIRING_TTL);
    let legacy = auth.pair(&legacy_token, "Legacy browser").unwrap();
    let legacy_team = client
        .get(format!("{base}/api/team/v1/workspace"))
        .bearer_auth(&legacy.bearer)
        .send()
        .await
        .unwrap();
    assert_eq!(legacy_team.status(), StatusCode::UNAUTHORIZED);

    let create = client
        .post(format!("{base}/api/team/v1/tasks"))
        .bearer_auth(&alice_bearer)
        .json(&serde_json::json!({
            "task_id": "task-team",
            "goal": "Implement the shared collaboration slice",
            "cwd": project.path(),
            "provider": { "custom": "team-test" },
            "collaborator_ids": ["member-bob"]
        }))
        .send()
        .await
        .unwrap();
    let (status, created) = json_response(create).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["collaboration"]["revision"], 1);

    let alice_snapshot = client
        .get(format!("{base}/api/team/v1/tasks/task-team"))
        .bearer_auth(&alice_bearer)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let bob_snapshot = client
        .get(format!("{base}/api/team/v1/tasks/task-team"))
        .bearer_auth(&bob_bearer)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(alice_snapshot, bob_snapshot);

    let comment = client
        .post(format!("{base}/api/team/v1/tasks/task-team/comments"))
        .bearer_auth(&bob_bearer)
        .json(&serde_json::json!({
            "expected_revision": 1,
            "body": "Please keep the existing Task execution path."
        }))
        .send()
        .await
        .unwrap();
    let (status, comment) = json_response(comment).await;
    assert_eq!(status, StatusCode::OK, "{comment}");
    assert_eq!(comment["comments"][0]["author_id"], "member-bob");
    assert_eq!(comment["revision"], 2);

    let suggestion = client
        .post(format!("{base}/api/team/v1/tasks/task-team/suggestions"))
        .bearer_auth(&bob_bearer)
        .json(&serde_json::json!({
            "expected_revision": 2,
            "body": "Run the accepted implementation once."
        }))
        .send()
        .await
        .unwrap();
    let (status, suggestion) = json_response(suggestion).await;
    assert_eq!(status, StatusCode::CREATED, "{suggestion}");
    let suggestion_id = suggestion["suggestions"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(suggestion["revision"], 3);
    let alice_attention: serde_json::Value = client
        .get(format!("{base}/api/team/v1/attention"))
        .bearer_auth(&alice_bearer)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(alice_attention.as_array().unwrap().len(), 1);
    assert_eq!(alice_attention[0]["kind"], "pending_suggestion");
    assert_eq!(alice_attention[0]["suggestion_id"], suggestion_id);
    let bob_attention: serde_json::Value = client
        .get(format!("{base}/api/team/v1/attention"))
        .bearer_auth(&bob_bearer)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(bob_attention.as_array().unwrap().is_empty());

    let forbidden = client
        .post(format!(
            "{base}/api/team/v1/tasks/task-team/suggestions/{suggestion_id}/approve"
        ))
        .bearer_auth(&bob_bearer)
        .json(&serde_json::json!({
            "command_id": "bob-approval",
            "expected_revision": 3
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    // Bob's transport is no longer involved. Once Alice accepts, Core owns the execution.
    let approval_url =
        format!("{base}/api/team/v1/tasks/task-team/suggestions/{suggestion_id}/approve");
    let approved = client
        .post(&approval_url)
        .bearer_auth(&alice_bearer)
        .json(&serde_json::json!({
            "command_id": "alice-approval-once",
            "expected_revision": 3
        }))
        .send()
        .await
        .unwrap();
    let (status, approved) = json_response(approved).await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    assert_eq!(approved["receipt"]["revision"], 4);
    assert_eq!(
        approved["snapshot"]["runtime"]["session_leases"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let resolved_attention: serde_json::Value = client
        .get(format!("{base}/api/team/v1/attention"))
        .bearer_auth(&alice_bearer)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(resolved_attention.as_array().unwrap().is_empty());

    let replay = client
        .post(&approval_url)
        .bearer_auth(&alice_bearer)
        .json(&serde_json::json!({
            "command_id": "alice-approval-once",
            "expected_revision": 3
        }))
        .send()
        .await
        .unwrap();
    let (status, replay) = json_response(replay).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["receipt"], approved["receipt"]);
    assert_eq!(
        replay["snapshot"]["runtime"]["session_leases"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let lines = std::fs::read_to_string(&prompt_log).unwrap_or_default();
            if lines.lines().count() == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the approved Suggestion reaches the provider exactly once");

    let session_id = approved["snapshot"]["runtime"]["session_leases"][0]["session_id"]
        .as_str()
        .unwrap();
    let legacy_ticket = auth.issue_ws_ticket(&legacy.device_id);
    let (mut legacy_socket, _) =
        tokio_tungstenite::connect_async(format!("ws://{address}/ws?ticket={legacy_ticket}"))
            .await
            .unwrap();
    let legacy_welcome = legacy_socket
        .next()
        .await
        .unwrap()
        .unwrap()
        .into_text()
        .unwrap();
    assert!(!legacy_welcome.contains(session_id));
    legacy_socket
        .send(Message::Text(
            serde_json::json!({
                "op": "prompt",
                "session": session_id,
                "doc": [{ "type": "text", "text": "unbound device bypass" }],
                "request_id": "legacy-raw-prompt"
            })
            .to_string(),
        ))
        .await
        .unwrap();
    let legacy_rejection = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = legacy_socket
                .next()
                .await
                .unwrap()
                .unwrap()
                .into_text()
                .unwrap();
            if message.contains("member-bound device is required") {
                break message;
            }
        }
    })
    .await
    .unwrap();
    assert!(legacy_rejection.contains("legacy-raw-prompt"));
    drop(legacy_socket);

    let bob_ticket = auth.issue_ws_ticket(&bob_device);
    let (mut bob_socket, _) =
        tokio_tungstenite::connect_async(format!("ws://{address}/ws?ticket={bob_ticket}"))
            .await
            .unwrap();
    let welcome = bob_socket
        .next()
        .await
        .unwrap()
        .unwrap()
        .into_text()
        .unwrap();
    assert!(welcome.contains(session_id));
    bob_socket
        .send(Message::Text(
            serde_json::json!({
                "op": "prompt",
                "session": session_id,
                "doc": [{ "type": "text", "text": "bypass owner approval" }],
                "request_id": "bob-raw-prompt"
            })
            .to_string(),
        ))
        .await
        .unwrap();
    let rejection = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = bob_socket
                .next()
                .await
                .unwrap()
                .unwrap()
                .into_text()
                .unwrap();
            if message.contains("Task owner approval is required") {
                break message;
            }
        }
    })
    .await
    .unwrap();
    assert!(rejection.contains("bob-raw-prompt"));
    assert_eq!(
        std::fs::read_to_string(&prompt_log)
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert!(auth.revoke_device(&bob_device));
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(message) = bob_socket.next().await {
            if message.is_err() {
                break;
            }
        }
    })
    .await
    .expect("revoking a team Device closes its live socket");
    let revoked = client
        .get(format!("{base}/api/team/v1/tasks/task-team"))
        .bearer_auth(&bob_bearer)
        .send()
        .await
        .unwrap();
    assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);

    server.abort();
}


/// A task whose leased Core session is another actor's private session must disappear from every
/// Team surface for other members (common ShareStore can_view), while the creator and a public
/// session keep working.
#[tokio::test]
async fn private_leased_session_hides_team_task_from_other_members() {
    let project = git_project();
    let prompt_log = project.path().join("prompt-log.jsonl");
    let provider_id = ProviderId::Custom("team-test".into());
    let mut launch = LaunchSpec::new("python3", ["-u", "-c", TEAM_AGENT]);
    launch.args.push(prompt_log.to_string_lossy().to_string());
    let provider = Provider {
        id: provider_id,
        display_name: "Team test provider".into(),
        launch,
        needs_node: false,
    };
    let store = Arc::new(Store::open_in_memory().unwrap());
    let workspace_id = WorkspaceId::new("workspace-team");
    let alice = MemberId::new("member-alice");
    let bob = MemberId::new("member-bob");
    store
        .create_workspace(workspace_id.clone(), "CodeTwo Team", 1)
        .unwrap();
    store
        .create_member(&workspace_id, alice.clone(), "Alice", WorkspaceRole::Admin, 2)
        .unwrap();
    store
        .create_member(&workspace_id, bob.clone(), "Bob", WorkspaceRole::Member, 3)
        .unwrap();
    let (engine, rx) = Engine::with_store(vec![provider], SkillLibrary::new(vec![]), store.clone());
    let engine = Arc::new(engine);
    let share = Arc::new(codetwo_core::share::ShareStore::in_memory());
    engine.attach_share_store(share.clone());
    let auth = Arc::new(AuthState::load(None));
    let (address, _server) = bind_and_serve_with_canvas(
        engine,
        fanout(rx),
        "127.0.0.1:0".parse().unwrap(),
        auth.clone(),
        store.clone(),
        CanvasFeatureGate::default(),
    )
    .await
    .unwrap();
    let base = format!("http://{address}");
    let client = Client::new();
    let (alice_device, alice_bearer) = pair_member(&client, &base, &auth, &alice, "Alice Mac").await;
    let (_bob_device, bob_bearer) = pair_member(&client, &base, &auth, &bob, "Bob Mac").await;

    let (status, created) = json_response(
        client
            .post(format!("{base}/api/team/v1/tasks"))
            .bearer_auth(&alice_bearer)
            .json(&serde_json::json!({
                "task_id": "task-private", "goal": "Private leased work",
                "cwd": project.path(), "provider": { "custom": "team-test" },
                "collaborator_ids": ["member-bob"]
            }))
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let (_, suggestion) = json_response(
        client
            .post(format!("{base}/api/team/v1/tasks/task-private/suggestions"))
            .bearer_auth(&bob_bearer)
            .json(&serde_json::json!({ "expected_revision": 1, "body": "SECRET-SUGGESTION-BODY" }))
            .send()
            .await
            .unwrap(),
    )
    .await;
    let suggestion_id = suggestion["suggestions"][0]["id"].as_str().unwrap().to_string();
    let (status, approved) = json_response(
        client
            .post(format!(
                "{base}/api/team/v1/tasks/task-private/suggestions/{suggestion_id}/approve"
            ))
            .bearer_auth(&alice_bearer)
            .json(&serde_json::json!({ "command_id": "approve-private", "expected_revision": 2 }))
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    let session_id = approved["snapshot"]["runtime"]["session_leases"][0]["session_id"]
        .as_str()
        .expect("the approved Suggestion attaches an engine session lease")
        .to_string();

    // Public: Bob sees the leased session id through the Team snapshot (authorized behaviour).
    let alice_participant = share.participant(&alice_device);
    share
        .register_session(&session_id, &alice_participant, codetwo_core::share::Visibility::Public)
        .unwrap();
    let public_detail = client
        .get(format!("{base}/api/team/v1/tasks/task-private"))
        .bearer_auth(&bob_bearer)
        .send()
        .await
        .unwrap();
    assert_eq!(public_detail.status(), StatusCode::OK);
    assert!(public_detail.text().await.unwrap().contains(&session_id));

    // Creator makes the session private: every Team route hides it from Bob.
    share
        .set_visibility(&session_id, &alice_participant, codetwo_core::share::Visibility::Private)
        .unwrap();
    for (method, path, body) in [
        ("GET", "tasks/task-private", None),
        (
            "POST",
            "tasks/task-private/comments",
            Some(serde_json::json!({ "expected_revision": 4, "body": "probe" })),
        ),
        (
            "POST",
            "tasks/task-private/suggestions",
            Some(serde_json::json!({ "expected_revision": 4, "body": "probe" })),
        ),
    ] {
        let url = format!("{base}/api/team/v1/{path}");
        let request = if method == "GET" { client.get(url) } else { client.post(url) };
        let request = request.bearer_auth(&bob_bearer);
        let response = match body {
            Some(body) => request.json(&body),
            None => request,
        }
        .send()
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {path}");
        let text = response.text().await.unwrap();
        assert!(!text.contains(&session_id) && !text.contains("SECRET-SUGGESTION-BODY"));
    }
    let bob_list = client
        .get(format!("{base}/api/team/v1/tasks"))
        .bearer_auth(&bob_bearer)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!bob_list.contains(&session_id) && !bob_list.contains("task-private"), "{bob_list}");
    let alice_detail = client
        .get(format!("{base}/api/team/v1/tasks/task-private"))
        .bearer_auth(&alice_bearer)
        .send()
        .await
        .unwrap();
    assert_eq!(alice_detail.status(), StatusCode::OK);
    assert!(alice_detail.text().await.unwrap().contains(&session_id));
}

/// Typed session fields without a session lease: `assigned_session_id` is set through the Task
/// Graph (no lease constructor invariant), so the Team surfaces must judge it by the same common
/// `can_view` policy instead of relying on leases alone.
#[tokio::test(flavor = "multi_thread")]
async fn unleased_assigned_private_session_hides_team_task_from_other_members() {
    use codetwo_core::{TaskGraph, TaskId, WorkItem, WorkItemId, WorkItemStatus};
    let project = git_project();
    let prompt_log = project.path().join("prompt-log.jsonl");
    let provider_id = ProviderId::Custom("team-test".into());
    let mut launch = LaunchSpec::new("python3", ["-u", "-c", TEAM_AGENT]);
    launch.args.push(prompt_log.to_string_lossy().to_string());
    let provider = Provider {
        id: provider_id,
        display_name: "Team test provider".into(),
        launch,
        needs_node: false,
    };
    let store = Arc::new(Store::open_in_memory().unwrap());
    let workspace_id = WorkspaceId::new("workspace-team");
    let alice = MemberId::new("member-alice");
    let bob = MemberId::new("member-bob");
    store.create_workspace(workspace_id.clone(), "CodeTwo Team", 1).unwrap();
    store
        .create_member(&workspace_id, alice.clone(), "Alice", WorkspaceRole::Admin, 2)
        .unwrap();
    store
        .create_member(&workspace_id, bob.clone(), "Bob", WorkspaceRole::Member, 3)
        .unwrap();
    let (engine, rx) = Engine::with_store(vec![provider], SkillLibrary::new(vec![]), store.clone());
    let engine = Arc::new(engine);
    let share = Arc::new(codetwo_core::share::ShareStore::in_memory());
    engine.attach_share_store(share.clone());
    let auth = Arc::new(AuthState::load(None));
    let (address, _server) = bind_and_serve_with_canvas(
        engine,
        fanout(rx),
        "127.0.0.1:0".parse().unwrap(),
        auth.clone(),
        store.clone(),
        CanvasFeatureGate::default(),
    )
    .await
    .unwrap();
    let base = format!("http://{address}");
    let client = Client::new();
    let (alice_device, alice_bearer) = pair_member(&client, &base, &auth, &alice, "Alice Mac").await;
    let (_bob_device, bob_bearer) = pair_member(&client, &base, &auth, &bob, "Bob Mac").await;
    let (status, created) = json_response(
        client
            .post(format!("{base}/api/team/v1/tasks"))
            .bearer_auth(&alice_bearer)
            .json(&serde_json::json!({
                "task_id": "task-unleased", "goal": "Unleased assignment",
                "cwd": project.path(), "provider": { "custom": "team-test" },
                "collaborator_ids": ["member-bob"]
            }))
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    // No lease exists, yet a work item can name a (private) session of the creator.
    let hidden_session = "private-session-no-lease";
    let alice_participant = share.participant(&alice_device);
    share
        .register_session(hidden_session, &alice_participant, codetwo_core::share::Visibility::Private)
        .unwrap();
    let task_id = TaskId::new("task-unleased");
    assert!(store.list_task_session_leases(&task_id).unwrap().is_empty());
    let graph = TaskGraph {
        revision: 1,
        work_items: vec![WorkItem {
            id: WorkItemId::new("w1"),
            objective: "assigned without lease".into(),
            result_contract_conditions: Vec::new(),
            scenes: Vec::new(),
            agent_skills: Vec::new(),
            input_artifacts: Vec::new(),
            expected_outputs: Vec::new(),
            completion_evidence: Vec::new(),
            status: WorkItemStatus::Ready,
            blocker: None,
            assigned_session_id: Some(hidden_session.to_string().into()),
            reason: "test".into(),
        }],
        edges: Vec::new(),
    };
    store.apply_task_graph(&task_id, 0, &graph, "assign", 10).unwrap();

    let alice_detail = client
        .get(format!("{base}/api/team/v1/tasks/task-unleased"))
        .bearer_auth(&alice_bearer)
        .send()
        .await
        .unwrap();
    assert_eq!(alice_detail.status(), StatusCode::OK);
    assert!(alice_detail.text().await.unwrap().contains(hidden_session), "creator keeps the field");
    let bob_detail = client
        .get(format!("{base}/api/team/v1/tasks/task-unleased"))
        .bearer_auth(&bob_bearer)
        .send()
        .await
        .unwrap();
    assert_eq!(bob_detail.status(), StatusCode::NOT_FOUND);
    assert!(!bob_detail.text().await.unwrap().contains(hidden_session));
    let bob_list = client
        .get(format!("{base}/api/team/v1/tasks"))
        .bearer_auth(&bob_bearer)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!bob_list.contains(hidden_session) && !bob_list.contains("task-unleased"), "{bob_list}");
}

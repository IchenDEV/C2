use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use codetwo_core::canvas::CanvasFeatureGate;
use codetwo_core::skill::{builtin_skills, SkillLibrary};
use codetwo_core::{Engine, Store};
use codetwo_server::{bind_and_serve_with_web_ui, fanout, AuthState, WebUiCommandCaller};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, PartialEq)]
struct RecordedCall {
    device_id: String,
    name: String,
    args: Value,
    project_path: Option<String>,
}

#[derive(Default)]
struct RecordingCaller {
    calls: Mutex<Vec<RecordedCall>>,
}

#[async_trait::async_trait]
impl WebUiCommandCaller for RecordingCaller {
    async fn call(
        &self,
        device_id: &str,
        name: &str,
        args: Value,
        project_path: Option<String>,
    ) -> Result<Value, String> {
        self.calls.lock().unwrap().push(RecordedCall {
            device_id: device_id.to_string(),
            name: name.to_string(),
            args: args.clone(),
            project_path: project_path.clone(),
        });
        if name == "test.reject" {
            return Err("host rejected the command".into());
        }
        Ok(json!({
            "name": name,
            "args": args,
            "project_path": project_path,
        }))
    }
}

async fn http(addr: SocketAddr, bearer: Option<&str>, body: &str) -> (u16, String) {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let auth = bearer
        .map(|value| format!("Authorization: Bearer {value}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "POST /api/web-ui/call HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Type: application/json\r\n{auth}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or_default();
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default();
    (status, body)
}

async fn server(
    auth: Arc<AuthState>,
    caller: Option<Arc<dyn WebUiCommandCaller>>,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    server_with_assets(auth, caller, None).await
}

async fn server_with_assets(
    auth: Arc<AuthState>,
    caller: Option<Arc<dyn WebUiCommandCaller>>,
    web_ui_dir: Option<std::path::PathBuf>,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (engine, receiver) = Engine::with_store(
        codetwo_core::provider::default_registry(),
        SkillLibrary::new(builtin_skills()),
        store.clone(),
    );
    bind_and_serve_with_web_ui(
        Arc::new(engine),
        fanout(receiver),
        "127.0.0.1:0".parse().unwrap(),
        auth,
        store,
        CanvasFeatureGate::default(),
        None,
        caller,
        web_ui_dir,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn full_web_assets_replace_only_the_compact_spa_routes() {
    let assets = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(assets.path().join("assets")).unwrap();
    std::fs::write(
        assets.path().join("index.html"),
        "<!doctype html><title>Full Web UI fixture</title>",
    )
    .unwrap();
    std::fs::write(
        assets.path().join("assets/app.js"),
        "export default 'web-ui';",
    )
    .unwrap();
    let (full_addr, full_handle) = server_with_assets(
        Arc::new(AuthState::load(None)),
        None,
        Some(assets.path().to_path_buf()),
    )
    .await;

    let root = reqwest::get(format!("http://{full_addr}/"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(root.contains("Full Web UI fixture"));
    let deep_link = reqwest::get(format!("http://{full_addr}/pair"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(deep_link.contains("Full Web UI fixture"));
    let script = reqwest::get(format!("http://{full_addr}/assets/app.js"))
        .await
        .unwrap();
    assert_eq!(
        script.headers().get(reqwest::header::CONTENT_TYPE).unwrap(),
        "text/javascript"
    );
    assert!(script.text().await.unwrap().contains("web-ui"));
    assert_eq!(
        reqwest::get(format!("http://{full_addr}/health"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "ok"
    );
    full_handle.abort();

    let (compact_addr, compact_handle) = server(Arc::new(AuthState::load(None)), None).await;
    let compact = reqwest::get(format!("http://{compact_addr}/"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!compact.contains("Full Web UI fixture"));
    assert!(compact.contains("C2"));
    compact_handle.abort();
}

#[tokio::test]
async fn browser_commands_require_a_non_member_device_and_preserve_call_context() {
    let auth = Arc::new(AuthState::load(None));
    let token = auth.issue_pairing_token(Duration::from_secs(60));
    let paired = auth.pair(&token, "Web UI").unwrap();
    let member_token = auth.issue_member_pairing_token("member-1", Duration::from_secs(60));
    let member = auth.pair(&member_token, "Team member").unwrap();
    let caller = Arc::new(RecordingCaller::default());
    let (addr, handle) = server(auth, Some(caller.clone())).await;
    let body = json!({
        "name": "sessions.transcript",
        "args": { "session": "session-1", "limit": 20 },
        "project_path": "/projects/alpha",
    })
    .to_string();

    assert_eq!(http(addr, None, &body).await.0, 401);
    assert_eq!(http(addr, Some(&member.bearer), &body).await.0, 403);

    let (status, response) = http(addr, Some(&paired.bearer), &body).await;
    assert_eq!(status, 200, "command failed: {response}");
    let response: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(response["result"]["name"], "sessions.transcript");
    assert_eq!(response["result"]["args"]["session"], "session-1");
    assert_eq!(response["result"]["project_path"], "/projects/alpha");

    let calls = caller.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0],
        RecordedCall {
            device_id: paired.device_id,
            name: "sessions.transcript".into(),
            args: json!({ "session": "session-1", "limit": 20 }),
            project_path: Some("/projects/alpha".into()),
        }
    );
    drop(calls);

    let rejected = json!({ "name": "test.reject", "args": null }).to_string();
    let (status, response) = http(addr, Some(&paired.bearer), &rejected).await;
    assert_eq!(status, 400);
    assert!(response.contains("host rejected the command"));
    handle.abort();
}

#[tokio::test]
async fn browser_command_route_fails_closed_without_a_host_adapter() {
    let auth = Arc::new(AuthState::load(None));
    let (addr, handle) = server(auth, None).await;
    let (status, response) = http(addr, None, r#"{"name":"sessions.list"}"#).await;
    assert_eq!(status, 404);
    assert!(response.contains("commands are unavailable"));
    handle.abort();
}

#[tokio::test]
async fn another_c2_client_origin_may_preflight_and_call_with_a_bearer_header() {
    let auth = Arc::new(AuthState::load(None));
    let (addr, handle) = server(auth, Some(Arc::new(RecordingCaller::default()))).await;
    let client = reqwest::Client::new();

    let preflight = client
        .request(
            reqwest::Method::OPTIONS,
            format!("http://{addr}/api/web-ui/call"),
        )
        .header("Origin", "views://mainview")
        .header("Access-Control-Request-Method", "POST")
        .header(
            "Access-Control-Request-Headers",
            "authorization,content-type",
        )
        .send()
        .await
        .unwrap();
    assert!(preflight.status().is_success());
    let headers = preflight.headers();
    assert_eq!(headers["access-control-allow-origin"], "*");
    let allowed = headers["access-control-allow-headers"].to_str().unwrap();
    assert!(allowed.contains("authorization") && allowed.contains("content-type"));
    // Credentials stay header-only: the server must never opt in to ambient cookies.
    assert!(headers.get("access-control-allow-credentials").is_none());

    let denied = client
        .post(format!("http://{addr}/api/web-ui/call"))
        .header("Origin", "views://mainview")
        .json(&json!({ "name": "sessions.list" }))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 401);
    handle.abort();
}

#[tokio::test]
async fn a_paired_owner_can_review_and_revoke_devices() {
    let auth = Arc::new(AuthState::load(None));
    let first = auth
        .pair(&auth.issue_pairing_token(Duration::from_secs(60)), "Laptop")
        .unwrap();
    let second = auth
        .pair(&auth.issue_pairing_token(Duration::from_secs(60)), "Phone")
        .unwrap();
    let member = auth
        .pair(
            &auth.issue_member_pairing_token("member-1", Duration::from_secs(60)),
            "Team member",
        )
        .unwrap();
    let (addr, handle) = server(auth.clone(), None).await;
    let client = reqwest::Client::new();
    let url = |path: &str| format!("http://{addr}{path}");

    // No credential, and team members, cannot even list.
    assert_eq!(
        client
            .get(url("/api/devices"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .get(url("/api/devices"))
            .bearer_auth(&member.bearer)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );

    let listed: Value = client
        .get(url("/api/devices"))
        .bearer_auth(&first.bearer)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let entries = listed.as_array().unwrap();
    assert_eq!(entries.len(), 3);
    let current: Vec<_> = entries.iter().filter(|e| e["current"] == true).collect();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0]["id"], first.device_id);
    assert!(entries.iter().all(|e| e.get("token_hash").is_none()));

    // Revoking another device cuts it off immediately and says it was not the caller.
    let revoked: Value = client
        .post(url(&format!("/api/devices/{}/revoke", second.device_id)))
        .bearer_auth(&first.bearer)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(revoked, json!({ "revoked": true, "was_current": false }));
    assert_eq!(
        client
            .get(url("/api/devices"))
            .bearer_auth(&second.bearer)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert!(auth.authorize_bearer(&second.bearer).is_none());
    assert_eq!(
        client
            .post(url(&format!("/api/devices/{}/revoke", second.device_id)))
            .bearer_auth(&first.bearer)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );

    // A revoked device cannot revoke anyone.
    assert_eq!(
        client
            .post(url(&format!("/api/devices/{}/revoke", first.device_id)))
            .bearer_auth(&second.bearer)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );

    // Signing itself out is allowed and reported.
    let own: Value = client
        .post(url(&format!("/api/devices/{}/revoke", first.device_id)))
        .bearer_auth(&first.bearer)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(own, json!({ "revoked": true, "was_current": true }));
    assert!(auth.authorize_bearer(&first.bearer).is_none());
    handle.abort();
}

//! A silent provider must not create selectable model ids that were never discovered.

use codetwo_core::event::Event;
use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
use codetwo_core::skill::SkillLibrary;
use codetwo_core::{Engine, Op};

/// A minimal ACP agent: replies to `initialize`, ignores the rest.
const MOCK_AGENT: &str = r#"
import json, sys
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    msg = json.loads(line)
    if msg.get("method") == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": {"protocolVersion": 1}}), flush=True)
"#;

#[tokio::test]
async fn a_silent_provider_does_not_invent_a_model_list() {
    let provider = Provider {
        id: ProviderId::Grok,
        display_name: "Mock".into(),
        launch: LaunchSpec::new("python3", ["-c", MOCK_AGENT]),
        needs_node: false,
    };
    let (engine, mut rx) = Engine::new(vec![provider], SkillLibrary::new(vec![]));

    engine
        .submit(Op::NewSession {
            provider: ProviderId::Grok,
            cwd: std::env::temp_dir().to_string_lossy().to_string(),
            use_worktree: false,
            worktree_base: None,
            worktree_base_sha: None,
            request_id: Some("desktop-request".into()),
            model: Some("grok-code-fast-1".into()),
            initial_policy: None,
        })
        .await
        .unwrap();

    let mut created_request = None;
    let mut listed: Option<(Vec<String>, String)> = None;
    while let Ok(ev) = rx.try_recv() {
        match ev {
            Event::SessionCreated { request_id, .. } => created_request = request_id,
            Event::Models {
                available, current, ..
            } => {
                listed = Some((available.into_iter().map(|m| m.id).collect(), current));
                break;
            }
            Event::Error { message, .. } => panic!("unexpected error: {message}"),
            _ => {}
        }
    }

    assert_eq!(created_request.as_deref(), Some("desktop-request"));
    assert!(
        listed.is_none(),
        "silent provider invented choices: {listed:?}"
    );
    engine.shutdown();
}

/// Probe in a subprocess so environment overrides cannot race other integration tests.
#[test]
fn codex_runtime_override_is_forwarded_to_the_adapter() {
    const PROBE: &str = "CODETWO_RUNTIME_OVERRIDE_PROBE";
    if std::env::var_os(PROBE).is_some() {
        let expected = std::env::var("CODEX_PATH").unwrap();
        let runtime = codetwo_core::codex_runtime::CodexRuntimeDiscovery::detect();
        assert_eq!(runtime.codex_path, Some(std::path::PathBuf::from(&expected)));
        let provider = codetwo_core::provider::registry_with_codex_runtime(&runtime)
            .into_iter()
            .find(|provider| provider.id == ProviderId::Codex)
            .unwrap();
        assert!(provider.launch.env.contains(&("CODEX_PATH".into(), expected)));
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "codex_runtime_override_is_forwarded_to_the_adapter"])
        .env(PROBE, "1")
        .env("CODEX_PATH", dir.path().join("explicit-runtime"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

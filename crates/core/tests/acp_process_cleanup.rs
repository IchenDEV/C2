//! Adapter wrappers can spawn children that retain ACP stdio after the wrapper is killed.
#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;

use codetwo_core::acp::{spawn, RecordingHandler};
use codetwo_core::provider::LaunchSpec;
use serde_json::json;

const WRAPPER: &str = r#"
import json, os, subprocess, sys
child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])
for line in sys.stdin:
    message = json.loads(line)
    if message.get('method') == 'initialize':
        print(json.dumps({'jsonrpc':'2.0', 'id':message['id'], 'result':{
            'protocolVersion':1, 'agentInfo':{'name':str(child.pid), 'version':'test'}}}), flush=True)
        if os.environ.get('EXIT_WRAPPER'): break
"#;

fn is_running(pid: u32) -> bool {
    let output = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let state = String::from_utf8_lossy(&output.stdout);
    !state.trim().is_empty() && !state.trim().starts_with('Z')
}

#[tokio::test]
async fn terminate_and_drop_close_descendants_and_pending_rpc() {
    // An unrelated process must survive cleanup of each separately owned ACP process group.
    let mut unrelated = tokio::process::Command::new("python3")
        .args(["-c", "import time; time.sleep(60)"])
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    for (explicit, wrapper_exits) in [(true, false), (false, false), (true, true)] {
        let mut launch = LaunchSpec::new("python3", ["-c", WRAPPER]);
        if wrapper_exits {
            launch.env.push(("EXIT_WRAPPER".into(), "1".into()));
        }
        let client = spawn(&launch, Arc::new(RecordingHandler::default()))
            .await
            .unwrap();
        let info = client
            .initialize(json!({}))
            .await
            .unwrap()
            .agent_info
            .unwrap();
        let descendant = info.name.parse::<u32>().unwrap();
        assert!(is_running(descendant));
        let connection = client.connection().clone();
        if explicit {
            client.terminate();
            client.terminate(); // Idempotent while the handle still exists.
        }
        drop(client);
        let closed = tokio::time::timeout(
            Duration::from_secs(2),
            connection.request::<_, serde_json::Value>("test/pending", json!({})),
        )
        .await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while is_running(descendant) && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let leaked = is_running(descendant);
        if leaked {
            // Dispose the exact fixture child even when this regression fails on the old code.
            let _ = std::process::Command::new("kill")
                .args(["-KILL", &descendant.to_string()])
                .status();
        }
        assert!(!leaked, "the adapter descendant survived teardown");
        assert!(matches!(closed, Ok(Err(_))), "pending RPC did not close");
        assert!(is_running(unrelated.id().unwrap()));
    }
    unrelated.kill().await.unwrap();
}

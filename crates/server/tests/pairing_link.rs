//! Browser `remote.pairing_link`: the ordinary adapter mints one-time links from the server's
//! existing `AuthState`, only for the origin the browser reached, and each link pairs once.

use std::sync::Arc;

use codetwo_core::plugins::{AppConfig, CoreApp, PluginManager};
use codetwo_server::{AuthState, KernelWebUiCommands, WebUiCommandCaller};
use serde_json::json;

#[tokio::test]
async fn browser_pairing_link_is_one_time_and_origin_bound() {
    let data = tempfile::tempdir().unwrap();
    let app = CoreApp::boot(AppConfig::new(data.path())).await.expect("boot");
    let manager = app.service::<PluginManager>().expect("plugin manager");
    let auth = Arc::new(AuthState::load(None));
    let kernel = KernelWebUiCommands::new(manager.clone()).with_pairing(auth.clone());

    let link = kernel
        .call(
            "device-a",
            "remote.pairing_link",
            json!({ "base_url": "http://192.0.2.7:4100" }),
            None,
        )
        .await
        .expect("link");
    let url = link["url"].as_str().unwrap();
    let token = link["token"].as_str().unwrap();
    assert_eq!(url, format!("http://192.0.2.7:4100/pair#token={token}"));
    assert!(link["expires_in"].as_u64().unwrap() > 0);

    assert!(auth.pair(token, "receiver-c").is_some(), "fresh link pairs once");
    assert!(auth.pair(token, "receiver-d").is_none(), "consumed link cannot pair again");

    for bad in ["", "javascript:alert(1)", "http://h/path", "http://h:1/?x=1", "ftp://h"] {
        let denied = kernel
            .call("device-a", "remote.pairing_link", json!({ "base_url": bad }), None)
            .await;
        assert!(denied.is_err(), "{bad:?} must be rejected");
    }

    // Hosts that did not wire an auth instance never mint links.
    let plain = KernelWebUiCommands::new(manager);
    assert!(plain
        .call("device-a", "remote.pairing_link", json!({ "base_url": "http://h:1" }), None)
        .await
        .is_err());
}

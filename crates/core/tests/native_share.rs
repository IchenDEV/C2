//! Actual native local entrypoint (`CoreApp::call_local`, the path NAPI and the desktop host use)
//! against a real booted Core in a disposable data directory.

use codetwo_core::plugins::{AppConfig, CoreApp, EngineService};
use codetwo_core::provider::ProviderId;
use codetwo_core::session::Session;
use codetwo_core::share::{shared_store, Visibility, LOCAL_OWNER_ID};
use codetwo_core::Event;
use serde_json::json;

fn ids(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["id"].as_str().map(str::to_string))
        .collect()
}

fn ids_of(value: &serde_json::Value, key: &str) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item[key].as_str().map(str::to_string))
        .collect()
}

#[tokio::test]
async fn native_local_path_applies_creator_only_policy_with_one_shared_context() {
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let cwd = project
        .path()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let app = CoreApp::boot(AppConfig::new(data.path()))
        .await
        .expect("boot");
    let engine = app.service::<EngineService>().unwrap().0.clone();
    let store = engine.store().expect("durable store");
    store
        .add_project(&cwd, None, codetwo_core::session::now_millis())
        .unwrap();

    // One context: the native adapter, the engine and the process registry share one Arc.
    let share = app.share_store().expect("share context");
    assert!(std::sync::Arc::ptr_eq(
        &share,
        &engine.share_store().unwrap()
    ));
    assert!(std::sync::Arc::ptr_eq(
        &share,
        &shared_store(data.path().join("session-shares.json")).unwrap()
    ));

    // Seed: a legacy local session (no record), a guest-private and a guest-public session.
    let mut ids_made = Vec::new();
    for _ in 0..3 {
        let session = Session::new(ProviderId::ClaudeCode, cwd.clone());
        store.upsert_session(&session).unwrap();
        ids_made.push(session.id.to_string());
    }
    let (legacy, private, public) = (&ids_made[0], &ids_made[1], &ids_made[2]);
    let guest = share.participant("guest");
    share
        .register_session(private, &guest, Visibility::Private)
        .unwrap();
    share
        .register_session(public, &guest, Visibility::Public)
        .unwrap();

    // Trusted stable local actor on the native path.
    let me = app.call_local("share.whoami", json!({})).await.unwrap();
    assert_eq!(me["id"], LOCAL_OWNER_ID);

    // Native list: host owner sees legacy + public, never the guest's private session.
    let listed = ids(&app.call_local("sessions.list", json!({})).await.unwrap());
    assert!(
        listed.contains(legacy) && listed.contains(public),
        "{listed:?}"
    );
    assert!(
        !listed.contains(private),
        "private leaked to host owner: {listed:?}"
    );

    // Direct read / control of the private task is denied; public is nonexclusive.
    for command in [
        "sessions.transcript",
        "engine.cancel",
        "engine.prepare_session",
    ] {
        let denied = app.call_local(command, json!({"session": private})).await;
        assert!(denied
            .unwrap_err()
            .to_string()
            .contains("session not found"));
    }
    assert!(app
        .call_local("share.get", json!({"session": private}))
        .await
        .is_err());
    assert!(app
        .call_local(
            "share.set_visibility",
            json!({"session": private, "visibility": "public"})
        )
        .await
        .is_err());
    app.call_local("share.follow", json!({"session": public}))
        .await
        .unwrap();
    // Legacy stays original local-only: usable locally, never shared.
    assert!(app
        .call_local("share.get", json!({"session": legacy}))
        .await
        .is_err());

    // Remote adapter fixture over the SAME real Core and SAME context: guest sees own private +
    // public, never the legacy local task.
    let guest_view = codetwo_core::share_policy::shared_web_ui_call(
        &share,
        "guest",
        "sessions.list",
        json!({}),
        |args| async {
            app.call("sessions.list", args)
                .await
                .map_err(|e| e.to_string())
        },
    )
    .await
    .unwrap();
    let guest_ids = ids(&guest_view);
    assert!(guest_ids.contains(private) && guest_ids.contains(public));
    assert!(
        !guest_ids.contains(legacy),
        "legacy local task leaked to a guest"
    );

    // Streamed events: same policy at the native event exit.
    let created = |session: &str| Event::SessionCreated {
        session: session.into(),
        cwd: cwd.clone(),
        project_path: None,
        worktree_path: None,
        worktree_baseline: None,
        request_id: None,
    };
    assert!(!app.local_event_visible(&created(private)));
    assert!(app.local_event_visible(&created(public)));
    assert!(app.local_event_visible(&created(legacy)));

    // Native create registers a private-at-creation intent BEFORE dispatch and a default-public
    // one otherwise; a failed dispatch leaves no pending intent behind.
    let request_id = engine
        .prepare_local_create(Some("native-private".into()), Visibility::Private)
        .unwrap();
    assert!(share.has_pending(&request_id));
    share.claim_created(&request_id, "native-session").unwrap();
    let record = share
        .read("native-session", &share.participant(""))
        .unwrap();
    assert_eq!(record.visibility, Visibility::Private);
    assert_eq!(record.creator, LOCAL_OWNER_ID);
    let defaulted = engine
        .prepare_local_create(None, Visibility::default())
        .unwrap();
    share.claim_created(&defaulted, "native-default").unwrap();
    assert_eq!(
        share
            .read("native-default", &share.participant(""))
            .unwrap()
            .visibility,
        Visibility::Public
    );

    // R7: alternate task-bearing native exits (the native adapter has no browser allowlist, so
    // the one policy must gate them): a guest-private task is denied to the host owner.
    for name in ["engine.prompt_delivery", "sessions.diff_stat", "engine.switch_provider"] {
        let denied = app
            .call_local(name, json!({"session": private, "request_id": "r", "provider": "claude_code"}))
            .await
            .expect_err(name);
        assert!(denied.to_string().contains("session not found"), "{name}: {denied}");
    }
    let search = app
        .call_local("sessions.search", json!({"query": "anything"}))
        .await
        .unwrap();
    assert!(ids_of(&search, "session_id").iter().all(|id| id != private));
    let snapshot = app.call_local("assistant.snapshot", json!({})).await.unwrap();
    assert!(ids(&snapshot["sessions"]).iter().all(|id| id != private));

    // Persistence/reopen at the same data directory.
    drop(app);
    let reopened =
        codetwo_core::share::ShareStore::open(data.path().join("session-shares.json")).unwrap();
    assert!(reopened.read(private, &reopened.participant("")).is_none());
    assert_eq!(
        reopened
            .read("native-session", &reopened.participant(""))
            .unwrap()
            .visibility,
        Visibility::Private
    );
}

//! Ordinary `/api/web-ui/call` share policy: exercised through `shared_web_ui_call` with a fake
//! Kernel dispatcher so the same gate used in production is covered without a live Core.

use codetwo_core::share::ShareStore;
use codetwo_server::shared_web_ui_call;
use serde_json::{json, Value};

async fn call(store: &ShareStore, device: &str, name: &str, args: Value) -> Result<Value, String> {
    shared_web_ui_call(store, device, name, args.clone(), |args| async move {
        Ok(match name {
            "engine.new_session" | "engine.new_parallel_task" => {
                json!({ "id": args["new_id"].as_str().unwrap_or("s-new") })
            }
            "sessions.list" | "sessions.archived" => json!([
                {"id": "pub", "title": "public"},
                {"id": "priv", "title": "secret"},
                {"id": "legacy", "title": "legacy"}
            ]),
            "sessions.previews" => json!({"pub": "a", "priv": "secret", "legacy": "l"}),
            _ => json!(true),
        })
    })
    .await
}

async fn seeded() -> ShareStore {
    let store = ShareStore::in_memory();
    call(
        &store,
        "alice",
        "engine.new_session",
        json!({"new_id": "pub"}),
    )
    .await
    .unwrap();
    call(
        &store,
        "alice",
        "engine.new_session",
        json!({"new_id": "priv", "visibility": "private"}),
    )
    .await
    .unwrap();
    store
}

#[tokio::test]
async fn new_sessions_default_public_and_every_participant_controls_them() {
    let store = seeded().await;
    let record = call(&store, "bob", "share.get", json!({"session": "pub"}))
        .await
        .unwrap();
    assert_eq!(record["visibility"], "public");
    assert_eq!(record["creator"], "device:alice");
    call(&store, "carol", "share.follow", json!({"session": "pub"}))
        .await
        .unwrap();
    call(&store, "alice", "share.follow", json!({"session": "pub"}))
        .await
        .unwrap();
    for name in [
        "engine.prompt",
        "engine.queue",
        "engine.steer",
        "engine.cancel",
    ] {
        call(&store, "bob", name, json!({"session": "pub"}))
            .await
            .unwrap();
    }
    let record = call(&store, "bob", "share.get", json!({"session": "pub"}))
        .await
        .unwrap();
    assert_eq!(record["followers"].as_array().unwrap().len(), 2);
    let journal = record["journal"].as_array().unwrap();
    assert_eq!(journal.len(), 4);
    assert!(journal.iter().all(|op| op["actor"] == "device:bob"));
}

#[tokio::test]
async fn private_session_is_denied_to_guest_and_host_on_every_command_exit() {
    let store = seeded().await;
    for viewer in ["bob", ""] {
        for name in [
            "engine.prompt",
            "sessions.transcript",
            "engine.cancel",
            "share.get",
            "share.follow",
        ] {
            let err = call(&store, viewer, name, json!({"session": "priv"}))
                .await
                .unwrap_err();
            assert_eq!(err, "session not found", "{viewer:?} {name}");
        }
        // Unknown ids are indistinguishable from private ones for paired devices; the host's
        // original local UI keeps legacy/unowned sessions.
        let unknown = call(&store, viewer, "engine.prompt", json!({"session": "nope"})).await;
        assert_eq!(unknown.is_err(), !viewer.is_empty());
        // Missing target fails closed.
        assert!(call(&store, viewer, "engine.prompt", json!({}))
            .await
            .is_err());
        for list in ["sessions.list", "sessions.archived"] {
            let listed = call(&store, viewer, list, json!({})).await.unwrap();
            let ids: Vec<_> = listed
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["id"].as_str().unwrap())
                .collect();
            let expected: &[&str] = if viewer.is_empty() {
                &["pub", "legacy"]
            } else {
                &["pub"]
            };
            assert_eq!(
                ids, expected,
                "private never listed; legacy local-only: {viewer:?}"
            );
        }
        let previews = call(&store, viewer, "sessions.previews", json!({}))
            .await
            .unwrap();
        let mut keys = previews
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        keys.sort();
        let expected: &[&str] = if viewer.is_empty() {
            &["legacy", "pub"]
        } else {
            &["pub"]
        };
        assert_eq!(keys, expected);
    }
    let own = call(&store, "alice", "sessions.list", json!({}))
        .await
        .unwrap();
    assert_eq!(own.as_array().unwrap().len(), 2);
    assert!(
        call(&store, "alice", "engine.prompt", json!({"session": "priv"}))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn legacy_unowned_sessions_stay_local_and_actor_spoofing_is_ignored() {
    let store = seeded().await;
    // Legacy: no record. Only the original local UI reaches it; never a paired device.
    assert!(
        call(&store, "", "engine.prompt", json!({"session": "legacy"}))
            .await
            .is_ok()
    );
    assert!(
        call(&store, "bob", "engine.prompt", json!({"session": "legacy"}))
            .await
            .is_err()
    );
    // Private is creator-only, even for the host's local UI.
    assert!(
        call(&store, "", "engine.prompt", json!({"session": "priv"}))
            .await
            .is_err()
    );
    // A client-supplied actor/creator field is never trusted.
    call(
        &store,
        "mallory",
        "engine.new_session",
        json!({"new_id": "spoof", "visibility": "private", "actor": "device:alice", "creator": "device:alice"}),
    )
    .await
    .unwrap();
    assert!(
        call(&store, "alice", "share.get", json!({"session": "spoof"}))
            .await
            .is_err()
    );
    call(
        &store,
        "mallory",
        "engine.prompt",
        json!({"session": "pub", "actor": "device:alice"}),
    )
    .await
    .unwrap();
    let record = call(&store, "alice", "share.get", json!({"session": "pub"}))
        .await
        .unwrap();
    assert_eq!(record["journal"][0]["actor"], "device:mallory");
    // Only the creator changes visibility, and that grants nobody exclusive control.
    assert!(call(
        &store,
        "bob",
        "share.set_visibility",
        json!({"session": "pub", "visibility": "private"})
    )
    .await
    .is_err());
    call(
        &store,
        "alice",
        "share.set_visibility",
        json!({"session": "pub", "visibility": "private"}),
    )
    .await
    .unwrap();
    assert!(
        call(&store, "bob", "engine.prompt", json!({"session": "pub"}))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn nickname_is_label_only() {
    let store = ShareStore::in_memory();
    let me = call(
        &store,
        "bob",
        "share.set_nickname",
        json!({"nickname": "  Bobby  "}),
    )
    .await
    .unwrap();
    assert_eq!(me["nickname"], "Bobby");
    assert_eq!(me["id"], "device:bob");
    let other = call(
        &store,
        "carol",
        "share.whoami",
        json!({"nickname": "Bobby"}),
    )
    .await
    .unwrap();
    assert_ne!(other["id"], "device:bob");
}

fn created(session: &str, request_id: Option<&str>) -> codetwo_core::Event {
    codetwo_core::Event::SessionCreated {
        session: session.into(),
        cwd: "/tmp".into(),
        project_path: None,
        worktree_path: None,
        worktree_baseline: None,
        request_id: request_id.map(str::to_string),
    }
}

#[test]
fn event_policy_claims_creation_before_visibility_and_fails_closed() {
    use codetwo_core::share::Visibility;
    use codetwo_server::share_event_visible as visible;
    let store = ShareStore::in_memory();
    let alice = store.participant("alice");
    store.expect_create("req-pub", &alice, Visibility::Public);
    store.expect_create("req-priv", &alice, Visibility::Private);
    // Event arrives before any command-side registration: metadata is claimed first.
    assert!(visible(&store, "bob", &created("s-pub", Some("req-pub"))));
    assert!(!visible(
        &store,
        "bob",
        &created("s-priv", Some("req-priv"))
    ));
    assert!(visible(
        &store,
        "alice",
        &created("s-priv", Some("req-priv"))
    ));
    assert!(
        !visible(&store, "", &created("s-priv", Some("req-priv"))),
        "host denied private"
    );
    // Legacy create (no pending intent): local UI only.
    assert!(!visible(&store, "bob", &created("s-old", None)));
    assert!(visible(&store, "", &created("s-old", None)));
    // Session events follow the same record; session-less global events never reach guests.
    let title = codetwo_core::Event::SessionTitleChanged {
        session: "s-priv".into(),
        title: "secret".into(),
    };
    assert!(visible(&store, "alice", &title));
    assert!(!visible(&store, "bob", &title));
    assert!(!visible(&store, "", &title));
    let alert = codetwo_core::Event::AssistantAlert {
        session: None,
        id: "a".into(),
        goal_id: "g".into(),
        title: "t".into(),
        body: "b".into(),
    };
    assert!(!visible(&store, "bob", &alert));
    assert!(visible(&store, "", &alert));
    // A create error is delivered only to the actor whose pending create it answers.
    store.expect_create("req-fail", &alice, Visibility::Public);
    let failure = codetwo_core::Event::Error {
        session: None,
        message: "boom".into(),
        terminal: true,
        request_id: Some("req-fail".into()),
    };
    assert!(visible(&store, "alice", &failure));
    assert!(!visible(&store, "bob", &failure));
}

#[tokio::test]
async fn request_ids_are_actor_scoped_and_failed_dispatch_is_not_attributed() {
    let store = seeded().await;
    let args = json!({"session": "pub", "request_id": "r1"});
    call(&store, "bob", "engine.prompt", args.clone())
        .await
        .unwrap();
    // Same participant retry: deduplicated attribution, not a second journal row.
    call(&store, "bob", "engine.prompt", args.clone())
        .await
        .unwrap();
    // Another participant reusing the same client id is an independent operation in its own
    // actor-scoped namespace: it succeeds and is never replayed as bob's command.
    call(&store, "carol", "engine.prompt", args.clone())
        .await
        .unwrap();
    // A failed dispatch (adapter error) records no author.
    let failed = shared_web_ui_call(
        &store,
        "bob",
        "engine.prompt",
        json!({"session": "pub", "request_id": "r2"}),
        |_| async { Err::<Value, String>("provider down".into()) },
    )
    .await;
    assert!(failed.is_err());
    let record = call(&store, "alice", "share.get", json!({"session": "pub"}))
        .await
        .unwrap();
    let journal = record["journal"].as_array().unwrap();
    assert_eq!(journal.len(), 2);
    assert_eq!(journal[0]["actor"], "device:bob");
    assert_eq!(journal[0]["request_id"], "device:bob/r1");
    assert_eq!(journal[1]["actor"], "device:carol");
    assert_eq!(journal[1]["request_id"], "device:carol/r1");
}

// ---- R7 privacy audit: alternate task-bearing exits (old-red: these passed through unfiltered).

async fn call_with(
    store: &ShareStore,
    device: &str,
    name: &str,
    args: Value,
    reply: Value,
) -> Result<(Value, Value), String> {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Value::Null));
    let sink = seen.clone();
    let out = shared_web_ui_call(store, device, name, args, move |args| async move {
        *sink.lock().unwrap() = args;
        Ok(reply)
    })
    .await?;
    let args = seen.lock().unwrap().clone();
    Ok((out, args))
}

#[tokio::test]
async fn search_snapshot_and_alternate_session_commands_follow_the_one_policy() {
    let store = seeded().await;
    let hits = json!([
        {"session_id": "pub", "snippet": "a"},
        {"session_id": "priv", "snippet": "SECRET"},
        {"session_id": "legacy", "snippet": "old"}
    ]);
    for (device, expect) in [("bob", vec!["pub"]), ("alice", vec!["pub", "priv"]), ("", vec!["pub", "legacy"])] {
        let (out, _) = call_with(&store, device, "sessions.search", json!({"query": "x"}), hits.clone())
            .await
            .unwrap();
        let ids: Vec<&str> = out.as_array().unwrap().iter().map(|h| h["session_id"].as_str().unwrap()).collect();
        assert_eq!(ids, expect, "search for {device:?}");
    }
    let snapshot = json!({
        "state": {"goal": "g"},
        "sessions": [{"id": "pub"}, {"id": "priv"}, {"id": "legacy"}],
        "memories": []
    });
    let (guest, _) = call_with(&store, "bob", "assistant.snapshot", json!({}), snapshot.clone()).await.unwrap();
    // Visible rows carry authoritative share metadata (creator nickname + visibility); private and
    // unknown rows are absent entirely, so the privacy assertion is the id set, not the shape.
    let ids = |v: &Value| -> Vec<String> {
        v["sessions"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap().to_string()).collect()
    };
    assert_eq!(ids(&guest), ["pub"]);
    assert_eq!(guest["sessions"][0]["share"]["visibility"], "public");
    assert_eq!(guest["sessions"][0]["share"]["creator_name"], "Guest-lice");
    assert_eq!(guest["state"], json!({"goal": "g"}));
    let (host, _) = call_with(&store, "", "assistant.snapshot", json!({}), snapshot).await.unwrap();
    assert_eq!(ids(&host), ["pub", "legacy"], "host owner is not exempt from private");
    assert_eq!(host["sessions"][1]["share"], json!({"legacy": true}), "legacy unowned is marked local-only");
    assert!(!host.to_string().contains("priv"), "no private id leaks into the snapshot");
    // Alternate session-bearing commands: private is creator-only for guest and host alike,
    // and an unlisted command that names a session cannot bypass the viewer check.
    for name in ["engine.prompt_delivery", "engine.switch_provider", "sessions.diff_stat", "worktrees.discard_session", "future.unlisted_command"] {
        let args = json!({"session": "priv", "request_id": "r"});
        for device in ["bob", ""] {
            let err = call_with(&store, device, name, args.clone(), json!(true)).await.unwrap_err();
            assert_eq!(err, "session not found", "{name} for {device:?}");
        }
        call_with(&store, "alice", name, args.clone(), json!(true)).await.unwrap();
        // Unknown and legacy-foreign are indistinguishable from private for a guest.
        let unknown = call_with(&store, "bob", name, json!({"session": "nope", "request_id": "r"}), json!(true)).await.unwrap_err();
        assert_eq!(unknown, "session not found");
    }
}

#[tokio::test]
async fn delivery_lookup_uses_the_callers_actor_namespace() {
    let store = seeded().await;
    let (_, args) = call_with(&store, "bob", "engine.prompt_delivery", json!({"session": "pub", "request_id": "r1"}), json!(true)).await.unwrap();
    assert_eq!(args["request_id"], "device:bob/r1");
    let (_, args) = call_with(&store, "carol", "engine.prompt_delivery", json!({"session": "pub", "request_id": "r1"}), json!(true)).await.unwrap();
    assert_eq!(args["request_id"], "device:carol/r1", "same client id, independent actor");
    // A delivery lookup that omits the id has nothing to scope and is denied.
    assert!(call_with(&store, "bob", "engine.prompt_delivery", json!({"session": "pub"}), json!(true)).await.is_err());
}

#[tokio::test]
async fn host_cannot_spell_another_actors_scoped_request_id() {
    let store = seeded().await;
    for name in ["engine.prompt", "engine.queue", "engine.new_session"] {
        for spoof in ["device:bob/r1", "device:alice/x", "share-create:abc", "local-owner/r"] {
            let err = call_with(&store, "", name, json!({"session": "pub", "request_id": spoof}), json!({"id": "s-x"})).await.unwrap_err();
            assert_eq!(err, "invalid request id", "{name} {spoof}");
        }
    }
    // Host-local ordinary ids and every device actor stay valid; device ids stay namespaced.
    call_with(&store, "", "engine.prompt", json!({"session": "pub", "request_id": "plain"}), json!(true)).await.unwrap();
    // WS creation entry (prepare_create) shares the same policy: a spoof is replaced by a
    // generated id outside every device namespace.
    use codetwo_core::share_policy::client_id_allowed;
    assert!(!client_id_allowed(&store.participant(""), "device:bob/r1"));
    assert!(client_id_allowed(&store.participant("bob"), "device:bob/r1"));
    assert!(client_id_allowed(&store.participant(""), "plain"));
}

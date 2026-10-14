//! R9 boundaries: notice kinds, nickname refresh, and global snapshot task-derived records.

use codetwo_core::share::{ShareStore, Visibility};
use codetwo_core::share_policy::shared_web_ui_call;
use serde_json::json;
use std::sync::{Arc, Mutex};

fn recording_store() -> (Arc<ShareStore>, Arc<Mutex<Vec<(String, &'static str)>>>) {
    let store = Arc::new(ShareStore::in_memory());
    let log: Arc<Mutex<Vec<(String, &'static str)>>> = Arc::default();
    let sink = log.clone();
    store.set_notifier(Arc::new(move |change| {
        sink.lock().unwrap().push((change.session, change.kind));
    }));
    (store, log)
}

#[test]
fn revoked_only_on_public_to_private_never_for_noop_or_private_to_private() {
    let (store, log) = recording_store();
    let alice = store.participant("alice");
    store.register_session("pub", &alice, Visibility::Public).unwrap();
    store.register_session("priv", &alice, Visibility::Private).unwrap();
    assert!(log.lock().unwrap().is_empty(), "creation emits no notice, private or public");

    store.set_visibility("priv", &alice, Visibility::Private).unwrap(); // private -> private
    store.set_visibility("pub", &alice, Visibility::Public).unwrap(); // public -> public
    store.set_visibility("pub", &alice, Visibility::Private).unwrap(); // public -> private
    let kinds: Vec<_> = log.lock().unwrap().iter().cloned().collect();
    assert_eq!(
        kinds,
        vec![
            ("priv".to_string(), "visibility"),
            ("pub".to_string(), "visibility"),
            ("pub".to_string(), "revoked"),
        ]
    );
}

#[test]
fn nickname_change_notifies_only_sessions_the_participant_took_part_in() {
    let (store, log) = recording_store();
    let alice = store.participant("alice");
    let bob = store.participant("bob");
    store.register_session("a-own", &alice, Visibility::Public).unwrap();
    store.register_session("b-own", &bob, Visibility::Private).unwrap();
    store.record_op("b-own", &alice, "prompt", Some("r1")).unwrap();
    store.set_nickname("alice", "Ally").unwrap();
    let mut seen: Vec<_> = log.lock().unwrap().iter().cloned().collect();
    seen.sort();
    assert_eq!(
        seen,
        vec![("a-own".to_string(), "nickname"), ("b-own".to_string(), "nickname")]
    );
    // The notice is metadata-free; recipients are still selected by the ordinary view policy.
    assert!(store.can_view("a-own", &bob));
    assert!(!store.can_view("b-own", &store.participant("carol")));
}

#[tokio::test]
async fn assistant_snapshot_drops_task_derived_records_by_subject() {
    let store = ShareStore::in_memory();
    let alice = store.participant("alice");
    store.register_session("pub", &alice, Visibility::Public).unwrap();
    store.register_session("priv", &alice, Visibility::Private).unwrap();
    // "legacy" has no record: original local-only.
    let snapshot = json!({
        "sessions": [{"id": "pub"}, {"id": "priv"}, {"id": "legacy"}],
        "state": {
            "scoped_runs": [
                {"run": {"id": "r1"}, "session_id": "pub"},
                {"run": {"id": "r2"}, "session_id": "priv"},
                {"run": {"id": "r3"}, "session_id": "legacy"}
            ],
            "attempts": {"a1": {"session_id": "priv"}, "a2": {"session_id": "pub"}},
            "questions": [{"id": "q1", "context": {"used_session_refs": ["pub", "priv"]}}],
            "summary": "goal-level text"
        },
        "memories": [
            {"id": "m1", "session_id": "priv"},
            {"id": "m2", "session_id": "pub"},
            {"id": "m3", "session_id": null, "sources": [{"session_id": "priv"}]},
            {"id": "m4", "session_id": null}
        ]
    });
    let call = |device: &'static str| {
        let snapshot = snapshot.clone();
        let store = &store;
        async move {
            shared_web_ui_call(store, device, "assistant.snapshot", json!({}), |_| async move {
                Ok(snapshot)
            })
            .await
            .unwrap()
        }
    };

    let guest = call("bob").await;
    let ids = |v: &serde_json::Value, key: &str| -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|i| i[key].as_str().unwrap_or_default().to_string())
            .collect()
    };
    assert_eq!(ids(&guest["sessions"], "id"), vec!["pub"]);
    assert_eq!(ids(&guest["state"]["scoped_runs"], "session_id"), vec!["pub"]);
    assert!(guest["state"]["attempts"].get("a1").is_none());
    assert!(guest["state"]["attempts"].get("a2").is_some());
    assert!(guest["state"]["questions"].as_array().unwrap().is_empty());
    assert_eq!(guest["state"]["summary"], "goal-level text");
    assert_eq!(ids(&guest["memories"], "id"), vec!["m2", "m4"]);

    // The host is the trusted local actor, not an exemption: it sees public + legacy only.
    let host = call("").await;
    assert_eq!(ids(&host["sessions"], "id"), vec!["pub", "legacy"]);
    assert_eq!(ids(&host["state"]["scoped_runs"], "session_id"), vec!["pub", "legacy"]);
    assert_eq!(ids(&host["memories"], "id"), vec!["m2", "m4"]);

    // The creator keeps their private records.
    let creator = call("alice").await;
    assert_eq!(ids(&creator["memories"], "id"), vec!["m1", "m2", "m3", "m4"]);
    assert_eq!(creator["state"]["questions"].as_array().unwrap().len(), 1);
}

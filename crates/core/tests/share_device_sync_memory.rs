//! Device-sync memory rows follow the common ShareStore `can_view`: session-scoped memories of a
//! private or legacy (unowned) session never reach, or get written by, another actor's device;
//! session-less (public) memories stay available and never name a hidden memory.

use codetwo_core::device_sync::DeviceSyncDocument;
use codetwo_core::share::{ShareStore, Visibility};
use codetwo_core::share_policy::{check_device_sync_write, redact_device_sync_document};
use serde_json::{json, Value};

fn memory(id: &str, session: Option<&str>, supersedes: Option<&str>) -> Value {
    json!({
        "id": id, "project_path": "/p", "session_id": session, "layer": "project",
        "category": "fact", "content": format!("content-{id}"), "keywords_json": "[]",
        "confidence": 1.0, "pinned": false, "active": true, "created_at": 1, "updated_at": 1,
        "origin": "test", "forgotten_at": null, "supersedes_id": supersedes,
        "conflict_with_id": supersedes, "conflict_reason": supersedes.map(|s| format!("clashes with {s}")),
    })
}

fn document(memories: Vec<Value>) -> DeviceSyncDocument {
    serde_json::from_value(json!({
        "schema_version": 1, "revision": 1, "generated_at": 1, "writer_device_id": "owner-device",
        "memories": memories,
    }))
    .expect("document")
}

#[test]
fn private_and_legacy_session_memories_are_hidden_but_public_memory_remains() {
    let share = ShareStore::in_memory();
    let owner = share.participant("owner-device");
    share.register_session("pub-s", &owner, Visibility::Public).unwrap();
    share.register_session("priv-s", &owner, Visibility::Private).unwrap();
    // "legacy-s" is deliberately never registered (unowned historical session).

    let build = || {
        document(vec![
            memory("m-global", None, None),
            memory("m-pub", Some("pub-s"), None),
            memory("m-priv", Some("priv-s"), None),
            memory("m-legacy", Some("legacy-s"), None),
            // Session-less memory that supersedes / conflicts with a hidden one.
            memory("m-ref", None, Some("m-priv")),
        ])
    };

    let mut for_peer = build();
    redact_device_sync_document(&share, "peer-device", &mut for_peer);
    let ids: Vec<&str> = for_peer.memories.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["m-global", "m-pub", "m-ref"]);
    let serialized = serde_json::to_string(&for_peer).unwrap();
    assert!(!serialized.contains("m-priv") && !serialized.contains("content-m-priv"));
    assert!(!serialized.contains("m-legacy") && !serialized.contains("priv-s"));

    // The creator still sees its own private memory (common actor can_view).
    let mut for_owner = build();
    redact_device_sync_document(&share, "owner-device", &mut for_owner);
    assert!(for_owner.memories.iter().any(|m| m.id == "m-priv"));

    // A peer cannot write memories for private/legacy sessions; public + session-less writes pass.
    for hidden in ["priv-s", "legacy-s"] {
        let write = document(vec![memory("w", Some(hidden), None)]);
        assert!(check_device_sync_write(&share, "peer-device", &write).is_err(), "{hidden}");
    }
    let write = document(vec![memory("w1", Some("pub-s"), None), memory("w2", None, None)]);
    assert!(check_device_sync_write(&share, "peer-device", &write).is_ok());
}

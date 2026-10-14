//! Durable message authorship + actor-scoped request ids through the real Engine, the real
//! participant policy (`shared_web_ui_call`) and the real event policy.
//!
//! FIXTURE: the agent is a disposable python ACP stub (zero fee). No paid Agent is involved.

use std::sync::Arc;
use std::time::Duration;

use codetwo_core::event::Event;
use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
use codetwo_core::skill::DocBlock;
use codetwo_core::share::{ShareStore, Visibility};
use codetwo_core::share_policy::{share_event_visible, shared_web_ui_call};
use codetwo_core::skill::SkillLibrary;
use codetwo_core::store::Store;
use codetwo_core::{Engine, Op};
use serde_json::{json, Value};

const SCRIPT: &str = r#"
import json,sys
for line in sys.stdin:
    m=json.loads(line); method=m.get('method'); mid=m.get('id'); result={}
    if method=='initialize': result={'protocolVersion':1}
    elif method=='session/new': result={'sessionId':'fx'}
    elif method=='session/prompt': result={'stopReason':'end_turn'}
    elif mid is None: continue
    print(json.dumps({'jsonrpc':'2.0','id':mid,'result':result}),flush=True)
"#;

async fn prompt_as(
    share: &ShareStore,
    engine: &Engine,
    device: &str,
    session: &str,
    text: &str,
    args_extra: Value,
    fail: bool,
) -> Result<Value, String> {
    let mut args = json!({"session": session, "request_id": "same-client-id"});
    args.as_object_mut()
        .unwrap()
        .extend(args_extra.as_object().unwrap().clone());
    let engine = engine.clone();
    let (session, text) = (session.to_string(), text.to_string());
    shared_web_ui_call(share, device, "engine.prompt", args, move |args| async move {
        if fail {
            return Err("fixture dispatch failure".to_string());
        }
        engine
            .submit(Op::Prompt {
                session,
                doc: vec![DocBlock::Text { text }],
                request_id: args["request_id"].as_str().map(str::to_string),
            })
            .await
            .map_err(|e| e.to_string())?;
        Ok(json!(true))
    })
    .await
}

async fn wait_idle(engine: &Engine, session: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while engine.current_turn(session).is_some() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn two_actors_same_client_id_get_durable_authors_on_real_transcript_rows() {
    let root = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let share_path = dir.path().join("session-shares.json");
    let share = Arc::new(ShareStore::open(&share_path).unwrap());
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (engine, mut events) = Engine::with_store(
        vec![Provider {
            id: ProviderId::Grok,
            display_name: "Authorship fixture (zero-fee, not a paid Agent)".into(),
            launch: LaunchSpec::new("python3", ["-c", SCRIPT]),
            needs_node: false,
        }],
        SkillLibrary::new(vec![]),
        store.clone(),
    );
    engine.attach_share_store(share.clone());
    let cwd: String = root.path().to_string_lossy().into();
    let (alice, bob) = (share.participant("alice"), share.participant("bob"));
    share.set_nickname("alice", "Alice").unwrap();
    share.set_nickname("bob", "Bob").unwrap();

    // Creation through the real policy + engine: the record exists BEFORE SessionCreated is seen.
    let create_engine = engine.clone();
    shared_web_ui_call(
        &share,
        "alice",
        "engine.new_session",
        json!({"request_id": "create-1", "visibility": "public"}),
        move |args| async move {
            create_engine
                .submit(Op::NewSession {
                    provider: ProviderId::Grok,
                    cwd,
                    use_worktree: false,
                    worktree_base: None,
                    worktree_base_sha: None,
                    request_id: args["request_id"].as_str().map(str::to_string),
                    model: None,
                    initial_policy: None,
                })
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!(null))
        },
    )
    .await
    .unwrap();
    let session = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(Event::SessionCreated {
                session,
                request_id,
                ..
            }) = events.recv().await
            {
                assert_eq!(request_id.as_deref(), Some("device:alice/create-1"));
                break session;
            }
        }
    })
    .await
    .unwrap();
    let record = share.read(&session, &alice).expect("registered before event");
    assert_eq!(record.creator, alice.id);
    assert_eq!(record.visibility, Visibility::Public);

    // Failed dispatch leaves no author intent: a fresh intent for the same scoped id inserts.
    prompt_as(&share, &engine, "alice", &session, "x", json!({}), true)
        .await
        .unwrap_err();
    assert!(share.expect_author(&session, "device:alice/same-client-id", &alice));
    share.drop_author("device:alice/same-client-id");

    // Spoofed identity fields are ignored; same client id from two actors both succeed.
    let spoof = json!({"actor": "device:bob", "nickname": "Mallory", "creator": "device:bob"});
    prompt_as(&share, &engine, "alice", &session, "from alice", spoof, false)
        .await
        .unwrap();
    wait_idle(&engine, &session).await;
    prompt_as(&share, &engine, "bob", &session, "from bob", json!({}), false)
        .await
        .unwrap();
    wait_idle(&engine, &session).await;

    let read = |share: Arc<ShareStore>, engine: Engine, device: &'static str, session: String| async move {
        shared_web_ui_call(
            &share,
            device,
            "sessions.transcript",
            json!({"session": session}),
            move |args| async move {
                let page = engine
                    .transcript_page(args["session"].as_str().unwrap(), None, 50)
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(page).map_err(|e| e.to_string())
            },
        )
        .await
        .unwrap()
    };
    let page = read(share.clone(), engine.clone(), "bob", session.clone()).await;
    let authored: Vec<(String, String, String)> = page["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry.get("author").is_some())
        .map(|entry| {
            (
                entry["part"]["text"].as_str().unwrap_or_default().to_string(),
                entry["author"]["actor"].as_str().unwrap().to_string(),
                entry["author"]["nickname"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        authored,
        vec![
            ("from alice".into(), "device:alice".into(), "Alice".into()),
            ("from bob".into(), "device:bob".into(), "Bob".into()),
        ],
        "{page}"
    );

    // Realtime: a message_author notice per accepted message, no metadata in the event.
    let mut kinds = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let Event::SessionShareChanged { kind, .. } = &event {
            kinds.push(kind.clone());
            assert!(share_event_visible(&share, "bob", &event));
        }
    }
    assert_eq!(kinds.iter().filter(|k| *k == "message_author").count(), 2);

    // Reopen: authorship survives restart of the share store.
    let reopened = ShareStore::open(&share_path).unwrap();
    let authors = reopened.message_authors(&session).unwrap();
    assert_eq!(authors.len(), 2);
    assert!(authors.values().any(|a| a.nickname == "Alice"));

    // Privacy invalidation: after the creator goes private, the revoke notice reaches everyone
    // (metadata-free) but content, follow and author events stay creator-only.
    share
        .set_visibility(&session, &alice, Visibility::Private)
        .unwrap();
    let mut revoked = None;
    while let Ok(event) = events.try_recv() {
        if let Event::SessionShareChanged { kind, .. } = &event {
            if kind == "revoked" {
                revoked = Some(event);
            }
        }
    }
    let revoked = revoked.expect("revoked event emitted");
    assert!(share_event_visible(&share, "bob", &revoked));
    let follow = Event::SessionShareChanged {
        session: session.clone(),
        kind: "follow".into(),
        revision: 99,
    };
    assert!(!share_event_visible(&share, "bob", &follow));
    assert!(!share_event_visible(&share, "", &follow), "host owner too");
    assert!(share_event_visible(&share, "alice", &follow));
    assert!(prompt_as(&share, &engine, "bob", &session, "late", json!({}), false)
        .await
        .is_err());
    assert!(
        read_denied(&share, "bob", &session).await,
        "open public view cannot keep reading after private"
    );
}

async fn read_denied(share: &ShareStore, device: &str, session: &str) -> bool {
    shared_web_ui_call(
        share,
        device,
        "sessions.transcript",
        json!({"session": session}),
        |_| async { Ok(json!({"entries": []})) },
    )
    .await
    .is_err()
}

#[test]
fn author_history_is_not_bounded_by_the_operation_journal_and_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.json");
    let store = ShareStore::open(&path).unwrap();
    let a = store.set_nickname("a", "Ada").unwrap();
    store.register_session("s", &a, Visibility::Public).unwrap();
    for seq in 0..(codetwo_core::share::MAX_JOURNAL as i64 + 120) {
        let id = format!("device:a/r{seq}");
        assert!(store.expect_author("s", &id, &a));
        store.record_op("s", &a, "prompt", Some(&id)).unwrap();
        store.record_message_author("s", seq, &id).unwrap();
    }
    let reopened = ShareStore::open(&path).unwrap();
    let record = reopened.read("s", &a).unwrap();
    assert_eq!(record.journal.len(), codetwo_core::share::MAX_JOURNAL);
    assert_eq!(
        record.message_authors.len(),
        codetwo_core::share::MAX_JOURNAL + 120,
        "authorship is durable beyond the bounded journal"
    );
    assert_eq!(record.message_authors[&0].actor, "device:a");
}

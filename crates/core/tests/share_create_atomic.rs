//! Creation transaction boundary through the real Engine: creator/visibility metadata commits
//! BEFORE any session content exists. A forced metadata write failure creates nothing (no
//! `SessionCreated`, no persisted session) so a new private task can never reopen as unowned
//! legacy (host-visible). FIXTURE: disposable zero-fee python ACP stub; no paid Agent.

use std::sync::Arc;
use std::time::Duration;

use codetwo_core::event::Event;
use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
use codetwo_core::share::{ShareStore, Visibility};
use codetwo_core::skill::SkillLibrary;
use codetwo_core::store::Store;
use codetwo_core::{Engine, Op};

const SCRIPT: &str = r#"
import json,sys
for line in sys.stdin:
    m=json.loads(line); method=m.get('method'); mid=m.get('id'); result={}
    if method=='initialize': result={'protocolVersion':1}
    elif method=='session/new': result={'sessionId':'fx'}
    elif mid is None: continue
    print(json.dumps({'jsonrpc':'2.0','id':mid,'result':result}),flush=True)
"#;

fn new_session(cwd: &str, request_id: &str) -> Op {
    Op::NewSession {
        provider: ProviderId::Grok,
        cwd: cwd.into(),
        use_worktree: false,
        worktree_base: None,
        worktree_base_sha: None,
        request_id: Some(request_id.into()),
        model: None,
        initial_policy: None,
    }
}

#[tokio::test]
async fn metadata_failure_creates_no_legacy_visible_private_task_and_success_orders_metadata_first() {
    let root = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session-shares.json");
    let share = Arc::new(ShareStore::open(&path).unwrap());
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (engine, mut events) = Engine::with_store(
        vec![Provider {
            id: ProviderId::Grok,
            display_name: "Create-order fixture (zero-fee, not a paid Agent)".into(),
            launch: LaunchSpec::new("python3", ["-c", SCRIPT]),
            needs_node: false,
        }],
        SkillLibrary::new(vec![]),
        store.clone(),
    );
    engine.attach_share_store(share.clone());
    let cwd: String = root.path().canonicalize().unwrap().to_string_lossy().into();
    let alice = share.participant("alice");

    // Forced failure: the metadata write cannot complete.
    std::fs::create_dir(path.with_extension("tmp")).unwrap();
    share.expect_create("device:alice/fail", &alice, Visibility::Private);
    engine.submit(new_session(&cwd, "device:alice/fail")).await.unwrap();
    let mut saw_error = false;
    while let Ok(Some(event)) =
        tokio::time::timeout(Duration::from_millis(500), events.recv()).await
    {
        match event {
            Event::SessionCreated { .. } => panic!("SessionCreated leaked despite metadata failure"),
            Event::Error { terminal: true, request_id, message, .. } => {
                assert_eq!(request_id.as_deref(), Some("device:alice/fail"));
                assert!(message.contains("sharing"), "{message}");
                saw_error = true;
            }
            _ => {}
        }
    }
    assert!(saw_error, "creator sees the failure");
    assert!(store.list_sessions().unwrap().is_empty(), "no content persisted -> cannot reopen as legacy");
    // Restart view: nothing legacy and host-visible exists, and the private record never existed.
    let reopened = ShareStore::open(&path.with_file_name("session-shares.json")).unwrap();
    assert!(reopened.read("anything", &alice).is_none());

    // Positive ordering: with writes healthy, the record is durable by the time the event shows.
    std::fs::remove_dir(path.with_extension("tmp")).unwrap();
    share.drop_pending("device:alice/fail");
    share.expect_create("device:alice/ok", &alice, Visibility::Private);
    engine.submit(new_session(&cwd, "device:alice/ok")).await.unwrap();
    let session = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(Event::SessionCreated { session, .. }) = events.recv().await {
                break session;
            }
        }
    })
    .await
    .unwrap();
    let on_disk = ShareStore::open(&path).unwrap();
    let record = on_disk.read(&session, &alice).expect("durable before/at SessionCreated");
    assert_eq!(record.visibility, Visibility::Private);
    assert!(!on_disk.can_view(&session, &on_disk.participant("")), "host denied private");
    assert_eq!(store.list_sessions().unwrap().len(), 1);
}

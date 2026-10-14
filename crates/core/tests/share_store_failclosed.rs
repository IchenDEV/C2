//! ShareStore durability: a corrupt or unwritable store fails closed (no any-actor fallback),
//! one Arc per data file, and the creator is immutable across reopen.

use codetwo_core::share::{shared_store, ShareStore, Visibility};

#[test]
fn corrupt_store_refuses_to_open_instead_of_exposing_everything() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("share.json");
    std::fs::write(&path, b"{ not json").unwrap();
    let error = ShareStore::open(&path).err().expect("corrupt store must not open");
    assert!(error.contains("corrupt"), "{error}");
    assert!(shared_store(&path).is_err(), "registry must not cache or default a bad store");
}

#[test]
fn unwritable_store_propagates_the_error_and_never_widens_visibility() {
    let dir = tempfile::tempdir().unwrap();
    // The store path is a directory: reads fail as unreadable, so open itself fails closed.
    let blocked = dir.path().join("share.json");
    std::fs::create_dir(&blocked).unwrap();
    assert!(ShareStore::open(&blocked).is_err());

    // Persist failure after open: the parent becomes a file, so every write errors.
    let ok = dir.path().join("data");
    let path = ok.join("share.json");
    let store = ShareStore::open(&path).unwrap();
    let alice = store.participant("alice");
    let bob = store.participant("bob");
    std::fs::write(&ok, b"now a file").unwrap();
    assert!(store.register_session("s1", &alice, Visibility::Private).is_err());
    // The failed registration must not leave a session any other actor could see.
    assert!(!store.can_view("s1", &bob));
}

#[test]
fn one_arc_per_path_and_creator_survives_reopen_immutably() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("share.json");
    let a = shared_store(&path).unwrap();
    let b = shared_store(&path).unwrap();
    assert!(std::sync::Arc::ptr_eq(&a, &b));
    let alice = a.participant("alice");
    let bob = a.participant("bob");
    a.register_session("s1", &alice, Visibility::Private).unwrap();
    a.register_session("s1", &bob, Visibility::Public).unwrap();
    let reopened = ShareStore::open(&path).unwrap();
    let record = reopened.read("s1", &alice).unwrap();
    assert_eq!(record.creator, "device:alice");
    assert!(!reopened.can_view("s1", &bob));
    assert!(!reopened.can_view("s1", &reopened.participant("")), "host owner is not exempt");
}

/// R8 transaction safety: with the disk write forced to fail (the temp file path is a directory),
/// every persistent mutation leaves memory, revision and disk exactly as before and sends no
/// change notice; after the fault clears, a reopen shows the prior state.
#[test]
fn failed_persist_rolls_back_every_mutation_without_false_notice() {
    use std::sync::{Arc, Mutex};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("share.json");
    let store = ShareStore::open(&path).unwrap();
    let notices = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = notices.clone();
    store.set_notifier(Arc::new(move |c| sink.lock().unwrap().push(format!("{}:{}", c.kind, c.revision))));
    let alice = store.set_nickname("alice", "Alice").unwrap();
    let bob = store.participant("bob");
    store.register_session("s", &alice, Visibility::Private).unwrap();
    store.register_session("p", &alice, Visibility::Public).unwrap();
    assert!(store.expect_author("p", "device:alice/r", &alice));
    store.set_follow("p", &bob, true).unwrap();
    notices.lock().unwrap().clear();
    let before = std::fs::read(&path).unwrap();
    let revision = store.read("p", &alice).unwrap().revision;

    // Fault: writes fail until the directory is removed.
    std::fs::create_dir(path.with_extension("tmp")).unwrap();
    assert!(store.set_visibility("s", &alice, Visibility::Public).is_err());
    assert!(store.set_visibility("p", &alice, Visibility::Private).is_err());
    assert!(store.set_follow("p", &bob, false).is_err());
    assert!(store.set_nickname("alice", "Mallory").is_err());
    assert!(store.record_op("p", &alice, "prompt", Some("x")).is_err());
    assert!(store.record_message_author("p", 3, "device:alice/r").is_err());
    assert!(store.register_session("new", &alice, Visibility::Private).is_err());
    assert!(!store.expect_author("p", "device:alice/other", &alice), "intent not persisted");

    assert!(!store.can_view("s", &bob), "private stayed private in memory");
    assert!(store.can_view("p", &bob), "public stayed public in memory");
    let p = store.read("p", &alice).unwrap();
    assert_eq!(p.revision, revision);
    assert!(p.followers.contains(&bob.id) && p.journal.is_empty() && p.message_authors.is_empty());
    assert_eq!(store.participant("alice").nickname, "Alice");
    assert!(!store.can_view("new", &bob) && store.read("new", &alice).is_none());
    assert!(notices.lock().unwrap().is_empty(), "false change notice: {:?}", notices.lock().unwrap());
    assert_eq!(std::fs::read(&path).unwrap(), before, "disk untouched");
    // The pending author intent survived and still binds once writes work again.
    std::fs::remove_dir(path.with_extension("tmp")).unwrap();
    store.record_message_author("p", 3, "device:alice/r").unwrap();
    assert_eq!(notices.lock().unwrap().len(), 1);

    let reopened = ShareStore::open(&path).unwrap();
    assert!(!reopened.can_view("s", &bob));
    assert_eq!(reopened.read("p", &alice).unwrap().message_authors.len(), 1);
}

use codetwo_core::{assistant::*, provider::ProviderId, Store};

fn setup(store: &Store) -> AssistantState {
    for (path, name) in [
        ("/repo-a1", "A1"),
        ("/repo-a2", "A2"),
        ("/repo-b", "B"),
        ("/shared", "S"),
    ] {
        store.add_project(path, Some(name), 0).unwrap();
    }
    let state = store
        .edit_assistant(
            0,
            AssistantEdit::Settings {
                settings: AssistantSettings {
                    enabled: true,
                    projects: vec!["/repo-a1".into(), "/repo-a2".into(), "/repo-b".into()],
                    provider: ProviderId::Codex,
                    model: None,
                    reasoning_effort: None,
                    concurrency: 2,
                    turn_limit: 5,
                    dispatch_limit: 2,
                },
            },
        )
        .unwrap();
    store
        .edit_assistant(
            state.revision,
            AssistantEdit::Goal {
                id: None,
                project_path: "/repo-a1".into(),
                title: "API".into(),
                acceptance: "API passes".into(),
                priority: 1,
            },
        )
        .unwrap()
}

fn edit(store: &Store, edit: HierarchyEdit) -> AssistantState {
    let rev = store.assistant_state().unwrap().revision;
    store.edit_hierarchy(rev, edit).unwrap()
}

fn rev(store: &Store) -> u64 {
    store.assistant_state().unwrap().revision
}

fn project(store: &Store, name: &str) -> String {
    let state = edit(store, HierarchyEdit::CreateProject { name: name.into() });
    state
        .hierarchy
        .unwrap()
        .projects
        .into_iter()
        .find(|p| p.name == name)
        .unwrap()
        .id
}

fn bind(store: &Store, project_id: &str, path: &str) -> String {
    let state = edit(
        store,
        HierarchyEdit::BindWorkspace {
            project_id: project_id.into(),
            path: path.into(),
        },
    );
    state
        .hierarchy
        .unwrap()
        .project(project_id)
        .unwrap()
        .bindings
        .iter()
        .find(|b| b.path == path && b.active)
        .unwrap()
        .id
        .clone()
}

fn with_writer(store: &Store, goal_id: &str, session: &str) {
    let mut state = store.assistant_state().unwrap();
    let expected = state.revision;
    let goal = state.goals.iter_mut().find(|g| g.id == goal_id).unwrap();
    goal.assignments.push(Assignment {
        id: "assignment-1".into(),
        instruction: "work".into(),
        session_id: Some(session.into()),
        submitted: true,
        taken_over: false,
        owned: true,
        contract_revision: 1,
        confirmed_revision: 1,
        protocol: 0,
        stop_requested: false,
        stop_sent: false,
        result: None,
        inputs: vec![],
        pending_inputs: None,
        prior_results: vec![],
    });
    store.save_assistant(expected, state).unwrap();
}

#[test]
fn legacy_bytes_and_migration_preserve_everything() {
    let store = Store::open_in_memory().unwrap();
    let legacy = setup(&store);
    store
        .add_memory("/repo-a1", "constraint", "Keep API stable", true)
        .unwrap();
    let before = serde_json::to_string(&legacy).unwrap();
    assert!(
        !before.contains("hierarchy"),
        "absent hierarchy is never serialized"
    );
    let round: AssistantState = serde_json::from_str(&before).unwrap();
    assert_eq!(serde_json::to_string(&round).unwrap(), before);

    let upgraded = edit(&store, HierarchyEdit::Enable {});
    assert!(upgraded.hierarchy.is_some());
    let strip = |s: &AssistantState| {
        let mut v = serde_json::to_value(s).unwrap();
        let o = v.as_object_mut().unwrap();
        o.remove("hierarchy");
        o.remove("revision");
        v
    };
    assert_eq!(
        strip(&legacy),
        strip(&upgraded),
        "goals/conversation/receipts untouched"
    );
    assert_eq!(store.list_memories("/repo-a1", 10).unwrap().len(), 1);
}

#[test]
fn stable_identity_multi_workspace_and_explicit_ambiguity() {
    let store = Store::open_in_memory().unwrap();
    let state = setup(&store);
    let goal = state.goals[0].id.clone();
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    let other = project(&store, "Other");
    let a1 = bind(&store, &a, "/repo-a1");
    bind(&store, &a, "/repo-a2");
    bind(&store, &other, "/repo-a1");
    // Same directory serves two Projects: the user must choose, never the most recent one.
    let h = store.assistant_state().unwrap();
    assert_eq!(candidate_projects(&h, &goal).len(), 2);
    // Unregistered directory and relative paths are rejected.
    let r = rev(&store);
    assert!(store
        .edit_hierarchy(
            r,
            HierarchyEdit::BindWorkspace {
                project_id: a.clone(),
                path: "/nowhere".into()
            }
        )
        .is_err());
    assert!(store
        .edit_hierarchy(
            r,
            HierarchyEdit::BindWorkspace {
                project_id: a.clone(),
                path: "repo-a1/../x".into()
            }
        )
        .is_err());
    // The goal's workspace must be bound to the chosen project.
    assert!(store
        .edit_hierarchy(
            r,
            HierarchyEdit::AssignGoal {
                goal_id: goal.clone(),
                project_id: a.clone(),
                binding_id: Some("forged".into())
            }
        )
        .is_err());
    let owned = edit(
        &store,
        HierarchyEdit::AssignGoal {
            goal_id: goal.clone(),
            project_id: a.clone(),
            binding_id: Some(a1.clone()),
        },
    );
    assert_eq!(
        owned.hierarchy.as_ref().unwrap().goal_owners[&goal].epoch,
        1
    );
    // Rename keeps the UUID.
    let renamed = edit(
        &store,
        HierarchyEdit::RenameProject {
            project_id: a.clone(),
            name: "Alpha".into(),
        },
    );
    assert_eq!(
        renamed.hierarchy.unwrap().project(&a).unwrap().name,
        "Alpha"
    );
}

#[test]
fn actor_authorization_is_host_created_and_scoped() {
    let store = Store::open_in_memory().unwrap();
    let state = setup(&store);
    let goal = state.goals[0].id.clone();
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    let b = project(&store, "B");
    let ab = bind(&store, &a, "/repo-a1");
    bind(&store, &b, "/repo-b");
    let state = edit(
        &store,
        HierarchyEdit::AssignGoal {
            goal_id: goal.clone(),
            project_id: a.clone(),
            binding_id: Some(ab),
        },
    );

    let chief = HostActor::chief();
    let pa = HostActor::project(&state, &a).unwrap();
    let pb = HostActor::project(&state, &b).unwrap();
    assert!(authorize(&state, &chief, ManagerAction::RouteProject, None, None).is_ok());
    assert!(authorize(&state, &chief, ManagerAction::Dispatch, Some(&goal), None).is_err());
    assert!(authorize(&state, &pa, ManagerAction::Dispatch, Some(&goal), Some(1)).is_ok());
    assert!(
        authorize(&state, &pa, ManagerAction::Dispatch, Some(&goal), Some(2)).is_err(),
        "stale epoch"
    );
    assert!(
        authorize(&state, &pb, ManagerAction::Dispatch, Some(&goal), None).is_err(),
        "sibling project"
    );
    assert!(HostActor::project(&state, "not-a-project").is_err());
    // Provider text cannot assert an actor: edits are strict and carry no actor field.
    assert!(serde_json::from_str::<HierarchyEdit>(r#"{"kind":"enable","actor":"chief"}"#).is_err());
    assert!(serde_json::from_str::<HierarchyEdit>(
        r#"{"kind":"confirm_instruction","proposal_id":"x","text_hash":"y","actor":"project:1"}"#
    )
    .is_err());

    // Pause stops new coordination but is not revocation.
    let paused = edit(
        &store,
        HierarchyEdit::PauseProject {
            project_id: a.clone(),
        },
    );
    assert!(authorize(&paused, &pa, ManagerAction::Dispatch, Some(&goal), None).is_err());
    assert!(authorize(&paused, &pa, ManagerAction::Report, None, None).is_ok());
}

#[test]
fn instructions_need_exact_human_confirmation() {
    let store = Store::open_in_memory().unwrap();
    setup(&store);
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    let pa = HostActor::project(&store.assistant_state().unwrap(), &a).unwrap();
    // A manager can neither propose outside its scope nor activate anything.
    assert!(store
        .propose_instruction_as(
            rev(&store),
            &pa,
            HierarchyScope::Global,
            "x",
            Reach::Executor
        )
        .is_err());
    let (proposed, id) = store
        .propose_instruction_as(
            rev(&store),
            &pa,
            HierarchyScope::Project(a.clone()),
            "Reply in English",
            Reach::Executor,
        )
        .unwrap();
    let h = proposed.hierarchy.unwrap();
    assert!(h.instructions.is_empty(), "proposal is inert");
    let hash = h.proposals[0].hash.clone();
    assert_eq!(hash, instruction_hash("Reply in English"));
    assert!(effective_instructions(&store.assistant_state().unwrap(), &pa).is_empty());
    // Wrong hash is rejected, exact hash activates revision 1.
    let r = rev(&store);
    assert!(store
        .edit_hierarchy(
            r,
            HierarchyEdit::ConfirmInstruction {
                proposal_id: id.clone(),
                text_hash: "bad".into()
            }
        )
        .is_err());
    let confirmed = edit(
        &store,
        HierarchyEdit::ConfirmInstruction {
            proposal_id: id.clone(),
            text_hash: hash.clone(),
        },
    );
    let eff = effective_instructions(&confirmed, &pa);
    assert_eq!(
        (eff.len(), eff[0].revision, eff[0].hash.as_str()),
        (1, 1, hash.as_str())
    );
    // Replaying the confirmation, or a competing older proposal, cannot re-activate.
    let r = rev(&store);
    assert!(store
        .edit_hierarchy(
            r,
            HierarchyEdit::ConfirmInstruction {
                proposal_id: id,
                text_hash: hash
            }
        )
        .is_err());
    // Chief-only global docs do not reach project managers; executors only see Executor reach.
    edit(
        &store,
        HierarchyEdit::SetInstruction {
            scope: HierarchyScope::Global,
            text: "Be brief".into(),
            reach: Reach::Chief,
        },
    );
    let now = store.assistant_state().unwrap();
    assert!(effective_instructions(&now, &pa)
        .iter()
        .all(|i| i.scope != HierarchyScope::Global));
    assert_eq!(effective_instructions(&now, &HostActor::chief()).len(), 1);
}

#[test]
fn memory_is_scoped_shared_by_exact_reference_and_revocable() {
    let store = Store::open_in_memory().unwrap();
    setup(&store);
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    let b = project(&store, "B");
    bind(&store, &a, "/repo-a1");
    bind(&store, &b, "/repo-b");
    let own_a = store
        .add_memory(
            &project_memory_scope(&a),
            "fact",
            "A private decision",
            false,
        )
        .unwrap();
    store
        .add_memory(
            &project_memory_scope(&b),
            "fact",
            "B private decision",
            false,
        )
        .unwrap();
    store
        .add_memory("/repo-a1", "fact", "A workspace knowledge", false)
        .unwrap();
    let global = store
        .add_memory(GLOBAL_MEMORY, "preference", "Reply in Chinese", false)
        .unwrap();

    let state = store.assistant_state().unwrap();
    let pa = HostActor::project(&state, &a).unwrap();
    let texts = |actor: &HostActor| -> Vec<String> {
        let s = store.assistant_state().unwrap();
        let grant = memory_grant(&s, actor).unwrap();
        let mut v: Vec<String> = store
            .scoped_memory_read(&grant, 50)
            .unwrap()
            .into_iter()
            .map(|m| m.record.content)
            .collect();
        v.sort();
        v
    };
    let seen = texts(&pa);
    assert!(seen.contains(&"A private decision".to_string()));
    assert!(seen.contains(&"A workspace knowledge".to_string()));
    assert!(
        !seen
            .iter()
            .any(|t| t.contains("B private") || t.contains("Chinese")),
        "no sibling or unshared global leak"
    );

    // Share the exact global text down to project A's managers.
    let chief = HostActor::chief();
    let (_, share) = store
        .propose_memory_share_as(
            rev(&store),
            &chief,
            &global.id,
            HierarchyScope::Project(a.clone()),
            Reach::ProjectManager,
        )
        .unwrap();
    assert!(
        !texts(&pa).iter().any(|t| t.contains("Chinese")),
        "proposal alone grants nothing"
    );
    let hash = store.memory_share_hash(&global.id).unwrap();
    edit(
        &store,
        HierarchyEdit::ConfirmShare {
            share_id: share.clone(),
            content_hash: hash,
        },
    );
    assert!(texts(&pa).iter().any(|t| t.contains("Chinese")));
    assert!(
        !texts(&HostActor::project(&store.assistant_state().unwrap(), &b).unwrap())
            .iter()
            .any(|t| t.contains("Chinese"))
    );
    let grant = memory_grant(&store.assistant_state().unwrap(), &pa).unwrap();
    let read = store.scoped_memory_read(&grant, 50).unwrap();
    assert!(
        read.iter()
            .any(|m| m.via.starts_with("share:") && m.record.id == global.id),
        "provenance edge kept"
    );

    // Correcting the memory changes its hash and silently invalidates the reference.
    store
        .update_memory(&global.id, "preference", "Reply in English")
        .unwrap();
    assert!(!texts(&pa)
        .iter()
        .any(|t| t.contains("Chinese") || t.contains("English")));
    // A project manager cannot share a memory it cannot read.
    assert!(store
        .propose_memory_share_as(
            rev(&store),
            &pa,
            &own_a.id,
            HierarchyScope::Project(b.clone()),
            Reach::ProjectManager
        )
        .is_ok());
    let pb_actor = HostActor::project(&store.assistant_state().unwrap(), &b).unwrap();
    assert!(store
        .propose_memory_share_as(
            rev(&store),
            &pb_actor,
            &own_a.id,
            HierarchyScope::Global,
            Reach::Chief
        )
        .is_err());

    // Revocation removes the grant at once.
    let s2 = store
        .assistant_state()
        .unwrap()
        .hierarchy
        .unwrap()
        .shares
        .iter()
        .find(|s| s.memory_id == own_a.id)
        .unwrap()
        .id
        .clone();
    let h2 = store.memory_share_hash(&own_a.id).unwrap();
    edit(
        &store,
        HierarchyEdit::ConfirmShare {
            share_id: s2.clone(),
            content_hash: h2,
        },
    );
    assert!(
        texts(&HostActor::project(&store.assistant_state().unwrap(), &b).unwrap())
            .iter()
            .any(|t| t.contains("A private decision"))
    );
    edit(&store, HierarchyEdit::RevokeShare { share_id: s2 });
    assert!(
        !texts(&HostActor::project(&store.assistant_state().unwrap(), &b).unwrap())
            .iter()
            .any(|t| t.contains("A private decision"))
    );
}

#[test]
fn revocation_keeps_unresolved_writer_and_blocks_replacement() {
    let store = Store::open_in_memory().unwrap();
    let state = setup(&store);
    let goal = state.goals[0].id.clone();
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    let b = project(&store, "B");
    let ab = bind(&store, &a, "/repo-a1");
    let bb = bind(&store, &b, "/repo-a1");
    edit(
        &store,
        HierarchyEdit::AssignGoal {
            goal_id: goal.clone(),
            project_id: a.clone(),
            binding_id: Some(ab.clone()),
        },
    );
    with_writer(&store, &goal, "session-1");

    let revoked = edit(
        &store,
        HierarchyEdit::RevokeBinding {
            project_id: a.clone(),
            binding_id: ab,
        },
    );
    let g = &revoked.goals[0];
    assert_eq!(g.status, GoalStatus::NeedsAttention);
    let w = g.assignments.last().unwrap();
    assert!(
        w.stop_requested && w.session_id.as_deref() == Some("session-1") && w.submitted,
        "writer retained"
    );
    assert!(authorize(
        &revoked,
        &HostActor::project(&revoked, &a).unwrap(),
        ManagerAction::Dispatch,
        Some(&goal),
        None
    )
    .is_err());

    // The owner cannot move to B (or be dropped) before the host records a verified release.
    let r = rev(&store);
    assert!(store
        .edit_hierarchy(
            r,
            HierarchyEdit::TransferGoal {
                goal_id: goal.clone(),
                project_id: b.clone(),
                binding_id: Some(bb.clone())
            }
        )
        .is_err());
    // User text cannot manufacture the host's terminal/stop receipt.
    assert!(serde_json::from_value::<HierarchyEdit>(serde_json::json!({
        "kind": "release_writer", "goal_id": goal, "session_id": "session-1"
    }))
    .is_err());
    // A raw save which drops or alters the owner of an unresolved writer is rejected.
    let mut forged = store.assistant_state().unwrap();
    forged.hierarchy.as_mut().unwrap().goal_owners.clear();
    forged.hierarchy.as_mut().unwrap().revision += 1;
    assert!(store.save_assistant(forged.revision, forged).is_err());
    assert_eq!(
        store
            .assistant_state()
            .unwrap()
            .hierarchy
            .unwrap()
            .goal_owners[&goal]
            .epoch,
        1
    );
}

#[test]
fn stale_and_downgrade_writers_are_rejected_by_the_persisted_store() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("assistant.db");
    let store = Store::open(db.to_str().unwrap()).unwrap();
    setup(&store);
    let enabled = edit(&store, HierarchyEdit::Enable {});
    let current = store.assistant_state().unwrap();

    // New-code stale writer: same expected revision as an older read.
    assert!(store
        .edit_hierarchy(
            enabled.revision - 1,
            HierarchyEdit::CreateProject { name: "X".into() }
        )
        .is_err());
    // New-code writer which loses the hierarchy.
    let mut dropped = current.clone();
    dropped.hierarchy = None;
    assert!(store.save_assistant(current.revision, dropped).is_err());
    // New-code writer which edits the hierarchy without bumping its revision.
    let mut silent = current.clone();
    silent.hierarchy.as_mut().unwrap().projects.clear();
    silent.hierarchy.as_mut().unwrap().shares.clear();
    silent.hierarchy.as_mut().unwrap().schema = 0;
    assert!(store.save_assistant(current.revision, silent).is_err());

    // An old binary has no `hierarchy` field in its struct: simulate its raw UPDATE directly.
    let conn = rusqlite::Connection::open(&db).unwrap();
    let body: String = conn
        .query_row(
            "SELECT body FROM assistant_state WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut legacy: serde_json::Value = serde_json::from_str(&body).unwrap();
    legacy.as_object_mut().unwrap().remove("hierarchy");
    legacy["revision"] = serde_json::json!(current.revision + 1);
    let old_write = conn.execute(
        "UPDATE assistant_state SET revision=?1, body=?2 WHERE singleton=1",
        rusqlite::params![current.revision + 1, legacy.to_string()],
    );
    assert!(
        old_write.is_err(),
        "database refuses a hierarchy-dropping old writer"
    );
    assert!(conn
        .execute("DELETE FROM assistant_state WHERE singleton=1", [])
        .is_err());
    // Lowering the hierarchy revision is refused too.
    let mut lowered: serde_json::Value = serde_json::from_str(&body).unwrap();
    lowered["hierarchy"]["revision"] = serde_json::json!(0);
    assert!(conn
        .execute(
            "UPDATE assistant_state SET body=?1 WHERE singleton=1",
            [lowered.to_string()]
        )
        .is_err());
    // Nothing changed.
    assert_eq!(store.assistant_state().unwrap().revision, current.revision);
    assert!(store.assistant_state().unwrap().hierarchy.is_some());
}

// ---- memory grants are re-validated against current authority on every read ----------------

fn shared_memory_for(store: &Store, a: &str) -> (String, String) {
    let m = store
        .add_memory(
            GLOBAL_MEMORY,
            "preference",
            "private shared preference",
            false,
        )
        .unwrap();
    let (_, share) = store
        .propose_memory_share_as(
            rev(store),
            &HostActor::chief(),
            &m.id,
            HierarchyScope::Project(a.into()),
            Reach::ProjectManager,
        )
        .unwrap();
    let hash = store.memory_share_hash(&m.id).unwrap();
    edit(
        store,
        HierarchyEdit::ConfirmShare {
            share_id: share.clone(),
            content_hash: hash,
        },
    );
    (m.id, share)
}

fn reads(store: &Store, grant: &MemoryGrant, id: &str) -> bool {
    store
        .scoped_memory_read(grant, 50)
        .map(|r| r.iter().any(|m| m.record.id == id))
        .unwrap_or(false)
}

#[test]
fn cached_grant_does_not_survive_revoked_share() {
    let store = Store::open_in_memory().unwrap();
    setup(&store);
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    bind(&store, &a, "/repo-a1");
    let (memory, share) = shared_memory_for(&store, &a);
    let s = store.assistant_state().unwrap();
    let grant = memory_grant(&s, &HostActor::project(&s, &a).unwrap()).unwrap();
    assert!(reads(&store, &grant, &memory), "approved share is readable");
    edit(&store, HierarchyEdit::RevokeShare { share_id: share });
    assert!(
        !reads(&store, &grant, &memory),
        "a cached grant cannot keep a revoked share readable"
    );
}

#[test]
fn target_deny_blocks_all_context_sources() {
    use codetwo_core::memory::{MemoryPolicyValue, MemoryProjectPolicy};
    let store = Store::open_in_memory().unwrap();
    setup(&store);
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    bind(&store, &a, "/repo-a1");
    store
        .add_memory("/repo-a1", "fact", "A workspace knowledge", false)
        .unwrap();
    let (memory, _) = shared_memory_for(&store, &a);
    let s = store.assistant_state().unwrap();
    let grant = memory_grant(&s, &HostActor::project(&s, &a).unwrap()).unwrap();
    assert!(reads(&store, &grant, &memory));
    let policy = |inject| MemoryProjectPolicy {
        project_path: project_memory_scope(&a),
        capture: MemoryPolicyValue::Inherit,
        inject,
        include_external_context: MemoryPolicyValue::Inherit,
    };
    store
        .set_memory_project_policy(&policy(MemoryPolicyValue::Deny))
        .unwrap();
    let read = store.scoped_memory_read(&grant, 50).unwrap();
    assert!(
        !read.iter().any(|m| m.record.id == memory),
        "target deny takes priority over an approved source share"
    );
    assert!(
        read.is_empty(),
        "target deny suppresses workspace and owned memory as well as shares"
    );
    store
        .set_memory_project_policy(&policy(MemoryPolicyValue::Inherit))
        .unwrap();
    assert!(
        reads(&store, &grant, &memory),
        "lifting the deny restores the read"
    );
    assert!(store
        .scoped_memory_read(&grant, 50)
        .unwrap()
        .iter()
        .any(|m| m.record.content == "A workspace knowledge"));
}

#[test]
fn unrelated_project_changes_keep_an_equivalent_grant_and_keep_provenance() {
    let store = Store::open_in_memory().unwrap();
    setup(&store);
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    bind(&store, &a, "/repo-a1");
    let (memory, share) = shared_memory_for(&store, &a);
    let s = store.assistant_state().unwrap();
    let grant = memory_grant(&s, &HostActor::project(&s, &a).unwrap()).unwrap();
    let b = project(&store, "B");
    bind(&store, &b, "/repo-b");
    edit(&store, HierarchyEdit::RetireProject { project_id: b });
    let read = store.scoped_memory_read(&grant, 50).unwrap();
    let hit = read.iter().find(|m| m.record.id == memory).unwrap();
    assert_eq!(hit.via, format!("share:{share}"), "provenance edge kept");
}

#[test]
fn retired_project_and_moved_owner_invalidate_cached_grants() {
    let store = Store::open_in_memory().unwrap();
    let state = setup(&store);
    let goal = state.goals[0].id.clone();
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    let b = project(&store, "B");
    let ab = bind(&store, &a, "/repo-a1");
    let bb = bind(&store, &b, "/repo-a1");
    store
        .add_memory("/repo-a1", "fact", "A workspace knowledge", false)
        .unwrap();
    edit(
        &store,
        HierarchyEdit::AssignGoal {
            goal_id: goal.clone(),
            project_id: a.clone(),
            binding_id: Some(ab),
        },
    );
    let s = store.assistant_state().unwrap();
    let executor = memory_grant(&s, &HostActor::executor(&s, &goal).unwrap()).unwrap();
    let manager = memory_grant(&s, &HostActor::project(&s, &a).unwrap()).unwrap();
    let workspace = |g: &MemoryGrant| {
        store
            .scoped_memory_read(g, 50)
            .map(|r| {
                r.iter()
                    .any(|m| m.record.content == "A workspace knowledge")
            })
            .unwrap_or(false)
    };
    assert!(workspace(&executor) && workspace(&manager));

    edit(
        &store,
        HierarchyEdit::TransferGoal {
            goal_id: goal,
            project_id: b,
            binding_id: Some(bb),
        },
    );
    assert!(
        !workspace(&executor),
        "a cached executor grant does not follow a transferred goal"
    );
    assert!(
        workspace(&manager),
        "the unrelated manager grant is unaffected"
    );

    edit(&store, HierarchyEdit::RetireProject { project_id: a });
    assert!(
        store.scoped_memory_read(&manager, 50).is_err(),
        "a retired project's cached grant is denied"
    );
}

#[test]
fn guard_probe_cannot_replace_unknown_writer_session() {
    let store = Store::open_in_memory().unwrap();
    let state = setup(&store);
    let goal = state.goals[0].id.clone();
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    let ab = bind(&store, &a, "/repo-a1");
    edit(
        &store,
        HierarchyEdit::AssignGoal {
            goal_id: goal.clone(),
            project_id: a,
            binding_id: Some(ab),
        },
    );
    with_writer(&store, &goal, "unknown-writer");
    let mut next = store.assistant_state().unwrap();
    let expected = next.revision;
    next.goals
        .iter_mut()
        .find(|g| g.id == goal)
        .unwrap()
        .assignments
        .last_mut()
        .unwrap()
        .session_id = Some("replacement-writer".into());
    assert!(
        store.save_assistant(expected, next).is_err(),
        "same assignment id must not authorize a replacement session while old outcome is unknown"
    );
}

#[test]
fn guard_probe_cannot_append_replacement_before_release() {
    let store = Store::open_in_memory().unwrap();
    let state = setup(&store);
    let goal = state.goals[0].id.clone();
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    let ab = bind(&store, &a, "/repo-a1");
    edit(
        &store,
        HierarchyEdit::AssignGoal {
            goal_id: goal.clone(),
            project_id: a,
            binding_id: Some(ab),
        },
    );
    with_writer(&store, &goal, "unknown-writer");
    let mut next = store.assistant_state().unwrap();
    let expected = next.revision;
    let g = next.goals.iter_mut().find(|g| g.id == goal).unwrap();
    let mut replacement = g.assignments.last().unwrap().clone();
    replacement.id = "replacement-assignment".into();
    replacement.session_id = Some("replacement-writer".into());
    g.assignments.push(replacement);
    assert!(
        store.save_assistant(expected, next).is_err(),
        "keeping old assignment id must not authorize a second unresolved writer"
    );
}

#[test]
fn memory_guard_probe_old_grant_does_not_survive_revoked_share() {
    let store = Store::open_in_memory().unwrap();
    setup(&store);
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    bind(&store, &a, "/repo-a1");
    let m = store
        .add_memory(
            GLOBAL_MEMORY,
            "preference",
            "private shared preference",
            false,
        )
        .unwrap();
    let (_, share) = store
        .propose_memory_share_as(
            rev(&store),
            &HostActor::chief(),
            &m.id,
            HierarchyScope::Project(a.clone()),
            Reach::ProjectManager,
        )
        .unwrap();
    let hash = store.memory_share_hash(&m.id).unwrap();
    edit(
        &store,
        HierarchyEdit::ConfirmShare {
            share_id: share.clone(),
            content_hash: hash,
        },
    );
    let s = store.assistant_state().unwrap();
    let actor = HostActor::project(&s, &a).unwrap();
    let grant = memory_grant(&s, &actor).unwrap();
    edit(&store, HierarchyEdit::RevokeShare { share_id: share });
    let read = store.scoped_memory_read(&grant, 50);
    assert!(
        read.is_err() || !read.unwrap().iter().any(|r| r.record.id == m.id),
        "a cached grant cannot keep a revoked share readable"
    );
}

#[test]
fn memory_guard_probe_target_deny_blocks_shared_read() {
    use codetwo_core::memory::{MemoryPolicyValue, MemoryProjectPolicy};
    let store = Store::open_in_memory().unwrap();
    setup(&store);
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    bind(&store, &a, "/repo-a1");
    let m = store
        .add_memory(
            GLOBAL_MEMORY,
            "preference",
            "private shared preference",
            false,
        )
        .unwrap();
    let (_, share) = store
        .propose_memory_share_as(
            rev(&store),
            &HostActor::chief(),
            &m.id,
            HierarchyScope::Project(a.clone()),
            Reach::ProjectManager,
        )
        .unwrap();
    let hash = store.memory_share_hash(&m.id).unwrap();
    edit(
        &store,
        HierarchyEdit::ConfirmShare {
            share_id: share,
            content_hash: hash,
        },
    );
    store
        .set_memory_project_policy(&MemoryProjectPolicy {
            project_path: project_memory_scope(&a),
            capture: MemoryPolicyValue::Inherit,
            inject: MemoryPolicyValue::Deny,
            include_external_context: MemoryPolicyValue::Inherit,
        })
        .unwrap();
    let s = store.assistant_state().unwrap();
    let actor = HostActor::project(&s, &a).unwrap();
    let grant = memory_grant(&s, &actor).unwrap();
    let read = store.scoped_memory_read(&grant, 50).unwrap();
    assert!(
        !read.iter().any(|r| r.record.id == m.id),
        "target deny must take priority over approved source share"
    );
}

#[test]
fn unresolved_writer_flags_cannot_forge_release_but_stop_and_progress_are_saved() {
    let store = Store::open_in_memory().unwrap();
    let initial = setup(&store);
    let goal = initial.goals[0].id.clone();
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    let binding = bind(&store, &a, "/repo-a1");
    edit(
        &store,
        HierarchyEdit::AssignGoal {
            goal_id: goal.clone(),
            project_id: a,
            binding_id: Some(binding),
        },
    );
    with_writer(&store, &goal, "unknown-writer");
    let before = store.assistant_state().unwrap();
    for flag in ["completed", "submitted", "result"] {
        let mut forged = before.clone();
        let g = forged.goals.iter_mut().find(|g| g.id == goal).unwrap();
        let w = g.assignments.last_mut().unwrap();
        match flag {
            "completed" => g.status = GoalStatus::Completed,
            "submitted" => w.submitted = false,
            "result" => {
                w.result = Some(SubmittedResult {
                    created_at: String::new(),
                    id: "forged".into(),
                    contract_revision: 1,
                    activity_revision: 0,
                    evidence: "Idle is not a terminal receipt".into(),
                    artifacts: vec![],
                    state: "submitted".into(),
                    inputs: vec![],
                })
            }
            _ => unreachable!(),
        }
        assert!(
            store.save_assistant(before.revision, forged).is_err(),
            "{flag} must not release an unknown writer"
        );
        assert_eq!(store.assistant_state().unwrap().revision, before.revision);
    }
    let mut control = before.clone();
    control.goals[0]
        .assignments
        .last_mut()
        .unwrap()
        .stop_requested = true;
    control.goals[0].status = GoalStatus::NeedsAttention;
    control.goals[0].next_step = "Waiting for original execution receipt".into();
    let saved = store.save_assistant(before.revision, control).unwrap();
    assert!(saved.goals[0].assignments.last().unwrap().stop_requested);
    let taken_over = store
        .edit_assistant(saved.revision, AssistantEdit::Takeover { id: goal.clone() })
        .unwrap();
    let writer = taken_over.goals[0].assignments.last().unwrap();
    assert!(
        writer.taken_over && !writer.owned,
        "takeover intent is recorded normally"
    );
    assert!(
        writer_unresolved(&taken_over, &taken_over.goals[0]),
        "control intent does not prove terminal execution"
    );
    assert!(store
        .edit_hierarchy(
            taken_over.revision,
            HierarchyEdit::TransferGoal {
                goal_id: goal.clone(),
                project_id: taken_over.hierarchy.as_ref().unwrap().goal_owners[&goal]
                    .project_id
                    .clone(),
                binding_id: Some(
                    taken_over.hierarchy.as_ref().unwrap().goal_owners[&goal]
                        .binding_id
                        .clone()
                ),
            }
        )
        .is_err());
}

#[test]
fn executor_grant_is_fenced_by_owner_epoch_even_with_the_same_binding() {
    let store = Store::open_in_memory().unwrap();
    let initial = setup(&store);
    let goal = initial.goals[0].id.clone();
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    let binding = bind(&store, &a, "/repo-a1");
    edit(
        &store,
        HierarchyEdit::AssignGoal {
            goal_id: goal.clone(),
            project_id: a.clone(),
            binding_id: Some(binding.clone()),
        },
    );
    let state = store.assistant_state().unwrap();
    let grant = memory_grant(&state, &HostActor::executor(&state, &goal).unwrap()).unwrap();
    edit(
        &store,
        HierarchyEdit::TransferGoal {
            goal_id: goal.clone(),
            project_id: a,
            binding_id: Some(binding),
        },
    );
    assert!(store.scoped_memory_read(&grant, 50).is_err());
    let current = store.assistant_state().unwrap();
    let current_grant =
        memory_grant(&current, &HostActor::executor(&current, &goal).unwrap()).unwrap();
    assert!(store.scoped_memory_read(&current_grant, 50).is_ok());
}

#[test]
fn unresolved_legacy_writer_is_not_migrated_into_a_project() {
    let store = Store::open_in_memory().unwrap();
    let initial = setup(&store);
    let goal = initial.goals[0].id.clone();
    with_writer(&store, &goal, "unknown-legacy-writer");
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    let binding = bind(&store, &a, "/repo-a1");
    let before = store.assistant_state().unwrap();
    assert!(store
        .edit_hierarchy(
            before.revision,
            HierarchyEdit::AssignGoal {
                goal_id: goal.clone(),
                project_id: a,
                binding_id: Some(binding)
            }
        )
        .is_err());
    let after = store.assistant_state().unwrap();
    assert_eq!(after.revision, before.revision);
    assert!(!after.hierarchy.unwrap().goal_owners.contains_key(&goal));
    assert_eq!(
        after.goals[0].assignments[0].session_id.as_deref(),
        Some("unknown-legacy-writer")
    );
}

#[test]
fn uncreated_claim_survives_takeover_and_cannot_be_erased_to_transfer() {
    let store = Store::open_in_memory().unwrap();
    let initial = setup(&store);
    let goal = initial.goals[0].id.clone();
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    let binding = bind(&store, &a, "/repo-a1");
    edit(
        &store,
        HierarchyEdit::AssignGoal {
            goal_id: goal.clone(),
            project_id: a.clone(),
            binding_id: Some(binding.clone()),
        },
    );
    with_writer(&store, &goal, "seed");
    // Set up the pre-network claim in a fresh Store document, as the actual runtime does.
    // The seed writer cannot be altered under a mapped owner, so use a separate empty goal.
    let prior = store.assistant_state().unwrap();
    let mut state = store
        .edit_assistant(
            prior.revision,
            AssistantEdit::Goal {
                id: None,
                project_path: "/repo-a1".into(),
                title: "Creating".into(),
                acceptance: "Created once".into(),
                priority: 1,
            },
        )
        .unwrap();
    let creating_goal = state.goals.last().unwrap().id.clone();
    state = store
        .edit_hierarchy(
            state.revision,
            HierarchyEdit::AssignGoal {
                goal_id: creating_goal.clone(),
                project_id: a.clone(),
                binding_id: Some(binding.clone()),
            },
        )
        .unwrap();
    let mut assignment = state.goals[0].assignments[0].clone();
    assignment.id = "creating-assignment".into();
    assignment.session_id = None;
    assignment.submitted = false;
    state.goals.last_mut().unwrap().assignments.push(assignment);
    state.attempts.insert(
        "create:creating-assignment".into(),
        AssistantAttempt {
            state: "started".into(),
            outcome: String::new(),
        },
    );
    state = store.save_assistant(state.revision, state).unwrap();
    let taken_over = store
        .edit_assistant(
            state.revision,
            AssistantEdit::Takeover {
                id: creating_goal.clone(),
            },
        )
        .unwrap();
    assert!(writer_unresolved(
        &taken_over,
        taken_over.goals.last().unwrap()
    ));
    assert!(store
        .edit_hierarchy(
            taken_over.revision,
            HierarchyEdit::TransferGoal {
                goal_id: creating_goal,
                project_id: a,
                binding_id: Some(binding)
            }
        )
        .is_err());
    for terminal in [None, Some("cancelled"), Some("failed")] {
        let mut forged = taken_over.clone();
        if let Some(state) = terminal {
            forged
                .attempts
                .get_mut("create:creating-assignment")
                .unwrap()
                .state = state.into();
        } else {
            forged.attempts.remove("create:creating-assignment");
        }
        assert!(
            store.save_assistant(taken_over.revision, forged).is_err(),
            "an unresolved create claim needs a real receipt before removal or terminal changes"
        );
    }
}

// ---- M2 Two-Tier Coordination & Routing Tests ---------------------------------------------

#[test]
fn legacy_scope_fingerprint_and_say_hash_are_byte_equivalent() {
    let store = Store::open_in_memory().unwrap();
    let state = setup(&store);
    assert!(state.hierarchy.is_none());

    // 1. turn_args_hash with actor=None is byte-for-byte identical to args_hash
    let h1 = args_hash("turn-1", "hello", &["/repo-a1".into()], Some("q-1"));
    let h2 = turn_args_hash("turn-1", "hello", &["/repo-a1".into()], Some("q-1"), None);
    assert_eq!(h1, h2, "turn_args_hash with None actor must be byte-for-byte identical to args_hash");

    // 2. turn_args_hash with actor=Some differs from None
    let h3 = turn_args_hash("turn-1", "hello", &["/repo-a1".into()], Some("q-1"), Some("chief"));
    assert_ne!(h1, h3, "actor=Some must change hash to prevent forgery across actors");
}

#[test]
fn two_tier_review_routing_and_host_action_authorization() {
    let store = Store::open_in_memory().unwrap();
    let initial = setup(&store);
    let goal_id = initial.goals[0].id.clone();
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    let binding_a = bind(&store, &a, "/repo-a1");
    let b = project(&store, "B");
    let _binding_b = bind(&store, &b, "/repo-b");

    edit(
        &store,
        HierarchyEdit::AssignGoal {
            goal_id: goal_id.clone(),
            project_id: a.clone(),
            binding_id: Some(binding_a.clone()),
        },
    );

    let state = store.assistant_state().unwrap();
    let chief = HostActor::chief();
    let manager_a = HostActor::project(&state, &a).unwrap();
    let manager_b = HostActor::project(&state, &b).unwrap();
    let owner_epoch = state.hierarchy.as_ref().unwrap().goal_owners[&goal_id].epoch;

    // 1. Chief is denied Dispatch to a project-managed goal
    let chief_dispatch = authorize(&state, &chief, ManagerAction::Dispatch, Some(&goal_id), Some(owner_epoch));
    assert!(chief_dispatch.is_err(), "Chief cannot dispatch to a project-managed goal");

    // 2. Project Manager A is authorized to Dispatch its own goal
    let mgr_a_dispatch = authorize(&state, &manager_a, ManagerAction::Dispatch, Some(&goal_id), Some(owner_epoch));
    assert!(mgr_a_dispatch.is_ok(), "Manager A must be authorized to dispatch its own goal");

    // 3. Project Manager B is denied Dispatch to Project A's goal
    let mgr_b_dispatch = authorize(&state, &manager_b, ManagerAction::Dispatch, Some(&goal_id), Some(owner_epoch));
    assert!(mgr_b_dispatch.is_err(), "Manager B cannot dispatch Project A's goal");

    // 4. Stale epoch is rejected for Manager A
    let stale_epoch_dispatch = authorize(&state, &manager_a, ManagerAction::Dispatch, Some(&goal_id), Some(owner_epoch - 1));
    assert!(stale_epoch_dispatch.is_err(), "Stale epoch must be rejected");

    // 5. Manager A cannot RouteProject or ProposeCrossProject
    assert!(authorize(&state, &manager_a, ManagerAction::RouteProject, None, None).is_err());
    assert!(authorize(&state, &manager_a, ManagerAction::ProposeCrossProject, None, None).is_err());
}

#[test]
fn intake_isolation_and_boundary_protection() {
    let store = Store::open_in_memory().unwrap();
    setup(&store);
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    let binding_a = bind(&store, &a, "/repo-a1");
    let b = project(&store, "B");
    bind(&store, &b, "/repo-b");

    // Create a goal in project A
    let mut state = store.assistant_state().unwrap();
    let goal_id = "goal-proj-a".to_string();
    state.goals.push(AssistantGoal {
        id: goal_id.clone(),
        project_path: "/repo-a1".into(),
        title: "Feature A".into(),
        acceptance: "Pass tests".into(),
        priority: 1,
        status: GoalStatus::Active,
        next_step: String::new(),
        blocker: String::new(),
        assignments: vec![],
        verdict: None,
        contract_revision: 1,
        dependencies: vec![],
    });
    assign_goal(&mut state, &goal_id, &a, Some(&binding_a)).unwrap();
    store.save_assistant(state.revision, state).unwrap();
    with_writer(&store, &goal_id, "session-writer-a");
    let state = store.assistant_state().unwrap();

    // 1. Chief intake message targeting project A goal is rejected
    let mut chief_state = state.clone();
    chief_state.conversation.push(ConversationTurn {
        id: "turn-chief-1".into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        author: TurnAuthor::User,
        content: "Chief directive: update goal".into(),
        reply_to: None,
        project_paths: vec!["/repo-a1".into()],
        status: TurnStatus::Reviewing,
        goal_ids: vec![goal_id.clone()],
        question_ids: vec![],
        error: None,
        source_run_id: Some("run-chief-1".into()),
        actor: Some("chief".into()),
    });
    let stamps_chief = turn_stamps(&chief_state, chief_state.conversation.last().unwrap());
    chief_state.intake_stamps.insert("run-chief-1".into(), stamps_chief);
    let decision = IntakeDecision {
        summary: "Chief attempting message".into(),
        actions: vec![IntakeAction::Message {
            goal_id: goal_id.clone(),
            content: "Chief overriding".into(),
            quote: "Chief directive: update goal".into(),
        }],
    };
    assert!(
        apply_intake(&chief_state, &[], "run-chief-1", decision).is_err(),
        "Chief intake message to a project-managed goal must be rejected"
    );

    // 2. Project Manager B intake message targeting Project A goal is rejected
    let mut mgr_b_state = state.clone();
    mgr_b_state.conversation.push(ConversationTurn {
        id: "turn-mgr-b-1".into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        author: TurnAuthor::User,
        content: "Manager B directive: update goal".into(),
        reply_to: None,
        project_paths: vec!["/repo-a1".into()],
        status: TurnStatus::Reviewing,
        goal_ids: vec![goal_id.clone()],
        question_ids: vec![],
        error: None,
        source_run_id: Some("run-mgr-b-1".into()),
        actor: Some(format!("project:{b}")),
    });
    let stamps_b = turn_stamps(&mgr_b_state, mgr_b_state.conversation.last().unwrap());
    mgr_b_state.intake_stamps.insert("run-mgr-b-1".into(), stamps_b);
    let decision_b = IntakeDecision {
        summary: "Manager B attempting message".into(),
        actions: vec![IntakeAction::Message {
            goal_id: goal_id.clone(),
            content: "Manager B invading".into(),
            quote: "Manager B directive: update goal".into(),
        }],
    };
    assert!(
        apply_intake(&mgr_b_state, &[], "run-mgr-b-1", decision_b).is_err(),
        "Manager B intake message to Project A's goal must be rejected"
    );

    // 3. Project Manager A intake message targeting Project A goal succeeds
    let mut mgr_a_state = state.clone();
    mgr_a_state.conversation.push(ConversationTurn {
        id: "turn-mgr-a-1".into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        author: TurnAuthor::User,
        content: "Manager A directive: update goal".into(),
        reply_to: None,
        project_paths: vec!["/repo-a1".into()],
        status: TurnStatus::Reviewing,
        goal_ids: vec![goal_id.clone()],
        question_ids: vec![],
        error: None,
        source_run_id: Some("run-mgr-a-1".into()),
        actor: Some(format!("project:{a}")),
    });
    let stamps_a = turn_stamps(&mgr_a_state, mgr_a_state.conversation.last().unwrap());
    mgr_a_state.intake_stamps.insert("run-mgr-a-1".into(), stamps_a);
    let decision_a = IntakeDecision {
        summary: "Manager A sending message".into(),
        actions: vec![IntakeAction::Message {
            goal_id: goal_id.clone(),
            content: "Manager A updating goal".into(),
            quote: "Manager A directive: update goal".into(),
        }],
    };
    assert!(apply_intake(&mgr_a_state, &[], "run-mgr-a-1", decision_a).is_ok());
}

#[test]
fn say_idempotency_with_actor_rejects_mismatched_actor() {
    let store = Store::open_in_memory().unwrap();
    setup(&store);
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    bind(&store, &a, "/repo-a1");

    let s1 = store
        .edit_assistant(
            rev(&store),
            AssistantEdit::Say {
                turn_id: "say-1".into(),
                content: "Hello from chief".into(),
                project_paths: vec!["/repo-a1".into()],
                answers_question: None,
                actor: Some("chief".into()),
            },
        )
        .unwrap();

    // Replay with exact same actor succeeds (idempotent replay)
    let s2 = store
        .edit_assistant(
            s1.revision,
            AssistantEdit::Say {
                turn_id: "say-1".into(),
                content: "Hello from chief".into(),
                project_paths: vec!["/repo-a1".into()],
                answers_question: None,
                actor: Some("chief".into()),
            },
        )
        .unwrap();
    assert_eq!(s1.revision, s2.revision);

    // Replay with different actor (e.g. project manager) is rejected!
    let s_err = store.edit_assistant(
        s2.revision,
        AssistantEdit::Say {
            turn_id: "say-1".into(),
            content: "Hello from chief".into(),
            project_paths: vec!["/repo-a1".into()],
            answers_question: None,
            actor: Some(format!("project:{a}")),
        },
    );
    assert!(s_err.is_err(), "Replaying same turn_id with different actor must be rejected");

    // Invalid actor format is rejected
    let s_invalid = store.edit_assistant(
        s2.revision,
        AssistantEdit::Say {
            turn_id: "say-2".into(),
            content: "Hello from unknown".into(),
            project_paths: vec!["/repo-a1".into()],
            answers_question: None,
            actor: Some("not-valid-actor".into()),
        },
    );
    assert!(s_invalid.is_err(), "Invalid actor format must be rejected");
}

fn create_test_session(store: &Store, id: &str, project_path: &str) {
    let mut session = codetwo_core::session::Session::new(ProviderId::Codex, project_path);
    session.id = id.into();
    store.upsert_session(&session).unwrap();
}
#[test]
fn executor_context_isolates_memory_without_leaking_global() {
    let store = Store::open_in_memory().unwrap();
    create_test_session(&store, "session-exec-1", "/repo-a1");
    let initial = setup(&store);
    let goal_id = initial.goals[0].id.clone();
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    let binding_a = bind(&store, &a, "/repo-a1");

    edit(
        &store,
        HierarchyEdit::AssignGoal {
            goal_id: goal_id.clone(),
            project_id: a.clone(),
            binding_id: Some(binding_a.clone()),
        },
    );

    // Store private global memory and project A memory
    store.add_memory(GLOBAL_MEMORY, "preference", "TOP_SECRET_CHIEF_ONLY", false).unwrap();
    store.add_memory("/repo-a1", "constraint", "PROJECT_A_CONFIG", true).unwrap();

    // Set worker assignment
    with_writer(&store, &goal_id, "session-exec-1");
    let state = store.assistant_state().unwrap();
    let goal = state.goals.iter().find(|g| g.id == goal_id).unwrap();
    let _assignment = goal.assignments.last().unwrap();

    // Call assistant_worker_command with WorkerOperation::Context
    let context_val = store
        .assistant_worker_command(
            "session-exec-1",
            "cmd-1",
            WorkerOperation::Context,
        )
        .unwrap();

    let mem_str = context_val["memory"].as_str().unwrap_or_default();
    let shared_mem_str = context_val["shared_memory"].as_str().unwrap_or_default();

    // Assert that project memory is present
    assert!(mem_str.contains("PROJECT_A_CONFIG"), "Project memory must be recalled");
    // Assert that GLOBAL_MEMORY was NOT leaked to executor!
    assert!(!mem_str.contains("TOP_SECRET_CHIEF_ONLY"), "Global memory must not leak into memory block");
    assert!(!shared_mem_str.contains("TOP_SECRET_CHIEF_ONLY"), "Global memory must not leak into shared block");
    assert!(shared_mem_str.is_empty(), "Executor shared_memory should be empty when global is isolated");
}

#[test]
fn scope_fingerprint_isolates_unrelated_project_modifications() {
    let store = Store::open_in_memory().unwrap();
    let initial = setup(&store);
    let goal_id = initial.goals[0].id.clone();
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    let binding_a = bind(&store, &a, "/repo-a1");
    let b = project(&store, "B");
    let _binding_b = bind(&store, &b, "/repo-b");

    edit(
        &store,
        HierarchyEdit::AssignGoal {
            goal_id: goal_id.clone(),
            project_id: a.clone(),
            binding_id: Some(binding_a.clone()),
        },
    );

    // Create session for goal
    create_test_session(&store, "session-fp-1", "/repo-a1");

    // Fingerprint helper
    let snapshot_a = |store: &Store| {
        let state = store.assistant_state().unwrap();
        let _sessions = store.list_sessions().unwrap();
        let _memories = store.list_memories("/repo-a1", 100).unwrap();
        let settings = state.settings.clone().unwrap();
        // Compute facts exactly as scope_fingerprint does
        let mut hierarchy_facts = None;
        if let Some(h) = state.hierarchy.as_ref() {
            if let Some(goal) = state.goals.first() {
                if let Some(owner) = h.goal_owners.get(&goal.id) {
                    let p_version = h.project(&owner.project_id).map(|p| p.version).unwrap_or(0);
                    let b_version = h
                        .project(&owner.project_id)
                        .ok()
                        .and_then(|p| p.bindings.iter().find(|b| b.id == owner.binding_id))
                        .map(|b| b.version)
                        .unwrap_or(0);
                    let inst = h
                        .active_instruction(&HierarchyScope::Project(owner.project_id.clone()))
                        .map(|i| (i.hash.clone(), i.revision));
                    let shares: Vec<_> = h
                        .shares
                        .iter()
                        .filter(|s| {
                            s.target == HierarchyScope::Project(owner.project_id.clone())
                                && s.state == ShareState::Approved
                        })
                        .map(|s| (s.id.clone(), s.content_hash.clone()))
                        .collect();
                    hierarchy_facts = Some(serde_json::json!([
                        owner.epoch,
                        p_version,
                        b_version,
                        inst,
                        shares,
                    ]));
                }
            }
        }
        let facts = serde_json::json!([
            state.goals,
            state.requests,
            state.messages,
            settings,
            hierarchy_facts
        ]);
        blake3::hash(facts.to_string().as_bytes()).to_hex().to_string()
    };

    let fp_before = snapshot_a(&store);

    // Edit Project B (unrelated project instruction)
    let mgr_b = HostActor::project(&store.assistant_state().unwrap(), &b).unwrap();
    let (_, id_b) = store
        .propose_instruction_as(
            rev(&store),
            &mgr_b,
            HierarchyScope::Project(b.clone()),
            "Project B guideline",
            Reach::Executor,
        )
        .unwrap();
    let hash_b = instruction_hash("Project B guideline");
    edit(
        &store,
        HierarchyEdit::ConfirmInstruction {
            proposal_id: id_b,
            text_hash: hash_b,
        },
    );

    let fp_after_b_edit = snapshot_a(&store);
    assert_eq!(
        fp_before, fp_after_b_edit,
        "Modifying Project B instructions must NOT alter Project A scope fingerprint!"
    );

    // Edit Project A (related project instruction)
    let mgr_a = HostActor::project(&store.assistant_state().unwrap(), &a).unwrap();
    let (_, id_a) = store
        .propose_instruction_as(
            rev(&store),
            &mgr_a,
            HierarchyScope::Project(a.clone()),
            "Project A guideline",
            Reach::Executor,
        )
        .unwrap();
    let hash_a = instruction_hash("Project A guideline");
    edit(
        &store,
        HierarchyEdit::ConfirmInstruction {
            proposal_id: id_a,
            text_hash: hash_a,
        },
    );

    let fp_after_a_edit = snapshot_a(&store);
    assert_ne!(
        fp_before, fp_after_a_edit,
        "Modifying Project A instructions MUST alter Project A scope fingerprint!"
    );
}

#[test]
fn intake_route_action_creates_project_request_idempotently() {
    let store = Store::open_in_memory().unwrap();
    setup(&store);
    edit(&store, HierarchyEdit::Enable {});
    let a = project(&store, "A");
    bind(&store, &a, "/repo-a1");

    let mut state = store.assistant_state().unwrap();
    let turn_id = "turn-route-user".to_string();
    state.conversation.push(ConversationTurn {
        id: turn_id.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        author: TurnAuthor::User,
        content: "Please build feature in Project A".into(),
        reply_to: None,
        project_paths: vec!["/repo-a1".into()],
        status: TurnStatus::Reviewing,
        goal_ids: vec![],
        question_ids: vec![],
        error: None,
        source_run_id: Some("run-route-1".into()),
        actor: Some("chief".into()),
    });
    let stamps = turn_stamps(&state, state.conversation.last().unwrap());
    state.intake_stamps.insert("run-route-1".into(), stamps);

    let route_action = IntakeAction::Route {
        project_path: "/repo-a1".into(),
        content: "Build feature A".into(),
    };
    let decision = IntakeDecision {
        summary: "Routing to project A".into(),
        actions: vec![route_action.clone()],
    };

    let next = apply_intake(&state, &[], "run-route-1", decision.clone()).unwrap();
    assert_eq!(next.requests.len(), 1);
    let req = &next.requests[0];
    assert_eq!(req.managed_project_id.as_deref(), Some(a.as_str()));
    assert_eq!(req.parent_request_id.as_deref(), Some("turn-route-user"));
    assert_eq!(req.content, "Build feature A");

    // Duplicate route to same project within the same intake is rejected
    let dup_decision = IntakeDecision {
        summary: "Routing twice to project A".into(),
        actions: vec![route_action.clone(), route_action],
    };
    assert!(apply_intake(&state, &[], "run-route-1", dup_decision).is_err(), "Duplicate route to the same project must be rejected");
}

#[test]
fn atlas_and_beacon_pilot_practice_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("pilot_assistant.db");
    let store = Store::open(db_path.to_str().unwrap()).unwrap();

    let atlas_api = "/atlas-api";
    let atlas_web = "/atlas-web";
    let beacon_report = "/beacon-report";

    for (p, n) in [
        (atlas_api, "Atlas API"),
        (atlas_web, "Atlas Web"),
        (beacon_report, "Beacon Report"),
    ] {
        store.add_project(p, Some(n), 0).unwrap();
    }

    let initial_state = store
        .edit_assistant(
            0,
            AssistantEdit::Settings {
                settings: AssistantSettings {
                    enabled: true,
                    projects: vec![atlas_api.into(), atlas_web.into(), beacon_report.into()],
                    provider: ProviderId::Codex,
                    model: None,
                    reasoning_effort: None,
                    concurrency: 2,
                    turn_limit: 10,
                    dispatch_limit: 5,
                },
            },
        )
        .unwrap();

    // 0. Enable hierarchy and register pilot projects
    let _state = store
        .edit_hierarchy(initial_state.revision, HierarchyEdit::Enable {})
        .unwrap();
    let atlas_id = project(&store, "Atlas");
    let beacon_id = project(&store, "Beacon");

    let atlas_bind_api = bind(&store, &atlas_id, atlas_api);
    let atlas_bind_web = bind(&store, &atlas_id, atlas_web);
    let beacon_bind_rep = bind(&store, &beacon_id, beacon_report);

    // 1. Step 1: User asks Global Chief about overall progress ("问整体进展")
    let s_rev = rev(&store);
    let state = store
        .edit_assistant(
            s_rev,
            AssistantEdit::Say {
                turn_id: "turn-progress-1".into(),
                content: "问整体进展".into(),
                project_paths: vec![],
                answers_question: None,
                actor: Some("chief".into()),
            },
        )
        .unwrap();
    assert_eq!(state.conversation.last().unwrap().actor.as_deref(), Some("chief"));

    // 2. Step 2: Route two-repo task to Atlas ("A 内两仓库交办")
    let mut state = store.assistant_state().unwrap();
    let turn_atlas = "turn-atlas-task".to_string();
    state.conversation.push(ConversationTurn {
        id: turn_atlas.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        author: TurnAuthor::User,
        content: "Deploy Atlas API Gateway and Web UI".into(),
        reply_to: None,
        project_paths: vec![atlas_api.into(), atlas_web.into()],
        status: TurnStatus::Reviewing,
        goal_ids: vec![],
        question_ids: vec![],
        error: None,
        source_run_id: Some("run-atlas-route".into()),
        actor: Some("chief".into()),
    });
    let stamps_atlas = turn_stamps(&state, state.conversation.last().unwrap());
    state.intake_stamps.insert("run-atlas-route".into(), stamps_atlas);

    let route_atlas_action = IntakeAction::Route {
        project_path: atlas_api.into(),
        content: "Implement API Gateway".into(),
    };
    let decision_atlas = IntakeDecision {
        summary: "Routing to Atlas".into(),
        actions: vec![route_atlas_action],
    };
    let state = apply_intake(&state, &[], "run-atlas-route", decision_atlas).unwrap();
    store.save_assistant(state.revision, state).unwrap();

    // Atlas manager creates goals in both atlas_api and atlas_web
    let state = store.assistant_state().unwrap();
    let goal_api_id = "goal-atlas-api-1".to_string();
    let goal_web_id = "goal-atlas-web-1".to_string();

    let mut state = state;
    state.goals.push(AssistantGoal {
        id: goal_api_id.clone(),
        project_path: atlas_api.into(),
        title: "Atlas API Gateway".into(),
        acceptance: "Gateway returns 200".into(),
        priority: 1,
        status: GoalStatus::Active,
        next_step: "Implement endpoints".into(),
        blocker: String::new(),
        assignments: vec![],
        verdict: None,
        contract_revision: 1,
        dependencies: vec![],
    });
    state.goals.push(AssistantGoal {
        id: goal_web_id.clone(),
        project_path: atlas_web.into(),
        title: "Atlas Web Dashboard".into(),
        acceptance: "Dashboard loads".into(),
        priority: 1,
        status: GoalStatus::Active,
        next_step: "Connect to gateway".into(),
        blocker: String::new(),
        assignments: vec![],
        verdict: None,
        contract_revision: 1,
        dependencies: vec![goal_api_id.clone()],
    });
    assign_goal(&mut state, &goal_api_id, &atlas_id, Some(&atlas_bind_api)).unwrap();
    assign_goal(&mut state, &goal_web_id, &atlas_id, Some(&atlas_bind_web)).unwrap();
    let state = store.save_assistant(state.revision, state).unwrap();

    let owners = &state.hierarchy.as_ref().unwrap().goal_owners;
    assert_eq!(owners[&goal_api_id].project_id, atlas_id);
    assert_eq!(owners[&goal_api_id].binding_id, atlas_bind_api);
    assert_eq!(owners[&goal_api_id].epoch, 1);
    assert_eq!(owners[&goal_web_id].project_id, atlas_id);
    assert_eq!(owners[&goal_web_id].binding_id, atlas_bind_web);
    assert_eq!(owners[&goal_web_id].epoch, 1);

    // 3. Step 3: Route parallel task to Beacon ("B 并行交办")
    let goal_beacon_id = "goal-beacon-1".to_string();
    let mut state = store.assistant_state().unwrap();
    state.goals.push(AssistantGoal {
        id: goal_beacon_id.clone(),
        project_path: beacon_report.into(),
        title: "Beacon Monthly Report".into(),
        acceptance: "Generate PDF".into(),
        priority: 1,
        status: GoalStatus::Active,
        next_step: "Fetch statistics".into(),
        blocker: String::new(),
        assignments: vec![],
        verdict: None,
        contract_revision: 1,
        dependencies: vec![],
    });
    assign_goal(&mut state, &goal_beacon_id, &beacon_id, Some(&beacon_bind_rep)).unwrap();
    let state = store.save_assistant(state.revision, state).unwrap();
    assert_eq!(state.hierarchy.as_ref().unwrap().goal_owners[&goal_beacon_id].project_id, beacon_id);

    // 4. Step 4: Atlas execution question / requirements change / stop ("A 执行中提问/改需求/停止")
    // Atlas executor encounters a question
    let mut state = store.assistant_state().unwrap();
    state.questions.push(CoordinationQuestion {
        created_at: chrono::Utc::now().to_rfc3339(),
        id: "q-auth-standard".into(),
        goal_id: goal_api_id.clone(),
        assignment_id: "assignment-web-1".into(),
        contract_revision: 1,
        author: "Atlas Executor".into(),
        title: "Which authentication standard?".into(),
        context: "JWT Bearer or Session Cookie?".into(),
        options: vec!["JWT Bearer".into(), "Session Cookie".into()],
        blocking: true,
        state: "open".into(),
        answer: None,
        answered_by: None,
        source_input: None,
        request_id: None,
        answer_delivery_id: None,
        factual: false,
    });
    // User updates goal requirements for Atlas API
    let g_api = state.goals.iter_mut().find(|g| g.id == goal_api_id).unwrap();
    g_api.acceptance = "Gateway returns 200 with JWT authentication".into();
    g_api.contract_revision += 1;
    // User stops Atlas web goal
    let g_web = state.goals.iter_mut().find(|g| g.id == goal_web_id).unwrap();
    g_web.status = GoalStatus::NeedsAttention;
    g_web.assignments.push(Assignment {
        id: "assignment-web-1".into(),
        instruction: "Build dashboard".into(),
        session_id: Some("session-web-1".into()),
        submitted: true,
        taken_over: false,
        owned: true,
        contract_revision: 1,
        confirmed_revision: 1,
        protocol: 0,
        stop_requested: true,
        stop_sent: false,
        result: None,
        inputs: vec![],
        pending_inputs: None,
        prior_results: vec![],
    });
    let state = store.save_assistant(state.revision, state).unwrap();

    // Beacon goal is completely unaffected
    let beacon_goal = state.goals.iter().find(|g| g.id == goal_beacon_id).unwrap();
    assert_eq!(beacon_goal.status, GoalStatus::Active);
    assert_eq!(state.hierarchy.as_ref().unwrap().goal_owners[&goal_beacon_id].epoch, 1);

    // 5. Step 5: Restart & Recovery ("重启恢复")
    drop(store);
    let store_reloaded = Store::open(db_path.to_str().unwrap()).unwrap();
    let reloaded_state = store_reloaded.assistant_state().unwrap();

    let h = reloaded_state.hierarchy.as_ref().unwrap();
    assert_eq!(h.projects.len(), 2);
    assert_eq!(h.project(&atlas_id).unwrap().bindings.len(), 2);
    assert_eq!(h.project(&beacon_id).unwrap().bindings.len(), 1);
    assert_eq!(h.goal_owners[&goal_api_id].epoch, 1);
    assert_eq!(h.goal_owners[&goal_beacon_id].epoch, 1);
    assert_eq!(reloaded_state.questions.len(), 1);

    // 6. Step 6: Two-tier Memory and Instruction receipt verification ("检查实际两层 Memory/instruction receipt")
    // Propose & confirm Global instruction
    let chief = HostActor::chief();
    let (_, prop_global) = store_reloaded
        .propose_instruction_as(
            reloaded_state.revision,
            &chief,
            HierarchyScope::Global,
            "Enterprise Policy: strict typing required.",
            Reach::Chief,
        )
        .unwrap();
    let hash_global = instruction_hash("Enterprise Policy: strict typing required.");
    let state = store_reloaded
        .edit_hierarchy(
            rev(&store_reloaded),
            HierarchyEdit::ConfirmInstruction {
                proposal_id: prop_global,
                text_hash: hash_global.clone(),
            },
        )
        .unwrap();

    // Propose & confirm Atlas instruction
    let mgr_atlas = HostActor::project(&state, &atlas_id).unwrap();
    let (_, prop_atlas) = store_reloaded
        .propose_instruction_as(
            state.revision,
            &mgr_atlas,
            HierarchyScope::Project(atlas_id.clone()),
            "Atlas Rule: all endpoints must be RESTful JSON.",
            Reach::ProjectManager,
        )
        .unwrap();
    let hash_atlas = instruction_hash("Atlas Rule: all endpoints must be RESTful JSON.");
    let state = store_reloaded
        .edit_hierarchy(
            rev(&store_reloaded),
            HierarchyEdit::ConfirmInstruction {
                proposal_id: prop_atlas,
                text_hash: hash_atlas.clone(),
            },
        )
        .unwrap();

    // Propose & confirm Beacon instruction
    let mgr_beacon = HostActor::project(&state, &beacon_id).unwrap();
    let (_, prop_beacon) = store_reloaded
        .propose_instruction_as(
            state.revision,
            &mgr_beacon,
            HierarchyScope::Project(beacon_id.clone()),
            "Beacon Rule: reports must be formatted in Markdown.",
            Reach::ProjectManager,
        )
        .unwrap();
    let hash_beacon = instruction_hash("Beacon Rule: reports must be formatted in Markdown.");
    let state = store_reloaded
        .edit_hierarchy(
            rev(&store_reloaded),
            HierarchyEdit::ConfirmInstruction {
                proposal_id: prop_beacon,
                text_hash: hash_beacon.clone(),
            },
        )
        .unwrap();

    // Check active instructions receipts
    let h = state.hierarchy.as_ref().unwrap();
    let active_global = h.active_instruction(&HierarchyScope::Global).unwrap();
    assert_eq!(active_global.hash, hash_global);
    assert_eq!(active_global.reach, Reach::Chief);

    let active_atlas = h.active_instruction(&HierarchyScope::Project(atlas_id.clone())).unwrap();
    assert_eq!(active_atlas.hash, hash_atlas);
    assert_eq!(active_atlas.reach, Reach::ProjectManager);

    let active_beacon = h.active_instruction(&HierarchyScope::Project(beacon_id.clone())).unwrap();
    assert_eq!(active_beacon.hash, hash_beacon);
    assert_eq!(active_beacon.reach, Reach::ProjectManager);

    // Cross-project memory share proposal and approval
    let mem = store_reloaded
        .add_memory(
            GLOBAL_MEMORY,
            "constraint",
            "Global OAuth client credentials",
            false,
        )
        .unwrap();
    let (_, share_id) = store_reloaded
        .propose_memory_share_as(
            rev(&store_reloaded),
            &chief,
            &mem.id,
            HierarchyScope::Project(atlas_id.clone()),
            Reach::ProjectManager,
        )
        .unwrap();
    let share_hash = store_reloaded.memory_share_hash(&mem.id).unwrap();
    let state = store_reloaded
        .edit_hierarchy(
            rev(&store_reloaded),
            HierarchyEdit::ConfirmShare {
                share_id: share_id.clone(),
                content_hash: share_hash,
            },
        )
        .unwrap();

    // Verify Atlas manager can read the shared memory
    let grant_atlas = memory_grant(&state, &mgr_atlas).unwrap();
    let atlas_mems = store_reloaded.scoped_memory_read(&grant_atlas, 10).unwrap();
    assert!(atlas_mems.iter().any(|m| m.record.content.contains("Global OAuth client credentials")));

    // Verify Beacon manager CANNOT read the memory shared only to Atlas
    let grant_beacon = memory_grant(&state, &mgr_beacon).unwrap();
    let beacon_mems = store_reloaded.scoped_memory_read(&grant_beacon, 10).unwrap();
    assert!(!beacon_mems.iter().any(|m| m.record.content.contains("Global OAuth client credentials")));
}

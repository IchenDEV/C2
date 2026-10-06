use codetwo_core::{assistant::*, assistant_observation::*, provider::ProviderId, Store};

fn principal() -> Principal {
    Principal {
        plugin: "bundle:chat".into(),
        realm: "global".into(),
        connector_id: "chat".into(),
    }
}

fn event(id: &str, content: &str) -> ObservationEvent {
    ObservationEvent {
        event_id: id.into(),
        kind: "message.created".into(),
        occurred_at: "2026-10-06T00:00:00+08:00".into(),
        actor_id: "u1".into(),
        resource_id: "room".into(),
        object_id: format!("m-{id}"),
        object_version: None,
        reply_to: None,
        content: Some(content.into()),
        content_origin: ContentOrigin::Event,
        reference: None,
    }
}

fn request(
    batch: &str,
    before: Option<&str>,
    after: Option<&str>,
    events: Vec<ObservationEvent>,
) -> RecordRequest {
    RecordRequest {
        connector_id: "chat".into(),
        account_scope: "acct-1".into(),
        stream_id: "s1".into(),
        batch_id: batch.into(),
        recovery: Recovery::Resumable,
        checkpoint_before: before.map(Into::into),
        checkpoint_after: after.map(Into::into),
        events,
        reset: None,
        cursor_invalid: None,
    }
}

/// Settings + two goals + one enabled binding fanning out to both goals.
fn bound(store: &Store) -> (AssistantState, SourceBinding) {
    store.add_project("/project-a", Some("A"), 0).unwrap();
    store.add_project("/project-b", Some("B"), 0).unwrap();
    let mut state = store
        .edit_assistant(
            0,
            AssistantEdit::Settings {
                settings: AssistantSettings {
                    enabled: true,
                    projects: vec!["/project-a".into()],
                    provider: ProviderId::Codex,
                    model: None,
                    reasoning_effort: None,
                    concurrency: 1,
                    turn_limit: 5,
                    dispatch_limit: 2,
                },
            },
        )
        .unwrap();
    for title in ["One", "Two"] {
        state = store
            .edit_assistant(
                state.revision,
                AssistantEdit::Goal {
                    id: None,
                    project_path: "/project-a".into(),
                    title: title.into(),
                    acceptance: "done".into(),
                    priority: 1,
                },
            )
            .unwrap();
    }
    let goal_ids = state.goals.iter().map(|g| g.id.clone()).collect();
    let state = store
        .edit_assistant(
            state.revision,
            AssistantEdit::Source {
                binding: input(goal_ids, 0, true, vec!["/project-a".into()]),
            },
        )
        .unwrap();
    let binding = state.sources[0].clone();
    (state, binding)
}

fn input(
    goal_ids: Vec<String>,
    version: u64,
    enabled: bool,
    projects: Vec<String>,
) -> SourceBindingInput {
    SourceBindingInput {
        source_id: None,
        expected_version: version,
        principal: principal(),
        provider: "chat".into(),
        account_scope: "acct-1".into(),
        resource_filter: vec!["room".into()],
        actor_filter: vec![],
        project_paths: projects,
        goal_ids,
        streams: vec!["s1".into()],
        enabled,
    }
}

#[test]
fn canonical_hash_is_a_frozen_vector_independent_of_defaults() {
    let mut e = event("e1", "hi");
    e.actor_id = "u1".into();
    e.resource_id = "r1".into();
    e.object_id = "m1".into();
    assert_eq!(
        e.canonical_json(),
        r#"{"event_id":"e1","kind":"message.created","occurred_at":"2026-10-06T00:00:00+08:00","actor_id":"u1","resource_id":"r1","object_id":"m1","object_version":null,"reply_to":null,"content":"hi","content_origin":"event","reference":null}"#
    );
    assert_eq!(
        e.content_hash(),
        "91719c3b1a2358df89b7f056046d0d619ec67a6add22376120b5f17e4899f901"
    );
}

#[test]
fn unbound_traffic_is_counted_not_stored_and_never_bumps_revision() {
    let store = Store::open_in_memory().unwrap();
    let before = store.assistant_state().unwrap().revision;
    for i in 0..140 {
        let r = store
            .observation_record(
                &principal(),
                request(
                    &format!("b{i}"),
                    None,
                    None,
                    vec![event(&format!("e{i}"), "secret body")],
                ),
            )
            .unwrap();
        assert_eq!(r.status, RecordStatus::Rejected);
        assert_eq!(r.reason.as_deref(), Some("needs_binding"));
    }
    let health = store.observation_health().unwrap();
    assert_eq!(
        health.len(),
        1,
        "unbound traffic aggregates per principal, not per stream/account string"
    );
    assert_eq!(health[0].unbound, 140);
    assert_eq!(health[0].state, "needs_binding");
    assert!(store.observation_pending(10).unwrap().is_empty());
    assert_eq!(store.assistant_state().unwrap().revision, before);
}

#[test]
fn record_dedup_conflict_cas_replay_and_fanout() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("o.db");
    let source;
    let revision;
    {
        let store = Store::open(path.to_str().unwrap()).unwrap();
        let (state, binding) = bound(&store);
        source = binding.source_id.clone();
        revision = state.revision;

        // A foreign payload connector cannot ride on this principal.
        let mut forged = request("bx", None, Some("c1"), vec![event("e1", "x")]);
        forged.connector_id = "other".into();
        assert_eq!(
            store
                .observation_record(&principal(), forged)
                .unwrap()
                .reason
                .as_deref(),
            Some("principal_mismatch")
        );
        let mut other_acct = request("by", None, Some("c1"), vec![event("e1", "x")]);
        other_acct.account_scope = "acct-2".into();
        assert_eq!(
            store
                .observation_record(&principal(), other_acct)
                .unwrap()
                .reason
                .as_deref(),
            Some("needs_binding")
        );
        let mut stream = request("bz", None, Some("c1"), vec![event("e1", "x")]);
        stream.stream_id = "nope".into();
        assert_eq!(
            store
                .observation_record(&principal(), stream)
                .unwrap()
                .reason
                .as_deref(),
            Some("unknown_stream")
        );

        // Recorded once; body stored once; two independent per-goal receipts.
        let first = store
            .observation_record(
                &principal(),
                request(
                    "b1",
                    None,
                    Some("c1"),
                    vec![event("e1", "hello"), {
                        let mut f = event("e2", "ignored");
                        f.resource_id = "other-room".into();
                        f
                    }],
                ),
            )
            .unwrap();
        assert_eq!(first.status, RecordStatus::Recorded);
        assert_eq!(first.current_checkpoint.as_deref(), Some("c1"));
        assert_eq!(first.event_receipts[0].goal_ids.len(), 2);
        assert_eq!(first.event_receipts[1].status, "filtered");
        let pending = store.observation_pending(10).unwrap();
        assert_eq!(pending.len(), 2);
        assert!(pending
            .iter()
            .all(|t| t.state == "recorded" && t.binding_version == 1));

        // Same batch id replays the stored result after the cursor moved; changed args conflict.
        let second = store
            .observation_record(
                &principal(),
                request("b2", Some("c1"), Some("c2"), vec![event("e3", "x")]),
            )
            .unwrap();
        assert_eq!(second.status, RecordStatus::Recorded);
        let replay = store
            .observation_record(
                &principal(),
                request(
                    "b1",
                    None,
                    Some("c1"),
                    vec![event("e1", "hello"), {
                        let mut f = event("e2", "ignored");
                        f.resource_id = "other-room".into();
                        f
                    }],
                ),
            )
            .unwrap();
        assert_eq!(replay.status, RecordStatus::Recorded);
        assert_eq!(replay.event_receipts, first.event_receipts);
        assert_eq!(replay.current_checkpoint.as_deref(), Some("c2"));
        let changed = store
            .observation_record(
                &principal(),
                request("b1", None, Some("c1"), vec![event("e1", "other")]),
            )
            .unwrap();
        assert_eq!(changed.reason.as_deref(), Some("batch_conflict"));

        // CAS: stale/unknown before is out_of_order and writes nothing.
        let stale = store
            .observation_record(
                &principal(),
                request("b3", Some("c1"), Some("c3"), vec![event("e4", "x")]),
            )
            .unwrap();
        assert_eq!(stale.status, RecordStatus::OutOfOrder);
        assert_eq!(stale.current_checkpoint.as_deref(), Some("c2"));

        // Duplicate returns the original; different content never overwrites it.
        let dup = store
            .observation_record(
                &principal(),
                request("b4", Some("c2"), Some("c3"), vec![event("e1", "hello")]),
            )
            .unwrap();
        assert_eq!(dup.event_receipts[0].status, "duplicate");
        let conflict = store
            .observation_record(
                &principal(),
                request("b5", Some("c3"), Some("c4"), vec![event("e1", "edited")]),
            )
            .unwrap();
        assert_eq!(
            conflict.status,
            RecordStatus::Recorded,
            "conflict is recorded, cursor may pass"
        );
        assert_eq!(conflict.event_receipts[0].status, "conflict");
        let stamp = |h: &str, id: &str| ObservationStamp {
            observation_id: id.into(),
            hash: h.into(),
            source: SourceStamp {
                source_id: source.clone(),
                version: 1,
            },
        };
        let id = first.event_receipts[0].observation_id.clone().unwrap();
        let hash = event("e1", "hello").content_hash();
        let goal = pending[0].goal_id.clone();
        let read = store.observation_read(&goal, &[stamp(&hash, &id)]).unwrap();
        assert_eq!(
            read[0].event.content.as_deref(),
            Some("hello"),
            "original text is kept"
        );
        assert!(store
            .observation_read(&goal, &[stamp("0000", &id)])
            .is_err());
        assert!(store
            .observation_read("unrelated-goal", &[stamp(&hash, &id)])
            .is_err());

        // Empty polls: noop writes nothing; attested-empty advances and is recorded.
        assert_eq!(
            store
                .observation_record(&principal(), request("b6", Some("c4"), Some("c4"), vec![]))
                .unwrap()
                .status,
            RecordStatus::Noop
        );
        assert_eq!(
            store
                .observation_record(&principal(), request("b7", Some("c4"), Some("c5"), vec![]))
                .unwrap()
                .status,
            RecordStatus::Recorded
        );

        // Poison: oversize body is a terminal rejected receipt and the cursor passes.
        let big = store
            .observation_record(
                &principal(),
                request(
                    "b8",
                    Some("c5"),
                    Some("c6"),
                    vec![event("big", &"x".repeat(MAX_BODY_BYTES + 1))],
                ),
            )
            .unwrap();
        assert_eq!(big.status, RecordStatus::Recorded);
        assert_eq!(big.event_receipts[0].status, "rejected");
        // Malformed/oversize batch: structured reject, no cursor movement.
        let many = (0..17).map(|i| event(&format!("m{i}"), "x")).collect();
        assert_eq!(
            store
                .observation_record(&principal(), request("b9", Some("c6"), Some("c7"), many))
                .unwrap()
                .reason
                .as_deref(),
            Some("batch_too_large")
        );
        assert_eq!(
            store.assistant_state().unwrap().revision,
            revision,
            "record never bumps assistant revision"
        );
    }
    // Everything above is durable across reopen, including dedup keys and the cursor.
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let dup = store
        .observation_record(
            &principal(),
            request("b10", Some("c6"), Some("c7"), vec![event("e1", "hello")]),
        )
        .unwrap();
    assert_eq!(dup.event_receipts[0].status, "duplicate");
    let health = store.observation_health().unwrap();
    let h = health.iter().find(|h| h.key == source).unwrap();
    assert_eq!((h.conflicts, h.filtered), (1, 1));
}

#[test]
fn unfinished_quota_backpressures_without_advancing_and_live_only_gap_is_visible() {
    let store = Store::open_in_memory().unwrap();
    bound(&store);
    let mut cursor: Option<String> = None;
    for i in 0..MAX_UNFINISHED_BINDING {
        let next = format!("c{i}");
        let r = store
            .observation_record(
                &principal(),
                request(
                    &format!("b{i}"),
                    cursor.as_deref(),
                    Some(&next),
                    vec![event(&format!("e{i}"), "x")],
                ),
            )
            .unwrap();
        assert_eq!(r.status, RecordStatus::Recorded);
        cursor = Some(next);
    }
    let mut over = request(
        "over",
        cursor.as_deref(),
        Some("next"),
        vec![event("late", "x")],
    );
    over.recovery = Recovery::LiveOnly;
    let r = store
        .observation_record(&principal(), over.clone())
        .unwrap();
    assert_eq!(r.status, RecordStatus::Backpressure);
    assert_eq!(r.retry_after_ms, Some(RETRY_AFTER_MS));
    assert_eq!(r.current_checkpoint, cursor);
    let health = store.observation_health().unwrap();
    assert_eq!(
        health[0].gap_count, 1,
        "live_only loss is counted, not hidden"
    );
    // Nothing stored: the same event is still new once capacity is released.
    assert!(store
        .observation_pending(1000)
        .unwrap()
        .iter()
        .all(|t| t.observation_id != "late"));
    // Terminal receipts free unfinished capacity.
    let pending = store.observation_pending(1000).unwrap();
    let updates: Vec<_> = pending
        .iter()
        .take(2)
        .map(|t| ReceiptUpdate {
            observation_id: t.observation_id.clone(),
            goal_id: t.goal_id.clone(),
            from: "recorded".into(),
            to: "rejected".into(),
            run_id: Some("run".into()),
            output_id: Some("out".into()),
        })
        .collect();
    store.observation_commit(None, &updates).unwrap();
    over.recovery = Recovery::Resumable;
    assert_eq!(
        store.observation_record(&principal(), over).unwrap().status,
        RecordStatus::Recorded
    );
}

#[test]
fn history_capacity_needs_explicit_versioned_raise() {
    let store = Store::open_in_memory().unwrap();
    let (state, binding) = bound(&store);
    let mut cursor: Option<String> = None;
    // Filtered inputs are terminal, so only the history (dedup) quota can stop them.
    let filtered = |i: usize, cursor: &mut Option<String>| {
        let mut e = event(&format!("f{i}"), "x");
        e.resource_id = "other".into();
        let next = format!("c{i}");
        let r = store
            .observation_record(
                &principal(),
                request(&format!("fb{i}"), cursor.as_deref(), Some(&next), vec![e]),
            )
            .unwrap();
        if r.status == RecordStatus::Recorded {
            *cursor = Some(next);
        }
        r
    };
    for i in 0..2_000 {
        assert_eq!(filtered(i, &mut cursor).status, RecordStatus::Recorded);
    }
    // 2_000 inputs + 2_000 batch receipts reached the per-source default.
    let r = filtered(2_000, &mut cursor);
    assert_eq!(r.status, RecordStatus::Backpressure);
    assert_eq!(r.reason.as_deref(), Some("needs_capacity"));
    let health = store.observation_health().unwrap();
    assert_eq!(health[0].state, "needs_capacity");
    assert!(health[0]
        .detail
        .as_deref()
        .unwrap()
        .contains("Raise the limit"));
    // A stale version cannot raise it; the current version can; keys are never discarded.
    assert!(store
        .edit_assistant(
            state.revision,
            AssistantEdit::SourceCapacity {
                source_id: binding.source_id.clone(),
                binding_version: 9,
                source_history_limit: Some(4000),
                global_history_limit: None
            }
        )
        .is_err());
    store
        .edit_assistant(
            state.revision,
            AssistantEdit::SourceCapacity {
                source_id: binding.source_id,
                binding_version: 1,
                source_history_limit: Some(4000),
                global_history_limit: None,
            },
        )
        .unwrap();
    assert_eq!(filtered(2_000, &mut cursor).status, RecordStatus::Recorded);
}

#[test]
fn binding_edits_need_exact_scope_and_shrink_old_receipts() {
    let store = Store::open_in_memory().unwrap();
    let (state, binding) = bound(&store);
    // Outside the chief-of-staff project scope.
    assert!(store
        .edit_assistant(
            state.revision,
            AssistantEdit::Source {
                binding: input(vec![], 0, true, vec!["/project-b".into()])
            }
        )
        .is_err());
    store
        .observation_record(
            &principal(),
            request("b1", None, Some("c1"), vec![event("e1", "x")]),
        )
        .unwrap();
    let pending = store.observation_pending(10).unwrap();
    // Revoking (enabled=false) at the exact version invalidates open receipts.
    let mut revoke = input(
        state.goals.iter().map(|g| g.id.clone()).collect(),
        1,
        false,
        vec!["/project-a".into()],
    );
    revoke.source_id = Some(binding.source_id.clone());
    let saved = store
        .edit_assistant(
            state.revision,
            AssistantEdit::Source {
                binding: revoke.clone(),
            },
        )
        .unwrap();
    assert_eq!(saved.sources[0].version, 2);
    assert!(store
        .observation_pending(10)
        .unwrap()
        .iter()
        .all(|t| t.state == "invalidated"));
    // Stale version is rejected, a changed account needs a new source, and disabled drops traffic.
    assert!(store
        .edit_assistant(saved.revision, AssistantEdit::Source { binding: revoke })
        .is_err());
    let r = store
        .observation_record(
            &principal(),
            request("b2", Some("c1"), Some("c2"), vec![event("e2", "x")]),
        )
        .unwrap();
    assert_eq!(r.reason.as_deref(), Some("needs_binding"));
    // Re-enabling does not widen what the old receipts may do: they stay invalidated and cannot
    // be moved to a non-shrinking state.
    let mut again = input(
        saved.goals.iter().map(|g| g.id.clone()).collect(),
        2,
        true,
        vec!["/project-a".into()],
    );
    again.source_id = Some(binding.source_id);
    let saved = store
        .edit_assistant(saved.revision, AssistantEdit::Source { binding: again })
        .unwrap();
    let update = ReceiptUpdate {
        observation_id: pending[0].observation_id.clone(),
        goal_id: pending[0].goal_id.clone(),
        from: "invalidated".into(),
        to: "handled".into(),
        run_id: None,
        output_id: Some("o".into()),
    };
    assert!(store
        .observation_commit(Some((saved.revision, saved.clone())), &[update])
        .is_err());
}

#[test]
fn state_output_and_receipt_commit_atomically() {
    let store = Store::open_in_memory().unwrap();
    let (state, _) = bound(&store);
    store
        .observation_record(
            &principal(),
            request("b1", None, Some("c1"), vec![event("e1", "x")]),
        )
        .unwrap();
    let target = store.observation_pending(1).unwrap().remove(0);
    let good = ReceiptUpdate {
        observation_id: target.observation_id.clone(),
        goal_id: target.goal_id.clone(),
        from: "recorded".into(),
        to: "handled".into(),
        run_id: Some("run-1".into()),
        output_id: Some("obs-out:run-1".into()),
    };
    let bad = ReceiptUpdate {
        goal_id: "missing".into(),
        ..good.clone()
    };
    let mut next = state.clone();
    next.summary = "changed".into();
    back_output(&mut next, &target, "run-1");
    // A failing receipt rolls the assistant document back with it.
    assert!(store
        .observation_commit(Some((state.revision, next.clone())), &[good.clone(), bad])
        .is_err());
    assert_eq!(store.assistant_state().unwrap().summary, state.summary);
    assert_eq!(store.assistant_state().unwrap().revision, state.revision);
    assert_eq!(
        store
            .observation_receipts(&target.observation_id)
            .unwrap()
            .iter()
            .find(|t| t.goal_id == target.goal_id)
            .unwrap()
            .state,
        "recorded"
    );
    // A stale revision fails before any receipt moves.
    assert!(store
        .observation_commit(Some((state.revision + 7, next.clone())), &[good.clone()])
        .is_err());
    // Success commits both; a retry of the same output id is an idempotent replay.
    let saved = store
        .observation_commit(Some((state.revision, next)), &[good.clone()])
        .unwrap()
        .unwrap();
    assert_eq!(saved.summary, "changed");
    assert_eq!(
        store
            .observation_receipts(&target.observation_id)
            .unwrap()
            .iter()
            .find(|t| t.goal_id == target.goal_id)
            .unwrap()
            .state,
        "handled"
    );
    store.observation_commit(None, &[good]).unwrap();
}

fn back_output(state: &mut AssistantState, target: &TargetReceipt, run: &str) {
    state.scoped_runs.push(ScopedReview {
        scope: target.goal_id.clone(),
        retry_on_capacity: Some(false),
        run: AssistantRun {
            id: run.into(),
            session_id: None,
            submitted: true,
            base_revision: state.revision,
            input: String::new(),
        },
        observed: "fixture".into(),
        state: "completed".into(),
        summary: "report".into(),
        error: None,
        observation: Some(ObservationRun {
            external: true,
            inputs: vec![ObservationStamp {
                observation_id: target.observation_id.clone(),
                hash: target.hash.clone(),
                source: SourceStamp {
                    source_id: target.source_id.clone(),
                    version: target.binding_version,
                },
            }],
        }),
        actor: None,
        credential: None,
    });
    state.conversation.push(serde_json::from_value(serde_json::json!({
        "id":format!("obs-out:{run}"),"created_at":"2026-10-06T00:00:00Z","author":"assistant","content":"Sourced unconfirmed report","reply_to":null,"project_paths":[target.project_path],"status":"handled","goal_ids":[target.goal_id],"question_ids":[],"error":null,"source_run_id":run
    })).unwrap());
}

#[test]
fn reset_requires_the_exact_persisted_user_decision_and_replays_only_original_batch() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.db");
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let (state, binding) = bound(&store);
    store
        .observation_record(&principal(), request("start", None, Some("old"), vec![]))
        .unwrap();
    let mut reset = request("reset", Some("old"), Some("baseline"), vec![]);
    reset.reset = Some(ResetMark {
        approval_ref: "invented".into(),
    });
    assert_eq!(
        store
            .observation_record(&principal(), reset.clone())
            .unwrap()
            .reason
            .as_deref(),
        Some("reset_not_approved")
    );
    let decision = AssistantEdit::SourceReset {
        source_id: binding.source_id.clone(),
        binding_version: 1,
        stream_id: "s1".into(),
        checkpoint_before: Some("old".into()),
        checkpoint_after: Some("baseline".into()),
        reason: "provider history expired; missing interval acknowledged".into(),
    };
    assert!(store
        .edit_assistant(state.revision + 1, decision.clone())
        .is_err());
    store.edit_assistant(state.revision, decision).unwrap();
    let approved = store.observation_resets().unwrap().remove(0);
    reset.reset = Some(ResetMark {
        approval_ref: approved.approval_ref,
    });
    let mut wrong = reset.clone();
    wrong.checkpoint_after = Some("widened".into());
    assert_eq!(
        store
            .observation_record(&principal(), wrong)
            .unwrap()
            .status,
        RecordStatus::Rejected
    );
    let result = store
        .observation_record(&principal(), reset.clone())
        .unwrap();
    assert_eq!(result.status, RecordStatus::Recorded);
    assert!(store.observation_resets().unwrap().is_empty());
    drop(store);
    let store = Store::open(path.to_str().unwrap()).unwrap();
    assert_eq!(
        store.observation_record(&principal(), reset).unwrap(),
        result
    );
    assert_eq!(store.observation_health().unwrap()[0].gap_count, 1);
    assert_eq!(
        store.observation_streams().unwrap()[0]
            .checkpoint
            .as_deref(),
        Some("baseline")
    );
}

#[test]
fn unknown_is_quarantined_when_binding_is_revoked_and_cannot_be_downgraded() {
    let store = Store::open_in_memory().unwrap();
    let (state, binding) = bound(&store);
    store
        .observation_record(
            &principal(),
            request("b", None, Some("c"), vec![event("e", "feedback")]),
        )
        .unwrap();
    let t = store.observation_pending(1).unwrap().remove(0);
    let mut u = ReceiptUpdate {
        observation_id: t.observation_id.clone(),
        goal_id: t.goal_id.clone(),
        from: "recorded".into(),
        to: "unknown".into(),
        run_id: Some("uncertain".into()),
        output_id: None,
    };
    store.observation_commit(None, &[u.clone()]).unwrap();
    let mut edit = input(
        state.goals.iter().map(|g| g.id.clone()).collect(),
        1,
        false,
        vec!["/project-a".into()],
    );
    edit.source_id = Some(binding.source_id);
    store
        .edit_assistant(state.revision, AssistantEdit::Source { binding: edit })
        .unwrap();
    assert_eq!(
        store
            .observation_receipts(&t.observation_id)
            .unwrap()
            .iter()
            .find(|r| r.goal_id == t.goal_id)
            .unwrap()
            .state,
        "unknown"
    );
    u.from = "unknown".into();
    u.to = "invalidated".into();
    assert!(store.observation_commit(None, &[u]).is_err());
    let view = store.observation_inspection(10).unwrap();
    assert_eq!(view[0].hash, event("e", "feedback").content_hash());
    assert!(serde_json::to_string(&view)
        .unwrap()
        .find("feedback")
        .is_none());
}

#[test]
fn poison_duplicate_scope_and_opaque_object_versions_remain_visible_without_revision_writes() {
    let store = Store::open_in_memory().unwrap();
    let (state, binding) = bound(&store);
    // Force the binding order to differ from the duplicate SQL order, even with UUIDs.
    let mut ids = binding.goal_ids.clone();
    ids.sort_by(|a, b| b.cmp(a));
    let mut reversed = input(ids.clone(), binding.version, true, binding.project_paths);
    reversed.source_id = Some(binding.source_id);
    let state = store
        .edit_assistant(state.revision, AssistantEdit::Source { binding: reversed })
        .unwrap();
    let mut e = event("first", "original");
    e.object_version = Some("z-version".into());
    let first = store
        .observation_record(
            &principal(),
            request("b1", None, Some("c1"), vec![e.clone()]),
        )
        .unwrap();
    let dup = store
        .observation_record(
            &principal(),
            request("b2", Some("c1"), Some("c2"), vec![e.clone()]),
        )
        .unwrap();
    assert_eq!(
        dup.event_receipts[0].goal_ids,
        first.event_receipts[0].goal_ids
    );
    ids.sort();
    assert_eq!(first.event_receipts[0].goal_ids, ids);
    let mut update = e.clone();
    update.event_id = "opaque".into();
    update.object_version = Some("a-version".into());
    update.content = Some("possibly old".into());
    let newer = store
        .observation_record(
            &principal(),
            request("b3", Some("c2"), Some("c3"), vec![update]),
        )
        .unwrap();
    assert!(store
        .observation_receipts(newer.event_receipts[0].observation_id.as_deref().unwrap())
        .unwrap()
        .iter()
        .all(|r| r.state == "needs_attention"));
    let mut withdrawn = e.clone();
    withdrawn.event_id = "withdraw".into();
    withdrawn.kind = "message.deleted".into();
    withdrawn.reply_to = Some("first".into());
    withdrawn.content = None;
    store
        .observation_record(
            &principal(),
            request("b4", Some("c3"), Some("c4"), vec![withdrawn]),
        )
        .unwrap();
    assert!(store
        .observation_receipts(first.event_receipts[0].observation_id.as_deref().unwrap())
        .unwrap()
        .iter()
        .all(|r| r.state == "superseded"));
    let mut poison = event("poison", "body");
    poison.kind = "".into();
    let r = store
        .observation_record(
            &principal(),
            request("b5", Some("c4"), Some("c5"), vec![poison]),
        )
        .unwrap();
    assert_eq!(r.status, RecordStatus::Recorded);
    assert_eq!(r.event_receipts[0].status, "rejected");
    assert_eq!(store.assistant_state().unwrap().revision, state.revision);
}

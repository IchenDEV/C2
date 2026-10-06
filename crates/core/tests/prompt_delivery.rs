use codetwo_core::{
    assistant::*,
    prompt_delivery::PromptDelivery,
    provider::ProviderId,
    session::{Part, Session, SessionActivity, SessionRunState},
    skill::DocBlock,
    Store,
};

#[tokio::test]
async fn supported_provider_accepts_live_delivery_while_an_ordinary_prompt_waits() {
    use codetwo_core::{
        event::Event,
        provider::{LaunchSpec, Provider},
        skill::SkillLibrary,
        Engine, Op,
    };
    use std::{sync::Arc, time::Duration};
    let script = r#"
import json,sys
pending=None
for line in sys.stdin:
    m=json.loads(line); method=m.get('method'); mid=m.get('id'); result={}
    if method=='initialize': result={'protocolVersion':1,'_meta':{'steering':{'supported':True}}}
    elif method=='session/new': result={'sessionId':'live'}
    elif method=='session/prompt': pending=mid; continue
    elif method=='_session/steering': result={'outcome':'injected'}
    elif method=='session/cancel':
        if pending is not None: print(json.dumps({'jsonrpc':'2.0','id':pending,'result':{'stopReason':'cancelled'}}),flush=True)
        pending=None; continue
    elif mid is None: continue
    print(json.dumps({'jsonrpc':'2.0','id':mid,'result':result}),flush=True)
"#;
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (engine, mut events) = Engine::with_store(
        vec![Provider {
            id: ProviderId::Grok,
            display_name: "Steering transport fixture".into(),
            launch: LaunchSpec::new("python3", ["-c", script]),
            needs_node: false,
        }],
        SkillLibrary::new(vec![]),
        store.clone(),
    );
    engine
        .submit(Op::NewSession {
            provider: ProviderId::Grok,
            cwd: root.path().to_string_lossy().into(),
            use_worktree: false,
            worktree_base: None,
            worktree_base_sha: None,
            request_id: Some("live-create".into()),
            model: None,
            initial_policy: None,
        })
        .await
        .unwrap();
    let session = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(Event::SessionCreated { session, .. }) = events.recv().await {
                break session;
            }
        }
    })
    .await
    .unwrap();
    assert!(engine.session_can_steer(&session));
    engine
        .submit(Op::Prompt {
            session: session.clone(),
            doc: vec![DocBlock::Text {
                text: "Work until cancelled".into(),
            }],
            request_id: Some("live-start".into()),
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while engine.current_turn(&session).is_none() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    engine
        .deliver_prompt(
            &session,
            vec![DocBlock::Text {
                text: "Later input".into(),
            }],
            "queue",
            Some("later".into()),
            None,
        )
        .await
        .unwrap();
    let result = engine
        .deliver_prompt(
            &session,
            vec![DocBlock::Text {
                text: "Live correction".into(),
            }],
            "steer",
            Some("live-correction".into()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(result.state, "queued");
    tokio::time::timeout(Duration::from_secs(5), async {
        while store
            .prompt_delivery("live-correction")
            .unwrap()
            .unwrap()
            .state
            != "accepted"
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        store
            .prompt_delivery("live-correction")
            .unwrap()
            .unwrap()
            .outcome,
        "injected"
    );
    assert_eq!(
        store.prompt_delivery("later").unwrap().unwrap().state,
        "queued"
    );
    assert_eq!(
        engine
            .deliver_prompt(
                &session,
                vec![DocBlock::Text {
                    text: "Live correction".into()
                }],
                "steer",
                Some("live-correction".into()),
                None
            )
            .await
            .unwrap()
            .outcome,
        "injected"
    );
    assert_eq!(
        store
            .transcript(&session)
            .unwrap()
            .iter()
            .map(|(_, part)| part)
            .filter(|p| matches!(p,Part::Prompt{text,..} if text=="Live correction"))
            .count(),
        1
    );
    engine.cancel_pending_prompts(&session).unwrap();
    engine
        .submit(Op::Cancel {
            session: session.clone(),
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while engine.current_turn(&session).is_some() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        engine
            .deliver_prompt(
                &session,
                vec![DocBlock::Text {
                    text: "Live correction".into()
                }],
                "steer",
                Some("live-correction".into()),
                None
            )
            .await
            .unwrap()
            .outcome,
        "injected"
    );
    engine.shutdown();
}
fn setup(store: &Store) -> (AssistantState, Session, PromptDelivery) {
    let s = Session::new(ProviderId::Codex, "/project");
    store.upsert_session(&s).unwrap();
    let mut state = AssistantState::default();
    state.settings = Some(AssistantSettings {
        enabled: true,
        projects: vec!["/project".into()],
        provider: ProviderId::Codex,
        model: None,
        reasoning_effort: None,
        concurrency: 1,
        turn_limit: 5,
        dispatch_limit: 2,
    });
    state.goals.push(AssistantGoal {
        id: "goal".into(),
        project_path: "/project".into(),
        title: "Goal".into(),
        acceptance: "File".into(),
        priority: 1,
        status: GoalStatus::Active,
        next_step: String::new(),
        blocker: String::new(),
        assignments: vec![Assignment {
            id: "assignment".into(),
            instruction: "Goal".into(),
            session_id: Some(s.id.clone()),
            submitted: true,
            taken_over: false,
            owned: true,
            contract_revision: 1,
            confirmed_revision: 1,
            protocol: 1,
            stop_requested: false,
            stop_sent: false,
            result: None,
            inputs: vec![],
            pending_inputs: None,
            prior_results: vec![],
        }],
        verdict: None,
        contract_revision: 1,
        dependencies: vec![],
    });
    let state = store.save_assistant(0, state).unwrap();
    let d = PromptDelivery {
        id: "delivery-1".into(),
        session_id: s.id.clone(),
        mode: "queue".into(),
        doc: vec![DocBlock::Text {
            text: "Supplement".into(),
        }],
        state: "queued".into(),
        outcome: String::new(),
        goal_id: Some("goal".into()),
        contract_revision: Some(1),
        assignment_id: Some("assignment".into()),
        expected_turn: None,
    };
    (state, s, d)
}

#[test]
fn a_failed_delivery_migration_rolls_back_the_entire_coordination_install() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("migration.db");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TABLE prompt_deliveries(sequence INTEGER PRIMARY KEY, id TEXT); INSERT INTO prompt_deliveries VALUES(1,'retained');").unwrap();
    drop(connection);
    assert!(Store::open(path.to_str().unwrap()).is_err());
    let connection = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(connection.query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('assistant_state','assistant_worker_receipts')",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(
        connection
            .query_row("SELECT id FROM prompt_deliveries", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "retained"
    );
}
#[test]
fn durable_queue_replays_only_identical_commands_and_claims_once() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("db");
    let (s, d) = {
        let store = Store::open(path.to_str().unwrap()).unwrap();
        let (_, s, d) = setup(&store);
        store.enqueue_delivery(&d).unwrap();
        (s, d)
    };
    let store = Store::open(path.to_str().unwrap()).unwrap();
    assert_eq!(store.pending_deliveries().unwrap().len(), 1);
    store.enqueue_delivery(&d).unwrap();
    let mut other = d.clone();
    other.doc = vec![DocBlock::Text {
        text: "Different".into(),
    }];
    assert!(store.enqueue_delivery(&other).is_err());
    assert!(store.claim_delivery(&d.id).unwrap());
    assert!(!store.claim_delivery(&d.id).unwrap());
    let activity = SessionActivity {
        revision: 1,
        state: SessionRunState::Running {
            turn_id: "turn".into(),
            prompt_request_id: Some(d.id.clone()),
        },
    };
    let part = Part::Prompt {
        text: "Supplement".into(),
        display: "Supplement".into(),
    };
    let (_, replay) = store
        .append_prompt_activity_and_receipt(
            "c2-delivery-prompt",
            &d.id,
            Some(&d.id),
            &s.id,
            &part,
            0,
            &activity,
        )
        .unwrap();
    assert!(!replay);
    let (_, replay) = store
        .append_prompt_activity_and_receipt(
            "c2-delivery-prompt",
            &d.id,
            Some(&d.id),
            &s.id,
            &part,
            0,
            &activity,
        )
        .unwrap();
    assert!(replay);
    assert_eq!(store.transcript(&s.id).unwrap().len(), 1);
}
#[test]
fn saved_control_invalidates_queued_and_fences_already_claimed_prompts() {
    for change in [
        "revision",
        "takeover",
        "cancel",
        "remove_project",
        "disable",
        "replacement",
    ] {
        for claimed in [false, true] {
            let store = Store::open_in_memory().unwrap();
            let (mut state, s, d) = setup(&store);
            store.enqueue_delivery(&d).unwrap();
            if claimed {
                assert!(store.claim_delivery(&d.id).unwrap());
            }
            match change {
                "revision" => state.goals[0].contract_revision += 1,
                "takeover" => state.goals[0].assignments[0].taken_over = true,
                "cancel" => state.goals[0].status = GoalStatus::Cancelled,
                "remove_project" => state.settings.as_mut().unwrap().projects.clear(),
                "disable" => state.settings.as_mut().unwrap().enabled = false,
                "replacement" => state.goals[0].assignments[0].id = "replacement".into(),
                _ => unreachable!(),
            }
            store.save_assistant(state.revision, state).unwrap();
            if !claimed {
                assert_eq!(
                    store.prompt_delivery(&d.id).unwrap().unwrap().state,
                    "cancelled"
                );
                assert!(!store.claim_delivery(&d.id).unwrap());
            }
            let activity = SessionActivity {
                revision: 1,
                state: SessionRunState::Running {
                    turn_id: "turn".into(),
                    prompt_request_id: Some(d.id.clone()),
                },
            };
            assert!(
                store
                    .append_prompt_activity_and_receipt(
                        "c2-delivery-prompt",
                        &d.id,
                        Some(&d.id),
                        &s.id,
                        &Part::Prompt {
                            text: "stale".into(),
                            display: "stale".into()
                        },
                        0,
                        &activity
                    )
                    .is_err(),
                "{change}, claimed={claimed}"
            );
            assert!(store.transcript(&s.id).unwrap().is_empty());
        }
    }
}
#[test]
fn busy_session_backlog_does_not_hide_an_independent_session() {
    let store = Store::open_in_memory().unwrap();
    let (_, s, mut d) = setup(&store);
    d.goal_id = None;
    d.assignment_id = None;
    d.contract_revision = None;
    for n in 0..250 {
        d.id = format!("d-{n}");
        store.enqueue_delivery(&d).unwrap();
    }
    let independent = Session::new(ProviderId::Codex, "/independent");
    store.upsert_session(&independent).unwrap();
    d.id = "independent".into();
    d.session_id = independent.id;
    store.enqueue_delivery(&d).unwrap();
    let pending = store.pending_deliveries().unwrap();
    assert_eq!(pending.len(), 2);
    assert!(pending.iter().any(|d| d.id == "independent"));
    assert!(pending.iter().any(|d| d.session_id == s.id));
}

#[test]
fn live_steering_has_a_separate_lane_and_cannot_cross_into_a_later_turn() {
    let store = Store::open_in_memory().unwrap();
    let (_, s, queue) = setup(&store);
    store.enqueue_delivery(&queue).unwrap();
    let activity = SessionActivity {
        revision: 1,
        state: SessionRunState::Running {
            turn_id: "turn-1".into(),
            prompt_request_id: None,
        },
    };
    store
        .append_prompt_and_activity(
            &s.id,
            &Part::Prompt {
                text: "first turn".into(),
                display: "first".into(),
            },
            0,
            &activity,
        )
        .unwrap();
    let mut steer = queue.clone();
    steer.id = "steer-1".into();
    steer.mode = "steer".into();
    steer.expected_turn = Some("turn-1".into());
    store.enqueue_delivery(&steer).unwrap();
    let pending = store.pending_deliveries().unwrap();
    assert_eq!(pending.len(), 2);
    assert!(pending.iter().any(|d| d.id == steer.id));
    assert!(store.claim_delivery(&steer.id).unwrap());
    let mut stale = steer.clone();
    stale.id = "stale-steer".into();
    stale.expected_turn = Some("earlier-turn".into());
    store.enqueue_delivery(&stale).unwrap();
    assert!(!store.claim_delivery(&stale.id).unwrap());
    assert_eq!(
        store.prompt_delivery(&stale.id).unwrap().unwrap().state,
        "cancelled"
    );
}

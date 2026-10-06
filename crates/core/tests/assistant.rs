use codetwo_core::{
    assistant::*,
    provider::ProviderId,
    session::{RunFailureReason, Session, SessionActivity, SessionRunState},
    Store,
};

fn configured(store: &Store) -> AssistantState {
    store.add_project("/project-a", Some("A"), 0).unwrap();
    store.add_project("/project-b", Some("B"), 0).unwrap();
    let state = store
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
    store
        .edit_assistant(
            state.revision,
            AssistantEdit::Goal {
                id: None,
                project_path: "/project-a".into(),
                title: "Repair login".into(),
                acceptance: "Regression passes; provide the commit".into(),
                priority: 1,
            },
        )
        .unwrap()
}
fn dispatch(state: &AssistantState) -> AssistantDecision {
    AssistantDecision {
        summary: "Repair the regression".into(),
        actions: vec![AssistantAction::Dispatch {
            goal_id: state.goals[0].id.clone(),
            instruction: "Repair login within the project and run the regression".into(),
        }],
    }
}
#[test]
fn durable_goals_reject_stale_decisions_and_preserve_user_takeover() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("assistant.db");
    let old = {
        let store = Store::open(db.to_str().unwrap()).unwrap();
        let state = configured(&store);
        store
            .add_memory(
                "/project-a",
                "constraint",
                "Keep the login API stable",
                true,
            )
            .unwrap();
        state
    };
    let store = Store::open(db.to_str().unwrap()).unwrap();
    let state = store.assistant_state().unwrap();
    assert_eq!(state.goals[0].title, old.goals[0].title);
    let paused = store
        .edit_assistant(
            state.revision,
            AssistantEdit::Takeover {
                id: state.goals[0].id.clone(),
            },
        )
        .unwrap();
    let stale = apply_decision(&state, &[], dispatch(&state)).unwrap();
    assert!(store.save_assistant(state.revision, stale).is_err());
    assert!(apply_decision(&paused, &[], dispatch(&paused)).is_err());
    assert_eq!(
        store.list_memories("/project-a", 10).unwrap()[0].content,
        "Keep the login API stable"
    );
}
#[test]
fn scope_concurrency_and_interrupted_receipts_fail_closed() {
    let store = Store::open_in_memory().unwrap();
    let state = configured(&store);
    let dispatched = apply_decision(&state, &[], dispatch(&state)).unwrap();
    assert!(apply_decision(&dispatched, &[], dispatch(&dispatched)).is_err());
    let mut limited = state.clone();
    limited.dispatches = limited.settings.as_ref().unwrap().dispatch_limit;
    assert!(apply_decision(&limited, &[], dispatch(&limited))
        .unwrap_err()
        .to_string()
        .contains("allowance exhausted"));
    let mut outside = Session::new(ProviderId::Codex, "/project-b");
    outside.id = "outside".into();
    assert!(apply_decision(
        &state,
        &[outside],
        AssistantDecision {
            summary: "link".into(),
            actions: vec![AssistantAction::Link {
                goal_id: state.goals[0].id.clone(),
                session_id: "outside".into()
            }]
        }
    )
    .is_err());
    let mut interrupted = Session::new(ProviderId::Codex, "/project-a");
    interrupted.activity = SessionActivity {
        revision: 2,
        state: SessionRunState::Failed {
            turn_id: None,
            reason: RunFailureReason::Interrupted,
            message: "restart".into(),
        },
    };
    let mut unresolved = dispatched;
    unresolved.goals[0].assignments[0].session_id = Some(interrupted.id.clone());
    assert!(apply_decision(&unresolved, &[interrupted], dispatch(&unresolved)).is_err());
}
#[test]
fn idle_is_not_acceptance_and_verdicts_bind_the_current_revision() {
    let store = Store::open_in_memory().unwrap();
    let mut state = configured(&store);
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("report.txt"), b"report v1").unwrap();
    let mut session = Session::new(ProviderId::Codex, temp.path().to_string_lossy().to_string());
    session.project_path = Some("/project-a".into());
    state = apply_decision(&state, &[], dispatch(&state)).unwrap();
    state.goals[0].assignments[0].session_id = Some(session.id.clone());
    assert_eq!(state.goals[0].status, GoalStatus::Active);
    state.goals[0].next_step = "Wait for the worker".into();
    state.goals[0].blocker = "Pending evidence".into();
    let artifacts = vec![ArtifactStamp {
        path: "report.txt".into(),
        sha256: {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(b"report v1"))
        },
    }];
    store.upsert_session(&session).unwrap();
    store.save_assistant(state.revision, state).unwrap();
    store
        .assistant_worker(
            &session.id,
            WorkerOperation::Confirm {
                contract_revision: 1,
                message_ids: vec![],
            },
        )
        .unwrap();
    store
        .assistant_worker(
            &session.id,
            WorkerOperation::Submit {
                contract_revision: 1,
                evidence: "report checked".into(),
                artifacts,
            },
        )
        .unwrap();
    state = store.assistant_state().unwrap();
    let accept = |revision| AssistantDecision {
        summary: "Checked the result".into(),
        actions: vec![AssistantAction::Accept {
            goal_id: state.goals[0].id.clone(),
            session_id: session.id.clone(),
            activity_revision: revision,
            evidence: "report v1; login regression passed".into(),
            artifacts: vec![ArtifactStamp {
                path: "report.txt".into(),
                sha256: {
                    use sha2::{Digest, Sha256};
                    format!("{:x}", Sha256::digest(b"report v1"))
                },
            }],
        }],
    };
    assert!(apply_decision(&state, &[session.clone()], accept(3)).is_err());
    let done = apply_decision(
        &state,
        &[session.clone()],
        accept(session.activity.revision),
    )
    .unwrap();
    assert_eq!(done.goals[0].status, GoalStatus::Completed);
    assert!(done.goals[0].next_step.is_empty() && done.goals[0].blocker.is_empty());
    std::fs::write(temp.path().join("report.txt"), b"report v2").unwrap();
    assert!(verify_artifacts(
        &session.cwd,
        &done.goals[0].verdict.as_ref().unwrap().artifacts
    )
    .is_err());
    assert!(apply_decision(
        &state,
        &[session.clone()],
        accept(session.activity.revision)
    )
    .is_err());
}
#[test]
fn forgetting_and_global_switch_control_actual_project_recall() {
    let store = Store::open_in_memory().unwrap();
    configured(&store);
    let session = Session::new(ProviderId::Codex, "/project-a");
    store.upsert_session(&session).unwrap();
    let note = store
        .add_memory(
            "/project-a",
            "preference",
            "Prefer a stable login API",
            true,
        )
        .unwrap();
    store
        .add_memory("/project-b", "fact", "PRIVATE_B_ONLY", true)
        .unwrap();
    let before = store
        .memory_context_with_receipt("/project-a", &session.id, "login")
        .unwrap();
    assert!(before.items.iter().any(|m| m.id == note.id));
    assert!(!before.block.contains("PRIVATE_B_ONLY"));
    store.set_memory_active(&note.id, false).unwrap();
    assert!(!store
        .memory_context_with_receipt("/project-a", &session.id, "login")
        .unwrap()
        .items
        .iter()
        .any(|m| m.id == note.id));
    store
        .add_memory(GLOBAL_MEMORY, "preference", "Use short updates", true)
        .unwrap();
    let mut settings = store.memory_settings().unwrap();
    settings.enabled = false;
    store.set_memory_settings(settings).unwrap();
    assert!(store
        .memory_context_with_receipt(GLOBAL_MEMORY, &session.id, "updates")
        .unwrap()
        .block
        .is_empty());
}
#[test]
fn entire_proposal_is_rejected_without_partial_dispatch() {
    let store = Store::open_in_memory().unwrap();
    let state = configured(&store);
    let mut proposal = dispatch(&state);
    proposal.actions.push(AssistantAction::Update {
        goal_id: "unknown".into(),
        next_step: "bad".into(),
        blocker: String::new(),
    });
    assert!(apply_decision(&state, &[], proposal).is_err());
    assert!(store.assistant_state().unwrap().goals[0]
        .assignments
        .is_empty());
    assert!(parse_decision(r#"{"summary":"x","actions":[],"enable_all_projects":true}"#).is_err());
}

#[test]
fn takeover_of_uncreated_intent_releases_slot_and_resume_can_dispatch() {
    let store = Store::open_in_memory().unwrap();
    let state = configured(&store);
    let dispatched = apply_decision(&state, &[], dispatch(&state)).unwrap();
    let saved = store.save_assistant(state.revision, dispatched).unwrap();
    let paused = store
        .edit_assistant(
            saved.revision,
            AssistantEdit::Takeover {
                id: saved.goals[0].id.clone(),
            },
        )
        .unwrap();
    assert_eq!(paused.goals[0].assignments.len(), 1);
    assert!(!paused.goals[0].assignments[0].owned);
    assert!(paused.goals[0].assignments[0].taken_over);
    let resumed = store
        .edit_assistant(
            paused.revision,
            AssistantEdit::Status {
                id: paused.goals[0].id.clone(),
                status: GoalStatus::Active,
            },
        )
        .unwrap();
    assert!(apply_decision(&resumed, &[], dispatch(&resumed)).is_ok());
}

#[cfg(unix)]
#[test]
fn artifact_verification_rejects_fifo_and_workspace_escape() {
    let temp = tempfile::tempdir().unwrap();
    let pipe = temp.path().join("pipe");
    assert!(std::process::Command::new("mkfifo")
        .arg(&pipe)
        .status()
        .unwrap()
        .success());
    let stamp = ArtifactStamp {
        path: "pipe".into(),
        sha256: "0".repeat(64),
    };
    assert!(verify_artifacts(temp.path().to_str().unwrap(), &[stamp])
        .unwrap_err()
        .to_string()
        .contains("regular file"));
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), temp.path().join("outside")).unwrap();
    let stamp = ArtifactStamp {
        path: "outside".into(),
        sha256: "0".repeat(64),
    };
    assert!(verify_artifacts(temp.path().to_str().unwrap(), &[stamp])
        .unwrap_err()
        .to_string()
        .contains("escapes"));
}

fn managed(store: &Store, root: &std::path::Path) -> (AssistantState, Session) {
    let state = configured(store);
    let mut state = apply_decision(&state, &[], dispatch(&state)).unwrap();
    let mut session = Session::new(ProviderId::Codex, root.to_string_lossy().to_string());
    session.project_path = Some("/project-a".into());
    store.upsert_session(&session).unwrap();
    state.goals[0].assignments[0].session_id = Some(session.id.clone());
    state.goals[0].assignments[0].submitted = true;
    store.save_assistant(state.revision, state).unwrap();
    store
        .assistant_worker(
            &session.id,
            WorkerOperation::Confirm {
                contract_revision: 1,
                message_ids: vec![],
            },
        )
        .unwrap();
    (store.assistant_state().unwrap(), session)
}
fn stamps(root: &std::path::Path, bytes: &[u8]) -> Vec<ArtifactStamp> {
    use sha2::{Digest, Sha256};
    std::fs::write(root.join("report.txt"), bytes).unwrap();
    vec![ArtifactStamp {
        path: "report.txt".into(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
    }]
}
#[test]
fn worker_identity_idempotency_questions_and_final_review_are_versioned() {
    let store = Store::open_in_memory().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (state, s) = managed(&store, root.path());
    assert!(store
        .assistant_worker("forged-session", WorkerOperation::Context)
        .is_err());
    let op = WorkerOperation::Progress {
        content: "Checked local input".into(),
    };
    let response = store
        .assistant_worker_command(&s.id, "progress-1", op.clone())
        .unwrap();
    assert_eq!(
        response,
        store
            .assistant_worker_command(&s.id, "progress-1", op)
            .unwrap()
    );
    assert_eq!(store.assistant_state().unwrap().messages.len(), 1);
    assert!(store
        .assistant_worker_command(
            &s.id,
            "progress-1",
            WorkerOperation::Progress {
                content: "different".into()
            }
        )
        .is_err());
    let artifacts = stamps(root.path(), b"version 1");
    store
        .assistant_worker(
            &s.id,
            WorkerOperation::Submit {
                contract_revision: 1,
                evidence: "Content checked".into(),
                artifacts,
            },
        )
        .unwrap();
    let question = store
        .assistant_worker(
            &s.id,
            WorkerOperation::Ask {
                title: "Which locale?".into(),
                context: "Product decision".into(),
                options: vec!["English".into(), "Chinese".into()],
                blocking: true,
                factual: false,
            },
        )
        .unwrap();
    let before = store.assistant_state().unwrap();
    assert!(store
        .edit_assistant(
            before.revision,
            AssistantEdit::Review {
                goal_id: state.goals[0].id.clone(),
                verdict: "accepted".into(),
                evidence: "Independent file check".into()
            }
        )
        .is_err());
    let answered = store
        .edit_assistant(
            before.revision,
            AssistantEdit::Answer {
                question_id: question["question_id"].as_str().unwrap().into(),
                answer: "Chinese".into(),
            },
        )
        .unwrap();
    assert!(answered
        .messages
        .iter()
        .any(|m| m.reply_to.as_deref() == question["question_id"].as_str()));
    // Requirements change while the answer is queued: all old questions/results become stale.
    let changed = store
        .edit_assistant(
            answered.revision,
            AssistantEdit::Goal {
                id: Some(state.goals[0].id.clone()),
                project_path: "/project-a".into(),
                title: "Updated report".into(),
                acceptance: "Version 2".into(),
                priority: 1,
            },
        )
        .unwrap();
    assert_eq!(changed.goals[0].contract_revision, 2);
    assert!(changed.goals[0].assignments[0].stop_requested);
    assert!(changed.goals[0].assignments[0].result.is_none());
    assert!(store
        .assistant_worker(
            &s.id,
            WorkerOperation::Confirm {
                contract_revision: 1,
                message_ids: vec![]
            }
        )
        .is_err());
    assert!(store
        .assistant_worker(
            &s.id,
            WorkerOperation::Submit {
                contract_revision: 1,
                evidence: "stale".into(),
                artifacts: stamps(root.path(), b"stale")
            }
        )
        .is_err());
}
#[test]
fn native_question_retry_clears_the_rejected_attempt_and_permissions_remain_specific() {
    let store = Store::open_in_memory().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (_, s) = managed(&store, root.path());
    store
        .assistant_worker(
            &s.id,
            WorkerOperation::Ask {
                title: "Read a file?".into(),
                context: String::new(),
                options: vec![],
                blocking: true,
                factual: false,
            },
        )
        .unwrap();
    let mut state = store.assistant_state().unwrap();
    let id = state.questions[0].id.clone();
    state.questions[0].source_input=Some(serde_json::from_value(serde_json::json!({"input_id":"input-1","kind":"permission","title":"Read a file?","options":[["allow_once","Allow once"]],"sequence":1})).unwrap());
    state.questions[0].answer_delivery_id = Some("native:rejected".into());
    state = store.save_assistant(state.revision, state).unwrap();
    assert!(store
        .edit_assistant(
            state.revision,
            AssistantEdit::Answer {
                question_id: id.clone(),
                answer: "invented_option".into()
            }
        )
        .is_err());
    let answered = store
        .edit_assistant(
            state.revision,
            AssistantEdit::Answer {
                question_id: id,
                answer: "allow_once".into(),
            },
        )
        .unwrap();
    assert!(answered.questions[0].answer_delivery_id.is_none());
    assert_eq!(answered.questions[0].answered_by.as_deref(), Some("user"));
}
#[test]
fn dependency_files_and_submitted_input_versions_cannot_be_laundered_by_confirmation() {
    let store = Store::open_in_memory().unwrap();
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let (mut state, sa) = managed(&store, a.path());
    let gid_a = state.goals[0].id.clone();
    let artifacts = stamps(a.path(), b"a1");
    store
        .assistant_worker(
            &sa.id,
            WorkerOperation::Submit {
                contract_revision: 1,
                evidence: "a1 checked".into(),
                artifacts,
            },
        )
        .unwrap();
    state = store.assistant_state().unwrap();
    state = store
        .edit_assistant(
            state.revision,
            AssistantEdit::Review {
                goal_id: gid_a.clone(),
                verdict: "accepted".into(),
                evidence: "Actual a1 content".into(),
            },
        )
        .unwrap();
    state = store
        .edit_assistant(
            state.revision,
            AssistantEdit::Goal {
                id: None,
                project_path: "/project-a".into(),
                title: "Dependent report".into(),
                acceptance: "Uses accepted A".into(),
                priority: 1,
            },
        )
        .unwrap();
    let gid_b = state.goals[1].id.clone();
    state = store
        .edit_assistant(
            state.revision,
            AssistantEdit::Dependencies {
                goal_id: gid_b.clone(),
                dependencies: vec![gid_a.clone()],
            },
        )
        .unwrap();
    state = apply_decision(
        &state,
        &[sa.clone()],
        AssistantDecision {
            summary: "Use the input".into(),
            actions: vec![AssistantAction::Dispatch {
                goal_id: gid_b.clone(),
                instruction: "Read accepted A".into(),
            }],
        },
    )
    .unwrap();
    let mut sb = Session::new(ProviderId::Codex, b.path().to_string_lossy().to_string());
    sb.project_path = Some("/project-a".into());
    store.upsert_session(&sb).unwrap();
    state.goals[1].assignments[0].session_id = Some(sb.id.clone());
    state = store.save_assistant(state.revision, state).unwrap();
    let version = state.goals[1].contract_revision;
    store
        .assistant_worker(
            &sb.id,
            WorkerOperation::Confirm {
                contract_revision: version,
                message_ids: vec![],
            },
        )
        .unwrap();
    store
        .assistant_worker(
            &sb.id,
            WorkerOperation::Submit {
                contract_revision: version,
                evidence: "Uses A1".into(),
                artifacts: stamps(b.path(), b"b from a1"),
            },
        )
        .unwrap();
    // No periodic reconcile occurs between editing A and attempting to accept B.
    std::fs::write(a.path().join("report.txt"), b"a changed externally").unwrap();
    state = store.assistant_state().unwrap();
    assert!(store
        .edit_assistant(
            state.revision,
            AssistantEdit::Review {
                goal_id: gid_b.clone(),
                verdict: "accepted".into(),
                evidence: "B file checked".into()
            }
        )
        .is_err());
    // Simulate a new accepted input version. Merely confirming B must discard its old result.
    state.goals[0].contract_revision += 1;
    state.goals[0].verdict.as_mut().unwrap().artifacts = stamps(a.path(), b"a2");
    state = store.save_assistant(state.revision, state).unwrap();
    assert!(store
        .assistant_worker(
            &sb.id,
            WorkerOperation::Confirm {
                contract_revision: version,
                message_ids: vec![],
            }
        )
        .is_err());
    state.goals[0].verdict.as_mut().unwrap().contract_revision = state.goals[0].contract_revision;
    store.save_assistant(state.revision, state).unwrap();
    store
        .assistant_worker(
            &sb.id,
            WorkerOperation::Confirm {
                contract_revision: version,
                message_ids: vec![],
            },
        )
        .unwrap();
    state = store.assistant_state().unwrap();
    assert!(state.goals[1].assignments[0].result.is_none());
    assert!(store
        .edit_assistant(
            state.revision,
            AssistantEdit::Review {
                goal_id: gid_b,
                verdict: "accepted".into(),
                evidence: "Only confirmed".into()
            }
        )
        .is_err());
}

#[test]
fn a_recorded_native_answer_is_revoked_atomically_before_changed_control_can_deliver_it() {
    for change in ["revision", "stop", "cancel", "takeover", "remove_scope"] {
        let store = Store::open_in_memory().unwrap();
        let root = tempfile::tempdir().unwrap();
        let (mut state, _) = managed(&store, root.path());
        let goal = state.goals[0].clone();
        let assignment = goal.assignments[0].clone();
        let q:CoordinationQuestion=serde_json::from_value(serde_json::json!({"id":"native:q","goal_id":goal.id,"assignment_id":assignment.id,"contract_revision":1,"author":"worker","title":"Allow read?","context":"Concrete operation","options":[],"blocking":true,"state":"answered","answer":"allow_once","answered_by":"user","source_input":{"input_id":"input","kind":"permission","title":"Read","options":[["allow_once","Allow once"]],"sequence":1},"request_id":null,"answer_delivery_id":null})).unwrap();
        assert!(native_answer_valid(&state, &q));
        state.questions.push(q);
        match change {
            "revision" => state.goals[0].contract_revision += 1,
            "stop" => state.goals[0].assignments[0].stop_requested = true,
            "cancel" => state.goals[0].status = GoalStatus::Cancelled,
            "takeover" => state.goals[0].assignments[0].taken_over = true,
            "remove_scope" => state.settings.as_mut().unwrap().projects.clear(),
            _ => unreachable!(),
        }
        let state = store.save_assistant(state.revision, state).unwrap();
        assert!(!native_answer_valid(&state, &state.questions[0]));
        assert_eq!(state.questions[0].state, "expired");
        assert_eq!(
            state.questions[0].answer_delivery_id.as_deref(),
            Some("control:revoked")
        );
    }
}

#[test]
fn unknown_delivery_and_claimed_native_answers_block_submission_and_acceptance() {
    for transport in [
        "recorded",
        "queued",
        "submitting",
        "unknown",
        "accepted",
        "failed",
    ] {
        let store = Store::open_in_memory().unwrap();
        let root = tempfile::tempdir().unwrap();
        let (_state, session) = managed(&store, root.path());
        let artifacts = stamps(root.path(), b"current result");
        store
            .assistant_worker(
                &session.id,
                WorkerOperation::Submit {
                    contract_revision: 1,
                    evidence: "checked".into(),
                    artifacts: artifacts.clone(),
                },
            )
            .unwrap();
        let mut state = store.assistant_state().unwrap();
        let goal = state.goals[0].clone();
        let id = add_message(
            &mut state,
            &goal.id,
            "Required correction".into(),
            "auto",
            "user",
            None,
        )
        .unwrap();
        state
            .messages
            .iter_mut()
            .find(|m| m.id == id)
            .unwrap()
            .state = transport.into();
        let state = store.save_assistant(state.revision, state).unwrap();
        assert!(
            store
                .assistant_worker(
                    &session.id,
                    WorkerOperation::Submit {
                        contract_revision: 1,
                        evidence: "checked".into(),
                        artifacts
                    }
                )
                .is_err(),
            "{transport}"
        );
        assert!(
            store
                .edit_assistant(
                    state.revision,
                    AssistantEdit::Review {
                        goal_id: goal.id,
                        verdict: "accepted".into(),
                        evidence: "checked".into()
                    }
                )
                .is_err(),
            "{transport}"
        );
    }
    for outcome in ["delivery_unknown", "expired", "answered"] {
        let store = Store::open_in_memory().unwrap();
        let root = tempfile::tempdir().unwrap();
        let (mut state, session) = managed(&store, root.path());
        let artifacts = stamps(root.path(), b"result");
        let goal = state.goals[0].clone();
        state.questions.push(serde_json::from_value(serde_json::json!({
            "id":"uncertain-answer", "goal_id":goal.id,"assignment_id":goal.assignments[0].id,
            "contract_revision":1,"author":"worker","title":"Permission?","context":"Pending operation",
            "options":[],"blocking":true,"state":outcome,"answer":"allow_once","answered_by":"user",
            "source_input":null,"request_id":null,"answer_delivery_id":"native:claimed"
        })).unwrap());
        store.save_assistant(state.revision, state).unwrap();
        assert!(
            store
                .assistant_worker(
                    &session.id,
                    WorkerOperation::Submit {
                        contract_revision: 1,
                        evidence: "checked".into(),
                        artifacts
                    }
                )
                .is_err(),
            "{outcome}"
        );
    }
}

#[test]
fn one_free_slot_cannot_admit_two_independent_waiters() {
    let store = Store::open_in_memory().unwrap();
    let mut state = configured(&store);
    let mut second = state.goals[0].clone();
    second.id = "independent-waiter".into();
    state.goals.push(second);
    let first = apply_decision(&state, &[], dispatch(&state)).unwrap();
    // The first durable intent reserves capacity even before session creation starts.
    let second_decision = AssistantDecision {
        summary: "The other waiter also saw a free slot".into(),
        actions: vec![AssistantAction::Dispatch {
            goal_id: state.goals[1].id.clone(),
            instruction: "Work only after capacity is available".into(),
        }],
    };
    assert!(matches!(
        apply_decision(&first, &[], second_decision),
        Err(codetwo_core::store::StoreError::AssistantConcurrencyLimit)
    ));
    assert_eq!(first.goals[0].assignments.len(), 1);
    assert!(first.goals[1].assignments.is_empty());
}

// ---- Agent-first conversation intake -------------------------------------------------------

fn two_projects(store: &Store) -> AssistantState {
    let state = configured(store);
    let mut settings = state.settings.clone().unwrap();
    settings.projects = vec!["/project-a".into(), "/project-b".into()];
    store
        .edit_assistant(state.revision, AssistantEdit::Settings { settings })
        .unwrap()
}
fn say_edit(id: &str, content: &str) -> AssistantEdit {
    AssistantEdit::Say {
        turn_id: id.into(),
        content: content.into(),
        project_paths: vec![],
        answers_question: None,
        actor: None,
    }
}
/// What the runtime does when it claims a recorded message for an intake run.
fn claim(store: &Store, turn: &str, run: &str) -> AssistantState {
    let mut state = store.assistant_state().unwrap();
    let i = state
        .conversation
        .iter()
        .position(|t| t.id == turn)
        .unwrap();
    state.conversation[i].status = TurnStatus::Reviewing;
    state.conversation[i].source_run_id = Some(run.into());
    let stamps = turn_stamps(&state, &state.conversation[i]);
    state.intake_stamps.insert(run.into(), stamps);
    store.save_assistant(state.revision, state).unwrap()
}
fn decision(json: serde_json::Value) -> IntakeDecision {
    parse_intake(&json.to_string()).unwrap()
}

#[test]
fn say_is_idempotent_scoped_once_and_creates_no_goal() {
    let store = Store::open_in_memory().unwrap();
    let state = two_projects(&store);
    let one = store
        .edit_assistant(
            state.revision,
            say_edit("t1", "How is the login fix going?"),
        )
        .unwrap();
    assert_eq!(one.goals.len(), 1);
    assert_eq!(one.conversation.len(), 1);
    let turn = &one.conversation[0];
    assert_eq!(turn.status, TurnStatus::Recorded);
    assert_eq!(turn.project_paths, vec!["/project-a", "/project-b"]);
    // Lost response: replay at a stale revision is the same record, not a second one.
    let replay = store
        .edit_assistant(
            state.revision,
            say_edit("t1", "How is the login fix going?"),
        )
        .unwrap();
    assert_eq!(replay.revision, one.revision);
    assert_eq!(replay.conversation.len(), 1);
    assert!(store
        .edit_assistant(one.revision, say_edit("t1", "Something else"))
        .unwrap_err()
        .to_string()
        .contains("different content"));
    assert!(store
        .edit_assistant(
            one.revision,
            AssistantEdit::Say {
                turn_id: "t2".into(),
                content: "x".into(),
                project_paths: vec!["/elsewhere".into()],
                answers_question: None,
                actor: None,
            }
        )
        .is_err());
    // The persisted scope never widens when settings are later widened.
    let mut settings = one.settings.clone().unwrap();
    settings.projects.push("/project-c".into());
    store.add_project("/project-c", None, 0).unwrap();
    let wider = store
        .edit_assistant(one.revision, AssistantEdit::Settings { settings })
        .unwrap();
    assert_eq!(wider.conversation[0].project_paths.len(), 2);
    // Old documents keep loading.
    let mut raw = serde_json::to_value(&wider).unwrap();
    for key in [
        "conversation",
        "memory_proposals",
        "turn_args",
        "user_gen",
        "intake_stamps",
    ] {
        raw.as_object_mut().unwrap().remove(key);
    }
    let old: AssistantState = serde_json::from_value(raw).unwrap();
    assert!(old.conversation.is_empty() && old.memory_proposals.is_empty());
}

#[test]
fn multi_project_assignment_routes_to_independent_requests_and_links_goals() {
    let store = Store::open_in_memory().unwrap();
    let state = two_projects(&store);
    let state = store
        .edit_assistant(
            state.revision,
            say_edit("t1", "Watch A login and B release"),
        )
        .unwrap();
    let state = claim(&store, "t1", "run-1");
    let done = apply_intake(
        &state,
        &[],
        "run-1",
        decision(serde_json::json!({"summary":"Routed both","actions":[
            {"kind":"route","project_path":"/project-a","content":"Watch login"},
            {"kind":"route","project_path":"/project-b","content":"Prepare release"}]})),
    )
    .unwrap();
    assert_eq!(done.requests.len(), 2);
    assert!(done
        .requests
        .iter()
        .all(|r| r.turn_id.as_deref() == Some("t1")));
    let reply = done
        .conversation
        .iter()
        .find(|t| t.id == "reply:t1")
        .unwrap();
    assert_eq!(reply.reply_to.as_deref(), Some("t1"));
    assert_eq!(reply.source_run_id.as_deref(), Some("run-1"));
    assert_eq!(done.conversation[0].status, TurnStatus::Handled);
    // Each request is a separate scope: handling one cannot fail the other project.
    let a = done.requests[0].id.clone();
    let b = done.requests[1].id.clone();
    let create = |id: &str| AssistantDecision {
        summary: "created".into(),
        actions: vec![AssistantAction::CreateGoal {
            request_id: id.into(),
            title: "Goal".into(),
            acceptance: "Done".into(),
            priority: 1,
        }],
    };
    let after_a = apply_decision(&done, &[], create(&a)).unwrap();
    let after_b = apply_decision(&after_a, &[], create(&b)).unwrap();
    assert_eq!(after_b.goals.len(), 3);
    assert_eq!(after_b.conversation[0].goal_ids.len(), 2);
    // A handled message is never interpreted again.
    assert!(apply_intake(
        &done,
        &[],
        "run-1",
        decision(serde_json::json!({"summary":"again"}))
    )
    .is_err());
}

#[test]
fn discussion_replies_without_actions_and_stays_handled() {
    let store = Store::open_in_memory().unwrap();
    let state = two_projects(&store);
    let state = store
        .edit_assistant(state.revision, say_edit("t1", "What do you think?"))
        .unwrap();
    let state = claim(&store, "t1", "run-1");
    let done = apply_intake(
        &state,
        &[],
        "run-1",
        decision(serde_json::json!({"summary":"My view"})),
    )
    .unwrap();
    assert_eq!(done.goals.len(), 1);
    assert!(done.requests.is_empty());
    assert_eq!(done.conversation.len(), 2);
    assert_eq!(done.conversation[0].status, TurnStatus::Handled);
    assert!(done.intake_stamps.is_empty());
}

#[test]
fn explicit_controls_reuse_original_logic_and_old_decisions_never_win() {
    let store = Store::open_in_memory().unwrap();
    let state = two_projects(&store);
    let dispatched = apply_decision(&state, &[], dispatch(&state)).unwrap();
    let state = store.save_assistant(state.revision, dispatched).unwrap();
    let goal = state.goals[0].id.clone();
    let state = store
        .edit_assistant(
            state.revision,
            say_edit("t1", "Please stop A and make it urgent"),
        )
        .unwrap();
    let state = claim(&store, "t1", "run-1");
    let act =
        |kind: serde_json::Value| decision(serde_json::json!({"summary":"ok","actions":[kind]}));
    // Quotes must come from the original message; guessed authorization fails.
    assert!(apply_intake(&state, &[], "run-1", act(serde_json::json!({"kind":"control","goal_id":goal,"operation":"cancel","quote":"cancel everything"}))).is_err());
    // Another project's goal cannot be reached by naming it.
    let mut hinted = store.assistant_state().unwrap();
    hinted.conversation[0].project_paths = vec!["/project-b".into()];
    assert!(apply_intake(
        &hinted,
        &[],
        "run-1",
        act(
            serde_json::json!({"kind":"set_priority","goal_id":goal,"priority":0,"quote":"urgent"})
        )
    )
    .is_err());
    let stopped = apply_intake(
        &state,
        &[],
        "run-1",
        decision(serde_json::json!({"summary":"Stopping A","actions":[
            {"kind":"control","goal_id":goal,"operation":"stop","quote":"stop A"},
            {"kind":"set_priority","goal_id":goal,"priority":0,"quote":"urgent"}]})),
    )
    .unwrap();
    assert_eq!(stopped.goals[0].status, GoalStatus::Paused);
    assert!(stopped.goals[0].assignments[0].stop_requested);
    assert_eq!(stopped.goals[0].priority, 0);
    assert_eq!(stopped.conversation[0].goal_ids, vec![goal.clone()]);
    // A newer explicit user operation on the same goal invalidates the older interpretation...
    let newer = store
        .edit_assistant(
            state.revision,
            AssistantEdit::Goal {
                id: Some(goal.clone()),
                project_path: "/project-a".into(),
                title: state.goals[0].title.clone(),
                acceptance: state.goals[0].acceptance.clone(),
                priority: 2,
            },
        )
        .unwrap();
    let err = apply_intake(
        &newer,
        &[],
        "run-1",
        act(
            serde_json::json!({"kind":"set_priority","goal_id":goal,"priority":0,"quote":"urgent"}),
        ),
    )
    .unwrap_err();
    assert!(err.to_string().contains(STALE_INTAKE), "{err}");
    // ...but a change to an unrelated goal does not.
    let mut other = state.clone();
    other.goals.push({
        let mut g = other.goals[0].clone();
        g.id = "b-goal".into();
        g.project_path = "/project-b".into();
        g
    });
    other.goals[1].priority = 3;
    assert!(apply_intake(
        &other,
        &[],
        "run-1",
        act(
            serde_json::json!({"kind":"set_priority","goal_id":goal,"priority":0,"quote":"urgent"})
        )
    )
    .is_ok());
}

#[test]
fn direction_change_uses_versioned_revision_and_pause_followup_keeps_worker() {
    let store = Store::open_in_memory().unwrap();
    let state = two_projects(&store);
    let dispatched = apply_decision(&state, &[], dispatch(&state)).unwrap();
    let state = store.save_assistant(state.revision, dispatched).unwrap();
    let goal = state.goals[0].id.clone();
    let state = store
        .edit_assistant(
            state.revision,
            say_edit("t1", "Switch A to the OAuth flow, pause follow-up"),
        )
        .unwrap();
    let state = claim(&store, "t1", "run-1");
    let done = apply_intake(&state, &[], "run-1", decision(serde_json::json!({"summary":"Updated","actions":[
        {"kind":"change","goal_id":goal,"title":"OAuth login","acceptance":"OAuth passes","reason":"user redirected","quote":"Switch A to the OAuth flow"}]}))).unwrap();
    assert_eq!(done.goals[0].contract_revision, 2);
    assert_eq!(done.changes.len(), 1);
    let paused = apply_intake(&state, &[], "run-1", decision(serde_json::json!({"summary":"Paused","actions":[
        {"kind":"control","goal_id":goal,"operation":"pause_followup","quote":"pause follow-up"}]}))).unwrap();
    assert_eq!(paused.goals[0].status, GoalStatus::Paused);
    assert!(!paused.goals[0].assignments[0].stop_requested);
}

#[test]
fn answers_question_uses_the_original_path_and_never_approves_permissions() {
    let store = Store::open_in_memory().unwrap();
    let state = two_projects(&store);
    let dispatched = apply_decision(&state, &[], dispatch(&state)).unwrap();
    let mut state = store.save_assistant(state.revision, dispatched).unwrap();
    let goal = state.goals[0].id.clone();
    let q = open_question(
        &mut state,
        &goal,
        "Language?".into(),
        String::new(),
        vec!["EN".into()],
        true,
        "assistant",
    )
    .unwrap();
    let state = store.save_assistant(state.revision, state).unwrap();
    let say = |id: &str, q: &str| AssistantEdit::Say {
        turn_id: id.into(),
        content: "EN".into(),
        project_paths: vec![],
        answers_question: Some(q.into()),
        actor: None,
    };
    let answered = store.edit_assistant(state.revision, say("t1", &q)).unwrap();
    assert_eq!(answered.questions[0].state, "answered");
    assert_eq!(answered.questions[0].answered_by.as_deref(), Some("user"));
    assert_eq!(answered.conversation[0].status, TurnStatus::Handled);
    assert_eq!(answered.conversation[0].question_ids, vec![q.clone()]);
    assert!(store
        .edit_assistant(answered.revision, say("t2", &q))
        .is_err());
    assert!(store
        .edit_assistant(answered.revision, say("t3", "missing"))
        .is_err());
}

#[test]
fn acknowledgement_cannot_cancel_change_or_create_work() {
    let store = Store::open_in_memory().unwrap();
    let state = two_projects(&store);
    let state = store
        .save_assistant(
            state.revision,
            apply_decision(&state, &[], dispatch(&state)).unwrap(),
        )
        .unwrap();
    let goal = state.goals[0].id.clone();
    let state = store
        .edit_assistant(state.revision, say_edit("t1", "好的！"))
        .unwrap();
    let state = claim(&store, "t1", "run-1");
    for action in [
        serde_json::json!({"kind":"control","goal_id":goal,"operation":"cancel","quote":"好的"}),
        serde_json::json!({"kind":"change","goal_id":goal,"title":"Changed","acceptance":"Changed checks","reason":"yes","quote":"好的"}),
        serde_json::json!({"kind":"route","project_path":"/project-a","content":"Start new work"}),
    ] {
        assert!(apply_intake(
            &state,
            &[],
            "run-1",
            decision(serde_json::json!({"summary":"x","actions":[action]}))
        )
        .is_err());
    }
    let after = store.assistant_state().unwrap();
    assert_eq!(after.goals[0].contract_revision, 1);
    assert!(!after.goals[0].assignments[0].stop_requested);
    assert!(after.changes.is_empty());
    assert!(after.requests.is_empty());
}

#[test]
fn conflicting_supplement_and_direction_change_fail_atomically() {
    let store = Store::open_in_memory().unwrap();
    let state = two_projects(&store);
    let state = store
        .save_assistant(
            state.revision,
            apply_decision(&state, &[], dispatch(&state)).unwrap(),
        )
        .unwrap();
    let goal = state.goals[0].id.clone();
    let state = store
        .edit_assistant(
            state.revision,
            say_edit("t1", "Switch A to OAuth and include the Chinese report"),
        )
        .unwrap();
    let state = claim(&store, "t1", "run-1");
    let error = apply_intake(&state, &[], "run-1", decision(serde_json::json!({"summary":"Changed","actions":[
        {"kind":"message","goal_id":goal,"content":"Include the Chinese report","quote":"include the Chinese report"},
        {"kind":"change","goal_id":goal,"title":"OAuth","acceptance":"OAuth checks","reason":"requested","quote":"Switch A to OAuth"}
    ]}))).unwrap_err();
    assert!(error.to_string().contains("include supplements"));
    let after = store.assistant_state().unwrap();
    assert!(after.messages.is_empty());
    assert!(after.changes.is_empty());
    assert_eq!(after.goals[0].contract_revision, 1);
    assert_eq!(
        after.conversation[0].content,
        "Switch A to OAuth and include the Chinese report"
    );
}

#[test]
fn memory_candidates_need_exact_hash_and_do_not_become_memory_by_themselves() {
    let store = Store::open_in_memory().unwrap();
    let state = two_projects(&store);
    let state = store
        .edit_assistant(state.revision, say_edit("t1", "Remember A uses OAuth only"))
        .unwrap();
    let state = claim(&store, "t1", "run-1");
    let done = apply_intake(&state, &[], "run-1", decision(serde_json::json!({"summary":"Noted","actions":[
        {"kind":"propose_memory","project_path":"/project-a","category":"constraint","content":"A uses OAuth only"},
        {"kind":"propose_memory","project_path":"/project-a","category":"constraint","content":"A uses OAuth only"}]}))).unwrap();
    assert_eq!(done.memory_proposals.len(), 1);
    assert!(store.list_memories("/project-a", 10).unwrap().is_empty());
    let p = done.memory_proposals[0].clone();
    assert_eq!(p.state, ProposalState::Proposed);
    // Global scope is not reachable from a project conversation; bad category fails.
    for (path, category) in [(GLOBAL_MEMORY, "fact"), ("/project-a", "nonsense")] {
        assert!(apply_intake(
            &state,
            &[],
            "run-1",
            decision(serde_json::json!({"summary":"x","actions":[
            {"kind":"propose_memory","project_path":path,"category":category,"content":"y"}]}))
        )
        .is_err());
    }
    let state = store.save_assistant(state.revision, done).unwrap();
    assert!(store
        .edit_assistant(
            state.revision,
            AssistantEdit::ConfirmMemory {
                proposal_id: p.id.clone(),
                content_hash: "bad".into()
            }
        )
        .is_err());
    let confirmed = store
        .edit_assistant(
            state.revision,
            AssistantEdit::ConfirmMemory {
                proposal_id: p.id.clone(),
                content_hash: p.content_hash.clone(),
            },
        )
        .unwrap();
    assert_eq!(
        confirmed.memory_proposals[0].state,
        ProposalState::Confirmed
    );
    assert!(confirmed.memory_proposals[0].memory_id.is_none());
    assert!(store
        .edit_assistant(
            confirmed.revision,
            AssistantEdit::RejectMemory {
                proposal_id: p.id.clone(),
                content_hash: p.content_hash.clone()
            }
        )
        .is_err());
}

#[test]
fn conversation_capacity_reports_instead_of_dropping_unhandled_messages() {
    let store = Store::open_in_memory().unwrap();
    let mut state = two_projects(&store);
    let mut failed = None;
    for i in 0..30 {
        match store.edit_assistant(state.revision, say_edit(&format!("t{i}"), "hello")) {
            Ok(next) => state = next,
            Err(e) => {
                failed = Some(e.to_string());
                break;
            }
        }
    }
    assert!(failed.unwrap().contains("limit"));
    assert_eq!(state.conversation.len(), MAX_OUTSTANDING_TURNS);
}

#[test]
fn same_goal_stop_and_priority_commit_together_and_bump_the_user_generation() {
    let store = Store::open_in_memory().unwrap();
    let state = two_projects(&store);
    let dispatched = apply_decision(&state, &[], dispatch(&state)).unwrap();
    let state = store.save_assistant(state.revision, dispatched).unwrap();
    let goal = state.goals[0].id.clone();
    let before = state.user_gen.get(&goal).copied().unwrap_or(0);
    let state = store
        .edit_assistant(state.revision, say_edit("t1", "stop A and make it urgent"))
        .unwrap();
    let state = claim(&store, "t1", "run-1");
    let done = apply_intake(
        &state,
        &[],
        "run-1",
        decision(serde_json::json!({"summary":"ok","actions":[
        {"kind":"control","goal_id":goal,"operation":"stop","quote":"stop A"},
        {"kind":"set_priority","goal_id":goal,"priority":0,"quote":"urgent"}]})),
    )
    .unwrap();
    assert_eq!(done.goals[0].priority, 0);
    assert!(done.goals[0].assignments[0].stop_requested);
    assert_eq!(done.user_gen.get(&goal).copied().unwrap_or(0), before + 1);
    // A failing action rejects the whole group: nothing is half applied.
    assert!(apply_intake(
        &state,
        &[],
        "run-1",
        decision(serde_json::json!({"summary":"ok","actions":[
        {"kind":"set_priority","goal_id":goal,"priority":0,"quote":"urgent"},
        {"kind":"set_priority","goal_id":goal,"priority":9,"quote":"urgent"}]}))
    )
    .is_err());
}

#[test]
fn a_late_started_earlier_message_cannot_override_a_later_user_decision() {
    let store = Store::open_in_memory().unwrap();
    let state = two_projects(&store);
    let state = store
        .edit_assistant(
            state.revision,
            AssistantEdit::Goal {
                id: None,
                project_path: "/project-b".into(),
                title: "Prepare release".into(),
                acceptance: "Release notes are ready".into(),
                priority: 3,
            },
        )
        .unwrap();
    let a = state.goals[0].id.clone();
    let b = state.goals[1].id.clone();
    let state = store
        .edit_assistant(state.revision, say_edit("t1", "make A urgent"))
        .unwrap();
    let state = store
        .edit_assistant(state.revision, say_edit("t2", "relax A for now"))
        .unwrap();
    // The later message runs and commits first.
    let state = claim(&store, "t2", "run-2");
    let later = apply_intake(
        &state,
        &[],
        "run-2",
        decision(serde_json::json!({"summary":"ok","actions":[
        {"kind":"set_priority","goal_id":a,"priority":3,"quote":"relax A"}]})),
    )
    .unwrap();
    let state = store.save_assistant(state.revision, later).unwrap();
    assert_eq!(state.goals[0].priority, 3);
    // The earlier message starts late; its stamps are fresh but user order still wins.
    let state = claim(&store, "t1", "run-1");
    let err = apply_intake(
        &state,
        &[],
        "run-1",
        decision(serde_json::json!({"summary":"ok","actions":[
        {"kind":"set_priority","goal_id":a,"priority":0,"quote":"urgent"}]})),
    )
    .unwrap_err();
    assert!(err.to_string().contains(STALE_INTAKE), "{err}");
    // An unrelated goal decided by nobody else is not discarded.
    let ok = apply_intake(
        &state,
        &[],
        "run-1",
        decision(serde_json::json!({"summary":"ok","actions":[
        {"kind":"set_priority","goal_id":b,"priority":0,"quote":"urgent"}]})),
    )
    .unwrap();
    assert_eq!(ok.goals[1].priority, 0);
}

#[test]
fn known_failed_memory_can_be_reconfirmed_but_unknown_outcome_is_never_rewritten() {
    let store = Store::open_in_memory().unwrap();
    let state = two_projects(&store);
    let state = store
        .edit_assistant(state.revision, say_edit("t1", "Remember A uses OAuth only"))
        .unwrap();
    let state = claim(&store, "t1", "run-1");
    let done = apply_intake(&state, &[], "run-1", decision(serde_json::json!({"summary":"Noted","actions":[
        {"kind":"propose_memory","project_path":"/project-a","category":"constraint","content":"A uses OAuth only"}]}))).unwrap();
    let p = done.memory_proposals[0].clone();
    let key = format!("memory:{}", p.id);
    let state = store.save_assistant(state.revision, done).unwrap();
    let confirm = |revision| {
        store.edit_assistant(
            revision,
            AssistantEdit::ConfirmMemory {
                proposal_id: p.id.clone(),
                content_hash: p.content_hash.clone(),
            },
        )
    };
    // Known failure: the Memory Store refused, nothing was written; retry clears that attempt.
    let mut known = state.clone();
    known.memory_proposals[0].state = ProposalState::Failed;
    known.memory_proposals[0].error = Some("disk full".into());
    known.attempts.insert(
        key.clone(),
        AssistantAttempt {
            state: "failed".into(),
            outcome: "disk full".into(),
        },
    );
    let known = store.save_assistant(known.revision, known).unwrap();
    let retried = confirm(known.revision).unwrap();
    assert_eq!(retried.memory_proposals[0].state, ProposalState::Confirmed);
    assert!(retried.memory_proposals[0].error.is_none());
    assert!(!retried.attempts.contains_key(&key));
    // Unknown outcome: the write may exist, so confirm cannot reset it for a second write.
    let mut unknown = retried.clone();
    unknown.memory_proposals[0].state = ProposalState::Failed;
    unknown.attempts.insert(
        key.clone(),
        AssistantAttempt {
            state: "started".into(),
            outcome: String::new(),
        },
    );
    let unknown = store.save_assistant(unknown.revision, unknown).unwrap();
    let err = confirm(unknown.revision).unwrap_err();
    assert!(err.to_string().contains("unknown"), "{err}");
    let after = store.assistant_state().unwrap();
    assert_eq!(after.memory_proposals[0].state, ProposalState::Failed);
    assert!(after.attempts.contains_key(&key));
}

#[test]
fn confirming_a_proposal_never_revives_a_forgotten_note() {
    let store = Store::open_in_memory().unwrap();
    two_projects(&store);
    let old = store
        .add_memory("/project-a", "constraint", "A uses OAuth only", true)
        .unwrap();
    store.set_memory_active(&old.id, false).unwrap();
    // The confirmation write path is add_memory with the exact proposed content.
    let fresh = store
        .add_memory("/project-a", "constraint", "A uses OAuth only", true)
        .unwrap();
    assert_ne!(fresh.id, old.id);
    let active: Vec<_> = store
        .list_memories("/project-a", 10)
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(active, vec![fresh.id]);
}

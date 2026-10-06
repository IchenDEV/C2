    #[tokio::test]
    async fn supervisor_resource_release_retries_waiting_independent_scope() {
        use crate::session::{Session, SessionActivity};
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open_in_memory().unwrap());
        let (runtime, _rx) = runtime(store.clone(), temp.path()).await;
        let project = temp.path().canonicalize().unwrap().to_string_lossy().to_string();
        std::fs::write(temp.path().join("report.txt"), b"fixture report v1").unwrap();
        store.add_project(&project, Some("fixture"), 0).unwrap();
        let mut state = store.edit_assistant(0, AssistantEdit::Settings {
            settings: AssistantSettings { enabled: true, projects: vec![project.clone()],
                provider: ProviderId::Grok, model: None, reasoning_effort: None,
                concurrency: 2, turn_limit: 20, dispatch_limit: 10 }
        }).unwrap();
        for title in ["Busy A", "Busy B", "Waiting C"] {
            state = store.edit_assistant(state.revision, AssistantEdit::Goal {
                id: None, project_path: project.clone(), title: title.into(),
                acceptance: "Versioned report".into(), priority: 1
            }).unwrap();
        }
        let waiting_id = state.goals[2].id.clone();
        let mut occupied = Vec::new();
        for index in 0..2 {
            let mut session = Session::new(ProviderId::Grok, project.clone());
            session.project_path = Some(project.clone());
            store.upsert_session(&session).unwrap();
            assert!(store.update_session_activity(&session.id, 0, &SessionActivity {
                revision: 1, state: SessionRunState::Running {
                    turn_id: format!("busy-{index}"), prompt_request_id: None
                }
            }).unwrap());
            occupied.push(session.id.clone());
            state.goals[index].status = GoalStatus::Paused;
            state.goals[index].assignments.push(serde_json::from_value(j!({
                "id": format!("existing-{index}"), "instruction": "Existing worker",
                "session_id": session.id, "submitted": true, "taken_over": false,
                "owned": false
            })).unwrap());
        }
        store.save_assistant(state.revision, state).unwrap();
        let first = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                runtime.tick().await.unwrap();
                let state = store.assistant_state().unwrap();
                if state.scoped_runs.iter().any(|r| r.scope == waiting_id &&
                    r.state == "failed" && r.error.as_deref().is_some_and(|e| e.contains("concurrency limit reached"))) {
                    return state;
                }
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
        }).await;
        if first.is_err() { runtime.engine.shutdown(); }
        let first = first.expect("C was reviewed while unrelated A/B occupied the two worker slots");
        eprintln!("supervisor: initial C failed with full resources; turns={}", first.turns);
        for id in occupied {
            assert!(store.update_session_activity(&id, 1, &SessionActivity {
                revision: 2, state: SessionRunState::Idle
            }).unwrap());
        }
        let result = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                runtime.tick().await.unwrap();
                let state = store.assistant_state().unwrap();
                if state.goals.iter().find(|g| g.id == waiting_id).unwrap().assignments.len() > 0 {
                    return state;
                }
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
        }).await;
        let final_state = store.assistant_state().unwrap();
        eprintln!("supervisor: after resource release C assignments={}, turns={}, reviews={:?}",
            final_state.goals.iter().find(|g| g.id == waiting_id).unwrap().assignments.len(),
            final_state.turns,
            final_state.scoped_runs.iter().map(|r| (&r.scope,&r.state,&r.error)).collect::<Vec<_>>());
        runtime.engine.shutdown();
        assert!(result.is_ok(), "independent C must resume automatically after unrelated A/B release worker slots");
    }

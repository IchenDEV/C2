//! Conformance: the six example definitions under `schemas/agent-scenes/1.0.0/examples/` are
//! normative fixtures (docs/reference/scenes.md §Versioning & conformance). A conforming implementation
//! must load all of them and reproduce the authored structure.

use codetwo_core::scene::{Gate, SceneLibrary, TransitionTrigger};
use codetwo_core::{SceneSessionMode, SceneWorktree, SlotKind};

#[test]
fn legacy_loader_ignores_a_scenes_v2_document() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("software-development.scene.json"),
        r#"{
          "$schema": "https://codetwo.app/schemas/scenes/2.0.0/scene.schema.json",
          "id": "official:software-development",
          "version": "2.0.0",
          "title": "Software Development",
          "description": "Software engineering domain context.",
          "domain": "software-development",
          "provenance": { "kind": "official" },
          "agent_skill_selectors": ["software-development"],
          "capability_namespaces": ["workspace", "source-control"]
        }"#,
    )
    .unwrap();

    let library = SceneLibrary::load(Some(dir.path()), None, &[]);

    assert!(library
        .scenes()
        .iter()
        .all(|entry| entry.scene.name != "software-development"));
}

#[test]
fn all_six_fixtures_load() {
    let lib = SceneLibrary::builtin();
    let names: Vec<&str> = lib.scenes().iter().map(|s| s.scene.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["research", "develop", "test", "fix", "acceptance"]
    );
    assert_eq!(lib.pipelines().len(), 1);
    assert_eq!(lib.pipelines()[0].pipeline.name, "rnd-lifecycle");
    assert!(lib.scenes().iter().all(|entry| entry.scene.icon.is_none()));
    assert!(lib
        .pipelines()
        .iter()
        .all(|entry| entry.pipeline.icon.is_none()));
}

#[test]
fn develop_brief_is_typed() {
    let lib = SceneLibrary::builtin();
    let develop = &lib.resolve("develop").unwrap().scene;
    let brief = develop.brief.as_ref().unwrap();
    assert_eq!(brief.slots.len(), 3);
    assert_eq!(brief.slots[0].id, "goal");
    assert_eq!(brief.slots[0].kind, SlotKind::Multiline);
    assert!(brief.slots[0].required);
    assert_eq!(brief.slots[2].id, "out-of-scope");
    assert!(!brief.slots[2].required);

    let execution = develop.execution.as_ref().unwrap();
    assert_eq!(execution.session_mode, Some(SceneSessionMode::AutoEdit));
    assert_eq!(execution.worktree, Some(SceneWorktree::Current));
    assert!(serde_json::to_value(execution)
        .unwrap()
        .get("plan_first")
        .is_none());

    let artifact_ids: Vec<&str> = develop.artifacts.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(artifact_ids, vec!["plan", "change-summary"]);
    assert!(develop.artifacts.iter().all(|a| a.required));
}

#[test]
fn rnd_lifecycle_graph_matches_authoring() {
    let lib = SceneLibrary::builtin();
    let pipeline = &lib.resolve_pipeline("rnd-lifecycle").unwrap().pipeline;

    assert_eq!(pipeline.entry.as_deref(), Some("research"));
    let stage_ids: Vec<&str> = pipeline.stages.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        stage_ids,
        vec!["research", "develop", "test", "fix", "acceptance"]
    );

    let acceptance = pipeline
        .stages
        .iter()
        .find(|s| s.id == "acceptance")
        .unwrap();
    assert_eq!(acceptance.gate, Some(Gate::Confirm));

    // The test stage carries from develop and (silently skippable) from fix, with an as-label.
    let test_stage = pipeline.stages.iter().find(|s| s.id == "test").unwrap();
    let fix_carry = test_stage.carry.iter().find(|c| c.from == "fix").unwrap();
    assert_eq!(fix_carry.artifact, "fix-summary");
    assert_eq!(fix_carry.as_label.as_deref(), Some("Latest fix summary"));

    assert_eq!(pipeline.transitions.len(), 4);
    let edge = |from: &str, to: &str| {
        pipeline
            .transitions
            .iter()
            .find(|t| t.from == from && t.to == to)
            .unwrap()
    };
    assert_eq!(edge("test", "fix").when, TransitionTrigger::TestsFailed);
    assert_eq!(
        edge("test", "acceptance").when,
        TransitionTrigger::ExitCriteriaMet
    );
    assert_eq!(edge("test", "acceptance").gate, Some(Gate::Confirm));
    assert_eq!(edge("fix", "test").when, TransitionTrigger::ExitCriteriaMet);
    assert_eq!(
        edge("acceptance", "develop").when,
        TransitionTrigger::UserRequest
    );
}

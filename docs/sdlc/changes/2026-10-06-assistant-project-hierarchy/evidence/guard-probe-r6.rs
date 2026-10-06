

#[test]
fn guard_probe_cannot_replace_unknown_writer_session() {
    let store = Store::open_in_memory().unwrap();
    let state = setup(&store); let goal = state.goals[0].id.clone();
    edit(&store, HierarchyEdit::Enable);
    let a = project(&store, "A"); let ab = bind(&store, &a, "/repo-a1");
    edit(&store, HierarchyEdit::AssignGoal { goal_id:goal.clone(),project_id:a,binding_id:Some(ab) });
    with_writer(&store, &goal, "unknown-writer");
    let mut next=store.assistant_state().unwrap(); let expected=next.revision;
    next.goals.iter_mut().find(|g|g.id==goal).unwrap().assignments.last_mut().unwrap().session_id=Some("replacement-writer".into());
    assert!(store.save_assistant(expected,next).is_err(),"same assignment id must not authorize a replacement session while old outcome is unknown");
}

#[test]
fn guard_probe_cannot_append_replacement_before_release() {
    let store = Store::open_in_memory().unwrap();
    let state = setup(&store); let goal = state.goals[0].id.clone();
    edit(&store, HierarchyEdit::Enable);
    let a = project(&store, "A"); let ab = bind(&store, &a, "/repo-a1");
    edit(&store, HierarchyEdit::AssignGoal { goal_id:goal.clone(),project_id:a,binding_id:Some(ab) });
    with_writer(&store, &goal, "unknown-writer");
    let mut next=store.assistant_state().unwrap(); let expected=next.revision;
    let g=next.goals.iter_mut().find(|g|g.id==goal).unwrap();
    let mut replacement=g.assignments.last().unwrap().clone();replacement.id="replacement-assignment".into();replacement.session_id=Some("replacement-writer".into());
    g.assignments.push(replacement);
    assert!(store.save_assistant(expected,next).is_err(),"keeping old assignment id must not authorize a second unresolved writer");
}

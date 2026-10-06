

#[test]
fn memory_guard_probe_old_grant_does_not_survive_revoked_share() {
    let store=Store::open_in_memory().unwrap();setup(&store);edit(&store,HierarchyEdit::Enable);
    let a=project(&store,"A");bind(&store,&a,"/repo-a1");
    let m=store.add_memory(GLOBAL_MEMORY,"preference","private shared preference",false).unwrap();
    let (_,share)=store.propose_memory_share_as(rev(&store),&HostActor::chief(),&m.id,HierarchyScope::Project(a.clone()),Reach::ProjectManager).unwrap();
    let hash=store.memory_share_hash(&m.id).unwrap();edit(&store,HierarchyEdit::ConfirmShare{share_id:share.clone(),content_hash:hash});
    let s=store.assistant_state().unwrap();let actor=HostActor::project(&s,&a).unwrap();let grant=memory_grant(&s,&actor).unwrap();
    edit(&store,HierarchyEdit::RevokeShare{share_id:share});
    let read=store.scoped_memory_read(&grant,50);
    assert!(read.is_err()||!read.unwrap().iter().any(|r|r.record.id==m.id),"a cached grant cannot keep a revoked share readable");
}

#[test]
fn memory_guard_probe_target_deny_blocks_shared_read() {
    use codetwo_core::memory::{MemoryProjectPolicy,MemoryPolicyValue};
    let store=Store::open_in_memory().unwrap();setup(&store);edit(&store,HierarchyEdit::Enable);
    let a=project(&store,"A");bind(&store,&a,"/repo-a1");
    let m=store.add_memory(GLOBAL_MEMORY,"preference","private shared preference",false).unwrap();
    let (_,share)=store.propose_memory_share_as(rev(&store),&HostActor::chief(),&m.id,HierarchyScope::Project(a.clone()),Reach::ProjectManager).unwrap();
    let hash=store.memory_share_hash(&m.id).unwrap();edit(&store,HierarchyEdit::ConfirmShare{share_id:share,content_hash:hash});
    store.set_memory_project_policy(&MemoryProjectPolicy{project_path:project_memory_scope(&a),capture:MemoryPolicyValue::Inherit,inject:MemoryPolicyValue::Deny,include_external_context:MemoryPolicyValue::Inherit}).unwrap();
    let s=store.assistant_state().unwrap();let actor=HostActor::project(&s,&a).unwrap();let grant=memory_grant(&s,&actor).unwrap();
    let read=store.scoped_memory_read(&grant,50).unwrap();
    assert!(!read.iter().any(|r|r.record.id==m.id),"target deny must take priority over approved source share");
}

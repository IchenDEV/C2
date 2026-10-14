//! Transport-neutral share policy: one creator-only gate used by the native local adapter
//! (`CoreApp::call_shared`) and the remote web-ui adapter, so local and remote cannot drift.

pub const SHARE_DENIED: &str = "session not found";

/// Session-scoped ordinary commands; each carries the target in `args.session`.
fn session_scoped_command(name: &str) -> bool {
    matches!(
        name,
        "sessions.transcript"
            | "sessions.rename"
            | "sessions.set_archived"
            | "sessions.set_pinned"
            | "engine.close_transient_session"
            | "engine.prompt"
            | "engine.prepare_session"
            | "engine.queue"
            | "engine.steer"
            | "engine.answer_permission"
            | "engine.answer_elicitation"
            | "engine.set_permission_mode"
            | "engine.set_execution_policy"
            | "engine.set_sandbox"
            | "engine.set_model"
            | "engine.set_config_option"
            | "engine.cancel"
            // Reads/mutations that also carry `args.session`; the native adapter has no
            // browser allowlist, so the policy itself must gate them.
            | "engine.prompt_delivery"
            | "engine.switch_provider"
            | "sessions.diff_stat"
            | "worktrees.discard_session"
    )
}

fn list_session_command(name: &str) -> bool {
    matches!(
        name,
        "sessions.list" | "sessions.archived" | "sessions.previews" | "sessions.search"
    )
}

pub fn item_session_id(item: &serde_json::Value) -> Option<&str> {
    item.get("id")
        .or_else(|| item.get("session"))
        .or_else(|| item.get("session_id"))
        .and_then(|value| value.as_str())
}

/// Adds canonical `share` metadata to an already-authorized list row: `{visibility,
/// creator_name}` from the ShareStore, or `{legacy: true}` for an unowned local-only session.
/// Visibility is never inferred; rows without an id are left untouched.
fn annotate_share(share: &crate::share::ShareStore, mut item: serde_json::Value) -> serde_json::Value {
    let Some(id) = item_session_id(&item).map(str::to_string) else {
        return item;
    };
    let meta = match share.list_summary(&id) {
        Some((visibility, creator_name)) => {
            serde_json::json!({ "visibility": visibility, "creator_name": creator_name })
        }
        None => serde_json::json!({ "legacy": true }),
    };
    if let Some(map) = item.as_object_mut() {
        map.insert("share".into(), meta);
    }
    item
}

/// Drop everything the actor may not view. Unknown shapes fail closed to an empty result.
fn filter_session_listing(
    share: &crate::share::ShareStore,
    actor: &crate::share::Participant,
    value: serde_json::Value,
) -> serde_json::Value {
    match value {
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .into_iter()
                .filter(|item| item_session_id(item).is_some_and(|id| share.can_view(id, actor)))
                .map(|item| annotate_share(share, item))
                .collect(),
        ),
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .filter(|(id, _)| share.can_view(id, actor))
                .collect(),
        ),
        _ => serde_json::Value::Array(Vec::new()),
    }
}

/// Typed fields that name an engine session (task). One list for every record scan, including
/// Team `execution_session_id` / `assigned_session_id`, which may exist without a session lease.
fn is_session_subject_key(key: &str) -> bool {
    matches!(
        key,
        "session_id" | "session" | "execution_session_id" | "assigned_session_id"
    )
}

/// Public form of the one record scan: true when any nested typed session field of `value`
/// names a session `actor` may not view (same `can_view`, no second policy).
pub fn value_references_hidden_session(
    share: &crate::share::ShareStore,
    actor: &crate::share::Participant,
    value: &serde_json::Value,
) -> bool {
    references_hidden_task(share, actor, value)
}

/// True when `value` is a record whose task reference (`session_id`, `session`, or any entry of
/// `used_session_refs`) names a task the actor cannot view. The check recurses into nested
/// records so a wrapper cannot hide the subject.
fn references_hidden_task(
    share: &crate::share::ShareStore,
    actor: &crate::share::Participant,
    value: &serde_json::Value,
) -> bool {
    match value {
        serde_json::Value::Object(map) => map.iter().any(|(key, inner)| match (key.as_str(), inner) {
            (key, serde_json::Value::String(id)) if is_session_subject_key(key) => {
                !share.can_view(id, actor)
            }
            ("used_session_refs", serde_json::Value::Array(ids)) => ids
                .iter()
                .any(|id| id.as_str().is_some_and(|id| !share.can_view(id, actor))),
            _ => references_hidden_task(share, actor, inner),
        }),
        serde_json::Value::Array(items) => items.iter().any(|i| references_hidden_task(share, actor, i)),
        _ => false,
    }
}

/// A keyed record whose OWN task reference is hidden (containers are not judged as a whole).
fn directly_hidden(
    share: &crate::share::ShareStore,
    actor: &crate::share::Participant,
    value: &serde_json::Value,
) -> bool {
    let Some(map) = value.as_object() else {
        return false;
    };
    map.iter().any(|(key, inner)| match (key.as_str(), inner) {
        (key, serde_json::Value::String(id)) if is_session_subject_key(key) => {
            !share.can_view(id, actor)
        }
        ("used_session_refs", serde_json::Value::Array(ids)) => ids
            .iter()
            .any(|id| id.as_str().is_some_and(|id| !share.can_view(id, actor))),
        _ => false,
    })
}

fn scrub_hidden_task_records(
    share: &crate::share::ShareStore,
    actor: &crate::share::Participant,
    value: &mut serde_json::Value,
) {
    match value {
        serde_json::Value::Array(items) => {
            items.retain(|item| !references_hidden_task(share, actor, item));
            for item in items {
                scrub_hidden_task_records(share, actor, item);
            }
        }
        serde_json::Value::Object(map) => {
            // A keyed record (e.g. `attempts`) goes whole; containers recurse so only their
            // offending members are removed, not the sibling records.
            map.retain(|_, inner| !directly_hidden(share, actor, inner));
            for inner in map.values_mut() {
                scrub_hidden_task_records(share, actor, inner);
            }
        }
        _ => {}
    }
}

/// The host's local UI keeps unscoped ids, so a host-supplied id that already looks like a scoped
/// (`device:<id>/…`) or host-namespaced id could impersonate a paired actor's request/create
/// intent. Denied, indistinguishably from any other bad request.
pub fn client_id_allowed(actor: &crate::share::Participant, id: &str) -> bool {
    checked_client_id(actor, id).is_ok()
}

fn checked_client_id(actor: &crate::share::Participant, id: &str) -> Result<(), String> {
    if actor.id == crate::share::LOCAL_OWNER_ID
        && (id.starts_with("device:")
            || id.starts_with("local-owner/")
            || id.starts_with("share-create:"))
    {
        return Err("invalid request id".into());
    }
    Ok(())
}

/// Author intent registered on the trusted boundary before an authored command is dispatched.
#[derive(Debug, Clone, Default)]
pub struct PreparedAuthoredOp {
    /// Actor-scoped delivery id the command must be dispatched with (None: session not shared).
    pub request_id: Option<String>,
    inserted_author: bool,
}

impl PreparedAuthoredOp {
    /// Unshared/unpolicied dispatch: nothing to roll back.
    pub fn passthrough(request_id: Option<String>) -> Self {
        Self {
            request_id,
            inserted_author: false,
        }
    }
}

/// Prompt/queue/steer authorship, shared by HTTP and the raw WebSocket. Same actor namespace,
/// same durable author intent; the client-supplied actor is never read.
pub fn prepare_authored_op(
    share: &crate::share::ShareStore,
    device_id: &str,
    session: &str,
    client_id: Option<String>,
) -> Result<PreparedAuthoredOp, String> {
    let actor = share.participant(device_id);
    if !share.can_view(session, &actor) {
        return Err(SHARE_DENIED.to_string());
    }
    let client_id = client_id.filter(|id| !id.is_empty());
    if let Some(id) = &client_id {
        checked_client_id(&actor, id)?;
    }
    if !share.is_shared_session(session) {
        return Ok(PreparedAuthoredOp {
            request_id: client_id,
            inserted_author: false,
        });
    }
    let scoped = crate::share::scoped_request_id(
        &actor,
        &client_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
    );
    let inserted_author = share.expect_author(session, &scoped, &actor);
    Ok(PreparedAuthoredOp {
        request_id: Some(scoped),
        inserted_author,
    })
}

/// Complete an authored command: journal it on success, roll back the author intent on failure.
pub fn finish_authored_op(
    share: &crate::share::ShareStore,
    device_id: &str,
    session: &str,
    kind: &str,
    prepared: &PreparedAuthoredOp,
    accepted: bool,
) -> Result<(), String> {
    if !accepted {
        if prepared.inserted_author {
            if let Some(id) = &prepared.request_id {
                share.drop_author(id);
            }
        }
        return Ok(());
    }
    let actor = share.participant(device_id);
    share.record_op(session, &actor, kind, prepared.request_id.as_deref())
}

/// Participant/share policy for every ordinary `/api/web-ui/call` command. `dispatch` runs the
/// underlying Kernel command only after the policy admits it. The actor is derived from the
/// paired device (`device_id`, empty = host local UI); no client-supplied identity is read.
pub async fn shared_web_ui_call<F, Fut>(
    share: &crate::share::ShareStore,
    device_id: &str,
    name: &str,
    args: serde_json::Value,
    dispatch: F,
) -> Result<serde_json::Value, String>
where
    F: FnOnce(serde_json::Value) -> Fut,
    Fut: std::future::Future<Output = Result<serde_json::Value, String>>,
{
    use crate::share::Visibility;
    let actor = share.participant(device_id);
    let arg_str = |key: &str| {
        args.get(key)
            .and_then(|value| value.as_str())
            .map(str::to_string)
    };
    match name {
        "share.whoami" => return serde_json::to_value(&actor).map_err(|e| e.to_string()),
        "share.set_nickname" => {
            let nickname = arg_str("nickname").unwrap_or_default();
            let updated = share.set_nickname(device_id, &nickname)?;
            return serde_json::to_value(updated).map_err(|e| e.to_string());
        }
        "share.get" | "share.set_visibility" | "share.follow" => {
            let session = arg_str("session").ok_or_else(|| SHARE_DENIED.to_string())?;
            let record = match name {
                "share.get" => share
                    .read(&session, &actor)
                    .ok_or(SHARE_DENIED.to_string())?,
                "share.set_visibility" => {
                    let visibility: Visibility =
                        serde_json::from_value(args.get("visibility").cloned().unwrap_or_default())
                            .map_err(|_| "visibility must be public or private".to_string())?;
                    share.set_visibility(&session, &actor, visibility)?;
                    share
                        .read(&session, &actor)
                        .ok_or(SHARE_DENIED.to_string())?
                }
                _ => {
                    let follow = args.get("follow").and_then(|v| v.as_bool()).unwrap_or(true);
                    share.set_follow(&session, &actor, follow)?
                }
            };
            return serde_json::to_value(record).map_err(|e| e.to_string());
        }
        _ => {}
    }

    if session_scoped_command(name) {
        let session = arg_str("session").ok_or_else(|| SHARE_DENIED.to_string())?;
        if !share.can_view(&session, &actor) {
            return Err(SHARE_DENIED.to_string());
        }
        let authored = matches!(name, "engine.prompt" | "engine.queue" | "engine.steer");
        let client_id = arg_str("request_id").filter(|id| !id.is_empty());
        if let Some(id) = &client_id {
            checked_client_id(&actor, id)?;
        }
        // Actor-scoped request namespace on the trusted boundary: the same actor retrying a
        // client id is one operation; different actors using the same client id are independent.
        // Spoofed actor/nickname fields in args are never read.
        let mut args = args;
        let mut request_id = client_id.clone();
        if name == "engine.prompt_delivery" && share.is_shared_session(&session) {
            // Delivery lookups use the same actor namespace as the submit, so another actor's
            // (or another private task's) delivery is unreachable by guessing a client id.
            let Some(id) = &client_id else {
                return Err(SHARE_DENIED.to_string());
            };
            if let Some(map) = args.as_object_mut() {
                map.insert(
                    "request_id".into(),
                    serde_json::Value::String(crate::share::scoped_request_id(&actor, id)),
                );
            }
        }
        // Prompt/queue/steer authorship uses the same prepare/finish functions as the raw WS.
        let prepared = if authored {
            let prepared = prepare_authored_op(share, device_id, &session, client_id.clone())?;
            if share.is_shared_session(&session) {
                if let (Some(map), Some(id)) = (args.as_object_mut(), &prepared.request_id) {
                    map.insert("request_id".into(), serde_json::Value::String(id.clone()));
                }
                request_id = prepared.request_id.clone();
            }
            Some(prepared)
        } else {
            None
        };
        let result = match dispatch(args).await {
            Ok(result) => result,
            Err(error) => {
                if let Some(prepared) = &prepared {
                    let _ = finish_authored_op(share, device_id, &session, name, prepared, false);
                }
                return Err(error);
            }
        };
        if let Some(prepared) = &prepared {
            finish_authored_op(
                share,
                device_id,
                &session,
                name.trim_start_matches("engine."),
                prepared,
                true,
            )?;
        } else if name == "engine.cancel" {
            share.record_op(&session, &actor, "cancel", request_id.as_deref())?;
        }
        if name == "sessions.transcript" {
            return Ok(attach_message_authors(share, &session, result));
        }
        return Ok(result);
    }

    if name == "assistant.snapshot" {
        // Global snapshot: nested task list is per-actor; nothing else names a task.
        let mut result = dispatch(args).await?;
        if let Some(sessions) = result.get_mut("sessions") {
            *sessions = filter_session_listing(share, &actor, sessions.take());
        }
        // Task-derived records anywhere else in the snapshot (scoped runs, attempts, memories,
        // questions, notifications, ...) are dropped by subject BEFORE the client aggregates or
        // counts them. No host exemption beyond the ordinary `can_view` policy.
        for key in ["state", "memories"] {
            if let Some(value) = result.get_mut(key) {
                scrub_hidden_task_records(share, &actor, value);
            }
        }
        return Ok(result);
    }

    if list_session_command(name) {
        let result = dispatch(args).await?;
        return Ok(filter_session_listing(share, &actor, result));
    }

    if name == "engine.new_session" || name == "engine.new_parallel_task" {
        let visibility: Visibility = match args.get("visibility") {
            None | Some(serde_json::Value::Null) => Visibility::default(),
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|_| "visibility must be public or private".to_string())?,
        };
        // Metadata intent is recorded before dispatch so the SessionCreated event can never
        // outrun registration. Generated ids are outside every idempotency-receipt prefix.
        let request_id = match arg_str("request_id") {
            Some(id) if !id.is_empty() => {
                checked_client_id(&actor, &id)?;
                crate::share::scoped_request_id(&actor, &id)
            }
            _ => format!("share-create:{}", uuid::Uuid::new_v4()),
        };
        let mut args = args;
        if let Some(map) = args.as_object_mut() {
            map.remove("visibility");
            map.insert(
                "request_id".into(),
                serde_json::Value::String(request_id.clone()),
            );
        }
        share.expect_create(&request_id, &actor, visibility);
        let result = match dispatch(args).await {
            Ok(result) => result,
            Err(error) => {
                share.drop_pending(&request_id);
                return Err(error);
            }
        };
        let created = match &result {
            serde_json::Value::String(id) => Some(id.clone()),
            other => item_session_id(other).map(str::to_string),
        };
        if let Some(id) = created {
            share.claim_created(&request_id, &id)?;
            share.register_session(&id, &actor, visibility)?;
        }
        share.drop_pending(&request_id);
        return Ok(result);
    }

    // Unlisted commands: any that names a task must still pass the one viewing policy.
    if let Some(session) = arg_str("session") {
        if !share.can_view(&session, &actor) {
            return Err(SHARE_DENIED.to_string());
        }
    }
    dispatch(args).await
}

/// Attach the durable author to each canonical user row by its transcript `seq` (never by text
/// or index). Rows without a durable author (legacy, host-local pre-share) stay unattributed.
fn attach_message_authors(
    share: &crate::share::ShareStore,
    session: &str,
    mut page: serde_json::Value,
) -> serde_json::Value {
    let Some(authors) = share.message_authors(session) else {
        return page;
    };
    if let Some(entries) = page.get_mut("entries").and_then(|v| v.as_array_mut()) {
        for entry in entries {
            let Some(seq) = entry.get("seq").and_then(|v| v.as_i64()) else {
                continue;
            };
            if let (Some(author), Some(map)) = (authors.get(&seq), entry.as_object_mut()) {
                map.insert(
                    "author".into(),
                    serde_json::to_value(author).unwrap_or_default(),
                );
            }
        }
    }
    page
}

/// One event policy for every live exit. Claims pending creation metadata before judging a
/// `SessionCreated`, fails closed for session-less events and attribution gaps.
pub fn share_event_visible(
    share: &crate::share::ShareStore,
    device_id: &str,
    event: &crate::Event,
) -> bool {
    let actor = share.participant(device_id);
    match event {
        // Public -> private reaches every participant (no metadata) so open views drop the
        // session; everything else about a private session stays creator-only.
        crate::Event::SessionShareChanged { kind, .. } if kind == "revoked" => true,
        crate::Event::SessionCreated {
            session,
            request_id,
            ..
        } => {
            if let Some(request_id) = request_id {
                let _ = share.claim_created(request_id, session);
            }
            share.can_view(session, &actor)
        }
        // Session-less variants: fail closed unless the error answers this actor's own create.
        crate::Event::Error {
            session: None,
            request_id,
            ..
        } => {
            actor.id == crate::share::LOCAL_OWNER_ID
                || request_id
                    .as_deref()
                    .and_then(|id| share.pending_creator(id))
                    .is_some_and(|creator| creator == actor.id)
        }
        crate::Event::AssistantAlert { session: None, .. } => actor.id == crate::share::LOCAL_OWNER_ID,
        // A task is as private as its most private subject: ALL subjects must pass `can_view`
        // (same decision as the HTTP task read), so a viewer of one public session never learns
        // the id/revision of a task that also touches a private one. No subject = local only.
        crate::Event::TaskSnapshotChanged { sessions, .. } => {
            if sessions.is_empty() {
                actor.id == crate::share::LOCAL_OWNER_ID
            } else {
                sessions
                    .iter()
                    .all(|session| share.can_view(session.as_str(), &actor))
            }
        }
        other => match event_session_id(other) {
            Some(session) => share.can_view(&session, &actor),
            None => actor.id == crate::share::LOCAL_OWNER_ID,
        },
    }
}

fn event_session_id(event: &crate::Event) -> Option<String> {
    serde_json::to_value(event)
        .ok()?
        .get("session")?
        .as_str()
        .map(str::to_string)
}

/// A C2 sync peer is an authenticated device and therefore a share participant like any other:
/// it sees a session only if the one common `can_view` decision allows its device identity
/// (public yes; private = creator only; historical unowned = local owner only, never shared).
fn peer_can_view(
    share: &crate::share::ShareStore,
    peer: &crate::share::Participant,
    session: &str,
) -> bool {
    share.can_view(session, peer)
}

/// Remove sessions, transcript parts and session-scoped memories the authenticated peer device
/// may not view. Same ShareStore decision as every other outlet; no second policy state.
pub fn redact_device_sync_document(
    share: &crate::share::ShareStore,
    peer_device_id: &str,
    document: &mut crate::device_sync::DeviceSyncDocument,
) {
    let peer = share.participant(peer_device_id);
    document.sessions.retain(|s| peer_can_view(share, &peer, &s.id));
    document
        .parts
        .retain(|p| peer_can_view(share, &peer, &p.session_id));
    let hidden: std::collections::HashSet<String> = document
        .memories
        .iter()
        .filter(|m| {
            m.session_id
                .as_deref()
                .is_some_and(|session| !peer_can_view(share, &peer, session))
        })
        .map(|m| m.id.clone())
        .collect();
    document.memories.retain(|m| !hidden.contains(&m.id));
    // A visible memory must not name a hidden one (id, or the reason that describes it).
    for memory in &mut document.memories {
        if memory.supersedes_id.as_ref().is_some_and(|id| hidden.contains(id)) {
            memory.supersedes_id = None;
        }
        if memory.conflict_with_id.as_ref().is_some_and(|id| hidden.contains(id)) {
            memory.conflict_with_id = None;
            memory.conflict_reason = None;
        }
    }
}

/// A peer write that names a session the peer cannot view cannot create/overwrite it (the peer
/// never saw it, so a legitimate write never carries it). Err = reject the whole write.
pub fn check_device_sync_write(
    share: &crate::share::ShareStore,
    peer_device_id: &str,
    document: &crate::device_sync::DeviceSyncDocument,
) -> Result<(), String> {
    let peer = share.participant(peer_device_id);
    let denied = document.sessions.iter().any(|s| !peer_can_view(share, &peer, &s.id))
        || document
            .parts
            .iter()
            .any(|p| !peer_can_view(share, &peer, &p.session_id))
        || document.memories.iter().any(|m| {
            m.session_id
                .as_deref()
                .is_some_and(|session| !peer_can_view(share, &peer, session))
        });
    if denied {
        Err(SHARE_DENIED.to_string())
    } else {
        Ok(())
    }
}

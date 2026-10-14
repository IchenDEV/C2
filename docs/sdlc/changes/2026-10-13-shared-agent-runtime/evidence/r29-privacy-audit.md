# R29 privacy audit: native task data, subject fields, task events, handoff collision

Date 2026-10-13, HEAD 926ebdb5, dirty tree preserved. Brief sha256 6dd2f225... verified (5184 B).

## Native TaskStore boundary (verified by registry search)
- No native Core command returns TaskStore data. `Store::get_task/task_snapshot/shared_task_snapshot/list_shared_tasks/list_attention_items/get_task_graph` are called only by the orchestrator (internal) and the Team HTTP handlers in `crates/server/src/lib.rs`. The only task-adjacent native commands are `engine.new_parallel_task` (creation, policy-gated) and `assistant.snapshot` (assistant store, scrubbed by the common policy). So the native boundary is the event stream; no alias was invented.
- Real event exit: `Event::TaskSnapshotChanged` published by `publish_task_snapshot`. It was session-less, and `share_event_visible` gives session-less task events to the local owner, so the machine owner saw private tasks' id/revision. FIXED: the event is now published once per subject session (leases + graph `assigned_session_id`), so the existing common `share_event_visible`/`can_view` decides for every outlet (native host_events, ws, T3). A task with no session subject stays session-less (local owner/legacy). Lookup error publishes nothing (fail closed).
- Test `tests::task_snapshot_events_follow_subject_session_visibility_for_local_owner` (server lib): real Store + real `publish_task_snapshot` + real broadcast; private -> local owner "" false, other false, creator true; public -> local owner and other true; no-subject -> local owner true. Red (publish session-less) fails at the session assertion; green passes.

## Unleased `assigned_session_id` / `execution_session_id`
- Invariant does NOT guarantee a lease: `apply_task_graph` stores `assigned_session_id` without a lease (no FK). `execution_session_id` is always derived from a lease in the approve route, but is covered by the same scan.
- Fix: `share_policy::is_session_subject_key` is the single typed-field list (`session_id`, `session`, `execution_session_id`, `assigned_session_id`) used by the existing record scans (also hardens `assistant.snapshot` scrub); `team_task_visible` additionally judges the member's own snapshot with `value_references_hidden_session` (same `can_view`).
- Test `team_collaboration::unleased_assigned_private_session_hides_team_task_from_other_members`: no lease, private session in a work item; creator sees it, other member gets 404 on detail and list. Red (new check disabled) fails 200 != 404; green passes.

## Handoff collision
- Test `tests::authenticated_handoff_collision_never_touches_existing_private_session` (real routes, real stores, ShareStore private session): duplicate accept 400, activate 409, rollback 409; session row identical before/after, creator still can_view, other cannot, credential not consumed. Store invariant (`accept_handoff` refuses an existing id with a different fence; rollback requires a matching fence) already prevents the bypass, so NO source change; behavioural evidence only. Not red-tested (no code change). Known residue: the refusal text echoes the caller-supplied id (existence of an id is inferable by an authenticated handoff peer) - not changed.

## Legacy receipt message
- Refusal now says the pre-upgrade receipt has unknown original actor, was neither replayed nor re-run, and that the task state must be checked first, starting a deliberate new operation only if still needed (earlier command may have completed). No blind-resend advice. Unit test asserts both. Compatibility behaviour recorded; no human approval claimed.

## Commands (all exit 0 unless noted)
team_collaboration (3 pass); server lib (58 pass); native_share (1); server share_policy/t3_share_privacy/ws_share_parity; core share_authorship/share_boundaries; git diff --check; sdlc --worktree. Red runs exit 101 (expected) for both new regression tests; sources restored (cmp verified). Logs in .local/agent-work/20261013-full-sharing-audit-r29/logs.

## Remaining
- Multi-subject tasks mixed public/private: a viewer of any one subject session receives the id/revision event (refetch is still denied by `team_task_visible`). Not fixed.
- lifecycleEval not rerun (no lifecycle/checker change). No UI verification this round.

# R30 privacy audit: multi-subject task events and handoff refusal

Brief: `.local/agent-work/20261013-full-sharing-audit-r30/brief.md` (2912 B, sha256 ebfea7de6ff076728636a11949c9f52f3477cbe0d0b709554d7f76986317174c) plus two in-flight steers. Inherits R29 constraints. First native start 16:01:53.727Z, cutoff 16:11:53.727Z.

## Fix 1: `TaskSnapshotChanged` requires ALL subjects

- `Event::TaskSnapshotChanged` now carries `sessions: Vec<SessionId>` (every lease and graph `assigned_session_id`) instead of one event per session. `publish_task_snapshot` publishes ONE canonical event.
- `share_event_visible` (`crates/core/src/share_policy.rs`) allows it only if every subject passes the common `can_view`. Native host, raw WS, replay and T3 all use that function. An empty subject list stays local-owner only (legacy).
- The desktop bridge type in `apps/desktop/src/bridge.ts` is now `sessions: string[]` (contract only).
- Test `task_snapshot_events_follow_subject_session_visibility_for_local_owner` covers: mixed public/private (public-session viewer, other private creator and local owner all denied, creator allowed), two private creators (neither alone sees it, local owner denied), all-public multi-subject (still flows), single private, no-subject legacy, and exactly one event per task.
- Old-red: with `.any` in place of `.all` the test failed ("local owner is not exempt" on the mixed task); exit 101. Green after restoring `.all`.

## Fix 2: handoff refusal

- Handoff routes now check the actual authenticated caller (`authorization.device_id`) with the common `can_view` BEFORE any store call. Without a ShareStore context the bare-server behavior is unchanged.
- With sharing configured, a hidden private session, a historical unowned (local/unshared) session viewed by a paired device, and an unknown session all get one answer for activate/rollback: 409 `task handoff target is not available`. It carries no session id or fence text. Accept of a hidden existing id gets 400 with the same text and the store is never touched.
- Tests: `authenticated_handoff_collision_never_touches_existing_private_session` (hidden vs unknown activate and rollback give equal status and body; no id echo; row unchanged; credential not consumed) and `handoff_target_hidden_follows_common_can_view` (creator/public allowed; hidden private, local owner on private, legacy unowned for paired devices denied; no ShareStore unchanged). `authenticated_handoff_routes_move_one_writable_task` still passes.
- Old-red: with the guard disabled, the unit test failed and the route test failed with a real oracle (hidden private activate returned `409 "invalid task handoff: target handoff fence no longer matches"`, while the unknown path is the different store text `target session is missing`, read from source and not run old). Exit 101. Green after restoring.

## Remaining limits (not fixed, by design)

- Accept of an UNKNOWN id succeeds (insert-only) while a hidden existing id is refused. This difference is inherent to insert-only handoff and cannot be removed without refusing every legitimate fresh handoff.
- With sharing configured, a freshly accepted handoff session has no creator record, so it stays local/unshared; a paired device's follow-up activate/rollback is refused by `can_view`. The protocol carries no creator provenance, so a sharing-enabled remote activation flow needs a future protocol field. No cross-host identity or migration was invented, and the session is not made public. The handoff route tests cover only the bare server; this limit is recorded, not proven by a sharing-enabled positive test.
- No UI, TS typecheck or desktop tests were run this round.

## Verification (actual exits)

| Command | Exit |
| --- | --- |
| red: server lib `task_snapshot_events handoff_target authenticated_handoff` with bypasses | 101 (3 failed, 1 passed) |
| server lib, all tests (59) | 0 |
| server tests team_collaboration, share_policy, t3_share_privacy, ws_share_parity, web_ui_commands | 0 |
| core tests native_share, share_authorship, share_boundaries | 0 |
| `git diff --check` | 0 |
| `bun script/verify/sdlc.ts --worktree` | 0 |

Logs: `.local/agent-work/20261013-full-sharing-audit-r30/logs/`.

## Removed / Retained / Processes / Evidence

- Removed: temporary `.any` and `false &&` bypasses (restored, verified by grep).
- Retained: `logs/` directory; reviewed R29 files untouched.
- Processes: none left running. One earlier background cargo run was killed by a tool interrupt; no name-based kills.
- Evidence: this file, `result.md`.
- Main checkout `/Users/chenli/projects/codeTwo` untouched (refactor/architecture-simplification f94b88b4c1307013f280c01d0c08f382eaf275f1).

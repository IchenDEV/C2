# R28 privacy and replay audit

Workspace HEAD `926ebdb5e1a8ec644cf1f8d4efc02d28a8e9c6ef` (research/shared-agent-20261008, dirty tree preserved). Main checkout `/Users/chenli/projects/codeTwo` (refactor/architecture-simplification f94b88b4) not touched. One executor, no children.

## Findings and fixes

| Boundary | Result | Real test |
| --- | --- | --- |
| Team HTTP (`/api/team/v1` list, detail, comments, suggestions, approve, attention) with a creator-private engine session attached as a task lease | **Proven bypass, fixed.** Snapshots carry `runtime.session_leases[].session_id`, so a collaborator saw a private session's id and task data. `team_task_visible` in `crates/server/src/lib.rs` now applies the common `ShareStore::can_view` to every leased session; a hidden task is 404, omitted from list and attention. | `team_collaboration::private_leased_session_hides_team_task_from_other_members`. Red (guard disabled): fails at `GET tasks/task-private` expecting 404. Green with the guard. |
| Legacy unscoped T3 command receipts | **Fixed (conservative).** Receipts persisted before actor namespacing now load as `scoped:false` and are refused, not replayed or re-executed. Error: `command <id> has an ambiguous pre-upgrade receipt and was not replayed; resend it with a new command id`. Actor-scoped receipts and distinct actors' identical raw ids are unchanged. | `t3_compat::tests::pre_scoping_command_receipt_is_refused_not_replayed_or_reexecuted` (no session created, two viewers refused, legacy row survives persist/reload). Red not separately run; the old code looked up only `viewer/<id>` and would have re-executed. |
| Device-sync memory | Top-level `session_id` filtering already existed. Fixed a residual leak: a visible memory's `supersedes_id`/`conflict_with_id`/`conflict_reason` could name a hidden private memory; these are now cleared. | `crates/core/tests/share_device_sync_memory.rs` (private + legacy hidden, public and session-less kept, owner still sees, peer write to private/legacy refused). Red for the cross-reference clearing not separately run. |
| Handoff duplicate/activate/rollback vs another actor's private task | **UNVERIFIED.** Only the existing `authenticated_handoff_routes_move_one_writable_task` was re-run (green). No collision/no-overwrite test was added this round. | none new |

Not checked: native Core command aliases for the same task data; Team surfaces other than the six routes above; task `execution_session_id`/`assigned_session_id` not backed by a lease.

## Compatibility

Pre-upgrade T3 command receipts are permanently refused for that raw id (all actors). Clients must resend with a new command id. This is a recorded risk, not user approval.

## Commands and exits (all `GHOSTTY_SOURCE_DIR` and `--target-dir` as briefed, `SHELL=/bin/bash`)

All exit 0: `team_collaboration` (2 tests), `share_device_sync_memory`, server `share_policy`, `t3_share_privacy`, `t3_mobile_compat`, `ws_share_parity`, lib `t3_compat`, lib `authenticated_handoff`. Logs in `.local/agent-work/20261013-full-sharing-audit-r28/evidence/`. The red run exited non-zero as intended and the temporary bypass was restored.

## Removed / Retained / Processes

Removed: temporary `R28_RED` bypass and `/tmp/r28-lib.rs.bak`. Retained: logs under `.local/`. Processes: only foreground cargo runs, none left running. No commit, push, install or data-dir use.

## Remaining

No broad feature PASS. Next: handoff collision test, native alias audit, final two-client UI run, fresh fixture/bundle evidence, coherent four stages.

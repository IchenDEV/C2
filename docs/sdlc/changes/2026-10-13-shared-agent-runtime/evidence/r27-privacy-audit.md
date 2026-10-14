# R27 privacy audit (remaining outlets)

Scope: application private-task data only. No UI, no OS/terminal isolation. Not a broad feature PASS.
Historical unowned tasks stay local/unshared; no legacy visibility was granted or waived.

## Fixed (red -> green, real tests)

| Item | Red | Green |
| --- | --- | --- |
| `share_policy.rs` list assertion | stale shape (`share` metadata added in R21) | ids asserted exactly, `share.visibility/creator_name` verified, legacy rows marked `{legacy:true}`, no `priv` anywhere (9/9) |
| `WebUiCommandCaller::session_visible/event_visible` defaults | allow | default deny; `unwired_custom_caller_denies_sessions_and_events_by_default`; intentional unshared adapter `RecordingCaller` opts in explicitly |
| `/api/device-sync/v1/snapshot` GET+PUT | served every session, transcript part, session memory of every actor (red: `private leaked`, mutation of the filter) | filter and write guard use the common `ShareStore::can_view` with the authenticated C2 device's participant; test `c2_device_sync_never_sends_or_accepts_private_task_data` (private + legacy unowned hidden and unwritable, public still syncs) |
| `/api/artifacts/:id` orphan blob | deleting a task (`Engine::delete_idle_session`) removes `artifact_refs` but keeps the blob; old code allowed unreferenced blobs: private payload downloaded, HTTP 200 | unreferenced = no authority when sharing is enabled (404, also for the old creator); valid public/creator downloads unchanged |

## Four-route triage

| Route | Reach | Verdict |
| --- | --- | --- |
| `/api/device-sync/v1/*` | C2 bearer (separate credential protocol from T3/web-ui devices), not the `/external-mcp` loopback token. Whole-state replica of sessions/parts/memories. | REAL leak, FIXED (behavioral). Consequence: legacy unowned sessions no longer sync to a peer; that is the stated invariant, any desired owner-sync exception needs a human decision (not recorded as accepted). Session-scoped memory/part filters share the same code but only sessions+parts are exercised by the test: memories STATIC. |
| `/api/team/v1/*` | member-bound bearer, membership-checked store (`list_shared_tasks`, `shared_task_snapshot`). Snapshot holds task runtime with `session_leases` (session ids only, no transcript). Approve creates a NEW session (unowned = local only) and prompts it. | Not engine private-task content; no read/mutate of another actor's session found. STATIC only. Residual: lease session ids visible to team members; created session metadata not registered in ShareStore. |
| `/api/canvas/*` | paired device, per-owner drafts (`OwnerMismatch`); canvas.rs has no session reference. | not-app-task-data. STATIC. |
| `/api/codetwo/handoffs*` | T3 bearer with `orchestration:operate`. Inbound-only: inserts a caller-supplied session (PK/handoff guard, no overwrite), activate/rollback need the handoff id. Activated session is unowned = local only. | No read of private data found; STATIC, no behavioral test. |

## Still FAIL / UNVERIFIED
- Team and handoff behavioral tests absent; team lease id exposure unresolved.
- `/external-mcp` (separate loopback credential) and its event stream not share-filtered.
- Terminal id/title: OS terminal access, not app task data; unchanged.
- Old unscoped T3 receipts re-run once after upgrade: recorded risk, NOT human-accepted, unchanged.
- No UI/revoke evidence this round; full feature NOT PASS.

# R26 T3 privacy gap closure (HEAD 926ebdb5 + dirty worktree, 2026-10-13)

Verdict: **PARTIAL**. Not a full-feature or broad privacy PASS. Frontend same-instance control/revocation, final manifest and a
coherent four-stage handoff remain for the next task. Main checkout `/Users/chenli/projects/codeTwo`
`refactor/architecture-simplification` f94b88b4c1307013f280c01d0c08f382eaf275f1 was clean and unchanged.

## Priority A - missing-policy fallback (FIXED, red -> green)
Before: `T3CompatState::session_allowed` returned `true` with no policy. Nothing enforced "no policy => no private task":
`bind_and_serve_with_web_ui(.., web_ui_commands=None)` with an engine that owns a `ShareStore` exposed private sessions
(red: `logs/t3-red-mutation.log`, exit 101, shell snapshot listed the private session). Trait defaults of
`WebUiCommandCaller::{session_visible,event_visible}` are also allow (only `KernelWebUiCommands` implements them in production,
and overrides them; the `RecordingCaller` test double keeps the defaults - residual, see below).
Now: no policy => allowed only when `engine.share_store().is_none()` (no ShareStore means no private/owned metadata can
exist); engine WITH a store and NO policy denies reads and refuses T3 creates ("share policy is not wired"). Public T3 behaviour
is unchanged when the policy is wired. Test: `t3_without_wired_policy_fails_closed_when_engine_has_a_share_store`.

## Priority B - real T3 WebSocket frames (PASS for the tested paths)
`t3_websocket_frames_enforce_private_visibility_revoke_and_actor_receipts` (real loopback `/ws?wsTicket=`, three distinct
actors, real `KernelWebUiCommands` + persisted ShareStore, zero-fee python ACP stub): creator-private id absent from
`subscribeShell` frames of two actors; direct `subscribeThread` of private = Exit Failure "unknown thread"; public thread
subscription works; same `commandId` from two actors both create (actor-scoped receipts); creator public->private while
subscribed => shell refreshes without the task, NO further thread chunk, no post-revoke frame contains the id; WS dispatch
against it = "unknown thread". Not tested: replay/resume after reconnect, alias replay across restart, queued update held
while `awaiting_ack` at the exact revoke instant (code path `Ack` -> `subscription_value` uses the same gate).
Compatibility: old unscoped persisted T3 receipts are ignored after upgrade, so one replayed pre-upgrade commandId may
re-execute once. Not migrated (no new registry); accepted and recorded, not claimed safe.

## Remaining routes (ordinary paired credential)
| Route | Result | Evidence |
|---|---|---|
| `/api/artifacts/:id` | **FAIL -> FIXED** | Bypass: `require_device` only; `artifact_refs(session_id, artifact_id)` associates blobs to tasks, so any paired device that knew an id downloaded a private task's payload. Fix: `ArtifactStore::referencing_sessions` + `session_visible` (deny => 404; unreferenced blob allowed; store without policy denies). Test `artifact_download_follows_the_referencing_task_visibility` (red mutation `logs/artifact-red-mutation.log` exit 101; green). |
| `/api/terminals`, `/kill` | static PASS | `TerminalInfo{id,title}` only, PTY registry not tied to engine sessions (`terminal.rs:75`). Kill is any-device by authorized design. No test. |
| `/api/team/v1/*` | UNVERIFIED | Member-bound store (`list_shared_tasks(actor)`), separate from engine sessions; content not traced to engine tasks. |
| `/api/canvas/*` | UNVERIFIED | Not traced (separate Canvas store/gate; default closed). |
| `/api/codetwo/handoffs*` | UNVERIFIED | `TaskHandoffManager` writes tasks into the engine; whether it can read/overwrite a private session not traced. |
| `/api/device-sync/v1/snapshot` | UNVERIFIED | `device_sync.snapshot()` is a host-owned replica; content not inspected for session data (C2 device only). |
| `/external-mcp`, `/mcp` | separate boundary | Independent loopback external token, not paired-device proof; event stream not share-filtered (R25 note unchanged). |

## Commands (cargo env: `GHOSTTY_SOURCE_DIR`, shared target-dir from the brief)
- `cargo test -p codetwo-server --test t3_share_privacy` exit 0 (4 tests) - `logs/t3-artifact.log`.
- `cargo test --no-fail-fast -p codetwo-server --test t3_share_privacy --test t3_mobile_compat --test ws_share_parity --test share_policy` exit 101:
  t3_share_privacy 4/4, t3_mobile_compat 2/2, ws_share_parity 1/1 PASS; `share_policy` 8/9 - **FAIL (not fixed, not caused by
  this round, file untouched)**: `search_snapshot_and_alternate_session_commands_follow_the_one_policy` (share_policy.rs:389)
  expects `[{id}]` but the response now carries `share{creator_name,visibility}` metadata: stale assertion vs. the in-progress
  share-metadata work. Owner: next task. `logs/t3-suite3.log`.
- `cargo test -p codetwo-server --lib t3_compat` exit 0 (13 tests).
(Private logs under `.local/agent-work/20261013-full-sharing-audit-r26/logs/`.)

## Files (SHA256 / bytes)
- crates/server/src/t3_compat.rs a9fb6bd1f6b15a587cbcfddd3613d7adfe0d28962b23b64459f820a6ba630fee / 99248
- crates/server/src/lib.rs 605fb7bf9894ab00852934d93bfc6856b80688311c0bc4d589285874cc05a631 / 139366
- crates/server/tests/t3_share_privacy.rs 8e31d6e7652b90f6d1d7605a0d580dfe550aa556a2f2110bd0142e06353a6cc4 / 25584
- crates/core/src/artifact.rs 2904e80ae175e8bd543b1b6294e52b0c8b95ca5039b8030f120cfb5c22d98c10 / 36155

## Residual FAIL / UNVERIFIED
Trait defaults still allow for custom `WebUiCommandCaller`s; team/canvas/handoffs/device-sync UNVERIFIED; replay/resume,
nested git/provider response subjects, external-MCP event filter; share_policy.rs stale assertion; compile-only for
desktop. No UI changes. Removed: none. Retained: this file, new tests. Processes: none left (test-owned loopback listeners and
python stubs exited with the tests); no kill commands used.

## Checks
`bun script/verify/sdlc.ts --worktree` exit 0 (after adding `crates/core/src/artifact.rs` to plan scope; first run exit 1 on that path);
`bun script/verify/docs.ts` valid; `git diff --check` clean. Active lifecycle Eval
(`bun test script/verify/checks.test.ts ...`) **DEFERRED, not run** (no authorization/routing/checker change this round).
`cargo fmt --check` reports pre-existing repo-wide diffs; not reformatted (would touch others' dirty files).

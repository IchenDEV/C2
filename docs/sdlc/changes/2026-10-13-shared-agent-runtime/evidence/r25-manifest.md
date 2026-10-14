# R25 private-outlet audit (HEAD 926ebdb5 + dirty worktree, 2026-10-13)

Verdict: **PARTIAL**. One proven bypass (secondary T3-compat API) reproduced red and repaired through the
common `ShareStore`; the rest of the matrix is source-traced with existing tests, not freshly re-proven.
No full-privacy PASS is claimed.

## Proven bypass (FIXED, red -> green)
Ordinary paired credential via `/oauth/token` -> `/api/orchestration/{shell,threads/:id,dispatch}` and WS
`orchestration.subscribeShell/Thread/dispatchCommand` read `engine.list_sessions()` with no share policy.
Red: `cargo test -p codetwo-server --test t3_share_privacy` leaked another actor's private task id in the shell
snapshot (exit 101). Fix (`crates/server/src/t3_compat.rs`, `lib.rs`): viewer device id threaded through
HTTP/WS; every snapshot/subscription refresh uses `WebUiCommandCaller::session_visible` (= `ShareStore::can_view`);
dispatch denies an existing invisible thread as `unknown thread`; command ids/receipts/`t3-create` idempotency are
actor-scoped (`<device>/<commandId>`), so another actor cannot read or swallow a receipt; T3-created tasks register
creator metadata (default public) via `register_created` -> `expect_create` before `NewSession`.
Fallback semantics: with NO share policy wired (`bind_and_serve`, tests) the adapter keeps legacy allow-all, which is
safe only because no ShareStore means no private/owned task; production `serve.rs` always wires it.
Behaviour change: legacy unowned threads (incl. pre-existing T3 threads) are no longer shown to paired T3 devices when
the policy is wired (matches "legacy local/unshared"). Old unscoped persisted receipts no longer dedupe after upgrade.

## Matrix (guard = ShareStore::can_view via share_policy/KernelWebUiCommands; evidence = existing tests)
| Exit | Guard | Behavioural evidence | Uncovered |
|---|---|---|---|
| web-ui call list/read/search/archived/previews/transcript | `shared_web_ui_call` | share_policy.rs | nested response subjects of git/provider/search not re-audited |
| raw `/ws` ops, welcome, live events | `session_visible`/`event_visible` | ws_share_parity.rs | buffered replay/resume not re-run |
| native host (`call_local`) | Core policy, host=local owner | core native_share tests | not re-run |
| T3 HTTP/WS | now `session_visible` | t3_share_privacy.rs (new) | WS-frame level test absent (shared code path); revoke-after-subscribe relies on per-update snapshot gate |
| receipts/delivery ids | actor-scoped ids | share_authorship, t3_share_privacy | indirect/missing session delivery receipts not re-run |
| `/api/artifacts/:id` | device auth only, opaque id | none | UNVERIFIED: artifact id is a capability, not session-scoped |
| `/api/team/v1/*`, canvas, terminals, handoffs, device-sync snapshot | separate stores / member-bound | not audited | UNVERIFIED for task-data content |
| `/external-mcp`, `/mcp` | loopback + external token, not paired-device creds | n/a | event stream session_filter not share-filtered (separate credential) |

## Commands
`cargo test -p codetwo-server --test t3_share_privacy --test t3_mobile_compat --test ws_share_parity` exit 0
(logs/t3-red.log exit 101 -> logs/t3-green3.log exit 0, private under `.local/agent-work/20261013-full-sharing-audit-r25/`).
Processes: only test-owned loopback listeners/python stubs (exited with tests). Retained: this file, the new test.

# Source evidence

Checked against the live checkout `research/shared-agent-20261008`, HEAD `fb3cb82964acd3643ccd81f35f73ec0ab0489863` (equal to `origin/main`).

## Code facts (read, not run)

| Fact | Location |
| --- | --- |
| `KernelWebUiCommands::call` receives `_device_id` and does not use it. Commands carry no per-device identity today. | `crates/server/src/lib.rs:180-186` |
| `require_device` authorizes a bearer, then rejects a device bound to a team member with `team members must use the shared Task surface`. A normal paired device passes. | `crates/server/src/lib.rs:1142-1173` (message at 1168) |
| Engine events go through one `fanout` into a `broadcast` channel so many clients can subscribe. `ServerState` holds `Arc<Engine>` and the `broadcast::Sender<Event>`. | `crates/server/src/lib.rs:692-715` |
| The browser transport pairs at `/api/pair`, gets a WebSocket ticket at `/api/ws-ticket`, calls commands at `/api/web-ui/call`. The WebSocket is the single engine-event stream. | `apps/desktop/src/coreTransport.ts:80-82,119,171,247` |
| Environment routing for remote Cores. | `apps/desktop/src/remoteEnvironments.ts:178,470,541` |
| One-time short-TTL pairing token, then a long-lived revocable per-device bearer (stored hashed). This is a stable device credential, not hardware identity. | `crates/server/src/auth.rs:5-13`, `pair` at 376, `authorize_bearer` at 523 |
| `TaskRecord` has no `visibility` field today; it is new. The creator can serve as the task owner, but the execution-device field and creator semantics were not re-checked in this round. | `crates/core/src/task_store.rs:260` |

Not line-checked in this round: existing queue/steer and worktree behavior in `crates/core/src/engine.rs` and `crates/core/src/worktree.rs`. The proposal treats them as "reuse, verify later".

## What is not proven

- No real two-browser E2E was run. Multi-connection event fan-out exists in code, but two real clients were not connected in this round.
- The prototype self-check simulates events in one page. It does not show server-side filtering.

## Gemini draft (`codetwo-sharing-report-gemini-20261008-r1`)

The task was completed but not accepted, and no readable draft file was found in the allowed scope. This note replaces it. These Gemini claims are rejected:

- "hardware credential": the correct wording is stable device credential.
- "two real clients connected": no such acceptance was done.
- "simulation confirms server filtering": a simulation cannot prove that.

## Out of scope evidence

The istack 43 tests and SHA `69f18fbf79a3ff1287e2f1032a3183ad4aa97960` belong to an earlier writing-standard change. They are not part of this change's acceptance.

## Run identifiers (scrubbed for sharing)

Original request, task, thread and dispatcher identifiers, and the chat-delivery notification failure, are intentionally not published. They stay in the private, ignored local task records. Public facts kept: the research ran on model `claude-sonnet-5-5` with a 300k context and high reasoning effort under an `auto` runtime mode; the executor was forbidden to delegate again, so it has no downstream tasks; a notification to the dispatcher was rejected by the host with `runtime_mode_escalation_denied` and was not bypassed. Earlier delegated attempts `r2` (interrupted) and the Gemini draft are history only.

Current prototype evidence (final round): 11505 bytes, hash in [verification.md](../verification.md). Current rules (simulation only): a new task is public by default; public cannot return to private; a legacy private task can only be published one-way, and its owner is unchanged. Owner avatar, execution-device icon and share/lock icon are separate, each with title/aria-label; a light tap opens details. Older r5/r7 hashes and any two-way toggle wording are superseded. The run identifiers above are history only; source-code facts were not rechecked or changed this round. Board: 2026-10-08-shared-agent.

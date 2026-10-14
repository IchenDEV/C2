# Shared Agent: first real slice, implementation preparation (2026-10-12)

Status: preparation only. Runtime stays `codetwo-sharing-runtime=planned_not_implemented`. This document authorizes no implementation, service start, push, PR or invitation. Nothing was run; every source fact below was read statically.

Checkout caveat: facts were read in the dedicated research worktree, branch `research/shared-agent-20261008`, HEAD `fb3cb82964acd3643ccd81f35f73ec0ab0489863`, with unrelated dirty files present (see the private local task result; not published). `crates/server/src/lib.rs` and `crates/core/src/task_store.rs` were not in the dirty list at start. `crates/core/src/engine.rs` was dirty, so engine line numbers may move. Re-read before implementing.

## 1. Provenance of the rules

| Rule | Source | Type |
| --- | --- | --- |
| Guests use the normal full UI, start tasks, join others' public tasks, message/continue/stop | `intent.md` "Outcome (user decisions)" | Recorded decision |
| public/private is per task; one project holds both | `intent.md` same list | Recorded decision |
| No complex accounts/org/RBAC, no view-only guest, no own-task-only rule, no old Team Suggestion approvals | `intent.md` "Non-goals (user decisions)" | Recorded decision |
| Stable anonymous participant ID + nickname, bound to stable device credential, nickname is not authentication | `proposal.md` item 2, `spec.md` Design | Current proposal (research output) |
| Private = creator only | `intent.md` "Assumption, not a user decision" | Prototype assumption |
| Machine owner sees private tasks | `proposal.md` "Private tasks (assumption)", open | Undecided |
| New task public by default; public never returns to private; legacy private publishes one-way, owner unchanged | `intent.md` "Current rules (final round)", `proposal.md` line 40 | Simulation rules only; not approved runtime policy |
| Who may publish a legacy private task | `proposal.md` last paragraph, open | Undecided |
| Owner and execution-device labels, owner = creator, mark only | `intent.md`, `proposal.md` item 5, `spec.md` | Current proposal; owner label is not a permission |

### Provenance gap: claiming

- Current `intent.md` lists "any task-claim or follow-up flow" under "Non-goals (user decisions)", and `proposal.md`/`spec.md` use creator/owner plus execution-device labels. No `claimed_by` remains (`spec.md` AC-3).
- The dispatch brief says an earlier direct user brief explicitly requested nonexclusive claiming. I could not find that request in the change records or in the private run3 review. I did not search other threads.
- So the current records attribute the claim exclusion to the user, while the only pointer to an earlier user request for claiming comes from the dispatch brief. That is unconfirmed attribution in either direction. I did not invent a decision and did not reintroduce `claimed_by`. This document follows the current proposal (creator/owner label, mark only). **Human decision needed (D1):** does the user confirm that claiming is dropped, or want a nonexclusive "participant/follower" mark in addition to the creator?

## 2. EXISTING source facts

All paths relative to the repo root. Symbol and line were read from the live checkout.

| # | Fact | Location |
| --- | --- | --- |
| E1 | `KernelWebUiCommands::call` takes `_device_id` and ignores it. Commands carry no per-device identity today. | `crates/server/src/lib.rs:182-184` |
| E2 | `web_ui_call` runs `require_device`, then `commands.call(&device_id, &body.name, body.args, body.project_path)`. This is the single HTTP command exit. | `crates/server/src/lib.rs:967-985`; route at `:831` |
| E3 | `require_device` authorizes a bearer, then rejects a device bound to a team member: `team members must use the shared Task surface`. A normal paired device passes. | `crates/server/src/lib.rs:1142-1172` (message ~1168) |
| E4 | Pairing routes `/api/pair`, `/api/ws-ticket`. Pairing is single-use; the device record keeps only a token hash. | `crates/server/src/lib.rs:851-852`; `crates/server/src/auth.rs:60-75` (`Device`), `:376` (`pair`), `:523` (`authorize_bearer`) |
| E5 | `fanout` turns one engine channel into a `broadcast::Sender<Event>`. | `crates/server/src/lib.rs:707` |
| E6 | `handle_socket` computes `team_member_id` from the device, sends a welcome from `engine.list_sessions()`, then filters. Team members see only `member_can_access_session`; non-members have team-managed sessions removed. | `crates/server/src/lib.rs:2319-2340` |
| E7 | The same function forwards broadcast events with the same split. Team members see events passing `member_can_access_task/session`; non-members skip `TaskSnapshotChanged` and team-managed session events. | `crates/server/src/lib.rs:2352-2392` |
| E8 | Existing access helpers for the Team model: `member_can_access_task`, `member_can_access_session`, `session_is_team_managed`. | `crates/core/src/task_store.rs:2515`, `:2538`, `:2585` |
| E9 | `Task` has `id, status, result_contract, provider_configuration, budget`; no visibility, creator or device. `TaskRecord` wraps it with timestamps. | `crates/core/src/task.rs:476-482`; `crates/core/src/task_store.rs:260-265` |
| E10 | `Engine::enqueue_prompt(session, doc, mode, request_id, fence)` has a `mode == "steer"` branch and persists a `PromptDelivery` with `state: "queued"`. The signature carries no author. | `crates/core/src/engine.rs:5462-5500` (dirty file) |
| E11 | `session_can_steer` depends on `capabilities.steering == SteerSupport::Native`. | `crates/core/src/engine.rs:5575-5581` |
| E12 | Worktrees are created per session (`prepare_session_worktree`) and listed per project. | `crates/core/src/engine.rs:8290`, `:4372` |
| E13 | Browser transport: `/api/pair`, `/api/ws-ticket`, `/api/web-ui/call`. | `apps/desktop/src/coreTransport.ts:80-82,119,171,247` (from `source-evidence.md`, not re-read here) |

### Key finding: "task" in the mock is a Session in source

The mock's "task" is what the normal UI shows, which comes from `engine.list_sessions()` (E6). The existing `Task`/`TaskRecord` types (E9) belong to the Team Task surface that normal devices are deliberately kept out of (E3, E6, E7). So visibility, creator and participant data should attach to the **Session** identity, not to `Task`, and the Team Task surface should stay untouched. `source-evidence.md` says "`TaskRecord` has no `visibility`"; that is true but points at the wrong record for this slice. This must be settled first (D5).

### Not verified (static reading only)

- Whether `web_ui_call` command names map to sessions, projects, or both, and which commands take a session id. The command table was not read. Without it, the "HTTP command" filter cannot be specified precisely.
- Queue/steer: only the signature and a branch were read (E10, E11). Ordering, concurrent senders, a stop during a queued message, and multi-author behavior are **unverified**.
- Worktree: sessions get their own worktree (E12), but two participants writing into one session's worktree is **not claimed safe**. Nothing was run.
- Frontend event handling (`apps/desktop/src`) was not read.

## 3. PROPOSED change map (nothing exists yet)

Principle: one authoritative store. Application visibility only; this is not hostile-tenant filesystem or credential isolation, and none is added.

### P1. Single data source (new table(s) in the core store)

Proposed, placed beside the session/task store in `crates/core/src/task_store.rs` or a new sibling module:

- `participants(participant_id PK, nickname, created_at)`.
- `participant_devices(device_id PK, participant_id FK)`; `device_id` is the existing paired device id (E4).
- `session_share(session_id PK, visibility CHECK in ('public','private'), creator_participant_id, execution_device_label, created_at, published_at NULL)`.
- `session_message_author(session_id, delivery_id/request_id, participant_id, op_kind in ('message','continue','stop'), ts)`.

Invariants:
1. Every session visible to a normal device has exactly one `session_share` row. A missing row is treated as the legacy case, see D3, and **fail closed** (invisible) until D3 is decided.
2. `visibility` transitions only `private -> public`, enforced in a store transaction and a SQL guard, not only in the UI.
3. Creator never changes after creation, including on publish and on stop/continue by others.
4. Row insert with the session creation, and the author row with the delivery (E10 `enqueue_prompt`), are in the same transaction as the operation they describe. Otherwise a crash leaves unattributed or unlabeled work.
5. Public shared control is not exclusive ownership: no code path may use `creator_participant_id` to allow or deny public-task message/continue/stop.

### P2. Access predicate (new)

One function, for example `can_view_session(device_id, session_id) -> bool`, in the same module: public -> true; private -> the device's participant is the creator (prototype assumption), with an explicit hook for the machine-owner decision (D2). All four exits must call this function and no other copy of the rule.

### P3. The four exits (all must be covered)

| Exit | Existing site | Proposed change |
| --- | --- | --- |
| List | `handle_socket` welcome, `crates/server/src/lib.rs:2330-2340`; any list command behind E2 | Add `retain(can_view_session)` in the non-member branch. Include the web-ui list commands |
| Direct read | Commands behind E2 that take a session id or return a transcript/snapshot | Check inside the command layer; return the same response as "not found" for private-hidden so existence does not leak |
| HTTP command | `web_ui_call` -> `commands.call`, E2 | Pass `device_id` through (E1 drops it today), check before executing, and record author |
| Realtime event | event loop, `crates/server/src/lib.rs:2352-2392` | Add `can_view_session(event_session_id)` in the non-member branch. Events with no session id (global) must be reviewed one by one |

Member-only Team path: keep E3/E6/E7 unchanged. A team-bound device continues to be rejected from the ordinary surface; the new predicate applies only to non-member devices.

### P4. Participant identity

- At pairing (`auth.rs` `pair`, E4), create or attach a `participant_id` and nickname for the device. A re-pair on a new device is a new participant unless D4 says otherwise. Keep the wording "stable device credential", never "hardware identity". Nickname is a label, not authentication.
- Return `participant_id`/nickname to the web client and show them on messages; the author comes from the server-side device mapping, never from client-supplied fields.

### P5. Client (apps/desktop)

Minimal: nickname entry on first connect, per-session owner/device/visibility icons per the prototype (title/aria-label), author label on messages, one-way "publish" action only if D3 is confirmed. No claim UI.

### P6. Out of scope

No accounts/SSO/RBAC, no spectator mode, no Team Suggestion flow, no filesystem/credential isolation, no cross-device scheduling for the execution-device label (a label only; its real source is open, D6).

## 4. Ordered implementation plan (for a future authorized executor)

1. Read the `web_ui_call` command table and list every command that takes or returns a session. Output: command -> read/write/list classification. This resolves the biggest static gap and needs no decision.
2. Add schema and `can_view_session` in core with unit tests (transitions, fail closed, creator immutable).
3. Pass `device_id` through `KernelWebUiCommands::call`; add authorship recording in the same transaction as the operation.
4. Apply the predicate to list, direct read, command and event exits; one regression test per exit.
5. Pairing: create the participant, expose id/nickname.
6. Client labels, nickname entry, author display.
7. Run the acceptance matrix with a real isolated Core and two real isolated browser profiles.

Items 1-2 can start now independent of D2/D3 (default-public and creator-only predicates with the hook). Items 3-7 depend on D5 only. Publish (D3) and machine-owner view (D2) can be added last.

## 5. Acceptance matrix (two independent isolated clients in the normal UI)

Client A and client B: separate browser profiles, separate pairing, same Core, one project. Evidence must be actual server logs/responses plus screenshots at the real UI. A simulation, mock or unit test does not stand in for E2E.

| # | Scenario | Expected |
| --- | --- | --- |
| A1 | A creates public task T1 | B sees T1 without refresh trickery; creator = A, device label shown |
| A2 | B messages T1; A sees it | Author shows B's nickname on both clients |
| A3 | B continues, then stops, T1; A observes | State syncs; creator stays A; B was not blocked by "not owner" |
| A4 | A messages T1 while B is also sending | Both land via queue/steer; order and authors recorded. Behavior observed and written down, not assumed |
| A5 | B creates public T2 | A can participate; creator = B |
| A6 | Reload both clients | Labels, authors, visibility persist |
| N1 | A has a private task P1 | B's list does not show P1 (list) |
| N2 | B requests P1 by id directly | Same result as nonexistent (direct read) |
| N3 | B sends message/continue/stop to P1 over HTTP | Rejected, no side effect, no event emitted (HTTP command) |
| N4 | While B is connected, A messages P1 | B's WebSocket receives no event for P1, including state/stream/snapshot events (realtime) |
| N5 | B reconnects mid-run of P1 | No replay of P1 history |
| N6 | Team-bound device uses the ordinary surface | Still rejected with the existing message (E3) |
| N7 | Error messages and counts | No P1 title/id/count leaks in errors, badges or lagged-count messages |
| P1 | If D3 confirmed: A publishes P1 | B sees it; creator unchanged; reverse (public to private) is rejected server-side, via direct HTTP, not just UI |
| P2 | Publish by B on A's private task | Behavior per D3; if undecided, reject |
| M1 | Mixed project with public and private tasks | All four exits correct simultaneously |

Minimal meaningful tests: store tests for transitions and fail-closed; one server test per exit with two device ids; one E2E (two clients) covering A1-A3, N1-N4. Do not add tests that only assert the mock.

## 6. Preflight and stop conditions for a future executor

Preflight (before any launch):
- Read `.agents/skills/codetwo-operations/references/desktop-instances.md` and `docs/design/desktop-development-profiles.md`. One desktop data directory has exactly one live Core owner; profile-based launches are not supported until implemented and verified.
- Use a new, separately authorized implementation worktree; do not use the main checkout or this research worktree.
- Use a throwaway data directory; never real user data, credentials or Agent storage; check the port is free without stopping any process.
- Record branch, HEAD, status, and the process list before and after.

Stop and report if:
- The target port/data directory belongs to a live Core owned by someone else.
- A needed command table or store change would touch Team Task semantics (E3/E6/E7).
- Any exit cannot be filtered at the server without a client change.
- Queue/steer or worktree behavior under two authors appears unsafe (lost message, cross-write); do not paper over it.
- D2/D3/D5 need a decision that is not recorded.
- The host denies an action: stop that action, save the raw error.

## 7. Decisions and independence

| ID | Open decision | Blocks |
| --- | --- | --- |
| D1 | Claiming: confirm it is dropped, or a nonexclusive mark is wanted (provenance gap, section 1) | Label set in the client only |
| D2 | Can the machine owner see private tasks? | Predicate hook only |
| D3 | Legacy private publish: who may publish, and is one-way publish approved as runtime policy? Also how legacy sessions without a share row are treated | Publish action, legacy migration |
| D4 | Does a new device of the same person count as the same participant? | Pairing step |
| D5 | Confirm that visibility attaches to Session and not to Team `Task` (section 2) | Schema placement |
| D6 | Real source of the execution-device label | Device label content |

Can proceed independently: plan steps 1-2, and the four-exit tests with default-public and creator-only predicates. The preview acceptance stays PARTIAL/blocked and is unaffected by this document. Even a later passing simulation does not prove a real Agent, two real clients, or server-side filtering.

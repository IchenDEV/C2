---
id: 2026-10-13-shared-agent-runtime
schema: 5
stage: spec
status: accepted
owner: research-shared-agent r5 executor
created: 2026-10-13
based_on: intent.md
---

# Spec: Shared Agent Runtime

## Design

Route trace (live checkout, HEAD fb3cb82):

- Ordinary React UI: `apps/desktop/src/coreTransport.ts` posts commands to `/api/web-ui/call` with the bearer, pairs at `/api/pair`, and gets a ticket from `/api/ws-ticket` for `/ws?ticket=` (events). Server: `web_ui_call` -> `require_device` (bearer -> `device_id`) -> `WebUiCommandCaller::call(device_id, name, args, project_path)` -> `KernelWebUiCommands::call`, which ignores `_device_id` (crates/server/src/lib.rs). Events: `ws_handler` `?ticket=` -> `handle_socket(device_id)`.
- T3 compatibility (`crates/server/src/t3_compat.rs`: `/api/orchestration/dispatch`, `/ws?wsTicket=`, `/oauth/token`) is a different mobile/T3 surface. The frontend never calls it.
- Conclusion: R2/R4 changed the wrong surface. The candidate was withdrawn: `t3_compat.rs` equals HEAD again. The patches are archived and were applied in reverse after `git apply -R --check`.

Next increment (not done): carry the bearer-derived `device_id` from `web_ui_call` to a Core consumer. Ordinary UI path has no actor consumer, and `crates/core` is dirty from other workers, so this is recorded as a dependency.

Proposal, unapplied and NOT approved (high risk, needs an independent design decision and a corrected design before any implementation): per-actor receipt keys for the inner SQLite `t3-prompt`/`t3-create` and for any ordinary-UI command receipts. An earlier draft read legacy actor-less rows as "any actor". That any-actor legacy fallback is NOT an approved privacy design: a legacy receipt visible to every actor would leak across the creator-only private rule. Legacy rows must fail closed (invisible) or be migrated under an explicit owner decision. No code or format change exists for it.

Not enforced (NOT_IMPLEMENTED; private creator-only including the machine owner is a confirmed requirement, nonexclusive claiming remains): private creator-only visibility, machine-owner filtering, nicknames, claiming, a sharing entry in the UI.

## Acceptance criteria

- [x] AC-1: The ordinary UI path is traced from the frontend transport to the server command and the actor seam, and distinguished from the T3 compatibility surface.
- [x] AC-2: The wrong-surface, persistence-changing `t3_compat.rs` candidate is withdrawn with the patches archived; no other worker's change is touched.
- [ ] AC-3: Real compile and tests of the ordinary UI actor path run (`cargo test -p codetwo-server`); the draft test `draft-web-ui-actor-test.rs` is wired and passes.
- [x] AC-4: A concrete draft test and an unapplied persistence proposal exist; neither is called accepted or tested.

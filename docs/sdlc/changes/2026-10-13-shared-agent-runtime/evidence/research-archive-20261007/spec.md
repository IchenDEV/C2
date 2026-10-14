---
id: 2026-10-07-shared-agent-research
schema: 5
stage: spec
status: accepted
owner: shared-agent-research-subagent
created: 2026-10-07
based_on: intent.md
---

# Spec: Shared Agent Research

## Design

Recommended approach (details in [evidence/proposal.md](evidence/proposal.md), source facts in [evidence/source-evidence.md](evidence/source-evidence.md)):

- Reuse the normal remote React Web UI, the headless Core, HTTP/WebSocket transport and one-time device pairing.
- Add only: a minimal share entry, a stable anonymous participant ID plus nickname, a per-task `visibility` (new tasks default public; public never returns to private; an existing private fixture may only be published one-way), authorship on messages and actions, and two display labels per task: task owner (reuse the creator as the single source) and execution device (simulated fixtures in the prototype; no real cross-device scheduling is claimed). The owner label is not tied to permissions; no new RBAC.
- A nickname is not strong authentication. The stable device credential binds ownership. No password or SSO is added.
- Every public task works normally for every participant.
- Future private tasks need server-side filtering at four exits: list, direct read, HTTP command, live event. Hiding cards in the frontend is not enough.
- Application-level visibility is not hostile-tenant isolation of files or credentials on the same machine and OS user. The user did not ask for that isolation, so the platform is not extended.
- Several people messaging one task reuse the existing queue/steer behavior. Independent development tasks use the existing worktrees. Safe concurrent editing in one shared worktree is not claimed.

Prototype: [evidence/prototype.html](evidence/prototype.html) is a single page that simulates events in memory. It has no real Agent connection. Its self-check proves only the simulated event handling, owner and simulated-device labels, new-task default owner, and that public stop/continue leave the owner unchanged. It does not prove the product, two real clients, or server-side filtering.

## Acceptance criteria

- [x] AC-1: The Intent, Spec, Plan and Verification are complete, contain no template placeholders, and state that private-visibility semantics are an assumption.
- [x] AC-2: Source facts are cited with real paths and lines, and the record states that no real two-browser E2E was run and the old Gemini wording ("hardware credential", "two real clients connected", "simulation proves server filtering") is rejected.
- [ ] AC-3 (PARTIAL, blocked; see verification.md): The prototype list and selected task both show task owner and a simulated execution device; no claim UI or `claimed_by` remains; a new task defaults to its starter; public participation and stop/continue keep the owner. Owner/device/visibility show as compact distinct icons (avatar per person, computer vs server SVG, share vs lock SVG) with names in title/aria-label and tap-to-open `details`. It must be rendered at 728 and 390 and the self-check must pass (historical, unreproducible claim: final 11505-byte version previewed at 728 and 390; no saved PNG and no input manifest exist). Failure path: if rendering or capture is unavailable, Verification must say so rather than claim a pass.
- [x] AC-4: No runtime code, catalog, real data, auth or Agent storage is changed, and the main checkout is unchanged.

<!-- final round: default-public tasks; legacy private only one-way publishable, owner unchanged; 11505-byte prototype verified, see verification.md; board 2026-10-08-shared-agent -->

<!-- Update 2026-10-13: private is creator-only including the machine owner (confirmed); claiming remains nonexclusive; public-default and one-way publish are simulation proposals, not approved runtime rules. AC-3 is PARTIAL. See evidence/handoff.md. -->

# Proposal: share a cloud Agent with other people

Status: research proposal. Nothing here is implemented.

Update 2026-10-13: private = creator only, including the machine owner (confirmed); the "assumption" and "open" wording below is superseded. Claiming stays a nonexclusive mark, so the "no claim" wording is superseded. Public default and one-way publish remain simulation proposals, not approved runtime rules. See [handoff.md](handoff.md).

## Approach

Reuse what exists: the normal remote Web UI, the headless Core, HTTP/WebSocket transport, and one-time device pairing (see [source-evidence.md](source-evidence.md)). Add the smallest set of things:

1. A share entry that produces a normal one-time pairing link.
2. A stable anonymous participant ID plus a nickname, bound to the stable device credential. A nickname is not strong authentication. No password or SSO.
3. A `visibility` field (public/private) on each task.
4. Author on messages and on stop/continue actions.
5. Show per task a simulated owner label (who it belongs to) and execution device label (which machine runs it). The owner reuses the creator and is a mark only: no exclusive control, flow or permission layer. New tasks default to the starting participant.

All public tasks behave normally for all participants.

## Private tasks (assumption)

The prototype treats private as "creator only". This is an assumption. The user has not decided whether the machine owner can also see a private task.

A real implementation must filter on the server at four exits: task list, direct read, HTTP command, live event. Hiding cards in the frontend is not enough.

## Limits

- Application visibility is not isolation of files, credentials or processes between hostile users on one machine and OS user. The user did not ask for that, so it is not planned.
- Many people messaging one task reuse the existing queue/steer. Independent development tasks use the existing worktrees. Concurrent editing of one shared worktree is not claimed to be safe until tested.

## First implementation slice (future, not evidence today)

Two isolated local clients open the same normal Web UI and show:

- public tasks visible to both;
- owner and device labels shown per task (owner never changes who may take part or stop/continue);
- message, continue and stop in both directions;
- authorship and status in sync;
- a mixed public/private set filtered by the server.

This E2E belongs to a later real implementation. It was not run in this round.

Current prototype evidence (final round): 11505 bytes, hash in [verification.md](../verification.md). Current rules (simulation only): a new task is public by default; public cannot return to private; a legacy private task can only be published one-way, and its owner is unchanged. Owner avatar, execution-device icon and share/lock icon are separate, each with title/aria-label; a light tap opens details. Older r5/r7 hashes and any two-way toggle wording are superseded.

Next stage (not started): real sharing is tracked separately as `codetwo-sharing-runtime=planned_not_implemented`. This round only passed the prototype; multi-user sharing is not done. No implementer or isolated workspace is assigned yet; the root dispatcher decides. Future acceptance needs two independent real clients on one Core covering public-task participation, attribution and stop/continue sync; real owner and device ownership; one-way publish; and server-side list/read/command/event filtering. Open: whether the machine owner sees private tasks and who may publish. Queue/steer/worktree stay unverified.

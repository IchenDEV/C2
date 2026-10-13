---
id: 2026-10-07-shared-agent-research
schema: 5
stage: intent
status: accepted
owner: shared-agent-research-subagent
created: 2026-10-07
source: user
risk: low
approved_by: "requester (user)"
approved_at: "2026-10-07"
approval_source: "User request to research and simulate sharing a cloud Mac/VM Agent; local research and a discussion prototype only. Authorizes no runtime implementation, service start, push, PR, release or invitation."
next_trigger: A separate request to implement the first slice in evidence/proposal.md.
---

# Intent: Shared Agent Research

## Intent

Problem: the owner wants to share an Agent that runs on a cloud Mac/VM with other people.

Outcome (user decisions):

- A guest opens the same full interface as a normal user. They can start tasks, join other people's public tasks, and message, continue or stop a running Agent.
- Each task only shows whose it is (task owner) and which device runs it (execution device). The owner is an ownership mark, not an exclusive control, flow or permission layer. A new task defaults to the participant who starts it.
- public/private is set per task. One project can hold both.

Non-goals (user decisions): complex accounts, organizations or RBAC; a view-only guest UI; a rule that people can only act on their own tasks; any task-claim or follow-up flow; the old Team Suggestion approval flow.

Assumption, not a user decision: for the discussion prototype, a private task is visible only to its creator. Whether the machine owner can also see it is open.

Scope of this round: research and a simulated prototype only. No real service is started, nothing is deployed, no invitation is sent. The four-stage acceptance covers only this authorized low-risk research and discussion prototype. It does not approve any high-risk implementation.

Original request source: user request in a T3 thread (request identifiers are private and not published). Details are in [evidence/source-evidence.md](evidence/source-evidence.md).

## Simulation rules (final research round; proposals, not approved runtime rules)

Simulation rules only: a new task is public by default; public cannot return to private; a legacy private task can only be published one-way, and its owner is unchanged. Owner avatar, execution-device icon and share/lock icon are separate, each with title/aria-label; a light tap opens details. The 11505-byte prototype is checked in [verification.md](verification.md). Board: 2026-10-08-shared-agent.

## Update 2026-10-13 (supersedes the assumptions and non-goals above where they conflict)

Latest recorded user decisions, relayed by the coordinating thread and not independently authenticated (see [evidence/handoff.md](evidence/handoff.md)):

- Private tasks are visible to their creator only. This includes the machine owner: the owner is filtered by the same rule. This is CONFIRMED; the "assumption, not a user decision" line above is superseded.
- Claiming stays a nonexclusive following/signature, never exclusive control. The earlier non-goal "any task-claim or follow-up flow" and the "no claim" proposal wording are superseded.
- The simulation rules "new task public by default", "public never returns to private" and "legacy private publishes one-way" are NOT approved runtime rules. Public default and one-way publish still need a runtime decision.
- The prototype preview verdict is now PARTIAL/blocked, see [verification.md](verification.md).

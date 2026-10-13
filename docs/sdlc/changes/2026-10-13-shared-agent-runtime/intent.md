---
id: 2026-10-13-shared-agent-runtime
schema: 5
stage: intent
status: accepted
owner: research-shared-agent r5 executor
created: 2026-10-13
source: user
risk: medium
approved_by: chenli (user, claim recorded by the executor; not authenticated)
approved_at: 2026-10-13
approval_source: User request in this session 2026-10-13 ("继续完成"; "User authorized function development and creator-only private") and parent brief r5 v1.
next_trigger: A coordinator picks the next increment (ordinary Web UI actor plumbing in KernelWebUiCommands) and decides the persistence design in Spec.
---

# Intent: Shared Agent Runtime

## Intent

Share one cloud Mac/VM Agent through the ordinary full Web UI. Participants start tasks and may nonexclusively claim (follow) them,
join other participants' public tasks, and message, continue, or stop the running Agent. Visibility
is per task (public or private) inside one project. Claim is a nonexclusive following/signature, never
exclusive control. Participant identity is stable and anonymous, tied to a stable device credential,
and authorship is derived by the server.

User decisions (authorization recorded as a claim, not authenticated):

- "进入功能开发，你可以派出 subagent" (2026-10-13): functional development of this feature is authorized as bounded local work.
- Private visibility choice "仅创建者可见": a private task is visible only to its creator, and the machine owner is filtered by the same rule.
- "继续完成" (r5): continue within the same bounds.

Not authorized: production or user services, real user data, push/PR/release, commit, auth/network settings, payments or privilege changes.

## Scope of this record

Risk `medium` applies to this increment only: route trace, withdrawal of a wrong-surface candidate, and
an unapplied draft. The whole feature (private visibility, persisted per-actor keys, migration) stays high
risk; those parts are proposals in Spec and are not accepted or implemented here.

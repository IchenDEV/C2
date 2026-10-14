---
id: 2026-10-13-shared-agent-runtime
schema: 5
stage: intent
status: accepted
owner: research-shared-agent r5 executor
created: 2026-10-13
source: user
risk: high
approved_by: chenli (user, claim recorded by the executor; not authenticated)
approved_at: 2026-10-13
approval_source: User requests in this session on 2026-10-13 ("进入功能开发，你可以派出 subagent"; "仅创建者可见"; "继续完成"; "开始实现完整分享功能") and the parent briefs for rounds r1-r36. Original user messages (positions 172, 183, 340, 348, 349 of the parent thread) are listed with timestamps in spec.md `design_approval_source`.
next_trigger: Human review of the draft PR and required checks, then merge under the 2026-10-14 authorization; release or rollout need their own authorization.
---

# Intent: Shared Agent Runtime

## Intent

Share one cloud Mac/VM Agent through the ordinary full Web UI. Participants start tasks, may follow
(sign) a task without taking exclusive control, and may message, continue, queue, or stop the
running Agent of any task they can see. Participant identity is a stable anonymous device actor
derived by the server; authorship is server-derived, never taken from client arguments.

Current decisions (user, recorded as claims, not authenticated):

- New tasks default to **public** (visible to every paired participant and the host). A task may be
  created **creator-only private**; the machine owner is filtered like any other participant (included
  only when they created the task).
- Sessions that have no share record (created before sharing, or by a cross-host handoff without
  creator provenance) stay **local, unshared**, and remote activation of them is unsupported.
- Normal full UI, public shared control, follow tracking and signatures. No accounts or RBAC.

## Authorization actually recorded

Bounded local functional development of this feature, in this worktree only, as given by the user
requests above. Update 2026-10-14: the user wrote `merge` and said the original author is offline and the Agent now owns the delivery. That authorizes the feature commit, push, a draft PR and merge after independent review and required checks, for this sharing change only. The user's product and privacy decisions above are unchanged. Still not authorized and not done: release, production or user services,
real user data or auth, invites, payments, installs, network or permission changes. No human code review,
waiver, or high-risk rollout approval exists or is claimed (the user's product/privacy decisions are the existing design decisions linked in spec.md). The runtime privacy design
(`private` as creator-only, durable authors, per-actor request ids) touches authorization behavior; it
is high risk (an earlier `medium` was carried over from research and was wrong). The user's explicit bounded
feature, private and public decisions stand; no human review or rollout approval was recorded for it. Release or rollout would need its own independent decision.

## Scope boundary

Application visibility only, not OS isolation. Fixture-free limits: the zero-fee ACP test agent does not
advertise native steering, so native steer is unsupported and unverified; no paid Agent, cloud, CI or
deployment evidence exists.

History: earlier wording (wrong-surface `t3_compat.rs` candidate, unapplied persistence proposal,
NOT_IMPLEMENTED verdict, high-risk-only framing) is superseded; exact prior versions are retained in the
ignored private archive named in [final-verification-r36](evidence/final-verification-r36.md).

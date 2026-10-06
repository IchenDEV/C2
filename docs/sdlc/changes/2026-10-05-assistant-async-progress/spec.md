---
id: 2026-10-05-assistant-async-progress
schema: 5
stage: spec
status: accepted
owner: codex
created: 2026-10-05
based_on: intent.md
design_approved_by: user
design_approved_at: 2026-10-04
design_approval_source: "Original user message at position 709: 继续实现, following position 707 and linked coordination spec sections 6 and 9: Provider outside transaction, independent goals progress, durable attempts, stop before replacement. Original thread read on 2026-10-05."
---

# Spec: Independent asynchronous progress

## Design

This is a structural correction to the [approved coordination design](../2026-10-04-assistant-coordination-loop/spec.md), not a replacement engine. One revisioned assistant document remains authoritative. Its short commit gate serializes edits, worker calls and decision commits; it never encloses a Provider await. Each goal/request has its own bounded review run and fingerprint of the actual scoped inputs, dependencies, controls and memory. A completion commits against the latest document only if its run id and scoped inputs still match. Global revision protects the write transaction, not model-result freshness. Global resource limits are checked again at commit.

Session creation and initial prompt attempts are durably claimed before asynchronous execution. Live ownership is ephemeral; recovery uses the original Engine command receipt. Missing receipts after a claimed attempt mean unknown, not automatic resend. Completed creation attaches only to the original assignment/run; revoked control cannot authorize its prompt. Legacy global runs are retired after recording their original receipt/outcome; they never block new independent scopes or restart.

Engine delivery keeps its persistent outbox. Each session has a separate delivery lane and no global Provider lock. Claim, send and receipt remain distinct. Stop/cancel persists before the synchronous Engine control submission, independently of data delivery. A pending stop or initialization/unknown attempt still owns the writer slot. Native question answers record a claim before touching the live channel; restart never replays an uncertain permission.

Use existing Tokio and SQLite. No broker, new task store or parallel execution owner. Review results/errors are scoped and visible; a waiting review does not freeze unrelated scopes. One active review per scope, bounded by existing concurrency/turn allowance.

## Acceptance criteria

- [x] AC-1: Slow A initialize/session-new/model/steer does not block B intake, dispatch or receipt, or acquisition of edit/worker commit gate.
- [x] AC-2: Stop/cancel persists promptly and reaches Engine control independently; no replacement before the old writer has a terminal receipt, including initialization.
- [x] AC-3: A/B reviews progress independently; B updates preserve A decisions; same-scope control/requirements/dependency changes reject stale decisions.
- [x] AC-4: Claim/send/receipt crash windows retain questions and controls, preserve original attempt ids and never blindly replay unknown operations; old data reads safely.
- [x] AC-5: Rendered app distinguishes recorded, accepted and adopted, including steering-supported and unsupported routes; existing internal coordination and shared Engine paths regress cleanly.
- [x] AC-6: Applicable build, docs, lifecycle, independent verification and accountable cleanup complete; prior passed evidence is labeled historical for affected behavior.

Resource-release correction: a review rejected locally for worker concurrency may start a fresh bounded review when the same global admission predicate reports free capacity. This is a scheduling wake condition, not a change to the running review input fence or a replay of a Provider attempt. Commit still rechecks capacity and authority.

- [x] AC-7: The terminal13 real ACP resource-release probe passes; no retry while full or on unknown outcomes, and competing waiters retain the global concurrency bound.

Existing capacity failures without the new marker require the original accepted prompt receipt, an Idle review session and a valid scoped dispatch output before restoring eligibility. Explicit non-retryable attempts never enter this recovery path.

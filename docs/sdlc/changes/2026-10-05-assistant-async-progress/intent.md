---
id: 2026-10-05-assistant-async-progress
schema: 5
stage: intent
status: accepted
owner: codex
created: 2026-10-05
source: user
risk: high
approved_by: user
approved_at: 2026-10-05
approval_source: "Original thread 91d5d393-cf9a-4530-add8-406180f05ce9: user approved first design at position 65, requested implementation of coordination spec at 709; continuation 82a98172-c09b-4a1f-b4a5-eb69d75516f4 explicitly requests bounded async completion."
---

# Intent: Independent asynchronous progress

## Intent

Complete the already authorized internal coordination behavior. Slow session creation, prompt setup or steering for A must not hold edit/worker intake or independent B. Preserve one Engine, Memory Store, identity binding, durable receipts and one delivery path. Do not add external messages, cloud services, permissions, release or a second implementation team.

Original decisions were read from the original thread, including the linked [coordination spec](../2026-10-04-assistant-coordination-loop/spec.md). The latest model proposal alone is not approval. This follow-up implements the approved transaction-outside-provider and independent-goal contracts; it does not authorize mail/Feishu integration. Supervision inputs are the user-supplied task-7 probes and result JSON outside this repository.

Follow-up authorization: user feedback `codetwo-r1-resource-release-20261005-terminal13` requests the narrow resource-release correction and the same real ACP probe plus original regressions. No new development team, independent task, model/permission changes or release.

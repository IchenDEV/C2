---
id: 2026-10-13-shared-agent-runtime
schema: 5
stage: plan
status: accepted
owner: research-shared-agent r5 executor
created: 2026-10-13
based_on: spec.md
scope: apps/desktop/src-host/src/remote.rs, apps/desktop/src-host/src/host_events.rs, apps/desktop/src-host/src/lib.rs, crates/napi/src/lib.rs, crates/core/src/share_policy.rs, crates/core/src/share.rs, crates/core/src/engine.rs, crates/core/src/event.rs, crates/core/src/lib.rs, crates/core/src/artifact.rs, crates/core/src/prompt_delivery.rs, crates/core/src/plugins/app/mod.rs, crates/core/src/plugins/app/plugins/engine.rs, crates/core/examples/live_demo.rs, crates/core/tests/native_share.rs, crates/core/tests/share_authorship.rs, crates/core/tests/share_store_failclosed.rs, crates/core/tests/share_create_atomic.rs, crates/core/tests/share_boundaries.rs, crates/core/tests/share_device_sync_memory.rs, crates/core/tests/prompt_delivery.rs, crates/server/src/lib.rs, crates/server/src/serve.rs, crates/server/src/t3_compat.rs, crates/server/examples/share_fixture.rs, crates/server/tests/share_policy.rs, crates/server/tests/ws_share_parity.rs, crates/server/tests/t3_share_privacy.rs, crates/server/tests/web_ui_commands.rs, crates/server/tests/team_collaboration.rs, crates/server/tests/pairing_link.rs, apps/desktop/src/App.tsx, apps/desktop/src/bridge.ts, apps/desktop/src/session/share.ts, apps/desktop/src/session/ShareControls.tsx, apps/desktop/src/session/turns.ts, apps/desktop/src/session/TurnCard.tsx, apps/desktop/src/session/SideChatPanel.tsx, apps/desktop/src/session/sessionEvents.ts, apps/desktop/src/sidebar/SessionRail.tsx, apps/desktop/src/styles.css, apps/desktop/tests/share.test.ts, apps/desktop/tests/sharedTurnReconcile.test.ts, apps/desktop/tests/sessionState.test.ts, apps/desktop/tests/windowChromeContract.test.ts, docs/sdlc/changes/2026-10-13-shared-agent-runtime/
---

# Plan: Shared Agent Runtime

## Plan

Delivered in rounds r1-r35 (history in the evidence manifests): share store and policy, native and
remote exits, durable authorship, ordinary UI, fixture and real paired-device renders. R36 consolidates
these four records into one current contract and runs the current checks; it changes no source.

Scope: every sharing-changed path listed in the front matter, including `crates/core/examples/live_demo.rs`
(Event variant adaptation), `apps/desktop/tests/sessionState.test.ts` and `share.ts`/`share.test.ts` (R33/R34 changes).
Files shared with the visualization change (`TranscriptPane.tsx`, `VisualizationFrame.tsx`, `visualization.ts`,
`providerTools.ts`, engine/plugin hunks) keep their owner in
`docs/sdlc/changes/2026-10-07-visualization-response-bridge/`; hunks are not reformatted or reassigned.

R37 ran the current-bundle UI check (creator-private, 390 px header, actors from the store; no source change). R38 was the independent Agent review (AC-6, not human review). Remaining: human review; no commit, push, PR or release is authorized.

Delivery correction (R51, docs only): the historical research change `2026-10-07-shared-agent-research` has a blocked
verification (its prototype render was never saved), so it cannot be an active Ready change. Its 11 files are moved
unchanged (git renames, 100 % similarity) to `evidence/research-archive-20261007/` in this change, where its blocked status
and hypotheses stay as written; the original branch `research/shared-agent-20261008` still holds the same files. This change
is the only active Change in the PR. No source, test or config differs from the head the user reviewed.

Scratch: ignored private task directories under `.local/agent-work/`, no tracked scratch.

Rollback: nothing is committed. To discard, revert the paths above in the worktree (preserve the visualization hunks); the archived
previous record versions are in the ignored private archive. Do not touch other workers' dirty files.

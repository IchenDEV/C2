---
id: 2026-10-05-assistant-async-progress
schema: 5
stage: plan
status: accepted
owner: codex
created: 2026-10-05
based_on: spec.md
scope: docs/sdlc/changes/2026-10-05-assistant-async-progress/, docs/sdlc/changes/2026-10-04-assistant-coordination-loop/verification.md, crates/core/src/assistant.rs, crates/core/src/store.rs, crates/core/examples/live_demo.rs, crates/core/src/plugins/app/plugins/assistant.rs, crates/core/src/engine.rs, crates/core/src/prompt_delivery.rs, crates/core/tests/assistant.rs, crates/core/tests/prompt_delivery.rs, apps/desktop/src/assistant/, apps/desktop/tests/assistant.test.tsx
---

# Plan: Independent asynchronous progress

## Plan

Codex alone implements this bounded follow-up; reuse the existing independent reviewer for read-only verification, not a second development team. Remove global Provider waits, keep durable attempts and per-scope review input fences, then add positive regressions derived from the supervision probes. Check real ACP initialize/new/prompt/steering, stop and restoration, scoped decisions and crash windows. Run affected Core/Engine and desktop tests, renderer build and actual isolated Web Core rendering, then docs/sdlc checks. No changes to desktop instance isolation.

Temporary resources: `.codex/run/assistant-async/` owns build output, disposable databases, fixture scripts and processes. Keep small logs/artifacts under this record's evidence directory; stop owned processes and remove disposable output before handoff. Preserve prior deliverables, shared caches and other workers' resources.

Migration: serde-default scoped runs and attempts coexist only as data with the original document; the old global run has a one-way retirement path, never a second scheduler. Never drop an unknown original attempt or replay it during migration.

Rollback: Disable follow-up, stop owned execution and reconcile pending attempts before reverting implementation. Retain SQLite and deliverables; no destructive downgrade or replay of unknown attempts. Previous verification does not accept this implementation.

Terminal13 follow-up: Codex directly repairs the local capacity-rejection wake condition in the same domain/Runtime path. Extract the existing capacity predicate once, tag only the typed local rejection, and keep model input fingerprints scoped. Preserve and run the supplied probe plus original Runtime/coordination/delivery regressions. Task scratch: `.codex/run/assistant-capacity/`; no UI behavior changes or new independent task. Reuse matching UI evidence and clean the dedicated build before handoff.

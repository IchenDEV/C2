---
id: 2026-10-05-assistant-interaction-upgrade
schema: 5
stage: plan
status: accepted
owner: codex
created: 2026-10-05
based_on: spec.md
scope: apps/desktop/src/assistant/AssistantWorkspace.tsx, apps/desktop/src/assistant/CoordinationPanel.tsx, apps/desktop/src/assistant/AssistantSelect.tsx, apps/desktop/tests/assistant.test.tsx, docs/sdlc/changes/2026-10-05-assistant-interaction-upgrade
---

# Plan: Assistant Interaction Upgrade

## Plan

Codex owns this bounded renderer change. First reuse current API/domain guards and shared primitives; then reduce visible sections, migrate selected-goal/inbox flows, and update behavioral tests for the new navigation. No child team/task is started. Check affected assistant/notification/rail tests, lint, TypeScript and renderer/Web builds. Render the actual Web app on a single isolated Core with follow-up disabled and offline data; inspect light/dark/narrow views and execute request, selection, message and unknown-outcome flows. Rust behavior is unchanged, so previous matching backend tests are retained; only build the server if needed to render the actual app. Finish docs/sdlc and whitespace checks.

Temporary resources: owned `.codex/run/assistant-interaction-upgrade/` contains disposable Cargo target, Web assets, Core data and logs. Any server uses its own data directory and loopback port 14674; never stop a user process. One owned T3 preview tab is closed at handoff. Keep only compact logs and referenced screenshots in this bundle's evidence; remove the exact scratch tree after stopping its owner and checking ports/processes.

Rollback: restore only this turn's renderer/test edits from the pre-edit baseline, preserving original Core and chief-of-staff work; the change introduces no storage migration.

---
id: 2026-09-28-remove-plan-goal
schema: 5
stage: plan
status: accepted
owner: codex
created: 2026-09-28
based_on: spec.md
scope: crates/core/src/skill.rs, crates/core/src/plugins/app/service.rs, crates/server/tests/t3_mobile_compat.rs, apps/desktop/src-host/src/scene_mcp.rs, crates/core/schemas/agent-scenes, docs/reference/scenes.md, apps/desktop/src/App.tsx, apps/desktop/src/bridge.ts, apps/desktop/src/session, apps/desktop/src/environment/EnvironmentPopover.tsx, apps/desktop/src/i18n/strings.ts, apps/desktop/tests, crates/core/src/acp, crates/core/src/engine.rs, crates/core/src/event.rs, crates/core/src/session.rs, crates/core/src/memory.rs, crates/core/src/store.rs, crates/core/src/scene.rs, crates/core/src/plugins/app/plugins/engine.rs, crates/core/src/plugins/app/plugins/scene_commands.rs, crates/core/examples/live_demo.rs, crates/core/tests, crates/server/src/t3_compat.rs, docs/sdlc/changes/2026-09-28-remove-plan-goal
---

# Plan: Remove Plan Goal

## Plan

Codex owns this removal in the listed files, preserving the earlier uncommitted stability changes. Remove native Goal routes first; then remove Plan presentation, selectors and scene/draft activation. Keep only read-only historical decoding where required for data compatibility. Add regressions for ignored metadata, retired command/config rejection and legacy record loading. Run affected Core/integration tests, desktop tests, renderer types/lint/build, actual browser rendering, workspace check and documentation/SDLC checks.

Temporary resources: task-owned target/, apps/desktop/dist/, and .codex/run/remove-plan-goal/ for logs/preview. Stop preview processes and remove build outputs/scratch harness at handoff; retain small verification evidence until review. Reuse existing node_modules until worktree retirement. Do not restart a user desktop or delete provider/account data.

Rollback: revert only this feature-removal diff; no data migration.

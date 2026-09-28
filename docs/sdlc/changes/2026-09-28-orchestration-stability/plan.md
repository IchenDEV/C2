---
id: 2026-09-28-orchestration-stability
schema: 5
stage: plan
status: accepted
owner: codex
created: 2026-09-28
based_on: spec.md
scope: crates/core/src/worktree.rs, crates/core/src/store.rs, crates/core/src/acp/client.rs, crates/core/src/acp/mod.rs, crates/core/src/unix_process_group.rs, crates/core/src/plugins/app/protocol/mod.rs, crates/core/tests/acp_process_cleanup.rs, crates/core/src/codex_runtime.rs, crates/core/src/models.rs, crates/core/src/engine.rs, crates/core/src/lib.rs, crates/core/src/acp/wire.rs, crates/core/src/event.rs, crates/core/src/cost.rs, crates/core/tests/engine_builtin_models.rs, crates/core/tests/engine_provider_switch.rs, crates/core/tests/engine_store.rs, crates/core/tests/engine_permission.rs, crates/server/src/t3_compat.rs, apps/desktop/src/App.tsx, apps/desktop/src/bridge.ts, apps/desktop/src/session/Composer.tsx, apps/desktop/src/session/config.ts, apps/desktop/src/session/turns.ts, apps/desktop/src/providers/registry.ts, apps/desktop/src/i18n/strings.ts, apps/desktop/tests, docs/sdlc/changes/2026-09-28-orchestration-stability
---

# Plan: Orchestration Stability

## Plan

Codex owns the listed paths. Add regression cases before fixes; remove fabricated catalogues, improve the existing projection and continuation consumption, then reconcile picker state. Run focused Core integration/unit tests, workspace check for shared exports, desktop affected tests/types/lint/build, actual browser rendering, docs and SDLC worktree checks. No database schema or release changes. Persisted Unix worktree identity gains an optional birth timestamp; old records remain readable.

Continuation verification: use the opt-in canary with locally installed, authenticated providers, leave model choice to runtime discovery/defaults, and exercise a synthetic 64 Ki-character history across repeated switches. Resolve requested provider ids through the registry; always shut down the test-owned engine even if the canary fails. Record account/adapter failures separately from successful routes. The live probe exposed different Codex binaries for catalogue discovery and ACP execution; align both with the explicit override or discovered installed CLI and retest, without changing model ids or adapter versions. Follow-up verification covers additional locally installed agents in independent in-memory sessions, first via bidirectional routes with a verified provider, then via a direct chain among available agents; unavailable accounts remain explicit gaps. Repeated real switching exposed ACP wrapper descendants surviving direct-child termination. Add a failing process/stdio regression, reuse the existing Unix process-group signalling helper at crate scope, and isolate ACP process groups so termination and drop stop their ordinary descendants without affecting unrelated processes. Recheck plugin teardown, Core regressions, and the full live directed-pair route. Windows process trees remain outside the Unix guarantee.

CI follow-up: Linux reused an inode and Git admin path after worktree removal, exposing a false ownership match in the existing worktree regression. Include filesystem birth time when available in the existing identity receipt, retain legacy comparison for old receipts, and test recycled identity, content changes and serialization. Do not weaken checkout rejection or skip the regression.

Temporary resources: task-owned `.codex/run/orchestration-stability/` for preview evidence; this fresh worktree created `target/`, `apps/desktop/node_modules/` and `apps/desktop/dist/`. Remove dedicated compiled outputs after checks; retain installed dependencies until this worktree is retired. Stop the task's renderer process and remove disposable evidence at handoff. No user desktop restart.

Rollback: revert only this change's scoped edits; there is no migration.

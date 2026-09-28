---
id: 2026-09-28-remove-plan-goal
schema: 5
stage: verification
status: passed
owner: codex
created: 2026-09-28
based_on: plan.md
revision: "Worktree based on 6ed3561f plus prior orchestration stability changes and this removal"
verification_mode: owner
verified_by: codex
verified_at: 2026-09-28
release_target: none
cleanup_status: complete
---

# Verification: Remove Plan Goal

## Verification

- AC-1: PASS — Actual Vite-rendered workspace inspected through the in-app browser at 1280 × 720. Expanded session settings retain permissions/memory; the environment panel retains project/Git controls. Neither exposes Plan/Goal. Screenshot: `.codex/run/remove-plan-goal/workspace.png`. Composer, environment plan panel, scene Plan-first control, draft state, translations and plan-document actions are removed.
- AC-2: PASS — `cargo test -p codetwo-core --lib --test engine_activity --test scene_conformance --quiet` passed 552 unit, 10 activity and four scene-conformance tests. Regressions cover ignored Plan notifications/Goal metadata, removed planning selectors, rejected direct planning config, supported config changes and ordinary lifecycle behavior. The native Goal command and builtin Plan-first fragment are removed. `cargo test -p codetwo-server --lib t3_compat --quiet` passed 13 tests; `cargo test -p codetwo-server --test t3_mobile_compat --quiet` passed two HTTP/WebSocket integration tests, including rejected Plan mode, no prompt injection, long ordinary messages, busy-turn rejection and restart recovery.
- AC-3: PASS — Historical plan decoding remains read-only; plans are omitted from renderer turns, context handoff, memory answer extraction and T3 activity projection. Legacy draft and scene keys are ignored and not re-exported; old mobile interactionModes metadata no longer activates a mode. `bun test tests/retiredSessionFeatures.test.ts tests/composerDrafts.test.ts tests/scene.test.ts tests/reasoningScaleRendered.test.tsx tests/sceneChip.test.tsx tests/checkoutPickerRendered.test.tsx tests/sessionState.test.ts` passed 86 tests / 283 expectations. Scene editor tests passed three tests / 21 expectations; the final scene chip, studio and issue-delegation tests passed 22 tests / 83 expectations. Core stored-history/scene and mobile restart regressions also passed. No persisted user data was deleted.
- AC-4: PASS — `cargo check --workspace --all-targets`, `bun run build:renderer` (lint, TypeScript, Vite), final `bun run lint`, `bun script/verify/docs.ts`, `bun script/verify/sdlc.ts --worktree`, and `git diff --check` passed. Earlier in this change, the Core/library/provider-switch/model/activity/store/process suite passed 578 tests, with the opt-in real-provider canary ignored. Source inspection covered remaining active adapters, scene schema/examples and the legacy desktop host. Cleanup completed below.

Verdict: verified.
Residual risk: UI evidence is the browser renderer, not a native desktop/account run. Its browser-only bridge reports an empty provider registry; it does not prove live provider discovery. This removal did not rerun the earlier real-provider matrix, remote CI, packaging or release acceptance. Vite retains its existing large-chunk warning. Old planning configuration is accepted only for legacy data reading; new control requests fail explicitly.

Evidence: logs and screenshot retained under `.codex/run/remove-plan-goal/`. An unused mobile update helper exposed by compiler warnings was removed; the final workspace check is clean. Repository-wide Rust formatting has pre-existing drift; changed Rust hunks were formatted without rewriting unrelated code.

## PR validation follow-up

PR #242's first CI run failed two desktop tests that still expected retired Plan output (`environmentPopoverRendered` and `transcript`). Updated those expectations to assert absence of the plan panel/property while preserving ordinary environment controls and legacy prompts. No runtime behavior changed in this follow-up. Full `bun run test:ci` then passed 966 tests / 5,776 expectations; three native/profile opt-in tests were skipped by the suite. Evidence: `.codex/run/remove-plan-goal/full-desktop-tests.log`. The initial remote failure is [CI run 36383859967](https://github.com/IchenDEV/codeTwo/actions/runs/36383859967); replacement remote CI remains required before merge.

## Cleanup

Removed: task-owned `target/` (3.7 GiB), `apps/desktop/dist/` (48 MiB), superseded type/test logs.
Retained: compact `.codex/run/remove-plan-goal/` evidence, reused `apps/desktop/node_modules`, prior `.codex/run/orchestration-stability/` evidence and all source changes.
Retention owner: Codex for these verification artifacts; prior stability evidence retains its existing owner.
Cleanup trigger: review completion or worktree retirement; reused dependencies at worktree retirement.
Processes: own Vite preview stopped through its launcher; TCP 1437 released and browser test tab closed. Test processes exited. No user desktop or account process was stopped.
Evidence: `.codex/run/remove-plan-goal/cleanup.txt`; exact-path `du -sh` before removal, path/ownership checks, post-removal existence checks and `lsof -nP -iTCP:1437 -sTCP:LISTEN`.

## Review and release

Approval: local implementation authorized in Intent. The user explicitly authorized PR creation and merge with "pr & merge" on 2026-09-28. Human review and remote checks are separate facts.
Rollback: revert only this feature-removal diff; no data migration.
Release: PR delivery and merge authorized; remote CI and merge outcome pending. No versioned release or production deployment requested.
Feedback: link a concrete failure to its regression if follow-up runtime evidence changes this result.

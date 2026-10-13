---
id: 2026-10-13-shared-agent-runtime
schema: 5
stage: plan
status: accepted
owner: research-shared-agent r5 executor
created: 2026-10-13
based_on: spec.md
scope: crates/server/src/t3_compat.rs, docs/sdlc/changes/2026-10-13-shared-agent-runtime/
---

# Plan: Shared Agent Runtime

## Plan

Increment r5: (1) trace the ordinary UI path; (2) withdraw the wrong-surface `t3_compat.rs` candidate (the only source path, now equal to HEAD; the path stays in scope because the reversal is part of this diff); (3) try to prepare the pinned Ghostty source once; (4) keep a draft ordinary-path test and a persistence proposal outside the runnable tree; (5) close the records.

Edited: `crates/server/src/t3_compat.rs` (reverted), this change directory. Read only: `apps/desktop/src/coreTransport.ts`, `crates/server/src/lib.rs`, `vendor/libghostty-vt-sys/build.rs`. Not touched: `crates/core`, frontend, skills, checkers, catalog.

Checks: `git diff --stat`/sha for `t3_compat.rs`; one `curl` of the pinned Ghostty commit archive; then `bun script/verify/sdlc.ts --worktree`, `bun script/verify/docs.ts`, `git diff --check`. No Rust compile is possible without the Ghostty source. No UI change, no rendering.

Scratch and processes: ignored private local task scratch only (not published) (`scratch/` empty after the failed download). No child processes, ports or services.

Rollback: there is no source change to roll back. `crates/server/src/t3_compat.rs` equals HEAD (the wrong-surface candidate is already withdrawn). Do NOT re-apply the archived R2/R4 patches: they target the wrong surface and are kept only as private history. To discard this change, delete this change directory. Nothing is committed or pushed.

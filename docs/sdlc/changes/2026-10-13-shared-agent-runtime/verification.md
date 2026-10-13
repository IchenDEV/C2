---
id: 2026-10-13-shared-agent-runtime
schema: 5
stage: verification
status: blocked
owner: research-shared-agent r5 executor
created: 2026-10-13
based_on: plan.md
revision: worktree on fb3cb82964acd3643ccd81f35f73ec0ab0489863 plus other workers' dirty edits; t3_compat.rs sha256 466073e7f1d818e9715271b1d6a6202e52d5a54ba49edf9bdf6d8936538408c7 (equals HEAD)
verification_mode: owner
verified_by: r5 executor
verified_at: 2026-10-13
release_target: none
cleanup_status: complete
next_trigger: Provide a complete Ghostty source at commit a887df42c56f6de86c0fe6da9c4eeca37931e083 (GHOSTTY_SOURCE_DIR) or network access to fetch it, then wire and run the draft ordinary-path test.
---

# Verification: Shared Agent Runtime

## Verification

- AC-1: PASS — static trace by reading `apps/desktop/src/coreTransport.ts` (`/api/web-ui/call`, `/api/pair`, `/api/ws-ticket`, `/ws?ticket=`) and `crates/server/src/lib.rs` (`web_ui_call`, `require_device`, `KernelWebUiCommands::call` with unused `_device_id`, `ws_handler`); T3 routes (`wsTicket`, `/api/orchestration/dispatch`) are served only by `t3_compat`. Reading, not a runtime test.
- AC-2: PASS — `git apply -R --check` then `git apply -R` of the live diff (sha 6c9f54b7… equals the R4 patch); afterwards `git diff --stat -- crates/server` is empty and `t3_compat.rs` sha256 is 466073e7….
- AC-3: BLOCKED — `curl -L https://github.com/ghostty-org/ghostty/archive/a887df42….tar.gz` exited 6 "Could not resolve host: github.com" (a resolver/connectivity failure in that runtime; not a quoted permission rejection; raw output kept in private local logs); no retry, no escalation, no clone. `zig` 0.15.2 exists at /opt/homebrew/bin/zig; `libghostty-vt-sys/build.rs` needs `GHOSTTY_SOURCE_DIR` with `build.zig`. The R4 `libsqlite3-sys` bindings-copy error and the cargo target outside the workspace are not retried (use `--target-dir` inside the scratch next time). No cargo command ran in r5, so no compile or test result exists.
- AC-4: PASS — The draft ordinary-path test (kept in private local scratch, not published, outside the runnable tree) is an uncompiled, unrun draft (HTTP bearer -> `WebUiCommandCaller`, spoofed body/header, rejected bearer). Persistence proposal in Spec is unapplied. Neither counts as tested.

SDLC and docs gates: final-diff exit codes are summarized in [../2026-10-07-shared-agent-research/evidence/handoff.md](../2026-10-07-shared-agent-research/evidence/handoff.md).

Verdict: blocked (NOT_IMPLEMENTED). Ordinary UI actor propagation (the ordinary route `/api/web-ui/call` -> `KernelWebUiCommands` drops `device_id`), per-actor receipts, private visibility, nicknames and claiming are NOT implemented.
Residual risk: no executed server tests; `KernelWebUiCommands` still drops the device id; inner SQLite receipt keys unchanged; draft test may need adjustment when compiled (device-sync routes must be enabled for pairing).

## Cleanup

Removed: the empty failed download file in the private local scratch.
Retained: private local task directories (archived patches, logs, draft test), not published.
Retention owner: parent coordinator.
Cleanup trigger: after the next increment is accepted or this change is abandoned.
Processes: none started in r5 (only one short `curl`, exited); no cargo, Core, service or preview.
Evidence: the private scratch directory was empty after removal; `git status --short crates/server` is empty.

## Review and release

Approval: pending.
Rollback: See plan.md.
Release: No release requested; merge and external actions require their own authorization.
Feedback: Link an Incident and regression Eval when a real failure occurs.

Not done: no server or two-browser test ran, the T3/mobile patch is withdrawn, and compilation is blocked by the missing exact Ghostty source plus a download DNS failure. No high-risk approval or pass is claimed.

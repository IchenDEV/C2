---
id: 2026-10-06-external-mcp-pr-integration
schema: 5
stage: verification
status: passed
owner: codex
created: 2026-10-06
based_on: plan.md
revision: "Isolated assembled worktree on origin/main 40a2c7c48fc51cbe373a89c5073be7234a931207; excludes unrelated worktree lanes"
verification_mode: fresh-context
verified_by: "Cursor Sonnet 5.5 scoped prerequisite reviewer; retained independent core/events/transport reviewers"
verified_at: 2026-10-07
release_target: none
cleanup_status: complete
---

# Verification: External MCP PR Integration

## Verification

- AC-1: PASS — `git diff --name-only` and `git ls-files --others --exclude-standard` confirm no remote-environments UI, src-host, existing desktop tests, T3 client/protocol V2, sidecar or standalone scheduler changes. Independent scope-review-r1 identified the exact host/event prerequisites; its exclusions and manual hunk adjustments were applied. The integration Plan owns this exact subset, so the predecessor's uncompleted full-program verification is not copied or asserted passed.
- AC-2: PASS — `cargo check --workspace --all-targets --locked`; `cargo test --locked -p codetwo-core --lib -- external_mcp host_mcp` (72); eight selected core integration targets (96); `cargo test --locked -p codetwo-server --lib` (49); eleven selected server integration targets (79), including legacy T3, WebSocket, web UI, artifact and canvas authentication regressions. Desktop `bun run check`, `bunx tsc --noEmit`, `bun run test:ci` (1043 PASS, 3 skip), `bunx vite build`; `bun script/verify/docs.ts`, `bun script/verify/sdlc.ts --worktree`, and lifecycle Eval 32 PASS. The Quick fake helper's fixed 300ms delay failed once under load; bounded observable-state waiting and shutdown-before-assert now pass all 23 remote tests.
- AC-3: PASS — `PR_BODY` from the linked-record PR body with `PR_IS_DRAFT=false` and `PR_BASE_SHA=40a2c7c48fc51cbe373a89c5073be7234a931207 ./script/devflow check-pr` validates Ready preflight. This is local gate evidence; GitHub CI and merge are still pending and must be independently checked before merge.

Verdict: verified.
Residual risk: real tunnels, provider-backed external writes and native desktop credential end-to-end are unverified. Retained actual browser component interaction/layout evidence applies to unchanged external settings behavior. Independent scope review ran no tests; root ran the commands above. Host MCP sliding TTL/sidecar credential visibility and process-local subagent storage remain documented prerequisites. Windows compilation is not proven. No complete local workspace test success is claimed; CI runs it on Linux.

## Cleanup

Removed: the abandoned task-owned Ghostty network clone processes were stopped before restarting with a private copy of the existing pinned source; no inherited cache was changed. No user data or unrelated workers' resources were disposed.
Retained: /tmp/c2-pr-delivery (cleanable isolated Git worktree after delivery), /tmp/c2-target-pr-delivery (approximately 4.5 GiB Cargo/source output), /tmp/c2-pr-*.log and /tmp/c2-pr-body.md (task-only checks/body). The desktop node_modules symlink points to existing dependencies and its target must not be deleted.
Retention owner: codex root of this thread.
Cleanup trigger: on the next CI completion/failure turn, resolve fixes and merge or blockage, then remove Cargo output/logs/body and remove the isolated worktree only after safe push and clean Git status; review retention no later than 2026-10-07. No cleanup automation was created.
Processes: test and build commands are terminal; no real desktop, T3 or tunnel was started. The scoped read-only child is terminal. No task-owned lint, test or build process remains after follow-up checks.
Evidence: `du -sh /tmp/c2-target-pr-delivery /tmp/c2-pr-delivery`; `ps -axo pid,ppid,command`; exact Git diff/untracked inventory. Original worktree files and user desktop tests were never written by this PR assembly.

## Review and release

Approval: user explicitly requested `pr & merge` in the current thread; merge only after current-head CI passes and conflicts are absent.
Rollback: revert the PR merge; preserve the original dirty worktree.
Release: no versioned release or real remote activation requested. GitHub CI and merge evidence will be linked after they occur.
Feedback: no Incident created for the fixed test scheduling race.

### CI cancellation follow-up

GitHub run 37484943983 on f6a5a9fa reached the 30-minute job limit. Repository/docs/Ready gate, desktop lint/types/tests/build and Rust workspace check all passed. Rust workspace tests stalled in engine_provider_switch (four tests over 60 seconds) from 15:18 until cancellation at 15:39 UTC. This is not a passing CI run or a merge authorization waiver. Supervised Cursor task pr246-provider-switch-hang-r1 exclusively owns engine.rs in the isolated checkout; root owns diagnosis/checks and records. Investigate and fix the underlying lock/wait before rerun; do not skip tests or increase timeout to hide it.

### Provider-switch deadlock repair and acceptance

The root reproduced `concurrent_switches_have_one_owner_and_one_durable_result` in an owned subprocess/process group under a 45-second deadline; it hung and exited 124 after owned group termination. The host credential revoke helper re-entered the same std sessions Mutex held by the provider-switch commit. The implementation now uses a registry-only revoke in that already-locked path, immediately before replacing the old runtime; other call paths retain runtime bearer clearing. No outcome classification, retry or provider fallback behavior changed.

Independent pr246-provider-switch-review-r2 returned static PASS for credential invalidation, provider identity, lock ordering and one-writer/no-replay invariants, conditional on root runtime checks. The root ran `cargo test --locked -p codetwo-core --test engine_provider_switch`: 9 PASS, 1 ignored real-provider test. The concurrent switch regression now also proves the old bearer is valid before and revoked after switching. The old fail-once process fixture failed after the specified Unknown eviction; its marker now survives its process replacement and recognizes the target's own cursor while still refusing a foreign source cursor. The regression asserts Broken before terminal failure and successful explicit fresh-user retry with retained handoff context; no automatic replay was added. Two fixture failures are superseded by this final PASS.

Root `cargo test --locked -p codetwo-core --lib engine` passed 67; seven selected host/native/external/prompt integration targets passed 94. Both delegated tasks are terminal and ran no tests. Applicable tests and local verification pass; the prior cancelled CI is not accepted as passing. GitHub CI for the new pushed head remains a separate merge prerequisite. The package's passed state denotes local verification under the workflow, not remote CI success.

Cleanup follow-up: the bounded reproduction process group was terminated/reaped and final tests exited. `ps -axo pid,ppid,command` found no matching task test/fixture child; no user process was touched. New `/tmp/c2-pr-ci-cancelled.log`, switch/Engine/regression logs are retained under the existing root ownership and next-CI checkpoint. The independent review is terminal. No retry job or fee change was created.

### CI turn-state fixture repair and acceptance

GitHub run 37490537058 on 1941c36f passed engine_provider_switch (9 PASS, 1 ignored) and then failed provider_prompt_failure_keeps_the_prompt_correlation in engine_turn_state. Its no-Store fixture queried list_sessions after Unknown had correctly evicted the runtime. The test now observes the already-published SessionActivityChanged failure snapshot and additionally asserts Broken disposition and runtime removal. Existing request correlation, transcript absence, revision and provider-error assertions remain. No production behavior, timeout or skip changed.

Root ran `cargo test --locked -p codetwo-core --test engine_turn_state`: 5 PASS. Root then ran all 45 Core integration targets in one bounded Cargo invocation: all targets PASS (the log is /tmp/c2-pr-core-all-integrations.log). This broader local evidence does not claim full-workspace CI success. Documentation, SDLC and Ready checks are rerun before push; current-head GitHub CI remains the merge gate.

Cleanup checkpoint 2026-10-07: the bounded integration command exited normally and its process group was reaped. Task-only delivery worktree, build output and logs remain owned by root for the next CI completion; dispose after merge or an explicit final blockage handoff. No user service, data directory or unrelated cache was changed.

### CI runtime dependency repair

GitHub run 37492881180 on 1ef714e3 passed provider-switch and turn-state tests, then failed workspace_reads_require_registered_projects_and_search_treats_query_as_data. Workspace search invokes the existing ripgrep binary; the workflow did not install it and the published Ubuntu 24.04 runner tool manifest does not list ripgrep. Root reproduced the identical sanitized Internal error with the existing test executable and an empty PATH (exit 101). This is an environment prerequisite failure, not a waived assertion.

The Rust CI path now installs ripgrep with apt and prints its version before workspace checks/tests. The headless contract documents the server PATH dependency. No production implementation or test assertion changed. The same external MCP target passes with ripgrep available (17 PASS). Documentation and worktree SDLC checks PASS; Ready gate is rerun after the scoped commit. Linux apt installation and full-workspace success remain for the new CI run to prove. Retained /tmp/c2-pr-ci-r3.log, /tmp/c2-pr-runner-tools.md and /tmp/c2-pr-missing-rg.log have the existing root owner and next-CI cleanup checkpoint.

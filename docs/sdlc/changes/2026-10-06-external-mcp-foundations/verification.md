---
id: 2026-10-06-external-mcp-foundations
schema: 5
stage: verification
status: passed
owner: codex
created: 2026-10-06
based_on: plan.md
revision: "Worktree on t3code/6738e658 at 255ef578 plus uncommitted changes; no commit made"
verification_mode: owner
verified_by: codex
verified_at: 2026-10-06
release_target: none
cleanup_status: complete
---

# Verification: External Mcp Foundations

## Verification

- AC-1: PASS — `cargo test -p codetwo-core --lib -- external_mcp host_mcp` (40 passed incl. the generic-handler tests) and `cargo test -p codetwo-core --test host_mcp` (4 passed) with `CARGO_TARGET_DIR=/tmp/c2-target-check`.
- AC-2: PASS — same command; `external_mcp::clients` tests cover expiry, revoke, unknown and wrong-prefix tokens, group-readable file refusal, TTL above 90 days, the 65th client, Approve without the allow flag, path escape, rate limit and Debug output.
- AC-3: PASS — `external_mcp::catalog` `validate_catalog` tests in the same run.
- AC-4: PASS — `external_mcp::audit` tests in the same run.
- AC-5: PASS — `external_mcp::events` tests (15) in the same run: eviction, stale and foreign cursor reset, project isolation, content-policy leak test, bounded wait.

Verdict: verified.
Residual risk: pure library code only; no Engine gate, HTTP route, or integration exists yet, so none of the security properties are proven end to end. `git_*`, `workspace_file_read` and `events_poll` use domain-external verbs allowed by a whitelist in `validate_catalog`; `approval.resolved`, `question.resolved`, `queue.drained` and `automation.run_finished` have no source `Event` yet.

## Cleanup

Removed: the partial fresh target `/tmp/c2-target-main` (failed offline Ghostty fetch).
Retained: none from this foundations task. At parent handoff, `/tmp/c2-target-check` and `/tmp/c2-research/` were removed after active checks terminated; findings are retained in the parent Spec.
Processes: no task-owned process is running; no desktop, T3 or data-directory process was touched.
Evidence: original `ls -d /tmp/c2-target-*`; parent cleanup session 46032 confirmed exact-path removal of both inherited foundations resources.

## Review and release

Approval: local implementation authorized in Intent; no human review yet.
Rollback: See plan.md.
Release: No release requested; merge and external actions require their own authorization.
Feedback: Link an Incident and regression Eval when a real failure occurs.

---
id: 2026-10-04-builtin-chief-of-staff
schema: 5
stage: verification
status: passed
owner: codex
created: 2026-10-04
based_on: plan.md
revision: "14c20fce plus the scoped chief-of-staff worktree changes"
verification_mode: pair
verified_by: assistant_independent_review
verified_at: 2026-10-05
release_target: none
cleanup_status: complete
next_trigger: Human review of the local implementation; no release requested.
---

# Verification: Builtin Chief Of Staff

## Verification

- AC-1: PASS — `cargo test -p codetwo-core plugins::app::plugins::assistant::tests --lib` links an existing real Engine session in project A and dispatches only project B through mock ACP. Both goals finish with concrete files. Actual Codex also dispatched two isolated projects; see [real-provider.json](evidence/real-provider.json).
- AC-2: PASS — Real Codex workers first returned write blockers; idle did not complete either goal. After the dispatcher workspace fix, the real coordinator issued one corrective assignment per project. Both workers returned `report.txt`, byte checks and SHA-256; the coordinator accepted both. Parent independently reread both 23-byte files and recomputed hashes. Actual artifact mutation revoked acceptance. [Provider evidence](evidence/real-provider.json).
- AC-3: PASS — Runtime tests exercise real Engine creation receipts, replay, unknown prompt outcomes and duplicate reconciliation. A separate Engine instance recovers a created session before its first prompt and restores effort. Store reopen preserves goals and memory. Actual isolated Core restarts retained two goals, assignments and corrected memory without replaying the completed worker prompts. [Command evidence](evidence/checks.txt).
- AC-4: PASS — Core tests cover scope, stale revisions, atomic rejection, concurrency, interrupted work, takeover/resume, uncreated-intent takeover, current-assignment acceptance, artifact escape/FIFO and memory policies. Runtime tests reject an unavailable selected model and unsupported effort before provider work, including recovery. Independent reviewer also executed historical-assignment, removed-project-concurrency and version-change probes. [Command evidence](evidence/checks.txt).
- AC-5: PASS — T3 collaborative preview rendered the actual React UI backed by isolated Core in light/dark, 1280-wide and 390×844 states. Sidebar entry and keyboard navigation worked; narrow page had no horizontal overflow. Memory add/correct/forget worked, and corrected text survived restart. Execution links and takeover have rendered interaction regression coverage. [Light memory](evidence/memory-light.png), [narrow memory](evidence/memory-narrow.png), [project invalidation](evidence/projects-dark.png).
- AC-6: PASS — Disabled follow-up retained goals/memory and did not interrupt already running fixture workers. Re-enabling reconciled terminal results. Existing Engine/store/memory/activity/model tests, automation tests and T3 create/prompt replay integration passed. Module lifetime belongs to Core; no offline/cloud execution claim. [Command evidence](evidence/checks.txt).

Verdict: verified.
Residual risk: Real model behavior was exercised with the available Codex default (`gpt-6.1-sol`); other providers use existing permission/configuration paths and have not received this end-to-end trial. The manager's read-only policy is provider-enforced, not a new OS sandbox. Limits count reviews/assignments rather than money. Native Electrobun packaging, remote CI, merge and release were not performed. The shared React renderer was tested through authenticated Web Core.

## Checks and independent review

Consolidated actual command output: [checks.txt](evidence/checks.txt).

Environment: macOS arm64, Rust 1.95, Bun, task-owned Core port 14674 and Vite port 14673. User data directory and running user apps were not used.

| Actual command | Result |
| --- | --- |
| `cargo test -p codetwo-core --test assistant` | 7 passed; independently repeated |
| `cargo test -p codetwo-core plugins::app::plugins::assistant::tests --lib` | 5 passed; independently repeated |
| `cargo test -p codetwo-core --test engine_store --test engine_memory --test engine_activity --test engine_builtin_models` | 20 passed |
| `cargo test -p codetwo-core automation --lib` | 8 passed |
| `cargo test -p codetwo-server browser_renderer_has_one_bounded_core_capability_set` | Paired renderer capability test passed |
| `cargo test -p codetwo-server --test t3_mobile_compat native_mobile_creates_a_thread_and_dispatches_a_prompt` | 1 passed, including existing durable receipt replay |
| `cargo check --workspace` | Passed Core, server and desktop host |
| `cargo build -p codetwo-server --bin codetwo-server` | Passed, exercised final production logic in isolated Core |
| Desktop `bun test tests/assistant.test.tsx tests/missionControlRendered.test.tsx tests/missionControl.test.ts tests/sessionRailRendered.test.tsx` | 46 passed, 341 expectations |
| Desktop `bun run build:renderer` | Lint, TypeScript and production Vite build passed; existing large-chunk advisory remains |
| Changed Rust files: `rustfmt --edition 2021 --check --config skip_children=true,reorder_modules=false …` | Passed |
| `bun script/verify/docs.ts`, `bun script/verify/sdlc.ts --worktree`, `git diff --check` | Passed before cleanup; repeated at handoff |

`cargo fmt --all -- --check` reports pre-existing formatting drift in unrelated host and Core files. No broad reformat was applied. An initial `--no-default-features` probe hit existing unguarded terminal references; the supported default-feature workspace build passed.

The read-only independent reviewer found and verified fixes for old-assignment acceptance, takeover recovery, deselected-project concurrency, recovered memory policy, artifact hashing/FIFO, selected model/effort rejection and effort restoration. Rendered/real-provider testing also fixed source-checkout paths, streamed evidence truncation and stale post-acceptance status text. Final independent result: no remaining blocking findings, 7 Core plus 5 Runtime tests passed.

## Preparation evidence

Inspected clean baseline `14c20fce`, recent `7c099fd1`, existing Store/Engine/memory/automation and legacy Orchestrator callers. Read the attached istack thread with pagination. Gist and pstack were read through GitHub API/raw content; the X reference used a public text mirror after X returned 403. No browser access to GitHub was used. The product keeps one Engine execution path and one Memory Store.

## Follow-up experience test (2026-10-05)

User request: “测试一下 体验一下”. The scoped follow-up changed only the goal editor and its UI regressions; the earlier independent Core/runtime review remains applicable.

- Reproduced project misrouting: select A in the new-goal form, then filter the workspace to B. Before the fix the form retained A. The visible workspace filter now owns the target; its project selector is locked while filtered.
- Reproduced duplicate-entry friction: a successful save retained the previous draft. The new-goal title and acceptance now clear only after success. A failed revision check retains both fields and displays the error.
- Actual browser flow with two registered fixture projects: configure scope without enabling automation; create a B goal after selecting A; verify the Core record belongs to B; pause/resume; change priority to P0 and acceptance; add/correct project B memory; verify A is empty; add global memory and forget a temporary note.
- Restarted only the task-owned Core. The full assistant state and memory snapshot were byte-equivalent JSON before/after restart. The fresh browser tab displayed the retained goal and global memory. At 390×844, both pages measured 390px scroll width with no horizontal overflow. [Live state and rendered text](evidence/experience.json).
- `bun test tests/assistant.test.tsx tests/missionControlRendered.test.tsx tests/missionControl.test.ts tests/sessionRailRendered.test.tsx`: 48 passed, 354 expectations. `bun run build:renderer`: lint, TypeScript and production build passed. [Follow-up command output](evidence/experience-checks.txt).
- Screenshot capture failed repeatedly with the T3 preview client’s `PreviewAutomationExecutionError`, including a fresh tab. Click/type/evaluate and viewport resizing worked. This round supplies live interaction/state/layout evidence, not new visual screenshots. The earlier linked light/dark screenshots remain historical evidence. No new provider dispatch was needed for these form fixes; follow-up remained disabled and dispatch count stayed zero.

## Cleanup

Removed: task-owned `target/` (3.8 GB), `apps/desktop/dist/` (48 MB), `.codex/run/builtin-chief-of-staff/` (3.8 MB), three `/tmp/codetwo-assistant-*.log` files, two fixture repositories and their isolated worktrees. The aborted external fixture worktree `1c6373f4-de16-4570-9e90-9f59444faa8b` and its branch were removed after confirming clean Git state. No source edits or user data were deleted.
Retained: local source changes and small evidence files under this change record; normal shared dependencies and provider/browser-owned conversation artifacts.
Retention owner: repository owner for deliverables; package/provider owners for shared caches and history.
Cleanup trigger: normal repository or provider retention policy; these are not active temporary runtime resources.
Processes: task-owned Core and Vite stopped through their exact process IDs; provider children had exited; ports 14673/14674 released; task browser tab closed. No user process was replaced.
Evidence: exact candidate inventory and `du -sh` before deletion; bounded Python path checks and existence assertions after deletion; `lsof -nP -iTCP:14673 -iTCP:14674 -sTCP:LISTEN`, `ps -axo pid,ppid,command`, Git worktree/branch absence, `git status --short` and final documentation/scope checks. Independent reviewer also removed all of its own fixtures.

Follow-up cleanup: stopped exact task-owned Vite PID 44950 and Core PID 58346; earlier bootstrap Core PIDs 50418 and 51651 had already exited. Closed preview tabs 2 and 3. Removed `.codex/run/assistant-experience/` including build, data and both fixture repositories, plus the newly generated `apps/desktop/dist/`. Retained only source/tests and linked experience evidence. No provider sessions were started in this round. Rechecked listening ports and final worktree/documentation gates.

## Coordination audit clarification (2026-10-05)

The earlier PASS results remain evidence for the named first-release tests. At the design audit, the initial four-action protocol did not implement the broader coordination experience end to end. Those historical tests do not prove the expanded scope. The subsequent user-authorized implementation replaces that protocol with versioned messages, routed questions, worker tools, review and durable notifications; see the [coordination verification](../2026-10-04-assistant-coordination-loop/verification.md) for current evidence and remaining platform limits.

## Review and release

Approval: User implementation/design approval recorded in Intent and Spec; independent technical verification complete. No PR/merge/release authorization requested or inferred.
Rollback: Disable follow-up; the additive state table remains inert on code rollback and preserves data. Existing execution sessions remain available.
Release: none. Source changes are local and uncommitted.
Feedback: Real-provider boundary failures became workspace-path, strict model/effort and streamed-evidence regressions in this change.

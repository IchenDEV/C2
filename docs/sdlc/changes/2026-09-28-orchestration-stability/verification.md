---
id: 2026-09-28-orchestration-stability
schema: 5
stage: verification
status: passed
owner: codex
created: 2026-09-28
based_on: plan.md
revision: "Worktree based on 6ed3561f, including this change's scoped implementation and regressions"
verification_mode: owner
verified_by: codex
verified_at: 2026-09-28
release_target: none
cleanup_status: complete
---

# Verification: Orchestration Stability

## Verification

- AC-1: PASS — `cargo test -p codetwo-core --lib` passed 550 tests. Context regressions cover full short requests, separate consecutive user prompts, oversized Unicode output, hundreds of small records, initial/latest user anchors, record/content bounds and private-state exclusion. `cargo test -p codetwo-core --test engine_provider_switch` passed nine cases, including four rounds of A/B/A/B switching without intermediate prompts or nested continuation.
- AC-2: PASS — `cargo test -p codetwo-core --lib --test engine_provider_switch --test engine_builtin_models --test acp_models --test engine_permission --test engine_store --test acp_process_cleanup --quiet` passed 574 tests after the live-discovered runtime and process cleanup fixes. Cases include failed startup, busy/awaiting approval, concurrent switches, managed leases, missing durable history, first-prompt failure/retry, cancelled first prompt followed by engine restart/native restore, stale callback suppression, and dropping initialization with candidate-client cleanup. The ordinary suite excludes the opt-in authenticated canary; its separate live result is recorded below.
- AC-3: PASS — The same Core suite checks fresh catalogue queries, configured environment, failure recovery, paginated model discovery and cursor-cycle rejection, no fabricated catalogue, ACP model/effort choices, and runtime catalogue projection. `cargo test -p codetwo-server --lib t3_compat --quiet` passed 12 tests, including registration of an unknown custom integration without invented models. `rg -n 'builtin_models|FALLBACK_PROVIDERS|fallbackProviders' crates apps/desktop/src` returned no matches. Price-estimation tables and test/demo data are not model-selection catalogues and remain outside this change.
- AC-4: PASS — `bun test tests/reasoningScaleRendered.test.tsx tests/sceneChip.test.tsx tests/sessionState.test.ts tests/providerRegistry.test.ts` in `apps/desktop` passed 69 tests / 218 expectations. Actual Vite-rendered production ModelPicker components were inspected through the in-app browser in light mode and dark 390px mode: disabled switching status, empty discovery with Retry, and a custom discovered model menu. Browser errors were empty. The App mutation path also has a synchronous per-session request fence; focused/background panes receive their own switching state.
- AC-5: PASS — `cargo check --workspace --all-targets --quiet`, `bun run build:renderer` (lint, TypeScript, Vite), `bun script/verify/docs.ts`, `bun script/verify/sdlc.ts --worktree`, and `git diff --check` passed. Local environment: macOS, rustc 1.95.0, Bun 1.4.2. Existing full-tree `cargo fmt --all --check` reports unrelated baseline drift; changed Rust hunks were formatted without reformatting unrelated code. Vite retains its existing large-chunk advisory.

Verdict: verified.
Residual risk: Cursor live switching is unverified because its installed CLI returned Authentication required; no login or account configuration was changed. The additional provider matrix below separates successful routes from unavailable accounts/adapters. Full Electrobun integration, remote CI and release were not exercised. ACP metadata remains provider-owned; inaccurate adapter declarations must be fixed at their source. Long-history continuation is deliberately bounded to 48 Ki characters and 128 records, with omission markers; canonical stored history is retained. These tests do not claim lossless transfer of every historical detail or acceptance of every provider/account.

## Authenticated provider verification

PASS — `CODETWO_LIVE_SWITCH_PROVIDERS=codex,grok,codex,grok,codex CODETWO_LIVE_SWITCH_CONTEXT_CHARS=65536 CODETWO_LIVE_SWITCH_CWD="$PWD/.codex/run/orchestration-stability/live-cwd" cargo test -p codetwo-core --test engine_provider_switch live_providers_switch_in_place_and_back -- --ignored --nocapture` completed in 44.95 seconds. All five real turns recovered the generated continuity key, including four switches within the same durable Session. The initial synthetic history exceeds the 48 Ki-character transfer budget. Later prompts do not repeat the key. No model override was provided. Test data used an in-memory Store and a task-owned empty working directory; permission and elicitation requests are declined.

The first probe failed before switching: the ACP adapter bundled Codex 0.148.0 while model discovery found local Codex 0.157.1, so the configured model was rejected by the older runtime. The fix pins ACP and catalogue discovery to the same explicit or discovered installed executable, without changing the adapter version or selecting a fixed model. A subprocess-isolated regression verifies explicit runtime forwarding; the repeated live route confirms the installed-runtime fallback. The canary now resolves requested ids against the registry, requires an explicit provider sequence, supports bounded synthetic long context, and shuts down its Engine even on assertion failure.

A separate `grok,cursor,grok,cursor,grok` probe passed Grok's first long-context turn, then Cursor returned an authentication-required error. This is recorded as an unavailable live route, not a switching pass. Failed-run evidence is retained alongside the successful run; test-owned provider processes were shut down after each attempt.

## Additional agent coverage

The same opt-in canary was run with 65,536 synthetic context characters, runtime-owned default models, an in-memory Store and a separate temporary working directory per run. Executables were confirmed locally before running. No login, subscription or account configuration was changed.

| Route | Result | Evidence |
| --- | --- | --- |
| Claude Code → Grok → Claude Code | PASS, three recalled keys, 48.40 s | `live-matrix-claude_code.log` |
| OpenCode → Grok → OpenCode | PASS, three recalled keys, 21.48 s | `live-matrix-opencode.log` |
| Amp → Grok → Amp | PASS, three recalled keys, 43.64 s | `live-matrix-amp.log` |
| Kimi → Grok → Kimi | BLOCKED before switching: account returned HTTP 403, subscription has no Kimi Code access | `live-matrix-kimi.log` |
| ZCode (GLM) → Grok → ZCode | BLOCKED before switching: no API key environment or stored credential file was configured; adapter 1.12.0 returned RPC -32603 Internal error, so there is no live continuation result | `live-matrix-zcode.log` |
| Pi → Grok → Pi | BLOCKED before switching: API key/OAuth authentication required | `live-matrix-pi.log` |
| Droid → Grok → Droid | BLOCKED before switching: authentication required | `live-matrix-droid.log` |
| OpenCode 2 → Grok → OpenCode 2 | Initial OpenCode 2 and Grok turns passed; return failed with provider authentication required. A sequential retry failed with the same error on the first turn, before any switch. Full route remains unverified. | `live-matrix-opencode2.log`, `live-matrix-opencode2-retry.log` |

The direct-pair follow-up derives its candidate set from completed successful probes and constructs a route covering every directed pair once. This is a verification fixture, not a product provider/model catalogue.

PASS after the process cleanup fix — Amp, Claude Code, Codex, Grok and OpenCode completed every one of their 20 directed switching pairs in one durable Session, with 21 successful continuity-key responses and 65,536 initial synthetic context characters. The complete run took 198.48 seconds. The route was `amp,opencode,grok,opencode,codex,opencode,claude_code,opencode,amp,grok,codex,grok,claude_code,grok,amp,codex,claude_code,codex,amp,claude_code,amp`. No model override was provided. Evidence: `live-direct-pairs.log`, `live-direct-pairs-results.json`; the pre-fix route's successful context result is retained as `live-direct-pairs-before-cleanup-fix.log`. Intermediate post-fix process snapshots showed only current/candidate adapter groups, and the final snapshot showed no remaining test-owned provider processes.

## Provider process ownership regression

The first 20-direction real route completed all 21 turns, but intermediate process snapshots showed orphaned npx adapter descendants persisting while the Engine continued running. The new `acp_process_cleanup` regression reproduced this on the old implementation: killing only the wrapper left a descendant holding stdio and pending RPCs open.

ACP launches now create isolated Unix process groups and terminate the owned group on explicit shutdown or client drop. The existing plugin process-group signal helper moved to crate scope and is shared; plugin lifecycle semantics are unchanged. Arbitrary children supplied through the public client constructor retain direct-child ownership. The regression verifies explicit termination, repeated termination, drop, an already-exiting wrapper, closed pending RPCs and survival of an unrelated process. It failed before the fix and passed afterward (three lifecycle cases in one test). Evidence: `process-cleanup-before.log`, `process-cleanup-after.log`, the 574-test Core rerun and `live-process-samples.log`.

Unix process groups cover ordinary descendants; descendants that deliberately create a new process group and Windows process-tree ownership are not claimed. Windows retains direct-child cleanup.

## Cleanup

Removed: Task-created `target/` (2.8 GiB at the first handoff; 2.3 GiB rebuilt and removed after each authenticated verification continuation), the empty live-test working directory, per-probe temporary working directories, one-off matrix runner scripts, `apps/desktop/dist/` (48 MiB), and temporary browser harness HTML/TSX under `apps/desktop/.codex/run/orchestration-stability/`. The temporary browser tab was closed and its viewport override reset.
Retained: `.codex/run/orchestration-stability/` contains small test logs, successful/failed authenticated canary evidence, and light/dark rendered evidence; `apps/desktop/node_modules/` (1.7 GiB) contains installed development dependencies.
Retention owner: Codex for this change and checkout.
Cleanup trigger: Review retained evidence on the next continuation and remove disposable logs/captures after this change is reviewed or closed; remove the installed dependencies when this worktree is retired. Provider-managed synthetic session histories and shared package caches are left to their normal lifecycle; no account history is deleted by this task.
Processes: Successful/failed canary engines shut down their provider processes; process inspection after the final run showed no task-owned provider remaining. Task-owned Vite launcher stopped; port 1437 is released. No task-owned mock provider, test, build or desktop process remains. User processes were not stopped.
Evidence: `du -sh target apps/desktop/node_modules apps/desktop/dist .codex/run/orchestration-stability` before removal; exact-path inspection/removal and post-removal inventory; `ps -Ao pid,command` and `lsof -nP -iTCP:1437 -sTCP:LISTEN`. Retained logs are `core-tests.log`, `desktop-tests.log`, `workspace-check.log`, `switch-regression-tests.log`, `live-provider-tests.log`, `live-grok-cursor-tests.log`, and `live-codex-grok-tests.log`, `live-matrix-*.log`, matrix/direct-pair result JSON, `live-direct-pairs*.log`, `process-cleanup-before.log`, `process-cleanup-after.log`, `live-process-samples.log`, and `live-final-processes.log`; render captures are `light.png` and `dark-narrow.png` in the evidence directory.

## Review and release

Approval: local implementation authorized in Intent. The user explicitly authorized PR creation and merge with "pr & merge" on 2026-09-28. Human review and remote checks are separate facts.
Rollback: Revert this scoped change; no schema migration.
Release: PR delivery and merge authorized; remote CI and merge outcome pending. No versioned release or production deployment requested.
Feedback: Rerun the Cursor route after its account is authenticated. Use the opt-in canary for future provider regressions; keep model selection owned by runtime discovery.

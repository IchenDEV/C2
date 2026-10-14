---
id: 2026-10-07-shared-agent-research
schema: 5
stage: verification
status: blocked
next_trigger: "Re-run the 728/390 preview in a host that allows html_preview and save PNGs plus an input manifest; the real sharing runtime is a separate stage (codetwo-sharing-runtime, planned_not_implemented)"
owner: shared-agent-research-subagent
created: 2026-10-07
based_on: plan.md
revision: "base fb3cb82964acd3643ccd81f35f73ec0ab0489863 + uncommitted worktree; board 2026-10-08-shared-agent"
verification_mode: owner
verified_by: final-round UI executor (claude-sonnet-5-5, full-access); verdict corrected 2026-10-13
verified_at: "2026-10-07"
release_target: none
cleanup_status: complete
---

# Verification: Shared Agent Research

## Verification

- AC-1: PASS — Owner/device correction is written in Intent, Spec, Plan and the proposal; no claim/follow-up/`claimed_by` remains in the current plan. Command `bun script/verify/sdlc.ts --worktree` and artifacts [evidence/proposal.md](evidence/proposal.md) show four stage files and the evidence files filled. No template placeholders remain. The private-visibility assumption is written in Intent, Spec and the proposal.
- AC-2: PASS — Source facts carry real paths and lines in [evidence/source-evidence.md](evidence/source-evidence.md). It states that no real two-browser E2E ran and rejects the Gemini wording. Queue/steer and worktree facts in `engine.rs` / `worktree.rs` were not line-checked and stay unverified.
- AC-3: PARTIAL / blocked (corrected 2026-10-13; the text after the dash is the HISTORICAL claim from a simulation-only prototype, now unreproducible: no saved PNG and no input manifest exist) — Final `prototype.html` (11505 bytes, SHA-256 `12f3fc86c9c84f24bfbb626461108f4d8ac8f101f7cef628b5e688611996a8dc`) ran in native `html_preview` once at 728 and once at 390; PNGs viewed, no overflow, console only `PASS: simulation ...`, no warning/error. Owner/device/visibility icons are distinguishable and carry title/aria-label; summary click expand/collapse is in the self-check. Rules: new task default public; public cannot return to private; legacy private stays private until one-way publish, owner unchanged; everyone participates, stops/continues; messages signed. Details: [evidence/preview-acceptance.md](evidence/preview-acceptance.md). The old r5 pass and the old 728 failure are history only. Limit: no PNG file is stored; the HTML was hand-transcribed into the tool input.
- AC-4: PASS — Command `git status` / `git diff --check` and artifact [evidence/preview-acceptance.md](evidence/preview-acceptance.md): only files under the change directory were written. The main checkout was compared at start and end (see below).

Verdict: PARTIAL / blocked (corrected 2026-10-13). AC-1, AC-2 and AC-4 stand as documentation checks. AC-3 is not reproducibly verified: the preview PNGs and the input manifest were never saved, and later `html_preview` attempts were rejected by the host. The earlier "verified" verdict is historical and applies only to the in-memory mock, not to the product, two real clients or server-side filtering. See [evidence/handoff.md](evidence/handoff.md).

Prior evidence (kept): an earlier auto-mode host rejected `html_preview` with "Local SDK runs cannot request interactive approval for this MCP tool call". That run saw no screenshot and left AC-3 blocked. The older r2 task was interrupted and r3-auto completed with that rejection; both are stopped. The r4 full-access run replaced them.
Retry in the auto-mode thread (2026-10-10 11:08 UTC, cursor/claude-sonnet-5-5, runtimeMode=auto): one `html_preview` call (width 728) on the current 11505-byte `prototype.html` (SHA-256 `12f3fc86...a8dc`) was rejected again, raw result: "Tool rejected: User rejected MCP: t3-code-html_preview - Local SDK runs cannot request interactive approval for this MCP tool call. Keep the action within the configured sandbox policy or Smart Auto Review's auto-approval boundary, or re-run without sandboxing/autoReview enabled." On 2026-10-10 12:05 UTC the user authorized exactly one identical retry (same thread, same executor, same prototype at width 728, same tool and parameters). That second call returned the same raw text, verbatim: "Tool rejected: User rejected MCP: t3-code-html_preview - Local SDK runs cannot request interactive approval for this MCP tool call. Keep the action within the configured sandbox policy or Smart Auto Review's auto-approval boundary, or re-run without sandboxing/autoReview enabled." The prototype was still 11505 bytes with the same SHA-256 and HEAD was still `fb3cb82964acd3643ccd81f35f73ec0ab0489863`. No further attempt was made, so no 390 call was made, no screenshot was taken, and no self-check ran in that thread. This retry adds no new pass evidence. The AC-3 pass above rests only on the earlier full-access round in `evidence/preview-acceptance.md`, which still has no stored PNG and no manifest. A reviewer who wants stored screenshots must rerun the preview in a thread that the host lets call `html_preview`.

Residual risk: the self-check proves simulated event handling only. It does not prove a real Agent, two real clients, server-side private filtering, or the product. Undecided: whether private is visible to the machine owner too (kept as creator-only simulation, no RBAC invented), and who may publish a legacy private task (only a one-way publish is simulated; no two-way switch). Queue/steer/worktree remain unverified.

Checks run (exit codes in the final handoff message): `bun script/verify/sdlc.ts --worktree`, `bun script/verify/docs.ts`, `git diff --check`. Skipped: Rust tests, lifecycle Eval (no runtime or checker change), real two-client E2E (future slice).

Independent input check (reported by an independent checker, 18:47:38 UTC, not reproducible from saved files), parsed the 728/390 HTML from this round's original activity input objects and compared them with Python `read_bytes`/strict UTF-8; both equal the 11505-byte final file, and the three SHA-256 values are all `12f3fc86c9c84f24bfbb626461108f4d8ac8f101f7cef628b5e688611996a8dc`. Hand transcription into the tool input stays stated. The executor viewed the PNGs (728 contentHeight 583, 390 contentHeight 1005, console only PASS) and read its own child configuration; the checker read the parent and child configurations independently.

Next stage: real sharing is tracked separately as `codetwo-sharing-runtime=planned_not_implemented`. This round passed only the prototype; multi-user sharing is not done. No implementer or isolated workspace is assigned; the root dispatcher decides. Future acceptance needs two independent real clients on one Core (public-task participation, attribution, stop/continue sync), real owner and device ownership, one-way publish, and server-side list/read/command/event filtering. Open: machine-owner visibility of private tasks and who may publish; queue/steer/worktree unverified. Nothing of this stage was started.

Final prototype: `evidence/prototype.html`, 11505 bytes, SHA-256 `12f3fc86...a8dc` (full value above). Hashes of the other seven files are in the final handoff.
Config: this executor read its own `t3_thread_configuration`: cursor/claude-sonnet-5-5, contextWindow=300k, reasoning_effort=high, full-access, default. Quota, window and refresh are unknown to this thread. The old r5 executor did not read config and hand-transcribed its input; the old supervisor read configs and compared two inputs with the 7262-byte file. Old r5 timing: stage 17:02:04 with a 5 min limit to 17:07:04; notification queued 17:07:15 (11 s late); the old r5 durable run 17:01:46.991 to 17:07:51.706 exceeded 5 min (this is the old r5 only, not a total-round claim). `T3_ACP_MCP_NODE` is unset; ACP was not used. Board id: 2026-10-08-shared-agent.
Proves simulation only: not the product, two real clients, server-side private filtering, or real cross-device execution.

Run identifiers and notification failure are in [evidence/source-evidence.md](evidence/source-evidence.md). Actual mode read in this thread: cursor/claude-sonnet-5-5, contextWindow=300k, reasoning_effort=high, runtimeMode=full-access, interactionMode=default; tool transport is the native `t3-code` MCP. Skills used: codetwo-develop, ponytail full, explain-clearly. No further delegation.

## Cleanup

Removed: earlier, task-owned `.codex/run/shared-agent-research-20261008/` (its `prototype.html` was copied to evidence first). This round, three task-owned temporary check logs created by earlier verification commands, re-checked unchanged and containing only validation lines before deletion. No other temporary file was removed.
Retained: the change directory, its four stage files and `evidence/{source-evidence.md,proposal.md,prototype.html,preview-acceptance.md}`; branch `research/shared-agent-20261008`. Nothing is committed.
Retention owner: the main owner (reviewer) of this research branch.
Cleanup trigger: after review, remove the branch/worktree or commit the records.
Processes: none started; none stopped. No services, no Core, no real app. This round's final checks capture output in stdout only, with no new log file. The preview used T3's own headless browser through the tool.
Evidence: files above; read-only git state of this worktree and the main checkout (a separate branch at `f94b88b4`, clean).

## Review and release

Approval: pending.
Rollback: See plan.md.
Release: No release requested; merge and external actions require their own authorization.
Feedback: Link an Incident and regression Eval when a real failure occurs.

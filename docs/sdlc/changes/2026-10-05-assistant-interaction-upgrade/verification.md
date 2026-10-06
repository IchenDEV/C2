---
id: 2026-10-05-assistant-interaction-upgrade
schema: 5
stage: verification
status: passed
owner: codex
created: 2026-10-05
based_on: plan.md
revision: "Uncommitted renderer follow-up on 14c20fce841384dbeaa16c48c7a7264d6e4a9f81"
verification_mode: owner
verified_by: codex
verified_at: 2026-10-05
release_target: none
cleanup_status: complete
---

# Verification: Assistant Interaction Upgrade

## Verification

- AC-1: PASS — [62 desktop regressions](evidence/desktop-tests.txt) and the [real UI report](evidence/ui-result.json) show zero native details/summary/select and zero visible overview textareas. Single request entry, goal selection, answer/change selection and keyboard-capable shared controls work. Request submission records the selected project and shows confirmation.
- AC-2: PASS — Current task sections preserve composition when switching views. Existing tests still reject stale requirement/deliverable decisions and retain failed drafts; unrelated worker activity does not reject a valid edit. Message, takeover, dependency, review and control identifiers/actions use the existing API unchanged. Real UI recorded a supplement, then stopped A through its persistent toolbar; the durable stopped receipt and paused UI passed [the terminal check](evidence/ui-terminal-check.txt). B remained active. Queued supplements were cancelled by that stop.
- AC-3: PASS — [Desktop tests](evidence/desktop-tests.txt) cover single selected Inbox forms and unknown-answer replay lockout. Actual UI inspected the disabled unknown-answer action, advanced settings hidden/open, and memory composition hidden/open. These navigation checks changed neither model settings nor permission state.
- AC-4: PASS — [Lint](evidence/lint.txt), [TypeScript](evidence/types.txt), [desktop renderer build](evidence/renderer-build.txt), [Web renderer build](evidence/web-build.txt) and the [server build](evidence/server-build.txt) passed. Actual Web React was paired to one isolated Core with follow-up disabled. Light 1280×800 and dark 640×900 views were operated and visually inspected. The 640px workspace and document had 640px scroll width. [Final scope/documentation/lifecycle checks](evidence/final-checks.txt) pass; cleanup is recorded below.

Verdict: verified.
Residual risk: deterministic local fixtures exercise real renderer/Core paths; no paid-model business task, native packaged notification, remote CI, email/Feishu integration or deployment is claimed. This UI change does not alter backend coordination/control/delivery logic. The previous run14 independent acceptance remains evidence for its pinned Core source, not independent acceptance of this new UI.

## Evidence and reproduction

The [source manifest](evidence/source-sha256.json) pins the four renderer/test files and unchanged Core domain/Runtime. Original authorization is the current user request; no new team, model or permission change was introduced. Codex performed this medium-risk local UI verification.

From `apps/desktop`:

```sh
bun test tests/assistant.test.tsx tests/assistantNotification.test.ts tests/missionControlRendered.test.tsx tests/missionControl.test.ts tests/sessionRailRendered.test.tsx
bun run lint
bunx tsc --noEmit
bunx vite build --mode web --outDir ../../.codex/run/assistant-interaction-upgrade/web-dist
bunx vite build --outDir ../../.codex/run/assistant-interaction-upgrade/desktop-dist
```

Core rendering server: `cargo build -p codetwo-server --target-dir .codex/run/assistant-interaction-upgrade/target`, then loopback-only `webui --no-open --ui-dir <owned web-dist> --data-dir <owned ui-data>` on port 14674. Boot the empty disposable database once, stop its owner, apply [the offline seed](evidence/seed-ui.py), and restart the same owner. Automatic follow-up stays disabled, so no external Provider/model turn starts.

T3 preview opened and operated the actual application first. It then reported an explicit unavailable automation host during the final request action; no failed T3 submission was assumed accepted. The remaining checks used an isolated headless Chromium context with Playwright 1.63.0 installed only inside the disposable scratch tree. Existing cached browser binaries were reused. [The UI script](evidence/ui-check.ts) and [terminal read-only script](evidence/ui-terminal-check.ts) capture the exercised paths. Copy them to the owned `browser-harness` directory when reproducing; that directory's package supplies Playwright. No browser/package dependency was added to the product.

The initial browser script waited for an intermediate “waiting for stop receipt” label. The idle fixture execution settled before that label could be observed. The [UI report](evidence/ui-result.json) retains the timeout and its explanation. A corrected read-only terminal check verified the final paused state and persisted stopped notification without replaying requests or messages. JavaScript page errors: none in the completed headless UI run. The earlier T3 pairing route produced an HTTP 404 before SPA bootstrap; the final root URL pairing and UI run completed without that route.

Actual screenshots:

- [Project overview, light](evidence/projects-light.png)
- [Selected goal progress, light](evidence/goal-light.png)
- [Communication with explicit receipts, light](evidence/updates-light.png)
- [Unknown answer in Inbox, light](evidence/inbox-light.png)
- [Project overview, dark narrow](evidence/projects-dark-narrow.png)
- [Inbox, dark narrow](evidence/inbox-dark-narrow.png)
- [Persisted stopped notification, light](evidence/stopped-light.png)

## Cleanup

Removed: exact task-owned `.codex/run/assistant-interaction-upgrade/` after stopping its Core and closing the isolated browser. Also removed the accidental owned evidence staging path `.codex/docs/sdlc/changes/2026-10-05-assistant-interaction-upgrade/` after consolidating it here. No shared browser/toolchain cache or user data was removed.
Retained: renderer/test source, lifecycle records, compact logs, reproducible fixture scripts and seven referenced screenshots.
Retention owner: codex for this delivery; original owners for all pre-existing work and external evidence.
Cleanup trigger: normal repository evidence retention; no local task runtime is retained.
Processes: only exact task-owned Core processes stopped. Isolated browser contexts were closed by `finally`. Port 14674 is free and scratch has no process matches. The T3 automation host became unavailable, but the app-owned normal tab-close API acknowledged closure of task tab `tab_4`. No browser tab or worker is retained.
Evidence: [cleanup inspection and disposal report](evidence/cleanup.json). The normal T3 tab-close acknowledgement is recorded; no claim is made about unobserved renderer internals.

## Review and release

Approval: current user directly requested this bounded local interaction upgrade; see Intent. No external-action approval inferred.
Rollback: see plan.md; restore only these renderer/test changes, preserve accepted Core state and unrelated edits.
Release: local verified change only. No push, PR, merge, package release or deployment.
Feedback: use list-to-detail navigation, keep communication/management on demand, and preserve explicit control/receipt state.

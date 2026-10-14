---
id: 2026-10-07-shared-agent-research
schema: 5
stage: plan
status: accepted
owner: shared-agent-research-subagent
created: 2026-10-07
based_on: spec.md
scope: docs/sdlc/changes/2026-10-07-shared-agent-research
---

# Plan: Shared Agent Research

## Plan

Write the four stage files and four evidence files under `docs/sdlc/changes/2026-10-07-shared-agent-research/`: `evidence/source-evidence.md`, `evidence/proposal.md`, `evidence/prototype.html`, `evidence/preview-acceptance.md` (8 files in total). Nothing else is written.

Checks, chosen by risk (low, docs only):

- `bun script/verify/sdlc.ts --worktree`
- `bun script/verify/docs.ts`
- `git diff --check`
- Rendered prototype at widths 728 and 390 with T3 `html_preview`, plus the in-page simulation self-check (owner/device labels, new-task default owner, stop/continue keeps owner, private filtering, attribution). Rendering applies because the prototype is a UI artifact. Rust tests and the lifecycle Eval do not apply: no runtime or checker changed.

Real two-client E2E is a future implementation slice, not a check for this plan.

Temporary resources: the task-owned copy `.codex/run/shared-agent-research-20261008/prototype.html` is removed after its content is copied to `evidence/prototype.html`. No processes or services are started.

Rollback: delete the change directory `docs/sdlc/changes/2026-10-07-shared-agent-research/`. Nothing is committed or pushed.

Current rules (final round): Current rules (simulation only): a new task is public by default; public cannot return to private; a legacy private task can only be published one-way, and its owner is unchanged. Owner avatar, execution-device icon and share/lock icon are separate, each with title/aria-label; a light tap opens details. Board: 2026-10-08-shared-agent.

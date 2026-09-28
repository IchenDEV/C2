---
id: 2026-09-28-orchestration-stability
schema: 5
stage: spec
status: accepted
owner: codex
created: 2026-09-28
based_on: intent.md
---

# Spec: Orchestration Stability

## Design

Retain the existing switch transaction and callback fence. Canonical transcript remains the history source; reserve bounded initial/latest user context alongside recent neutral records. Cancelled or failed first prompts retain pending continuation, including native restore. Do not transfer reasoning or raw tool data.

Discover models from installed CLI APIs or ACP session metadata; remove static model aliases, provider/model-name effort rewrites and permanent process caches. ACP metadata owns the actual selectable values. Codex catalogue discovery and ACP execution must share the selected executable; explicit executable overrides fail honestly rather than falling through to a different runtime. Empty discovery is honest and retryable. Renderer providers come only from the host registry. Built-in launch integrations remain registered by Core; registration is not evidence of installation or models.

Show switching explicitly, fence duplicate local requests synchronously, and ignore provider metadata in transcript reducers. Preserve the old provider on failed switching. Unix ACP launches own isolated process groups; switching, shutdown and client drop terminate ordinary wrapper descendants and release pending RPCs. Reuse the plugin process-group signalling boundary; never signal an inherited or unrelated group.

Targeted corrections cost less to implement, validate and maintain than replacing the engine: ownership, storage commit and callback isolation already have contracts/tests. Module-level replacement of the fabricated catalogue is warranted; an engine rewrite would add migration/runtime risks without improving these boundaries.

## Acceptance criteria

- [x] AC-1: Long and Unicode histories retain initial/latest user context within a bounded neutral payload, and repeated switches use canonical history without nested handoffs.
- [x] AC-2: Failed/cancelled continuation and provider startup, busy and competing switches preserve recoverable state, including retry after native restore.
- [x] AC-3: Model discovery has no static fallback catalogue or permanent cache; unknown providers/models are never fabricated by the renderer.
- [x] AC-4: Provider metadata cannot crash transcript projection; switching has visible pending state and duplicate actions are fenced. Actual renderer checks cover the affected controls.
- [x] AC-5: Relevant Rust, desktop, documentation and worktree lifecycle checks pass; cleanup and unchecked live-provider boundaries are recorded.

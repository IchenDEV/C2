---
id: 2026-09-28-remove-plan-goal
schema: 5
stage: spec
status: accepted
owner: codex
created: 2026-09-28
based_on: intent.md
---

# Spec: Remove Plan Goal

## Design

Remove native Goal discovery/control/snapshots and Plan mode selection, scene plan-first activation and plan checklist surfaces. Provider-advertised planning controls must not reappear through generic configuration. Remove the built-in Plan-first fragment and mobile compatibility mode/prompt injection as well. The mobile contract exposes only its default interaction mode and rejects retired mode requests. Incoming plan updates are ignored without breaking ordinary text/tool streams. Historical stored plan rows remain readable as read-only legacy data, without active projection or new plan writes. Existing drafts/scenes with removed keys continue loading while those keys no longer affect execution. Keep a single effective path for supported session behavior; remove unused helper code and tests rather than adding a second feature toggle.

A scoped removal is cheaper and safer than replacing the session engine or rewriting persistence. No database migration is needed. Ordinary task objective text and scene document artifacts are not native Goal/Plan control features.

## Acceptance criteria

- [x] AC-1: No Plan/Goal controls, plan checklist panels or corresponding local draft state remain in the rendered workspace.
- [x] AC-2: Native Goal commands and capability/snapshot routes are removed; Plan updates and planning config cannot activate removed behavior. Text, tools, steering and model selection continue working.
- [x] AC-3: Existing stored plan history and old drafts/scenes load safely without enabling removed features.
- [x] AC-4: Relevant Rust/frontend regressions, workspace types/build, actual rendering, documentation and lifecycle checks pass; task resources are cleaned up.

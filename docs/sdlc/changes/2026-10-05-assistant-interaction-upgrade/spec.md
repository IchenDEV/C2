---
id: 2026-10-05-assistant-interaction-upgrade
schema: 5
stage: spec
status: accepted
owner: codex
created: 2026-10-05
based_on: intent.md
---

# Spec: Assistant Interaction Upgrade

## Design

Use a list-to-detail flow. The overview presents a project brief and compact goal rows. One request entry opens an intentional form; precise manual goal creation is an alternative within that entry. Selecting a goal opens its own page with progress, updates and management sections. Forms are not repeated across every goal. Keep submitted deliverables, unknown outcomes and stop receipts visible in their relevant context. Pause-follow-up, stop, cancel and takeover remain distinct actions; stop remains directly accessible for the selected execution.

Configuration is its own workspace section. Show management scope and follow-up first; model overrides and budgets are optional advanced settings. Memory starts with saved records and an explicit add/correct action. Inbox summaries open one answer/change form at a time. No native details/summary and no chat-bubble interface. Reuse shared Button, Tabs, Select and Separator; no dependency or backend additions.

## Acceptance criteria

- [x] AC-1: Overview has no native details/summary or per-goal forms; request creation and selected-goal navigation work with keyboard-accessible controls.
- [x] AC-2: Progress, communication and management are distinct; message, stop, takeover, dependency and review actions keep original identifiers/version checks. Failed or stale edits preserve drafts.
- [x] AC-3: Inbox keeps unknown answers non-replayable; answering and proposals require explicit selection. Memory, scope configuration and advanced settings remain reachable without appearing on the overview.
- [x] AC-4: Affected renderer tests, lint/type/build checks and real light/dark/narrow rendering pass; actual UI actions and cleanup are recorded. Backend and external verification limits remain explicit.

---
id: 2026-10-06-external-mcp-pr-integration
schema: 5
stage: spec
status: accepted
owner: codex
created: 2026-10-06
based_on: intent.md
design_approved_by: user
design_approved_at: 2026-10-06
design_approval_source: "Reuse user-confirmed default-off MCP/remote/scoped-credential design in ../2026-10-06-external-mcp-events-remote-headless/spec.md; predecessor backend design was approved 2026-10-06 18:01 (都批准). Current pr & merge authorizes delivery; no new security design."
---

# Spec: External MCP PR Integration

## Design

Copy only owned external MCP work and required host/event backend prerequisites into a clean branch on current main. Exclude desktop remote-T3 UI and host plugin, existing user desktop tests, and predecessor subagent UI. Limit bridge changes to the external credential surface; leave predecessor subagent renderer types to their owning change. Restore the existing repository lifecycle checks to CI and remove a broken historical design link while preserving its documented authority contract. Record checks on this assembled tree separately from prior full-worktree evidence.

## Acceptance criteria

- [x] AC-1: The isolated diff excludes active remote-environments UI and user-existing desktop tests; independent review checks required backend prerequisites.
- [x] AC-2: Affected Rust integrations, desktop types/tests/build, documentation and lifecycle checks pass for the assembled tree.
- [x] AC-3: The Ready PR preflight accepts authorized scope and linked verified records; merging remains conditional on passing CI and no conflicts.

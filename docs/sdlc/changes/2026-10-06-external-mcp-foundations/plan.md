---
id: 2026-10-06-external-mcp-foundations
schema: 5
stage: plan
status: accepted
owner: codex
created: 2026-10-06
based_on: spec.md
scope: docs/sdlc/changes/2026-10-06-external-mcp-foundations/, crates/core/src/lib.rs, crates/core/src/host_mcp/protocol.rs, crates/core/src/host_mcp/mod.rs, crates/core/src/external_mcp/
---

# Plan: External Mcp Foundations

## Plan

Two lanes with disjoint files: lane A (`host_mcp/protocol.rs`, `host_mcp/mod.rs`, `external_mcp/{clients,catalog,audit}.rs`) and lane B (`external_mcp/events.rs`); the owner added `external_mcp/mod.rs` and one line in `lib.rs`. Checks: `cargo test -p codetwo-core --lib -- external_mcp host_mcp` and `--test host_mcp`; no UI, server, or integration surface is touched, so no rendered or HTTP evidence applies.

Temporary resources: `/tmp/c2-target-check` (cargo cache inherited from the previous change, reused because a fresh build needs network access for Ghostty) and `/tmp/c2-research/`; both are retained until the parent change is handed off.

Rollback: Revert this diff; nothing is wired in and no data is migrated.

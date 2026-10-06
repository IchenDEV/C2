---
id: 2026-10-06-external-mcp-foundations
schema: 5
stage: spec
status: accepted
owner: codex
created: 2026-10-06
based_on: intent.md
---

# Spec: External Mcp Foundations

## Design

`host_mcp/protocol.rs` gains an `McpToolHost` trait so the same JSON-RPC handler serves the internal and the future external surface; `HostMcpDispatch` implements it with unchanged behavior and names. `external_mcp/clients.rs` holds hash-only, expiring, revocable, project-scoped client credentials persisted atomically with mode 0600 (refused when group/world readable), plus a per-client rate limiter. `external_mcp/catalog.rs` defines each tool once (name `codetwo_<domain>_<verb>`, class R/W/X/D, scope, schema, annotations) and excludes deferred and destructive tools from the default catalog. `external_mcp/audit.rs` records decisions with an argument hash only. `external_mcp/events.rs` defines the versioned envelope, content policy, mapping from `Event`, and an epoch-cursor ring with project filtering and bounded waits. Full design: parent change spec.

## Acceptance criteria

- [x] AC-1: The generic handler leaves internal host MCP behavior and tests unchanged.
- [x] AC-2: Client credentials are hash-only, expiring, revocable, project-confined, 0600, and never appear in Debug output; negative cases are tested.
- [x] AC-3: The tool catalog passes naming, uniqueness and class/annotation validation and excludes D and deferred tools by default.
- [x] AC-4: The audit sink never stores tokens or argument text.
- [x] AC-5: The event ring is bounded, epoch-safe, project-isolated, and its envelopes carry no prompt, argument or path text.

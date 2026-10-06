---
id: 2026-10-06-external-mcp-events-remote-headless
schema: 5
stage: intent
status: accepted
owner: codex
created: 2026-10-06
source: user
risk: high
approved_by: user
approved_at: 2026-10-06
approval_source: "User message 2026-10-06 18:24 (new worker thread continuing t3code/6738e658): 做对外的 MCP 支持、事件、远端访问、无头 — phases 0-2 with subagents; high-risk points (external exposure, long-lived credentials, write/delete capabilities) must be confirmed by the user before being enabled."
---

# Intent: External MCP, Events, Remote Access, Headless

## Intent

Four connected capabilities, all default off and explicitly enabled:

1. External MCP: external AI agents/clients operate CodeTwo through MCP (create projects and threads, send, stop, answer approvals and questions, settings, worktree, automation, and so on), covering as much first-party UI functionality as is safe. One naming convention, one authorization gate, tool annotations, scoped revocable client credentials, audit.
2. Events: a CodeTwo-owned bounded, replayable event envelope (task completed, state change, approval needed, question needed, error or broken session) with a mapping to stable MCP mechanisms; draft MCP event protocols are experimental only.
3. Remote access: optional external `cloudflared` integration for the Web UI and external MCP, default off, authentication mandatory on every exposed route.
4. Headless: one `codetwo` executable (serve + a minimal CLI) sharing the same core operations as the external MCP.

Constraints (inherited): one writer per session; `TurnOutcome::Unknown` is never replayed; native backends never fall back to ACP; unknown-outcome operations are not retried automatically; no commit, stash, checkout, reset, add, PR, merge or release; the user's desktop app, the live T3 service on 127.0.0.1:3773 (read-only probing only) and real data directories are never started, stopped or written; existing uncommitted changes and `apps/desktop/tests/` edits are preserved. Capabilities that expose the service beyond loopback, issue long-lived credentials, or write/delete user data require the user's explicit design confirmation before they are enabled; this record never invents that approval.

Non-goals: OAuth authorization server, DPoP, T3 Connect relay, outbound webhooks or any message sent to third parties, bundled/auto-downloaded cloudflared, MCP draft Events/Triggers as a standard dependency.

Original request: see `approval_source` and the user message of 2026-10-06 18:24. Research notes (read-only, temporary): `/tmp/c2-research/{A,B,C,D,E}.md`; findings are summarized in the Spec.

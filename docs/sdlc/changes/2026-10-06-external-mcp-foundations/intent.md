---
id: 2026-10-06-external-mcp-foundations
schema: 5
stage: intent
status: accepted
owner: codex
created: 2026-10-06
source: user
risk: medium
approved_by: user
approved_at: 2026-10-06
approval_source: "User message 2026-10-06 18:24: 低风险、可逆、默认关闭且不对外暴露的部分(命名整理、事件封装的内部类型、工具注册表重构)可以先做."
---

# Intent: External Mcp Foundations

## Intent

Land the pre-confirmation, low-risk foundations of the external MCP program (parent design and high-risk decisions: `2026-10-06-external-mcp-events-remote-headless`): a generic MCP JSON-RPC handler, the external client credential registry, the tool catalog and naming rules, an audit sink, and the event envelope with a bounded ring. All of it is pure library code with unit tests; nothing is routed, enabled or reachable from the network, and the existing internal host MCP behaves identically.

Constraints: no commit/stash/checkout/reset/add; no desktop, T3 or data-directory use; no wiring into `Engine` or `crates/server`. Credentials are never logged. Enabling any surface needs the parent change's user design confirmation.

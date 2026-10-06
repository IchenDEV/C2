---
id: 2026-10-04-builtin-chief-of-staff
schema: 5
stage: intent
status: accepted
owner: codex
created: 2026-10-04
source: user
risk: high
approved_by: chenli
approved_at: 2026-10-05
approval_source: "Current conversation: introduce a built-in assistant; user selected A, whole-CodeTwo cross-project coordination."
---

# Intent: Builtin Chief Of Staff

## Intent

在 CodeTwo 内置一个跨项目幕僚。用户给出目标和授权，幕僚掌握全局、派工、跟进阻塞、核对交付，执行 Agent 完成专业工作。用户无需在项目之间搬运上下文或催促下一步，仍可查看和接管执行。

来源：当前会话提出“相当于引入一个内置版的 openai dots 或者说 grok bots”，并明确选择“A：整个 CodeTwo，跨项目协调”。关联线程 `bb1ce412-fd4b-42ef-bedf-090f8c64cdc7` 提供幕僚与执行者分工的背景，不视为本次外部操作授权。

约束：遵循现有 Core、内置功能和权限边界；复用已有项目、会话、产物与执行状态；保持一个事实来源。普通执行者的默认角色不变。首版运行范围、持续唤醒及跨项目授权见 Spec，尚未取得独立设计确认。

风险为 high：涉及跨项目读取与派工、持久状态、后台唤醒和恢复。原始请求授权本地准备工作；代码实施需满足仓库的独立设计决定要求。未授权发布、部署或在用户真实项目中启用持续运行。

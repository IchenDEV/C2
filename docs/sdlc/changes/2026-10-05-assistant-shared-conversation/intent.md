---
id: 2026-10-05-assistant-shared-conversation
schema: 5
stage: intent
status: accepted
owner: codex
created: 2026-10-05
source: user
risk: medium
approved_by: chenli
approved_at: 2026-10-05
approval_source: "Current conversation: 用户要求幕僚复用主对话组件和逻辑，默认只见对话，执行过程隐藏，记忆及持续任务控制供开发者按需查看，以图标减少文字。"
next_trigger: Implement and render the bounded shared-conversation UI refinement.
---

# Intent: Assistant Shared Conversation

## Intent

幕僚的独立气泡、输入框和导航与主对话不一致，执行入口和说明文字仍过多。用户要求直接复用主对话逻辑，默认只看对话，记忆与持续任务控制按需查看，按钮优先使用图标。

本轮是 [已批准 agent 设计](../2026-10-05-assistant-agent-first/spec.md) 的本地 UI 收敛。保留原 AssistantState、Engine、Memory Store、异步投递、版本和未知结果约束。无新协议、模型、权限、付费模型实战、外部集成、云端常驻、push、PR、merge、部署或发布。保留用户进程与其他工作树差异。风险 medium：共用 UI 组件涉及主对话回归，需要检查和实际渲染；不新增高风险行为。

## Follow-up 2026-10-06

Current conversation 用户选择 A：只改显示，换成暂停图标和悬停提示，去掉常驻文字。沿既有共享对话范围做机械 UI 修补；不启用跟进，不改变暂停、对话处理或主动协调契约。授权来源为用户本次明确选择，不以模型建议代替。

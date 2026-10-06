---
id: 2026-10-05-assistant-event-ingress
schema: 5
stage: intent
status: accepted
owner: codex
created: 2026-10-05
source: "Current conversation: 邮件、飞书反馈、其他 webhook 或 MCP event 能否主动跟进；随后用户要求 把这些能力抽象好。"
risk: high
approved_by: user
approved_at: 2026-10-05
approval_source: "Current human message: 把这些能力抽象好。授权本地统一能力抽象及必要复核；不代表已确认新增身份、持久接收或自动动作协议。日期由 devflow 的 UTC 日期生成，本地日期为 2026-10-06。"
next_trigger: Implement the confirmed local protocol and independently verify it.
---

# Intent: Assistant Event Ingress

## Intent

用户希望邮件、飞书反馈、Webhook 和 MCP 通知都能主动唤醒同一个幕僚。当前渠道事件通道、用户对话 intake 和事项审查各有现成实现，但没有可靠且保留来源身份的外部接收链路。本轮将接收、来源绑定、事项跟进与结果投递抽象成一个可审查契约，并提供最小实施计划。

保留现有 Engine、Memory Store、AssistantState、事项控制、持久 attempt/receipt 和唯一执行投递路径；外部反馈不冒充用户命令、不直接变成确认记忆、不复制项目/执行事实。接收不等待 Provider，独立事项可并发；停止、接管、版本校验和未知结果不重放继续生效。

本次先完成文档与只读复核。新协议涉及身份与持久化，风险为 high，生产代码接入须有具体设计的独立人类确认。沿用原本已授权的幕僚目标，不重新改界面、模型或权限，不连接账户、启动监听或云端常驻，不向外发消息，不 push/PR/merge/deploy。

## Implementation authorization, 2026-10-06 local

用户在当前会话明确回复「确认设计，继续实现统一接入层」，并指定 Cursor Sonnet 5.5、300K、high 并行开发。该决定授权本 Spec 的本地统一接入实现；仍不包含账户连接、外发、发布、权限扩大或云端常驻。此前文档准备范围保留为历史。

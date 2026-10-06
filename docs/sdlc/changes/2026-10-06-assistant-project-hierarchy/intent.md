---
id: 2026-10-06-assistant-project-hierarchy
schema: 5
stage: intent
status: accepted
owner: codex-main
created: 2026-10-06
source: user
risk: high
approved_by: "user (current T3 human requester)"
approved_at: 2026-10-06
approval_source: "Current T3 thread 91d5d393-cf9a-4530-add8-406180f05ce9: hierarchical Project agents request and explicit Cursor Sonnet 5.5 / 300K / high authorization."
---

# Intent: 全局管家与 Project 管家

## Intent

用户要求三级 agent 架构：全局大管家统筹所有项目，每个 Project 有小管家管理细节，执行 agent 完成具体工作。两层管家各有范围不同的记忆和指令。先完成架构设计，再开始实践；主 Agent 负责监督、协调、检查，可使用 Cursor Sonnet 5.5、300K、high。

2026-10-06 用户明确选择：**业务项目，可包含多个仓库／工作目录**。当前路径身份不能直接代替业务 Project。

授权本地调研、设计、可复核记录及已确定边界内的工作。层级身份、控制权与信息范围属于 high risk；具体设计需独立人类确认。既有异步/长期记忆/事件接入批准有效，但不代替新增层级设计批准。

## Outcome and constraints

- 用户主要用自然对话管理；复用主对话，图标有名称/提示，记忆与执行细节按需观察。
- 管家有持续身份/上下文/行为，按事件启动模型；不要求每个 Project 永久驻留 Provider 进程。
- 复用同一 Engine、Store、Memory Store、身份、持久尝试和唯一投递路径。
- 保留逐事项异步、相关版本、停止/接管、容量释放重试与 unknown 不自动重放。
- 外部反馈仍只是观察；新派工、执行补充、权限扩大遵守既有确认边界。
- Cursor 委派按用户后续明确决定使用 Sonnet 5.5、300K、High、Full Access；除此不改模型、产品会话权限、预算；不加云常驻/真实邮件飞书账号；不外发消息、push/PR/merge/deploy。保留其他文件和用户进程。

## Basis

沿用已批准的 [agent-first](../2026-10-05-assistant-agent-first/spec.md)、[async progress](../2026-10-05-assistant-async-progress/spec.md)、[long-term memory](../2026-10-05-assistant-long-term-memory/spec.md) 和 [event ingress](../2026-10-05-assistant-event-ingress/spec.md)。本次补充层级，不重写这些事实源/协议。

---
id: 2026-10-06-native-provider-connectors
schema: 5
stage: intent
status: accepted
owner: codex-main
created: 2026-10-06
source: user
risk: high
approved_by: "user (current T3 human requester)"
approved_at: 2026-10-06
approval_source: "Current T3 thread 91d5d393-cf9a-4530-add8-406180f05ce9: replace four mainstream ACP connectors with official SDK/App Server; split research/build/check work and delegate Cursor Sonnet team."
---

# Intent: 官方原生连接器与第三方 ACP 双通道

## Intent

用户插入的新任务：Claude Code、Codex、Cursor、OpenCode 尽可能使用各自官方 SDK 或 App Server，由 CodeTwo 自有薄连接器接入；ACP 保留第三方标准扩展。参考 T3 或 herdr 的官方资料/源码，把调研、实现、检查拆成可并行细项，主 Agent 监督 Cursor Sonnet 小队。

用户随后明确指定 Cursor 工作运行环境：**Sonnet 5.5、300K、High、Full Access**。这修改本次委派运行模式，不推导为所有产品会话使用 full-access，也不授权新付费/账户设置。

## Scope and constraints

与 [Project 层级](../2026-10-06-assistant-project-hierarchy/intent.md) 并行设计，两者共用稳定的 Engine domain interface。任务不取消原层级设计。

协议、持久 session 恢复、工具审批、控制/回执属于 high risk。现有用户请求授权本地调研、设计、可复核拆解和已定边界工作；具体新增协议设计仍需独立人类决定，不能以 Cursor 建议代替。

- Engine/Store/Memory/Session/控制/尝试与唯一投递保持原事实源。
- 不将各官方协议重新硬压成 ACP；也不重新实现官方 agent loop。
- 模型、context/effort、用户 runtime 权限和预算保持；不静默切 Auto/其他 Provider/ACP。
- 不读/输出凭据、不自动登录/签发 key/启用付费、开云常驻、不替换用户进程/数据、不 push/PR/merge/deploy。
- 保留其他 dirty 工作树；本轮先做设计，不能将文档或旧回归称为新连接器验收。

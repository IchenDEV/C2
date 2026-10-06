---
id: 2026-10-05-assistant-agent-first
schema: 5
stage: intent
status: accepted
owner: codex
created: 2026-10-05
source: user
risk: high
approved_by: chenli
approved_at: 2026-10-05
approval_source: "Current conversation: 用户明确幕僚是个人助手 agent，辅助管理多个并行项目；随后要求先规划，再用已指定 Cursor 模型加速并行开发并完成。此为本地开发授权，不代替新增高风险设计的人类确认。"
next_trigger: Review the concrete agent-first design, then implement its accepted scope.
---

# Intent: Assistant Agent First

## Intent

恢复原目标：一个用户级个人幕僚 agent，持续与用户沟通，记住有来源的背景，管理受授权的多个项目。用户交办、讨论、纠正和接收结果；幕僚承担规划、派工、跟进与交付审查。不能把管理工作重新交给用户操作表单和看板。

本请求纠正 [上一轮界面设计](../2026-10-05-assistant-interaction-upgrade/spec.md) 的产品方向。其中“无聊天界面”不是本功能的用户决定，不沿用。该轮的实际渲染和回归仍是历史证据，不证明个人助手体验成立。保留 [原首版授权与边界](../2026-10-04-builtin-chief-of-staff/spec.md)、[双向协调协议](../2026-10-04-assistant-coordination-loop/spec.md) 和 [异步/R1 独立验收](../2026-10-05-assistant-async-progress/verification.md)。

用户于本轮明确要求规划并调用 Cursor 加速开发。主 Agent 保留当前模型并负责设计、监督、整合和验收。Cursor 目标由本轮真实能力目录确认：`providerInstanceId=cursor`、`claude-sonnet-5-5`、`contextWindow=300k`、`reasoning_effort=high`。子任务使用 `approval-required`，不再委派，单轮 10 分钟内受监督；计划审查轮为 8 分钟。

范围：原 CodeTwo 的本地 agent 沟通与协调。复用 Engine、Memory Store、身份绑定、持久尝试/回执和唯一投递路径。无第二套任务事实源。保留原工作树与用户进程。默认不启用真实项目持续运行，不跑付费模型业务实战，不改变模型或权限，不接入邮件/飞书，不增加云端常驻，不 push、PR、merge、部署或发布。

风险为 high：新增持久用户对话和从自然语言到已有控制操作的路径。开发授权已具备；具体设计需满足仓库独立人类决定要求。先完成可审阅 Spec 和 Plan，不能把 Cursor 建议当成人类批准。

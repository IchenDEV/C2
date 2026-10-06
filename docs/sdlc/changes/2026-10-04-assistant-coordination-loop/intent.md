---
id: 2026-10-04-assistant-coordination-loop
schema: 5
stage: intent
status: accepted
owner: codex
created: 2026-10-04
source: user
risk: high
approved_by: chenli
approved_at: 2026-10-05
approval_source: "Current conversation: user replied 继续实现 after reviewing the complete coordination design; authorizes bounded local implementation and verification."
next_trigger: Complete local implementation, integration evidence and independent verification.
---

# Intent: Complete the assistant coordination loop

## Intent

用户指出执行中沟通、需求变更、完成通知和提问能力缺失，要求全面重新梳理。继续以跨项目管理和长期记忆为核心。用户于 2026-10-05 在完整方案后明确要求继续实现。实施该协调闭环并验证，保留真实能力审计和行为契约作为依据。

关联：[首版设计](../2026-10-04-builtin-chief-of-staff/spec.md)、[首版验证](../2026-10-04-builtin-chief-of-staff/verification.md)。首版已有测试是局部能力证据，不构成完整协调闭环的验收。此前批准属于原设计；本次继续实现是对已展示协调方案的明确实施决定。

问题分类：协调模块的结构性缺口。修改 Goal 不等于改变运行中的任务；文本 blocker 不等于一个可回答的问题；轮次结束不等于验收；通知已发送不等于用户已读。保留 Engine、Memory Store 和普通会话，替换协调层的单次派工/终态取结果协议。

范围：CodeTwo 内部协调、站内待办、桌面通知适配、现有权限体系下的运行控制。不默认接入飞书/邮件，不新建云端常驻服务，不以产品看板充当仓库研发流程记录。

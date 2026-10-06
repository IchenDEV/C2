---
id: 2026-10-05-assistant-long-term-memory
schema: 5
stage: intent
status: accepted
owner: codex
created: 2026-10-05
source: user
risk: medium
approved_by: user
approved_at: 2026-10-05
approval_source: "Current conversation: 参考 openclaw 或者其他记忆库，给幕僚引入长期记忆；可用 Cursor Sonnet 5.5 300K high 并行开发。当地日期 2026-10-06。"
next_trigger: Verify bounded chief recall and restart/correction/forgetting behavior.
---

# Intent: Assistant Long Term Memory

## Intent

幕僚需要跨会话保留并使用个人偏好和项目决定。现有 Memory Store 已持久存储确认笔记，但关键词无交集时核心偏好未带入，且搜索先截最近 600 条导致旧知识被遮挡。补足常驻的有界核心记忆和相关检索；不新增另一记忆库、云服务、自动猜测保存、后台收费模型或历史归档。复用已确认的 [记忆确认及范围设计](../2026-10-05-assistant-agent-first/spec.md#记忆)，确认、纠正、忘记、来源及未知写入约束不变。

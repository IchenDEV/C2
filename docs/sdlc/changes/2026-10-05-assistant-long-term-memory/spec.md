---
id: 2026-10-05-assistant-long-term-memory
schema: 5
stage: spec
status: accepted
owner: codex
created: 2026-10-05
based_on: intent.md
---

# Spec: Assistant Long Term Memory

## Design

Ponytail full：以现有 codetwo.db Memory Store 为唯一知识事实源，复用 L1/来源/确认/纠正/遗忘，改进读取选择，不改 schema 或自动提取/保存规则；legacy origin 回填只在首次补字段时执行，不在每次重开时重标已有 automatic 来源。Core/Provider 记忆设置和 session Deny 继续优先。

借鉴 [OpenClaw](https://docs.openclaw.ai/concepts/memory) 的紧凑长期知识与按需回忆，及 [Letta memory blocks](https://docs.letta.com/v1-sdk/memory/memory-blocks) 的有界常驻背景。不是安装或接通这些服务，不采用其 Markdown 为第二写入者，也不新增 embeddings、网络、定时模型整理或付费调用。

幕僚每次 intake/事项审查从授权范围读取：既有 pinned pocket，最多四条 active、无冲突、manual 或 user_correction L1 preference/constraint 作为核心区，再按本次问题选择相关 L1/L2/L0/L3。每 scope L1 总数仍不超过 12；当前来源、原文、relevance 和不可信声明保留。Project 核心记忆不提升为全局。自动提取/候选和外部原文不进入确认核心区。

长期搜索在全 scope 的 active records 上先按查询匹配筛选/排序，再应用候选容量；避免无关的新记录遮挡旧知识。使用现有 SQLite/Rust 词法算法，不新增索引事实源；关键词限制及候选/上下文上限仍有明确界限，语义改写召回仍不承诺。

纠正立即影响之后召回，忘记的笔记及旧 profile 不再进入核心区或检索；不撤回已发送的 Provider 上下文。记忆确认仍按精确内容/hash、范围与持久 attempt 保存，任务进展与聊天保留在原所有者，不自动转为事实。

## Acceptance criteria

- [x] AC-1: 原确认笔记跨 Store 重开后保留；幕僚闲聊无关键词交集仍得到有界核心偏好/约束和精确来源，未确认/自动候选不会进入核心区。
- [x] AC-2: 超过 600 条无关新记录仍能找回旧英文/中文项目知识；搜索及主对话现有回归保持通过，结果/上下文有界。
- [x] AC-3: 全局/项目/session read 开关及项目隔离仍有效；纠正与遗忘影响下一次核心回忆，原未知写入不重放。
- [x] AC-4: 生产 intake 和事项审查调用同一 Memory Store 的增强读法；适用 Core/记忆/幕僚回归及独立复核有实际证据。UI 结构未变，无需重复渲染旧记忆界面。

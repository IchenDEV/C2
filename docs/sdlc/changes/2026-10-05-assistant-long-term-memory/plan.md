---
id: 2026-10-05-assistant-long-term-memory
schema: 5
stage: plan
status: accepted
owner: codex
created: 2026-10-05
based_on: spec.md
scope: crates/core/src/memory.rs, crates/core/src/plugins/app/plugins/assistant.rs, docs/reference/memory.md, docs/sdlc/changes/2026-10-05-assistant-long-term-memory
---

# Plan: Assistant Long Term Memory

## Plan

Cursor memory worker 只写 memory.rs 的选择逻辑与必要回归，10 分钟预算，Sonnet 5.5 300K/high、approval-required、default，不再委派。主 Agent 将两个幕僚 context 构建调用接到增强读法，保持原 Memory Store 独占写入和 settings 约束，更新 reference 文档及测试/独立复核。与事件协议 worker 文件隔离，后续 Runtime 写入按轮顺序协调。

检查：记忆单元测试、幕僚协调/投递/生产 Runtime，Rust check，docs/sdlc/diff，独立复核。新增测试验证旧记录、core pocket 边界、修正/遗忘与重开，不复制实现。无 UI 布局变化，无真实模型/外部服务请求。

Temporary resources: 与统一事件接入共用 task-owned `.codex/run/assistant-event-memory/` 隔离构建，不能并发写同一 target；结束移除，保留 compact evidence，由 codex 负责随记录维护。保留用户及其他工作状态。

Rollback: 撤回新的读法即可，旧 memory schema、数据和写入者未变；保留全部用户已确认记忆和回执。

Owner 首次测试 22 PASS/1 FAIL 揭示 install 的 origin 回填每次重开重复执行，导致无来源 automatic 测试行变为 manual。按原精确确认/来源边界修正为仅首次添加 origin 字段时回填；schema 不变，既有有 origin 数据不重标。不推翻历史来源，增加旧 schema/重复打开回归。

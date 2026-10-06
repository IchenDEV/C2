---
id: 2026-10-05-assistant-agent-first
schema: 5
stage: plan
status: accepted
owner: codex
created: 2026-10-05
based_on: spec.md
scope: docs/sdlc/changes/2026-10-05-assistant-agent-first, crates/core/src/assistant.rs, crates/core/src/assistant_conversation.rs, crates/core/src/plugins/app/plugins/assistant.rs, crates/core/src/memory.rs, crates/core/tests/assistant.rs, apps/desktop/src/assistant, apps/desktop/tests/assistant.test.tsx, apps/desktop/tests/assistantThread.test.tsx, docs/reference/memory.md
---

# Plan: Assistant Agent First

## Plan

主 Agent 负责最终接口、记录、监督、整合、真实渲染和验收。设计确认后才派可写实施任务。每轮不超过 10 分钟，保留稳定 clientRequestId、taskId、childThreadId 和开始时间；短 wait 超时不重派。发生重复报告或超预算时，对原任务原生取消并确认终态。

1. **Core 工作者**只写 `crates/core/src/assistant.rs`、可选 `crates/core/src/assistant_conversation.rs`、`crates/core/src/plugins/app/plugins/assistant.rs`、`crates/core/tests/assistant.rs`。实现幂等用户消息、上下文/路由、关联回复、相关版本校验、确认记忆和就绪工作优先级。复用短事务、持久尝试、已有控制和 Memory Store；不改 Engine、bridge 身份或共享 delivery 路径。接口冻结后前端并行。
2. **前端工作者**只写 `apps/desktop/src/assistant/`、`apps/desktop/tests/assistant.test.tsx`、`apps/desktop/tests/assistantThread.test.tsx`。以冻结的 JSON 契约和 fake API 开发主沟通入口；复用问题/变更/详情组件，删除失效首页表单。不得修改 App、MissionControl、SessionRail 和 Core 文件。
3. **主 Agent 整合**复核同一真实文件与哈希、端到端数据契约、来源和回执，不以子任务 completed 代替验收。修复必须归到单一文件 owner；不同时写相同文件。更新 Memory contract，只描述实际生效行为。
4. **独立复核者**在新受监督只读任务中检查最终 diff、恢复/权限/版本和验收证据，不修改实现。主 Agent 处理结论并完成最终检查及清理。

终态复核修补由主 Agent 单独负责：在原 Memory Store 中保留手动确认的准确文本，并将新增记忆与派生档案更新放入同一事务；自动学习仍保留相似去重。前端区分已确认待写入和已持久保存。此修补落实已批准的准确内容与单一 Memory Store 契约，不新增记忆写入者或权限。

检查：新增 intake/路由/控制/记忆 domain 与真实 ACP fixture 回归；原 Runtime 异步及 R1 资源释放回归、Core、协调和 prompt_delivery。前端跑受影响测试、lint、types、Web/renderer build。使用隔离 Core 与离线 ACP fixture 真实渲染及操作 AC-7；不调用付费业务模型、不跑原生打包通知或远程 CI。记录实际通过数、源码身份和明确限制。最终跑 `bun script/verify/docs.ts`、`bun script/verify/sdlc.ts --worktree` 与 `git diff --check`；仅在修改生命周期规则/检查器时运行 lifecycle Eval。

Temporary resources: 本轮预留 `.codex/run/assistant-agent-first/`，Core/UI 工作者使用各自子目录；共用编译输出只能有一个构建 owner。启动前按 [实例预检](../../../../.agents/skills/codetwo-operations/references/desktop-instances.md) 查数据目录/端口/用户进程。只关闭本任务预览、测试 Core 和 fixture 进程；压缩必要证据后移除构建、离线数据与临时浏览器资源。不得动用户进程、共享缓存或其他工作者文件。

Rollback: 保存本轮前的相关文件字节/哈希；只恢复本轮新增差异，不使用工作树级 reset。新增数据字段保留 serde 默认，停止 intake 仍保留原问题、执行与回执，不重放未知尝试。修复后的迁移必须用旧数据 fixture 验证。UI 回退仍保留无原生 details/summary 的现状；不擦除用户对话/记忆数据。

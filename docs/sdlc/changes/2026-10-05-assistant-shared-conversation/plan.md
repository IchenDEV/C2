---
id: 2026-10-05-assistant-shared-conversation
schema: 5
stage: plan
status: accepted
owner: codex
created: 2026-10-05
based_on: spec.md
scope: docs/sdlc/changes/2026-10-05-assistant-shared-conversation, apps/desktop/src/assistant, apps/desktop/src/session/Composer.tsx, apps/desktop/src/session/TranscriptPane.tsx, apps/desktop/src/session/TurnCard.tsx, apps/desktop/src/session/useTranscriptScroll.ts, apps/desktop/src/editor/Editor.tsx, apps/desktop/tests/assistant.test.tsx, apps/desktop/tests/assistantThread.test.tsx, apps/desktop/tests/assistantSharedConversation.test.tsx
---

# Plan: Assistant Shared Conversation

## Plan

1. 已核对主 App 的 TranscriptPane / Composer / DocEditor 路径、幕僚重复实现及上一轮授权和验收。保存本轮前相关源文件字节及 634 文件哈希。
2. Cursor 单一 UI 实施 owner：只写上述 assistant、session、Editor 与对应测试范围，不改 App.tsx、Core、桥接和其他工人的文件。复用真实公共会话组件，删除幕僚重复路径，图标化入口并保留必要决定与确定性控制。主 Agent 持续监督 10 分钟一轮，统一验收。
3. r1 已终态；主 Agent 接管所有源码整合，修正共享框架双方实际使用、记录顺序、失败提示和共享编辑器 Markdown 导出。r2 仅负责三个幕僚测试；r3 仅复核最终源码及执行检查。各轮独立 taskId，未重用子会话派新任务，均已回收真实终态且无待运行子 turn；时间及异常见各轮证据。主 Agent 负责本 bundle、整合和实际渲染。运行受影响会话/幕僚回归、desktop lint、tsc、Web 与 renderer 构建；记录独立检查或 owner 检查的真实范围。Core 源码不变时复用上一轮 Core 证据，不宣称新跑 Core 或真实模型实战。
4. 在隔离、离线 fixture 的实际 React Web 中操作对话、开发者记忆/任务入口与窄窗口；共享主对话也检查渲染。优先 T3 preview。渲染证明实际 React 交互，合成 fixture 不证明模型业务完成。没有包/通知/远程集成变更，不运行相应验收。
5. 更新 Verification，运行 docs 与 sdlc --worktree，检查差异和清理。

Temporary resources: `.codex/run/assistant-shared-conversation/` 为本轮独占构建/fixture 根；启动前检查端口和所有者，不操作用户 Core。仅清理本轮服务、浏览器 tab、构建与 fixture 数据。r3 还产生 `/tmp/review_r3/range.test.ts` 的测试副本，收尾一并核对清理；Plan 约定不写仓库源码不等于运行时无副作用。正式 evidence 保留小型日志、哈希、截图和回退压缩包，owner codex；用户接受本轮或要求回退后可移除回退包，其余证据随生命周期记录保留。

Rollback: 按 evidence/before-source.zip 与 before-sha256.json 仅恢复本轮新增差异；不使用全工作树 reset，不恢复其他人后续改动，不动用户对话、记忆或任务数据。

## Follow-up 2026-10-06

由主 Agent 直接完成这处小改动。只写 ChiefThread 与本 bundle；保留其他源码。运行相关幕僚回归、lint/types、Web/renderer 构建及实际浅/深色窄窗的状态图标/提示渲染；复用未变 Core/主会话证据。独占临时根 `.codex/run/assistant-paused-indicator-20261006/`，仅本轮 Vite 服务和预览 tab；完整清理后完成 docs/sdlc/diff 检查。

---
id: 2026-10-04-builtin-chief-of-staff
schema: 5
stage: plan
status: accepted
owner: codex
created: 2026-10-04
based_on: spec.md
scope: docs/sdlc/changes/2026-10-04-builtin-chief-of-staff/, crates/core/src/assistant.rs, crates/core/src/lib.rs, crates/core/src/store.rs, crates/core/src/engine.rs, crates/core/src/plugins/app/plugins/assistant.rs, crates/core/src/plugins/app/plugins/mod.rs, crates/core/tests/assistant.rs, crates/server/src/lib.rs, apps/desktop/src/assistant/, apps/desktop/src/sidebar/MissionControl.tsx, apps/desktop/src/sidebar/SessionRail.tsx, apps/desktop/src/App.tsx, apps/desktop/src/i18n/strings.ts, apps/desktop/tests/assistant.test.tsx, apps/desktop/tests/missionControlRendered.test.tsx, docs/reference/memory.md
---

# Plan: Builtin Chief Of Staff

## Plan

用户已批准 Spec。由 codex 实施，先完成记忆与项目管理，再接持久派工和有界唤醒。使用结构化决定及 Core 验证，避免为首版增加另一套工具传输。

1. Core 协调状态和工具：沿现有 Store、Engine 与 `crates/core/src/plugins/app/` 实现内置功能。先打通派工意图、创建/提示回执、版本验收与暂停修订；共享幂等路径应消除重复实现，不能冒用 T3 编码作为新公开协议。
2. 有界唤醒与恢复：Core 事件驱动幕僚会话，模块提供低频对账时钟。复用生命周期、事件订阅和事务领取方式；不把现有每次新会话的 Automation 当作持续幕僚，不复制第二套执行状态机。
3. 全局入口：扩展现有 Mission Control，通过 `bridge.ts` 调用统一命令；协调状态不进入 App.tsx 业务逻辑。保留普通会话入口和可见接管路径。
4. 验证：Core 单元测试覆盖范围、修订、去重、验收版本和预算；故障注入覆盖创建/提示崩溃窗口与恢复；真实 Provider 跑通双项目场景。桌面执行相关测试、类型与构建，并实际渲染浅色/深色/窄窗口。高风险验收由独立验证者执行。
5. 清理：删除本改动造成的失效路径与临时资源，保留既有无关模块和历史用户数据；运行适用 Rust/桌面检查、文档检查和 SDLC worktree 检查。无关生命周期规则未改变，不运行其行为 Eval。

文档准备检查：`bun script/verify/docs.ts`、`bun script/verify/sdlc.ts --worktree`、`git diff --check`。这些检查只验证记录与链接，不能作为 AC-1 至 AC-6 的产品证据。

Temporary resources: `.codex/run/builtin-chief-of-staff/` owns isolated test data, two fixture repositories and worktrees, logs and the Core process on port 14674. A task-owned Vite process serves port 14673. `target/` and `apps/desktop/dist/` were created for this verification and are disposed after evidence collection; shared installed dependencies remain. Cleanup includes the one aborted fixture worktree created outside this root, identified by its creation UUID.

Rollback: Disable follow-up through Configure. Existing workers remain visible and may be stopped individually. The new assistant_state table is additive; ordinary sessions and existing memory tables keep their owners and schema. Reverting code leaves that table inert and preserves user data. Store reopen and ordinary Engine/automation regressions verify coexistence; no destructive down migration is introduced.

Follow-up experience test (2026-10-05): user requested actual testing. Reuse the accepted scope for bounded UI fixes. `.codex/run/assistant-experience/` owns the fresh build, isolated Core database, fixture repositories and logs; reuse free ports 14673/14674 only for task-owned processes. Remove this directory and renderer build after collecting evidence.

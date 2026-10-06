---
id: 2026-10-05-assistant-event-ingress
schema: 5
stage: plan
status: accepted
owner: codex
created: 2026-10-05
based_on: spec.md
scope: docs/sdlc/changes/2026-10-05-assistant-event-ingress, crates/core/src/assistant.rs, crates/core/src/assistant_observation.rs, crates/core/src/lib.rs, crates/core/src/store.rs, crates/core/src/plugins/app/protocol, crates/core/src/plugins/app/bundle_runtime.rs, crates/core/src/plugins/bundle.rs, crates/core/src/plugins/app/plugins/assistant.rs, crates/core/tests/assistant_observation.rs, crates/core/tests/fixtures, docs/reference/plugin-protocol.md, docs/reference/plugin-standard.md, apps/desktop/src/assistant, apps/desktop/tests/assistantEventIngress.test.tsx
---

# Plan: Assistant Event Ingress

## Plan

以下步骤记录此前准备阶段。其时 Spec 未确认、Plan 为 draft；现在已由用户具体确认并接受实施，当前授权与执行分工见后文。

1. 主 Agent 核对 connector/event 的宿主身份、assistant.edit 的 user-only say、Store 事务和 Runtime 的相关事项指纹。记录 635 个现有源/测试文件基线，保存唯一事实源和调用边界。
2. 主 Agent 编写 Spec。r1 Cursor 只读复核，300K/high、approval-required、10 分钟监督预算，已正常 completed/pending=false，发现 5 个 P1 和 6 个 P2；主 Agent 据真实所有者代码修正。冻结修订稿后用新的原生 r2 任务做一次定向只读复核，预算 5 分钟，包含原约定、全部原结果和响应；子 Agent 不再委派、不写文件。人类设计确认不能由其建议替代。
3. 修正复核缺口，完成 docs/sdlc --worktree/diff 与状态清理；交付具体 Spec 和最小人类决定。本轮没有 UI/运行时行为变化，不重新启动 Core、渲染器或真实外部监听，不运行无关回归。
4. 人类确认后更新 Spec 状态与字段，再接受实施 Plan：先在原 Store 附表增加 Observation/消费位置/回执，在 AssistantState 保存用户 Binding，再增加宿主验证身份的 record 入口，将固定输入纳入现有 Runtime scope/wake。接收不改用户 revision，Core 实施 observation-only allowlist。通过真实 Core 协议夹具覆盖 AC-1 至 AC-6，复跑原异步/R1、协调、投递；不另建调度器。
5. 再用一个本地合成 SourceAdapter 验证 common contract；实际平台插件只负责传输适配。统一投影到现有对话/开发者检查，UI 变化时实际浅/深色/窄窗渲染。AC-7 的外部账户实战、发送及原生/远程检查需对应授权和环境，不能用本地夹具称已接通。

待确认后的候选源码边界：`crates/core/src/assistant.rs`、新增 `assistant_observation.rs` 与必要的 module 声明、`plugins/app/protocol/mod.rs`/`peer.rs`/`wire.rs`、`plugins/bundle.rs`、`plugins/app/plugins/assistant.rs`、相关 Core 测试、插件标准及协议文档，以及必要的现有 assistant 投影。接受 Plan 前根据确认的实现冻结精确 scope 和写入 owner；本轮不修改这些源码。

Preparation resources (historical): 当时不创建构建、Core、模型业务、浏览器或监听服务。本 bundle evidence 保留来源基线、小型 native 复核回执和检查日志，owner codex，随设计记录保留。只读子任务仍按真实终态及其实际副作用清点，不将 Plan 模式当成隔离保证。

Rollback: 仅撤回本轮新建 bundle；不重置原工作树或删除其他人的文档/源码。未来代码回退保留已持久 Observation、未知 attempt 与执行控制事实，在生产实施 Plan 中单独证明恢复；不以删除状态回退。

## Confirmed implementation ownership

本地 2026-10-06 用户确认后，本 Plan accepted。上文准备步骤是历史。Cursor 第一轮只写 assistant_observation.rs、assistant.rs、lib.rs、store.rs、protocol/、bundle.rs、tests/assistant_observation.rs，负责绑定、同 Store 接收/回执和真实宿主入口。主 Agent 负责 Runtime 整合、文档、独立验收与实际 UI。长期记忆并行 worker 只写 memory.rs，避免重叠。每轮 10 分钟、原生 wait/status/cancel 监督；不新增队伍或第二调度器。原协议标准必须保持兼容。

本轮 scratch 唯一根为 `.codex/run/assistant-event-memory/`，构建仅在隔离源码副本使用既有同版本 Ghostty pkg-config，不改用户 Cargo manifest，不运行真实付费模型。最终清除隔离构建、fixtures、浏览器和本轮进程；保留 compact evidence，owner codex，随记录保留。Runtime 用生产路径和协议夹具覆盖 AC-1 至 AC-6，适用原回归和共享 UI 实际渲染覆盖 AC-7；外部账号实战仍未验证。

Runtime r4 Cursor 单独拥有 plugins/app/plugins/assistant.rs，保留主 Agent 新记忆读法和测试；UI r5 Cursor 只拥有 assistant/api.ts、AssistantWorkspace.tsx、ObservationInspector.tsx 与 assistantEventIngress.test.tsx。主 Agent 拥有 bundle_runtime 清单能力接线、观察 Store/宿主剩余补丁、验收和文档。r3 在原 10 分钟预算到限后原生取消，真实 interrupted、pending=false；部分实现由主 Agent 接续，不称完成或验收通过。

最终本地交付步骤：owner 接续 r3/r6/r7 未完成内容，合并单 Store 接收及 Runtime allowlist、来源公平/容量出口、明确游标重置审批与长期记忆策略；修复实际夹具/类型/lint 后跑完整 Core 和 affected UI。r8 独立复跑发现回执排序与慢 A 测试控制时序；生产排序补丁及确定性正反回归记录于 Verification。r9 独立复核冻结源码，owner 同时记录实际 UI、收尾文档和证据；通过后清理唯一 scratch，执行 docs/sdlc/diff 检查。未扩展真实平台 adapter、账号订阅、自动外发或发布。

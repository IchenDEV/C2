---
id: 2026-10-04-assistant-coordination-loop
schema: 5
stage: plan
status: accepted
owner: codex
created: 2026-10-04
based_on: spec.md
scope: docs/sdlc/changes/2026-10-04-assistant-coordination-loop/, docs/sdlc/changes/2026-10-04-builtin-chief-of-staff/verification.md, crates/core/src/assistant.rs, crates/core/src/assistant_bridge.rs, crates/core/src/prompt_delivery.rs, crates/core/src/engine.rs, crates/core/src/event.rs, crates/core/src/lib.rs, crates/core/src/store.rs, crates/core/src/plugins/app/plugins/assistant.rs, crates/core/src/plugins/app/plugins/engine.rs, crates/core/tests/assistant.rs, crates/core/tests/prompt_delivery.rs, crates/server/src/lib.rs, crates/server/src/main.rs, apps/desktop/src-host/src/main.rs, apps/desktop/src/assistant/, apps/desktop/src/electrobun/index.ts, apps/desktop/tests/assistant.test.tsx, apps/desktop/tests/assistantNotification.test.ts, docs/reference/memory.md
---

# Plan: Coordination redesign

## Plan

用户在展示方案后明确要求继续实现。codex 负责列出的 Core、宿主和 UI 文件；先补共享投递和身份，再贯通问答/变更/交付通知。高风险实施验收由独立验证者执行。

执行顺序：

1. 共享投递与身份：改造 Engine 提示队列的持久命令/回执，覆盖普通 UI 与幕僚，定义 Provider steering/提问能力适配、未知回执和恢复。移除内存队列的唯一生产职责，不双写双跑。
2. 问答闭环：执行者报告/提问，幕僚按证据代答或升级到用户待办；同步原会话答复和接管；采用任务级阻塞，替换全局 attention 承担所有阻塞的逻辑。
3. 版本化变更：为目标和委派绑定需求版本；修改要求后实际投递、确认、停止/续接；阻止旧版本验收和双写入者；依赖失效和有限返工。
4. 交付与通知：result.submit/review.record，持久站内通知、桌面投递适配、深链和去重；移除由终态文字推断问题/完成的旧协议。
5. 记忆与体验：确认决定进入现有记忆；待办、变更差异和投递状态融入现有幕僚页面；自然语言交办生成可检查对象。

这是同一目标的递增交付，不能做完派工就宣称协调闭环完成。工程上先完成一条真实端到端场景：执行者提问 → 用户改变需求 → 正确中断/续接 → 执行者确认 → 交付/返工/验收 → 收到通知 → 重启不丢状态。再补跨项目公平性和失败矩阵。

比较与迁移：继续用现有 assistant 目标和 Engine 会话作为当前产品权威路径，不启用尚未接入的完整 Task/Scenes 运行时。读取 Task Store 的既有修订/幂等契约供复用评估；若发现适合迁移的已生产路径，先更新本 Spec 明确一次性迁移、去掉旧写入口，禁止运行两个可独立修改的目标源。现存首版 accepted 记录保留原需求快照，迁移后不把它当作新需求版本的验收。

检查：Core 完整单元测试、assistant/共享投递及普通 Engine 生命周期集成测试；真实 ACP 即时补充与不支持能力的排队路径；身份、幂等、迟到版本、依赖恢复、原生权限答复撤销、旧数据与中断回执；桌面共享 React 的实际 Web Core 交互；真实 Codex 工具试跑；UI、通知适配、生产 renderer 构建、workspace 检查及独立复核。完成后执行 docs、sdlc worktree 和 diff 检查。系统通知是否展示仍由平台决定，记录实际检查边界，不把适配单测当成 OS 展示证明。

Temporary resources: `.codex/run/assistant-coordination/` owns dedicated Cargo output, disposable data, fixtures, provider logs and task processes. Use free ports 14673/14674 after instance preflight. Collect small evidence then dispose the owned root and `apps/desktop/dist/`; preserve shared dependencies and user processes.

Migration: assistant_state 保留原单例和 revision；新增记录通过 serde 默认值读取旧 JSON，旧委派保留 protocol=0，新委派使用 protocol=1。新增 prompt_delivery 和 assistant_worker_receipts；旧队列表增列在同一 Store 初始化事务中完成。读取旧记录不生成新派工。queued 可恢复，submitting 只有已提交的 Engine 回执才能转 accepted，无法核对的网络结果转 unknown。

Rollback: 先禁用自动跟进；请求并核对自己管理的执行停止，保留工作区和结果。核对所有 submitting/unknown 命令，不能由旧二进制重放；备份静止的 SQLite 与用户产物。旧版本只能在跟进关闭时读取/使用普通会话，不承诺用旧协调协议继续新委派。重新启用须恢复本版本或经单独迁移验收。新增表可保留为惰性数据；不执行删除用户历史的反向迁移。迁移/恢复检查证明新增字段、id、回执和默认值保留；没有执行用户数据库降级。

---
id: 2026-10-06-assistant-project-hierarchy
schema: 5
stage: plan
status: accepted
owner: codex-main
created: 2026-10-06
based_on: spec.md
scope: docs/sdlc/changes/2026-10-06-assistant-project-hierarchy/, crates/core/src/assistant.rs, crates/core/src/assistant_project_hierarchy.rs, crates/core/src/memory.rs, crates/core/src/store.rs, crates/core/tests/assistant_project_hierarchy.rs, crates/core/src/plugins/app/plugins/assistant.rs, crates/core/src/assistant_conversation.rs, crates/core/src/assistant_observation.rs, apps/desktop/src/assistant/, apps/desktop/src/i18n/strings.ts, apps/desktop/tests/assistantProjectHierarchy.test.tsx
next_trigger: "Run supervised 30-minute Cursor rounds under confirmed Full Access, validate concrete artifacts and continue integration."
---

# Plan: 分阶段实践与并行开发

## Plan

最初设计阶段只做文档。父Agent综合方案，Cursor 两轮只读并行核对官方资料和 Core；再用新只读轮复核具体草案。r1/r2 按当时规则 approval-required；用户随后明确要求 Full Access，r3 原生取消并核实 interrupted 后，r4 按 Sonnet 5.5、300K、high、full-access、Plan 复核。8 分钟累计预算，保留 taskId/childThreadId/开始时间/终态并监督。不改模型/账户付费及产品权限，不额外 per-command override。失败如实入册，不把被派出当完成。

下表是已获用户设计确认的实施方案；上方scope已列精确产品路径，每轮仅领取明确ownership，不另建计划注册表。

| 阶段 | 最小交付 | 并行边界 |
| --- | --- | --- |
| M1 身份/记忆/指令 | ManagedProject/绑定、目标归属和 epoch、版本化指令、Memory 范围组合、legacy/schema guard | Core 数据/迁移一名执行者；独立验收用例设计另一名只读者。先定 wire 类型，不同时改 Store/assistant domain |
| M2 两层协作 | chief 路由、Project 范围上下文/白名单、原 run/attempt/outbox、控制/交接 | Runtime 一名执行者；M1 接口完成后 UI 一名执行者；主 Agent 合并 domain/Runtime 接口 |
| M3 对话体验 | 共享主对话入口切换全局/Project，按需记忆/指令/控制观察，显示来源与采用 | UI 只拥有 assistant UI/i18n/相关测试；不改调度；用实际 light/dark/narrow 渲染验证 |
| M4 迁移/实践 | 两个业务 Project，其中至少一个关联两仓库，证明全链路；独立复核 | 首先 deterministic ACP fixture；再在用户选定试点和现有预算内实践。不额外开启付费/云服务 |

## Checks and acceptance

本轮执行官方/source 核对、具体设计只读 review、`bun script/verify/docs.ts`、`bun script/verify/sdlc.ts --worktree`、`git diff --check`；重算 pre-existing dirty 文件 hash 保证未变。本轮无产品/UI行为改变，不启动 Core、UI、付费模型业务实战，不把之前通过的回归称为层级通过。

批准后：AC-1/2/3/4/7/8 用 Store migration/domain/receipt integration；AC-5/6 用真实 ACP fixture 生产 Runtime.tick，验证非阻塞/资源/恢复；AC-9 实际 T3 React Web/Core 渲染，启动前执行 instance preflight；AC-10 跑适用 Core/桌面 tests/types/lint/build 及独立验收。保留原异步、容量释放、未知结果、观察限制和 Memory 回归。

## First practice

先展示并确认两个业务 Project 的准确工作区绑定、项目指令和共享偏好范围。不从目录名猜业务边界。先迁移无活动 assignment 的目标；活动执行保留原控制直至交接可核对。

实践顺序：问整体进展 → A 内两仓库交办 → B 并行交办 → A 执行中提问/改需求/停止 → 重启恢复 → 检查实际两层 Memory/instruction receipt。付费模型、外部账号、原生通知未做就保留未验证。

Temporary resources: 本轮无 Core/UI/build 或专用 scratch；只保留本 bundle 的紧凑任务状态、调研、基线、review 和检查证据。owner 为 codex-main，本次交付前及下次设计变更时审查。Cursor 资源用原生终态检查；不终止用户进程。

Rollback: 本轮仅文档。实施后先停止新协调并确认全部在途控制，再恢复未迁移范围；不能切开关复活旧 owner/丢 unknown。已升级状态阻止旧 writer，不能直接运行不兼容二进制；按已验证备份/显式迁移恢复，保留原目标与回执。

## Implementation authorization and ownership

2026-10-06用户确认已展示的具体设计并要求立即Cursor实施。前面的设计阶段限制作为历史记录保留；本节启用上述精确production scope。每轮任务只领取其中明确文件，其他文件由主Agent集成；不同时修改共享Store/Engine/Cargo/lockfiles。所有子任务固定Sonnet 5.5/300K/high/full-access，实施default、研究plan，r5单轮10分钟，当前r6已批准30分钟，由主Agent持有taskId、监督及验收。

## r5 checkpoint and next stop

本轮三个任务均耗尽默认10分钟预算后原生取消，interrupted/no pending；两路候选不完整，已保存精确补丁并撤出会破坏现有编译的active修改。按全局AGENTS.md第36行“累计时间预算耗尽后仍运行时…取消…保留部分结果并报告异常，不重派”不自动新开同一任务。设计和已有实施授权保持；最小所需人类决定是允许更长单轮预算（建议30分钟），不是再次设计批准或增加模型/产品权限。后续利用现有候选，缩小每轮完整模块；不重复全量调研。

## r6 budget decision and resumed ownership

2026-10-06用户明确允许延长前述单轮预算至30分钟，并报告cos挂掉原因为未开fork access。将该原因记为用户报告；当前原生目录没有独立fork access权限字段，不虚构参数或创建普通fork线程。继续显式runtimeMode=full-access、Sonnet 5.5/300K/high，以实际工具动作和终态验收。此前r5均已原生核对interrupted/no pending后才派新轮。

本轮30分钟预算从新delegate调用开始累计，较短wait窗口仅用于持续监督，不重置时间；继续同一目标和当前工作树，不另建开发队伍。新轮利用精确候选与已知失败，不重复全量调研。Project domain可以写crates/core/src/assistant_project_hierarchy.rs；native基础仍独占Engine/lib/runtime/ACP接口。构建由父Agent串行控制，以隔离源码默认terminal feature及既有同版本Ghostty pkg-config验证；此前no-default-features错误不是产品回归通过。

## Pilot choice — current human decision

用户对试点边界回复“随便找两个，或者新建两个玩一玩。”主Agent选择两个隔离业务Project：Atlas关联API/Web目录，Beacon关联报告目录；准确路径与来源见 [选择记录](evidence/pilot-selection.json)。这是本地示例选择授权；不是新增模型、账户、权限或真实现有目标迁移授权。当前只创建目录，运行及验收结果必须另外证明。

## M2 integration decisions after r9

r9已completed/no pending；[真实只读结果与抽查边界](evidence/runtime-plan-result-r9.json)。主Agent核对ensure_session、Engine实际MCP挂载和worker Context，确认候选M1未接Runtime，manager目前仍拿到通用MCP，executor Context读全局记忆。报告是实施输入，不是人类设计批准或产品通过。

沿已批准Spec执行：

- 继续同一scope调度、CAS、attempt与delivery。actor由持久目标owner/用户选定对话范围绑定；映射目标排除旧chief协调。legacy无层级字段保持原fingerprint；只围栏实际相关的owner/binding/instruction/memory，不hash整个hierarchy。
- 先在Engine/native adapter兑现协调角色的零内置工具及按角色MCP限制；不能证明的后端标unavailable。ReadOnly/prompt和空MCP列表不能单独证明原生工具已禁止。M1完成后再按唯一ownership接Runtime与UI，避免与当前r7/r8交叉写文件。
- 不采用r9“chief所有Message/Change一律拒绝”的建议：chief自主审查不能绕过Project管家，用户在全局对话的明确指令仍须host校验来源后路由至所属Project，复用原补充/变更/控制路径。停止/优先级及时记账，不等待模型。
- 不采用r9以Engine Idle单独写release的建议：释放必须绑定现有原attempt、session/run和实际终态/停止证明；未知与单纯空闲不解除旧writer。
- Project/executor上下文复用Memory Store的实际检索、策略与注入回执；新增读取API存在不等于已经注入或采用。需补相关版本/来源测试。

这些决定细化已有确认设计，不新增产品目标、模型或权限授权。后端可用性与实际完整链路仍待fixture及兼容验证。

## Current continuation checkpoint

指定Cursor r13已failed/no pending，实际Sonnet 5.5/300K/high/Full Access/default，SDK为connection_stalled。20个已记录原任务均终态无待运行子回合；不重复派同配置任务或扩大权限。主Agent完成已批准范围的共享Engine出站接入与M1持久创建意图保护，当前owner Core814/领域19/native runtime15和workspace/all-targets通过，源码与环境/清理记录在Verification。I6持久原生身份/迁移、I7中性回调/权限、四官方backend及M2/M3/M4仍需实现，不能据此激活新后端或层级。指定Cursor连接恢复是续作触发，现有设计及本地开发授权继续有效；最终独立验收保持待做。

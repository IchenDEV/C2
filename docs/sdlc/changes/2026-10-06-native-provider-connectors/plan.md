---
id: 2026-10-06-native-provider-connectors
schema: 5
stage: plan
status: accepted
owner: codex-main
created: 2026-10-06
based_on: spec.md
scope: docs/sdlc/changes/2026-10-06-native-provider-connectors/, crates/core/src/provider_runtime.rs, crates/core/src/connectors/, crates/core/src/acp/, crates/core/src/engine.rs, crates/core/src/provider.rs, crates/core/src/session.rs, crates/core/src/session_import.rs, crates/core/src/permission.rs, crates/core/src/skill.rs, crates/core/src/lib.rs, crates/core/src/event.rs, crates/core/src/store.rs, crates/core/Cargo.toml, Cargo.toml, Cargo.lock, crates/core/tests/native_provider_runtime.rs, crates/core/tests/canvas_v1.rs, crates/core/tests/native_provider_connectors.rs, crates/core/tests/fixtures/native-providers/, script/provider-sidecars/, apps/desktop/src/bridge/, apps/desktop/src/assistant/, apps/desktop/src/i18n/strings.ts, apps/desktop/tests/nativeProviderCapabilities.test.tsx
next_trigger: "Resume the already-approved official backend tasks after Cursor connection_stalled is resolved; keep Sonnet 5.5/300K/high/Full Access."
---

# Plan: 调研、共享契约、并行连接器、检查

## Plan

最初设计阶段仅允许写本bundle；用户确认后，上方scope已启用精确产品路径。下面列出受监督实施细项与依赖，每轮只领取明确ownership。主 Agent负责决策、共享文件合并、监督与最终验收。具体协议确认已记录在Spec；不另建任务事实库。

Cursor 固定 Sonnet 5.5、300K、High、Full Access，来自用户本轮明确决定。调研/复核 Plan 只读；实施 default，互不覆盖写目录；r5默认10分钟，当前r6经用户确认每轮30分钟，状态/取消可用才派出，保存 taskId/childThreadId/start。在原生终态和验收前不报完成，预算超出对同task取消一次并确认，无常驻轮询 Agent。Context 不默认1M，不Auto/MAX，不改账户/付费。

## Task breakdown and dependencies

| ID | 子任务/产物 | 依赖/并行及唯一写入者 |
| --- | --- | --- |
| R1 | 四官方接口能力矩阵：create/resume/send/steer/stop/events/permission/MCP/tools/usage，版本/auth限制 | Cursor SDK research已completed，主Agent复核官方原文；实际/tmp取源码副作用已清理并记账 |
| R2 | 现有ACP耦合、Engine seam、事件/权限/恢复落点与最小改法 | Cursor Engine seam已completed；与R1并行，只读分析不等于运行验收 |
| R3 | T3/Herdr源码对照：实际commit/文件路径、采用/不采用理由 | R1内T3分析已回收；主Agent自行确定herdrdev/herdr并核对官方资料，记录实际抽查范围 |
| R4 | 具体Spec反例复核，冻结中性接口与能力行为 | 新独立Cursor review超时原生取消，interrupted/no pending；没有取得独立verdict。主Agent已检查并修订草案；用户后续确认了设计，产品实施仍待实际验收 |
| I0 | 定义Provider-neutral operations/events/capabilities/identity与snapshot版本 | 人类设计确认后，主Agent/单一Core执行者拥有新runtime types与Engine接缝；先交可编译契约 |
| I1 | Codex AppServer connector：JSON-RPC、initialize、thread/turn、steer/interrupt、approvals/MCP | I0；独占拟定 crates/core/src/connectors/codex.rs 与自己的fixtures/tests |
| I2 | Claude AgentSDK connector：受监督sidecar、stream/session、permission、MCP、interrupt/dispose | I0；独占拟定 connectors/claude.rs 与 script/provider-sidecars/claude/；不改共享lockfiles |
| I3 | Cursor官方TS SDK sidecar connector，固定Bridge缺少steer已由probe确认；model/options、run/stop/permissions仍须生产验收 | I0+版本/能力确认；独占拟定 connectors/cursor.rs 和必要Cursor sidecar；不同时实现两个fallback |
| I4 | OpenCode V1官方HTTP/events与V2独立SDK/Client：权限/问题/abort/恢复按各版本能力 | I0；独占拟定 connectors/opencode.rs、必要 script/provider-sidecars/opencode2/ 与V1/V2 fixtures，不共用codec或误用V1 server |
| I5 | ACP实现接到中性接口，Custom/Grok等第三方入口保持 | I0；单人拥有 crates/core/src/acp/ adapter，避免四native workers改ACP |
| I6 | registry/session backend持久字段、legacy writer排空/恢复与版本锁定 | I1–I5后；集成人单人拥有 provider.rs/session.rs/store.rs/engine.rs，其他worker只提交接口建议 |
| I7 | host ToolBroker/权限回执/MCP/Memory provenance完整接入 | I6；集成人与权限reviewer分开，不能把SDK默认auto-approve直接接进产品 |
| I8 | 新会话默认native、去除已替代builtinACP包装和自动fallback；legacy引用清理 | 对应backend合同与迁移验收通过后逐个执行，保留第三方ACP |
| I9 | 前端能力/诊断与审批/停止状态复用主对话展示 | I6 wire冻结后；UI owner只写受影响消费者/i18n/tests，不重做对话 |
| T0 | 各backend协议fixtures正反例：实际事件、方法/版本拒绝、unsupported能力 | 可与I1–I5分文件并行，固定oracle不只是镜像实现 |
| T1 | 真实生产Engine的慢A/独立B/及时stop、单writer、unknown/崩溃/断线/去重 | I6/I7；不以mock transport全部成功替代生产路径 |
| T2 | legacy迁移/回滚、审批悬挂、ID区分、版本与native失效不fallback | I6/I8；副本数据验证，不能动用户当前db/进程 |
| T3 | permission/MCP/tool身份隔离、跨session请求/过期审批、路径/预算不扩大 | I7；独立review+负向合同测试；模型所说“有权”不是oracle |
| T4 | 实际React/Core light/dark/narrow审批、能力、不支持steering与控制状态 | I9；instance preflight后实际渲染，不能仅DOM测试/图片 |
| T5 | 每家已安装CLI/SDK version的local compatibility canary | 对应backend+可用授权auth；无凭据或付费不默认补开。fixture通过与实际后端通过分别记账 |
| T6 | Core/desktop适用checks、原幕僚/Memory/observation回归、docs/sdlc及独立终态验收 | 版本冻结后；保存具体版本/hash/命令/日志、清理所属资源 |

I1/I2/I3/I4/I5是可并行叶任务，按实际监督能力分批，不固定高并发。共享 Cargo/package/lockfiles、runtime trait、registry、Engine、Store只由集成人写；子任务不得修改他人文件或另委派。失败先读取原task和receipt，再新轮返工，不重复创建未知执行。

## Validation and handoff

本轮：官方/source实读、只读独立设计review、docs/sdlc/diff checks、与此前dirty baseline逐文件hash比较。没有实施代码/SDK运行/真实UI变化，不跑无关Core/UI/付费实战。

产品阶段：AC-1由I0/I5/T0；AC-2–5各后端T0/T5；AC-6/7由T1/T3；AC-8由T2；AC-9由T4与原功能回归；AC-10由T6。任何缺少真实账户/版本验收的后端仍列未验证，不能以论文/源码阅读称为连接成功。

Temporary resources: 当前无clone/build/SDK安装或Core/UI进程，只保留本bundle紧凑调研/任务状态/check证据，codex-main负责交付前与续作时审查。实施probe使用ignored .codex/run/native-provider-connectors/<worker>/ 专用根，版本freeze后只留必要evidence，交付前清除所属sidecar/server与scratch，不停止用户服务。

Rollback: 本轮仅文档。实施后禁用某后端的新派工并先核对所有原生writer/unknown，再回到已验证版本；backend已写入Session不能靠registry切换成ACP。恢复副本/迁移由既有控制和无活动writer证明约束，第三方ACP独立保留。

## Implementation authorization and ownership

2026-10-06用户确认已展示的具体设计并要求立即Cursor实施。前面的设计阶段限制作为历史记录保留；本节启用上述精确production scope。每轮任务只领取其中明确文件，其他文件由主Agent集成；不同时修改共享Store/Engine/Cargo/lockfiles。所有子任务固定Sonnet 5.5/300K/high/full-access，实施default、研究plan，r5单轮10分钟，当前r6已批准30分钟，由主Agent持有taskId、监督及验收。

## r5 checkpoint and next stop

本轮三个任务均耗尽默认10分钟预算后原生取消，interrupted/no pending；两路候选不完整，已保存精确补丁并撤出会破坏现有编译的active修改。按全局AGENTS.md第36行“累计时间预算耗尽后仍运行时…取消…保留部分结果并报告异常，不重派”不自动新开同一任务。设计和已有实施授权保持；最小所需人类决定是允许更长单轮预算（建议30分钟），不是再次设计批准或增加模型/产品权限。后续利用现有候选，缩小每轮完整模块；不重复全量调研。

## r6 budget decision and resumed ownership

2026-10-06用户明确允许延长前述单轮预算至30分钟，并报告cos挂掉原因为未开fork access。将该原因记为用户报告；当前原生目录没有独立fork access权限字段，不虚构参数或创建普通fork线程。继续显式runtimeMode=full-access、Sonnet 5.5/300K/high，以实际工具动作和终态验收。此前r5均已原生核对interrupted/no pending后才派新轮。

本轮30分钟预算从新delegate调用开始累计，较短wait窗口仅用于持续监督，不重置时间；继续同一目标和当前工作树，不另建开发队伍。新轮利用精确候选与已知失败，不重复全量调研。Project domain可以写crates/core/src/assistant_project_hierarchy.rs；native基础仍独占Engine/lib/runtime/ACP接口。构建由父Agent串行控制，以隔离源码默认terminal feature及既有同版本Ghostty pkg-config验证；此前no-default-features错误不是产品回归通过。

## Current continuation checkpoint

指定Cursor r13已failed/no pending，实际Sonnet 5.5/300K/high/Full Access/default，SDK为connection_stalled。20个已记录原任务均终态无待运行子回合；不重复派同配置任务或扩大权限。主Agent完成已批准范围的共享Engine出站接入与M1持久创建意图保护，当前owner Core814/领域19/native runtime15和workspace/all-targets通过，源码与环境/清理记录在Verification。I6持久原生身份/迁移、I7中性回调/权限、四官方backend及M2/M3/M4仍需实现，不能据此激活新后端或层级。指定Cursor连接恢复是续作触发，现有设计及本地开发授权继续有效；最终独立验收保持待做。

Scope clarification: `crates/core/tests/canvas_v1.rs`是已批准中性prompt content返回类型的现有Rust消费者；只更新一个matcher的类型名，Canvas行为、UI、存储及图片策略未改。首次收尾scope检查指出此前漏列该消费者，补齐既定接口改动的文件范围后重查，不新增产品目标。

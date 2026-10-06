---
id: 2026-10-05-assistant-event-ingress
schema: 5
stage: verification
status: passed
owner: codex
created: 2026-10-05
based_on: plan.md
revision: "14c20fce841384dbeaa16c48c7a7264d6e4a9f81 + preserved dirty worktree; evidence/source-sha256.json (358 frozen files, final formatting snapshot checked by r10)"
verification_mode: fresh-context
verified_by: cursor-independent-r9-and-r10
verified_at: 2026-10-06
release_target: none
cleanup_status: complete
---

# Verification: Assistant Event Ingress

## Verification

本轮用户已经具体确认身份、持久接收和 observation-only 设计。下列 PASS 区分 owner 本地证据、r9 独立完整复跑与 r10 最终格式/定向核对。两轮均 completed/pending=false；358 最终 hash 无变化，清理完成。源码审查是有限审查，不是完整协议安全审计，也不是发布。此前静态审查、旧异步验收或实施子任务 completed 没有单独当成新协议通过。

- AC-1: PASS — 真实插件进程经 initialize / observation/record 对 mail、feishu、webhook、MCP 四种合成 connector 使用同一入口；宿主保留 raw plugin/realm/connector，未绑定及 payload 伪造被拒绝。见 [owner Core 日志](evidence/core-owner-r9.log)、[冻结源码](evidence/source-sha256.json)；账户身份仍是用户固定的 adapter 声明，不称为远端认证证明。
- AC-2: PASS — 10 项 Store 集成验证 batch/content hash 幂等、冲突隔离、绑定 CAS、多事项回执及单份正文。歧义进入现有通知。r8 发现的首次/重复回执顺序问题已统一排序，并用逆序绑定作确定性回归；见 [r8 原独立结果](evidence/independent-r8-result.json)、[响应](evidence/r8-response.json) 和 [owner 回归](evidence/core-owner-r9.log)。
- AC-3: PASS — 同一 SQLite 事务持久接收/游标/批次回执；原 batch 返回原结果，毒消息终态可见，live_only 缺口和需重置可见，exact user approval_ref 只消费一次。claim/session-new/send/commit 的持久 attempt、原 run/output ID、unknown 不自动重放及撤销不降级有运行测试；见 [Core 日志](evidence/core-owner-r9.log)。不会盲目 fetch reference 或重放不确定发送。
- AC-4: PASS — 慢 A 不挡 B 接收、观测处理、原授权派工和回执，250ms 内取得共用 gate 并完成 A 的用户暂停；固定输入排除到达中的后续事件，无关 B 不废弃 A。A 自身派工、需求/控制及来源变化废弃旧决定，原 R1 资源释放/停止写入者约束回归继续通过。r8 慢 A 失败的原因与确定性正反用例见 [响应](evidence/r8-response.json)、[9 项定向运行](evidence/r8-fixes-focused-r2.log) 和 [完整 Core](evidence/core-owner-r9.log)。生产版本 fence 未改。
- AC-5: PASS — 接收不增加 AssistantState revision；unfinished/history/目标回执有独立上限、backpressure/needs_capacity/needs_reset 出口。暂停/budget/attention/终态对应持久 deferred；source 轮转、两空位优先不同来源、通知复用不刷屏有回归；原用户输入/控制独立推进。见 [Core 日志](evidence/core-owner-r9.log)。输入容量满时需原 adapter 根据回执保留并恢复，不能宣称上游已 ACK。
- AC-6: PASS — Core 的观测动作只允许 scoped summary、向用户非阻塞提问和需求提案；拒绝 dispatch/update/control/accept/memory/permission/外发，外部输出持续带 observation 污点。用户本地回答不投递执行者；原授权工作仍走原 Engine/delivery。见 [Core 日志](evidence/core-owner-r9.log) 及 [协议参考](../../../reference/plugin-protocol.md)。
- AC-7: PASS — 当前 owner 全 Core 780 PASS / 0 FAIL / 1 既有 live Provider ignored，含 603 unit、27 coordination、10 observation、6 delivery、4 doctest；affected UI 64 PASS，types/lint PASS。实际生产 React 浅/深色/480px 窄窗与 reset 批准交互已渲染，默认共享对话及 icon 入口保持；详细环境见 [owner 结果](evidence/owner-results.json)、[渲染记录](evidence/render-results.json)。r9 独立 Core/UI/types/lint 已通过，r10 确认最终仅有格式变动并独立重跑 21 项事件 UI；外部平台实战、付费模型业务、原生桌面通知及远程 CI 未验证。

Verdict: verified.
Residual risk: 最终格式核对与清理已经完成。r9 为有限独立审查，未覆盖完整协议安全审计；单次 Cargo 沙箱外权限偏离保留。只交付本地统一观察抽象与授权范围内的 Runtime/UI；真实邮件、飞书、Webhook 服务、MCP 事件提供方的鉴权/订阅/上游恢复未接通。未运行真实付费业务模型、原生打包通知、远程 CI、PR、发布或部署。词法记忆检索不保证语义改写召回。

## Current evidence

[最终本地结论与限制](evidence/final-results.json) 汇总真实计数、准确版本、执行偏离和清理；不是 PR、发布或外部平台验收。

- [实现基线](evidence/implementation-baseline.json) 与 [本轮精确变化](evidence/implementation-source-delta.json) 区分原工作树内容；保留既有 Engine、主对话和其他修改。新 facts 仍由同一个 Store 持久化。
- [r9 源码 SHA-256](evidence/source-sha256.json) 覆盖 358 个源码/fixture/UI-test/reference 文件；[r8 旧冻结版本](evidence/source-sha256-r8.json) 只用于追溯历史失败。
- [r8 独立结果](evidence/independent-r8-result.json) 是 changes-required，曾发现 2 个 P2。对应 [fail-fast](evidence/independent-r8-core-all.log)、[no-fail-fast](evidence/independent-r8-core-all-nofailfast.log)、[排序复跑](evidence/independent-r8-flake-poison-dup.log)、[慢 A 复跑](evidence/independent-r8-flake-slow-a.log) 保留；不抹成最终通过。
- [r9 独立终态](evidence/independent-r9-result.json) completed/pending=false，有限审查 pass；[完整独立 Core](evidence/independent-r9-core-all-rerun.log) 780 PASS / 0 FAIL / 1 ignored，[UI](evidence/independent-r9-ui.log) 64 PASS，[types](evidence/independent-r9-tsc.log)/[lint](evidence/independent-r9-lint.log) EXIT 0。首轮 [不完整后台日志](evidence/independent-r9-core-all.log) 缺 EXIT，仅留作环境记录。两次单命令权限偏离、Cargo.lock 的 pkg-config 派生差异和审查局限见 [响应](evidence/independent-r9-response.json)。
- r9 之后只有 [两处格式变动](evidence/format-final-response.json)，旧独立字节为 [r9 快照](evidence/source-sha256-r9.json)，不伪装为最终字节。Runtime/Memory 未变；[Rust 格式检查](evidence/format-final-rust.log)、[UI 格式检查](evidence/format-final-ui.log) 已通过。bundle.rs 其他三处原格式差异保留，新增 helper 的规范化前后字节一致。
- [r10 独立终态](evidence/independent-r10-result.json) completed/pending=false，有限范围 pass；[21 项前台 UI 运行](evidence/independent-r10-ui-test.log)、[开始 hash](evidence/independent-r10-hash-before.log)/[结束 hash](evidence/independent-r10-hash-after.log) 与 [格式等价](evidence/independent-r10-equivalence.json) 证明最终版本。
- r10 还核实了 600 是候选记录数，920 条新记录/650 条 raw filler 的回归 oracle 有效；local:observation 只记用户本地回答，来源 claim 时间恢复及 A/B 两空位公平与生产实现一致。详细局限见 [最终报告](evidence/independent-r10-report.md)。reason 为可选诊断字符串，未枚举不是 wire 不一致；保留 low 意见，不扩大设计。
- [r10 最终核对委派](evidence/independent-r10-delegation.json) 与 [能力目录](evidence/independent-r10-capabilities.json) 为同模型/300K/high、approval-required、5 分钟，不编译 Rust，不使用沙箱外权限。
- [r9 能力目录](evidence/independent-r9-capabilities.json) 与 [独立委派](evidence/independent-r9-delegation.json) 固定 Cursor / claude-sonnet-5-5 / 300K / high、approval-required、10 分钟原生监督。主 Agent 的日志没有改名为独立日志。
- r3 在预算结束原生取消，r6 failed，r7 interrupted；部分实现均由 owner 接续修复并验收。[store](evidence/implementation-store-result.json)、[runtime](evidence/implementation-runtime-result.json)、[UI](evidence/implementation-ui-result.json)、[hardening](evidence/implementation-hardening-result.json)、[reset r7](evidence/implementation-reset-r7-result.json) 是各轮真实终态，不能单独证明交付完成。

## Actual UI rendering

T3 原生协作预览执行真实生产 React/Vite 组件。预览无法连接本机 loopback，故将编译 JS/CSS 压缩后注入同一原生预览，以显式 fake API 和页内 storage 运行；没有改用其他浏览器，没有称为 UI/Core round trip。标准模式最终新错误列表为空；早期导航拒绝、doctype/Yjs 重载诊断保留在 [渲染记录](evidence/render-results.json)。唯一后续 UI 修改是等价 Unicode 转义大小写，原渲染证据继续适用。

[可复现夹具源码](evidence/render-fixture.zip) 和 [渲染元数据](evidence/render-results.json) 保留。截图均为合成数据，不能证明真实外部账户连接。

- 共享对话：[浅色](evidence/conversation-light.png)、[深色](evidence/conversation-dark.png)、[窄窗](evidence/conversation-narrow.png)。常用入口为 icon，执行诊断及事件来源在更多工具内，无原生 details/summary。
- 记忆：[现有检查面板](evidence/memory-light.png)，精确内容、来源/范围及确认卡沿用。
- 来源与重置：[批准前准确字段](evidence/source-reset-light.png)、[批准后仍等待 adapter](evidence/source-approved-light.png)、[深色](evidence/source-approved-dark.png)、[480px 深色](evidence/source-approved-narrow-dark.png)。批准 reference 不表示游标已前移；同一个 action 保存 source/version/stream/before/after/reason，旧 checkpoint 仍可见。

## Preparation history — not current acceptance

此前文档准备使用 codetwo-develop 与 Ponytail full，唯一事实源/范围/事务已按 Spec 核对。源与测试基线为 [635 文件哈希](evidence/before-source-sha256.json)，实际 [调用路径复核](evidence/preparation.json)。

两轮只读 Cursor 均为 `claude-sonnet-5-5`，明确 300K/high、approval-required、Plan，目录 `/Users/chenli/.t3/worktrees/codeTwo/t3code-6738e658`，使用独立原生 taskId、未向 childThreadId 派新轮。真实目录能力见 [目录回执](evidence/capabilities.json)。r1 10 分钟预算，真实 [委派](evidence/delegation-r1.json) / [终态结果](evidence/result-r1.json)：completed、hasPendingChildRuns=false；静态判定初稿不宜提交，5 P1 和 6 P2，不能称通过。主 Agent 保留 [其审查版本](evidence/reviewed-spec-r1.json)，修正同一 Store 附表、宿主主体/回执入口、checkpoint/毒消息、固定输入与额度、Core 动作 allowlist 等，见 [逐项响应](evidence/response-r1.json)。

r2 5 分钟预算，真实 [委派](evidence/delegation-r2.json) / [终态结果](evidence/result-r2.json)：completed、hasPendingChildRuns=false，静态发现剩余 1 P1 与若干 P2，结论为改正后可交人类确认，不能称最终独立 PASS。[冻结的审查版本](evidence/reviewed-spec-r2.json) hash 与其开始/结束版本一致。主 Agent 在终态后补充 observation/goal 的不同运行身份、提交 allowlist、输出与应用回执同事务、容量出口、元数据/空批/返回状态/游标重置/状态迁移与提交版本校验，见 [r2 响应及最终 Spec hash](evidence/response-r2.json)。最终修订为 owner 文档修补，未再独立复跑。两轮静态检查不覆盖生产 AC，也不构成人类批准。

文档与范围检查见 [收尾日志](evidence/handoff.log)，资源及 635 个源码/测试未变化的真实复核见 [清理/范围报告](evidence/cleanup.json)。准备阶段未运行产品测试、UI 渲染或真实接入，因为没有实施生产变更；原验证不计作本新协议通过。


## Cleanup

Removed: 唯一 task-owned `.codex/run/assistant-event-memory/` 全部删除，含隔离源码/target、编译 React/字体缓存、重试日志与格式旧副本；两个已确认本轮 `/tmp/obs_lane.rs`、`/tmp/obs_tests.rs` 也删除。共 2,899,426,067 逻辑字节。只在核对 PID/cwd/argv 后停止本轮 Vite 68684，14677 空闲；三个本轮预览关闭。
Retained: 两个 change bundle 的四阶段文档、compact 原生任务终态/能力/监督记录、owner 与独立真实日志、原失败、源码 SHA-256/差异、截图及压缩 UI 夹具源码。T3 管理的原截图历史和用户附件不清扫；canonical copies 在 evidence。
Retention owner: codex。
Cleanup trigger: 生命周期证据与本次变化永久保留；无运行 scratch、测试服务或监听器保留。
Processes: r3/r7 interrupted、r6 failed，其余本轮终态已回收；r8 changes-required、r9/r10 pass，均 pending=false。最终实际进程扫描无本轮残留，14677 空闲，预览列表为空。保留 T3/Cursor/用户进程和共享工具链。
Evidence: [第一阶段清理](evidence/cleanup-phase1.json)、[终态清理/哈希/进程报告](evidence/cleanup-final.json)。原 [准备阶段清理](evidence/cleanup.json) 是历史。清理之后运行 [最终 docs/sdlc/diff 检查](evidence/handoff-final.log)，没有重新建立构建。

## Review and release

Approval: 用户当前会话明确确认本 Spec 的身份、持久接收和 observation-only 边界，并授权继续实现统一接入层及 Cursor Sonnet 5.5 300K/high 并行开发。依据为实际用户回复，不以模型建议或记录姓名认证批准。开发范围为本地实现、适用验收和清理。
Rollback: See plan.md；撤回新入口/读取行为时保留既有 Store 数据、观测回执、控制和 unknown attempts，不通过删除事实回退。
Release: 未发布。没有 push/PR/merge/deploy、真实账户监听或外部消息、云端常驻、模型/付费/权限设置变更。
Feedback: 原生 Cursor r7 自动中断/自动续跑后按原累计预算取消；r8 的 16 路压力提议被自动审查拒绝，未执行或绕过。保留这些限制，不声称无人值守委派可靠。

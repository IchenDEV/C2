---
id: 2026-10-05-assistant-long-term-memory
schema: 5
stage: verification
status: passed
owner: codex
created: 2026-10-05
based_on: plan.md
revision: "14c20fce841384dbeaa16c48c7a7264d6e4a9f81 + preserved dirty worktree; ../2026-10-05-assistant-event-ingress/evidence/source-sha256.json"
verification_mode: fresh-context
verified_by: cursor-independent-r9-and-r10
verified_at: 2026-10-06
release_target: none
cleanup_status: complete
---

# Verification: Assistant Long Term Memory

## Verification

下列 PASS 区分 owner 与独立证据。r9 完整独立回归、r10 最终格式及三项定向核对均 completed/pending=false、有限审查 pass。最终 358 hash 无变化，清理完成；没有把实施子任务或静态 review 单独算作验收。

- AC-1: PASS — 24 项 Memory 单元回归包含 Store 重开、legacy origin 仅一次回填、无关键词交集的核心偏好、最多四条确认核心及 L1 总数最多 12。automatic / candidate / inactive / 冲突笔记不会提升为确认核心。见 [完整 Core 日志](../2026-10-05-assistant-event-ingress/evidence/core-owner-r9.log)。
- AC-2: PASS — 旧英文/中文知识在超过 600 条新无关记录后仍可检索；先匹配后限候选，结果及上下文有界，主对话多层检索回归保留。见 [Core 日志](../2026-10-05-assistant-event-ingress/evidence/core-owner-r9.log) 与 [记忆读法契约](../../../reference/memory.md)。检索为现有词法算法，不保证语义改写召回。
- AC-3: PASS — 全局开关、project/session read 和项目隔离仍优先；纠正和遗忘立即影响下一次召回，原 profile/旧笔记不再进入核心。实际 chief Runtime 确认同一 proposal 只写一次、重开后遗忘不会复活；精确原文/来源/范围及 unknown 写入不重放。见 [Core 日志](../2026-10-05-assistant-event-ingress/evidence/core-owner-r9.log)。已经发送的 Provider 上下文不能撤回。
- AC-4: PASS — 生产 intake 及事项审查读取同一个 Memory Store 增强方法；真实 ACP 夹具验证无查询交集仍带确认核心、设置撤销及 exact answer policy。全 Core 780 PASS（603 unit、27 coordination、6 delivery 等），仅 1 既有 live Provider ignored。见 [owner 结果](../2026-10-05-assistant-event-ingress/evidence/owner-results.json)。r9 独立全 Core 已通过，Memory/Runtime 字节未再改变；既有记忆 UI 结构未改，仍复用 [实际记忆面板渲染](../2026-10-05-assistant-event-ingress/evidence/memory-light.png)。

Verdict: verified.
Residual risk: 最终核对及清理完成。r9 源码审查是有限审查，不是完整协议安全审计，单命令权限偏离已入册。没有新增第二个记忆库、embedding 模型、后台付费整理或自动保存外部事实。未验证真实付费模型业务、外部账户、原生打包通知、远程 CI、PR 或发布。旧 notes 的未记录来源无法从新读法反推历史真实性；保留现有来源与首次 migration 规则。

## Evidence and previous attempts

[共享最终结论](../2026-10-05-assistant-event-ingress/evidence/final-results.json) 保留来源/版本、owner 与独立计数、未验证范围及清理。

[实现基线](evidence/implementation-baseline.json)、[原生实施委派](evidence/implementation-delegation.json) 及 [终态](evidence/implementation-result.json) 保留 worker 责任与范围。r1 implementation completed/pending=false，但其当时没有完成运行测试，不能单独视为验收。

[r2 静态复核](evidence/review-r2.json) 提出长期检索和 provenance 约束；owner 修正并完成真实测试。初次 owner 测试 22 PASS / 1 FAIL 揭示 reopen 重复 origin 回填，已改为首次字段迁移才回填，并加入 legacy/reopen 回归；历史失败不是当前通过。

[r8 独立复跑](../2026-10-05-assistant-event-ingress/evidence/independent-r8-result.json) 对冻结的 358 文件三次 hash 无变化、UI 64 PASS、types PASS；事件排序及慢 A 测试有 2 P2，故整体 changes-required，不能用于最终通过。[r8 响应](../2026-10-05-assistant-event-ingress/evidence/r8-response.json) 与 [r9 重新委派](../2026-10-05-assistant-event-ingress/evidence/independent-r9-delegation.json) 区分各轮版本和 owner/独立执行。[r9 完整独立结果](../2026-10-05-assistant-event-ingress/evidence/independent-r9-result.json) 是 pass，真实 Core 780/0、UI 64/0、types/lint EXIT0；[原完整运行日志](../2026-10-05-assistant-event-ingress/evidence/independent-r9-core-all-rerun.log)、[r9 响应及局限](../2026-10-05-assistant-event-ingress/evidence/independent-r9-response.json) 区分测试、只读范围和单次沙箱权限偏离。之后 Memory/Runtime 未变，只改 bundle helper/新 UI test 的格式；[最终格式核对 r10](../2026-10-05-assistant-event-ingress/evidence/independent-r10-delegation.json) 另核对最终版本，已 completed/pending=false，复用等价代码证据；[21 项 UI 与定向 oracle 独立结果](../2026-10-05-assistant-event-ingress/evidence/independent-r10-result.json) 核实 600 是记录数、920 条新记录仍找回旧中英文知识，不是 r9 所误读的字符数。当前 [358 文件 SHA-256](../2026-10-05-assistant-event-ingress/evidence/source-sha256.json) 保留准确源码和 fixture 身份。

## Cleanup

Removed: 两目标共用的 `.codex/run/assistant-event-memory/` 完全移除，含隔离 Cargo/React、测试/格式旧副本、历史重试 scratch；两份已确认本轮 `/tmp` fixture 也删除，共 2,899,426,067 逻辑字节。Vite 68684 在核对所有权后停止，14677 空闲，三个本轮预览关闭。
Retained: 本 bundle 与统一事件 bundle 的四阶段文档、必要 compact 哈希/独立任务终态/日志/原失败、截图和压缩夹具源码。保留用户原始附件和 T3 管理截图历史。
Retention owner: codex。
Cleanup trigger: 生命周期文档与实际验收证据随本变化永久保留；无运行 scratch 或新记忆后台服务。
Processes: memory r1/r2 及最终 r9/r10 已结束、pending=false；最终实际扫描无本轮残留。保留用户/平台进程与共享工具链，没有真实模型业务或外部监听。
Evidence: [第一阶段清理](../2026-10-05-assistant-event-ingress/evidence/cleanup-phase1.json)、[最终清理/哈希/进程清点](../2026-10-05-assistant-event-ingress/evidence/cleanup-final.json)、[最终 docs/sdlc/diff](../2026-10-05-assistant-event-ingress/evidence/handoff-final.log)。没有为文档收尾重建 target。

## Review and release

Approval: 用户当前会话直接要求引入长期记忆，并授权 Cursor Sonnet 5.5 300K/high 并行开发。沿用此前明确确认的精确内容、来源、项目/全局范围、纠正/遗忘及未知写入边界，没有新增记忆确认协议。
Rollback: See plan.md；撤回读法时保留既有用户笔记、历史事实及回执，不删数据。
Release: 未发布。无 push/PR/merge/deploy、模型/付费/权限设置变更或外部消息。
Feedback: 实际 origin/reopen 失败已经收敛为回归；只将真实 owner 与独立结果分别入册，不称为上线或外部产品接通。

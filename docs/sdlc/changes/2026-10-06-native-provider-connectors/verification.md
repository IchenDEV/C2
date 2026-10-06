---
id: 2026-10-06-native-provider-connectors
schema: 5
stage: verification
status: blocked
owner: codex-main
created: 2026-10-06
based_on: plan.md
revision: "14c20fce841384dbeaa16c48c7a7264d6e4a9f81 plus preserved dirty worktree; owner source snapshot fad903affa9afe3e2a05b8f36116ca0592e0d6451bb10e1ad70b38ac1822b7ab"
verification_mode: owner
verified_by: "codex-main (source inspection, official probe cold rerun and isolated checks; final independent acceptance pending)"
verified_at: 2026-10-06
release_target: none
cleanup_status: complete
next_trigger: "Resume the approved I1-I4/I6-I7 tasks after the specified Cursor Sonnet 5.5/300K/high/Full Access connection is restored; no new design approval is needed."
---

# Verification: 原生连接器设计与拆解

## Verification

以下AC尚在实施，不将旧ACP回归称为native通过。

- AC-1: PENDING — Engine出站调用、配置与诊断已走中性Runtime并通过ACP夹具；回调/权限入口仍ACP形状，完整domain合同及native接入待实施。
- AC-2: PENDING — Codex真实AppServer合同与安装版本待验收。
- AC-3: PENDING — Claude SDK/认证/权限/sidecar待验收。
- AC-4: PENDING — CursorBridge/SDK具体路径与能力待probe。
- AC-5: PENDING — OpenCode V1/V2版本/真实server待验收。
- AC-6: PENDING — 生产Engine异步/控制新增后端待验收。
- AC-7: PENDING — unknown/恢复/审批隔离待故障测试。
- AC-8: PENDING — legacy迁移/回滚与不fallback待验收。
- AC-9: PENDING — 当前owner全Core 814通过，workspace/all-targets检查通过；官方原生路径和新增能力UI仍未验收。
- AC-10: PENDING — 独立运行验收/清理在产品阶段执行。

Verdict: BLOCKED — 本轮Engine出站接入与M1修补的owner检查通过；指定Cursor r13实际failed/no pending，SDK错误connection_stalled。四官方连接器、持久原生身份/权限入口及最终独立验收仍未完成。
Residual risk: 合同/版本能力和认证尚未运行验证。SDK账号/API key/费率或许可证前提不足时不默认新增付费。真实模型/多OS打包/云端/外部账号/远程CI未验证；无PR发布部署。

## Design evidence

[SDK research task](evidence/research-task.json) 与 [Engine seam task](evidence/seam-task.json) 使用用户指定Sonnet 5.5 / 300K / high / Full Access、Plan、10分钟预算。两轮均completed/no pending，真实结果见 [SDK result](evidence/research-result.json) / [Engine result](evidence/seam-result.json)。官方研究发现的目录/auth/能力尚未实机探针，不是生产通过。

主Agent按OpenAI Docs及官方原始资料核对 [sources](evidence/official-sources.json)，独立重算/抽查 [T3 source](evidence/t3-source-observations.json)，据此修订OpenCode V1/V2、Cursor steering明确否定与unknown区别、审批缺口和legacy imported ID。

Engine分析的“engine.rs并发被改”未有证据：主Agent重算原dirty baseline全部271文件无变化；将其理解为已有dirty工作树，不登记为他人本轮修改。

SDK调研虽为只读约定，但实际HTTP取源码写了7个/tmp快照；其报告开头“未写文件”与末段不一致，按实际工具记录入册。[research cleanup](evidence/research-cleanup.json)记录已复核并清理11,356,961逻辑字节，未写产品文件/运行SDK/读凭据或自动登录。Cursor平台自动工具缓存属于共享运行记录，未当scratch删除。

具体设计r3使用Full Access/Plan、4分钟单轮预算，累计超时仍运行，原生取消一次后确认interrupted/no pending；[review result](evidence/review-result.json)。没有可接受的独立架构verdict，没有自动重派，不称其通过。所有7轮真实taskId/childThreadId、工作目录、参数、预算与终态见 [supervision](evidence/supervision.json)。

两份草案最终版本见 [SHA-256](evidence/source-sha256.json)。本轮纯文档适用检查见 [checks](evidence/checks.json)，不代替实现/真实后端/独立运行验收；未改生命周期规则与工具，无需重复其Eval。

## Cleanup

Removed: 本次所属验证源副本、Cargo target和临时日志/快照已清理，共2,897,234,885逻辑字节、5,846个文件；必要版本、原始失败、当前检查日志保留在生命周期evidence。
Retained: `.codex/run/native-provider-connectors/validation/sync.py`作为续作隔离复制工具；Atlas/Beacon三个README目录作为已获用户允许的试点脚手架。它们尚不是产品内已运行的Project或Agent。共享Ghostty库、T3记录/缓存与用户数据原样保留。
Retention owner: codex-main.
Cleanup trigger: 下次已授权实施续作时核对；最终功能验收后删除复制工具，M4试点验收或取消时删除/替换README脚手架。
Processes: 7个owner Cargo检查均已真实退出；20个已记录原生子任务终态且无待运行子回合；精确所属compiler/Engine夹具进程审查无残留。未启动所属Core/UI/SDK业务server或监听端口，未停止用户进程。
Evidence: [当前完整检查](evidence/engine-routing-core-confirmed.json)、[源码/环境核对](evidence/engine-routing-checks.json)、[清理](evidence/engine-routing-cleanup.json)、[20个原任务终态](evidence/owned-task-terminal-audit.json).

## Review and release

Approval: 本地调研/实施与Cursor Full Access已授权；用户在当前会话明确确认两份已展示的具体high-risk设计，见Spec的Human design decision。
Rollback: See [plan.md](plan.md)。
Release: No PR, push, merge, deploy, cloud daemon or external message.
Feedback: 源码与工具限制如实入册，不能视为实现验收。

## Implementation checkpoint — 2026-10-06

用户已确认具体设计并要求Cursor实施，解除仅由design pending产生的停点；上文设计阶段结果与限制保留为历史。现在进入实现，AC未验收不称通过。真实worker/版本/检查/清理证据随后补入本记录。

Approved draft version: [exact draft hashes](../2026-10-06-native-provider-connectors/evidence/design-draft-sha256.json)。本轮生产源码版本在实现冻结后重新记录；不覆盖旧草案证据。

## Supervised implementation result — r5

首批三路固定Cursor Sonnet 5.5/300K/high/full-access，实施default，SDK研究plan；[原约定](evidence/implementation-briefs-r5.json)、[taskId/childThreadId/开始时间与10分钟预算](evidence/implementation-tasks-r5.json)。全部在累计预算耗尽后取消一次，并实际确认interrupted/no pending，没有自动重派；[真实终态](evidence/implementation-results-r5.json)。设计确认/开发授权保持有效，当前停点是单轮预算内没有完整交付，不是缺少设计批准。

本轮终态后才观察到activity与checkpoint回写：两路实施留下3个新文件和3个修改文件；此前实时工作树检查未看到这些变化，不能据此推断未调用工具。SDK probe没有交付可验收报告或脚本，不称调研完成。可见记录范围见 [activity](evidence/implementation-activity-r5.json)。共享checkpoint罗列别人的文件，不据此认定该子任务作者。

实际`cargo check --offline -p codetwo-core --no-default-features`在独立task-owned target执行exit101：候选新增未定义ProjectHierarchy/validate_hierarchy/AcpRuntime/RuntimeBlock；另有关闭terminal feature产生的既有错误。保存完整候选后仅撤出这次修改，重跑同命令exit101，新增错误消失，剩4个terminal相关既有错误。两个检查均未通过，不能称default-feature Core回归通过。见 [实际诊断](evidence/partial-checks-r5.json)。未运行新产品tests/真实provider/渲染，没有新产品功能生效。

最终非bundle的271个既有文件SHA-256均与实施前一致，两个bundle以外无新路径；[清理](evidence/implementation-cleanup-r5.json)。本次变更只有已确认生命周期记录及必要候选/监督/检查证据。文档检查结论见 [本轮检查](evidence/implementation-handoff-checks-r5.json)。

## r6 resume — user-authorized 30-minute rounds

用户已允许单轮30分钟，解除r5的预算停点；r5失败、候选撤出与清理保持真实历史。本轮验证重新进行，不将前述文档检查或旧回归当产品通过。用户报告未开fork access；目录未有独立该参数，显式Full Access并核对真实执行。新taskId/childThreadId、开始时间及所有权见 [r6约定](evidence/implementation-briefs-r6.json) / [r6任务](evidence/implementation-tasks-r6.json)；真实终态与源码验收随后补入。

## r6 capability acceptance and r7 foundation retry

r6底座实际failed/no pending，错误“Provider turn failed.”；该run provider终态错误为“[unknown] [canceled] This operation was aborted”。源码基线未改变，不将失败归因于未经证明的fork access。见 [结果](evidence/native-foundation-result-r6.json) / [错误](evidence/native-foundation-provider-error-r6.json)。核对终态与无修改后，只作一轮有界r7重试，原约定/参数/开始时间和taskId见 [重试记录](evidence/native-foundation-retry-r7.json)。

r6官方探针completed/no pending；主Agent审查后移除任意scratch参数、限制清理到所属目录、阻止symlink、限制下载等待，并独立冷复跑39/39，2脚本检查通过，任意scratch负向用例按预期exit1。见 [真实终态](evidence/capability-probes-result-r6.json)、[主Agent验收](evidence/capability-probes-acceptance-r6.json)、[当前报告](../../../../script/provider-sidecars/capability-probes/report.md)。来源/固定版本/脚本hash可追溯；HOME仍允许读取本机CLI配置，不声称完整凭据隔离。没有SDK业务turn、认证或付费调用。

基线默认feature Core all-targets检查exit0、既有生产Runtime测试41通过，见 [检查](evidence/baseline-check-r6.json) / [Runtime基线](evidence/baseline-runtime-r6.json)。这些是新增底座写入前的基线，不能代替新连接器验收。固定Cursor源码表明Bridge缺少steer，SDK的revert_to_followup也可代表ack超时/提交异常；本设计选择唯一TS SDK路径并将该结果保持unknown，禁止自动补发。审批及协调工具限制仍需生产验证，不能由worker Full Access推导产品权限。

## r7 actual candidate failures and r12 completion

r7实际completed/no pending，候选编译成功但Engine仍保留具体ACP client，只使用被移动的helper，不能称中性运行底座已接入。默认feature新integration测试6通过1失败，断线被错误归类为Rejected；[实际检查](evidence/native-tests-r7-owner.json) / [日志](evidence/native-tests-r7-owner.log)。额外原始负向探针0通过2失败：写侧断开而读侧保持打开，请求超过250ms未结算；远端通用RPC错误没有preexecution证明却被归类Rejected。[探针](evidence/native-fault-probe-r7.json) / [源码](evidence/native-fault-probe-r7.rs) / [日志](evidence/native-fault-probe-r7.log)。都是owner检查，不称独立验收。

r12完整原约定及上述失败一并交付Cursor，固定Sonnet 5.5/300K/high/Full Access、default；[taskId/childThreadId/开始时间/30分钟预算](evidence/native-engine-complete-r12.json)。2026-10-06T04:07:58Z平台重启取消ordinal1，随后server自动恢复ordinal2；主Agent读到该真实状态，仍从原04:01:59Z累计到04:31:59Z，未新派任务或扩大权限。终态与源代码验收随后补入。本轮不启动四官方叶实现，须先证明真实Engine接缝和unknown/控制契约。

## Current transport repair and controlled routing round

r12最终failed/no pending，原始错误“Provider turn failed.”，见 [真实终态](evidence/native-engine-result-r12.json)。Root没有把平台续跑取消或failed称为completed，没有活动子任务遗漏。旧Engine client路径仍在，不能称I0完成。

Owner定点修补：reader或writer关闭时，在短pending锁内标记关闭并丢弃等待reply channel，不伪造RPC rejection；watch关闭两侧任务，网络等待不在锁内。排队/关闭同边界，之后请求明确NotSent；泛RPC错误保留Unknown，不自动补发。Runtime诊断拥有中性类型，ACP只在adapter显式转换。此前原探针原文保留，加入实际产品test；新增“reader仍开而writer断开后第二次send不挂起”及诊断不含prompt检查。

真实默认feature全Core检查809通过0失败、exit0，含native adapter 11、M1 18、Core lib603和全部现有integration/doc tests：[当前冻结版本/环境/命令](evidence/root-core-regression.json) / [日志](evidence/root-core-regression.log)。是owner检查，不是独立验收；还没有真实Engine RuntimeHandle路径或四官方SDK接入，不把adapter通过改称native迁移通过。

用户明确fork access即Full Access。r11/r12真实子会话与SDK发送参数核对为Sonnet 5.5/context300k/high；Full Access为T3实际线程模式，旧失败保持真实。当前r13把工作缩小到Engine接入，固定相同参数且实际读回配置，绝对开始04:22:27Z/结束预算04:52:27Z；[原约定、taskId、子会话与配置](evidence/native-engine-routing-r13.json)。共享contract冻结、原子任务均终态后才新派；没有另建团队、换模型或增加产品权限。最终结果与新源验收随后补入。

## r13 terminal and owner Engine integration

r13已按原taskId读取真实终态：failed/result_available、hasPendingChildRuns=false。实际线程是Sonnet 5.5/300K/high/Full Access/default；SDK原始错误为`connection_stalled: Connection stalled repeatedly`，时长457,356ms。不是未开Full Access，也不是模型自然完成。[终态](evidence/native-engine-result-r13.json)、[精确原会话错误和源码对照](evidence/native-engine-provider-error-r13.json)。两份独占文件在任务终态仍与派工前hash一致；这不证明不可见hooks没有副作用。没有再派同配置重试、另建团队或更改权限/付费设置。

主Agent在已批准本地范围内完成共享接缝补救：SessionRuntime、startup/create/revival/switch跟踪全部持有RuntimeHandle；initialize/start/restore/send/steer/stop/options/diagnostics/terminate均通过同一接口。Engine旧restore/replay与model/config解析已从执行路径删除，ACP adapter拥有唯一协议转换。只有回调/permission与public AcpError兼容边界保留，I7继续移出；backend持久字段、Accepted/run引用和迁移仍是I6，未切任何builtin默认后端。Canvas lowering现在返回中性content，唯一Rust matcher同步更新；没有修改React布局或domain JSON。

唯一现有delivery保存回执：确定未发送/明确拒绝为failed，已发但结果不明为unknown；steering仅肯定回执可accepted，缺失或陌生ACP结果保留unknown，不补发。接受不证明采用。发送后的记录失败也保持unknown，没有新增发送器或第二Store。原停止/接管和Memory注入路径保持。

当前默认features全Core实际814通过0失败（40 suites），含Core lib603/既有Runtime41、M1 19、native runtime/ACP adapter 15、协调27、投递6；`cargo check --offline --workspace --all-targets`通过，含NAPI/Server/desktop host。[全日志](evidence/engine-routing-core-confirmed.log)、[结果与时间](evidence/engine-routing-core-confirmed.json)、[检查与确切源码](evidence/engine-routing-checks.json)。三个生产Engine夹具把A卡在创建、执行或补充时，B实际派到后端并完成，持久回执约5.8–6.3ms；A停止进入控制并完成C2活动投影约0.074–6.2ms。日志的Stop terminal仅指C2活动已结算，创建场景没有独立等待OS退出；不将其或Idle当Project旧写入者释放证明。额外缺失steering回执用例保持unknown，三次drain也不重发。这些都是owner检查，只覆盖ACP夹具，不是官方SDK或Project Runtime全链路验收。

源文件307项在测试后逐一核对与当前工作树相符；隔离副本305项与产品字节一致，两处环境例外固定为同版本Ghostty pkg-config的Core manifest及Cargo生成的lock引用。产品Cargo/lock未改，没有新包版本。此前owner记录只说明manifest例外，没有完整记下自动lock解析差异；本次补精确diff/hash和TOML比对，不改称旧测试为独立验收。[环境纠正](evidence/isolation-lock-resolution.json)。267个无关既有文件与原dirty baseline逐字节hash相同。

真正停点：按用户指定Cursor执行的后续I1–I4/I6–I7等待连接恢复；官方认证/兼容、manager零内置工具约束、Project运行与UI还需后续实现和验收。设计/开发授权仍有效，不需要重复批准。最终高风险独立验收、实际付费模型业务、原生桌面通知/打包、远程CI、邮件/飞书或云常驻未验证。没有PR、发布或部署。文档/范围检查见 [收尾检查](evidence/engine-routing-handoff-checks.json)。

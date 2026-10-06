---
id: 2026-10-06-assistant-project-hierarchy
schema: 5
stage: verification
status: blocked
owner: codex-main
created: 2026-10-06
based_on: plan.md
revision: "14c20fce841384dbeaa16c48c7a7264d6e4a9f81 plus preserved dirty worktree; owner source snapshot fad903affa9afe3e2a05b8f36116ca0592e0d6451bb10e1ad70b38ac1822b7ab"
verification_mode: owner
verified_by: "codex-main (source inspection and isolated current-source checks; final independent acceptance pending)"
verified_at: 2026-10-06
release_target: none
cleanup_status: complete
next_trigger: "Resume approved M2/M3 after the specified Cursor connection is restored and the native runtime can enforce manager tool restrictions; M1 is not activated."
---

# Verification: Project 层级设计

## Verification

下列AC进入已批准实施，尚未验收；原设计阶段的分析证据不代替产品运行结果。

- AC-1: PENDING — M1领域19/19 owner测试通过；Project生产路由、控制回执和撤销尚未接入。
- AC-2: PENDING — 路由/对话/唯一目标待实现与幂等验收。
- AC-3: PENDING — 记忆矩阵/策略/来源/删除待真实 receipt 验收。
- AC-4: PENDING — 指令版本/优先级/采用待 Provider 路径验收。
- AC-5: PENDING — 异步/控制/版本待新增真实 ACP Runtime 验收。
- AC-6: PENDING — 额度/释放/排他待层级回归，不能复用旧结果代替。
- AC-7: PENDING — 交接/unknown/崩溃恢复待验收。
- AC-8: PENDING — legacy/回滚待实际迁移验收。
- AC-9: PENDING — 共享 UI 设计已列；本轮无 UI 变化，没有新渲染验收。
- AC-10: PENDING — 本轮文档与设计复核不代替产品独立验收。

Verdict: BLOCKED — M1领域19/19、当前全Core 814和workspace/all-targets的owner检查通过；Project管家Runtime、真实记忆注入/工具限制、写入者释放、复用对话UI及独立验收仍未完成。指定Cursor r13连接停滞，等待执行依赖恢复；没有生产层级激活。
Residual risk: 新增身份、层级权限和范围共享仍为未接入运行路径的候选；在途迁移尚未验收；真实付费模型业务、原生通知/桌面打包、云常驻、邮件/飞书/外部账号及远程 CI 未验证。无 PR、发布或部署。

## Design evidence

- 参数/目录：[capabilities](evidence/capabilities.json)、[research task](evidence/research-task.json)、[Core task](evidence/core-task.json)。
- 官方调研 r1：failed，原始错误 “Provider turn failed.”，无待运行子任务；[result](evidence/research-result.json)、[可见 activity](evidence/research-failure-activity.json)。主 Agent 已直接核对 Spec 链接的三份官方文档；不把 r1 称为通过。
- Core r2：completed，无待运行子任务；[result](evidence/core-result.json)。只读源码分析，无运行验收。建议由主 Agent 按用户具体需求独立判断，不构成人类批准。
- 此前 dirty worktree：[baseline](evidence/preexisting-baseline.json)；收尾重算全部271个文件及HEAD均未变化，两个新设计bundle以外无新路径，见 [worktree handoff](../2026-10-06-native-provider-connectors/evidence/worktree-handoff.json)。
- 具体设计r3因用户指定Full Access而原生取消，终态interrupted/no pending：[record](evidence/review-r3-interrupted.json)。r4已实际使用full-access，但超出8分钟累计预算仍运行，原生取消后确认interrupted/no pending：[record](evidence/review-r4-result.json)。未取得可接受的独立架构verdict，没有自动重派，不称其通过。
- 主Agent复核现有Memory/规则编译和r2分析，选择单AssistantState CAS、明确Project/工作区归属与资源隔离，没有照搬模型提出的“目录只能属于一个业务项目”。补齐收权控制、旧writer guard、legacy fingerprint及后端角色约束。
- 官方事实：[sources](evidence/official-sources.json)。设计和只读可行性分析不代替独立人类设计确认，也不代替实现后独立运行验收。
- 具体草案版本：[SHA-256](../2026-10-06-native-provider-connectors/evidence/source-sha256.json)；本轮纯文档检查见 [checks](../2026-10-06-native-provider-connectors/evidence/checks.json)。没有改动生命周期工具或规则，无需重复其Eval。

## Cleanup

Removed: 本次所属验证源副本、Cargo target和临时日志/快照已清理，共2,897,234,885逻辑字节、5,846个文件；必要版本、原始失败、当前检查日志保留在生命周期evidence。
Retained: `.codex/run/native-provider-connectors/validation/sync.py`作为续作隔离复制工具；Atlas/Beacon三个README目录作为已获用户允许的试点脚手架。它们尚不是产品内已运行的Project或Agent。共享Ghostty库、T3记录/缓存与用户数据原样保留。
Retention owner: codex-main.
Cleanup trigger: 下次已授权实施续作时核对；最终功能验收后删除复制工具，M4试点验收或取消时删除/替换README脚手架。
Processes: 7个owner Cargo检查均已真实退出；20个已记录原生子任务终态且无待运行子回合；精确所属compiler/Engine夹具进程审查无残留。未启动所属Core/UI/SDK业务server或监听端口，未停止用户进程。
Evidence: [领域检查](evidence/domain-current-regression.json)、[完整Core检查](../2026-10-06-native-provider-connectors/evidence/engine-routing-core-confirmed.json)、[清理](../2026-10-06-native-provider-connectors/evidence/engine-routing-cleanup.json)、[原任务终态](../2026-10-06-native-provider-connectors/evidence/owned-task-terminal-audit.json).

## Review and release

Approval: 用户授权本地实施/调研和受监督Cursor，并明确确认两份具体high-risk设计，见Spec的Human design decision。
Rollback: See [plan.md](plan.md)。
Release: 无 PR、发布、部署或云端常驻。
Feedback: 实际 review 发现及处理在本记录补齐，不能代替人类设计决定。

## Implementation checkpoint — 2026-10-06

用户已确认具体设计并要求Cursor实施，解除仅由design pending产生的停点；上文设计阶段结果与限制保留为历史。现在进入实现，AC未验收不称通过。真实worker/版本/检查/清理证据随后补入本记录。

Approved draft version: [exact draft hashes](../2026-10-06-native-provider-connectors/evidence/design-draft-sha256.json)。本轮生产源码版本在实现冻结后重新记录；不覆盖旧草案证据。

## Supervised implementation result — r5

首批三路固定Cursor Sonnet 5.5/300K/high/full-access，实施default，SDK研究plan；[原约定](../2026-10-06-native-provider-connectors/evidence/implementation-briefs-r5.json)、[taskId/childThreadId/开始时间与10分钟预算](../2026-10-06-native-provider-connectors/evidence/implementation-tasks-r5.json)。全部在累计预算耗尽后取消一次，并实际确认interrupted/no pending，没有自动重派；[真实终态](../2026-10-06-native-provider-connectors/evidence/implementation-results-r5.json)。设计确认/开发授权保持有效，当前停点是单轮预算内没有完整交付，不是缺少设计批准。

本轮终态后才观察到activity与checkpoint回写：两路实施留下3个新文件和3个修改文件；此前实时工作树检查未看到这些变化，不能据此推断未调用工具。SDK probe没有交付可验收报告或脚本，不称调研完成。可见记录范围见 [activity](../2026-10-06-native-provider-connectors/evidence/implementation-activity-r5.json)。共享checkpoint罗列别人的文件，不据此认定该子任务作者。

实际`cargo check --offline -p codetwo-core --no-default-features`在独立task-owned target执行exit101：候选新增未定义ProjectHierarchy/validate_hierarchy/AcpRuntime/RuntimeBlock；另有关闭terminal feature产生的既有错误。保存完整候选后仅撤出这次修改，重跑同命令exit101，新增错误消失，剩4个terminal相关既有错误。两个检查均未通过，不能称default-feature Core回归通过。见 [实际诊断](../2026-10-06-native-provider-connectors/evidence/partial-checks-r5.json)。未运行新产品tests/真实provider/渲染，没有新产品功能生效。

最终非bundle的271个既有文件SHA-256均与实施前一致，两个bundle以外无新路径；[清理](../2026-10-06-native-provider-connectors/evidence/implementation-cleanup-r5.json)。本次变更只有已确认生命周期记录及必要候选/监督/检查证据。文档检查结论见 [本轮检查](../2026-10-06-native-provider-connectors/evidence/implementation-handoff-checks-r5.json)。

## r6 resume — user-authorized 30-minute rounds

用户已允许单轮30分钟，解除r5的预算停点；r5失败、候选撤出与清理保持真实历史。本轮验证重新进行，不将前述文档检查或旧回归当产品通过。用户报告未开fork access；目录未有独立该参数，显式Full Access并核对真实执行。新taskId/childThreadId、开始时间及所有权见 [r6约定](../2026-10-06-native-provider-connectors/evidence/implementation-briefs-r6.json) / [r6任务](../2026-10-06-native-provider-connectors/evidence/implementation-tasks-r6.json)；真实终态与源码验收随后补入。

## r6 domain result and supervised repair

r6领域任务实际completed/no pending，返回代码、未运行Cargo，见 [真实终态](evidence/domain-result-r6.json)。主Agent在独立源码副本运行默认feature `cargo test --offline -p codetwo-core --test assistant_project_hierarchy`：编译成功，6通过、1失败，exit101。失败为Enable单位variant接受多余actor字段；新增ProposalState重名警告和没有真实停止回执的ReleaseWriter入口也要求收紧。[命令/源码hash/结果](evidence/domain-tests-r6.json)、[完整日志](evidence/domain-tests-r6.log)。这是owner检查，不称外部独立验收。

r8沿同一批准M1范围修补，保持原模型/权限；不得用用户edit伪造停止证明，不接入未验证角色权限。新taskId、childThreadId、原约定与已知失败见 [修补轮](evidence/domain-repair-r8.json)。旧结果保留，最终验收待修补后重新执行。两层协作/共享UI尚未改变；不把新增API存在称为agent层级功能完成。

### Additional M1 negative probes

主Agent在同一隔离r6源码副本追加原测试运行两组负向探针：未知旧writer同ID更换session、保留旧assignment再追加替代执行，0通过2失败；缓存grant在共享撤销后仍读取、目标Project inject deny被共享绕过，0通过2失败。均exit101、真实命令和原始日志保留：[writer探针](evidence/guard-probe-r6.json) / [源码](evidence/guard-probe-r6.rs) / [日志](evidence/guard-probe-r6.log)，[memory探针](evidence/memory-guard-probe-r6.json) / [源码](evidence/memory-guard-probe-r6.rs) / [日志](evidence/memory-guard-probe-r6.log)。产品测试文件未由主Agent改写；源码例外仅为隔离副本追加探针，不能称外部独立验收。

这些反馈经原生in-flight steer进入同一个r8 run，不启动新轮或延长预算；receipt仅表示消息被steered，处理/采用需由终态源码与重跑证明。r9只读核对M2运行接入：[约定与原始开始时间](evidence/runtime-plan-r9.json)；分析不授予设计批准，不计产品完成。

## M1 repaired by owner — current result

r8实际completed/no pending，但只修补共享读取；主Agent真实复跑12通过3失败（严格Enable及两个旧writer探针），不能采用worker未可追溯的Cargo通过声明。见 [r8 owner检查](evidence/domain-tests-r8-owner.json) / [日志](evidence/domain-tests-r8-owner.log)。r9只读Runtime计划completed，主Agent裁决在Plan内；不是产品通过。r10与一次有界r11重试均failed/no pending，原始错误“Provider turn failed.”，见 [r10](evidence/domain-complete-result-r10.json) / [r11](evidence/domain-retry-result-r11.json)。不继续盲目重试同一Provider失败。

主Agent沿原M1授权修补：Enable改严格空struct variant、层级proposal类型唯一命名、删除可伪造ReleaseWriter及第二份released session事实；CAS保留未知writer的session/assignment/owner，拒绝替代与伪造终态，同时允许停止、接管请求和进展记账；接管标记不解除旧writer保护。业务Project的destination deny在所有context来源之前执行，grant每次重新核对当前映射，executor还绑定owner epoch；未知旧执行不自动迁移。

真实默认feature隔离全Core检查中的M1 suite为18通过0失败、exit0，包含原始四个负向探针及旧writer flags/正常停止进展、同binding换epoch、未知legacy迁移用例：[完整版本/环境/命令](evidence/domain-root-repair.json) / [日志](evidence/domain-root-repair.log)。这是owner验收，不是独立验收。M1交接释放明确不可用，待M2连接同一Engine的实际持久terminal/stop回执；Idle和用户edit均不代替停止证明。没有生产层级激活、任务迁移或UI变化。

同一冻结源码的原功能回归实际通过：Core lib 603、协调27、投递6，exit0，包含生产Runtime既有41项；[回归版本/命令](evidence/domain-legacy-regression.json) / [日志](evidence/domain-legacy-regression.log)。这不覆盖r12后续源码或新native后端。非本次范围266个既有文件与实施前hash一致，无用户进程变更。

最新owner全Core回归809通过0失败，含M1 18和native adapter 11项，版本/环境在 [全Core结果](../2026-10-06-native-provider-connectors/evidence/root-core-regression.json)。r11 SDK实际错误为“[unknown] [canceled] This operation was aborted”，[精确任务过滤](evidence/domain-provider-error-r11.json)。用户已澄清fork access就是Full Access；没有另一项权限要求，也未据此改称旧失败已解决。

## Current pending-creation guard and shared Engine checkpoint

新增持久create意图保护：assignment尚未获得session_id时，原attempt仍可能started/unknown。正常停止与接管可以记账，但不能删原attempt、伪造cancelled/failed或用空session_id改派；仍沿既有attempts、Store CAS和owner/binding/epoch，不增加另一释放事实。当前19项领域用例包含该正向回归。[当前领域结果](evidence/domain-current-regression.json)、[原始suite日志](evidence/domain-current-regression.log)。

当前同版全Core814与workspace/all-targets检查通过，共用Engine出站接口已真实接入，三个慢A/独立B/停止及缺失补充回执不重发夹具通过；[同版检查与限制](../2026-10-06-native-provider-connectors/evidence/engine-routing-checks.json)。只是owner检查，层级未接Runtime；M1实际写入者释放仍不可用，M2须绑定原session/run/attempt的真实terminal或停止证明，单纯Idle和用户编辑均不代替它。全局/Project真实记忆注入、不同角色禁止工具、Project管家会话、共享主对话UI与Atlas/Beacon真实实践均未完成。

r13真实failed/no pending，精确错误`connection_stalled`，已确认Full Access；[原始错误](../2026-10-06-native-provider-connectors/evidence/native-engine-provider-error-r13.json)。指定Cursor执行依赖恢复后按原批准方案继续；没有重复权限确认、切模型或伪造另一fork开关。清理与267个无关既有文件保留已实际核对；[清理](../2026-10-06-native-provider-connectors/evidence/engine-routing-cleanup.json)。本次无UI改动，不增加渲染证据，旧UI截图不能证明M3已实现。[文档/范围收尾](../2026-10-06-native-provider-connectors/evidence/engine-routing-handoff-checks.json)。

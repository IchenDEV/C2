---
id: 2026-10-04-assistant-coordination-loop
schema: 5
stage: verification
status: passed
owner: codex
created: 2026-10-04
based_on: plan.md
revision: "14c20fce plus scoped builtin-chief-of-staff and coordination worktree changes"
verification_mode: pair
verified_by: assistant_independent_review
verified_at: 2026-10-05
release_target: none
cleanup_status: complete
next_trigger: Human review of the local verified diff; no release authorization.
---

# Verification: Complete assistant coordination loop

## Verification

本轮实现并验证跨项目协调闭环。Core 保存目标、委派、版本、消息、问题、变更、交付、审查及通知；Engine/Activity 仍是会话运行事实的唯一来源。普通输入和协调消息共用持久投递，原内存提示队列已删除。新委派通过宿主绑定的 MCP 工具报告事实，不能仅凭最后一轮文字完成目标。历史 protocol=0 保留首版读取路径，新委派只使用 protocol=1。

- AC-1: PASS — 实际 Web Core 中项目 A 提出阻塞语言问题时，B 完成独立交付；用户回答 Chinese 只投递 A。Runtime 双项目测试复跑通过。[体验记录](evidence/experience.json)。
- AC-2: PASS — `supported_provider_accepts_live_delivery_while_an_ordinary_prompt_waits` 经真实 Engine/ACP 传输即时指令，普通队列仍等待；回合切换和幂等测试通过。真实 Codex 补充消息实际采用 queue，Engine accepted 后才取得 worker confirm，并重新 submit。界面区分回执和采用。[Core 输出](evidence/final-core.txt)、[真实回执](evidence/experience.json)。
- AC-3: PASS — 实际 UI 将 A 的合约 v1 改成 v2，取得 stop/续接回执和执行者确认，旧交付保留，新文件实际变为 fixture report v2。Core 拒绝旧版本提交/验收、撤销已记录原生答案，Runtime 在上游文件变化后停止下游，直到最新依赖重新确认；没有第二个活动写入者。[体验记录](evidence/experience.json)、[Core 输出](evidence/final-core.txt)。
- AC-4: PASS — Runtime 对事实代答逐项核对授权范围内的 manual/L1 记忆及答案内容；选择和原生权限仍要求用户。身份伪造、command_id 内容替换、过期原生请求及已撤销答案测试通过。真实 Codex 的选择问题等待实际 UI 回复。[Core 输出](evidence/final-core.txt)、[体验记录](evidence/experience.json)。
- AC-5: PASS — B 的首次交付进入 rework 历史，重新提交后经 UI 验收。Codex 初次 submit 未完成目标，补充确认后再次 submit；父代理独立读取26字节文件/哈希，随后用 UI 验收当前交付。accepted 通知包含版本和证据；点击目标及标为已读未改变交付事实。[体验记录](evidence/experience.json)。
- AC-6: PASS — Store 事务、命令内容比较、单次 claim、submitting 恢复核对/unknown、旧回合 steering 拒绝和旧原生通道过期测试通过。实际隔离 Core 重启保留问题、委派、交付和记忆 id，没有重复派工。重启本身不被当成恢复了 Provider 的旧 oneshot。[Core 输出](evidence/final-core.txt)、[体验记录](evidence/experience.json)。
- AC-7: PASS — UI 勾选记忆后，已落实的 A 变更生成带 change_id 的项目 manual/L1 记忆，重启保留同一 memory_id。既有 Memory Store 的纠正/遗忘/冲突和实际项目回忆测试通过；UI 显示受影响委派并发送单独纠正，消息不冒充执行者已忘记。[Core 输出](evidence/final-core.txt)、[UI 输出](evidence/final-ui.txt)、[记忆记录](evidence/experience.json)。
- AC-8: PASS — 暂停跟进、请求停止、取消和接管分别保留独立控制状态；停止和恢复检查 Engine 事实，观察关联不能获得写权限。Runtime 验证缺失 A 执行者不冻结 B；UI 回归验证独立按钮、回执和失败草稿保留。[Core 输出](evidence/final-core.txt)、[UI 输出](evidence/final-ui.txt)。
- AC-9: PASS — T3 实际渲染 Web Core 的交办、提问/答复、记住变更、补充、返工、验收及站内目标深链。390×844 无横向溢出；浅色/深色使用生产主题设置应用并读取实际样式。长目标卡定位标题。生产桌面 helper 的平台抛错回归通过，事件继续交给 renderer、未读仍保留；OS 权限对话框/展示未实测，详见限制。[体验记录](evidence/experience.json)、[UI 输出](evidence/final-ui.txt)。
- AC-10: PASS — 旧 JSON 默认字段/protocol=0、回执恢复、首版 id/会话/记忆/证据保留和投递表增列事务检查通过。故意失败迁移整体回滚并保留旧行。普通 Engine activity/store/memory、queue/steer 和 server renderer command 白名单回归通过；生产只保留共享 prompt_delivery 路径。[Core 输出](evidence/final-core.txt)、[Server 输出](evidence/final-server-test.txt)。

Verdict: verified.
Residual risk: OS 通知展示与权限拒绝对话框未实测，native 点击直达目标不受当前宿主 API 支持。站内未读/深链是已验证来源。即时补充经过 ACP fixture 的真实 Engine 传输，不能据此保证每个真实 Provider；实际 Codex 使用排队路径。未知网络回执不自动重发，不保证跨进程 exactly-once。Core 离线不执行；额度和记录上限会显式停止新增工作，期限提醒/归档仍不在本版范围。没有用户数据库降级或线上部署证据。

## Checks and actual experience

| Command / inspection | Result |
| --- | --- |
| `CARGO_TARGET_DIR=.codex/run/assistant-coordination/target cargo test -p codetwo-core --lib --test assistant --test prompt_delivery --test engine_activity --test engine_builtin_models --test engine_memory --test engine_store` | 598 passed: 561 lib, 11 assistant, 6 delivery, 20 ordinary Engine. [Output](evidence/final-core.txt) |
| `CARGO_TARGET_DIR=.codex/run/assistant-coordination/target cargo check --workspace` | Core, NAPI, server and desktop host passed. [Output](evidence/final-workspace.txt) |
| `cargo test -p codetwo-server browser_renderer_has_one_bounded_core_capability_set` | 1 passed; Web renderer command set remains bounded. [Output](evidence/final-server-test.txt) |
| `bun test tests/assistant.test.tsx tests/assistantNotification.test.ts tests/missionControlRendered.test.tsx tests/missionControl.test.ts tests/sessionRailRendered.test.tsx` in apps/desktop | 56 passed, 0 failed. [Output](evidence/final-ui.txt) |
| `bun run build:renderer` in apps/desktop | Lint, TypeScript and Vite production build passed. Existing large-chunk warning retained. [Output](evidence/final-build.txt) |
| `bun test script/verify/checks.test.ts script/verify/four-stage.test.ts script/devflow.test.ts` | Active lifecycle Eval: 32 passed. [Output](evidence/final-lifecycle.txt) |
| Actual Core + Web Core + worker stdio MCP | Two disposable Git projects, ACP fixture scenarios and real Codex gpt-6.1-sol canary; parent independently reread all three files and SHA-256. [Evidence](evidence/experience.json) |

真实 Codex 的六工具中 context 为只读工具；confirm、progress、ask、submit 取得实际持久回执。它先结束本轮等待问题回复，采用回复和第二条补充后重交付。canary.txt 内容精确为 `coordination canary passed`，26字节，无换行；SHA-256 为 `65526580f670e6eb5d9d49b0dd57d8c17874ed77aac1d83e3656ca45cff20fa6`。propose_change 的实际 stdio 回执由 ACP fixture 取得；不声称真实 Codex 本轮调用了该工具。

体验测试的人工操作由父代理在一次性测试数据中模拟，不是对用户生产任务的人工批准。真实自然语言 canary 首次建目标后未自动派工，需要一次 UI 复查唤醒；本轮修复 CreateGoal 唤醒指纹，并在 Runtime 的 Request→CreateGoal→Dispatch 回归中验证。fixture 最初缺少自己的 Git 根导致隔离路径构造失败；修复一次性项目后重跑成功。旧失败通知保留为历史，不改成成功。

T3 snapshot 连续返回 PreviewAutomationExecutionError，未改用独立浏览器。click/type/evaluate 可用；本轮提供真实交互、DOM、计算样式、实际后端及布局测量，不提供新的截图。首版记录中的截图仍只证明其当时界面。

独立验证者 assistant_independent_review 完成源码审查和实际复跑：assistant 11、prompt_delivery 6、Runtime 8、assistant UI 10。最终两项局部 UI 修正再复跑 assistant/notification 12 项。无剩余已证实 P1/P2。验证者没有操作现有服务或改产品文件；真实 Codex 最终提交与 UI 验收由父代理验证。[独立复核记录](evidence/independent-review.txt)。

Final handoff gates: `bun script/verify/docs.ts`、`bun script/verify/sdlc.ts --worktree`、`git diff --check` 全部通过。所有受影响 Rust 文件的 `rustfmt --edition 2021 --check --config skip_children=true` 通过。最终核对专用 scratch/dist 不存在、95个记录进程身份不存在、两个测试端口无监听；修改范围与两份 Plan 一致。

## Cleanup

Removed: 本轮 Core PID 25439、Vite PID 83211 已停止，95个根/后代进程身份均不再存在；ports 14673/14674 无监听，tab_3 已关闭。删除3个有归属证据的 disposable Git worktrees及分支、3.9 GB 专用构建/数据根和48 MB renderer dist。先保留无凭据的命令、体验和真实小型产物；包含配对令牌的运行日志随临时根删除。[清理记录](evidence/cleanup.json)。
Retained: 本地源码、测试、两份生命周期记录和所链接的约185 KB小型证据；真实 [canary.txt](evidence/canary.txt)、[项目A产物](evidence/project-a-report.txt)、[项目B产物](evidence/project-b-report.txt) 已保留。共享 node_modules 及 Provider 自有历史/缓存按原所有者政策保留。[重启前身份核对](evidence/restart.json)。
Retention owner: repository owner for deliverables; package/provider owners for shared dependencies and history. 两个外部候选 worktree 无本轮归属证据，保持原所有者控制，未删除；以原仓库/worktree 保留政策为触发条件，详见清理记录。
Cleanup trigger: normal repository/provider retention policy; no retained active automation is authorized.
Processes: Core PID 25439 and Vite PID 83211 stopped through their owned PTY sessions; all 95 recorded root/descendant process identities are absent. Preview tab_3 closed. No user process was stopped.
Evidence: `ps -axo pid,ppid,comm`, per-PID start-time checks, `lsof -nP -iTCP:14673 -iTCP:14674 -sTCP:LISTEN`, exact `git worktree remove --force` / branch deletion in the disposable fixture repositories, and bounded Python existence assertions. [Cleanup inventory](evidence/cleanup.json) records owned identities, preserved artifacts and removal paths.

## Review and release

Approval: 用户在完整设计之后明确要求“继续实现”；Intent 与 Spec 记录本次扩展范围的授权，不用首版批准替代。高风险实现经独立技术复核。
Rollback: 按 Plan 先停止并核对受管理工作、备份静止数据；新增表/历史保留，未知回执不由旧二进制重放。迁移回滚已测试；未降级用户数据库。
Release: 本地未提交修改；没有 push、PR、merge、安装、发布或外部消息。
Feedback: 运行中交流、问题等待、需求版本和交付验收现已具有独立事实。实测暴露的自动续接、依赖确认、原生答案撤销、steer 重试、草稿优先级、长卡定位和通知失败边界均有修复及适用回归。

## Async follow-up boundary

The passed results above describe the earlier implementation only. Supervision reproduced global gate and review stalls. They do not accept independent asynchronous progress; see the [current follow-up](../2026-10-05-assistant-async-progress/verification.md).

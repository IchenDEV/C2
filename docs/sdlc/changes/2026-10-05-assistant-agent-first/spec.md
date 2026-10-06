---
id: 2026-10-05-assistant-agent-first
schema: 5
stage: spec
status: accepted
owner: codex
created: 2026-10-05
based_on: intent.md
design_approved_by: chenli
design_approved_at: 2026-10-05
design_approval_source: "Current conversation: 用户阅读本 Spec 后明确回复 确认设计，继续并行开发（推荐）；问题列出持续沟通、多项目交办、控制、记忆确认及异步/版本/未知结果约束。"
---

# Spec: Assistant Agent First

## Design

本方案已由用户明确确认并要求继续并行开发。实现与运行状态以 Verification 为准。两个 Cursor 只读审查轮已正常 completed；其检查范围、建议与限制见 [规划证据](evidence/planning-review.json)。模型建议不构成批准。

## 产品行为

首页是与同一幕僚的持续沟通入口。用户可以询问进展、讨论取舍、多项目交办、补充背景和调整方向。问题、重要进展、记忆建议与版本绑定交付返回到此入口。详细项目、执行、产物和记忆按需查看，不要求用户维护管理表单。

普通询问或讨论只回复，不创建目标。“盯住 A 的登录修复和 B 的发布准备”可拆成两个有明确范围的交办。含糊的项目或要求只问缺少的信息。原工作已存在时先检查，不能因标题相近自动接管。

明确指向当前受控事项的暂停、停止和优先级调整通过已有操作落实。取消和实质需求变更展示具体对象、影响和版本；明确用户指令是授权来源，含糊的“好的”不能推断成未指明的决定或外部授权。具体权限问题仍走原权限路径，幕僚不得代批。确定性的停止入口一直可用，不排在模型调用后。

优先级影响调度器选择就绪工作和新审查的顺序。同优先级保留等待公平性；它不隐含中断已运行的工作。

## 唯一数据源与方案选择

采用持久用户消息 intake 加已有事项审查。用户看到同一个幕僚；内部可有多个有界、身份绑定的管理尝试。不要把所有工作串进一个忙碌的 Provider 主会话。

| 内容 | 唯一所有者 |
| --- | --- |
| 用户对幕僚的消息、回复关联、解释/路由结果 | 现有 `AssistantState` 的新增对话记录 |
| 目标、要求版本、委派、问题、变更、验收 | 原协调状态；对话只引用其 id |
| 执行、真实活动、权限、网络投递 | 原 Engine、Activity、prompt_delivery 与持久回执 |
| 稳定知识、偏好和决定 | 原 Memory Store；不把聊天另存为知识库 |

原 `request` 路径迁移为 intake 路由的项目交办记录，不保留第二个面向用户的建目标入口。旧记录和未明尝试保持可读并照原规则恢复。运行中的旧执行者仍持有原控制权，迁移不重发提示。

## Core 与 UI 契约

- 新增 `assistant.edit` 的 `say`：客户端稳定 `turn_id`、`content`、可选 `project_paths` 和明确的答复引用。同 id 同参数幂等；同 id 异参拒绝。接收即持久记账，不等待 Provider。
- 对话记录保存作者、时间、原文、`reply_to`、源 run、相关目标/问题/变更/记忆 id 与 intake 处理状态。状态只描述消息处理，不复制执行事实。UI 从同一 snapshot 投影。
- intake 读取授权项目的紧凑状态、最近有界对话和带来源的相关记忆。已处理消息不自动再次解释。讨论回复可以没有动作；跨项目交办路由到项目 Request，再用原流程创建目标。
- 有界 intake 尝试与事项审查分开计数，不占工作执行额度。各条独立消息可分别推进；同事项的迟到决定检查要求版本、用户控制代次和最新用户决定顺序。无关事项变化不撤销仍有效的决定。
- 动作必须引用原用户消息、具体对象和解释时的相关版本；Core 验证后才同事务提交。模型不能通过传入另一个 id 扩张授权。停止、优先级、问题答复和变更复用原实现，不复制控制逻辑。
- 创建、发送、接受与回复记录使用现有持久 attempt 和 receipt；未知结果可见，不自动重放。只有确定的本地提交拒绝允许重新分析最新状态，使用新的 run，不重放旧动作。
- 对话记录有显式容量上限，达到上限报错，不静默删除未处理消息。首轮不增加历史归档或摘要模型调用。

## 冻结的前端 JSON 接口

`say` 输入：`{kind:"say", turn_id:string, content:string, project_paths?:string[], answers_question?:string}`。空/省略的项目提示表示本次已授权受管项目，由 Core 记录当次范围。确认记忆为 `{kind:"confirm_memory", proposal_id:string, content_hash:string}`；拒绝为 `reject_memory`，同样绑定内容 hash。

新增 `state.conversation`，默认空数组。每项为 `{id, created_at, author:"user"|"assistant", content, reply_to:string|null, project_paths:string[], status:"recorded"|"reviewing"|"handled"|"failed"|"unknown", goal_ids:string[], question_ids:string[], error:string|null, source_run_id:string|null}`。状态为 intake 处理状态，执行仍读目标/回执。新增 `state.memory_proposals`，默认空数组；每项为 `{id, turn_id, project_path, category, content, content_hash, state:"proposed"|"confirmed"|"rejected"|"failed", memory_id:string|null, error:string|null}`。

新增字段必须由主 Agent 协调整合，不能各自改名。旧 snapshot 的前端读法允许字段缺省；Core 数据恢复用 serde defaults。

## 记忆

每次解释按项目及全局范围回忆，保留来源。稳定事实和决定可以形成记忆提案；确认界面展示准确内容和范围，并绑定内容版本。用户确认后写入同一 Memory Store，幂等关联源消息与 memory_id。猜测、临时进展、阻塞和模型自述不成为确认事实。确认的项目事实可沿已有代答规则使用；产品取舍和权限仍需用户决定。

纠正、忘记和查看来源复用已有记忆管理。新回忆不使用已忘记内容；不声称撤回已发给 Provider 的文本。跨项目不会将一个项目决定自动提升为全局偏好。

## 主界面

默认显示沟通流和单个输入入口。项目范围是可选提示，不是发送前的必填表单。问题、提案、交付和重要通知按需内联呈现，直接引用原记录并复用现有组件。Stop、接管、原执行会话和真实回执仍可到达。普通消息“已记录”、Provider 接受、执行者确认采用、交付提交和验收通过各有不同文案。

移除首页手动交办/建目标双表单及重复待办导航；保留详情中的编辑和控制。设置、记忆和执行检查是次级入口。不使用原生 `<details>`/`<summary>`，不新增依赖或另一套会话消息传输。

参考：[Cursor Projects](https://cursor.com/docs/agent/projects) 的长期协调、共享背景和反馈入口；[OpenAI Dots](https://openai.com/index/introducing-dots/) 的个人连续性和多项目跟进。只借鉴工作方式，不照搬云端常驻、外部应用或产品权限。

## Acceptance criteria

- [x] AC-1: 普通询问/讨论有持久且关联原消息的回复，不创建目标；刷新、重启保留连续性。
- [x] AC-2: 一句话交办 A/B，拆成两项受授权工作；先检查已有执行，不要求用户先选单个项目，不重复派工。
- [x] AC-3: 运行中补充、明确停止/优先级调整、问题答复和实质变更走原控制/版本路径；含糊确认和权限请求不被推断为批准。
- [x] AC-4: 慢 A 的模型/session/new/steering 不挡 B 或新消息接收；无关 B 更新不废弃 A 的有效决定，同事项新决定使旧动作失效。
- [x] AC-5: 记忆提案按准确内容/范围确认，有源消息；新回忆尊重纠正/遗忘，未确认推测不能用于事实代答。
- [x] AC-6: 接收、领取、创建、发送和回执前后崩溃均保留问题/控制；同 turn_id 不重复，未知结果不自动重放，停止回执前不替换写入者。
- [x] AC-7: 主沟通、问题、交付和详情在实际 React Web/Core 浅色、深色、窄窗口渲染并可操作；发送失败/轮询保留草稿，无原生 details/summary。
- [x] AC-8: 原异步/R1、协调、投递及适用 Core/桌面回归通过；新独立复核检查最终源码和实际产物，owner 测试不冒称独立。

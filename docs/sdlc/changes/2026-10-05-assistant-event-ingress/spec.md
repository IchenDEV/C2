---
id: 2026-10-05-assistant-event-ingress
schema: 5
stage: spec
status: accepted
owner: codex
created: 2026-10-05
based_on: intent.md
design_approved_by: user
design_approved_at: 2026-10-05
design_approval_source: "Current conversation: 用户引用此 Spec 及 observation-only、身份和持久接收边界后明确回复 确认设计，继续实现统一接入层；本地日期 2026-10-06。"
---

# Spec: Assistant Event Ingress

## Design

本协议已由用户在当前会话具体确认；实现及验收状态见 Verification。此前静态复核不替代运行验收。

Ponytail full：复用现有插件宿主、Store、AssistantState、事项审查、Memory Store 与 Engine。采用一条持久 observe 入口，不建 broker、另一套调度器/任务库/记忆库或云端服务。外部输入是反馈，不是用户本人命令。

## 五个边界

| 能力 | 职责及唯一所有者 |
| --- | --- |
| SourceAdapter | 平台插件将邮件、飞书、Webhook、MCP 通知归一化；传输、订阅、来源验证和可选历史补收由它负责。 |
| SourceBinding | 用户在 AssistantState 保存来源身份、资源/发送者过滤、允许项目/事项及绑定版本；默认只授权观察和跟进提案。 |
| Observation | 原 Store 的附表独占不可变输入、幂等键、消费位置及逐事项处理结果；不重复存进 conversation 或整行 AssistantState。 |
| FollowUp | 现有 Runtime 认领相关输入并固定本次集合，按原事项/依赖版本、run、控制权和 attempt 约束推进。 |
| Communication | 沿原对话、问题和通知向用户报告；外部回复由既有 connector messaging 路径承载，第一版不开启自动外发。 |

## 1. 身份与接入

当前 KernelHost 只知道原始 plugin 名，ConnectorEvent 只有经去前缀的 plugin_id；它们不能证明账户或租户，也不能直接作为新来源的唯一身份。宿主在生产实施时增加原始插件名（保留 bundle: 等前缀）、CommandRealm 和当前 bundle 清单中的 connector id。三项是宿主验证的 principal；提供方/account_scope 是适配器自报并由用户在绑定时固定的范围，不称为宿主证明的远端身份。

SourceBinding 保存持久 source_id，绑定上述 principal、provider 和 account_scope；普通重连保留 id，账户/realm/connector 改变不能自动继承，须重新绑定。payload 的 plugin_id、realm、owner 或项目路径不能替代宿主 principal。发送者 id 由已信任适配器从验证后的提供方消息得到，不自动映射为 CodeTwo 用户。

新增宿主专用 JSON-RPC 请求 `observation/record`，返回持久回执；与 peer 的 command/call 并列，由 KernelHost 绑定 principal，不公开 raw assistant.edit，也不将新方法作为任意 extension_public 命令。清单闭合能力表增加 `observations` 并规定此请求的所有权；复用 existing connector command 的读取/消息能力，不再新增 watch/read/reply 等万能操作词。插件未启用、未获信任、connector 未声明 observations 或 principal 不匹配时拒绝。

initialize 的宿主能力显式声明 observations 支持，缺省 false；适配器仅在支持时发 record。新字段采用可缺省的向后兼容读法，旧主机/适配器仍走旧接口，不将 unknown method 反复重试当作运行恢复。不得静默放宽现有插件标准版本兼容检查。

`connector/event` 继续作为唤醒提示，其监听只 notify 后立即返回，不读写 Store；沿用 Runtime 的 2 秒兜底，不增加另一套唤醒调度器。event/emit 没有持久应答，不能据此确认上游消费成功。

SourceAdapter 必须在提交前完成原文读取与归一化，网络始终在宿主事务/全局 gate 外。若只能得到引用，可提交 reference-only Observation，正文为 null、content_origin=reference，Core 不自动 fetch 任意 URL。适配器读取仅限当前来源及用户绑定授权资源；快照读取标明 snapshot，不冒充通知发生时的原文。

请求结构为：

```json
{
  "connector_id": "declared-connector-id",
  "account_scope": "adapter-reported-and-user-pinned-account",
  "stream_id": "stable-within-source",
  "batch_id": "stable-id-for-this-exact-batch",
  "recovery": "resumable",
  "checkpoint_before": "opaque-cursor-or-null",
  "checkpoint_after": "opaque-cursor-or-null",
  "events": [{
    "event_id": "stable-provider-event-id",
    "kind": "message.created",
    "occurred_at": "2026-10-06T00:00:00+08:00",
    "actor_id": "provider-scoped-sender-id",
    "resource_id": "group/thread/resource-id",
    "object_id": "message-or-resource-id",
    "object_version": null,
    "reply_to": null,
    "content": "bounded original text",
    "content_origin": "event",
    "reference": "provider-owned-retrieval-reference"
  }]
}
```

宿主记录 source_id/received_at。null 是实际 null，版本/引用/回复字段可选；object_version 不作字符串排序。请求每批最多 16 项，每条正文最多 16 KiB（UTF-8 字节），超长正文终态 rejected，不静默截断。稳定内容哈希是规范化 JSON 中事件字段的 SHA-256，排除宿主接收时间、临时传输头和签名；集合/可选字段的规范化在实现中冻结并用固定向量测试。

reply_to 仅保留线程关联元数据，不代表答案或回复授权。MCP 等没有事件 id 的来源可用受声明保证的 resource/version/content hash 构成稳定键；没有可靠版本/补收时只标 live_only，不能伪称提供方保证逐事件 exactly-once。

## 2. 单一 Store、持久接收和补收

Store 新增观察附表，分别拥有输入/键、stream checkpoint 与逐事项应用回执；仍用同一连接及事务，不是第二个状态服务。AssistantState 保留用户绑定、目标、要求、控制和原运行事实；Observation 附表只引用它们的 id。接收不递增 assistant_state.revision，也不整份 clone 外部正文；只按 scope 读取本次输入。

幂等键 `(source_id,event_id)`：同键同哈希返回原回执；异哈希不覆盖原文，记录 conflict 元数据并拒绝自动采用。编辑消息使用新 event_id、相同 object_id。去重凭据跨重启保留；不得删除未知或未处理输入来腾空间。

批次回执另以 `(source_id,stream_id,batch_id)` 和规范化请求 hash 保存，同 batch_id 同参数返回同一持久结果，异参拒绝；即使当前 checkpoint 已前进也可回收原结果。只有全部事件/过滤/冲突记录、目标 receipt、批次结果和 checkpoint 在同事务成功后才返回批次 recorded。批次回执纳入上述持久容量限制，未知事务结果先查原批次，不生成新 batch_id 盲重试。

每个 `(source_id,stream_id)` 最多一个在途 record，请求并发不得用模型任务队列承载；同插件全部流最多 4 个在途，低于现有 peer 的 16 callback 保护上限。接收做短本地事务，背压是正常返回值，不靠回调溢出/超时关闭进程。

宿主对 checkpoint 做 CAS：仅当前游标等于 checkpoint_before 时处理本批；同批重试返回既有回执与当前位置，未知/倒序位置返回 out_of_order，适配器从 Store 回执中的已提交位置恢复，不自行取最大时间/id。不透明游标的覆盖语义由适配器声明，不能比较大小；一个批次必须代表从 before 到 after 的完整覆盖。

本批所有输入（含确定重复或终态过滤/拒绝记录）与 checkpoint_after 同事务提交后，宿主返回 recorded，适配器才确认上游/提交消费位置。一个可恢复的毒事件采用已记录的终态 rejected/filtered 及可检查原因，允许游标越过；认证/身份不符不属于可承认消费，本请求拒绝且不推进。背压或事务失败不提交整批、也不推进 checkpoint，不能部分成功后跳过剩余输入。

结果结构固定为 `{status,batch_id,current_checkpoint,event_receipts,retry_after_ms}`，状态含 recorded、noop、backpressure、out_of_order、unsupported、rejected、needs_reset。只有显式 recorded 是持久 ACK；noop 仅用于无状态空轮询。结构化拒绝表示该批没有提交；超时、断连或无明确提交保证的错误是未知，仅用原 batch_id/参数回收结果。不能将通用 JSON-RPC 错误字符串当作可重放的执行失败；能力缺省 unsupported 时不发送新请求。

空批 events=[] 且 before=after 为无状态 noop，不新增回执；空批推进游标则是适配器声明完整覆盖的 attested-empty，写批次回执、游标与计数后 recorded。宿主无法独立验证远端覆盖完整性，该声明是明确的适配器信任假设。

recovery 为 resumable 或 live_only。resumable 必须有提供方覆盖和历史读取保证；resume 使用 Store 当前位置及上游历史，再走同一 record 入口。自动确认且无补收的来源只能标 live_only；断线、背压等缺口写入有保留空间的固定 source health/gap 计数和范围，不声称零丢失。Core 未运行时实时处理不可用，能否恢复取决于来源能力；不引入常驻云端。

游标失效返回 needs_reset 和提供方原因，不无限重试 out_of_order。用户在原沟通入口确认具体来源/旧游标/新基线及缺口后，可提交带 reset 标记和批准引用的 record；仍 CAS 旧位置，并同事务记录缺口和新基线。reset 不改变 principal/account_scope，不删幂等键/未知 attempt，不重开终结事项，也不等同于批准遗漏区间执行。

## 3. 绑定、范围与容量

Binding 固定 principal/account_scope、resource_filter/actor_filter、project_paths/goal_ids 和 version。项目必须在当前幕僚授权范围内；适配器不决定授权目标。清晰关联在绑定内路由；绑定内但事项不明的输入保持 needs_attention 并提出引用事件的问题。一个事件最多 fan-out 30 个事项，只存一份原文，每事项独立 receipt；A 已处理不代表 B 已处理。

未绑定或过滤之外的流量不进入正文收件池：仅计数并在小型限额内保存 id/hash/原因/来源引用，提示 needs_binding，不向模型供应内容。撤销或改变绑定、停用连接使相关未提交决定失效；重新授权不能自动放大历史输入的已记录项目范围。

第一版暂定：未终结输入全局 256、每来源 64、每绑定 32；全局目标回执 7,680；每批 16；死信/缺口摘要独立保留 128 名额，超额聚合为每来源的固定计数，不能挤占用户 conversation/控制接收。持久去重凭据上限 10,000、单来源 2,000；达限对相应来源明确 backpressure/attention，不能让整个 Runtime 写全局 attention 停摆。终结正文可无损压缩，不自动删键/hash；第一版不实现无限归档或静默淘汰。

每来源最多 8 个用户配置 stream，批次回执同样最多 10,000/单来源 2,000，health 每配置来源仅 1 行；未绑定流量按宿主 principal 聚合，不按适配器自报的 account_scope/stream 字符串建无限新行。过滤消息的 recorded 意味已 ACK 且无正文；之后新增绑定不能取回被忽略的正文，只有来源保留历史且用户明确要求重读时才能补收，不能悄悄扩大旧输入权限。

历史容量到限是可操作的 needs_capacity：向用户提出准确来源、当前占用、提高来源/全局历史限额的具体值与影响；用户通过带 Binding.version 的配置决定提高限额后继续接收。不会静默永久等待，也不自动删除或回放旧键。第一版采用这种显式维护出口，不承诺无限无人维护；自动安全归档属于另一次明确设计。

配额只约束外部输入，且短事务/批量上限须用输入洪峰下确定性停止延迟验证。来源容量、模型 turns/history 上限写入逐来源/逐事项 deferred(reason) 和用户通知，不抛成全局 tick failure。一个吵闹来源不能独占自动审查预算：默认每来源每事项 2 秒合并窗口，最多每 30 秒一轮，另受原全局 turn_limit/concurrency 约束；未处理输入保留，控制和用户对话不受该节流。

## 4. 固定审查输入与允许动作

沿用现有 scope run，不创建新的外部 agent 或审查队列。认领时将 input_observation_ids 与原文 hash、Binding.version 固定到持久 ScopedReview。reviewing 从这个原 run 推导，不存第二份运行状态。指纹只包含本次固定输入及相关事项/依赖事实，不包含 receipt 状态；处理中 arriving 事件留下一轮，不能使当前审查自己作废自己。用户新要求、绑定撤销、相关控制和依赖版本仍使旧决定失效。

运行身份使用明确的 `(kind,goal_id)` 键：普通事项 kind=goal、观察审查 kind=observation，两个键可在同一 Runtime/jobs/scoped_runs 内独立推进；旧记录缺省 kind=goal。观察 run 持久 external=true，提交依据这个原 run 标记执行 allowlist，模型不能自行切换标记。这样不会因观察模式限制普通事项的既有调度权限，也不会让外部原文进入持有派工权限的同一 run。

外部输入的提交动作实行 Core allowlist，不能只靠 prompt：只可产生带“来源声明”标记的状态说明、ask、propose_change 和面向用户的通知；不能改要求/优先级/控制/依赖、创建新目标、派新执行者、自动 accept、写确认记忆、答权限或外发。任意 dispatch、向执行者 communicate/steering 都须有另一个明确用户决定并复用原 user intake/控制路径。本版本的主动跟进是复查、提问和提案；已在运行的工作继续其原授权，不由外部正文产生新执行指令。未来需要基于反馈自动续派时，另确认绑定的动作授权，不在 observation-only 中暗含。

来源的“完成”保留为报告，不自动变成已验收事实。外部原文在审查上下文中明确为不可信输入；模型引用 observation_id 不赋予用户权限。说明和提案的来源引用跨后续处理保留，不能用一条中间 Update 将未确认外部指令洗成用户决定。用户采纳提案后，新的 user 输入成为权限来源，原 Observation 继续只作证据。

外部状态说明写入带来源的观察结果，不修改普通事项的要求/next_step/控制事实。普通 goal prompt 不包含 Observation 正文或其未确认自动总结，仅可有输入引用和处理状态；其他提示词中的外部说明/提案只以不可信引用块呈现，不能作为用户授权依据。用户采纳后才按原 User 输入/版本路径进入后续执行指令。

应用回执的迁移、问题/提案/对话或通知输出、run 完成及 attempt 结果通过同一 Store 事务提交；沿用 save_assistant_with_receipt 的同事务先例，不能先 handled 后写问题。每个输出绑定原 input refs/run 和稳定输出 id，提交重试只能回收相同结果。接收阶段不改用户 revision，真正产生用户可见协调输出时才按原 CAS 合并；失败保留原输入及运行记录。

提交再次校验当前 Binding.version、enabled、授权范围、目标/依赖版本、user_gen、控制代次与原 run；不匹配 invalidated，不因旧绑定留下的输入放大权限。若同 object_id 出现可确定的更新/撤回，相关旧提案标 superseded；仅有不透明乱序版本则 needs_attention，不能按接收时间推断最新版本或将旧声明提升为现态。

Active 且符合原审查条件的事项按以上规则处理；Paused/已完成/未提交/需要关注事项不自动重开或新派工，回执标 deferred_goal_state 并通知用户。全局 settings.enabled=false 为 deferred_paused，global attention 为 deferred_attention；没有 settings/授权项目为 needs_configuration。observe 只在已启用、已信任且已经配置的 SourceBinding 下接收，幕僚暂停不等于停用渠道；恢复后按资格推进，终结输入不会自动重开事项。确定性停止/接管仍走原路径，不等慢收件或模型。

领取、创建、发送前先持久保存本次 input refs/run/attempt。创建、Provider 接受、执行者采用和最后验收分别读原 Engine/回执。崩溃后的未知尝试保留 unknown，不自动重发；只有确定本地版本拒绝可使用新 run 分析。额度释放仍推进等待事项，旧写入者停止回执前不得派替代。

逐事项回执的状态和容量如下；reviewing 只是原 run 的投影。

| 状态 | 计入未终结容量 | 后续触发 |
| --- | --- | --- |
| recorded / reviewing | 是 | 资格满足时认领或等待原 run 提交 |
| deferred_paused / deferred_attention / deferred_budget | 是 | 暂停恢复、attention 解决或相应预算/资源释放后按原输入资格推进 |
| needs_configuration / needs_attention / invalidated | 是 | 明确用户配置/答复或重新校验；权限只可缩小，不能自动重放副作用 |
| unknown | 是且不可淘汰 | 核对原 attempt/真实回执；不能自动启动替代执行 |
| deferred_goal_state | 否，当前关联终结 | 事项暂停/完成/未提交时只通知；重新打开必须是新用户决定和显式重新关联，不自动派工 |
| handled / rejected / filtered / superseded / revoked | 否，终结 | 保留键/hash/结果，不自动重新处理 |

Observation 仅在其所有目标应用都终结且没有 unknown run/attempt 时允许无损压缩正文；零目标的过滤/拒绝条目是终结元数据记录。needs_capacity 是 source health 状态，不挤占正文池或创建无限通知。

## 5. 对话、记忆与可选外部回复

向用户的问题/通知沿原幕僚沟通入口，并以 observation_id 引用原输入；不把外部正文复制成 TurnAuthor::User，不建外部看板。开发者检查可查 principal、Binding、scope、input refs、run 和 receipt，默认界面仍只对话。

长期记忆仍由 Memory Store 独占；从反馈提取的候选必须展示准确内容、范围和来源，经用户确认再写入。原纠正/遗忘约束继续生效。

第一版不新增 CommunicationIntent 表或 reply/delivery_lookup 万能接口、不发送外部消息。外部回复的长期扩展边界为既有 connector messaging：目的地从原 Observation + Binding 得到，用户批准须绑定准确内容/hash、目的地、连接及 Binding.version，发送前重新校验；不可取模型任意收件人。实际外发持久 attempt 的设计和平台幂等/查询能力须另行确认验收；未知结果不盲目重发。Engine 仍是唯一执行者 prompt 投递者，本接入层不增加第二执行写入者。

## 实施前的最小人类决定

确认本版 observation-only 边界及上述持久接收/身份设计：外部反馈可在绑定项目/事项内触发复查、提问和提案；不能直接成为新的交办、控制、派工/执行补充、已验收事实、确认记忆或外发授权。已有执行继续原授权，暂停/版本/未知结果规则不变。本决定已在 frontmatter 记录；用户明确确认此边界，不能由模型扩大。

## 现有契约依据

[插件协议](../../../reference/plugin-protocol.md#events)、[连接器能力及操作](../../../reference/plugin-standard.md)、[原幕僚设计](../2026-10-05-assistant-agent-first/spec.md)、[原异步与 R1 验收](../2026-10-05-assistant-async-progress/verification.md)。这些证据只覆盖旧实现，不是新外部协议的运行验收。静态 [r1 报告](evidence/result-r1.json) 指出的缺口和本轮响应分别保留。

## Acceptance criteria

- [x] AC-1: 四种来源遵守同一 record/capability/receipt 契约；保留前缀/realm/清单 owner，未绑定账户或 payload 冒充不能进入授权路径。
- [x] AC-2: 同事件幂等、异内容冲突、Binding 版本和多事项独立回执通过；正文只存一份，歧义可见且不扩大范围。
- [x] AC-3: 接收/领取/发送/回执边界崩溃可恢复；批次/游标 CAS 不越过未记录输入，毒事件可见且不卡死流，live_only 缺口明确，未知结果不重放。
- [x] AC-4: 慢 A 不挡 B 接收与原已授权派工/回执；固定输入的独立审查不自我废弃，相关要求/绑定/接管使旧决定失效，额度释放推进，停止回执前不替代写入者。
- [x] AC-5: 接收不改变用户状态 revision；洪峰/死信不挤占用户输入/停止，明确容量及来源公平；各种暂停/attention/终态有正确 deferred，恢复不重派未知执行。
- [x] AC-6: Core allowlist 阻止外部触发派工、执行补充、控制、验收、记忆、权限或外发；已运行工作保持原授权，来源声明不经中间说明洗成批准。
- [x] AC-7: 生产 Core 协议夹具、适用原回归及实际共享对话/开发者回执渲染通过；真实账户、付费模型业务、原生通知、远程 CI 及外部平台验收分别记录。

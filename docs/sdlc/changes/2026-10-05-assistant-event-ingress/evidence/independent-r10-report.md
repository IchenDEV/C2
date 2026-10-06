# independent-r10 终态核对报告：pass（有限范围）

时间：2026-10-05T19:26:59Z – 19:28:17Z，单轮，无委派，未提权，未用 `&`。

## 1. 源 hash 与格式等价
- 开始/结束都重算 `evidence/source-sha256.json` 的 358 个 live 文件：mismatch = `[]`（`hash-before.log`、`hash-after.log`）。
- r9 → final 快照只有两个路径不同：`crates/core/src/plugins/bundle.rs`、`apps/desktop/tests/assistantEventIngress.test.tsx`。
- bundle.rs diff（`bundle.diff`）：仅 `observation_connector_ids` 的 `.filter(|c| ...)` closure 换行，无其他变化。
- bundle.rs 规范化：对 `format-before` 旧件与现件分别 `rustfmt --edition 2021 --emit stdout`，去掉首行路径 header 后 `cmp` 相同，sha256 均为 `9812be81…1576`（`old.fmt`/`new.fmt`）。
- UI 测试 diff（`ui.diff`，27 行）：两处 oxfmt 换行（`resetButton` 的 find、`Keep my draft…` 的 expect/find）。去掉全部空格和换行后两文件内容相同（`ui-wsstrip.log` exit 0）。没有改断言、oracle 或测试逻辑。
- 现件 sha：bundle.rs `06f1b14f…ea1d`，UI 测试 `659c0ddf…7881`。

## 2. UI 命令
- `cd apps/desktop && bun test tests/assistantEventIngress.test.tsx`，前台运行，**EXIT=0，21 pass / 0 fail，1888 expect()**，30.83s（`ui-test.log`）。
- 复用 r9 的证据：独立全 Core 38 汇总 780 pass / 0 fail / 1 live provider ignored，其余 UI 43 项，tsc 和 lint EXIT0。这两个文件的改动只是格式，所以复用成立。本轮没有重新编译 Rust，没有跑别的测试，没有跑付费业务。

## 3. 三项定向 oracle（只覆盖这些路径）
1. **记忆候选窗口**（`memory.rs`）：
   - `SEARCH_CANDIDATE_LIMIT=600` 是记录条数，不是字符数。
   - 搜索在候选 LIMIT 之前先做 SQL 词项预筛（`any_content`）和按匹配分排序，再按层取 L1 400 / L2 150 / L3 50。raw 转录同理，`LIMIT 600`。
   - 回归测试把目标 note 的 `updated_at` 回填到 1000，再插入 `SEARCH_CANDIDATE_LIMIT+120`（720）条 L1 和 200 条 L2 的较新 filler（总计 920 条，超过 600 条上限），另有 650 条无关 raw 文本。
   - 断言英文和中文旧 note 仍排第一，chief context 命中 L1，且 `AUTO_L1_LIMIT` 不被突破。
   - r9 把它读成 600 字符，这个读法是错的；oracle 有效。
   - 说明：filler 不含查询词，它能挡住旧的“先按新旧截断再过滤”的实现，不能单独证明 L1/L2/L3 的比例分配。
2. **`local:observation`**（`assistant.rs` 545–602）：
   - `observation:` 作者的问题且无 `request_id`，只在本地完成。
   - 要求 `author=="user"`、无 `source_input`、`contract_revision` 匹配、goal 未取消或完成、项目仍在设置里。
   - 只设 `answer_delivery_id="local:observation"`，不调用 add_message，也没有 worker 投递。外部反馈需要走用户显式执行指令。注释和代码一致。
3. **Runtime 公平性**（`plugins/assistant.rs`）：
   - 每次 claim 之后，`groups` 按 `(source_claims, goal_claims, 最小 seq)` 重新排序。
   - claim 会写入 `goal_claims` 和 `source_claims`（`now_ms`）。`obs_claim_times` 从 `claimed_at:` 持久化记录恢复，所以重启后保持。
   - 测试 `observation_source_fairness_gives_two_free_slots_to_distinct_sources` 的设置：来源 A 有 3 个更旧的 goal，来源 B 有 1 个较新的 goal，concurrency=2。
   - 如果按纯 seq 排序，会选 A、A；现在的排序下第二个空位给 B。
   - 断言 running=2、来源集合恰好是 {A,B}，且 claim 时间在 JSON 往返后相等且 >0。测试与生产排序对应。

## 4. reason 文档
- `reason` 在 wire 契约里是 Optional<String>，是诊断用的附加字段（plugin-protocol.md：“additive `reason` when needed”）。代码里是 `Option<String>`，值有 `malformed`、`body_too_large`、`filtered`、`different_content_for_same_event`、provider 自己提供的 cursor reason 等。
- 文档没有把 reason 描述为封闭枚举，也没有承诺枚举，所以没有实质性文档不一致。reason 值未枚举是 low 级别意见，保留，不要求改。
- `evidence/render-results.json` 已记录真实 UI 渲染（`executor`、`browser`、`transport` 字段）和 synthetic API 边界，我没有核对 UI 渲染本身，只确认文件存在和字段名。本轮没开浏览器或桌面。

## 5. 局限与偏离
- 只覆盖上述有限路径，不是完整协议或安全审计。外部账号、原生通知、真实付费模型、CI/发布仍未验证。
- 最终字节的 Core/UI 全套测试没有重跑，是通过格式等价论证复用 r9 的结果。
- `bundle.rs` 里原有 import 和 scene-validator 三处旧格式债，rustfmt 对整文件仍有输出差异，但新旧一致，按约定保留。
- 无偏离：没有提权、没有后台命令、没有改源码或文档（只写本目录）。沙箱不允许 `ps`，所以进程检查只能说明：本轮所有命令前台运行且已结束，没有启动后台进程。
- 本报告不构成新设计批准，也不构成发布批准。

日志：`.codex/run/assistant-event-memory/independent-r10/`
`hash-before.log`、`hash-after.log`、`bundle.diff`、`ui.diff`、`old.fmt`/`new.fmt`、`ui-wsstrip.log`、`ui-test.log`、`report.md`

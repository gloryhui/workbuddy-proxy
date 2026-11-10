# 用量与费用：从 usage 到请求成本快照

本篇接着 [统计持久化导读](STATS_PERSISTENCE.md)，面向 Java / Spring Boot 开发者解释用量字段、价格选择、整数金额、结算状态，以及设置表单怎样把价格传到核心。这里的费用是本机按价格快照计算的估算值；供应商真实账单不是这条链路的输入。

## 1. 先看数据经过哪些对象

```text
Chat 直通响应                         原生协议响应
  Collector / UsageAccumulator          协议 Decoder 归一 usage
              \                        /
               RequestGuard / UsageOutcome
                          |
                     FinishRecord
                          |
Start 已保存的 PriceSnapshot + 当前请求的用量
                          |
             finish_request -> pricing::settle
                          |
               Settlement { status, parts? }
                          |
             usage_requests + usage_costs
```

对应 Spring 的思路：collector / decoder 像外部接口适配器，`UsageOutcome` 像经过校验的领域 DTO，`pricing::settle` 像纯计算 Service，`store` 则负责事务和持久化。没有在价格计算函数里发 HTTP、查最新价格或操作界面。

阅读源码可按 [`collector.rs`](../../src/stats/collector.rs) → [`event.rs`](../../src/stats/event.rs) → [`store.rs`](../../src/stats/store.rs) → [`pricing.rs`](../../src/stats/pricing.rs) 的顺序。原生协议的归一细节见 [响应转换导读](PROTOCOL_RESPONSE.md)；不要把 Chat 累积器的字段规则直接套在所有原生协议上。

## 2. 用量：哪些字段是总量，哪些只是其中的一部分

[`UsageNumbers`](../../src/stats/collector.rs) 用 `Option<i64>` 保存每个字段。`None` 是未知，`Some(0)` 是已经确认的零，不能使用 Java 中 `null → 0` 的默认转换习惯。

| 字段 | 业务含义 | 计算时的注意点 |
| --- | --- | --- |
| `input` | 归一输入总量 | 缓存读、缓存写通常不能再无条件加到它上面 |
| `output` | 归一输出总量 | reasoning 可能已经包含其中 |
| `total` | 输入和输出的合计 | 只有两个可信父字段都存在时才能推导 |
| `cache_read` | 缓存读取部分 | `read_subset` 证明它属于 input；未知不能猜为 0 |
| `cache_write` | 缓存写入部分 | 单独出现不能证明读、写、非缓存三者互斥 |
| `reasoning` | 推理 token 明细 | 不是额外再计一遍的输出量，本计算器没有单独推理单价 |
| `uncached` | 已证明的非缓存输入量 | 位于 `UsageOutcome`，与 `InputBreakdown` 一起说明拆分证据 |

`InputBreakdown` 记录的是“字段之间的关系能证明到哪一步”，不是简单的页面分类：

- `unknown`：不能可靠拆分；
- `read_subset`：已知缓存读是输入的子集；
- `disjoint`：已有足够证据，非缓存输入、缓存读、缓存写互斥。

Chat 累积器当前只能给出 `read_subset / unknown`，不自行推断 disjoint；例如看到 `cache_write_tokens` 并不足以推导三部分互斥。原生协议适配器可按其真实字段关系给出其他归一结果。Anthropic 的输入归一会涉及非缓存输入、缓存读和缓存创建，缺失分量必须保留未知，不能当作零补齐。

请求 `outcome`、`usage_status` 和 `cost.status` 是三组状态。HTTP 200 或请求 completed 不保证 usage 完整；客户端取消前也可能已经观察到完整 usage。费用计算不依据“请求成功了就一定有用量”这样的假设。

## 3. Chat 的 UsageAccumulator 怎样合并重复事件

流里可能出现多个 usage 对象。它们是累计结果或互补字段，不是待相加的消费账单。

`UsageAccumulator::observe` 做三件事：

1. 只读白名单字段，确认非负整数、大小上限和父字段一致性。JSON 字符串 `"10"`、负数、小数等不是合法 token 整数；缺失或 null 仍是 None。
2. 新字段可以补齐旧快照的 None，已有可信值不被后来的冲突值静默覆盖。重复相同事件不会让总量翻倍。
3. 把冲突记在状态里，由 `finalize` 给出结果。父字段冲突使整体 invalid；可选明细冲突保留父字段，记录明细异常。

例如先收到 `input=1000`，后收到 `output=100`，可以形成 `total=1100`；如果后续声称 `input=1200`，这不是再加 1200，也不是直接用新值替换旧值，而是父字段冲突。

可选子字段的处理更细：负数、类型错误等会被拒绝；单次事件中大于已知父量的明细会被过滤。跨事件两个已知明细冲突时，当前实现保留先前值并标记 `subfield_invalid`。不要把它写成“发生冲突后一律把这个字段清空”。父用量仍可能 complete，reason 则指出 `invalid_numbers`。

`finalize` 的优先级为：父字段已 invalid → invalid；没有候选 → missing；两项父用量都已知 → complete；否则 partial。它会在父量齐全时补 total，但不会用一个 total 反推缺失的另一项父量。

之后 guard 的请求级规则还会调整结果：明确未派发上游时记为 not_applicable；中断且没有完整用量时不能伪造完成数据。`event::decompose` 对 partial 不写推测的 total，对 invalid 不写互相矛盾的数值。数据库看到的不是未经验证的原始 JSON。

## 4. 钱的单位：为什么输入 2，API 却传 2000000000

[`dto::parse_human_amount`](../../src/stats/dto.rs) 和 [`pricing::parse_rate`](../../src/stats/pricing.rs) 处理不同层的输入：

| 层 | 示例 | 含义 |
| --- | --- | --- |
| 设置表单 | `"2"` | 每百万 token 收取 2 个币种单位 |
| 管理 API 单价 | `"2000000000"` | 每百万 token 收取 20 亿个 nano 币种单位 |
| SQLite 单价 | `2000000000`，INTEGER | 同一个 nano 单价 |
| 请求费用 | `2000000` nano | 1000 token 按上述输入单价计算，等于 0.002 个币种单位 |

1 个币种单位 = 1,000,000,000 nano。**单价还带“每百万 token”这个分母，费用不带它。**

Java 类比可以是用 `BigDecimal` 表达外部十进制，再按明确尺度转换成整数。本项目直接解析十进制字符，用整数缩放，避免让费用经过 `f64`。两种入口的约束不同：

- 表单解析最多接受 9 位小数，以补零方式转 nano；不支持科学计数法或符号，不把超过精度的小数偷偷舍入。
- 核心 `parse_rate` 接受 nano 单价的纯非负整数字符串，并限制不超过 `MAX_RATE_NANOS=10^15`；它不会再次乘 10^9。
- 公开目录的 JSON 数字由 [`price_catalog::rate`](../../src/price_catalog.rs) 单独处理，可解析指数表示；缩放到 nano 时有自己的舍入。不要把目录数字的入口规则搬到管理 API 上。

输出同样有边界：金额 DTO 使用十进制字符串传输，避免整数大小被 JSON 消费端的数字精度限制。`format_amount_text` 用 i128 解析并格式化，太小的非零值可显示 `<0.0001`；这是一种展示方式，不会把存储金额改为 0。整额可显示 `"0"` 或 `"2"`，不强制补四个小数位。

## 5. 价格从哪里来，怎样匹配到请求

请求热路径 [`snapshot_for`](../../src/stats/store.rs) 只用 `(provider_id, upstream_model)` 精确查当前已发布 cache。这不是按客户端 alias、响应 model 或请求开始时间去选择生效区间。价格表为每对键保留一条当前价格。

`StatsHandle::begin` 在准入时取得快照，Start 将它写入请求自己的 `usage_costs` 行；Finish 从该行结算。`source_price_id / revision` 仅用于追溯，不需要当前价格记录仍存在；修改、删除价格不改历史，也不改已经准入的在途请求。

`price_snapshot_at_ms` 是本请求取得价格的时刻，不是价格配置修改时间。请求里携带的是小快照，不是每次复制整张价格表。`load_price_cache` 会读取当前价格表全部条目，缓存仍由 writer 在设置变化后重建并发布。

没有匹配价格时，系统构造 `price_source=default_zero` 的快照：四项单价明确为零，币种取当时默认币种。它不是“不知道价格但假装算到了供应商账单”。以后补价格也不会重新计算这些历史记录。

公开目录的查找是另一层逻辑：[`Prices::find`](../../src/price_catalog.rs) 先找模型名、canonical 或带 provider 前缀的候选，再优先核对供应商 API endpoint；必要时尝试唯一 canonical 的原厂记录、显式原厂前缀，最后检查候选价格是否一致。多条候选不一致时返回 None，不随便挑一条经销商价格。

目录匹配完成后，价格导入当前本地配置，才供热路径按精确键查找。新下载可以覆盖匹配的现有配置，包括手动编辑；复用当天目录为新增路由补价时，只初始化缺失键。目录价格使用 USD，不做汇率转换。

当前 `load_price_cache` 把已配置的价格都标成 `manual`，其中也可能包括目录自动导入的价格。这里的 manual 是数据库结算来源分类，不能单凭它证明某个人手工输入过这条单价。

## 6. 三种输入计价模式

`pricing::settle` 用 [`PriceRule`](../../src/stats/pricing.rs) 及已归一的 `UsageOutcome` 做纯计算。以下每项金额都先独立舍入到整数 nano，再相加：

| 模式 | 输入相关费用 | 必须具备的证据 |
| --- | --- | --- |
| `flat` | `input × input_rate` | 可信输入总量；缓存明细已包含其中，不再单收 |
| `read_discount` | `(input - cache_read) × input_rate + cache_read × read_rate` | 已知缓存读、缓存读单价，且相减非负 |
| `disjoint` | `uncached × input_rate + cache_read × read_rate + cache_write × write_rate` | 明确 disjoint，以及三部分用量和缓存两项单价 |

三种模式都另算 `output × output_rate`。表中省略了统一的每百万 token 分母。`reasoning` 不在输出总量之外再加一遍。

以现有 `read_discount_matches_the_documented_example` 为例：输入 1000，其中缓存读 800，输出 100；输入单价 2/M、缓存读 0.2/M、输出 8/M：

```text
非缓存输入：200 × 2 / 1,000,000   = 0.0004
缓存读：    800 × 0.2 / 1,000,000 = 0.00016
输出：      100 × 8 / 1,000,000   = 0.0008
总额：                            0.00136
存储 total_cost_nanos：            1,360,000
```

选择 flat 时同样的 usage 算 0.0028，因为缓存读不单独打折。选择 disjoint 则需要额外拆分证据；不能把未知 cache_write 当成 0 让公式勉强通过。

## 7. 舍入、溢出与结算状态

`part` 的单位换算是：

```text
product = tokens × rate_nanos_per_million        // i128
amount_nanos = (product + 500000) / 1000000      // 非负数 half-up
```

例如 100,000 token、单价 5 nano/M，原始结果是 0.5 nano，舍入为 1 nano；单价 4 nano/M 时是 0.4，舍入为 0。四个分项先各自舍入，然后用 i128 相加，最后尝试转换成 i64。不是先加所有未舍入小数、最后只舍入总金额。

输入遵守前置的非负值校验；i128 用作中间计算范围，`checked_mul / try_from` 等处理范围问题，超出可表示的单项或总额时返回 overflow，不能整数回绕。它不是任意精度整数，也不是任意有符号小数都适用的通用舍入库。

要分清**存储层分支**和**纯计算函数分支**：

1. `finish_request` 先检查请求 Start 保存的 `price_source`。默认零价直接给已知零；未派发上游为 not_applicable，已派发为 default_zero。
2. 已配置价格进入 `pricing::settle`：先判断未派发上游，再检查 complete usage，再检查是否有 rule，最后按模式和数值范围计算。
3. 纯函数的 `rule=None` 可返回 unpriced，但正常缺价请求在外层已经有 default_zero 快照，不能把单测 `settle(None, ...)` 当作普通缺价请求的最终展示规则。

| 状态 | 金额是否已知 | 常见触发 |
| --- | --- | --- |
| `pending` | 尚未结算 | Start 已写入，Finish 未提交 |
| `not_applicable` | 已知零 | 明确没有派发上游的正常结算 |
| `default_zero` | 已知零 | 缺少当前价格配置所产生的显式默认快照 |
| `priced` | 已计算 | 已配置价格，必需用量和拆分可用，范围有效 |
| `usage_incomplete` | 未知 | 已配置价格，但父用量不完整 |
| `unsupported_breakdown` | 未知 | 计价模式所需缓存拆分或单价缺失 |
| `overflow` | 未知 | 金额无法落入受支持整数范围 |
| `legacy_unknown / unpriced` | 按其来源保留未知 | 历史来源无法恢复，或纯函数收到无规则输入 |

尤其注意：**手动创建的全零价格不会绕过 complete usage 和拆分检查**。`price_source=manual` 时即使单价全 0，也走上述纯函数条件；用量不完整仍可能得到未知金额。这与“完全没价格，采用 default_zero 业务默认”不同。

重启与丢失 Finish 的恢复通过 `settle_abandoned_cost` 读取旧快照和已有事实，不能回头按今天的价格补算。恢复分支对 default_zero、legacy_unknown 以及 `upstream_started=NULL` 有自己的保守处理；不能把正常 Finish 的每条分支直接套到旧数据修复上。

## 8. 从设置表单保存价格：在哪儿转换、提交和发布

跟 [`PriceDraft::submit`](../../gpui/src/stats_controller.rs) → [`Statistics::save_price`](../../gpui/src/statistics.rs) → [`parse_price_save / handle_control`](../../src/stats/mod.rs) → [`save_price_rule`](../../src/stats/store.rs)：

1. 表单收集进草稿。submit 将人类单价转换一次为 nano 字符串，留空价格按零处理，生成带 request_tag 和版本的保存请求。
2. GPUI 交给统计后台写任务，经本机管理 API 到核心；等待核心回复期间保留草稿，避免失败后丢失输入。
3. 核心校验字段、价格 ID 与 expected_revision；新建不能用非零 revision 偷偷覆盖旧记录。
4. writer 在事务内新增或更新当前价格。更新保留 `(provider_id, upstream_model)` 身份；过期 revision 返回冲突，缺失记录有单独错误。
5. 提交后读取结果并重新加载 cache；成功返回协调层后才替换请求可见的 `Inner.prices`。
6. 新准入请求取得新快照；历史与在途请求继续使用原快照。

这是乐观并发控制，可类比 JPA `@Version`，但 SQL 条件与更新行数检查都直接写在代码里。删除也带 expected_revision，并且只删当前价格；没有对历史费用做级联删除或按新价重算。

数据库提交与内存发布不是跨资源原子事务。提交之后读取 DTO 或重建 cache 仍可能失败，所以“收到保存错误”不能无条件证明数据库没有变化。分辨问题时，要看错误发生在 commit 前还是 commit 后，不要把所有异常都等同于事务回滚。

## 9. 阅读验证入口

下列现有测试可用于学习契约；本次只补注释和文档，未执行这些业务测试。

| 所在源码 | 测试函数 | 关注点 |
| --- | --- | --- |
| [`collector.rs`](../../src/stats/collector.rs) | `partial_candidates_merge_but_never_sum` | 不把重复累计 usage 相加 |
| 同上 | `conflicting_subfield_drops_only_the_subfield` | 核对测试实际覆盖的明细异常及父量保留，不只凭名称推断所有冲突 |
| [`pricing.rs`](../../src/stats/pricing.rs) | `the_settings_form_converts_a_human_amount_exactly_once` | 表单与 API 单位只转换一次 |
| 同上 | `read_discount_matches_the_documented_example` | 折扣输入分项计算 |
| 同上 | `disjoint_requires_a_proven_split` | 缓存互斥关系必须有证据 |
| 同上 | `overflow_is_reported_instead_of_wrapping` | 大数范围检查 |
| 同上 | `rounding_is_half_up_per_part` | 每项独立舍入 |
| [`price_catalog.rs`](../../src/price_catalog.rs) | `provider_prices_win_and_ambiguous_gateways_do_not_pick_a_reseller` | 目录候选优先级与歧义拒绝 |

读完应能回答：为什么 input 不能再加一次缓存读？为什么缺价格可显示零，缺用量却可能显示未知？为什么相同的请求在不同计价模式下费用不同？为什么删除价格不能让历史明细失去单价？下一步按 [统计查询与界面导读](STATS_QUERY_UI.md) 查看这些事实怎样聚合并到达界面。

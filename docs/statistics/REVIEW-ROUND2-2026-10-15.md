# 使用统计第二轮复核与价格改造合并结论

日期：2026-10-15。审查对象：`/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics`，分支`codex/usage-statistics`，HEAD仍为`e1725c3`，功能修改未提交。

**结论：上一轮多项核心修复有效，但仍有阻断供应商选择和汇总翻页的问题，GPUI状态机尚未闭环；用户最新的“当前价格可改、调用明细独立快照、缺价0元”还没有进入业务实现。** 下一轮应按[合并开发Prompt](NEXT-ITERATION-COMBINED-PROMPT.md)继续，不能直接验收/发版。

本轮只复核、运行隔离测试及生成文档，没有修改被审查worktree的Rust/GPUI/Python业务代码。

## 1. 实际验证结果

| 验证 | 本轮结果 |
| --- | --- |
| 核心 `cargo test --locked --quiet` | 105 passed |
| 核心release构建 | 通过 |
| 核心fmt/clippy | 通过 |
| GPUI `cargo test --manifest-path gpui/Cargo.toml --locked --quiet` | 34 passed，使用RUST_MIN_STACK=67108864 |
| GPUI fmt/clippy | 通过，见本轮执行结果 |
| 现有statistics_test.py | 34项通过 |
| 直接引用实际Collector源码的探针 | 原冲突合并/新增缓存细分/大chunk三项均得到正确结果 |
| 真实核心+隔离Mock补充验证 | JSON错误、冲突usage、正常停服、空桶、瞬时锁恢复已改善；新价格需求仍未实现 |
| 直接引用实际StatsScheduler源码的探针 | 复现供应商筛选意图被模型筛选意图覆盖 |
| 正式API汇总分页探针 | 复现服务器自己的next_cursor被拒绝；绕过错误校验后又触发aggregate SQL错误 |
| 直接引用实际Writer源码的锁探针 | busy_timeout缩短到30ms时，64事件批次失败仍耗时2474ms |
| 主工作区v2设计SQL验证 | 通过，仅证明设计，不表示当前Rust已迁移v2 |

所有核心实例均为临时配置目录、假Key、本地Mock/SQLite。未访问真实供应商、未改真实配置和数据库。没有重新运行百万行性能、Rust1.88或Windows构建；未进行完整GPUI三尺寸人工像素验收。

补充证据在[本轮证据目录](review-round2-2026-10-15/README.md)，包含结果、源码SHA-256和探针。探针输出是观察证据，不是“所有产品行为已通过”的断言。

## 2. 上轮已确认改善的部分

- F01：冲突部分usage现在记invalid、数字为空，后续采集保持ready；同样父项补缓存细分不会误判冲突。
- F05：同一195098字节合法SSE整块/4096字节喂入均Complete。
- F06：HTTP200中的JSON error、畸形JSON都不再标completed。
- F08：正常停止核心时，慢流记录interrupted/shutdown。
- F09：采集覆盖可证且无请求的空桶，input/output/total明确为0。
- F02常见锁场景：Start已落库、Finish遭锁后释放，核心恢复ready，旧行变interrupted，新调用继续completed，区间有degraded记录。
- F03的纳单位契约已修复：API接受整数纳单位，旧的十亿倍转换问题不再重现。

这些改善需要保留；下列问题不要求推倒重写全部统计实现。F02的批次耗时/孤立行边界和F10的页面身份隔离仍有缺口。

## 3. 仍需修复的问题

### R01 [P1] 两个不同筛选请求被调度器当成同一意图，供应商列表不会加载

位置：开发worktree `gpui/src/statistics.rs:758-773`；`gpui/src/stats_backend.rs:131-145`。

`request_filters()`先提交provider筛选，再提交model筛选，二者都是JobKind::Filters。StatsScheduler按JobKind保存最新read，第二次insert直接覆盖第一次，尚未pump就已丢失provider请求。

直接使用实际调度器和本地假核心复现：

```text
提交：filters-0(kind=provider)、models-0(kind=model)
实际发出：[models-0]
收到回复：[models-0]
```

`provider_options`初始化为空且依靠filters回复填充，因而影响供应商选择、供应商usage模式，以及价格表单的供应商选择。这是功能阻断，现有4个调度单测没有覆盖“同Kind、不同资源”。

要求：合并键至少区分查询资源/参数作用域，如ProviderFilters、ModelFilters(provider_id,query,cursor)。ProviderMode读取也按provider区分。只允许同一资源的旧意图被新意图替代。补“两个筛选同tick都发出、同资源多次只保留最新”的回归。

### R02 [P1] 汇总分页无法使用服务器自己返回的游标

位置：`src/stats/query.rs:1107-1147`、`:1448-1484`、`:1550-1561`；`src/stats/mod.rs:522`、`:580`、`:1082`；`gpui/src/statistics.rs:739-752`、`:2156`。

模型汇总3组、limit=1，第一页正常返回key=model-ok的next_cursor。把这个原样交回服务端，收到：

```text
stats_cursor_expired: cursor was issued for a different grouping
```

原因：cursor.key实际存的是最后一个分组的值，但validate_breakdown_cursor把它当列名，要求等于upstream_model/provider_id/provider_name。正常模型名和供应商ID都会被拒绝。

诊断时把key改成upstream_model以越过错误校验，后续SQL又返回`misuse of aggregate: SUM()`：翻页条件把SUM/COUNT聚合表达式拼进WHERE，应在HAVING或外层已聚合结果上做keyset。

另外，GUI分组查询根本没传cursor，收到BreakdownDto也没保存next_cursor，翻页按钮只给Requests页签；超过首20/50/100组的结果不可达。

要求：明确区分group_by和last_group_key，修合法游标校验；用CTE/子查询/HAVING分页；覆盖相同指标并列、NULL指标最后、全部未知用量、provider/model和两种排序。请求/分组都接通前后翻页。实际reader执行时再次核验维护版本，不能只在进入队列前校验，以免排队期间clear/retention后继续用旧游标。

### R03 [P2] 引入seq后仍没隔离页面/筛选身份，迟到回复能污染新状态

位置：`gpui/src/statistics.rs:652-657`、`:924-949`、`:1004-1068`及页签点击回调。

静态调用链确认：

1. mark_dirty只增加generation，没有立即使当前dashboard_seq/rows_seq/groups_seq失效；debounce发新请求前，旧seq仍被判当前并更新新筛选下页面。
2. stale分支仍清pending_dashboard/pending_rows，可能解除更新请求的pending。
3. kind为Rows或Groups后仍通过`rows-`标签进入apply_reply，再按**回复到达时的self.tab**选择DTO。切到另一个页签并未统一失效所有旧查询；旧Groups可被拿去解析成RequestPageDto，失败后静默忽略。
4. Groups只按一个groups_seq区分，需明确provider/model、sort、cursor作用域；不能只依赖tag尾数字。

要求：建立QueryIdentity(filter_generation, tab/group_by, sort, cursor, request_seq)。改筛选/切页签立即失效旧身份，旧回复既不更新数据也不清新pending；按回复自身kind和身份解析DTO，解析失败报协议错误。加入可控迟到回复的页面/控制器测试，不只测scheduler队列。

### R04 [P2] 查询错误把完整内部SQL回传GUI

位置：`src/stats/query.rs:190-205`。

R02的错误响应包含完整SELECT、所有统计CASE、WHERE、排序和SQL offset，远超稳定用户错误信息。API契约明确不回传SQL。

要求：对外固定错误码+简短本地化提示；SQL/诊断细节只在开发诊断中保留且不包含敏感参数。回归断言错误消息不含SELECT/FROM/WHERE/数据库路径，超时仍为stats_query_timeout。

### R05 [P2] 数据库故障隔离会把锁等待按事件放大，关闭预算也未传入实际写入

位置：`src/stats/store.rs:346-390`；`src/stats/mod.rs:1446-1499`、`:1523-1559`。

当前无论是CHECK坏数据还是SQLITE_BUSY，整批重试失败后都逐事件再尝试。探针把busy_timeout缩短到30ms，对64个Start事件持外部写锁，实际失败耗时2474ms；生产默认busy_timeout=2000ms，最坏单批存在约(3+64)次锁等待，远大于目标刷新/flush预算。flush的deadline只在调用apply_batch前检查，无法约束内部循环。

还有两个静态边界：remember_orphans超过512直接丢掉最旧ID，没有先结算或水位兜底；recover_collection先take掉degraded_since，区间写入失败仍可设ready，重试依据丢失。

要求：保留rusqlite错误类别，暂时性锁/IO错误不逐条隔离；仅确定性行约束错误隔离；传统一批次/关闭deadline并缩短剩余busy等待。溢出的孤立行必须有受界限的SQL/水位补偿，不能永久遗留in_flight；恢复元数据/区间失败时保留待恢复状态，不提前宣称可信覆盖。新增64事件锁故障预算、关闭、>512丢终态及区间提交失败回归。

### R06 [P2] 自定义日期会静默改成另一个有效范围

位置：`gpui/src/statistics.rs:856-863`。

parsed_custom_range对无法解析的输入静默沿用上次值，对to<=from直接钳制成from+1000ms。用户输入无效/反向区间后看到的是旧数据或1秒窗口，没有明确错误。

要求：输入非法/区间反向/超366天时就地显示错误，不发新查询、不改变已应用筛选；提供应用动作或明确验证后的debounce。用合法/非法/跨DST/反向/过长范围测试。默认预设、分页冻结窗口保持现有正确做法。

### R07 [P2] 已披露的筛选/语言缺项与生命周期仍需完成

位置：`gpui/src/statistics.rs:636-649`、`:758-773`、`:877-922`及价格表单。

- provider/model选项只取100，不使用next_key，也没有服务端搜索分页。
- model筛选请求没传当前provider_id，选择供应商后不即时收窄模型。
- set_language只改zh和表格，已创建的Select选项/placeholder/对话框未重建。
- 自动刷新只看visible，缺窗口前台/最小化和模态控制；tick开头先pump，离开页面后仍可能继续发送已排队只读任务。
- 价格表单在provider为空时仍取provider_options.first，而非真正要求用户明确选择；这也与旧进度里的“供应商必选已修”不符。按新价格表单改造时一并修。

这些不能由“实际观测到统计API调用”代替交互验收。补明确的控制器/接口测试及适用人工检查。

## 4. P01：最新价格业务变更尚未实施（本轮必须合并）

当前代码仍为API v1/schema v1，只有0001.sql；使用stats_price_rules的生效区间与历史引用外键。实际请求缺价得到`unpriced + total_cost_nanos=null`，usage_costs没有input_rate_nanos或price_snapshot_at_ms，已引用规则仍拒绝编辑。

这不是继续要求旧区间方案；**用户已明确改成**：

- 当前价格配置可新增/直接修改/删除，历史引用不能锁定它。
- 每次调用独立存四项单价、币种、模式、时间、分项金额和总金额。
- 请求接纳时冻结价格，终态计算；中途/之后调价不改本次记录。
- 缺价单价/金额0；以后补价不反改历史；Token未知仍NULL。
- 统计/详情不靠当前价格表；移除价格外键和历史重算入口。

执行主工作区[PRICING-DECISION.md](PRICING-DECISION.md)、[v2 schema.sql](schema.sql)、[PRICING-CHANGE-PROMPT.md](PRICING-CHANGE-PROMPT.md)。必须新增正式v1→v2迁移，不只修改0001、不删库、不对旧纳单位整体乘除1e9。

## 5. 交付和验证仍缺的证据

- 按用户新价格模型重做“手动价/缺价默认0/非零价缺拆分”三类10万/100万数据基准；旧全unpriced快路径不足以代表真实使用。
- 实测Rust1.88最低版本；本机1.98通过不是MSRV证据。
- Windows构建/打包/人工验收单列；本轮没有该证据。
- GUI三尺寸、中英文、迟到回复、三页分页、无效日期、价格修改/删除/缺价等实际交互检查。
- 真实供应商usage和真实账单未验证，继续明确披露；新缺价0是用户的统计规则。

合并任务见[NEXT-ITERATION-COMBINED-PROMPT.md](NEXT-ITERATION-COMBINED-PROMPT.md)。执行顺序建议：R01/R02 → R03/R04/R05 → P01价格改造 → R06/R07 → 完整回归和证据整理。

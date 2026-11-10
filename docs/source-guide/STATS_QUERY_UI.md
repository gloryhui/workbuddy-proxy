# 统计查询与界面：从筛选条件到可显示的结果

本篇接着 [统计持久化](STATS_PERSISTENCE.md) 和 [用量与费用](STATS_USAGE_PRICING.md)。前两篇说明事实怎样产生、入队和落库；这里解释怎样读出事实、聚合成报表，再安全地交给 GPUI。面向 Java / Spring Boot 开发者，建议把它当作一次跨客户端、Controller、查询服务与页面状态的跟踪练习。

## 1. 先分清进程、线程和对象

```text
GPUI 设置进程
  Statistics：控件、当前筛选、分页、loading/error、已显示数据
      ↓ 生成不可再读取控件的命令 JSON
  StatsScheduler：合并待发读意图、保留写操作、跟踪 in_flight
      ↓ 有界 channel
  statistics-control 标准线程：逐条调用本机管理 HTTP
      ↓
Core 进程
  管理入口：鉴权、解析统计命令、检查准入
      ↓ ReadJobEnvelope
  stats-reader 标准线程：顺序执行查询，持有自己的只读 SQLite 连接
      ↓ query 模块 / SQLite 读事务
  DTO → JSON → 管理 envelope
      ↓ HTTP 返回
GPUI 的 StatsReply channel
      ↓ tick：complete(key)，再判断回复身份
  Statistics：接收当前结果，更新 Entity，notify 重绘
```

源码入口为 [`stats_backend.rs`](../../gpui/src/stats_backend.rs)、[`stats_controller.rs`](../../gpui/src/stats_controller.rs)、[`statistics.rs`](../../gpui/src/statistics.rs)、[`stats/mod.rs`](../../src/stats/mod.rs) 和 [`query.rs`](../../src/stats/query.rs)。

`Statistics` 可类比页面 ViewModel，`stats_controller` 提供不依赖窗口的纯状态规则；`StatsScheduler` 类似带合并策略的客户端任务队列。Core 查询函数像显式 SQL 的 Repository/查询 Service。这里没有跨进程共享 Java 对象，也没有依赖注入容器代为管理这些对象。

资源有独立 `JobKey`，不表示每个资源有独立线程。GPUI 的统计 HTTP worker 逐条执行，Core reader 也顺序处理读取；资源身份主要解决合并、排队和结果归属问题。

## 2. 改筛选时，先使旧结果失效，再等防抖

`Statistics::mark_dirty` 不是立刻发 HTTP，而是先改变本地状态：

1. 使 Recent 的读取失效，递增页面筛选 `generation`。
2. 重置请求表和分组表的分页历史，并冻结本次时间范围。
3. 立即清除 Rows/Groups 的旧查询身份，令 `dashboard_seq=0`。
4. 设置防抖截止时间，通知界面。
5. `tick` 在页面可见、阻塞弹层关闭且防抖到期后调用刷新。

假设 A 查询“今天”，用户切到“昨天”，但 B 还没发出。A 在防抖期间返回时就必须过期，否则旧内容会被画到新筛选下面。因此失效发生在用户改变条件时，不能延迟到 B 真正发出之后。

`refresh_report` 根据当前视图请求 dashboard、导入状态、当前列表或 Recent，再读取 health。普通周期刷新在第二页及以后保留分页时间窗；显式刷新或筛选变化可以重新捕获时间。这样“最近 24 小时”的第二页不会因每次重新计算 now 而悄悄换范围。

`filter()` 将状态编码成 JSON。范围是 `[from_ms, to_ms)`，空时区让核心解析机器的 IANA 时区。来源、采集方式、记录种类、供应商、模型、outcome 和 usage_status 是不同约束；不能把“导入记录”简单当作某种代理 HTTP 请求。

## 3. 五种相似的身份，分别解决什么问题

| 名称 | 谁生成 | 用途与边界 |
| --- | --- | --- |
| 页面筛选 `generation` | GPUI 本地状态 | 筛选变化时使旧读取失效；Recent 和查询身份会使用它 |
| scheduler `seq` | `StatsScheduler` | 标识一次实际提交的任务；回复原样带回，是多个界面门禁的直接比较值 |
| `request_tag` | 请求构造方 | 如 `dash-*`、`rows-*`，帮助分派资源及区分草稿保存 |
| DTO `generation` | Core `Inner::generation` | `run_id:commits` 的运行/提交代次信息；不是 GPUI 筛选计数器 |
| `filter_key` | Core `StatsFilter::key` | 表示规范化筛选，参与游标绑定；不是数据库锁，也不等于 seq |

这些值不能互相代替。尤其不能看到 Core DTO 里也叫 generation，就以为它是界面发送后原样返回的筛选代次。Core 的运行信息来自原子计数读取，也不能单凭这个字符串宣称取得了某个精确事务版本。

界面并非对每个回复都统一比较 DTO 的全部字段。Dashboard 比当前 `dashboard_seq`；Rows/Groups 由 `ReplyGate`、seq 和当前 tab 判断；Recent 比自己的 seq 与页面 generation；其他资源有各自 seq/tag 规则。`filter_key` 的服务端游标用途与这些客户端门禁要分别跟踪。

## 4. 调度：读意图可以合并，写操作保序

`submit_read` 将最新意图存入以 `JobKey` 为键的 Map。同一个 Dashboard 的两次待发刷新可以只留最新一次；供应商选项和模型选项是不同 key，不能相互覆盖。Recent 与分页请求表也有不同 key。

`pump_with_reads` 先处理写操作，再选择可发出的读操作；请求行、Recent、分组、详情优先于后台读资源。同 key 已在途时，新读取等到旧回复被 `complete`。channel 满时放回任务，读任务放回时不能覆盖期间新产生的意图。

写操作使用有界 FIFO，队列满明确返回错误。离页时 `cancel_reads` 只清尚未发出的读意图，不停止已经进入 worker 的 HTTP，也不取消 SQL；已授权的写操作继续排队执行。不要把“忽略旧结果”理解成“旧查询已被物理取消”。

`StatsScheduler::complete` 只释放调度占用。即使结果过期也要先 complete，否则等待中的同资源新意图可能无法发出。释放任务与接收内容是两个独立步骤。

## 5. 管理 HTTP 怎样进入只读查询

`stats_backend::call` 从实例记录取得本机端口和管理 key，禁用代理与重定向，向 `/_control/action` 发命令。它不是用供应商 API Key，也不直接打开 SQLite。管理回复外层有 envelope，统计 payload 在 `message` 字符串内，`parse_message` 再解析一次；畸形 payload 是错误，不能当空列表显示。

Core 的 reader 按需建立只读连接，在专用线程中顺序调用 `run_read`。分页 cursor 在命令准入和实际执行时都需要核对，因为排队期间维护操作可能改变 revision。查询返回 DTO 后再序列化，最终经原回复通道回到管理调用方。

这里有多层时间与容量约束：

- GPUI 一般统计 HTTP 超时为 3 秒；价格目录刷新有单独的 45 秒超时。
- `query::Budget` 设置 2 秒 deadline，SQLite progress handler 每 2000 条虚拟机步骤检查一次；Drop 时撤下 handler。
- 它不把排队、准入等待或任意 Rust CPU 工作强制中断到 2 秒内，也不是浏览器取消 API。
- dashboard 汇总合并时检查 `ROW_BUDGET`，超过范围要求缩小筛选；桶数也有上限。
- 请求列表首屏虽读取 COUNT，当前函数并没有因此自动执行同样的 ROW_BUDGET 检查。不能只凭旧函数注释把行上限扩展到每个查询。

异常通过 `Result<_, String>` 和稳定统计错误码返回；`?` 把失败交给上层，不会自动生成一个业务上可信的全零 DTO。

## 6. Dashboard：同一读事务里的汇总、趋势和费用

沿 `dashboard → dashboard_inner → dashboard_aggregate::aggregate` 阅读：

1. 根据范围和粒度创建桶，解析时区。
2. 在 reader 连接开启一次 SQLite 读事务，让本次聚合和费用读取使用同一数据库快照。
3. 每个桶分别读代理与导入来源的聚合，再合并到总汇总。内存保存的是每桶摘要，不是把全部请求装成对象。
4. `scan_costs` 一次扫描已分类的代理费用，按币种汇总，同时填充每桶费用。
5. 根据聚合及采集覆盖区间生成 series、summary、warnings。
6. 结束读事务，返回 `DashboardDto`。

[`dashboard_aggregate.rs`](../../src/stats/query/dashboard_aggregate.rs) 的“共享聚合”指同一批桶摘要供 overview 与 series 使用，不是跨请求结果缓存。耗时均值按已知 completed duration 的个数加权，不能把各桶均值直接相加后除以桶数。

SQL 可以 SUM token；金额按币种在 Rust 中用 i128 累积，再以十进制字符串输出。USD 和 CNY 不相加，WorkBuddy credit 也单独累计，不加入供应商费用。导入记录不拥有代理的网络耗时、HTTP 状态与本地价格费用，不能为表格整齐而补出来。

正常费用扫描只覆盖 `record_origin=proxy`；导入未定价数量另从来源聚合加入。`legacy_unknown` 也不能一概当作没有金额：扫描会保留该状态下实际已存的金额。价格当前值不参与重算历史费用。

## 7. 趋势图里的 0、未知和部分可知

`Option` 的业务含义一直延续到趋势点：

- 某桶没有请求，且采集覆盖能证明整桶都在采集，可以给出确认零。
- 某桶没有请求，但没有完整采集覆盖，可能只是没采到，不能画成确认零。
- 有请求但没有完整 usage 时，token 仍可能是 None；图表应该表达缺口。
- 一部分有可信 usage，可以显示已知部分并保留质量或警告信息，不宣称所有请求都完整。

`nullable_add` 的作用是加总已有可信值：两侧都 None 才保持 None，一侧有值就保留已有值。它不保证汇总里的每条请求都有数值，所以必须结合 complete/eligible 计数和 `SeriesQuality` 解释。

小时桶按毫秒步进；日、周桶跟本地日历边界，周按星期一对齐。夏令时的“一天”可能不是 24 小时。首尾仍裁在用户筛选范围，所有桶严格递增且不重叠；桶太多会报错，不偷偷采样。

## 8. 分页：固定新增行边界，不长期持有数据库事务

请求列表按 `(started_at_ms DESC, id DESC)` 稳定排序。第一页捕获 `as_of_id`，后续页要求 `id <= as_of_id`，并以最后一行时间和 id 继续向后找。读取 `page_size + 1` 行判断是否还有下一页，再裁成所需大小。

这可以防止新插入行不断挤动已浏览的页，但不表示跨 HTTP 保存同一 SQLite 事务；在途请求的原行仍可能更新。cursor 还绑定 filter_key、Core run_id、maintenance_revision；分组 cursor 额外绑定分组维度、排序与末组键。

普通下一页使用 keyset。直接跳页需要先在有序键上定位偏移，再读取目标页，不能把“支持跳页”讲成所有路径都没有 OFFSET。后续页的 total_rows 可以用 -1 表示沿用既有总数，不是负一条请求。

Core 查询层构造 cursor 后，`run_read` 补上运行身份和维护版本。运行重启、维护或筛选改变会使旧 cursor 失效。Rows 与 Groups 平时各自维护分页；当前界面处理 `cursor_expired` 时却会调用 `reset_pagination` 重置两个分页器并重捕获时间窗，只立即重取刚失败的列表第一页，另一个列表后续再读取。

分组查询的首屏总组数是有界计数：超过范围时可以用 `MAX_GROUPS + 1` 表示“还有更多”，不代表实际精确总数，也不因此禁止后续分页。它有自己的分组排序契约；request detail 则按 request_id 读取请求及其历史价格/费用分项。详情不用当前列表某一行的缓存去猜完整数据。

当前 detail 用两次 autocommit SELECT 读取主行及费用快照，没有像 dashboard 那样显式包在同一个读事务里。因此“所有统计查询都由一份快照组成”的说法也不成立。

## 9. 回复到界面：成功、错误、过期是三条路径

`tick` 从 channel 取回复，先 complete，再交给 `handle_reply`。后者按 kind 分派，在对应分支执行可见性、seq、tag 或草稿身份等检查，再解析或合并数据；并不存在对所有资源统一比较全部身份字段的总门禁。

| 情况 | 页面处理 |
| --- | --- |
| 当前 Dashboard/列表查询成功 | 更新结果、分页、时间戳和相关 loading/error |
| 旧查询成功或失败 | 忽略内容，不清新请求的 pending，也不展示旧错误 |
| 当前普通 Dashboard/列表查询失败 | 保留已显示数据，标明错误；不能暗示这仍是刚查到的新结果 |
| cursor 失效 | 重置两个分页器和时间窗，立即重取失败列表的第一页 |
| Recent 当前查询为空或失败 | 清掉旧 Recent，显示空态或错误，避免把旧行标成“最新” |
| 价格保存失败 | 保留草稿与错误；旧草稿的回复不能关闭后来打开的新草稿 |

Recent 使用 [`RecentRead`](../../gpui/src/statistics/recent.rs) 独立读取当前筛选下的第一页，再取首条。它不复用用户停在第三页的列表，也不携带列表旧 cursor。这里的失败策略与“保留普通列表旧数据”不同。

页面 health 从 `stats_health` 的完整快照读取。能查询数据库不等于采集正常：采集可能暂停、队列可能丢弃过事件，或数据库正在恢复。这些事实不能由一次 HTTP 200 推断。

## 10. 为什么查询还需要和历史导入协调

[`report_scheduling.rs`](../../src/stats/report_scheduling.rs) 不负责业务查询结果，而是避免 reader 报表与一次历史导入 I/O 重叠。它没有阻止普通代理事件、控制消息或关闭流程；SQLite/WAL 的并发机制也仍然存在。

`ReadActivity::enter` 返回借用门禁对象的 ReadGuard；导入 writer 用非阻塞的 `try_enter_import`。有 reader 时只预约导入，writer 继续处理其他工作；最后一个 reader 退出后给预约一次交接窗口。

`ImportYield` 在持续报表下每 400ms 提供一次导入机会，真正准入仍要等待活跃报表结束。没有报表就立即允许。reader 为尚未开始的交接最多等待 75ms，writer 有预约时将空闲轮询缩到最多 25ms。这些不是导入时延保证；优先级更高的工作仍可延迟导入。

Java 类比为 `Lock / Condition` 保护计数与预约；Rust 守卫 Drop 在正常返回、`?` 早退和 panic 展开时释放准入。数据库事务仍归查询/导入函数管理，不能把释放 Mutex 或 Guard 当成事务提交。源文件读取、批次 ACK 与 ledger 去重继续看 [历史导入源码专题](WORKBUDDY_IMPORT.md)。

## 11. 用现有测试理解契约

以下是阅读入口，本轮只做注释和文档检查，未执行这些业务测试：

| 位置 | 现有测试 | 观察点 |
| --- | --- | --- |
| [`query.rs`](../../src/stats/query.rs) | `dst_days_are_shorter_or_longer_but_still_strictly_increasing` | 日历桶与 DST |
| 同上 | `the_three_scopes_partition_the_fact_table` | 来源是同一事实表的划分 |
| 同上 | `credit_is_summed_on_its_own_and_never_added_to_costs` | credit 与费用分离 |
| 同上 | `different_currencies_are_never_added_together` | 币种分离 |
| [`stats_backend.rs`](../../gpui/src/stats_backend.rs) | `leaving_cancels_reads_but_preserves_and_prioritizes_authorized_writes` | 离页读取消与写保留 |
| 同上 | `recent_reads_do_not_replace_the_paginated_request_intent` | 资源 key 独立 |
| [`stats_controller.rs`](../../gpui/src/stats_controller.rs) | `a_stale_reply_cannot_overwrite_a_newer_read` | 旧回复门禁 |
| [`recent.rs`](../../gpui/src/statistics/recent.rs) | `empty_and_failed_latest_reads_do_not_relabel_an_old_row_as_latest` | Recent 的失败边界 |
| [`report_scheduling.rs`](../../src/stats/report_scheduling.rs) | `an_active_report_cannot_be_overlapped_by_an_import_opportunity` | 机会不绕过准入 |

可以手工推演一次“切换来源后，旧查询恰好返回，再翻页”的过程：画出本地 generation、seq、游标与 Core run_id，分别指出哪层拒绝了什么。能沿源码回答这些问题，就已掌握查询和界面衔接的主要状态边界。

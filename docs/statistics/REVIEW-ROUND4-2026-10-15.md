# 第四轮复核与继续开发任务

日期：2026-10-15。对象：`/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics`，`codex/usage-statistics`，HEAD=e1725c3，功能改动仍未提交。schema=2，统计API=2，control protocol=6。

**结论：上轮清理、正常拒绝和缺价汇总已改善；有价请求异常恢复仍会误判0元，汇总分页金额不受水位约束，重复新建价格仍会假成功。故障恢复与大部分界面任务尚未完成，继续施工。**

本轮没有修改开发worktree业务代码、真实用户配置或数据库；只生成隔离审查证据与本报告。[原始结果与探针](review-round4-2026-10-15/README.md)。本文件包含可直接执行的下一轮任务，不要求重做已经通过的功能。

## 1. 本轮实际验证及确认改善

| 验证 | 本轮结果 |
| --- | --- |
| 核心test | 113 passed |
| GPUI test，RUST_MIN_STACK=67108864 | 37 passed |
| statistics_test.py | 37项通过 |
| 核心release、核心/GPUI fmt/clippy | 通过；GPUI依赖block v0.1.6有原有future-incompatibility提示 |
| core_compatibility.py，Python3.13 | 通过，验证当前control protocol=6 |
| 上轮Writer维护探针按当前源码复跑 | Clear应删600实删600，Retention应删1200实删1200，范围外不删 |
| 真实核心补充API | 缺价汇总已为CNY0；缺Key且有价请求现为rejected/not_applicable/0；clear预览1实删1，范围外记录存活；供应商价格过滤、next游标和显式0缓存价已改善 |

本轮未重跑其余四类smoke/路由/连接/模型能力脚本；没有新百万行基准、Rust1.88、Windows、真实供应商或三尺寸人工GUI验收。常规测试通过不覆盖下面的专项失败。

## 2. U01 [P1] 有价请求异常退出后被误记“未发上游、0元”

位置：`src/stats/store.rs:1433-1535`，尤其读取upstream_started处的`Option<i64>.unwrap_or(0)`；`pricing::settle`先处理未发上游。

真实核心复现：为model-slow保存手动非零价格，发流式调用并**实际收到上游响应字节**，等待Start落库，SIGKILL后重启。恢复结果：

```text
outcome=interrupted
upstream_started=NULL
price_source=manual
cost.status=not_applicable
total_cost_nanos=0
```

Start没存是否发过上游；NULL表示未知，不能转false。default_zero分支的注释已经知道此边界，manual分支却仍错误使用它。正确应保留当次非零单价和未知Token，金额`usage_incomplete/NULL`，不能假定免费。

**修复：**恢复结算保持三态upstream_started；只有明确0才not_applicable/0；未知且非零价缺必要用量时为usage_incomplete/NULL；default_zero依旧0；已有完整用量则只按旧快照计算；legacy已有金额不反算。加手动价/default_zero、崩溃/孤立Finish、明确未发/已发/未知、重复恢复测试，不能只测缺价崩溃。

## 3. U02 [P1] 旧v2终态pending记录不会被修复

位置：`src/stats/store.rs:178-225`、`:1433-1540`；本轮没有新增数据修正迁移。

恢复只枚举outcome=in_flight。本项目上一版已经实际产生过`interrupted + cost.pending`（见第三轮原始结果）。本轮构造这个已知旧v2状态并重启，仍是`interrupted/pending/NULL`。所以“都是代码路径，不需处理已v2库”的声明不成立。

**修复：**提供有界、幂等的启动修复事务或后续正式迁移，涵盖“请求已终态但费用pending”。保留原快照，遵守U01三态，不能用今天价格补价或重写正常历史。还要识别本轮U01留下的可证明错误组合，例如interrupted/manual/not_applicable但upstream_started=NULL；按证据修成未知，不能猜实际账单。修复数量/失败可观测，失败不宣称ready。

验收：v1升级、现存v2首次启动、重复启动、量大分批、故障回滚；已完成且正确结算的金额/Token不变，legacy_unknown已有金额保留。不删库重建，不仅修改已经执行过的0002。

## 4. U03 [P1] 分组翻页的请求/Token固定水位，金额却计入新增请求

位置：`src/stats/query.rs:478-535`、`:1606`。scan_costs_grouped只有filter，没有as_of_id，与同一事务内的分组请求计数使用不同成员集合。

真实API：三个模型各一条有价请求，第一页limit=1，as_of_id=3；随后给第二个模型新增两条调用，用原游标获取第二页。结果：

```text
as_of_id=3
model-page-b.request_count=1
total_tokens=1100
cost=8400000 nanos
settled_cost_request_count=3
```

冻结水位内实际只有一条、金额应为2800000。不是“状态变化导致报表不冻结”的允许边界：新增id>as_of_id本应被排除。

**修复：**金额、default_zero/settled/incomplete等计数与请求/Token共享同一filter+as_of谓词和读事务。grouped扫描传入水位，不能只改金额常量/界面。补provider/model、两种sort、三页前后回放、翻页期间新增和多币种；每组settled计数不超过该组匹配请求数。

reader实际执行时复查维护版本仍未做；补排队后clear/retention变更的游标回归，不能只在入队前校验。

## 5. U04 [P2] 价格重复新建被拒写却返回成功；price_id仍被忽略

位置：`src/stats/store.rs:1351-1399`；`src/stats/mod.rs`的parse_price_save与delete。

两个独立复现：

1. 已有输入价2000000000/revision1，不带ID/revision重复新建并提交99000000000。SQL的冲突UPDATE WHERE不匹配，所以没改库；但代码未检查affected_rows，又SELECT旧记录，API成功返回旧2000000000/revision1。进度里写“返回stats_revision_conflict”与实测不符。GUI会误以为保存成功并关表单。
2. 按正式v2文档传price_id和expected_revision编辑，解析器仍只读rule_id。换模型时被当新建，生成新price_id，原记录不变；不能静默把编辑变新增。

**修复：**新建重复组合键明确冲突，检查实际写入结果；编辑必须精确price_id+revision，未知ID不新增，过期revision不写；采用v2 canonical price_id，如保留rule_id别名须显式兼容并拒绝两者冲突。组合键要么允许修改并完整事务校验，要么界面禁止且服务端拒绝不一致，不能静默忽略。save/delete/list/DTO/GUI/测试统一。

加真实API测试：新建冲突、过期编辑、并发两编辑者、错误ID、canonical/别名、改模型/供应商、重试；冲突不发布缓存，GUI保留草稿与错误。

## 6. U05 [P1] 孤立费用结算失败后仍ready，解除故障也不再补偿

位置：`src/stats/mod.rs:1541-1595`及writer_loop的探测条件。

本轮补充故障注入：只在临时SQLite中用触发器拒绝usage_costs状态更新，让Finish及后续孤立结算失败。recover_collection即使保留orphans，仍只根据“区间写入成功”把状态变ready；degraded_since已清空，空闲探测也停止。

实测故障时`ready + in_flight/pending`；移除触发器、等待后仍`ready + in_flight/pending`。**一个孤立请求就能触发，不需要>512溢出。** 修好的故障区间保留分支要保留，但不能据此称故障恢复已完成。

**修复：**恢复条件同时包含区间持久化、孤立请求/费用补偿、溢出水位等；任一步失败保留待办和可重试调度，未补偿完不宣称ready。退避要真实生效且有界，不形成空转。解除故障且没有新请求时也必须恢复。

上轮明确未修的T04一并完成：

- trim_orphans先drain再尝试写，失败ID仍丢；需要有界内存与可靠补偿，不能仅等下次重启。
- WriteBudget未把busy_timeout缩成剩余时间。本轮实际Writer持锁，**1ms预算耗274ms**才返回；这不是已满足统一总预算。
- 隔离循环用outcome.applied当遍历下标，前面有坏事件时剩余切片会错。用真实索引和精确丢失计数，只对确定性行约束隔离。

验收：本轮触发器单条故障、>512终态持续锁/恢复、区间成功但cost失败、无新请求恢复、关闭截止时间、坏事件后遇到锁/超时，均不能静默遗留in_flight/pending。

## 7. U06 [P2] 价格界面仍无独立草稿，编辑会污染全局筛选

位置：`gpui/src/statistics.rs:3133-3176`、sync_filter_state、mark_dirty、reset_pagination。此项为静态事件链审查，未声称人工GUI已复现。

币种/模式回填已增加，应保留。但edit_price通过`self.provider=...`并修改**全局provider_select**实现供应商回填，没有独立PriceDraft；也没有mark_dirty。之后sync_filter_state看到两个值已经一致，不会触发查询失效/刷新，因此可能在“新供应商选中”的筛选下保留旧供应商统计；取消表单也会留下变化。编辑模式price_open也未正确设true。

建立独立草稿和供应商选择，编辑/取消不修改全局报表filter。新增与编辑的id/revision、provider/model、currency/mode、四价/label、loading/error一次完整管理。加真正控制器测试：交替编辑CNY/disjoint与USD/flat、仅改label、取消、保存失败、并发冲突、供应商不在当前筛选选项、报表筛选前后不变。

T07剩余：价格列表GUI仍只请求首100条，不用next；默认币种虽可经API改，GUI没有设置入口；input/output空白仍拒绝（实测blank input→invalid），而四项空白默认0是既定要求。i128汇总仍不能用parse::<i64>().unwrap_or(0)或显示“—”，统一定点格式化并回归超i64金额。

## 8. U07 [P2] T08仍全部未完成，本轮必须进入界面实现

现状再次对照代码确认：

- 分组请求未传cursor、回复未保存next，供应商/模型分页还不可用。
- 没有完整QueryIdentity。mark_dirty后debounce前旧seq仍有效，stale回复会清新pending，Rows/Groups按当前tab解析且错误静默。
- 非法日期沿用旧值、反向范围钳制1秒，没有就地验证。
- 筛选搜索/分页、供应商变更即时重载模型、语言重建、前台/最小化/模态及离页只读任务取消仍未闭环。

按[第三轮报告](REVIEW-ROUND3-2026-10-15.md)T08及原合并Prompt完成，不再仅转写为“下一轮”。先用可控迟到回复控制器测试固定状态身份，再接分组分页和交互，避免边写边靠人工猜竞态。页面三尺寸人工检查无法执行时单列边界，但控制器自动测试不能省略。

## 9. 发给DeepSeek的执行要求

继续现有worktree与分支，完整读本报告及[第三轮报告](REVIEW-ROUND3-2026-10-15.md)、[价格规则](PRICING-DECISION.md)、[正式API](API.md)。使用**主工作区**最新文件，不把开发worktree旧副本当最新设计。

本轮目标为U01–U07全部闭环，顺序：U01/U02恢复计费 → U03水位一致性 → U04价格保存 → U05可靠补偿 → U06草稿/价格界面 → U07统计页面 → 回归/基准。保留已经确认改善的清理、零费用、显式0缓存价、control6等行为。

每项先补旧实现会失败的正式回归，再改源码；附本轮探针是诊断资料，不可将“复现匹配”的exit0当产品通过。改设置/价格失败需保留输入；所有统计只读事实金额/快照，不用当前价重算历史。旧v2数据修正要有幂等和失败回滚证据。

执行核心/GPUI fmt、test、clippy、release，statistics集成、smoke、多路由、连接、核心兼容、模型能力及设置生命周期；新增本报告崩溃/孤立/水位/保存/界面状态机测试。完成三类新价格模型10万/100万查询性能，记录预算、i128、转发P95/RSS。Rust1.88、Windows、真实供应商、三尺寸人工GUI没有条件时明确未验证，不能编造通过。

更新IMPLEMENTATION-STATUS：T03/T07不能沿用错误“已完成”声明，旧v2需修正不能宣称无数据影响。最终给U01–U07闭环、测试命令/结果、未验证项、schema/API/control版本、worktree/分支/git status。上下文不足时保存精确断点；不得以既有37项通过或一次轮次结束替代未完成需求。

继续`/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics`、`codex/usage-statistics`；不reset/stash、不覆盖用户修改、不改主工作区业务代码、不提交/推送/发布/安装、不操作真实用户配置/数据库。无需逐批询问是否继续。

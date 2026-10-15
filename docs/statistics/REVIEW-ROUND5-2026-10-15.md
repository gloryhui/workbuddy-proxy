# 第五轮复核：后端修复确认与剩余施工要求

日期：2026-10-15。审查对象：`/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics`，分支`codex/usage-statistics`，HEAD=e1725c3加未提交功能修改。schema=2/API=2/control=6。

**结论：上轮U01–U04的主要失败样例已修；U05孤立补偿仍能复现，U06/U07界面源码没有变化。U02的有界启动修复与U04的契约边界还需收尾，不能整体验收。**

本轮只审查、运行隔离测试并保存文档/证据，没有修改开发worktree业务代码、真实用户配置或数据库。[原始证据](review-round5-2026-10-15/README.md)。

## 1. 本轮实际验证

| 验证 | 结果 |
| --- | --- |
| 核心单测 | 114 passed |
| GPUI单测，RUST_MIN_STACK=67108864 | 37 passed |
| 统计集成 | 39项通过 |
| 核心release、fmt、clippy | 通过 |
| 第四轮API/分页/孤立/Writer探针按当前源码复跑 | 见下面逐项结果 |

本轮未重跑GPUI release/clippy、其余五类smoke/兼容脚本，也没有新增百万行、Rust1.88、Windows、真实供应商或人工GUI证据。

对第四轮源文件SHA作比较：`store.rs/mod.rs/query.rs/statistics_test.py`改变；`gpui/src/statistics.rs`、`stats_backend.rs`、`usage_table.rs`与上一轮逐字节相同。因此界面待办不是测试缺证据，而是尚未修改实现。

## 2. 已确认改善，下一轮保留

- **U01**：有非零手动价格、实际收到上游字节后SIGKILL/重启，现在是`interrupted/manual/usage_incomplete/NULL`，不再伪造未发上游和0元。
- **U02主要正确性**：构造上一版已存在的`interrupted+cost.pending`，重启现在结算为`usage_incomplete/NULL`。单测另覆盖旧错误0元组合和重复恢复幂等；但启动处理规模限制仍未完成，见下。
- **U03**：第一页as_of_id=3，后新增两次第二模型调用，第二页现为请求1、settled1、USD2800000 nanos，水位一致，不再计成三条金额。
- **U04主要路径**：不带ID/revision重复新建明确revision_conflict；canonical price_id不再静默被忽略，改模型被明确拒绝；空白输入价现在保存0。正式39项集成也覆盖canonical正常编辑、未知ID及其他相关场景。
- **此前修复**：清理应删600实删600，retention应删1200实删1200，范围外不删；缺Key且手动价仍正确rejected/not_applicable/0；缺价汇总仍明确CNY0。

## 3. U05 [P1] 孤立费用失败后仍ready且不再补偿，实际未修

位置：`src/stats/mod.rs`的`recover_collection`、writer_loop探测条件、`trim_orphans`；`src/stats/store.rs`的`apply_batch`与`WriteBudget`。

本轮重新运行真实核心故障探针：只对临时SQLite创建触发器，拒绝费用状态更新，使Finish和随后孤立补偿失败。结果依旧：

```text
故障时：health=ready，request=in_flight，cost=pending
移除触发器并等待：health=ready，request=in_flight，cost=pending
```

recover_collection只把区间记录结果计入recorded；孤立结算失败虽然保留ID，却清掉degraded_since并切ready。空闲探测因此不再运行。一个请求就可复现，不需要>512。

**本轮必须完成：**

1. 将恢复待办明确拆成区间持久化、请求/费用补偿、溢出补偿；任一步失败保留待办，不切ready。恢复探测由“有待恢复工作”驱动，不能只看degraded_since；无新请求也重试。
2. 失败期间暂存与调度有界、真实退避，不空转。区间已记录不要反复插入，费用补偿保持幂等。
3. trim_orphans先drain再写失败仍丢ID，必须有可靠补偿兜底；不能仅记日志、加计数、等下次重启。也不能为限内存而误结算仍运行的流。
4. WriteBudget当前仍未缩短SQLite锁等待。本轮1ms预算持锁实际耗**280ms**。按剩余时间设置等待并统一约束批次、隔离、关闭阶段，避免多次等待乘出预算。
5. 隔离循环用outcome.applied作为已遍历下标仍不正确，前面有坏事件后会重算/重复计丢失。使用真实索引，只隔离确定性约束错误，精确统计每条未落库事件。

验收先补失败回归：本轮单条cost触发器→移除故障→无需新请求恢复；区间成功但cost失败；>512终态持续锁；坏事件后遇锁/超时；关闭预算；请求/费用终态同事务。不能只重跑目前39项就关闭U05。

## 4. U02 [P2] 启动修复仍是全量收集和单一长事务

位置：`src/stats/store.rs:181-260`。

新的旧v2修复正确处理了小样例，但两个查询将所有request_id放入Vec、在一个Immediate事务逐行结算，无LIMIT、批次水位、时间预算或进度。大量旧pending/错误0元时，内存和独占写事务随待修数量增长。这是源代码确认的有界性缺口，本轮没有把它包装成已测量的百万行耗时。

按上一轮有界要求完成启动修复：以稳定ID/水位分页，每批行数/内存/事务受限，保留恢复进度和幂等；修复未完成时明确migrating/degraded等真实状态，不能提前接受统计或宣称完整数据。失败可恢复，当前价不参与，正常历史不改。不必为了修正数据强行改schema版本；但选择的机制要覆盖现有v2库，不能只改0002。

补多批、批间失败/重启、重复启动、历史快照删除来源、正常已结算金额不变的回归，并记录大样例的时间/内存边界。

## 5. U04 [P2] 新建/编辑边界和delete canonical字段仍不完整

新增专项真实API复现：

1. **没有price_id/rule_id，却带existing revision**：已有输入价2000000000/revision1，仍按“新建”调用只加expected_revision=1，即可覆盖同组合键为99000000000/revision2。代码仍保留ON CONFLICT UPDATE；这不符合正式API“新建revision0，重复组合键冲突；编辑精确price_id+revision”。
2. **canonical delete失败**：`stats_price_delete(price_id=现有ID, expected_revision=2)`返回`stats_invalid_argument: rule_id is required`。save已兼容canonical字段，delete却仍只读旧rule_id。当前GUI仍用旧字段所以原测试没发现。

修复：新建无ID必须expected_revision=0（缺省可按0），非零直接拒绝；重复键只报冲突，不走更新。编辑必须显式ID+revision。save/delete共用canonical/别名解析器，两个字段冲突均拒绝；DTO/list命名与正式v2契约统一，过渡别名明确标注。补“不带ID不能更新”“canonical save/delete完整往返”“双ID冲突”“旧别名兼容”“错误或过期revision不写、不发布缓存”的API回归。

## 6. U06 [P2] 价格界面尚未修改，请先做独立草稿及控制器测试

已核对GUI文件SHA与上一轮相同；[第四轮报告](REVIEW-ROUND4-2026-10-15.md)U06全部继续适用：

- edit_price仍改全局provider/provider_select，不触发mark_dirty；取消可留下新筛选配旧报表。
- 没有独立PriceDraft；编辑price_open状态未正确管理。
- 输入组合键允许改但后台拒绝，需要禁用编辑键或明确交互，不能靠通用错误提示掩盖。
- 价格GUI仍只取首100条、无默认币种入口，i128总额仍有i64解析导致0/未知的显示问题。

要求：独立草稿保存id/revision/provider/model/currency/mode/四价/label，取消/编辑不改变报表filter；保存失败保留输入，成功才关；前后台版本冲突可纠正。补CNY/disjoint与USD/flat交替编辑、只改label、取消、新建、失败、并发、供应商不在当前选项等**控制器自动测试**。添加价格列表分页、默认币种设置和i128精确格式化，不引入WebView/JS图表。

## 7. U07 [P2] 统计界面主待办仍全部存在，下一轮必须进入实现

- 分组查询没有cursor，回复没保存next，模型/供应商分页不能遍历三页。
- 没有完整QueryIdentity；旧回复仍清新pending，Rows/Groups仍按当前tab解析，debounce期间身份未立即失效。
- 日期非法沿用旧值，反向范围钳制1秒；应就地报错且不发新查询。
- 筛选搜索/next_key、供应商变化即时模型收窄、语言重建、前台/最小化/模态、离页取消只读任务仍未完成。
- reader仍只在排队前校验游标，实际执行时缺维护版本复查。

按[第三轮T08](REVIEW-ROUND3-2026-10-15.md)、[第四轮U07](REVIEW-ROUND4-2026-10-15.md)和原合并Prompt落实。先建可测试控制器/QueryIdentity，再完成分页与交互；不要继续只在文档中搬移“未完成”。控制器迟到回复测试不能由scheduler队列测试或37个旧GPUI单测替代。

## 8. 直接执行的下一轮任务

继续现有worktree `/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics`、分支`codex/usage-statistics`。先读**主工作区**本报告、第四轮报告和PRICING-DECISION/API，不用开发worktree旧副本覆盖最新设计。

顺序：**U05可靠补偿 → U02有界修复/U04接口收尾 → U06 PriceDraft及测试 → U07完整页面控制器和交互 → 回归/性能/交付。** U01、U03已确认改善，不再推倒重写。

先补旧代码会失败的正式回归，再改源码；探针exit0代表观测，不代表缺陷关闭。每批记录真实闭环。用户要继续修改，不要每批问是否继续，也不要只修后端后再次把全部GUI留在未完成表中。

最终执行核心/GPUI fmt/test/clippy/release、正式统计和既有转发/设置生命周期回归、新增故障/价格API/控制器回归；完成三类新价格10万/100万预算、i128、转发P95/RSS证据。Rust1.88、Windows、真实供应商、三尺寸人工GUI如无条件仍明确未验证，不能伪造通过。

更新IMPLEMENTATION-STATUS：区分U01/U03的确认关闭，U02/U04主样例改善但尚有边界，U05/U06/U07仍待完成；纠正旧表中F02/F10“已修复”等与现状矛盾的声明为历史记录。最后给逐项闭环、命令/结果、版本及git status。上下文不足保存精确断点，不以轮次结束代替需求完成。

保留用户修改，不reset/stash、不改主工作区业务代码、不提交/推送/发布/安装、不操作真实用户配置/数据库；数据修正仅在隔离样例中验证。

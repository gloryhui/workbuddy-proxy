# 第七轮验收复核与剩余施工

日期：2026-10-15。对象：`/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics`，`codex/usage-statistics`，HEAD=e1725c3，功能未提交；schema3/API2/control6。

**整个功能尚未完成。** DeepSeek最新消息实际写的是“本轮完成S01/S02等后端修复，U06/U07仍未施工”，不是整个统计功能验收完成。后端复核也仍有两处可复现缺陷，见下；界面需要实际进入编码。

本轮只审查、运行隔离测试、保存文档和探针，没有改业务代码或真实配置/数据库。[本轮证据](review-round7-2026-10-15/README.md)。

## 1. 本轮实跑与可以保留的修复

| 验证 | 结果 |
| --- | --- |
| 核心单测 | 119 passed |
| GPUI单测，RUST_MIN_STACK=67108864 | 37 passed，本次未复现上轮测试桩问题 |
| statistics集成 | 39项通过 |
| 核心release/fmt/clippy | 通过 |
| 已有v2库升级 | ready/user_version=3/error=null，上轮S01原样例已修 |
| 1200条旧终态pending启动修复 | 1200条全部default_zero，队列0，ready，上轮原样例已修 |
| 20000条旧终态pending | 16640条结算、3360条仍pending、队列0、ready，继续等待不变 |
| Start与Finish均写失败 | HTTP200，解除锁后仍degraded；请求0、backlog0、write_failures2，未修 |

本轮未重跑其余兼容脚本、GPUI release/clippy、百万行三类价格性能、Rust1.88、Windows、真实供应商或三尺寸人工界面。`statistics.rs`和`usage_table.rs`SHA与上轮相同；`stats_backend.rs`变化集中于测试桩，U06/U07仍无主体实现。

## 2. R7-01 [P1] 把256上限扩大成64轮后，超出16640条仍假完成

位置：`src/stats/store.rs:199-344`，`RECOVERY_ROUNDS=64`；writer恢复探测目前只认degraded_since、内存orphans和已登记backlog。

真实核心、合法FK样例：20000条旧“请求已终态、cost.pending”，启动后为：

```text
state=ready
default_zero=16640
pending=3360
stats_recovery_backlog=0
继续等待仍相同
```

原因：先修256条，再64×256条；到轮数上限后没有保存剩余扫描任务，也没有重新核验数据库里未登记的待修事实，只按backlog长度0判完成。上轮“首批剩余未登记”的根因仍在，只是触发阈值变大。

同时，这64轮全部在同一个Immediate事务里，原全量UPDATE in_flight也仍存在。给每次SELECT加LIMIT，不等于事务/启动总耗时受限。本轮不伪称已经测量百万行耗时，但代码结构明确未实现原要求的真正分批交还调度。

**修复要求：**

1. 为启动恢复保留独立的scan_pending状态、稳定cursor与旧数据as_of水位/运行范围。每次只处理一批或明确时间预算；事务提交后交还writer处理实时事件/控制。
2. 到预算或轮数上限仍有未扫描数据时，保存继续扫描的工作；不能把“没有登记进队列”当成“没有待办”。下次探测继续扫描，不依赖新请求/重启。
3. ready必须满足扫描已完成、已登记待办全部确认完成；任意查询失败不能当0。新接纳活跃请求不进入旧数据恢复范围。
4. 不以增大RECOVERY_ROUNDS、无界循环、一次全量INSERT/UPDATE替代；数据修复按原快照，不用当前价格，不改正常历史。

先补正式回归：至少20000条、大于当前阈值；混合pending/旧错误0元/in_flight/已正确结算、批间失败和重启、扫描/排空交错；最终不能残留合格pending，未完成不能ready。补writer让出控制与内存/事务边界证据。

## 3. R7-02 [P2] Missing分类只接入backlog，内存孤立入口还在走旧逻辑

位置：`src/stats/mod.rs:1671-1730`；`src/stats/store.rs`的`settle_orphans`和`repair_one`。

本轮按正式事件链复跑：外部锁覆盖一次正常HTTP200调用的Start与Finish落库，两批均失败，解除锁后两次观察仍是：

```text
state=degraded
usage_requests=0
stats_recovery_backlog=0
write_failures=2
```

新repair_one已经能识别Missing/Done/Retry，但recover_collection的**内存orphans**仍调用旧settle_orphans，并比较“本次改变行数”和pending.len。不存在的ID永远改变0行，所以永远保留。给持久化队列增加Missing单测没有覆盖实际进入内存列表的入口。

统一内存、持久化、启动三个入口的恢复语义与实际调用链：Missing清待办但保留loss计数/区间，不伪造请求；Done必须请求和费用均终态；Retry保留并退避；失败不称ready。已终态、用户清理后缺失、终态pending、in_flight丢Finish、部分成功混合列表均覆盖。

需要真实核心的“锁住Start和Finish→解除锁→无新请求也回ready、无伪造行、loss仍可见”回归，不能只测repair_one或手动push_backlog辅助函数。

## 4. U06：直接开始价格界面实现

修完上述两个后端阻断后，本轮必须出现可复核的GUI源码与新增控制器测试，不要再次只写“下一轮做”。建议将纯状态从GPUI视图提取成`stats_controller.rs`等可测模块；模块必须被真实页面调用，不能另建一套测试专用实现。

### 价格草稿

建立独立PriceDraft，至少保存price_id、expected_revision、provider_id、upstream_model、currency、input_mode、四项金额输入、label、保存状态与错误。GPUI输入Entity/Select只映射此草稿，不复用报表的provider/model过滤器。

- 新建：独立供应商选择，四项空白默认0，expected_revision=0；组合键冲突保持草稿。
- 编辑：完整回填全部字段；当前不支持改组合键就禁用对应输入并清楚显示；price_open正确设true。
- 取消：不改变报表筛选、generation、游标或已显示数据。
- 保存：使用canonical price_id；失败保留输入与错误，确认成功才关闭并更新价格列表；旧回复不能关闭更新的草稿。
- 连续编辑CNY/disjoint与USD/flat、只改label、取消重开、保存失败、并发冲突、供应商不在当前筛选选项分别测。

### 列表、默认币种、金额

接通价格列表next游标及加载更多/分页，不能停在首100条；按明确供应商/模型查询。增加默认币种CNY/USD设置入口，提交成功才更新UI，新请求冻结新币种、历史不变。

统一i128定点格式化，汇总字符串超i64不能显示0/“—”；价格输入的人类金额仅转换一次。测试超i64总额、0、最小显示阈值、CNY/USD并列。

## 5. U07：页面控制器、分页和筛选交互

### QueryIdentity与迟到回复

身份至少含filter_generation、tab/group_by、sort、cursor、seq。改筛选/切tab立即失效旧身份，不等debounce发出新请求才失效。回复根据自身kind/identity解析，旧回复不能改数据、错误、loading或清新pending。

由真实Statistics页面请求/回复处理调用该控制器。先测：筛选变化debounce期、Rows↔Groups、provider↔model、翻页/刷新交错、旧错误最后到达、旧草稿保存回复。反序列化错误要可观察，不能静默吞掉。GPUI测试注意显式导入，避免`use super::*`引入宏遮蔽。

### 分页

请求和breakdown分别保存current/next cursor及前页起始游标栈，模型/供应商各自至少三页1→2→3→2→1、20/50/100页大小。请求分组查询必须实际发送cursor，回复必须保存next。切筛选/tab/sort正确重置；维护/重启导致过期时提示并回第一页。reader执行时再次复查维护版本，覆盖排队后clear/retention。

### 筛选与生命周期

非法/反向/>366天日期就地报错、不发新查询，保留已应用范围；合法后再Apply或debounce，覆盖DST。provider/model搜索及next_key分页；切供应商重载模型选项、处理不兼容选择并隔离旧回复。

语言切换重建Select、placeholder、对话框，保留选择/草稿；自动刷新仅前台、未最小化、统计页且无冲突模态；离页取消未派发只读任务和debounce，已授权写操作不静默丢。按锁定GPUI Kit0.7.3源码实现，不用WebView/JS图表。

## 6. 下一轮执行与交付

先读**主工作区**本报告及[第六轮报告](REVIEW-ROUND6-2026-10-15.md)、PRICING-DECISION/API。继续现有worktree和分支；U01/U03/价格主路径、v2迁移、1200恢复、测试桩修复已改善，保留即可。

顺序：R7-01/R7-02先补失败回归并修接线 → U06 PriceDraft与至少对应场景控制器测试 → U07 QueryIdentity与分页/交互 → 全面回归及证据。可分小批修改，但本轮目标包含GUI实际实现，不能以“本轮后端已完成”代替原统计功能完成。

最终执行核心/GPUI fmt/test/clippy/release、正式统计与新增恢复/控制器回归、转发/兼容/设置生命周期；完成三类新价格10万/100万预算、i128、P95/RSS。Rust1.88、Windows、真实供应商、三尺寸人工GUI无条件时如实标未验证，不阻止可编码功能继续推进；截图不是自动状态机测试的替代。

更新IMPLEMENTATION-STATUS，撤回“批量恢复已完全关闭”“缺失ID已全部接通”等不符实测声明。交付逐项闭环、实际源码文件、命令/结果、未验证边界、schema/API/control及git status。无需逐批询问继续；上下文不足保存精确断点。

不reset/stash、不覆盖用户修改、不改主工作区业务代码、不提交/推送/发布/安装、不操作真实用户配置/数据库；迁移/故障均在隔离样例验证。

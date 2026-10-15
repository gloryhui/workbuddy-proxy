# 第八轮：GUI已开始施工，但交互接线和恢复仍未通过验收

日期：2026-10-15。审查对象：`/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics`，分支`codex/usage-statistics`，HEAD=`e1725c3`，未提交。schema3 / API2 / control6。

这轮已有真实GUI代码，不能再说U06/U07完全没施工；也不能称完成。新增`stats_controller.rs`被页面调用，价格草稿、分组游标、i128金额显示已开始接入，但真实按钮和回复路径仍绕过或破坏控制器语义。

本轮只审查、隔离测试、保存报告/探针，没有修改实现worktree业务代码或真实用户配置/数据库。[原始证据及复跑说明](review-round8-2026-10-15/README.md)。

## 1. 独立验证

| 检查 | 本轮结果 |
| --- | --- |
| 核心`cargo test` | 121 passed |
| `RUST_MIN_STACK=67108864 cargo test --manifest-path gpui/Cargo.toml` | 49 passed |
| `statistics_test.py --exe target/debug/workbuddy-proxy` | 39 passed，完整日志已保存 |
| 核心`cargo build` | 通过，有dead_code警告 |
| 核心`cargo clippy --all-targets -- -D warnings` | **失败，exit101**，`src/stats/dto.rs:72 format_amount_text`未使用 |
| 锁住Start+Finish后解除锁 | HTTP200；degraded→ready；请求行0、backlog0、loss仍可见，原R7-02已有改善 |
| 启动扫描中单行结算失败 | **真实核心ready、pending1、backlog0；解除故障后仍ready/pending1** |
| 1200条旧pending实际writer调度 | 0.81秒余688，4.5秒余176，9秒清空ready |
| 20000条旧pending实际writer调度 | 0.81秒余19488，9.01秒仍余18720、degraded；未等待全程 |

本轮未声称完成GPUI人工操作/截图验收、release、GPUI clippy/fmt、Windows、Rust1.88、真实供应商、百万行性能/P95/RSS。DeepSeek报告的这些历史验证不能替代本轮证据。

## 2. R8-01 [P1] 价格对话框保存链仍立即关闭并丢失草稿

位置：`gpui/src/statistics.rs:2904-2930,3055-3063,3067-3101,1252-1263`。

可见“保存”按钮调用`save_price(cx)`后无条件`window.close_dialog(cx)`。`on_close`又调用`cancel_price`，将PriceDraft替换为空白。结果：校验失败、队列满、服务错误/版本冲突也关闭；正常提交的回复到达时草稿tag已被清除，无法触发正确成功流程。另一方面，`on_ok`返回false，成功处理只改未驱动真实窗口的`price_open=false`，没有关闭真正的对话框。`price_open`从未被设true；`price_pending`只设true，未形成完整等待状态。现有单测只测`PriceDraft.apply_reply`，没有经过这些入口。

要求：统一按钮、Enter/on_ok、关闭/X/Escape的真实事件链。保存校验/入队失败留在表单并显示错误；等待中禁用重复提交；匹配成功回复才关闭真实dialog，刷新价格列表；失败保持全部输入、pending复位；旧回复不改新草稿/错误/loading。若允许等待中取消，已授权写请求仍执行，结果刷新列表但不能关闭新草稿。入队失败也必须撤销draft.pending/tag。不要靠仅设置一个未被读取的bool代替实际窗口操作。

草稿使用独立计数，不要在new/edit里递增报表`generation`；取消不得改变报表generation/筛选/游标。补页面真实调用的动作状态测试，覆盖两种提交入口、校验错误、队列满、成功、冲突、取消重开后旧成功/旧失败。

## 3. R8-02 [P1] 新建价格的供应商选择根本没有渲染/填充

位置：`gpui/src/statistics.rs:400-414,2932-3065,3173-3241`。

`price_provider`只初始化一个空选项，未用供应商返回数据填充，也未出现在`price_body`中。全部供应商筛选下新建价格，`PriceDraft.provider_id`为空、is_complete拒绝，用户没有控件可选供应商。编辑依赖draft内原provider兜底，但这不是完成的独立价格选择器。

要求：真实渲染独立供应商Select，加载配置供应商及需要保留的历史项；当前价格供应商不在首100选项时也补入并显示。新建可选供应商；编辑组合键固定时明确显示/禁用。与报表筛选彻底分离，测试“报表全部供应商→新建并保存”“编辑不在当前筛选列表的供应商”“取消不改报表”。

删除表单中“按当前价格重算历史”的误导提示：`stats_reprice_start`已经被后端拒绝，当前价格不回写历史是锁定要求。

## 4. R8-03 [P1] ReplyGate已接入，但真实handle_reply仍破坏迟到回复隔离

位置：`gpui/src/statistics.rs:680-690,1024-1105,1127-1179,2188-2198`。

- `stale`分支仍把`pending_dashboard/pending_rows=false`，会清掉更新请求的等待状态。
- Requests↔Groups切tab只reset分页并发送当前种类请求，没有使离开的gate失效。Rows A在途→切Models并发Groups B→Rows A仍通过自己的gate；两类tag都为`rows-*`，apply_reply又按**当前self.tab**解析，旧Rows会尝试当Breakdown解析并清B的等待状态；旧错误也能覆盖当前页。
- mark_dirty只失效rows/groups，dashboard_seq保持旧值，筛选变化后debounce窗口内旧dashboard仍可应用。
- 反序列化失败仍静默`if let Ok`，会出现停止loading但不显示错误/不更新数据。
- 分组游标过期分支无条件`request_rows`，即使当前在Models/Providers，也会发错资源。

要求：由真实页面使用统一的查询状态控制器（不是额外测试模型），reply以自身kind+不可复用identity路由，身份在筛选/tab/group/sort/page大小改变的当下失效。旧成功/旧错误一律不动当前数据、错误、loading、pending；等待状态只由对应当前请求结束/显式取消清理。Dashboard、Rows、Groups和筛选选项均覆盖。cursor过期按当前tab重置并请求正确类型，显示提示。解码失败按当前kind显示可观察错误。

补真正穿过页面使用的请求构建/回复处理动作的测试：Rows→Groups、Groups→Rows、Models→Providers、debounce期旧dashboard、旧错误最后到、分页/刷新交错、过期Groups、畸形JSON。仅测试`may_apply_seq`布尔值不能证明页面状态正确。

## 5. R8-04 [P1] 启动扫描失败后仍越过该行，未登记重试，真实核心假ready

位置：`src/stats/store.rs:1578-1602`。

每行repair_one返回Retry或Err只跳过/打印，然后cursor无条件推进，短批直接finished，没有把失败行放入backlog。另一个边界是扫描cursor在tx.commit成功之前已改动，提交失败时也不得发布这个进度。

正式schema、隔离核心、终态+pending一条，启动前装BEFORE UPDATE临时失败trigger，启动扫描遇错：

```json
{"first_state":"ready","pending_before":1,"state_after_trigger_removed":"ready","pending_after":1,"backlog":0}
```

源模块直接探针也为finished=true/pending1/backlog0；解除故障再次扫描返回0，不再处理该行。证据：`startup_failure_probe.py`和`store-probe`。

要求：失败/Retry在同一可提交事务内可靠登记持久化待办后才允许越过，或不推进相应扫描水位；登记/提交失败时保持旧水位并degraded。SQL statement可能已部分改变数据，必要时每行savepoint，不能吞错误当完成。只有事务提交成功才发布新cursor。稳定区间扫描与实时请求隔离保持。正式回归覆盖首/中/末行失败、混合成功、解除故障后无新流量自动恢复、批次提交失败/重启；待修存在绝不ready，历史仍只用自身快照。

## 6. R8-05 [P2] 正常批次仍走指数故障退避，扫描越成功越慢

位置：`src/stats/mod.rs:1554-1581,1681-1717`。

`recover_collection`的false同时表示“正常还有下一批”和“真的失败”。writer对所有false将probe_backoff×2，上限30秒。成功每批256，正常大队列后期也变成每30秒仅处理一批。2万条采样见§1；完整几十分钟耗时是依据调度代码推算，本轮未等待全程，不能伪称实测。

现有2万条单测通过`run_recovery_scan`紧循环直接调用store，因此没有验证生产writer调度。

要求：区分Progress/Pending/RetryFailure/Complete；正常有进度时及时让出控制后继续调度，只有真正失败才指数退避。保持事务/每轮预算，不改成一次无界排空，不增加巨大轮数。补真实StatsHandle/writer入口20k回归和合理上限，观察控制消息/实时事件能在恢复批间得到服务。

## 7. R8-06 [P2] 新共用格式化函数让核心严格检查退化

`src/stats/dto.rs:72`的`format_amount_text`被GPUI用，但核心编译单元未用，`cargo clippy --all-targets -- -D warnings`退出101。按现有跨二进制共享模式合理处理，不全局压制warnings；补i128>i64/零/阈值/CNY+USD显示测试，不能只宣称使用i128。恢复核心和GPUI对应检查。

## 8. U06/U07仍需完成的明确功能

1. 价格列表`next_rule_id`分页/搜索，不停在首100条；默认币种CNY/USD入口，revision冲突和保存成功后的本地状态正确，新请求冻结新币种、历史保持原快照。
2. 日期草稿与已应用范围分离，非法/反向/>366天就地错误，不发新统计查询，不把反向范围`.max(from+1000)`偷偷改成合法；支持本地时区/DST。
3. provider/model搜索和`next_key`分页、切供应商立即清理不兼容模型并重载；独立身份隔离旧选项回复。
4. 语言切换重建Select/items/placeholder/dialog，保留选择/草稿。
5. 前台/最小化/当前统计页/模态共同决定自动刷新；离页取消未派发只读任务/debounce、失效在途只读回复，保留已授权写操作。当前tick先pump后看visible，单纯set_visible并没有取消调度器待发任务。
6. Reader执行时复查维护revision/run绑定，覆盖请求入队后clear/retention；不要只在命令入口检查。
7. 分组和请求20/50/100大小，真实页面请求/回复至少三页1→2→3→2→1，含翻页错误恢复、cursor过期和刷新交错。CursorStack容器单测不是页面验收。

## 9. 下一轮执行顺序与交付约束

保持现有worktree、所有已有修复及未提交改动。不要重新通读全部历史报告，不要先重跑所有后端脚本消耗一轮：

A. **先修R8-01/02/03真实GUI接线并补对应动作测试**，这是本轮主要工作。
B. **完成价格分页+默认币种、日期校验、筛选/语言/生命周期**，逐块聚焦测试；不要在报告中再次把尚可编码内容全部推给下一轮。
C. 修R8-04/05这两个明确恢复缺陷与R8-06严格检查；只做与证据对应的必要后端改动。
D. reader维护检查和分页回放；最后才全套回归。分批编辑不等于每批结束会话，继续直到本轮目标完成或有真实外部阻断。

更新IMPLEMENTATION-STATUS，区分已接入/已单测/实际writer或页面动作回放/人工GUI验证；不把49个测试或release构建当界面验收。保存准确退出码，不用`cargo ... | tail`的末端退出码遮蔽失败；测试异常保留原输出再判定，不能无证据称无关。

不要reset/stash/覆盖用户改动，不修改主工作区业务代码，不提交/推送/发布/安装，不用真实用户数据库/配置/供应商跑故障测试。只在隔离样例验证迁移/锁/trigger。无须逐批询问继续。

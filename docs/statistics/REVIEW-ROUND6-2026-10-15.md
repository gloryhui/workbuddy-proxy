# 第六轮复核与继续施工任务

日期：2026-10-15。对象：`/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics`，分支`codex/usage-statistics`，HEAD=e1725c3，功能未提交。当前schema=3/API=2/control=6。

**结论：单条孤立故障恢复、1ms锁等待预算和价格接口边界已有改善；新增schema v3却不能升级现有v2库，批量恢复还会漏掉绝大多数待修费用并假装ready。GUI两组任务仍未修改实现。不能验收。**

本轮仅审查、运行隔离测试、生成文档/探针，没有改业务源码或真实用户配置/数据库。[本轮原始证据](review-round6-2026-10-15/README.md)。本报告是下一轮直接施工要求。

## 1. 本轮验证与确认改善

| 验证 | 实际结果 |
| --- | --- |
| 核心test | 117 passed |
| GPUI test | 首跑36 passed/1 failed；聚焦复跑失败、全量再跑失败、之后一次全量37 passed，**存在不稳定测试，不能笼统写全绿** |
| statistics集成 | 39项通过 |
| 核心release、fmt、clippy | 通过 |
| 旧单条孤立cost故障探针 | 故障时degraded；解除故障且无新请求时ready/interrupted/default_zero/0，原问题有改善 |
| 直接引用当前Writer源码的锁预算探针 | 1ms预算耗1ms，不再280ms |
| 价格边界 | 无ID却带revision的新建已被拒；canonical price_id删除成功 |
| 清理/保留旧探针 | 应删600实删600，应删1200实删1200；范围外不删 |
| 新增v2迁移/1200旧费用恢复/丢Start+Finish探针 | 复现下面S01–S03 |

本轮没有重新执行GPUI release/clippy、其余五类兼容脚本、百万行三类价格基准、Rust1.88、Windows、真实供应商或三尺寸人工GUI验收。GUI三个主要文件SHA仍与前轮相同。

## 2. S01 [P1] 已有v2库升级会重跑0002，统计被禁用

位置：`src/stats/store.rs:125-174`，migrate无条件执行MIGRATION_0002再0003。

实际核心：用未修改的正式0001+0002生成合法空v2数据库，设置PRAGMA user_version=2，启动当前核心。结果：

```text
stats_health.state=disabled
PRAGMA user_version=2（没有升级成功）
error=table stats_model_prices already exists ...
```

这覆盖用户现有开发数据库的实际升级路径。0003里的IF NOT EXISTS不代表整条迁移幂等；不能在v2库上再执行创建stats_model_prices和重建usage_costs的0002。

**修复：**按数据库版本逐步应用，仅version<1执行0001、<2执行0002、<3执行0003；每步版本推进和事务/外键设置正确，失败回滚并保留备份。不删表/删库来绕过。补真实0→3、1→3、**2→3**、3→3重复启动、新版本拒绝、失败重试与已有Token/金额/快照/价格不变测试。

还发现错误信息边界：本次health.error_code直接包含**11421字符、带CREATE TABLE的整份迁移SQL**。对外固定错误码和短提示，SQL/路径只留内部诊断，不能把DbError全文当错误码。补迁移失败响应不含SQL的断言。

## 3. S02 [P1] 启动恢复只修首256条，backlog又丢掉未结算费用

位置：`src/stats/store.rs:188-296`、`:711-738`、`:1589-1619`，以及writer启动状态。

实际核心、合法FK隔离样例：生成1200条“请求已终态、费用pending”的旧记录后重启。结果：

```text
state=ready
default_zero=256
pending=944
backlog=0
继续等待仍相同
```

**三个不同缺口：**

1. recover只修首256条，再只把下一批256条写入backlog，没有持续扫描剩余记录的任务/游标；后面的待办根本没有登记。
2. drain_backlog调用settle_orphans；后者只有request.outcome=in_flight更新成功才调用费用结算。旧终态pending的request已经是终态，所以不会结算费用；drain却无条件删除backlog ID，把“请求终态”错误等同“请求与费用都完成”。
3. writer启动先set_state(Ready)，recovery.queued>0只设置degraded_since，并没有像进度声明那样显式进入degraded。并且recover里更新所有in_flight的UPDATE仍不受LIMIT约束，仅给SELECT加LIMIT没有让整段修复有界。

**修复：**

- 将启动恢复建成真正可持续推进的有界扫描任务，固定旧数据水位/运行范围、分批扫描登记，直到扫描完成且队列排空才可ready。不能只取首批剩余ID，也不能用一次无界INSERT SELECT替代。
- 用统一的请求/费用恢复函数，正确处理in_flight、终态pending、已正确结算、已清理/不存在；必须验证费用终态才删除待办。保留本次快照，不读当前价格。
- 分批事务内原子地完成费用修复和待办确认，或证明失败/重启时可幂等恢复；不要无条件删掉没修完的ID。
- 真正限制每次SELECT、UPDATE、Vec和事务的规模；修复未结束时状态准确，新接纳的活跃请求不能落入旧数据修复范围。

验收至少1200/大于三批，分别测试旧终态pending、旧错误not_applicable、旧in_flight以及混合记录；扫描与排空分别故障/重启；费用未知/缺价0/多币种、正常金额不变；最终全部合格费用终态、无未登记残留，扫描未完成不ready。现有“直接push600个in_flight进backlog”的单测没有覆盖这里的启动入口与终态费用路径。

## 4. S03 [P2] Start和Finish都丢失时，恢复会永久保留不存在的ID

位置：`recover_collection`比较settled与pending.len；`settle_orphans`只返回“本次UPDATE改变行数”。

实际核心：临时外部写锁覆盖一次正常HTTP200调用的Start与Finish落库，两者都失败后解除锁。两次观察仍：

```text
health=degraded
usage_requests=0
stats_recovery_backlog=0
write_failures=2
```

恢复把数据库中根本没有Start行的ID当成“未修完”，每次UPDATE=0就重新保留；已正常终态的重复ID、用户清理后不存在的ID也有相同判定风险。

**修复：**区分“本次改了几行”和“该ID是否还需要修复”。不存在但已确认Start丢失的请求，保留丢失计数/区间，清除不可能再执行的待办；已正确终态幂等完成；终态pending仍必须按S02结算；真正需要补偿的in_flight才进入修复。不能伪造一条0用量/0金额请求填洞。补无Start、已终态、被清理、终态pending、部分成功混合ID的回归。

## 5. S04 [P2] 故障回退内存仍无上限，backlog查询失败又当0

位置：`src/stats/mod.rs:1586-1616`，writer_loop/recover_collection的backlog_len(...).unwrap_or(0)。静态源码确认，本轮没有声称跑了长期内存压力。

trim_orphans在SQLite持续不可写时，将刚drain的溢出ID再次extend回Vec。代码注释和交付说明也明确“宁可变长”，这违反统计旁路有界、故障不拖累聊天的要求；同时每批对越来越长的溢出尝试结算/持久化，会放大故障成本。

需要有界降级策略/可证明的安全水位或粗粒度丢失标记：保持明确loss_observed与未知覆盖，不要求在完全不可写时无损保留无限请求ID；不能无界占内存，也不能误结算活跃流。恢复登记/扫描任务统一纳入S02。

backlog_len失败不能unwrap_or(0)并据此称“无待办”，应保留故障状态与重试原因；健康的0必须是成功查询证明。补持续写锁超过512终态、DB不可达、计数读取失败、解除故障与内存上限证据。

## 6. S05 [P2] GPUI错误传递测试不稳定

位置：`gpui/src/stats_backend.rs:661-686`，statistics_errors_keep_the_core_message。

本轮首跑此项失败，报Core is not responding；聚焦单项再跑报Invalid core response；再跑全套仍失败，之后一次全套才37项通过。复跑日志已保存，不能只报最后成功那次。

测试桩只stream.read一次，没有确认完整header/Content-Length body就回400并关闭，存在分包敏感路径；它与旁边会完整消费请求的成功测试不同。先让测试桩按HTTP长度完整读取请求、正确发送完响应并join线程，验证真实错误信息仍保留。不要改成接受通用错误、删测试、加固定sleep或无限重试掩盖。修复后做有限次数重复与全套测试；如仍失败继续查transport错误路径。

## 7. U06/U07 [P2] GUI实现依旧未动，本轮必须施工

`gpui/src/statistics.rs`、`stats_backend.rs`、`usage_table.rs`与上轮源码SHA一致。独立PriceDraft、界面QueryIdentity、分组分页等不是“已有实现待截图”，而是尚未实现。按[第五轮报告](REVIEW-ROUND5-2026-10-15.md)的完整验收继续：

- 独立PriceDraft，不改全局报表filter；新增/编辑/取消/失败/并发/价格模式与币种完整管理，成功才关表单。
- 价格列表分页、默认币种设置、i128定点格式化，超i64不显示0或未知。
- 完整QueryIdentity与可控迟到回复控制器测试，旧回复不清新pending、不跨tab解析。
- 模型/供应商三页双向分页、日期合法后应用、筛选搜索next_key、供应商变化模型收窄。
- 语言重建、前台/最小化/模态、离页只读取消，以及reader真正执行时复查维护版本。

先补控制器测试再连交互；不能用37个旧GPUI测试、实际API调用或截图权限问题替代源码实现。

## 8. 直接执行任务

继续`/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics`、`codex/usage-statistics`，先完整读**主工作区**本报告及证据，不用开发worktree旧副本。

本轮目标：**S01旧库迁移 → S02批量恢复 → S03/S04恢复完整性与资源上限 → S05测试稳定性 → U06/U07实际GUI → 完整回归和证据**。保留已确认改善的价格、单条故障恢复、锁预算、清理和分页金额；不推倒重写。

每项先补旧代码会失败的正式回归，再改源码。特别加入v2→v3和1200旧终态pending，而不是继续只测新库、v1升级和直接入队in_flight。数据修复按旧快照，未知与0区分，正常历史不改；新增正式迁移若需要，只追加，不删库、不重放已执行0002。

完成核心/GPUI fmt/test/clippy/release、39项统计及新增回归、既有转发/兼容/设置生命周期、三类新价格10万/100万预算与P95/RSS。Rust1.88、Windows、真实供应商、三尺寸人工GUI无条件时明确未验证，不能伪造。GUI自动状态机测试仍必须做。

更新IMPLEMENTATION-STATUS：撤回“U02有界修复已完成”，区分U05单条改善与批量/缺失ID未闭环，记录GPUI失败和复跑，而非只写全绿。最后给S01–S05、U06/U07闭环、实际命令/结果、schema/API/control、worktree/git status。

无需逐批询问继续；可编码部分持续推进，不能再次只写未完成GUI后结束。上下文不足保存准确断点。保留用户修改，不reset/stash、不改主工作区业务代码、不提交/推送/发布/安装、不操作真实用户配置/数据库；迁移和故障修复仅隔离验证。

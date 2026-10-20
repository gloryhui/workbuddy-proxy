# 第三轮复核与继续施工要求

审查日期：2026-10-15。开发目录：`/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics`；分支`codex/usage-statistics`；HEAD `e1725c3`，功能改动未提交。

**结论：部分修复和价格快照已落地，但还不能验收。新发现清理范围越界、批次提前结束、未发上游的有价请求结算失败；价格汇总、崩溃恢复和价格编辑表单也未闭环。** 原R02界面、R03、R06、R07仍需继续实现。

本轮只审查与生成隔离探针/文档，没有修改开发worktree业务代码、真实配置或真实数据库。证据见[本轮证据目录](review-round3-2026-10-15/README.md)。本报告同时是下一轮修复任务，覆盖旧Prompt中未完成部分；原价格业务要求继续有效。

## 1. 本轮重新执行的验证

| 验证 | 结果 |
| --- | --- |
| 核心`cargo test --locked --quiet` | 110 passed |
| GPUI测试，RUST_MIN_STACK=67108864 | 37 passed |
| 核心release | 通过 |
| 核心/GPUI fmt与clippy | 通过；GPUI依赖block v0.1.6有既存future-incompatibility提示 |
| statistics_test.py | 37项通过 |
| smoke/multi_routing/connection_test/model_capabilities_test | 全通过 |
| core_compatibility | 系统Python缺tomllib，改Python3.13后通过；不是产品失败 |
| 专项真实核心API + 临时SQLite + Mock | 复现本报告T01/T02/T03/T04/T05/T07的相关问题 |
| 直接引用实际Writer源码的维护探针 | 复现清理和保留任务只处理一批后结束 |

确认保留的改善：R01的资源键已区分provider/model；R02核心已采用聚合CTE和显式分组游标，现有分页集成检查通过；R04查询错误不再拼接SQL；BUSY不再逐事件重试；新请求开始/完成路径已保存并使用单价快照，改删价格不反改正常完成的历史；0002迁移和基本价格回归通过。

“P01已完成”“R05已完成”仍不成立，下面有独立复现。没有重新执行百万行三类价格基准、Rust1.88、Windows、真实供应商或完整人工GUI验收。

## 2. T01 [P1] 清理超出预览范围，且维护任务只删除第一批

位置：`src/stats/store.rs:769-807`；预览和确认：`src/stats/mod.rs:1815-1848`、`:1885-1920`。

**两个独立问题：**

1. Clear调用`delete_finished(task, false)`，SQL只在retention=true时加`started_at_ms<cutoff`。确认token虽然保存了before_ms，执行删除时却忽略它，只按id水位删除。
2. 删除一批后，cursor_id被设置为**尚未删除记录的全局MAX(id)**，而非本批实际选中记录的最后id。下一步跳过所有剩余记录并标completed。

真实API：两条已结束请求分别在cutoff前后；preview.row_count=1；确认后两条都消失，affected_rows=2。直接Writer探针：1200行、batch=500，清理600条应删记录只删500、剩100；retention应删1200条只删500、剩700，均两步后结束。

**必须先修：**预览/执行共用同一资格谓词，包含cutoff、确认水位、已结束；一次事务先选本批id，按这些id删除，cursor只推进至本批已扫描/选中边界，不取剩余表MAX。保持有界批次，不能改成单次全表删除。维护日志affected和完成状态必须与实际匹配。

验收：600/1200/多于两批；cutoff边界、id洞、时间/id不单调、预览后的新请求、in_flight、cost级联、任务失败；预览外记录一条不删，所有合格记录最终删除。

## 3. T02 [P1] 有手动价格但未发上游时，零费用结算违反约束

位置：`src/stats/pricing.rs:79-90`、`src/stats/store.rs:1444-1478`；0002的金额CHECK。

manual分支调用settle；upstream_started=false时返回NotApplicable但parts=None，随后写入NULL分项和总价；v2 CHECK要求not_applicable金额全为0，导致Finish事务失败。

真实核心：先给model-ok配置价格，再在缺供应商Key的临时配置发请求。HTTP503正确，但数据库结果变成`interrupted / upstream_started=NULL / cost.pending / total=NULL`，健康报告write_failures=1。正确事实应为`rejected/missing_key、upstream_started=0、not_applicable、0元`。

修复所有“已确认未发上游”路径的零金额结算，不只给缺Key打补丁。不可把合法拒绝改成采集故障，也不可伪造Token。补missing_key、请求重写/Go验证失败、直接Guard重复Finish等回归。

## 4. T03 [P1] 异常恢复只关闭请求，不结算pending费用

位置：`src/stats/store.rs:178-208`、`:605-623`及孤立行恢复路径。

recover和settle_orphans只更新usage_requests。v2每次Start已经创建pending费用，恢复后请求interrupted但费用永久pending，不再有Finish可以结算。

真实核心SIGKILL/重启：`interrupted / price_source=default_zero / cost.status=pending / total=NULL`。正常停服的对应探针为default_zero/0，说明问题集中在异常恢复，而非所有关闭路径。

把请求终态与费用终态放在同一恢复事务：缺价默认0保留快照并结算0；手动非零价缺用量为usage_incomplete/NULL；只有已证明未发上游才not_applicable/0；legacy_unknown保持来源和已有金额。不能读取当前价格替换旧快照。覆盖崩溃、孤立Finish、迁移旧in_flight、重启幂等，不能留下“请求终态、费用pending”的组合。

## 5. T04 [P2] R05恢复元数据失败仍丢证据并宣称ready；孤立行溢出仍无本轮兜底

位置：`src/stats/mod.rs:1471-1570`。

recover_collection仍先`degraded_since.take()`，记录区间失败只打印日志，随后清degraded_recorded并设Ready。并未实现进度所称“失败保持degraded”。

实际故障注入：临时SQLite触发器拒绝恢复时插入collecting区间，持锁使Finish写失败后放锁。日志确认“could not record the degraded window”，health却ready，数据库仍只有旧的开放collecting区间，没有持久化故障覆盖。这会把不可信窗口标成可信。

trim_orphans在尝试结算前drain溢出ID；结算仍失败时只增加计数，ID已丢，日志承认等重启。既无数据库待补偿清单也无安全水位，同一运行期仍会永久in_flight。现有“>512”测试只证明能写库时的直接settle，不覆盖持续锁下溢出。

修复：全部恢复写成功后才清内存证据/切ready；失败保留状态并退避重试。溢出采用有界内存加持久化/可证明的水位补偿，恢复时只关闭确实丢终态的请求，不误伤仍在运行的流。测试持续锁+>512终态+解除锁、区间提交失败、孤立费用结算失败。

WriteBudget也需收尾：当前只在尝试前看deadline，没有将busy_timeout缩为剩余时间；批次隔离用outcome.applied当已遍历下标，遇到前面坏事件后会错误计算剩余切片。保留错误类型，只隔离确定性行约束错误，按真实遍历索引记丢失，补“坏事件+后续锁/超时”准确计数回归。

## 6. T05 [P1] 缺价0元未进入金额汇总；legacy_unknown金额同样被漏计

位置：`src/stats/query.rs:455-560`；`src/stats/dto.rs:438-475`；`gpui/src/statistics.rs:1813-1834`。

scan_costs/scan_costs_grouped只累计status=priced，直接忽略default_zero和legacy_unknown。Summary仍只有unpriced_request_count，没有default_zero_request_count或确定金额覆盖计数。

真实核心缺价请求：明细`default_zero/CNY/0`，dashboard.summary.costs=[]、series.costs=[]、模型汇总costs=[]。GPUI将空costs显示“未定价”，与用户“缺价按0元”相反。legacy_unknown若含历史已知金额，也会从总览消失。

按币种累计所有适用的已确定金额，至少含priced、default_zero、legacy_unknown的已知金额；pending/未知金额不伪造0。新增default_zero/settled等DTO计数并统一总览/趋势/分组/明细。全缺价应明确CNY0或当次USD0，不能显示未定价。用混合币种、缺价且Token缺失、手动非零价用量未知、legacy已知金额、in_flight测试一致性。

## 7. T06 [P1] 编辑价格没有回填币种和模式，保存可静默改坏价格

位置：`gpui/src/statistics.rs:3131-3166`、`:2980-3002`、`:392-424`。这是静态事件调用链确认，尚无人工GUI操作证据。

edit_price只回填四项数字、model和label，没有回填price_currency/price_mode，也没绑定供应商身份。save_price使用当前Select值，未选时回退USD/flat。因此打开CNY/disjoint记录仅改label后保存，就可能把价格改为USD/flat；初次打开与连续编辑不同记录都受影响。

表单必须建立独立PriceDraft，完整回填price_id/revision/provider/model/currency/mode/四价/label。新增供应商必须明确选择，不能provider为空时取provider_options.first。编辑组合键若不支持改名则禁用并校验；支持则事务更新并校验冲突，不能允许用户编辑却后台静默忽略。正确维护edit时price_open和错误/保存pending状态。

回归：CNY+disjoint与USD+flat交替打开，只改label保存，其他字段完全不变；取消重开、新建、保存失败保草稿、并发版本冲突、供应商切换。

## 8. T07 [P2] P01接口和设置尚有明确缺口

1. **价格列表忽略过滤与分页**：`src/stats/mod.rs:1763-1787`把provider/model/after_rule_id直接丢弃；store LIMIT limit而非limit+1，next永远空。实际按provider-b查询返回provider-a；after游标仍返回第一页。按稳定组合键实现搜索/过滤/keyset并接通GUI，不能只看前100条。
2. **零缓存价被拒**：store.rs:1249-1261把0当未配置，read_discount/disjoint拒绝合法免费缓存；表单也拒绝空白。四项留空均默认0，显式0必须允许。非零价缺拆分仍正确保持未知。
3. **重复新建绕过并发控制**：store.rs:1296-1324的ON CONFLICT直接覆盖已有组合键，不需expected_revision。实测重复“新建”revision1→2并改价。新建重复键应冲突；编辑精确price_id+revision，过期不写、不发布缓存。
4. **快照时间写错**：load_price_cache把价格updated_at_ms当price_snapshot_at_ms，snapshot_for只clone不改。实测请求开始时间晚于快照时间，后者等于价格编辑时间。当前配置更新时间和每次请求选价时间分开；入站复制时填本次started_at_ms。
5. **默认币种仅有数据库列**：Settings/SettingsDto/get/update/GUI没有default_currency，无法按要求改USD；改后需提交成功再发布缓存，历史不变。
6. **协议未升级**：SCHEMA_VERSION=2，但API_VERSION仍1、CONTROL_PROTOCOL仍5，DTO仍混rule_id/effective区间/referenced。对齐正式API v2（price_id、价格分页、快照和计数），需要兼容别名则明确支持；核心与GUI兼容识别同步，旧核心不能被当成支持v2。

额外格式边界：聚合金额是i128十进制字符串，GUI overview却parse::<i64>().unwrap_or(0)，分组amount也只接受i64。保留定点精度并支持i128格式化，不能把大汇总显示0或未知。

## 9. T08 [P2] 原R02界面/R03/R06/R07继续完成，不能再次只交后端

- **R02 GUI**：request_groups没提交cursor，回复没保存next_cursor，上一页/下一页仍只用于Requests。供应商、模型分别完成至少三页双向回放与20/50/100切换。核心读任务真正执行时仍需再次校验维护版本；当前只在入队前校验。
- **R03**：mark_dirty后debounce前旧seq仍可接受，旧回复仍清新pending；Rows/Groups都用rows标签并按当前tab解析，错误被静默忽略。完整QueryIdentity(filter_generation, tab/group_by, sort, cursor, seq)，按回复身份分派，旧回复不改新状态，解析错明确提示。用可控迟到回复的控制器测试验证。
- **R06**：非法日期继续沿用旧值、反向范围钳制1秒。验证后才应用，非法/反向/>366天就地报错且不发查询，保留已应用范围，测DST。
- **R07**：筛选搜索/next_key、语言重建、前台/最小化/模态、离页取消只读任务未完成。模型请求现在会带provider_id，这是改善，但供应商变更时sync_filter_state只mark_dirty，没有即时重新加载模型选项和处理不兼容选择；补全触发链与回复身份。

## 10. 给DeepSeek的执行要求

你现在继续现有worktree施工，完成本报告T01–T08及原合并Prompt尚未闭环项。无需每批询问是否继续。**顺序：T01清理 → T02/T03结算恢复 → T04故障证据 → T05汇总 → T06/T07价格UI与契约 → T08剩余交互 → 回归/基准/交付。**

先读以下主工作区最新文件，勿用开发worktree旧副本替代：

- `docs/statistics/REVIEW-ROUND3-2026-10-15.md`
- `docs/statistics/NEXT-ITERATION-COMBINED-PROMPT.md`
- `docs/statistics/PRICING-DECISION.md`
- `docs/statistics/API.md`
- `docs/statistics/schema.sql`
- `docs/statistics/review-round3-2026-10-15/`

保留已通过修复，先补能捕获旧缺陷的失败回归，再改源码。探针只是审查观察，不能把“成功复现缺陷”的exit0当产品通过。不要只更新进度声明、只修价格表、只跑现有37项就宣称闭环。

0002尚未发布但已可能在开发库运行：新修复不能只修改已执行的0002并假装既有v2库已修复。需要数据修正时提供后续幂等迁移/修复事务；原v1升级路径和已v2再启动都要验证。不得删库重建。对已经丢失的数据只如实报告，不能伪造恢复。

每批更新IMPLEMENTATION-STATUS为真实状态，旧S0–S6/F02/F10/P01“已完成”与现状矛盾的段落要标历史。测试失败先修，不把上下文到顶当完成；需要续轮时保存准确断点。

最终执行核心/GPUI fmt、test、clippy、release；statistics集成、smoke、多路由、连接、核心兼容、模型能力、设置生命周期；加入本报告维护/结算/故障/表单/控制器回归。补新价格模型三类10万/100万性能，记录超时预算、i128金额、转发P95/RSS。Rust1.88、Windows、真实供应商、三尺寸人工界面如无条件单列未验证，不能以Mock/本机编译替代。

最后给T01–T08逐项闭环表、实际测试命令/结果、schema/API/control版本、worktree/分支/git status。继续使用由 `git worktree list --porcelain` 核实的统计开发 worktree、`codex/usage-statistics`；不reset/stash、不覆盖用户修改、不改主工作区业务代码、不提交/推送/发布/安装、不操作真实用户配置/数据库。需要提交时另按用户授权及AGENTS署名规则执行。

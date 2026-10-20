# 第二轮复核后的合并开发 Prompt：剩余修复 + 价格解耦

以下分隔线内全文交给DeepSeek。本文件同时覆盖第二轮审查问题和用户最新价格要求，是下一轮唯一主推进任务；旧Prompt的价格生效区间、引用锁定、历史重算要求不再执行。

---

继续开发 WorkBuddy Proxy 使用统计。本轮必须同时完成：第二轮审查R01–R07、用户最新价格解耦P01、对应回归和交付证据。不要只修价格或只修页面，不要只改文档，不要推倒重写已通过验证的采集基础。

## 1. 工作区与先读资料

继续使用独立worktree：

统计开发 worktree（通过 `git worktree list --porcelain` 核实 `codex/usage-statistics` 对应目录）

分支`codex/usage-statistics`；审查时HEAD=e1725c3，功能修改尚未提交。检查实际git status、HEAD、worktree列表和AGENTS.md，保留现有实现/用户修改，不reset、不stash、不新建平行实现。

最新审查与价格设计保存在**主工作区**，先读这些文件，不能误读开发worktree旧副本：

1. `docs/statistics/REVIEW-ROUND2-2026-10-15.md`
2. `docs/statistics/PRICING-DECISION.md`
3. `docs/statistics/PRICING-CHANGE-PROMPT.md`
4. `docs/statistics/schema.sql`
5. `docs/statistics/API.md`
6. `docs/statistics/REQUIREMENTS.md`
7. `docs/statistics/review-round2-2026-10-15/`

只同步本次所需设计/审查资料，不覆盖当前IMPLEMENTATION-STATUS或正在修改的源码。更新进度为“第二轮复核后的修复与价格改造中”，保留旧测试证据和已修成果。

本轮复跑基线：核心105单元、GPUI34单元、statistics_test34项、核心release及两端fmt/clippy通过。Collector冲突合并/分块、JSON错误、正常停止、健康空桶、常见锁恢复已实际改善，应保留。基线通过不代表下面的问题已关闭。

## 2. 第一批：R01筛选和R02汇总翻页，先补失败回归

### R01 不同资源不能互相覆盖

request_filters连续submit provider/model，两者都用JobKind::Filters，pending.reads第二次insert覆盖第一次。实际调度探针只发models-0，provider_options始终空，影响供应商选择和价格表单。

把读意图合并键改为资源作用域：区分供应商筛选、模型筛选(provider_id/search/cursor)，以及不同provider的模式读取。同一资源重复刷新可只保留最新，不同资源必须各自发出。

测试同tick两个筛选都派发；同一资源多次只保留最新；满通道下仍不丢不同资源；用户写操作保持有界FIFO并有明确失败反馈，不被持续刷新饿死。不要为了修覆盖而改成无界排队。

### R02 让合法汇总游标真正工作

当前next_cursor.key保存模型名/供应商ID，校验却把它当列名，所以第一页返回的合法游标立刻被拒绝；绕过该校验后又因WHERE使用SUM/COUNT报misuse of aggregate。

1. DTO明确group_by与last_group_key，保留真实分组值，不能要求模型名等于upstream_model字面量。
2. 在CTE/子查询聚合后，或HAVING中做keyset；不在WHERE使用聚合函数。
3. 正确处理排序指标并列、NULL指标排最后、所有usage未知、provider/model以及total_tokens/request_count两种排序。
4. cursor绑定run、filter、维护版本、group_by、sort及合法范围。reader实际执行时复核，覆盖排队期间清理/保留删除的场景。
5. GUI分组查询保存和提交服务端cursor，接通上/下页和20/50/100页大小；请求分页也保留正确实现。不能只显示首20组。

用至少3页数据，经正式API和页面控制器分别测试1→2→3→2→1，无重复/遗漏；另测NULL指标组、篡改/过期游标和新请求插入。

## 3. 第二批：R03状态隔离、R04错误信息、R05故障预算

### R03 使用完整查询身份

只增加seq仍不够：mark_dirty后debounce前旧seq还会被接受；过期回复还清pending；Rows/Groups回复仍按当前tab解析，切页签可混DTO。

建立QueryIdentity，包含filter_generation、tab/group_by、sort、cursor和request_seq。改条件/页签立即失效旧身份。旧回复不改数据、不清新pending、不覆盖错误。按回复自身kind/身份解析DTO，反序列化错误有可观察的协议错误，不能静默丢弃。

加入可控迟到回复测试：筛选变更debounce期、Rows↔Groups、provider↔model、翻页与刷新交错、旧失败最后到达；断言显示数据身份和loading/pending始终对应最新意图。已有scheduler测试不能代替页面状态机测试。

### R04 对外不输出SQL

budget_error当前把rusqlite Error的完整SQL拼入API。对外返回固定错误码+短提示，不含SELECT/FROM/WHERE、路径或敏感参数；开发诊断按需记录。超时保持stats_query_timeout，其他错误保持稳定分类，测试覆盖实际错误响应。

### R05 对DB故障设置真实总预算

apply_batch对SQLITE_BUSY整批3次后还逐事件尝试；64事件锁探针在busy_timeout=30ms时仍耗2474ms，默认2s会严重放大。flush的外层deadline约束不到内部循环。

- 保留SQLite错误类别，BUSY/LOCKED/IO等暂时性故障有限退避/探测，不逐行隔离；只对确定性行约束错误隔离坏事件。
- 每批和shutdown传统一deadline，busy等待/重试都受剩余时间约束，不让64事件乘出上百秒。
- 记孤立Finish的受界限方案必须有数据库/水位兜底；超过512不能直接丢旧ID导致永久in_flight。
- 区间恢复失败时不能先丢degraded_since、再设ready。保留待恢复证据，确实成功后才宣称可信覆盖。
- 回归64事件锁、请求连续流入、关闭、>512丢终态、恢复区间提交失败；聊天仍透传且队列有界。

## 4. 第三批：P01价格彻底解耦，用户最新要求优先

用户明确要求：**价格表可以直接改；调用明细必须保存本次单价和总价；缺模型价格默认0元。** 当前业务代码还是schema/API v1，这部分还没实现。

### 4.1 当前价格表

按主工作区v2设计建立stats_model_prices，provider_id+upstream_model组合唯一，保存price_id、currency、input_mode、四项rate_nanos、label、revision和时间。

可以直接新增、修改、删除；取消生效区间、引用后锁定、区间重叠要求。revision只防并发覆盖，不限制历史已使用的价格。价格编辑必须明确选择供应商，不允许provider为空时静默取第一项。

### 4.2 每次调用自己的单价快照与金额

usage_costs与usage_requests按request_id一对一，独立保存：

- input/output/cache_read/cache_write_rate_nanos；
- currency、input_mode、price_source、price_snapshot_at_ms；
- 可选source_price_id/source_price_revision（只作追踪线索，不对当前价格建外键）；
- input/output/cache_read/cache_write_cost_nanos、total_cost_nanos；
- status、算法版本、计算时间。

请求接纳时从已提交的价格缓存复制PriceSnapshot进Guard/Start事件，Start事务保存pending快照；Finish用这份快照和Token一次性结算。进行中调价、之后删价、重启都不得改用今天价格。重复Finish不覆盖已结算快照和费用。

writer事务提交当前价格变更后发布有界缓存；热路径不查SQLite、不克隆全价格表。详情和dashboard/series/breakdown只读已存快照/金额，不JOIN当前价格重新计算。

### 4.3 缺价默认0

没有模型价格：四项单价0，price_source=default_zero，终态分项/总金额0，以后补价不改这条历史。单价项空白也默认0。

Token没收到仍NULL，不伪造Token=0。已经手动配置非零价格但缺必需用量/拆分时，金额仍未知并说明原因，不能把缺Token误当缺价格。明确未发上游金额0、不适用。默认币种来自stats_settings.default_currency（初始CNY，可设USD），当次冻结，历史不随设置改变。

### 4.4 单位、API和界面

保留F03修复：API *_rate_nanos是整数纳单位字符串，$2/M为2000000000；表单普通金额定点转换一次。**旧数据库价格已经是纳单位，不可整体再乘除1e9。**

实现v2 price save/list/delete和详情快照DTO；缺价显示0元，可带“未配置价格，按0计算”，金额汇总包含这些0。保存成功后再关表单，失败保留输入；支持直接编辑/删除。

删除历史重算按钮，旧stats_reprice_start返回stats_operation_removed；旧维护日志可只读保留。用户没有要求用当前价格反改历史，不能自动“补价重算”。

### 4.5 正式迁移与兼容

新增v1→v2正式迁移0002，不只改0001，不删库重建。迁移前backup，事务失败rollback：

1. 把旧费用所引用规则的单价复制为历史快照，保留原Token与金额。
2. 旧unpriced按新口径迁为default_zero与0费用。
3. 已有金额但无法可靠恢复旧单价：保留金额并legacy_unknown，不用当前价/伪造0单价替换。
4. 解除价格外键/区间限制，提取迁移时刻的当前配置；处理已有in_flight，重启不丢快照来源。
5. API/schema升v2，核心/GUI同步适配控制协议兼容，旧GUI不能误读新结果。

具体字段、约束与迁移边界以PRICING-DECISION和schema.sql为准。

### 4.6 价格必测场景

P1价下A完成→改P2→A不变/B用P2；A开始后调价→A仍P1；删价→A仍完整/C为0；缺价D为0→补价后D仍0/E用新价；未知Token和0费用区分；来源价格删除后详情/汇总仍完整；v1迁移金额不变；重复终态、并发调价、重启和清理级联正确。

## 5. 第四批：R06/R07剩余交互

- 自定义日期非法、反向、超过366天时就地报错，不发查询，不静默沿用旧值或钳制1秒；保留已应用范围，测试DST和合法范围。
- provider/model筛选实现服务端搜索和next_key分页，不截在首100条；模型按当前provider收窄，切供应商清除不兼容模型选择。
- 语言切换重建Select选项、placeholder和对话框，保留选择/草稿，不能中英混排。
- 自动刷新仅前台统计页且无冲突模态；离开页取消待派发只读任务和debounce，已授权写任务不静默丢弃。
- 保持原生GPUI Kit0.7.3、现有主题、模型管理与草稿行为；不引入WebView/JS图表。

## 6. 最终验收与交付

每批先补能捕获旧问题的失败回归，再改实现，记录实测结果。不能只修改断言迎合错误实现，不能拿探针exit0当产品通过。

必须完成：

1. 核心/GPUI fmt、test、clippy和release构建，正式统计集成测试及本轮回归。
2. 现有smoke、多路由、连接测试、核心兼容、多模态/思考和设置生命周期回归；原响应字节、HTTP状态、背压、取消与4并发许可释放保持。
3. v2设计SQL及正式v1→v2迁移测试，隐私边界、默认0与NULL、历史快照独立、FK/integrity检查。
4. 按新模型测“手动非零价格/缺价默认0/非零价缺拆分”三类10万/100万查询，金额i128分币种、超时/行数预算和转发P95/RSS；不只测旧全unpriced快路径。
5. GUI三尺寸、中英文、筛选与三页翻页、迟到回复、非法日期、价格改/删/缺价、维护与故障提示。截图受权限限制则明确保留人工项，但完成可控状态机自动测试。
6. Rust1.88最低版本、Windows构建/打包、真实供应商验证分别给证据；没有条件就标未验证，不能用本机1.98/Mac/Mock替代。

持续更新IMPLEMENTATION-STATUS，分别跟踪R01–R07、P01及验证缺项。最后给逐项闭环表、实际命令/结果、截图或明确未验证边界、schema/API版本、worktree/分支和git status。

本轮不授权push、release、安装、改真实用户配置/数据库或清理历史；提交遵守实际授权和AGENTS署名条件，不冒充其他工具贡献、不force push。不要改动主工作区其他内容。

现在从第一批开始，持续推进到本轮完整目标，无需每批结束询问是否继续。

---

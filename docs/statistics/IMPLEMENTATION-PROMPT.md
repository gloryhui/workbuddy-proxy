> 用户已修订价格业务：优先执行[PRICING-DECISION.md](PRICING-DECISION.md)和[PRICING-CHANGE-PROMPT.md](PRICING-CHANGE-PROMPT.md)。本Prompt的价格内容已相应更新。

# 交给 deepseek-v4.1-flash 的统计功能开发推进 Prompt

下面整段可以作为开发模型的任务提示。先确认它能访问当前仓库和本设计包；看不到文件时应取得这些文件，不能根据文件名臆造内容。

---

你正在开发 WorkBuddy Proxy 的本机使用统计功能。请实际实现、联调和验证，不只给方案。按小批次推进，遇到上下文不足先保存进度文件，下一轮从已有成果继续。你是编码执行者，不能把待验证项写成已完成。

## 一、必须先读的文件

1. 本项目及父目录生效的AGENTS.md。
2. `docs/statistics/README.md`、`REQUIREMENTS.md`、`DEVELOPMENT.md`、`API.md`、`schema.sql`、`PRICING-DECISION.md`、`PRICING-CHANGE-PROMPT.md`、`UI-WIREFRAME.svg`、`validate_design.py`。
3. 实际代码：根目录和gpui的Cargo.toml，`src/main.rs`、`src/proxy.rs`、`src/config.rs`、`src/frontend.rs`、`src/gpui_host.rs`，以及`gpui/src/main.rs`、`backend.rs`、`ui.rs`、`model_table.rs`。
4. `docs/ARCHITECTURE.md`、`docs/DESKTOP.md`，以及你要运行的脚本的参数说明。

设计依据的旧基线为main/0b2fe84415cb148a498b091e5814ce4fb8d97e35、v0.5.3、GPUI Kit=0.7.3；开始时重新核实HEAD、版本、API与已有改动。以实际仓库为准，发现差异先记录并调整设计，不盲套旧行号。

## 二、独立worktree是硬性要求

- 开始先检查git status、分支、HEAD、git worktree list；有Codex桌面工具时先list_artifacts并优先复用本任务合适的worktree。
- 使用独立worktree和`codex/usage-statistics`分支（已存在则检查，必要时选唯一后缀），明确从当前审定HEAD创建，不默认从不相干远端分支开始。
- 未提交设计包不在HEAD中时，只复制本任务`docs/statistics/`的已授权文档进入worktree，不复制整个脏目录。不stash、不reset、不覆盖主工作区用户修改。
- 文档调研时`scripts/model_capabilities_test.py`存在用户修改；必须重新检查，不能擅自覆盖/提交。后续如要改同文件，保留现有改动并说明合并范围。
- 所有编辑、cargo、Mock、截图和性能测试在正确worktree和临时config-dir中进行，不操作正在运行的用户代理、不读取真实Key做测试。
- 实现不等于授权push、发版、安装包或清理用户数据；没有相应授权不要执行。Git提交如经授权，遵守真实Author/Committer和共同作者规则，不假冒其他工具贡献，不force push。

## 三、交付目标

完成需求的P0-A、P0-B和P1：

1. 本地SQLite保存每次请求摘要。
2. 请求、模型、供应商、时间范围等筛选和统计API。
3. GPUI Kit原生统计页：质量提示、总览、趋势、请求日志、供应商/模型统计、详情。
4. 可修改当前价格、逐请求单价/金额快照、分币种估算费用、暂停采集、保留策略、明确确认的历史清理。
5. 单元、Mock、实际二进制、GUI构建、性能和平台验证报告。

界面参考Aiotto截图的信息层次和magpie的统计交互；采用本项目现有青蓝原生主题，不引入WebView、React、ECharts或第二套平台设置页。

## 四、禁止破坏的边界

- 不存messages、tools、代码/图片/正文、Key、Authorization、原始错误响应。
- 不收集全部响应后再返回，不改变响应字节、HTTP状态、端到端头、背压、取消和4并发许可释放。
- 统计故障不影响聊天；有界队列、缓存、查询预算，不为统计无限重试或后台持续读已取消流。
- 默认observe，不能为所有供应商自动加include_usage；显式按供应商request模式才合并该参数，保留stream_options未知字段。
- 不新增上游自动重试/换Key/换模型，不改模型管理、同步、思考、多模态业务含义。
- 不把缺失usage当0，不把缓存/思考子项额外加进总Token，不把估算费用称为真实账单。
- 不把Mock、编译、包生成当真实供应商计费/Windows人工验收。

## 五、后端实现要求

### 5.1 文件和线程

按DEVELOPMENT.md建立`src/stats/{mod,dto,event,collector,store,query,pricing}.rs`及正式migration。DTO纯serde/std，GPUI可共享，不能拉入SQLite。

核心新增rusqlite bundled/backup/hooks、chrono/chrono-tz，版本匹配Rust1.88并写Cargo.lock。不要改GPUI Kit版本来迁就新API。

Bridge仅存StatsHandle；SQLite写线程一个连接，查询线程一个只读连接，query线程惰性启动/闲置退出。所有DB操作离开Tokio I/O线程，UI不读DB。事件队列1024、控制16、查询8，写线程批量最多64事件/100ms，维护任务分批穿插，查询2秒预算。

### 5.2 数据库

数据库放配置目录usage.sqlite3，权限按现有私有目录规则。用schema.sql的8张表和stats_request_facts视图；正式迁移由Rust事务管理，不嵌套设计SQL的BEGIN/COMMIT。

连接WAL/NORMAL/foreign_keys/busy_timeout，检查实际WAL模式。迁移前对旧库使用SQLite backup API，未来schema不强行降级，失败不删除原库。启动迁移不阻断正常转发，健康状态可migrating/degraded。

Start INSERT ON CONFLICT DO NOTHING，Finish UPDATE WHERE outcome=in_flight；affected_rows=1才结算cost，同事务提交。不要INSERT OR REPLACE，不建双写滚动总数。配置删除/更名不改历史provider/route快照。

run记录和采集区间必须实现：异常退出恢复in_flight为interrupted；不伪造耗时、零Token。故障、暂停和无法确认区间须能在图表质量中显示。当前run原子计数和已落盘快照不要重复相加。

### 5.3 Guard与原始流

保持鉴权→permit→限量body→路由顺序，在有效路由后接纳统计；无法路由/鉴权/拿permit的拒绝不写逐请求行，页面文案说明覆盖边界。

接纳前try_reserve_owned两槽，分别Start和预留Finish，失败整次统计丢弃并告警；不能阻塞聊天。Guard持有Finish permit，正常、错误、提前return、Body Drop、shutdown只结算一次。

实现结构化Stream包装reqwest bytes_stream：观察&Bytes后原样yield同一Bytes，错误原样传递，EOF做协议完整性判断；Drop记录取消并释放原permit。不要另起任务读完响应。

缺Key记rejected/not_applicable；网络错误、HTTP错误、SSEerror、异常EOF、客户端取消分开。200+异常流不能算成功。连接测试同样走Recorder，source=connection_test，默认查询排除，不重复统计。现有Bridge::test首个有效choices事件就成功返回并丢弃上游流，必须保留；统计流记cancelled/探测提前结束，usage缺失保持未知，不影响原连接测试成功，不为了usage等DONE。

### 5.4 Collector

SSE需要处理跨chunk、UTF8、CRLF、多data行、注释、DONE；事件64KiB上限，待解析驻留128KiB上限，超大事件丢解析至下个边界后继续，不改变转发字节。末尾usage不等于每chunk增量，重复相同usage忽略，冲突完整usage标invalid。

JSON响应旁路副本最多256KiB；超限释放副本并missing/body_too_large。编码gzip/br等第一版unsupported_encoding，不开启隐式解压。未知Content-Type不硬当SSE。

openai_chat_v1规范化：prompt_tokens→input，completion_tokens→output，total校验input+output（省略时可推导），cached_tokens→cache_read，reasoning_tokens→输出子集。cache_write/第三方字段必须有明确适配语义，不能猜。缓存拆分unknown/read_subset/disjoint，无法证明互斥不能算新增输入或分项计费。

usage状态complete/partial/missing/invalid/not_applicable，非负整数与上限校验。父用量有效但细分非法时只丢细分并告警；父项冲突则invalid。第一版不本地估Token。

记录duration单调时钟、首正文字节TTFB、首可识别内容first_content；空delta/role/usage不算首内容。未知保持null。

### 5.5 统计API

严格实现API.md：沿用/_control/action及management_key，保留外层message字符串，在内部输出api_version=1 DTO；CONTROL_PROTOCOL升到5并测试旧核心处理。

必须实现stats_health/dashboard/requests/request_detail/breakdown/filters，以及P1settings/provider_settings/prices/price_delete/clear/maintenance命令；旧reprice入口明确停用。

统一StatsFilter builder，范围[from,to)、最多366天、最多100万匹配行；模型/供应商精确ID、默认不含测试、时区IANA。SQL参数绑定，排序白名单，查询deadline/progress_handler，响应<1MiB。

dashboard一个读事务生成summary+series；主Token只取已结束complete行。空集Token为null，计数0；空桶按可证采集覆盖判零/未知。有部分用量给覆盖率。

分页keyset(started_at_ms,id) DESC，每页20/50/100，as_of_id，filter_key，维护revision和run失效规则；GUI游标栈上一页，不OFFSET扫大表。详情查request_id，不提供原文。

分桶在Rust按chrono-tz生成，<=400桶，DST测试；汇总SQL的空桶LEFT JOIN过滤放ON。缓存命中率用同一批已知cache_read的input分母。费用i128按币种累计后字符串返回，不用REAL混币相加。

统计查询静默，不每次写普通事件日志。不要通过现有backend普通Job导致反复snapshot/config reload或全局busy。

### 5.6 可修改当前价、独立快照和维护

以PRICING-DECISION.md及PRICING-CHANGE-PROMPT.md执行：当前价格可直接修改/删除，不维护生效区间、不锁定历史引用。每次请求Start保存四项单价/币种/模式/来源/快照时间，Finish用本次快照结算分项/总金额。费用不对价格表建外键，查询不关联当前价。

没有模型价格时四项单价和总金额0；以后改价不改历史。Token缺失仍NULL。API rate_nanos为整数纳单位，UI转换一次。停用历史重算。新库设计为v2，已有v1必须正式迁移并保留原金额，不能删库重建。

retention默认180天，只清理已结束老行，每批1000；手动clear先preview后确认，保护进行中/新请求。费用随请求删除，当前价格独立。

## 六、GPUI实现要求

1. 新增Statistics Entity及独立StatsBackend、usage_table、usage_chart模块；Proxy切Page::Models/Statistics，不影响模型草稿。
2. 沿用现有标题/主题/窗口尺寸；导航168px，主内容单列滚动，gap/padding16，圆角12，不保留统计页右侧模型日志栏。
3. 筛选条自动换行：供应商/模型/状态/完整性/含测试/时间/自动刷新/刷新。默认30天，刷新关闭；切条件250ms合并，generation丢过期回复。
4. 质量警告必须有真实数据；缺价单价/金额默认0，Token未知不填0；主卡“已确认Token”而非“真实全部消耗”；不展示不可靠会话数/客户端。
5. 次级卡按主区宽3/4列；主图280px，费用图140px，DataTable400px，表内横向滚动，父flex节点min_h_0/min_w_0；不能复用带h_full的card造成每卡占全屏。
6. 请求表列和宽度按开发文档，双行64/72px，最多100条，悬停/详情/复制request_id；供应商/模型页签用服务端汇总。
7. 原生图表锁定GPUI Kit0.7.3。先用已有LineChart验证渲染；成品多序列/断点使用自定义UsagePlot+PlotElement，单一坐标与tooltip，不调用不存在的.series()，不叠多张独立坐标图。
8. 输入蓝、输出绿、缓存读紫、写橙；缓存注明输入子集不可相加。直线分段，None断开，零和未知区分，图例切换只本地重绘，hover统一显示精确数和质量。Token/费用分图，币种切换不混算。
9. 数据几何缓存按generation/尺寸/主题/图例，最多400桶；稳定id，刷新不重新播放图表动画，不每250ms克隆全量数据或查DB。
10. 统计Job独立loading，查询3秒超时、禁代理、禁重定向、<1MiB；自动刷新仅前台统计页且不叠加，离开页停止。
11. P1成本定价与采集存储折叠区、原生价格表单、清理确认和维护进度要可操作。中英文和accessibility labels同步完成。

## 七、按阶段推进并保留进度

先在worktree创建`docs/statistics/IMPLEMENTATION-STATUS.md`，记基线、已存在修改、文档差异、阶段、测试、未验证边界。每完成一批更新一次，不只在对话里记。

- S0：worktree、版本、DTO、正式migration；运行设计验证。
- S1：store/Guard/Collector/pricing核心单元，先证明有界与幂等。
- S2：relay/连接测试/退出/重启接入，用Mock证明原字节透传和取消。
- S3：全部P0管理接口与SQL，API集成和过滤/时区/分页验收。
- S4：GPUI导航/卡片/表格/详情/原生图表，实际构建与截图。
- S5：P1当前价格、请求快照、设置、保留与清理，后端和UI贯通。
- S6：完整回归、性能、平台和README/ARCHITECTURE更新。

当前任务预算不足时，只声明已完成切片与下一步，不能用假数据/空实现把未完成API标done。不要在只有后端或只有静态界面时宣称功能完成。

## 八、必须验证

1. `python3 docs/statistics/validate_design.py`。
2. 核心和gpui分别cargo fmt --check、test、clippy --all-targets -- -D warnings、release build；实际脚本参数先看帮助。
3. Collector分块/CRLF/超大事件恢复/usage重复冲突/NULL0/压缩/取消/异常EOF；Guard终态一次，permit释放。
4. 迁移失败/未来schema/锁/磁盘故障/队列满/崩溃恢复/去重/价格修改/删除与历史快照解耦/清理保护。
5. 新增临时目录本地Mock的`scripts/statistics_test.py`，测正式管理接口；与直连逐字节比对响应，observe请求保真，request只改明确参数。
6. 回归现有smoke、多供应商路由、连接测试、思考和多模态、核心兼容、退出/重开设置与未保存草稿。
7. 10万/100万条无敏感种子数据的查询、API大小、额外RSS和转发P95基准。目标失败给数字和原因，不能隐藏缺失数据。
8. 1100×700/1180×780/1440×1000的实际GPUI截图，中文/英文、空/满/未知/错误/长模型/多币种/维护，检查布局与交互。
9. Mac和Windows验证分开报告；真实供应商usage字段/收费准确性未验证时明确写明，不用Mock替代。

最终交付：变更概述、worktree/分支、实现的接口清单、SQL/schema版本、各阶段验证结果、实际截图、性能记录、未验证项和待用户验收项、git status。只有用户授权时提交/推送/打包，不碰其他改动。

现在开始执行S0，并持续推进到完整目标；对已授权且可逆的实现无需反复请求确认。

---

使用方式：把分隔线内全文作为首轮任务，并保证目标模型能读取整个`docs/statistics`目录。继续推进可使用：

```text
继续 WorkBuddy Proxy 使用统计开发。先读取当前 worktree 的
docs/statistics/IMPLEMENTATION-STATUS.md 与 IMPLEMENTATION-PROMPT.md，
核对真实代码和git状态，从第一个未完成阶段继续。
保持原始透传、有界采集、NULL语义、独立worktree和用户已有改动边界。
每完成一阶段更新进度与实测结果，推进到P0+P1完整验收。
```

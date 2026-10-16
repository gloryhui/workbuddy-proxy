> 价格设计已修订为v2，优先执行[PRICING-DECISION.md](PRICING-DECISION.md)；原价格生效区间/不可修改/历史重算方案已撤销。

# 使用统计详细开发文档

日期：2026-10-15；状态：待实现。协议以 [API.md](API.md) 为准，业务口径以 [REQUIREMENTS.md](REQUIREMENTS.md) 为准，DDL以 [schema.sql](schema.sql) 为准。遇到冲突先统一文档与测试，不能各写一套行为。

## 1. 现有代码事实与接入位置

| 文件/符号 | 当前行为 | 本功能接入方式 |
| --- | --- | --- |
| `src/main.rs::run` | 创建Bridge、单线程Tokio I/O runtime、启动axum、退出后join | 声明stats模块，管理统计工作线程与退出flush；CONTROL_PROTOCOL 4→5 |
| `src/proxy.rs::Bridge` | 配置/路由、4并发Semaphore、calls原子计数、内存事件日志 | 新增轻量StatsHandle，不放rusqlite连接 |
| `relay_to` | 鉴权→permit→8MiB请求体→借用model选路→重写→上游→bytes_stream透传 | 选路成功后创建RequestGuard；所有返回分支结算；在原Bytes流旁观察usage |
| `Bridge::upstream` | 统一向选定供应商发Chat Completions | 共享上游启动时间记录，错误只记类别；不自动重试 |
| `Bridge::test` | 直接发stream=true,max_tokens=1，收到首个有效choices事件即返回；解析最多64KiB测试响应 | 使用source=connection_test的相同Recorder，默认统计查询排除；保留首事件即返回，不为usage延长探测 |
| `rewrite_request` | 无思考覆盖时仅替换顶层model；RawValue保留messages/tools | observe不改变请求；request模式显式合并stream_options，保留未知字段 |
| `Bridge::command/command_inner` | 现有控制命令Result<String,String>、部分命令更新status/log | 增加stats命令分派与静默识别；保留外层message字符串协议 |
| `gpui/src/backend.rs` | 有界mpsc、ureq本地调用，非poll/metadata会再取snapshot并加载config | 独立统计Job/Reply类别或StatsBackend，避免额外snapshot/config I/O |
| `gpui/src/ui.rs::Proxy` | 250ms UI timer，约2秒snapshot轮询，全局busy，模型管理三栏 | 增加一级Page状态；统计独立loading，不沿用模型编辑全局busy |
| `gpui/src/model_table.rs` | GPUI Kit DataTable/TableDelegate、虚拟列表 | 复用模式创建usage_table，不把统计行塞入现有模型编辑表 |
| `gpui/src/main.rs` | GPUI Kit =0.7.3、1180×780、最小1100×700、现有主题 | 沿用主题/窗口生命周期，增加模块，不新增WebView或第二套平台UI |
| `src/gpui_host.rs` | 按需启动/结束设置子进程 | 不让关闭统计页影响核心和数据库线程 |
| `src/config.rs` | 私有目录、配置写入及Key | 仅复用目录/权限工具；统计设置放SQLite，不往config.json塞请求明细 |

本轮调研没有统计数据库，也没有rusqlite依赖。`calls`当前是在收到上游响应头后递增，不包含所有失败且重启清零，不能迁移成历史请求总数。

## 2. worktree与工作边界（必须执行）

编码必须在独立worktree。开始前读取本项目及父级生效的AGENTS.md，检查主工作区status和现有附件。文档编写时发现 `scripts/model_capabilities_test.py` 有已有工作区修改；它不属于本设计包，不得重置、包含或自动转移。

有Codex桌面工具时先list_artifacts，复用适合当前统计任务的active worktree；否则create_worktree，明确ref为本任务审定基线（当前分支HEAD），不能默认切到不相干远端主分支。创建后使用返回的workspace_directory，检查分支；需要分支时使用`codex/usage-statistics`，已存在则核实用途再复用或选唯一后缀。

在普通终端的等效示例（变量名/路径应先核实，不能直接照搬已存在分支）：

```sh
git status --short
git rev-parse HEAD
git worktree list
git worktree add -b codex/usage-statistics ../workbuddy-proxy-usage-statistics HEAD
```

若设计文档尚未进入基线提交：先确认本设计包文件列表；把这些已授权的文档逐个复制进worktree，或在用户授权提交后从对应提交开始。**不复制整个脏工作区，不擅自stash，不把其他修改一起提交。** 模型交接Prompt必须提醒它在正确worktree内执行所有构建/测试。

本设计包本轮只包含文档与SQL设计验证；后续执行编码Prompt时，编码和验证授权也不等于授权push/release。创建有意义的提交须遵循项目Conventional Commits及共同作者规则；Codex实质参与时末尾唯一 `Co-authored-by: Codex <codex@openai.com>`，保留真实Author/Committer，不force push、不为补署名改历史。

## 3. 推荐模块与依赖

```text
src/stats/
  mod.rs           StatsHandle、启动/关闭、公开facade
  dto.rs           统计API DTO、过滤器、枚举（纯serde/std，可被GPUI按path引用）
  event.rs         Start/Finish事件、RequestGuard和定长元数据
  collector.rs     有界SSE/JSON旁路、规范化与完整性校验
  store.rs         写线程、连接、事务、迁移、恢复、retention
  query.rs         过滤器builder、dashboard/list/breakdown/filter查询
  pricing.rs       规则匹配、i128定点计算、费用状态
  migrations/0001.sql  从设计DDL移入的正式迁移，开发时去掉外层BEGIN/COMMIT
gpui/src/
  stats_backend.rs 独立统计任务队列/HTTP调用（或结构化扩展backend）
  statistics.rs    统计页Entity、状态、交互、cards和页面结构
  usage_table.rs   请求/供应商/模型表格delegate
  usage_chart.rs   多序列原生图表与tooltip
```

核心新增 `rusqlite`（启用bundled、backup、hooks；若实际API不需某feature可去掉）、`chrono`、`chrono-tz`。以Rust1.88/各发布平台能编译的版本为准，固定Cargo.lock。无需sqlx、数据库服务、HTTP微服务、JS图表或完整连接池。

DTO放独立轻量文件，不能通过引用整个stats模块让GPUI链接SQLite。沿用项目 `#[path="../../src/..."]` 共享模式即可；核心和GPUI的Serde枚举snake_case统一，未知api_version报兼容错误。

### 3.1 线程拓扑

```text
Tokio proxy-io (现有单线程，4个并发请求)
  ├─ RequestGuard + Collector（每请求有界）
  ├─ bounded event queue 1024 ──> stats-writer OS线程（唯一写连接）
  └─ stats查询异步oneshot <────> stats-reader OS线程（一个只读连接）
                                    ↑ bounded query queue 8
GPUI UI线程 ── StatsBackend线程 ── 管理HTTP ─┘
```

写线程只在启用统计或维护设置需要时存在；reader惰性启动（首次stats查询），空闲60秒释放连接/线程，避免常驻无用线程。健康快照主要为原子/小Mutex，不触发reader全表扫描。

核心不得在Tokio I/O线程执行sqlite调用、std::mpsc阻塞recv、join、等待锁；reader/writer完成后用tokio oneshot回复。也不能在GPUI render/notify闭包中读DB或同步HTTP。

写队列与维护/设置控制队列分开：事件1024槽，控制16槽，查询8槽。writer每轮最多处理64事件或10ms，然后检查控制和维护；不得让维护任务阻塞新请求落库。批量commit每100ms或64事件，低流量也须及时落盘，不等凑满批次。

## 4. SQLite部署、迁移与事务

数据库文件：`<config_dir>/usage.sqlite3`，分别位于README已经说明的Mac/Windows/Linux配置目录。目录0700，Unix数据库及其WAL/SHM按0600创建/校验；Windows沿用当前用户私有目录。不给GPUI打开数据库路径的权限接口。

写连接设置：

```sql
PRAGMA foreign_keys=ON;
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
PRAGMA busy_timeout=2000;
PRAGMA wal_autocheckpoint=1000;
```

检查journal_mode返回值确为wal；本地文件系统不支持WAL时统计降级，不能静默把读写放回热路径。reader用只读连接+query_only，foreign_keys按需开启，busy_timeout=1000，所有查询2秒预算。WAL仍可能BUSY，不能承诺绝无锁冲突。NORMAL在断电下可能回滚最近提交；如果未来加FULL模式，必须重新测性能。

迁移步骤：

1. writer打开连接，读取PRAGMA user_version；新库顺序执行0001+0002至v2；已有v1只执行0002。
2. 对已有非空库升级前使用SQLite backup API生成同目录私有备份，不能直接复制活跃的`.sqlite3`而漏掉WAL。
3. `BEGIN IMMEDIATE`，执行版本对应DDL，更新user_version，COMMIT。设计schema.sql是独立执行脚本，正式migration函数不嵌套它的BEGIN。
4. 版本高于本程序认识的版本：禁用统计写入、健康报告unsupported_schema，继续转发，禁止降级重建。
5. 迁移失败：rollback，保留原库/备份，不删除坏库、不自动重命名假装恢复。
6. 完成恢复事务后发布StatsHandle ready。启动迁移放专用线程，不延迟启动网络到不可用；health可以先migrating。

退出先停止接纳新统计、让已销毁的Body/Guard发完终态，再发writer flush屏障；等待最多2秒，join在网络runtime外执行。现有main使用select退出server，**不假定它已有完整优雅排空**；必须明确drop server/runtime使Guard终结，再flush。写线程不被主线程持有的Sender悬挂：先drop所有Sender/保留一个shutdown控制通道，再join。正常退出记录run stopped与interval关闭，PASSIVE checkpoint；不在请求热路径TRUNCATE/VACUUM。

## 5. 数据模型与字段规则

DDL见schema.sql，8张表用途：

| 表 | 写入/更新时机 | 作用 |
| --- | --- | --- |
| stats_settings | 首次初始化、用户修改 | 采集开关、保留天数、乐观revision |
| stats_provider_settings | 用户修改 | 单供应商observe/request模式；不存Key |
| stats_runs | 核心启动、周期健康快照、退出/恢复 | 当前批次ID、写失败/丢弃、崩溃识别 |
| stats_collection_intervals | 开关/故障/恢复/退出 | 可验证采集覆盖区间，不把暂停/故障画成零 |
| usage_requests | Start插入、Finish更新一次 | 请求事实、名称快照、模型、Token与状态 |
| stats_model_prices | 用户新增/修改/删除 | 可变当前价格，供应商+模型唯一，不用生效区间 |
| usage_costs | Start保存价格快照，Finish一次结算 | 请求自己的四项单价、币种/模式、分项/总金额 |
| stats_maintenance_events | 清理/迁移及旧维护记录 | 维护进度/失败记录，不保存任意日志文本 |

不建provider/model外键到config.json；删除配置不能删除历史。请求→run、费用→请求采用外键。费用来源价格ID仅作线索，不对可变价格表建外键；价格可以直接删除，历史不受影响。

字符串在请求接纳时从已选路由克隆快照并限制长度，不在流结束时读取“最新配置”。provider_name/route_name/requested/upstream/response模型禁止控制字符；从上游取得的response_model最多160字符，超限忽略并记固定警告，不保存任意响应字段。

request_id/run_id用项目rand生成128位以上随机十六进制（例如32字符）；每个上游调用一条，不拿客户端response id当唯一键。未来若代理增加重试，每个实际尝试另有attempt设计；当前不能擅自加重试。

### 5.1 Start/Finish事务及幂等

Start预编译INSERT，`ON CONFLICT(request_id) DO NOTHING`，不要 `INSERT OR REPLACE`（会删关联cost）。Finish在同一事务中：

```sql
UPDATE usage_requests SET
  ended_at_ms=:ended, duration_ms=:duration, ttfb_ms=:ttfb,
  first_content_ms=:first_content, upstream_started=:sent,
  response_model=:response_model, http_status=:http_status,
  outcome=:outcome, error_kind=:error_kind,
  usage_status=:usage_status, usage_reason=:usage_reason, usage_source=:usage_source,
  input_tokens=:input, output_tokens=:output, total_tokens=:total,
  cache_read_tokens=:cache_read, cache_write_tokens=:cache_write,
  reasoning_tokens=:reasoning, input_uncached_tokens=:uncached,
  input_breakdown=:breakdown, updated_at_ms=:updated
WHERE request_id=:request_id AND outcome='in_flight';
```

只有affected_rows=1才用Start已保存的单价快照结算cost；重复Finish不改run计数、不改快照和既有金额、不再累加。原始事实与cost同事务提交。不建“每来一次就sum +=”的双写聚合。

Start和Finish必须FIFO。同一请求即使在同一事务处理，也先INSERT再UPDATE。Finish不能通过先insert空行绕过Start。最终事件携带完整定长摘要，不持有请求体Bytes、reqwest Response、HeaderMap或Key。

### 5.2 有界队列与终态保障

使用tokio::sync::mpsc有界channel（writer用blocking_recv/try_recv）。接纳统计时先非阻塞申请两个OwnedPermit：一个Start，一个预留Finish；第二个申请失败则释放第一个，整次统计不接纳、dropped_requests+1。成功发送Start，Finish permit放进RequestGuard。

这样已接纳请求结束或Drop时不必等待队列空间。不得在Drop里await、写DB或创建无限后台任务；预留permit同步send一次Finish。禁用统计时无Guard，不走解析器。队列满时聊天仍继续，健康标degraded；此次请求不伪造一条tokens=0的成功记录。

推荐Guard结构（概念骨架，不是可直接粘贴编译代码）：

```rust
struct RequestGuard {
    started_instant: Instant,
    upstream_instant: Option<Instant>,
    finish_slot: Option<OwnedPermit<StatsEvent>>,
    summary: RequestSummary, // 有限String + Option<i64> + enum
    collector: Collector,
    finished: bool,
}
// finish(outcome)使用Option::take消费slot；Drop仅在未完成时finish(cancelled)。
// shutdown通过共享closing标志把runtime退出造成的Drop分类interrupted/shutdown。
```

DB写失败：整批事务rollback，最多短暂退避重试2次（writer线程内），失败后标degraded并丢弃该批事件，计write_failures。不能无界重放。若开始事件已丢失，后续Finish不得假装已完整记录；恢复时按最后可信提交点将不确定区间标unknown，并处理残留in_flight。统计健康计数按每请求计drop，不能Start/Finish各加一次。

### 5.3 崩溃、覆盖区间与孤立行恢复

每次成功提交更新stats_runs.last_persisted_at_ms及admitted/drop/failure快照。内存原子计数是当前run的权威状态；存盘是最后快照，health不能双加。

启动恢复事务：旧run仍无stopped_at的，标stop_reason=unclean，stopped_at为恢复检测时刻（这是检测边界，非真实关机时间）。旧in_flight→interrupted/unclean_shutdown，ended_at为恢复检测时刻，duration/ttfb/first_content保持未知，不从墙钟构造假耗时；无可验证usage保留missing。旧run最后可信提交后至恢复时刻标unknown；开放collecting区间在最后可信时间关闭。

同进程内遇到磁盘故障，先记录最早fault_at于小型内存状态；恢复后补写degraded区间，不能因补写成功把故障期间改为collecting。手动暂停/恢复同样以事务关闭/打开interval，只有提交后才切原子enabled。每个run最多一个开放interval；服务层防止交叉或倒置时间区间。

writer批失败后可能留下已Start但未Finish的孤立行。维护idle检查本进程活跃request_id集合（最多4个，测试请求也纳入同一上限或单独明确有界），不再活跃且终态事件已处理/丢弃的in_flight标interrupted；不能依靠“请求超过30秒就判中断”，模型请求可能很长。退出flush后统一检查一次。维护中不能访问/记录聊天内容。

## 6. 采集器实现

### 6.1 接纳点和时间

保持现有鉴权、permit和body限制的先后关系。完成路由解析后取得配置快照，创建Guard并发Start；缺Key分支完成rejected/not_applicable。重写失败按rejected/other结算；将 `usage_reason=not_sent`。

Start入库的upstream_started为NULL，表示持久化时尚不能证明是否已发上游；Guard内在发上游前设置true和Instant，Finish写确定的布尔值。若Start后崩溃、没有Finish，恢复保留NULL，不伪装成确定未发送。缺Key等确定未发送分支的Finish为false。连接失败记failed/connect，http_status=502（本地实际返回），usage缺失。收到headers后保存http_status和响应Content-Type/Content-Encoding的分类，不保存全headers。

started/ended/updated为UTC墙钟；墙钟回拨时ended至少钳制到started、updated至少钳制到created，并记录时间异常健康提示。耗时继续来自Instant，不通过钳制后的UTC相减计算。区间边界同样需避免倒置，无法确认的时段标unknown。

收到第一个非空Bytes记录ttfb；如果客户端未读流，则不会人为提前读，只遵循现有背压。

**连接测试例外必须显式接入：** 当前Bridge::test收到首个有效choices事件就报告探测成功并丢弃上游流，通常收不到末尾usage。保留其原有45秒超时、首事件成功和立即停止行为；统计source=connection_test，主动提前结束流按outcome=cancelled、error_kind=client_drop记录，UI对该来源显示“探测提前结束”，不影响原测试报告中的成功结果。已取得完整usage可保留，否则missing/client_drop。不能把测试改成等DONE才返回，更不能因统计里的cancelled把连接诊断改为失败。测试的request模式注入仍按供应商显式设置生效。

### 6.2 流包装

将现有unfold替换为持有source、原permit、Guard的结构化Stream。必要时用pin-project-lite或Box::pin持有source，选能在Rust1.88稳定编译的最小实现。

```text
poll_next(source)
  Pending       → 原样Pending
  Some(Ok(b))   → guard.observe(&b); 原样返回同一Bytes b
  Some(Err(e))  → finish(failed/upstream_stream); 原样返回Err(e)
  None          → guard.finish(根据协议终止标记判断completed/interrupted); None
Stream被drop     → guard.finish(cancelled 或 shutdown)，原permit一起释放
```

不得collect完整响应、提前拉取到后台、复制整段历史、重新序列化响应、吞掉error、生成新的SSE、替换原Bytes。解析超限/错误只影响统计，不影响返回字节。

Chat Completions SSE在正常EOF时还应看到`[DONE]`；无DONE即使HTTP200也标interrupted，已完整usage可保留。某供应商若合法使用其他结束方式，要用明确的供应商适配器+测试扩展，不按所有EOF一律成功。非流式JSON按成功2xx完整EOF处理；未知协议无法确认内容级成功时保守提示，不写假的SSE解析结果。

### 6.3 SSE framing

网络chunk不等于SSE事件，UTF-8字符/CRLF/JSON都可能跨chunk。解析器必须：

1. 增量扫描字节到换行，支持LF/CRLF/跨chunk CR；忽略注释心跳、event/id/retry。
2. 收集同一事件多个`data:`行并用LF连接，遇空行才解析事件；允许data冒号后一个空格。
3. `[DONE]`设协议终止标记，不生成Token。
4. 每个事件最多64KiB（含等待中的行）；超过后discard直到下一个事件边界，继续识别后续小usage事件。不要因为一条超大正文永久关闭解析。
5. 每请求待解析总驻留缓存<=128KiB；限制JSON嵌套深度64，设置合理字段/字符串长度限制，禁止保存全部events。
6. 只提取白名单model、usage和少量可判断首内容/error的字段；正文解析后立即丢弃，不作为日志或DTO字段。
7. 末尾残留未结束事件可尝试一次解析但标记协议不完整；正常成功仍要DONE。

### 6.4 非流式JSON、编码和类型

Content-Type application/json（含charset后缀）分支用最多256KiB的旁路副本，原始响应仍流式转发；超过即释放副本、body_too_large/missing。第一版允许超大JSON缺usage，明确在页面提示；后续才做复杂的顶层字段增量JSON扫描。

不主动修改客户端Accept-Encoding，也不对响应开启隐式解压。现有reqwest依赖没有响应解压feature；若收到gzip/br等编码正文，记录unsupported_encoding/missing，仍保留原始header/body。若未来支持，只能做有界旁路解码并专项测zip bomb边界，不能一边解码一边保留旧Content-Length返回。

未知Content-Type不按SSE硬拆；若没有可信协议适配，usage missing、TTFB可用、首内容未知。

### 6.5 usage规范化

第一版规范适配器 `openai_chat_v1`：

| 上游字段 | 规范字段/规则 |
| --- | --- |
| usage.prompt_tokens | input_tokens |
| usage.completion_tokens | output_tokens |
| usage.total_tokens | 校验等于input+output；省略且前两者有效时可推导 |
| usage.prompt_tokens_details.cached_tokens | cache_read_tokens，输入子集 |
| usage.prompt_tokens_details.cache_write_tokens | 仅字段存在且供应商/适配器语义已验证时记录cache_write_tokens；不凭名称假设与read互斥 |
| usage.completion_tokens_details.reasoning_tokens | reasoning_tokens，输出子集 |

不能看到 `input_tokens`/`cache_creation_input_tokens` 就全局当成Anthropic加法；本项目不是Anthropic协议转换器。第三方扩展（如prompt_cache_hit_tokens）必须按供应商/协议添加显式适配器和测试，避免同时匹配两套usage重复累计。

校验i64整数、非负、每字段<=1e12，total<=2e12；JSON浮点、小数、数字字符串不静默转整数。父项有效但可选细分非法：忽略该细分，保留可信父项complete，并usage_reason=invalid_numbers；父项/total非法则整条usage invalid。缺一父项为partial，均缺为missing。

只有input/output都有效才算complete；total缺省可以安全相加，不能凭total反推缺失父项。缓存read<=input，reasoning<=output，write<=input；未知字段忽略。适配器仅确认read子集时input_breakdown=read_subset；明确read/write互斥且均已知才disjoint并计算uncached；否则unknown。不能把cache_write缺省为0后声称完整拆分。

流中多次usage不累加：先保存最新候选的字段集合；完全相同重复忽略；缺字段的候选只补充兼容字段；两个完整候选不一致时conflicting_usage/invalid。第三方若有中途累计usage，需要独立已测试适配器明确“末次快照”语义，不能用max或sum瞎合并。

### 6.6 请求include_usage

未配置的供应商默认request，已保存的选择保持。provider usage_mode=request且stream=true时，合并顶层stream_options.include_usage=true，保留其他字段。已有false会被request模式覆盖，UI提供按供应商关闭的入口。

stream_options已有非object且非null：保留转发、记录该请求usage请求参数无法注入的健康警告，不把一个本能工作的聊天变成400。不能无条件加到所有供应商，也不能上游400后自动重发（可能产生重复消耗）。若请求体实际stream=false，不注入stream_options。

将model/思考/usage改写合并为一次顶层RawValue序列化；messages/tools保持RawValue。observe下默认路径的字节保真回归测试必须继续通过。

## 7. 查询SQL和统计一致性

`StatsFilter::validate`只执行一次；由builder生成固定白名单条件和rusqlite params。不得拼接用户字符串为SQL。查询只读线程开`BEGIN DEFERRED`，完成summary+series立即COMMIT，不跨HTTP或GUI帧持有事务。

### 7.1 总览SQL模板

以下为无可选过滤器时的可执行模板，参数由开发测试绑定。真实builder在WHERE添加精确过滤，不用OR-null导致所有索引失效。

```sql
SELECT COUNT(*) AS request_count,
  SUM(CASE WHEN outcome<>'in_flight' THEN 1 ELSE 0 END) AS finished_count,
  SUM(CASE WHEN outcome='in_flight' THEN 1 ELSE 0 END) AS in_flight_count,
  SUM(CASE WHEN outcome='completed' THEN 1 ELSE 0 END) AS completed_count,
  SUM(CASE WHEN outcome IN ('failed','rejected') THEN 1 ELSE 0 END) AS failed_count,
  SUM(CASE WHEN outcome='rejected' THEN 1 ELSE 0 END) AS rejected_count,
  SUM(CASE WHEN outcome='cancelled' THEN 1 ELSE 0 END) AS cancelled_count,
  SUM(CASE WHEN outcome='interrupted' THEN 1 ELSE 0 END) AS interrupted_count,
  SUM(CASE WHEN outcome<>'in_flight' AND upstream_started=1 THEN 1 ELSE 0 END) AS usage_eligible_count,
  COALESCE(SUM(counted_usage),0) AS usage_complete_count,
  SUM(counted_input_tokens) AS input_tokens,
  SUM(counted_output_tokens) AS output_tokens,
  SUM(counted_total_tokens) AS total_tokens,
  SUM(CASE WHEN counted_usage=1 THEN cache_read_tokens END) AS cache_read_tokens,
  SUM(CASE WHEN counted_usage=1 AND cache_read_tokens IS NOT NULL THEN 1 ELSE 0 END) AS cache_read_known_count,
  SUM(CASE WHEN counted_usage=1 AND cache_read_tokens IS NOT NULL THEN input_tokens END) AS cache_read_denominator_input_tokens,
  AVG(CASE WHEN outcome='completed' THEN duration_ms END) AS avg_duration_ms
FROM stats_request_facts
WHERE started_at_ms>=:from_ms AND started_at_ms<:to_ms
  AND (:include_tests=1 OR source='relay');
```

计数空集字段在DTO映射统一COALESCE为0，**Token SUM空集保留NULL**。部分usage/缺失等计数用同样CASE补齐。成功率使用已结束分母；failed_count包含rejected，不能再次把rejected相加构造总数。

完整覆盖率complete分子要与eligible分母对齐，若有not_applicable/特殊本地行，分子限定upstream_started=true。缓存命中率分母只包括cache_read非NULL的那一组完整usage，不能所有输入混进去。

### 7.2 明细keyset

```sql
SELECT * FROM stats_request_facts
WHERE started_at_ms>=:from_ms AND started_at_ms<:to_ms
  AND (:include_tests=1 OR source='relay')
  AND id<=:as_of_id
  AND (started_at_ms<:last_time OR (started_at_ms=:last_time AND id<:last_id))
ORDER BY started_at_ms DESC,id DESC LIMIT :limit_plus_one;
```

第一页省略最后游标条件。as_of_id在第一页事务读取MAX(id)，并在同事务COUNT过滤总数。不要使用OFFSET扫描第几百万行。详情查UNIQUE request_id。

### 7.3 时序分桶

Rust用chrono-tz生成有序`Vec<Bucket{start_ms,end_ms}>`，小时为真实UTC经过时间，日/周为指定时区本地日历边界，处理23/25小时日；周一为周起始。边界时间歧义选较早、非存在的当地午夜向后找第一个有效时刻，保证严格递增、无重叠、覆盖整个请求区间。用DST/半小时偏移时区测试。

用有界VALUES CTE或reader临时表传<=400桶：

```sql
WITH buckets(bucket_start,bucket_end) AS (VALUES (:b0,:e0),(:b1,:e1))
SELECT b.bucket_start,b.bucket_end,COUNT(r.id) AS requests,
       SUM(r.counted_input_tokens) AS input_tokens,
       SUM(r.counted_output_tokens) AS output_tokens
FROM buckets b LEFT JOIN stats_request_facts r
  ON r.started_at_ms>=b.bucket_start AND r.started_at_ms<b.bucket_end
 AND (r.source='relay' OR :include_tests=1)
GROUP BY b.bucket_start,b.bucket_end ORDER BY b.bucket_start;
```

可选provider/model等条件放JOIN ON中，不能放WHERE把LEFT JOIN变成INNER JOIN丢空桶。用运行区间与collection_intervals判质量；空桶是否零值由Rust依据覆盖规则决定，不用COALESCE(SUM,0)覆盖所有未知。

### 7.4 模型/供应商汇总

`GROUP BY provider_id`或`GROUP BY upstream_model`，key是枚举决定的固定列；metric排序只允许两种白名单。label使用该组最新(started,id)历史快照，不能用SQLite非聚合裸列随机选名称。provider_count使用COUNT(DISTINCT provider_id)。费用按currency另行聚合后按组key合并。

### 7.5 金额、溢出和查询预算

不能用SQLite REAL/TOTAL保存或累计金额。逐行读取已保存i64 nanos（含default_zero的0金额），在reader用checked i128按币种/桶/组累计，再序列化十进制字符串；单请求cost仍必须能放入i64，否则status=overflow。Token SQL SUM也可能溢出：第一版一次查询最多100万条匹配明细，超过返回`stats_query_too_large`要求缩小范围，不用浮点降级。1e6×2e12=2e18，在i64内。

先按索引统计匹配行数预算，再执行查询；用rusqlite progress_handler（hooks feature）检查Instant deadline，超2秒interrupt返回stats_query_timeout。即使reader串行也必须有队列容量和等候超时，不能堆积查询。GUI默认3秒HTTP超时；100秒旧管理默认值不适合统计查询。

核心可加最多8项、5秒TTL的只读缓存，key=规范filter+粒度+数据库commit_revision，用户刷新允许bypass；第一版可先不缓存，测量后决定。UI不关闭即每250ms重查不合格。

## 8. 当前价格、逐请求快照与维护（修订v2）

实现细节及v1→v2迁移以[PRICING-DECISION.md](PRICING-DECISION.md)为准。stats_model_prices保存可变当前配置；usage_costs在Start事务保存四项单价/币种/模式/来源/快照时间，Finish只用本次快照结算，不能重新查当前价格。没有匹配价格时创建全0快照，终态明确总金额0。

当前价格保存/删除提交后发布有界缓存，新请求从缓存复制快照；已接纳请求即使之后调价也不变。费用查询不得JOIN当前价格配置。价格可以任意修改/删除，来源ID不能建价格表外键。

2026-10-16：0005 将未配置供应商的 usage_mode 默认改为 request，保留已有显式选择。0006 添加 stats_price_catalog，保存完整价格目录 JSON、最近成功的本机日期、更新时间/数量与路由指纹。核心每分钟检查日期；未成功的当天联网拉取，失败后至少间隔 15 分钟，手动 stats_prices_update 跳过日期/退避检查。网络下载限 16 MiB/30 秒，十进制定点转换不做浮点金额计算；目录导入、当前价覆盖和日期标记在同一 writer 事务内提交，随后发布价格缓存。新路由在同一天只用缓存补齐缺价。UI 价格更新走不可丢弃的写任务，单独使用 45 秒超时，不改变其他统计查询 3 秒超时。当前价格 label 记录 models.dev 供应商/模型来源；调用仍使用现有本地价格快照契约。

flat：全部input乘输入价，缓存不再另收；read_discount：非缓存部分乘输入价、cache_read乘缓存读价；disjoint：已证明互斥的新增/读/写分别计价；output始终按输出价，reasoning不重复计。每项公式为`(tokens * rate_nanos + 500_000) / 1_000_000`，i128中间值，checked转i64和求和。

缺价格或缺价格项默认0；缺必需Token但有非零手动价仍金额未知。缺价全0时金额直接0且Token保持未知状态。API rate_nanos严格整数；表单普通金额先定点转换一次。不做历史重算，移除原reprice入口和writer新任务分支，保留旧任务日志只读。

schema.sql为新库v2设计；正式实现必须增加0002迁移而非改旧0001。迁移前备份，先按旧rule_id复制历史单价、保留已有金额，再解除价格外键/区间限制；无法恢复旧单价但已有金额时标legacy_unknown，不伪造0单价。旧unpriced迁为明确default_zero。迁移不是本轮在真实库执行的动作。

清理/retention仍只删除已结束请求，费用随请求级联删除，当前价格不删除。SQL模板保持：

```sql
DELETE FROM usage_requests WHERE id IN (
 SELECT id FROM usage_requests
 WHERE started_at_ms<:cutoff AND outcome<>'in_flight' AND id<=:as_of_id
 ORDER BY id LIMIT 1000
);
```

retention每日/启动空闲执行；清理使maintenance_revision递增。数据删除后刷新最早记录/大小和筛选缓存；大文件空间回收不放请求热路径。

## 9. GPUI Kit界面实现

### 9.1 页面状态与Entity

`Proxy`使用`Page::Providers | Page::Statistics`和`Entity<Statistics>`。模型管理保留原三栏。统计页替换主内容为宽单列，不渲染原右侧日志；不要将统计UI塞入models的center/right函数。

```text
Statistics
  filter: StatsFilterDraft（SelectState/InputState实体）
  active_tab: Requests | Providers | Models
  dashboard: Option<DashboardDto>
  table_state: Entity<TableState<UsageRows>>
  prices/state/dialogs: P1专用状态
  loading_dashboard/loading_rows/error/last_updated
  generation: u64（客户端筛选代数，区别服务端commit generation）
  pending_generation/last_refresh/refresh_interval
  cursor_stack/current_cursor/as_of_id
  visible_series: BTreeSet<SeriesKind>
  _subscriptions/_tasks: 保存生命周期句柄，避免事件订阅立即释放
```

`Proxy.providers_expanded` 控制供应商二级列表与操作栏；两个一级标题放入 Sidebar.header，供应商放入可滚动 content，操作放入 Sidebar.footer，因此长列表不影响按钮可达性。窗口右上图标使用自定义 Button 尺寸，按 GPUI Kit 的图标 75% 比例得到 18.2px 图标；工具栏高度 26px 与主标题行对齐。

统计页以 8px gap 依次放置状态、筛选、Token 总用量、趋势、明细。`more_filters_open` 仅控制额外条件显示，不重置已应用筛选；日期与刷新使用独立 Popover，更多区只保留附加筛选。0.7.3 的 Select 样式作用于内层输入，外层 BaseSelect 固定 size_full，横向行内必须用明确宽高的 Div 包裹；主筛选、更多筛选和币种选择均约束各自的 host，避免整行占满和异常空白。筛选卡片 on_prepaint 记录可用宽度，预留时钟/日期/操作空间后分配三个下拉框宽度；展开区搜索、用量和测试开关按基线横排并可换行。两个统计模块标题共用 stats_heading；GPUI 原生组件 locale 随应用语言同步。

日期面板直接复用 `component::calendar::{Calendar, CalendarState, Date}`，Popover 使用受控 open 与 `on_open_change`。`draft_preset/draft_follow_now/custom_from/custom_to` 仅为草稿，打开时从已应用范围初始化；`apply_date_draft` 校验后一次性写入 `range/custom_range/custom_follow_now` 并 mark_dirty。取消/外部关闭只清草稿错误。Calendar 完整选择更新日期并保留时分，显式结束日期关闭跟随；不完整选择禁止确认。输入轮询只同步草稿日历，不使查询失效。`resolve_custom_range` 解析跟随时钟，`reset_pagination` 捕获 frozen_range，翻页不移动窗口。面板打开时暂停自动刷新，确认后按新 generation 重新查询。自动刷新菜单的时钟使用现有 AssetSource 的轻量 SVG 路径；日期/时钟/左箭头均有对应图形。

统计backend线程最多一个dashboard和一个rows查询在途，队列full只保留最新筛选意图，不排100个过期任务。UI回调根据request_tag/generation判断，丢掉迟到回复；过期错误也不能覆盖当前状态。

现有250ms timer可以调用statistics.tick判断debounce/refresh/replies，不另建永不退出的while轮询。只有当前Page=Statistics且窗口前台、未被模态遮挡时自动刷新；离开页保留显示数据但停止查询。只在数据/状态变化时notify，不每tick无条件重绘路径。

### 9.2 布局树

```text
Root v_flex size_full
 ├ TitleBar（现有）
 ├ AppHeader h=48（现有品牌/语言/退出）
 └ h_flex flex_1 min_h_0
    ├ AppNavigation w=168（模型管理 / 使用统计）
    └ StatisticsScroll flex_1 min_w_0 overflow_y_scroll p=16
       ├ Header + quality badge
       ├ Filters card（wrap，控件高32/36，gap8）
       ├ Warning card（仅有警告时显示）
       ├ Overview（主指标 + 次级3/4列可换行）
       ├ Trends（主图280 + 可选费用图140 + legend）
       ├ Details（Tabs + DataTable固定高400 + Pagination）
       ├ Pricing disclosure（P1）
       └ Storage disclosure（P1）
```

为滚动容器、charts、table、filters设置稳定element id。中间flex节点必须min_h_0/min_w_0避免表格撑破窗口。不要直接复用现有card()的`.h_full()`给所有统计卡，否则每张都占满屏高；新增stats_card()按内容高度，仅表格/图表给固定或最小高度。

复用Kit的Button、Select/SelectState、Checkbox、Label、Tag、Dialog、DataTable。页签可用Kit已存在组件，若版本API不确定则用三个互斥Button呈现，保持键盘和选中语义。日期第一版可用预设Select+自定义ISO日期Input，读取验证通过再发请求，不要求为此增加日期组件依赖。

UI查阅本机`gpui-component-0.7.3`示例确认select/column/table API，不写不存在的`LineChart::series()`或`DataTable::pagination()`。

### 9.3 请求DataTable

参考model_table.rs实现`TableDelegate`，行数据为`Vec<RequestRowDto>`而非无限Value数组。只传本页最多100行。

请求表现有九列：时间、供应商、模型、请求 ID、Token/缓存命中率、金额、耗时、tok/s、状态。`usage_table.rs::fitted_widths` 按实际表格容器宽度分配自动列，时间固定 110，其他列有可读的最小值；模型/Token/供应商权重较高，数值列的自动最大值较小。`stats-table-host.on_prepaint` 测量宽度并只在尺寸变化时刷新表头。宽度不足时保留横向滚动，仍只渲染当前服务端分页。

除时间列外启用 `Column.resizable(true)`；订阅 `TableEvent::ColumnWidthsChanged`，只将相对上次布局发生变化的列记为用户覆盖宽度，其余列继续自适应。翻页和轮询只更新 delegate.rows 并 notify，避免刷新列元数据中断拖动；切语言重建表头时读取同一覆盖值。宽度仅保留于当前设置窗口，不写统计 SQLite。

每行缓存比例和输出速率从 RequestRowDto 已存的计数和单调时钟耗时派生，不新增 SQLite 字段或接口版本。公式及未知值规则见需求 3.3/4.3，单测覆盖零、缺失、越界和流式/非流式差异。供应商从 Start 事件写入的 provider_name 快照直接查询；不关联当前配置补名，集成测试用真实管理接口完成复制、调用、改名、再次调用、删除，再验证 SQLite 和详情接口的名称均保留。

列排序只提供API支持的白名单，第一版请求列表固定时间降序，不在GUI本页点击列头排序假装全量已排序。点击行详情使用request_id请求新详情。显示长模型ellipsis+tooltip，可复制request_id，禁止把未知字段自动dump成原始JSON给用户。

### 9.4 图表：单序列原型与多序列成品

锁定版本确有 `gpui_kit::component::chart::{LineChart,AreaChart,BarChart}`，以及`gpui_kit::base::plot::{Plot,PlotElement,...}`。已核实LineChart的`.x/.y/.stroke/.linear/.id/.interactive/.appear/.tooltip_value/.y_domain`，**一个LineChart是一条序列**。

先做原生单序列验证（以下是版本核对后的示意，用实体实际类型补全）：

```rust
use gpui_kit::component::chart::LineChart;
let chart = LineChart::new(points)
    .id("usage-total-chart")
    .x(|p: &ChartPoint| p.label.clone())
    .y(|p: &ChartPoint| p.value as f64)
    .stroke(rgb(0x5477ee))
    .linear()
    .appear(false);
// chart放入min_h(px(280.))、w_full的容器；points只能含连续已知段。
```

不能过滤未知点后直接画一根线，那会跨过缺失区间。成品要有多条序列和断点，推荐在`usage_chart.rs`实现一个UsagePlot，使用Plot/PlotElement，单一坐标轴和统一hover。不要叠4个各自带坐标轴/tooltip的LineChart。

`UsagePlot`字段：Arc<ChartSnapshot>、稳定ElementId、可见series集合、theme色、hovered index、绘图矩形缓存。ChartSnapshot由已验证series DTO生成，最多400桶×4条线，值保留Option<i64>，不是NaN魔数。

#### 绘制步骤

1. 以容器bounds预留左64px、右16px、上12px、下28px给坐标标签，得到plot_rect；所有线/hover使用同一矩形。
2. x用桶中心真实UTC时间按比例映射，保证DST小时长度正确。`x=left+(center-first_center)/(last_center-first_center)*width`；只有一桶画居中点。
3. y只看可见序列的已知最大值，取nice_max（1/2/5×10^n向上），最小=0；全零用[0,1]，全未知不画线只显示空状态。
4. y映射`top+height*(1-value/ymax)`，绘制5条浅色水平网格，X轴按实际宽度挑2–8个标签并包含两端；整个时间范围超过36小时显示月/日，日内显示时:分，避免30天日桶都标为00:00。
5. 每条线按Option切成连续known段，用PathBuilder或Kit plot shape::Line的linear曲线，stroke=2px。None处断开；一段仅一点画圆点。部分覆盖桶用空心/低透明度点，不改变数值。
6. 绘图区保持白底，不做面积填充和堆叠；避免缓存读已包含输入的视觉重复相加。
7. unknown/degraded 桶不涂背景；图表角落显示质量提示，悬浮显示每桶覆盖状态。没有完整 usage 的桶保留 None 并断线，不能画到零。旧 rgba(0xf1f4f8) 缺少 alpha 字节，实际造成青色背景，现已移除。
8. 图例在绘图区外，每项有颜色+文字+toggle状态；开关只重建几何，不访问API。

自定义Plot须按0.7.3源码实现：`paint(bounds,window,cx)`；`id()->Some(ElementId)`；`tooltip_state(position,bounds,cx)`；`tooltip(...)`。需要标签测量时放prepaint返回Vec<AnyElement>；不能在paint里随意layout_as_root。PlotElement的构造和IntoElement实现参照该版本源码，不在文档假设不存在的宏导入。

hover 最近桶以 x 距离查找。PlotElement 传入的是局部坐标，必须扣除左侧标签留白，不能加减屏幕/滚动原点；gutter 不触发提示。使用 GPUI Kit `component::plot::tooltip::{Tooltip,CrossLine,Dot}` 显示一条竖线、各 known 点和统一卡片，自动按容器边缘翻转。卡片列出可见序列、桶范围、请求/完整用量数及覆盖情况。tooltip显示精确整数，卡片缩写只用于总览。图表提供可读文本摘要帮助可访问性，不要求读屏逐像素读取。

费用图同一时间轴，独立Y轴单位货币，UI选择USD/CNY；缺价默认0也进入费用图，可提示“未配置价格，按0计算”。费用只有known部分的桶也必须带coverage提示。第一版不实现双轴混图。

#### 渲染性能

ChartSnapshot只在新的dashboard或图例变化时生成；geometry cache键为data_generation+可见series+viewport_size+theme。render不clone全量历史，不逐帧查DB。使用稳定id、`.appear(false)`或无appear_generation，避免刷新动画反复回零。点数上限400，不用step_by丢点。

### 9.5 P1表单

当前价格用原生对话框：供应商、上游模型、币种、计价模式和四项每百万价格；空白单价项按0。提供直接编辑/删除，不显示生效区间或已引用锁定。保存成功提示“仅新调用使用新价格，历史保持原价”；请求详情展示自己的单价与金额。

存储区：采集开关、180天等保留选项、大小/最早时间/质量、清除按钮。清除预览返回条数后再打开确认对话框，确认token有效期内发送start，过期重新预览。维护进度2秒轮询且只在该任务UI可见时进行，完成后刷新dashboard/rows，不阻塞整个设置界面。

## 10. 测试方案与固定数据

### 10.1 设计阶段

先跑 `python3 docs/statistics/validate_design.py`。这只证明DDL、SQL口径和样例，不证明Rust已实现。脚本仅用TemporaryDirectory，不读实际用户数据库。

### 10.2 Rust单元

- Collector对同一SSE按每个可能字节边界切chunk，包含UTF8、CRLF、多data行、comment、DONE、重复usage、冲突usage、usage-null、超大事件后恢复。
- JSON正常/超过256KiB/非法JSON、压缩编码、不认识Content-Type；禁止正文持久化。
- Guard正常EOF/错误/提前return/Drop/shutdown只Finish一次，permit都释放；队列满下已接纳终态仍可send。
- store新库/迁移rollback/未来schema/崩溃恢复/FK/去重/同事务cost/维护分批；数据库锁与磁盘满用故障注入。
- query所有过滤组合、NULL/0、带测试/不带测试、价格币种、分页同毫秒并列/新插入/游标失效、DST日/周边界。
- pricing flat/read_discount/disjoint、缺价默认0、调价/删除后历史快照不变、非法拆分、溢出、半纳舍入，不用浮点断言金额。

### 10.3 二进制Mock集成

新增`scripts/statistics_test.py`沿用scripts/smoke.py模式：临时config-dir+假Key+本地Mock上游+随机端口+核心headless；测试后终止/清理，不连接用户正在运行的代理。使用现有管理API测试outer message双层JSON与错误。

Mock包含：多供应商同模型、流式完整usage、无usage、中途断流、客户端cancel、JSON、HTTP429、压缩响应、SSEerror、超大data事件。比对直连/代理body的hash或逐字节一致性；request observe模式验证原有model替换外其他bytes保持。

断言SQL记录和API结果一致、重启恢复、测试请求默认排除、设置关闭后新请求不采集、当前价编辑/删除不改历史、清理不删进行中、新请求、以及普通local_key不能调用stats。尽量通过正式API验收，必要只在已停止的临时核心上离线检查测试DB。

### 10.4 编译和回归命令

在独立worktree中核对脚本CLI后执行（已核实下列现有核心脚本使用`--exe`，新脚本或不同版本仍先看--help）：

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
cargo fmt --manifest-path gpui/Cargo.toml --check
cargo test --manifest-path gpui/Cargo.toml
cargo clippy --manifest-path gpui/Cargo.toml --all-targets -- -D warnings
cargo build --manifest-path gpui/Cargo.toml --release
python3 docs/statistics/validate_design.py
python3 scripts/smoke.py --exe target/release/workbuddy-proxy
python3 scripts/multi_routing.py --exe target/release/workbuddy-proxy
python3 scripts/connection_test.py --exe target/release/workbuddy-proxy
python3 scripts/core_compatibility.py --exe target/release/workbuddy-proxy
python3 scripts/benchmark.py --exe target/release/workbuddy-proxy --output bench-results/statistics.json
```

另按变更范围运行smoke、multi_routing、connection_test、model_capabilities_test、core_compatibility及benchmark；使用实际脚本帮助指定独立临时目录/二进制，不改已有用户数据。

### 10.5 界面与平台

Mac截图至少1100×700、1180×780、1440×1000：空库、完整数据、未知usage、数据质量警告、长模型名、多币种、表格分页、详情、定价、维护状态。核对无溢出/遮挡、滚动正常、中文/英文、键盘操作、草稿保存、关闭重开设置进程。

Windows需真实目标CI编译/打包；Mac编译成功不能代替Windows证据。真实供应商逐家验证include_usage支持、usage字段语义、缓存拆分，填验收矩阵；未验证标unknown，不用Mock结果宣称真实账单准确。

## 11. 开发批次与完成定义

| 批次 | 输出 | 本批完成检查 |
| --- | --- | --- |
| S0 | worktree、调研差异、DTO和正式迁移 | 设计SQL通过，协议明确，无业务改动污染 |
| S1 | store/pricing基础、Guard、Collector | Rust单元通过，有界/取消/去重可证 |
| S2 | relay与连接测试接入、shutdown/恢复 | Mock逐字节透传和异常路径通过 |
| S3 | stats_health/dashboard/requests/detail/breakdown/filters | API端到端、分页、时区、口径一致 |
| S4 | GPUI导航/cards/filters/table/detail/原生charts | 实际GPUI构建与目标尺寸截图 |
| S5 | P1设置/当前定价/快照/维护与页面 | 调价/删除历史不变、缺价默认0、清理边界 |
| S6 | 回归/性能/平台矩阵/用户文档 | 有实际结果、未验证边界清楚，按授权交付 |

每批写短进度文件（建议`docs/statistics/IMPLEMENTATION-STATUS.md`）记录HEAD/文件、完成API、实测命令和结果、未完成项及下一步。只在有新变更或失败时重复相关测试，不用无意义全量重跑拖延。

功能完成：本需求P0+P1全部可操作，既有模型管理/同步/思考/多模态/取消路径回归通过，有SQL/Mock/GUI/平台证据，README和架构说明更新，未保存草稿和真实用户配置无损。文档中“当前不解析SSE”的旧描述应在实现后改为“原字节透传+有界统计旁路”，本设计阶段不提前改成已实现。

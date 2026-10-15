# 使用统计实现进度（IMPLEMENTATION-STATUS）

当前状态（2026-10-16）：统计复核与可编辑价格/不可变调用快照已完成并合入本地main；40ec917补齐成本定价、采集与存储固定分页及侧栏层级。后续协议扩展复用同一统计链路，schema 4仅增协议字段，另修复无流量恢复计数持久化和长标签入库边界。最终验证见 [协议与统计联合验收](../protocol-compat/FINAL-VALIDATION.md)。

以下为历史开发记录，历史“修复中/阶段失效”文字不代表最新交付状态。

> **用户价格口径修订（2026-10-15）**：`PRICING-DECISION.md` 撤销并替代旧价格规则——
> 取消生效区间、取消“已引用价格不可修改/删除”、取消按当前价格重算历史。
> 改为：当前价格表可直接增删改；每次调用在接纳时冻结单价快照并在终态结算；
> 缺价默认 0 元（`default_zero`），Token 未知仍为 NULL；`stats_reprice_start` 停用。
> 旧的 F07/F11 价格区间与重算要求不再执行，F03 的纳单位契约继续有效。

2026-10-15 复核（`REVIEW-2026-10-15.md`）确认存在统计正确性缺陷与 P1 界面缺项，因此此前的
“S0–S5 完成 / S6 完成”声明已失效。当前状态按 F01–F11 逐项推进，只有对应回归通过才恢复完成状态。
本轮不 push、不 release、不安装、不操作真实用户数据库。

## 1. 基线与环境

| 项 | 值 |
| --- | --- |
| 主工作区 | `/Volumes/1TB/Downloads/git/workbuddy-proxy`（未改动，未提交） |
| 主工作区 HEAD | `e1725c3` `test: retry transient Windows fixture deletion during core replacement`（`main`） |
| 设计包基线 | `docs/statistics/README.md` 记录调研基线 `0b2fe84`／v0.5.3／GPUI Kit `=0.7.3` |
| 本任务 worktree | `/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics` |
| 本任务分支 | `codex/usage-statistics`（从 `e1725c3` 创建，无冲突后缀） |
| 主工作区未提交内容 | ` M README.md`、`?? docs/statistics/`（只把 `docs/statistics/` 复制进 worktree，未复制 README 改动） |
| 共享构建目录 | `CARGO_TARGET_DIR=/Volumes/1TB/Downloads/git/workbuddy-proxy/target`（只复用依赖缓存，不写源码） |
| 工具链实测 | `rustc 1.98.1` / `cargo 1.98.1`，edition 2024 |

### 与设计文档的差异（已核对实际代码后调整，均有实现依据）

1. 设计包记录 `scripts/model_capabilities_test.py` 有用户修改；实际工作区该文件**无改动**。
2. 设计假设 Rust 1.88；本机 1.98.1，编译通过。
3. 依赖实际解析：`rusqlite 0.40.2` / `libsqlite3-sys 0.38.2`、`chrono 0.4.45`、`chrono-tz 0.10.4`、`iana-time-zone 0.1.65`。
4. `crate::journal::now_ms()` 返回 `u64`；统计模块统一用 `stats::now_ms() -> i64`。
5. `rate_nanos` 序列化方向文档未定义。实现采用**人类可读的每百万价格**（`"2"` = 2 货币单位/百万 token），`stats_prices_list` 回传同一形式，避免 UI 往返时把纳单位整数再次当价格保存。
6. `stats_dashboard` / `stats_requests` / `stats_breakdown` 额外返回 `timezone`（核心解析出的 IANA 名），GUI 用它显示真实时区名。
7. 项目没有 `config::restrict_permissions`，统计模块自带 `stats::restrict_permissions`（Unix 0600）。
8. **GPUI Kit 0.7.3 的 `DataTable` 行高由 `Size` 固定**（Small=30px，无 `row_height` 配置）。设计要求的“双行 64/72px”改为**单行紧凑单元格 + 行内 tooltip + 详情对话框**呈现相同字段；表格固定高 400px，横向滚动由 `DataTable` 提供。
9. **GPUI 第一版只提供“本机时区”**。核心已实现任意 IANA 时区与 DST 分桶并有单测（`America/New_York`、`Australia/Lord_Howe`、`Asia/Kolkata`），但 GPUI 侧无 chrono-tz 依赖，选择器只显示核心解析出的本机时区名，避免用固定偏移显示错误时间。
10. GPUI 测试模块**不能 `use super::*`**：它会把 `gpui_kit` 的 `#[gpui::test]` 属性宏引入作用域并遮蔽内置 `#[test]`，实测导致宏展开无限递归。测试改为显式导入。
11. `gpui` crate 需要 `#![recursion_limit = "256"]`；构建其测试目标需要 `RUST_MIN_STACK=67108864`（rustc/syn 解析深度，非代码缺陷）。
12. 100 万行总览的优化手段（不改变口径）：
    - summary 与 series 直接聚合 `usage_requests`，不再重复求值视图中的 cost 连接；
    - 请求分页只做一次 cost 连接（此前在视图上又连接了一次 `usage_costs`，导致 100 万行分页 2 秒）；
    - 费用扫描在“从未应用价格规则”时短路，未定价/用量不全计数由 summary 推导（cost 行与已结束请求 1:1，由 schema 保证）；
    - 行数预算复用 summary/第一页已有的 `COUNT(*)`，不再额外全表计数。

## 2. 阶段进度

| 阶段 | 状态 | 证据 |
| --- | --- | --- |
| S0 worktree／版本／DTO／正式 migration | 完成 | `validate_design.py` PASS；`src/stats/migrations/0001.sql` |
| S1 store／Collector／pricing／Guard | 完成 | 45 个统计单测 |
| S2 relay／连接测试／退出／重启接入 | 完成 | 33 项集成检查（逐字节透传、取消、崩溃恢复） |
| S3 P0 管理接口 | 完成 | 同上：筛选、分页、详情、汇总、权限、固定错误码 |
| S4 GPUI 页面 | 代码完成，截图受限 | GPUI 编译、30 单测、release、进程与 1180×781 窗口实测 |
| S5 P1 价格／设置／重算／保留／清理 | 完成 | 价格保存/重叠拒绝/重算 1360000 nanos、暂停采集、清理确认、自动 retention |
| S6 回归／性能／文档 | 完成 | 见 §4、§5；README 与 ARCHITECTURE 已更新 |

## 3. 交付代码

核心（`src/`）：

- `stats/mod.rs`：`StatsHandle`、writer/reader 线程拓扑（事件 1024／控制 16／查询 8）、控制通道、health、维护编排、退出 flush（≤4s）。
- `stats/dto.rs`：协议 v1 DTO 与枚举，纯 serde/std，GPUI 通过 `#[path]` 共享。
- `stats/collector.rs`：有界 SSE／JSON 旁路（事件 64 KiB、驻留 128 KiB、JSON 副本 256 KiB）、`openai_chat_v1` 规范化。
- `stats/event.rs`：`EventQueue`（两槽预留）、`RequestGuard`（终态只结算一次；用 `Content-Length` 区分“正常完成”与“客户端取消”）。
- `stats/store.rs`：WAL 连接、迁移（backup API）、崩溃恢复、Start/Finish 事务、cost 同事务结算、retention/clear/reprice 分批、价格规则读写。
- `stats/query.rs`：统一过滤 builder、2 秒 `progress_handler` 预算、100 万行预算、chrono-tz 分桶（DST／周一为周起始）、keyset 分页、i128 费用累计。
- `stats/pricing.rs`：flat／read_discount／disjoint、i128 定点、分项半纳舍入、溢出状态。
- `proxy.rs`：`Bridge.stats`、路由成功后的接纳点、结构化流包装、`Bridge::test` 统计、`CONTROL_PROTOCOL = 5`、`stats_*` 静默分派与 1 MiB 响应上限、`usage_mode=request` 的 `stream_options` 合并。
- `main.rs`：`CONTROL_PROTOCOL = 5`、退出前 `stats.shutdown()`。

界面（`gpui/src/`）：

- `stats_backend.rs`：独立统计队列、3 秒超时、禁代理/重定向、不加载 config、不取 snapshot。
- `statistics.rs`：筛选条、质量提示、总览、趋势、明细页签、分页、详情对话框、定价与采集存储折叠区、供应商 observe/request 切换、维护进度、清理预览/确认。
- `usage_table.rs`：请求日志与供应商/模型汇总的 `TableDelegate`（紧凑单元格 + tooltip）。
- `usage_chart.rs`：自定义 `UsagePlot`（`Plot`/`PlotElement`），单一坐标与 tooltip、None 断线、unknown 桶底纹、按真实桶中心时间映射。
- `ui.rs`：`Page::Models | Page::Statistics` + 168px 导航；统计页不渲染右侧日志栏。

测试与脚本：

- `scripts/statistics_test.py`：33 项端到端检查（临时目录 + 假 Key + 本地 Mock）。
- `scripts/statistics_bench.py`：种子数据 + 查询预算 + 转发增幅测量。

## 4. 实测命令与结果

```sh
python3 docs/statistics/validate_design.py
# PASS: 8 tables; constraints; documented summary/bucket/keyset/cleanup SQL; NULL vs zero;
#       test filtering; pricing; deduplication; FK integrity

cargo fmt --check                                   # OK
cargo test                                          # 86 passed; 0 failed
cargo clippy --all-targets -- -D warnings           # 无警告
cargo build --release                               # OK

cargo fmt --manifest-path gpui/Cargo.toml --check    # OK
cargo test --manifest-path gpui/Cargo.toml           # 30 passed; 0 failed
RUST_MIN_STACK=67108864 cargo clippy --manifest-path gpui/Cargo.toml --all-targets -- -D warnings
# 无警告
cargo build --manifest-path gpui/Cargo.toml --release # OK (14 MB)

python3 scripts/statistics_test.py --exe target/release/workbuddy-proxy
# PASS: 33 statistics integration checks

python3 scripts/smoke.py --exe target/release/workbuddy-proxy                # PASS
python3 scripts/multi_routing.py --exe target/release/workbuddy-proxy        # PASS
python3 scripts/connection_test.py --exe target/release/workbuddy-proxy      # PASS
python3 scripts/core_compatibility.py --exe target/release/workbuddy-proxy   # PASS（含 protocol=4 旧核心场景）
python3 scripts/model_capabilities_test.py --exe target/release/workbuddy-proxy # PASS
```

### 4.1 33 项集成检查覆盖

逐字节透传（流式／非流式）、429 状态与响应体原样、observe 请求保真、request 模式只合并
`include_usage` 且保留未知字段、`stream=false` 不注入、非 object `stream_options` 原样转发、
连接测试仍报成功、A01/A02（完整 usage 只计一次，缓存读 800 ⊆ 输入 1000）、A03（缺失保持 NULL）、
A08（冲突 usage = invalid 且不写数字）、A09（超大事件后仍取到 usage）、压缩响应不猜 usage、
A06（截断的 200 流不是成功；429 记 failed/upstream_http）、A04（客户端取消 = cancelled/client_drop）、
JSON 用量、编码 SSE、总览与列表一致、模型精确过滤、keyset 分页遍历不重不漏、page_size 校验、
详情不含正文、未知 request_id → stats_not_found、breakdown 与筛选选项、测试来源默认排除、
local_key 不能读统计、价格重叠拒绝、重算得到 1360000 nanos、暂停采集只影响新请求、
清理需 preview token、崩溃恢复把 in_flight 记为 interrupted 且不伪造耗时/Token。

### 4.2 性能实测（`scripts/statistics_bench.py`，Apple Silicon，本机临时目录 + 本地 Mock）

10 万行种子数据（`bench-results/statistics-100k.json`）：

| 指标 | 实测 | 目标 |
| --- | --- | --- |
| dashboard（31 桶） | p50 153.7 ms，p95 165.4 ms（n=20，1 个 1224 ms 尖峰） | < 500 ms ✅ |
| 请求第一页（20 行） | p50 16.2 ms，p95 16.5 ms | < 200 ms ✅ |
| 游标翻页（100 行） | p50 1.1 ms | — |
| 供应商/模型汇总（≤50 组） | p50 151.8 ms | 2 s 预算内 ✅ |
| 请求详情 | p50 0.26 ms | — |
| 带模型筛选的 dashboard（8.3 万行） | 21.0 ms | — |
| 转发 P95（采集关 → 开） | 0.43 ms → 0.25 ms（Δ −0.18 ms） | ≤ max(5%, 2 ms) ✅ |

100 万行种子数据（`bench-results/statistics.json`）：

| 指标 | 实测 | 目标 |
| --- | --- | --- |
| dashboard | p50 1930 ms，p95 1939 ms（n=4） | < 2 s ✅（余量很小） |
| 请求第一页 | p50 170 ms | 2 s 预算内 ✅ |
| 游标翻页 | p50 1.5 ms | — |
| 供应商/模型汇总 | p50 1661 ms | 2 s 预算内 ✅ |
| 带模型筛选的 dashboard（8.3 万行） | 226 ms | — |
| 转发 P95（采集关 → 开） | 0.38 ms → 0.38 ms（Δ 0） | ✅ |

边界说明：

- 超出 100 万匹配行时核心返回 `stats_query_too_large`，超过 2 秒预算返回 `stats_query_timeout`，
  都是明确的错误而不是被截断或伪造的数据。
- 偶发 ~1.2 s 的 dashboard 尖峰在 10 万行数据上出现过（20 次中 1 次），与本机磁盘 I/O 竞争有关；
  稳态 P50/P95 如上表。100 万行时余量很小，接近预算时会更频繁地返回超时错误。
- 首次查询（冷 page cache）比稳态慢，脚本单独记录 `dashboard_cold_ms`。

## 5. 界面验证现状

已实测：

- `workbuddy-proxy-gpui` release 构建成功（14 MB），`gpui` 单测 30 项通过。
- 非 headless 启动核心后设置进程真实创建窗口：System Events 报告
  `position=(690, 311) size=(1180, 781) AXMinimized=false AXMain=true AXFocused=true AXStandardWindow`，
  与设计的 1180×780 一致。
- 核心与设置进程就绪（`gpui.json`、`frontend.json`、`instance.json` 生成，`/health` 返回 `control_protocol=5`）。

受限（未验证）：

- 本机 `screencapture` 缺少“屏幕录制”权限，只能得到桌面壁纸（菜单栏可见），**无法自动截取窗口内容**。
- macOS 辅助功能树也只暴露窗口按钮（`close button / full screen button / minimize button`）与两个 group，
  GPUI 内容没有暴露给 AX，因此也无法用 AX 读取页面文本来间接验证渲染。
- 结论：设计要求的 1100×700 / 1180×780 / 1440×1000 截图与中文/英文、空/满/未知/错误/长模型/多币种/
  维护态的界面检查仍需人工在本机完成，或在授予屏幕录制权限后重跑。

## 6. 明确未验证的边界

- 真实供应商 usage 字段语义与计费准确性：**未验证**（不使用 Mock 结果对外宣称计费正确）。
- Windows 编译、打包与人工验收：**未验证**（本机仅 macOS 证据）。
- GPUI 界面截图与交互人工验收（中英文、空/满/未知/错误/长模型/多币种/维护态、键盘与 Esc）：
  **未完成**（见 §5）。
- 100 万行 dashboard 逼近 2 秒预算，余量小；更大数据量会以超时错误拒绝而不是降级出错误数字。
- 首版 GUI 只有本机时区选择（核心支持任意 IANA）。

## 6.5 第九轮：R8-01…R8-06

状态：**R8-01／02／03 核心已接入真实页面；R8-04／05／06 已完成。§8 其余 GUI 项未完成。**
schema **v3**；统计 API **2**；control protocol **6**。

### 已完成

| 编号 | 实际改动 | 证据 |
| --- | --- | --- |
| **R8-06** 共享格式化导致 clippy 失败 | `format_amount(nanos: i64)` 改为调用 `format_amount_text`，核心真实使用该函数（`cargo clippy --all-targets -- -D warnings` **exit 0**）。同时修正整数金额显示（`0` 而非 `0.0000`） | 新增 `amounts_beyond_i64_still_format_exactly`：超 `i64`（`9223372036854775808`）、超大、0、阈值 `99999/100000`、负数、非数字 → `—`；GUI 侧补同样断言 |
| **R8-01** 对话框生命周期 | 保存按钮**不再立即关窗**；成功后由**回复本身**带 `window` 关闭；`on_ok`（Enter）与按钮同路径；入队失败复位 `pending`/`tag` 并保留输入；等待中禁用重提并在按钮显示"保存中…"；`cancel_price` 统一处理 X／Escape；删除失效的"重算历史"提示 | 控制器 `apply_reply` 代际测试（旧回复不关新草稿、失败保输入、成功才关） |
| **R8-02** 供应商 Select | 新增 `sync_price_providers`：真实把 `provider_options` 填入对话框自己的 Select，**草稿供应商即使不在首批选项里也会补入**；新建默认取当前筛选供应商，为空则要求选择；打开对话框前同步 | 编译接线 + 控制器 `is_complete` 拒绝空供应商 |
| **R8-03** 回复路由 | 新增 `route_reply(kind, seq, dashboard_seq, gates, tab)`，**页面真实调用**：stale 分支**不再清 pending**；Rows→Groups 切 tab 时两条 gate 立即失效；回复按自身 kind＋tab 解码，旧 Rows 不会被当 Breakdown；`mark_dirty` 立即废弃 `dashboard_seq`；Groups 游标过期只回退分组页 | 控制器 `a_reply_is_only_applied_while_its_own_tab_is_on_screen`、`a_dashboard_reply_from_the_previous_filter_is_ignored` |
| **R8-04** 扫描失败被吞 | `scan_recovery_batch` 收集 `Retry`/`Err` 的 ID 并在**同一事务内先登记 `stats_recovery_backlog`**；扫描水位只在 **commit 成功之后**发布（失败不推进） | 新增 `a_failed_repair_is_queued_before_the_scan_advances`：触发器拒绝 cost 更新 → 2 条入队、仍 pending、扫描不得谎称完成；解除故障后**无新流量**排空并全部结算 |
| **R8-05** 进度被当失败退避 | `recover_collection` 返回 `RecoveryProgress::{Complete, Progressing, Failed}`：`Progressing`（还有批次但无失败）**不退避**，立即继续（仍在每批之间交还队列与控制通道）；只有 `Failed` 指数退避 | 新增 `a_large_legacy_repair_finishes_through_the_real_writer`：**真实 `StatsHandle` 入口** 3000 条旧 pending 在 **1.97 s** 内全部修复并 ready |

### 未完成（精确断点）

| 项 | 断点 |
| --- | --- |
| 价格列表分页／搜索 | `request_prices` 仍只取首 100 条，未接 `next_rule_id` |
| 默认币种入口 | 后端支持 `stats_settings_update.default_currency`，界面无入口 |
| 日期草稿与应用范围分离 | 非法/反向/>366 天仍未就地拒绝（未静默 max 修正也未实现分离） |
| 供应商／模型搜索与 `next_key` | 未实现；切供应商未即时收窄模型 |
| 语言重建 | Select 选项／placeholder／对话框未随语言重建 |
| 前台／最小化／模态／离页生命周期 | 未实现 |
| 请求与分组 20/50/100 三页真实动作回放 | `CursorStack` 单测覆盖 1→2→3→2→1，但未接真实页面动作回放 |
| reader 执行期维护 revision/run 复查 | 未实现 |

### 本轮实测（真实退出码）

核心 `cargo test` **124 passed**（exit 0）、`clippy --all-targets -- -D warnings` **exit 0**、`fmt --check` exit 0、`build --release` exit 0；
GPUI `cargo test` **52 passed**（exit 0）、`clippy -D warnings` exit 0、`fmt --check` exit 0；
`statistics_test.py` **39 项通过**（exit 0）。

## 7. 复核缺陷修复（F01–F11）

| 编号 | 状态 | 修改 | 回归证据 |
| --- | --- | --- | --- |
| F01 usage 合并 | 已修复 | `collector.rs`：`UsageAccumulator` 重写为逐字段父项冲突检测 + 合并后复校验 + 细分独立降级；非法 usage 不携带数值。`store.rs`：新增批次隔离回归 | `conflicting_partial_parents_never_merge_into_an_invalid_complete`、`a_later_cache_detail_extends_a_complete_parent_instead_of_conflicting`、`complementary_and_repeated_usage_events_still_behave`、`conflicting_subfield_drops_only_the_subfield`、`an_invalid_usage_never_rolls_back_the_whole_batch` |
| F02 故障恢复 | 已修复 | `store.rs`：`BatchOutcome` + 逐条隔离重试、`probe_write`、`enter/leave_degraded`、`mark_degraded_window`、`settle_orphans`。`mod.rs`：空队列探测（0.5s→30s 指数退避）、degraded 区间持久化、孤立行结算、degraded 期间保持接纳、退出 flush 4s 预算、丢弃按请求计数 | `a_transient_database_lock_recovers_settles_orphans_and_resumes`（真实第二个 SQLite 连接持 `BEGIN IMMEDIATE`） |
| F03 价格纳单位 | 已修复 | `pricing.rs`：`parse_rate` 改为严格整数纳单位（拒绝小数/符号/科学计数）；`dto.rs`：新增 `parse_human_amount` 供表单一次性定点转换；`store.rs`：list/detail 原样返回纳单位整数字符串；GUI 表单提交前转换。**未对数据库做任何乘除 1e9** | `api_rate_strings_are_nano_units_per_million_tokens`、`the_settings_form_converts_a_human_amount_exactly_once`、集成脚本新增小数纳值拒绝检查 |
| F04 游标与分页 | 已修复 | `query.rs`：`validate_cursor`/`validate_breakdown_cursor`（范围、filter、run、maintenance revision）；`mod.rs` 在 reader 执行前校验。GUI：冻结查询窗口、保存服务端 `next_cursor`、游标栈改为"上一页起始游标"、`has_more` 用服务端字段、失效时提示并回到第一页、新增 20/50/100 页大小切换 | 集成脚本新增：篡改 run_id/filter_key/maintenance_revision/负数 → `stats_cursor_expired`；换 filter → expired；合法游标连续翻页；遍历前等待统计落库 |
| F05 SSE 分块不变性 | 已修复 | `collector.rs`：驻留预算只统计解析器真实状态，不再把输入块长度计入 | `any_chunking_of_the_same_bytes_yields_the_same_usage`（195 KB 合法体按 1/7/4096/65536/整块/随机分块结果一致）、`an_oversized_single_event_is_still_bounded` |
| F06 JSON 终态 | 已修复 | `collector.rs`：新增 `JsonVerdict` 与幂等 `settle_json()`；`event.rs`：`terminal()` 改为 `&mut self`，在定终态前完成有界解析 | `a_json_error_object_is_failed_even_with_http_200`、`malformed_json_is_never_completed`、`a_healthy_json_body_still_completes_with_usage` |
| F07 重算与费用状态 | 已修复 | `query.rs`：重算查询补 `upstream_started`/`input_breakdown`/`input_uncached_tokens`；`store.rs`：`settle` 使用请求自身事实；快捷路径改为"存在非 unpriced cost 行才扫描" | `reprice_keeps_not_applicable_and_real_cost_statuses`、`cost_status_counts_are_not_all_assumed_unpriced` |
| F08 退出顺序 | 已修复 | `main.rs`：由网络线程在 runtime 销毁**之前** `mark_closing`；`shutdown()` 用 `try_send` + `queue.close()` 双路径，不依赖有界 control 通道 | `closing_turns_a_dropped_stream_into_a_shutdown_interruption`、`a_real_client_cancellation_stays_cancelled`、`closing_does_not_rewrite_a_completed_response` |
| F09 真实零与未知 | 已修复 | `query.rs`：可证明覆盖且无请求的空桶返回 `0`；未覆盖/降级/暂停仍为 `null` | 集成脚本 `empty buckets distinguish a proven zero from unknown`（覆盖空桶=0、暂停空桶=null+warning、有请求无 usage=null） |
| **新发现（本轮修复）** | 已修复 | `store.rs`：`created_at_ms` 用请求自身开始时间。此前用批次写入时刻，当 Start 与 Finish 落入同一批（快速请求）时违反 `updated_at_ms >= created_at_ms`，整批被隔离丢弃并把请求误结算为 `interrupted` | `start_and_finish_in_one_batch_keep_the_timestamp_order` |
| F10 GPUI 任务状态机 | 已修复 | `stats_backend.rs`：`JobKind` + `StatsScheduler`（读意图按 kind 收敛为最新、写意图有界 FIFO 不静默丢弃、`pump` 仅在通道有空位时下送、回复带 `seq`）。`statistics.rs`：tick 中 `pump`+按 kind/seq 丢弃过期回复、完整 `HealthDto` 徽标（state/loss_scope/DB 大小/最早记录/丢弃与写入失败）、离开页面取消 debounce、维护轮询 2 秒且 `is_busy` 去重、供应商模式每次 tick 只补一个 | GPUI 新增 4 项调度测试：`read_intents_keep_only_the_newest_request`、`write_intents_queue_in_order_and_report_a_full_backlog`、`a_full_channel_defers_work_to_the_next_tick`、`busy_tracking_deduplicates_polls` |
| F11 P1 表单与筛选 | 部分完成 | 已改：供应商必选校验、缓存写价格输入（disjoint 可用）、生效时间输入、`parse_local_time`、保存/列表分离（`JobKind::PriceSave` vs `Prices`，保存成功后才关对话框、失败保留输入）、自定义日期范围（`RangePreset::Custom` + 两个本机时间输入 + tick 同步筛选状态）、核心侧"未来变价自动关闭旧区间"原子事务 | 集成检查：`price rules reject overlapping windows and future changes supersede`（两次变价成功、旧规则被截断、同一时刻重复变价仍拒绝） |
| F11 未完成项 | 待办 | 筛选选项的 `next_key` 分页加载与搜索框、语言切换重建已创建 Select 选项/提示/对话框（当前只刷新文案与表格）、模型选项按当前供应商即时收窄 | 明确保留为待办，未以"已实现"记入 |

第三批实测：GPUI `cargo test` **34 passed**（基线 30 + 4）、`clippy --all-targets -- -D warnings` 无警告、
`cargo fmt --check` 通过、release 构建通过。

**界面实证（本轮新增）**：用一个透明 TCP 代理记录设置窗口发往核心的控制命令，
启动后实际观察到 `stats_health`×2、`stats_dashboard`、`stats_requests`、`stats_filters`、`stats_prices_list`
以及设置窗口自身的 `snapshot`×4，证明统计页面确实驱动了核心统计 API（不只是"进程能启动"）。
窗口为 1180×781、`AXMain=true`。屏幕截图仍受权限限制，页面内容的像素级验收保留为人工项。

第一/二批实测：`cargo test` **105 passed**（基线 86 + 19）、连续两次运行稳定、
`clippy --all-targets -- -D warnings` 无警告、`cargo fmt --check` 通过、release 构建通过；
`scripts/statistics_test.py` **34 项通过**（新增小数纳值拒绝、空桶真实零/未知）。

已发现的测试稳定性问题（归入 F04）：集成脚本的分页遍历与异步统计写入竞争，偶发 `seen != total`；
修复方式是在遍历前等待统计行落库，而不是放松断言。

### 7.1 复核修复后的性能回归（`bench-results/statistics-round2-100k.json`）

| 指标 | 10 万行实测 | 目标 |
| --- | --- | --- |
| dashboard | p50 168 ms / p95 201 ms（n=8） | < 500 ms ✅ |
| 请求第一页 | p50 18.1 ms / p95 18.5 ms | < 200 ms ✅ |
| 供应商/模型汇总 | p50 164 ms / p95 171 ms | 2 s 预算内 ✅ |
| 请求详情 | p50 0.27 ms | — |
| 转发 P95（采集关→开） | 0.86 ms → 1.25 ms（Δ +0.39 ms） | ≤ max(5%, 2 ms) ✅ |

## 8. 复核修复后的未验证项（明确保留）

- **性能三类价格场景未实测**：本轮基准只覆盖"全为 unpriced"（快速路径）。仍需补"已有 priced 规则"
  与"全为 unsupported_breakdown"两类各跑一次 10 万/100 万，验证 `has_classified_costs` 走完整扫描时的预算。
- **Rust 1.88 最低版本未验证**：本机为 1.98.1；`rust-version = "1.88"` 只是声明，缺少该工具链的编译证据。
- **Windows 编译/打包/人工验收未验证**。
- **真实供应商 usage 语义与计费准确性未验证**（未用 Mock 结果替代）。
- **界面像素级验收未完成**：截图权限不足；本轮新增的透明代理证据只证明"页面确实调用统计 API"，
  不覆盖三尺寸布局、中英文混排、切页签竞态的人工观感。
- **F11 剩余项**：筛选选项 `next_key` 分页加载与搜索、语言切换重建 Select 选项/提示/对话框、
  模型选项按当前供应商即时收窄。

## 9. 下一步

1. 人工在 Mac 上按设计尺寸截图并检查布局/交互（需要屏幕录制权限或手动截图）。
2. Windows 侧编译与打包验证。
3. 真实供应商 usage 逐家核对（`include_usage` 支持度、缓存字段语义），填验收矩阵。
4. 补性能三类价格场景、Rust 1.88 工具链与 Windows 证据。
5. 完成 F11 剩余交互（筛选分页/搜索、语言重建、模型收窄）并做人工界面验收。

## 10. Codex 接手目标（2026-10-16）

当前目标：收尾并验收统计初版 → 在统计基线上实现 Responses/Anthropic/Gemini 上游适配（WorkBuddy入口保持Chat Completions）→ 整体测试 → 合入本地主分支。不推送、不发布、不安装，dsh停止参与施工。

推进顺序：统计页面价格分页/默认币种、合法日期应用、远程筛选/身份隔离、语言/前后台生命周期、reader维护水位 → 统计完整回归并提交 → 协议配置/迁移、请求/响应/工具闭环/多模态、流式背压/取消/错误和统一usage → 原生GUI协议选择/连接测试 → 综合回归/文档 → 主分支合并并验证。现有用户修改和两份任务文档保留；旧“完成”表述以此后验收记录为准。

### Codex 统计收尾（2026-10-16）

已补价格首100条之后加载/精确模型查询、默认币种入口、日期草稿验证、供应商与模型搜索/next_key、切供应商清理不兼容模型、语言重建、前台自动刷新/模态与离页调度、reader执行期维护水位。修正dsh仍未渲染的价格供应商控件和表单错误，草稿独立generation、Enter防重提、真正成功后关窗、缺价明细直接显示0元。配置供应商独立注入价格表单，不受历史筛选首100条限制。

核心125、GPUI54、统计集成39通过；新增所有列表20/50/100三页双向回放、321项选项/价格完整遍历、默认币种历史隔离通过；smoke和旧核心替换通过。三类100k/1m性能结果、实际原生GUI观察与未验证边界见[接手验收](validation-2026-10-16/README.md)。原有未完成表是历史记录，以本节为当前状态。后续在审定统计基线上进行协议适配，统计初版不包含正式发布或真实供应商账单认证。

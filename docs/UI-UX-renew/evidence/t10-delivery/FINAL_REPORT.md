# WorkBuddy Proxy UI 改造交付报告

**T10 / 最终交付与审计**
**工作区：** `/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign`
**分支 / HEAD：** `codex/ui-redesign` / `30cfd66c457cccfcbed6ff97ee915b2b0bdc4493`
**报告日期：** 2026-10-20
**提交状态：** 本报告不提交、不推送；等待 Codex 独立复核。

## 结论

- **代码实施：完成**（T01—T08 代码已提交；T07 验收 PARTIAL；T09 仅归档受限验收）。
- **Windows 验收：BLOCKED**（本机无 Windows 实机/VM/远程/Wine；无任何 Windows 编译或 GUI 证据）。
- **macOS 验收：PARTIAL**（SC01/SC08/SC09/SC10 有中文证据；SC02—SC07、SC11、SC12（8 个）未跑；英文、Retina、DPI、IME、Tab 未测；X04 生命周期 PASS，X03/X05/X06 PARTIAL）。
- **完整设计验收：不通过 / 待验收**。本报告不宣称 full acceptance，也不使用“测试都通过”。
- **仍阻止完整验收的问题：** Windows 全矩阵与 DPI 缺失；macOS 英文/Retina 缺失；8 个画面未跑；IME/Tab 未验证；T08 R06 失败路径未测；T07 的 L03 每页重算、L04 分组分页/排序、L05 完整逐值对照未闭合；D04 标题栏与 D16 控件高度待用户确认；F02/F03 复核为 PARTIAL（差异表缺独立影响列、T04—T06 证据因 1TB 缺失不可核对）。

## 实施范围

### 基线与最终版本

| 项 | 值 |
|---|---|
| T00 基线 | `cbb3a9191a4284765c324a40075e2e727062b3c0`（`cbb3a91`，T00 静态基线） |
| 最终版本 | `30cfd66c457cccfcbed6ff97ee915b2b0bdc4493`（`codex/ui-redesign`） |
| 提交区间 | 17 个提交：`aae634e` → `30cfd66` |
| 未提交修改标识 | `IMPLEMENTATION_STATUS.md`（` M`，既有未提交状态文档）；未跟踪用户设计资料与规划文档（见 §安全边界）；T10 证据目录未跟踪 |
| 产品源码未提交 diff | **无**（`git diff HEAD -- gpui/src src` 为空；本轮只创建 T10 证据） |

### 新增 / 修改文件（代码）

`git diff --stat cbb3a91..HEAD -- gpui/src`：**12 个文件，+3,890 / −1,544**；根 `src/` **0 文件**；`Cargo.toml`/`gpui/Cargo.toml`/`gpui/Cargo.lock` **无 diff**。

| 文件 | 变更 | 作用 |
|---|---|---|
| `gpui/src/theme.rs` | +406 | 新增集中视觉 token（颜色/字体/字号/间距/圆角/阴影/表格参数） |
| `gpui/src/ui_components.rs` | +335 | 新增 Button/Input/Select/Checkbox/Segmented/Card/Table/Dialog/Pager/Tag 薄封装 |
| `gpui/src/ui.rs` | 大改（见 diff stat） | 重建窗口骨架（224px 侧栏、36px 顶部六动作、30px 状态栏）、供应商页/右栏/确认框 |
| `gpui/src/statistics.rs` | 大改（见 diff stat） | 统计总览、筛选、日志、定价/采集存储、价格弹窗、详情 |
| `gpui/src/statistics/overview.rs` | +160 | KPI hero + 卡片布局 |
| `gpui/src/usage_chart.rs` | +9 | 图表拆分与系列样式 |
| `gpui/src/usage_table.rs` | +457 | 请求日志 11 列、两行 Token、来源/点数合并、列宽/横向滚动 |
| `gpui/src/model_options.rs` | +217 | 选择/编辑模型弹窗与能力字段 |
| `gpui/src/model_table.rs` | +26 | 模型表动作与行高 |
| `gpui/src/protocol_options.rs` | +137 | 四种协议弹窗与 Responses `none` |
| `gpui/src/main.rs` | +30 | 主题/标题栏接入，窗口默认/最小尺寸未改 |
| `gpui/src/assets.rs` | +17 | 状态栏 info 图标 |

新增/复用 token 与组件：`theme.rs` 的语义色/字体/尺寸 token；`ui_components.rs` 的 10 类封装；沿用既有 `gpui-kit`/`gpui-component` 0.7.3，无依赖版本变更。

### 保护的既有修改

- `gpui/src/usage_table.rs` 在进入 worktree 前已有未提交修改（+22/−5，Token 槽位/最小宽度）。该修改在 T07 提交 `f5e69e7` 中**保留并纳入**，未被重置/覆盖；当前无该文件的未提交 diff。
- `docs/UI-UX-renew/IMPLEMENTATION_STATUS.md` 的既有未提交修改**未纳入任何提交**，本任务只读。
- 用户未跟踪设计资料（12 PNG、PDF、旧截图）**未删除/未改写**；规划文档（ACCEPTANCE/TASK_BREAKDOWN/DESIGN_SPEC/AGENT_TASK/IMPLEMENTATION_PROMPT）保持未跟踪。

## 画面与功能

### SC01—SC12 实现位置与证据

状态口径：**NEW**=本轮/提交内新增证据；**REUSE**=既有证据（同 HEAD 或注明阶段）；**NOT RUN**=本机条件不足未执行；**BLOCKED**=Windows 缺失。证据链接均为当前工作区文件。

| 画面 | 代码位置 | macOS 证据 | 状态边界 |
|---|---|---|---|
| SC01 数据统计 | `statistics.rs`（页头/筛选/详情）、`statistics/overview.rs`（KPI）、`usage_chart.rs`（趋势/费用）、`usage_table.rs`（日志） | **NEW（T09）** [1440×900](../t09-cross-platform/shots/mac-1440x900-sc01.png)、[1280×800](../t09-cross-platform/shots/mac-1280x800-sc01.png)、[1100×700](../t09-cross-platform/shots/mac-1100x700-sc01.png)；**REUSE** T07 [L01](../t07-l01-three-sizes-001/REVIEW.md)、[L03](../t07-l03-pagesize-001/REPORT.md)、[L04](../t07-l04-group-paging-sort-001/REPORT.md)、[L05](../t07-l05-full-values-001/REPORT.md) | macOS 中文部分；Windows **BLOCKED**；英文/Retina **NOT RUN** |
| SC02 供应商管理 | `ui.rs`（供应商页/右栏/全局设置/日志）、`model_table.rs` | 无当前工作区截图 | **NOT RUN**（T04 仅有文字记录；T04 证据在原 1TB 路径，未挂载） |
| SC03 选择模型弹窗 | `model_options.rs`（picker）、`ui.rs` | 无 | **NOT RUN** |
| SC04 上游协议下拉 | `protocol_options.rs`、`ui.rs` | 无 | **NOT RUN** |
| SC05 协议 OpenAI / Responses none | `protocol_options.rs`（`EFFORTS`/`responses_efforts`） | 无 | **NOT RUN** |
| SC06 协议 Anthropic | `protocol_options.rs` | 无 | **NOT RUN** |
| SC07 协议 Gemini | `protocol_options.rs` | 无 | **NOT RUN** |
| SC08 成本定价/采集存储展开 | `statistics.rs::pricing_section/storage_section` | **REUSE（T08）** [两列展开](../t08-residual-003/shots/1440-storage-settings.png)、[定价展开](../t08-residual-003/shots/1440-pricing-open.png)、[1100 堆叠](../t08-residual-003/shots/1100-settings-narrow.png) | macOS 中文；Windows **BLOCKED**；英文 **NOT RUN** |
| SC09 新建价格 | `statistics.rs::open_price_dialog/price_body` | **REUSE（T08）** [空白](../t08-residual-003/shots/1440-r03-pricenew-empty.png)、[四价填写](../t08-residual-003/shots/1100-r03-input-filled-004.png)、[USD 前缀](../t08-residual-003/shots/1100-r03-usd-prefix-004.png) | macOS 中文 |
| SC10 编辑价格 | `statistics.rs::price_body`（身份锁定） | **REUSE（T08）** [锁定态](../t08-residual-003/shots/1440-r04-priceedit.png)、[保存后](../t08-residual-003/shots/1100-r04-saved-004.png)、[CNY 前缀](../t08-residual-003/shots/1100-r04-cny-prefix-004.png) | macOS 中文 |
| SC11 编辑模型 | `model_options.rs`（编辑弹窗/能力联动）、`ui.rs` | 无 | **NOT RUN** |
| SC12 同步 WorkBuddy 确认 | `ui.rs`（sync/unsync/port/startup 确认框） | 无当前工作区证据（T05 的 `T05-15` 在未挂载的 1TB，链接不可用） | **NOT RUN**；验收 D05 **PARTIAL**（仅取消路径有文字记录） |

### 原功能映射检查结果

- 功能映射表（旧入口/回调/新入口/验收 ID）见 [IMPLEMENTATION_STATUS.md §4](../../IMPLEMENTATION_STATUS.md)（只读；文档本身为受保护未提交修改）。
- 检查结果：T01—T08 未删除或改写入口 ID、回调、busy/disabled、草稿/持久化边界；根 `src/` 零改动，配置格式/协议/统计语义未改。
- 关键保留：顶部六个真实动作（D01）、Responses `none`（D03）、同步/取消同步/端口/启动确认（D02/D06）、价格快照与危险操作（T08）、日志列/分页/详情语义（T07）。
- 未覆盖页面与状态：供应商/模型聚合表、请求详情（有 T09 新证）、日期面板、导入面板、空/错/加载、深色主题、Windows 标题栏（D07）；保留现有行为并登记 TODO(design)。

## 构建与测试

### 本轮（T10）

| 检查 | 平台 | 版本 | 结果 | 证据 |
|---|---|---|---|---|
| `git diff --check` | macOS | HEAD 30cfd66 | **PASS（exit 0）** | [verification.md](../t10-delivery/logs/verification.md) |
| `cargo fmt / clippy / test / build` | — | — | **NOT RUN（本轮）** | 按用户要求不跑构建/测试；`/Volumes/1TB` 未挂载，AGENTS.md 禁止系统盘 TMP |
| 主矩阵 / DPI / 输入法 / 生命周期 | macOS | HEAD 30cfd66 | **PARTIAL / BLOCKED / NOT RUN** | [T09 MATRIX](../t09-cross-platform/MATRIX.md) |

### 历史记录（**非本轮重跑**）

| 检查 | 平台/版本 | 结果 | 证据 |
|---|---|---|---|
| `cargo fmt --check`（T08 修复轮） | macOS / HEAD 内容 | PASS（exit 0） | [fmt-004.log](../t08-residual-003/logs/fmt-004.log) |
| `cargo check`（T08 修复轮） | macOS | PASS（exit 0） | [check-004.log](../t08-residual-003/logs/check-004.log) |
| `cargo build --release`（T08 修复轮） | macOS / GPUI 0.5.6 | PASS | [build-gpui-004.log](../t08-residual-003/logs/build-gpui-004.log) |
| `cargo test`（T08 修复轮） | macOS | 138 passed | [test.log](../t08-residual-003/logs/test.log) |
| `cargo clippy --all-targets -- -D warnings`（T08 修复轮） | macOS | PASS（exit 0） | [clippy.log](../t08-residual-003/logs/clippy.log)、[clippy.exit](../t08-residual-003/logs/clippy.exit) |
| T07 构建（前后版本） | macOS | exit 0 | [build-pre-t07.log](../t07-l05-full-values-001/logs/build-pre-t07.log)、[build-current-restore.log](../t07-l05-full-values-001/logs/build-current-restore.log) |
| T09 构建/测试 | — | **NOT RUN**（1TB 未挂载） | [T09 REPORT](../t09-cross-platform/REPORT.md) |

> 上述 T07/T08 结果来自历史提交前的验证日志，**不能冒充本轮构建**；本轮唯一执行的验证是只读审计命令（`git diff --check`、状态、哈希、环境、trailer、1TB 检查）。

## 差异与未完成事项

| ID | 文件/函数 | 设计要求 | 当前行为 | 原因 | 处理结论/待定 |
|---|---|---|---|---|---|
| 差异 D01 | `ui.rs::app_controls` | 顶部电源/刷新/全屏/账号 | 六个真实动作：开机启动/检查更新/语言/恢复窗口/GitHub/反馈 | 图标与动作非字面对应 | 保留真实动作，tooltip/aria 已改；**待用户确认视觉映射** |
| 差异 D02 | `workbuddy_restart.rs`、`ui.rs` | Windows/macOS 均可“同步并重启” | 自动重启仅 macOS；非 macOS 返回错误，按钮禁用+说明 | 平台能力限制 | **Windows 保持禁用说明**；未在 Windows 实机验证 |
| 差异 D03 | `protocol_options.rs` | 六项 effort 映射 | Responses 额外 `none`，共 7 项 | 保留既有映射 | 保留；验收 D01 已测 |
| 差异 D04 | `main.rs`、`TitleBar::window_options()` | 标题栏必须原生 | macOS 透明标题栏+原生红绿灯；Windows 由 Kit `ControlIcon` 绘制 | 沿用 Kit 标题栏 | **未关闭**；D04 仅登记，需用户接受或确认原生改造范围 |
| 差异 D05 | `theme.rs` 字体解析 | 两端字形完全一致 | 使用平台系统字体；macOS 系统 sans + SF Mono/Menlo/Monaco，Windows 候选 Segoe UI Variable/Segoe UI/YaHei UI | 平台字体策略 | 允许字形差异；**GUI 字形未逐字核对** |
| 差异 D06 | 窗口尺寸 | 1280×800 基准 | 默认 1440×900、最小 1100×700；设计 PNG 1280×800 | 既有窗口策略 | 长页滚动，不整页缩小；T09 已三档截图 |
| 差异 D07 | 全局 | 所有界面有设计 | 供应商/模型聚合表、详情、日期、导入、空/错/加载、深色、Windows 标题栏等无设计 | 设计材料缺项 | 保留现状 + TODO(design)；**未关闭** |
| 差异 D08 | 文案 | 完全照稿 | 动态值绑定真实状态；部分旧文案保留 | 数据动态 | 仅静态文案照稿；未定价/未知保留 |
| 差异 D09 | `statistics` 来源筛选 | “全部来源/官方/供应商” | “全部来源/内置模型/自定义模型” | 既有 `SourceScope` 语义 | 保留并登记；**待用户确认命名** |
| 差异 D10 | `overview.rs` | 总 Token + 8 卡 | 总 Token + 9 卡（含 WorkBuddy 点数） | 保留点数功能 | 保留；A03 已按 9 项核对 |
| 差异 D11 | `usage_table.rs` | 9 列 | 11 列（来源、点数） | 保留既有数据 | 保留；L01/L02/L05 已覆盖部分 |
| 差异 D12 | `usage_chart.rs` | 趋势/费用两张并排卡 | T06 已拆两张并排，窄窗纵向堆叠 | 已落地 | A04 PARTIAL（币种/图例交互未测） |
| 差异 D13 | `ui.rs` 状态栏 | 30px 全宽状态栏 | 已落地，状态由真实 `CoreState` 推导 | 已落地 | 三态反证有证据；代理/上游健康未纳入 |
| 差异 D14 | `ui.rs` 右栏 | 全局设置/日志两卡 | 已拆两卡，300px，独立滚动 | 已落地 | 右侧日志滚动 **NOT RUN** |
| 差异 D15 | `statistics.rs` 刷新间隔 | 设计 5 秒 | 默认 0（关闭），可选 0/5/10/30/60 | 行为默认值 | 保留现状；**待用户确认是否改默认** |
| 差异 D16 | Kit `Input` 高度 | 输入 36 / 按钮 32 / 映射 34 | 按钮 32；单行 Input/Select 26–28 | Kit rem/内边距 | **未解决**；影响全部页面，需共享层方案/用户确认 |
| T08 R06 | `statistics.rs` 清除历史/删除价格 | 预览/取消/确认/失败明确 | 预览/确认/维护完成 PASS；失败（token 过期/维护错误） | 未模拟 | **失败路径 NOT RUN** |
| T07 L03 | `statistics.rs` 分页 | 每页数量改变后总页数/当前页重算 | 按钮响应，重算 PARTIAL | 只验证了三页边界/详情 | 剩余项待补 |
| T07 L04 | `statistics.rs::details` | 分组分页/排序 | 供应商/模型标签 PASS；分组分页 NOT VERIFIED；`sort-tokens`/`sort-requests` 排序交互 NOT RUN | 夹具分组不足 | **未闭合**；代码会更新 `group_sort`，不能称“无排序” |
| T07 L05 | `usage_table`/金额格式 | 前后逐值对照 | 13 个 formatter/算法字节级一致；抽样 PASS；完整逐值 NOT RUN | 证据/时间限制 | **PARTIAL** |
| T05 D05 | `ui.rs` 同步确认 | 取消/仅同步/同步并重启 | 取消路径 PASS；仅同步/重启未执行 | 会写入并重启 WorkBuddy | **PARTIAL**；Windows 禁用说明静态保持 |
| T06 A02/A04/A05 | `statistics.rs` 刷新/日期/图表/告警 | 全状态交互 | A02/A04/A05 PARTIAL；币种/图例、已暂停等未测 | 自动化限制 | **未闭合** |

## TODO(design)

| 文件/函数 | 问题 | 当前保留方式 | 影响 | 后续所需信息 |
|---|---|---|---|---|
| `ui_components.rs` 选择模型 | “仅看已勾选”激活态用 Outline，已添加行底色用 `BG_TABLE_HEAD`（设计略偏青 #F3F8F7） | 保留现有反馈 | 视觉细节差异 | 设计色值确认 |
| `ui.rs` 侧栏 | 设计标题行有前置图标，当前只有 chevron | 保留真实折叠语义 | 视觉差异 | 图标资源 |
| `ui.rs` 顶部 | 六个动作图标与设计字面不对应（D01） | tooltip/aria 反映真实动作 | 认知差异 | 用户确认映射 |
| `theme.rs`/Kit | 单行 Input 高度 26–28 vs 设计 36（D16） | 保留 Kit 行为 | 全局控件高度 | 共享层方案/用户取舍 |
| `main.rs`/TitleBar | Windows 标题栏由 Kit 绘制（D04） | 只登记 | 平台一致性 | 用户接受或原生改造范围 |
| 全局 | 设计未覆盖的聚合表/详情/日期/导入/空错加载/深色（D07） | 保留现有行为 | 未逐像素对照 | 补充设计材料 |


## F01—F04 验收状态（ACCEPTANCE §7.11）

| ID | 状态 | 依据（不推断） |
|---|---|---|
| F01 | **PARTIAL** | `git diff --stat cbb3a91..HEAD -- src Cargo.toml gpui/Cargo.toml gpui/Cargo.lock` 为 0；`gpui/src` 12 文件；`git diff --check` exit 0；`IMPLEMENTATION_STATUS.md`（` M`）与设计资料未纳入提交。**但** `usage_table.rs` 的既有未提交修改被纳入 `f5e69e7`，该提交信息未区分原作者；“未被回滚”有证据，“未被冒领”无法仅凭当前记录确认，需复核裁定。 |
| F02 | **PARTIAL** | 差异表 D01—D16 含文件/函数、设计要求、当前行为、原因、待定结论，但**无独立“影响”列**；`grep -rn "TODO(design)" gpui/src` 返回 **0**，报告 TODO 表来自 `IMPLEMENTATION_STATUS.md` 文档登记而非代码字面标记，无法逐项与代码对应。 |
| F03 | **PARTIAL** | 报告 46 个相对链接全部存在（脚本校验）；T08 `fmt.exit=0`、`clippy.exit=0`、`test.exit=0`；T09 截图物理尺寸/哈希见 `shot-sizes.txt`；T08/T07 日志与标注版本对应。**但** T04/T05/T06 证据目录不在当前工作区（仅 T07—T10），`/Volumes/1TB` 未挂载，无法核对这三阶段实机证据的版本匹配；报告未提供其失效链接，故不是“链接失效”，而是**证据完整性部分未过**。 |
| F04 | **PASS（报告层面）** | 本报告已补 SC01—SC12 表、验收 ID 状态汇总、未测单列与 BLOCKED 明示；未把代码完成等同于跨平台完成。**注意：F04 PASS 仅表示交付报告覆盖完整，整体验收仍为不通过/待验收，Windows 仍 BLOCKED，macOS 仍 PARTIAL。** |

## 验收 ID 状态汇总（F04 对照）

来源：`IMPLEMENTATION_STATUS.md` §7（T01—T06 状态）、T07 报告、T08 报告、T09 报告；未注明的交互按 NOT RUN/PARTIAL 处理，不推断为通过。

| ID | 状态 | 依据 |
|---|---|---|
| B01 | PASS | 基线/既有 diff/Kit 版本已记录，`git diff --check` 0 |
| B02 | PASS（静态） | 功能映射覆盖入口/回调；无 GUI 点击 |
| B03 | PASS | 12 PNG + 12 页 PDF；无 HTML/Windows 基线，已登记 |
| B04 | PARTIAL | T00 标 NOT RUN；T07/T08/T09 已实际启动隔离 core/mock 并清理，但非 T00 原始验收 |
| V01 | PASS（静态） | 色值集中 token；扫描已迁移 UI |
| V02 | PASS（静态） | 尺寸/字号/圆角等集中；D16 高度未解决 |
| V03 | NOT RUN | 两平台字体/长文本 GUI 字形未测（D05） |
| V04 | NOT RUN | 重开/语言切换后的主题一致性未测 |
| C01 | NOT RUN | 按钮 busy/disabled 真实点击未测（静态 PASS） |
| C02 | PARTIAL | T09 剪贴板中文粘贴 PASS；组合输入/IME 候选/选区未测 |
| C03 | NOT RUN | 下拉鼠标/方向键/Enter/Escape/边缘未测 |
| C04 | NOT RUN | Checkbox/Segmented 真实交互未测 |
| C05 | PARTIAL | T09 详情 Escape PASS；长 Dialog Tab/滚动/Enter 校验未测 |
| C06 | NOT RUN | 图标 tooltip/可访问名称悬停未测（静态 0 缺失） |
| S01 | PARTIAL | 长列表/切换/固定工具条 PASS；busy 切换、添加/复制/删除未点击 |
| S02 | PARTIAL | 状态栏固定/统计滚动 PASS；右侧日志滚动 NOT RUN |
| S03 | PARTIAL | 最小窗口/恢复尺寸 PASS；中英文切换 GUI NOT RUN |
| S04 | PARTIAL | 六动作/状态源静态 PASS；busy 禁用、代理状态切换、部分动作未执行 |
| P01 | PARTIAL | 字段/草稿/长 URL PASS；保存写入与 IME NOT RUN |
| P02 | NOT RUN | 获取模型/测试连接实际调用（网络副作用）未执行 |
| P03 | PARTIAL | 端口确认/取消 PASS；复制/清空日志未点击 |
| P04 | NOT RUN | Shift/⌘ 连选、Delete/Backspace、⌘A、双击未执行 |
| P05 | PARTIAL | F2 打开/取消/确认 PASS；双击/Enter 未执行 |
| P06 | PARTIAL | 自动识别/手动/非法值 PASS；原生限制部分已核对 |
| P07 | PARTIAL | 切页/切供应商/删除取消 PASS；复制/添加/空列表未执行 |
| D01 | PASS | 四种协议、Responses `none` 已核对 |
| D02 | PASS | 空映射/非法预算不关窗、草稿保留 |
| D03 | PASS | 应用/取消边界与配置 SHA 不变 |
| D04 | PASS | 模型选择计数/全选/反选/清空/确认/取消 |
| D05 | PARTIAL | 同步确认取消 PASS；仅同步/同步并重启未执行；Windows 静态禁用 |
| D06 | PASS | 取消同步/删除/端口/启动确认保留，取消无写入 |
| A01 | PASS | 三来源 KPI 前后逐值一致 |
| A02 | PARTIAL | 刷新/日期结构静态；快速连击/日期草稿未实测 |
| A03 | PASS | KPI 逐字节一致；未知/零/未定价/不全可区分 |
| A04 | PARTIAL | 图表拆分/断线/图例静态；币种/图例交互未测 |
| A05 | PARTIAL | 错误态/告警真实；已暂停/降级未构造 |
| L01 | PASS | T07-L01 REVIEW：三尺寸/分页/长值/56px 行高通过 |
| L02 | PASS | 240px Token 下限、横向两端、表头同步、恢复（口径警示见 T07-L02） |
| L03 | PARTIAL | 分页边界/详情 PASS；每页数量重算 PARTIAL |
| L04 | PARTIAL | 标签/汇总部分 PASS；分组分页/排序未验证 |
| L05 | PARTIAL | formatter 字节级一致/抽样 PASS；完整逐值未跑 |
| R01 | PASS | 折叠/搜索/刷新/分页 + 1100 工具栏可达 |
| R02 | PARTIAL | 更新中/成功 PASS；失败仅健康行提示，详情未捕获 |
| R03 | PASS | 非法值不关窗、取消无落库、合法保存 output 非零 |
| R04 | PASS | 身份锁定/币种/模式/保存/历史快照不变 |
| R05 | PASS | 币种/采集/保留/供应商模式 API 与 UI 一致 |
| R06 | PARTIAL | 预览/确认/维护完成 PASS；失败（token 过期/维护错误）NOT RUN |
| X01 | BLOCKED | Windows 0 画面；macOS 4 画面部分；英文/Retina 0 |
| X02 | BLOCKED / NOT RUN | 无 Windows DPI；macOS 仅 100% 非 Retina |
| X03 | PARTIAL | Escape/剪贴板中文 PASS；IME/Tab 未验证 |
| X04 | PASS（macOS） | 最小化/恢复、关闭重开、多实例、core 存活 |
| X05 | PARTIAL | 5 次详情/tab/滚动稳定；无性能基线 |
| X06 | PARTIAL（静态） | 自动重启仅 macOS/非 macOS 禁用说明；D04 标题栏未实测 |
| F01 | PARTIAL | 见上：diff/依赖/核心目录通过；usage_table 归属需裁定 |
| F02 | PARTIAL | 差异表缺独立影响列；代码 TODO(design) 0 |
| F03 | PARTIAL | 链接/退出码/尺寸通过；T04—T06 证据因 1TB 缺失不可核对 |
| F04 | PASS（报告层面） | 本报告补全 SC/验收 ID/未测/BLOCKED；整体验收仍不通过 |

## 验证边界

- **Mock / 合成数据：** T07/T08 使用隔离 config/DB/本地 catalog mock；T09 使用 L05 副本；均未读写真实配置/Key/历史库，未发真实上游请求。T07/T08 的夹具与清理日志在各证据目录。
- **自动化 / 人工：** `axdrive`（AX dump/press/shot/resize）、CGEvent 点击/滚动/键盘、Vision OCR；人工对照设计/截图并复核 API/DB。自动化不能证明视觉完全一致，OCR 有误读。
- **未执行及原因：** Windows 全矩阵/DPI（无环境）；macOS 英文/Retina（夹具 zh、显示器 100% 非 Retina）；SC02—SC07/SC11/SC12（无安全夹具/1TB 未挂载）；IME 候选框、Tab 焦点；R06 失败、T05 仅同步/重启、T06 币种/图例；T07 L03/L04/L05 剩余项；本轮构建/测试（用户要求 + 1TB 未挂载）。
- **标题栏结论：** macOS 原生红绿灯 + 透明标题栏；Windows Kit 绘制（D04 未关闭）；未在 Windows 实测。
- **字体结论：** 平台系统字体/回退候选已静态核对；D05 的 GUI 字形与等宽数字逐字核对未完成。
- **下一步准确起点：** 在具备 **Windows x64 实机/VM** 且 **`/Volumes/1TB` 挂载** 的环境，按 T09 矩阵补跑 Windows 100/125/150/200% DPI、SC01—SC12、中英文；补齐受限夹具（供应商/模型/协议/同步确认）与 IME/Tab/R06 失败/T07 剩余项；完成后交独立复核。**在此之前不得宣布完整设计验收。**

## 提交与 trailer 审计

- 近 3 个本地提交：
  - `30cfd66` test(ui): record T09 macOS subset and Windows block — **恰 1 条** `Co-authored-by: Codex <codex@openai.com>`，位于末尾。
  - `6533bf1` feat(ui): complete T08 pricing and storage redesign — **恰 1 条**，位于末尾。
  - `7da2685` docs: add T07-L02 residual GUI evidence — **恰 1 条**，位于末尾。
- 区间 `cbb3a91..HEAD` 另发现 **3 个早期提交缺少该 trailer**：`36c5853`（statistics 总览）、`bf811b9`（模型选择）、`2c91060`（供应商/协议）。**不重写历史**；记录为规范缺口，后续提交按 AGENTS.md 补齐。
- 保护内容未纳入提交：`IMPLEMENTATION_STATUS.md`（` M`）、未跟踪设计 PNG/PDF、ACCEPTANCE/AGENT_TASK/DESIGN_SPEC/IMPLEMENTATION_PROMPT/TASK_BREAKDOWN、T10 证据目录。近 3 个提交的文件列表仅含 `docs/UI-UX-renew/evidence/...`，不含上述受保护文件。

## 安全边界与清理

- 本轮未创建 TMP/运行目录、未启动进程、未构建/测试；无真实配置/数据库/Key 读写；无网络请求。
- 系统：macOS 27.0.1（26A434），arm64；rustc/cargo 1.99.1；CLT 27.0.0.0.1788430756（`/Library/Developer/CommandLineTools`）；git 2.55.0；`/Volumes/1TB` **未挂载**；系统盘可用 **195 GiB**（只读记录）。
- 清理：本轮无临时目录可删；T09 runtime 已在 T09 清理；`ps` 无任务进程；`/tmp` 无本项目遗留（T09 日志）。
- 产物：仅 `docs/UI-UX-renew/evidence/t10-delivery/`（本报告 + 验证日志），未跟踪、未提交。

## 证据索引

- 本报告：[FINAL_REPORT.md](FINAL_REPORT.md)
- 本轮只读验证：[logs/verification.md](logs/verification.md)
- 跨平台矩阵（T09）：[MATRIX.md](../t09-cross-platform/MATRIX.md)、[REPORT.md](../t09-cross-platform/REPORT.md)、[CODEX_REVIEW.md](../t09-cross-platform/CODEX_REVIEW.md)
- T08 交付与审核：[REPORT.md](../t08-residual-003/REPORT.md)、[CODEX_REVIEW.md](../t08-residual-003/CODEX_REVIEW.md)、[api-verification-004.json](../t08-residual-003/api-verification-004.json)
- T07 证据：[L01](../t07-l01-three-sizes-001/REVIEW.md)、[L02](../t07-l02-residual-002/REPORT.md)、[L03](../t07-l03-pagesize-001/REPORT.md)、[L04](../t07-l04-group-paging-sort-001/REPORT.md)、[L05](../t07-l05-full-values-001/REPORT.md)
- 状态/映射：[IMPLEMENTATION_STATUS.md](../../IMPLEMENTATION_STATUS.md)（只读）、[ACCEPTANCE.md](../../ACCEPTANCE.md)、[TASK_BREAKDOWN.md](../../TASK_BREAKDOWN.md)
- 构建/测试日志：[T08 fmt/check/build/test/clippy](../t08-residual-003/logs)、[T07 build](../t07-l05-full-values-001/logs)

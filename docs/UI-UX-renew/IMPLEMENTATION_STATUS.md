# WorkBuddy Proxy UI 改造施工状态

- 文档用途：T00 基线核查、功能映射、差异登记、后续任务起点。
- 编制日期：2026-10-19。
- 工作区：`/Volumes/1TB/codex/worktrees/ui-redesign/workbuddy-proxy`。
- 当前分支：`codex/ui-redesign`。
- T00 基线 HEAD：`cbb3a9191a4284765c324a40075e2e727062b3c0`（`cbb3a91`）。
- 当前版本：`0.5.6`（根 `Cargo.toml`、`gpui/Cargo.toml`）。
- T00 阶段性质：只做基线核查与施工记录；未修改应用代码；未提交、未推送；未启动真实 WorkBuddy；未读写真实配置、数据库或开机启动；未向真实上游发请求。
- 当前进度：T00 已记录；T01 基线提交为 `aae634e`；T02 代码、静态核对和自动化验证已完成（C01—C06 GUI/IME 交互仍待验收）。

## 0. 结论摘要

1. **T00 静态基线完成**：分支、HEAD、工作区差异、Kit 版本、入口/宿主、主题/字体、窗口尺寸、主要 UI 动作的真实回调链均已核对。
2. **已有修改保护**：`gpui/src/usage_table.rs` 是进入本 worktree 前已有的未提交修改（`22 insertions / 5 deletions`），本轮未触碰、未格式化、未重置；当前 SHA-256 为 `ff14cd5c47ff9cc450b4b5ecfcfa6981a02797c2370eb3eab91a786377a275f3`。
3. **设计资料缺项已确认**：12 张 PNG 和 1 个 12 页 PDF 存在；**没有 HTML 画板，没有 Windows 旧界面截图**。PDF 是画板的局部裁切/放大页，不是额外画板。现有旧截图是 macOS v0.5.5。
4. **D01—D09 已逐项登记**；D10—D15 是 T00 额外发现的语义/结构差异，保留以便后续跟踪。
5. **T00 未执行构建/测试；T01/T02/T03 已执行 gpui fmt/clippy/test/release build（§8.3）。T03 首次启动了真实 GPUI 窗口**（隔离夹具、合成 30 供应商配置），取得骨架与状态源截图；B04 仍为 `NOT RUN`（未跑完整隔离代理/Mock 链路），Windows 为 `BLOCKED`（无 Windows 环境）。不得把静态核查当作 GUI 通过，也不得据本轮 macOS 证据宣称 Windows GUI 已验收。
6. **任务状态**：T00 DONE（静态）；T01 DONE（代码/静态，V03/V04 NOT RUN）；T02 DONE（代码/静态，C01—C06 交互 NOT RUN）；T03 DONE（代码/静态 + macOS GUI 骨架与交互证据，S01—S04 部分项 NOT RUN，见 §8.5）；T04—T10 仍为 `TODO`。
7. **字体探测是部分失败/非结论**：CoreText 查询未命中的名称不能当作“系统未安装”或无可用字体的结论；T01 必须用 GPUI 运行时字体表与真实布局复核。
8. **T01 已落盘**：仅新增 `gpui/src/theme.rs` 并接入 `gpui/src/main.rs`；页面、业务逻辑与 `usage_table.rs` 未改；详见 §8.3。
9. **T02 已落盘**：新增 `gpui/src/ui_components.rs`，并在 `ui.rs`/`statistics.rs` 的真实调用点接入 Button/Input/Select/Checkbox/Segmented/Card/Table/Dialog/Pager/Tag；未改业务、配置、统计、协议或依赖，`usage_table.rs` 哈希未变；详见 §8.3。
10. **T03 已落盘**：`gpui/src/ui.rs` 重建窗口骨架（224px 固定侧栏、36px 顶部六动作行、30px 可信状态栏），`gpui/src/assets.rs` 增加状态栏 info 图标；未改 `main.rs`/主题/依赖/协议/代理/统计语义，`usage_table.rs` 哈希仍为 `ff14cd5c…`；D13 已用真实核心状态落地，D04 只登记；详见 §8.3—§8.5。

本轮要求项索引：功能映射 §4；D01—D09 §3.3—§3.4；B01—B04 §8.2；T01—T10 §8.3；T03 验收结果 §8.5。

## 1. 基线与工作区保护

### 1.1 Git 基线

| 项目 | 值 |
|---|---|
| worktree | `/Volumes/1TB/codex/worktrees/ui-redesign/workbuddy-proxy` |
| branch | `codex/ui-redesign` |
| HEAD | `cbb3a9191a4284765c324a40075e2e727062b3c0` |
| 上游 main 当前 HEAD | 同为 `cbb3a919...`（`git worktree list` 显示 main 在 `/Volumes/1TB/Downloads/git/workbuddy-proxy`） |
| remote | `origin https://github.com/gloryhui/workbuddy-proxy.git` |
| 本阶段提交/推送 | 未提交、未推送 |

基线 `git status --porcelain=v1`：

```text
 M gpui/src/usage_table.rs
?? docs/UI-UX-renew/
```

说明：

- `gpui/src/usage_table.rs` 的修改属于既有未提交工作，不是本轮 T00 的成果。
- `docs/UI-UX-renew/` 在 T00 前已是未跟踪目录，包含本次输入的设计资料；T00 在其中新增了本文件，目录整体仍显示为未跟踪。
- 除以上两项外，没有其他已跟踪文件修改。`git diff --check` 退出码为 `0`。
- 其他 worktree（main、protocol-compat、workbuddy-usage-import、usage-statistics）不参与本轮修改。

### 1.2 既有未提交差异（必须保留）

`git diff --stat`：

```text
 gpui/src/usage_table.rs | 27 ++++++++++++++++++++++-----
 1 file changed, 22 insertions(+), 5 deletions(-)
```

`git diff --numstat`：`22	5	gpui/src/usage_table.rs`。

关键内容（完整 diff 见附录 A）：

- `token_cell` 从 `flex_shrink_0` 的固定内容宽度，改为带中文/英文不同权重的等分槽位，并给标签、分隔符、值分别设置 `flex_shrink_0` / `flex_1` / `min_w_0` / `text_ellipsis()`；注释说明“固定比例槽位让标签在行间对齐”。
- `MIN_WIDTHS` 中 Token 列从 `340.` 提高到 `360.`。

保护要求：

- T00—T06 不修改 `gpui/src/usage_table.rs`；T07 必须在该差异之上实现两行 Token/命中率布局和最小列宽，不得 `git checkout`、`git restore`、格式化整文件或用 HEAD 版本覆盖。
- 任何后续 diff 审查都必须把上述 22/5 行识别为他人既有修改；不得计入本次成果。
- 当前文件哈希：`ff14cd5c47ff9cc450b4b5ecfcfa6981a02797c2370eb3eab91a786377a275f3`；HEAD 版本哈希：`03d7c7942a8ced5e9afe35b1a949e89a13f8422105a05c2898cdcbe9c6fe4657`。

### 1.3 GPUI Kit 锁定与入口

| 项目 | 实际值 | 证据 |
|---|---|---|
| Kit 版本 | `gpui-kit = "=0.7.3"` | `gpui/Cargo.toml` |
| 锁文件 | `gpui-kit 0.7.3`，crates.io registry，校验和 `0fac401f...` | `gpui/Cargo.lock` |
| 依赖策略 | 不升级 Kit、不新增依赖 | T00 只核对 |
| 入口（核心/托盘） | `src/main.rs` 解析 `--headless`、`--autostart`、`--port`、`--config-dir`、`--help`、`--version`；非 headless 时进入托盘前端 | `src/main.rs` |
| 入口（设置窗口） | `gpui/src/main.rs` 解析 `--config-dir`、`--version`；要求核心已写 `instance.json`；用 `gpui.lock` 做单实例 | `gpui/src/main.rs:289-327` |
| 窗口宿主 | 托盘进程按平台查找设置 helper：Windows `internal/workbuddy-settings.exe`（兼容 `workbuddy-proxy-gpui.exe`），macOS `../Helpers/WorkBuddy Proxy Settings.app/...`，其他平台同级 `workbuddy-proxy-gpui` | `src/gpui_host.rs` |
| 启动方式 | helper 以 `--config-dir <dir>` 启动；宿主不直接链接 GPUI | `src/gpui_host.rs`、`docs/DESKTOP.md` |
| 配置目录覆盖 | `WORKBUDDY_PROXY_CONFIG_DIR`；默认 macOS `~/Library/Application Support/WorkBuddyProxy`，Windows `%APPDATA%\WorkBuddyProxy` | `src/config.rs:237` |
| 窗口默认尺寸 | `1440 × 900` | `gpui/src/main.rs:46-48` |
| 最小窗口 | `1100 × 700` | `gpui/src/main.rs:373` |
| 标题栏 | `TitleBar::window_options()`：透明标题栏、`app_owns_titlebar_drag: true`、macOS 红绿灯位置 `(9,9)`；Kit 标题栏高 34px | Kit `src/title_bar.rs` |

## 2. 现状：主题、字体、窗口与页面骨架

### 2.1 主题

- 入口在 `gpui/src/main.rs`：`Theme::change(ThemeMode::Light, None, cx)` 后执行 `Theme::update`。
- 当前仍是旧蓝/青主题，不是 DESIGN_SPEC 的新绿主题：
  - `primary = #008daa`（按钮主色仍是 `#008daa` / hover `#007e99` / active `#006e86`）。
  - 页面背景 `BG = #f4f7fb`（`ui.rs`、`statistics.rs` 各自定义）。
  - 文字 `#19334d`、次要文字 `#6b7c90`、边框 `#e2e9f1`、侧栏 `#eaf2f8`。
  - 圆角 `radius = 8`、`radius_lg = 12`。
- 颜色和尺寸仍有大量内联值散落在 `ui.rs`、`statistics.rs`、`statistics/*`、`usage_table.rs`、`usage_chart.rs`、`model_options.rs`；T01 需要集中 token，页面迁移在 T03—T08 逐步进行。
- 仅浅色主题：`ThemeMode::Light` 固定；设计规范也只覆盖浅色，深色未设计。

### 2.2 字体

- 项目没有设置 `Theme.font_family`，使用 Kit 默认 `.SystemUIFont`；默认 `font_size = 16px`。
- 项目大量显式字号：11/12/13/15/18/22/28 等，没有全局字号 token。
- 等宽字体使用 Kit 默认：macOS `Menlo`，Windows `Consolas`；缺失时 Kit 会按候选列表回落。
- 本机只读字体探测（macOS，CoreText 返回 248 个字体族）：这是一次**部分失败/非结论**查询，不能作为字体是否安装的最终结论。
  - 本次命中：`PingFang SC`、`Hiragino Sans GB`、`Menlo`、`Monaco`、`Helvetica Neue`；只能作为 T01 的候选线索。
  - 本次未命中（查询失败/非结论）：`Segoe UI`、`Segoe UI Variable`、`Microsoft YaHei UI`、`Consolas`、`SF Mono`。精确名称未命中可能来自 family 名称差异或查询方式限制，不能据此断定系统没有对应字体，也不能据此宣称 GPUI 运行时不可用。
- 结论：macOS 不能直接假定设计稿的 Segoe/Consolas/SF Mono 一定可用；T01 必须用 GPUI `all_font_names()` 与实际布局再解析，缺字时回落到系统字体，不打包字体。
- GPUI 支持 OpenType feature：`FontFeatures` + `.font_features(...)`；Kit 自带 `tabular_figures()` 使用 `("tnum", 1)`。T01 可复用该机制启用等宽数字，但必须实际验证目标字体是否支持。

### 2.3 窗口与标题栏

- 窗口由 `gpui_kit::open_window` 创建，`WindowOptions` 使用 `TitleBar::window_options()`。
- `app_owns_titlebar_drag: true` 表示应用自己处理拖动/双击；macOS 使用透明标题栏和原生红绿灯，Kit 不绘制 macOS 按钮。
- Windows 上 `WindowControls` 由 Kit 绘制最小化/最大化/关闭按钮（固定 35px，`ControlIcon`），不是完全由系统标题栏绘制；Linux 仅在 client-side decorations 时绘制。
- 设计稿要求“原生标题栏”，当前实现属于 **Kit 自绘 + 平台装饰混合**，D04 未解决；T03 保留现状并登记，不借机改窗口宿主。
- 内容区高度不能被当作 1280×800 或 1440×900：系统装饰和 34px 标题栏会减少实际内容区；T03/T09 必须分别记录窗口尺寸和内容区尺寸。

### 2.4 当前页面骨架

- 主区：`ui.rs::render` 顶部是品牌（36px logo + 22px 标题 + 版本副标题）和 `app_controls` 六按钮；下面左中右三栏；底部只有右下角 16px 的托盘提示。
- 左栏：`Sidebar` 宽 `168 * 1.2 = 201.6px`，圆角 12，包含“数据统计 / 供应商管理”和供应商列表；底部添加/复制/删除是三个 ghost 图标按钮。
- 中栏：供应商页“连接设置”卡；统计页是独立的长内容滚动区。
- 右栏：供应商页单个 `322px` 卡片，标题“全局设置与日志”，同时包含同步、代理、端口、计数和日志；统计页没有右栏。
- 统计页：无固定状态栏；页头 18px；KPI 为 1 个总 Token 区 + 9 张卡；图表在单卡内先 Token 后费用；明细卡为三个普通按钮 + 表格；底部是成本定价、采集与存储两张折叠卡。
- 这些都与 DESIGN_SPEC 的 224px 侧栏、36px 顶部图标行、30px 状态栏、260px+4 列 KPI、两张并排图表卡、300px 右栏、两卡拆分不同；T03/T04/T06/T07/T08 分别处理。

> **T03 更新（本条以上为 T00 记录的改造前状态）**：侧栏、顶部行与底部栏已按 §8.3 重建 —— 品牌移入 224px 固定侧栏，顶部只剩 36px 右对齐六动作行，底部新增 30px 可信状态栏。中栏/右栏内部仍是改造前布局，留给 T04/T06/T07/T08。

## 3. 设计资料核查

### 3.1 实际存在的材料

| 材料 | 是否存在 | 说明 |
|---|---|---|
| 12 张 PNG | 是 | 全部位于 `docs/UI-UX-renew/`；11 张 `2560×1600`，`数据统计@2x.png` 为 `2560×3080`，`成本定价 · 采集与存储（展开）@2x.png` 为 `2560×2020` |
| 视觉重设计 PDF | 是 | `WorkBuddy Proxy 视觉重设计.pdf`，12 页；页面尺寸 960×600，数据统计页 960×1155，成本页 960×757 |
| PDF 的完整画板 | 否 | 12 页是画板的局部裁切/放大页；多页只显示页面中下部、弹窗局部或表单局部，不含额外画面或说明。完整视觉仍以 12 张 PNG 为准 |
| HTML 画板 | **否** | 仓库内未找到 `Main.dc.html`、`Providers.dc.html` 等画板 HTML |
| Windows 旧界面截图 | **否** | 仓库内没有 Windows 截图 |
| macOS 旧界面截图 | 是 | `docs/screenshots/` 5 张 JPG，v0.5.5，1440×900 窗口、Retina 2880×1802（含标题栏） |
| 协议兼容旧截图 | 是 | `docs/protocol-compat/validation-2026-10-16/ui-1100x700.png`、`ui-protocols.png`，1100×701；验收文档明确为 macOS，无 Windows GUI 结果 |
| 当前 v0.5.6 界面截图 | 否 | 现有截图是 v0.5.5，不能当作当前 HEAD 的完整基线 |
| 设计 HTML 数值查询材料 | **否** | 因而不能用 HTML 查具体数值；只能依据 `DESIGN_SPEC.md` 与 PNG |

补充：

- `docs/workbuddy-import/UI-WIREFRAME.svg.png` 是实现线框图，不是重设计画板，也不是 Windows 旧截图。
- 旧截图 README 明确写明：macOS Apple Silicon、v0.5.5、独立临时 HOME、演示 Key、loopback mock，不是 Windows 实机截图。
- 当前 HEAD 已有旧截图之后的功能（例如请求日志来源列、WorkBuddy 点数、导入面板），旧截图不能作为逐像素基线。

### 3.2 设计资料缺项

- 无 HTML 画板，无法核对设计生成器的原始尺寸/内联样式。
- 无 Windows 旧界面截图，无法建立 Windows 旧版功能对照。
- 无当前版本（v0.5.6 / HEAD cbb3a91）截图。
- 设计未覆盖：悬停/按下/禁用/加载中的完整状态；供应商统计/模型统计两个标签的表格内容；官方/供应商来源下的特殊筛选；取消自动识别后的编辑态；预算/档位模式的占位与错误提示；空列表、错误、网络失败、长文本溢出；深色主题；Windows/Mac 标题栏差异；导入面板。
- 设计示例数据（openccode、deepseek-v4.1-flash、v0.5.6 等）不能当作真实数据或功能依据。

### 3.3 D01—D08 差异登记

| ID | 设计/需求 | 实际核实 | 默认处理 / 关闭条件 |
|---|---|---|---|
| 差异 D01 | 顶部为电源、刷新、全屏、账号等动作 | 实际六个位置是：开机启动（`autostart`）、检查更新（`check_update`）、语言（globe 下拉）、恢复默认窗口尺寸（`restore_window_size`）、GitHub 主页、提交反馈。第 1/2/4/6 个图标外观与动作不是字面对应；tooltip 和无障碍名称已反映真实动作 | 保留六个真实动作；T03 只调整位置/视觉，不按图标改功能；S04 逐项验证动作；用户确认视觉与动作映射后关闭 |
| 差异 D02 | 同步并重启，Windows 与 macOS 相同 | 自动重启仅 macOS：`gpui/src/workbuddy_restart.rs` 在非 macOS 直接返回错误；UI 对 sync/unsync/port/startup 的重启按钮使用 `cfg!(target_os = "macos")` 禁用并显示说明 | Windows 保留禁用和说明；不得伪造可用状态；D02 不计入跨平台视觉缺陷；T05/T08/X06 复核 |
| 差异 D03 | 协议映射六项（minimal/low/medium/high/xhigh/max） | `protocol_options.rs` 的 `EFFORTS` 有 7 项，`Responses` 额外显示 `none` 映射行；`Editor::load/read` 对 Responses 读写 `responses_efforts`；其他协议隐藏 `none` | 保留 Responses 的 `none` 行；T04 调整弹窗滚动/高度，验证载入、草稿提交和 `max` 显式映射；验收 D01—D03 |
| 差异 D04 | 标题栏必须原生 | 当前使用 Kit `TitleBar`：macOS 透明标题栏 + 原生红绿灯；Windows 最小化/最大化/关闭由 Kit `ControlIcon` 绘制，非完全原生；标题栏高 34px | **T03 已按约定只登记、未关闭**：`main.rs`、`TitleBar::window_options()`、窗口生命周期与恢复行为均未改动（静态断言 `window_host_untouched = true`）；打开状态保持，仍需用户明确接受现状或确认原生改造范围后才能关闭 |
| 差异 D05 | 两端字形完全一致 | 设计本身使用平台字体列表；当前项目未设置 `font_family`，使用 `.SystemUIFont`；本次字体查询部分失败/非结论：PingFang SC/Menlo/Monaco 命中，Segoe UI/Consolas/SF Mono 未命中，未命中不等于未安装 | 默认平台系统字体，允许字形差异；T01 用 GPUI `all_font_names()` 和实际布局重新解析并回落；不打包字体；若要求同字体，另行确认许可、体积和回落 |
| 差异 D06 | 1280×800 基准 | 项目默认 1440×900，最小 1100×700；设计 PNG 是 1280×800（数据统计长图 1280×1540、成本页 1280×1010）；系统标题栏/内容区需分开测量 | 长页面滚动，不缩小整页塞入一屏；T03/T06/T08 在 1100×700、1280×800 内容区、1440×900 窗口分别记录；不用窗口尺寸冒充内容区 |
| 差异 D07 | 所有界面都有设计 | 未覆盖：供应商统计/模型统计表、请求详情、日期面板、启动检查/端口/取消同步/删除等确认框、导入面板、空/错/加载状态、部分悬停/按下状态、深色主题、Windows 标题栏差异 | 保留现有功能和样式，加 `TODO(design)`；T02/T04/T05/T07/T08 逐处登记；不得删除入口或新造产品功能 |
| 差异 D08 | 文案完全照稿 | 设计稿含“运行中”、来源、时间、金额等示例状态；实际数据是动态的，且部分旧文案与设计不同 | 仅静态且已覆盖的文案照稿；动态文案绑定真实状态；失败/未知/未定价保留原有语义；不把示例数值写死 |

### 3.4 新发现差异（D09 起；D01—D09 为本轮完整列出项）

D09 是本轮要求完整列出的最后一项；D10—D15 是 T00 额外发现的语义/结构差异，保留以便 T01+ 跟踪，不改变 D01—D09 的完整性。

| ID | 发现 | 实际核实 | 影响 / 处理 |
|---|---|---|---|
| 差异 D09 | 来源筛选文案语义 | 设计为“全部来源 / 官方请求 / 供应商请求”；实际为“全部来源 / 内置模型 / 自定义模型”，对应 `SourceScope::{All,Builtin,Supplier}`；`builtin` 对应 WorkBuddy 内置模型记录，`supplier` 对应代理与 WorkBuddy 自定义记录 | T06 只改静态标签时，必须映射到同一 `SourceScope`，不得改数据语义；建议保留现有语义并在 STATUS 记录待用户确认，避免“官方”被误解为官方 API 计费 |
| 差异 D10 | KPI 数量 | 设计为左侧总 Token + 8 张卡；实际 `statistics/overview.rs` 为总 Token + 9 张卡，额外包含 `WorkBuddy credits`（点数） | 保留点数功能，不删卡凑截图；T06 按设计排 8 张的同时为点数确定位置或单独登记布局；A03 以真实 9 项核对 |
| 差异 D11 | 请求日志列 | 设计 9 列：时间、供应商、模型、请求标识、Token/命中率、费用、耗时、tok/s、状态；实际 `REQUEST_COLUMNS = 11`，另有“来源”列和“点数”列；设计中来源是供应商下的 pill | 保留来源列与点数；T07 设计列宽/两行 Token 时考虑 11 列；L01/L02/L05 逐项验证；不得为 9 列删除数据 |
| 差异 D12 | 图表布局 | 设计为“使用趋势”和“估算费用”两张并排卡片；实际在同一张趋势卡内先 Token 图、后费用图（垂直排列） | T06 改布局但保持 `UsagePlot`/`ChartMode`/图例/断线/币种语义；A04 复验 |
| 差异 D13 | 底部状态栏与运行状态 | 设计为 30px 全宽状态栏，左绿点“本地代理运行中”，右侧托盘说明；实际没有状态栏，只有右下角 16px 托盘提示；右侧服务卡的“运行中”是硬编码 `Tag::secondary()`，不绑定真实健康状态 | **T03 已落地**：新增 30px 全宽状态栏 + `CoreState`，状态只由 `poll` 的真实 reply 推导（无 snapshot ⇒ 不可达，未收到 ⇒ 未知，地址为空 ⇒ 未知）；右侧服务卡同一硬编码 Tag 一并改为同一可信源。真实 GUI 以 `SIGSTOP`/`SIGCONT` 隔离核心反证三态（§8.3）。剩余：代理/上游转发健康仍未纳入该状态栏，见 §8.6 |
| 差异 D14 | 右侧设置与日志卡片 | 设计为两张卡（全局设置、日志）；实际是一个 `322px` 卡“全局设置与日志” | **T04 已落地**：拆为“全局设置”与“日志”两卡，右栏基准 300px（实测白底卡 298px），日志区独立滚动（§8.3 夹具二）；全部动作与回调保留 |
| 差异 D15 | 自动刷新默认值 | 设计显示 5 秒；实际 `refresh_interval` 默认 `0`（关闭），手动刷新按钮存在；可选 0/5/10/30/60 秒 | 这是行为默认值，不在 T00 擅自改为 5 秒；T06 先保留现状并登记，是否改默认需用户确认；A02/A05 |
| 差异 D16 | 控件高度基准（rem） | 设计 §3：输入框 36、按钮 32、映射输入 34。T04 实测：按钮 32（`ui_components::button` 显式像素后）✓；单行 `Input`/`Select` 26–28。根因：Kit 用 `theme.font_size`（T01 设为 13）作为窗口 rem（`gpui-component/src/root.rs`），`input_h(Size::Small)=h_6=1.5rem≈19.5` 加内边距/边框，且单行输入忽略调用方 `.h()`；设计稿实测供应商预设选择器 37.5 逻辑像素高 | 影响所有页面（统计页同样如此），不能在供应商页单独修。建议在共享层一次性解决（窗口 rem 恢复 16 并按页面显式设文字尺寸，或让 Kit 支持单行输入显式高度），T09/T10 前需用户确认取舍；T04 只保证结构/顺序/分组/颜色/对齐，不声称 36px 通过 |

其余纯视觉/布局差异（侧栏 201.6 vs 224；右栏 322 vs 300；统计标题 18 vs 22；明细标签“用量明细” vs “请求日志”；顶部行 26/48 vs 36；内容区 padding 12 vs 6/24/16；供应商底部图标按钮 vs 文字按钮；请求日志行高未显式 56/表头 32；Token 单元格当前仍是一行五段而非设计的两行）归入 T03/T04/T06/T07 的施工清单，不单独编号；若其中涉及数据语义，再升为 D16+。

## 4. 功能映射（旧入口 → 真实回调/状态 → 建议新入口 → 验收 ID）

说明：

- “旧入口”使用当前 HEAD 的文件::函数/控件 ID；“建议新入口”只表示改造后的展示位置，不表示新增功能。
- “差异 Dxx”是第 3 节的设计差异；“验收 Dxx”是 ACCEPTANCE.md §7.6 的弹窗验收 ID，二者不同。
- 行号是当前 HEAD 的参考，T01 之后可能变化；以函数名和控件 ID 为准。

### 4.1 顶部、导航与全局

| # | 动作 | 旧入口（文件::函数/ID） | 真实回调/状态链 | 建议新入口 | 验收 ID |
|---|---|---|---|---|---|
| G01 | 切到数据统计 | `ui.rs::sidebar` `nav-statistics` → `switch_page(Page::Statistics)` | 设置 `Statistics::set_configured_providers`、`set_visible(true)`；保留供应商草稿 | 224px 侧栏导航项 | S01/S03 |
| G02 | 切到供应商管理 | `ui.rs::sidebar` `nav-providers` → `switch_page(Page::Providers)` | 展开/收起 `providers_expanded`；聚焦模型表 | 侧栏“供应商管理”面板 | S01/S03 |
| G03 | 选中供应商 | `ui.rs::sidebar` `SidebarMenuItem` → `switch_provider(id)` | 先 `capture` 当前草稿，再 `load_form`；不保存到磁盘 | 供应商列表行 | S01/P07 |
| G04 | 开机启动 | `ui.rs::app_controls` `autostart` → `send("autostart", command)` | 核心 `src/proxy.rs` 处理 `autostart`，调用 `src/autostart.rs::set`（macOS LaunchAgent / Windows Run），失败回滚；`snapshot` 的 `autostart` 更新 `cfg.autostart`；按钮 `selected/toggled/primary` 反映真实状态 | 顶部第 1 个图标按钮 | S04 / 差异 D01 / C06 |
| G05 | 检查更新 | `app_controls` `check-update` → `send("check_update", force=true)` | `tick` 收到回复后显示“发现新版本 … / 当前已是最新版本”；不改配置 | 顶部第 2 个图标按钮 | S04 / 差异 D01 |
| G06 | 切换语言 | `app_controls` `language` → `language_menu` → `set_language(code)` | 先 `capture`；设置 `zh`/locale；更新统计页语言和输入占位；发送 `{"command":"language"}`；菜单选中 `cfg.language` | 顶部第 3 个地球下拉 | S03/V04 / 差异 D01 |
| G07 | 恢复默认窗口尺寸 | `app_controls` `restore-window-size` → `restore_window_size` | `window.resize(1440×900)`；全屏先 `toggle_fullscreen`，最大化先 `zoom_window`，异步等待；5 秒超时报错；不持久化尺寸 | 顶部第 4 个图标按钮（设计图标为全屏） | S03 / 差异 D01 |
| G08 | GitHub 主页 | `app_controls` `project-homepage` | `cx.open_url(PROJECT_URL)`；无应用内状态 | 顶部第 5 个图标按钮 | S04 / 差异 D01 |
| G09 | 提交反馈 | `app_controls` `submit-feedback` | `cx.open_url(FEEDBACK_URL)`；无应用内状态 | 顶部第 6 个图标按钮（设计图标为账号） | S04 / 差异 D01 |
| G10 | 新增供应商 | `ui.rs::sidebar` `provider-add` → `send("add")` | 核心新增草稿供应商；`update_config` 选中最后一个；不自动保存 Key/模型 | 侧栏底部固定“添加” | P07 |
| G11 | 复制供应商 | `sidebar` `provider-copy` → `dirty_steps` + `duplicate` | 先保存当前脏草稿步骤，再 `duplicate`；核心复制；`update_config` 选中副本 | 侧栏底部固定“复制” | P07 |
| G12 | 删除供应商 | `sidebar` `provider-delete` → `delete_open` → 确认框 | 取消/确认；确认发送 `{"command":"delete"}`；设计未覆盖该确认框 | 侧栏底部固定“删除” | P07 / 验收 D06 |
| G13 | 关闭设置窗口 | `ui.rs::new` `on_window_should_close` | `capture` + `drafts.persist`；GPUI 退出；托盘/核心继续 | 原生窗口关闭 | X04 |
| G14 | 退出应用 | `main.rs` `cmd-q` → `ui::Quit` → `quit()` | 持久化草稿，写 `gpui-exit-host`，`cx.quit`；托盘宿主处理退出 | 快捷键/托盘 | X04 |

### 4.2 供应商连接、协议与保存

| # | 动作 | 旧入口（文件::函数/ID） | 真实回调/状态链 | 建议新入口 | 验收 ID |
|---|---|---|---|---|---|
| P01 | 选择供应商预设 | `ui.rs::preset_menu` `preset_select` → `apply_preset` | 按预设填名称/URL/协议；`capture`；只改草稿；不保存 | 连接设置“供应商预设” | P01 |
| P02 | 名称/API Key/URL | `ui.rs` `self.name/key/url` `InputState`；`capture` → `Draft` | 变更进入草稿；API Key `mask_toggle` 只改显示；`url` 变更触发 `sync_preset`；不提前持久化 | 连接设置字段 | P01/C02 |
| P03 | 上游协议 | `protocol_select` 订阅 `SelectEvent::Confirm` | 更新 `draft.protocol`；重置 `protocol_settings`；Azure 认证条件保留；不保存 | 连接设置“上游协议” | P01 / 验收 D01 |
| P04 | 协议选项弹窗 | `protocol-options` → `Editor::load` → `protocol_open` | 共用 `protocol_options::Editor`；`confirm_protocol` 调 `read()` 校验后写回草稿；取消不改；不保存 | 连接设置“协议选项”按钮 + Dialog | P01—P02 / 验收 D01—D03 / 差异 D03 |
| P05 | 获取模型 | `fetch` → `send("fetch", url/key/protocol/settings)` | 回复打开 `picker`；清空搜索/选择；`search` 聚焦；只加入待添加列表，不保存 | 模型列表“获取模型” | P02/P04 |
| P06 | 测试连接 | `test` → `save_steps` + `{"command":"test"}` | 把当前草稿作为步骤保存后测试；选中模型时带 `model_index`；busy/错误/日志走原链路 | “测试连接” | P02 |
| P07 | 保存供应商 | `save` 按钮 / `cmd-s` / `Save` action → `save()` | 有弹窗/端口/编辑态时阻止；`save_steps` → `send("save")`；回复 `update_config` 刷新；成功才持久化 | 连接设置“保存” | P01/P03 / C05 |

### 4.3 模型选择、编辑与键盘

| # | 动作 | 旧入口（文件::函数/ID） | 真实回调/状态链 | 建议新入口 | 验收 ID |
|---|---|---|---|---|---|
| M01 | 模型行选择 | `ui.rs::click_model`；`model_table` 行点击 | `Selection::click(i, shift, control||platform)`；Shift 连选、Ctrl/⌘ 切换；双击（无修饰键）打开编辑；`table_focus` | 模型表格行 | P04/P05 |
| M02 | 模型键盘操作 | `main.rs:357-368` 绑定；`ui.rs` `ModelList` key context | Delete/Backspace → `RemoveModels` → `remove_models`；Cmd/Ctrl+A → `SelectModels`；F2/Enter → `EditModel` → `edit_selected`；Escape → `Dismiss` | 模型表格焦点 | P04/P05 |
| M03 | 添加模型行 | `model-add` → `edit(len, true)` | 打开编辑 Dialog；`new_model=true`；确认后 push；最多 256 | 模型列表“＋” | P05/P07 |
| M04 | 移除模型 | `model-remove` / Delete / Backspace → `remove_models` | 仅移除选中行；编辑/选择弹窗/ busy 时阻止；不持久化直到保存 | 模型列表“－”/键盘 | P04 |
| M05 | 编辑模型 | 行编辑图标 → `edit_model_row`；双击/F2/Enter → `edit_selected` | `edit(i,false)` 加载名称/ID/能力；防止重复打开；确认写回草稿；取消丢弃 | 模型编辑 Dialog | P05 |
| M06 | 选择模型弹窗筛选 | `ui.rs::filtered`、`picker-select-all`、`picker-invert`、`picker-clear`、`picker-only` | 搜索过滤；全选/反选只作用于当前筛选；清空；仅看已勾选；已添加项禁用 | 选择模型 Dialog | 验收 D04 |
| M07 | 选择模型确认 | `picker-confirm` → `apply_picker` | 只把缺失项 push 到草稿；已有模型保留；256 上限；不保存；取消丢弃 | 选择模型 Dialog | 验收 D04 |
| M08 | 编辑模型能力 | `model_options::render`；`edit_model` 变更触发 metadata 轮询 | 自动识别/手动；工具/图片/思考/仅思考/允许关闭/自定义协议；默认/支持思考强度；上下文/token；未知来源提示；确认 `confirm_edit` 校验 | 编辑模型 Dialog | P05/P06 / 验收 D06? 实际用 P06 |
| M09 | 思考强度 | `effort_menu` `effort_select` → `self.effort` | 写入草稿 `Draft.effort`；选项 `keep/default/none/low/high/max`；不即时保存 | 连接设置“思考强度” | P01/P06 |

> 注：选择模型弹窗的键盘操作目前只有 Kit Checkbox 的焦点/空格与 Dialog Enter，**没有**主模型表那样的 Shift/Ctrl/⌘/Delete 多选绑定；T05 不得把主表绑定误认为弹窗已有能力。设计未明确该弹窗的键盘语义，登记为风险。

### 4.4 同步、端口、日志与启动检查

| # | 动作 | 旧入口（文件::函数/ID） | 真实回调/状态链 | 建议新入口 | 验收 ID |
|---|---|---|---|---|---|
| S01 | 同步 WorkBuddy | `ui.rs::right` `sync` → `sync_open` | 打开确认 Dialog；不立即写入 | 右栏“全局设置”主按钮 | P07 / 验收 D05 / 差异 D02 |
| S02 | 同步并重启/仅同步 | `sync-restart` / `sync-only` → `sync(true/false)` | `dirty_steps`（保存当前脏供应商）+ `install` + `install_restart` 或 `install`；非 macOS 重启按钮禁用；`busy` 防重复 | 同步确认 Dialog | P07 / 验收 D05 / 差异 D02 |
| S03 | 取消同步 | `right` `restore` → `unsync_open`；`unsync-restart` / `unsync-only` | `restore_restart` / `restore`；只移除同步条目，不保存草稿；非 macOS 重启禁用 | 取消同步确认 Dialog | P07 / 验收 D05—D06 / 差异 D02 |
| S04 | 修改本地端口 | `right` `port-apply` → `pending_port` | 校验 1–65535 且与当前不同；打开确认 Dialog；`confirm_port` → `set_port`；取消恢复端口输入；非 macOS 重启禁用 | 右栏本地端口输入 + 确认 | P03 / 验收 D06 / 差异 D02 |
| S05 | 启动时地址不一致 | `ui.rs::tick` 一次性 `workbuddy_check`；`startup_report` | 仅不匹配时打开 `startup_sync_open`；`confirm_startup_sync` 只用已保存配置发 `install`；取消保留配置；非 macOS 禁用 | 启动检查确认 Dialog | 验收 D05—D06 / 差异 D02 |
| S06 | 清空日志 | `right` `clear-logs` → `send("clear_logs")` | 核心清空；`snapshot` 刷新 `log_text`；只影响日志，不碰草稿 | 日志卡标题栏 | P03 / C06 |
| S07 | 复制日志 | `right` `copy-logs` | `cx.write_to_clipboard(summary + log_text)`；状态“日志已复制”；不发请求 | 日志卡标题栏 | P03 / C06 |

### 4.5 统计筛选、日期与刷新

| # | 动作 | 旧入口（文件::函数/ID） | 真实回调/状态链 | 建议新入口 | 验收 ID |
|---|---|---|---|---|---|
| A01 | 来源切换 | `statistics/source_controls.rs::source_group` → `select_source` | `SourceFilter::select`；重复点击不变更、不发请求；`capture.reconcile`；重置依赖筛选；`coordinator.change`；键盘左/右/Home/End/Space/Enter；ARIA radio group | 页头来源 Segmented（设计为带文字） | A01 / 差异 D09 |
| A02 | 供应商/模型/状态筛选 | `statistics.rs::filters` 中的 `provider_select` / `model_select` / `outcome_select`；`sync_filter_state` | 变更标记 dirty；250ms debounce；`mark_dirty` → `refresh`；供应商/模型搜索分页；不重复轮询 | 筛选栏下拉 | A01/A02 |
| A03 | 更多筛选 | `source_controls.rs::filter_more`、`capture_control`、`filter_search`、`usage_select`、`include-tests` | 采集方式、供应商/模型搜索、用量完整性、包含测试连接；`capture`/`include_tests` 进入查询；不改变统计含义 | “更多筛选”展开区 | A01/A05 |
| A04 | 日期范围 | `statistics.rs::date_control` → `Popover` → `date_panel` | `open_date_draft` 复制当前应用值；预设 当天/1天/7天/14天/30天/90天；自定义开始/结束 + 跟随当前；`apply_date_draft` 校验顺序/366 天；取消不改应用值；Calendar 选中同步草稿 | 筛选栏日期按钮 | A02 |
| A05 | 自动刷新间隔 | `statistics.rs::refresh_control` | 0/5/10/30/60 秒；默认 0；`tick` 按 `last_refresh` 调度；只影响刷新，不改查询 | 筛选栏刷新间隔 | A02/A05 / 差异 D15 |
| A06 | 立即刷新 | `source_controls.rs::filter_refresh` `stats-refresh` | `generation += 1`；清 debounce；`applied=generation`；`refresh`；busy 时禁用 | 筛选栏刷新按钮 | A02 |
| A07 | 采集健康栏 | `statistics.rs::health_bar` | `stats_health` 的 `HealthDto`；状态、记录数、数据库大小、最早记录、更新时间；异常/丢失范围真实显示 | 页头下方健康栏 | A05 |
| A08 | 页面标题/副标题 | `statistics.rs::header` | 静态标题“使用统计”18px + 副标题；不改数据 | 页头 | S03/T06 |

### 4.6 KPI、图表、明细与请求详情

| # | 动作 | 旧入口（文件::函数/ID） | 真实回调/状态链 | 建议新入口 | 验收 ID |
|---|---|---|---|---|---|
| A09 | 总 Token / KPI | `statistics/overview.rs::overview` | 总 Token hero + 9 张卡；`DashboardDto.summary`；未知显示 `—`；缓存是输入子集；费用按币种 | 总览卡片 | A03 / 差异 D10 |
| A10 | 使用趋势 | `statistics.rs::trends`；`usage_chart::UsagePlot` | `visible_series` 图例独立开关；未知区间断线；Tooltip；token 图 + cost 图 | 使用趋势/费用卡 | A04 / 差异 D12 |
| A11 | 币种切换 | `currency_select` | 选择 USD/CNY；只改变费用展示/查询币种；不改变已记录快照 | 费用卡右上币种 | A04 |
| A12 | 明细标签页 | `statistics.rs::details` `tab-requests` / `tab-providers` / `tab-models` | 切换 `Tab`；重置分页；`rows_gate`/`groups_gate` 失效在途回复；请求/分组各自查询 | 明细卡 Segmented | A01/L03/L04 |
| A13 | 请求日志分页 | `details` `page-prev/page-size/page-next` | `rows_cursors` 游标栈；20/50/100；过期游标回第一页；只有核心给出的 cursor 才可前进 | 明细卡 Pager | L03 |
| A14 | 分组统计分页/排序 | `details` `groups-prev/groups-next/group-page-size/sort-*` | 独立 `group_cursor_stack`；`SortMetric`；`GroupBy` provider/model；不混用请求游标 | 明细卡 Pager/排序 | L04 |
| A15 | 请求详情 | `usage_table.rs::render_tr` 行 `on_click` → `Statistics::open_detail` | 取 `request_id`；`stats_request_detail`；`details`/`detail_open`；560px Dialog，可滚动；只显示摘要，不含聊天正文/Key | 请求日志行点击 | L03/L05 |
| A16 | 详情字段 | `statistics.rs::detail_content` | 请求标识、时间、供应商/路由、模型、协议、状态、用量、Token、性能、命中率、tok/s、费用、price snapshot、记录时间；未知用 `—` | 用量详情 Dialog | L05 |
| A17 | 表格列宽 | `usage_table.rs::fitted_widths` / `column` | 最小宽度、可拖拽自定义宽度、总宽不足时横向滚动；既有 diff 已调整 Token 列最小宽度 360 | 请求日志表格 | L02 / 既有 diff |
| A18 | 折叠成本定价/采集存储 | `statistics.rs::section_toggle` | 切换 `pricing_open`/`storage_open`；不刷新数据 | 底部折叠卡 | R01/R05 |

### 4.7 定价、采集存储与危险操作

| # | 动作 | 旧入口（文件::函数/ID） | 真实回调/状态链 | 建议新入口 | 验收 ID |
|---|---|---|---|---|---|
| R01 | 价格搜索 | `pricing_section` `price_search` | `sync_filter_state` 比较 `price_query`；`request_prices`；精确模型 ID | 成本定价卡搜索框 | R01 |
| R02 | 新建价格规则 | `price-add-top` → `new_price` | `PriceDraft::blank`；打开 520px Dialog；只更新草稿 | 成本定价“新建价格规则” | R03 |
| R03 | 刷新价格 | `price-reload` → `request_prices` | 重新读取价格列表；不改规则 | 成本定价“刷新价格” | R01 |
| R04 | 手动更新价格 | `price-sync-models-dev` | `stats_prices_update`；`JobKind::PriceSync`；`price_sync_pending=true`；按钮变“正在更新价格…”并禁用；成功/失败都复位；成功后 `price_sync`/列表刷新；失败不永久禁用 | 成本定价“手动更新价格” | R02 |
| R05 | 编辑/删除价格 | `price-edit` / `price-delete` | 编辑打开 Draft；删除发 `stats_price_delete`；删除只影响新请求，历史快照不动 | 价格规则行操作 | R04/R06 |
| R06 | 价格弹窗保存 | `statistics.rs::open_price_dialog` → `price_body` → `save_price` | 供应商/模型 ID（编辑锁定）、计价方式、币种、四项价格、备注；`collect_price_draft`；校验；`stats_price_save`；成功才关闭；失败保留草稿；Enter 与保存一致 | 价格规则 Dialog | R03/R04 |
| R07 | 缺省默认币种 | `storage_section` `stats-default-currency` → `save_default_currency` | `stats_settings_update` with `expected_revision`；仅新请求（界面说明）；不改历史 | 采集与存储左列 | R05 |
| R08 | 启用摘要采集 | `collect-enabled` Checkbox → `update_settings` | 写 `enabled`；暂停只影响新请求；保留已有记录 | 采集与存储左列 | R05 |
| R09 | 保留天数 | `retention-*` → `update_settings` | 30/90/180/365/永久(0)；`expected_revision`；显示当前值 | 采集与存储左列 | R05 |
| R10 | 供应商采集模式 | `provider-modes-refresh`；`provider-mode` Checkbox → `set_provider_mode` | 按供应商读取 `stats_provider_settings_get`；切换 observe/request；分页；失败提示 | 采集与存储右列 | R05 |
| R11 | 清除历史统计 | `clear-preview` → `preview_clear`；`clear-confirm` → `confirm_clear` | `stats_clear_preview` 返回 row_count + 短时 token；确认用 `stats_clear_start(token)`；`maintenance` 轮询 `stats_maintenance_status`；预览失败/过期不删除；只在隔离数据库验证 | 采集与存储危险操作区 | R06 / 验收 D06 |
| R12 | WorkBuddy 导入面板 | `statistics.rs::storage_section` → `import_controls` | 导入开关、状态、计数、映射；设计未覆盖；保留现有样式/行为 | 采集与存储下方 | 差异 D07 / A05 |
| R13 | 设置/价格错误 | `sync_filter_state`、`save_price`、`update_settings`、`set_provider_mode` | 错误写入 `error`/`price_draft.error`，在对应卡/Dialog 显示；不静默失败 | 各卡片/Dialog | R02/R03/R05/R06 |

### 4.8 窗口、平台与生命周期

| # | 动作 | 旧入口（文件::函数/ID） | 真实回调/状态链 | 建议新入口 | 验收 ID |
|---|---|---|---|---|---|
| W01 | 默认/最小/恢复尺寸 | `gpui/src/main.rs` `default_window_size`、`window_min_size`；`ui.rs::restore_window_size` | 1440×900 默认；1100×700 最小；恢复按钮异步处理全屏/最大化；不持久化 | 顶部第 4 个按钮 | S03 / 差异 D06 |
| W02 | 标题栏/平台装饰 | `TitleBar::window_options()`；Kit `WindowControls` | macOS 透明标题栏 + 原生红绿灯；Windows 控件由 Kit 绘制；标题栏 34px | 保留 Kit 集成 | X06 / 差异 D04 |
| W03 | 关闭窗口/托盘 | `on_window_should_close`；`tick` 检查 `gpui-stop.json`/`instance.json`；`gpui_host.rs` | 关闭窗口持久化草稿并退出 GPUI；托盘/核心继续；宿主可写 `gpui-stop.json` 停止；`gpui-exit-host` 请求退出 | 原生关闭/托盘 | X04 |
| W04 | 自动重启平台 | `workbuddy_restart.rs`；UI 的 sync/unsync/port/startup | 仅 macOS 调用 C FFI；非 macOS 返回错误；UI 禁用重启按钮并说明 | 同步/端口确认框 | X06 / 差异 D02 |
| W05 | 主题/字体重开 | `main.rs` `Theme::change/update` | 启动固定 Light；没有按平台字体 token；弹窗使用同一 `Theme` | 统一主题接入 | V01—V04 |

## 5. 拟改文件边界

### 5.1 允许新增/修改（按任务）

| 任务 | 拟新增 | 拟修改 | 说明 |
|---|---|---|---|
| T01 | `gpui/src/theme.rs` | `gpui/src/main.rs`（`mod theme`、`Theme::update` 接线） | 只做 token、字体、圆角、颜色；不迁移页面 |
| T02 | `gpui/src/ui_components.rs`（实际结构确定为单文件薄封装） | `gpui/src/main.rs`（`mod ui_components`）、`ui.rs`、`statistics.rs` 的真实调用点 | 不重写 InputState/SelectState；不建第二套输入系统；未迁移整页 |
| T03 | 如需补充图标，`gpui/src/assets.rs` + `assets/icons/*.svg` | `gpui/src/ui.rs` 的骨架、侧栏、顶部、状态栏；必要时 `main.rs` 布局常量 | 保留六个动作和窗口生命周期；D04 只登记 |
| T04 | 未新增图标（复用现有 `IconName`） | `gpui/src/ui.rs` 连接设置/右栏；`gpui/src/protocol_options.rs` 显示层；`gpui/src/model_table.rs` 单元显示；`gpui/src/ui_components.rs` 按钮高度；`gpui/src/theme.rs` 追加 token | 保留协议 Editor 的解析/校验/草稿语义；实测改动见 §8.3 T04 |
| T05 | 无 | `gpui/src/ui.rs` 选择模型/编辑模型/同步确认；`gpui/src/model_options.rs`、`gpui/src/model_table.rs` | 键盘、选择、草稿、能力联动、平台禁用全部保留 |
| T06 | 无 | `gpui/src/statistics.rs`、`gpui/src/statistics/overview.rs`、`gpui/src/statistics/source_controls.rs`、`gpui/src/usage_chart.rs`、`gpui/src/source_filter.rs`（视觉层） | 不改查询、筛选语义、轮询策略、指标计算 |
| T07 | 无 | `gpui/src/usage_table.rs`（在既有 diff 之上）；`gpui/src/statistics.rs` 明细容器 | 先保留既有改动；两行 Token、最小列宽、行高、横向滚动 |
| T08 | 无 | `gpui/src/statistics.rs` 定价/存储/价格弹窗 | 保留价格快照、校验、危险操作、设置 revision |
| T09 | `validation/<date>/` 证据 | 只修共享层缺陷，不做平台特判堆叠 | Windows 无环境则 BLOCKED |
| T10 | `validation/<date>/report.md` 等 | 只改交付文档 | 不提交/不推送 |

### 5.2 必须保护、原则上不改

- `gpui/src/usage_table.rs` 的既有未提交修改（T00—T06 不触碰；T07 在其上施工）。
- `gpui/src/state.rs`、`backend.rs`、`stats_backend.rs`、`stats_controller.rs`、`stats_dto.rs`（只读理解）。
- 根 `src/`（核心、代理、协议编码、统计查询、计价、数据库、autostart）、`native/`、`gpui/src/workbuddy_restart.rs`。
- 配置格式、接口字段、保存时机、草稿/持久化语义、数据保留策略、统计聚合与数值格式。
- 依赖版本（Kit 锁定 `=0.7.3`）、Cargo.lock、打包脚本的平台行为。
- `ui.rs` 已有事件订阅、`InputState`/`SelectState`、`DataTable` 委托；不能为了布局复制状态或重复订阅。

### 5.3 资产与依赖

- 可以补充确有需要的 SVG 图标；图标必须与真实动作一致，不能因为设计稿画了某个符号就赋予新功能。
- 不新增字体文件、不打包字体；不新增依赖、不升级 Kit；不引入 WebView/前端框架。
- 不新增永久调试菜单；测试展示页仅限测试使用，不进入产品菜单。

## 6. 字体与原生标题栏策略

### 6.1 字体策略

当前：

- `font_family = .SystemUIFont`（未显式设置），`font_size = 16`。
- `mono_font_family` 默认 macOS Menlo / Windows Consolas；Kit 在缺失时回落。
- 没有字体 token，页面大量显式 `text_size`。

T01 建议：

1. 启动时读取 `cx.text_system().all_font_names()`，只在候选字体确实存在时设置显式 family；否则保留 `.SystemUIFont`。GPUI 的 `font_family` 是单个 family，不是 CSS 列表。
2. 无衬线候选：
   - Windows：`Segoe UI Variable` → `Segoe UI` → `Microsoft YaHei UI` → `.SystemUIFont`（未实机验证，仅候选）。
   - macOS：`.SystemUIFont`（系统字体；中文由系统回落 PingFang SC）；如确需显式中文，候选 `PingFang SC`。
3. 等宽候选：
   - Windows：`Consolas` → `Cascadia Mono` → `.SystemUIFont`（未实机验证，仅候选）。
   - macOS：`Menlo` → `Monaco` → `.SystemUIFont`（本次查询未命中 `SF Mono`/`Consolas`，属失败/非结论；T01 运行时复核）。
4. 不在源码中把设计稿的 CSS 字体列表塞进一个 family；实际解析结果记录到 V03 证据。
5. 字号/行高按 DESIGN_SPEC 建立 token；不改变 Kit 控件内部必要的字体行为。T01 设置基础字号后，页面逐步迁移显式字号，避免一次性重写。
6. 中英混排、长模型 ID、URL、数字分别用实际字体验证；缺字/截断必须记录，不静默回落到错误字体。

### 6.2 等宽数字

- GPUI 支持 `FontFeatures`；Kit 已有 `tabular_figures()`（`[("tnum", 1)]`）。
- T01 在 `theme.rs` 提供 `pub(crate) fn tabular_figures() -> FontFeatures`，供后续数字 `Label`/自定义文本使用。
- 需要应用到：KPI 数值、总 Token、费用/单价、Token 两行、耗时、tok/s、命中率、分页计数、日期/时间、价格输入/列表。
- 字体本身不支持 `tnum` 时，GPUI 不会自动变等宽；T01/T06/T07 必须实际显示验证（V03/L05），必要时采用经过验证的数字字体回落，不能改变数值格式或截断值。
- 不把 `tnum` 加到正文/中文标签，避免影响中文字距。

### 6.3 原生标题栏策略

- 保留当前 `TitleBar::window_options()` 和 `app_owns_titlebar_drag`，不在 T03 重写窗口宿主。
- macOS 红绿灯为原生；Windows 控件由 Kit 绘制，无法宣称“完全原生”。D04 保持打开。
- 内容区布局不以系统标题栏高度为设计内容；T03 记录窗口 bounds 和实际内容区尺寸，D06 分别标注。
- 设计稿没有绘制系统标题栏；截图对比时单独评价标题栏，不纳入内容区像素差异。
- 用户明确接受现状或确认原生改造范围前，T10 只能写“部分完成/待验收”，不能写完整设计验收通过。

## 7. 隔离测试配置与合成数据方案

### 7.1 已核实的真实 CLI（不杜撰）

核心/托盘（`src/main.rs`）：

```text
workbuddy-proxy [--headless | --autostart] [--port PORT] [--config-dir DIR]
workbuddy-proxy --help
workbuddy-proxy --version
```

设置窗口（`gpui/src/main.rs`）：

```text
workbuddy-proxy-gpui --config-dir DIR
workbuddy-proxy-gpui --version
```

约束：

- GPUI 没有 `--headless`；不要给设置窗口传不存在的参数。
- GPUI 需要核心先写 `instance.json`；启动前先等核心就绪。
- 两进程都支持 `--config-dir`；`WORKBUDDY_PROXY_CONFIG_DIR` 是环境变量覆盖，不是额外 CLI 参数。
- 单实例锁 `gpui.lock`；已有实例时再次启动会写 `gpui-wake.json` 而不是创建第二个窗口。

### 7.2 现有隔离启动方式（已核查，未执行）

| 方式 | 脚本/位置 | 实际做法 | 注意 |
|---|---|---|---|
| 核心 + GPUI 夹具 | `scripts/workbuddy_import_ui_fixture.py` | 合成 `config.json`；隔离 `HOME`/`USERPROFILE`/`APPDATA`/`LOCALAPPDATA`/XDG/TMPDIR/TMP/TEMP；`WORKBUDDY_PROXY_CONFIG_DIR`；本地 Mock HTTP；核心 `--headless --config-dir DIR --port PORT`；GPUI `--config-dir DIR`；合成 JSONL/SQLite/导入记录 | macOS 沙箱 bootstrap/签名逻辑；默认会写 `dist/workbuddy-import-review/ui-live-current.json`；使用前必须读参数并改输出到会话临时目录。T00 未运行 |
| 打包应用测试 | `scripts/gpui_package_test.py` | `--exe`/`--output`/`--directory`/`--hold`；隔离 HOME；本地 Mock；启动打包 host；等待 `gpui.json`；检查生命周期 | 需要打包结构（核心 + 同级/Helper GPUI），裸核心不能冒充；T00 未运行 |
| 统计/协议核心测试 | `scripts/statistics_test.py`、`statistics_pagination_test.py`、`native_*_test.py` 等 | 临时目录 + Mock 上游 + 控制 API | 只覆盖核心，不等于 GUI；T00 未运行 |
| 旧截图记录 | `docs/screenshots/README.md`、`docs/protocol-compat/FINAL-VALIDATION.md` | 临时 HOME、演示 Key、loopback Mock、合成 SQLite；Mac v0.5.5 | 不能当作当前版本或 Windows 基线 |

### 7.3 T01+ 安全启动方案（未执行）

1. 在 `/Volumes/1TB` 下创建本会话专用目录（例如 `/Volumes/1TB/tmp/ui-ux-t01-<timestamp>/`），并设置：
   - `TMPDIR`、`TMP`、`TEMP` 指向该目录下的 `tmp/`。
   - 配置目录 `config/`、隔离 HOME `home/`、Mock 端口、日志目录。
2. 生成合成 `config.json`（演示 Key、loopback 上游、合成供应商/模型），不读取真实 `~/Library/Application Support/WorkBuddyProxy` 或 `%APPDATA%\WorkBuddyProxy`。
3. 启动核心：

   ```sh
   workbuddy-proxy --headless --config-dir "$DIR/config" --port "$PORT"
   ```

4. 等待 `"$DIR/config/instance.json"` 出现并读取 management key；在此之前不启动 GPUI。
5. 启动 GPUI：

   ```sh
   workbuddy-proxy-gpui --config-dir "$DIR/config"
   ```

   等待 `"$DIR/config/gpui.json"`；截图/交互只在这个进程。
6. 上游请求只指向本地 Mock；不启动真实 WorkBuddy；不点会修改真实开机启动/端口/同步的确认按钮。需要验证这些弹窗时，只用隔离配置并记录“Mock/未执行真实系统集成”。
7. 停止：优先写 `"$DIR/config/gpui-stop.json"` 让 GPUI 退出；再按已有夹具的 `_control/stop` 或托盘退出路径停止核心；等待 PID 退出后再清理。
8. 清理：只删除本轮创建的 `$DIR`；先 `ps`/`lsof` 确认无进程引用；保留报告/截图/日志到 `validation/<date>/`。使用 `try/finally` 或 `trap`。
9. 构建/测试若执行：复用 `/Volumes/1TB/Downloads/git/workbuddy-proxy/gpui/target`（`CARGO_TARGET_DIR`），不运行 `cargo clean`；构建进程退出后再清理临时目录。

T00 没有启动上述任何进程，因此 B04 为 `NOT RUN`。

### 7.4 合成数据夹具（FX01—FX10）

| 夹具 | 构造方式 | 验证目的 |
|---|---|---|
| FX01 基准供应商 | 合成 `config.json`：至少两个供应商、多个模型、短/长名称 | 选中、切换、保存、统计计数 |
| FX02 长列表 | 生成 30 个供应商、200 个模型 | 侧栏底部固定、搜索、滚动、多选 |
| FX03 长文本 | 合成中文/英文长名称、长模型 ID、长 URL、长请求标识 | 截断、Tooltip、输入完整值、列宽 |
| FX04 基准统计 | 隔离 SQLite + 控制 API/Mock 上游；成功/失败/进行中、来源、客户端、币种 | 来源、筛选、KPI、分页 |
| FX05 缺失与零 | 零、未知、未定价、用量不全、已知缓存为零、非法命中率 | 不混淆 0/未知/未定价，禁止伪造 100% |
| FX06 图表 | 连续、单点、可靠零值、未知间隔、部分记录不全 | 断线、Tooltip、图例、费用 |
| FX07 多页日志 | 超过两页的日志/聚合结果；大数、小金额、长耗时 | 翻页、列调整、精确值、详情 |
| FX08 价格 | 新建/编辑、不同计价方式、CNY/USD、合法/非法输入 | 字段锁定、校验、失败保留草稿 |
| FX09 异步异常 | Mock 延迟/失败/断开；快速切筛选；更新价格失败 | 加载、禁用、恢复、过期结果隔离 |
| FX10 模型能力 | 自动识别成功/未知、手动模式、原生协议限制 | 禁用、来源提示、联动与草稿 |

构造只使用已有测试脚本、控制 API 和隔离 SQLite；不新增生产接口，不使用真实 Key 或真实账单。

## 8. 验证与任务状态

### 8.1 T00 记录

```markdown
### T00 — 核查代码与建立基线
- 状态：DONE（仅静态核查与施工记录；B04 未执行）
- 基线：branch codex/ui-redesign；HEAD cbb3a91；既有修改 gpui/src/usage_table.rs 22/5；docs/UI-UX-renew/ 未跟踪；Kit 0.7.3
- 修改文件：新增 docs/UI-UX-renew/IMPLEMENTATION_STATUS.md；未改应用代码
- 复用的状态/回调：见第 4 节功能映射
- 对应验收 ID：B01—B04
- 验证结果：B01 PASS / B02 PASS / B03 PASS / B04 NOT RUN；构建、单测、GUI、Windows 均 NOT RUN
- 命令与日志路径：静态命令见第 11 节；无构建/测试日志
- 截图与环境：未截图；没有启动 GUI；未使用真实配置
- 设计差异 / TODO(design)：D01—D15 与第 3.2 节缺失材料
- 未完成事项及下次准确起点：见第 10 节；T01 从 theme.rs + main.rs 接线开始
```

### 8.2 B01—B04 逐项结果

| ID | 操作 | 结果 | 证据 / 说明 |
|---|---|---|---|
| B01 | 保存 HEAD、分支、status 和既有 diff；核对 Kit 版本 | **PASS** | `git status --short`、`git diff --numstat`、`gpui/Cargo.toml`、`gpui/Cargo.lock`；usage_table 哈希已记录；`git diff --check` 退出 0；未覆盖既有修改 |
| B02 | 对每个现有 UI 动作追踪回调和状态更新 | **PASS（静态）** | 第 4 节覆盖顶部六个、自动启动、更新价格、Responses none、模型键盘、请求详情、日期、清除、端口、同步/平台差异及主要页面动作；未进行 GUI 点击，未把图标外观当作功能 |
| B03 | 盘点设计材料和旧截图，登记 D01—D08 | **PASS** | 12 PNG + 12 页 PDF 实数；无 HTML；无 Windows 截图；Mac v0.5.5 旧截图 5 张 + 协议验证 2 张；D01—D08 及 D09—D15 已登记 |
| B04 | 启动隔离配置和 Mock，停止后检查测试目录 | **NOT RUN** | 只核查了已有隔离启动方式（核心 `--headless --config-dir --port`、GPUI `--config-dir`、现有夹具脚本）；按本阶段约束未启动任何进程，未读写真实配置/数据库/开机启动，未发真实上游请求 |

### 8.3 T01/T02 记录与 T03—T10 状态

#### T01 — 统一主题、字体与视觉参数

- 状态：`DONE（代码与静态检查；V03/V04 NOT RUN）`
- 基线：branch `codex/ui-redesign`；HEAD `cbb3a91`；Kit `0.7.3`；`usage_table.rs` 既有修改 `ff14cd5c...`（22/5）未变化。
- 修改文件：新增 `gpui/src/theme.rs`；修改 `gpui/src/main.rs`（`mod theme;` + `theme::install(cx)`）；更新本文件。未改 `ui.rs`、`statistics.rs`、`usage_table.rs`、`state.rs`、核心/代理/配置或 T02—T10 页面。
- 复用的状态/回调：`Theme::change(ThemeMode::Light, None, cx)`、`Theme::update(cx, ...)`、`cx.text_system().all_font_names()`；没有平行主题状态。
- 对应验收 ID：V01—V04。
- 验证结果：V01 **PASS（静态）**；V02 **PASS（静态）**；V03 **NOT RUN**；V04 **NOT RUN**；fmt/clippy/test/release build/diff check **PASS**。
- 命令与日志路径：见下方“T01 验证命令”；日志保留在 `/Volumes/1TB/tmp/ui-ux-t01/session-t01/logs/`。
- 截图与环境：没有截图；没有启动 GPUI；没有真实 WorkBuddy/配置/数据库/开机启动/上游。
- 设计差异 / TODO(design)：D05 字体策略已落地但未做 GUI 字形核对；D04 标题栏未改；无新增页面样式偏差。
- 未完成事项及下次准确起点：V03/V04 需真实 GPUI 运行时；T02 从 `gpui/src/ui_components/` 的薄封装开始，不重写页面状态。

**Token 与接线**

- `theme::color`：`PRIMARY`/`PRIMARY_DARK`（hover/active 复用 primary-dark，设计稿未给独立状态色）、`PRIMARY_TINT`、`PRIMARY_BORDER`、`PRIMARY_RING`、`PRIMARY_DISABLED_FOREGROUND`、`HERO_BG`、`TEXT_1/2/3`、`BORDER`、`BORDER_INPUT`、`DIVIDER`、`CHECKBOX_BORDER`、`BG_PAGE/BAR/TABLE_HEAD/SEG/DISABLED/MENU_SELECTED/TAG`、`DANGER/DANGER_BG/DANGER_BORDER`、`WARNING`、`SCRIM_RGBA`、`CHART_INPUT/OUTPUT/CACHE_READ/CACHE_WRITE/COST`、`WHITE`。
- `theme::font`、`line_height`、`space`、`radius`、`size`、`stroke`、`shadow`：集中字号/行高/间距/圆角/控件高度/表格行高/弹窗宽度/进度条/阴影参数；没有分页大小、保留天数、Token 阈值等业务值。
- `theme::install` 的顺序：`Theme::change(Light)` → 运行时解析字体 → `Theme::update` 写颜色、字号、圆角、阴影和构建期固定的视觉字段。`theme.font_size = 13`，`mono_font_size = 12`，`radius = 8`，`radius_lg = 12`。
- 主题映射覆盖 `ThemeColor` 的语义字段（primary/button/secondary/muted/foreground/border/input/ring/sidebar/table/title/status/danger/warning/overlay/chart 等）。页面内仍保留原有内联色值，T03—T08 才逐页迁移；T01 没有一次性重写渲染代码。

**字体策略**

- 启动时用 `all_font_names()` 取 GPUI 实际可见 family；查询未命中的名称只表示本次候选跳过，不推断“未安装”。
- 无衬线：macOS 保留 Kit 解析的系统字体（不强制 Pin 一个 family，保留缺字回落）；Windows 依次尝试 `Segoe UI Variable`、`Segoe UI`、`Microsoft YaHei UI`，未命中则保留系统字体。
- 等宽：macOS 依次尝试 `SF Mono`、`SFMono-Regular`、`Menlo`、`Monaco`；Windows 依次尝试 `Consolas`、`Cascadia Mono`；全部未命中则保留 Kit 默认/系统回落。
- `theme::tabular_figures()` 返回 `("tnum", 1)`，与 Kit 时间控件相同；T02—T08 在数字文本上使用，T01 不批量改页面。
- 不下载、不打包字体，不新增依赖，不升级 Kit。
- 本机记录：此前 CoreText 探测是**部分失败/非结论**；本次未运行 GUI，因而没有观察 `install` 实际选中的 family。macOS 运行时候选/实际 selection 需 V03 记录；Windows 字体与字形未验证（NOT RUN/BLOCKED）。
- D05 仍开放：允许平台字形差异；若要求两端完全一致，需另行确认字体许可、体积和回落。

**T01 验证命令**

环境：`TMPDIR`、`TMP`、`TEMP` 均指向 `/Volumes/1TB/tmp/ui-ux-t01/session-t01/tmp`；`CARGO_TARGET_DIR=/Volumes/1TB/Downloads/git/workbuddy-proxy/gpui/target`；未执行 `cargo clean`。

| 检查 | 命令 | 退出码 | 结果 / 日志 |
|---|---|---:|---|
| 格式 | `cargo fmt --manifest-path gpui/Cargo.toml --check` | 0 | PASS；`logs/fmt.log`、`logs/fmt.exit` |
| Clippy（bin） | `cargo clippy --manifest-path gpui/Cargo.toml --locked --bin workbuddy-proxy-gpui -- -D warnings` | 0 | PASS；`logs/clippy-bin.log` |
| Clippy（all targets） | `cargo clippy --manifest-path gpui/Cargo.toml --locked --all-targets -- -D warnings` | 0 | PASS；`logs/clippy-all-targets-2.log` |
| 测试 | `cargo test --manifest-path gpui/Cargo.toml --locked` | 0 | PASS；137 passed / 0 failed；`logs/test.log` |
| Release 构建 | `cargo build --manifest-path gpui/Cargo.toml --release --locked` | 0 | PASS；47.95s；`logs/build.log` |
| Git 空白 | `git diff --check` | 0 | PASS |

失败记录（如实保留）：

- 首次 `cargo clippy --all-targets` 退出 101：rustc 在测试目标上 stack overflow（`thread 'rustc' has overflowed its stack`）；原始日志 `logs/clippy.log`。
- 增大栈后重试退出 101：命中 crate 的 `recursion_limit=256`，错误定位在 T01 临时添加的 `theme.rs` 测试模块；原始日志 `logs/clippy-all-targets-retry.log`。
- 处理：删除 T01 临时的 `#[cfg(test)]` 测试模块（不涉及产品代码或既有测试），最终 `--all-targets` 退出 0。未提高 crate 全局 recursion limit，避免扩大 T01 边界。
- Clippy/build 的剩余告警仅为依赖 future-incompat：`block v0.1.6`；不是本次代码告警。

**V01 / V02 静态核对**

- V01 PASS（静态）：主题/接线文件不再含旧的 `#008daa`、`#007e99`、`#006e86`、`#19334d`、`#6b7c90`、`#e2e9f1`、`#eaf2f8`、`#c5ebf4`、`#006b83`；DESIGN_SPEC §1.1 的颜色值均可在 `theme.rs` 找到；旧页面色值仍存在，属于 T03—T08 的迁移范围。
- V02 PASS（静态）：`theme.rs` 共 115 个 `pub const`，按 color/font/line_height/space/radius/size/stroke/shadow 分组；没有分页大小、保留天数、Token 阈值、256 模型上限、366 天等业务参数。
- V03 **NOT RUN**：没有启动 GPUI，无法记录实际解析字体、中文/长模型 ID/URL/数字显示。
- V04 **NOT RUN**：没有重开窗口、切换语言或检查新弹窗的实际主题一致性。

**T01 剩余风险**

1. 基础字号从 Kit 默认 16 改为 13，会影响未显式设置字号的 Kit 控件；必须由 V03/V04 真实 GUI 检查，不能在 T01 声称视觉通过。
2. hover/active 复用 `primary-dark`，因为 DESIGN_SPEC 没有独立值；后续如确认新色值，再在 token 中替换。
3. 页面仍保留旧内联色值和旧布局；V01 只证明主题入口已集中，不证明页面已迁移。
4. macOS 无 `Segoe UI/Consolas`（此前探测为部分失败/非结论）；当前布局依赖系统回退。Windows 字体和缺字回退未验证。
5. `tabular_figures()` 只提供 API；页面尚未应用，V03 才能验证目标字体是否真正等宽。
6. `theme.rs` 使用模块级 `#![allow(dead_code)]`，因为 token 会被 T02—T08 逐步消费；T10 前应复核未使用 token，不能让它掩盖页面问题。
7. D04 标题栏未改；T10 前仍需用户对“原生标题栏”给结论。

#### T02 — 控件薄封装与状态一致性

- 状态：`DONE（代码与静态核对；C01—C06 的 GUI/IME 交互 NOT RUN）`
- 基线：branch `codex/ui-redesign`；T01 已提交 `aae634e`；`usage_table.rs` 哈希仍为 `ff14cd5c...`；Kit `0.7.3`。
- 修改文件：新增 `gpui/src/ui_components.rs`；修改 `gpui/src/main.rs`（`mod ui_components;`）、`gpui/src/ui.rs`、`gpui/src/statistics.rs`；更新本文件。未改 `usage_table.rs`、`state.rs`、核心、配置、统计/计价逻辑、协议语义、依赖版本；未做 T03 骨架，未迁移整页。
- 复用的状态/回调：`InputState`、`SelectState`、`Selection`、`on_click`/`on_change` 与现有订阅全部留在调用点；wrapper 返回 Kit 控件，不复制内部状态、不建立第二套输入系统。
- 对应验收 ID：C01—C06。

**组件映射与真实调用点**

| Kit 控件 | 薄封装 | 已接入的真实调用点 | 验收 |
|---|---|---|---|
| Button | `button`/`ButtonKind`（Primary/Outline/Normal/Danger/DangerOutline/Ghost） | `ui.rs` 保存/测试连接；`statistics.rs` 清除历史危险按钮 | C01 |
| 图标 Button | `icon_button`/`icon_button_sized`/`named_icon_button`/`named_icon_button_sized` | `ui.rs::app_controls` 的六个顶部动作（包含语言菜单） | C01/C06 |
| Input | `text_input`/`masked_input`/`compact_input` | `ui.rs` 供应商名称、API Key、API 地址、本地端口 | C02 |
| Select | `select`（泛型透传 `SelectState`） | `ui.rs` 思考强度、供应商预设、上游协议 | C03 |
| Checkbox | `checkbox` | `ui.rs` 选择模型弹窗已有模型禁用项 | C04 |
| Segmented | `segment_button`/`segmented_row` | `statistics.rs::details` 请求日志/供应商统计/模型统计标签 | C04 |
| Card | `panel_card`/`content_card` | `ui.rs::card` 与 `statistics.rs::stats_card`（两页所有卡片经由此处） | C01/C05 |
| Table 外框 | `table`（统一 `small`、32px 表头、双滚动条） | `ui.rs` 模型表；`statistics.rs` 请求日志与分组表 | C05/L |
| Dialog | `dialog_body`/`dialog_body_sized`/`dialog_footer`/`scrollable_form`/`dialog_title` | `ui.rs` 协议选项、编辑模型；`statistics.rs` 价格、详情等弹窗（11 处 body / 10 处 footer） | C05 |
| Pager | `pager_button`/`pager_status` | `statistics.rs::details` 上一页/条每页/下一页 | C01 |
| Tag | `tag`/`TagKind` | `ui.rs` 供应商计数、运行中标签 | C01/C06 |

**C01—C06 逐项结果**

| ID | 静态核对 | GUI / 鼠标 / 键盘 / IME |
|---|---|---|
| C01 按钮状态与事件只执行一次 | **PASS（静态）**：`button` 始终写入 `disabled(disabled || busy)` 与 `loading(busy)`；Kit 自带 `disabled_legacy_button_keeps_existing_pointer_blocking`、`loading_button_keeps_existing_pointer_blocking`、`loading_button_without_callback_still_blocks_parent_activation` 测试证明禁用/加载会阻断指针与父级激活；`on_click` 未被包装或复制 | **NOT RUN** |
| C02 中文组合输入/粘贴/选区/API Key 显隐 | **PASS（静态）**：`text_input`/`masked_input` 只把原 `InputState` 交给 Kit；`mask_toggle` 保留；没有新建输入状态或文本缓存 | **NOT RUN** |
| C03 下拉鼠标/方向键/Enter/Escape/边缘弹层 | **PASS（静态）**：`select` 只附加 `.small()`/`.disabled()`；`SelectState`、`SelectEvent::Confirm` 与 8 处订阅保持原位；未改菜单尺寸/锚点配置 | **NOT RUN** |
| C04 Checkbox/Segmented 禁用语义 | **PASS（静态）**：`checkbox` 转发 `checked/disabled`；选择模型已有项仍 `disabled(existing)`；`segment_button` 转发 `disabled`，三个统计标签仍为 `false`，与原逻辑一致 | **NOT RUN** |
| C05 长 Dialog 焦点与滚动 | **PASS（静态）**：协议/编辑模型弹窗使用 `dialog_body_sized` + `scrollable_form`；价格/详情弹窗使用 `dialog_body`/`dialog_footer`；`window.open_dialog` 的 `close_button`、`overlay_closable`、`on_ok`、`on_close` 与焦点设置未改 | **NOT RUN** |
| C06 图标 Tooltip/可访问名称 | **PASS（静态）**：迁移的顶部六个图标与语言按钮由 wrapper 强制 tooltip + accessibility label；扫描 `ui.rs`/`statistics.rs` 的纯图标按钮，缺失 tooltip/aria 的数量为 0；带可见 `label` 的图标按钮依赖可见文本作为名称（如“同步 WorkBuddy”“获取模型”），不计为纯图标按钮 | **NOT RUN**（悬停/读屏未执行） |

**T02 验证命令**

环境：`TMPDIR`、`TMP`、`TEMP` 均指向 `/Volumes/1TB/tmp/ui-ux-t02/session-t02/tmp`；`CARGO_TARGET_DIR=/Volumes/1TB/Downloads/git/workbuddy-proxy/gpui/target`；未执行 `cargo clean`。

| 检查 | 命令 | 退出码 | 结果 / 日志 |
|---|---|---:|---|
| 模块编译 | `cargo check --manifest-path gpui/Cargo.toml --locked` | 0 | PASS；`logs/check-module-2.log`、`logs/check-integration-2.log` |
| 格式 | `cargo fmt --manifest-path gpui/Cargo.toml --check` | 0 | PASS；`logs/fmt.log` |
| 新模块格式 | `rustfmt --edition 2024 --check gpui/src/ui_components.rs` | 0 | PASS；`logs/rustfmt-ui-components.log` |
| Clippy | `cargo clippy --manifest-path gpui/Cargo.toml --locked --all-targets -- -D warnings` | 0 | PASS；`logs/clippy.log` |
| 测试 | `cargo test --manifest-path gpui/Cargo.toml --locked` | 0 | 137 passed / 0 failed；`logs/test.log` |
| Release 构建 | `cargo build --manifest-path gpui/Cargo.toml --release --locked` | 0 | PASS；1m 13s；`logs/build.log` |
| Git 空白 | `git diff --check` | 0 | PASS；`logs/diff-check.log` |

- Clippy/build 的剩余告警仅为依赖 future-incompat：`block v0.1.6`；没有本次代码告警。
- `ui_components.rs` 编译为无警告；`InputState`/`SelectState` 均为原实体，未新增状态字段。
- `usage_table.rs` 在全部命令前后 SHA-256 一致：`ff14cd5c47ff9cc450b4b5ecfcfa6981a02797c2370eb3eab91a786377a275f3`。
- 测试策略：复用现有 137 项 gpui 测试；未新增机械断言，未创建测试专用展示页，也未增加产品菜单或永久调试入口。

**T02 未验证项与剩余风险**

1. C01—C06 全部 GUI/鼠标/键盘/IME/悬停/滚动/读屏验收均 **NOT RUN**；没有启动 GPUI、没有截图、没有隔离夹具运行。
2. 只有部分调用点迁移到 wrapper：仍直接调用 `Button::new` 68 处、`Input::new` 13 处、`Select::new` 5 处、`Checkbox::new` 4 处；T03—T08 继续迁移，T02 未整页重写。
3. 供应商名称/Key/URL 输入改为统一 36px 字段高度；真实 GUI 视觉与 IME 行为待 V03/V04，不能在 T02 声明通过。
4. 带可见 label 的图标按钮没有单独 tooltip（依赖标签）；C06 的悬停/读屏语义仍需 GUI 验证。
5. `ui_components.rs` 使用 `#![allow(dead_code)]`，因为部分封装要等 T03—T08 消费；T10 前必须复核。
6. 未改窗口骨架，T03 才开始侧栏/顶部/状态栏；D04（标题栏）与 D13（状态栏可信数据源）仍开放。

**T02 清理与下一步**

- 本轮临时目录 `/Volumes/1TB/tmp/ui-ux-t02/session-t02/tmp` 为空（0B），已删除；验证日志约 **60K** 保留在 `/Volumes/1TB/tmp/ui-ux-t02/session-t02/logs/`。
- 磁盘：`/Volumes/1TB` 可用约 **385 GiB**，系统盘可用约 **48 GiB**。
- T03 起点：在 `ui.rs` 建立固定侧栏 224px、顶部图标行 36px、底部状态栏 30px；复用 T02 wrapper；保留六个真实动作；状态栏数据源必须可信（D13）；不迁移供应商/统计整页。

#### T03 — 窗口骨架与固定区域

**状态：** DONE（代码 / 静态 / macOS 真实 GUI 骨架与交互证据）。S01、S02 有真实 GUI 证据；S03 的窗口尺寸部分有 GUI 证据、语言切换为 GUI `NOT RUN`；S04 的六个动作映射为静态证据、状态源有真实 GUI 证据。详见本节与 §8.5。

**开工前置确认（本轮，全部符合，否则应停止）**

| 项 | 要求 | 实测 | 结论 |
|---|---|---|---|
| 供应商 / 账号 | OpenCode Go（账号 1） | `providers.opencode-go`，`displayName` = `OpenCode Go (账号 1)` | 符合 |
| 模型 | DeepSeek V4.1 Flash | 运行时元数据 `provider=opencode-go`、`model=deepseek-v4.1-flash` | 符合 |
| 推理档位 | Max | `modelSelection.lastUsed.reasoningEffort = max`；`agent-default-model.reasoningEffort = max` | 符合 |
| 权限 | 完全权限 | `permissions.sandbox = danger-full-access`、`permissions.approval = never` | 符合 |
| 工作区 | 独立 worktree + 分支 | `/Volumes/1TB/codex/worktrees/ui-redesign/workbuddy-proxy`，`codex/ui-redesign` | 符合 |
| 基线 | `c1aad4f…` | HEAD = `c1aad4f6b0de64317a49ea35faa4d55f125ae78c` | 符合 |

**改动文件（仅 2 个代码文件 + 本记录）**

| 文件 | 改动 | 说明 |
|---|---|---|
| `gpui/src/ui.rs` | 骨架重构 | 224px 固定侧栏、36px 顶部图标行、30px 状态栏、可信核心状态源 |
| `gpui/src/assets.rs` | 新增 `info` 图标分支与 1 项测试 | 状态栏托盘提示图标；有独立分支，不会落入通用加号兜底 |
| `docs/UI-UX-renew/IMPLEMENTATION_STATUS.md` | 本记录 | — |

未改：`gpui/src/main.rs`（`TitleBar::window_options`、窗口宿主、34px 标题栏、默认 1440×900、最小 1100×700、恢复行为）、`gpui/src/theme.rs`、`gpui/src/ui_components.rs`、`gpui/src/statistics.rs`、`gpui/src/usage_table.rs`、根 `src/`、`native/`、依赖与 `Cargo.lock`。

**核心布局（窗口骨架，DESIGN_SPEC §2）**

```text
root(flex col, size_full)
 ├ Kit TitleBar（34px，未改；D04 只登记）
 └ content(flex col, size_full, bg page)
    ├ middle(flex row, flex_1, min_h_0)
    │   ├ sidebar(224px, flex_shrink_0, h_full, 内边距 18/12, bg-bar, 右边框 1px)
    │   │    ├ 品牌区（34px mark + 应用名 15/600 + 副标题 11 text-3，版本取自 CARGO_PKG_VERSION）
    │   │    ├ 导航项「数据统计」（高 38，选中态 primary-tint + primary-dark）
    │   │    └ 供应商面板（白底 / 1px 边框 / 圆角 12 / 占满剩余高度）
    │   │         ├ 标题行（高 40，标题 + chevron；Providers 页选中色；tooltip 保留）
    │   │         ├ 供应商列表（flex_1 min_h_0 overflow_y_scroll，行高 34、圆点 7、右侧计数）
    │   │         └ 工具条（高 44，flex_shrink_0，上边框，添加/复制/删除 三个等宽按钮）
    │   └ main column(flex_1 min_w_0 min_h_0, 内边距 6/24/16, 子块间距 14)
    │        ├ 顶部图标行（h 36，justify_end，六个真实动作）
    │        └ 页面内容（flex_1 min_h_0：center [+ right]，页面自身滚动）
    └ status bar（h 30，flex_shrink_0，横贯整个窗口宽度，上边框）
```

- 只有页面内容区与供应商列表滚动；侧栏工具条与状态栏是滚动区的兄弟节点，因此不随滚动移动。
- 供应商面板标题行的展开/折叠语义与基线一致（折叠时列表与工具条一并隐藏，与原 `Sidebar` footer 行为相同）。

**S03 内容区与窗口边界（实测，2x Retina 截图回读）**

| 窗口（逻辑） | Kit 标题栏 | 状态栏 | 内容区（标题栏下沿 → 状态栏上沿） | 侧栏 |
|---|---:|---:|---:|---:|
| 1440 × 901（默认） | 34（含 1px 边框） | 30（含 1px 边框） | **838.5** | 224 |
| 1280 × 800 | 34 | 30 | **737.5** | 224 |
| 1100 × 700（最小） | 34 | 30 | **637.5** | 224 |

内容区高度 = 窗口高度 − 34 − 30；三个尺寸下侧栏、标题栏、状态栏均不随窗口变化，且窄窗口下页面重排而非挤出窗口。窗口系统装饰与 Kit 标题栏不计入设计内容区高度。

**六个顶部动作（严格沿用既有回调，未按图标外观改语义）**

| 位置 | ID | 动作 | 回调（与基线逐字相同） | 禁用条件 | tooltip / 无障碍名称 |
|---:|---|---|---|---|---|
| 1 | `autostart` | 开机启动 | `send("autostart", {"command":"autostart","enabled":…})` | `self.busy` | 保留且随状态变化 |
| 2 | `check-update` | 检查更新 | `send("check_update", {"force":true})` | `self.busy` | 保留 |
| 3 | `language` | 切换语言（下拉） | `language_menu` → `set_language` → `send("language")` | `self.busy` | 保留 |
| 4 | `restore-window-size` | 恢复默认窗口尺寸 | `restore_window_size` → `window.resize(1440×900)` | `self.resetting_window` | 保留 |
| 5 | `project-homepage` | GitHub 主页 | `cx.open_url(PROJECT_URL)` | 无 | 保留 |
| 6 | `submit-feedback` | 提交反馈 | `cx.open_url(FEEDBACK_URL)` | 无 | 保留 |

D01 的图标外观差异保持登记状态：设计稿画的是电源/刷新/语言/全屏/GitHub/账号，实际动作仍为开机启动/检查更新/语言/恢复尺寸/GitHub/反馈；本轮只把按钮从旧的约 24px 提升到设计规定的 32×32，图标尺寸随 Kit 的 0.75 比例。

**状态栏数据源（D13 落地）**

- 新增私有枚举 `CoreState { Unknown, Responding, Unreachable }` 与字段 `Proxy::core_state`，只在 `tick` 处理 `reply.tag == "poll"` 时更新：该次 poll 带回 snapshot ⇒ `Responding`，没有带回 ⇒ `Unreachable`；未收到过 poll 结果 ⇒ `Unknown`。
- 显示前再做一次收口：`Responding` 但 `proxy_address` 为空时降级为 `Unknown`，避免"核心在答但代理地址未知"被显示成正常。
- 三态文案与圆点：`本地代理运行中`（primary-dark 圆点）/ `代理状态未知，等待核心响应`（text-3 圆点）/ `无法连接核心，代理状态未知`（danger 圆点 + danger 文字）。没有任何"运行中"字符串被硬编码到正常路径。
- 右侧原「本地代理」服务卡的硬编码 `Tag::secondary()("运行中")` 一并改为同一可信源（`core_tag()`）：`Success/运行中`、`Secondary/状态未知`、`Danger/未响应`。这是 T03 内 D13 状态源的最小收口，未迁移该卡片的其他内容（T04 范围）。

**自动验证（环境：`TMPDIR/TMP/TEMP=/Volumes/1TB/tmp/ui-ux-t03/session-t03/tmp`；`CARGO_TARGET_DIR=/Volumes/1TB/Downloads/git/workbuddy-proxy/gpui/target`；未执行 `cargo clean`）**

| 检查 | 命令 | 退出码 | 结果 / 日志 |
|---|---|---:|---|
| 编译 | `cargo check --manifest-path gpui/Cargo.toml --locked --bin workbuddy-proxy-gpui` | 0 | PASS |
| 格式 | `cargo fmt --manifest-path gpui/Cargo.toml --check` | 0 | PASS；`logs/` 见清理小节 |
| Clippy | `cargo clippy --manifest-path gpui/Cargo.toml --locked --all-targets -- -D warnings` | 0 | PASS；`logs/clippy.log` |
| 测试 | `cargo test --manifest-path gpui/Cargo.toml --locked` | 0 | **138 passed / 0 failed**（T02 为 137，新增 1 项 info 图标测试）；`logs/test.log` |
| Release 构建 | `cargo build --manifest-path gpui/Cargo.toml --release --locked` | 0 | PASS；1m 16s；`logs/build.log` |
| 静态验收 | `verify.py`（S01—S04 代码级断言 + 工作区保护） | 0 | 全部为 true；`logs/static-verify.json` |

- 格式化只对改动的 `ui.rs`、`assets.rs` 运行 `rustfmt --edition 2024`；**未运行 `cargo fmt`**，以免触碰 `usage_table.rs`。`cargo fmt --check` 随后为 0。
- Clippy/build 的剩余告警仅为依赖 future-incompat `block v0.1.6`，无本次代码告警。
- `usage_table.rs` 在全部命令前后 SHA-256 一致：`ff14cd5c47ff9cc450b4b5ecfcfa6981a02797c2370eb3eab91a786377a275f3`。

**真实 macOS GUI 证据（隔离夹具，非真实配置）**

夹具用合成 `config.json`（**30 个供应商 / FX02 长列表**，含超长名称与超长模型 ID），隔离 `HOME`/`APPDATA`/`LOCALAPPDATA`/XDG/`TMPDIR`，核心 `--headless --config-dir … --port …`，上游指向未被监听的 loopback 端口且**全程未发送任何代理请求**；未启动真实 WorkBuddy，未读写真实配置/统计库，未触发真实开机启动/端口/同步。

| 截图 | 场景 | 观察到的证据 |
|---|---|---|
| `01-default-1440x900.png` | 默认窗口 + 30 项长列表 | 侧栏 224；列表溢出后被裁剪（最后一行仅约 19.5px 高）；工具条固定在面板底部；顶部六图标右对齐；底部 30px 状态栏 |
| `02-1280x800.png` | 1280×800 | 同上；页面在窄宽度重排 |
| `03-min-1100x700.png` | 最小窗口 1100×700 | 无布局挤出窗口；侧栏与状态栏完整；工具条仍可达 |
| `04-providers-page.png` | 点击面板标题行 | 切到供应商页；标题行与选中行 primary-tint；`展开供应商列表` tooltip 与实际折叠状态一致 |
| `06-provider-switch-05.png` | 点击「供应商 05」 | 选中态迁移到该行；中间表单名称与模型列表（`mock-model-p05-*`）随之切换；无错误重置 |
| `07-core-unreachable-statusbar.png` | 对**隔离核心** `SIGSTOP` 后 | 状态栏变 `● 无法连接核心，代理状态未知`（圆点 RGB 实测 `(221,156,131)`） |
| `08-core-recovered-statusbar.png` | `SIGCONT` 恢复后 | 状态栏回到 `● 本地代理运行中`（圆点 RGB 实测 `(54,139,121)`） |
| `09-restored-1440x900.png` | 点击「恢复默认窗口尺寸」 | 窗口由 1280×800 实测变为 **1440×900**，位置不变 |

- 07/08 是 D13 与 S04 "底部状态来自实际数据、未知/异常不显示为正常" 的**直接反证**：同一窗口、同一代码，仅改变核心可用性，圆点颜色与文案随之改变。
- 测量方式：2x 截图回读像素（`measure.py`、`geometry.json`），不是照抄源码常量。
- 供应商选中态只在供应商管理页出现（`page == Providers && id == selected`），与基线语义一致；统计页下所有行均为普通底色。

#### 8.5 T03 验收结果（S01—S04）

| ID | 结果 | 依据 / 未做部分 |
|---|---|---|
| S01 | **PASS（部分项 NOT RUN）** | 列表独立滚动 + 工具条固定在 30 项长列表下由截图证实；非 busy 状态点击切换供应商、当前项高亮、表单/模型随切换更新由截图证实；添加/复制/删除的回调、禁用规则、tooltip、无障碍名称由静态断言证实。**busy 状态下切换页面/供应商的真实 GUI 验证 NOT RUN**：多次尝试受窗口焦点、唤醒重置与遮挡干扰，最终截图无法确认目标页面；仅保留回调入口 busy guard 的代码与静态检查证据，不计为交互通过。添加/复制/删除三个按钮的真实点击与结果也为 **NOT RUN**（隔离写入和核心副作用）。 |
| S02 | **PASS（部分项 NOT RUN）** | 状态栏是滚动区的兄弟节点且 `flex_shrink_0`，三尺寸下实测固定；统计页与供应商表单的滚动在 1280/1100 截图可见且状态栏未移动。**右侧日志区域的滚动 NOT RUN**（需要更长的合成日志才能滚出可视区）。 |
| S03 | **PASS（语言切换 NOT RUN）** | 最小 1100×700 无挤出窗口（截图）；恢复默认尺寸实测 1280×800 → 1440×900；`main.rs` 未改动，默认/最小尺寸与 `TitleBar::window_options`、恢复语义静态断言通过。**中文/英文切换的 GUI 交互 NOT RUN**：语言控件在顶部图标行第 3 位仍可见且 `set_language` 路径未被 T03 修改，但本轮用 System Events 注入的点击（单次/双次/不同间距）均未能打开 GPUI 下拉菜单，故不声称通过。 |
| S04 | **PASS（部分项 NOT RUN）** | 六个动作的 ID、回调、禁用条件、tooltip/无障碍名称与基线逐字一致（静态断言）；底部状态与右侧状态卡均来自真实核心状态，`SIGSTOP`/`SIGCONT` 反证（截图 + 圆点像素）。**busy 状态下动作禁用的真实 GUI 验证 NOT RUN**：自动化无法稳定保持 GPUI 窗口焦点，未取得可靠截图；逐个触发顶部动作也未全部执行：`autostart` 会调用真实系统级开机启动集成、`check-update` 会访问网络、GitHub/反馈会打开浏览器，按本轮约束不触发；`restore-window-size` 已实际点击并通过。**代理状态切换 NOT RUN**：没有在隔离环境构造可控的上游/健康切换。 |

不得把本节的静态断言当作 GUI 通过；标为 NOT RUN 的项在 T09/T10 前仍需真实交互证据。

#### 8.6 T03 未验证项与剩余风险

1. 语言下拉菜单的 GUI 打开与中英切换未取得证据（见 S03），需要人工或具备更多输入注入能力的环境复核。
2. busy 状态下侧栏供应商行和顶部动作禁用行为仅有代码/静态证据；自动化窗口焦点、唤醒与遮挡问题使 GUI 复验无法形成可靠证据（见 S01/S04），后续需在稳定的隔离 GUI 环境复核。
3. 状态栏覆盖的是"核心是否响应"，不是"上游/代理是否可转发"；上游健康仍由统计页采集健康栏与日志表达，本轮未扩展该语义。
4. 侧栏面板标题行在设计稿中还有一个前置图标，本轮未新增对应资源，只保留 chevron（真实可折叠语义），记为 TODO(design)。
5. 面板折叠时列表与工具条一并隐藏，与基线 `Sidebar` footer 行为一致；若设计要求折叠时工具条仍可达，需要在 T04/T09 明确并修改。
6. `NAV_ROW_HEIGHT`、`PROVIDER_ROW_HEIGHT`、`FOOTER_HEIGHT`、`STATUS_BAR_HEIGHT` 等 token 已存在但 `TOP_ICON_ROW_HEIGHT` 之外的部分仅由本骨架消费；T04 起继续复用。
7. D04 仍未关闭：Windows 上 Kit 自绘最小化/最大化/关闭与"原生标题栏"要求的冲突本轮只登记、未改窗口宿主。

#### 8.7 T03 清理与磁盘

- 本轮专用临时目录 `/Volumes/1TB/tmp/ui-ux-t03/session-t03/tmp` 与 GUI 夹具临时目录均在本轮内清空；隔离夹具停止并确认进程退出后，删除本轮合成配置、SQLite 数据、草稿及临时自动化脚本。保留构建/测试日志、测量数据和截图（包括标记为 NOT RUN 的 busy 验证尝试），未将证据放入仓库。
- 证据（截图、测量 JSON、静态断言 JSON、构建日志）保留在 `/Volumes/1TB/tmp/ui-ux-t03/session-t03/`，未进入仓库，未提交。
- 本轮未执行 `cargo clean`，始终复用 `/Volumes/1TB/Downloads/git/workbuddy-proxy/gpui/target`。

#### T04 — 供应商页面与协议弹窗代表切片

**状态：** DONE（代码 / 静态 / macOS 真实 GUI 代表切片证据）。P01、P03、D01、D02、D03 有真实 GUI 证据；P02 只有入口/禁用规则的静态与截图证据，实际点击为 **NOT RUN**。Windows GUI 仍 **BLOCKED**。

**开工前置确认（本轮，全部符合，否则应停止）**

| 项 | 要求 | 实测 | 结论 |
|---|---|---|---|
| 供应商 / 账号 | OpenCode Go（账号 1） | `cordis.patch.yml` → `llm-pi-ai.providers.opencode-go.displayName = "OpenCode Go (账号 1)"` | 符合 |
| 模型 | DeepSeek V4.1 Flash | `agent-default-model.provider = opencode-go`、`model = deepseek-v4.1-flash` | 符合 |
| 推理档位 | High | `agent-default-model.reasoningEffort = high` | 符合 |
| 权限 | 完全权限 | 会话文件策略 `danger-full-access`，审批提示关闭（自动拒绝升级） | 符合 |
| 工作区 | 独立 worktree + 分支 | `/Volumes/1TB/codex/worktrees/ui-redesign/workbuddy-proxy`、`codex/ui-redesign` | 符合 |
| 基线 | `6f19c42…` | HEAD = `6f19c426af1c9b5bc57d061bd0828a2827a65be0` | 符合 |
| 保护文件 | `gpui/src/usage_table.rs` 不得变动 | SHA-256 全程为 `ff14cd5c47ff9cc450b4b5ecfcfa6981a02797c2370eb3eab91a786377a275f3` | 符合 |

**改动文件**

| 文件 | 改动 | 说明 |
|---|---|---|
| `gpui/src/ui.rs` | 供应商页布局、右栏拆卡、端口输入、协议弹窗宽度 | 回调、`draft` 规则与 `protocol_payload` 未改 |
| `gpui/src/protocol_options.rs` | 共用协议 Editor 视觉 | 顶部字段按协议切换、映射区标题/提示框/等宽标签列 |
| `gpui/src/model_table.rs` | 模型 ID 等宽、截断值 tooltip | 显示截断不再丢失实际值（FX03） |
| `gpui/src/ui_components.rs` | 按钮显式 32px 高 / 14px 内边距 | T02 包装层缺失的设计值（见差异 D16） |
| `gpui/src/theme.rs` | 新增 token | `BUTTON_PADDING_X`、`MAPPING_LABEL_WIDTH`、`MAPPING_INPUT_HEIGHT`、`EFFORT_SELECT_WIDTH`、`DIALOG_PROTOCOL_WIDTH` |
| `gpui/src/usage_table.rs` | 未改 | 哈希不变，T07 才动 |
| `docs/UI-UX-renew/IMPLEMENTATION_STATUS.md` | 本记录 | — |

未改：`gpui/src/main.rs`（窗口宿主、默认/最小尺寸、恢复行为）、`gpui/src/state.rs`（草稿结构）、`gpui/src/statistics.rs`（统计页尚未迁移）、代理协议语义、核心/后端、数据库结构、依赖与 `Cargo.lock`。

**布局（DESIGN_SPEC §4.3 / §5）**

```text
主区
 ├ 连接设置卡片（padding 16、间距 12）
 │   ├ 卡片头：连接设置 15/600 ＋ 右「修改后保存」11 text-3
 │   ├ 供应商预设（12 标签 ＋ 整宽下拉）
 │   ├ 供应商名称 ｜ API Key（两个等宽 flex_1 列）
 │   ├ 上游协议（12 标签；下拉 flex_1 ＋ 协议选项 Normal 按钮）
 │   ├ API 地址（12 标签；等宽字体输入 ＋ 11px 提示）
 │   ├ 模型列表（标题 ＋ 获取模型 Outline / ＋ / ×）
 │   ├ 模型表（播 DataTable）＋ 键盘提示 11px
 │   └ 底部操作行：思考强度 ＋ 200px 下拉 ｜ 右对齐 测试连接 Outline ＋ 保存 Primary
 └ 右栏 300（自身不滚动，只有日志卡内部滚动）
     ├ 全局设置卡：同步 WorkBuddy / 取消同步 ＋ 说明 ＋ 本地代理子卡
     │   （运行点＋可信状态、等宽代理地址、本地端口输入＋应用端口、供应商/模型/请求计数）
     └ 日志卡：标题＋清空/复制图标按钮 ＋ 独立滚动的等宽 11px 文本区
协议弹窗（宽 600，四种协议共用 protocol_options::Editor）
 ├ OpenAI Chat/Responses：API Key 认证 ＋ 思考模式 ＋ 思考强度映射
 ├ Anthropic：默认最大输出 Token ＋ Anthropic-Version 只读行 ＋ 思考模式 ＋ 映射
 ├ Gemini：思考模式 ＋ 映射
 └ 映射区：「思考强度映射」标题 ＋ 浅灰提示框 ＋ 等宽 84px 标签列 ＋ 输入
```

- 供应商预设/名称/API Key/URL/协议/模型表/思考强度/两个动作的相对顺序与设计一致；协议选项按钮为 Normal（白底）而非 Outline，与设计稿一致。
- 右栏由原「全局设置与日志」单卡拆为两张卡（差异 D14 落地），基准宽 300px（实测白底卡 298px：300 减 1px 边框，行 500 处白底区间 957–1254）。
- 日志区由卡片自身 `flex_1 min_h_0` 承载，`Textarea` 内部滚动；页面与日志互不影响（证据见下）。
- 底层逻辑未动：`confirm_protocol` 仍只写 `drafts.providers[id].protocol_settings`；`save`/`test`/`fetch`/`confirm_port`/`confirm_startup_sync`/`sync`/`unsync`/日志复制与清空仍是原回调。

**验收结果（P01—P03、D01—D03）**

| ID | 结果 | 依据 / 未做部分 |
|---|---|---|
| P01 | **PASS（保存写入与 IME NOT RUN）** | 真实 GUI：预设、名称/API Key、协议/选项、地址与提示、模型列表、思考强度、操作按钮按设计排列；API Key 显隐见 C02 证据；切换供应商往返后草稿保留（长 URL 113 字符读回逐字节一致）。**保存按钮未点击**：本轮约束禁止触发保存副作用，只核对入口与禁用规则（`save` 在任意弹窗打开时直接返回，按钮 `disabled(busy)`） |
| P02 | **入口 PASS / 实际调用 NOT RUN** | 获取模型为 Outline 按钮、测试连接为 Outline 且 `disabled(busy || models.is_empty())`，回调仍是原 `send("fetch"…)` / `test()`；本轮不点亮，因为会触发上游网络请求（约束禁止），未做 Mock 成功/失败/延迟矩阵 |
| P03 | **PASS（复制/清空未点击）** | 长 URL 完整输入并保留（见 P01）；端口 `99999` → 右栏错误“请输入 1–65535 的端口号”；端口 `52000` → 呈现“修改端口并重启 WorkBuddy？”确认弹窗（同意并重启 / 取消），**取消**后端口输入回填 `54163`、弹窗关闭、隔离核心仍在 54163 监听、`config.json` SHA-256 不变；清空/复制日志入口存在且保持原回调。复制清空本身未点击（约束禁止副作用动作） |
| D01 | **PASS** | 四种协议逐个打开：OpenAI Chat（API Key 认证 ＋ 原生 effort）、Responses（**含 `none` 映射行**，值 `none`，`max=xhigh`，无输出限制字段）、Anthropic（默认最大输出 Token `8192` ＋ `Anthropic-Version: 2023-06-01` ＋ 固定预算）、Gemini（仅思考模式 ＋ 思考档位，`high=high`）；同一 Editor 渲染，未丢既有设置 |
| D02 | **PASS** | 模式选择后预算仍非空且模式为“未配置” → “请选择思考模式或清空映射 / Select a thinking mode or clear the mappings”，弹窗**不关闭**、草稿保留；预算填 `abcXYZ` → “预算须为正整数 / Budget must be positive tokens”，弹窗不关闭；Anthropic 三种模式与 Gemini 三种模式的菜单项、说明文字与顺序与设计一致；非法输入未触发任何持久化 |
| D03 | **PASS** | 应用合法预算（low=2048）→ 弹窗关闭、`config.json` SHA-256 全程不变（`76e5689d…`，与初始值逐字节相同）、重开弹窗读到 `2048`、切供应商往返后仍为 `2048`；草稿按基线写入 `gpui-drafts.json`（`base` 指向当前配置哈希），**配置字段结构未改变**。正式保存未执行（约束禁止） |

**自动验证（环境：`TMPDIR/TMP/TEMP=/Volumes/1TB/tmp/ui-ux-t04/session-t04/tmp`；`CARGO_TARGET_DIR=/Volumes/1TB/Downloads/git/workbuddy-proxy/gpui/target`；未执行 `cargo clean`）**

| 检查 | 命令 | 退出码 | 结果 / 日志 |
|---|---|---:|---|
| 编译 | `cargo check --manifest-path gpui/Cargo.toml --locked --bin workbuddy-proxy-gpui` | 0 | PASS；`logs/check-1.log`、`logs/check-2.log` |
| 格式 | `rustfmt --edition 2024`（仅改动文件）＋ `cargo fmt --manifest-path gpui/Cargo.toml --check` | 0 | PASS；未对 `usage_table.rs` 运行 rustfmt |
| Clippy | `cargo clippy --manifest-path gpui/Cargo.toml --locked --all-targets -- -D warnings` | 0 | PASS；`logs/clippy.log`、最终源码复跑 `logs/clippy-final.log`；仅依赖 `block v0.1.6` 的 future-incompat 提示 |
| 测试 | `cargo test --manifest-path gpui/Cargo.toml --locked` | 0 | **138 passed / 0 failed**；`logs/test.log`、最终源码复跑 `logs/test-final.log` |
| Release 构建 | `cargo build --manifest-path gpui/Cargo.toml --release --locked` | 0 | PASS；`logs/build.log`、`logs/build2.log` |
| Git 空白 | `git diff --check` | 0 | PASS |
| 保护文件 | `shasum -a 256 gpui/src/usage_table.rs` | — | `ff14cd5c…a275f3`，与基线一致 |

- UI 二进制（含未提交改动）：`/Volumes/1TB/Downloads/git/workbuddy-proxy/gpui/target/release/workbuddy-proxy-gpui`，SHA-256 `58ca8fd5348a35d87ac363e658e491b779d34dbb8b19437a42c696daaa852d2d`（构建时间 2026-10-19，未提交，版本 0.5.6）。
- 核心二进制：`/Volumes/1TB/Downloads/git/workbuddy-proxy/target/release/workbuddy-proxy`，SHA-256 `24826a5233aaf2904052d554798b437072f84d441a4588c07449909e34b1242b`；worktree 中 `src/`、`native/`、`Cargo.toml`、`Cargo.lock` 与 `main` 无差异，故复用该 0.5.6 核心不会掩盖本次改动。

**真实 macOS GUI 证据（隔离夹具，非真实配置 / 非真实上游）**

夹具一（`gui/`）：合成 `config.json`（5 个供应商：openai_chat、长名称/长 URL/长模型 ID、openai_responses 含 `none` 映射、anthropic_messages 固定预算、gemini_generate_content 思考档位），隔离 `HOME`/`APPDATA`/`LOCALAPPDATA`/XDG/`TMPDIR`，核心 `--headless --config-dir … --port 54163` 仅监听 loopback，GPUI `--config-dir`；全程未启动真实 WorkBuddy、未读写真实配置/统计库、未触发开机启动/同步/端口变更/真实上游请求。

| 截图（`shots/`） | 场景 | 观察到的证据 |
|---|---|---|
| `T04-01-providers-1280x800.png` | 供应商页 1280×800 | 连接设置分区顺序与设计一致；右栏 300px 两卡分离；日志卡独占剩余高度 |
| `T04-02-providers-long-values-1280x800.png` | 长名称供应商 | 侧栏长名称省略号，表单内长名称/长 URL 完整输入，模型表长 ID 省略号 |
| `T04-03-providers-min-1100x700.png` | 最小窗口 1100×700 | 无布局挤出窗口；两卡均可达；表单自身滚动 |
| `T04-04-proto-openai-min-1100x700.png` | 最小窗口内协议弹窗 | 弹窗完整可见、内容可滚动、应用到草稿可达 |
| `T04-05-proto-menu.png` | 上游协议下拉展开 | 四项：OpenAI Chat Completions / OpenAI Responses / Anthropic Messages / Gemini GenerateContent，当前项带勾选 |
| `T04-06-proto-openai.png` | OpenAI Chat 弹窗 | API Key 认证 ＋ 思考模式 ＋ 映射（minimal…max，无 `none`） |
| `T04-07-proto-openai-authmenu.png` | 认证下拉展开 | 协议默认 / Protocol default（勾选）＋ Azure API Key |
| `T04-08-proto-responses.png` | Responses 弹窗 | **`none` 行存在且为 `none`**，`max=xhigh`，无输出限制字段 |
| `T04-09-proto-anthropic.png` | Anthropic 弹窗 | 默认最大输出 Token 8192、`Anthropic-Version: 2023-06-01` 只读行、固定预算、low=1024/high=8192 |
| `T04-10-proto-anthropic-modemenu.png` | Anthropic 模式菜单 | 未配置 / 固定预算 / 自适应 |
| `T04-11-proto-error-mode.png` | 未配置模式 + 非空映射 | 弹窗内红色错误“请选择思考模式或清空映射”，弹窗未关闭 |
| `T04-12-proto-error-budget.png` | 非法预算 | 输入 `abcXYZ` 保留在草稿，红色错误“预算须为正整数”，弹窗未关闭 |
| `T04-13-proto-gemini.png` | Gemini 弹窗 | 仅思考模式（思考档位）＋ high=high 映射 |
| `T04-14-proto-gemini-modemenu.png` | Gemini 模式菜单 | 未配置 / 固定预算 / 思考档位 |
| `T04-15-apikey-masked.png` / `T04-16-apikey-shown.png` | API Key 显隐 | 同一字段：掩码圆点＋眼睛图标 → 明文＋眼睛划除图标，值不变 |
| `T04-17-long-url-typed.png` / `T04-18-long-url-after-switch.png` | 长 URL | 等宽输入框完整持有 113 字符；切走再切回后 AX 读回逐字节一致 |
| `T04-19-port-invalid.png` | 端口 `99999` | 右栏错误“请输入 1–65535 的端口号”，无确认弹窗 |
| `T04-20-port-confirm.png` | 端口 `52000` | 确认弹窗“修改端口并重启 WorkBuddy？”＋ 同意并重启 / 取消（未执行同意） |
| `T04-21-port-cancelled.png` | 取消端口变更 | 弹窗关闭、端口回填 `54163`、核心仍在 54163 监听、配置未变 |
| `T04-22-log-long-tail.png` / `T04-23-log-scrolled.png` | 日志独立滚动 | 60 行合成日志；滚轮上滚后日志区内容变化（区域哈希不同），同帧表单区哈希不变 |

夹具二（`gui-log/`）：**本地 Mock 控制面**（`mock_core.py`）按真实 `/_control/action` 契约返回含 60 行合成日志的快照，用于在无真实代理流量的前提下验证日志区滚动；未启动真实核心、未发送任何请求。

- 采集方式：窗口级截图（`screencapture -l <CGWindowID>`），因此不受其他应用窗口遮挡影响；交互通过 macOS AX（`AXPress` / `AXFocused` / `AXValue`）与真实鼠标/滚轮事件（CGEvent）完成。
- 显示器在取证过程中由 5K HiDPI 切换为 1920×1080（1x）；最终一套证据全部为 1:1 逻辑像素，早期 2x 截图已删除，避免混用两种比例。
- 输入方式限制：本环境该窗口无法稳定保持键盘前台（`NSRunningApplication.activate` 与 System Events `set frontmost` 均不生效，改用 AX `AXFrontmost`），因此长 URL 的"完整输入"以 AX 取值路径复核；**真实键盘逐字输入在切换显示器之前已单独通过**（读回逐字节一致，见 `logs/long-url-readback.txt`），最终一套截图使用 AX 取值路径。

**未测项与剩余风险**

1. **保存/同步/获取模型/测试连接/复制/清空日志的真实点击未执行**（约束禁止副作用动作）；只核对了入口、禁用规则与回调不变，P02 的实际请求矩阵仍为 NOT RUN。
2. **中文输入法、Tab/Enter/Escape 键盘路径、剪贴板** 未做真实 GUI 交互（窗口前台不稳定）；`C02/C03/C05` 在 T02 已登记为 NOT RUN，T04 未改变结论。
3. **差异 D16（控件高度基准）仍未解决**：设计 §3 输入框 36 / 按钮 32 / 映射输入 34；实测按钮 32（本轮显式像素修正）✓，单行 Input/Select 为 26–28。根因是 Kit 用 `theme.font_size`（T01 设为 13）作为窗口 rem，`input_h(Size::Small)` = `h_6` = 1.5rem ≈ 19.5 加内边距/边框，且单行输入会忽略调用方 `.h()`；设计稿实测供应商预设选择器 37.5 逻辑像素高。该偏差影响所有页面（统计页同样受影响），不能在供应商页单独修，**必须在 T09/T10 前于共享层一次性解决**并复验全部页面。
4. **协议弹窗宽度**取 600（设计约 596.5，规范区间 520–660）；选择器的弹层宽度沿用 Kit 行为，未与设计逐像素比对。
5. **`gpui-drafts.json`** 仍由基线机制在草稿变化时写入（`tick` 的脏检查），这是"应用到草稿"的固有语义；`config.json` 不受影响。正式保存链路未在隔离环境点击验证。
6. Windows GUI、DPI、输入法、窗口生命周期：**BLOCKED**（无实机）；本记录不得被解读为 Windows 通过。
7. 侧栏底部“添加/复制/删除”在 1280×800 下第三个按钮右缘略被裁切，属 T03 骨架范围，本轮未改，登记给 T09/T10。

**清理与磁盘**

- 本轮专用目录 `/Volumes/1TB/tmp/ui-ux-t04/session-t04/`；构建/测试的 `TMPDIR/TMP/TEMP` 指向其下 `tmp/`，未在系统盘创建 Cargo 目录，未执行 `cargo clean`。
- 两套隔离夹具在本轮内停止并确认进程退出后，删除其合成配置、`instance.json`、`gpui-drafts.json`、`usage.sqlite3*`、锁文件、启动日志、Mock 脚本与隔离 HOME/XDG 目录。
- 保留：截图（`gui/shots/`）、采集与校验日志（`logs/`）、几何测量（`logs/geometry.txt`、`logs/design-measure.txt`）、夹具生成脚本（`gui/`、`tools/winctl.swift`）与设计裁切图（`design/`）。全部位于 `/Volumes/1TB`，未进入仓库。

| 任务 | 状态 | 依赖 | 备注 |
|---|---|---|---|
| T00 | DONE（静态） | — | B04 NOT RUN；不阻止 T01 |
| T01 主题、字体、视觉 token | DONE（代码/静态；V03/V04 NOT RUN） | T00 | `theme.rs` + `main.rs`；页面未迁移 |
| T02 公共组件与状态 | DONE（代码/静态；C01—C06 交互 NOT RUN） | T01 | `ui_components.rs`；调用点部分迁移 |
| T03 窗口骨架 | DONE（代码/静态；macOS GUI 骨架与交互证据；S01—S04 部分项 NOT RUN） | T02 | 224px 侧栏 / 36px 顶部行 / 30px 状态栏；D04 只登记，D13 已落地 |
| T04 供应商页 + 协议弹窗 | DONE（代码/静态；macOS GUI 代表切片；P02 实际调用与保存/复制/清空未执行） | T03 | 右栏拆两卡 300px；Responses `none` 保留；草稿/持久化边界未变；D16 仍未解决 |
| T05 模型选择/编辑/同步确认 | TODO | T04 | 键盘、草稿、平台禁用 |
| T06 统计总览/筛选/图表 | TODO | T04 | 保留统计语义、轮询策略 |
| T07 日志/聚合表格/分页 | TODO | T06；T00 已记录 usage_table | 在既有 diff 上做两行 Token/列宽 |
| T08 成本定价/采集存储/价格弹窗 | TODO | T07 | 价格快照、危险操作 |
| T09 双平台矩阵/回归 | TODO | T05—T08 | Windows 无环境则 BLOCKED |
| T10 交付审计 | TODO | 全部；完整通过依赖 T09 | 不提交/不推送 |

### 8.4 未执行与阻断

- **T00 基线**：`cargo fmt --check`、`cargo clippy`、`cargo test`、`cargo build`：**NOT RUN**。T00 只做静态核查，没有运行 `cargo clean`。
- **T01 更新**：上述 gpui 检查已实际执行并通过（§8.3）；V03/V04 GUI 仍 **NOT RUN**。
- **T02 更新**：`cargo check`/`fmt`/`clippy`/`test`/`release build`/`git diff --check` 已执行并通过（§8.3）；C01—C06 的 GUI/鼠标/键盘/IME 交互仍 **NOT RUN**。
- **T03 更新**：`cargo check`/`fmt --check`/`clippy --all-targets`/`test`（138 passed）/`release build` 已执行并通过；macOS 真实 GPUI 窗口已在隔离夹具中启动并取得骨架、切换与状态源截图（§8.3、§8.5）。S01/S02/S03/S04 的部分交互仍为 **NOT RUN**（详见 §8.5），不得据此声称四项全部通过。
- **T04 更新**：`cargo check`/`rustfmt`+`fmt --check`/`clippy --all-targets`/`test`（138 passed）/`release build`/`git diff --check` 已执行并通过（§8.3 T04）。macOS 真实 GPUI 隔离夹具已取得供应商页、最小窗口、四种协议弹窗、API Key 显隐、长 URL、端口校验与确认、日志独立滚动截图；`config.json` SHA-256 在应用到草稿/长 URL/端口流程中全程不变。
- **T04 未执行**：保存、同步、获取模型、测试连接、复制/清空日志的真实点击（约束禁止副作用动作，P02 实际调用 NOT RUN）；中文输入法与 Tab/Enter/Escape 键盘路径（窗口无法稳定保持键盘前台）。Windows GUI：**BLOCKED**。
- macOS GUI 截图/交互：**T03 已部分执行**（仅窗口骨架、供应商切换、恢复尺寸、状态源三态；语言下拉未打开、破坏性动作未触发）。
- Windows GUI、DPI、输入法、窗口生命周期：**BLOCKED**（当前环境没有 Windows 实机；不能以编译代替 GUI）。本机为 macOS，因此不宣称 Windows GUI 已验收。
- 字体探测：**部分失败/非结论**；CoreText 未命中的名称只作候选线索，不能作为 V03 通过证据，也不能据此宣称运行时不可用。
- Windows GUI、DPI、输入法、窗口生命周期：**BLOCKED**（当前环境没有 Windows 实机；不能以编译代替 GUI）。
- 真实 WorkBuddy 同步/重启、真实开机启动、真实端口变更、真实上游请求：**NOT RUN**，本阶段明确禁止。
- B04 隔离启动：**NOT RUN**（方案已记录，未执行）。
- 提交/推送：未执行。

## 9. 风险清单

1. **既有 diff 被覆盖**：`usage_table.rs` 的 22/5 行是他人工作；T07 与任何格式化操作都可能覆盖。每次动该文件前先记录哈希/diff，只做增量编辑。
2. **大文件内联样式与事件**：`ui.rs` 约 2700 行、`statistics.rs` 约 4938 行；容易在拆分布局时复制状态、重复订阅或丢失焦点。T02 只做视觉封装，事件/状态留在原位。
3. **主题 token 迁移不彻底**：旧蓝绿/散落色值会被遗漏；T01 先集中 token，T03—T08 逐页迁移，最后 V01 扫描。
4. **字体不可用/字形差异**：本次字体查询部分失败/非结论（macOS 未命中 Segoe/Consolas/SF Mono，不等于未安装）；Windows 未实机验证。GPUI 只接受单一 family，不能照搬 CSS 列表；必须运行时解析并回落，否则可能出现缺字或错误字体。
5. **等宽数字不生效**：`tnum` 是否被目标字体支持需实测；不能为了等宽数字改数值格式或截断值。
6. **业务语义被设计稿带偏**：来源“官方/供应商”与实际“内置/自定义”、9 个 KPI（含 WorkBuddy 点数）、11 列日志（来源/点数）、默认刷新 0 秒，都必须保留真实语义；不得删功能凑截图。
7. **平台能力被误改**：Windows 自动重启禁用、端口变更禁用、同步说明必须保留；不能为了“同步并重启”改 Windows 生命周期。
8. **标题栏 D04 未定**：Windows 控件由 Kit 绘制；T03 只登记未改（`main.rs` 零改动）；T10 前必须有用户结论，否则只能“部分完成/待验收”。
9. **状态栏数据源（T03 已收口）**：原“运行中”是硬编码 Tag；T03 已改为由 `poll` 真实 reply 推导的 `CoreState`，未知/不可达有独立文案与颜色，并以隔离核心 `SIGSTOP`/`SIGCONT` 反证。剩余风险：只反映“核心是否响应”，不反映上游/转发健康（§8.6）。
10. **长页面/小窗口**：1100×700 下协议弹窗、价格弹窗、日期面板、明细表、右栏日志都可能溢出；T03 只覆盖骨架与供应商列表，T04/T06/T07/T08 仍需逐项验证滚动与可达性。
11. **设计资料不完整**：无 HTML、无 Windows 旧截图、PDF 只是局部；不能据此推断未覆盖行为，也不能伪造 Windows 证据。
12. **旧截图版本错位**：v0.5.5 截图早于当前 HEAD 的部分功能；只能作参考，不能当当前基线或 Windows 基线。
13. **选择模型弹窗键盘能力有限**：主模型表的多选键盘不自动存在于 picker；T05 不能声称已覆盖未实现的键盘语义。
14. **请求日志两行 Token 尚未实现**：当前 `token_cell` 仍是一行五段；T07 要在保留既有比例槽位/最小宽度的基础上重构，不能回退。
15. **默认刷新间隔变更风险**：设计显示 5 秒、实际默认关闭；擅自改默认会改变轮询行为，需用户确认，T06 先登记 D15。
16. **T01 基础字号风险**：`Theme.font_size` 改为 13 会影响未显式设置字号的 Kit 控件；V03/V04 未运行前不能判定视觉通过。
17. **T01 token 提前定义**：`theme.rs` 使用模块级 `#![allow(dead_code)]`，为 T02—T08 预留 token；T10 前必须复核消费情况，避免掩盖页面问题。
18. **T01 运行时字体未观察**：`all_font_names()` 只在 GPUI 启动时执行；本阶段未启动 GUI，实际选中的 macOS family 和 Windows 候选均未取得证据。
19. **T02 封装覆盖不完整**：仍有 68 处 `Button::new`、13 处 `Input::new`、5 处 `Select::new`、4 处 `Checkbox::new` 直接调用；T03—T08 必须继续迁移，不能让两套写法长期并存。
20. **T02 字段高度变化**：供应商名称/Key/URL 统一为 36px token 高度；真实 GUI 视觉与 IME 未验证。
21. **T02 wrapper 预留代码**：`ui_components.rs` 的 `#![allow(dead_code)]` 会掩盖未消费封装；T10 前复核。

## 10. T01 的准确起点

> T01、T02 已按本节起点完成；以下保留为 T00 时的计划。T03 的起点见 §8.3 的 T02 记录。

前置检查（每个任务开始都做）：

```sh
git status --porcelain=v1
git rev-parse HEAD
shasum -a 256 gpui/src/usage_table.rs
```

预期仍是 `M gpui/src/usage_table.rs` + `?? docs/UI-UX-renew/`，哈希仍为 `ff14cd5c...`；若变化，先停止并查明归属。

T01 第一步：

1. 新建 `gpui/src/theme.rs`：
   - 颜色 token：完整覆盖 DESIGN_SPEC §1.1（primary/primary-dark/primary-tint/primary-border/primary-ring/hero-bg/text-1/text-2/text-3/border/border-input/divider/bg-page/bg-sidebar/bg-table-head/bg-seg/bg-disabled/danger/danger-bg/danger-border/warning/scrim；图表系列色）。
   - 尺寸 token：圆角、控件高、图标尺寸、间距、行高、卡片/弹窗尺寸、表格行高/表头高；业务值（Token 阈值、页大小、保留天数）不得混入。
   - 字体：运行时解析实际安装字体；无衬线优先系统字体，等宽按平台候选；提供 `tabular_figures()`。
   - `install(cx)`：在现有 `Theme::update` 机制上设置颜色、圆角、字体、字号；不创建平行主题状态。
2. 修改 `gpui/src/main.rs`：
   - 增加 `mod theme;`。
   - 将当前 `Theme::change(...)` + `Theme::update(...)`（约 332—356 行）改为调用 `theme::install(cx)`，保留 `ThemeMode::Light` 和所有窗口/键位/生命周期代码。
   - 不改窗口尺寸、标题栏选项、快捷键。
3. 本任务不迁移页面、不改 `usage_table.rs`、不改统计/代理/配置逻辑；页面样式在 T03—T08 逐步替换。
4. T01 完成后：
   - V01/V02 可用源码扫描与静态核对；V03/V04 必须等真实 GUI 运行，未运行就标 `NOT RUN`。
   - 若需要编译，遵守：`TMPDIR/TMP/TEMP` 指向 `/Volumes/1TB` 会话目录；`CARGO_TARGET_DIR=/Volumes/1TB/Downloads/git/workbuddy-proxy/gpui/target`；不执行 `cargo clean`；进程退出后清理。
5. T02 的入口是 `gpui/src/ui_components/`，只封装实际重复的 Button/Input/Select/Checkbox/Segmented/Card/Table/Dialog/Pager/Tag；不要先改 `ui.rs` 全部页面。

## 11. 静态核查命令与证据

本阶段实际执行的都是只读/临时检查，没有项目构建：

```sh
git branch --show-current
git rev-parse HEAD
git status --porcelain=v1
git diff --stat
git diff --numstat
git diff --check
git diff -- gpui/src/usage_table.rs
grep -n 'gpui-kit' gpui/Cargo.toml
grep -n -A6 'name = "gpui-kit"' gpui/Cargo.lock
sips -g pixelWidth -g pixelHeight docs/UI-UX-renew/*.png
```

设计 PDF 使用 PDFKit 渲染为临时 PNG 逐页查看；字体使用 CoreText 只读查询本机字体族；临时脚本/图片位于 `/Volumes/1TB/tmp/ui-ux-t00/session-t00/`，不属于交付物，任务结束后清理。

关键静态结论：

- HEAD `cbb3a919...`；branch `codex/ui-redesign`。
- `usage_table.rs` 当前哈希 `ff14cd5c...`；HEAD 版本 `03d7c794...`；差异 22/5；`git diff --check` 退出 0。
- Kit `=0.7.3`，Cargo.lock 0.7.3。
- 12 PNG 全部存在；PDF 12 页；无 HTML；无 Windows 截图；Mac 旧截图 v0.5.5。
- 字体探测：CoreText 返回 248 个 family；PingFang SC/Menlo/Monaco 等命中；Segoe UI/Consolas/SF Mono 未命中——该查询**部分失败/非结论**，未命中不等于未安装，T01 必须用 GPUI 运行时字体表复核。
- 未运行 `cargo`；未启动 GUI；未使用真实 WorkBuddy/配置/数据库/开机启动/上游。

## 附录 A：`usage_table.rs` 既有未提交 diff（基线，不可覆盖）

```diff
diff --git a/gpui/src/usage_table.rs b/gpui/src/usage_table.rs
index 3fe003d..84c69a4 100644
--- a/gpui/src/usage_table.rs
+++ b/gpui/src/usage_table.rs
@@ -415,9 +415,14 @@ fn cell(
         .into_any_element()
 }

-/// Keep labels visually secondary without making users decode glyphs. Each
-/// label/value pair stays together on one line; exact values remain in the tooltip.
+/// Fixed proportional slots align labels across rows regardless of value length.
+/// Keep pairs on one line; clipped values remain available in the tooltip.
 fn token_cell(item: &RequestRowDto, row: usize, zh: bool) -> AnyElement {
+    let weights = if zh {
+        [1.05, 0.95, 1.3, 1.4, 1.35]
+    } else {
+        [0.9, 1.0, 1.65, 1.15, 1.55]
+    };
     let fields = [
         (
             if zh { "输入" } else { "In" },
@@ -482,19 +487,31 @@ fn token_cell(item: &RequestRowDto, row: usize, zh: bool) -> AnyElement {
                 .map(|(index, (label, value))| {
                     div()
                         .flex()
+                        .flex_1()
+                        .flex_grow(weights[index])
+                        .min_w_0()
+                        .overflow_hidden()
                         .items_baseline()
                         .gap(px(2.))
-                        .flex_shrink_0()
                         .when(index > 0, |this| {
-                            this.child(Label::new("|").text_size(px(10.)).text_color(rgb(0xc4ceda)))
+                            this.child(
+                                Label::new("|")
+                                    .flex_shrink_0()
+                                    .text_size(px(10.))
+                                    .text_color(rgb(0xc4ceda)),
+                            )
                         })
                         .child(
                             Label::new(label)
+                                .flex_shrink_0()
                                 .text_size(px(10.))
                                 .text_color(rgb(0x6b7c90)),
                         )
                         .child(
                             Label::new(value)
+                                .flex_1()
+                                .min_w_0()
+                                .text_ellipsis()
                                 .text_size(px(12.))
                                 .text_color(rgb(0x19334d)),
                         )
@@ -506,7 +523,7 @@ fn token_cell(item: &RequestRowDto, row: usize, zh: bool) -> AnyElement {
 pub const REQUEST_COLUMNS: usize = 11;
 // Time is just wide enough for MM-DD HH:MM:SS at the table's 12px font.
 const MIN_WIDTHS: [f32; REQUEST_COLUMNS] =
-    [110., 100., 68., 112., 80., 340., 110., 66., 60., 102., 90.];
+    [110., 100., 68., 112., 80., 360., 110., 66., 60., 102., 90.];
 const MAX_AUTO_WIDTHS: [f32; REQUEST_COLUMNS] = [
     110., 100., 180., 350., 110., 460., 140., 82., 78., 130., 90.,
 ];
```

基线：文件 `ff14cd5c47ff9cc450b4b5ecfcfa6981a02797c2370eb3eab91a786377a275f3`；HEAD 版本 `03d7c7942a8ced5e9afe35b1a949e89a13f8422105a05c2898cdcbe9c6fe4657`。

## 附录 B：设计文件清单与尺寸

| 文件 | 像素尺寸（@2x） | 对应画面 |
|---|---|---|
| `数据统计@2x.png` | 2560×3080 | SC01 统计总览（长页） |
| `成本定价 · 采集与存储（展开）@2x.png` | 2560×2020 | SC08 定价/采集存储（长页） |
| `供应商管理@2x.png` | 2560×1600 | SC02 供应商管理 |
| `选择模型弹窗@2x.png` | 2560×1600 | SC03 选择模型 |
| `上游协议下拉@2x.png` | 2560×1600 | SC04 协议下拉 |
| `协议选项 · OpenAI@2x.png` | 2560×1600 | SC05 OpenAI 协议 |
| `协议选项 · Anthropic@2x.png` | 2560×1600 | SC06 Anthropic 协议 |
| `协议选项 · Gemini@2x.png` | 2560×1600 | SC07 Gemini 协议 |
| `新建价格规则@2x.png` | 2560×1600 | SC09 新建价格 |
| `编辑价格@2x.png` | 2560×1600 | SC10 编辑价格 |
| `编辑模型@2x.png` | 2560×1600 | SC11 编辑模型 |
| `同步 WorkBuddy 确认@2x.png` | 2560×1600 | SC12 同步确认 |
| `WorkBuddy Proxy 视觉重设计.pdf` | 12 页，页面 960×600；第 1 页 960×1155，第 8 页 960×757 | 画板局部裁切/放大页；无额外画面 |

材料完整性：12/12 PNG 存在；PDF 12 页存在；HTML 0；Windows 旧截图 0；Mac v0.5.5 截图 5 张；协议验证截图 2 张（Mac，1100×701）。

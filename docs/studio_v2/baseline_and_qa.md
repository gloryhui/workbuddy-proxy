# Studio V2 实施与验收记录

## 环境与边界（2026-10-28，北京时间）

- 原分支 `main` / `origin/main`：`e96f5a867946a63504b6ec7d9ce4a78762434e32`。fetch 后一致。
- 原目录的未跟踪 `output/` 保留；通过 managed worktree 从 `origin/main` 建立 `feat/studio_v2_gpui`。用 `git worktree list --porcelain` 查当前路径，不依赖特定机器路径。
- macOS 15.8.1 (24H32), Apple Silicon arm64；cargo 1.98.1、rustc 1.98.1、stable-aarch64-apple-darwin。依赖与锁文件未升级，gpui-kit 仍为 `=0.7.3`。
- 复用原 checkout 的两个 Cargo target 缓存；会话通过本地 `CARGO_TARGET_DIR` 指定。TMPDIR/TMP/TEMP 指向系统临时目录内本会话子目录；没有 cargo clean。
- 不变更核心热路径、协议、配置格式、SQLite、同步恢复和版本号。默认皮肤仍为黑白，六个旧枚举值、appearance.json version 1 保持。

## 改造前基线

| 检查 | 结果 |
| --- | --- |
| 核心 fmt / clippy / test / release | Passed；573 项单测 |
| GPUI fmt / clippy / test / release | Passed；155 项单测 |
| 既有提示 | 依赖 `block 0.1.6` future incompatibility warning；未放宽 lint |
| Release 核心 / GPUI 大小 | 8,428,096 / 14,761,312 bytes |
| 同机 loopback mock 基准 | [baseline-core.json](benchmarks/baseline-core.json)，200 小请求，64 模型，80 个 1 MiB 请求、4 并发 |
| 核心空闲 RSS | 13.11 MiB；采样峰值/负载后 61.78 MiB |
| 请求 p50 / p95 | 0.190 / 0.449 ms；本地 HTTP mock，不包含 TLS/真实模型 |
| UI 基线（21 秒单次采样） | 核心 13,392 KiB + UI 95,776 KiB = 109,168 KiB（106.61 MiB） |
| UI 基线启动 / 截图 | gpui.json 就绪 0.709 秒（不代表首次绘制）；[baseline](screenshots/baseline-1440x900.png) |

发现原 `scripts/benchmark.py --gui` 只读取启动进程和 instance.json PID；当前架构中这两个是托盘和核心，gpui.json 才是设置 UI，原脚本遗漏 UI。修正前不使用其 GUI 输出声称性能改善。

## 小功能 1：七项原生工具栏（Figma 103:442）

- `ui_components::system_button` 统一 Kit Button、20px SVG、36px 命中区、Tooltip/无障碍名称与禁用态；`app_controls` 严格按衣服、电源、刷新、地球、恢复、GitHub、反馈排列，间隔 8px，总宽 300px。
- 七个 SVG 来自官方 MCP 高保真 context，根尺寸/viewBox 均为 20×20，本地嵌入，不使用临时 Figma URL 作为运行资源。保留原皮肤弹层、开机启动 action、更新、语言菜单、尺寸恢复与打开 URL 的真实处理器。
- 修改文件：`gpui/src/{assets.rs,theme.rs,ui.rs,ui_components.rs,ui/skin_controls.rs}`；新增 `gpui/assets/studio/`。未改造其他页面结构，后续继续施工。
- 验证：GPUI fmt / clippy / 155 test / release build 全部 Passed。macOS 隔离 GUI：7/7 图标可见、AX 控件存在，六皮肤弹层/Esc、语言菜单/Tooltip、中英实际切换、缩小后恢复 1440×900 均通过。未操作真实登录项和外部浏览器链接；36px 仅由代码与截图核对，未测 AX 几何。Windows、Intel Mac、HiDPI/键盘逐项验证未运行。
- [工具栏截图](screenshots/toolbar-1440x900.png)：原始窗口捕获 1440×901，PNG 转换仅裁去底部 1 行 macOS 窗口框，内容没有重绘。两图均为隔离合成 Review 账户、空记录。核心有内核沙盒；GUI 使用隔离 HOME/配置，受保护 GUI 无法由 CUA 绑定，截图轮沿用仓库默认 GUI 启动方式。
- 工具栏后单次 RSS（44 秒）：核心 13,616 KiB + UI 99,488 KiB = 113,104 KiB（110.45 MiB）。与基线采样时刻不同，不据此宣称内存改善/退化。启动至 gpui.json 为 0.812 秒；启动至首次 AX 返回上界 4.958 秒，包含观察间隔。最终由修正后的同口径脚本复测。
- UI 子代理已结束本轮全部进程，清理会话专属测试目录（94,192 KiB），交付截图保留，临时文件剩余 0。系统盘剩余约 39 GiB。此轮通过终止测试进程退出，尚未验收用户关闭窗口后核心继续运行。

## 后续范围

继续按功能拆分：修正 GUI 性能测量；Studio 语义 Tokens 与主布局；供应商搜索、容量禁用、固定操作栏与草稿保护；主编辑区/四协议；统计页面与全部弹窗。最终逐画板、六皮肤、两种窗口尺寸、生命周期和跨平台验证。不能把本次工具栏的小功能视为整个 V2 完成。

## 小功能 2：修正独立 GPUI 进程的基准测量

- `scripts/benchmark.py` 的 GUI 模式等待并读取 `gpui.json`，只将真实核心与设置 UI 的 RSS 相加；托盘单列。UI 未启动或记录失效会拒绝报告 GUI 基准。增加核心健康端点与 UI 记录就绪时间，明确后者不是首次绘制/可交互时刻。
- 结束时停止采样线程、发送 UI 停止标记、等待 UI 记录释放，然后退出会话临时目录。`scripts/benchmark_test.py` 的 4 项回归覆盖核心/托盘/UI 分离、有效/非法/已退出的 UI 记录，全部 Passed；headless 完整基准 Passed。
- 两轮 GUI 基准均继承验证过的内核沙盒，隔离 HOME 与配置；均为同机 release、64 模型、200 小请求、80 大请求、4 并发，无构建并行。两轮进程全部退出，清理本轮目录 24,752 KiB；临时文件剩余 0，系统盘约剩 39 GiB。

| 同口径单次测量 | 基线 | 工具栏 c505d22 |
| --- | --- | --- |
| 核心 RSS / UI RSS (MiB) | 13.88 / 75.27 | 13.55 / 74.16 |
| 两者合计 / 托盘 (MiB) | 89.14 / 26.38 | 87.70 / 26.39 |
| 采样峰值两者合计 (MiB) | 133.39 | 133.08 |
| 核心就绪 / UI 记录就绪 (ms) | 45.924 / 796.346 | 345.674 / 1142.557 |
| 代理请求 p50 / p95 (ms) | 0.177 / 0.209 | 0.179 / 0.221 |

原始报告：[baseline-gui.json](benchmarks/baseline-gui.json)、[toolbar-gui.json](benchmarks/toolbar-gui.json)、[corrected headless](benchmarks/baseline-core-corrected.json)。单次采样有调度与缓存差异，不能据此声称性能趋势；RSS 相加可能重复计算共享页。启动耗时也需后续多轮复测。

## 小功能 3：Studio 原生窗口框架与共享 Tokens

- `theme.rs` 接入官方 V2 变量：深色侧栏、画布/正文/边线/成功状态；256px 侧栏、32px 内容边距、28px 工作区状态栏、36px 按钮、40px 输入/选择、43px 导航、38px 供应商行。原六皮肤与默认黑白不变；没有迁移已有外观文件。
- 七项工具栏进入唯一页面标题区，统计标题随概览/明细/分析/设置视图变化。原生 TitleBar 保留拖动与系统窗口控制，24px 区域视觉上与两侧背景相连，没有独立白色工具顶栏或面包屑。状态栏移至工作区底部，不跨越深色侧栏。
- 使用现有 Kit `ButtonCustomVariant` 为深色导航指定正常/悬停/按下颜色。真实 UI 测试发现普通 builder `.hover` 被 Kit 的 variant 样式覆盖；最初修补未生效，随后按 0.7.3 源码采用 custom variant，再次实测通过，未修改依赖。
- GPUI fmt / 155 tests / clippy / release 全部 Passed。macOS 1440×900 与 1100×700、中英、统计视图切换、六皮肤弹层与 Blue/Mono 保存、缩小再恢复尺寸均实测通过。小窗口供应商表单需要在卡片内滚动取回模型操作；工具栏与标题无重叠。导航与新增/复制 hover 对比已复验通过。
- 原生拖动已操作，但工具没有返回屏幕坐标，尚不能断言位移验证通过；Windows 系统按钮、全屏、HiDPI/IME/键盘仍待验。
- [1440×900 明细](screenshots/shell-1440x900.png)、[1100×700 供应商](screenshots/shell-1100x700.png)。两图来自最终 custom variant 修复版（SHA256 前缀 `2279bfb0`），各自原始窗口高出 1 点，仅裁去底部窗口框，没有重绘。
- 本批只完成原生框架。供应商搜索、完整侧栏导航/本地状态卡、两块式编辑区、统计筛选重排与各弹窗视觉仍未完成。
- [shell-gui.json](benchmarks/shell-gui.json)：同口径单次核心/UI/托盘空闲 RSS 为 13.80/74.50/26.92 MiB，两者合计 88.30 MiB，峰值 132.55 MiB；核心/UI 记录就绪为 230.179/932.980 ms。没有加入新计时器或持续重绘，不从单次波动推断性能改善。
- 本轮全部隔离进程已退出并等待，测试配置、数据库、程序副本和原始截图清理约 139,360 KiB；修复前问题截图暂留 184 KiB，最终截图/JSON 已保存在仓库。系统盘剩余约 39 GiB。

## 小功能 4：本地供应商搜索与容量保护

- `provider_filter.rs` 仅过滤可见供应商名称，大小写不敏感、裁去查询首尾空白，尊重已保留的名称草稿和原排序，不搜索 Key/URL/内部 ID；不发送上游请求、写数据库或添加定时器。InputState 与模型搜索状态独立，筛选不改变当前编辑项。
- 侧栏增加搜索、清空、匹配 n/总数、空列表/无结果提示；达到核心定义的 64 个供应商上限时禁用新增/复制。保持原独立滚动列表和固定操作栏；新增/复制成功后清空搜索以显示新草稿。禁用操作同时降低透明度，避免自定义前景使禁用态看起来可用。
- 设计参照 `54:2`、`83:454/455`；补充读取搜索状态的真实高保真子树 `79:438`（父 `79:400`）、`80:456`（父 `80:418`）及截图。搜索 SVG 为导出的 17px 资源，输入高 38px。**暂保留浅色 Kit 搜索外观**以保证六套皮肤的文字和插入光标可读；最终深色搜索、完整三导航及侧栏本地代理卡仍待施工，不能视为一比一完成。
- GPUI fmt / 158 tests / clippy / release Passed；新增 3 项测试覆盖 0/1/2/20/64、大小写/中文/草稿名称与不搜索敏感字段。UI 已实测 0/1/2/20/64：空列表新增、末尾筛选、清空/无结果、匹配计数、64 条滚动后操作栏固定、满载新增/复制不执行。
- 实测搜索过滤不切换当前编辑项，未保存名称切换后保留；正常关闭窗口后原 GPUI PID 退出、gpui.json 移除，核心仍存活且 `/health` 为 200 ready。重新打开同一隔离配置后，未保存名称仍能搜索和恢复。隔离 mock 在最终搜索操作期间记录的上游请求数为 0。
- [2 条 / 1440×900](screenshots/provider-search-2-1440x900.png)、[64 条 / 1100×700](screenshots/provider-search-64-1100x700.png)，均为合成配置截图，原窗口高出 1 点，仅裁去底部窗口框。两条截图及性能报告采于禁用透明度修正前，该修正不改变正常态和查询逻辑；64 条截图随后更新为透明度修正版。
- [provider-search-gui.json](benchmarks/provider-search-gui.json)：同口径 1 供应商/64 模型（不同于 64 供应商 UI 验收），核心/UI/托盘空闲 RSS 13.41/72.98/27.05 MiB，两者合计 86.39 MiB、峰值 132.78 MiB；核心/UI 记录就绪 214.477/709.523 ms，请求 p50/p95 0.179/0.210 ms。单次数据不代表性能趋势，就绪标记不代表首帧/可交互。
- Release 核心 / 设置程序：8,428,096 / 14,761,328 bytes。核心没有改动。Windows/Intel Mac/HiDPI 与完整键盘、IME、四协议和统计逐画板视觉验收仍待进行。
- 本轮主 GUI 验收进程全部退出并等待，测试配置/数据库/副本清理约 195,520 KiB，临时文件剩余 0；最后禁用透明度截图复验的清理另行记录。
- 透明度修正版（SHA256 前缀 `d27f9fb0`）实测满载新增/复制明确变灰，删除保持可用色，64 条最终图已覆盖。额外测试进程均退出并等待，清理约 38,172 KiB。

## 本轮收尾与下一步

- 正常提交、按小功能推送到 `feat/studio_v2_gpui`，没有更改 main、版本号或创建 Release。原目录的 `output/` 未触碰。
- 子代理与主代理均确认测试子进程退出，随后删除主会话基线程序副本、构建/测试日志及专属临时目录（22,832 KiB），删除已解决 hover 问题的两张临时证据及空目录（184 KiB）。截图、基准 JSON 和必要文字证据已保存在本目录；兼容 Cargo 缓存继续复用，未删除。
- 全部本轮隔离测试/基准/构建临时清理累计约 515,012 KiB（503 MiB），本会话临时文件剩余 0 bytes。系统临时目录另有 2 项项目相关名称的历史内容，归属未确认，保留不动。系统盘当前剩余 39.29 GiB。
- Goal 仍 active，尚未完成完整 V2。下一批继续完整侧栏的第三导航/本地状态卡及深色搜索、当前供应商摘要和双卡片编辑区，再推进协议/模型弹窗、统计页面和全部状态的视觉/键盘回归。不得把首批 4 个小功能的通过当成全部 40 张画板验收完成。

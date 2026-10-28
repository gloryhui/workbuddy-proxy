# Studio V2 设计映射

官方 Figma MCP 于 2026-10-28 读取文件 `zxDfIQ4lhNcTXrFT7qk8QZ`、页面 `51:2`（WorkBuddy Proxy · Studio V2 · Rust GPUI）。以下 40 张 1440×900 状态画板直接来自 page metadata；未验收表示尚未逐画板进行运行与视觉对照。Rust 落点均相对于 `gpui/src/`。

| Frame | 设计状态 | Rust 落点 | 验收 |
| --- | --- | --- | --- |
| 52:2 | 01 / STUDIO V2 · 使用概览 | statistics/overview.rs, layout.rs, source_controls.rs, usage_chart.rs | 部分：导航/筛选、四KPI/详细指标/空值已验；四KPI/趋势/计费/最近请求已验，整帧最终审计待续 |
| 54:2 | 02 / STUDIO V2 · 供应商管理 | ui/sidebar.rs, ui/providers.rs, ui/relay.rs, state.rs, model_table.rs | 部分：侧栏/双卡/运行中心/日志交互通过；完整视觉与无障碍待续 |
| 56:8 | 03 / STUDIO V2 · 用量明细 | statistics/*, usage_table.rs, usage_chart.rs | 部分：导航/筛选已验，报表内容与完整视觉待续 |
| 57:8 | 04 / STUDIO V2 · 汇总分析 | statistics/*, usage_table.rs, usage_chart.rs | 部分：导航/筛选已验，报表内容与完整视觉待续 |
| 58:8 | 05 / STUDIO V2 · 统计设置 | statistics/*, usage_table.rs, usage_chart.rs | 部分：导航/筛选已验，报表内容与完整视觉待续 |
| 59:11 | 06 / STUDIO V2 · 选择模型 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 59:279 | 07 / STUDIO V2 · 编辑模型能力 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 60:27 | 08 / STUDIO V2 · 同步 WorkBuddy | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 60:249 | 09 / STUDIO V2 · 新建价格规则 | statistics/*, usage_table.rs, usage_chart.rs | 未验收 |
| 60:475 | 10 / STUDIO V2 · 请求详情 | statistics.rs, detail_metadata, usage_table.rs | 部分：最近请求入口/正确ID/Esc已验，详情整帧视觉待续 |
| 61:289 | 11 / STUDIO V2 · 协议选项 · OpenAI Chat | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 61:561 | 11 / STUDIO V2 · 协议选项 · OpenAI Responses | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 61:838 | 11 / STUDIO V2 · 协议选项 · Anthropic | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 61:1111 | 11 / STUDIO V2 · 协议选项 · Gemini | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 62:308 | 13 / STUDIO V2 · 黑白主题示例 | ui.rs, ui/skin_controls.rs, theme.rs | 未验收 |
| 64:335 | 14 / STUDIO V2 · 外观与主题 | ui/skin_controls.rs, theme/skin.rs, ui_components.rs | 部分：旧六主题+可选 Studio、保存/恢复/深色选区通过；完整视觉待续 |
| 66:320 | 17 / STUDIO V2 · 上游协议下拉 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 66:538 | 18 / STUDIO V2 · 供应商预设搜索 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 66:760 | 19 / STUDIO V2 · API Key 显示演示 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 67:338 | 20 / STUDIO V2 · OpenCode Go 供应商 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 67:548 | 21 / STUDIO V2 · 思考强度菜单 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 68:350 | 22 / STUDIO V2 · 取消同步确认 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 69:359 | 23 / STUDIO V2 · 编辑模型价格 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 78:364 | 24 / STUDIO V2 · 新增供应商草稿 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 78:605 | 25 / STUDIO V2 · 编辑供应商资料 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 78:843 | 26 / STUDIO V2 · 复制供应商草稿 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 78:1080 | 27 / STUDIO V2 · 删除供应商确认 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 78:1325 | 28 / STUDIO V2 · 供应商满载滚动 (64) | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 78:1804 | 29 / STUDIO V2 · 删除后仅剩 OpenCode Go | statistics/*, usage_table.rs, usage_chart.rs | 未验收 |
| 79:400 | 30 / STUDIO V2 · 64 个供应商搜索 | ui.rs, provider_filter.rs | 部分：本地过滤/容量通过，最终侧栏视觉待续 |
| 79:885 | 31 / STUDIO V2 · 新供应商保存完成 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 79:1102 | 32 / STUDIO V2 · 供应商复制完成 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 80:418 | 33 / STUDIO V2 · 供应商搜索（2个） | ui.rs, provider_filter.rs | 部分：过滤/草稿保护通过，最终侧栏视觉待续 |
| 80:646 | 34 / STUDIO V2 · 编辑 OpenCode Go | statistics/*, usage_table.rs, usage_chart.rs | 未验收 |
| 80:864 | 35 / STUDIO V2 · 删除 OpenCode Go 确认 | statistics/*, usage_table.rs, usage_chart.rs | 未验收 |
| 80:1114 | 36 / STUDIO V2 · 删除 OpenCode Go 后剩 sub2api | statistics/*, usage_table.rs, usage_chart.rs | 未验收 |
| 104:1378 | 37 / STUDIO V2 · 开机启动已开启（原型） | ui.rs, ui/skin_controls.rs, theme.rs | 未验收 |
| 104:1605 | 38 / STUDIO V2 · 检查更新交互（原型） | ui.rs, ui/skin_controls.rs, theme.rs | 未验收 |
| 104:1823 | 39 / STUDIO V2 · 语言切换菜单 | ui.rs, ui/skin_controls.rs, theme.rs | 未验收 |
| 104:2046 | 40 / STUDIO V2 · 恢复默认窗口尺寸（原型） | ui.rs, ui/skin_controls.rs, theme.rs | 未验收 |

共享组件：`103:442` 七项工具栏 → `ui.rs::app_controls`、`ui_components.rs`；`83:454` 供应商行、`83:455` 固定操作栏 → `ui.rs::sidebar`；`71:364` 原生实施契约 → `theme.rs`、本目录记录。

首批已读取高保真 design context 和渲染：`103:442`、`71:364`、`54:2`。`54:2` 变量已读取，包括 sidebar `#111525`、canvas `#F7F8FC`、ink `#161C2C`、accent `#655CF4`、success `#11976F`。工具栏自身未单独绑定变量（variable defs 返回空对象），尺寸来自共享组件 context。后续阶段按需读取对应画板，不能把 metadata 清单视为已实现。

右栏高保真 context `54:298` / `54:320` 已读取：`ui/relay.rs` 运行中心与日志、`ui_components::TerminalLogs` 的本地深色原生编辑器投影。两尺寸/计数/复制/日志滚动/端口取消的真实回归见 `baseline_and_qa.md`。

外观高保真 context `64:335` 已读取，并沿用 V2 variables 新增显式可选的 Studio Indigo；六套旧键、默认 Mono 及 appearance version 1 保持。Kit Textarea 的延迟 Input 覆盖选区已通过真实补验定位、修复，见实施记录小功能 8。

统计导航/筛选高保真 `52:79` / `52:87` 已实现并做两尺寸回归：原生tabs、主筛选/完整来源组、更多有界浮层；设置页不再显示报表筛选。概览KPI/图表/计费、明细/分析内容和完整视觉仍待续，不能以控件回归替代整帧验收。

概览高保真 context `52:107` / `52:151` / `52:179` / `52:191` 已读取。当前完成四KPI与保留七指标的原生详细浮层；主卡柱形取真实最近14桶，不使用设计静态柱高冒充数据。趋势/费用面板和最近请求内容仍待施工。

趋势/计费 `52:151` / `52:179` 已原生落地到 `statistics/trends.rs`：保留真实UsagePlot/四图例/缺失单点，计费摘要区分缺失币种和真实零值，费用曲线保留在原生浮层。两尺寸/币种/图例/Esc已验，最近请求仍待落地。

最近请求 `52:191` 已落地 `statistics/recent.rs`，独立读取API允许的最小首批10条并仅展示第一条，不用分页缓存；同核心22条合成记录完成最新ID/详情/来源过滤/空态/小窗回归。概览主要结构已完成，整帧多主题/异常/键盘审计仍保留为待办。

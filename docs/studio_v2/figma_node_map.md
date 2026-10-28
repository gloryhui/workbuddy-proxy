# Studio V2 设计映射

官方 Figma MCP 于 2026-10-28 读取文件 `zxDfIQ4lhNcTXrFT7qk8QZ`、页面 `51:2`（WorkBuddy Proxy · Studio V2 · Rust GPUI）。以下 40 张 1440×900 状态画板直接来自 page metadata；未验收表示尚未逐画板进行运行与视觉对照。Rust 落点均相对于 `gpui/src/`。

| Frame | 设计状态 | Rust 落点 | 验收 |
| --- | --- | --- | --- |
| 52:2 | 01 / STUDIO V2 · 使用概览 | statistics/overview.rs, layout.rs, source_controls.rs, usage_chart.rs | 部分：导航/筛选、四KPI/详细指标/空值已验；四KPI/趋势/计费/最近请求已验，整帧最终审计待续 |
| 54:2 | 02 / STUDIO V2 · 供应商管理 | ui/sidebar.rs, ui/providers.rs, ui/relay.rs, state.rs, model_table.rs | 部分：侧栏/双卡/运行中心/日志交互通过；完整视觉与无障碍待续 |
| 56:8 | 03 / STUDIO V2 · 用量明细 | statistics/*, usage_table.rs | 部分：八列/来源筛选/分页/详情/窄窗首末列已验，整帧最终审计待续 |
| 57:8 | 04 / STUDIO V2 · 汇总分析 | statistics/analysis.rs, layout.rs, usage_table.rs | 部分：表卡/两指标卡/真实排序/分组分页/两尺寸已验，完整视觉与无障碍待续 |
| 58:8 | 05 / STUDIO V2 · 统计设置 | statistics/settings_layout.rs, settings_capture.rs, layout.rs, import_controls.rs | 部分：分区导航/价格卡CRUD与游标/快照/失败提示、采集/导入双卡及主要业务已验；整帧视觉、完整键盘/IME/无障碍待续 |
| 59:11 | 06 / STUDIO V2 · 选择模型 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 59:279 | 07 / STUDIO V2 · 编辑模型能力 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 60:27 | 08 / STUDIO V2 · 同步 WorkBuddy | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 60:249 | 09 / STUDIO V2 · 新建价格规则 | statistics/price_layout.rs, stats_controller.rs, stats_backend.rs | 部分：原生574×625布局/两尺寸/四费率与备注/币种模式/取消与Enter/焦点自动滚动/校验已验；完整IME/无障碍/跨平台与精细视觉待续 |
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
| 69:359 | 23 / STUDIO V2 · 编辑模型价格 | statistics/price_layout.rs, stats_controller.rs, stats_backend.rs | 部分：原生编辑布局/身份锁定/取消恢复/版本冲突及手动刷新重开/旧快照金额保护已验；完整IME/无障碍/跨平台与精细视觉待续 |
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

明细表高保真 `56:209` 与分析整帧 `57:8` 已读取。明细按V2八列实现：时间、供应商、模型、Token使用量、缓存命中、费用/积分、耗时、状态；请求ID保留在时间提示和原详情，tok/s保留在耗时提示及原详情。时间排序是核心真实的最新优先说明，没有添加空菜单。分析表本批只补原生首/末列访问，六列聚合和原排序/分页保持，整帧内容仍待后续。

汇总分析 `57:8` 的表卡 `57:225`、Token构成 `57:275` 与缓存/覆盖 `57:287` 已原生落地到 `statistics/analysis.rs`；真实分组/排序/服务端分页保持。表体43px行、38px表头，底部两卡用既有dashboard字段，窄窗正文独立滚动与纵排。没有使用稿中的静态509万、86.3%或100%冒充真实数据。

设置高保真 `58:223` / `58:272` / `58:301` 本轮已读取。三项导航已以原生ScrollAnchor接入；价格面板优先落地到 `statistics/settings_layout.rs`，保留精确搜索、20/50/100游标分页和原生CRUD/更新菜单，新增独立价格同步失败反馈。采集/存储与本地导入当前仍是原控件组，后续拆双卡；价格模态框 `60:249` / `69:359` 的完整视觉也未在本批施工。上述阶段记录为历史进度，当前范围以主表及最新实施记录为准。

采集/存储 `58:272` 与导入 `58:301` 已拆为原生双卡，窄窗纵排；真实设置/模式、21供应商有界分页/内层滚动、六计数与checkpoint、增量/完整核对及全局截止清理取消/确认已回归。旧“仍为原控件组”段落保留作阶段历史，当前范围以主表和最新QA记录为准；整帧及价格模态框视觉仍待续。

价格模态框 `60:249` / `69:359` 已落地原生 `statistics/price_layout.rs`：42px字段、四价格2×2、独立表单滚动、固定取消/保存/错误底栏，保留备注与旧修订确认。已验两尺寸、中英文/Studio及Mono、键盘字段自动滚入视野与版本冲突恢复；此前“视觉待施工”是历史进度，精细视觉/IME/无障碍及跨平台仍待最终审计。

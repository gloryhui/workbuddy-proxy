# Studio V2 设计映射

官方 Figma MCP 于 2026-10-28 读取文件 `zxDfIQ4lhNcTXrFT7qk8QZ`、页面 `51:2`（WorkBuddy Proxy · Studio V2 · Rust GPUI）。以下 40 张 1440×900 状态画板直接来自 page metadata；未验收表示尚未逐画板进行运行与视觉对照。Rust 落点均相对于 `gpui/src/`。

| Frame | 设计状态 | Rust 落点 | 验收 |
| --- | --- | --- | --- |
| 52:2 | 01 / STUDIO V2 · 使用概览 | statistics/overview.rs, layout.rs, source_controls.rs, usage_chart.rs | 部分：导航/筛选、四KPI/详细指标/空值已验；四KPI/趋势/计费/最近请求已验，整帧最终审计待续 |
| 54:2 | 02 / STUDIO V2 · 供应商管理 | ui/sidebar.rs, ui/providers.rs, ui/relay.rs, state.rs, model_table.rs | 部分：侧栏/双卡/运行中心/日志交互通过；完整视觉与无障碍待续 |
| 56:8 | 03 / STUDIO V2 · 用量明细 | statistics/*, usage_table.rs | 部分：八列/来源筛选/分页/详情/窄窗首末列已验，整帧最终审计待续 |
| 57:8 | 04 / STUDIO V2 · 汇总分析 | statistics/analysis.rs, layout.rs, usage_table.rs | 部分：表卡/两指标卡/真实排序/分组分页/两尺寸已验，完整视觉与无障碍待续 |
| 58:8 | 05 / STUDIO V2 · 统计设置 | statistics/settings_layout.rs, settings_capture.rs, layout.rs, import_controls.rs | 部分：分区导航/价格卡CRUD与游标/快照/失败提示、采集/导入双卡及主要业务已验；整帧视觉、完整键盘/IME/无障碍待续 |
| 59:11 | 06 / STUDIO V2 · 选择模型 | ui/model_picker.rs, ui.rs, ui/providers.rs | 部分：原生726×672/小窗自适应、38/4096虚拟候选、过滤与勾选/键盘、取消/确认草稿/显式保存、跨供应商256边界与错误/空态已验；完整IME/无障碍/跨平台待续 |
| 59:279 | 07 / STUDIO V2 · 编辑模型能力 | ui/model_editor.rs, model_options.rs, ui.rs | 部分：原生728×800/两尺寸、自动/手动/真实来源、六能力/五档与默认Select、四限制校验/草稿保存、取消与Popover、Tab/焦点及resize/错误后恢复已验；完整IME/无障碍/跨平台及精细审计待续 |
| 60:27 | 08 / STUDIO V2 · 同步 WorkBuddy | ui/sync_dialog.rs, ui.rs, backend.rs, workbuddy_restart.rs | 部分：526原生确认/两尺寸、取消/无隐式Enter、仅同步/当前草稿与全部saved范围、非法文件/重试、守卫下写入后重启失败反馈已验；真实WorkBuddy重启与完整busy/无障碍/跨平台未验 |
| 60:249 | 09 / STUDIO V2 · 新建价格规则 | statistics/price_layout.rs, stats_controller.rs, stats_backend.rs | 部分：原生574×625布局/两尺寸/四费率与备注/币种模式/取消与Enter/焦点自动滚动/校验已验；完整IME/无障碍/跨平台与精细视觉待续 |
| 60:475 | 10 / STUDIO V2 · 请求详情 | statistics/detail_layout.rs, detail_metadata.rs, usage_table.rs | 部分：650×770原生分组/两尺寸/真实代理与导入/完整元数据换行/本次快照/错误重试/各关闭路径已验；Tooltip视觉、完整无障碍/跨平台与精细审计待续 |
| 61:289 | 11 / STUDIO V2 · 协议选项 · OpenAI Chat | ui/protocol_dialog.rs, protocol_options.rs, ui/providers.rs, ui.rs, main.rs | 部分：620×724原生四tab/两尺寸、认证Azure/各协议真实字段与映射/8192输出、临时输入往返、Apply仅草稿/显式保存、标签Enter/Space不误应用、Tab/ShiftTab/缩窗、X/Esc及菜单优先Esc/标题空白Esc已验；完整IME/无障碍/跨平台待续 |
| 61:561 | 11 / STUDIO V2 · 协议选项 · OpenAI Responses | ui/protocol_dialog.rs, protocol_options.rs, ui/providers.rs, ui.rs, main.rs | 部分：620×724原生四tab/两尺寸、认证Azure/各协议真实字段与映射/8192输出、临时输入往返、Apply仅草稿/显式保存、标签Enter/Space不误应用、Tab/ShiftTab/缩窗、X/Esc及菜单优先Esc/标题空白Esc已验；完整IME/无障碍/跨平台待续 |
| 61:838 | 11 / STUDIO V2 · 协议选项 · Anthropic | ui/protocol_dialog.rs, protocol_options.rs, ui/providers.rs, ui.rs, main.rs | 部分：620×724原生四tab/两尺寸、认证Azure/各协议真实字段与映射/8192输出、临时输入往返、Apply仅草稿/显式保存、标签Enter/Space不误应用、Tab/ShiftTab/缩窗、X/Esc及菜单优先Esc/标题空白Esc已验；完整IME/无障碍/跨平台待续 |
| 61:1111 | 11 / STUDIO V2 · 协议选项 · Gemini | ui/protocol_dialog.rs, protocol_options.rs, ui/providers.rs, ui.rs, main.rs | 部分：620×724原生四tab/两尺寸、认证Azure/各协议真实字段与映射/8192输出、临时输入往返、Apply仅草稿/显式保存、标签Enter/Space不误应用、Tab/ShiftTab/缩窗、X/Esc及菜单优先Esc/标题空白Esc已验；完整IME/无障碍/跨平台待续 |
| 62:308 | 13 / STUDIO V2 · 黑白主题示例 | ui.rs, ui/skin_controls.rs, theme.rs | 未验收 |
| 64:335 | 14 / STUDIO V2 · 外观与主题 | ui/skin_controls.rs, theme/skin.rs, ui_components.rs | 部分：旧六主题+可选 Studio、保存/恢复/深色选区通过；完整视觉待续 |
| 66:320 | 17 / STUDIO V2 · 上游协议下拉 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 66:538 | 18 / STUDIO V2 · 供应商预设搜索 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 66:760 | 19 / STUDIO V2 · API Key 显示演示 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 67:338 | 20 / STUDIO V2 · OpenCode Go 供应商 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 67:548 | 21 / STUDIO V2 · 思考强度菜单 | ui.rs, state.rs, model_options.rs, protocol_options.rs | 未验收 |
| 68:350 | 22 / STUDIO V2 · 取消同步确认 | ui/sync_dialog.rs, ui.rs, backend.rs, workbuddy_restart.rs | 部分：原生确认/两尺寸、取消/仅移除、保留providers与草稿/恢复原条目、守卫下移除后重启失败反馈已验；真实WorkBuddy重启与完整busy/无障碍/跨平台未验 |
| 69:359 | 23 / STUDIO V2 · 编辑模型价格 | statistics/price_layout.rs, stats_controller.rs, stats_backend.rs | 部分：原生编辑布局/身份锁定/取消恢复/版本冲突及手动刷新重开/旧快照金额保护已验；完整IME/无障碍/跨平台与精细视觉待续 |
| 78:364 | 24 / STUDIO V2 · 新增供应商草稿 | ui/provider_new.rs, ui/providers.rs, provider_create.rs, draft_discovery.rs, state.rs | 部分：独立pending/取消/切换恢复/重开、零供应商真实发现/自动能力及小窗Save、最终两尺寸已验，精细视觉/IME/无障碍待续 |
| 78:605 | 25 / STUDIO V2 · 编辑供应商资料 | ui/provider_edit.rs, ui/providers.rs, ui.rs, state.rs, main.rs | 部分：原位编辑/取消完整草稿与选择、A/B独立、保存边界、重开草稿、两尺寸已验；整帧精细审计及IME/无障碍待续 |
| 78:843 | 26 / STUDIO V2 · 复制供应商草稿 | ui/provider_new.rs, provider_create.rs, ui/sidebar.rs, ui/providers.rs | 部分：来源dirty先保存/副本本地取消、显式Save/Test真实新目标/新路由和混合effort已验；最终两尺寸及Cancel补验、四轮性能及1000请求复核已验，完整审计待续 |
| 78:1080 | 27 / STUDIO V2 · 删除供应商确认 | ui/provider_delete.rs, ui/sidebar.rs, ui.rs, main.rs | 部分：532原生删除确认/保存目标与dirty/取消键盘/失败重试/无自动同步、两尺寸与合法长名实拍及四轮性能已验；完整视觉/无障碍待续 |
| 78:1325 | 28 / STUDIO V2 · 供应商满载滚动 (64) | ui/sidebar.rs, ui.rs, provider_filter.rs | 部分：最新高保真已读、native选中行/缩窗自动可见、手动9秒不回跳、固定CRUD/64容量与两尺寸已验；通用header差异及整帧/键盘/无障碍待续 |
| 78:1804 | 29 / STUDIO V2 · 删除后仅剩 OpenCode Go | ui/providers.rs, ui/sidebar.rs, ui.rs | 未验收 |
| 79:400 | 30 / STUDIO V2 · 64 个供应商搜索 | ui.rs, provider_filter.rs | 部分：最新高保真已读、64末行1/64与无结果0/64、清除恢复选中行/保持草稿/两尺寸与容量已验；独立搜索态实拍及完整键盘/无障碍待续 |
| 79:885 | 31 / STUDIO V2 · 新供应商保存完成 | ui/providers.rs, ui/provider_new.rs, ui.rs | 部分：真实Save与绿色完成徽标、dirty/失败/Cancel及两尺寸实拍、四轮性能已验；精细整帧/IME/无障碍/跨平台待续 |
| 79:1102 | 32 / STUDIO V2 · 供应商复制完成 | ui/providers.rs, ui/provider_new.rs, ui.rs | 部分：成功copy真实ID的会话徽标、dirty/Cancel/切换不串标/重开genericSaved、两尺寸与四轮性能已验；精细整帧/IME/无障碍/跨平台待续 |
| 80:418 | 33 / STUDIO V2 · 供应商搜索（2个） | ui.rs, provider_filter.rs | 部分：过滤/草稿保护通过，最终侧栏视觉待续 |
| 80:646 | 34 / STUDIO V2 · 编辑 OpenCode Go | ui/provider_edit.rs, ui/providers.rs, ui/sidebar.rs, ui.rs | 共用原位编辑/取消组件已实现；该整帧尚未逐项对照 |
| 80:864 | 35 / STUDIO V2 · 删除 OpenCode Go 确认 | ui/provider_delete.rs, ui/sidebar.rs, ui.rs | 部分：高保真已读，同一动态目标/模型数的原生确认；真实B键盘Space删除/小窗长名与两尺寸已验，整帧最终审计待续 |
| 80:1114 | 36 / STUDIO V2 · 删除 OpenCode Go 后剩 sub2api | ui/providers.rs, ui/sidebar.rs, ui.rs | 未验收 |
| 104:1378 | 37 / STUDIO V2 · 开机启动已开启（原型） | ui.rs, ui/skin_controls.rs, theme.rs | 未验收 |
| 104:1605 | 38 / STUDIO V2 · 检查更新交互（原型） | ui.rs, ui/skin_controls.rs, theme.rs | 未验收 |
| 104:1823 | 39 / STUDIO V2 · 语言切换菜单 | ui.rs, ui/skin_controls.rs, theme.rs | 未验收 |
| 104:2046 | 40 / STUDIO V2 · 恢复默认窗口尺寸（原型） | ui.rs, ui/skin_controls.rs, theme.rs | 未验收 |

共享组件：`103:442` 七项工具栏 → `ui.rs::app_controls`、`ui_components.rs`；最新 `54:2` / `90:442` 已将其独立置于窗口top8/right32、300×36，七项事件保持原样。最终049版1440×900与恢复界面操作后补拍的1100×700实拍已检查；最终小窗Skin/Language上缘点击、模态遮罩及Restore已验，快捷键与菜单Esc的完整范围见实施记录。`83:454` 供应商行、`83:455` 固定操作栏 → `ui.rs::sidebar`；`71:364` 原生实施契约 → `theme.rs`、本目录记录。

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

请求详情 `60:475` / `60:692` 已提取原生 `statistics/detail_layout.rs`，650×770固定头/提示/关闭底栏，15行主摘要与独立滚动；原额外metadata和价格快照在原生More浮层完整换行保留。真实代理/导入/失败/SSE、费用快照、同ID错误重试/恢复、关闭及复制ID已回归；Tooltip仍受CUA hover能力限制未作视觉验收，精细视觉/无障碍/跨平台继续保留待办。

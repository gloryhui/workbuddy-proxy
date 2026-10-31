# Studio V2 设计映射

官方 Figma MCP 于 2026-10-31 刷新40节点清单并完整读取B01的19份高保真上下文和原图，文件 `zxDfIQ4lhNcTXrFT7qk8QZ`、页面 `51:2`（WorkBuddy Proxy · Studio V2 · Rust GPUI）。以下 40 张 1440×900 状态画板直接来自 page metadata；未验收表示尚未逐画板进行运行与视觉对照。Rust 落点均相对于 `gpui/src/`。

批次计划v2起，下面“已验收”指该帧的最新设计、完整路径和两尺寸界面签收；全局输入、平台、性能与安装发行在 `release_plan.json` 的 R01–R04 独立记录。正式版须两者同时通过，不能把界面签收等同发布。B01按真实证据签收19帧，剩余21帧；不是因重排计划自动升级。

| Frame | 设计状态 | Rust 落点 | 验收 |
| --- | --- | --- | --- |
| 52:2 | 01 / STUDIO V2 · 使用概览 | statistics/overview.rs, trends.rs, recent.rs, layout.rs, usage_chart.rs | 部分：UI66主布局＋UI67最近28px标签/行位修复，同版自然0→22→23/12100/95.7%、筛选/币种/指标/详情、方向键/焦点和两尺寸通过；图表拖动真实点已验。匹配短测空闲合计+0.58MiB/峰值+12.63MiB保留；完整状态/无按键hover/IME/AX/六皮肤及平台终验仍待 |
| 54:2 | 02 / STUDIO V2 · 供应商管理 | ui/sidebar.rs, ui/providers.rs, ui/relay.rs, state.rs, model_table.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；默认A及真实连接200，2供应商5模型；四行小窗末行已独立补验。 全局R01–R04不在此签收内。 |
| 56:8 | 03 / STUDIO V2 · 用量明细 | statistics/*, usage_table.rs | 部分：八列/来源筛选/分页/详情/窄窗首末列已验；UI56/57最终候选两尺寸、14/22真实记录与原负载已验，a51表头top364/outer39；UI58共享标题墨迹修正。完整视觉/键盘/IME/AX/跨平台待终验 |
| 57:8 | 04 / STUDIO V2 · 汇总分析 | statistics/analysis.rs, layout.rs, usage_table.rs | 部分：UI56/57最终两尺寸与原负载已验，汇总2供应商5模型/22条12100Token范围浮层、小窗右列下卡可滚；标题列/314px切换区与表头top363/38已实拍。UI58共享标题实现修正，本轮未重拍Summary，完整状态/视觉待终验 |
| 58:8 | 05 / STUDIO V2 · 统计设置 | statistics/settings_layout.rs, settings_capture.rs, layout.rs, import_controls.rs | 部分：UI65完成官方整帧首审与设置布局/48px价格表/保留期等宽、两尺寸及窄窗下方可达；搜索3→1→3、价格2→2.5→2/保留180→90→180及历史14行不变有独立证据；原分页/额外操作保留，精细视觉和全量状态待收口 |
| 59:11 | 06 / STUDIO V2 · 选择模型 | ui/model_picker.rs, ui.rs, ui/providers.rs | 部分：UI63完整画板首审；769原生勾选资产/徽章/标题与字段文字修正，四类两尺寸及九Core快照、模型5→6/隔离同步6/价格拒绝修正与新请求快照/详情内滚、Mono抽查及原负载四轮留证；保留scrim/X、正文/价格几何、全量键盘/IME/AX/平台差异，未终验；UI64共用原生关闭/Enter取消及48%普通/49%删除遮罩、两尺寸专项已验，保留精细差异待整帧终验 |
| 59:279 | 07 / STUDIO V2 · 编辑模型能力 | ui/model_editor.rs, model_options.rs, ui.rs | 部分：原生728×800/两尺寸、自动/手动/真实来源、六能力/五档与默认Select、四限制校验/草稿保存、取消与Popover、Tab/焦点及resize/错误后恢复已验；UI53首轮全帧审查/16px原生checkbox+12px原资源/颜色列对齐与两尺寸实拍已记录；仍有1.5px纵向、档位宽度、字体/阴影/共享资产余差，完整键盘IME/无障碍/跨平台待续，未整帧通过；UI64共用原生关闭/Enter取消及48%普通/49%删除遮罩、两尺寸专项已验，保留精细差异待整帧终验 |
| 60:27 | 08 / STUDIO V2 · 同步 WorkBuddy | ui/sync_dialog.rs, ui.rs, backend.rs, workbuddy_restart.rs | 部分：UI63完整画板首审；769原生勾选资产/徽章/标题与字段文字修正，四类两尺寸及九Core快照、模型5→6/隔离同步6/价格拒绝修正与新请求快照/详情内滚、Mono抽查及原负载四轮留证；保留scrim/X、正文/价格几何、全量键盘/IME/AX/平台差异，未终验；UI64共用原生关闭/Enter取消及48%普通/49%删除遮罩、两尺寸专项已验，保留精细差异待整帧终验 |
| 60:249 | 09 / STUDIO V2 · 新建价格规则 | statistics/price_layout.rs, stats_controller.rs, stats_backend.rs | 部分：UI63完整画板首审；769原生勾选资产/徽章/标题与字段文字修正，四类两尺寸及九Core快照、模型5→6/隔离同步6/价格拒绝修正与新请求快照/详情内滚、Mono抽查及原负载四轮留证；保留scrim/X、正文/价格几何、全量键盘/IME/AX/平台差异，未终验；UI64共用原生关闭/Enter取消及48%普通/49%删除遮罩、两尺寸专项已验，保留精细差异待整帧终验 |
| 60:475 | 10 / STUDIO V2 · 请求详情 | statistics/detail_layout.rs, detail_metadata.rs, usage_table.rs | 部分：UI63完整画板首审；769原生勾选资产/徽章/标题与字段文字修正，四类两尺寸及九Core快照、模型5→6/隔离同步6/价格拒绝修正与新请求快照/详情内滚、Mono抽查及原负载四轮留证；保留scrim/X、正文/价格几何、全量键盘/IME/AX/平台差异，未终验；UI64共用原生关闭/Enter取消及48%普通/49%删除遮罩、两尺寸专项已验，保留精细差异待整帧终验 |
| 61:289 | 11 / STUDIO V2 · 协议选项 · OpenAI Chat | ui/protocol_dialog.rs, protocol_options.rs, ui/providers.rs, ui.rs, main.rs | 部分：UI62完整四画板首审；db899标签几何/原生白色选中态、徽章/映射Inter/字段色已改，四协议两尺寸原图、八tab键与九Core快照、非法映射/预算校验、四Apply→显式Save及原负载四轮留证；保留Select/标题光学、X/scrim、完整键盘/IME/AX/平台差异，未终验；UI64共用原生关闭/Enter取消及48%普通/49%删除遮罩、两尺寸专项已验，保留精细差异待整帧终验 |
| 61:561 | 11 / STUDIO V2 · 协议选项 · OpenAI Responses | ui/protocol_dialog.rs, protocol_options.rs, ui/providers.rs, ui.rs, main.rs | 部分：UI62完整四画板首审；db899标签几何/原生白色选中态、徽章/映射Inter/字段色已改，四协议两尺寸原图、八tab键与九Core快照、非法映射/预算校验、四Apply→显式Save及原负载四轮留证；保留Select/标题光学、X/scrim、完整键盘/IME/AX/平台差异，未终验；UI64共用原生关闭/Enter取消及48%普通/49%删除遮罩、两尺寸专项已验，保留精细差异待整帧终验 |
| 61:838 | 11 / STUDIO V2 · 协议选项 · Anthropic | ui/protocol_dialog.rs, protocol_options.rs, ui/providers.rs, ui.rs, main.rs | 部分：UI62完整四画板首审；db899标签几何/原生白色选中态、徽章/映射Inter/字段色已改，四协议两尺寸原图、八tab键与九Core快照、非法映射/预算校验、四Apply→显式Save及原负载四轮留证；保留Select/标题光学、X/scrim、完整键盘/IME/AX/平台差异，未终验；UI64共用原生关闭/Enter取消及48%普通/49%删除遮罩、两尺寸专项已验，保留精细差异待整帧终验 |
| 61:1111 | 11 / STUDIO V2 · 协议选项 · Gemini | ui/protocol_dialog.rs, protocol_options.rs, ui/providers.rs, ui.rs, main.rs | 部分：UI62完整四画板首审；db899标签几何/原生白色选中态、徽章/映射Inter/字段色已改，四协议两尺寸原图、八tab键与九Core快照、非法映射/预算校验、四Apply→显式Save及原负载四轮留证；保留Select/标题光学、X/scrim、完整键盘/IME/AX/平台差异，未终验；UI64共用原生关闭/Enter取消及48%普通/49%删除遮罩、两尺寸专项已验，保留精细差异待整帧终验 |
| 62:308 | 13 / STUDIO V2 · 黑白主题示例 | ui.rs, ui/skin_controls.rs, model_table.rs, assets.rs, theme.rs | 部分：UI65完整首审及Mono两尺寸/恢复Studio；旧27272a与键/默认/字体分支保留，和Figma字体示例差异明确列出；六皮肤全量状态及精细视觉待收口 |
| 64:335 | 14 / STUDIO V2 · 外观与主题 | ui/skin_controls.rs, theme/skin.rs, ui_components.rs | 部分：UI65完整首审，Studio350px菜单/六个原22pxSVG显式路由与原色像素盒对齐，保留Studio第七入口；两尺寸/换肤落盘/Esc与外点关闭通过，旧主题分支/选中样式与全量状态待收口 |
| 66:320 | 17 / STUDIO V2 · 上游协议下拉 | ui/provider_choices.rs, ui/providers.rs, ui.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；四协议下拉，两尺寸打开/Esc；未改原选择处理器。 全局R01–R04不在此签收内。 |
| 66:538 | 18 / STUDIO V2 · 供应商预设搜索 | ui/provider_choices.rs, ui.rs, presets.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；预设Bedrock搜索、方向高亮及关闭重开；原项确认另有同候选实测。 全局R01–R04不在此签收内。 |
| 66:760 | 19 / STUDIO V2 · API Key 显示演示 | ui/providers.rs, ui_components.rs, ui.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；仅合成Key两尺寸显隐，随后恢复遮罩。 全局R01–R04不在此签收内。 |
| 67:338 | 20 / STUDIO V2 · OpenCode Go 供应商 | ui/providers.rs, ui.rs, ui/provider_feedback.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；B单模型与真实200，两尺寸及小窗下方帮助/底栏可达。 全局R01–R04不在此签收内。 |
| 67:548 | 21 / STUDIO V2 · 思考强度菜单 | ui/provider_choices.rs, ui.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；思考菜单上方锚定，两尺寸完整；Low→Follow实际切换另有同候选证明。 全局R01–R04不在此签收内。 |
| 68:350 | 22 / STUDIO V2 · 取消同步确认 | ui/sync_dialog.rs, ui.rs, backend.rs, workbuddy_restart.rs | 部分：原生确认/两尺寸、取消/仅移除、保留providers与草稿/恢复原条目、守卫下移除后重启失败反馈已验；真实WorkBuddy重启与完整busy/无障碍/跨平台未验；UI64共用原生关闭/Enter取消及48%普通/49%删除遮罩、两尺寸专项已验，保留精细差异待整帧终验 |
| 69:359 | 23 / STUDIO V2 · 编辑模型价格 | statistics/price_layout.rs, stats_controller.rs, stats_backend.rs | 部分：UI65完整首审，最终e6abd两尺寸gpt-6 USD2/10检查/X取消；c2eb真实编辑保存/恢复revision3且旧14行全字段摘要不变；原身份锁定/缓存0保持，精细阴影/文字与全量状态待收口 |
| 78:364 | 24 / STUDIO V2 · 新增供应商草稿 | ui/provider_new.rs, ui/providers.rs, provider_create.rs, draft_discovery.rs, state.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；新增草稿取消，非法URL拒绝，合法本地保存与后续删除。 全局R01–R04不在此签收内。 |
| 78:605 | 25 / STUDIO V2 · 编辑供应商资料 | ui/provider_edit.rs, ui/providers.rs, ui.rs, state.rs, main.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；A编辑/取消，实际改名保存、切换读回与还原。 全局R01–R04不在此签收内。 |
| 78:843 | 26 / STUDIO V2 · 复制供应商草稿 | ui/provider_new.rs, provider_create.rs, ui/sidebar.rs, ui/providers.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；复制独立草稿、取消、保存；原Key继承逻辑保持，不从遮罩推断为空。 全局R01–R04不在此签收内。 |
| 78:1080 | 27 / STUDIO V2 · 删除供应商确认 | ui/provider_delete.rs, ui/sidebar.rs, ui.rs, main.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；删除A确认及取消/X，遮罩阻断工具栏，明确确认后仅删A。 全局R01–R04不在此签收内。 |
| 78:1325 | 28 / STUDIO V2 · 供应商满载滚动 (64) | ui/sidebar.rs, ui.rs, provider_filter.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；64家真实满载，新增/复制禁用点击不产生草稿。 全局R01–R04不在此签收内。 |
| 78:1804 | 29 / STUDIO V2 · 删除后仅剩 OpenCode Go | ui/provider_feedback.rs, ui/providers.rs, ui/provider_delete.rs, ui.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；删除A后B独立保留，原字段/模型保持。 全局R01–R04不在此签收内。 |
| 79:400 | 30 / STUDIO V2 · 64 个供应商搜索 | ui.rs, provider_filter.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；64家搜索27/64/无匹配/清空，末项选择及小窗保持。 全局R01–R04不在此签收内。 |
| 79:885 | 31 / STUDIO V2 · 新供应商保存完成 | ui/providers.rs, ui/provider_new.rs, ui.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；新增保存后真实3家5模型，新家0模型与成功上下文。 全局R01–R04不在此签收内。 |
| 79:1102 | 32 / STUDIO V2 · 供应商复制完成 | ui/providers.rs, ui/provider_new.rs, ui.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；复制保存后真实3家9模型，原供应商与复制模型非身份字段保持。 全局R01–R04不在此签收内。 |
| 80:418 | 33 / STUDIO V2 · 供应商搜索（2个） | ui.rs, provider_filter.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；两家列表搜索OpenCode与无匹配/清空，选择身份保持。 全局R01–R04不在此签收内。 |
| 80:646 | 34 / STUDIO V2 · 编辑 OpenCode Go | ui/provider_edit.rs, ui/providers.rs, ui/sidebar.rs, model_table.rs, ui.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；B编辑改名与取消恢复，完整值可从字段/侧栏读取。 全局R01–R04不在此签收内。 |
| 80:864 | 35 / STUDIO V2 · 删除 OpenCode Go 确认 | ui/provider_delete.rs, ui/sidebar.rs, ui.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；删除B确认，Esc/默认Return/Cancel与显式确认分别留证。 全局R01–R04不在此签收内。 |
| 80:1114 | 36 / STUDIO V2 · 删除 OpenCode Go 后剩 sub2api | ui/provider_feedback.rs, ui/providers.rs, ui/provider_delete.rs, ui.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；删除B后A及4模型原字段保持。 全局R01–R04不在此签收内。 |
| 104:1378 | 37 / STUDIO V2 · 开机启动已开启（原型） | ui/autostart_notice.rs, ui.rs, assets.rs, theme.rs | 部分：原生提示/实际开关与隔离plist、分候选失败回滚/重试和Pending关闭、动态Tooltip实时刷新、遮罩/两尺寸与四轮性能已验；初始Tab/完整键盘/IME/无障碍及跨平台待续 |
| 104:1605 | 38 / STUDIO V2 · 检查更新交互（原型） | ui/update_notice.rs, ui.rs | 部分：299×113原生等待/实际HTTP403与重试、关闭不回包重开、Esc/X/外部/缩窗、草稿筛选/遮罩/两尺寸与六轮性能已验；公网成功/完整键盘/IME/无障碍及跨平台待续 |
| 104:1823 | 39 / STUDIO V2 · 语言切换菜单 | ui/language_menu.rs, ui.rs | 部分：原241×184/三模式保存与重复不写/取消/草稿筛选/重开等证据保留；最终0892 Studio工具焦点/Enter与Space打开/Esc返回/复开及Tab接续已验，菜单30s CPU样本已留；长期CPU/完整IME/读屏/原生AX及跨平台待续 |
| 104:2046 | 40 / STUDIO V2 · 恢复默认窗口尺寸（原型） | ui/window_restore.rs, ui.rs | 部分：原生提示/两尺寸、普通/zoom/全屏恢复、缩窗Esc/X、真实Enter/Space、草稿筛选/遮罩与四轮性能已验；超时失败、完整IME/无障碍/跨平台待续 |

共享组件：`103:442` 七项工具栏 → `ui.rs::app_controls`、`ui_components.rs`；最新 `54:2` / `90:442` 已将其独立置于窗口top8/right32、300×36，七项事件保持原样。最终049版1440×900与恢复界面操作后补拍的1100×700实拍已检查；最终小窗Skin/Language上缘点击、模态遮罩及Restore已验，快捷键与菜单Esc的完整范围见实施记录。`83:454` 供应商行、`83:455` 固定操作栏 → `ui.rs::sidebar`；`71:364` 原生实施契约 → `theme.rs`、本目录记录。

共享模型表专项（小功能40）：`model_table.rs`、`ui/model_navigation.rs`、`state.rs`补齐原生表/行/具名表头/单元格语义、方向/Home/End与Shift范围、活动选中行Enter/F2、单一表级Tab入口和可见焦点。最终0c5c真实键盘、1440/1100两尺寸及四轮性能已验；原生AXSelected/SelectedRows/系统焦点属性仍待方法授权和运行核对，完整IME/读屏/跨平台及精细视觉未完成。该共享证据不改变40部分/0整帧最终验收计数。

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

共享工具栏专项（小功能41）：0892保留103:442七项顺序/20px资源/36px热区/8px间距/top8/right32，改原生focus_visible为显式1px主色框，未改变全局ring/selection。统计页初始Tab/反向七项与语言Enter/Space/Esc/Tab接续、皮肤非模态浮层离开关闭/内部保存、Sync遮罩、两尺寸及菜单CPU/八轮原基准已验；完整无障碍/其他控件焦点与跨平台待续，不改变40部分/0整帧最终验收计数。

共享生命周期专项（小功能42）：同一0892/25f字节未改；macOS实际Core/Tray/UI三进程、仅关设置后2驻留且真实loopback200、官方重复启动/已有Tray生成新UI及草稿恢复、实际CmdQ整体退出和统计run正常关闭已验，另补原脚本正常冷启动/重启路径。1440/1100为新UI重开实际截图。CUA托盘绑定timeout，物理菜单点击和拖动位移/最小化状态未验证；全局坐标方法仍待授权，Windows/Intel/HiDPI/完整视觉等不在通过结论内。40帧继续部分验收。

Windows专项（小功能43）：实际锁定Kit0.7.3/pre-windows0.3.9源码的静态模型发现右上窗口按钮与top8/right32工具栏末两项重叠，两宽度均有明确交集；不是Windows实拍。GNU检查在依赖manifest缺windres处失败，应用Windows类型检查未完成，另有rc.exe源码前置条件；Windows布局偏好/实机/HiDPI仍待核对，Mac位置及40部分/0整帧最终验收计数不变。

共享模型编辑图标（小功能44）：最新54:246/54:264与黑白62:438高保真确认同一15px原资源；b527仅修Studio/Mono可用按钮的Kit遮罩颜色，图标/28px热区及事件保持，其他皮肤沿用。两尺寸、Mono/Blue切换、鼠标首行/Down+F2第二行、取消/遮罩/草稿往返与显式Save、192测试/四轮原基准已验；不覆盖完整整帧/IME/原生AX/Windows/Intel/HiDPI待办，仍40部分/0最终验收。

请求详情专项（小功能45）：重读60:692高保真，0b43仅将原生Tooltip内容改为有界换行Label，保留全部字符串、主行、API与事件。旧b527越框原图及最终两尺寸/自然长提示完整/More键盘和两层Esc/48字符真实ID及原数据/3×30s/六轮原基准均留证；三个字段独立精确hover与完整IME/AX/跨平台等未验，40帧仍部分验收。

共享输入专项（小功能46）：同0b43/25f无产品代码变更；59:279显示名在CUA setValue后可编辑中文/emoji/分解重音，模型取消/普通Latin Enter与Esc、确认仅草稿、显式保存及新Core/UI从实际保存模型载荷冷载入已验，三张原生图/资源与字节证据保留。普通Unicode键入与真实IME候选组合当时未验，TIS只读授权在小功能50获准并补模型显示名实测；不改变40部分/0最终验收计数，也不把码点删除标为字素簇语义通过。

共享模型提示专项（小功能47）：54:246原资源/尺寸保持，c6ee将模型表与详情接入同一有界原生Tooltip；旧name越窗与新两尺寸五行尾标、编辑/保存/More两层Esc、六轮性能已留证。独立表格ID提示未验、首帧与整帧/IME/AX/跨平台仍待续，40部分/0最终验收不变。

共享侧栏选中色专项（小功能48）：54:19/75:379与Mono76:1344/76:1376明确#2D3251；983c仅pin active导航和active非busy供应商行实色，原Kit交互定义和事件保留。新c6ee基线#171B2E、Studio两尺寸/Mono/Blue的精确像素和填充范围、A/B/三导航/TabReturn及换肤恢复、Core配置未变与四轮基准均留证；精确hover/busy/IME/AX/跨平台和整帧仍待续，40部分/0最终不变。

共享模型目录标记专项（小功能49）：54:246/54:249/54:262数量94×27与配置来源65×27已原生补齐；b921显式列垂直padding0及cell/header居中，修复中间ff2标记裁成24px。最终两尺寸中英/选中态、编辑取消/能力模式独立/供应商计数、Core模型投影保持与六轮性能均留证；独立长ID提示、256满载标记与完整视觉/IME/AX/跨平台仍待专项，40部分/0最终不变。

真实IME专项（小功能50）：59:279模型显示名在当前青简/Hans下逐键拼音→Space汉字、组合Esc/Return原拼音、UndoRedo/草稿/实际Save及新Core冷读已验。87eb仅加入原生InputState控制字符validator，修复你好前缀取消后混入U001B；宽/小窗保留原生2×像素，TIS测试侧5次只读名ID均同源。其他字段/输入源/候选面板/完整AX/跨平台仍待验，RSS上升与Native对照timeout/私有保留边界见QA50；40部分/0最终保持。

共享模型输入与满载专项（小功能51）：沿用54:246/59:279布局资源，无新设计读取；675f仅main.rs/ui.rs增加模型输入专属Escape路由并保留原生marked composition优先。宽首末/小末行F2普通Esc、两尺寸逐键拼音首Esc仅取消组合/再Esc关闭及256首末标记网格已验；旧87eb64/256容量/搜索空态/256范围删除仅草稿与Core不变、当前对齐16次滚动资源、四确认原基准及一保留预检已留证。独立长ID悬浮/其他源和数字字段/完整视觉AX跨平台仍未验；旧Esc冲突和失败中间版见QA51，40部分/0最终保持。

平台专项（小功能52）：官方Figma59:279/59:484在用户重新授权后恢复高保真读取。当前同源Intel Core/UI发行版已真实链接并以Rosetta执行192 release测试、编辑/草稿/实际Save与两尺寸native截图、4轮原负载；译运行不是Intel实机，未修改UI布局资产，完整40帧一比一验收和平台门禁保持未完成。报告现区分专项任务数/画板完成数，40部分/0最终，详QA52。

全局逐帧首轮审查（小功能53）：已重新读取51:2完整40帧清单及59:279/59:484高保真和1440×900参考。独立台账 `visual_review.json` 目前39帧尚未开始、1帧已审查保留差异；这不替代主表最终验收。模型checkbox/notice几何和颜色、31资产哈希核对、最终两尺寸真实交互与8轮原基准见QA53。实际发现1.5px行偏移、五档位flex宽度、系统字体、Kit阴影和共享SVG差异，40部分/0最终保持。


统计共享布局专项（小功能56）：最新52:2/56:8/57:8整帧与资产已审查；b313统一筛选区、四16px精确caret、概览17px段间距及汇总标题/分组切换布局。最终1440/1100原生1×七图、四箭头点击、模型3/供应商12/全部14、分页草稿、2/5分组与真实14/7700范围浮层已验，六轮原基准保留全部波动。汇总表头top363/38符合稿，明细top363/39较稿仍上移1px；字体/完整键盘/IME/AX/全部状态和跨平台仍待终验。首轮台账35未开始、1审查中、4已审查保留差异；40部分/0最终验收不变。后续字体候选不属于本版b313，见独立后续记录。


共享字体基础专项（小功能57）：原Noto Sans SC三字重/Inter四字重首次Studio进程内加载，恢复旧native sans且不改六旧皮肤键或默认Mono；共享侧栏、27/39标题与13/19副标题、统计KPI/表头/九种单元格/日期轴/卡片角色接入。a51的195tests/strictclippy/Intelcheck/release、最终两尺寸15原图/六皮肤/22条12100Token与万单位、供应商/模型可见区抽查及四轮原基准完成；此前edb启动增量留证并修重复字体枚举，不把两组样本混池宣称因果优化。明细表头当前364/39符合稿；标题墨迹1px、其他全部字体角色/版本/栅格、完整IME/AX/跨平台等仍待。40部分/0最终、35未首审/1审查中/4审查保留差异不变。

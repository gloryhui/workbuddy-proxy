# Studio V2 功能保护矩阵

状态区分：源码确认（旧实现存在）、自动验证（当前分支测试）、GUI 验证（隔离数据真实应用）。未验证不能视为通过。

| 功能 | 原实现落点 | 当前状态 / 必须补验 |
| --- | --- | --- |
| 七功能工具栏独立定位与遮罩层级 | ui.rs::render/app_controls、ui_components.rs、theme.rs | 原top8/right32/300×36、36px按钮/20pxSVG/8px间距及七处理器保留，旧049两尺寸/点击/Restore和71ae快捷键证据保留；最终0892 Studio首Tab/反向七项清晰原生主色框、语言Enter/Space/Esc与Tab接续、皮肤双向Tab离开关闭/Esc恢复、Sync遮罩/两尺寸实拍/菜单CPU与八轮短测 Passed；OS焦点属性、完整IME/读屏/跨平台仍待验 |
| Windows 标题栏与工具栏兼容性 | ui.rs::render、Kit TitleBar、gpui-pre-windows | UI43静态审计发现GitHub/Feedback与右上窗口按钮区域重叠，未作Windows运行结论；当前GNU check在依赖manifest资源编译缺windres处exit101，应用rc.exe另为源码前置条件，类型检查未完成。布局偏好待回复，Windows窗口按钮/Ctrl/IME/HiDPI/两尺寸与性能均待验；未改Mac top8/right32或工作流 |
| Intel 发行版与译运行 | gpui/build.rs、原生Core/UI、scripts/benchmark.py | UI52真正x86_64 Core/UI release链接、thin x86_64测试harness192passed/2既有ignored、Rosetta实际F2/取消/草稿与一次Save、两尺寸和4轮原负载已验；源代码未改。AppleSilicon译运行不替代Intel CPU/GPU实机；译运行启动更慢/不同RSS计数及旧高值保留，不作硬件性能通过或跨架构排名 |
| 六套旧皮肤 + 可选 Studio 靛蓝、保存失败不切换 | theme/skin.rs、ui/skin_controls.rs | 原七项选择/原键保存、默认Mono/Studio重开和非法目录测试保留；最终0892菜单内Blue→Studio落盘/草稿保持、Esc及双向Tab离开关闭已验；七皮肤工具按钮21组浅面焦点色对比回归通过，仅工具栏焦点样式变化，全局ring/selection不改；其他控件焦点/完整视觉与跨平台待续 |
| 开机启动、失败回滚 | ui/autostart_notice.rs、ui.rs、assets.rs、原Core action | 原动作/忙碌禁用/回滚保留；真实假HOME开关/plist/失败重试、分候选Pending取消不重开/已聚焦Enter与Space/草稿筛选及重开、最终power/选中色/遮罩和持续hover实时更新、两尺寸与四轮短测 Passed，覆盖按候选分开记录。真实LaunchAgents受内核沙盒保护；0892 Studio新窗口Tab焦点已验，完整键盘/IME/无障碍/跨平台及实际OS登录待续 |
| 检查更新、busy/error | ui/update_notice.rs、ui.rs::app_controls/tick、原Core update | 原force:true/网络/版本/缓存不改；原生等待、真实HTTP403与重试、Pending关闭不重开、X/外部/Esc/实际缩窗、草稿/Custom过滤/语言保持及模态遮罩、两尺寸/六轮短测 Passed；UI响应与原Core两项契约测试通过，公网成功未实机验。busy精确请求数、完整键盘/IME/无障碍/跨平台待续 |
| 系统/中文/English | ui/language_menu.rs、ui.rs::set_language | 原241×184/三模式保存与重复不写、方向/取消/草稿筛选/重开与遮罩等先前证据保留；最终0892 Studio主色焦点、Enter/Space打开/Esc返回/复开/Tab到Restore已验，菜单30s CPU2.53%单核/0.17%Core与RSS原值保留；三模式未在0892全套重验，长期CPU/完整IME/读屏/原生AX与跨平台待续 |
| 恢复 1440×900、退出全屏 | ui/window_restore.rs、ui.rs::restore_window_size | 原算法逐字核对保持；325×103提示、普通/zoom/全屏恢复、实际缩窗后Esc/X、键盘Return/Space、草稿/Custom筛选与遮罩、两尺寸和四轮短测 Passed；互斥/重复仅旧80ad实测、超时失败未注入，完整IME/无障碍/跨平台未验 |
| GitHub 与新 Issue | ui.rs::app_controls | 原外部URL和open_url处理器保持；最终0892 Studio两按钮Tab可达/主色焦点已验，本轮未激活外链 |
| 最小窗口 1100×700、系统按钮/拖动 | main.rs、TitleBar | 原两尺寸/中英/缩小恢复证据保留；UI42同0892重开草稿两尺寸已实拍，CUA暴露Raise/close/minimize/fullscreen/zoom，原生close真实退出设置已验；drag/minimize动作成功不等于位移/最小化状态通过，工具未提供属性且外部坐标查询待授权，Windows/HiDPI待验 |
| 供应商 0/1/2/20/64、固定 CRUD | ui.rs::sidebar、provider_filter.rs | macOS 5 种数量、末尾搜索/空结果/清空、64 滚动/新增复制禁用 Passed；选中尾行保存/resize/清搜索/展开自动可见、手动9秒不回跳与20/64两尺寸 Passed，整帧/键盘/无障碍待续 |
| 侧栏选中导航/供应商实色 | ui/sidebar.rs、ui_components.rs、theme.rs | UI48 983c只在active导航/active且非busy供应商行pin原#2D3251，绕开Kit默认混色；新基线#171B2E与最终Studio两尺寸/Mono/Blue像素、224×43/216×38填充范围/原18px及7px资源已核对。A/B/三导航/一次Tab+Return与换肤恢复、Core配置不变、192测试和四轮原基准 Passed；busy/精确hover按住/其他四皮肤/完整AX/IME与跨平台未逐项实机验 |
| 新增、复制、删除确认与取消 | ui/provider_delete.rs、ui/sidebar.rs、ui.rs、state.rs | 原生532删除确认/已保存目标与dirty说明、Cancel/X/Esc与草稿保护、默认Return不删/Tab确认Enter或Space、Core失败重试、仅删A保留B及最终删B到空列表、无自动同步/哨兵不变、两尺寸/合法长名实拍与四轮性能 Passed；headless未知/重复删除配置不变。未实际双击/hover按压，合法长名未触发内部滚动；新增/复制独立pending、取消/恢复/重开、来源dirty先保存、Save/Test新目标/新route与mixed effort、零供应商发现及可见小窗Save已实机Passed；完整IME/无障碍/跨平台待续 |
| 资料编辑/取消与历史草稿 | ui/provider_edit.rs、ui/providers.rs、model_table.rs、ui.rs | 既有Cancel完整Draft/Selection、Name Return及按钮键盘证据保留；本轮B原位Cancel恢复先有未保存名/新增模型、A独立草稿、显式Save仅B、Studio普通白/蓝与原生上下选中边线/Shift多选仅草稿删除、两尺寸四图/遮罩/重开与四轮同主题性能 Passed；编辑快照仅窗口内，完整视觉/键盘/IME/无障碍及跨平台待专项 |
| 新增/复制保存完成提示 | ui/providers.rs、ui/provider_new.rs、ui.rs | 真实Save后宽窗绿色Saved/Saved copy、小窗单状态、dirty/非法Save/Cancel、切B返回不串标、关闭重开genericSaved、两尺寸与四轮性能 Passed；会话来源不写配置，通用已保存行及重开无左创建徽标、New/Copy区分两尺寸已验；busy未完整实测 |
| 单模型提示/删除后结果 | ui/provider_feedback.rs、ui/providers.rs、ui/provider_delete.rs、config_refresh.rs、backend.rs | 实际草稿1→2→1、664×50提示/英文小窗换行；真实删A/B后幸存者1/1与1/4计数/两尺寸、edit/dirty/普通Save与重开清除 Passed；外部先删目标后的失败能只读对齐可信配置、保留原错误/dirty及Cancel快照/重开草稿，最后供应商空态、缺失/非法文件不初始化或覆盖及恢复重试、两尺寸与4轮性能 Passed；busy/三供应商切换/Core-unresponsive/完整IME/无障碍待续 |
| 未保存切换/恢复草稿 | ui.rs::capture、switch_provider、Drafts | macOS 搜索不切换、切换保留、正常关闭重开恢复 Passed；提示表现待续 |
| 协议/预设/思考菜单、名称/URL、Key 显隐 | ui/provider_choices.rs、ui/providers.rs、ui.rs、presets.rs、InputState | 原生Popover/Input/虚拟List三菜单尺寸与行间距、四协议说明/六档原值、鼠标/方向Enter/Esc/外部取消、地区搜索/空结果Return不应用、重开清查询、中英保值、显式Save前后Core真实对象与两尺寸 Passed；Key原toggle_masked、17px设计SVG、Tab Enter/Space与中英Show/Hide、AX secure无值 Passed，仅合成Key入图；真实Key不用于截图/日志，完整IME/无障碍/跨平台待续 |
| 四协议不同字段、认证、tokens/思考 | ui/protocol_dialog.rs、protocol_options.rs、ui/providers.rs、main.rs | 原生四tab/620×724及小窗、Chat/Responses Azure、Responses none/max、Anthropic budget/adaptive/8192输出、Gemini budget/level、临时非法文本往返保留、Apply只改draft/显式CmdS后Core对象、局部错误8秒不外溢、X/Esc/标题空白Esc/Select优先Esc、标签Enter/Space不Apply及键盘选未选tab、Tab/ShiftTab/带焦点缩窗可见、最终98fb四协议×两尺寸及错误图 Passed；完整IME/无障碍/跨平台待续 |
| 模型发现 4096、总量 256、手动编辑 | ui/model_picker.rs、model_table.rs、ui/providers.rs | macOS 原生picker两尺寸、38/4096候选虚拟化、过滤/全选/反选/清空/仅已选/已有禁用、空态、Tab/方向/Home/End/Page/Space/Enter、Cancel/X/Esc、确认仅草稿/显式保存、其他供应商253+当前1到256与第257拒绝 Passed；原新增/编辑取消/空模型/256手动新增禁用/128行模型表 Passed，完整IME/无障碍与跨平台待续 |
| 模型表键盘、焦点与语义结构 | ui/model_navigation.rs、state.rs、model_table.rs、ui/providers.rs | 最终0c5c真实方向/Home/End边界、Shift范围反向/活动行F2、Tab单入口/进出、全选仅草稿删除、空表与两尺寸/四轮性能 Passed；CUA有具名Table/Row/ColumnHeader/Cell和模型编辑按钮。Kit后置覆盖多选元数据已源码修复，AXSelected/SelectedRows/系统焦点未外查，待方法授权及运行验证，完整IME/读屏/跨平台未验 |
| 模型行编辑图标与保存入口 | model_table.rs、ui/model_editor.rs | UI44 b527在Studio/Mono使用原15px SVG灰蓝色，Blue保持原色、busy保留Kit禁用样式；鼠标正确首行/Down+F2正确第二行、遮罩/Esc、确认仅草稿/复开/B→A保持/显式Save后Core更新、两尺寸与三皮肤实拍/192测试及四轮基准 Passed。只修颜色，完整视觉/IME/无障碍/跨平台仍待续 |
| 模型表完整值提示 | model_table.rs、ui_components.rs | UI47 c6ee：旧0b43 name提示越窗口已实拍复现，共享限宽换行后1440/1100原生name提示五行尾标完整；F2/Esc、草稿/显式保存与请求More/Esc、192测试/六轮基准通过。独立ID Tooltip未触发，不以源码或完整AX值代替视觉通过；性能小幅上升/首帧及跨平台限制见QA47 |
| 模型目录数量与配置来源标记 | model_table.rs、ui/providers.rs、state.rs | UI49 b921：原94×27数量及65×27 Custom标记补齐，按provider配置来源而非能力手动模式；修Kit padding导致中间24px裁切，最终29px行内完整27px/选中边框、两尺寸中英/徽标选行F2与铅笔/自动和手动取消/A3B1A3及Core模型投影保持、192测试与六轮基准已验。UI51补当前675f两尺寸256首末标记网格与实际滚动资源，旧87eb64/256容量、搜索/空态和256范围删除草稿边界已验；独立长ID悬浮、全皮肤/AX/跨平台仍待续 |
| 模型能力自动/手动、思考档位与Token/上下文 | ui/model_editor.rs、model_options.rs、ui.rs | 原生两尺寸；builtin/unknown与未知图片警告、手动等待不覆盖、六能力依赖与Chat/native自定义协议区别、五档/default及数字/上下文/空ID校验、局部错误持久、草稿确认/显式保存、Cancel/X/Esc与Help Esc、Tab/ShiftTab、带焦点缩窗及错误高度变化后可见、滚动等待不回跳 Passed；完整IME/无障碍/跨平台与精细审计待续 |
| Shift/Cmd/Ctrl 多选、快捷键、焦点/IME | state.rs、model_table.rs、main.rs | macOS Shift/Cmd 多选、Delete/Backspace、Cmd+S、编辑聚焦 Passed；Windows Ctrl/完整 IME 待验 |
| Unicode 字段编辑、保存与冷载入 | ui/model_editor.rs、ui.rs、原生InputState | UI46 setValue后的Unicode编辑/保存/冷载入证据保留；UI50已获只读TIS名ID授权，当前青简下真实逐键拼音/Space汉字、前缀组合Esc/Return原拼音、UndoRedo、草稿/显式Save/新Core冷读真实IME产物已验。修复87eb仅给模型edit_name加既有Core控制字符规则，阻止Esc混入U001B；两尺寸原生2×图及六轮性能留证（RSS中位上升项保留）。候选窗口/其他字段和源/完整AX/跨平台未验，不称全局IME完成 |
| 模型编辑输入框 Escape | main.rs、ui.rs、原生InputState | UI51：旧版Esc不稳定及父级路由中间版失败均留证；675f模型输入专属作用域/检查当前焦点非空marked range，宽首末/小末行F2普通Esc关闭、两尺寸真实拼音首Esc仅取消组合/无U001B且第二Esc关闭，Core及全部草稿模型投影保持。其他源/数字字段/完整读屏与跨平台未全验 |
| 同步/取消、备份恢复与重启 | ui/sync_dialog.rs、ui.rs、backend.rs、workbuddy_restart.rs、核心 | 原生确认/取消与容器Enter、仅同步只保存当前草稿并写所有saved、保留其他草稿/无关条目、仅移除不保存draft并恢复原对象、非法文件不变及重试 Passed；guard核对失败不进入restart/成功后明确部分成功，随机唯一bundle组件正常重开/拒退不强杀/多实例不碰 Passed；真实WorkBuddy重启、完整并发busy、无障碍/跨平台未验 |
| 概览/明细/汇总/设置、全部/内置/自定义 | statistics/*、source_filter.rs | macOS 三报表/设置返回、2/0/2来源、日期/刷新/更多浮层 Passed；概览四KPI/七详细指标/空值/趋势四图例/计费和小窗滚动 Passed，最近请求/详情ID与过滤 Passed；分析表/Token构成/缓存80%与空值、12模型真实分页和两种实际排序、1440并排/1100两卡滚动与独立位置 Passed；Tooltip及完整视觉审计待续 |
| 日期、分页、每页条数、固定表头 | statistics/pagination.rs、usage_table.rs | 日期草稿取消/应用、每页10/20条、上一页/下一页/输入跳页 Passed；明细八列与分析首/末列按钮在1100同步表头/表体取回最右列 Passed，保留服务端分页/虚拟表体；最终1100中英文两完整行、分页/导航不换行，英文状态完整可读；每页Select即时中英切换保值、10/20翻页及非默认20关闭重开恢复 Passed，完整本地化仍待审计 |
| 未知/缺失、缓存、Token 完整性 | usage_chart/continuity.rs、usage_table.rs | 既有自动测试 Passed；GUI保留缺失/单点提示、图例独立；缓存子集和未知不造零语义保持 |
| CNY/USD/积分、价格快照与规则 CRUD | statistics/trends.rs、statistics/price_layout.rs、statistics.rs、stats_backend.rs | 计费摘要CNY真实零/USD缺失、独立积分/默认零价及费用趋势 Passed；价格规则新建/编辑取消与保存/菜单删除、精确搜索、21规则20/50真实分页、旧金额不变/新revision生效及更新失败持续反馈；原生新建/编辑两尺寸、四费率/备注/身份锁定、Tab/Shift+Tab自动滚动/Enter/Cancel/X/Esc、校验和服务端revision冲突恢复 Passed；完整IME/无障碍/跨平台与精细视觉待续 |
| 采集、保留天数、本地导入去重 | statistics/settings_capture.rs、statistics/import_controls*、workbuddy_import.rs | 原生双卡及窄窗纵排；CNY/USD、30/90/永久/365天、REQUEST/OBSERVE、21供应商20/50分页与独立滚动、摘要暂停/恢复真实请求、导入暂停追加再启用、六计数/实际checkpoint、完整核对/分类入口、全局截止清理预览取消/确认及源文件不变 Passed；全量键盘/IME/无障碍与跨平台待续 |
| 真实代理状态、计数、端口 | ui/relay.rs、core_state、status_bar | macOS 合成2供应商/6模型/1请求、暂停核心未响应/恢复、复制完整端点 Passed；端口0/65536拒绝、确认Esc/取消不重启 Passed；真实重启/回滚待验 |
| 最近 100 条日志、清空/复制/滚动 | ui.rs::snapshot、ui/relay.rs、ui_components::TerminalLogs | macOS 深色只读/复制/清空、无新日志不抢首部/新日志到底 Passed；Studio 补验发现选区覆盖后已修复，Mono/Studio 实拍选区可读；100条上限源码确认 |
| 最近请求、详情与独立读身份 | statistics/recent.rs、stats_backend.rs | 167自动回归；同核心22记录、最新ID/详情、明细第2页不误用、来源/供应商与快速切源、小窗Esc Passed；请求最小10条仅取首条 |
| 请求详情、元数据与本次单价 | statistics/detail_layout.rs、statistics/detail_metadata.rs | 原650×770/15行、代理/导入/失败/SSE/费用快照/同ID错误重试恢复/关闭/64位ID复制证据保留；UI45仅修长Tooltip越边框，0b43两尺寸自然出现请求模型提示完整且30s首尾同图、More的Tab+Return/Space/两层Esc、原API零与未知/价格@1、小窗底部、192测试/六轮基准 Passed。三个精确hover/完整无障碍/跨平台与整帧审计待续 |
| 关闭 UI 释放、托盘/核心续存、单实例 | main.rs、原生托盘、backend.rs | UI42真实3→2→3、原生关窗后Core/Tray存活且loopback200、官方重复启动同PID与已有Tray生成新UI/草稿恢复、实际Settings CmdQ使Core/Tray/UI全退且唯一统计run正常关闭 Passed；正常冷启动及重启原脚本另验三角色/单实例/关闭重开/两run normal，冷路径全角色kernel保护。show/CLI不是物理菜单点击；指定托盘CUA绑定timeout、菜单点击仍待验，Windows/Intel实机待续 |
| 外观/中英/HiDPI/无障碍 | theme.rs、ui_components.rs、model_table.rs、assets.rs | macOS既有七主题/中英分项证据保留；本轮Mono白/浅蓝交替行、三设计SVG实际加载、多选跨Blue→Mono及中英切换后草稿删除2→0/Core仍3、名称草稿/重开Mono持久、两尺寸与遮罩 Passed；完整初始Tab/HiDPI/IME/无障碍及Windows/Intel实机待专项 |

后续按 `release_plan.json` 的完整功能批次更新此矩阵；共享组件与同类路径集中验证，不按单个间距反复拆轮。历史分项保留，不将核心未改动当作运行回归通过。

模型选择 `59:11` / `59:216` 已提取原生 `ui/model_picker.rs`：Kit虚拟List与Checkbox支持4096候选，固定批量操作/底栏与小窗自适应，保留已有项、过滤批选、草稿确认后显式保存和跨供应商总256约束。两尺寸、键盘、错误/空态与取消已验；不改变模型表原有Shift/Cmd/Ctrl多选。完整IME/无障碍/跨平台仍待最终审计。

模型能力编辑 `59:279` / `59:484` 已提取 `ui/model_editor.rs`，保留设计未呈现的默认思考Select、真实元数据来源、未知图片警告及完整媒体/Chat URL说明；六能力Flag逻辑、模型能力五档与原验证器保持。局部错误不再写入Runtime，固定底栏/原生滚动/缓存焦点组与布局后单次校正解决缩窗及错误时的遮挡；全面IME/无障碍/跨平台继续保留待办。

同步确认 `60:27` / `60:232` 与取消同步 `68:350` / `68:555` 已提取 `ui/sync_dialog.rs`，三按钮保持原save/install/restore与restart job顺序。正常隔离GUI验证文件/草稿语义，重启产品路径以denial-only guard核对部分成功；无guard的随机bundle脚本独立验证生命周期基础函数，未将其冒充真实WorkBuddy重启。启动地址不一致确认仍为原分支，后续视觉施工与整帧审计继续。

UI53 全帧审查与原生能力控件：BaseCheckbox/Indicator继续使用Kit焦点、disabled、checked与键盘引擎，16px盒/12px原SVG/10px标签间距。最终af174 AUTO只读点击、手动六项依赖、Medium与AUTO点击/Space、两尺寸及限制滚动/全部取消已验；中间bb289在仅effort Label样式差异前的Confirm/主页Save由Core独立读回。最终候选完整Tab/ShiftTab/Enter、IME/AX/全帧视觉仍未通过；不扩大既有专项结论。


统计共享控件与布局（UI56）：最终b313仅Rust/GPUI布局，Core/查询/原事件保持。四caret实际区域点击、模型方向/Enter筛选3/供应商12后恢复14、日期Today→7天/刷新Off→5、Jump/Prev及5秒设置下7.5秒未提交页码保留、Summary供应商2/模型5/原生范围浮层14记录7700Token、小窗内部滚动与右侧列、侧栏选中及回到供应商路由通过。两尺寸原生1×七图、编译门禁与六轮原负载均留证。UI由CUA原生Close/PID消失验证，Core随后health200/配置不变并正常退出。b313条数按钮完整键盘焦点未可靠观察；自动刷新实际响应/窗口active未独立测，1px明细表头、字体/完整IME/AX/跨平台仍待验；不能将中间40a或后续字体候选证据混作本版通过。


Studio字体与六旧皮肤（UI57）：七原始静态face/SHA/原族名/权重/许可证已查，Studio首次加载一次、Noto主字体/Inter已检查角色及process-local CJK named cascade、默认Mono/六旧键和旧native sans恢复路径保留。最终a51两尺寸三统计页/万单位/六皮肤与供应商/模型可见区抽查、195tests193pass2ignored及strictclippy/Inteltypecheck/release、四轮原负载通过专项验证；初始edb六轮和204ms marker增量保留，删除重复全族枚举后复测，不冒称因果优化。Core配置未变、UI关闭后Core200，再Core/fixture wait0，GUI所有PID消失。实际每字形PS身份/Figma字体版本、亿UI、模型小窗下方字段/其他协议完整状态/所有40帧/完整IME/AX/Windows与Intel硬件/HiDPI仍未验。准确范围和启动/RSS/文件尺寸成本见QA57。

供应商整帧布局（UI58）：最终ec7e六个Rust/GPUI文件、五块主要区域及模型Studio列宽对齐；真实两尺寸、草稿删后取消恢复、模型/端口/协议/获取/新增弹窗取消、中英/Mono→Studio，以及各一次Test成功200/SSE与失败503均留证。Test原有先保存再测试语义保持，全部原字段递归读回一致，字节因新增序列化默认字段不同；16条SQLite按来源/HTTP独立核对。256容量End到fill-250、末行编辑取消、Add禁用及小窗footer可达。默认头部只隐藏原不可用Remove，选中即显示同一处理器；不删除选择/键盘/虚拟化。统计共享标题1px已在概览/明细原图修正。编译五门禁与四轮原负载完成，GUI关后Core200再正常退出，准确数据见QA58；独立连接徽章、输入状态颜色、原生边框/AA及完整IME/AX/跨平台仍待验，不升级为整帧最终通过。

连接徽章（UI59）：实际TestReport与payload供应商ID、确认配置和单模型绑定；编辑隐藏旧结果、Cancel可恢复同配置历史记录、503替换成功、Save失败未完成、复制留空Key/能力/混合effort按原Save继承且新ID重绑定、重开不保留、Core控制失联清除/恢复不自动探测。11状态测试及204通过/2既有忽略、最终53f两尺寸/自然重绘、分版本真实交互和四轮原负载留证，具体范围见QA59与source-scope；不改Core/Test调用顺序，不将Core运行视作上游在线。完整视觉/键盘/IME/AX/平台仍待。

连接输入默认样式（UI60）：Studio-only原生Input文本/背景/边框/10px圆角与19px行高、预设/协议触发器文字/圆角接入；排除思考菜单和旧皮肤分支，不改状态/业务处理。最终7d7名称Cancel、密码显隐再遮罩、菜单Esc、两尺寸/语言与Mono恢复、Test200/SSE、原字段及1/500/50/550读回和四轮原负载留证；两次中间候选颜色失败不算通过。busy未实际捕获/操作，焦点浅与眼睛左5px、AA/普通按钮/完整IME/AX/平台继续留差异，不作整帧终验。

输入焦点/眼睛/选区与busy（UI61）：三Studio输入FocusHandle+Focus/Blur显示/清除原语义实线，原24px显隐按钮/17pxSVG位置与键盘描边修正；Studio原生选区用22%强调色，六旧皮肤分支保持。最终63f实际Tab到Key/眼睛、Return/Space显隐回mask、Cmd+A/Backspace/Cancel、两尺寸与Mono恢复；受控控制转发等待中8项视觉置灰、仅预设点击未开菜单，AX无enabled字段，其他disabled未逐个操作。仅一次真实Core Test200/SSE/1条550Token、原字段保留与四轮原负载留证；原100s产品超时不改，人为51.438s不作性能。普通按钮完整状态/IME/AX/平台等仍待，不作整帧通过。


四协议整帧首审与共享表单（UI62）：仅两Rust渲染文件，Studio标签几何/左对齐、原生selected+active白底、徽章色/Inter、字段文字和标题光学位置修正；load/read/parse/validate/switch_to与Apply/Save处理保持。最终db899四协议×两尺寸原始PNG、四tab各Return/Space、Responses非法文本往返/取消重开、Anthropic128正确拒绝→1024合法、四协议Apply与CmdS分步及九Core快照均留证。49个原配置节点递归相同，2供应商5模型0请求；UI关后Core200再正常退出。五编译门禁/204通过2忽略及P/C/C/P原负载四轮完成；旧63f、新db899性能与小样本限制见QA62。两个中间候选白底未生效不作通过，scope/原始照片与Core无副作用证据保留。四帧改为已审查保留差异，仍有Select/映射标题光学、原生X/scrim及完整状态/键盘/IME/AX/平台差异，不宣称整帧最终验收。


四类业务弹窗整帧首审（UI63）：仅model_picker/sync_dialog/price_layout/detail_layout四Rust视图；Studio Picker复用现有NativeBaseCheckbox引擎与原12pxcheck资产/15px指示器/15px间隔，事件适配后仍走原set_picker_checked/list focus，六旧皮肤保留KitCheckbox。徽章/标题/字段文字、详情IDInter及行光学位置修正，最终769全门禁0/204pass2ignored。八默认原图与五交互原图、鼠标/Space/筛选/取消/Add到draft→显式Save的5→6模型边界；仅隔离HOME同步6模型、原49节点保持；价格-1拒绝再1.5/6/CNY/cache0/备注保存，原14费用快照不变，未来本地请求CNY0.00105/550Token，详情内滚/更多/Esc/Close通过。九Core阶段与21PID终态、四轮原负载留证。52ff中间版childLabel盖掉disabled前景不算通过，修为div继承并重建重验；注意raw observer字段installed_profiles实为安装模型条目，补充证明解释6条/单私有models.json。Mono→Studio抽查不等于六皮肤全部状态，Restart/完整键盘/IME/AX/平台及精确视觉仍待，不作整帧终验。

### UI64 共用弹窗专项（2026-10-31）

- 七类原生关闭外观覆盖12状态：四协议、Picker、模型编辑、同步/取消同步、两供应商删除、价格、详情；保留Base Cancel及原on_ok/on_close。实际发现0b焦点X的Enter被Confirm截走，修复为关闭控件专属Confirm→Cancel；5c63真实Return/Space通过。
- 价格42px原生控件槽、字段分列/行距/页脚、备注滚动取消及两尺寸负价拒绝通过；详情行线/小窗More→Esc/X通过。两尺寸默认24图及3补图、配置逐字节不变、0规则/0安装/私有WorkBuddy文件仍空/14条7700Token见[GUI](benchmarks/modal-chrome-final-gui-validation.json)、[独立数据](benchmarks/modal-chrome-isolation-data-proof.json)。
- 原始P/C/C/P负载与一对相同短操作历史的30秒对照未见明显回退；n2/n1不作等价/优化/泄漏结论，见[负载](benchmarks/modal-chrome-performance-summary.json)、[匹配序列](benchmarks/modal-chrome-matched-performance-summary.json)。源码保留六旧皮肤分支，本版只复测Mono回Studio；其余全量键盘/IME/AX/HiDPI和平台门禁继续保留。


UI65设置与主题/协议菜单（最终e6abd）：7个Rust呈现/资源文件和6个原22px色块，原21个listener体保持，Core/schema/依赖/锁/工作流不改。五帧完整首审与两尺寸11张最终原图、宽窗色块原色盒对齐、7项/Mono恢复/Esc外点/协议草稿保留和显式回Chat、窄窗下方卡片与诊断可达已验证。c2eb另真实搜索3→1→3、价格2→2.5→2保存回读及180→90→180，API/SQLite证明同规则revision3/settings3和历史14行全字段摘要不变；e6abd仅呈现/取消复验，合法保存没有伪称重复，最终独立证明rev1和原数据不变。204通过2既有忽略/五门禁0、四轮原负载与24PID退出见[QA65](baseline_and_qa.md)、[最终GUI](benchmarks/settings-menu-final-gui-validation.json)、[保存数据](benchmarks/settings-menu-price-and-retention-data-proof.json)、[性能](benchmarks/settings-menu-performance-summary.json)。19待首审/21已审有差异，40完整终验继续；保留分页/额外动作、Studio第七入口与动态真值，不以删功能贴静态图。Mono字体/菜单及精细整帧、完整键盘/IME/AX/平台待收口，后续按画板顺序从52:2闭环。


UI66使用概览：仅overview/trends/recent原生布局，保留原2处理器、Core/统计计算/配置/依赖不变。1bfd完整真实0→22→23自然更新、模型5/供应商2/来源0与22/日期、四图例与币种、七指标内滚末项/费用趋势/Esc、最近200详情550及503失败已通过；475f只补两处clock/标题paint后另验两尺寸/下方滚动/详情/失败，版本边界明确。原22请求与22费用快照、设置/价格与Core配置逐字段SHA保持；204pass2ignore/五门禁0与P/C/C/P各n2留证，合计RSS83.21→85.47MiB且范围重叠，不作优化或等价结论。见[QA66](baseline_and_qa.md)、[最终GUI](benchmarks/overview-closure-final-gui.json)、[历史保护](benchmarks/overview-closure-final-data-preservation-proof.json)、[性能](benchmarks/overview-closure-performance-summary.json)。实际标签26/28和最近行光学差异、图表hover/完整键盘IME/AX/平台待终验；仍在52:2同帧收口，剩余最终验收40。


UI67使用概览同版收口：仅recent原生margin/gap/28px模型标签，原详情listener与渲染方法外字节保持。冻结e25a全门禁0/204pass2ignore；完整真实0→22→23、模型5/供应商2/来源循环、日期/清筛选/侧栏搜索、来源方向键→Tab焦点/Return/Esc/ShiftTab、币种/费用趋势/七指标内滚/200详情及503失败、两尺寸十原图通过。CUA图表真实点拖动显示22/22/22与11000/1100/—，不冒称无按键hover；模型下拉无文本搜索框，空态详情仅视觉置灰。原22请求/22费用快照/设置/价格及Core配置SHA保持，27测试PID退出。原负载八轮保留最初v2空闲差异；统一尺寸/操作后空闲合计+0.58MiB，组合峰值+12.63MiB仍记成本，current2直连和代理同慢，短轮不作优化/等价/长时性能结论。详见QA67和overview-recent-closure证据。已知最近结构差异收口，完整状态/hover/键盘IME/AX/六皮肤与平台专项继续；剩余最终40，后续56:8整帧核对。时间线待较大完整功能节点统一刷新。


B01 / QA68完整批次：最新19份官方完整设计与同一2a12，38主图/9补图+四行定向补验、两尺寸CRUD/取消/保存读回/删除幸存/空态/64搜索与禁用通过；原14请求/费用全字段和非providers根配置保持。原状态/选择delegate/动作处理器保持，Core与配置逻辑不改；复制Key按原Draft继承，不新增清空行为。206常规测试204pass2ignore，另外真实Core发现/创建契约实际通过；四轮原负载均0，合计RSS+3.91MiB/峰+8.505MiB/marker+117.53ms成本留R03。忙态点击CUA失效、补验恢复后多一次Test如实记录，未冒称禁用拦截通过，R01统一复验。19/40界面签收、剩21；R01–R04未通过。详见b01-design-audit/native-consolidated/native-supplement-gui/performance-summary及artifacts。


## B02 / UI69 整批保护记录（2026-10-31）

模型发现/选择/容量、范围与Cmd/F2目标、能力和四协议真实保存往返已按同87db验证；取消/Apply/Core边界与原14历史保持，旧配置的9条默认序列化补全单列。6×2正式原生PNG及2底部补图、四轮原负载与完整生命周期留证；原CUA JPEG不冒充PNG。全部Core/配置schema/依赖及原事件保持，未触及真实WorkBuddy/登录项/外链。完整IME/AX/hover与跨平台、长时性能仍归R01–R04。[同版证据](benchmarks/b02-native-consolidated.json)、[字段审计](benchmarks/b02-default-serialization-audit.json)、[性能](benchmarks/b02-performance-summary.json)。

## B03 / UI70 整批保护记录（2026-10-31）

- 冻结4b，仅3Rust呈现文件、7原owner监听保持，Core/配置格式/依赖/锁文件未改。
- 隔离1000真实回环请求，分页/筛选/真实503与缺失用量；价格新建/编辑/删除/新ID恢复和后续请求快照逐字段核对。
- 合成导入7Builtin+1Custom、暂停恢复/去重/文件读错自然恢复；最终1014请求/1006费用、Core配置字节不变、全部已保护历史保持。
- 7帧14原生PNG和P/C/C/P原负载短测通过；初始Mono、夹具sessionId错误、原直接删除行为与未证实列拖动明确留档。原生输入R01、平台R02、长期性能R03和安装RC R04独立。
- 详见[QA70](baseline_and_qa.md)及[b03-native-consolidated](benchmarks/b03-native-consolidated.json)、[独立导入](benchmarks/b03-import-final-validation.json)、[性能成本](benchmarks/b03-performance-summary.json)。

## B04 / UI71 整批保护记录（2026-10-31）

- 同d08候选，只修语言失败后的视图回滚；原七事件、Core、配置格式、主题键与代理业务逻辑保持。
- 三语言/七主题、隔离开机真实失败重试、更新受控状态和独立一次真实公网检查、普通窗口恢复及模态遮罩；12张实际两尺寸PNG。
- 原14请求/14成本全字段保护至15/15，业务配置除声明语言/开机保存及既有Serde默认外保持，统计设置与价格不变；Core/夹具退出0、UI PIDgone。
- P/C/C/P原负载各n2；全部性能成本和R01输入/R02系统/R03长期性能/R04安装发行边界详见[QA71](baseline_and_qa.md)、[交互与数据](benchmarks/b04-native-consolidated.json)、[性能](benchmarks/b04-performance-summary.json)。

# Studio V2 功能保护矩阵

状态区分：源码确认（旧实现存在）、自动验证（当前分支测试）、GUI 验证（隔离数据真实应用）。未验证不能视为通过。

| 功能 | 原实现落点 | 当前状态 / 必须补验 |
| --- | --- | --- |
| 七功能工具栏独立定位与遮罩层级 | ui.rs::render/app_controls、theme.rs | top8/right32/300×36布局与原七项处理器逐字核对；最终049版1440×900及解锁后1100×700新实拍、Skin/Language上缘点击、Sync遮罩拦截/Esc及Restore通过；Cmd/Ctrl+S为首轮71ae证据。Skin菜单首次Esc未关闭，完整菜单键盘路径待审计 |
| 六套旧皮肤 + 可选 Studio 靛蓝、保存失败不切换 | theme/skin.rs、ui/skin_controls.rs | macOS 七项选择/原键保存、默认 Mono、配置 SHA 不变、Studio 重开恢复 Passed；非法目录保存失败由既有自动测试覆盖 |
| 开机启动、失败回滚 | ui.rs::app_controls、核心 action | 源码确认；待隔离验证，不能修改用户真实登录项 |
| 检查更新、busy/error | ui.rs::app_controls、tick | 源码确认；待 mock 与 GUI |
| 系统/中文/English | ui.rs::language_menu、set_language | macOS 隔离 GUI 菜单/中英实际切换 Passed；跟随系统待验 |
| 恢复 1440×900、退出全屏 | ui.rs::restore_window_size | macOS 缩小后恢复 Passed；最大化/全屏待验 |
| GitHub 与新 Issue | ui.rs::app_controls | 源码确认；外部 URL 保持 |
| 最小窗口 1100×700、系统按钮/拖动 | main.rs、TitleBar | macOS 两尺寸/中英/缩小恢复 Passed；拖动未测位移；Windows 待验 |
| 供应商 0/1/2/20/64、固定 CRUD | ui.rs::sidebar、provider_filter.rs | macOS 5 种数量、末尾搜索/空结果/清空、64 滚动/新增复制禁用 Passed；最终视觉待续 |
| 新增、复制、删除确认与取消 | ui/provider_delete.rs、ui/sidebar.rs、ui.rs、state.rs | 原生532删除确认/已保存目标与dirty说明、Cancel/X/Esc与草稿保护、默认Return不删/Tab确认Enter或Space、Core失败重试、仅删A保留B及最终删B到空列表、无自动同步/哨兵不变、两尺寸/合法长名实拍与四轮性能 Passed；headless未知/重复删除配置不变。未实际双击/hover按压，合法长名未触发内部滚动；新增/复制独立pending、取消/恢复/重开、来源dirty先保存、Save/Test新目标/新route与mixed effort、零供应商发现及可见小窗Save已实机Passed；完整IME/无障碍/跨平台待续 |
| 资料编辑/取消与历史草稿 | ui/provider_edit.rs、ui/providers.rs、ui.rs | 原位编辑/Cancel恢复进入前完整Draft及Selection、保留既有未保存模型/协议、A/B独立模式、失败Save保持/成功Save仅结束当前、Name Return不误提交、按钮Enter/Space、关闭重开草稿、两尺寸/已保存态实拍与4轮性能 Passed；快照仅窗口内，完整IME/无障碍与跨平台待续 |
| 新增/复制保存完成提示 | ui/providers.rs、ui/provider_new.rs、ui.rs | 真实Save后宽窗绿色Saved/Saved copy、小窗单状态、dirty/非法Save/Cancel、切B返回不串标、关闭重开genericSaved、两尺寸与四轮性能 Passed；会话来源不写配置，busy瞬态未实测 |
| 未保存切换/恢复草稿 | ui.rs::capture、switch_provider、Drafts | macOS 搜索不切换、切换保留、正常关闭重开恢复 Passed；提示表现待续 |
| 预设搜索、名称/URL、Key 显隐 | ui.rs、presets.rs、InputState | 源码确认；不得把 Key 加入过滤/截图/日志 |
| 四协议不同字段、认证、tokens/思考 | ui/protocol_dialog.rs、protocol_options.rs、ui/providers.rs、main.rs | 原生四tab/620×724及小窗、Chat/Responses Azure、Responses none/max、Anthropic budget/adaptive/8192输出、Gemini budget/level、临时非法文本往返保留、Apply只改draft/显式CmdS后Core对象、局部错误8秒不外溢、X/Esc/标题空白Esc/Select优先Esc、标签Enter/Space不Apply及键盘选未选tab、Tab/ShiftTab/带焦点缩窗可见、最终98fb四协议×两尺寸及错误图 Passed；完整IME/无障碍/跨平台待续 |
| 模型发现 4096、总量 256、手动编辑 | ui/model_picker.rs、model_table.rs、ui/providers.rs | macOS 原生picker两尺寸、38/4096候选虚拟化、过滤/全选/反选/清空/仅已选/已有禁用、空态、Tab/方向/Home/End/Page/Space/Enter、Cancel/X/Esc、确认仅草稿/显式保存、其他供应商253+当前1到256与第257拒绝 Passed；原新增/编辑取消/空模型/256手动新增禁用/128行模型表 Passed，完整IME/无障碍与跨平台待续 |
| 模型能力自动/手动、思考档位与Token/上下文 | ui/model_editor.rs、model_options.rs、ui.rs | 原生两尺寸；builtin/unknown与未知图片警告、手动等待不覆盖、六能力依赖与Chat/native自定义协议区别、五档/default及数字/上下文/空ID校验、局部错误持久、草稿确认/显式保存、Cancel/X/Esc与Help Esc、Tab/ShiftTab、带焦点缩窗及错误高度变化后可见、滚动等待不回跳 Passed；完整IME/无障碍/跨平台与精细审计待续 |
| Shift/Cmd/Ctrl 多选、快捷键、焦点/IME | state.rs、model_table.rs、main.rs | macOS Shift/Cmd 多选、Delete/Backspace、Cmd+S、编辑聚焦 Passed；Windows Ctrl/完整 IME 待验 |
| 同步/取消、备份恢复与重启 | ui/sync_dialog.rs、ui.rs、backend.rs、workbuddy_restart.rs、核心 | 原生确认/取消与容器Enter、仅同步只保存当前草稿并写所有saved、保留其他草稿/无关条目、仅移除不保存draft并恢复原对象、非法文件不变及重试 Passed；guard核对失败不进入restart/成功后明确部分成功，随机唯一bundle组件正常重开/拒退不强杀/多实例不碰 Passed；真实WorkBuddy重启、完整并发busy、无障碍/跨平台未验 |
| 概览/明细/汇总/设置、全部/内置/自定义 | statistics/*、source_filter.rs | macOS 三报表/设置返回、2/0/2来源、日期/刷新/更多浮层 Passed；概览四KPI/七详细指标/空值/趋势四图例/计费和小窗滚动 Passed，最近请求/详情ID与过滤 Passed；分析表/Token构成/缓存80%与空值、12模型真实分页和两种实际排序、1440并排/1100两卡滚动与独立位置 Passed；Tooltip及完整视觉审计待续 |
| 日期、分页、每页条数、固定表头 | statistics/pagination.rs、usage_table.rs | 日期草稿取消/应用、每页10/20条、上一页/下一页/输入跳页 Passed；明细八列与分析首/末列按钮在1100同步表头/表体取回最右列 Passed，保留服务端分页/虚拟表体；最终1100中英文两完整行、分页/导航不换行，英文状态完整可读；每页Select即时中英切换保值、10/20翻页及非默认20关闭重开恢复 Passed，完整本地化仍待审计 |
| 未知/缺失、缓存、Token 完整性 | usage_chart/continuity.rs、usage_table.rs | 既有自动测试 Passed；GUI保留缺失/单点提示、图例独立；缓存子集和未知不造零语义保持 |
| CNY/USD/积分、价格快照与规则 CRUD | statistics/trends.rs、statistics/price_layout.rs、statistics.rs、stats_backend.rs | 计费摘要CNY真实零/USD缺失、独立积分/默认零价及费用趋势 Passed；价格规则新建/编辑取消与保存/菜单删除、精确搜索、21规则20/50真实分页、旧金额不变/新revision生效及更新失败持续反馈；原生新建/编辑两尺寸、四费率/备注/身份锁定、Tab/Shift+Tab自动滚动/Enter/Cancel/X/Esc、校验和服务端revision冲突恢复 Passed；完整IME/无障碍/跨平台与精细视觉待续 |
| 采集、保留天数、本地导入去重 | statistics/settings_capture.rs、statistics/import_controls*、workbuddy_import.rs | 原生双卡及窄窗纵排；CNY/USD、30/90/永久/365天、REQUEST/OBSERVE、21供应商20/50分页与独立滚动、摘要暂停/恢复真实请求、导入暂停追加再启用、六计数/实际checkpoint、完整核对/分类入口、全局截止清理预览取消/确认及源文件不变 Passed；全量键盘/IME/无障碍与跨平台待续 |
| 真实代理状态、计数、端口 | ui/relay.rs、core_state、status_bar | macOS 合成2供应商/6模型/1请求、暂停核心未响应/恢复、复制完整端点 Passed；端口0/65536拒绝、确认Esc/取消不重启 Passed；真实重启/回滚待验 |
| 最近 100 条日志、清空/复制/滚动 | ui.rs::snapshot、ui/relay.rs、ui_components::TerminalLogs | macOS 深色只读/复制/清空、无新日志不抢首部/新日志到底 Passed；Studio 补验发现选区覆盖后已修复，Mono/Studio 实拍选区可读；100条上限源码确认 |
| 最近请求、详情与独立读身份 | statistics/recent.rs、stats_backend.rs | 167自动回归；同核心22记录、最新ID/详情、明细第2页不误用、来源/供应商与快速切源、小窗Esc Passed；请求最小10条仅取首条 |
| 请求详情、元数据与本次单价 | statistics/detail_layout.rs、statistics/detail_metadata.rs | 原生650×770/两尺寸、15行摘要、原metadata与长ID/模型/价格快照完整换行、真实代理/导入/连接失败/SSE与点数/未知值、not-found同IDRetry与8秒错误持久/新记录恢复、Esc/X/Close及Popover Esc、64位ID复制并恢复剪贴板 Passed；Tooltip视觉/完整无障碍/跨平台待续 |
| 关闭 UI 释放、托盘/核心续存、单实例 | main.rs、原生托盘、backend.rs | 正常关窗 UI 退出/核心 health 200/重开恢复 Passed；托盘唤醒及单实例待验 |
| 外观/中英/HiDPI/无障碍 | theme.rs、ui_components.rs | macOS 七主题选择与中英两尺寸 Passed；完整 HiDPI/IME/无障碍及 Windows/Intel Mac 未实际验证 |

此矩阵随每个小功能提交更新；不将核心未改动当作运行回归通过。

模型选择 `59:11` / `59:216` 已提取原生 `ui/model_picker.rs`：Kit虚拟List与Checkbox支持4096候选，固定批量操作/底栏与小窗自适应，保留已有项、过滤批选、草稿确认后显式保存和跨供应商总256约束。两尺寸、键盘、错误/空态与取消已验；不改变模型表原有Shift/Cmd/Ctrl多选。完整IME/无障碍/跨平台仍待最终审计。

模型能力编辑 `59:279` / `59:484` 已提取 `ui/model_editor.rs`，保留设计未呈现的默认思考Select、真实元数据来源、未知图片警告及完整媒体/Chat URL说明；六能力Flag逻辑、模型能力五档与原验证器保持。局部错误不再写入Runtime，固定底栏/原生滚动/缓存焦点组与布局后单次校正解决缩窗及错误时的遮挡；全面IME/无障碍/跨平台继续保留待办。

同步确认 `60:27` / `60:232` 与取消同步 `68:350` / `68:555` 已提取 `ui/sync_dialog.rs`，三按钮保持原save/install/restore与restart job顺序。正常隔离GUI验证文件/草稿语义，重启产品路径以denial-only guard核对部分成功；无guard的随机bundle脚本独立验证生命周期基础函数，未将其冒充真实WorkBuddy重启。启动地址不一致确认仍为原分支，后续视觉施工与整帧审计继续。

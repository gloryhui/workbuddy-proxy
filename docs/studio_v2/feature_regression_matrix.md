# Studio V2 功能保护矩阵

状态区分：源码确认（旧实现存在）、自动验证（当前分支测试）、GUI 验证（隔离数据真实应用）。未验证不能视为通过。

| 功能 | 原实现落点 | 当前状态 / 必须补验 |
| --- | --- | --- |
| 六套旧皮肤 + 可选 Studio 靛蓝、保存失败不切换 | theme/skin.rs、ui/skin_controls.rs | macOS 七项选择/原键保存、默认 Mono、配置 SHA 不变、Studio 重开恢复 Passed；非法目录保存失败由既有自动测试覆盖 |
| 开机启动、失败回滚 | ui.rs::app_controls、核心 action | 源码确认；待隔离验证，不能修改用户真实登录项 |
| 检查更新、busy/error | ui.rs::app_controls、tick | 源码确认；待 mock 与 GUI |
| 系统/中文/English | ui.rs::language_menu、set_language | macOS 隔离 GUI 菜单/中英实际切换 Passed；跟随系统待验 |
| 恢复 1440×900、退出全屏 | ui.rs::restore_window_size | macOS 缩小后恢复 Passed；最大化/全屏待验 |
| GitHub 与新 Issue | ui.rs::app_controls | 源码确认；外部 URL 保持 |
| 最小窗口 1100×700、系统按钮/拖动 | main.rs、TitleBar | macOS 两尺寸/中英/缩小恢复 Passed；拖动未测位移；Windows 待验 |
| 供应商 0/1/2/20/64、固定 CRUD | ui.rs::sidebar、provider_filter.rs | macOS 5 种数量、末尾搜索/空结果/清空、64 滚动/新增复制禁用 Passed；最终视觉待续 |
| 新增、复制、删除确认与取消 | ui.rs、state.rs | 源码确认；待独立草稿与取消路径验证 |
| 未保存切换/恢复草稿 | ui.rs::capture、switch_provider、Drafts | macOS 搜索不切换、切换保留、正常关闭重开恢复 Passed；提示表现待续 |
| 预设搜索、名称/URL、Key 显隐 | ui.rs、presets.rs、InputState | 源码确认；不得把 Key 加入过滤/截图/日志 |
| 四协议不同字段、认证、tokens/思考 | protocol_options.rs、model_options.rs | 源码确认；待各协议 GUI |
| 模型发现 4096、总量 256、手动编辑 | model_table.rs、ui/providers.rs | macOS 新增/编辑取消/空模型/256 禁用/128 行虚拟化 Passed；发现上限及全部错误态待续 |
| Shift/Cmd/Ctrl 多选、快捷键、焦点/IME | state.rs、model_table.rs、main.rs | macOS Shift/Cmd 多选、Delete/Backspace、Cmd+S、编辑聚焦 Passed；Windows Ctrl/完整 IME 待验 |
| 同步/取消、重启提示及失败回滚 | ui.rs、workbuddy_restart.rs、核心 | 源码确认；只对合成 WorkBuddy 目录验证 |
| 概览/明细/汇总/设置、全部/内置/自定义 | statistics/*、source_filter.rs | macOS 三报表/设置返回、合成记录2/0/2来源与方向键、日期取消/应用、刷新/清除及更多浮层 Passed；报表内容 V2 视觉待续 |
| 日期、分页、每页条数、固定表头 | statistics/pagination.rs、usage_table.rs | 日期草稿取消/应用 Passed，保留服务端分页/虚拟表体；1100右列横向取回未可靠验证，记录问题图待续 |
| 未知/缺失、缓存、Token 完整性 | usage_chart/continuity.rs、usage_table.rs | 源码确认；保留现有统计语义，待自动回归 |
| CNY/USD/积分、价格快照与规则 CRUD | statistics.rs、stats_backend.rs | 源码确认；待 GUI |
| 采集、保留天数、本地导入去重 | statistics/import_controls*、workbuddy_import.rs | 源码确认；待自动回归与 GUI |
| 真实代理状态、计数、端口 | ui/relay.rs、core_state、status_bar | macOS 合成2供应商/6模型/1请求、暂停核心未响应/恢复、复制完整端点 Passed；端口0/65536拒绝、确认Esc/取消不重启 Passed；真实重启/回滚待验 |
| 最近 100 条日志、清空/复制/滚动 | ui.rs::snapshot、ui/relay.rs、ui_components::TerminalLogs | macOS 深色只读/复制/清空、无新日志不抢首部/新日志到底 Passed；Studio 补验发现选区覆盖后已修复，Mono/Studio 实拍选区可读；100条上限源码确认 |
| 关闭 UI 释放、托盘/核心续存、单实例 | main.rs、原生托盘、backend.rs | 正常关窗 UI 退出/核心 health 200/重开恢复 Passed；托盘唤醒及单实例待验 |
| 外观/中英/HiDPI/无障碍 | theme.rs、ui_components.rs | macOS 七主题选择与中英两尺寸 Passed；完整 HiDPI/IME/无障碍及 Windows/Intel Mac 未实际验证 |

此矩阵随每个小功能提交更新；不将核心未改动当作运行回归通过。

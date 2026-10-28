# Studio V2 功能保护矩阵

状态区分：源码确认（旧实现存在）、自动验证（当前分支测试）、GUI 验证（隔离数据真实应用）。未验证不能视为通过。

| 功能 | 原实现落点 | 当前状态 / 必须补验 |
| --- | --- | --- |
| 六套皮肤、保存失败不切换 | theme/skin.rs、ui/skin_controls.rs | 源码确认；默认黑白与旧值保持；待 GUI |
| 开机启动、失败回滚 | ui.rs::app_controls、核心 action | 源码确认；待隔离验证，不能修改用户真实登录项 |
| 检查更新、busy/error | ui.rs::app_controls、tick | 源码确认；待 mock 与 GUI |
| 系统/中文/English | ui.rs::language_menu、set_language | macOS 隔离 GUI 菜单/中英实际切换 Passed；跟随系统待验 |
| 恢复 1440×900、退出全屏 | ui.rs::restore_window_size | macOS 缩小后恢复 Passed；最大化/全屏待验 |
| GitHub 与新 Issue | ui.rs::app_controls | 源码确认；外部 URL 保持 |
| 最小窗口 1100×700、系统按钮/拖动 | main.rs、TitleBar | macOS 两尺寸/中英/缩小恢复 Passed；拖动未测位移；Windows 待验 |
| 供应商 0/1/2/20/64、固定 CRUD | ui.rs::sidebar | 旧列表可滚动；待搜索、容量禁用与各数量 GUI |
| 新增、复制、删除确认与取消 | ui.rs、state.rs | 源码确认；待独立草稿与取消路径验证 |
| 未保存切换/恢复草稿 | ui.rs::capture、switch_provider、Drafts | 源码确认；待用户可见保护与 GUI |
| 预设搜索、名称/URL、Key 显隐 | ui.rs、presets.rs、InputState | 源码确认；不得把 Key 加入过滤/截图/日志 |
| 四协议不同字段、认证、tokens/思考 | protocol_options.rs、model_options.rs | 源码确认；待各协议 GUI |
| 模型发现 4096、总量 256、手动编辑 | model_table.rs、ui.rs | 源码确认；待自动回归与 GUI |
| Shift/Cmd/Ctrl 多选、快捷键、焦点/IME | state.rs、model_table.rs、main.rs | 源码确认；待 GUI |
| 同步/取消、重启提示及失败回滚 | ui.rs、workbuddy_restart.rs、核心 | 源码确认；只对合成 WorkBuddy 目录验证 |
| 概览/明细/汇总/设置、全部/内置/自定义 | statistics/*、source_filter.rs | 源码确认；待 V2 视觉及筛选回归 |
| 日期、分页、每页条数、固定表头 | statistics/pagination.rs、usage_table.rs | 源码确认；保留服务端分页与虚拟表体 |
| 未知/缺失、缓存、Token 完整性 | usage_chart/continuity.rs、usage_table.rs | 源码确认；保留现有统计语义，待自动回归 |
| CNY/USD/积分、价格快照与规则 CRUD | statistics.rs、stats_backend.rs | 源码确认；待 GUI |
| 采集、保留天数、本地导入去重 | statistics/import_controls*、workbuddy_import.rs | 源码确认；待自动回归与 GUI |
| 真实代理状态、计数、端口 | ui.rs::right、core_state、status_bar | 源码确认；未知/未响应不能显示正常 |
| 最近 100 条日志、清空/复制/滚动 | ui.rs::tick、right | 源码确认；待无新日志不抢阅读位置验证 |
| 关闭 UI 释放、托盘/核心续存、单实例 | main.rs、原生托盘、backend.rs | 源码确认；待进程验证 |
| 六皮肤/中英/HiDPI/无障碍 | theme.rs、ui_components.rs | 待 macOS GUI；Windows 未实际验证 |

此矩阵随每个小功能提交更新；不将核心未改动当作运行回归通过。

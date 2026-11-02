# Studio V2 设计映射 · v0.6.1

设计文件 [WorkBuddy Proxy · Studio V2](https://www.figma.com/design/zxDfIQ4lhNcTXrFT7qk8QZ)，页面 `51:2`。后续设计与实现遵守 [交付契约](DESIGN_SPEC.md) 和 [按风险验证规范](../CODEX_FAST_WORKFLOW.md)。下表保留原 40 帧的映射与验收范围；Rust 路径相对 `gpui/src/`。

“已验收”只代表各行记录的候选和场景，不等于所有平台、输入法、辅助技术或发布门禁均通过。完整历史过程及限制见 [固定版本台账](https://github.com/gloryhui/workbuddy-proxy/blob/v0.6.1/docs/studio_v2/baseline_and_qa.md)，当前发行结论见 [v0.6.1 说明](../RELEASE_v0.6.1.md)。

| Frame | 设计状态 | Rust 落点 | 验收 |
| --- | --- | --- | --- |
| 52:2 | 01 / STUDIO V2 · 使用概览 | statistics/overview.rs, trends.rs, recent.rs, layout.rs, usage_chart.rs | 已验收：B03 / QA70，最新设计＋冻结4b03原生7×2与实际1000请求、分页/价格/导入链及数据保护；输入限制与R01–R04独立记录。 |
| 54:2 | 02 / STUDIO V2 · 供应商管理 | ui/sidebar.rs, ui/providers.rs, ui/relay.rs, state.rs, model_table.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；默认A及真实连接200，2供应商5模型；四行小窗末行已独立补验。 全局R01–R04不在此签收内。 |
| 56:8 | 03 / STUDIO V2 · 用量明细 | statistics/*, usage_table.rs | 已验收：B03 / QA70，最新设计＋冻结4b03原生7×2与实际1000请求、分页/价格/导入链及数据保护；输入限制与R01–R04独立记录。 |
| 57:8 | 04 / STUDIO V2 · 汇总分析 | statistics/analysis.rs, layout.rs, usage_table.rs | 已验收：B03 / QA70，最新设计＋冻结4b03原生7×2与实际1000请求、分页/价格/导入链及数据保护；输入限制与R01–R04独立记录。 |
| 58:8 | 05 / STUDIO V2 · 统计设置 | statistics/settings_layout.rs, settings_capture.rs, layout.rs, import_controls.rs | 已验收：B03 / QA70，最新设计＋冻结4b03原生7×2与实际1000请求、分页/价格/导入链及数据保护；输入限制与R01–R04独立记录。 |
| 59:11 | 06 / STUDIO V2 · 选择模型 | ui/model_picker.rs, ui.rs, ui/providers.rs | 已验收：UI69/B02最终87db；最新完整设计/原资产、真实模型及四协议往返、256/4096边界、6×2正确原生PNG与四轮原负载通过；默认序列化和截图格式补拍诚实单列，完整输入/无障碍/跨平台/长时性能仍属R01–R04。 |
| 59:279 | 07 / STUDIO V2 · 编辑模型能力 | ui/model_editor.rs, model_options.rs, ui.rs | 已验收：UI69/B02最终87db；最新完整设计/原资产、真实模型及四协议往返、256/4096边界、6×2正确原生PNG与四轮原负载通过；默认序列化和截图格式补拍诚实单列，完整输入/无障碍/跨平台/长时性能仍属R01–R04。 |
| 60:27 | 08 / STUDIO V2 · 同步 WorkBuddy | ui/sync_dialog.rs, ui.rs, backend.rs, workbuddy_restart.rs | 已验收：B05 / QA72，最新两帧/冻结78原生2×2；同步/移除、草稿与文件保护、受控真实重启/端口部分成功、日志复制清空和正常冷启/用户物理托盘通过；R01–R04全局发布门禁独立。 |
| 60:249 | 09 / STUDIO V2 · 新建价格规则 | statistics/price_layout.rs, stats_controller.rs, stats_backend.rs | 已验收：B03 / QA70，最新设计＋冻结4b03原生7×2与实际1000请求、分页/价格/导入链及数据保护；输入限制与R01–R04独立记录。 |
| 60:475 | 10 / STUDIO V2 · 请求详情 | statistics/detail_layout.rs, detail_metadata.rs, usage_table.rs | 已验收：B03 / QA70，最新设计＋冻结4b03原生7×2与实际1000请求、分页/价格/导入链及数据保护；输入限制与R01–R04独立记录。 |
| 61:289 | 11 / STUDIO V2 · 协议选项 · OpenAI Chat | ui/protocol_dialog.rs, protocol_options.rs, ui/providers.rs, ui.rs, main.rs | 已验收：UI69/B02最终87db；最新完整设计/原资产、真实模型及四协议往返、256/4096边界、6×2正确原生PNG与四轮原负载通过；默认序列化和截图格式补拍诚实单列，完整输入/无障碍/跨平台/长时性能仍属R01–R04。 |
| 61:561 | 11 / STUDIO V2 · 协议选项 · OpenAI Responses | ui/protocol_dialog.rs, protocol_options.rs, ui/providers.rs, ui.rs, main.rs | 已验收：UI69/B02最终87db；最新完整设计/原资产、真实模型及四协议往返、256/4096边界、6×2正确原生PNG与四轮原负载通过；默认序列化和截图格式补拍诚实单列，完整输入/无障碍/跨平台/长时性能仍属R01–R04。 |
| 61:838 | 11 / STUDIO V2 · 协议选项 · Anthropic | ui/protocol_dialog.rs, protocol_options.rs, ui/providers.rs, ui.rs, main.rs | 已验收：UI69/B02最终87db；最新完整设计/原资产、真实模型及四协议往返、256/4096边界、6×2正确原生PNG与四轮原负载通过；默认序列化和截图格式补拍诚实单列，完整输入/无障碍/跨平台/长时性能仍属R01–R04。 |
| 61:1111 | 11 / STUDIO V2 · 协议选项 · Gemini | ui/protocol_dialog.rs, protocol_options.rs, ui/providers.rs, ui.rs, main.rs | 已验收：UI69/B02最终87db；最新完整设计/原资产、真实模型及四协议往返、256/4096边界、6×2正确原生PNG与四轮原负载通过；默认序列化和截图格式补拍诚实单列，完整输入/无障碍/跨平台/长时性能仍属R01–R04。 |
| 62:308 | 13 / STUDIO V2 · 黑白主题示例 | ui.rs, ui/skin_controls.rs, model_table.rs, assets.rs, theme.rs | 已验收：B04 / QA71，最新六帧＋同d08原生6×2、语言失败回滚/皮肤/隔离开机/更新/窗口/遮罩及数据保护，完整输入/平台/发行门禁独立。 |
| 64:335 | 14 / STUDIO V2 · 外观与主题 | ui/skin_controls.rs, theme/skin.rs, ui_components.rs | 已验收：B04 / QA71，最新六帧＋同d08原生6×2、语言失败回滚/皮肤/隔离开机/更新/窗口/遮罩及数据保护，完整输入/平台/发行门禁独立。 |
| 66:320 | 17 / STUDIO V2 · 上游协议下拉 | ui/provider_choices.rs, ui/providers.rs, ui.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；四协议下拉，两尺寸打开/Esc；未改原选择处理器。 全局R01–R04不在此签收内。 |
| 66:538 | 18 / STUDIO V2 · 供应商预设搜索 | ui/provider_choices.rs, ui.rs, presets.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；预设Bedrock搜索、方向高亮及关闭重开；原项确认另有同候选实测。 全局R01–R04不在此签收内。 |
| 66:760 | 19 / STUDIO V2 · API Key 显示演示 | ui/providers.rs, ui_components.rs, ui.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；仅合成Key两尺寸显隐，随后恢复遮罩。 全局R01–R04不在此签收内。 |
| 67:338 | 20 / STUDIO V2 · OpenCode Go 供应商 | ui/providers.rs, ui.rs, ui/provider_feedback.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；B单模型与真实200，两尺寸及小窗下方帮助/底栏可达。 全局R01–R04不在此签收内。 |
| 67:548 | 21 / STUDIO V2 · 思考强度菜单 | ui/provider_choices.rs, ui.rs | 已验收：B01 / QA68，最新设计＋冻结2a12完整路径和两尺寸原图；思考菜单上方锚定，两尺寸完整；Low→Follow实际切换另有同候选证明。 全局R01–R04不在此签收内。 |
| 68:350 | 22 / STUDIO V2 · 取消同步确认 | ui/sync_dialog.rs, ui.rs, backend.rs, workbuddy_restart.rs | 已验收：B05 / QA72，最新两帧/冻结78原生2×2；同步/移除、草稿与文件保护、受控真实重启/端口部分成功、日志复制清空和正常冷启/用户物理托盘通过；R01–R04全局发布门禁独立。 |
| 69:359 | 23 / STUDIO V2 · 编辑模型价格 | statistics/price_layout.rs, stats_controller.rs, stats_backend.rs | 已验收：B03 / QA70，最新设计＋冻结4b03原生7×2与实际1000请求、分页/价格/导入链及数据保护；输入限制与R01–R04独立记录。 |
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
| 104:1378 | 37 / STUDIO V2 · 开机启动已开启（原型） | ui/autostart_notice.rs, ui.rs, assets.rs, theme.rs | 已验收：B04 / QA71，最新六帧＋同d08原生6×2、语言失败回滚/皮肤/隔离开机/更新/窗口/遮罩及数据保护，完整输入/平台/发行门禁独立。 |
| 104:1605 | 38 / STUDIO V2 · 检查更新交互（原型） | ui/update_notice.rs, ui.rs | 已验收：B04 / QA71，最新六帧＋同d08原生6×2、语言失败回滚/皮肤/隔离开机/更新/窗口/遮罩及数据保护，完整输入/平台/发行门禁独立。 |
| 104:1823 | 39 / STUDIO V2 · 语言切换菜单 | ui/language_menu.rs, ui.rs | 已验收：B04 / QA71，最新六帧＋同d08原生6×2、语言失败回滚/皮肤/隔离开机/更新/窗口/遮罩及数据保护，完整输入/平台/发行门禁独立。 |
| 104:2046 | 40 / STUDIO V2 · 恢复默认窗口尺寸（原型） | ui/window_restore.rs, ui.rs | 已验收：B04 / QA71，最新六帧＋同d08原生6×2、语言失败回滚/皮肤/隔离开机/更新/窗口/遮罩及数据保护，完整输入/平台/发行门禁独立。 |

## v0.6.1 增量

检查更新弹窗对应 Figma `127:1027`，实现为 [update_notice.rs](../../gpui/src/ui/update_notice.rs)。共享工具栏保留 `103:442` 七项顺序；各语言/开机启动/窗口恢复浮层挂在各自按钮下方，皮肤弹层位置保持原有设计。

系统字体、原生标题栏避让和跨平台差异以源码及 [设计契约](DESIGN_SPEC.md) 为准，不把旧画板字体家族当作重新嵌入字体的要求。

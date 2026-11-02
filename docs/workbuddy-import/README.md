# WorkBuddy 本地用量导入 · v0.6.1

导入功能读取用户目录 `.workbuddy/projects/<项目>/<会话>.jsonl` 中的统计摘要，后台增量写入本产品 SQLite，与代理记录统一展示。默认关闭，由用户在统计设置中开启；界面刷新不直接扫描 JSONL。

- 代理记录优先；已归属代理的消息不会再生成第二条导入明细。
- 页面按全部、内置模型和自定义模型查看；采集方式可进一步区分代理与导入。不能证明历史归属的自定义模型需明确确认，不猜供应商。
- 导入费用未知时显示“—”；credit 独立列示，不换算为货币或账号余额。独立收费不计入 Token 完整率分母，同笔收费只计一次。
- 仅保存摘要；不保存正文、图片、密钥、工具参数或完整原始 JSONL 行。
- 超过 4 MiB 的单行会跳过；文件名必须与会话标识一致，不把改名当作新会话。

完整操作与限制见 [项目使用说明](../../README.md)。实现入口为 [src/stats/workbuddy_import](../../src/stats/workbuddy_import/)，数据库迁移以 [生产迁移](../../src/stats/migrations/) 为准。自动化合成输入保留在 [scripts/fixtures/workbuddy_import](../../scripts/fixtures/workbuddy_import/)。

旧设计 SQL、任务清单、模型执行提示、线框图和阶段验收包已从当前目录移除，可从 [v0.6.1 固定快照](https://github.com/gloryhui/workbuddy-proxy/tree/v0.6.1/docs/workbuddy-import) 追溯；它们不是当前施工指令或验收状态。

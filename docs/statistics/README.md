# 用量统计 · v0.6.1

统计功能由 Core 采集和写入本地 SQLite，GPUI 经鉴权管理接口查询。界面包括用量概览、请求明细、汇总分析与统计设置，代理请求及可选的 WorkBuddy 导入记录统一展示。

- 仅保存统计摘要，不保存聊天正文、图片、工具参数、API Key 或完整上游错误正文。
- 上游 Token 未知时保持未知；不能用零冒充缺失用量。采集失败不会阻断代理转发。
- 请求接纳时冻结价格快照；后续修改或删除当前价不改变历史及在途请求。具体规则见 [价格口径](PRICING-DECISION.md)。
- 导入费用、WorkBuddy credit 与代理计费分开表达；独立收费项不进入 Token 完整率分母。见 [导入说明](../workbuddy-import/README.md)。
- 筛选、分页和聚合通过服务端接口完成。关闭设置窗口后 Core 继续运行和采集。

## 实现与验证

当前正式版 SQLite schema 为 11、统计 API 为 4；定义以 [store.rs](../../src/stats/store.rs) 和 [dto.rs](../../src/stats/dto.rs) 为准。迁移使用 [生产迁移目录](../../src/stats/migrations/)，不再维护旧设计 SQL 副本。

代码入口、迁移和测试见 [开发说明](DEVELOPMENT.md)。旧设计、八轮复核和施工提示保留于 [固定版本快照](https://github.com/gloryhui/workbuddy-proxy/tree/v0.6.1/docs/statistics)，其中的旧 schema、待办和工作分支不代表当前状态。

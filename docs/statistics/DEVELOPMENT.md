# 统计实现入口 · v0.6.1

## 调用与数据边界

Core 独占 SQLite 写入；GPUI 通过本地鉴权管理协议操作统计，不直接打开数据库。代理转发、统计采集与 UI 查询保持独立边界，采集或存储故障不得阻断聊天。

首次阅读 Rust 的 Java / Spring Boot 开发者，可先按 [统计持久化导读](../source-guide/STATS_PERSISTENCE.md) 跟一遍请求守卫、双槽队列、写入事务、迁移和故障恢复，再进入下表中的具体模块。

用量合并、缓存拆分、价格快照和整数金额见 [用量与费用源码导读](../source-guide/STATS_USAGE_PRICING.md)。

| 关注点 | 当前源码 |
| --- | --- |
| Core 统计协调、生命周期与管理入口 | [src/stats/mod.rs](../../src/stats/mod.rs) |
| SQLite 连接、schema 版本与迁移 | [store.rs](../../src/stats/store.rs)、[migrations](../../src/stats/migrations/) |
| 接口 DTO 与统计 API 版本 | [dto.rs](../../src/stats/dto.rs) |
| 筛选、查询、分页与聚合 | [query.rs](../../src/stats/query.rs)、[query](../../src/stats/query/) |
| WorkBuddy 增量导入 | [workbuddy_import](../../src/stats/workbuddy_import/) |
| GPUI 页面与布局 | [statistics](../../gpui/src/statistics/) |
| GPUI 管理请求与状态 | [stats_backend.rs](../../gpui/src/stats_backend.rs)、[stats_controller.rs](../../gpui/src/stats_controller.rs) |

生产迁移是唯一数据库结构来源。改 schema 时核对旧库、非空 WAL、失败恢复及回滚边界；必须使用隔离的 DB+WAL 副本，不能以只复制主文件的结果证明安全升级。

## 验证

遵循 [按风险验证](../CODEX_FAST_WORKFLOW.md)。先确认实际测试入口和变更影响，再选相关 Core、GPUI 和集成检查；文档清理不触发编译。价格口径见 [PRICING-DECISION.md](PRICING-DECISION.md)。

历史的设计 SQL、验证脚本及阶段任务不再作为当前入口，可从 [v0.6.1 快照](https://github.com/gloryhui/workbuddy-proxy/tree/v0.6.1/docs/statistics) 追溯。

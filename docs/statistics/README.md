# 使用统计功能

日期：2026-10-16。统计功能已接入Rust核心与原生GPUI界面，当前schema6/API2/control7。支持每日模型价格更新与默认请求上游返回用量，已有调用继续保留各自价格快照。早期测试记录见[接手验收](validation-2026-10-16/README.md)和[实现记录](IMPLEMENTATION-STATUS.md)，协议联合验收见[联合验收](../protocol-compat/FINAL-VALIDATION.md)。历史设计与审查保留供追溯，价格口径以PRICING-DECISION为准。

代码调研基线：`main` / `0b2fe84415cb148a498b091e5814ce4fb8d97e35`，核心与 GPUI 包版本均为 `0.5.3`，GPUI Kit 锁定 `=0.7.3`。后续开发开始时重新确认实际 HEAD 和工作区状态。

## 阅读顺序

**历史第八轮复核：[第八轮复核与继续施工](REVIEW-ROUND8-2026-10-15.md)**，GUI已开始实现；先修真实价格对话框与回复接线，再完成剩余交互，并修复单行恢复失败假ready及正常扫描指数退避。

**第七轮历史入口：[第七轮验收复核与剩余施工](REVIEW-ROUND7-2026-10-15.md)**，含2万条恢复回归和明确GUI开发步骤。

**第六轮历史入口：[第六轮复核与继续施工任务](REVIEW-ROUND6-2026-10-15.md)**，先处理v2迁移与批量恢复阻断，再完成界面。

**第五轮历史入口：[第五轮复核与剩余施工要求](REVIEW-ROUND5-2026-10-15.md)**。上轮后端主路径已改善，继续故障补偿、价格草稿及统计界面。

**第四轮历史入口：[第四轮复核与继续开发任务](REVIEW-ROUND4-2026-10-15.md)**，包含U01–U07及本轮原始证据。

**第三轮历史入口：[第三轮复核与继续施工要求](REVIEW-ROUND3-2026-10-15.md)**。覆盖本轮新增T01–T08及上一轮未完成项，附隔离复现证据。

0. **本轮开发入口：[第二轮复核](REVIEW-ROUND2-2026-10-15.md)、[剩余修复与价格改造合并Prompt](NEXT-ITERATION-COMBINED-PROMPT.md)**。继续现有统计worktree，覆盖R01–R07与P01。

0. **价格规则：[最新价格口径](PRICING-DECISION.md)、[价格改造Prompt](PRICING-CHANGE-PROMPT.md)**：当前价可改/可删，逐请求单价和金额快照，缺价默认0。覆盖旧价格区间/引用锁定/重算规则。
1. [详细需求](REQUIREMENTS.md)：范围、计算口径、页面交互、异常与验收。
2. [详细开发文档](DEVELOPMENT.md)：真实代码接入点、Rust 采集、SQLite 写入、统计 SQL、GPUI 布局和绘图。
3. [接口契约](API.md)：请求参数、返回 DTO、鉴权、分页和一致性。
4. [可执行建表 SQL](schema.sql)：8 张表、约束、索引与统计视图。
5. [开发推进 Prompt](IMPLEMENTATION-PROMPT.md)：交给 deepseek-v4.1-flash 的完整任务，要求独立 worktree、分阶段交付。
6. [页面线框图](UI-WIREFRAME.svg)：按现有主题绘制的统计页视觉规格，不是已完成的 GPUI 页面。
7. [设计验证脚本](validate_design.py)：只在临时 SQLite 中检查 DDL、口径、去重和价格约束。

第一轮复核历史资料（下一轮推进以本页最新合并Prompt为准）：[2026-10-15审查报告](REVIEW-2026-10-15.md)、[下一轮修复Prompt](NEXT-ITERATION-PROMPT.md)。审查针对独立开发worktree，未在本设计目录中实现功能代码。

在项目根目录执行：

```sh
python3 docs/statistics/validate_design.py
```

## 核心决策

- 只保存请求统计摘要；不保存聊天正文、工具参数、图片、Key、Authorization、原始上游错误正文。
- 核心服务独占数据库写入；GPUI 只走已有本地管理 API，不直接打开数据库。
- 原始请求明细是统计事实源，第一版不引入第二套持久化汇总表，避免双写不一致。
- 上游 usage 是真实用量来源；Token未知保持 NULL，第一版不做本地 Token 估算。价格缺失则单价/费用默认0，当前价格可编辑删除，历史快照独立不变。
- 默认请求上游返回用量，流式 Chat 请求合并 `include_usage=true`；可按供应商关闭，已保存的选择保持不变。
- 流式响应保持原始字节、状态、背压和取消行为；统计旁路有界、失败不影响聊天。
- 功能编码必须在独立 Git worktree 中进行。正式迁移位于src/stats/migrations；schema.sql保留为设计验证材料。

## 参考与调研记录

- 用户提供的 Aiotto 截图：参考“筛选 → 数据质量提示 → 总览 → 趋势 → 请求/供应商/模型明细 → 定价”的信息层次；不将截图中的数字视为本项目数据。
- [magpie](https://github.com/yetone/magpie)，调研快照 `01c41e8ba2aad586ec885c92bc4b959a49005eda`。参考其 [usage 页面](https://github.com/yetone/magpie/blob/01c41e8ba2aad586ec885c92bc4b959a49005eda/internal/gui/assets/plugins/usage/page.js) 的明细与汇总切换、日期范围和状态反馈。其 Go/JS 实现不是本项目的 Rust/GPUI 实现模板，不复制其数据存储架构。
- [OpenAI 官方流式 usage 文档](https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events)：确认最后 usage 块及流中断可能缺失。第三方兼容接口另建 Mock 和真实供应商验证清单。
- [SQLite WAL](https://www.sqlite.org/wal.html)、[rusqlite Connection](https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html)：用于数据库线程、连接、checkpoint 设计。依赖实际版本由开发时选择并写入 Cargo.lock。
- GPUI API 已对照本机 Cargo 缓存中的 `gpui-kit-0.7.3`、`gpui-component-0.7.3/src/chart`、`gpui-base-0.7.3/src/plot`；以锁定版本源码为准，不能照搬上游 main 分支的新 API。

## 设计验证边界

`validate_design.py` 验证的是设计 SQL 和固定样例。Rust 编译、转发集成、真实上游计费、Windows 打包和 Mac/Windows 人工界面验收，需要后续实现阶段分别完成。本轮不修改实际用户配置或创建实际用量数据库。

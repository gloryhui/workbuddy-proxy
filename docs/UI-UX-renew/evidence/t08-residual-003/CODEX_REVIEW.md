# T08 独立复核

**原实现复核结论：代码与已执行范围通过。** 复核对象为 `codex/ui-redesign` 上 HEAD `7da26855805aa7e0877fda149b8a576efcade27c` 后的未提交 T08 实现与证据；没有修改产品源码。R06 失败路径当时未执行，故该旧结论不代表 R06 全项验收通过。后续补测结论见下方追加记录。

## 复核结果

- 代码 diff 仅将 T08 产品实现放在 `gpui/src/statistics.rs`。`git diff --check` 通过。
- 定价表保留 DTO 的 `label`，空值呈现“手工 / Manual”，第二行以配置中的供应商名称展示上游模型映射。截图 `shots/1440-pricing-labels-004.png` 同时呈现 `models.dev` 和 `manual`。
- 页脚使用页码和当前页实际行数，没有将页数误称总数。`shots/1440-page2-004.png` 显示“第 2 页 · 5 条”。
- 1100×700 的 AX 树和 `shots/1100-toolbar-004.png` 显示搜索、新建、刷新、手动更新及更新摘要都在窗口范围内。
- 输入、保存截图与 `api-verification-004.json` 相互印证：R03 四项价格均有值且 output 非零；R04 价格/label 更新至 revision 2；USD 与 CNY 前缀均覆盖四个价格字段。
- 价格快照语义没有改动。`usage_costs` 数据仍为 `default_zero/0`；R06 清除验证仅在隔离 fixture 上执行。R06 失败路径仍未模拟，按报告保留为未验证限制。
- `logs/fmt-004.log`、`logs/check-004.log`、`logs/build-gpui-004.log` 记录 DSH 修复轮的格式、检查和 release build 均通过；上一轮 test/clippy 日志仍在本目录。

## 环境与范围限制

本机当前没有挂载 `/Volumes/1TB`（`/Volumes` 与 `mount` 核查结果）。因此本次 Codex 复核只核对了 DSH 已保存的检查日志、源码 diff、截图、AX/API 证据和清理记录，没有另起 Cargo 构建或创建新的系统盘临时目录。DSH 修复轮已经停止并清理其专用临时目录，系统盘剩余空间记录为 195 GiB。

未测 Windows、英文界面或其他缩放；这些列入后续 T09 矩阵。状态文档 `IMPLEMENTATION_STATUS.md` 和用户设计文件为既有未提交内容，本次没有纳入。

## R06 后续失败路径复核（2026-10-20）

见 [T08-R06-FAILURE-001 报告](../t08-r06-failure-001/REPORT.md)。Codex 核对 9 张 1440×900 截图、AX/OCR、`failure-results.json` 与 core/UI 二进制哈希。令牌过期 A 场景保留 20 行、core ready，GUI 显示明确错误。B 场景的失败维护事件与 20 行请求/20 条费用留存相符；截图可见错误条和原始 abort 健康栏文案，移除 trigger 后状态仍 degraded。R06 的 B 因错误不具体且健康状态不恢复而维持 **PARTIAL**；没有修改产品源码或把这次失败路径标为通过。

清理复核：CAP 已删除，父会话 `tmp/` 为空；`ps`/`lsof` 无本轮进程或引用，证据约 2.7 MiB，系统盘余量约 195 GiB。清理记录只量到 `run` 删除后的 CAP 残留 192 KiB / 6 个文件；`CAP/run` 删除前大小没有记录，因此本轮总释放量无法准确计算。

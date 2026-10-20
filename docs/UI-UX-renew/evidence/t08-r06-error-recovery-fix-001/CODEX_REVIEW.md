# Codex 独立复核：T08-R06 维护失败恢复

**复核结论：PASS，可提交。**

## 复核范围

- 检查 `src/stats/dto.rs`、`src/stats/mod.rs`、`src/stats/store.rs`、`gpui/src/statistics.rs` 的完整差异。
- 核对失败事务的数据保留、固定安全错误码、重试后的健康状态转换，以及 UI 错误提示与维护事件状态。
- 检查新证据中的 before/failure/recovery JSON、截图、OCR、命令和构建/测试日志。
- 首次复核发现通用 `stats_maintenance_failed` 也会用于自动 Retention 维护，健康栏不能一律称为“清理失败”。同一 DSH 会话已将健康栏改成“统计维护失败；现有记录已保留”，同时保留手动 Clear 专属顶部提示；修订后的 GUI 证据确认两种提示准确。

## 验证

- Codex 独立运行 `cargo fmt --check`：通过。
- Codex 独立运行定向 core 回归 `maintenance_failure_keeps_rows_and_retry_can_complete`：1 passed。
- Codex 独立运行 `cargo test --manifest-path gpui/Cargo.toml --locked`：139 passed。
- DSH 串行 core 全量测试：552 passed；core 与 GPUI release build 通过。
- 隔离 GUI 证据：合成 trigger abort 后，20 条请求和 20 条费用保留、事件为 failed、health 为 degraded；顶部显示 Clear 专属可重试提示，健康栏显示通用维护失败提示；无原始 trigger 文本。移除 trigger 并重新预览/确认后，20 条记录按确认流程删除，事件 completed，health 回到 ready。
- 并行 core 测试日志记录 `automatic_custom_import_preserves_usage_and_restarts_without_confirmation` 偶发 `Degraded`/期望 `Ready`；该测试单独及串行全量通过。未将并行运行结果隐去，也未声称已在基线复现。
- `git diff --check`：通过。Windows、英文、Retina 与 IME 未测试。

## 临时文件清理

- 两个 Codex 验证 CAP 均已移除，父 `tmp/` 为空；保留的测试日志已复制到本证据目录。首次 CAP 的 3 个日志文件共 13,103 bytes；其清理 trap 使用了 zsh 保留变量名后未完成，随即确认无进程/引用并按精确 CAP 路径手动删除。第二 CAP 使用修正后的 trap 清理。
- 两个 DSH fixture 导入产生的精确 `__pycache__/fixture.cpython-314.pyc`（26,520 bytes 与 26,549 bytes）均已删除，fixture 源码和必要证据保留。
- 独立检查时系统盘可用空间为 193 GiB。除测试报告外未留下临时文件。

本复核未修改受保护的 `IMPLEMENTATION_STATUS.md`、既有设计资料或旧证据。未推送。

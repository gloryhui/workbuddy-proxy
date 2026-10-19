# T09 独立复核

**结论：通过记录本轮结果；T09 验收状态仍为 BLOCKED。** 复核基线为 `codex/ui-redesign` HEAD `6533bf1605ad4ab395d896d5cc215142d96b3a99`。本轮没有产品代码改动。

## 核对

- `MATRIX.md` 和 `REPORT.md` 将 Windows、Windows DPI、英文、Retina、缺失画面和未验证交互明确列为 BLOCKED / NOT RUN，没有把单平台证据写成双平台通过。
- 我查看了 1100×700、1280×800、1440×900 三张 macOS SC01 截图：物理尺寸与 `logs/shot-sizes.txt` 一致，内容区有统计数据且窄窗筛选控件换行。其余截图索引、哈希、AX 日志和生命周期记录均已保存。
- X04 的日志记录最小化状态 true/false、关闭 GUI 后 core 仍存活、重开和第二实例唤醒；X03 的 Escape 关闭详情和中文剪贴板值有证据。Tab 未见可确认的焦点变化，报告正确列为未验证。X05 没有性能基线，报告未声称通过性能验收。
- 当前仓库没有 `/Volumes/1TB` 挂载。按 AGENTS.md，本轮没有构建或运行 Cargo 测试；构建、test、clippy 都明确为 NOT RUN。隔离 runtime 是 L05 数据副本，清理日志说明副本已停止并删除，原 L05 与共享 target 缓存保留。
- `git diff --check` 通过；状态文档及未跟踪设计资料保持未暂存。

这次提交仅归档受限环境下已执行的 T09 macOS 子集与阻塞原因，不代表 T09 完成。要解除 BLOCKED，需在 Windows x64 环境和 `/Volumes/1TB` 可用后补跑主矩阵/DPI，并补齐报告列出的未测画面与交互。

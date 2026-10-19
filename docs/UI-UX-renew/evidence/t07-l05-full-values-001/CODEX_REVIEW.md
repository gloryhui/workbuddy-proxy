# Codex 独立复核 — T07-L05

## 结论

PASS。证据支持本轮限定的 T07-L05 结论：相同合成数据、相同 zh-Hans macOS GUI 环境下，T07 前与当前 UI 的日志可见值没有发现底层值变化；表格存在 OCR 未读/误读与截断项，按报告列明的放大截图人工复核后均可解释。10 个代表行的详情 AX 值全部匹配期望。该结论不覆盖 Windows、其它语言或其它窗口尺寸。

## 独立核对

- `row-by-row-compare.json` 可解析；16 行 × 14 字段 = 224 项。实算 verdict 与 JSON summary 一致：214 PASS、7 DISPLAY_TRUNCATED、2 ONE_SIDE_UNREAD、1 OCR_LIMITED；无 VALUE_CHANGED。
- `detail-compare.json` 可解析；10 行 × 20 字段 = 200 项，全部 PASS；每行字段数均为 20。
- `compare-summary.json` 显示改造前后各 18 个滚动屏，三个来源视图均覆盖行 ID 1–16。
- 检视 `verify/pre-t07-10-all-sources-01-11-模型.png`，OCR 标为未读的 `model-01` 截图实际清晰可辨；其余人工复核项及截断案例对应报告所列截图。
- 检查基线元数据、构建产物哈希、清理日志和 DSH 完成摘要：worktree HEAD 为 `f39096ea1575958622802c151c7197f106d59f92`；产品源码 diff 为空；当前共享 GPUI 构建产物哈希与记录一致；本任务 core/GPUI 进程已退出。
- 报告最初引用两份构建日志，但交付目录尚未包含它们。已从本任务专属临时目录原样复制至 `logs/build-pre-t07.log` 与 `logs/build-current-restore.log`，退出码分别为 0；没有重跑构建或改动源码。
- 只读解析/统计 JSON 并检查截图；未改产品代码、数据库或预先存在的状态/设计文件。

## 清理和空间

DSH 清理日志记录删除本任务生成的 `src-pre-t07`（22 MiB）和 `crops`（46 MiB），释放约 68 MiB；临时目录剩余 45 MiB（主要是复核用二进制副本、隔离 DB 与日志），没有本任务进程，`/tmp` 无遗留。Codex 复核期间新增的文件仅是本证据目录内两份构建日志和本说明；未清理缓存或其它会话文件。复核后系统盘可用空间约 200 GiB。

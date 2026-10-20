# T09 macOS 补充验收独立复核

**结论：** 证据归档可接受，macOS 补充验收为 **PARTIAL**；T09 跨平台整体仍为 **BLOCKED**。

## 复核依据

- 分支 `codex/ui-redesign`，基线 HEAD `afba1fbedbbb72e1a2536225eefb6899b29041dd`。复核时 `git diff HEAD -- gpui/src src` 为空；构建二进制与报告中的 SHA-256 相符，未重新构建。
- 检查 SC02、SC03、SC05 Responses、SC06 Anthropic、SC07 Gemini、SC11 与 SC12 截图。各自展示的窗口、表单和对话框与对应结论一致；SC03 的截图/AX 显示本地 mock 返回的两个模型，且确认添加按钮为 0。SC12 只打开确认框后取消，没有执行同步。
- 按 `ACCEPTANCE.md` / T09 矩阵核对编号：SC05 为 OpenAI（Chat 与 Responses/`none`），SC06 为 Anthropic，SC07 为 Gemini。修正后的文件名与报告/命令索引一致，旧标签引用不再残留。
- X03 记录为 **FAIL**：Tab 后按 Enter 使焦点逃逸到背景“新增供应商”，在合成配置中持久化了空供应商。config 前后哈希、快照、AX 与截图均保留。报告明确该写入仅限本轮隔离 CAP，没有把它记为通过，也没有修改产品代码。
- SC03 成功响应的 mock 请求日志原在已清理 CAP 中，未保留该日志文件；返回模型列表仍可由 GUI 截图、AX 与运行日志佐证。此项证据强度低于保留独立请求日志，报告已说明。
- Windows 缺失，英文、Retina/DPI 与真实 IME 未测。macOS 补充结果不能代表全矩阵通过。

## 清理与工作树

- 临时 CAP：`/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t09-macos-supplement-001` 已删除；共享父目录保留且为空。残留 CAP 的 204 KiB / 9 文件已删除；`run` 目录先由夹具清理，但删除前未测尺寸，所以总释放量无法准确还原。报告如实保留这一流程缺口。
- 复核时未发现本轮 GUI/core/mock 进程或该父目录内临时文件。证据目录大小 4,193,611 字节、71 个文件；系统盘约 195 GiB 可用。
- `IMPLEMENTATION_STATUS.md` 的既有未提交改动、未跟踪设计文件和其它既有证据均未暂存。没有产品源码改动，也没有推送。

## 判定

接受本轮 macOS 补充证据并保留 X03 缺陷记录。T09 仍不能标记 DONE：Windows 全矩阵/DPI、英文、Retina 与 IME 仍受环境限制；X03 失败需后续产品缺陷修复与回归。未运行构建或 Cargo 测试。

# Codex 独立复核：T09 X05 corrected repeat baseline

**结论：通过（受限于报告声明的平台和样本范围）。** 复核对象是 `REPORT.md` 的 corrected 10 轮数据；旧 `repeat.py` 预备测量已被报告明确标为筛选点击无效，不作为结论依据。

## 复核项

- 逐一检查 `corrected/ax/cycle-01..10-filter-failed.txt` 和 `...filter-all.txt`：每轮的 AX `请求状态` 依次为 `失败`、`全部状态`。
- 逐一解析 20 份 `corrected/ocr/cycle-XX-filter-{failed,all}.json`：每轮失败态 KPI OCR 为 `0代理`，恢复态为 `20代理`；CSV 每轮 `kpi_failed_0` 和 `kpi_restored_20` 均为 true。
- 检查首轮失败、首轮恢复和第十轮恢复截图：下拉值、0/20 KPI 和列表呈现与 AX/OCR 数据一致。
- 检查汇总与原始 CSV/JSON：10 轮，92.889 s；mock 请求增加 10，usage/cost 行数保持 20/20；config SHA 不变；FD 与窗口计数不变。指标作为单会话原始采样，未据此推断泄漏、性能阈值或长期稳定性。
- 检查 `repeat-corrected.py`：每轮先核验起始值，再核验失败值和恢复值，且将 KPI 交给 OCR 验证；任何状态/KPI 偏差会抛错并写失败证据。当前输出没有 `corrected-failure.json` 与 10 轮均成功相符。
- 独立核实 corrected CAP 路径已不存在；run.json 记录的 core/mock PID 均已退出。证据目录曾残留本轮生成的两个 Python bytecode 文件（26,524、26,554 bytes）和两个 4,096-byte `usage-final.sqlite3` 复制件，逐个检查 `lsof` 无引用后已删除，共释放 **61,270 bytes**。父临时目录为空；系统盘当时约 **193 GiB** 可用。保留截图、AX/OCR、CSV/JSON、脚本和清理日志作为必要证据。
- `git diff --check` 通过；源码 `gpui/src` 与 `src` 相对 HEAD 无变更。本复核只覆盖本 macOS 中文、非 Retina、1440×900 的 10 轮验证；不覆盖 Windows、英文、Retina、长期 soak 或其他交互组合。

## 清理结果

删除仅限本证据目录中上述三个已确认由本轮生成、无进程引用的确切文件。没有清理构建缓存或其它用户数据。`/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp` 保持为空。

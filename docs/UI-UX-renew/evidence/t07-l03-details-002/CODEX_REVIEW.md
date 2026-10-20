# Codex 独立复核：T07-L03-DETAILS-002

## 结论

PASS，限 macOS 27.0.1、中文、非 Retina、1440×900 与本轮 60 行隔离数据。没有修改产品源码。Windows、英文、Retina/DPI、IME、键盘 Enter、非零价格及超长详情滚动仍未覆盖。

## 独立检查

- 核对分支 `codex/ui-redesign`、HEAD `88affbc1f055b6a59330d75890c7ed3785f14c35`；`git diff HEAD -- gpui/src src` 为空。产品二进制为该源码对应的 T08 构建，复用了现有 `target/` 与 `gpui/target/`，本轮没有构建或测试。
- 阅读 `REPORT.md`、夹具/API/GUI/分析脚本、AX 树、OCR、预期值和结果 JSON。60 行夹具的 5 个 GUI 详情 ID 均有对应 API 详情样本；DB 期望与 API 值、AX 可见的 ID/供应商路由/模型/时间/状态/Token/费用相符。
- 独立解析结果：5/5 详情 `all_match` 且关闭后 `closed_clean`；20/50/100 行页及首页/末页边界 6/6 PASS；所有被引用的截图、AX、OCR 文件存在；13 张截图均为 1440×900。抽看 `shots/03-page2-row1.png`，真实窗口中的详情 ID `t07l03-0040`、模型、Token、状态与所在的第 2 页记录相符；`shots/06-page-size-50.png` 显示第 1/2 页。
- `acceptance-summary.json` 原先 `head` 为 `null`，已补为本轮核实的 HEAD。其余结论不因这项元数据修正改变。
- 独立清理复核：任务 CAP 已不存在，`ps`/`lsof` 无本轮进程或引用；父会话 `tmp/` 为空，任务证据目录约 4.0 MiB，`/tmp` 无项目遗留，系统盘剩余约 195 GiB。报告记录删除 CAP 6,644 KiB、19 个文件。

## 运行事故及影响

首次 `fixture.py setup` 因 `CAP` 常量未同步，令 `RUN=CAP` 指向会话缓存根 `.../tmp`；其 `shutil.rmtree(RUN)` 误删了 L05 的本机缓存运行副本 `t07-l05-full-values-001`。我在任务运行中立即停止 DSH，终止并确认隔离 core 退出，检查后只清理了本任务误写到共享根的 `config/`、`core.log`、`run.json`；随后确认 CAP 修正为精确任务路径才允许继续。事故日志保留于 `logs/incident-l05-cache-loss.log`，本轮报告也如实记录。

受影响的是之前保留作复核用的合成二进制副本、隔离数据库和日志；丢失大小约 45 MB，此为原 L05 报告记载值，删除前未独立计量。L05 的版本化报告、复核、截图、比较 JSON/CSV 和构建日志仍完整保存在 worktree。未删除任何产品源码、证据文件、`target/` 或 `gpui/target/`。L05 的既有 PASS 有其已留存证据支持，但原本的本机快速复跑缓存现已不可用；这限制再现便利性，不改变已复核的结果。

本地临时路径错误违反了本项目“只清理本轮确切目录”的要求，须在后续所有轮次继续采用独立目录并先核实 `realpath`、进程和文件引用。本次事故不掩盖，也不扩展本轮 PASS 的范围。

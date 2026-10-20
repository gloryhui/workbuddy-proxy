# Codex 独立复核：T08-R06-FAILURE-001

## 结论

R06 失败路径 **PARTIAL**。两种故障都真实运行在 macOS 的隔离 GUI/Core/SQLite 夹具中，20 条请求与 20 条费用数据均未被删。令牌过期场景 A 的失败提示明确、核心保持 ready；触发器 abort 场景 B 的维护事件记录失败且 UI 仍可预览，但错误文案暴露内部错误码和原始 abort 文本，核心刷新后仍 degraded，因此不把 B 标成 PASS。

## 独立检查

- 分支 `codex/ui-redesign`，HEAD `99ebb17b7df8f40a69d8a47cc06e555271afb8d3`；`git diff HEAD -- gpui/src src` 为空。
- 两个正在使用的二进制哈希与报告一致：GPUI `89049d01990643792cb9445825cd838f88986f4f0b81db284953fba309534ed0`，core `22cacc663af1293252611001e589b6bc49cde885234e42a5e6a200b53697d4ca`。本轮没有构建。
- 独立解析 `failure-results.json`：A 在 62 秒后确认，20 行仍在、健康状态 ready；B 维护事件为 `failed / maintenance_failed / affected_rows=0`，请求和费用行仍各 20，健康状态 degraded，失败后预览仍可打开。
- 目视检查 `shots/03-expired-top.png`：页面显示令牌过期错误且统计记录数为 20。检查 `shots/07-b-failed-top.png`：页面错误条显示 `stats_unavailable: statistics database is not ready`，健康栏同时显示 degraded 和 `t08 synthetic delete abort`。9 张 GUI 截图均为 1440×900。
- B 的错误提示表明失败与数据库不可用，但没有针对“清除历史失败”的可读说明；触发器自造文本显示在健康栏中，删除 trigger 并刷新后 degraded 仍在。报告如实保持 PARTIAL，未将“失败数据安全”误报为整个交互完全通过。
- 旧 T08 报告的 R06 总结已更新为“历史预览/确认/完成 PASS，A PASS，B PARTIAL”，保留原测试事实并关联新证据。

## 清理复核与缺口

- 本轮 CAP 已删除，父会话 `tmp/` 为空；`ps`/`lsof` 无 core、mock、GUI 进程或 CAP 引用；系统盘可用约 195 GiB，证据目录约 2.7 MiB。
- DSH 留存的 `cleanup-size.txt` 记录 `CAP/run` 清理后 CAP 剩余 192 KiB / 6 个文件被删除，但没有在删除 `CAP/run` 前测量其大小。因此总删除量缺少准确值；未用估算冒充精确释放量。
- Windows、英文、Retina/DPI、真实磁盘/权限错误、B 失败后重启恢复均未测。只使用合成夹具，没有真实用户数据或上游流量。

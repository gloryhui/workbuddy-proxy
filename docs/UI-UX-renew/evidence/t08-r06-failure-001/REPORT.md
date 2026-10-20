# T08 R06 失败路径验收（macOS 隔离夹具）

**任务：** T08-R06-FAILURE-001
**分支 / HEAD：** `codex/ui-redesign` / `99ebb17b7df8f40a69d8a47cc06e555271afb8d3`
**结论：** **R06 失败路径 PARTIAL**。A（预览 token 过期 62s 后确认）**PASS**：UI 显示具体失败文本，确认按钮消失、数据仍 20 行、core 仍 ready 可响应。B（合成 SQLite 删除 abort）**PARTIAL，不判 PASS**：维护记为 `failed / maintenance_failed`，20 行与 20 条费用均保留，UI 仍可预览；但错误条为泛化内部码 `stats_unavailable: statistics database is not ready`，健康栏把触发器的原始 abort 文本 `t08 synthetic delete abort` 作为 error_code 展示，且移除触发器并刷新后 core 仍保持 `degraded`、错误文案不消失。未改产品源码，未修复该行为。

## 1. 环境与基线

| 项 | 值 |
|---|---|
| 主机 | macOS 27.0.1（26A434），Apple M1，arm64，非 Retina |
| HEAD | `99ebb17b7df8f40a69d8a47cc06e555271afb8d3`；`git diff HEAD -- gpui/src src` 为空 |
| GPUI 二进制 | `gpui/target/release/workbuddy-proxy-gpui` sha256 `89049d01990643792cb9445825cd838f88986f4f0b81db284953fba309534ed0`（既有 T08 构建，本轮未构建） |
| core 二进制 | `target/release/workbuddy-proxy` sha256 `22cacc663af1293252611001e589b6bc49cde885234e42a5e6a200b53697d4ca` |
| 隔离夹具 | 20 行合成请求 + 25 条价格；一次性 config/DB，core/UI 仅 127.0.0.1 |
| CAP | `.../tmp/t08-r06-failure-001`（TMPDIR/TMP/TEMP 全部指向；realpath 已核对；本轮结束删除） |

## 2. 方法

1. 复制并适配 `t08-residual-003/fixture.py`：仅把 EV/CAP 改为本轮路径；`RUN=CAP/run`，`setup()` 只可能删除本任务 `run`，未触碰旧 CAP、共享 tmp 根或其它缓存。
2. `fixture.py setup` 启动隔离 core + 本地价格目录 mock，插入 20 行、创建 25 条价格规则；记录 core/mock pid、端口、binaries hash。
3. 启动当前 GPUI（同一隔离 config），System Events 置前，1440×900；滚动到“采集与存储”危险区，点击“清除历史统计…”。
4. A：记录预览 20 行；等待 62s（token TTL 60s）后点击“确认清除”；截图/AX/OCR、DB 行数、`stats_health`。
5. B：重新预览；在隔离库注入 `BEFORE DELETE ON usage_requests` 的 `RAISE(ABORT,'t08 synthetic delete abort')` trigger；点击确认；轮询 `stats_maintenance_events` 与 `stats_health`；截图/AX/OCR；移除 trigger 后刷新并再次预览，检查 UI 可用性。
6. 仅删除 CAP；核对 ps/lsof、父 tmp 内容、df。

## 3. A — token 过期后确认（PASS）

| 项 | 观察 |
|---|---|
| 预览 | 20 行；确认前 DB 20 行 |
| 确认延迟 | 预览后 **62s** 点击确认（TTL 60s） |
| UI 错误 | 顶部错误条原文：`stats_invalid_argument: the clear confirmation expired; preview again` |
| 弹窗/按钮 | “确认清除”消失，“清除历史统计…”恢复；无新维护事件 |
| 数据 | `usage_requests` 仍 **20** 行 |
| core | `stats_health.state=ready, record_count=20`，可继续响应 |
| 证据 | `shots/01-preview.png`、`shots/02-expired-confirm.png`、`shots/03-expired-top.png`、`ax/a-expired-*.txt`、`ocr-expired-*.json` |

结论：失败明确、数据保留、core 可继续；提示含内部错误码前缀（`stats_invalid_argument:`），作为 UX 观察记录，不改产品。

## 4. B — 合成删除 abort（PARTIAL，不判 PASS）

| 项 | 观察 |
|---|---|
| 注入 | `CREATE TRIGGER t08_r06_abort_delete BEFORE DELETE ON usage_requests ... RAISE(ABORT,'t08 synthetic delete abort')`（仅隔离合成库） |
| 维护事件 | `id=2, kind=clear, status=failed, error_code=maintenance_failed, affected_rows=0` |
| 数据 | `usage_requests=20`、`usage_costs=20`，原数据未丢失 |
| UI 错误条 | `stats_unavailable: statistics database is not ready`（泛化内部码；无“清除失败/请重试”本地化说明） |
| 健康栏 | `降级运行 数据更新失败 … t08 synthetic delete abort`；`stats_health.state=degraded`、`error_code=t08 synthetic delete abort`、`record_count=20` |
| 移除 trigger + 刷新 | 仍 `degraded`，错误条与健康栏原始 abort 文本仍显示；KPI 仍显示 20 记录 |
| 失败后 UI | “清除历史统计…”可再次打开预览，确认按钮出现（预览 20 行）；未再次确认 |
| 证据 | `shots/04-b-preview.png`、`05-b-running.png`、`06-b-failed.png`、`07-b-failed-top.png`、`08-b-recovered.png`、`09-b-preview-after-fail.png`、`ax/b-*.txt`、`ocr-b-*.json`、`failure-results.json` |

### B 的具体问题（未修复，交 Codex 复核）

1. **错误不具体/非本地化**：UI 只显示 `stats_unavailable: statistics database is not ready`，未说明“清除失败”；维护事件里的 `maintenance_failed` 未呈现为可读文案。
2. **原始数据库错误泄漏**：健康栏直接显示 trigger 的 `RAISE(ABORT)` 文本。若真实 SQLite 错误含路径/SQL 片段，存在泄漏风险；本轮未修改。
3. **失败后状态不自动恢复**：移除 trigger 并刷新后仍 `degraded`；UI 可读、可预览，但写入/维护可用性未恢复验证。
4. 这三点使“失败明确”不完整，故 B 判 **PARTIAL**，不把 R06 失败路径整体标 PASS。

## 5. 复用与新证据

- **REUSE**：`t08-residual-003` 的 R06 预览/取消/成功维护证据（`1440-r06-preview.png`、`1440-r06-done.png`）；本轮未重跑成功清库，避免破坏性操作。
- **NEW**：A 的 token 过期确认、B 的 trigger 注入失败、GUI 错误/健康状态、DB/API 前后计数、失败后 UI 预览可用性；均为本轮隔离夹具。
- 未用直接 API 失败冒充 GUI 错误：所有错误文本/健康状态取自当前 GUI 窗口的截图/AX/OCR；API/DB 仅作计数与事件核对。

## 6. 未测 / 限制

- 仅 macOS 中文、非 Retina；未测 Windows/英文/Retina。
- 合成 trigger 的 abort 文本是本轮自造；未模拟磁盘满、权限、真实数据库损坏、维护重试/重启恢复。
- B 失败后未再次确认清除（会再次触发同一 abort）；写入/维护恢复需重启验证，未执行。
- 未修改产品源码；未提交/推送。

## 7. 清理与删除事故

- 进程：GPUI pid 68295、core 68157、mock 68156 已停止；`ps` 无本任务进程；`lsof` 无 CAP 引用。
- `fixture.py cleanup` 删除 `CAP/run`；随后仅删除 CAP 本身。CAP 删除前未完整记录总字节数（流程缺口）；cleanup 后 CAP 剩余 **192 KiB / 6 文件**（bin 工具、源码副本、tmp、pid 文件）已删除；`run` 大小由 fixture cleanup 删除、未单独测量。**证据目录 2.7 MB 保留。**
- 父 `.../tmp` 目录保留且为空；未按前缀清理；未触碰其它缓存、`target/`、`gpui/target/` 或证据目录。
- **本轮未发生新的误删事故**；上一轮 T07-L03 的 L05 缓存误删事故仍记录在 `t07-l03-details-002/logs/incident-l05-cache-loss.log`，本轮未触碰 L05 相关路径。
- 系统盘可用 **195 GiB**。

## 8. 附件

`fixture.py`、`mock_catalog.py`、`logs/`（fixture-setup、test-a、test-b、cleanup、OCR）、`ax/`、`shots/`、`failure-results.json`。

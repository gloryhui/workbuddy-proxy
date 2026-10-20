# T08-R06 维护失败反馈与恢复修复

**任务：** T08-R06-ERROR-RECOVERY-FIX-001
**分支 / HEAD：** `codex/ui-redesign` / `29e834a64ba314a311c0cec7b69d0107f69f42d3`
**结论：** **R06 B 的反馈与恢复在 macOS 隔离 GUI 中修复并验证 PASS**。合成 SQLite 删除 abort 时，UI 显示“清理失败，历史数据仍保留；请检查存储后重新预览并重试”，健康栏显示“降级运行 / 统计维护失败；现有记录已保留”，不再显示原始 trigger 文本；20 行请求/20 条费用保留，维护事件为 failed。移除 trigger 后通过 UI **重新预览→确认清除**成功：事件 completed(affected_rows=20)，health 回到 ready，UI 显示“采集中 正常 记录 0”。**A 令牌过期路径、清理前确认、正常成功清除均保留**；**Windows/英文/Retina/IME 未测**。

## 1. 基线与范围

| 项 | 值 |
|---|---|
| HEAD / branch | `29e834a…` / `codex/ui-redesign` |
| 基线 source diff | `git diff HEAD -- gpui/src src` 为空；status 仅既有 `IMPLEMENTATION_STATUS.md` ` M` 与未跟踪设计/规划资料 |
| 改动文件 | `src/stats/dto.rs`（+1）、`src/stats/mod.rs`（+26/−?）、`src/stats/store.rs`（+69 测试）、`gpui/src/statistics.rs`（+51/−?） |
| 二进制 | core `9823273191cb06c3f39c46e6f6ec7f4d72fcdc30959c84bd692186adb5f8daa9`；GPUI `5cd6b5a04246be9542b62f4a754799f5c5230877a3729c375657f61f745fa2c2`（复核修订后） |
| 工具链 | rustup stable **1.99.1**；复用 `target/`、`gpui/target/`，未 `cargo clean` |
| 夹具 | 全新隔离 config/DB：20 行请求、25 条价格；core/UI 仅 127.0.0.1；未读真实配置/密钥/历史 |

## 2. 根因

1. **反馈错误**：`writer_thread` 维护失败时把 `failure.message`（SQLite/trigger 原文）写入 health `error_code`；UI 健康栏直接显示它，且主错误条被后续读取的 `stats_unavailable: statistics database is not ready` 覆盖，用户看不到“清理失败”。
2. **恢复缺失**：维护失败后 health 设为 `degraded`，但没有设置 `degraded_since`/恢复标记；`read()` 在非 Ready 时拒绝所有 dashboard/rows 读取，恢复探测只处理写入失败，导致维护成功后也不会回到 Ready，页面永久降级。

## 3. 修复（最小）

| 文件 | 修改 |
|---|---|
| `src/stats/dto.rs` | 新增固定安全码 `ERR_MAINTENANCE_FAILED = "stats_maintenance_failed"` |
| `src/stats/mod.rs` | 维护失败只写安全码，不再把 SQLite/trigger 文本写进 health；新增 `maintenance_failed` 标记，**仅当维护重试成功且没有写入/扫描降级时**恢复 Ready；不伪造 ready |
| `src/stats/store.rs` | 新增回归测试：trigger abort → 行数保留、事件 failed/`maintenance_failed`；移除 trigger 后重试 → 事件 completed、行删除 |
| `gpui/src/statistics.rs` | Failed 状态显示本地化“清理失败，历史数据仍保留；请检查存储后重新预览并重试”；健康栏把同一安全码映射为通用“统计维护失败；现有记录已保留”（该码也覆盖自动 Retention 失败，不能误称用户清理），不再显示原始错误；失败时不强行刷新被 guard 的 dashboard，避免覆盖提示；危险区保留重试入口 |

未改：清理预览 token/TTL、确认按钮流程、正常删除语义、A 路径、价格/统计查询。

## 4. 验证

### 测试与构建

| 检查 | 结果 | 证据 |
|---|---|---|
| `cargo fmt --check` | PASS | `logs/fmt.log` |
| `cargo test --locked -- --test-threads=1`（core） | **552 passed / 0 failed** | `logs/test-core-serial.log` |
| `cargo test --manifest-path gpui/Cargo.toml --locked` | **139 passed / 0 failed** | `logs/test-gpui.log` |
| 并行 core 测试 | 两次分别 550/552、551/552；失败均为 `workbuddy_import::runtime::sync_tests::automatic_custom_import_preserves_usage_and_restarts_without_confirmation`（期望 Ready 得 Degraded），单独复跑/串行全通过 | `logs/test-core.log` |
| release build | core/GUI 均 PASS | `logs/build-core.log`、`logs/build-gpui.log` |

> 并行失败是测试并发下的既有抖动迹象，未归因于本次维护修改；本修复相关的新测试单独和串行均通过。未用并行失败反推产品失败。

### A — token 过期（保留）

- 核心 API 预览后等待 61.5s 再确认：返回 `stats_invalid_argument: the clear confirmation expired; preview again`；health 仍 ready，无维护事件。见 `logs/a-token-expiry.json`。
- 旧 GUI A 证据（`t08-r06-failure-001/shots/03-expired-top.png`）作为 REUSE；本轮没有为非隔离场景重跑。

### B — 合成 trigger abort（修复前症状按旧证据，修复后本轮实测）

| 阶段 | 观察 | 证据 |
|---|---|---|
| 失败前 | health `ready`；usage 20 / costs 20 | `logs/before-failure.json` |
| 失败后 | health `degraded`、`error_code=stats_maintenance_failed`、record_count 20；usage 20 / costs 20；事件 `failed / maintenance_failed / affected_rows=0`；UI 本地化提示；无原始 trigger 文本 | `logs/after-failure.json`、`shots/04-failed.png`、`shots/05-failed-top.png`、`ocr/05-failed-top.json` |
| 移除 trigger + 重试 | 重新预览→确认；事件 `completed / affected_rows=20`；health `ready`、error_code null、record_count 0；usage 0 / costs 0；UI “采集中 正常 记录 0” | `logs/after-recovery.json`、`shots/06-retry-preview.png`、`shots/07-recovered-bottom.png`、`shots/08-recovered-top.png`、`ocr/08-recovered-top.json` |

OCR 失败文案原文：
`清理失败，历史数据仍保留；请检查存储后重新预览并重试。`
`降级运行 数据更新失败 ... 统计维护失败；现有记录已保留`
恢复后：
`采集中 正常 ... 记录 0`

失败时**没有**显示维护成功；事件仍为 failed，数据保留；恢复是重新预览/确认后的真实 Ready，不是从前端伪造。

### 复核修订 — 通用健康栏文案

Codex 复核指出：`ERR_MAINTENANCE_FAILED` 也被自动 Retention 维护失败复用，健康栏若写“清理失败”会把自动任务误报成用户清除。本次只改健康栏通用文案：

- **顶部错误条**（`maintenance-<generation>` 返回 Failed，明确手动 Clear）：仍为“清理失败，历史数据仍保留；请检查存储后重新预览并重试。”
- **健康栏**（任何 `ERR_MAINTENANCE_FAILED`）：改为“统计维护失败；现有记录已保留” / “Statistics maintenance failed; existing records are preserved”，不泄漏原始错误。
- 不扩展源码范围；A/B 语义与恢复逻辑不变。

复核修订重跑：`cargo fmt --check` PASS；GPUI **139 passed**；GPUI release build PASS（`5cd6b5a0…`）。用新的唯一 CAP 与全新合成夹具再次执行失败→恢复；截图 `review-fix/shots/01-failed-generic-health.png` 同时显示清除专属顶部提示与通用健康栏，`review-fix/shots/02-recovered-generic-health.png` 显示 `采集中 正常 记录 0`；OCR 见 `review-fix/ocr/`，细节 JSON 见 `review-fix/logs/`。

## 5. 证据

- `fixture.py`、`mock_catalog.py`、`logs/`（fixture-setup、test-core、test-core-serial、test-gpui、build、a-token-expiry、before/after-failure、after-recovery、cleanup、source.diff）
- `shots/`（失败、失败顶部、重试预览、恢复底部/顶部）
- `review-fix/`（复核修订：新 build 的通用健康栏/清除专属顶部提示、恢复截图、OCR、日志）
- `ax/`（storage/preview/failure/recovery 的 AX dump）
- `ocr/`（失败/恢复文案）

## 6. 未测 / 限制

- 仅 macOS 中文、非 Retina、1440×900；Windows/英文/Retina/真实 IME NOT RUN。
- 仅合成 trigger abort；未测真实磁盘满、权限、数据库损坏、重启后恢复（本轮恢复走 UI 重试，不重启）。
- 原始 SQLite 错误仍保留在 core stderr（内部日志），未进入 UI/health；这是有意保留诊断信息。
- 并行 core 测试的 1 个 import 抖动已记录；串行全量通过。
- 未提交/推送；未修改旧 T08/T09/T10 证据或 `IMPLEMENTATION_STATUS.md`。

## 7. 清理

- 停止 GPUI、core、mock catalog；`ps`/`lsof` 无本任务进程/CAP 引用。
- 首次修复 CAP：清理前 **1,748 KiB / 25 文件**；cleanup 后 **692 KiB / 9 文件**，随后删除，合计 **1,748 KiB / 25 文件**。
- 复核修订 CAP：清理前 **1,248 KiB / 22 文件**；cleanup 后 **192 KiB / 6 文件**，随后删除，合计 **1,248 KiB / 22 文件**。
- 父 `.../tmp` 保留且为空；未按前缀/glob 清理；未触碰其它缓存、`target/`、`gpui/target/`、旧证据。系统盘可用 **193 GiB**。

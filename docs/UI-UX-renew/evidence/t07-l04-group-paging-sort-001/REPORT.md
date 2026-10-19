# T07-L04 供应商/模型聚合统计分页与排序交互验收

**任务 ID：** UI-T07-L04-GROUP-PAGING-SORT-001
**结论：** PASS（GUI 固定序列 23/23 步；后端 API 与隔离数据库对照全部一致；未复现 L04 范围内的产品缺陷，产品源码零修改）
**平台：** 真实 macOS 桌面会话（macOS 27.0.1，aarch64）。未做 Windows 验收，也不声称 Windows 结果。

## 1. 范围

- 验收对象：`统计 → 供应商统计 / 模型统计` 的分页与排序交互，以及它们与请求日志分页的游标隔离。
- 仅 L04；未开始 L05/T08，未提交、未推送、未执行 `git reset`。
- 使用的都是隔离配置与合成数据；未读取或写入真实用户配置、数据库、密钥，未发任何上游请求。

## 2. 基线与保护内容

| 项 | 值 |
|---|---|
| worktree | `/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign` |
| 分支 | `codex/ui-redesign` |
| 起始 / 结束 HEAD | `192f038ea5a993c4d8e922ede2bc97c4bc060022`（未变） |
| 主 checkout 的 `gpui/src/usage_table.rs` 修改 | 未访问、未修改 |
| `docs/UI-UX-renew/IMPLEMENTATION_STATUS.md` 修改 | 未触碰 |
| 未跟踪设计资料/PDF/PNG | 未触碰 |
| 本任务新增 | 仅 `docs/UI-UX-renew/evidence/t07-l04-group-paging-sort-001/` |
| 产品源码 diff | 无（`git diff --stat` 在非 docs 路径为空） |

## 3. 环境与构建

- 构建工具链：`RUSTUP_TOOLCHAIN=stable` → `rustc 1.99.1 (58cf8f9c3 2026-10-13)`。默认 PATH 的 1.88.0 在 macOS 27 上会生成无法 `dlopen` 的 proc-macro dylib（见 §9）。
- `TMPDIR/TMP/TEMP` 全部指向本任务专用目录：`/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t07-l04-group-paging-sort-001`；**这是因 `/Volumes/1TB` 未挂载而采用的机器本地映射**（AGENTS 外置盘默认原则的替代）。
- 复用 worktree 既有 `target/` 与 `gpui/target/`，未执行 `cargo clean`，未另建 Cargo target。
- 构建命令与退出码（`logs/build-stable.log`）：
  - `cargo build --release --bin workbuddy-proxy` → `CORE_EXIT=0`（增量，复用既有产物）
  - `cargo build --release --manifest-path gpui/Cargo.toml` → `GPUI_EXIT=0`

被验收的二进制（`logs/binary-versions.txt`）：

| 二进制 | 版本 | sha256 | 大小 |
|---|---|---|---|
| `target/release/workbuddy-proxy` | 0.5.6 | `22cacc663af1293252611001e589b6bc49cde885234e42a5e6a200b53697d4ca` | 8,379,136 B |
| `gpui/target/release/workbuddy-proxy-gpui` | 0.5.6 | `efcc0a1f44491581d8867b34b1be2fac6f2d01e78df808f8571b7054fb5ed49c` | 14,633,424 B |

## 4. 隔离 fixture 与合成数据

`fixture.py setup` 在任务 TMPDIR 下新建一次性 config 目录并启动当前 HEAD 的 core（`--headless --config-dir ... --port <随机端口>`），向该 core 自建 SQLite 插入 60 行合成 `usage_requests`（`source=relay`、`record_origin=proxy`、全部 `complete/completed`），时间为本机当天。

矩阵设计（`logs/fixture-setup.log`）：

- 25 个不同供应商组（`prov-01`…`prov-25`，label `Provider 01`…`Provider 25`），出现次数 2/3/5，用于区分 token 排序与请求数排序。
- 27 个不同模型组（`model-01`…`model-27`），出现次数 2/3。
- 期望的排序领导者：供应商 token → `prov-07`（3,000,000）；供应商请求数 → `prov-25`（5 次）；模型 token → `model-20`（4,000,000）；模型请求数 → `model-01`（3 次）。四项 `checks` 全部 `true`。
- 分页规模：20/页 → 供应商 2 页（20+5）、模型 2 页（20+7）；50/页与 100/页 → 各 1 页。

## 5. 后端 API / 隔离数据库对照

`core-api-samples.json`（API 逐页结果 + 独立 SQL 期望）与 `db-api-crosscheck.json`（SQLite 全量聚合 vs 两页游标拼接）：

| 检查 | 结果 |
|---|---|
| 行数 / 供应商组 / 模型组 | 60 / 25 / 27 ✔ |
| API 首页 `total_groups` | 供应商 25、模型 27 ✔ |
| 20/页 首页 → 次页 → 边界 | 供应商 20+5、模型 20+7；末页 `next_cursor=null` ✔ |
| 50/页、100/页 | 供应商 25、模型 27，`next_cursor=null` ✔ |
| sort=tokens 顺序（全量） | SQL 顺序 == API 两页拼接顺序（供应商/模型均 `true`）✔ |
| sort=requests 顺序（全量） | 同上 ✔ |
| 首组汇总值 | 供应商 `prov-07`：2 请求、1,500,000 输入 / 1,500,000 输出；模型 `model-20`：2 请求、4,000,000 总量 —— 与 SQL 完全一致 ✔ |
| 跨分组游标 | provider 游标用于 model、model 游标用于 provider 均被拒绝：`stats_cursor_expired: cursor was issued for a different grouping` ✔ |
| 跨排序游标 | sort=tokens 游标用于 sort=requests 同样被拒绝 ✔ |

## 6. 真实 GUI 验收（不是模拟）

驱动方式：`axdrive`（Accessibility API）对可见控件执行 **AXPress**（与鼠标点击走同一 handler），每次操作后轮询窗口截图 + Vision OCR，直到目标状态连续两次一致才记录，随后保存窗口 PNG、OCR JSON、AX 树。固定序列**每项操作只执行一次**，无重启、无重复整套场景。

结果：`gui-steps.json` → **23/23 PASS**（`shots/` 23 张窗口截图、`ocr/` 23 份 OCR、`logs/ax-*.txt` 23 份 AX 树）。

| # | 操作 | 期望（来自 API） | 观测 | 判定 |
|---|---|---|---|---|
| 01 | 用量明细 | 第1/3、20 条/页 | 1/3、20 条/页 | PASS |
| 02 | 供应商统计 | 供应商列头、Provider 07、25 组、20/页 | 一致 | PASS |
| 03 | 下一页 | 第2页首行 Provider 18（5 行） | 18,17,09,08,04 | PASS |
| 04 | 下一页（末页边界） | 保持第2页 | 状态不变；截图中 Next 灰显 | PASS |
| 05 | 上一页 | 回第1页 Provider 07 | 一致 | PASS |
| 06 | 请求数 | Provider 25 第一（25 组） | 一致 | PASS |
| 07 | 下一页 | 请求数第2页 Provider 20…24 | 20,21,22,23,24 | PASS |
| 08 | 总 Token | 重置第1页 Provider 07 | 一致（分页重置） | PASS |
| 09 | 20 条/页 | 50 条/页、25 组一页 | 50 条/页、Prev/Next 均灰显 | PASS |
| 10 | 50 条/页 | 100 条/页、首行 Provider 07 | 一致 | PASS |
| 11 | 100 条/页 | 回 20 条/页、Provider 07 | 一致 | PASS |
| 12 | 下一页 | 供应商第2页 Provider 18 | 一致 | PASS |
| 13 | 模型统计 | 模型列头、model-20、27 组、**第1页** | 一致（未继承供应商第2页游标） | PASS |
| 14 | 下一页 | 模型第2页 model-03…07 | 完全一致（7 行） | PASS |
| 15 | 上一页 | 模型第1页 model-20 | 一致 | PASS |
| 16 | 请求数 | model-01 第一（27 组） | 一致，按钮高亮切换 | PASS |
| 17 | 下一页 | 模型请求数第2页 21…27 | 完全一致 | PASS |
| 18 | 总 Token | 重置第1页 model-20 | 一致 | PASS |
| 19 | 下一页 | 模型第2页 model-03 | 一致 | PASS |
| 20 | 供应商统计 | 供应商第1页 Provider 07 | 一致（切分组即重置） | PASS |
| 21 | 用量明细 | 请求日志 1/3、20 条/页 | 一致 | PASS |
| 22 | 下一页 | 请求日志 2/3 | 一致 | PASS |
| 23 | 供应商统计 | 供应商第1页 Provider 07 | 一致（rows 第2页未移动分组游标） | PASS |

关键交互证据：

- **身份/列语义不混淆**：供应商视图首列表头为「供应商」、行标签 `Provider XX`；模型视图首列表头为「模型」、行标签 `model-XX`；两种视图的其余列（请求 / 输入 / 输出 / 缓存命中 / 估算费用 / 用量覆盖）一致。见 `shots/02-providers-p1-tokens.png`、`shots/13-models-p1-tokens.png`。
- **排序确实触发新结果**：`请求数` / `总 Token` 按钮的激活样式随点击切换（`shots/13-models-p1-tokens.png` 中「总 Token」高亮，`shots/16-models-sort-requests-p1.png` 中「请求数」高亮），首行按设计从 `Provider 07`↔`Provider 25`、`model-20`↔`model-01` 变化，且从第2页点击排序后回到第1页（步骤 08、18）。
- **页边界**：末页 Next 在截图中灰显（`shots/04-providers-p2-next-boundary.png`、`shots/09-providers-size-50.png`），AXPress 后表格内容与分页状态完全不变。
- **游标栈隔离**：供应商停在第2页时切模型 → 模型第1页（步骤 12→13）；模型停在第2页时切供应商 → 供应商第1页（步骤 19→20）；请求日志第2页不移动分组游标（步骤 21→23）。
- **每页数量**：供应商 20→50→100→20 的切换均重置到第1页；50/页与 100/页时 25 组只占一页且 Prev/Next 均灰显。

### OCR 判读说明（不改变点击事实）

- `10-providers-size-100` 的 OCR 把「分组」识别为繁体「分組」，首轮解析漏掉「25 个分组」而误判未稳定。`reanalyze.py` 仅对**已保存的原图 OCR JSON** 用字形容忍正则重判（未重新点击、未重新截图），该步修正为 PASS，最终 23/23。
- `16-models-sort-requests-p1` 的 OCR 把第 9 行 `model-09` 读成 `model-00`（`gui-vs-api.json` 中唯一 `visible_is_prefix=false`）；同目录截图 `shots/16-models-sort-requests-p1.png` 目视可见 `model-09`，其余 11 个可见行与后端顺序完全一致，属 OCR 字形误读而非产品问题。

## 7. 缺陷判定

**未复现 L04 范围内的产品缺陷，未修改任何产品源码**（`git diff` 对非 docs 路径为空）。后端游标校验、分页重置、排序切换、分组隔离、列语义均符合预期。

## 8. 遗留限制

- 仅 macOS 真机；未验证 Windows。请求日志与分组共用同一个 `page_size` 字段（切换 tab 不重置每页数量），本轮序列在 20/页下结束，未观察到混淆，但该共享行为本身未作为缺陷判定；`请求日志↔分组` 的每页数量是否应各自独立，属设计决策，超出 L04。
- 未覆盖快速切换 tab 的并发竞态（`rows_gate/groups_gate` 的失效路径仅静态阅读，未做压力验证）。
- 未验证点击“已激活”排序按钮的幂等行为（本轮只点击了会改变 `group_sort` 的按钮）。
- 分组分页区域只显示「N 个分组」而无数页码文本；本轮以 Prev/Next 可用性 + 行内容/数量判定页数，未将该表现判为缺陷（无 L04 依据）。
- GPUI 的 accesskit 对 pager 按钮不暴露 disabled（AX 中始终 `enabled`，与 T07-L03 记录一致），因此边界结论采用“截图灰显 + 点击后状态不变”双重证据。

## 9. 既有构建缓存损坏与已删除文件（用户已在过程中纠正环境）

本轮开始用默认 `rustc 1.88.0` 构建时失败：多个 proc-macro dylib `dlopen` 报 `mis-aligned LINKEDIT string pool`。为让构建继续，删除了 **10 个已确认无法加载的 dylib**（删除前后均用 `ctypes.CDLL` 验证；删除 `libserde_derive-f07b4a39624f82dc.dylib` 后由 cargo 重建的同 hash 文件仍然损坏，因此判断为 1.88 工具链在 macOS 27 上的确定性产物问题，而非随机损坏）：

```
target/release/deps/libserde_derive-bef2d240a6faf685.dylib
target/release/deps/libserde_derive-f07b4a39624f82dc.dylib
target/release/deps/libtokio_macros-45f6aba56e397ef3.dylib
target/release/deps/libtokio_macros-51f9f532549ec167.dylib
target/release/deps/libyoke_derive-09aaa8bbb6ba3336.dylib
target/release/deps/libyoke_derive-26adbf5672a6c250.dylib
target/release/deps/libzerofrom_derive-6c77746e7ca662fc.dylib
target/release/deps/libzerofrom_derive-e75587f93851f2b2.dylib
target/release/deps/libzerovec_derive-528b706512528342.dylib
target/release/deps/libzerovec_derive-d30ec2e8d80433f7.dylib
```

依据与过程保存在 `logs/build.log`（首次失败）、`logs/build-core.log`（删除后仍失败）、`logs/build-corrupt-dylibs.log`（删除清单与 `ls` 记录）。收到环境纠正后改用 `RUSTUP_TOOLCHAIN=stable`（1.99.1），`logs/build-stable.log` 两次构建均 `EXIT=0`，**此后未再删除任何既有构建产物**，`target/` 与 `gpui/target/` 保持复用。

## 10. 清理

停止并等待本轮两个进程退出后清理（`fixture.py cleanup` + 明确路径删除）：

| 项 | 详情 |
|---|---|
| 停止的进程 | core PID 56346、GPUI PID 56707（SIGTERM/控制接口），确认已退出 |
| 删除的运行目录 | `<TMPDIR>/run`（隔离 config、`usage.sqlite3`、home、tmp、core/gpui 运行日志），6.3 MiB |
| 删除的工具/探针 | `<TMPDIR>/bin`（axdrive、ocr，164 KiB）、`<TMPDIR>/proc-macro-probe`（668 KiB）、`<TMPDIR>/scratch.png`（205 KiB）、`<TMPDIR>/gpui.pid` |
| 复制的可执行文件 | 无（直接运行 worktree 内二进制） |
| 释放 | 约 7.3 MiB（任务临时目录由 7.3 MiB → 0 B，空目录已移除） |
| 保留 | 证据目录（截图 5.7 MiB 全部内容）+ 构建缓存 `target/`、`gpui/target/` |
| 系统盘剩余 | 200 GiB 可用（`/dev/disk3s3s1`，460 GiB 容量） |
| 其他临时目录核查 | `/tmp` 无本轮遗留；会话缓存父目录 `.../tmp` 为 0 B |

## 11. 证据清单（本目录）

| 文件 | 内容 |
|---|---|
| `REPORT.md` | 本报告 |
| `fixture.py` | 隔离配置/合成数据/API 抽样复现脚本（setup/status/stop/cleanup） |
| `gui_drive.py` | 固定 GUI 操作序列驱动（23 步，一次执行） |
| `reanalyze.py` | 仅基于已存 OCR JSON 的重判 + GUI/API 行对照 |
| `tools/axdrive.swift`、`tools/ocr.swift` | AXPress/截图/AX dump 工具与 Vision OCR 工具源码 |
| `core-api-samples.json` | API 分页/排序样本 + 独立 SQL 期望 + 跨游标拒绝 |
| `db-api-crosscheck.json` | SQLite 全量聚合与 API 两页拼接/汇总值对照 |
| `gui-steps.json` | 23 步逐步状态、press 输出、判定与重判记录 |
| `gui-vs-api.json` | 每步可见 OCR 行 vs 后端分页 items |
| `shots/*.png`（25） | 真实窗口截图（含 23 步、试点、初始） |
| `ocr/*.json` | 对应截图的 Vision OCR |
| `logs/ax-*.txt` | 对应步骤的 AX 树；`logs/ax-preflight.txt` |
| `logs/build*.log`、`compile-tools.log`、`fixture-setup.log`、`gui-drive.log`、`reanalyze.log`、`db-crosscheck.log`、`binary-versions.txt`、`run.json`、`core.log`、`gpui.log` | 构建、工具编译、fixture、GUI 驱动、重判、DB 对照与运行日志 |

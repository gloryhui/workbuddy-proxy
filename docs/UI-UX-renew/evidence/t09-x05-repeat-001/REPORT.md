# T09 X05 重复交互基线（corrected）

**任务：** T09-X05-REPEAT-BASELINE-001
**分支 / HEAD：** `codex/ui-redesign` / `344b896d062f1f92f8f74be4c06a29a3f9b18eaa`
**结论：** **X05 PASS（corrected，macOS 中文、非 Retina、1440×900、固定合成数据、当前构建、10 轮）。** 10 轮中每轮均用 AX 确认 `请求状态`：`全部状态 → 失败（KPI 0）→ 全部状态（KPI 20）`，**10/10 通过**；模型选择器每轮恰好 1 次 loopback `/v1/models`（共 +10），FD 42/25、窗口 1、DB 20/20、config 哈希均不变。**没有性能阈值或“无泄漏”结论**，不外推 Windows/英文/Retina/长时 soak。

## 0. 权威数据与旧预备测量

| 数据 | 用途 |
|---|---|
| `corrected/logs/measurements-corrected.csv`、`corrected/logs/measurements-corrected.json`、`corrected/logs/summary-corrected.json`、`corrected/ax/`、`corrected/shots/`、`corrected/ocr/` | **权威 10 轮**；筛选每轮 AX + KPI 验证 |
| `logs/measurements.csv`、`logs/measurements.json`、`logs/summary.json`、`repeat.py` | **旧预备测量，superseded / inaccurate filter clicks**；其 initial/final 均为“已完成”，筛选步骤不得引用为通过 |

旧脚本菜单坐标漏加窗口内容原点 `y=30`：屏幕菜单项实际在 y≈250/272/295，脚本用 265 命中“已完成”、用 220 落在菜单上方，未恢复“全部状态”；更早中止/重跑已把状态留在“已完成”。旧耗时/RSS 数据保留作历史，但**不用于 X05 筛选结论**。

## 1. 基线与夹具

| 项 | 值 |
|---|---|
| HEAD | `344b896…`；`git diff HEAD -- gpui/src src` 为空；未改源码 |
| 构建 | GPUI `5cd6b5a04246be9542b62f4a754799f5c5230877a3729c375657f61f745fa2c2`；core `9823273191cb06c3f39c46e6f6ec7f4d72fcdc30959c84bd692186adb5f8daa9` |
| 夹具 | **25** 供应商 / **25** 模型 / **25** 条价格规则 / **20** 行 completed 请求 / **20** 行费用 |
| 本地 mock | `127.0.0.1` `/v1/models` → `mock-model-a/b`；虚构 Key `sk-test-local`；无外部上游 |
| 隔离 | 新 CAP `.../tmp/t09-x05-repeat-corrected-001`；config/DB/Home 全在 `CAP/run`；TMPDIR/TMP/TEMP=CAP；realpath 已核对 |
| 环境 | macOS 27.0.1 arm64，1440×900，中文，非 Retina |

## 2. Corrected 方法

10 轮，每轮固定顺序；关键筛选步骤逐步读 AX，不用固定坐标替代验值：

1. 激活 GUI，确保 frontmost；从统计页切到供应商 opencode。
2. 打开协议选项 → Escape 关闭。
3. 打开模型选择器（loopback mock）→ Escape 关闭；不加选、不保存。
4. 切换 Provider B → 回到 opencode。
5. 切到统计页；**AX 确认当前为“全部状态”**；打开菜单，屏幕 `y=295` 选“失败”，AX 必须为“失败”，KPI 必须 0；再打开菜单，`y=250` 选“全部状态”，AX 必须为“全部状态”，KPI 必须 20。任一不符立即停止，并在 `corrected/logs/` 写入 `corrected-failure.json`（本轮未触发，文件不存在）。
6. 请求日志向下/向上滚动。
7. 每轮采集 GUI/core RSS、累计 CPU、FD、线程、窗口、mock 请求数、DB 行数、config 哈希；每轮记录阶段耗时。

脚本：`repeat-corrected.py`；筛选 KPI 通过小区域截图 + OCR 读取（KPI 不在 AX 树），状态用 AX `AXPopUpButton` value 核验。

## 3. Corrected 结果

### 总量

| 指标 | 初始 | 最终 | 变化/范围 |
|---|---:|---:|---:|
| GUI RSS | 128,064 KiB | 129,520 KiB | **+1,456 KiB**；最大 129,520 |
| core RSS | 15,616 KiB | 15,760 KiB | **+144 KiB**；最大 15,760 |
| GUI 累计 CPU | 19.05 s | 29.83 s | +10.78 s |
| core 累计 CPU | 0.60 s | 0.84 s | +0.24 s |
| GUI FD / core FD | 42 / 25 | 42 / 25 | 不变 |
| GUI 线程 / core 线程 | 10 / 6 | 10 / 7 | core +1 采样值 |
| 窗口数 | 1 | 1 | 不变 |
| mock `/v1/models` | 2 | 12 | **+10（10 轮 × 1）** |
| usage_requests / costs | 20 / 20 | 20 / 20 | 不变 |
| config SHA-256 | `b09c3fe9…` | `b09c3fe9…` | 不变 |
| `请求状态` AX | 全部状态 | 全部状态 | 10/10 每轮往返核验 |
| 筛选验证 | — | — | **10/10 轮：失败+KPI0 → 全部状态+KPI20** |

### 每轮耗时

| 阶段 | 最小 | 平均 | 最大 |
|---|---:|---:|---:|
| 整轮 | 9.144 s | **9.289 s** | 9.804 s |
| 切供应商 A | 0.475 s | 0.485 s | 0.496 s |
| 协议弹窗打开/关闭 | 0.834 s | 0.850 s | 0.867 s |
| 模型选择器打开/关闭 | 1.447 s | 1.484 s | 1.543 s |
| Provider B→A | 0.746 s | 0.777 s | 0.803 s |
| 统计页+AX 筛选往返 | 4.899 s | 4.989 s | 5.339 s |
| 日志滚动 | 0.507 s | 0.547 s | 0.639 s |

10 轮合计 **92.889 s**。整轮与各阶段未见逐轮单调上升；FD/窗口/DB/config 稳定；无崩溃、假死或重复模型请求。筛选阶段比旧预备测量长约 2.5 s，主要来自每轮 AX/截图/OCR 验证，不是同一操作基线，不能与旧值直接比较。

### 观察边界

- GUI RSS 增加 1,456 KiB、core 增加 144 KiB；这是 **10 轮单次会话的原始采样**，不能据此断言泄漏或无泄漏。
- 无性能阈值、百分比或“性能提升/退化”结论；没有 100/1000 轮 soak。
- 旧 X05（3 次 picker/3 次协议/2 次同步，RSS 136 MB，无 before/after）不是同基线，不混算。

## 4. 证据

- `corrected/shots/00-initial.png`、`cycle-01/05/10-main.png`、`cycle-XX-filter-failed.png`、`cycle-XX-filter-all.png`、`99-final.png`
- `corrected/ax/`（`00-initial.txt`、`cycle-XX-filter-pre/failed/all.txt`、`cycle-XX-picker.txt`、`99-final.txt`）
- `corrected/ocr/`（`cycle-XX-filter-failed.json`、`cycle-XX-filter-all.json`，KPI crop OCR）
- `corrected/logs/measurements-corrected.csv`、`corrected/logs/measurements-corrected.json`、`corrected/logs/summary-corrected.json`、`corrected/logs/corrected-run.log`、`corrected/logs/run.json`
- 旧预备测量与 `filter-check/`（坐标修正/最小验证）保留在同一 evidence 目录，已明确标注用途。

## 5. 未测 / 限制

- 仅 macOS 中文、非 Retina、1440×900；Windows/英文/Retina/DPI 未测。
- 单次 10 轮；未做长时 soak/内存曲线/CPU 火焰图。
- CPU 为进程累计值；RSS 为 `ps` 采样；无阈值。
- 未覆盖所有筛选组合、日期面板、导入、价格弹窗、同步确认；未执行保存/同步/删除/测试连接/关闭代理。
- 模型选择器使用本轮 loopback mock；未发外部上游，未添加/保存模型。

## 6. 清理

- 停止 GUI/core/catalog/mock；`fixture.py cleanup` 删除 `CAP/run`；先读 `run.json` 的 mock pid 并 SIGTERM，确认无 stray mock。
- Corrected CAP：清理前 **1,196 KiB / 24 文件**；cleanup 后 **196 KiB / 7 文件**，随后删除；合计 **1,196 KiB / 24 文件**。
- 旧 10 轮 CAP：**1,196 KiB / 24 文件**；filter-check CAP：**1,184 KiB / 22 文件**（均已在前序复核中清理并记录）。
- 父 `.../tmp` 为空；无任务进程；证据 **12 MB**；系统盘 **193 GiB** 可用；无其它意外删除/写入。

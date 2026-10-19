# T07-L02 残余复核（列宽下限、横向两端、表头同步、拖动恢复）

**任务 ID：** UI-T07-L02-RESIDUAL-002
**结论：** **PASS（按 Token 列 240 px 下限口径）** —— 真实 macOS GUI 中，1100×700 / 1280×800 / 1440×900 三种窗口下：Token 列可被拖动到 240 px 且不能更小；水平滚动左右两端均可达全部 9 列；表头与表体横向同步（OCR 同步偏差 0–2 px）；拖回后自定义 340 px 宽度恢复，拉宽窗口时自定义宽度保留。产品源码零修改、未提交、未推送。
**口径警示（需 Codex 裁定）：** 本轮把“列宽不得小于 240 px”按源码 `usage_table.rs::DRAG_FLOORS` 的实现语义判定为**请求日志 Token 列的最小可拖动宽度 240 px**。同一常量表中其它列的拖动下限为 96 / 88 / 80 / 96 / 56 / 48 / 92 px（均 < 240 px），这是设计既定值。若验收要求“所有列一律 ≥ 240 px”，则本项应为 FAIL；证据与逐列下限见 §5。
**平台：** 真实 macOS 桌面会话（macOS 27.0.1，Apple M1，屏幕 1920×1080，非 Retina，scale=1.0）。仅本机中文界面；未测 Windows、英文与其它 DPI。

## 1. 范围

只做 T07-L02：把请求日志列拖到允许下限、水平滚动到左右两端、核对表头/表体同步、拖动后恢复与拉宽窗口检查。未开始 T08；未修改产品源码、`IMPLEMENTATION_STATUS.md`、未跟踪设计资料或主 checkout；全部使用一次性隔离配置与合成数据，未读取真实配置/历史库/Key，未发起上游请求。

## 2. 基线与保护内容

| 项 | 值 |
|---|---|
| worktree | `/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign` |
| 分支 / HEAD | `codex/ui-redesign` / `0664001459e4dea0325c67cd03725e8c65b427bd`（与任务给定一致，实测见 `logs/baseline.txt`） |
| 产品源码 diff | 空（`git diff --stat HEAD -- gpui/src src` 无输出） |
| 保护内容 | `docs/UI-UX-renew/IMPLEMENTATION_STATUS.md` 既有修改、未跟踪设计 PDF/PNG、主 checkout 的 `gpui/src/usage_table.rs` 均未触碰 |
| 本任务新增 | 仅 `docs/UI-UX-renew/evidence/t07-l02-residual-002/`（未提交） |

## 3. 环境、构建与隔离夹具

- 复用 worktree 既有 `target/` 与 `gpui/target/`，未执行 `cargo clean`、未新建 Cargo target；本轮**未重新编译产品源码**（源码无改动），直接复用已确认的 release 产物：

| 二进制 | 版本 | sha256 | 大小 |
|---|---|---|---|
| `gpui/target/release/workbuddy-proxy-gpui` | 0.5.6 | `efcc0a1f44491581d8867b34b1be2fac6f2d01e78df808f8571b7054fb5ed49c` | 14,633,424 B |
| `target/release/workbuddy-proxy` | 0.5.6 | `22cacc663af1293252611001e589b6bc49cde885234e42a5e6a200b53697d4ca` | 8,379,136 B |

- `TMPDIR` / `TMP` / `TEMP` 全部指向本任务专属目录 `/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t07-l02-residual-002`（`/Volumes/1TB` 未挂载，沿用本机缓存映射）。
- 夹具：`fixture.py setup` 在一次性 config 目录启动当前 core（`--headless --config-dir ... --port <随机>`），向该库插入 **45 行**合成 `usage_requests`（43 代理 + 1 内置 + 1 自定义），每页 20 行共 3 页；本机当天时间。默认“全部来源”返回 43 行。记录见 `logs/fixture-setup.log`、`expected.json`。
- 输入工具：`axdrive`（AXPress/dump/shot/resize，沿用已授权的 L05 二进制副本，源码同 `t07-l01-three-sizes-001/tools/axdrive.swift`）、`ocr`（Vision）、`hscroll`（水平滚轮）、`drag`（CGEvent 拖动）、`sscroll`、`raise`。源码与编译命令见 `tools/` 与 `logs/commands.md`。
- 关键操作前提：本机前台会被其它应用（实测为 ChatGPT）抢占；仅 `NSRunningApplication.activate` 不足，需用 `osascript` 经 System Events 把 `workbuddy-proxy-gpui` 置前后，鼠标拖动/滚轮才生效（失败尝试见 §7）。

## 4. 操作步骤（可复现）

1. `fixture.py setup` 启动隔离 core 并写入 45 行；随后启动 GPUI（`--config-dir <隔离配置>`）。
2. `axdrive resize <pid> W H` 设置窗口 1100×700 / 1280×800 / 1440×900；`axdrive windows` 记录 CGWindow bounds，`axdrive dump` 记录 AX 窗口与行几何。
3. 页面滚动到“用量明细”请求日志（滚轮点固定在表格左侧 `x=280`，避免滚到表格内部），并 `dump` 找到分页按钮确认表格在视口内。
4. 水平滚动（`hscroll <x> <y> ±1200/1500`）到左端：应见到时间/供应商/模型/请求标识/Token 左半；截图 `*-scroll-left.png`。
5. 滚到右端：应见到 Token/估算费用/耗时/tok\/s/状态；截图 `*-scroll-right.png`。
6. 列宽拖动：在 Token 列右边界（表头行高度内，`y` 取表头中心）按下，向左拖 140–160 px；截图 `*-drag-min-240*.png`。再从当前边界继续向左拖，验证不再变窄。
7. 恢复：从当前边界向右拖 100 px；截图 `*-drag-restored-340.png`。
8. 依次把窗口改为 1280×800、1440×900，重复 4–7；同时观察上一档的自定义列宽是否被保留。
9. 结束：停止 GUI 与 core，清理本任务临时目录。

## 5. 结果

### 5.1 窗口、内容区与表格几何（AX + CGWindow + PNG 实测）

| 窗口请求 | AXWindow frame | CGWindow bounds | 截图像素 | 应用内容（标题栏下） | AXRow 行高 | 表格视口宽 |
|---|---|---|---|---|---|---|
| 1100×700 | `[0,30 1100x700]` | 1100×700 @ (0,30) | 1100×700 | 1100×670 | 56 px | 747 px |
| 1280×800 | `[0,30 1280x800]` | 1280×800 @ (0,30) | 1280×800 | 1280×770 | 56 px | 927 px |
| 1440×900 | `[0,30 1440x900]` | 1440×900 @ (0,30) | 1440×900 | 1440×870 | 56 px | 1087 px |

窗口 frame 与内容视图同尺寸（macOS full-size content，无原生标题栏）；顶部 30 px 为应用自绘标题栏，故“应用内容”单独列出。证据：`logs/ax-*.txt`、`logs/windows-*.json`、`shots/*.png`。

### 5.2 水平滚动两端与表头同步

OCR 的 x 为 PNG 像素；`header/data` 为 Token 列表头与首行 Token 值（`总 Token 2.22亿`）的 x，差值即横向同步偏差。

| 尺寸 | 左端可见 | 右端可见 | Token 表头/数据（左端） | Token 表头/数据（右端） | 同步偏差 |
|---|---|---|---|---|---|
| 1100×700 | 时间 316、供应商、模型、请求标识、Token 左半 | 费用、耗时 824、tok/s 890、状态 950 | 798 / 798 | 356 / 356 | 0 px |
| 1280×800 | 时间 318、供应商、模型、请求、Token、费用 1134 | 费用 872、耗时 1004、tok/s 1070、状态 1130 | 798 / 798 | 536 / 536 | 0 px |
| 1440×900 | 时间 316、请求 693、Token 797、费用 1134 | 费用 1032、状态 1289（完整） | 797 / 797 | 695 / 697 | 0–2 px（OCR 噪声） |

结论：三档尺寸下**所有 9 列均可达**（左端覆盖时间…Token，右端覆盖 Token…状态），时间列与状态列分别为两端锚点；表头与内容在同一滚动容器内同向移动，偏差不超过 OCR 2 px。证据：`shots/*-scroll-left.png`、`shots/*-scroll-right.png`、`ocr/*.json`、`acceptance-summary.json`。

### 5.3 Token 列宽下限与拖动恢复（像素边界 / OCR 表头差）

像素边界取自表头行竖线（颜色 `#EBEEF1`，1100/1280 可稳定检出，检出值即列边界）；1440 下该抹线对比度不足，改用 OCR“Token 表头 ↔ 估算费用表头”的 x 差（与像素边界在 1100/1280 相差 ≤1 px，见括注）。

| 尺寸 | 初始（继承自定义 340） | 拖到下限 | 再拖更小 | 拖回恢复 |
|---|---|---|---|---|
| 1100×700 | 默认自动宽 360（源码 `MIN_WIDTHS[4]`，本轮未在拖动前量测该档） | 450→690 = **240 px** | 仍 450→690 = 240 px | 450→787 = 337 px（约 340） |
| 1280×800 | 528→865 = 337 px | 625→865 = **240 px** | 仍 240 px | 625→962 = 337 px（约 340） |
| 1440×900 | 表头差 337 px（1280 的 340 被保留） | 表头差 **243 px**（≈240） | 仍 243 px | 表头差 341 px（≈340） |

- **下限有效**：三档下 Token 列拖到 240 px 后不再变窄（重复拖动无变化），符合 `DRAG_FLOORS[4] = 240.`。
- **拖动恢复**：向右拖回后恢复为自定义约 340 px；`UsageRows::custom_widths` 只覆盖被用户拖过的列，其它列继续自适应。
- **拉宽窗口保留自定义宽度**：1100→1280→1440 过程中 Token 列保持约 340 px（1280 初始 337、1440 拖动前 337），说明用户调宽在窗口变化后仍保留。
- 证据：`shots/1100-drag-*.png`、`shots/1280-drag-*.png`、`shots/1440-drag-*.png`、`ocr/*drag*.json`、`acceptance-summary.json`、`logs/commands.md`。

### 5.4 逐列拖动下限（源码常量，与本轮实测口径）

`gpui/src/usage_table.rs::DRAG_FLOORS = [110, 96, 88, 80, 240, 96, 56, 48, 92]`，对应：时间（不可拖，`resizable=false`）、供应商、模型、请求标识、**Token 240**、估算费用、耗时、tok\/s、状态。本轮实测只把 **Token 列**拖到其下限 240 px；其它列的下限为源码常量，未在本轮逐个拖到最小值。因此“各窗口尺寸列宽不得小于 240 px”仅对 Token 列成立。

## 6. PASS / FAIL 汇总

| 检查项 | 1100×700 | 1280×800 | 1440×900 |
|---|---|---|---|
| Token 列拖到 240 px 且不能更小 | PASS | PASS | PASS（OCR 243±3） |
| 水平滚动左端可达（时间列） | PASS | PASS | PASS |
| 水平滚动右端可达（状态列） | PASS | PASS | PASS |
| 表头与内容同步（偏差 ≤2 px） | PASS | PASS | PASS |
| 拖动后恢复（约 340 px） | PASS | PASS | PASS |
| 拉宽窗口时自定义列宽保留 | PASS | PASS | PASS |
| 所有列 ≥ 240 px | FAIL（按字面口径；除 Token 外设计下限 < 240） | 同左 | 同左 |

## 7. 失败尝试与已知限制

- **前台抢占导致输入无效（真实现象）**：直接在 GUI 启动后用鼠标 `drag`/`hscroll` 操作时，窗口被 ChatGPT 抢占前台，`activate` 返回 true 但 `isActive=false`，拖动/滚轮全部无效（表现为列宽不变）。通过 `osascript -e 'tell application "System Events" to set frontmost of first process whose unix id is N to true'` 后恢复正常；本报告所有 PASS 结果均在激活验证通过后采集。
- **1440 下的像素边界检测失败**：表头竖线对比度降低，`#EBEEF1` 阈值检出为空；改用 OCR 表头 x 差，误差约 ±3 px（243/341），因此 1440 的下限判定标为“≈240”。
- 表头 OCR 偶发漏读“状态/费用”或在拖动后表格自动水平补偿滚动（如 1280 拖动后 Token 左边界平移约 97 px），因此宽度结论以“表头 x 差 + 像素边界”双重证据为准。
- **未测**：Windows/macOS 英文界面；Retina/非 100% 缩放；键盘可达性（列宽只用鼠标拖动）；列重排（拖动落在表头单元格而非边界时）；持久化（重启后自定义列宽是否保留未做本轮必测项）。
- 截图均为真实 GPUI 窗口 PNG（`screencapture -l`），未使用设计稿或仿真图。

## 8. 证据清单

| 文件 | 内容 |
|---|---|
| `REPORT.md` | 本报告 |
| `acceptance-summary.json` | 逐屏 Token 表头/数据 x、表头宽差、同步偏差与源码下限常量 |
| `summarize.py` | 从 OCR JSON 生成上述汇总 |
| `fixture.py` / `expected.json` / `logs/fixture-setup.log` | 隔离合成夹具与启动记录 |
| `shots/1100-*.png`、`1280-*.png`、`1440-*.png` | 三档端点、下限、恢复截图（15 张） |
| `ocr/*.json` | 每张截图的 Vision 文本块 |
| `logs/ax-*.txt` / `logs/windows-*.json` | 三档 AX 窗口、行几何与 CGWindow bounds |
| `logs/commands.md` | 关键命令与工具编译命令 |
| `tools/` | `drag` / `hscroll` / `sscroll` / `mouse2` / `raise` 源码 |

## 9. 清理

见 `logs/cleanup.txt`（结束时写入）：停止 GUI/core、等待子进程退出、删除本任务专属临时目录 `…/tmp/t07-l02-residual-002`、核查系统 `/tmp` 与进程引用；共享 `target/`、`gpui/target/`、L05 保留目录未清理。

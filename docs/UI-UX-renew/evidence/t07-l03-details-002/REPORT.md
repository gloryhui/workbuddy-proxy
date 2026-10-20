# T07-L03 详情绑定真实 GUI 验收（macOS）

**任务：** T07-L03-DETAILS-002
**分支 / HEAD：** `codex/ui-redesign` / `88affbc1f055b6a59330d75890c7ed3785f14c35`（T10 文档提交；产品源码与 `30cfd66` 无差异）
**结论：** **PASS（仅 macOS 中文、非 Retina、当前隔离夹具范围）**。真实 GUI 中首页两行、第 2 页、第 3 页与返回首页换行的 5 次详情打开，请求 ID、供应商/路由、模型、时间、状态、Token、费用均与所选行/DB/API 一一对应；每次关闭后无旧详情残留。页大小 20/50/100 与首页/末页边界复验通过。**不覆盖 Windows、英文、Retina/DPI。**

> **必须记录的运行事故：** 首次运行夹具时，`fixture.py` 的 `CAP` 常量未同步为任务目录，`RUN=CAP` 指向会话缓存根 `.../tmp`，`setup()` 的 `rmtree(RUN)` 删除了该根下的 **L05 本机缓存运行副本** `t07-l05-full-values-001`（约 45 MB，含旧二进制/DB/日志）。`docs/UI-UX-renew/evidence/t07-l05-full-values-001/` 的提交证据目录完整保留；但该旧构建运行副本无法从本机恢复。详见 `logs/incident-l05-cache-loss.log`。此后已把 `CAP` 精确改为 `.../tmp/t07-l03-details-002`，清理仅对本任务 CAP 执行；本报告不据此修复或改写 L05。

## 1. 环境与基线

| 项 | 值 |
|---|---|
| 主机 | macOS 27.0.1（26A434），Apple M1，arm64，1920×1080 非 Retina（scale=1.0） |
| 二进制 | GPUI `gpui/target/release/workbuddy-proxy-gpui` sha256 `89049d01…`（T08 构建，源码=当前 HEAD）；core `target/release/workbuddy-proxy` sha256 `22cacc66…` |
| 隔离目录 | `.../tmp/t07-l03-details-002`（CAP；TMPDIR/TMP/TEMP 全部指向；已清理） |
| 夹具 | 60 行合成请求（今日 07:50–08:19，`t07l03-0001`…`0060`，模型 `synth-model-a/b`，HTTP 200/已完成/用量完整，费用 `default_zero/CNY`） |
| 工具 | `axdrive`（AX dump/press/click/shot/resize/scroll）、`ocr`（Vision），源码复制自 T07-L02 tools 并编译到 CAP/bin（随 CAP 删除） |
| 数据来源 | 只读副本 core API + 隔离 SQLite；未读取真实配置/密钥/真实历史库；无外网 |
| 产品源码 | 未修改（`git diff HEAD -- gpui/src src` 为空） |

## 2. 方法

1. `fixture.py setup` 启动隔离 core（随机端口）并插入 60 行；`expected.py` 导出 60 行 DB 值、20/50 页 ID 列表、20 行游标三页（20/20/20）与 5 个 `stats_request_detail` API 样本。
2. 启动当前 GPUI（同一隔离 config），System Events 置前，1440×900。
3. 在首页点击第 1/2 行，翻到第 2 页点击第 1 行，翻到第 3 页点击第 1 行，返回首页点击第 2 行；每步记录行 frame、截图、OCR、AX 详情树，解析 `AXGroup` 的“请求标识/时间/供应商 · 路由/请求模型/上游模型/状态/Token/估算费用”等字段。
4. `Escape` 关闭后 dump 检查“请求标识”消失，确认无旧详情残留。
5. 页大小按钮循环 20→50→100，OCR 页文本；末页/首页边界点击“下一页/上一页”确认无效。
6. `analyze.py` 逐项比较：request ID、供应商/路由、模型、时间（DB→本地 `MM-DD HH:MM:SS`）、状态、输入/输出/总 Token、费用、DB 与 API 一致性、关闭清洁。

## 3. 结果

| 步骤 | 页面 | 详情请求 ID | 期望模型 | 时间 | Token | 结论 |
|---|---|---|---|---|---|---|
| 01-page1-row1 | 1/3 | `t07l03-0060` | synth-model-b | 10-20 08:20:13 | ↓1060 ↑100 Σ1160 | PASS |
| 02-page1-row2 | 1/3 | `t07l03-0059` | synth-model-a | 10-20 08:19:43 | ↓1059 ↑100 Σ1159 | PASS |
| 03-page2-row1 | 2/3 | `t07l03-0040` | synth-model-b | 10-20 08:10:13 | ↓1040 ↑100 Σ1140 | PASS |
| 04-page3-row1 | 3/3 | `t07l03-0020` | synth-model-b | 10-20 08:00:13 | ↓1020 ↑100 Σ1120 | PASS |
| 05-page1-row2-again | 1/3 | `t07l03-0059` | synth-model-a | 10-20 08:19:43 | ↓1059 ↑100 Σ1159 | PASS |

- 5/5 `all_pass`；`closed_clean` 5/5；DB 行与 `stats_request_detail` API 的 request/provider/route/model/status/token 一致（`db_api_consistent` 5/5）。
- 详情费用显示 `CNY ¥0`（夹具 `default_zero`），与 DB 一致；状态 `HTTP 200 · 已完成`；供应商/路由 `Synthetic Provider · Synthetic Route`。
- 证据：`gui-results.json`、`acceptance-summary.json`、`shots/01-…05-*.png`、`logs/ax-*-detail.txt`、`ocr/*.json`。

## 4. 页大小与边界（复验）

| 状态 | 页文本 | 结论 |
|---|---|---|
| 50 条/页 | 第 1/2 | PASS（60 行 → 2 页） |
| 100 条/页 | 第 1/1 | PASS（单页） |
| 20 条/页 | 第 1/3 | PASS（恢复三页） |
| 20/页 第 3 页再点“下一页” | 仍第 3/3 | PASS（末页禁用/无动作） |
| 20/页 第 1 页再点“上一页” | 仍第 1/3 | PASS（首页禁用/无动作） |

- 本轮补上了旧 L03 报告承认“未捕获 page 3/3”的缺口（`shots/09-page3-20.png`）。
- 旧 L03 的 20→50→100 截图与 API 样本为 **REUSE**（`docs/UI-UX-renew/evidence/t07-l03-pagesize-001/`）；本轮独立复验为 **NEW**。

## 5. 范围与限制

- 仅 macOS 中文、非 Retina、1440×900；未测 Windows、英文、Retina/DPI。
- 详情只核对 AX 可见字段与 DB/API；未测试超长值换行/滚动、键盘 Enter 打开详情、空/错误详情。
- OCR 用于页文本，存在识别噪声；关键 ID/Token 以 AX 树与 JSON 比较为准。
- 夹具费用均为 `default_zero`，未覆盖有价/混合币种详情的费用显示。
- 本轮未修改产品源码；未提交/推送。

## 6. 清理

- 停止 GPUI pid 65389 与隔离 core；`ps` 无本任务进程；`lsof` 无 CAP 引用。
- 仅删除本任务 CAP（`fixture.py cleanup`，RUN=CAP）：**6,644 KiB / 19 文件**；父 `.../tmp` 目录保留且现为空（不删除父目录）。
- 证据保留 **4.0 MB**；系统盘可用 **195 GiB**；`target/`、`gpui/target/` 未清理。
- L05 缓存副本误删事故已记录（见文首），未对其它缓存/目录执行清理。

## 7. 附件

- `fixture.py`（本任务 CAP 精确路径；60 行夹具）
- `expected.py` / `expected.json`（60 行 + API 详情 + 页映射）
- `gui_details.py`（5 详情案例 + 关闭检查）
- `analyze.py` / `acceptance-summary.json` / `gui-results.json`
- `logs/`（AX/fixture/expected/analyze/cleanup/incident）、`shots/`、`ocr/`

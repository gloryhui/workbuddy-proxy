# T09-X05-REPEAT-BASELINE-001 命令记录
- 基线 HEAD `344b896…`；`git diff HEAD -- gpui/src src` 空；未改源码。
- 构建：既有 GPUI `5cd6b5a0…`、core `98232731…`；未重构建、未 clean。
- CAP：`.../tmp/t09-x05-repeat-001`；TMPDIR/TMP/TEMP 全指向；realpath 已核对。
- 夹具：`fixture.py setup` → 25 providers / 25 models / 25 prices / 20 rows。
- 为模型选择器把 opencode upstream 临时指向 loopback `mock_provider.py`（`/v1/models` → mock-model-a/b，虚构 Key）；core 重启时使用 `start_core(..., write=False)` 以免覆盖合成 config。
- GUI：1440×900；`repeat.py 10` 执行 10 轮（切供应商、协议弹窗、模型选择器、统计筛选复原、日志滚动），采集 RSS/CPU/FD/线程/窗口/mock 请求/DB/config。
- 结果：`logs/measurements.*`、`logs/summary.json`；截图/AX/OCR 见 `shots/`、`ax/`、`ocr/`。
- 清理：停止 GUI/core/catalog；`fixture.py cleanup`；删除 CAP。CAP 1,196 KiB/24 文件。

## 筛选复核（Codex 反馈）
- 10 轮 `measurements` 的 initial/final AX 值均为“已完成”，不是“全部状态”；脚本菜单坐标未加窗口内容原点 y=30（菜单屏幕 y≈250/272/295），265 命中“已完成”，220 落在菜单上方，故只打开/关闭菜单而未恢复。
- 新 CAP `.../tmp/t09-x05-filter-verification-001`，全新合成 fixture（20 completed/25 providers/25 prices），最小验证：
  - 初始 AX `全部状态`；打开菜单（菜单项无 AX，截图定位）；
  - 屏幕 y=295 选“失败”：AX `失败`，KPI 0 代理；
  - y=272 选“已完成”：AX `已完成`，KPI 20；
  - y=250 恢复“全部状态”：AX `全部状态`，KPI 20。
- 证据 `filter-check/ax/10-before…13-restored-all`、`filter-check/shots`、`filter-check/logs/filter-state-check.json`；10 轮数据不重算。
- 筛选验证 CAP 1,184 KiB/22 文件 → cleanup 后 192 KiB/6 文件 → 删除。

## Corrected 权威 10 轮
- 新 CAP `.../tmp/t09-x05-repeat-corrected-001`；全新 25 providers/25 models/25 prices/20 completed；loopback mock。
- `repeat-corrected.py 10`：每轮 frontmost；协议/选择器/供应商切换/统计筛选/滚动；筛选前 AX `全部状态`，`y=295` 后 AX `失败` + KPI 0，`y=250` 后 AX `全部状态` + KPI 20；10/10 通过。
- 数据：`corrected/logs/measurements-corrected.*`、`summary-corrected.json`；AX/截图/OCR 在 `corrected/`。
- 旧 `repeat.py`/`measurements.*` 保留为坐标错误预备测量，不引用其筛选步骤为通过。
- 清理 corrected CAP 1,196 KiB/24 文件 → 196 KiB/7 → 删除；无 stray mock。

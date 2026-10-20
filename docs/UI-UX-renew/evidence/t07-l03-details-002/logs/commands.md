# T07-L03-DETAILS-002 命令日志

- HEAD 核实：`88affbc1f055b6a59330d75890c7ed3785f14c35`，`codex/ui-redesign`；`git diff 30cfd66..HEAD -- gpui/src src` 为空。
- CAP：`/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t07-l03-details-002`；TMPDIR/TMP/TEMP 全部指向 CAP。
- 夹具：`python3 fixture.py setup`（60 行；20/50/100 返回 20/50/60）。
- 期望：`python3 expected.py`（60 行 DB、20/50 ID、20 行游标三页、5 个 detail API）。
- GUI：`workbuddy-proxy-gpui --config-dir CAP/config`，HOME=CAP/home；System Events 置前；resize 1440×900。
- 详情：AXRow frame 中心点击（700,431 / 700,487）；`axdrive dump` 解析 `AXGroup title=请求标识：…`；`axdrive escape` 关闭。
- 页大小：`axdrive press "20 条/页"` → 50 → 100 → 20；OCR 页文本 `第 X/Y`。
- 边界：第 3 页再按“下一页”、第 1 页再按“上一页”，OCR 页文本不变。
- 清理：停止 GPUI（65389）、`fixture.py cleanup`（仅 CAP）；`du -sk` 6644 KiB；`ps`/`lsof` 核对。

# T08-R06-FAILURE-001 命令记录
- HEAD `99ebb17...`；`git diff HEAD -- gpui/src src` 空；GPUI `89049d…`、core `22cacc6…`。
- CAP `.../tmp/t08-r06-failure-001`；fixture EV/CAP 精确适配，`RUN=CAP/run`；未调用旧 CAP。
- `fixture.py setup` → 20 行、25 价格、core 68157、mock 68156、端口 60748。
- GUI：`workbuddy-proxy-gpui --config-dir CAP/run/config`，HOME=CAP/run/home，TMP=CAP/tmp；System Events 置前；resize 1440×900。
- 预览/确认：滚动到采集与存储底部；`清除历史统计…` (1345,845)；`确认清除` (649,876)。
- A：预览后 `sleep 62` 再确认；截图 02/03；`stats_health` ready；DB 20。
- B：`CREATE TRIGGER t08_r06_abort_delete BEFORE DELETE ON usage_requests ... RAISE(ABORT,'t08 synthetic delete abort')`；确认后查 `stats_maintenance_events`（id2 failed maintenance_failed affected 0）、rows/costs 20、health degraded；移除 trigger、刷新、再次预览。
- 清理：SIGTERM GPUI；`fixture.py cleanup`（删除 run）；核对 ps/lsof；删除 CAP（cleanup 后 192 KiB/6 文件）；父 tmp 空保留。

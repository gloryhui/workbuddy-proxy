# T08-R06-ERROR-RECOVERY-FIX-001 命令记录
- 基线 HEAD `29e834a…`；`git diff HEAD -- gpui/src src` 空；仅 IMPLEMENTATION_STATUS M + 未跟踪设计资料。
- CAP：`.../tmp/t08-r06-error-recovery-fix-001`；TMPDIR/TMP/TEMP 全指向；realpath 已核对。
- 修复：core 安全码 `stats_maintenance_failed`、维护重试成功恢复 Ready；UI 本地化失败提示/健康映射；store 回归测试。
- 测试：
  `cargo fmt --check`
  `cargo test --locked -- --test-threads=1` → 552 passed
  `cargo test --manifest-path gpui/Cargo.toml --locked` → 139 passed
- 构建：core/GUI `cargo build --release --locked`；哈希见 `binary-new.txt`。
- 夹具：`fixture.py setup` 20 行/25 价格；注入 trigger `logs/trigger.sql`；GUI 清除预览→确认失败；移除 trigger→重新预览→确认成功。
- A：预览后 61.5s 确认 → expired；health ready（`logs/a-token-expiry.json`）。
- 清理：`fixture.py cleanup` 仅 RUN=CAP/run；CAP 1748 KiB/25 文件。

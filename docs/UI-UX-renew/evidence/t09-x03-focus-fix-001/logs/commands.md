# T09-X03-FOCUS-FIX-001 命令记录
- 基线 HEAD `660c5dc…`；`git diff HEAD -- gpui/src src` 空；源码 status 仅 IMPLEMENTATION_STATUS M + 未跟踪设计资料。
- 修复：`protocol_options::Editor::initial_focus`（Anthropic=limit，其余=mode）；`ui.rs` protocol_open 使用该焦点；单元映射测试。
- CAP：`.../tmp/t09-x03-focus-fix-001`；TMPDIR/TMP/TEMP 全指向；真实路径已核对。
- 构建：
  `RUSTUP_TOOLCHAIN=stable cargo fmt --check`
  `RUSTUP_TOOLCHAIN=stable cargo test --locked` → 139 passed
  `RUSTUP_TOOLCHAIN=stable cargo build --release --locked` → 1cdc7dc5…
- 夹具：复制并适配 `t08-residual-003/fixture.py`，25 providers / 25 prices / 20 rows；隔离 core/UI。
- GUI：打开 opencode → 协议选项；Tab/Enter/Escape/Space；System Events 键码 48/36/49/53；截图 + AX dump。
- 结果：config hash 始终 `a424698d…`，providers 25，无空供应商；`gui-results.json`。
- 清理：`fixture.py cleanup`（仅 RUN=CAP/run）→ 删除 CAP；CAP 1,304 KiB/26 文件。

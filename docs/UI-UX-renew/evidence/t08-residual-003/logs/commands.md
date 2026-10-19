# T08 关键命令

TMP：`/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t08-residual-003`

## 构建/检查
```
RUSTUP_TOOLCHAIN=stable cargo build --release --manifest-path gpui/Cargo.toml --locked
RUSTUP_TOOLCHAIN=stable cargo fmt --manifest-path gpui/Cargo.toml --check
RUSTUP_TOOLCHAIN=stable cargo test  --manifest-path gpui/Cargo.toml --locked
RUSTUP_TOOLCHAIN=stable cargo clippy --manifest-path gpui/Cargo.toml --locked --all-targets -- -D warnings
```

## 隔离夹具
```
TMPDIR/TMP/TEMP=$TMPROOT/tmp python3 fixture.py setup
python3 fixture.py mode slow|fail|ok
TMPDIR/TMP/TEMP=$TMPROOT/tmp python3 fixture.py stop
```

## GUI
```
HOME=$RUN/home WORKBUDDY_PROXY_CONFIG_DIR=$RUN/config \
  gpui/target/release/workbuddy-proxy-gpui --config-dir $RUN/config
osascript -e 'tell application "System Events" to set frontmost of first process whose unix id is '"$PID"' to true'
bin/axdrive resize $PID 1440 900
bin/axdrive scroll 280 300 -700        # 页面滚动（点在表格左侧，避免滚到表格内部）
bin/axdrive dump $PID ax.txt
bin/axdrive click X Y                  # 折叠/按钮/分页（frame 取 AX 坐标）
bin/type "model-05"; bin/keys a cmd; bin/keys delete
bin/axdrive shot $WID shots/name.png; bin/ocr shots/name.png > ocr/name.json
```

## 关键操作坐标（1440×900）
- 成本定价展开：点击 `成本定价` 按钮 frame 中心；
- 新建价格：`(603,213)`；供应商下拉 `(720,227)` → 选 opencode `(520,275)`；
- 非法价格：输入框 `(534,382)`；保存 `(934,499)`；
- 编辑：价格行 `编辑` 按钮（AX frame `[1313,y 35x19]`）；
- 采集设置：`USD (468,442)`、`365 天 (582,520)`、采集开关 `(415,472)`、Provider B 模式 `(1230,552)`；
- 清除预览 `(1345,845)`，确认 `(649,876)`。

## 修复轮（TMP 004）
```
# paste 输入（原生 type 焦点不稳，改用剪贴板）
printf '%s' "0.8" | pbcopy; bin/keys a cmd; bin/keys v cmd
# 工具：axdrive/ocr 复制自 L05；keys/hscroll/drag/raise 由 tools 源码编译
# 1100 工具栏验证坐标：搜索框 [310,216]，新建 [556,219]，刷新 [656,219]，手动更新 [736,219]
# 价格弹窗（1100）：价格框 y=349，x=307/431/555/679；备注 y=404；保存 (764,479)
# 分页下一/上一页（价格表）：(1376,788)/(1259,788)（按 AX frame 取中心）
```

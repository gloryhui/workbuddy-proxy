# T09 命令与操作日志（macOS 本机，2026-10-20）

## 环境
```
sw_vers; uname -m; system_profiler SPDisplaysDataType
mount | grep -i 1TB           # 无输出：未挂载
which parallel VBoxManage prlctl utmctl wine   # 无输出：无 Windows 环境
git rev-parse HEAD             # 6533bf1605ad4ab395d896d5cc215142d96b3a99
shasum -a 256 gpui/target/release/workbuddy-proxy-gpui target/release/workbuddy-proxy
```

## 隔离运行（L05 副本，不触碰原始保留目录）
```
cp -a $L05/run/config $EV/runtime/config
rm -f $EV/runtime/config/{instance.json,gpui.lock,gpui-stop.json,gpui-wake.json}
core-current --headless --config-dir $EV/runtime/config --port <free>
workbuddy-proxy-gpui --config-dir $EV/runtime/config
```
TMPDIR/TMP/TEMP 指向既有 `$L05/tmp`（空）；未新建构建 TMP，未构建。

## 关键操作
- 三档：`axdrive resize PID 1100 700|1280 800|1440 900`；`axdrive shot WID`。
- 刷新统计：AX `刷新统计` [1034,200 32x32]（1440）。
- 最小化：AX `AXMinimized` window 1 true/false（System Events）。
- 关闭/重开：`keystroke "q" using command down`；重新启动二进制；`axdrive windows`。
- 多实例：同 config 启动第二进程 → 写 `gpui-wake.json` 后退出，窗口数 1。
- 详情：点击日志行 (700,437) → AX `关闭`；`axdrive escape`。
- 剪贴板：`printf '测试中文' | pbcopy`，焦点搜索框，`keystroke "v" using command down`。
- 统计 tab：`供应商统计` [376,351]、`模型统计` [451,351]（滚动后坐标）。
```

cat > "$EV/logs/cleanup.md" <<'EOF'
# T09 清理记录（待执行）

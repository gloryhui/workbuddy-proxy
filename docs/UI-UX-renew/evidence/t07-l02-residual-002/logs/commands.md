# T07-L02 残余复核：关键命令

时间：2026-10-19（Asia/Shanghai）。全部命令以本任务 TMP 为临时目录：
`TMPDIR=TMP=TEMP=/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t07-l02-residual-002`。

## 工具编译（源码见 `tools/`，产物仅在本任务 TMP/bin）

```sh
xcrun swiftc -O hscroll.swift -o bin/hscroll    # 水平滚轮（wheel2）
xcrun swiftc -O drag.swift    -o bin/drag       # CGEvent 鼠标拖动
xcrun swiftc -O sscroll.swift -o bin/sscroll    # Shift+滚轮（备用）
xcrun swiftc -O mouse2.swift  -o bin/mouse2     # postToPid 鼠标（备用，实测 GPUI 不接收）
xcrun swiftc -O raise.swift   -o bin/raise      # NSRunningApplication 激活诊断
cp <L05 保留目录>/bin/axdrive bin/axdrive       # 已授权的 AX 驱动（sha256 cd45327d…）
cp <L05 保留目录>/bin/ocr     bin/ocr           # Vision OCR
```

## 隔离夹具与启动

```sh
cd docs/UI-UX-renew/evidence/t07-l02-residual-002
python3 fixture.py setup            # 启动当前 core，插入 45 行合成数据
# GPUI：
HOME=$RUN/home WORKBUDDY_PROXY_CONFIG_DIR=$RUN/config \
  gpui/target/release/workbuddy-proxy-gpui --config-dir $RUN/config
```

## 窗口、端点、拖动（每档 W×H ∈ {1100×700, 1280×800, 1440×900}）

```sh
bin/raise $PID                                   # 诊断：active/frontmost
osascript -e 'tell application "System Events" to set frontmost of first process whose unix id is '$PID' to true'
bin/axdrive resize $PID $W $H
bin/axdrive dump  $PID logs/ax-${W}x${H}.txt
bin/axdrive windows $PID > logs/windows-${W}x${H}.json
bin/axdrive shot $WID shots/${W}-scroll-left.png
bin/axdrive ocr  ...                             # bin/ocr shot.json

# 页面滚到日志（滚轮点固定在表格左侧 x=280，避免滚到表格内部）
bin/axdrive scroll 280 300 -700      # ×3，再 +150
# 水平端点（点位于表格行区域 y = 分页按钮 y + 170）
bin/hscroll 550 500  1200            # 左端（×4）
bin/hscroll 550 500 -1500            # 右端（×4）

# 列宽拖动（先经 System Events 激活；起点为 Token 列右边界，表头行中心）
bin/drag <boundary_x> <header_y> <boundary_x-140> <header_y> 30   # 拖到下限
bin/drag <boundary_x> <header_y> <boundary_x-150> <header_y> 30   # 再拖，验证不再变窄
bin/drag <boundary_x> <header_y> <boundary_x+100> <header_y> 30   # 拖回恢复
```

## 测量

```sh
python3 summarize.py                 # OCR 表头/数据 x、同步偏差、表头宽差 → acceptance-summary.json
# 列边界像素检测（表头行竖线 #EBEEF1）：
python3 - <<'PY'
from PIL import Image
im=Image.open('shots/1280-scroll-right.png').convert('RGB')
xs=[x for x in range(300,1270) if sum(1 for y in range(214,246)
    if abs(im.getpixel((x,y))[0]-235)<10 and abs(im.getpixel((x,y))[1]-238)<10
    and abs(im.getpixel((x,y))[2]-241)<10)>=26]
print(xs)
PY
```

## 实测关键坐标

- 1100×700：表头行 PNG y≈334，Token 右边界 x=810（拖前）；表头屏幕 y≈372（窗口 y=30 偏移）。
- 1280×800：表头行 PNG y≈214，Token 右边界 x=865；表头屏幕 y≈252。
- 1440×900：表头行 PNG y≈314，Token 右边界 ≈费用表头 x−6；表头屏幕 y≈360。

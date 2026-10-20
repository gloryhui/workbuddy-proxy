# Windows 原生界面截图

Windows 截图优先使用 GDI：Python Pillow 的 `ImageGrab.grab(bbox=..., all_screens=True)`，配合目标进程 PID 和 DWM 原生窗口边界。2026-10-21 已用该方式完成 README 的五张 Windows 10 实拍，见[图片及来源说明](screenshots/README.md)。

## 选择这个方式的依据

同一台 Windows 10 22H2（19045）机器上，Windows Graphics Capture 截图多次出现 `FrameArrived timed out: timed out waiting on channel` 和 `window capture timed out: timed out waiting on channel`。重新绑定窗口、重建工具连接，以及用户切换为仅主屏后均未解决。GDI 随后成功取得实际界面像素。

该记录说明 GDI 在这次环境中可用，不能据此认定所有 WGC 超时的根因，也不能认定多显示器就是原因。后续无需先重复这些失败步骤。

## 操作步骤

1. 使用 Windows 本地交互桌面，保持解锁。公开截图启动隔离演示实例，使用演示 Key 和合成统计数据。
2. 选择已安装 Pillow 的 Python。普通 Python 不一定有 Pillow；Codex 可通过 `load_workspace_dependencies` 获取当前机器的依赖路径。不要把本机运行时、用户名或盘符写成共享脚本的要求。
3. 确认当前目标 PID：本项目可从本次演示配置目录的 `gpui.json` 读取 `pid`。PID 在每次重启后重新读取，不复用旧窗口句柄。
4. 把窗口完整放在屏幕内并置于前台，移开遮挡窗口、通知和不需要的悬浮提示。页面加载后再抓取。
5. 从仓库根目录运行下面的脚本，检查实际 PNG，再决定是否写入 README。

PowerShell 示例（变量值由当前会话提供）：

```powershell
# $capturePython：当前已安装 Pillow 的 Python 可执行文件
# $demoConfig：本轮隔离演示配置目录
# $captureOutput：已创建的本轮截图目录
$targetProcessId = (Get-Content -LiteralPath (Join-Path $demoConfig 'gpui.json') -Raw | ConvertFrom-Json).pid
& $capturePython scripts/windows_screenshot.py --pid $targetProcessId --output (Join-Path $captureOutput 'overview.png') --activate
```

`--activate` 只还原并激活已选中的窗口；脚本不启动应用、不改变页面、不点击按钮，也不调整窗口尺寸。输出位置的父目录必须存在，文件名必须是尚不存在的 PNG。成功时标准输出包含实际边界、像素尺寸、PID 和 SHA-256，便于保存为证据。

脚本在创建新的 Python 进程后先启用 per-monitor DPI awareness，以物理像素读取 DWM 的 `DWMWA_EXTENDED_FRAME_BOUNDS`，排除不可见窗口边框；`all_screens=True` 覆盖多显示器及负坐标。按 PID 枚举到的可见窗口必须唯一，最小化、未在前台或越出虚拟桌面时会报错。

## 拍摄与验收

- GDI 抓取屏幕上的可见像素，不提供后台或遮挡窗口的独立表面。前台检查不能保证没有悬浮窗口遮挡，仍须逐张查看。
- 多屏、DPI、分辨率或窗口尺寸改变后重新获取边界；不套用旧截图坐标。通常无需关闭副屏。跨屏窗口存在裁剪或缩放异常时，先把窗口完整放到一块屏幕内。
- 本次供应商、汇总和皮肤页使用 1440 × 900 外部窗口；概览和十条明细使用 1440 × 1100，以完整显示图表及记录。尺寸要根据实际工作区调整，避免离屏；不是所有机器必须使用这些尺寸。
- 数据通过真实应用界面展示。等待查询完成，移开图表提示后截取概览；皮肤菜单等需要展示的弹层保留。截图不重绘、不拼接，不把旧图改名当作新图。
- 检查字体、固定筛选区、表头、分页、工具名称和 Token 不适用等预期内容，以及是否有真实配置、密钥、其他应用或个人记录进入画面。
- 文档记录拍摄日期、系统、应用源码版本、主题、PNG 尺寸和演示数据来源；区分 main 构建与正式 Release 附件。
- 按项目临时目录规范运行隔离实例；结束后先停止并等待本轮子进程退出，再清理本轮配置、数据库及重复截图。保留交付图片和必要报告。

如果 GDI 失败，记录实际错误并检查桌面会话、目标 PID、前台状态、可见边界和 Python / Pillow 环境。没有新的环境变化时，不反复重试同一失败调用；换用其他通道前先核对其当前限制。

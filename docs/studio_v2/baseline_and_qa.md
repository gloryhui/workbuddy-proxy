# Studio V2 实施与验收记录

## 环境与边界（2026-10-28，北京时间）

- 原分支 `main` / `origin/main`：`e96f5a867946a63504b6ec7d9ce4a78762434e32`。fetch 后一致。
- 原目录的未跟踪 `output/` 保留；通过 managed worktree 从 `origin/main` 建立 `feat/studio_v2_gpui`。用 `git worktree list --porcelain` 查当前路径，不依赖特定机器路径。
- macOS 15.8.1 (24H32), Apple Silicon arm64；cargo 1.98.1、rustc 1.98.1、stable-aarch64-apple-darwin。依赖与锁文件未升级，gpui-kit 仍为 `=0.7.3`。
- 复用原 checkout 的两个 Cargo target 缓存；会话通过本地 `CARGO_TARGET_DIR` 指定。TMPDIR/TMP/TEMP 指向系统临时目录内本会话子目录；没有 cargo clean。
- 不变更核心热路径、协议、配置格式、SQLite、同步恢复和版本号。默认皮肤仍为黑白，六个旧枚举值、appearance.json version 1 保持。

## 改造前基线

| 检查 | 结果 |
| --- | --- |
| 核心 fmt / clippy / test / release | Passed；573 项单测 |
| GPUI fmt / clippy / test / release | Passed；155 项单测 |
| 既有提示 | 依赖 `block 0.1.6` future incompatibility warning；未放宽 lint |
| Release 核心 / GPUI 大小 | 8,428,096 / 14,761,312 bytes |
| 同机 loopback mock 基准 | [baseline-core.json](benchmarks/baseline-core.json)，200 小请求，64 模型，80 个 1 MiB 请求、4 并发 |
| 核心空闲 RSS | 13.11 MiB；采样峰值/负载后 61.78 MiB |
| 请求 p50 / p95 | 0.190 / 0.449 ms；本地 HTTP mock，不包含 TLS/真实模型 |
| UI 基线（21 秒单次采样） | 核心 13,392 KiB + UI 95,776 KiB = 109,168 KiB（106.61 MiB） |
| UI 基线启动 / 截图 | gpui.json 就绪 0.709 秒（不代表首次绘制）；[baseline](screenshots/baseline-1440x900.png) |

发现原 `scripts/benchmark.py --gui` 只读取启动进程和 instance.json PID；当前架构中这两个是托盘和核心，gpui.json 才是设置 UI，原脚本遗漏 UI。修正前不使用其 GUI 输出声称性能改善。

## 小功能 1：七项原生工具栏（Figma 103:442）

- `ui_components::system_button` 统一 Kit Button、20px SVG、36px 命中区、Tooltip/无障碍名称与禁用态；`app_controls` 严格按衣服、电源、刷新、地球、恢复、GitHub、反馈排列，间隔 8px，总宽 300px。
- 七个 SVG 来自官方 MCP 高保真 context，根尺寸/viewBox 均为 20×20，本地嵌入，不使用临时 Figma URL 作为运行资源。保留原皮肤弹层、开机启动 action、更新、语言菜单、尺寸恢复与打开 URL 的真实处理器。
- 修改文件：`gpui/src/{assets.rs,theme.rs,ui.rs,ui_components.rs,ui/skin_controls.rs}`；新增 `gpui/assets/studio/`。未改造其他页面结构，后续继续施工。
- 验证：GPUI fmt / clippy / 155 test / release build 全部 Passed。macOS 隔离 GUI：7/7 图标可见、AX 控件存在，六皮肤弹层/Esc、语言菜单/Tooltip、中英实际切换、缩小后恢复 1440×900 均通过。未操作真实登录项和外部浏览器链接；36px 仅由代码与截图核对，未测 AX 几何。Windows、Intel Mac、HiDPI/键盘逐项验证未运行。
- [工具栏截图](screenshots/toolbar-1440x900.png)：原始窗口捕获 1440×901，PNG 转换仅裁去底部 1 行 macOS 窗口框，内容没有重绘。两图均为隔离合成 Review 账户、空记录。核心有内核沙盒；GUI 使用隔离 HOME/配置，受保护 GUI 无法由 CUA 绑定，截图轮沿用仓库默认 GUI 启动方式。
- 工具栏后单次 RSS（44 秒）：核心 13,616 KiB + UI 99,488 KiB = 113,104 KiB（110.45 MiB）。与基线采样时刻不同，不据此宣称内存改善/退化。启动至 gpui.json 为 0.812 秒；启动至首次 AX 返回上界 4.958 秒，包含观察间隔。最终由修正后的同口径脚本复测。
- UI 子代理已结束本轮全部进程，清理会话专属测试目录（94,192 KiB），交付截图保留，临时文件剩余 0。系统盘剩余约 39 GiB。此轮通过终止测试进程退出，尚未验收用户关闭窗口后核心继续运行。

## 后续范围

继续按功能拆分：修正 GUI 性能测量；Studio 语义 Tokens 与主布局；供应商搜索、容量禁用、固定操作栏与草稿保护；主编辑区/四协议；统计页面与全部弹窗。最终逐画板、六皮肤、两种窗口尺寸、生命周期和跨平台验证。不能把本次工具栏的小功能视为整个 V2 完成。

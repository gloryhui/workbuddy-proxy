# Issue #4：系统字体、精简打包与 Windows 卸载修复

2026-11-02，本地分支 `codex/issue4-system-fonts`；基线 `763f2aefb5e644d84250590aaa3bdc96a90b6e11`。改动尚未提交、推送或发布。原工作区 #5、#6 的四个文档已逐一核对 SHA256，保持原样；当前交付位于独立 worktree，可用 `git worktree list --porcelain` 定位。

## 改动

- 删除 7 个完整 Noto Sans SC / Inter 字体文件（26,889,052 bytes）、`studio_fonts.rs`、进程字体状态和 macOS 注册 FFI / 注册专用测试。
- 移除 23 处 Latin 字体包装；页面文本继承原生 UI 字体，保留字号、权重、行高和数字特性。图表日期仍保留原来的 normal 权重，旧主题协议代码文本仍使用原有等宽字体。
- `theme.rs` 复用现有可用字体选择：Windows 只选择实际枚举到的候选；macOS 保留 Kit 原生字体与系统 fallback。各主题共用此策略，不再有 Studio 专属字体状态。没有改动依赖、Core、协议、配置格式或版本号。
- `scripts/package.py` 改为显式复制运行文件、图标及必要许可证。移除 `docs/`、`Documentation/`、`README.md`、`DESKTOP.md`、`CHANGELOG.md`、截图及历史材料。法律声明转成 `THIRD-PARTY-NOTICES.txt`，与许可证一起存放。
- `packaging/windows/workbuddy-proxy.iss` 增加卸载前退出：按可执行文件完整路径匹配本安装的窗口，发送 WM_CLOSE，让托盘既有退出路径关闭设置并停止 Core；最多等待 15 秒。文件仍被占用或无法访问则中止卸载并提示；只额外移除空的 `internal`，不递归删除整个安装目录。

[可阅读的完整源码 diff](../../dist/issue4-system-fonts/source-changes.patch)；完整文件清单见 [结构化测量记录](benchmarks/issue4-system-fonts.json)。

## 同条件体积对比

前后 UI 在同一工作区、同一 ARM macOS / Rust 1.98.1、同一 Cargo.lock、相同 Release 参数下构建；复用已有缓存。两个 DMG 均使用本次**去除文档后的同一打包脚本**、同一 Core、图标、许可证、Python 3.14 / dmgbuild 1.6.7、UDZO/HFS+ 和 ad-hoc 签名。旧字体一侧的 DMG 是本轮重打的对照包，不是旧公开 Release。

| 产物 | 原字体 bytes | 系统字体 bytes | 减少 bytes / 比例 |
| --- | ---: | ---: | ---: |
| GPUI 原始 Release 可执行文件 | 43,810,848 (41.78 MiB) | 16,697,632 (15.92 MiB) | 27,113,216 / 61.89% |
| macOS ARM64 DMG（均不含文档） | 39,109,397 (37.30 MiB) | 15,970,708 (15.23 MiB) | 23,138,689 / 59.16% |

只读挂载两份 DMG 后核对：每份 app 内均为 12 个明确的运行/图标/签名/许可证文件；没有 Markdown、docs、截图或松散字体。7 个完整字体数据在基线 UI 和包内 UI 中均存在，在候选 UI 与包内 UI 中均不存在。主 app、设置 app 的签名通过；除签名元数据外，包内 Core/UI 与对应输入二进制逐字节一致，Core 的规范化代码哈希前后一致。

两份 DMG、SHA256、UI 二进制及原始验证日志保存在仓库的 `dist/issue4-system-fonts/{before,after}/` 与 `validation-logs/`（Git 忽略，本地交付产物）。它们未上传或安装到系统应用目录。

## 已执行验证

- GPUI fmt、严格 `clippy --all-targets -- -D warnings`、单元测试：**215 passed / 2 ignored**（既有 real-core fixture 测试忽略，未改为通过）。
- ARM64 Release 编译、Intel `cargo check --target x86_64-apple-darwin` 通过；Intel 类型检查不等同原生 GUI 或 DMG 验收。
- 打包/版本回归 7 项通过，包含真实 Windows ZIP 生成路径的精确 8 文件白名单检查；输入 EXE 为合成夹具，不是 Windows 可运行程序验收。
- 三个改动 Python 脚本语法检查、diff 空白检查、包内清单、签名与代码身份检查通过。Rust 依赖 `block 0.1.6` 有既有 future-incompatibility 提示，本次未改依赖或放松门禁。

## 原生 GUI 与截图

macOS ARM64，隔离合成配置：1 供应商、2 模型、3 请求、36,150 Tokens；前后共用同一 Core / 数据。Key 保持遮罩。宽窗 1440×900、小窗 1100×700；原始 PNG 为 2880×1800、2200×1400，确认为 **2× HiDPI**。

| 画面 | 修改前 | 修改后 |
| --- | --- | --- |
| 供应商宽窗 | [before](screenshots/issue4-before-provider-wide.png) | [after](screenshots/issue4-after-provider-wide.png) |
| 统计概览宽窗 | [before](screenshots/issue4-before-overview-wide.png) | [after](screenshots/issue4-after-overview-wide.png) |
| 供应商小窗 | [before](screenshots/issue4-before-provider-small.png) | [after](screenshots/issue4-after-provider-small.png) |

已核对模型编辑弹窗的中文、英文、数字、万/亿、币种符号、日韩、阿拉伯和 emoji；合成文本输入、全选、复制粘贴 round trip、取消恢复；协议弹窗、统计设置/明细/图表、Studio↔Mono、中英切换及小窗滚动到底部动作可达。没有发现本次新增的明显裁切、缺字或交互阻塞。详见 [GUI 记录](issue4-gui-review.json) 与 [多语文本弹窗截图](screenshots/issue4-after-multilingual-dialog.png)。

既有问题经精确条件对照：新进程以 Studio 首次打开明细后切 Mono，旧、新版本的供应商标签都裁切；直接 Mono 启动则完整。它与初始表格状态有关，不能归为字体新增回归，本轮未扩大修改。对照：[原字体同路径](screenshots/issue4-before-studio-first-mono-detail.png) / [候选](screenshots/issue4-after-mono-detail.png)。供应商标题与超长模型名在基线中也已有省略号。运行区的 WorkBuddy 地址检查拒绝信息来自演示 Core 对真实配置路径的隔离，不是本次字体错误。

空闲 RSS 仅作观察：基线三次中位数 78,848 KiB，候选 90,144 KiB；候选走过更多页面/弹窗，工作负载不同，不能据此判断内存收益或回归。本轮未测受控冷启动与稳定空闲 RSS，不声称内存或启动加速。

## Windows 卸载：已改代码，待实机验证

现有 `CloseApplications=yes` 处理安装阶段；原测试先正常退出才卸载，未覆盖用户反馈的运行中残留。依据 [Inno 安装阶段说明](https://jrsoftware.org/ishelp/topic_setup_closeapplications.htm) 和 [卸载事件](https://jrsoftware.org/ishelp/topic_scriptevents.htm) 增加独立卸载退出流程。文件清理保留安装日志拥有的精确文件，并只移除空 `internal`，遵循 [UninstallDelete 的目录边界要求](https://jrsoftware.org/ishelp/topic_uninstalldeletesection.htm)。

Windows CI 测试已补：程序托盘/Core/设置运行中卸载，检查三个进程退出、统计正常结束、主程序与 internal 消失、外部配置字节不变；另启动其他路径的同名 Core，并在安装根目录放合成用户文件，要求二者保留。测试夹具拥有清理与等待逻辑。

**当前没有 Windows / Inno Setup 编译器：上述卸载测试和新 Inno 脚本尚未编译、未实机执行。** 旧 Windows 卸载程序不会因本地源码修改自动更新。本轮没有操作用户截图中的 Windows 安装目录。

其他未覆盖项：Windows 字体 GUI / 安装包体积 A/B、Intel GUI / DMG、1× 缩放、真实 IME 组合输入、屏幕阅读器、完整 40 画板及正式安装/升级门禁。

## 清理、审阅与回滚

测试进程已退出并等待，DMG 已卸载；本轮专用演示数据库、配置、应用副本、打包 venv、临时 DMG 与 Python 缓存已清理。两个已记录专用临时目录及 Python 缓存共移除约 294 MiB 逻辑文件；本轮临时资源保留 0 bytes，必要截图/报告和约 110 MiB 本地产物保留。Cargo 缓存保留；系统盘最后检查剩余约 40.85 GiB。

回滚仅需反向撤销此独立分支的字体/打包/卸载改动，恢复原模块、七个资源和调用点；无数据库或用户配置迁移。尚未创建提交/PR，未推送、合并、修改 main 或发布。建议分成字体与打包/卸载两组审阅后再提交，每条提交末尾使用：

```text
Co-authored-by: Codex <codex@openai.com>
```

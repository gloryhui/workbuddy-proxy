# 第二轮体积优化与风险验证

日期：2026-11-02。分支：`codex/issue4-system-fonts`。本轮基线已经包含 #4 系统字体和运行包排除文档的改动，不能与最初含内嵌字体的版本混作同一轮收益。原工作区 #5/#6 文档及本分支之前的 45 项文件状态均保留，逐文件 SHA-256 复核通过。未提交、未推送、未发布。

## 最终体积（macOS ARM64）

| 项目 | 优化前（字节） | 优化后（字节） | 减少 |
|---|---:|---:|---:|
| Proxy 可执行文件 | 8,428,112 | 6,240,208 | 25.96% |
| GPUI 设置可执行文件 | 16,697,632 | 14,649,264 | 12.27% |
| ICNS 单份资源 | 1,689,268 | 1,346,680 | 20.28% |
| 安装后 .app 文件合计 | 28,562,087 | 23,640,639 | 17.23% |
| ARM64 DMG | 15,970,730 | 13,279,002 | 16.85% |

MiB = 1,048,576 字节。DMG 为 15.23 → 12.66 MiB。两组采用相同版本、打包脚本、dmgbuild 工具和运行文件白名单，分别配套旧/新程序和图标；没有把文档排除收益重复计入。压缩容器与签名含生成时信息，不承诺不同重打包之间字节完全相同。

## 1. Proxy 编译参数 A/B

`Cargo.toml` 将 release `opt-level = 3` 改为 `"z"`，保留 thin LTO、单 codegen unit、strip 和 unwind 恢复。实际存在 `catch_unwind` 恢复路径，因此没有采用 panic=abort。没有删协议、配置、统计或依赖功能。

同源程序交错顺序 `3,s,z,z,s,3`，每种 2 次；每次 200 个小请求、64 个模型、80 个 1 MiB 请求/并发 4，独立 loopback HTTP mock，测试时没有并行编译。表格为两轮指标的中位数。

| release opt-level | 文件字节 | 小请求 p50 ms | 小请求 p95 ms | 大请求总时间 s | 空闲 RSS MiB | 就绪标记 ms |
|---|---:|---:|---:|---:|---:|---:|
| 3 | 8,428,112 | 0.1935 | 0.2780 | 0.0965 | 11.82 | 373.84 |
| s | 7,229,856 | 0.1975 | 0.2455 | 0.0885 | 11.07 | 545.05 |
| z | 6,240,208 | 0.2020 | 0.2395 | 0.0850 | 10.29 | 339.65 |

选择 z：比 3 小 25.96%，比 s 再少 989,648 字节。小请求 p50 增加 0.0085 ms（约 4.4%）；p95 和大请求结果没有显示出超出本轮波动的退化，但样本少，不能据此宣布变快。首轮启动明显较慢，就绪数值不能解释为磁盘冷启动性能。结果不覆盖 TLS、真实网络、长时间大流量或 Windows。最终重新编译的 Core 与参与 A/B 的 z 程序 SHA-256 完全相同。

## 2. GPUI 导出和解码器

在 `gpui/build.rs` 仅 macOS 的设置程序链接时加入 `-Wl,-no_exported_symbols`。该程序没有对外插件 ABI，Cocoa 回调通过注册地址使用；本轮 GUI 实测覆盖相关窗口/事件路径。Mach-O 导出 trie 从 2,032,464 字节降到 8 字节，文件减少 2,048,368 字节（12.27%）。Windows 不使用这条链接参数。

解码器按 Cargo feature 合并后的实际依赖检查。曾对 image/exr/tiff/zune-jpeg 试用局部 z 编译，额外只省 112,304 字节，同时 JPEG 示例 p50 从约 12.50 µs 到 20.62 µs，PNG 也出现变慢，因此撤回。`gpui/Cargo.toml` 已与本轮起始内容逐字节一致；没有移除任何解码格式，也没有新增运行依赖。

新增开发用 `gpui/examples/image_decode_probe.rs`，通过 GPUI 公开解码 API 对 10 个样本、9 类公开格式（PNG/JPEG/GIF/WebP/BMP/TIFF/ICO/SVG/PNM）核对像素 hash、帧尺寸和动画延时；每项预热 5 次、测 100 次。前后全部一致；最终 probe 程序本身与基线字节相同，说明解码器试验已完全回退。未为只在依赖内部开启的其他格式作完整契约声明。

GUI 六次启动顺序 before/after/after/before/before/after，复用隔离 Core、每次新 UI 进程，未在测量时编译：

| 指标 | 前 | 后 |
|---|---:|---:|
| gpui.json 就绪标记中位值 | 374.24 ms | 327.53 ms |
| 空闲 RSS（各次 3 个采样中位值，再取中位） | 90.86 MiB | 93.12 MiB |

首轮启动和 RSS 波动明显；后者约高 2.27 MiB，不声称内存下降。就绪标记不是首帧或可交互时刻，也不是磁盘冷启动。导出优化的确定收益是磁盘体积。

## 3. 图标

`scripts/optimize_icns.py` 使用标准库重压缩 PNG IDAT，无缩放、无重绘、无量化。保留全部 11 个 ICNS 槽位及顺序；8 个 PNG 槽位经独立 Pillow 解码逐像素一致，非 PNG 数据和 PNG 元数据均保留，包括 1024 像素资源。重压缩可重复且幂等。

单份 ICNS 节省 342,588 字节（20.28%）；主程序和设置 helper 各一份，安装资源合计节省 685,176 字节。没有借删除高分辨率槽位换体积。

## 验证与边界

- Core：fmt、严格 clippy all-targets、580 测试全部通过；release 编译、smoke、native_protocol_test、statistics_test 通过。
- GPUI：fmt、严格 clippy all-targets、215 测试通过，2 个既有 ignored；macOS Intel 类型检查通过（没有 Intel 链接或实机运行）。
- 打包：7 个版本/运行文件测试通过。两组 DMG 均 hdiutil verify，通过只读挂载、主/助手 codesign --verify --strict；12 个 .app 文件符合白名单，无 docs/Markdown/旧截图/字体。签名归一化比较全部非签名字节与已测二进制一致，并带代码篡改负向检查。
- GUI：1440×900 与 1100×700（2x），供应商、概览图表/图标、统计设置、多语种模型弹窗、复制粘贴/取消、协议弹窗、Studio/Mono 切换及小窗口滚动。截图和观察见 [GUI 记录](size2-gui-review.json)。没有保存真实数据、同步、登录或改变系统自启动。
- 原生关闭候选窗口后无该 helper 进程或锁持有者，Core 继续运行；之后通过管理接口正常停止 Core，3 条合成记录、36,150 tokens、normal stop、0 drop/0 write failure 均保留，测试进程全部退出。期间出现同一候选 bundle 的新 PID，触发原因未确定，已按实际进程补验并记入 GUI 记录。
- Cmd+W 在候选观察窗口中未关闭窗口，未建立相同基线快捷键比较，不据此声称该路径回归通过；原生关闭已验证。没有自动开启 VoiceOver 或改变输入法。
- Windows 编译/安装卸载实机环境当前不可用：本轮不提供 Windows 新体积或性能结论；此前卸载补丁也仍属于待 Windows 验收状态。未把 Mac 的收益外推到 Windows。

## 证据与改动

- 机器可读汇总：[benchmarks/size-round2.json](benchmarks/size-round2.json)。
- 本地未入包产物：`dist/size-round2/`，含 before/after DMG、SHA-256、3/s/z Core、GPUI 前后程序、原始 benchmark/decoder JSON、日志和逐像素审计。`gpui-decoder-z-rejected` 明确为已拒绝候选，不属于最终产物。
- 本轮实际新增/调整的运行构建文件只有 `Cargo.toml`、`gpui/build.rs`、`assets/app-icon.icns`；开发验证文件为 `gpui/examples/image_decode_probe.rs` 和 `scripts/optimize_icns.py`，以及本报告和测试截图。此前字体、打包、Windows 卸载和 #5/#6 文档改动均保留。
- 完整工作区补丁与仅本轮补丁保留在 `dist/size-round2/`。文档/截图只作为开发证据保留在仓库中，发布打包白名单不会包含它们。

## 临时文件清理

本轮专用临时目录已在进程退出、卸载 DMG 和完整路径/归属/引用检查后移除，删除临时内容 97,940,760 字节（约 93.4 MiB）；本轮临时目录剩余 0。未删除 Cargo 缓存、其他会话目录或用户数据。保留的本地证据与产物约 177.0 MiB（含补丁、候选对照程序和 DMG，不进入安装包）；清理后系统盘可用约 44.09 GiB。详细清理证据见 `dist/size-round2/cleanup-preflight.json` 与 `cleanup-result.json`。

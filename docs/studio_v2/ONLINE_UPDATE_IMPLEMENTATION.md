# 检查更新与在线安装实现验收

日期：2026-11-02。分支：`codex/issue4-system-fonts`，基线 `763f2ae`。功能仍未发布，版本仍为 0.6.0；未提交、推送或修改真实安装。#5 / #6 文档所在原工作区保持原状；本分支此前 #4 字体、编译、导出、图标、文档排除与 Windows 卸载改动均保留。

## 后续界面调整（同日）

根据实际使用反馈，已是最新版时隐藏“暂不更新”，保留“前往 GitHub / 完成”；移除“查看更新日志”入口及其展开状态，错误与备份位置仍直接展示，后台检查和安装日志继续保留。语言、开机启动反馈、恢复窗口反馈改为按各自按钮定位，下方间距为 4px，不再将整条工具栏作为锚点再下移 71–73px；皮肤设置保持原样。浮层展开期间暂停对应按钮的悬浮提示，避免遮住关闭控件。

这些后续改动在 GPUI release 构建和隔离窗口中单独验证，详见 [工具栏浮层验收](toolbar-popovers-gui-review.json)。下文的 DMG 大小、摘要、helper 测试和旧截图均对应首次实现时的构建，保留为历史证据，尚未重新打包包含本节调整的 DMG。

## 交付行为

- 根据 Figma `zxDfIQ4lhNcTXrFT7qk8QZ` 的 `127:1027` 更新弹窗落地，620 × 560、固定底部操作、可滚动 Release 说明与时间日志，支持中英和现有主题。只增加一枚设计稿关闭 SVG，沿用系统字体和现有依赖。
- 点击检查有明确反馈和活动日志，展示最新正式版名称、版本、时间、说明；提供“立即更新 / 前往 GitHub / 暂不更新”。失败有错误与重试，相同或旧版不触发升级。
- 下载进度与取消、精确平台包选择、长度和 SHA-256 校验、只读 DMG 准备、二次确认“重启并安装”。固定公共 GitHub 源，生产不接收任意 URL 或测试版本。
- 暂停新请求并排空旧请求（包括完整流式响应），冷备份程序、配置、SQLite + WAL，由独立 helper 替换并重启，独立只读窗口显示进度。
- 新 Core 的维护接入保护持续到版本、服务和统计库健康检查完成；失败恢复旧程序与数据并再次检查。恢复未确认时保留警告与接入保护，关闭弹窗不能清除恢复状态。

使用与恢复说明见 [AUTO-UPDATE.md](../AUTO-UPDATE.md)。原公开 v0.5.7 / v0.6.0 没有这套更新器，首次使用需要手动安装包含此次实现的新包。

## 改动入口

| 文件 | 内容 |
| --- | --- |
| `src/update.rs` | Release 元数据、缓存兼容、正式版本判断、平台可用性 |
| `src/updater/dto.rs` / `mod.rs` | 共享状态、下载、校验、取消、排空和安装调度 |
| `src/updater/install.rs` | 安装计划校验、独立 helper、锁、备份、健康检查与回滚 |
| `src/proxy.rs` | 认证命令、可见日志、请求与重启维护保护 |
| `src/main.rs` / `src/frontend.rs` | 控制协议 13、内部 helper 入口和兼容检查 |
| `gpui/src/ui/update_notice.rs` / `ui.rs` | 完整更新弹窗与交互 |
| `gpui/src/update_monitor.rs` | 安装期间独立只读进度窗口 |
| `gpui/src/backend.rs` / `main.rs` / `assets.rs` | 轻量状态轮询、进度入口、唯一新增 SVG |
| `scripts/online_update_test.py` | 合成认证 API 回归、可选真实检查 |
| `scripts/online_update_macos_test.py` | 真实 helper 安装成功 / 启动失败回滚夹具 |

既有协议回归脚本从协议 12 同步为 13。没有新增运行时依赖，两个 Cargo.lock 均未修改。原工作保存检查确认 58 项文件内容 / 删除状态一致，唯一重叠是 `gpui/src/main.rs` 增加进度入口，原系统字体设置保留。

## 验证结果与边界

本地原始输出保存在忽略目录 `dist/online-update/`，不进入安装包。报告中“通过”仅针对以下范围。

| 项目 | 结果 / 证据 |
| --- | --- |
| Core 全套 | 594 通过，0 忽略；`core-tests-final.log` |
| 末次恢复状态修复 | 14 个 updater 定向测试通过，含新增“关闭弹窗不能抹除维护警告”；`updater-tests-final.log`。全套 594 后只增加这一测试及对应保护分支，未再次重复整套 |
| GPUI | 214 通过，2 个既有忽略；`gpui-tests-final.log` |
| 静态检查 | Core / GPUI strict clippy all-targets、两边 fmt、diff whitespace 检查通过 |
| Intel Mac | Core / GPUI x86_64-apple-darwin 类型检查通过，没有链接或运行验收 |
| API | 认证、无效版本 / 未校验安装拒绝、日志、关闭保留日志、非法 apply 不阻断代理；`api-final-results.json` |
| 真实 Release 检查 | GitHub HTTP 200，v0.6.0，名称、日期和 2305 字节说明，非缓存；`api-results.json`。未执行公网安装包下载 / 安装 |
| 代理回归 | 原生三协议 JSON / SSE、错误、图片、思考、并行工具、用量和不可变价格通过；`native-protocol-test.log` |
| 下载单测 | 损坏 / 截断包、已有文件保护、未返回响应头时取消、精确清单匹配通过 |
| 数据保护单测 | SQLite 非空 WAL 冷复制、备份恢复、持有锁时不能替换、精确计划路径、新 Core 健康确认前拒绝请求通过 |
| macOS ARM helper | 真实替换、重启及启动失败回滚；配置字节一致，3 条 / 36,150 token 合成用量保留，健康确认后维护标记释放；`engine-delivery/engine-results.json` |
| GPUI 界面 | 1440 × 901、1100 × 700 窗口，长说明 / 日志滚动、固定底部、下载 42%、取消、稍后安装、安装不可取消、最新 / HTTP 403 / 重试、Tab / Shift+Tab / Enter / Escape；[GUI 原始报告](online-update-gui-review.json) |
| 包内容 | DMG 校验、主 / 设置程序严格签名、全部非签名机器码与最终构建一致，负对照有效；`package-audit.json` |
| 文档排除 | 最终 app 12 个有效载荷文件，无 docs、Markdown、截图或字体；Windows staging payload 单测同样通过 |

helper 集成测试使用隔离安装目录、内核拒绝访问真实用户配置和实际安装。旧 / 新二进制均显示 0.6.0，通过控制协议 12 → 13 区分；它直接提供已签名暂存应用与校验夹具，不伪装成跨公开版本的公网全链路升级。成功案例保留程序和数据备份；失败案例只有旧版实际重启并健康后才报告恢复。

GUI 验收使用真实 GPUI 加合成控制器，其截图展示的 v0.5.7 → v0.6.0 为演示状态；未在用户真实窗口点击下载、安装或打开外链。辅助功能树可读标题 / 版本 / 日期 / 正文 / 日志；AX 焦点仍报告窗口，未启用或验收 VoiceOver。

发现并修正的本轮问题包括底部边框裁切、正文缺少 AX 标签、阶段切换沿用旧滚动偏移、GPUI 测试误导入宏导致编译栈溢出，以及 macOS 测试夹具 `/var` 与 `/private/var` 别名比较。原始失败日志保留，不通过放松生产路径检查或增大编译栈来规避。

**尚未验证**：Windows 实机下载 / 安装 / 回滚、Intel Mac 运行、真实公网未来版本完整下载到安装、独立进度窗口的专项键鼠验收、系统安全提示和辅助技术朗读。Windows 实现保留，但本机没有 Windows 运行环境或 MinGW C 工具链，不将类型 / 单测结果表述为 Windows 构建成功。

## 体积对比（macOS ARM64）

基线为此前完成编译 / 导出 / 图标优化后的二进制，本次新增更新功能后测量，均为原始 release 可执行文件字节数：

| 项目 | 原大小 | 当前大小 | 增量 |
| --- | ---: | ---: | ---: |
| Proxy | 6,240,208 B | 6,358,000 B | +117,792 B，+1.89% |
| GPUI | 14,649,264 B | 14,715,408 B | +66,144 B，+0.45% |
| 合计 | 20,889,472 B | 21,073,408 B | +183,936 B，+0.88% |

本地 review DMG 为 **13,384,510 B（12.76 MiB）**，SHA-256 `db868096c0c53dd301bed1482c122e00ceee578c8792046016f648f8fed3b98c`。仍标记 0.6.0 且文件名含 `online-update-review`，仅用于本次本地审阅，不能冒充新正式发布。

本轮没有重新测量吞吐、CPU 或内存，不能宣称性能提升。下载 / 安装运行在独立线程，UI 仅在需要时轮询；常态接入路径不新增文件系统检查。安装明确暂停新请求属于预期行为，等待可以推迟。

## 审阅与交付

本地审阅包、校验、补丁和机器证据均在 `dist/online-update/`；功能补丁 `implementation.patch` 包含新文件，少量重叠文件同时保留原分支改动。截图在本目录 `screenshots/online-update-*-final.png`。操作时不需要覆盖当前真实安装才能审阅代码与图片。

后续如提交，建议消息：

```text
feat(updater): add release dialog and confirmed online updates

Co-authored-by: Codex <codex@openai.com>
```

此次没有执行提交命令，Author / Committer 和全局 Git 设置保持原状。

## 本轮临时文件收尾

已停止并等待本轮夹具与 helper 退出，确认无打开文件或挂载后清理专用临时目录，释放 **403.13 MiB**；本轮临时文件剩余 **0 B**。保留构建缓存和约 **13.50 MiB** 审阅包 / 报告 / 补丁，系统盘可用约 **42.62 GiB**。实际字节与清理预检见本地 `dist/online-update/cleanup-results.json` / `cleanup-preflight.json`。

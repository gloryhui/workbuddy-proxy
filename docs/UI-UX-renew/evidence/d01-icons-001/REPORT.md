# D01 顶部图标直观化

**任务：** D01-ICONS-001
**分支 / 基线：** `codex/ui-redesign` / `f0ba5f03cf87bb12838fd001f5d8abe8603b0b06`
**结论：** **完成（视觉/静态验证）**。六个动作、顺序、ID、真实回调、tooltip/aria、disabled/busy、autostart selected/toggled 语义均未改；仅重绘四个易误解图标，语言 globe 与 GitHub 保留。macOS 1440×900 与 1100×700 截图/AX 显示六个图标完整、无重叠，四个 tooltip 可见。**未点击任何动作**，未验证更新检查/外链/开机启动/恢复窗口的实际执行。

## 1. 基线、范围与构建

| 项 | 值 |
|---|---|
| 基线 HEAD | `f0ba5f03cf87bb12838fd001f5d8abe8603b0b06`；未 reset |
| 允许改动 | `gpui/src/assets.rs`、`gpui/src/ui.rs`（app_controls 图标）、本 evidence |
| 实际 source diff | 2 文件，+17 / −7（见 `logs/source.diff`） |
| 工具链 | stable 1.99.1；显式 `CARGO_TARGET_DIR=<worktree>/gpui/target`；未 clean、未新建 target |
| 新二进制 | `gpui/target/release/workbuddy-proxy-gpui` sha256 `8d9cb5a1f2f48fbcc76c5045c4e77e8cad9c0fd148573407fd5b2a63bd10711e` |
| 夹具 | 隔离 CAP：25 供应商 / 25 模型 / 25 价格 / 20 行请求；仅 loopback core/catalog；未读真实配置/Key/历史 |

## 2. 图标决定（代码事实）

| 动作 / ID | 旧图形 | 新图形 | 理由 |
|---|---|---|---|
| 开机启动 `autostart` | 电源符号 | **火箭**（24×24，1.7 stroke） | 表达 launch-at-login；不再像启动/停止代理 |
| 检查更新 `check-update` | `IconName::RefreshCw` | **云 + 下箭头**（`icons/check-update.svg`） | 表达检查/下载软件更新；不再像刷新统计数据 |
| 恢复默认窗口尺寸 `restore-window-size` | 四角全屏框 | **向内对角箭头** | 表达恢复默认尺寸；不再像进入全屏 |
| 提交反馈 `submit-feedback` | 用户 + 编辑笔 | **带文字的消息气泡** | 表达反馈消息；不再像账号/编辑资料 |
| 界面语言 | globe | globe | 已直观，保留 |
| GitHub 项目主页 | GitHub mark | GitHub mark | 已直观，保留 |

- 分支顺序：`check-update` 放在通用 `check` 之前，避免 asset `contains` 匹配误中；四个新图形均早于 plus fallback。
- `ui.rs` 仅把 `check-update` 从 `named_icon_button(RefreshCw)` 换成 `icon_button(Icon::path("icons/check-update.svg"))`，并更新注释；`app_controls` 的六动作顺序/ID/回调/tooltip/aria/disabled/busy/autostart selected/toggled 原样保留。
- Codex 接手后移除了仅复述 SVG 路径的新测试；保留原有 asset 测试。图标的视觉与 tooltip 采用本轮真实运行截图核对。

## 3. 验证

| 检查 | 结果 | 证据 |
|---|---|---|
| `cargo fmt --check` | PASS | `logs/fmt-final.log` |
| asset tests | **4 passed / 0 failed** | `logs/test-assets-final.log` |
| GPUI release build | PASS | `logs/build-gpui.log` |
| 1440×900 GUI | 六图标完整，顺序/位置正常；AX 名称：开机启动、检查更新、界面语言、恢复默认窗口尺寸、GitHub 项目主页、提交反馈 | `shots/00-1440.png`、`ax/00-1440.txt` |
| 1100×700 GUI | 六图标仍在顶部右侧，无裁切/重叠 | `shots/10-1100.png`、`ax/10-1100.txt` |
| tooltip | “检查更新”“开机启动：已关闭（点击开启）”“恢复默认窗口尺寸（1440 × 900）”“提交反馈”均可见 | `shots/02-tooltip-check-update-wait.png`、`03-tooltip-autostart.png`、`03-tooltip-restore.png`、`03-tooltip-feedback.png` |
| 动作执行 | **未点击任何图标**；未开机启动、未检查更新、未打开 GitHub/反馈、未点击恢复窗口按钮 | 本轮仅 hover + 截图；窗口 1100×700 由 `axdrive resize` 设置，不是按钮回调 |

GUI 期间 config 仍 25 供应商、DB 20 行，config SHA-256 `c1be95c2…`；未写真实用户数据。截图均来自本轮新构建。

## 4. 代码事实与截图边界

- **已证**：SVG 资源分支、`ui.rs` 引用、编译/测试、1440/1100 真实窗口、AX 可访问名称、tooltip 文本。
- **未证**：图标对用户的直觉程度（主观）、Windows 字体/渲染、深色主题、缩放/Retina、键盘焦点、点击动作的真实副作用。
- 没有把“图标看起来正确”扩大为“六个动作执行通过”。

## 5. 未测 / 限制

- 仅 macOS 27.0.1 arm64、中文、非 Retina，1440×900 与 1100×700。
- 未点击 autostart/check-update/restore/GitHub/feedback；未打开外网、未改系统登录项、未执行同步/保存/删除。
- 未测 Windows、英文、Retina/DPI、深色主题、高对比度；未做像素级设计比对。
- 未提交/推送；未改其它源码、依赖、`IMPLEMENTATION_STATUS.md` 或旧证据。

## 6. 清理

- 停止 GUI/core/catalog；`fixture.py cleanup` 删除 `CAP/run`；`ps`/`lsof` 无本任务进程/CAP 引用。
- CAP 清理前 **1,184 KiB / 22 文件**；cleanup 后 **192 KiB / 6 文件**，随后删除；合计 **1,184 KiB / 22 文件**。
- 无 bytecode/复制二进制残留；父 `tmp/` 为空；证据 2.7 MB；系统盘 **195 GiB** 可用。

## 7. Codex 接手复核修订

用户改为由 Codex 直接处理后，已在 DSH 界面停止该修订轮。Codex 删除了新增的 SVG 字面量镜像测试，最终运行原有 4 项 asset 测试及 fmt 检查；`test-assets.log` 的初测 5 项仅作历史记录，由 `test-assets-final.log` 取代。只删除 `cfg(test)` 内容，产品 release 代码与上述截图构建完全一致，未重复构建或启动 GUI。

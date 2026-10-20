# T09-X03 焦点逃逸修复

**任务：** T09-X03-FOCUS-FIX-001
**分支 / HEAD：** `codex/ui-redesign` / `660c5dc06af6481a7b97b70411f66776b8940328`
**结论：** **修复在 macOS 隔离 GUI 中验证 PASS（限定 OpenAI Chat 协议弹窗、中文、非 Retina、1440×900）**。根因是协议弹窗打开时把焦点放在窗口根句柄，GPUI 的 modal focus trap 未激活，Tab 可到背景“新增供应商”；Enter 随后创建并持久化空供应商。修复后弹窗初始焦点落在弹窗内首个控件，Tab 环绕不触发背景；Tab×3+Enter（两次）、Tab×20+Enter、Escape 均未改变 25 个供应商/ config hash；Tab 到“应用到草稿”后 Space 可键盘激活。**未改协议草稿/应用校验语义**；**Windows/英文/Retina/IME 仍 NOT RUN/BLOCKED**。

## 1. 基线与范围

| 项 | 值 |
|---|---|
| HEAD / branch | `660c5dc…` / `codex/ui-redesign` |
| 基线 source diff | `git diff HEAD -- gpui/src src` 为空；status 仅既有 `IMPLEMENTATION_STATUS.md` ` M` 与未跟踪设计/规划资料 |
| 改动文件 | `gpui/src/protocol_options.rs`（+34）、`gpui/src/ui.rs`（+7/−1）；**没有其它产品文件** |
| 工具链 | rustup stable **1.99.1**；`cargo fmt/test/build --locked`；复用 `target/`、`gpui/target/`，未 `cargo clean` |
| 新二进制 | `gpui/target/release/workbuddy-proxy-gpui` sha256 `1cdc7dc5d2fe9bd312bbb67c076385ef42f5385c4e2fc7b8be0055fd749352ed` |
| 夹具 | 全新隔离 config/DB：25 供应商、25 模型、25 价格、20 行统计；core/UI 仅 127.0.0.1；未读真实配置/密钥/历史 |

## 2. 根因与修复

**根因（代码）**：`ui.rs` 打开弹窗时对协议弹窗选择 `self.focus`（窗口根焦点句柄）。`gpui-base::Dialog` 的 focus trap 只在焦点**位于对话框容器内**时生效；根句柄在弹窗外，`active_focus_trap` 不命中，Tab 使用普通遍历走到背景“新增供应商”，Enter 触发它并持久化空供应商。

**修复（最小）**：
- `protocol_options.rs` 新增 `Editor::initial_focus(cx)`：
  - Anthropic：弹窗内“默认最大输出 Token”输入；
  - OpenAI Chat/Responses、Gemini：弹窗内“思考模式”选择。
- `ui.rs` 的弹窗打开路径在 `protocol_open` 时优先使用该句柄；picker/edit 仍用原有搜索/编辑句柄。
- 新增纯映射测试 `modal_initial_focus_target_matches_rendered_protocol_controls`（Anthropic→limit，其余→mode），防止以后改回窗口根焦点。
- 未改协议 `read()`/`confirm_protocol`/草稿写入/应用校验语义。

## 3. 验证结果

### 静态/构建

| 检查 | 结果 | 证据 |
|---|---|---|
| `cargo fmt --check` | PASS（exit 0） | `logs/fmt.log` |
| 针对性测试 | 2 passed | `logs/test-protocol.log` |
| 全量测试 | **139 passed，0 failed** | `logs/test-all.log` |
| release build | PASS | `logs/build-gpui.log` |
| 新二进制 hash | `1cdc7dc5…` | `logs/binary-new.txt` |

### GUI 键盘序列（隔离夹具）

配置基线：`config.json` sha256 `a424698d2b6f56467adce07a8331476efa9aebe038d1ae027a93cc2ea45d59ef`；`gpui-drafts.json` `d5549b31…`；25 供应商。

| 序列 | 结果 | 证据 |
|---|---|---|
| 打开协议弹窗 → Tab×1 | 焦点在弹窗内 `minimal` 映射输入（可见文本光标），弹窗仍开 | `shots/06-tab-1.png` |
| Tab×3 → Enter（第 1 次） | 弹窗关闭（既有 apply/confirm）；config hash **不变**；providers **25**；无空供应商；draft hash 不变 | `shots/02-tab3.png`、`03-after-enter-run1.png`、`ax/03-after-enter-run1.txt` |
| 重复 Tab×3 → Enter（第 2 次） | 同上；config hash 不变；providers 25 | `ax/04-after-enter-run2.txt`、`shots/04-after-enter-run2.png` |
| 打开 → Escape | 弹窗关闭；config hash 不变；无背景动作 | `ax/05-dialog-reopen.txt`、`ax/06-after-escape.txt` |
| 打开 → Tab×20 → Enter | 焦点环绕仍留在弹窗（无背景动作）；config 不变；providers 25 | `shots/06-tab-10.png`、`06-tab-20.png`、`ax/07-tabwrap-enter.txt` |
| 打开 → Tab×7（“应用到草稿”获得焦点）→ Space | 弹窗关闭；config 不变；drafts 不变；providers 25 | `shots/tab7-space.png`、`ax/tab7-space.txt` |
| 结果汇总 | `logs/gui-results.json`：所有序列 `empty_provider_created=false`、config hash 一致 | `logs/gui-results.json` |

**关于 Enter 语义**：本弹窗的 Enter 仍执行既有“应用到草稿/确认”动作（C05 的 Enter 校验语义），并非激活当前文本控件；关键修复是它不再触达背景。若验收要求 Enter 在输入框上完全无动作，需要产品另行决定，本轮未改变既有应用语义。

### 焦点可见性限制

- macOS AX 的 `AXFocusedUIElement`/递归 `AXFocused` 在本 GPUI 构建中只返回 `AXWindow`，不暴露弹窗内具体控件；因此焦点证据采用：
  - `shots/06-tab-1.png` 的 `minimal` 输入文本光标；
  - Tab/Enter/Escape 行为与 config/providers/drafts 前后哈希；
  - 背景没有新增供应商。
- 未把 AX 无法读取焦点误报为“焦点已验证”；这是可读性限制，不是行为失败。

## 4. 未测 / 限制

- 仅 OpenAI Chat 协议弹窗的 GUI 序列；Anthropic 的 `initial_focus` 仅由单元映射测试覆盖，未逐协议 GUI 重跑。
- Windows、英文、Retina/DPI、真实 IME、Shift+Tab 反向环绕、鼠标焦点恢复未测。
- 未在所有其它 Dialog（模型选择/编辑/同步确认）上重跑键盘回归；它们仍使用原有焦点路径。
- 仅本轮合成夹具；未执行真实保存/同步/上游请求。
- 未提交/推送；未改 `IMPLEMENTATION_STATUS.md` 或既有证据。

## 5. 清理

- 停止 GPUI pid 81322、core 81250、mock 81249；`ps`/`lsof` 无本任务进程/CAP 引用。
- CAP 清理前 **1,304 KiB / 26 文件**；`fixture.py cleanup` 删除 `run` 后剩 **312 KiB / 10 文件**，随后删除 CAP；合计删除 **1,304 KiB / 26 文件**。
- 父 `.../tmp` 保留且为空；未按前缀/glob 清理；未触碰其它缓存、`target/`、`gpui/target/`、旧证据。
- 证据保留 **3.8 MB**；系统盘 **195 GiB** 可用；无其它意外删除。

## 6. 附件

`fixture.py`、`mock_catalog.py`、`logs/`（命令、fmt/test/build、config/drafts、gui-results、cleanup）、`shots/`、`ax/`、`logs/source.diff`。

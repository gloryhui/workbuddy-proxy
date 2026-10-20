# Codex 独立复核：T09-X03-FOCUS-FIX-001

## 结论

**PASS（限 macOS 中文、非 Retina、1440×900 的 OpenAI Chat 协议弹窗）**。修复把初始焦点放入协议弹窗，使模态焦点陷阱生效；隔离 GUI 中重复的 Tab×3+Enter、Tab×20+Enter、Escape 均未触发背景供应商创建，合成 config 保持 25 个供应商且哈希不变。T09 整体仍受 Windows/语言/DPI 等矩阵缺口限制。

## 代码复核

- 基线 `660c5dc06af6481a7b97b70411f66776b8940328`，产品修改仅 `gpui/src/protocol_options.rs` 与 `gpui/src/ui.rs`；`git diff --check` 通过，根 `src/` 无改动。
- `Proxy::manage_dialog` 在 `protocol_open` 时调用 `Editor::initial_focus`，该句柄来自实际渲染的协议控件：Anthropic 的输出限制输入框，其他协议的思考模式选择。首次打开时将焦点转入模态容器，解决原窗口根句柄导致 focus trap 未激活的问题。picker/edit 原有焦点分支未改。
- `initial_focus_uses_limit` 映射与各协议实际渲染字段一致；新增单元测试覆盖 Anthropic、OpenAI Chat、OpenAI Responses、Gemini 四类。
- 未改 `Editor::read`、`confirm_protocol`、草稿持久化或 dialog 校验/确认逻辑。Enter 仍按 GPUI Kit 当前 Dialog 语义执行弹窗内确认；复核判定标准是它不再落到背景控件。若将来要求文本输入中 Enter 不触发确认，应作为独立键盘交互决策处理。

## 验证复核

- 报告中的 `cargo fmt --check`、针对性测试 2 项、全量测试 139 项和 release build 日志均存在；新二进制 SHA-256 `1cdc7dc5d2fe9bd312bbb67c076385ef42f5385c4e2fc7b8be0055fd749352ed` 与本机 `gpui/target/release/workbuddy-proxy-gpui` 实测一致。
- 目视检查 `01-dialog-open.png`、`06-tab-1.png`、`03-after-enter-run1.png`：协议弹窗可见；Tab 后 `minimal` 输入框有插入光标；Enter 后弹窗关闭且背景供应商列表未出现空项。GUI 结果 JSON 显示 5 个键盘序列 `empty_provider_created=false`、providers 始终 25、config SHA-256 未变化；Escape 路径也未写入。
- `AXFocusedUIElement` 只返回 `AXWindow`，无法读取 GPUI 内部具体焦点；报告如实将焦点证据限定为可见插入光标和行为/数据前后对照，没有夸大 AX 能力。
- CAP `/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t09-x03-focus-fix-001` 已删除，父任务 `tmp/` 为空；清理记录为 1,304 KiB / 26 文件。产品构建缓存和既有证据均保留，系统盘约 195 GiB 可用。

## 未覆盖

仅 OpenAI Chat 做 GUI 焦点序列；Anthropic 初始焦点仅有映射单元测试。Windows、英文、Retina/DPI、真实 IME、Shift+Tab、其它 Dialog 的键盘回归未测。未运行第二种平台验证；不得外推为跨平台 PASS。

# T09 macOS 补充验收（SC02/03/04—07/11/12、X03、X05）

**任务：** T09-MAC-SUPPLEMENT-001
**分支 / HEAD：** `codex/ui-redesign` / `afba1fbedbbb72e1a2536225eefb6899b29041dd`
**结论：** **macOS 补充 PARTIAL；T09 跨平台整体仍 BLOCKED**（无 Windows x64 环境）。SC02、SC03（本地 mock）、SC04、SC05、SC06、SC07、SC11、SC12 在真实窗口取得截图/AX；X05 有限重复稳定。**X03 的 Tab/Enter 失败并产生未预期写入**：在协议选项弹窗按 Tab×3 + Enter 后，隔离 config 被新增一个空供应商 `ad9cfca47922`（名称“供应商”、URL 为空）并保存，config SHA-256 由 `57a174b0…` 变为 `c42ecf9a…`；发现后立即停机并保留现场证据，未修产品。**Windows BLOCKED；英文/Retina/IME NOT RUN。**

## 1. 环境与基线

| 项 | 值 |
|---|---|
| 主机 | macOS 27.0.1（26A434），Apple M1，arm64，非 Retina（scale=1.0） |
| HEAD | `afba1fbedbbb72e1a2536225eefb6899b29041dd`；`git diff HEAD -- gpui/src src` 为空 |
| GPUI | `gpui/target/release/workbuddy-proxy-gpui` sha256 `89049d01990643792cb9445825cd838f88986f4f0b81db284953fba309534ed0`（既有构建；未构建） |
| core | `target/release/workbuddy-proxy` sha256 `22cacc663af1293252611001e589b6bc49cde885234e42a5e6a200b53697d4ca` |
| 隔离 | 一次性 config/DB；core/UI 仅 127.0.0.1；25 个合成供应商、25 个模型、25 条价格、20 行统计 |
| 上游 | SC03 使用本地 `127.0.0.1` mock_provider（虚构 Key `sk-test-local`）；其余画面不发起上游请求 |

## 2. 画面结果（macOS 中文，NEW）

| 画面 | 结果 | 证据 |
|---|---|---|
| SC02 供应商管理 | **PASS**：opencode 连接设置、API Key、上游协议、模型列表、全局设置与日志可见 | `shots/sc02-provider-opencode.png`、`ax/sc02-provider.txt` |
| SC03 选择模型 | **PASS（本地 mock）**：点击“获取模型”后本地 mock 返回 `mock-model-a/b`，弹窗显示“选择模型/已选 1/2·0 待添加/确认添加(0)”；未添加、未保存 | `shots/sc03-model-picker.png`、`ax/sc03-picker2.txt` |
| SC04 上游协议下拉 | **PASS**：四种协议（OpenAI Chat Completions、OpenAI Responses、Anthropic Messages、Gemini GenerateContent）与选中项可见 | `shots/sc04-protocol-menu.png` |
| SC05 OpenAI Chat | **PASS**：协议选项弹窗（思考模式/思考强度映射） | `shots/sc05-protocol-openai-chat.png` |
| SC05 OpenAI Responses（含 `none`） | **PASS**：`none` 映射行可见 | `shots/sc05-protocol-openai-responses.png` |
| SC06 Anthropic | **PASS**：默认最大输出 Token、Anthropic-Version、思考模式 | `shots/sc06-protocol-anthropic.png` |
| SC07 Gemini | **PASS**：Gemini GenerateContent、思考模式/档位 | `shots/sc07-protocol-gemini.png` |
| SC11 编辑模型 | **PASS**：显示名/模型 ID、自动识别能力启用、能力字段禁用、上下文/输出限制 | `shots/sc11-edit-model.png`、`ax/sc11-edit-model.txt` |
| SC12 同步 WorkBuddy 确认 | **PASS（仅 macOS）**：三个动作“同步并重启/仅同步/取消”与说明可见；未执行任何同步 | `shots/sc12-sync-confirm.png`、`ax/sc12-sync-confirm.txt` |
| X03 Tab/Enter/Escape | **FAIL（Tab/Enter）；Escape PASS** | 见 §3 |
| X05 有限重复 | **PARTIAL**：模型选择弹窗 3 次、协议菜单 3 次、同步确认 2 次打开/关闭；进程 RSS 136 MB、无崩溃；无性能基线 | `logs/x05-repeat.log`、`shots/x05-picker-repeat.png` |

### 未测 / 不可用

- Windows x64：**BLOCKED**（无实机/VM/远程/Wine）；SC12 的 Windows 平台限制说明未实测。
- 英文、Retina、DPI、真实 IME 候选：**NOT RUN**（无环境）。
- SC02 的保存/同步/测试连接/清空日志未执行（副作用约束）；仅展示。

## 3. X03 失败与未预期写入（已停机）

1. 打开同步确认框，按 Tab×3，截图 `shots/x03-tab-sync.png`；Escape 关闭。
2. 打开协议选项，按 Tab×3，截图 `shots/x03-tab-protocol.png`；按 Enter。
3. Enter 后弹窗仍显示（Close/应用到草稿仍在），但 **config.json 变化**：
   - before `57a174b0b68d1e9a31a159735b3833a350a9703607a8557cafd93e424529c1fc`
   - after `c42ecf9ae7b27875addefd5f26c1ed90175c6f043c69235eb1b0c1d1be17cb8c`
   - `providers` 由 25 增至 **26**；新增 `id=ad9cfca47922, name=供应商, upstream=''`；`gpui-drafts.selected=ad9cfca47922`。
4. 这是**隔离 CAP 内的合成 config**，未触碰真实用户配置/密钥/历史；但属于未预期写入与焦点逃逸（Enter 触发背景“新增供应商”并保存）。发现后立即停止 GUI/core/mock，保留 `logs/config-after-x03.json`、`logs/gpui-drafts-after-x03.json`、`logs/run-after-x03.json` 与 AX/截图，未修产品。

结论：X03 **FAIL**；Tab 焦点未限制在弹窗内，Enter 可触发弹窗外的“新增供应商”并持久化空供应商。需 Codex 复核并按产品缺陷处理。

## 4. 证据与命令

- 脚本/命令：`fixture.py`（适配 CAP）、`mock_provider.py`（仅 127.0.0.1）、`logs/commands.md`。
- 截图：`shots/`（SC02—SC12、X03、X05；重点 `sc05-protocol-openai-chat.png`、`sc05-protocol-openai-responses.png`、`sc06-protocol-anthropic.png`、`sc07-protocol-gemini.png`）。
- AX：`ax/`（`sc02-provider.txt`、`sc03-picker2.txt`、`sc05-protocol-openai-responses.txt`、`sc06-protocol-anthropic.txt`、`sc07-protocol-gemini.txt`、`sc11-edit-model.txt`、`sc12-sync-confirm.txt`、`x03-*`、`x05-*`）。
- OCR：`ocr/`；`logs/fixture-setup.log`、`logs/x05-repeat.log`、`logs/x03.log`、`logs/x03-unexpected-write.log`。
- 说明：SC03 的 mock 请求日志原在 CAP/logs，清理 CAP 时随目录删除；成功请求由 picker 的 `mock-model-a/b` 与 `已获取 2 个模型` 截图/AX 证明。

## 5. 清理

- 停止 GPUI、core、mock catalog；`fixture.py cleanup` 删除 `CAP/run`；随后删除 CAP。`mock_provider.py` 在 CAP 删除后仍存活，已单独 SIGTERM 并确认退出（顺序问题，已记录）。
- cleanup 后 CAP 剩余 **204 KiB / 9 文件**（bin、工具源码、tmp、pid）已删除；`run` 大小由 fixture cleanup 删除、未单独测量；总删除量未精确记录（流程缺口）。
- 父 `.../tmp` 保留且为空；未按前缀/globbing 清理，未触碰其它缓存、`target/`、`gpui/target/`、既有证据或 L05 路径。
- 证据保留 **4,193,611 字节（71 个文件）**；系统盘 **195 GiB** 可用。
- 本轮**没有其它意外删除**；唯一未预期写入是上述隔离合成 config（X03）。

## 6. 附

`fixture.py`、`mock_provider.py`、`shots/`、`ax/`、`ocr/`、`logs/`。

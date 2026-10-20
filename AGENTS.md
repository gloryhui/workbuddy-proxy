# Codex 协作规范

## Git 共同作者署名

- Codex 使用 GPT-6、GPT-6.1 或其他 GPT 模型实质参与本次提交的代码、测试、开发文档等工作时，统一使用 Codex 共同作者身份，不使用具体模型名称。
- 保持现有 Conventional Commits 等提交规范。正文结束后留一个空行，在末尾 trailer 区块添加以下精确行，且只保留一次：

```text
Co-authored-by: Codex <codex@openai.com>
```

- 提交前检查完整消息：已有署名时不重复追加，重复署名应去重。确认属于 Codex 的旧邮箱或模型名署名应统一为上述行；保留其他真实共同作者及 `Signed-off-by` 等合法 trailers，将 Codex 行放在 trailer 区块末尾，trailer 行之间不插空行。
- Author 和 Committer 保留当前有效 Git 用户身份；合法保留原作者的操作继续保留原有身份。不得为 AI 署名修改 `user.name`、`user.email`，或通过 `--author`、身份环境变量替换为 Codex。
- 纯人工开发、仅使用其他 AI 工具开发，或 Codex 仅执行提交而未实质参与提交内容时，不自动添加 Codex 署名；不得将其他工具伪装成 Codex。无法确认参与来源时先核实。
- 规则适用于 Codex 生成、编辑或执行的提交信息，包括命令行、编辑器、工具及 API 路径。人工之后自行提交 Codex 参与的改动时，应使用带该署名的信息；本文件不会自动拦截人工终端、其他 IDE、GitHub 网页或 CI 提交。
- 不得仅为补署名而 amend、rebase 或重写已有历史；不得执行 `git push --force` 或创建无意义的测试提交。不安装或改写 Git Hooks、全局提交模板，不绕过提交签名、Hooks 或 CI。推送须遵循用户授权。
- 可使用 `git interpret-trailers --parse` 离线验证消息。GitHub 依据提交消息中的 trailer 识别共同作者；提交需推送后才在 GitHub 可见。账号关联取决于该邮箱与 GitHub 账号的绑定，Contributions 和 Contributors 还受 GitHub 的统计条件限制，添加本文件本身不保证显示。

## 构建缓存、临时目录与清理

- 构建和临时目录必须兼容 Windows、macOS、Linux 及 CI，不得要求固定盘符、卷名、用户名或机器专属绝对路径。特定机器的磁盘与路径偏好应通过本地环境变量或会话指令指定，不得作为共享项目的全局限制。
- 文档中的操作命令使用仓库相对路径或明确标注的本地配置变量；worktree 位置通过 `git worktree list --porcelain` 核实，不以某台机器的目录作为唯一入口。带日期的历史日志、报告和归档脚本中的路径仅用于追溯，不能当作当前机器的配置、门禁或执行要求；复跑前应核对版本、平台及实际路径。
- 优先复用当前环境兼容的 Cargo 编译缓存，例如仓库内已有的 `target/`、`gpui/target/` 或本地 `CARGO_TARGET_DIR` 指定的目录。不得每轮创建新的 Cargo 编译目录，不得每次验证都执行 `cargo clean`；worktree 可显式配置缓存复用，但应避免不兼容的平台、工具链或构建配置相互影响。
- 构建、测试、打包默认使用操作系统提供的临时目录，并在其中创建本会话专用的子目录。如用户指定其他磁盘或目录，先确认路径存在、可写且空间充足，再按所用平台和工具设置临时目录环境变量（如 Unix 的 `TMPDIR`、Windows 的 `TMP` 和 `TEMP`）；需要跨工具一致时同步设置相关变量。Python 应在导入或使用 `tempfile` 前设置这些环境变量；已经缓存临时路径的进程应在当前测试安全结束后重新启动，不能假定修改父进程环境会迁移其临时文件。
- 每轮验证结束，先停止并等待本轮子进程全部退出，再清理本轮生成的临时数据库、配置、复制的可执行文件和临时构建目录。使用所用语言或 Shell 支持的清理机制（如 Python、PowerShell 的 `try/finally` 或 POSIX Shell 的 `trap`），确保失败和可捕获的中断也执行同样的退出、等待和清理流程；保留测试报告、必要证据及交付产物。
- 只删除本会话创建、创建归属明确且确认不再使用的确切目录或文件。清理前记录路径及归属证据，检查进程与引用；不得按 `wbp-*`、`workbuddy-*`、`gpui-*` 等前缀直接批量删除，不得影响其他会话、用户数据或在用构建缓存。
- 当前测试安全结束后，核查系统临时目录中的项目遗留文件，仅清理已确认由本会话创建且闲置的部分。归属不明确或仍被引用的内容应保留并说明；不得为清理而重置代码或删除必要报告、交付产物。
- 每轮完成后报告清理了哪些临时文件、释放空间、仍保留的临时文件大小，以及系统盘当前剩余空间。

## Windows 截图优先方式

- 用户已指定：Windows 原生应用截图优先使用 **GDI / Pillow `ImageGrab`**。本项目已于 2026-10-21 实测成功，不要每次重新从此前反复超时的 Windows Graphics Capture 通道开始。
- 使用 [`scripts/windows_screenshot.py`](scripts/windows_screenshot.py)，按当前目标进程 PID 选取唯一可见窗口，启用 DPI 感知，用 DWM 原生窗口边界裁取 PNG；具体流程见 [`docs/WINDOWS-SCREENSHOTS.md`](docs/WINDOWS-SCREENSHOTS.md)。Python / Pillow 的位置通过当前环境发现，不写死盘符或用户路径。
- GDI 抓取的是屏幕上的可见像素。目标窗口需位于已解锁的本地交互桌面、保持前台且无遮挡；不能依赖它抓取最小化、被遮挡或离屏的窗口。多屏使用 `all_screens=True` 和实际物理坐标，无需预先关闭副屏。
- README 等公开图片使用隔离演示配置与合成记录，检查每张截图的实际内容、尺寸和隐私，再替换文档资源；截图失败不得把旧图或重绘图冒充新实拍。
- 若其他截图通道报 `FrameArrived timed out` / `window capture timed out`，记录错误并优先切回上述已验证的 GDI 方式；不要在环境没有变化时反复重启、重复相同调用，或直接归因于多显示器。

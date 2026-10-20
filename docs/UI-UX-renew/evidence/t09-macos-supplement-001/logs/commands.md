# T09-MAC-SUPPLEMENT-001 命令记录
- HEAD `afba1fb...`；`git diff HEAD -- gpui/src src` 空；GPUI `89049d…`、core `22cacc6…`。
- CAP `.../tmp/t09-macos-supplement-001`；fixture EV/CAP 精确适配；`RUN=CAP/run`；未碰旧 CAP/父目录。
- `fixture.py setup` → 25 providers / 25 models / 25 prices / 20 rows；core 71951、mock 71947。
- GUI `workbuddy-proxy-gpui --config-dir CAP/run/config`；HOME=CAP/run/home，TMP=CAP/tmp；resize 1440×900。
- SC02：点击侧栏 opencode；SC04：点击上游协议；SC05-07：选择协议→协议选项；SC11：编辑模型；SC12：同步 WorkBuddy。
- SC03：关闭 GUI/core；把 provider upstream 改为 `http://127.0.0.1:<mock>/v1`、key `sk-test-local`；启动 `mock_provider.py` 与 core；点击获取模型。
- X03：同步框 Tab×3；协议选项 Tab×3 + Enter → 发现 config 新增空供应商，立即停机。
- X05：模型选择 3 次、协议菜单 3 次、同步确认 2 次打开/关闭；RSS 136 MB。
- 清理：停止进程；`fixture.py cleanup`；CAP 剩 204 KiB/9 文件后删除；父 tmp 空保留。

## 画面映射（ACCEPTANCE 7.10 / T09 矩阵）
- SC05 = OpenAI（Chat 与 Responses；Responses 含 `none`）
- SC06 = Anthropic
- SC07 = Gemini
- 对应证据：`sc05-protocol-openai-chat.*`、`sc05-protocol-openai-responses.*`、`sc06-protocol-anthropic.*`、`sc07-protocol-gemini.*`。

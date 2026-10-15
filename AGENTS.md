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

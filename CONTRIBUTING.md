# 贡献指南

欢迎提交可复现的问题、文档修正和范围清晰的 PR。项目由个人业余维护，提交、标签、讨论或确认收到不代表承诺实现、合并或按期修复；维护者可基于范围、兼容性、维护成本和可用时间延期或不采纳建议。详见[维护与支持说明](README.md#维护与支持说明)和[支持规则](SUPPORT.md)。

## 提交 Issue 前

1. 阅读 README 的[常见问题](README.md#常见问题)和相关功能说明，[搜索已有 Issue](https://github.com/gloryhui/workbuddy-proxy/issues?q=is%3Aissue)，包括已关闭的问题，避免重复。
2. 问题反馈使用 Bug 表单，提供 Release / commit、操作系统版本与架构；涉及 WorkBuddy 时填写其版本。仅在相关时补充供应商、协议、脱敏 URL 类型 / 模型 ID 和错误状态或类别。
3. 提供最小复现步骤、预期与实际结果、发生频率及影响范围；优先使用隔离配置、假 Key、本机模拟上游和合成数据，不要求公网服务、商业账号或真实密钥。无法复现时说明已知条件与排查结果。
4. 功能建议使用功能请求模板，说明用户场景、当前痛点、期望效果和替代方案。普通使用问题可按[支持指南](SUPPORT.md)提交。

## 脱敏与安全

**禁止上传 API Key、Authorization、访问令牌、真实对话内容、个人隐私、完整 `config.json`、`models.json`、`gpui-drafts.json` 或私密数据库（含 WAL / SHM）及备份。** 提交前检查正文、日志、截图、附件和 diff；只保留最小必要片段，用假值替代凭据、私人路径、账号及可识别信息。

疑似安全漏洞先阅读 [SECURITY.md](SECURITY.md)，确认私密渠道可用后再发送详情，不通过公开 Issue、PR 或评论披露利用步骤。

## 提交 PR 前

- 阅读根目录 [AGENTS.md](AGENTS.md)，遵循现有提交与 AI 协作共同作者署名、跨平台路径、临时目录和清理规范；保留正常的 Git Author / Committer 身份。
- 一个 PR 聚焦一个问题，说明触发条件、修改前后行为、关联 Issue、影响范围和已知限制，避免夹带无关重构或格式化。
- 遵循已有 Rust / GPUI 结构与代码风格；文档尽量使用简体中文并保持现有风格，公共命令使用仓库相对路径或明确的本地环境变量。
- 按修改范围更新文档和必要测试，记录实际执行的命令、结果及未验证的平台或场景，不把模拟上游结果当作真实账号或目标平台验收。

代码变更从仓库根目录运行对应的格式与相关测试检查，完整构建 / 平台验证按 [README 开发说明](README.md#开发与协议范围)及现有 CI 要求执行。例如：

```sh
cargo fmt --check
cargo test --locked
# 涉及 GPUI 时
cargo fmt --manifest-path gpui/Cargo.toml --check
cargo test --manifest-path gpui/Cargo.toml --locked
```

纯文档或 Issue 模板变更无需整套 Cargo 重编译；检查 Markdown、相对链接、YAML 及 GitHub Issue Form 字段结构即可。不要为此改动 CI、运行时配置或许可证。提交前检查 diff 和暂存范围，保留无关本地工作。

参与者和维护者均应遵守[社区行为准则](CODE_OF_CONDUCT.md)。本指南说明协作流程，不修改 [MIT LICENSE](LICENSE)，不为软件使用或商业使用增加条件。

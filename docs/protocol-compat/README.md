# 上游协议 · v0.6.1

WorkBuddy 使用本地 OpenAI Chat Completions 入口。供应商显式选择 Chat Completions、OpenAI Responses、Anthropic Messages 或 Gemini GenerateContent；原有 Chat 路径保留字节透传，其他三种协议通过原生适配器转换。

- [配置、思考选项、资源限制与验收边界](USAGE.md)
- [请求、响应与流式转换契约](CONTRACTS.md)
- [协议依据与调研日期](SOURCES.md)

实现位于 [src/protocol](../../src/protocol/)。文档中的官方资料是调研依据，不能自动扩展为供应商所有新接口或特性的兼容承诺；修改适配器前须重新核对相关官方协议。

旧施工任务、分支协作说明和阶段验收见 [固定版本快照](https://github.com/gloryhui/workbuddy-proxy/tree/v0.6.1/docs/protocol-compat)。当前工作区以实际 Git 状态和 [开发规范](../CODEX_FAST_WORKFLOW.md) 为准。

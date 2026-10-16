# 供应商地址预设

本地静态目录：[`assets/provider-presets.json`](../assets/provider-presets.json)。目前包含 78 个预设：原有 76 个 OpenAI Chat 兼容基地址、1 个 Anthropic 官方 Messages 基地址、1 个需要用户填写资源地址的 Azure OpenAI 入口。地区和套餐展开为独立选项，支持搜索，运行时不请求远程预设服务。

来源：[yetone/magpie 的 internal/provider/presets.go](https://github.com/yetone/magpie/blob/b1d02fa612a1462dbc5e5f08ae8c74271ee55585/internal/provider/presets.go)，提交 `b1d02fa612a1462dbc5e5f08ae8c74271ee55585`，整理日期 2026-10-15。保留其地区、套餐路径和 Bedrock 地区展开规则；中文显示名由本项目整理。上游 MIT 许可见 [magpie-LICENSE.txt](third-party/magpie-LICENSE.txt)。

选择预设填写当前表单的供应商名称、API 地址、上游协议与 API Key 认证方式，并恢复协议选项默认值；不自动请求上游、不自动保存，不修改已有 Key、模型或路由标识。仍可编辑名称和地址，再按原流程获取／添加模型并保存。现有自定义地址会显示“自定义地址”。

收录范围限定为 API 地址 + API Key。旧有 76 条继续使用 Chat 与 Bearer；新增 Anthropic 使用 Messages 与 `x-api-key`，Azure 使用 Chat 或 Responses 与 `api-key`。不增加 OAuth、订阅账号登录、Vertex 凭据刷新、决策路由或远端 Magpie 专用对接。

预设是地址快捷填写，不代表账号权限或真实接口验收。部分套餐／Bedrock 没有通用 `/models` 列表，需要手动填写模型 ID；本地无鉴权服务按现有约定可填写 `local` 占位 Key。实际可用模型、套餐 Key 和地区权限由对应供应商决定。

更新预设时保留完整 API 路径，例如 `/api/coding/paas/v4`、`/api/coding/v3`、`/compatible-mode/v1`，不可统一替换为 `/v1`。

## Anthropic 与 Azure

新增条目对照 [Magpie 预设](https://github.com/yetone/magpie/blob/e72b6a3327e1ef4537c02f73d8b6c53dc890795b/internal/provider/presets.go)。请求契约参见 [Anthropic API](https://platform.claude.com/docs/en/api/overview) 与 [Azure Responses v1](https://learn.microsoft.com/en-us/rest/api/aifoundry/azureopenai/responses)。

- Anthropic：`https://api.anthropic.com/v1`，Messages 协议，使用 `x-api-key` 与 `anthropic-version: 2023-06-01`。
- Azure：选择预设不会填入一个不可用的示例地址，也不会沿用其他供应商的地址；请填写自己的资源地址，如 `https://my-resource.openai.azure.com`，请求时统一使用 `/openai/v1`。已填写 Azure 地址后再次选择 Azure 可保留地址。显式网关前缀保留；旧 `/deployments/...` 推理地址和带查询参数的地址会提示改用资源地址。
- Azure 默认使用 Chat；可切换 Responses，`api-key` 认证保留。切换到其他供应商预设恢复对应协议和认证。协议选项中可显式选择 Azure API Key；不会根据域名擅自切换认证。
- Azure 模型 ID 使用部署名称。模型发现只使用同一资源的 `/openai/deployments?api-version=2022-12-01`；该接口不可用或返回空列表时，提示手动填写部署名称，不回退到可能返回基础模型 ID 的 `/openai/v1/models`。401、限流、超时和非法响应保持原错误，不自动重试；发现失败不修改已保存的供应商和模型，手动填写后仍可测试与转发。
- `protocol_settings.authentication` 默认 `protocol_default`，Azure 为 `azure_api_key`；旧配置缺省时保留原认证行为。保存、草稿恢复、复制、模型发现、连接测试与 JSON/SSE 转发使用同一设置。管理协议升级到 10，避免新界面向旧核心发送其不支持的设置，并与自动更新开发分支使用的协议 9 区分。

离线回归：`python3 scripts/api_key_providers_test.py --exe target/debug/workbuddy-proxy`。使用临时配置目录、假 Key 与 loopback Mock，不证明真实账号、部署权限或实际账单可用。

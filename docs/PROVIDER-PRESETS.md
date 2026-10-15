# 供应商地址预设

本地静态目录：[`assets/provider-presets.json`](../assets/provider-presets.json)。目前包含 76 个 OpenAI Chat 兼容 API 基地址，将地区和套餐展开为独立选项，支持搜索，运行时不请求远程预设服务。

来源：[yetone/magpie 的 internal/provider/presets.go](https://github.com/yetone/magpie/blob/b1d02fa612a1462dbc5e5f08ae8c74271ee55585/internal/provider/presets.go)，提交 `b1d02fa612a1462dbc5e5f08ae8c74271ee55585`，整理日期 2026-10-15。保留其地区、套餐路径和 Bedrock 地区展开规则；中文显示名由本项目整理。上游 MIT 许可见 [magpie-LICENSE.txt](third-party/magpie-LICENSE.txt)。

选择预设只填写当前表单的供应商名称和 API 地址，不自动请求上游、不自动保存，不修改已有 Key、模型或路由标识。仍可编辑名称和地址，再按原流程获取／添加模型并保存。现有自定义地址会显示“自定义地址”。

只收入有固定 Chat Completions 基地址、可按当前代理的 Bearer Key 请求方式使用的条目。Anthropic-only、Google Vertex OAuth、Azure 资源专用地址、远端 Magpie 地址及需要用户工作区 ID 的决策接口保留为自定义配置；本次不增加协议转换或 OAuth。

预设是地址快捷填写，不代表账号权限或真实接口验收。部分套餐／Bedrock 没有通用 `/models` 列表，需要手动填写模型 ID；本地无鉴权服务按现有约定可填写 `local` 占位 Key。实际可用模型、套餐 Key 和地区权限由对应供应商决定。

更新预设时保留完整 API 路径，例如 `/api/coding/paas/v4`、`/api/coding/v3`、`/compatible-mode/v1`，不可统一替换为 `/v1`。

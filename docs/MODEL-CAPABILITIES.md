# 模型能力与多模态转发（开发版）

模型表格双击一行，或选中后按 Enter / F2，可编辑该模型的 WorkBuddy 能力声明。默认使用模型默认能力；取消“使用模型默认能力”后，可以独立设置：

| 设置 | WorkBuddy 配置 |
| --- | --- |
| 工具调用、图片输入 | `supportsToolCall`、`supportsImages` |
| 思考模式、仅思考模式 | `supportsReasoning`、`onlyReasoning` |
| 允许关闭思考、默认档位、支持档位 | `reasoning.canDisableThinking`、`defaultEffort`、`supportedEfforts` |
| 输入／输出上限 | `maxInputTokens`、`maxOutputTokens`；留空时不写入限制 |
| 默认／可选上下文长度 | `contextWindow.defaultLength`、`supportedLengths` |
| 自定义协议 | `useCustomProtocol`；同步完整的 `/chat/completions` 地址 |

编辑确认后先保存，再同步 WorkBuddy。能力保存在代理配置和私有草稿中，复制供应商、重命名模型、再次同步时保留。旧客户端保存没有能力字段的模型行时也保留现有设置；明确恢复自动模式后才移除覆盖。设置无效时保留原配置，显示错误。

图片输入声明用于 WorkBuddy 的附件和读图工具。DeepSeek V4.1 Flash（`deepseek-v4.1-flash`、`deepseek-flash`）默认开启图片输入；旧代理配置缺少能力字段时也使用这个默认值。其余未知模型保持保守默认，可按供应商的实际能力手动开启。不会给所有模型统一开启图片或思考。

默认值依据：DeepSeek 的[模型卡](https://huggingface.co/deepseek-ai/DeepSeek-V4.1-Flash)声明原生图片／文本输入；OpenCode 的 [Go 模型定义](https://github.com/anomalyco/models.dev/blob/dev/providers/opencode-go/models/deepseek-v4.1-flash.toml)继承[基础模型能力](https://github.com/anomalyco/models.dev/blob/dev/models/deepseek/deepseek-v4.1-flash.toml)，输入为 `text`、`image`。已有 DeepSeek 上下文选项保持 128K／300K／1M、默认 300K；最大输入声明为 1M。

这些声明控制 WorkBuddy 的使用方式，不会对请求中的媒体内容做过滤、改写或协议转换。供应商级“思考强度”仍是转发时的参数覆盖；模型能力中的思考选项用于 WorkBuddy 的选择器，两者用途不同。希望 WorkBuddy 自行选择档位时，供应商级设置应使用“跟随 WorkBuddy”。

## 请求与响应边界

- 入口为 OpenAI 兼容 Chat Completions。`messages` 中的文字、图片 URL、Base64 图片，以及上游接受的音频／视频 JSON 字段和工具内容均保留。
- 默认只替换模型别名，其他请求字节保持原样。启用思考覆盖时仍保留媒体和工具字段的原始 JSON 内容。
- SSE 与非流式响应按字节转发，不截取或解释媒体输出。
- 不提供独立的视频生成、文件上传、音频转录等接口，不替 WorkBuddy 拆帧、转码或下载媒体。当前 WorkBuddy 没有独立的音频／视频能力开关；能否发送及识别这些内容，由客户端格式和上游支持决定。
- 请求体上限仍为 **8 MiB**，包含 Base64 膨胀后的媒体、全部历史消息和其他字段；超过上限返回 413。代理不扩充上游权限或模型能力。
- “自定义协议”在当前 WorkBuddy 中表示直接使用完整请求 URL。本代理仍只接受 Chat Completions；开启后同步完整的本地 Chat Completions 地址，不表示支持 Anthropic 或其他协议。

`scripts/multimodal_test.py` 使用隔离配置和本机模拟上游，验证媒体请求、媒体响应、能力保存／覆盖／恢复自动、同步及取消同步。测试不访问外部媒体，也不证明真实供应商账号支持相关模型或输入格式。

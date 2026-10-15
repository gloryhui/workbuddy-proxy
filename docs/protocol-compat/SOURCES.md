# 原生协议依据

核对日期：2026-10-16。这里记录REST契约依据；本地Mock不证明真实模型支持相同参数，也不证明真实供应商计费正确。模型差异以显式配置和安全错误处理，不通过重试删参数来绕过。

| 范围 | 官方依据 | 实施约束 |
| --- | --- | --- |
| Responses输入、function_call和结果 | [Responses Create](https://developers.openai.com/api/reference/resources/responses/methods/create) | 使用input items、call_id、function_call_output；不把SDK output_text便利属性当完整REST输出 |
| Responses结构化输出 | [Structured outputs](https://developers.openai.com/api/docs/guides/structured-outputs?api-mode=responses) | text.format承接JSON schema；保留约束，不支持的schema关键词明确拒绝 |
| Responses思考与历史 | [Reasoning](https://developers.openai.com/api/docs/guides/reasoning) | effort和加密推理历史必须按原生契约处理；本地max需要显式映射，不擅自改成high |
| Anthropic流事件 | [Streaming](https://platform.claude.com/docs/en/build-with-claude/streaming) | 按message/content block状态处理partial_json、thinking/signature，最终终态与usage分开校验 |
| Anthropic思考 | [Thinking](https://platform.claude.com/docs/en/build-with-claude/thinking)、[Effort](https://platform.claude.com/docs/en/build-with-claude/effort) | budget/adaptive是不同配置模式；不根据客户端档位猜Token预算；不同模型可用effort不相同 |
| Anthropic结构化输出 | [Structured outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs) | output_config.format与strict工具输入是独立特性；必须保留schema和strict语义 |
| Gemini REST | [GenerateContent](https://ai.google.dev/api/generate-content) | contents/parts、functionCall/functionResponse、parametersJsonSchema；固定非JSON结果封装，不新增图片下载器 |
| Gemini思考回放 | [Thought signatures](https://ai.google.dev/gemini-api/docs/generate-content/thought-signatures)、[Thinking](https://ai.google.dev/gemini-api/docs/thinking) | 签名可能位于任意part，回放保留整组原生parts及顺序，不能只复制functionCall |

当前保守实现：Gemini非text response_format与严格工具约束尚无冻结的已验收映射，返回明确unsupported错误。官方GenerateContent页面当前同时出现新旧响应格式字段，后续若扩大支持，需要先核实实际REST形状并补独立用例。不要把Interactions、Live或Vertex字段混进GenerateContent。

闭环待验收：真实上游、真实WorkBuddy连续工具调用、签名以及价格来源准确性均需对应环境证据；现阶段协议模块单元测试只证明本地校验/编码/解析与缓存行为。

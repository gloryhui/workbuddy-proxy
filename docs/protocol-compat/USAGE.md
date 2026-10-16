# 配置与验收说明

WorkBuddy 始终调用本地 OpenAI Chat Completions 接口。供应商连接页的“上游协议”决定代理如何调用供应商，不能根据模型名称自动判断协议。原有供应商没有协议字段时继续按 `openai_chat` 处理。

| 选择 | API 地址填写示例 | 原生鉴权 | 模型发现 |
| --- | --- | --- | --- |
| OpenAI Chat Completions | `https://host/gateway/v1` | Bearer | 同前缀 `/models` |
| OpenAI Responses | `https://host/gateway/v1` | Bearer | 同前缀 `/models` |
| Anthropic Messages | `https://api.anthropic.com/v1` | x-api-key、anthropic-version | `/v1/models`，自动 after_id 翻页 |
| Gemini GenerateContent | `https://generativelanguage.googleapis.com/v1beta` | x-goog-api-key | `/v1beta/models`，自动 pageToken 翻页并筛选 generateContent |

网关前缀和版本必须与供应商提供的地址一致；不会猜测或自动切换协议。前三种协议允许填写匹配的完整推理端点；Gemini 填基础地址，模型填 `gemini-…` 或 `models/gemini-…`。禁止 URL 凭据、query 和 fragment；HTTPS 为默认，HTTP 仅允许回环测试。

## 思考选项

“协议选项”先应用到草稿，再点击主窗口“保存”。未配置的参数不进行隐式推测。

- Responses：默认使用原生 effort；本地 `max` 需要明确映射到供应商实际接受的档位。不会偷偷替换成 high。
- Anthropic：支持固定 budget 与 adaptive 两种显式 profile。budget 为正整数且至少 1024，并小于实际请求输出上限；adaptive 的档位映射须由供应商文档确认。版本目前固定为 2023-06-01。请求未指定输出上限时使用配置默认值 4096，受已知模型输出上限约束。
- Gemini：支持 budget 或 level profile。budget 填实际 token 数，level 填原生 minimal/low/medium/high。关闭思考只在 budget profile 且模型允许时提供，不用 level 伪造关闭。
- 模型能力、协议适配器和显式映射共同决定同步给 WorkBuddy 的有效能力。只有 `thinking=true` 不能证明可用的 Responses effort 列表。资料不完整时可手工声明模型能力；显式已知不支持的图片、工具、思考或输出上限会在发往上游前拒绝。
- 原生供应商的 WorkBuddy `useCustomProtocol` 固定为 false，使 WorkBuddy 保持本地 Chat 调用方式。手工能力勾选不能绕过适配器限制。

## 对话与限制

三个原生适配器均支持 JSON/SSE、系统与开发者消息、普通多轮、图片 data URI、函数工具和多轮并行结果。Responses 和 Anthropic 也支持相应远程图片输入；Gemini 当前仅接受内联图片，不增加下载图片的后台请求。

客户端只需要保留标准 Chat 消息及 tool_calls / tool_call_id。原生调用 ID、加密推理、签名等通过有界进程内回放缓存关联，不要求客户端传私有字段。缓存最多 30 分钟、512 个调用 ID、32 MiB；进程重启、超时、淘汰或路由有效设置变化后，过期工具历史返回 `conversation_state_expired`，需重新开始对话。不会虚构签名或从其他路由恢复状态。

Gemini 当前明确拒绝非 text response_format、strict=true 工具和 parallel_tool_calls=false；JSON schema 仅接受契约列明的保守子集。未知/不可等价转换的字段返回安全错误，不静默删除后重试。完整白名单见 [CONTRACTS.md](CONTRACTS.md)。

请求体 8 MiB；原生 JSON/解析聚合状态 16 MiB；单 SSE 帧 1 MiB；单工具参数 256 KiB；单响应 64 工具。流式按需读取、增量发送，工具回放需要的完整原生 assistant blocks 受聚合/回放预算约束。超限、截断及上游错误不会发成功终态或 `[DONE]`。

## 统计与价格

四种协议使用同一统计生命周期、写队列和 SQLite；当前 schema 6；schema 4 引入的 upstream_protocol 使历史行默认 Chat，不改历史价格和费用。统计详情显示上游协议。

价格表可直接增删改。每次接纳请求时冻结币种、计价方式、输入/输出/缓存单价和来源版本；结束时按该快照结算。修改或删除当前价格不改变过去或已经开始的调用。缺少价格时按配置币种默认零价；缺少有效 Token 时仍为未知，不能用 0 冒充用量和费用。

成本定价、采集与存储供应商列表均采用固定 20/50/100 页窗口与前后翻页。定价在核心端使用 SQLite 游标，不把所有价格加载到 UI；供应商采集设置只查询当前页。明细及分组也使用服务端游标分页。总览聚合另有预算，过大范围会要求缩小筛选，不宣称百万行实时统计必定完成。

## 实际供应商与 WorkBuddy 验收

仓库自测使用隔离临时配置、虚构 Key、本地独立 REST fixtures、真实核心和 SQLite。它证明适配链行为，不代表真实供应商计费或 WorkBuddy 工具执行器验收。

具备相应测试账号后，应逐个协议在 WorkBuddy 中确认：普通多轮；真实图片；同名并行工具及连续两轮结果；思考/签名；中途取消；异常响应；配置保存重开；统计用量和供应商账单核对。不要把连接探测成功称为全部功能兼容。测试前备份 WorkBuddy models.json；自动化验收不使用真实 Key，也不会修改真实 WorkBuddy 配置。

# Native 请求：从 ChatRequest 到 Responses、Anthropic 与 Gemini

本文面向熟悉 Java / Spring Boot、第一次阅读 Rust 的开发者，沿着当前代码解释：

- 客户端 Chat Completions JSON 如何变成统一 `protocol::request::ChatRequest`；
- 严格 JSON、字段白名单和工具历史校验保护了什么；
- 同一份请求如何映射为 OpenAI Responses、Anthropic Messages 和 Gemini Generate Content；
- 协议配置、端点与上游认证如何选择；
- native 入口 `proxy::native::prepare` 在完整 HTTP 请求中的位置；
- 本地校验失败、上游失败、流结束和客户端取消分别落在哪个边界。

完整链路先读 [协议转换总览](PROTOCOL_CONVERSION.md)，服务启动和普通 Chat 直通路径见 [服务核心源码流程](SERVICE_CORE.md)。本文展开 native 请求转换；之后可继续阅读 [响应与流式解析](PROTOCOL_RESPONSE.md) 和 [工具状态回放](PROTOCOL_REPLAY.md)。

## 1. 先看数据流和所有权

native 请求的主链路是：

```text
proxy::relay_to
  -> stats admission
  -> native::relay
     -> prepare
        -> ChatRequest::parse
        -> 能力/输出上限检查 -> replay resolve
        -> protocol::encode::encode
        -> 有界 JSON 序列化
     -> open -> 端点/认证 -> reqwest 发送
     -> output / streaming / sse
```

`proxy::relay_to` 先读取客户端 JSON 的 `model`，结合路径上的路由作用域，在路由表中找出唯一 `Arc<proxy::Route>`。路由中有两项容易混淆的模型语义：

- 客户端 `model` 是选路标识，可以是 alias、已安装标识，或作用域允许的实际模型名；`Routes::resolve` 会检查归属与歧义；
- `Route.connection.model` 是实际发给供应商的上游 model。

`ChatRequest::parse` 仍要求请求中存在非空 `model`，但 `ChatRequest` 结构体不保存它。`proxy::native::prepare` 随后把 `Route.connection.model` 传给 `protocol::encode::encode`；不能把 native 请求体中的 `model` 解释成“客户端原样发给上游的 model”。

所有权变化可以分成三段：

1. Axum body 被读成 `Bytes`，`ChatRequest::parse(&bytes)` 只借用原始 buffer；
2. 解析成功的 `ChatRequest` 拥有消息、工具参数、参数原文和图片字符串；
3. `encode` 再借用这个请求与 replay 映射，返回新的 `serde_json::Value`，最后用 `Limited` 写入新的 `Vec<u8>` 并转成 `Bytes`。

`proxy::native` 没有为单个转换创建线程。`async` 函数表示可暂停的 future；本次转换步骤本身是同步校验和 JSON 构造。HTTP awaiting、超时和取消由外层的 Tokio 调用链管理。

源文件 [src/proxy_native.rs](../../src/proxy_native.rs) 在 [proxy.rs](../../src/proxy.rs) 中通过 `#[path = "proxy_native.rs"] mod native;` 引入，因此 Rust 符号路径是 `proxy::native`；路径中的下划线只属于磁盘文件名。

对 Java / Spring 开发者，可先用下列类比建立概念，再以 Rust 源码边界为准：

| Rust 结构 | 接近的 Java / Spring 概念 | 关键区别 |
| --- | --- | --- |
| `ChatRequest` + parse helpers | 手写 DTO、Validator 与错误路径 | 所有权和借用由编译器追踪；返回 `Result`，不是抛异常后由框架统一捕获 |
| `encode::{responses,anthropic,gemini}` | DTO 到供应商 request 的 mapper | 这里是纯函数；失败显式返回 `protocol::Error` |
| `UpstreamProtocol` | 策略枚举 / Strategy 选择键 | 枚举值同时参与持久化；分派不会尝试自动猜测协议 |
| `reqwest::Client` 调用 | Java `HttpClient` 调用 | `async` future 不等于新线程；超时、取消和响应流仍由外层拥有 |

## 2. 严格 JSON：先排除歧义，再解释业务字段

### 2.1 `strict_json` 的边界

`protocol::request::strict_json` 使用 `serde_json::Deserializer` 和自定义 `DeserializeSeed` / `Visitor` 构建 `Value`。它比普通的 `serde_json::from_slice::<Value>` 多做了几项检查：

| 检查 | 当前规则 | 失败结果 |
| --- | --- | --- |
| 总字节数 | 大于传入 `limit` 即拒绝 | `request_too_large` |
| 对象重复键 | 任意嵌套对象出现同名键即拒绝 | `invalid_json` |
| 嵌套深度 | 准备继续进入更深层时，当前 depth 已大于 64 即拒绝 | `invalid_json` |
| 尾随内容 | `Deserializer::end` 还会检查根值后是否还有第二个值 | `invalid_json` |
| 非有限数字 | 不能构造成 `serde_json::Number` 的浮点值被拒绝 | `invalid_json` |

Chat body 的 `INPUT_LIMIT` 是 `8 * 1024 * 1024` 字节，工具参数原文的 `TOOL_ARGUMENT_LIMIT` 是 `256 * 1024` 字节。后者会再次调用严格 JSON 解析。数字都按字节计，不是 token 数；等于上限仍可进入解析，只有大于上限才拒绝。

严格 JSON 不负责判断业务字段是否支持。顶层 object 完成后，`ChatRequest::parse` 才检查字段白名单、角色、类型、范围和工具关系。

### 2.2 不做隐式转换

当前解析器不会把这些值等价处理：

- `true` 与字符串 `"true"`；
- `null` 与字段缺失；
- 空数组与 `null`；
- `reasoning_effort: "none"` 与省略 `reasoning_effort`。

字符串、布尔、数字、数组和对象都走各自校验函数。`n`、`max_tokens`、`max_completion_tokens` 要求 JSON 整数，所以 `1.0` 不会通过；`temperature` 和 `top_p` 使用 `as_f64()`，整数 `1` 与浮点 `1.0` 都可得到合法数值。这样做的原因是 native 编码器会根据“有没有字段”和字段类型选择不同协议字段；宽松 coercion 会把客户端一个拼写或类型错误变成另一个业务含义。

顶层只允许当前转换层理解的键：

```text
model, messages, stream, stream_options, n, tools, tool_choice,
parallel_tool_calls, max_tokens, max_completion_tokens, temperature,
top_p, stop, reasoning_effort, response_format
```

未列出的键返回 `unsupported_parameter`。`n` 当前只接受 JSON 整数 1；`stream_options` 只允许 `include_usage`，而且只有 `stream=true` 时才能出现。`max_tokens` 与 `max_completion_tokens` 可以同时出现，但值必须相等。

## 3. 统一 `ChatRequest` 保存什么

| 字段 | 业务语义 | `None` / 空值差异 |
| --- | --- | --- |
| `stream` | 是否要求 Chat 流式响应 | 默认 `false` |
| `include_usage` | 流式响应是否请求 usage | 默认 `false`，且依赖 `stream=true` |
| `messages` | 已通过历史校验的消息 | 至少一条 |
| `tools` | function 工具声明 | 空表示没有声明工具 |
| `tool_choice` | 自动、禁止、必须或指定函数 | 缺省是 `Auto` |
| `parallel_tool_calls` | 是否允许并行工具调用 | `None` 是“未指定”，不等于 `false` |
| `max_output_tokens` | 合并后的输出 token 上限 | `None` 是“客户端未指定” |
| `temperature` | 采样温度，解析范围 `0.0..=2.0` | `Some(0.0)` 是真实值 |
| `top_p` | nucleus 采样上限，解析范围 `0.0..=1.0` | `Some(0.0)` 是真实值 |
| `stop` | 停止序列 | 空数组表示不生成停止字段 |
| `reasoning_effort` | 统一思考强度 | `None` 省略；`Some("none")` 显式关闭 |
| `response_format` | text、JSON object 或 JSON schema 配置 | `None` 不生成 structured output |

`messages` 中的 `Message` 保存角色、文本/图片 parts、assistant tool calls 和 tool 结果 ID。`ToolCall.arguments` 是已解析且确认是 object 的参数值；`ToolCall.arguments_text` 同时保存客户端原始 JSON 文本。两者都不是冗余字段：native 请求有的地方要 object，replay 和协议身份处理又可能需要原始调用语义。

## 4. 消息、图片和工具历史

### 4.1 角色与 content

统一角色只有 `system`、`developer`、`user`、`assistant`、`tool`。

| 角色 | 允许的 content 形状 | 特殊约束 |
| --- | --- | --- |
| system / developer | 字符串，或 text 数组 | 图片数组不能用于这两个角色 |
| user | 字符串，或 text / image 数组 | 远程图片和 base64 图片都可进入统一模型 |
| assistant | 字符串、text 数组，或带非空有效 `tool_calls` 时的 null/省略 | 只有 assistant 可声明 tool calls |
| tool | 字符串 | 必须带 `tool_call_id`，数组 content 被拒绝 |

对 system/developer/user/assistant，数组 content 都允许 text；图片 content 只允许 user。assistant 的 content 可以省略或为 null，但前提是请求还带了 `tool_calls` 键；该数组随后必须非空、类型正确并通过 ID/name/arguments 校验，否则仍然失败。

text 内容会保留原始字符串；`Message::text()` 按 parts 顺序无分隔符拼接所有文本并忽略图片。Anthropic/Gemini 的工具输出和部分 Responses assistant 历史使用这个文本投影。

### 4.2 图片校验

当前图片输入只有两种：

- `data:image/png;base64,...` 或 `data:image/jpeg;base64,...`；
- 无用户名/密码、无 fragment、无控制字符的 HTTPS URL。

base64 检查字母表、长度是否为 4 的倍数、padding 数量和末尾未用 bit。它不解码像素，也不检查 PNG/JPEG magic，因此“结构上像合法 base64”不等于“图片内容有效”。远程 URL 同样不会在 prepare 阶段抓取。当前没有单独的单图大小上限，整个请求仍受 8 MiB body 上限约束。

`detail` 缺省为 `auto`；请求层接受 `auto`、`low`、`high`。协议编码器可能进一步限制，例如 Anthropic/Gemini 的非 Responses 映射当前只接受 `auto`。

### 4.3 工具声明与 choice

当前只支持 function tool：

- 工具数组最多 64 项；
- function 名称非空、按字节最多 64，只能包含 ASCII 字母数字、`_`、`-`；
- 名称在本次 tools 中必须唯一；
- `parameters` 必须是 object，缺失时补空 object schema；
- `strict` 保留 `None`、`Some(false)`、`Some(true)` 的差异。

`tool_choice` 支持 `auto`、`none`、`required` 和指定 function。`required`/指定函数不能建立在空 tools 上，指定函数必须已声明。`required` 是发送给上游的请求约束，不保证每个响应都含工具调用：后续 `Output::validate_request` 只在解析出的正常 `Finish::Stop` 终态检查“required/named 但无 call”；`Length` 或 `Filter` 等非 Stop 终态没有工具调用时，由各协议自己的终态处理，不能被统一宣称为缺少必需调用。

### 4.4 tool call 与 tool result 必须成组闭合

assistant 可以在一轮中声明多个 calls。每个 call：

- ID 非空、按字节最多 256、不能含空白或控制字符；
- 在同一段历史中只能出现一次；
- `arguments` 原始文本必须通过严格 JSON 解析，且根值是 object。

后续 tool 消息的结果可以乱序，但必须覆盖上一轮全部调用，之后才能出现普通消息。匹配依赖 call ID，而不是函数名，所以同名函数可以并行调用。结果文本不要求是 JSON。

一个合成多轮例子：

```text
user:       上海天气怎么样？
assistant:  content=null, tool_calls=[
              id=call-weather,
              name=weather,
              arguments_text="{\"city\":\"上海\"}"
            ]
tool:       tool_call_id=call-weather, content="{\"temperature\":18}"
assistant:  上海当前 18 摄氏度。
```

三种 native 请求分别会把这段历史表示为：

```text
Responses: user -> function_call(call-weather) -> function_call_output -> assistant
Anthropic: user -> assistant(text?, tool_use) -> user(tool_result) -> assistant
Gemini: user -> model(text?, functionCall) -> user(functionResponse) -> model
```

这只是字段形状示意，不是可直接发送的完整请求。真实编码还会处理 replay、配置和供应商限制。

## 5. 协议配置、端点与鉴权

### 5.1 配置文件只有一条 core 类型路径

`main.rs` 使用：

```text
#[path = "protocol/config.rs"]
pub mod upstream_config;
```

把同一文件编译成 `crate::upstream_config`；`protocol::config` 是 `pub use crate::upstream_config as config` 的重导出。因此在 core crate 内，`crate::upstream_config::UpstreamProtocol` 与 `protocol::config::UpstreamProtocol` 是同一类型。

GPUI 是独立 crate/进程，会从同一源码文件独立编译自己的类型与内存值。相同类型名不表示两个进程共享配置对象或内存状态。

### 5.2 端点规则

| 协议 | 非流式路径 | 流式路径 |
| --- | --- | --- |
| OpenAI Chat | `<base>/chat/completions` | 同路径，`stream` 在 body 中 |
| OpenAI Responses | `<base>/responses` | 同路径，`stream` 在 body 中 |
| Anthropic Messages | `<base>/messages` | 同路径，`stream` 在 body 中 |
| Gemini | `<base>/models/{model}:generateContent` | `<base>/models/{model}:streamGenerateContent?alt=sse` |

URL 保留 gateway path。上游只允许 HTTPS，HTTP 仅允许 localhost 或 loopback IP。base 不能含前后空白、控制字符、内嵌凭据、query 或 fragment。前三种协议可以接受同协议完整推理端点或 API base；Gemini 要求 API base，不接受已拼好的 generation endpoint。模型列表复用相同前缀与认证，并推导到 `/models`。

Gemini 模型段允许可选的 `models/` 前缀，长度按字节不超过 256，只允许 ASCII 字母
数字和 `-_.`，且拒绝 `..`。这个检查防止模型名变成额外路径或 query，但不验证模型
是否存在。

### 5.3 认证与 Azure

`Authentication` 有两个值：

| 值 | 允许协议 | 发送方式 |
| --- | --- | --- |
| `protocol_default` | 全部协议 | 按协议使用 Bearer、`x-api-key` 或 `x-goog-api-key` |
| `azure_api_key` | OpenAI Chat / Responses | 使用 `api-key` header |

Azure 必须显式选择，不能因为 URL 主机名相似就自动切换。Azure v1 base 的资源根或 `/openai` 会归一为 `/openai/v1`；已经带 gateway 前缀的 `/openai/v1` 保留。显式 `/deployments/...` endpoint 当前被拒绝，因为此实现从路由 model 和 body 发送请求。

native 上游 header 具体为：

- Responses 默认认证：`Authorization: Bearer ...`；
- Responses Azure：`api-key: ...`；
- Anthropic：`x-api-key` 和 `anthropic-version`；
- Gemini：`x-goog-api-key`。

`proxy::native::open` 还固定 `Accept-Encoding: identity`，避免压缩响应妨碍 JSON/SSE
解析；客户端 `x-request-id` 只有在 ASCII 且不超过 128 字节时透传。

### 5.4 思考设置校验

`ProtocolSettings` 保存：

- `anthropic_version`，当前校验只接受已审核的 `2023-06-01`；
- `default_max_output_tokens`，当前用于 Anthropic 未显式给上限时的回退；
- `reasoning_profile.mode` 与显式的 `budgets` / `levels`；
- Responses 的 `responses_efforts` 覆盖表。

配置校验会检查协议与思考模式是否匹配。Anthropic 预算至少 1024 token；预算和档位
不会由代码自行推算。Responses 的 native effort 不直接支持 `max`，需要显式覆盖，
例如把本地 `max` 映射到 `xhigh`。

## 6. 三种协议的请求映射

### 6.1 总表

| Chat 语义 | OpenAI Responses | Anthropic Messages | Gemini Generate Content |
| --- | --- | --- | --- |
| system/developer | `input` 中保留各自 role | 只能合并到开头 `system` | 只能合并到开头 `systemInstruction` |
| user 文本/图片 | `input_*` content | text/image content blocks | Gemini text/inlineData parts |
| assistant 文本 | role message；replay 用原生 output items | assistant message；replay 用原生 blocks | model content；replay 用原生 content |
| tool call | `function_call` item | `tool_use` block | `functionCall` part |
| tool result | `function_call_output` item | user `tool_result` block | user `functionResponse` part |
| 输出上限显式值 | `max_output_tokens` | `max_tokens` | `generationConfig.maxOutputTokens` |
| 客户端未给上限 | 省略 | 使用配置默认值 | 省略 |
| `reasoning_effort` | `reasoning.effort` 或显式覆盖 | 预算/adaptive 映射 | 预算/level 映射 |
| `stop` | 当前拒绝 | `stop_sequences` | `generationConfig.stopSequences` |
| structured output | text/object/schema 子集 | text 或 strict json_schema | 当前只接受 text |

### 6.2 OpenAI Responses

Responses 的普通历史被展平为 `input` item 数组。普通 assistant 文本使用没有 native ID 的 role/content 形式；缓存命中的 assistant 轮次则使用 replay 保存的原生 output items，以保留加密思考或原生工具身份。

请求固定写入 `stream`、`store=false` 和
`include=["reasoning.encrypted_content"]`。工具选择映射为：

| Chat `tool_choice` | Responses |
| --- | --- |
| `auto` | `"auto"` |
| `none` | `"none"` |
| `required` | `"required"` |
| named function | `{"type":"function","name":...}` |

`parallel_tool_calls` 只在客户端显式给布尔值且请求有 tools 时写出。工具 `strict`
缺省会被编码成 `false`。`response_format=json_schema` 会改写为 Responses 的
`text.format` 形状，但仍只做当前保守 schema 子集检查。

### 6.3 Anthropic Messages

Anthropic 只有一个顶层 `system`。只有对话开头连续的 system/developer 能进入这个字段；一旦进入普通 user/assistant 对话，再出现 system/developer 就返回 `unsupported_system_position`。如果整段历史只有 system/developer，则返回 `missing_conversation`。

连续 tool 结果可以乱序到达，但编码时按上一轮 call 顺序合并成一条 user message。
base64 图片和 HTTPS 图片 URL 都能映射，但 `detail` 当前只支持 `auto`。

思考规则按配置模式区分：

- 预算模式写 `thinking.budget_tokens`，要求预算小于输出上限；温度若存在必须为 1，
  且不能同时设置 top_p；
- adaptive 模式写 `thinking.type=adaptive` 与 `output_config.effort`，不接受任何
  temperature/top_p；
- 显式 `none` 写 `thinking.type=disabled`。

`temperature` 与 `top_p` 同时存在会被拒绝。`json_schema` 只有在调用方没有显式
`strict:false` 时才能映射；`text` 不生成格式字段；`json_object` 及未知格式会明确
返回 400 `unsupported_response_format`，不会静默降级。
`parallel_tool_calls=false` 会变成 `disable_parallel_tool_use=true`。

### 6.4 Gemini Generate Content

开头 system/developer 合并到 `systemInstruction`；assistant 在 contents 中使用 `model` 角色。工具结果按上一轮顺序合并为 `functionResponse`，文本放入 `response.result`；存在 replay 时恢复原生 function call ID。

Gemini 当前请求能力的保守边界是：

- 只接受 `detail=auto` 的 base64 PNG/JPEG；远程图片 URL 被拒绝；
- `strict:true` 工具被拒绝；
- `parallel_tool_calls=false` 被拒绝，`true` 没有额外 Gemini 字段要写；
- `response_format` 只接受 `text`，JSON object/schema 当前被拒绝；
- 输出上限只在客户端显式提供时写入，不套用 Anthropic 的默认值。

Gemini 的预算/level 映射必须由 `reasoning_profile` 明确配置。GEMINI budget 的
显式 `none` 写 `thinkingBudget=0`；level 模式写 `thinkingLevel`。

## 7. schema 检查不是完整 JSON Schema 验证器

`protocol::encode::schema` 是当前适配器的保守键白名单与递归检查。它允许：

```text
type, description, title, enum, required, properties, $defs,
additionalProperties, items, anyOf, $ref
```

其中 `$defs` 只允许 Responses，`$ref` 只允许 Responses 且必须是以 `#` 开头的本地引用。未知键、`properties`/`$defs` 不是 object、`anyOf` 不是非空数组、递归子节点不是 object 等会失败。

它不验证所有 JSON Schema 关键字之间的一致性，不解析引用目标，不验证正则是否可由
上游接受，也不拿实例数据做 validation。不能把它称为“完整 JSON Schema 验证器”。

## 8. `prepare` 如何接入完整请求

`proxy::native::prepare(bridge, route, bytes)` 按顺序执行：

1. `ChatRequest::parse(bytes)`；
2. 若路由的 `reasoning_effort` 不是 `default`，用它覆盖客户端的 effort；
3. 通过 `Catalog::resolve_protocol` 解析图片、工具、思考和输出能力；
4. 在已知能力不支持时拒绝图片、工具或思考组合；
5. 若目录给出输出上限，拒绝超过上限的显式请求；Anthropic 缺省时再使用
   `min(设置默认值, 目录上限)`；
6. 在进程内 replay cache 中解析 `wbpt_*` 历史；
7. 调用 `protocol::encode::encode`；
8. 用实现 `io::Write` 的 `Limited` 序列化，超过 8 MiB 时返回
   `converted_request_too_large`。

目录能力中的 `image_known=false` 或 `reasoning_known=false` 表示当前来源不知道，不能擅自解释成“一定不支持”。代码只在相应能力已知时执行拒绝。

`prepare` 返回 `Prepared { request, body }`：

- `request` 是拥有数据的语义 Chat 请求，后续响应适配和 replay 还要使用；
- `body` 是路由上游 model 的 native JSON，大小不超过 `BODY_LIMIT`。

`prepare` 不访问网络，也不提交 replay。`open` 收到成功结果后只表示拿到了上游响应头；同步响应 body 读取、EOF、协议解析、replay 发布和客户端交付仍在后续阶段。

## 9. 真实失败码与边界

`protocol::Error` 的 `param` 只应包含结构路径，不包含用户字段值。常见失败如下：

| code | 典型状态 | 含义 |
| --- | --- | --- |
| `request_too_large` | 400 | Chat body 大于 8 MiB |
| `invalid_json` | 400 | 语法、重复键、过深嵌套或尾随值错误 |
| `unsupported_parameter` | 400 | 字段或选项不属于当前白名单 |
| `invalid_parameter` | 400 | 类型、范围或必需字段不合法 |
| `invalid_tool_history` | 400 | call/result 顺序或 ID 关系错误 |
| `incomplete_tool_results` | 400 | 上一轮调用没有全部返回结果 |
| `invalid_tool_arguments` | 400 | 工具参数原文不是 object JSON |
| `invalid_image` / `unsupported_image_url` | 400 | 图片编码或 URL 边界不合法 |
| `model_does_not_support_images` | 400 | 已知模型不支持图片 |
| `model_does_not_support_tools` | 400 | 已知模型不支持工具 |
| `model_requires_reasoning` | 400 | reasoning-only 模型收到显式 `none` |
| `output_limit_exceeded` | 400 | 显式输出上限超过已知模型上限 |
| `unconfigured_reasoning_effort` | 400 | effort 没有显式 native 映射 |
| `conversation_state_expired` | 409 | `wbpt_*` replay 状态缺失、过期或路由不符 |
| `invalid_protocol_settings` | 400 | 协议设置与所选协议不匹配 |
| `unsupported_json_schema` | 400 | schema 使用了当前保守子集之外的键 |
| `converted_request_too_large` | 400 | 转换后的 native JSON 超过 body 上限 |
| `invalid_upstream_endpoint` | 400 | base 或最终端点不能按协议构造 |
| `upstream_timeout` / `upstream_connection_error` | 502 | `open` 阶段的连接或请求超时 |
| `upstream_http_error` | 上游状态 | 上游返回非 2xx，客户端看到同一状态 |
| `unsupported_upstream_content_type` | 502 | 声明的 JSON/SSE 与响应头不符 |
| `upstream_output_too_large` | 502 | 非流式原生响应超过 16 MiB 收集上限 |

`proxy::native::reject` 会把带结构 `param` 的 400 和 replay 409 记为 rejected；其他
转换错误通常记为 failed。`open` 的连接/超时使用 `ErrorKind::Connect`。这些是统计
终态判断，不代表一次“事务回滚”，也不改变已经完成的上游副作用。

客户端提前断开时，响应 body future 被丢弃，permit 与统计 guard 随所有权一起释放。
这只能证明本地 future 被取消；上游是否已经执行、是否停止生成，不能仅从本地 drop
推出。非流式响应完全读入并通过解析后，才可能发布 replay；把 body 交给 HTTP body
层与客户端网络实际收到也仍是两个不同观察点。

## 10. 建议断点与现有测试

建议断点按问题类型选择：

| 要观察的问题 | 断点 |
| --- | --- |
| 客户端 JSON 在哪一步失败 | `protocol::request::strict_json`、`ChatRequest::parse` |
| 工具历史为何不闭合 | `protocol::request::validate_tool_history` |
| 路由 alias 是否被换成真实 model | `proxy::native::prepare` 中 `encode` 调用 |
| 图片/工具/思考能力在哪一步拒绝 | `proxy::native::prepare` 的能力检查 |
| 某个 Chat 字段生成什么 native JSON | `protocol::encode::{responses,anthropic,gemini}` |
| 思考组合为何被拒绝 | `protocol::encode::thinking` |
| schema 哪个键不支持 | `protocol::encode::schema` |
| 端点或认证是否选错 | `ProtocolSettings::endpoint`、`proxy::native::open` |
| 本地 400/409/502 如何形成 | `proxy::native::{reject,failure}` |

可直接作为测试过滤器的现有路径包括：

```text
upstream_config::tests::protocol_values_are_explicit_and_settings_are_closed
upstream_config::tests::incompatible_or_unsafe_urls_fail_without_guessing
upstream_config::tests::thinking_settings_never_invent_budgets_or_accept_wrong_modes
protocol::request::tests::duplicate_nested_keys_depth_and_trailing_values_are_rejected
protocol::request::tests::same_name_parallel_calls_reordered_results_and_continuous_rounds_are_valid
protocol::request::tests::malformed_arguments_and_tool_result_identity_fail_closed
protocol::request::tests::image_encoding_is_validated_without_fetching_or_decoding
protocol::encode::tests::text_system_and_limits_map_without_changing_the_real_model
protocol::encode::tests::parallel_results_keep_ids_and_non_json_text_and_merge_only_result_turns
protocol::encode::tests::schemas_choices_and_thinking_never_silently_weaken
proxy::native::tests::undelivered_replay_ticket_is_revoked_and_delivered_ticket_survives
```

注意 `src/protocol/config.rs` 在 core 中是根模块 `upstream_config`，所以配置测试
过滤前缀使用 `upstream_config::tests::`，不是为教学文档另造一条模块路径。

## 11. 当前实现限制

- 只统一转换 current Chat request 的当前子集，未覆盖 Chat API 全部可选参数；
- `schema` 是保守子集检查，不解析引用目标，也不执行完整 JSON Schema validation；
- 图片 data 只做 base64 结构检查和 MIME 白名单，不解码像素或探测真实格式；
- Gemini URL 图片、部分 strict tool、部分 structured output 和
  `parallel_tool_calls=false` 当前明确拒绝；
- Responses `stop`、Anthropic `json_object` 等无法无损映射的字段不会静默丢弃；
- `prepare` 成功只表示生成并序列化了本地 native 请求，不保证上游接受、生成完成或
  客户端收到完整响应；
- replay cache、响应解码、流式 SSE 差分模型和失败处理还需结合 [响应专题](PROTOCOL_RESPONSE.md) 与 [回放专题](PROTOCOL_REPLAY.md) 阅读。

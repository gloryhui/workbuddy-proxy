# 原生协议响应适配源码导读

本文面向熟悉 Java/Spring Boot、刚接触 Rust 的读者，说明 WorkBuddy Proxy 如何把三种原生上游协议转换成 OpenAI Chat 兼容响应：

- OpenAI Responses；
- Anthropic Messages；
- Gemini GenerateContent。

导读按当前源码中的实际调用方和实现编写。它只解释响应适配，不把三种协议当成同一种事件格式，也不把 SSE 分帧、协议终态、Chat 完成和 TCP/HTTP EOF 混为一谈。

## 1. 建议阅读顺序

1. [sse.rs](../../src/protocol/sse.rs)：网络字节如何变成 SSE Frame 和 Event。
2. [streaming/mod.rs](../../src/protocol/streaming/mod.rs)：`Decoder`、`Step`、`Common` 和统一 Chat delta。
3. [responses.rs](../../src/protocol/streaming/responses.rs)：Responses 事件状态机。
4. [anthropic.rs](../../src/protocol/streaming/anthropic.rs)：Anthropic block 状态机。
5. [gemini.rs](../../src/protocol/streaming/gemini.rs)：Gemini candidate/part 状态机。
6. [output.rs](../../src/protocol/output.rs)：非流式完整 JSON 和流式 terminal snapshot 的统一 Output。
7. [usage.rs](../../src/protocol/usage.rs)：供应商 usage 到 Chat usage 与统计口径的规范化。
8. [proxy_native.rs](../../src/proxy_native.rs)：真实 body stream 驱动、最终请求校验、replay 发布和错误结束。
9. [replay.rs](../../src/protocol/replay.rs)：工具调用所需的进程内不透明状态缓存。

编码侧可另读 [request.rs](../../src/protocol/request.rs) 和 [encode.rs](../../src/protocol/encode.rs)，但本页重点在响应方向。

## 2. 六个不能混为一层的数据

同一条模型回复从上游到达客户端，会经过多种表示。每个表示解决的问题不同。

| 层次 | 本项目中的类型 | 谁拥有 | 含义 |
| --- | --- | --- | --- |
| 网络 chunk | `Bytes` / transport chunk | `proxy::native::NativeStream` 当前输入 | 传输库给出的任意字节切片，边界没有任何 JSON/SSE 含义 |
| SSE 分帧器 | `sse::Parser` | 单个流响应 | 跨 chunk 保存未完成 Frame，并返回已消费字节数 |
| SSE 原始帧 | `sse::Frame` | `feed` 的调用方，随后被 `into_event` 消耗 | 以空行分隔、可含多条字段行的原始字节 |
| SSE 事件 | `sse::Event` | `Decoder::event` 参数 | `kind` 加多条 `data:` 拼接后的字符串 |
| 原生协议事件 | 适配器中的 JSON `Value` | 当前适配器局部状态 | Responses/Anthropic/Gemini 自己的事件结构和顺序规则 |
| Chat delta | `streaming::Step.delta` | 当前 `Decoder` 调用返回值 | 已转换成 OpenAI Chat 语义的增量 |

实际公共解码类型是 `streaming::Decoder`，不是笼统的 `Stream`。网络 body 的驱动者是 `proxy::native::NativeStream`。`NativeStream` 持有 `Parser`、`Decoder`、未消费网络字节和 offset；`Decoder` 只处理已经形成的 Event。

```text
reqwest byte chunk
  -> NativeStream.input + offset
  -> sse::Parser::feed
  -> sse::Frame::into_event
  -> sse::Event
  -> streaming::Decoder::event
  -> protocol adapter state
  -> streaming::Step.delta
  -> Chat chunk
```

一个网络 chunk 可以包含多个 Frame；一个 Frame 可以跨多个网络 chunk；一个 Event 可能不产生任何 Chat delta；多个原生 Event 也可能共同完成一个 Chat tool call。不能用“收到 chunk 数”解释“产生了几个模型 token”。

## 3. Rust 所有权在这条链上的实际用法

`Parser::feed(&mut self, chunk: &[u8])` 只借用当前 chunk。它把尚未完成的 Frame 字节复制到自己的 `Vec<u8>`；返回 `(usize, Option<Frame>)` 后，调用方必须从 `chunk[consumed..]` 继续，因为网络 chunk 中空行之后的字节还没有被消费。

函数没有把多个 Frame 排队。这一点既控制内存，也让“慢消费者”只能积压当前 Frame 和尚未消费的 transport 输入，不会在 Parser 内部再复制整块 response body。

`Frame` 独占 `Vec<u8>`。`into_event(self)` 消耗 Frame，并在原分配上整理 data，产出拥有 `String` 的 Event。之后 Frame 已移动，不能再用；Rust 编译器会阻止访问被移动的值。

`Decoder::event(&mut self, event: Event)` 取得 Event 的所有权，因为 JSON 字符串已经没有跨调用保留的必要。`Decoder` 自己保留真正需要跨事件累计的状态，例如：

- 文本通道的长度与 SHA-256；
- 工具参数 JSON 字符串；
- token usage；
- Anthropic/Gemini 重建最终 native snapshot 所需的状态。

`Common::Track` 不保存正文全文。它只记录已发出文本的 UTF-8 字节长度和增量 SHA-256，因此 terminal full snapshot 到来时可以确认“此前已发出的内容确实是快照前缀”，再只发送未见后缀。Anthropic/Gemini 自己的 State 会保留 native block/part，这是协议 replay/终态重建所需，和 `Common::Track` 的职责不同。

`Clone` 也不是 Java 中的通用深拷贝承诺。例如 `sse::Event` 本身没有实现 Clone，`Frame` 设计为一次性消费；代码中某个字段能 `.clone()`，只说明该具体类型实现了相应 trait。

## 4. SSE Parser：只做分帧，不解释业务事件

[sse.rs](../../src/protocol/sse.rs) 的职责是从任意字节切分中识别 SSE Frame。当前实现：

- 流首可选一个完整 UTF-8 BOM；
- 接受 `\n`、`\r\n` 和单独 `\r`；
- 用空行结束 Frame；仅含空格或 tab 的行不是空行，不会结束 Frame；
- 不含 `data:` 的注释行（例如 `: comment`）不产生业务字段；
- `event:` 最长 128 字节，缺省事件名是 `message`；
- 多条 `data:` 以 `\n` 连接；
- `id`、`retry` 和未知 SSE 字段不进入响应业务；
- 单个 Frame 最多 1 MiB；
- 没有 `data:` 的 Frame 返回 `None`，不形成 Event。

合成的 SSE 字节示例：

```text
: heartbeat\r\n
event: response.output_text.delta\r\n
data: {"type":"response.output_text.delta","delta":"你"}\r\n
data: \r\n
\r\n
```

解析结果不是“正文里有换行”，而是：

```text
Event.kind = "response.output_text.delta"
Event.data = {"type":"response.output_text.delta","delta":"你"}\n
```

`Parser::feed` 只做 BOM 序列、边界和大小检查；普通正文 UTF-8 的合法性在 `Frame::into_event` 中检查。Event data 的 JSON 解析又发生在 `Decoder::event`，仍是下一层。

网络 EOF 时还必须调用 `Parser::finish`。如果内部仍有未闭合 Frame 或半个 BOM，会得到 `incomplete_sse_frame`。Parser clean EOF 仍不代表协议成功，因为还没有检查具体协议的 finish、final snapshot 或工具终态。

## 5. 协议终态、finish chunk、DONE 和 EOF

这四个边界必须分开看：

| 边界 | 含义 | 是否足够成功 |
| --- | --- | --- |
| 协议 terminal marker | 收到 completed、message_stop、finishReason 等 | 不够；仍需终态快照和 clean EOF |
| `Decoder::finish` | clean EOF 后完成协议 finalize | 生成最终 `Output` 候选 |
| Chat `finish_chunk` | 发给客户端的 `finish_reason` chunk | 只是转换后客户端协议的一部分 |
| `data: [DONE]` | 发给 Chat 客户端的流结束标记 | 只能在成功 finalize 后发送 |
| transport EOF | 上游 body 不再有字节 | 可能是完整结束，也可能是截断 |

Responses 在 `response.completed`/`response.incomplete` 事件中调用 `Common::reconcile`，但实际 `Output` 要等 `NativeStream` 看到 transport EOF、Parser `finish` 成功、`Decoder::finish` 成功后才发布。

Anthropic 在 `message_stop` 组装 native message 并调用 `reconcile`，同样仍需 clean EOF。

Gemini 的 `finishReason` 或安全 block 只是 terminal 线索；parts、trailing usage 和 signature 仍可能在后面到达。它的最终 native JSON 在 `Gemini State::finish` 中构造。

SSE Parser 不理解 `data: [DONE]`。三个 native Decoder 都先把 Event data 当 JSON object 解析，因此原生流中的裸 `[DONE]` 不是它们接受的业务事件。Chat 兼容流里的 `data: [DONE]` 是适配完成后由代理自己发出的结束控制帧。

任何 SSE、JSON、协议顺序、tool 参数或 transport 错误都会进入失败路径。失败路径发送错误事件并结束 guard，不能再补发成功 `finish_chunk` 或成功 `[DONE]`。

## 6. `Decoder` 与 `Step` 的状态所有权

[streaming/mod.rs](../../src/protocol/streaming/mod.rs) 的 `Decoder` 包含两份状态：

```text
Decoder
├─ Common
│  ├─ text/reasoning/refusal: Track
│  ├─ per-slot Track
│  ├─ tools: Vec<Tool>
│  ├─ usage
│  ├─ model
│  ├─ bounds/confirmed/terminal
│  └─ final_output
└─ Native
   └─ Responses | Anthropic | Gemini State
```

`Common` 负责跨协议稳定语义。它生成统一 Chat delta、保持本地 `wbpt_` tool id 顺序、校验 terminal snapshot 是否与已发内容一致，并在 `reconcile` 后保存一次性 `final_output`。

三种协议 State 分别负责自己的事件名、index、block、candidate 和累计 usage。代码用 Rust `enum Native` 加 `match` 分派。这里不是在运行时扫描 Spring Bean，也没有自动注入；`Decoder::new(protocol)` 明确构造对应状态。

`Step.confirmed` 表示已经取得足以确认 assistant 的协议信息，外层可据此发送一次 role chunk。它不表示流已 terminal。

`Step.delta` 是普通 `Value`，可能为空 object。usage-only、stop-only 或 merely-confirm 的 Event 不应伪装成内容 chunk。

## 7. Responses 状态机

[responses.rs](../../src/protocol/streaming/responses.rs) 用 `output_index` 登记 item 身份，并保存 response id。关键映射如下：

| Responses 事件/字段 | 统一作用 | 终态限制 |
| --- | --- | --- |
| `response.created/in_progress` | 记录 response id、模型，confirmed | 不结束流 |
| `response.output_item.added` | 登记 message/reasoning/function_call item | index/id 不可重复 |
| `response.output_text.delta/done` | 可见 content | done 是累计全文，需校验已发前缀 |
| `response.refusal.delta/done` | refusal | 不自动等于整个流成功 |
| `response.reasoning_summary_text.delta/done` | reasoning_content | 不暴露加密 reasoning |
| `response.function_call_arguments.delta` | 参数文本增量 | 必须能归属已知 function_call |
| `response.function_call_arguments.done` | 参数累计快照 | 最终必须解析为 JSON object |
| `response.output_item.done` | 完整 item snapshot | 与原先 item id/type 一致 |
| `response.completed` | 解码完整 response | `reconcile` 后等待 clean EOF |
| `response.incomplete` | 可映射 length/filter 等 | 不完整工具调用不能被当作成功 |
| `response.failed/error` | 失败 | 不发送成功 finish/DONE |

Responses 的 SSE header event 名和 JSON `type` 必须一致，缺省 header `message` 是例外。未知 output item、未知 content、重复 ID、模型漂移和 terminal 后继续内容都会失败。

## 8. Anthropic 状态机

[anthropic.rs](../../src/protocol/streaming/anthropic.rs) 以 `message_start -> content_block_start -> ...delta... -> content_block_stop -> message_delta -> message_stop` 推进。

| Anthropic 事件/字段 | 统一作用 | 额外规则 |
| --- | --- | --- |
| `message_start` | 保存 message 外壳、模型和初始 usage | content 必须为空，stop_reason 为 null |
| `content_block_start` | 建立 text/thinking/redacted_thinking/tool_use | index 从 0 连续，之前 block 必须关闭 |
| `text_delta` | content | 必须属于未关闭 text block |
| `thinking_delta` | reasoning_content | thinking 文本和签名分开保存 |
| `signature_delta` | 补 native signature | 不进入公开 Chat delta |
| `input_json_delta` | 工具参数增量 | start 已有非空 input 时视为语义冲突 |
| `content_block_stop` | 关闭 block | tool args 终值必须是 object；thinking signature 必须存在 |
| `message_delta` | stop_reason/stop_sequence 和累计 usage | 所有 block 必须已关闭 |
| `message_stop` | 重建 message 并 reconcile | 仍等待 transport EOF |
| `ping` | 心跳 | 无业务 delta |

Anthropic 的一个 `message_delta` 可以同时把 stop reason 与新的累计 `output_tokens` 带进来。代码不以“字段出现”简单相加；它用白名单合并累计快照，再由 usage 模块校验是否完整或 invalid。

## 9. Gemini 状态机

[gemini.rs](../../src/protocol/streaming/gemini.rs) 的 SSE 请求使用 `alt=sse`，但每个 SSE Event 的 header 必须是缺省 `message`，JSON 内没有统一的协议 `type` 字段。

| Gemini 字段 | 统一作用 | 额外规则 |
| --- | --- | --- |
| `modelVersion` | 响应模型 | 出现后必须始终一致 |
| `candidates[0].content.parts` | text/thought/functionCall/signature | 只接受 index 0 的单个 candidate |
| part `text` | content 或 reasoning_content | `thought=true` 时进入 reasoning |
| part `thoughtSignature` | native replay 签名 | 不进入公开正文；可单独作为尾部 part |
| part `functionCall.name/args` | 工具调用 | args 必须是 object；name/args 字段白名单受限 |
| `finishReason` | terminal 线索 | 之后拒绝非空 text 或任何 functionCall；空 text、signature-only 和 usage 尾巴仍可进入字段校验 |
| `promptFeedback.blockReason` | safety/filter 终态 | 与正常 parts 互斥 |
| `usageMetadata` | 累计 usage | 可以在 terminal 前后分多条到达 |
| `candidates: []` 或仅 usage Event | 无新增内容的元数据 | Event 至少要有一个已支持字段 |

Gemini 的工具参数不是 Anthropic 那种 offset JSON 字符串增量。当前 adapter 收到的是完整 `args` object 快照：每个 functionCall 转成一次完整 Chat arguments 字符串并立即做 object 校验。若重复的非空 native id 带有相同 name/args，只允许补充相同或缺失的 signature；name/args 变化会失败。

`Gemini State::finish` 在 EOF 时集合 parts 和 finishReason。如果 final usage 没有被确认，它会去掉 candidates/thought/total 等未完成输出计数，只保留已证明输入。它随后通过与非流式相同的 `output::decode` 和 `Common::reconcile` 检查最终一致性。终态阶段若又发现未发送过的可见 delta，会失败，而不是在 EOF 后偷偷补整段文本。

## 10. 工具参数增量与终态校验

三种协议在“工具参数如何到达”上不同，不能共用“每个 JSON 字符一定是独立 delta”的假设。

| 协议 | 流中参数形式 | 完成信号 | 本项目终态动作 |
| --- | --- | --- | --- |
| Responses | `arguments.delta` 文本增量；done 为累计字符串 | `function_call_arguments.done` 或 item done | `Common::args` 拼接，done 时严格解析 object |
| Anthropic | `partial_json` 文本增量 | `content_block_stop` | 拼接后严格解析，并写回 native input |
| Gemini | functionCall `args` 完整 object 快照 | 该 part 自带完整对象 | 序列化后立即完成校验；重复调用须完全一致 |

`Common::Tool` 确实保留最终 JSON 参数字符串和本地 Chat id，因为：

- tool stop 时要把半截文本解析成 object；
- 客户端下一轮会返回 tool result；
- replay 需要把本地 `wbpt_...` id 还原成上游 call id；
- 同一个响应的 tool index/id 必须稳定。

函数名、参数 object 大小、工具数量和未知/重复 ID 都有边界。`Length` 或 `Filter` 终态若仍带工具调用会被拒绝，避免把不完整参数当可执行调用。

`Output::validate_request` 的最终调用入口是 `proxy::native::pending`，不是某一个 adapter 的 `State::event`。流式 `Decoder::finish` 先产出完整 Output；HTTP 集成层随后检查原 Chat request 的 tools、tool_choice 和 parallel_tool_calls。只有 Stop 且没有 call 时，required/named choice 才触发“缺少必需工具”；Length/Filter 有各自的终态规则。

## 11. usage：未知、部分、累计和 invalid

[usage.rs](../../src/protocol/usage.rs) 把供应商 token 字段转换成统计 collector 认识的 Chat usage。所有计数单位都是 token。

| 协议 | 主要输入字段 | 统一规则 |
| --- | --- | --- |
| Responses | `input_tokens`、`output_tokens`、`total_tokens` | 读取 input/output/reasoning/cached details |
| Anthropic | `input_tokens`、cache read/create、`output_tokens` | `input_tokens` 是未缓存输入；统一 prompt 为三者之和 |
| Gemini | prompt/candidate/thought/total/cache/tool-use prompt | completion 为候选与 thought 之和；受限 hosted-tool prompt 非零则 invalid |

四个质量边界：

- 缺失：字段不存在或 usage 整体缺失，统一数字为 `None`，不能补 0。
- 零：供应商明确返回合法整数 0，是已知值。
- 部分：只有部分父字段可证明；例如 Anthropic 缺 cache read/write 时，不能声称总输入完整。
- 无效：负值、浮点、字符串、越界、父子关系矛盾或 cumulative 下降；`Usage.invalid=true`，正文仍可继续。

Anthropic 和 Gemini 的流式 usage 是累计快照。`merge_cumulative` 用新值替换白名单字段，绝不把每帧相同 input tokens 相加；字段下降或非法会变成 null，让最终统计成为 Invalid。

流式过程使用 `normalize_partial` 隐藏尚未完成的输出计数：

- Anthropic 暂时移除中间 `output_tokens`；
- Gemini 暂时移除 candidate/thought/total；
- Responses 的 usage 主要来自完整 terminal response。

流式 `usage_chunk` 只在原 Chat request 设置 `stream_options.include_usage=true` 且最终 `Usage.complete=true` 时发送。非流式的 `Output::json` 规则不同：只要 `usage.chat` 不是 null，就可附带已知的部分数字。

## 12. 非流式 `Output` 与流式 final `Output`

非流式路径先检查 HTTP 状态和 content-type，再持续读取上游 body，把所有字节限制在 16 MiB 内，等 transport EOF 后调用 `output::parse_json`。严格 JSON、协议解码、final finish、请求工具约束和 replay 校验依次通过，才构造客户端响应。

流式路径不等 body 到齐才开始发 delta；它先发增量，最后用适配器状态重建同一语义 `Output`。两条路径共用的不是 `Output::json`：非流式经 `Output::json` 构造完整 completion，流式通过 `Identity::chunk/finish_chunk/usage_chunk` 发增量；二者共用统一 Output 语义及 `output::decode`、usage normalize 和 finish 校验，但错误的时点不同。

`Output` 的主要字段：

| 字段 | 业务意义 | 默认/边界 |
| --- | --- | --- |
| `assistant` | 可见 text 和本地 tool calls | tool id 是 `wbpt_` 生成值 |
| `reasoning` | 可公开 reasoning summary | 不包含机密 signature |
| `refusal` | Responses refusal 正文 | 其他协议可能只有 Filter 终态、没有正文 |
| `finish` | stop/tools/length/filter | 默认 Stop 只是占位，必须被协议规则确认 |
| `usage` | 规范化 token 或 invalid/missing | 不决定正文 relay 成功 |
| `replay` | 工具调用需要的 native opaque state | 无工具调用时为 None |
| `model` | 上游 terminal 模型 | None 时回退到路由模型，不一定等于客户端 alias |

## 13. pending replay 入口

只有带工具调用的响应才需要保存 native assistant state。`Output.replay` 保存：

- Responses output array；
- Anthropic content blocks；
- Gemini content；
- `wbpt_...` 到上游 call id/name 的映射。

真实入口是 [proxy_native.rs](../../src/proxy_native.rs) 的 `pending`：

```text
Output
-> Output::validate_request(original ChatRequest)
-> out.replay.take()
-> replay::Pending::new(...)
-> process-local Cache
```

`Pending` 绑定 route generation、对话前缀、assistant 工具调用和 opaque native turn。它不写 SQLite、不写日志，也不直接交给普通客户端。非流式或流式都必须先完成完整输出、请求约束和 Pending 校验，才能发布；失败、截断或尚未真正交给 HTTP body 的响应可以撤销这次发布。

后续 Chat 请求通过相同对话前缀和 route 解析 replay，再由编码器还原成对应上游协议。普通客户端只看到标准 tool call id/arguments 与 tool result，不接触签名、加密 reasoning 或供应商原始 response ID。

## 14. 上游原始统计观察与转换结果的区别

Native adapter 的统计观察入口是 `RequestGuard::observe_native`，不是把转换后的 Chat JSON 再喂给普通 collector。

它接收：

- 上游 transport chunk 的原始字节长度；
- 是否出现语义正文或工具内容；
- `Usage::outcome()`；
- 上游响应模型。

它不接收原始 message/tool/image/signature 内容，也不会把 `Step.delta`、Chat chunk 或 response model alias 当作“上游原始 body”重复计字节。原始消息、工具参数、图片和 opaque signature 不进入统计明细队列。

转换结果是发给客户端的 Chat 文本、reasoning、refusal、tool calls 和 control frames。它可以比上游原始体更大或更小，因为：

- SSE 框架与 JSON 包装会被移除；
- 一个 delta 可能被有界切成多个 Chat chunk；
- finish、usage 和 `[DONE]` 是代理新增的控制帧；
- native opaque state 不进入公开响应。

因此不要用 Chat 输出 chunk 数量推断上游原始字节，也不要用上游字节数推断 token 数。

## 15. 推荐断点

按一次流式请求的推进顺序：

1. `Parser::feed`：确认当前网络 chunk 消费到哪里。
2. `Frame::into_event`：确认 event 名、空 data 和多行 data。
3. `streaming::Decoder::event`：确认协议分派和空 delta。
4. `responses::State::event`、`anthropic::State::event` 或 `gemini::State::event`：查看协议专属顺序。
5. `Common::emit`：文本 delta 与 snapshot 去重。
6. `Common::start_tool`、`Common::args`：本地 tool id、index 和参数终值。
7. `Common::reconcile`：terminal snapshot 与已发前缀是否一致。
8. `Decoder::finish`：clean EOF 后是否真有 final Output。
9. `proxy::native::pending`：请求工具约束和 replay Pending。
10. `proxy::native::observe`：原始传输字节、首内容和 usage 如何进入统计。
11. `Output::json` 或 `Identity::finish_chunk/usage_chunk`：最终客户端 JSON/control frame。

非流式从 `output::parse_json`、`output::decode` 和三个协议 decoder 开始；流式则从 Parser 和 `streaming::Decoder` 开始。

## 16. 现有测试入口

SSE 与分帧：

- `protocol::sse::tests::all_chunk_boundaries_preserve_utf8_bom_crlf_and_multiline_data`
- `protocol::sse::tests::huge_transport_chunk_yields_only_one_frame_and_partial_eof_is_error`
- `protocol::sse::tests::oversized_frames_fail_before_retaining_unbounded_chunks`

非流式 Output：

- `protocol::output::tests::responses_preserves_refusal_reasoning_and_parallel_call_identity`
- `protocol::output::tests::anthropic_opaque_blocks_stay_private_and_usage_is_disjoint`
- `protocol::output::tests::gemini_keeps_signatures_on_text_and_tools_and_rejects_extra_outputs`
- `protocol::output::tests::malformed_error_unknown_tools_and_incomplete_states_never_succeed`

usage：

- `protocol::usage::tests::anthropic_cache_components_are_disjoint_and_missing_is_unknown`
- `protocol::usage::tests::gemini_thoughts_are_counted_once_and_totals_must_agree`
- `protocol::usage::tests::negative_fractional_huge_and_conflicting_usage_never_becomes_a_zero_bill`
- `protocol::usage::tests::cumulative_metadata_is_bounded_and_decreases_stay_invalid`

流式三协议：

- `protocol::streaming::tests::responses_delta_done_and_full_snapshots_do_not_duplicate_text_or_arguments`
- `protocol::streaming::tests::response_terminal_only_fallback_emits_once_and_keeps_stable_tool_indexes`
- `protocol::streaming::tests::anthropic_thinking_parallel_calls_and_cumulative_usage_are_separate`
- `protocol::streaming::tests::gemini_waits_for_trailing_usage_and_preserves_any_part_signatures`
- `protocol::streaming::tests::missing_terminal_truncated_frames_mismatched_types_and_unknown_content_fail`
- `protocol::streaming::tests::standard_chat_history_replays_two_tool_rounds_without_private_fields`
- `protocol::streaming::tests::invalid_usage_does_not_abort_valid_native_output`
- `protocol::streaming::tests::initial_counters_are_not_a_final_zero_output_bill`

HTTP 集成与 replay 发布：

- `proxy::native::tests::bounded_delta_cursor_preserves_unicode_and_arguments_without_building_an_event_queue`
- `proxy::native::tests::undelivered_replay_ticket_is_revoked_and_delivered_ticket_survives`
- `proxy::native::tests::converted_response_headers_never_reuse_payload_metadata_or_cookies`

统计观察：

- `stats::event::tests::native_cumulative_usage_reuses_the_exactly_once_finish_queue_with_disjoint_input`
- `stats::event::tests::native_cancellation_never_claims_a_valid_terminal`

测试名以源码中的模块路径为准。修改本导读涉及的协议行为后，应先运行与发生变化的协议和边界最接近的测试；若共享 Decoder/SSE/usage 规则变化，再扩大三协议测试范围。

## 17. 阅读时容易犯的错误

- 把 transport chunk 当一个 Event；
- 把空 `Step.delta` 当模型输出；
- 把 `response.created` 或 `message_start` 当完成；
- 把 Gemini `finishReason` 出现当 clean EOF；
- 把 `[DONE]` 当 SSE 分帧层自己理解的成功标记；
- 认为三种协议都有 offset 形式的工具参数 delta；
- 把 cumulative usage 每帧相加；
- 把 invalid usage 改写成 0；
- 把 `Output.replay` 序列化给普通客户端；
- 认为 `Decoder::finish` 已执行请求工具约束或发布 Pending；
- 用 Chat 转换后的字节数描述上游原始响应体；
- 认为 Java/Spring 会自动注入 Decoder 或 State；本项目由显式构造、所有权和 `match` 分派完成。

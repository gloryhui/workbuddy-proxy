# 协议转换契约

本文件的“必须”属于项目要求。原生字段的最终拼写、事件名和模型支持范围必须用 SOURCES.md 的当前官方API文档复核，并加入脱敏fixture。禁止根据SDK辅助属性臆造REST字段。特别是 Responses 的 `output_text` 是SDK便利属性，不能只读取REST中的同名顶层字段。

## C01 输入与顶层参数

跨协议输入必须是合法UTF-8 JSON对象，model为非空字符串，messages为数组。拒绝重复顶层键和会影响含义的嵌套重复键，避免路由与执行看到不同值；Chat fast path不因此全量重解析。拒绝超深结构，转换JSON最大嵌套深度64。

| Chat 输入 | Responses | Anthropic Messages | Gemini GenerateContent |
| --- | --- | --- | --- |
| model | 真实model | 真实model | URL中的真实model |
| stream缺失/false | 普通JSON | 普通JSON | generateContent |
| stream=true | SSE | SSE | streamGenerateContent + alt=sse |
| messages | input items，按顺序 | system + messages blocks | systemInstruction + contents/parts |
| max_completion_tokens/max_tokens | max_output_tokens | max_tokens | generationConfig.maxOutputTokens |
| temperature/top_p | 经核实同名字段 | 同名字段，按当前互斥约束校验 | generationConfig.temperature/topP |
| stop字符串/1至4字符串数组 | 无等价映射则400 | stop_sequences | generationConfig.stopSequences |
| tools | 原生function definitions | tools的input_schema | functionDeclarations |
| tool_choice | 原生函数选择 | auto/any/tool/none | functionCallingConfig |
| parallel_tool_calls | 原生等价字段 | 原生禁并行设置，如已支持 | 有文档等价才映射，否则显式非默认值400 |
| reasoning_effort | reasoning.effort | profile生成thinking/effort | profile生成thinkingConfig |
| response_format | text.format | 当前官方结构化输出等价字段 | 当前官方JSON schema/MIME配置 |
| stream_options.include_usage | 只控制本地Chat末尾usage事件 | 同左 | 同左 |

参数精确定义：

1. 缺失stream视为false，null/数字/字符串不转换成bool；stream_options仅允许include_usage布尔值。false流不接收流专用对象，除非已证明当前Chat语义允许且有fixture。
2. 输出上限两字段只给一个时映射；都给且相等时接受，都给且不同400。必须是正整数，不接受字符串、浮点、负数；不静默钳制。默认上限只用于原生必填且客户端缺失的情况，取配置与已知模型输出硬上限的较小值；这是默认选取，显式值超限则400。
3. n缺失/1可接受，n>1跨协议400；不能少返回choice又称满足n。其他n值400。
4. response_format缺失或type=text等价默认可接受。P0必须实现Responses json_schema；Gemini/Anthropic若当前API存在等价原生schema则实现已文档化子集并测试，否则明确400并显示能力不支持。json_object不得被当成严格schema保证。任何不支持的schema关键字400，不能默删 `$ref`/additionalProperties/required/enum改变约束。
5. tools只接受function类型；完整保留参数schema的可表达语义。不支持的JSON schema特性在发送前400，给字段路径。工具名/说明长度沿官方限制校验，不自行改名而不维护逆映射。
6. `tool_choice=auto/none/required`和指定function必须实现；指定工具必须存在。required不等价auto。某协议无法对当前模型实现指定模式时400，不删约束。
7. presence_penalty/frequency_penalty/logit_bias/logprobs/top_logprobs/seed/modalities/audio/prediction、legacy functions/function_call、任意厂商额外字段：除非本目录增加明确官方映射与测试，否则非空有效输入400。不能把0/null当成“非空有效”随意处理；每个容许默认值必须单列白名单测试。
8. `user`、`metadata`、`safety_identifier`等并非自动无语义，不默认删除。跨协议首版未映射就400，WorkBuddy实际若发送，必须先记录脱敏字段形状并补官方等价映射/明确本地消费规则，不能靠忽略请求通过。
9. 本地字段仅允许代理自身已声明并消费的兼容信息，不把任意下划线字段视为可忽略。未知字段默认400；错误param使用如 `messages[3].tool_call_id` 的路径，message不回显值。
10. Chat路径继续支持已有未知字段和参数原字节转发；不能用这份白名单过滤旧请求。

## C02 消息和系统提示

- user/assistant/system/developer/tool角色分别校验；未知角色400。content可为字符串或支持的内容数组；assistant仅tools时允许content=null/空，其他空输入按原生合法性校验。
- Responses按原顺序保留system/developer/messages，工具项插入原位置。assistant历史文本使用正确原生输出类型，不把所有角色都改成input_text。
- Anthropic/Gemini只有一个system容器时，允许把位于普通对话之前的system/developer块按原顺序转换为多text blocks。system与developer层级被合并是公开兼容限制，不伪称完全等价；普通消息中途插入system/developer无法等价表达时400，不把它移到最前。
- 同角色相邻消息必要时合并blocks而非简单拼字符串；普通文本、图片、工具结果顺序不能颠倒或丢分隔。role=tool映射为原生工具结果，不能转成一句“工具说…”的普通user文本。
- user多模态只支持text与image_url；assistant图片、文件、音频、视频及其他块本次400。不得返回200空文本掩盖不支持。
- 跨协议首版不替用户补“你是助手”等提示，不注入工具描述到system，不改用户正文、不截断历史。

## C03 图片

| 输入 | Responses | Anthropic | Gemini |
| --- | --- | --- | --- |
| data:image/{supported};base64,... | input_image data URL | image.source base64，media_type/data | inlineData mimeType/data |
| https远程图片URL | 原生支持则引用URL | 原生支持URL source则引用，否则400 | 仅官方允许的等价fileData URI；普通HTTPS不能保证支持时400 |
| http远程、file路径、未知data类型 | 400 | 400 | 400 |

不为图片新增服务器下载器，不获取用户URL、不读取本地文件、不把data URL写日志。至少支持各协议官方交集中的PNG/JPEG base64。校验base64格式、mime和大小预算，不能decode后放大超过入口预算又无上限。可保持编码字符串、增量校验以避免多份大副本。

image_url.detail非默认值只能在有等价能力时映射；无法映射时400。URL引用能否被上游读取属于真实验收，不由Mock成功推断。能力标记supportsImages表示有上述已实现图片子集，UI文档明确各协议远程URL限制。

## C04 工具声明与历史

每次调用以tool_call_id为唯一身份。函数名允许重复调用，同名并行绝不能按name合并。Chat arguments是JSON字符串，传给要求object的原生API前严格parse，必须为object；非法JSON/重复键/超限/非object400，禁止用空对象补救。

| Chat | Responses | Anthropic | Gemini |
| --- | --- | --- | --- |
| assistant.tool_calls[].function | function_call item | assistant tool_use block | model functionCall part |
| tool_call_id | 与call_id关联 | 与tool_use_id关联 | 原生id或代理维护的稳定关联，不仅name |
| role=tool content | function_call_output.output | user tool_result.content | functionResponse.response |

非JSON工具结果仍须作为字符串无损表达。Gemini需要对象时使用固定封装 `{"result": 原字符串}`，不要试图从任意文本猜JSON；JSON对象结果是否直接转object必须固定一种规则，首版统一该封装并测试。工具结果文本不得拼进function name。

要求：

1. 单工具、同名并行至少两工具、乱序工具结果、第二轮再调用、最终回答均跑通。
2. 缺失/重复/未知tool_call_id、重复结果、结果先于调用、未满足的上一轮调用夹杂普通user消息，发送前400；允许一次提交完整并行组的乱序结果，原生需顺序时按调用顺序重排结果块，内容关联不变。
3. 一个assistant同时有说明文本和tools时，两者保留。原生工具参数完成时不是整条响应完成。空arguments增量可接受，但最终空字符串无合法JSON则错误。
4. 连续多个role=tool在Messages合并为一个user的多个tool_result，在Gemini按文档组织functionResponse，禁止丢后面的结果。
5. tool_choice=none时上游却返回tool，作为502协议不匹配；不能交给WorkBuddy偷偷执行。
6. 不在代理内执行工具。工具名、参数的任何内容不成为系统命令、路径或日志格式。
7. 工具回放复原使用内部ID映射；不能相信客户端自行附带的原生signature字段来绕过缓存绑定。

## C05 Chat 非流式输出

原生成功输出统一：

```json
{
  "id": "chatcmpl-wbp-EXAMPLE",
  "object": "chat.completion",
  "created": 1792022400,
  "model": "actual-upstream-model",
  "choices": [{
    "index": 0,
    "message": {"role": "assistant", "content": "回答"},
    "finish_reason": "stop"
  }],
  "usage": {"prompt_tokens": 10, "completion_tokens": 4, "total_tokens": 14}
}
```

示例时间与内容只用于说明形状，实际created取本请求固定Unix秒。id对一个请求稳定，跨请求唯一；跨协议用代理chatcmpl ID，不直接冒用resp_/msg_类型。model优先有效上游模型字段，否则使用真实配置模型；不暴露本地别名。Chat透传原model/id不改。

tools输出时message.tool_calls包含id/type/function.name/function.arguments；arguments始终合法JSON字符串，只有tools时content=null。finish_reason依据C08。有文本和tools时两者并存。

原生空合法回答与拒绝必须区分。Responses拒绝块用Chat message.refusal；其它原生安全阻断无正文时使用已定义的content_filter终态，不捏造解释正文或把上游错误当拒绝。

原生文本response JSON包含多个非工具内容块时按序编码；专有不可表达的非文本输出不能默默消失。跨协议只接受单个generation，Gemini意外多candidates要明确拒绝或实现多choice契约并同步输入n，首版选择拒绝。

## C06 SSE 字节层

SSE解析是字节状态机，覆盖LF、CRLF和合法CR，空行定界，多data行用换行连接，注释/heartbeat忽略，event字段与data正确关联。UTF-8可以跨网络chunk。首个UTF-8 BOM按标准处理。HTTP chunk边界不等于行或事件。

每一个原生完整事件解析成语义事件，再编码 `data: {Chat chunk}\n\n`。标准结构：

```text
data: {"id":"chatcmpl-wbp-EXAMPLE","object":"chat.completion.chunk","created":1792022400,"model":"actual-upstream-model","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}

data: {"id":"chatcmpl-wbp-EXAMPLE","object":"chat.completion.chunk","created":1792022400,"model":"actual-upstream-model","choices":[{"index":0,"delta":{"content":"回答"},"finish_reason":null}]}

data: {"id":"chatcmpl-wbp-EXAMPLE","object":"chat.completion.chunk","created":1792022400,"model":"actual-upstream-model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]

```

首个role chunk在第一个有效原生语义事件确认后发出；可提前flush，不能等全回答。所有Chat chunk共用id/created/model，choice.index固定0。文本输出只包含新增文本，不把累计全文重复发送。

工具SSE index从0连续分配，按首次出现顺序固定；不要使用Anthropic content_block index或Responses output_index直接充当Chat tool index。首次工具delta发送完整id/type/name，后续arguments增量引用同index；参数可能包含转义、多字节和截断JSON，最终合并后严格验证object。不要把结束事件里的完整arguments再次append导致重复。

include_usage=true且用量完整时：finish chunk之后、DONE之前发usage-only chunk，choices=[]，usage为累计值。false/缺失不额外发usage-only；非流式不受该开关影响。未知用量不填0。

成功只发一个finish和一个DONE。错误之后没有成功finish和DONE；上游终止后的重复语义事件不能生成第二次终态。需要识别合法terminal，再处理其附带usage，不读无限尾流。

## C07 原生事件状态

### Responses

- 识别response.created/in_progress等metadata、output_item.added/done、content_part.added/done、output_text.delta/done、function_call_arguments.delta/done，以及response.completed/incomplete/failed和error。执行时以当前官方流事件表复核完整名称。
- 以item_id/output_index和call_id区分message/reasoning/function call；不把reasoning item当成回答。
- output_text.done、arguments.done、output_item.done和terminal完整快照是收尾校验，不重复追加已有delta。允许某兼容上游只给done而没有delta时按明确定义的fallback发送一次完整值，不能在已流输出后又补全文。
- completed才成功；incomplete依据原因映射length或错误；failed/error走安全错误。EOF未收到合法terminal是截断失败，不能按stop处理。
- reasoning summary是独立思考摘要，可映射reasoning_content扩展；encrypted_content属于opaque回放，不能输出为思考正文。
- 请求默认store=false，完整历史手动发送，不使用previous_response_id/conversations跨请求串会话。不启用任何上游托管工具。

### Anthropic Messages

- message_start → content_block_start/delta/stop（可多个）→ message_delta → message_stop。ping不算内容。
- text_delta → content；input_json_delta.partial_json → 对应工具arguments增量。block_start的非空初始input与随后delta必须遵照当前官方语义，不能先输出`{}`又拼对象。
- thinking_delta只映射reasoning_content，signature_delta、redacted_thinking和相关不可见block保留给C10。禁止签名进入content。
- message_delta的stop_reason并非立即关闭stream；需要message_stop确认。message_start/message_delta usage是累计/更新关系，不按事件次数累加。
- event:error即使HTTP200也是失败；原生事件JSON与event字段冲突、工具block未闭合、重复index、消息未终止均失败。

### Gemini GenerateContent

- 只实现REST GenerateContent的JSON或alt=sse，不把Interactions事件混进同一个适配器。
- 处理candidates[].content.parts文本、functionCall、thought标记/签名、finishReason、promptFeedback和usageMetadata。安全阻断可没有candidate，不能视为JSON损坏或普通空成功。
- 以candidate.index校验唯一generation；parts的text按当前原生流式增量定义输出。functionCall.args若为完整对象，只序列化一次，不人为按字节伪造增量。若当前API支持部分参数流，单独实现其字段/状态fixture，不将完整快照误拼为增量。
- functionCall缺原生id时生成代理ID并保留其parts顺序/签名绑定，下一轮据此复原。不能用name唯一定位。
- 合法finishReason出现后可继续接收合法的末尾usage帧；正常HTTP EOF完成流。EOF没有terminal则失败。晚到的工具签名必须在replay记录提交前接收，不能finishReason一到就丢流。
- 未知新事件/part不能无条件忽略。明确metadata可忽略；可能承载内容/工具的未知类型给unsupported_upstream_event，并在文档记录，避免静默丢结果。

## C08 结束原因

| 原生终态 | Chat finish_reason |
| --- | --- |
| 正常回答结束 / end_turn / stop_sequence / STOP 且没有tools | stop |
| 正常结束且本轮产生合法待执行function calls / tool_use | tool_calls |
| 输出token限制 / max_tokens / MAX_TOKENS / max_output_tokens incomplete | length |
| 已确认原生安全过滤，非HTTP服务错误 | content_filter |
| overloaded/failed/error/网络断开/无terminal/无效工具JSON | 安全错误，不生成成功finish |

不把工具参数截断映射成可执行的tool_calls。上游length时若tool尚未完成，不能提交部分工具给WorkBuddy；流已发的片段只能以错误结束而非声称可执行，独立消费者必须不执行未成功结束的工具。其它未知stop_reason先按错误处理，除非官方证据明确对应映射。

## C09 usage

跨协议Chat usage统一为非负整数；父项完整才输出完整usage，缺失保持未知，不估算。重复累计usage替换/校验，不累加。细分项不能重复加进总量。溢出、负数、小数、父项冲突给明确diagnostic code，不伪造0。

- Responses：input_tokens → prompt_tokens；output_tokens → completion_tokens；total_tokens与相加一致；缓存/思考子项只放对应details，不重复加入父项。
- Anthropic：总输入须按官方当前定义处理input_tokens、cache_creation_input_tokens、cache_read_input_tokens，不能仅复制input_tokens造成缓存输入丢失。缓存TTL细项是cache_creation的子项，不再次相加；输出取最终累计output_tokens。
- Gemini：不能默认candidatesTokenCount已包含thoughtsTokenCount。S0确认当前GenerateContent定义后固定prompt/completion/total及工具额外输入的公式，给两个可手算fixture（有思考、无思考）。无法证明一致时省略标准usage并标记unsupported_usage_shape，不自己凑数。
- 规范化usage必须交给已经合入的统计模块统一结算，不在适配器直接写SQL或另算费用。标准Chat usage无法完整表达的缓存/思考细项通过内部类型化usage事件传给统计，不依赖有损的对外JSON；见STATISTICS-INTEGRATION.md。

## C10 思考与 opaque 回传

reasoning_content是常见Chat扩展，不是保证WorkBuddy保留的标准历史字段。不要依赖WorkBuddy回传私有JSON字段来恢复签名，也不能把签名编码进工具名/参数/content。

本次必须实现进程内有界replay缓存，让WorkBuddy仅回传标准tool_calls.id和工具结果也能复原原生工具回合。Responses reasoning items/encrypted_content、Anthropic完整thinking/redacted blocks及signature、Gemini相应parts/thoughtSignature是opaque数据，原样回放，不解析、不重造。

缓存契约：

1. 原生调用暴露为随机、长度合法、带可识别`wbpt_`前缀的Chat tool ID（128bit随机量或更强），内部记录原生call ID、完整相关块/顺序、其一组并行调用、route ID、协议/URL/Key generation及上下文绑定。
2. binding使用当前调用前history的规范语义摘要 + 当前assistant tool calls/text摘要，不仅tool name/args。规范规则明确空content与null的允许等价，输入图片只哈希不缓存；与WorkBuddy实际回传形状核对。不能给模型A的tool ID补模型B的签名。
3. 先登记pending记录，只有上游工具块/签名完整且终态有效才原子commit；客户端取消、原生错误、参数不完整时丢pending，不能留下半个可重放工具。
4. 下次请求重放所有当前工具循环所需块，同一条记录允许重复读取以支持客户端重发/分支，不“读一次即删”。不自动重发原生请求。
5. 记录有随机ID、绑定、过期、容量预算；过期/淘汰/核心重启/Key或协议变更导致回放不可用，返回409 `conversation_state_expired`，提示重新开启本轮任务。不得退化为无签名原生请求，也不跨账号复用。普通新文本对话仍可用。
6. 带wbpt_前缀但不存在的ID与篡改上下文分别409/400安全错误；不根据全局最近同名调用推断。用户自带历史外部tool ID在无需原生opaque时可以按合法静态历史转换，缺必要签名时明确不支持。
7. cache命中/未命中日志只记录数量/错误码，不记录ID到签名/思考的对应值。清除供应商/变更凭据使其generation失效；进行中旧快照可完成返回，但旧generation记录不得被新请求使用。
8. 仅内存、TTL/项数/字节上限见架构文档。记录被清除时释放所有关联块；Arc持有的进行中引用仍计入预算，不因从map移除就假称内存释放。
9. 对“思考开启 + 工具结果 + 再调用 + 最终回答”三个原生协议分别测试；关闭思考也测试。不能以关闭思考作为默认修复绕过必测链。

公开兼容限制：需要opaque的工具任务依赖本代理进程的30分钟缓存，核心重启或超限可能需要重开任务。无需opaque的普通消息历史不应强制依赖缓存。UI和README明确此边界。

## C11 错误、头与取消

| 情况 | HTTP/流行为 |
| --- | --- |
| 本地输入或不支持字段 | 400，error.type=invalid_request_error，param给路径 |
| replay过期 | 409，error.code=conversation_state_expired |
| 原生HTTP 401/403/429/5xx | 尽量保留上游状态，转换为安全Chat error envelope |
| 连接失败 | 502 upstream_connection_error |
| 连接/读取deadline超时且尚未发头 | 504 upstream_timeout |
| 原生2xx但JSON/SSE结构不合法 | 尚未发头502；已发流则error事件后失败关闭 |
| 原生2xx合法安全阻断 | 正常Chat content_filter语义，不编造正文 |

错误形状固定为 `{"error":{"message":"安全简短说明","type":"...","param":null,"code":"稳定机器码"}}`。必要时附脱敏protocol/request ID，但不回显原始upstream body、headers、参数、URL query或Key。上游HTML/非JSON错误只给状态+安全摘要，不能直接塞进message。

转换模式响应headers只保留经过验证的Retry-After、安全request-id/速率限制元数据，设置正确Content-Type；移除原Content-Length、Content-Encoding、ETag、Digest、Content-MD5、Transfer-Encoding及hop-by-hop字段。不能转了body还留旧压缩长度/校验和。不要把原生Set-Cookie或鉴权header发给WorkBuddy。

原生请求声明Accept-Encoding: identity，首版不实现压缩转换；上游仍压缩时按unsupported_content_encoding失败，不全局开启reqwest隐式解压破坏Chat透传。JSON只接受预定JSON content type，流接受SSE；错误类型单独安全处理。

HTTP头已发之后不能再把200改成502。输出单次Chat形状的SSE error后终止body，不发成功finish/DONE。若WorkBuddy实际不展示该错误，记录真实现象并采用可验证的传输失败提示，不改为伪成功。独立消费者必须将异常EOF视为失败，不能执行未完成工具。

下游断开立即Drop源流和permit。慢消费者使上游poll自然受背压；任何为adapter创建的任务必须与body同生共灭，禁止detach。输出长度超过工程预算按错误停止，不能悄悄截断再发DONE。

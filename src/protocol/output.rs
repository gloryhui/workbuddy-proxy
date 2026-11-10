//! Native JSON decoding and common Chat output. Streaming state machines reuse
//! these terminal snapshot validators; usage is not a condition for relay success.
//!
//! 本模块是非流式原生协议的“最终语义输出”层，也为流式 Decoder 的 terminal
//! snapshot 提供同一套校验器。输入是已经完整读取的非流式上游 JSON，或适配器
//! 在流式终点重新组装的等价 native JSON；输出是项目内部统一的 `Output`。
//! 它运行在 `proxy_native` 的异步请求任务中，不另建线程，也不拥有 HTTP body。
//!
//! `Output` 不是上游原始 JSON 的透明包装。它只保存客户端需要的 assistant
//! 文本、代理生成的客户端 tool id/name/arguments、reasoning、refusal、finish、usage，
//! 以及仅在连续工具调用间传递的不透明 replay 状态。真实上游 call_id、
//! thinking signature、加密 reasoning 等可进入进程内 replay，但不会被
//! `Output::json` 暴露给普通 Chat 客户端。
//!
//! 非流式路径必须先读 transport EOF、完整 JSON、协议终态和工具调用约束，
//! 才构造成功响应；usage 的缺失或 invalid 只影响统计质量，不单独否决模型正文。
use super::{
    Error,
    config::UpstreamProtocol,
    encode::{NativeCall, ReplayTurn},
    replay,
    request::{
        self, ChatRequest, Message, Part, Role, TOOL_ARGUMENT_LIMIT, TOOL_LIMIT, ToolCall,
        ToolChoice,
    },
    usage::{self, Usage},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
/// 项目统一后的 Chat `finish_reason`。
///
/// 供应商各自有不同字符串；适配器只把有明确映射的终态放入这里。未知状态应
/// 返回上游协议错误，不能默认成 `Stop`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finish {
    /// 模型正常结束且没有工具调用。
    Stop,
    /// 响应包含可执行工具调用。
    Tools,
    /// 达到输出 token 上限。
    Length,
    /// 内容被安全、拒答或其他 content filter 终止。
    Filter,
}
impl Finish {
    /// 转换成 OpenAI Chat 兼容字符串；流式控制帧与非流式响应共享该映射。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Tools => "tool_calls",
            Self::Length => "length",
            Self::Filter => "content_filter",
        }
    }
}
/// 一个客户端可见 Chat 响应的稳定身份。
///
/// 同一次流式响应的 role、text/tool delta、finish 和可选 usage frame 共用同一
/// Identity，使客户端能把它们归到同一个 completion。
#[derive(Clone, Debug)]
pub struct Identity {
    /// 以 `chatcmpl-wbp-` 开头的本地随机 ID，不是上游 message/response ID。
    pub id: String,
    /// Unix 时间戳，单位秒。
    pub created: i64,
    /// Chat chunk 使用的稳定模型名，初值为路由的上游模型。流式调用方只在首个
    /// role chunk 前用已知的上游模型更新它；终态才出现的模型不会改写已发 chunk。
    /// 非流式 `Output::json` 则直接优先使用 `Output.model`，无需修改此字段。
    pub model: String,
}
impl Identity {
    /// 为本响应生成一次性身份。调用时机由 HTTP 集成层决定，与 Parser、Decoder
    /// 的构造相互独立。
    pub fn new(model: &str) -> Self {
        Self {
            id: format!("chatcmpl-wbp-{}", crate::config::token()),
            created: crate::stats::now_ms() / 1000,
            model: model.into(),
        }
    }
    /// 构造普通 Chat chunk。`delta` 必须已经是 Chat 语义对象，可含 content、
    /// reasoning_content、refusal 或 tool_calls。
    pub fn chunk(&self, delta: Value) -> Value {
        json!({"id":self.id,"object":"chat.completion.chunk","created":self.created,"model":self.model,"choices":[{"index":0,"delta":delta,"finish_reason":null}]})
    }
    /// 构造只携带 `finish_reason` 的 Chat chunk；它不是 SSE `[DONE]`。
    pub fn finish_chunk(&self, finish: Finish) -> Value {
        json!({"id":self.id,"object":"chat.completion.chunk","created":self.created,"model":self.model,"choices":[{"index":0,"delta":{},"finish_reason":finish.as_str()}]})
    }
    /// 构造 choices 为空、usage 在顶层的 Chat usage chunk。
    ///
    /// 上游流式路径只在请求声明 `stream_options.include_usage=true` 且
    /// `Usage.complete` 时发出它，避免把未完成累计数冒充最终账单。
    pub fn usage_chunk(&self, usage: &Usage) -> Value {
        json!({"id":self.id,"object":"chat.completion.chunk","created":self.created,"model":self.model,"choices":[],"usage":usage.chat})
    }
}
/// 三种原生协议在项目内的统一语义结果。
///
/// 非流式调用直接返回本类型；流式 Decoder 在 transport EOF 和协议终态都
/// 通过后返回同一个类型，再交给 `proxy_native` 生成 finish、usage 和 DONE。
#[derive(Debug)]
pub struct Output {
    /// 统一 assistant 消息。text 位于 parts，tool calls 使用本地 `wbpt_` ID；
    /// replay 的 native ID 另存于 `ReplayTurn`。
    pub assistant: Message,
    /// 可公开给 Chat 兼容客户端的 reasoning 摘要；不包含签名或加密原块。
    pub reasoning: String,
    /// Responses `refusal` content part 的拒答文本；没有该字段时为 None。
    /// Anthropic/Gemini 的 Filter 终态可以没有可直接展示的拒答正文。
    pub refusal: Option<String>,
    /// 已规范化终态；`blank` 的 Stop 只是构造默认值。携带普通 finish reason
    /// 的协议分支会调用 `finish` 映射，Gemini 安全阻断等分支则可直接设置
    /// Filter；所有分支都必须遵守各自协议的合法终态规则。
    pub finish: Finish,
    /// 规范化 usage；可能 Missing、Partial 或 Invalid，不影响正文是否成功。
    pub usage: Usage,
    /// 工具调用后续轮次需要的进程内不透明原生 assistant state。没有工具调用
    /// 时为 None；不能直接序列化给普通客户端。
    pub replay: Option<ReplayTurn>,
    /// 上游终态报告的模型名；必须通过长度和控制字符校验。None 时使用
    /// `Identity.model`，即当前路由配置的上游模型，不保证等于客户端请求里的别名。
    pub model: Option<String>,
}
impl Output {
    /// 构造非流式 OpenAI Chat completion JSON。
    ///
    /// 调用方是非流式 `proxy::native::relay`，或流式 body 之外的完整测试路径。
    /// 工具调用只输出本地 ID、函数名与 JSON 字符串参数；reasoning、refusal
    /// 和 replay 私密字段不会混入 tool arguments。usage 只要不是 Null 就附带，
    /// 因此非流式响应可以展示已知的部分计数；`complete=false` 表示统计口径
    /// 不完整，但 JSON 仍可携带已证明字段。
    pub fn json(&self, identity: &Identity) -> Value {
        let text = self.assistant.text();
        let mut message = json!({"role":"assistant","content":if text.is_empty() && !self.assistant.calls.is_empty(){Value::Null}else{json!(text)}});
        if !self.reasoning.is_empty() {
            message["reasoning_content"] = json!(self.reasoning);
        }
        if let Some(refusal) = &self.refusal {
            message["refusal"] = json!(refusal);
        }
        if !self.assistant.calls.is_empty() {
            message["tool_calls"]=json!(self.assistant.calls.iter().map(|c|json!({"id":c.id,"type":"function","function":{"name":c.name,"arguments":c.arguments_text}})).collect::<Vec<_>>());
        }
        let mut out = json!({"id":identity.id,"object":"chat.completion","created":identity.created,"model":self.model.as_deref().unwrap_or(&identity.model),"choices":[{"index":0,"message":message,"finish_reason":self.finish.as_str()}]});
        if !self.usage.chat.is_null() {
            out["usage"] = self.usage.chat.clone();
        }
        out
    }
    /// 按原 Chat 请求的工具选择约束检查最终输出。
    ///
    /// 该方法由 `proxy::native::pending` 在非流式完整解析后、流式
    /// `Decoder::finish` 返回后调用，并非解析器每收到一个 delta 就执行。调用方
    /// 随后才构造 replay `Pending`：禁止工具时出现 tool call、禁用并行时出现
    /// 多个 call、调用未声明/不匹配 named choice，均使整个响应失败。只有
    /// `Finish::Stop` 且没有 call 时，required/named choice 才会触发
    /// `missing_required_tool_call`；Length/Filter 的“无 call”由各协议终态规则
    /// 另行处理，不能在本函数里一律判成缺工具。
    pub fn validate_request(&self, r: &ChatRequest) -> Result<(), Error> {
        if !self.assistant.calls.is_empty() {
            if r.tool_choice == ToolChoice::None
                || r.parallel_tool_calls == Some(false) && self.assistant.calls.len() > 1
            {
                return Err(bad("unexpected_tool_call"));
            }
            for call in &self.assistant.calls {
                if !r.tools.iter().any(|t| t.name == call.name)
                    || matches!(&r.tool_choice,ToolChoice::Named(name) if name!=&call.name)
                {
                    return Err(bad("unexpected_tool_call"));
                }
            }
        } else if self.finish == Finish::Stop
            && matches!(r.tool_choice, ToolChoice::Required | ToolChoice::Named(_))
        {
            return Err(bad("missing_required_tool_call"));
        }
        Ok(())
    }
}
/// 构造不含任何用户输入的静态上游协议错误。
pub fn bad(code: &'static str) -> Error {
    Error::upstream(code)
}
/// 要求 JSON 对象字段存在且为字符串，并借用原 Value 中的文本。
///
/// 新 Rust 读者可把返回的 `&str` 理解为“函数结束后仍随原 Value 存活的一把
/// 只读引用”；原 `Value` 被移动或修改的边界由调用方借用检查器保证。
pub fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, Error> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| bad("invalid_upstream_response"))
}
/// 解析非流式上游 body 为统一 Output。
///
/// `strict_json` 在 16 MiB 上限内完成 UTF-8/JSON 严格解析；失败统一映射为
/// `invalid_upstream_json`。调用方必须先确认 HTTP 成功、content-type 为 JSON
/// 且 transport 已到 EOF，不能把只收到半段 body 的字节交给本函数并等待重试。
pub fn parse_json(protocol: UpstreamProtocol, bytes: &[u8]) -> Result<Output, Error> {
    let native =
        request::strict_json(bytes, OUTPUT_LIMIT).map_err(|_| bad("invalid_upstream_json"))?;
    decode(protocol, native)
}
/// 按已解析的 native JSON 选择协议解码器。
///
/// 顶层必须为对象且不能携带非 null `error`。OpenAI Chat 已是原生字节路径，
/// 在这里被明确拒绝；三个 native 协议分支各自检查自己的终态、内容和工具字段。
pub fn decode(protocol: UpstreamProtocol, native: Value) -> Result<Output, Error> {
    if !native.is_object() || native.get("error").is_some_and(|v| !v.is_null()) {
        return Err(bad("upstream_error"));
    }
    match protocol {
        UpstreamProtocol::OpenaiResponses => responses(native),
        UpstreamProtocol::AnthropicMessages => anthropic(native),
        UpstreamProtocol::GeminiGenerateContent => gemini(native),
        UpstreamProtocol::OpenaiChat => Err(bad("invalid_protocol")),
    }
}
/// 流式/增量构造时的空状态。
///
/// Stop 是类型需要的占位值，不代表空正文已经成功结束；最终必须由协议 finish
/// 字段重新赋值。usage 默认 Null/不完整，replay 默认 None。
fn blank() -> Output {
    Output {
        assistant: Message {
            role: Role::Assistant,
            parts: vec![],
            calls: vec![],
            tool_call_id: None,
        },
        reasoning: String::new(),
        refusal: None,
        finish: Finish::Stop,
        usage: Usage::default(),
        replay: None,
        model: None,
    }
}
/// 提取合法模型名：非空、最多 256 字节且不含控制字符。
fn model(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control))
        .map(String::from)
}
/// 仅当最后一个 part 是 Text 时追加文本；否则为非空文本新建一个 Text part。
///
/// tool call 和 refusal 分别保存在 `assistant.calls` / `refusal`，不会向
/// `assistant.parts` 插入边界占位，因此本函数不承担“遇到它们就另开 part”的语义。
fn append(out: &mut Output, s: &str) {
    if let Some(Part::Text(old)) = out.assistant.parts.last_mut() {
        old.push_str(s);
    } else if !s.is_empty() {
        out.assistant.parts.push(Part::Text(s.into()));
    }
}
/// 将供应商函数调用转换为项目 Chat ToolCall，并登记 replay 所需的 native ID。
///
/// 函数名必须为 64 字节内 ASCII 标识符，参数必须是 JSON object。每个客户端
/// tool id 都由 `replay::tool_id()` 新建，`mapping` 把它映射回供应商 call_id
/// 或空 ID；因此不同协议的 ID 不会直接混入客户端历史。
fn tool(
    out: &mut Output,
    mapping: &mut BTreeMap<String, NativeCall>,
    native_id: Option<&str>,
    name: &str,
    args: Value,
) -> Result<(), Error> {
    if out.assistant.calls.len() >= TOOL_LIMIT
        || name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        || !args.is_object()
    {
        return Err(bad("invalid_upstream_tool_call"));
    }
    if let Some(id) = native_id
        && (id.is_empty() || id.len() > 256 || mapping.values().any(|c| c.id == id))
    {
        return Err(bad("invalid_upstream_tool_id"));
    }
    let arguments_text =
        serde_json::to_string(&args).map_err(|_| bad("invalid_upstream_tool_arguments"))?;
    if arguments_text.len() > TOOL_ARGUMENT_LIMIT {
        return Err(bad("upstream_tool_arguments_too_large"));
    }
    let id = replay::tool_id();
    mapping.insert(
        id.clone(),
        NativeCall {
            id: native_id.unwrap_or("").into(),
            name: name.into(),
        },
    );
    out.assistant.calls.push(ToolCall {
        id,
        name: name.into(),
        arguments: args,
        arguments_text,
    });
    Ok(())
}
/// 把供应商 finish reason 映射成统一终态。
///
/// 普通 stop/completed/end_turn 有工具时提升为 Tools；tool_use 必须有调用；
/// max token 映射 Length；安全/拒答原因映射 Filter。未知字符串直接报错。
/// Length/Filter 同时带工具调用会被拒绝，防止把半截参数当可执行 JSON。
fn finish(out: &mut Output, reason: &str) -> Result<(), Error> {
    out.finish = match reason {
        "stop" | "completed" | "end_turn" | "stop_sequence" | "STOP" => {
            if out.assistant.calls.is_empty() {
                Finish::Stop
            } else {
                Finish::Tools
            }
        }
        "tool_use" => {
            if out.assistant.calls.is_empty() {
                return Err(bad("invalid_upstream_finish"));
            }
            Finish::Tools
        }
        "max_tokens" | "max_output_tokens" | "MAX_TOKENS" => Finish::Length,
        "content_filter" | "refusal" | "SAFETY" | "RECITATION" | "BLOCKLIST"
        | "PROHIBITED_CONTENT" | "SPII" => Finish::Filter,
        _ => return Err(bad("unsupported_upstream_finish")),
    };
    // Incomplete tools must never reach the client with executable arguments.
    if matches!(out.finish, Finish::Length | Finish::Filter) && !out.assistant.calls.is_empty() {
        return Err(bad("incomplete_upstream_tool_call"));
    }
    Ok(())
}
/// 仅在有工具调用时保存不透明 replay native state。
///
/// 无工具响应直接丢弃 native 包装；工具响应保留原始 assistant 结构和
/// `wbpt_` -> native ID/name 映射，供下一个 Chat 请求还原上游协议。
fn replay(out: &mut Output, native: Value, calls: BTreeMap<String, NativeCall>) {
    if !calls.is_empty() {
        out.replay = Some(ReplayTurn {
            assistant: native,
            calls,
        });
    }
}

/// 解码 OpenAI Responses 非流式响应。
///
/// 要求顶层 status 为 completed/incomplete。completed 中 refusal 文本会映射
/// Filter；incomplete 必须给出可映射 reason。output 数组只接受 message、
/// reasoning 和 function_call；item id 若为字符串会在此检查重复，native call id
/// 的长度/唯一性由 `tool()` 校验。函数
/// 参数是 JSON 字符串并再次严格解析为对象，未知 output item 一律失败，不能
/// 跳过并伪造成功。
fn responses(native: Value) -> Result<Output, Error> {
    let mut out = blank();
    out.model = model(&native, "model");
    out.usage = usage::normalize(UpstreamProtocol::OpenaiResponses, &native["usage"]);
    let status = text(&native, "status")?;
    if !matches!(status, "completed" | "incomplete") {
        return Err(bad("upstream_response_failed"));
    }
    let items = native
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("invalid_upstream_response"))?;
    let mut calls = BTreeMap::new();
    let mut item_ids = BTreeSet::new();
    for item in items {
        if let Some(id) = item.get("id").and_then(Value::as_str)
            && !item_ids.insert(id)
        {
            return Err(bad("duplicate_upstream_item"));
        }
        match text(item, "type")? {
            "message" => {
                if text(item, "role")? != "assistant" {
                    return Err(bad("invalid_upstream_role"));
                }
                for part in item
                    .get("content")
                    .and_then(Value::as_array)
                    .ok_or_else(|| bad("invalid_upstream_content"))?
                {
                    match text(part, "type")? {
                        "output_text" => append(&mut out, text(part, "text")?),
                        "refusal" => {
                            let refusal = text(part, "refusal")?;
                            out.refusal
                                .get_or_insert_with(String::new)
                                .push_str(refusal);
                        }
                        _ => return Err(bad("unsupported_upstream_content")),
                    }
                }
            }
            "reasoning" => {
                if let Some(summary) = item.get("summary") {
                    for part in summary
                        .as_array()
                        .ok_or_else(|| bad("invalid_upstream_reasoning"))?
                    {
                        if text(part, "type")? != "summary_text" {
                            return Err(bad("unsupported_upstream_reasoning"));
                        }
                        out.reasoning.push_str(text(part, "text")?);
                    }
                }
            }
            "function_call" => {
                let args = text(item, "arguments")?;
                let args = request::strict_json(args.as_bytes(), TOOL_ARGUMENT_LIMIT)
                    .map_err(|_| bad("invalid_upstream_tool_arguments"))?;
                tool(
                    &mut out,
                    &mut calls,
                    Some(text(item, "call_id")?),
                    text(item, "name")?,
                    args,
                )?;
            }
            _ => return Err(bad("unsupported_upstream_output")),
        }
    }
    let reason = if status == "completed" {
        if out.refusal.is_some() {
            "refusal"
        } else {
            "completed"
        }
    } else {
        native["incomplete_details"]["reason"]
            .as_str()
            .ok_or_else(|| bad("invalid_upstream_finish"))?
    };
    finish(&mut out, reason)?;
    replay(&mut out, Value::Array(items.clone()), calls);
    Ok(out)
}
/// 解码 Anthropic Messages 非流式响应。
///
/// 顶层必须是 assistant message。text 公开；thinking block 只读取
/// `thinking.thinking` 作为 reasoning，并校验 signature 存在。signature 和
/// redacted_thinking data 只留在 replay。tool_use.input 必须
/// 是 object，stop_reason 经统一 `finish` 映射。未知 block 类型不会被静默忽略。
fn anthropic(native: Value) -> Result<Output, Error> {
    if text(&native, "type")? != "message" || text(&native, "role")? != "assistant" {
        return Err(bad("invalid_upstream_response"));
    }
    let mut out = blank();
    out.model = model(&native, "model");
    out.usage = usage::normalize(UpstreamProtocol::AnthropicMessages, &native["usage"]);
    let blocks = native
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("invalid_upstream_content"))?;
    let mut calls = BTreeMap::new();
    for block in blocks {
        match text(block, "type")? {
            "text" => append(&mut out, text(block, "text")?),
            "thinking" => {
                out.reasoning.push_str(text(block, "thinking")?);
                text(block, "signature")?;
            }
            "redacted_thinking" => {
                text(block, "data")?;
            }
            "tool_use" => tool(
                &mut out,
                &mut calls,
                Some(text(block, "id")?),
                text(block, "name")?,
                block["input"].clone(),
            )?,
            _ => return Err(bad("unsupported_upstream_content")),
        }
    }
    finish(&mut out, text(&native, "stop_reason")?)?;
    replay(&mut out, Value::Array(blocks.clone()), calls);
    Ok(out)
}
/// 解码 Gemini GenerateContent 非流式响应。
///
/// 正常响应必须只有一个 index=0 candidate；无 candidates 时只接受明确
/// promptFeedback.blockReason 并映射为 Filter。每个 part 只允许 text、thought、
/// thoughtSignature、functionCall 的已审核组合；functionCall.args 缺省视为空
/// object，但若出现就必须是 object。thoughtSignature 留在 replay，不进入客户端。
fn gemini(native: Value) -> Result<Output, Error> {
    let mut out = blank();
    out.model = model(&native, "modelVersion");
    out.usage = usage::normalize(
        UpstreamProtocol::GeminiGenerateContent,
        &native["usageMetadata"],
    );
    let candidates = native.get("candidates").and_then(Value::as_array);
    if candidates.is_none_or(|v| v.is_empty()) {
        let reason = native["promptFeedback"]["blockReason"]
            .as_str()
            .ok_or_else(|| bad("invalid_upstream_response"))?;
        finish(&mut out, reason)?;
        if out.finish != Finish::Filter {
            return Err(bad("invalid_upstream_response"));
        }
        return Ok(out);
    }
    let candidates = candidates.expect("checked nonempty");
    if candidates.len() != 1
        || candidates[0]
            .get("index")
            .is_some_and(|i| i.as_u64() != Some(0))
    {
        return Err(bad("multiple_upstream_candidates"));
    }
    let c = &candidates[0];
    let mut calls = BTreeMap::new();
    if let Some(content) = c.get("content") {
        if content.get("role").is_some_and(|r| r != "model") {
            return Err(bad("invalid_upstream_role"));
        }
        for part in content
            .get("parts")
            .and_then(Value::as_array)
            .ok_or_else(|| bad("invalid_upstream_content"))?
        {
            let p = part
                .as_object()
                .ok_or_else(|| bad("invalid_upstream_content"))?;
            if p.keys().any(|k| {
                !matches!(
                    k.as_str(),
                    "text" | "thought" | "thoughtSignature" | "functionCall"
                )
            }) {
                return Err(bad("unsupported_upstream_content"));
            }
            if part.get("thought").is_some_and(|v| !v.is_boolean())
                || part.get("thoughtSignature").is_some_and(|v| !v.is_string())
            {
                return Err(bad("invalid_upstream_content"));
            }
            let mut seen = false;
            if let Some(s) = part.get("text") {
                let s = s.as_str().ok_or_else(|| bad("invalid_upstream_content"))?;
                if part["thought"] == true {
                    out.reasoning.push_str(s);
                } else {
                    append(&mut out, s);
                }
                seen = true;
            }
            if let Some(call) = part.get("functionCall") {
                if seen || part["thought"] == true {
                    return Err(bad("invalid_upstream_tool_call"));
                }
                let id = call
                    .get("id")
                    .map(|v| v.as_str().ok_or_else(|| bad("invalid_upstream_tool_id")))
                    .transpose()?;
                tool(
                    &mut out,
                    &mut calls,
                    id,
                    text(call, "name")?,
                    call.get("args").cloned().unwrap_or_else(|| json!({})),
                )?;
                seen = true;
            }
            if !seen && !part.get("thoughtSignature").is_some_and(Value::is_string) {
                return Err(bad("unsupported_upstream_content"));
            }
        }
    }
    finish(&mut out, text(c, "finishReason")?)?;
    replay(&mut out, c["content"].clone(), calls);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn responses_preserves_refusal_reasoning_and_parallel_call_identity() {
        let native = json!({"status":"completed","output":[{"type":"reasoning","summary":[{"type":"summary_text","text":"summary"}],"encrypted_content":"opaque"},
            {"type":"message","role":"assistant","content":[{"type":"output_text","text":"checking"}]},
            {"type":"function_call","call_id":"a","name":"find","arguments":"{\"q\":1}"},{"type":"function_call","call_id":"b","name":"find","arguments":"{\"q\":2}"}],"usage":{"input_tokens":10,"output_tokens":3,"total_tokens":13}});
        let out = decode(UpstreamProtocol::OpenaiResponses, native.clone()).unwrap();
        assert_eq!(out.finish, Finish::Tools);
        assert_eq!(out.reasoning, "summary");
        assert_eq!(out.assistant.text(), "checking");
        assert_eq!(out.assistant.calls.len(), 2);
        assert_ne!(out.assistant.calls[0].id, out.assistant.calls[1].id);
        assert!(out.assistant.calls[0].id.starts_with("wbpt_"));
        assert_eq!(out.replay.as_ref().unwrap().assistant, native["output"]);
        assert!(!out.json(&Identity::new("m")).to_string().contains("opaque"));
        let refusal=decode(UpstreamProtocol::OpenaiResponses,json!({"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"refusal","refusal":"no"}]}]})).unwrap();
        assert_eq!(refusal.finish, Finish::Filter);
    }
    #[test]
    fn anthropic_opaque_blocks_stay_private_and_usage_is_disjoint() {
        let native = json!({"type":"message","role":"assistant","model":"claude-test","stop_reason":"tool_use","content":[{"type":"thinking","thinking":"summary","signature":"secret-signature"},{"type":"redacted_thinking","data":"secret-redaction"},{"type":"tool_use","id":"native","name":"f","input":{}}],"usage":{"input_tokens":10,"cache_read_input_tokens":20,"cache_creation_input_tokens":5,"output_tokens":4}});
        let out = decode(UpstreamProtocol::AnthropicMessages, native.clone()).unwrap();
        assert_eq!(out.usage.chat["prompt_tokens"], 35);
        assert_eq!(out.replay.as_ref().unwrap().assistant, native["content"]);
        let value = out.json(&Identity::new("fallback")).to_string();
        assert!(!value.contains("secret-"));
        assert!(value.contains("summary"));
    }
    #[test]
    fn gemini_keeps_signatures_on_text_and_tools_and_rejects_extra_outputs() {
        let native = json!({"candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"checking","thoughtSignature":"on-text"},{"functionCall":{"name":"f","args":{}},"thoughtSignature":"on-tool"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":2,"thoughtsTokenCount":3,"totalTokenCount":15}});
        let out = decode(UpstreamProtocol::GeminiGenerateContent, native.clone()).unwrap();
        assert_eq!(out.finish, Finish::Tools);
        assert_eq!(
            out.replay.as_ref().unwrap().assistant,
            native["candidates"][0]["content"]
        );
        assert_eq!(out.usage.chat["completion_tokens"], 5);
        let blocked = decode(
            UpstreamProtocol::GeminiGenerateContent,
            json!({"promptFeedback":{"blockReason":"SAFETY"}}),
        )
        .unwrap();
        assert_eq!(blocked.finish, Finish::Filter);
        assert!(decode(UpstreamProtocol::GeminiGenerateContent,json!({"candidates":[{"finishReason":"STOP","content":{"parts":[{"inlineData":{"mimeType":"image/png","data":"x"}}]}}]})).is_err());
    }
    #[test]
    fn malformed_error_unknown_tools_and_incomplete_states_never_succeed() {
        for native in [
            json!({"error":{"message":"credential"}}),
            json!({"status":"in_progress","output":[]}),
            json!({"status":"completed","output":[{"type":"function_call","call_id":"a","name":"f","arguments":"[]"}]}),
            json!({"status":"completed","output":[{"type":"web_search_call"}]}),
            json!({"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"output":[{"type":"function_call","call_id":"a","name":"f","arguments":"{}"}]}),
        ] {
            assert!(decode(UpstreamProtocol::OpenaiResponses, native).is_err());
        }
        assert!(parse_json(UpstreamProtocol::OpenaiResponses, b"{broken").is_err());
        let length=decode(UpstreamProtocol::OpenaiResponses,json!({"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"output":[]})).unwrap();
        assert_eq!(length.finish, Finish::Length);
    }
}

//! 把已校验的统一 [`ChatRequest`] 编码为 native 上游请求 JSON。
//!
//! 上游调用方是 `proxy::native::prepare`。调用前，它已经把客户端 model 标识解析成
//! `Route` 并完成模型能力检查；这里接收路由携带的真实上游 `model`、协议设置和由
//! 进程内 replay cache 解析出的 opaque assistant 历史。这里只返回 `Value`；随后同
//! 一个 `prepare` 函数用 `serde_json::to_writer` 和有界 `Limited` 把它序列化成
//! `Bytes`，`proxy::native::open` 只负责拿到这段 body 后发 HTTP。因此本模块没有网络、
//! 超时、取消或统计副作用。
//!
//! 输入请求只被借用，输出是一个新 JSON 值；replay 映射通过 `Arc<ReplayTurn>` 持有
//! 不可变历史，克隆 Arc 只增加共享句柄，不会复制整份历史。编码原生 JSON 时仍可能
//! 克隆其中的 Value，因此不能把整个请求转换理解为零复制。客户端
//! JSON 不能直接提供这些私有 replay 字段，否则绕过历史绑定。各协议转换采用显式
//! 字段映射和保守失败：无法表达的图片、工具选择、schema 约束或思考组合会返回
//! `protocol::Error`，而不是被静默丢弃或弱化。
use super::{
    Error,
    config::{ProtocolSettings, ReasoningMode, UpstreamProtocol},
    request::{ChatRequest, Message, Part, Role, ToolChoice},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

/// replay 中保存的一条 native 工具调用身份。
///
/// Chat 客户端可能只看到代理生成的 `wbpt_*` ID；上游真正的 call ID 和工具名保存在
/// 这里。后续工具结果编码时通过它恢复原生 `call_id`/`tool_use_id`，从而保持
/// Anthropic、Gemini 对历史调用的关联。
#[derive(Clone, Debug)]
pub struct NativeCall {
    pub id: String,
    pub name: String,
}

/// 一轮 assistant 响应中需要原样 replay 的私有状态。
///
/// `assistant` 保存协议原生的 assistant content/output blocks，可能包含加密思考、
/// signature 或供应商生成的块；`calls` 把公开 Chat ID 映射回 native 调用。两者一起
/// 才能让多轮工具对话延续原协议状态，单独转发文本不足以重建这种历史。
#[derive(Clone, Debug)]
pub struct ReplayTurn {
    /// Responses: output array; Anthropic: content blocks; Gemini: content.
    pub assistant: Value,
    pub calls: BTreeMap<String, NativeCall>,
}

/// 历史消息位置到不可变 replay turn 的映射。
///
/// 键是 `ChatRequest.messages` 中 assistant 消息的索引。`Arc` 只共享同一份已提交
/// replay 数据；它还允许 cache 条目被淘汰后，正在进行中的请求继续安全持有该值，
/// 不会因缓存索引删除而出现悬空引用。克隆整个映射会克隆键和 `Arc` 句柄，不会按
/// 语义自动深拷贝 `ReplayTurn`。
pub type ResolvedHistory = BTreeMap<usize, Arc<ReplayTurn>>;

/// 按所选 native 协议编码一次请求。
///
/// 输入中的 `model` 来自路由解析结果，而不是 [`ChatRequest`]；`settings` 必须先与
/// `protocol` 一致，`history` 则由 replay cache 根据客户端历史解析。输出是尚未
/// 序列化和发送的供应商 JSON。
///
/// 函数先执行 [`ProtocolSettings::validate`]，再分派到 Responses、Anthropic 或
/// Gemini 编码器；OpenAI Chat 不属于 native 路径，会返回 `invalid_protocol`。这里
/// 不提交 replay，也不记录统计终态；结构化错误要到 `proxy::native::reject` 才转成
/// 统计和 HTTP failure。
pub fn encode(
    protocol: UpstreamProtocol,
    model: &str,
    settings: &ProtocolSettings,
    request: &ChatRequest,
    history: &ResolvedHistory,
) -> Result<Value, Error> {
    settings
        .validate(protocol)
        .map_err(|_| Error::input("invalid_protocol_settings", "protocol_settings"))?;
    match protocol {
        UpstreamProtocol::OpenaiResponses => responses(model, settings, request, history),
        UpstreamProtocol::AnthropicMessages => anthropic(model, settings, request, history),
        UpstreamProtocol::GeminiGenerateContent => gemini(settings, request, history),
        UpstreamProtocol::OpenaiChat => Err(Error::input("invalid_protocol", "protocol")),
    }
}

/// 把 Chat 工具 ID 恢复成 replay 保存的 native ID。
///
/// 已知调用使用 `calls` 中的映射；未知但以 `wbpt_` 开头的 ID 被视为代理生成的
/// 状态已过期，返回 409 `conversation_state_expired`。其他未知 ID 原样返回，以支持
/// 客户端自己构造的非代理工具历史。函数只查表，不修改 replay cache。
fn call_id<'a>(calls: &'a BTreeMap<String, NativeCall>, id: &'a str) -> Result<&'a str, Error> {
    if let Some(call) = calls.get(id) {
        return Ok(&call.id);
    }
    if id.starts_with("wbpt_") {
        return Err(Error {
            code: "conversation_state_expired",
            param: None,
            status: 409,
        });
    }
    Ok(id)
}

/// 把一轮消息中的公开调用 ID 登记到后续工具结果可使用的映射。
///
/// 有 replay 时优先复制 replay 的完整身份映射；否则把客户端声明的 ID、name 作为
/// native 身份。`extend` 会覆盖同键旧值，但跨消息 ID 唯一性已在
/// [`ChatRequest::parse`] 的历史校验中保证。
fn remember(
    calls: &mut BTreeMap<String, NativeCall>,
    message: &Message,
    replay: Option<&ReplayTurn>,
) {
    if let Some(replay) = replay {
        calls.extend(replay.calls.clone());
    } else {
        calls.extend(message.calls.iter().map(|c| {
            (
                c.id.clone(),
                NativeCall {
                    id: c.id.clone(),
                    name: c.name.clone(),
                },
            )
        }));
    }
}

/// 把统一文本/图片部分映射为 Responses input content。
///
/// data URL 会从 MIME 与 base64 文本重新组合；远程 URL 保持原地址；`detail` 原样
/// 传递。该函数只用于非 assistant 的消息 content。图片内容是否允许已在调用方的
/// 模型能力检查与请求解析中决定。
fn response_part(part: &Part) -> Value {
    match part {
        Part::Text(text) => json!({"type":"input_text","text":text}),
        Part::ImageData { mime, data, detail } => {
            json!({"type":"input_image","image_url":format!("data:{mime};base64,{data}"),"detail":detail})
        }
        Part::ImageUrl { url, detail } => {
            json!({"type":"input_image","image_url":url,"detail":detail})
        }
    }
}

/// 构造 OpenAI Responses 请求体。
///
/// 输入是路由的真实模型、协议设置、统一请求和已解析 replay。输出把所有历史展平成
/// `input` item 数组：普通用户/系统/开发者消息保留结构化 content，普通 assistant
/// 文本使用没有 native ID 的 role/content 形式，工具调用成为 `function_call`，
/// 工具结果成为 `function_call_output`。存在 replay 的 assistant 位置则展开原始
/// output blocks，确保思考状态和原生调用身份不丢失。
///
/// `store` 固定为 false，并请求 `reasoning.encrypted_content`，以便客户端后续历史
/// 能在不依赖供应商 server-side conversation 的情况下继续。`stop` 当前不受支持。
/// 工具 schema 仅通过保守的 [`schema`] 子集检查；不会声称做过完整 JSON Schema
/// 验证。失败返回 400 输入/能力错误或 409 replay 过期错误，不发送请求。
fn responses(
    model: &str,
    settings: &ProtocolSettings,
    r: &ChatRequest,
    history: &ResolvedHistory,
) -> Result<Value, Error> {
    if !r.stop.is_empty() {
        return Err(Error::input("unsupported_parameter", "stop"));
    }
    let mut input = Vec::new();
    let mut calls: BTreeMap<String, NativeCall> = BTreeMap::new();
    // 阶段一：按原消息顺序生成 input items，并用已解析 replay 的 assistant 块替换
    // 可重建的普通历史；后续工具结果通过 calls 恢复 native ID。
    for (i, m) in r.messages.iter().enumerate() {
        let replay = history.get(&i).map(Arc::as_ref);
        match m.role {
            Role::Tool=>input.push(json!({"type":"function_call_output",
                "call_id":call_id(&calls,m.tool_call_id.as_deref().expect("validated tool ID"))?,"output":m.text()})),
            _=>{
                if let Some(replay)=replay {
                    let items=replay.assistant.as_array().ok_or_else(|| Error::upstream("invalid_replay_state"))?;
                    input.extend(items.iter().cloned());
                } else {
                    if !m.parts.is_empty() {
                        let role=match m.role {Role::System=>"system",Role::Developer=>"developer",Role::User=>"user",_=>"assistant"};
                        // Ordinary assistant text uses EasyInputMessage. OutputMessage
                        // requires a native ID and is only used for cached replay.
                        let message=if m.role==Role::Assistant {
                            json!({"role":role,"content":m.text()})
                        } else {json!({"role":role,"content":m.parts.iter().map(response_part).collect::<Vec<_>>()})};
                        input.push(message);
                    }
                    for call in &m.calls {
                        input.push(json!({"type":"function_call","call_id":call_id(&calls,&call.id)?,
                            "name":call.name,"arguments":call.arguments_text}));
                    }
                }
                remember(&mut calls,m,replay);
            }
        }
    }
    // 阶段二：只写入客户端实际提供的生成参数。store/include 是当前适配器固定的
    // Responses 语义；未提供的 None 不会被默认值伪造。
    let mut out = json!({"model":model,"input":input,"stream":r.stream,"store":false,"include":["reasoning.encrypted_content"]});
    if let Some(max) = r.max_output_tokens {
        out["max_output_tokens"] = json!(max);
    }
    if let Some(v) = r.temperature {
        out["temperature"] = json!(v);
    }
    if let Some(v) = r.top_p {
        out["top_p"] = json!(v);
    }
    if let Some(effort) = &r.reasoning_effort {
        let native = settings
            .responses_efforts
            .get(effort)
            .map(String::as_str)
            .unwrap_or(effort);
        if native == "max" {
            return Err(Error::input(
                "unsupported_reasoning_effort",
                "reasoning_effort",
            ));
        }
        out["reasoning"] = json!({"effort":native});
    }
    if !r.tools.is_empty() {
        let mut tools = vec![];
        for tool in &r.tools {
            schema(
                &tool.parameters,
                UpstreamProtocol::OpenaiResponses,
                "tools.function.parameters",
            )?;
            let mut value = json!({"type":"function","name":tool.name,"parameters":tool.parameters,"strict":tool.strict.unwrap_or(false)});
            if let Some(description) = &tool.description {
                value["description"] = json!(description);
            }
            tools.push(value);
        }
        out["tools"] = json!(tools);
        out["tool_choice"] = match &r.tool_choice {
            ToolChoice::Auto => json!("auto"),
            ToolChoice::None => json!("none"),
            ToolChoice::Required => json!("required"),
            ToolChoice::Named(name) => json!({"type":"function","name":name}),
        };
        if let Some(v) = r.parallel_tool_calls {
            out["parallel_tool_calls"] = json!(v);
        }
    }
    // 阶段三：工具与 structured output 都先经过保守 schema 子集检查。
    if let Some(format) = &r.response_format {
        let mapped = match format["type"].as_str().expect("validated response format") {
            "json_schema" => {
                let mut value = format["json_schema"].clone();
                schema(
                    &value["schema"],
                    UpstreamProtocol::OpenaiResponses,
                    "response_format.json_schema.schema",
                )?;
                value["type"] = json!("json_schema");
                value
            }
            _ => format.clone(),
        };
        out["text"] = json!({"format":mapped});
    }
    Ok(out)
}

/// 把文本/图片部分映射成 Anthropic 或 Gemini 的非 assistant content。
///
/// 文本分别转成 Anthropic text block 或 Gemini `{"text":...}`。base64 图片原样转成
/// 两家各自结构；URL 图片可进入 Anthropic，但 Gemini 当前拒绝。两家在这里都只接受
/// `detail == "auto"`；`low`/`high` 无法无损表达时直接失败，不做静默降级。
fn nonresponses_parts(parts: &[Part], protocol: UpstreamProtocol) -> Result<Vec<Value>, Error> {
    parts
        .iter()
        .map(|p| match p {
            Part::Text(text) => Ok(if protocol == UpstreamProtocol::AnthropicMessages {
                json!({"type":"text","text":text})
            } else {
                json!({"text":text})
            }),
            Part::ImageData { mime, data, detail } => {
                if detail != "auto" {
                    return Err(Error::input(
                        "unsupported_image_detail",
                        "messages.content.image_url.detail",
                    ));
                }
                Ok(if protocol == UpstreamProtocol::AnthropicMessages {
                    json!({"type":"image","source":{"type":"base64","media_type":mime,"data":data}})
                } else {
                    json!({"inlineData":{"mimeType":mime,"data":data}})
                })
            }
            Part::ImageUrl { url, detail } => {
                if protocol == UpstreamProtocol::GeminiGenerateContent {
                    return Err(Error::input(
                        "unsupported_image_url",
                        "messages.content.image_url.url",
                    ));
                }
                if detail != "auto" {
                    return Err(Error::input(
                        "unsupported_image_detail",
                        "messages.content.image_url.detail",
                    ));
                }
                Ok(json!({"type":"image","source":{"type":"url","url":url}}))
            }
        })
        .collect()
}

/// 提取对话开头连续的 system/developer 部分。
///
/// Anthropic 与 Gemini 只有一个独立 system 区域，因此统一历史一旦出现普通对话
/// 消息，后续再出现 system/developer 就返回 `unsupported_system_position`。全部消息
/// 都是 system/developer 则返回 `missing_conversation`。
///
/// 返回的两项分别是映射后的 system content 和已消费的消息数；调用方从 `first`
/// 开始编码剩余对话。该函数不改变原消息，也不决定供应商是否接受 system 内容类型。
fn system_prefix(
    r: &ChatRequest,
    protocol: UpstreamProtocol,
) -> Result<(Vec<Value>, usize), Error> {
    let mut prefix = vec![];
    let mut count = 0;
    let mut conversation = false;
    for (i, m) in r.messages.iter().enumerate() {
        if matches!(m.role, Role::System | Role::Developer) {
            if conversation {
                return Err(Error::input(
                    "unsupported_system_position",
                    format!("messages[{i}].role"),
                ));
            }
            prefix.extend(nonresponses_parts(&m.parts, protocol)?);
            count += 1;
        } else {
            conversation = true;
        }
    }
    if count == r.messages.len() {
        return Err(Error::input("missing_conversation", "messages"));
    }
    Ok((prefix, count))
}

/// 按上一轮调用顺序重排一组连续 tool 结果。
///
/// 请求校验允许客户端并行返回结果时乱序；Anthropic 和 Gemini 则需要按上一轮
/// assistant 的 `call_order` 排列，才能稳定建立调用与结果关系。`start` 必须是第一条
/// tool 消息，`end` 是连续 tool 组之后的位置；返回的消息引用借用原历史。
///
/// 多余、缺失或未知 ID 返回 `invalid_tool_history`。该函数只处理一组紧邻结果，
/// 不会跨过普通消息寻找匹配项。
fn ordered_results<'a>(
    messages: &'a [Message],
    start: usize,
    order: &[String],
) -> Result<(Vec<&'a Message>, usize), Error> {
    let end = start
        + messages[start..]
            .iter()
            .take_while(|m| m.role == Role::Tool)
            .count();
    let mut pending = messages[start..end]
        .iter()
        .filter_map(|m| m.tool_call_id.as_ref().map(|id| (id, m)))
        .collect::<BTreeMap<_, _>>();
    let mut results = Vec::new();
    for id in order {
        results.push(
            pending
                .remove(id)
                .ok_or_else(|| Error::input("invalid_tool_history", "messages.tool_call_id"))?,
        );
    }
    if !pending.is_empty() {
        return Err(Error::input(
            "invalid_tool_history",
            "messages.tool_call_id",
        ));
    }
    Ok((results, end))
}

/// 构造 Anthropic Messages 请求体。
///
/// 首部连续 system/developer 被合并到顶层 `system`；普通对话按 user/assistant
/// 重新组织，连续 tool 结果按上一轮调用顺序合并成一条 user `tool_result` content。
/// 若某轮存在 replay，则使用保存的 Anthropic content blocks，而不是根据 Chat 文本
/// 重建可能缺少 signature/thinking 的块。
///
/// 输出上限优先取客户端 `max_output_tokens`，没有时使用设置中的
/// `default_max_output_tokens`。思考模式、图片 detail、工具 choice 和 structured
/// output 都有独立兼容性规则；不支持的组合返回 400，不静默删除字段。
fn anthropic(
    model: &str,
    settings: &ProtocolSettings,
    r: &ChatRequest,
    history: &ResolvedHistory,
) -> Result<Value, Error> {
    let protocol = UpstreamProtocol::AnthropicMessages;
    let (system, first) = system_prefix(r, protocol)?;
    let mut messages: Vec<Value> = vec![];
    let mut calls: BTreeMap<String, NativeCall> = BTreeMap::new();
    let mut call_order = Vec::new();
    let mut i = first;
    // 阶段一：把消息历史转成 Anthropic user/assistant turn，工具结果单独合并。
    while i < r.messages.len() {
        let m = &r.messages[i];
        let replay = history.get(&i).map(Arc::as_ref);
        if m.role == Role::Tool {
            let mut results = vec![];
            let (ordered, end) = ordered_results(&r.messages, i, &call_order)?;
            for m in ordered {
                results.push(json!({"type":"tool_result","tool_use_id":call_id(&calls,m.tool_call_id.as_deref().expect("validated ID"))?,"content":m.text()}));
            }
            i = end;
            messages.push(json!({"role":"user","content":results}));
            continue;
        }
        let content = if let Some(replay) = replay {
            replay.assistant.clone()
        } else {
            let mut parts = nonresponses_parts(&m.parts, protocol)?;
            for call in &m.calls {
                parts.push(json!({"type":"tool_use","id":call_id(&calls,&call.id)?,"name":call.name,"input":call.arguments}));
            }
            json!(parts)
        };
        messages.push(
            json!({"role":if m.role==Role::Assistant {"assistant"}else{"user"},"content":content}),
        );
        remember(&mut calls, m, replay);
        call_order = m.calls.iter().map(|c| c.id.clone()).collect();
        i += 1;
    }
    // 阶段二：生成参数。temperature 与 top_p 可先写入，但 Anthropic 当前不允许两者
    // 同时出现，错误返回后整个 Value 不会发送。
    let max = r
        .max_output_tokens
        .unwrap_or(settings.default_max_output_tokens);
    let mut out = json!({"model":model,"max_tokens":max,"messages":messages,"stream":r.stream});
    if !system.is_empty() {
        out["system"] = json!(system);
    }
    if let Some(v) = r.temperature {
        out["temperature"] = json!(v);
    }
    if let Some(v) = r.top_p {
        out["top_p"] = json!(v);
    }
    if r.temperature.is_some() && r.top_p.is_some() {
        return Err(Error::input("conflicting_parameters", "temperature"));
    }
    if !r.stop.is_empty() {
        out["stop_sequences"] = json!(r.stop);
    }
    thinking(&mut out, settings, r, protocol, max)?;
    // 阶段三：工具声明、选择策略和 strict structured output 的协议映射。
    if !r.tools.is_empty() {
        let mut tools = vec![];
        for tool in &r.tools {
            schema(&tool.parameters, protocol, "tools.function.parameters")?;
            let mut value = json!({"name":tool.name,"input_schema":tool.parameters});
            if let Some(description) = &tool.description {
                value["description"] = json!(description);
            }
            if let Some(strict) = tool.strict {
                value["strict"] = json!(strict);
            }
            tools.push(value);
        }
        out["tools"] = json!(tools);
        let mut choice = match &r.tool_choice {
            ToolChoice::Auto => json!({"type":"auto"}),
            ToolChoice::None => json!({"type":"none"}),
            ToolChoice::Required => json!({"type":"any"}),
            ToolChoice::Named(name) => json!({"type":"tool","name":name}),
        };
        if r.parallel_tool_calls == Some(false) {
            choice["disable_parallel_tool_use"] = json!(true);
        }
        if out.get("thinking").is_some_and(|t| t["type"] != "disabled")
            && matches!(r.tool_choice, ToolChoice::Required | ToolChoice::Named(_))
        {
            return Err(Error::input(
                "unsupported_tool_choice_with_thinking",
                "tool_choice",
            ));
        }
        out["tool_choice"] = choice;
    }
    if let Some(format) = &r.response_format {
        match format["type"].as_str().expect("validated format") {
            "text" => (),
            "json_schema" => {
                schema(
                    &format["json_schema"]["schema"],
                    protocol,
                    "response_format.json_schema.schema",
                )?;
                // Anthropic structured output is strict. Do not upgrade a
                // request that explicitly opts out of strict validation.
                if format["json_schema"]["strict"] == false {
                    return Err(Error::input(
                        "unsupported_parameter",
                        "response_format.json_schema.strict",
                    ));
                }
                out["output_config"]["format"] =
                    json!({"type":"json_schema","schema":format["json_schema"]["schema"]});
            }
            _ => {
                return Err(Error::input(
                    "unsupported_response_format",
                    "response_format",
                ));
            }
        }
    }
    Ok(out)
}

/// 构造 Gemini Generate Content 请求体。
///
/// Gemini 的对话放在 `contents`，开头 system/developer 进入
/// `systemInstruction`，assistant 角色改写为 `model`。连续工具结果按上一轮顺序
/// 合并成 user `functionResponse` parts；replay 存在时则使用原生的 model content。
///
/// `maxOutputTokens` 只在客户端显式提供时写入，不使用设置里的 Anthropic 默认值。
/// `parallel_tool_calls == false`、URL 图片、`strict:true` 工具和结构化 response
/// format 当前无法完整表达，因此明确拒绝；`parallel_tool_calls == true` 没有额外的
/// Gemini 字段需要写出。所有 schema 仍只经过保守子集检查。
fn gemini(
    settings: &ProtocolSettings,
    r: &ChatRequest,
    history: &ResolvedHistory,
) -> Result<Value, Error> {
    let protocol = UpstreamProtocol::GeminiGenerateContent;
    if r.parallel_tool_calls == Some(false) {
        return Err(Error::input("unsupported_parameter", "parallel_tool_calls"));
    }
    let (system, first) = system_prefix(r, protocol)?;
    let mut contents = vec![];
    let mut calls: BTreeMap<String, NativeCall> = BTreeMap::new();
    let mut call_order = Vec::new();
    let mut i = first;
    // 阶段一：按 Gemini contents 语义转换普通 turn 和工具结果组。
    while i < r.messages.len() {
        let m = &r.messages[i];
        let replay = history.get(&i).map(Arc::as_ref);
        if m.role == Role::Tool {
            let mut parts = vec![];
            let (ordered, end) = ordered_results(&r.messages, i, &call_order)?;
            for m in ordered {
                let id = m.tool_call_id.as_deref().expect("validated ID");
                let call = calls
                    .get(id)
                    .ok_or_else(|| Error::input("invalid_tool_history", "messages.tool_call_id"))?;
                let mut response = json!({"name":call.name,"response":{"result":m.text()}});
                let native_id = call_id(&calls, id)?;
                if !native_id.is_empty() {
                    response["id"] = json!(native_id);
                }
                parts.push(json!({"functionResponse":response}));
            }
            i = end;
            contents.push(json!({"role":"user","parts":parts}));
            continue;
        }
        let value = if let Some(replay) = replay {
            replay.assistant.clone()
        } else {
            let mut parts = nonresponses_parts(&m.parts, protocol)?;
            for call in &m.calls {
                parts.push(json!({"functionCall":{"id":call_id(&calls,&call.id)?,"name":call.name,"args":call.arguments}}));
            }
            json!({"role":if m.role==Role::Assistant {"model"}else{"user"},"parts":parts})
        };
        contents.push(value);
        remember(&mut calls, m, replay);
        call_order = m.calls.iter().map(|c| c.id.clone()).collect();
        i += 1;
    }
    // 阶段二：生成参数和 system instruction。
    let mut out = json!({"contents":contents,"generationConfig":{}});
    if !system.is_empty() {
        out["systemInstruction"] = json!({"parts":system});
    }
    if let Some(max) = r.max_output_tokens {
        out["generationConfig"]["maxOutputTokens"] = json!(max);
    }
    if let Some(v) = r.temperature {
        out["generationConfig"]["temperature"] = json!(v);
    }
    if let Some(v) = r.top_p {
        out["generationConfig"]["topP"] = json!(v);
    }
    if !r.stop.is_empty() {
        out["generationConfig"]["stopSequences"] = json!(r.stop);
    }
    thinking(
        &mut out,
        settings,
        r,
        protocol,
        r.max_output_tokens
            .unwrap_or(settings.default_max_output_tokens),
    )?;
    // 阶段三：function declarations、白名单模式选择；无法无损表达的 strict 或
    // structured output 直接失败。
    if !r.tools.is_empty() {
        let mut declarations = vec![];
        for tool in &r.tools {
            if tool.strict == Some(true) {
                return Err(Error::input(
                    "unsupported_parameter",
                    "tools.function.strict",
                ));
            }
            schema(&tool.parameters, protocol, "tools.function.parameters")?;
            declarations.push(json!({"name":tool.name,"description":tool.description.as_deref().unwrap_or(""),"parametersJsonSchema":tool.parameters}));
        }
        out["tools"] = json!([{ "functionDeclarations":declarations }]);
        out["toolConfig"] = json!({"functionCallingConfig":match &r.tool_choice {
            ToolChoice::Auto=>json!({"mode":"AUTO"}),ToolChoice::None=>json!({"mode":"NONE"}),ToolChoice::Required=>json!({"mode":"ANY"}),
            ToolChoice::Named(name)=>json!({"mode":"ANY","allowedFunctionNames":[name]}),
        }});
    }
    if r.response_format
        .as_ref()
        .is_some_and(|f| f["type"] != "text")
    {
        // The current API documentation is changing response-format fields.
        // Advertise no structured-output capability until one REST subset is
        // frozen and independently verified; never strip an explicit schema.
        return Err(Error::input(
            "unsupported_response_format",
            "response_format",
        ));
    }
    Ok(out)
}

/// 将统一 reasoning effort 写入 Anthropic 或 Gemini 的思考字段。
///
/// 未提供 effort 时不写任何思考字段；显式 `"none"` 对 Anthropic 写成 disabled，
/// 对启用 GeminiBudget 的配置写成 0 预算，其他模式因无法明确表达而失败。
///
/// 非 `none` 值必须命中 `ProtocolSettings.reasoning_profile` 中的显式映射。预算模式
/// 写入 token 预算；Anthropic 还要求预算小于输出上限，并要求温度若存在必须为 1、
/// 不得同时设置 top_p。档位模式写入 vendor level，Anthropic adaptive 不接受任何
/// temperature/top_p。函数只修改传入 JSON，不发送请求。
fn thinking(
    out: &mut Value,
    settings: &ProtocolSettings,
    r: &ChatRequest,
    protocol: UpstreamProtocol,
    max: u32,
) -> Result<(), Error> {
    let Some(effort) = &r.reasoning_effort else {
        return Ok(());
    };
    let profile = &settings.reasoning_profile;
    if effort == "none" {
        if protocol == UpstreamProtocol::AnthropicMessages {
            out["thinking"] = json!({"type":"disabled"});
        } else if profile.mode == ReasoningMode::GeminiBudget {
            out["generationConfig"]["thinkingConfig"] = json!({"thinkingBudget":0});
        } else {
            return Err(Error::input(
                "unsupported_reasoning_effort",
                "reasoning_effort",
            ));
        }
        return Ok(());
    }
    match profile.mode {
        ReasoningMode::AnthropicBudget | ReasoningMode::GeminiBudget => {
            let budget = profile
                .budgets
                .get(effort)
                .ok_or_else(|| Error::input("unconfigured_reasoning_effort", "reasoning_effort"))?;
            if profile.mode == ReasoningMode::AnthropicBudget {
                if *budget >= max || r.temperature.is_some_and(|t| t != 1.) || r.top_p.is_some() {
                    return Err(Error::input(
                        "incompatible_thinking_parameters",
                        "reasoning_effort",
                    ));
                }
                out["thinking"] = json!({"type":"enabled","budget_tokens":budget});
            } else {
                out["generationConfig"]["thinkingConfig"] = json!({"thinkingBudget":budget});
            }
        }
        ReasoningMode::AnthropicAdaptive | ReasoningMode::GeminiLevel => {
            let level = profile
                .levels
                .get(effort)
                .ok_or_else(|| Error::input("unconfigured_reasoning_effort", "reasoning_effort"))?;
            if profile.mode == ReasoningMode::AnthropicAdaptive {
                if r.temperature.is_some() || r.top_p.is_some() {
                    return Err(Error::input(
                        "incompatible_thinking_parameters",
                        "temperature",
                    ));
                }
                out["thinking"] = json!({"type":"adaptive"});
                out["output_config"] = json!({"effort":level});
            } else {
                out["generationConfig"]["thinkingConfig"] = json!({"thinkingLevel":level});
            }
        }
        ReasoningMode::None => {
            return Err(Error::input(
                "unconfigured_reasoning_effort",
                "reasoning_effort",
            ));
        }
    }
    Ok(())
}

/// 检查当前编码器支持的保守 JSON Schema 键子集并递归下降到已知子结构。
///
/// 允许的键只有 `type`、`description`、`title`、`enum`、`required`、
/// `properties`、`$defs`、`additionalProperties`、`items`、`anyOf` 和 `$ref`，并有
/// 协议限制：`$defs` 只允许 Responses，`$ref` 只允许 Responses 且指向本地 `#`
/// 引用。未知键、错误的容器类型或递归到非 object 的 schema 节点都会失败。
///
/// 这不是完整 JSON Schema 验证器：除函数明确检查的容器形状和协议差异外，它不检查
/// 类型约束之间的一致性、引用解析、正则合法性或实例数据。它的目的是在发送前阻止
/// 当前映射可能悄悄丢失的约束；失败返回 `invalid_json_schema` 或
/// `unsupported_json_schema`，不会发起上游请求。
fn schema(value: &Value, protocol: UpstreamProtocol, path: &str) -> Result<(), Error> {
    let m = value
        .as_object()
        .ok_or_else(|| Error::input("invalid_json_schema", path))?;
    for (key, v) in m {
        match key.as_str() {
            "type" | "description" | "title" | "enum" | "required" => (),
            "properties" | "$defs" => {
                if key == "$defs" && protocol != UpstreamProtocol::OpenaiResponses {
                    return Err(Error::input("unsupported_json_schema", path));
                }
                let values = v
                    .as_object()
                    .ok_or_else(|| Error::input("invalid_json_schema", path))?;
                for child in values.values() {
                    schema(child, protocol, path)?;
                }
            }
            "additionalProperties" => {
                if !v.is_boolean() {
                    schema(v, protocol, path)?;
                }
            }
            "items" => schema(v, protocol, path)?,
            "anyOf" => {
                let values = v
                    .as_array()
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| Error::input("invalid_json_schema", path))?;
                for child in values {
                    schema(child, protocol, path)?;
                }
            }
            "$ref"
                if protocol == UpstreamProtocol::OpenaiResponses
                    && v.as_str().is_some_and(|s| s.starts_with('#')) => {}
            _ => return Err(Error::input("unsupported_json_schema", path)),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(extra: Value) -> ChatRequest {
        let mut value = json!({"model":"local","messages":[{"role":"system","content":"one"},{"role":"developer","content":"two"},{"role":"user","content":"hello"}]});
        for (k, v) in extra.as_object().unwrap() {
            value[k] = v.clone();
        }
        ChatRequest::parse(&serde_json::to_vec(&value).unwrap()).unwrap()
    }
    #[test]
    fn ordinary_responses_assistant_history_uses_easy_input_message() {
        let r = request(json!({"messages":[{"role":"user","content":"one"},
            {"role":"assistant","content":[{"type":"text","text":"answer "},{"type":"text","text":"one"}]},
            {"role":"user","content":"two"}]}));
        let out = encode(
            UpstreamProtocol::OpenaiResponses,
            "real",
            &ProtocolSettings::default(),
            &r,
            &ResolvedHistory::new(),
        )
        .unwrap();
        assert_eq!(
            out["input"][1],
            json!({"role":"assistant","content":"answer one"})
        );
    }
    #[test]
    fn text_system_and_limits_map_without_changing_the_real_model() {
        let r = request(json!({"max_completion_tokens":100}));
        let s = ProtocolSettings::default();
        let h = ResolvedHistory::new();
        let out = encode(UpstreamProtocol::OpenaiResponses, "real", &s, &r, &h).unwrap();
        assert_eq!(out["model"], "real");
        assert_eq!(out["max_output_tokens"], 100);
        assert_eq!(out["input"][1]["role"], "developer");
        assert_eq!(out["store"], false);
        let out = encode(UpstreamProtocol::AnthropicMessages, "real", &s, &r, &h).unwrap();
        assert_eq!(out["system"][0]["text"], "one");
        assert_eq!(out["system"][1]["text"], "two");
        assert_eq!(out["max_tokens"], 100);
        let out = encode(UpstreamProtocol::GeminiGenerateContent, "real", &s, &r, &h).unwrap();
        assert_eq!(out["systemInstruction"]["parts"][1]["text"], "two");
        assert_eq!(out["generationConfig"]["maxOutputTokens"], 100);
    }
    #[test]
    fn parallel_results_keep_ids_and_non_json_text_and_merge_only_result_turns() {
        let r = request(
            json!({"messages":[{"role":"user","content":"do it"},{"role":"assistant","content":"checking","tool_calls":[
            {"id":"a","type":"function","function":{"name":"find","arguments":"{\"q\":1}"}},
            {"id":"b","type":"function","function":{"name":"find","arguments":"{\"q\":2}"}}
        ]},{"role":"tool","tool_call_id":"b","content":"text-B"},{"role":"tool","tool_call_id":"a","content":"text-A"}]}),
        );
        let s = ProtocolSettings::default();
        let h = ResolvedHistory::new();
        let out = encode(UpstreamProtocol::AnthropicMessages, "real", &s, &r, &h).unwrap();
        assert_eq!(out["messages"].as_array().unwrap().len(), 3);
        assert_eq!(out["messages"][2]["content"][0]["tool_use_id"], "a");
        assert_eq!(out["messages"][2]["content"][1]["content"], "text-B");
        let out = encode(UpstreamProtocol::GeminiGenerateContent, "real", &s, &r, &h).unwrap();
        assert_eq!(
            out["contents"][2]["parts"][0]["functionResponse"]["id"],
            "a"
        );
        assert_eq!(
            out["contents"][2]["parts"][0]["functionResponse"]["response"],
            json!({"result":"text-A"})
        );
    }
    #[test]
    fn opaque_replay_blocks_keep_order_and_original_call_ids() {
        let r = request(
            json!({"messages":[{"role":"user","content":"do it"},{"role":"assistant","content":null,"tool_calls":[
            {"id":"wbpt_a","type":"function","function":{"name":"find","arguments":"{}"}}
        ]},{"role":"tool","tool_call_id":"wbpt_a","content":"ok"}]}),
        );
        let mut h = ResolvedHistory::new();
        let s = ProtocolSettings::default();
        assert_eq!(
            encode(UpstreamProtocol::AnthropicMessages, "real", &s, &r, &h)
                .unwrap_err()
                .status,
            409
        );
        let blocks = json!([{ "type":"thinking","thinking":"summary","signature":"opaque" },{"type":"tool_use","id":"native","name":"find","input":{}}]);
        h.insert(
            1,
            Arc::new(ReplayTurn {
                assistant: blocks.clone(),
                calls: BTreeMap::from([(
                    "wbpt_a".into(),
                    NativeCall {
                        id: "native".into(),
                        name: "find".into(),
                    },
                )]),
            }),
        );
        let out = encode(UpstreamProtocol::AnthropicMessages, "real", &s, &r, &h).unwrap();
        assert_eq!(out["messages"][1]["content"], blocks);
        assert_eq!(out["messages"][2]["content"][0]["tool_use_id"], "native");
    }
    #[test]
    fn schemas_choices_and_thinking_never_silently_weaken() {
        let mut s = ProtocolSettings::default();
        let h = ResolvedHistory::new();
        let r = request(json!({"reasoning_effort":"max"}));
        assert!(encode(UpstreamProtocol::OpenaiResponses, "real", &s, &r, &h).is_err());
        s.responses_efforts.insert("max".into(), "xhigh".into());
        assert_eq!(
            encode(UpstreamProtocol::OpenaiResponses, "real", &s, &r, &h).unwrap()["reasoning"]["effort"],
            "xhigh"
        );
        s = ProtocolSettings::default();
        s.reasoning_profile.mode = ReasoningMode::AnthropicBudget;
        s.reasoning_profile.budgets.insert("high".into(), 2048);
        let r = request(json!({"reasoning_effort":"high","max_tokens":2048}));
        assert!(encode(UpstreamProtocol::AnthropicMessages, "real", &s, &r, &h).is_err());
        let r = request(
            json!({"response_format":{"type":"json_schema","json_schema":{"name":"answer","strict":true,"schema":{"type":"object","properties":{"text":{"type":"string"}},"additionalProperties":false,"required":["text"]}}}}),
        );
        let out = encode(
            UpstreamProtocol::OpenaiResponses,
            "real",
            &ProtocolSettings::default(),
            &r,
            &h,
        )
        .unwrap();
        assert_eq!(
            out["text"]["format"]["schema"]["additionalProperties"],
            false
        );
        let unsupported =
            json!({"type":"object","properties":{"x":{"type":"string","pattern":"x"}}});
        assert!(
            schema(
                &unsupported,
                UpstreamProtocol::GeminiGenerateContent,
                "tools"
            )
            .is_err()
        );
    }
}

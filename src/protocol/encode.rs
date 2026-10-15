//! Conversion of validated Chat requests. Replay data is supplied by the
//! process-local state cache, never accepted from private client JSON fields.
use super::{
    Error,
    config::{ProtocolSettings, ReasoningMode, UpstreamProtocol},
    request::{ChatRequest, Message, Part, Role, ToolChoice},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug)]
pub struct NativeCall {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug)]
pub struct ReplayTurn {
    /// Responses: output array; Anthropic: content blocks; Gemini: content.
    pub assistant: Value,
    pub calls: BTreeMap<String, NativeCall>,
}
pub type ResolvedHistory = BTreeMap<usize, Arc<ReplayTurn>>;

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

fn response_part(part: &Part, assistant: bool) -> Value {
    match part {
        Part::Text(text) if assistant => json!({"type":"output_text","text":text,"annotations":[]}),
        Part::Text(text) => json!({"type":"input_text","text":text}),
        Part::ImageData { mime, data, detail } => {
            json!({"type":"input_image","image_url":format!("data:{mime};base64,{data}"),"detail":detail})
        }
        Part::ImageUrl { url, detail } => {
            json!({"type":"input_image","image_url":url,"detail":detail})
        }
    }
}
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
                        let mut message=json!({"role":role,"content":m.parts.iter().map(|p| response_part(p,m.role==Role::Assistant)).collect::<Vec<_>>()});
                        if m.role==Role::Assistant {message["type"]=json!("message");message["status"]=json!("completed");}
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

/// Returns leading system/developer parts and rejects mid-dialog instructions
/// on APIs that have one separate system field.
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

/// A deliberately explicit schema subset. Unsupported constraints fail before
/// transport rather than disappearing during vendor schema conversion.
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

//! Native JSON decoding and common Chat output. Streaming state machines reuse
//! these terminal snapshot validators; usage is not a condition for relay success.
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finish {
    Stop,
    Tools,
    Length,
    Filter,
}
impl Finish {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Tools => "tool_calls",
            Self::Length => "length",
            Self::Filter => "content_filter",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Identity {
    pub id: String,
    pub created: i64,
    pub model: String,
}
impl Identity {
    pub fn new(model: &str) -> Self {
        Self {
            id: format!("chatcmpl-wbp-{}", crate::config::token()),
            created: crate::stats::now_ms() / 1000,
            model: model.into(),
        }
    }
    pub fn chunk(&self, delta: Value) -> Value {
        json!({"id":self.id,"object":"chat.completion.chunk","created":self.created,"model":self.model,"choices":[{"index":0,"delta":delta,"finish_reason":null}]})
    }
    pub fn finish_chunk(&self, finish: Finish) -> Value {
        json!({"id":self.id,"object":"chat.completion.chunk","created":self.created,"model":self.model,"choices":[{"index":0,"delta":{},"finish_reason":finish.as_str()}]})
    }
    pub fn usage_chunk(&self, usage: &Usage) -> Value {
        json!({"id":self.id,"object":"chat.completion.chunk","created":self.created,"model":self.model,"choices":[],"usage":usage.chat})
    }
}
#[derive(Debug)]
pub struct Output {
    pub assistant: Message,
    pub reasoning: String,
    pub refusal: Option<String>,
    pub finish: Finish,
    pub usage: Usage,
    pub replay: Option<ReplayTurn>,
    pub model: Option<String>,
}
impl Output {
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
pub fn bad(code: &'static str) -> Error {
    Error::upstream(code)
}
pub fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, Error> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| bad("invalid_upstream_response"))
}
pub fn parse_json(protocol: UpstreamProtocol, bytes: &[u8]) -> Result<Output, Error> {
    let native =
        request::strict_json(bytes, OUTPUT_LIMIT).map_err(|_| bad("invalid_upstream_json"))?;
    decode(protocol, native)
}
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
fn model(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control))
        .map(String::from)
}
fn append(out: &mut Output, s: &str) {
    if let Some(Part::Text(old)) = out.assistant.parts.last_mut() {
        old.push_str(s);
    } else if !s.is_empty() {
        out.assistant.parts.push(Part::Text(s.into()));
    }
}
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
fn replay(out: &mut Output, native: Value, calls: BTreeMap<String, NativeCall>) {
    if !calls.is_empty() {
        out.replay = Some(ReplayTurn {
            assistant: native,
            calls,
        });
    }
}

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

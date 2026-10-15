use super::*;
#[derive(Default)]
pub(super) struct State {
    message: Option<Value>,
    blocks: BTreeMap<u64, Block>,
}
struct Block {
    native: Value,
    tool: Option<usize>,
    closed: bool,
    initial_input: bool,
}
fn push(value: &mut Value, key: &str, s: &str) -> Result<(), Error> {
    match value.get_mut(key) {
        Some(Value::String(text)) => text.push_str(s),
        _ => return Err(bad("invalid_upstream_block")),
    }
    Ok(())
}
fn merge_usage(message: &mut Value, v: &Value) -> Result<(), Error> {
    super::super::usage::merge_cumulative(
        &mut message["usage"],
        v,
        UpstreamProtocol::AnthropicMessages,
    );
    Ok(())
}
impl State {
    pub(super) fn event(
        &mut self,
        event: &str,
        v: Value,
        c: &mut Common,
        d: &mut Map<String, Value>,
    ) -> Result<(), Error> {
        let kind = field(&v, "type")?;
        if event != "message" && event != kind {
            return Err(bad("upstream_event_type_mismatch"));
        }
        if kind == "ping" {
            return Ok(());
        }
        if kind == "error" {
            return Err(bad("upstream_stream_error"));
        }
        if c.terminal {
            return Err(bad("event_after_terminal"));
        }
        match kind {
            "message_start" => {
                if self.message.is_some() {
                    return Err(bad("duplicate_upstream_message"));
                }
                let m = v
                    .get("message")
                    .filter(|v| v.is_object())
                    .ok_or_else(|| bad("invalid_upstream_message"))?;
                if field(m, "type")? != "message"
                    || field(m, "role")? != "assistant"
                    || m.get("content")
                        .and_then(Value::as_array)
                        .is_none_or(|v| !v.is_empty())
                    || !m["stop_reason"].is_null()
                {
                    return Err(bad("invalid_upstream_message_start"));
                }
                c.model(m, "model")?;
                c.usage = super::super::usage::normalize(
                    UpstreamProtocol::AnthropicMessages,
                    &m["usage"],
                );
                let mut message = m.clone();
                message["usage"] = json!({});
                if let Some(usage) = m.get("usage") {
                    merge_usage(&mut message, usage)?;
                }
                self.message = Some(message);
                c.confirmed = true;
            }
            "content_block_start" => {
                if self.message.is_none() {
                    return Err(bad("block_before_message"));
                }
                let i = index(&v, "index")?;
                if i as usize != self.blocks.len()
                    || self.blocks.len() >= 4096
                    || self.blocks.values().any(|b| !b.closed)
                {
                    return Err(bad("invalid_upstream_block_order"));
                }
                let native = v
                    .get("content_block")
                    .filter(|v| v.is_object())
                    .ok_or_else(|| bad("invalid_upstream_block"))?
                    .clone();
                c.charge(
                    serde_json::to_vec(&native)
                        .map_err(|_| bad("invalid_upstream_block"))?
                        .len(),
                )?;
                let mut tool = None;
                let mut initial_input = false;
                match field(&native, "type")? {
                    "text" => c.emit(
                        &format!("a:{i}:text"),
                        "content",
                        field(&native, "text")?,
                        false,
                        false,
                        d,
                    )?,
                    "thinking" => {
                        c.emit(
                            &format!("a:{i}:thinking"),
                            "reasoning_content",
                            field(&native, "thinking")?,
                            false,
                            false,
                            d,
                        )?;
                        c.charge(
                            native
                                .get("signature")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .len(),
                        )?;
                    }
                    "redacted_thinking" => c.charge(field(&native, "data")?.len())?,
                    "tool_use" => {
                        let t = c.start_tool(
                            format!("anthropic:{i}"),
                            field(&native, "id")?,
                            field(&native, "name")?,
                            d,
                        )?;
                        let input = native
                            .get("input")
                            .and_then(Value::as_object)
                            .ok_or_else(|| bad("invalid_upstream_tool_arguments"))?;
                        if !input.is_empty() {
                            c.args(
                                t,
                                &serde_json::to_string(input)
                                    .map_err(|_| bad("invalid_upstream_tool_arguments"))?,
                                true,
                                false,
                                d,
                            )?;
                            initial_input = true;
                        }
                        tool = Some(t);
                    }
                    _ => return Err(bad("unsupported_upstream_content")),
                }
                self.blocks.insert(
                    i,
                    Block {
                        native,
                        tool,
                        closed: false,
                        initial_input,
                    },
                );
            }
            "content_block_delta" => {
                let i = index(&v, "index")?;
                let block = self
                    .blocks
                    .get_mut(&i)
                    .filter(|b| !b.closed)
                    .ok_or_else(|| bad("unknown_or_closed_upstream_block"))?;
                let delta = &v["delta"];
                match (field(&block.native, "type")?, field(delta, "type")?) {
                    ("text", "text_delta") => {
                        let s = field(delta, "text")?;
                        c.emit(&format!("a:{i}:text"), "content", s, false, false, d)?;
                        push(&mut block.native, "text", s)?;
                    }
                    ("thinking", "thinking_delta") => {
                        let s = field(delta, "thinking")?;
                        c.emit(
                            &format!("a:{i}:thinking"),
                            "reasoning_content",
                            s,
                            false,
                            false,
                            d,
                        )?;
                        push(&mut block.native, "thinking", s)?;
                    }
                    ("thinking", "signature_delta") => {
                        let s = field(delta, "signature")?;
                        c.charge(s.len())?;
                        if block.native.get("signature").is_none() {
                            block.native["signature"] = json!("");
                        }
                        push(&mut block.native, "signature", s)?;
                    }
                    ("tool_use", "input_json_delta") => {
                        if block.initial_input {
                            return Err(bad("ambiguous_initial_tool_input"));
                        }
                        c.args(
                            block.tool.expect("tool block"),
                            field(delta, "partial_json")?,
                            false,
                            false,
                            d,
                        )?;
                    }
                    _ => return Err(bad("unsupported_upstream_block_delta")),
                }
            }
            "content_block_stop" => {
                let i = index(&v, "index")?;
                let block = self
                    .blocks
                    .get_mut(&i)
                    .filter(|b| !b.closed)
                    .ok_or_else(|| bad("unknown_or_closed_upstream_block"))?;
                if let Some(t) = block.tool {
                    if c.tools[t].args.is_empty() {
                        c.args(t, "{}", true, true, d)?;
                    } else {
                        c.args(t, "", false, true, d)?;
                    }
                    block.native["input"] =
                        request::strict_json(c.tools[t].args.as_bytes(), TOOL_ARGUMENT_LIMIT)
                            .map_err(|_| bad("invalid_upstream_tool_arguments"))?;
                }
                if block.native["type"] == "thinking"
                    && block
                        .native
                        .get("signature")
                        .and_then(Value::as_str)
                        .is_none_or(|s| s.is_empty())
                {
                    return Err(bad("missing_thinking_signature"));
                }
                block.closed = true;
            }
            "message_delta" => {
                if self.blocks.values().any(|b| !b.closed) {
                    return Err(bad("unclosed_upstream_block"));
                }
                let message = self
                    .message
                    .as_mut()
                    .ok_or_else(|| bad("delta_before_message"))?;
                let delta = v
                    .get("delta")
                    .and_then(Value::as_object)
                    .ok_or_else(|| bad("invalid_upstream_message_delta"))?;
                for key in delta.keys() {
                    if !matches!(key.as_str(), "stop_reason" | "stop_sequence") {
                        return Err(bad("unsupported_upstream_message_delta"));
                    }
                }
                for (key, value) in delta {
                    message[key] = value.clone();
                }
                if let Some(usage) = v.get("usage") {
                    merge_usage(message, usage)?;
                }
                c.usage = super::super::usage::normalize(
                    UpstreamProtocol::AnthropicMessages,
                    &message["usage"],
                );
            }
            "message_stop" => {
                if self.blocks.values().any(|b| !b.closed) {
                    return Err(bad("unclosed_upstream_block"));
                }
                let mut message = self
                    .message
                    .take()
                    .ok_or_else(|| bad("stop_before_message"))?;
                if !message["stop_reason"].is_string() {
                    return Err(bad("missing_upstream_finish"));
                }
                message["content"] = json!(
                    std::mem::take(&mut self.blocks)
                        .into_values()
                        .map(|b| b.native)
                        .collect::<Vec<_>>()
                );
                let out = output::decode(UpstreamProtocol::AnthropicMessages, message)?;
                c.reconcile(out, d)?;
            }
            _ => return Err(bad("unsupported_upstream_event")),
        }
        Ok(())
    }
}

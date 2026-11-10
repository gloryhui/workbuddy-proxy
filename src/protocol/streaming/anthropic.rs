//! Anthropic Messages 流事件状态机。
//!
//! 上游输入由 `streaming::Decoder::event` 分派，已经完成 SSE 分帧并解析为
//! JSON object；下游仍通过 `Common` 生成 Chat delta。与 Responses 不同，
//! Anthropic 用 message_start、顺序 content block、message_delta 和
//! message_stop 组装终态。State 保存重建最终 message 所需的 native blocks，
//! 因而正文在该协议自己的 buffer 中会保留到 message_stop；`Common::Track`
//! 本身仍只保存正文长度/hash。State 由单个响应独占，不跨 await 并发共享。
use super::*;
/// Anthropic 流状态。
#[derive(Default)]
pub(super) struct State {
    /// message_start 中的 message 外壳。它先以空 content 保存，message_stop
    /// 时再填入所有已关闭 native block，用于最终 Output 和 replay 校验。
    message: Option<Value>,
    /// content block index -> native block、工具映射和关闭状态。
    blocks: BTreeMap<u64, Block>,
    /// 是否已经观察到带 stop_reason 的 message_delta 和 output_tokens 更新；
    /// 只有此时 output_tokens 才可作为最终/较可信用量。
    usage_final: bool,
}
/// 一个 Anthropic content block 的累计状态。
struct Block {
    /// 从 content_block_start 复制并在 text/thinking/signature/input 增量上
    /// 更新的 native block。message_stop 用它构造最终协议快照。
    native: Value,
    /// tool_use block 在 `Common.tools` 中的下标；非工具 block 为 None。
    tool: Option<usize>,
    /// content_block_stop 后置为 true；之后该 index 不得再收到 delta。
    closed: bool,
    /// content_block_start 是否已经带有非空 input。已有完整 input 时，再收到
    /// `input_json_delta` 会造成两种语义相撞，因此明确失败。
    initial_input: bool,
}
/// 把一个 `signature_delta` 或文本增量追加到 native 字符串字段。
///
/// 只有目标字段原本就是 String 才允许；类型漂移不会被强转。
fn push(value: &mut Value, key: &str, s: &str) -> Result<(), Error> {
    match value.get_mut(key) {
        Some(Value::String(text)) => text.push_str(s),
        _ => return Err(bad("invalid_upstream_block")),
    }
    Ok(())
}
/// 把新的累计 usage 白名单字段合并到 message.usage。
///
/// 这是替换/补齐累计快照，不是把每个事件的 output_tokens 相加。
fn merge_usage(message: &mut Value, v: &Value) -> Result<(), Error> {
    super::super::usage::merge_cumulative(
        &mut message["usage"],
        v,
        UpstreamProtocol::AnthropicMessages,
    );
    Ok(())
}
impl State {
    /// 消费一个有名称的 Anthropic SSE Event。
    ///
    /// 除缺省 `message` header 外，`event` 与 JSON `type` 必须一致。ping 可被
    /// 忽略；error 立即失败；终态完成后的任何业务事件都失败。
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
                // 起始 message 必须还没有内容、stop_reason 为 null；这避免把
                // 非标准响应当成一个可增量推进的 assistant message。
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
                // message_start 的 output_tokens 常为 0，只代表初始下界。
                // normalize_partial 会隐藏尚未完成的 output 计数。
                c.usage = super::super::usage::normalize_partial(
                    UpstreamProtocol::AnthropicMessages,
                    &m["usage"],
                );
                let mut message = m.clone();
                message["usage"] = json!({});
                if let Some(usage) = m.get("usage") {
                    // native 重建也仅保留 usage 白名单，不能让任意元数据留在
                    // replay/终态 Value 中无界增长。
                    merge_usage(&mut message, usage)?;
                }
                self.message = Some(message);
                c.confirmed = true;
            }
            "content_block_start" => {
                // index 必须从 0 连续增长，且之前所有 block 已关闭；这使 Chat
                // tool_calls 顺序和 native replay 顺序都有确定含义。
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
                        // start 携带初始 text；通常为空，但仍作为严格增量处理。
                        &format!("a:{i}:text"),
                        "content",
                        field(&native, "text")?,
                        false,
                        false,
                        d,
                    )?,
                    "thinking" => {
                        // thinking 文本可映射 reasoning_content；signature 只进入
                        // native replay，不向普通客户端泄露。
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
                    // 只记录不透明 data 的驻留预算，不把它转换成公开 Chat 文本。
                    "redacted_thinking" => c.charge(field(&native, "data")?.len())?,
                    "tool_use" => {
                        // tool block 的初始 input 是 object。若非空，start 就会
                        // 发出首段 arguments，并且后续 input_json_delta 被禁止。
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
                // delta 必须指向已开始且未关闭的 block；类型组合也必须与 start
                // 时声明的一致，不能在 text/tool/thinking 之间漂移。
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
                        // partial_json 是真正增量；只有 content_block_stop 才
                        // 对整个拼接结果做 JSON object 的终值校验。
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
                // 关闭工具 block 前补齐空 input 为 `{}`，再严格解析全部参数；
                // thinking block 则必须已有非空 signature，后续 replay 才能完整。
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
                // message_delta 不是 transport EOF。它要求所有 block 已关闭，
                // 只接受 stop_reason/stop_sequence，并合并新的累计 usage。
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
                    if message["stop_reason"].is_string() && usage.get("output_tokens").is_some() {
                        // 之后的 output_tokens 可进入最终 normalize；是否数值
                        // 合法仍由 usage 模块判断，不能仅凭“看过字段”信任。
                        self.usage_final = true;
                    }
                }
                c.usage = if self.usage_final {
                    super::super::usage::normalize(
                        UpstreamProtocol::AnthropicMessages,
                        &message["usage"],
                    )
                } else {
                    super::super::usage::normalize_partial(
                        UpstreamProtocol::AnthropicMessages,
                        &message["usage"],
                    )
                };
            }
            "message_stop" => {
                // message_stop 是协议 marker。这里组装 native message 并 reconcile，
                // 但 Decoder 仍等待 transport EOF 后才把它作为成功 Output 交出。
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
                if !self.usage_final
                    && message["usage"]
                        .get("output_tokens")
                        .is_some_and(|v| !v.is_null())
                {
                    // 没有带 stop_reason 的最终 output 快照时，不把累计的中间
                    // output_tokens 当最终账单。
                    message["usage"]
                        .as_object_mut()
                        .expect("usage object")
                        .remove("output_tokens");
                }
                message["content"] = json!(
                    std::mem::take(&mut self.blocks)
                        .into_values()
                        .map(|b| b.native)
                        .collect::<Vec<_>>()
                );
                let out = output::decode(UpstreamProtocol::AnthropicMessages, message)?;
                // reconcile 校验此前发出的 text/reasoning/tool 前缀与 native
                // message 一致；只有这一步成功才生成 final_output。
                c.reconcile(out, d)?;
            }
            _ => return Err(bad("unsupported_upstream_event")),
        }
        Ok(())
    }
}

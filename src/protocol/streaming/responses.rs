//! OpenAI Responses 流事件状态机。
//!
//! 本文件只处理 Responses 的 event JSON，不知道网络 chunk/SSE Frame 的边界，
//! 也不直接写客户端响应。`streaming::Decoder::event` 按 destination protocol
//! 分派到这里；输出通过 `Common` 形成 Chat delta，并在 response.completed 或
//! response.incomplete 的完整 response snapshot 上调用 `Common::reconcile`。
//! State 与 Decoder 同属一个响应/body future，不创建线程或被多个请求共享。
use super::*;
/// Responses 流中的 item 身份和确定性终态状态。
#[derive(Default)]
pub(super) struct State {
    /// `output_index` -> `(item.id, item.type)`。
    ///
    /// 只保存身份元数据，不把每个 text delta 再复制一份；正文去重前缀由
    /// `Common::Track` 记录，工具参数由 `Common::Tool` 保存。
    items: BTreeMap<u64, (String, String)>,
    /// response.created/in_progress 首次确认的上游 response id；后续事件冲突即失败。
    response_id: Option<String>,
}
impl State {
    /// 校验增量事件引用的 output item 与第一次登记的类型/ID 一致。
    ///
    /// 返回 `output_index`，后续用它生成稳定 slot/tool key。未知 item、类型
    /// 漂移或 item_id 不一致都不能猜测修复。
    fn item(&self, v: &Value, kind: &str) -> Result<u64, Error> {
        let i = index(v, "output_index")?;
        let (id, t) = self
            .items
            .get(&i)
            .ok_or_else(|| bad("unknown_upstream_item"))?;
        if t != kind || v.get("item_id").is_some_and(|v| v.as_str() != Some(id)) {
            return Err(bad("inconsistent_upstream_item"));
        }
        Ok(i)
    }
    /// 将 Responses item 的完整快照交给跨协议去重逻辑。
    ///
    /// snapshot 可能和先前 delta 重叠，因此 `Common::emit/args` 采用累计快照
    /// 校验而非再次追加全文。message/refusal/reasoning summary 的 slot 由
    /// output/content/summary index 共同定位；function_call 使用 output_index
    /// 保持跨帧工具身份。
    fn snapshot(
        &mut self,
        i: u64,
        item: &Value,
        c: &mut Common,
        d: &mut Map<String, Value>,
    ) -> Result<(), Error> {
        let kind = field(item, "type")?;
        match kind {
            "function_call" => {
                let key = format!("response:{i}");
                let tool = c.start_tool(key, field(item, "call_id")?, field(item, "name")?, d)?;
                c.args(tool, field(item, "arguments")?, true, true, d)?;
            }
            "message" => {
                if field(item, "role")? != "assistant" {
                    return Err(bad("invalid_upstream_role"));
                }
                for (j, part) in item
                    .get("content")
                    .and_then(Value::as_array)
                    .ok_or_else(|| bad("invalid_upstream_content"))?
                    .iter()
                    .enumerate()
                {
                    let (channel, key) = match field(part, "type")? {
                        "output_text" => ("content", "text"),
                        "refusal" => ("refusal", "refusal"),
                        _ => return Err(bad("unsupported_upstream_content")),
                    };
                    c.emit(
                        &format!("r:{i}:{j}:{channel}"),
                        channel,
                        field(part, key)?,
                        true,
                        true,
                        d,
                    )?;
                }
            }
            "reasoning" => {
                if let Some(summary) = item.get("summary") {
                    for (j, part) in summary
                        .as_array()
                        .ok_or_else(|| bad("invalid_upstream_reasoning"))?
                        .iter()
                        .enumerate()
                    {
                        if field(part, "type")? != "summary_text" {
                            return Err(bad("unsupported_upstream_reasoning"));
                        }
                        c.emit(
                            &format!("r:{i}:{j}:reasoning_content"),
                            "reasoning_content",
                            field(part, "text")?,
                            true,
                            true,
                            d,
                        )?;
                    }
                }
            }
            _ => return Err(bad("unsupported_upstream_output")),
        }
        Ok(())
    }
    /// 消费一个 Responses SSE Event 并推进状态机。
    ///
    /// `event` 是 SSE header 的 event 名，`v["type"]` 是 JSON 内部类型。除缺省
    /// header `message` 外二者必须相等，避免把错配事件交给错误分支。
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
        if c.terminal {
            return Err(bad("event_after_terminal"));
        }
        match kind {
            "error" | "response.failed" => return Err(bad("upstream_stream_error")),
            "response.created" | "response.in_progress" => {
                // 这是“流已建立”的不是“响应已完成”；只锁定 response id 和模型，
                // 并设置 confirmed 以允许代理发送一次 role chunk。
                let response = &v["response"];
                let id = field(response, "id")?;
                if id.is_empty() || id.len() > 256 {
                    return Err(bad("invalid_upstream_response_id"));
                }
                if self.response_id.as_ref().is_some_and(|old| old != id) {
                    return Err(bad("inconsistent_upstream_response"));
                }
                self.response_id = Some(id.into());
                c.model(response, "model")?;
                c.confirmed = true;
            }
            "response.output_item.added" => {
                // 先登记 item 唯一身份，再初始化工具或等待后续 content/argument
                // 增量。重复 output_index 或重复 item id 均失败。
                let i = index(&v, "output_index")?;
                let item = &v["item"];
                let id = field(item, "id")?;
                if id.is_empty() || id.len() > 256 {
                    return Err(bad("invalid_upstream_item_id"));
                }
                let kind = field(item, "type")?;
                if self.items.contains_key(&i) || self.items.values().any(|(old, _)| old == id) {
                    return Err(bad("duplicate_upstream_item"));
                }
                if !matches!(kind, "message" | "reasoning" | "function_call") {
                    return Err(bad("unsupported_upstream_output"));
                }
                self.items.insert(i, (id.into(), kind.into()));
                c.charge(id.len() + 128)?;
                if kind == "function_call" {
                    let tool = c.start_tool(
                        format!("response:{i}"),
                        field(item, "call_id")?,
                        field(item, "name")?,
                        d,
                    )?;
                    if let Some(args) = item.get("arguments") {
                        let args = args
                            .as_str()
                            .ok_or_else(|| bad("invalid_upstream_tool_arguments"))?;
                        if !args.is_empty() {
                            c.args(tool, args, true, false, d)?;
                        }
                    }
                }
                c.confirmed = true;
            }
            "response.output_text.delta"
            | "response.output_text.done"
            | "response.refusal.delta"
            | "response.refusal.done" => {
                // delta 直接作为新增文本；done 携带该 part 的累计全文，交给
                // Common 校验此前发出前缀并只补缺失后缀。
                let i = self.item(&v, "message")?;
                let j = index(&v, "content_index")?;
                let refusal = kind.starts_with("response.refusal");
                let channel = if refusal { "refusal" } else { "content" };
                let done = kind.ends_with(".done");
                let key = if !done {
                    "delta"
                } else if refusal {
                    "refusal"
                } else {
                    "text"
                };
                c.emit(
                    &format!("r:{i}:{j}:{channel}"),
                    channel,
                    field(&v, key)?,
                    done,
                    done,
                    d,
                )?;
                c.confirmed = true;
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_summary_text.done" => {
                // reasoning summary 可以公开成 Chat reasoning_content；加密
                // reasoning/opaque signature 不在此分支外泄。
                let i = self.item(&v, "reasoning")?;
                let j = index(&v, "summary_index")?;
                let done = kind.ends_with(".done");
                c.emit(
                    &format!("r:{i}:{j}:reasoning_content"),
                    "reasoning_content",
                    field(&v, if done { "text" } else { "delta" })?,
                    done,
                    done,
                    d,
                )?;
                c.confirmed = true;
            }
            "response.function_call_arguments.delta" | "response.function_call_arguments.done" => {
                // delta 是参数文本增量；done 的 arguments 是累计快照。关闭前必须
                // 能整体解析为 JSON object，否则本流的最终工具调用无效。
                let i = self.item(&v, "function_call")?;
                let tool = c
                    .tools
                    .iter()
                    .position(|t| t.key == format!("response:{i}"))
                    .ok_or_else(|| bad("unknown_upstream_tool"))?;
                let done = kind.ends_with(".done");
                c.args(
                    tool,
                    field(&v, if done { "arguments" } else { "delta" })?,
                    done,
                    done,
                    d,
                )?;
                c.confirmed = true;
            }
            "response.output_item.done" => {
                // 完整 item snapshot 会再次覆盖 message/reasoning/function_call；
                // snapshot 校验阻止重复输出或终态参数与此前增量不一致。
                let i = index(&v, "output_index")?;
                let item = &v["item"];
                if let Some((id, kind)) = self.items.get(&i)
                    && (id != field(item, "id")? || kind != field(item, "type")?)
                {
                    return Err(bad("inconsistent_upstream_item"));
                }
                self.snapshot(i, item, c, d)?;
                c.confirmed = true;
            }
            "response.content_part.added" | "response.content_part.done" => {
                // added/done 都携带 part 文本快照；added 通常为空，done 通常
                // 是全文。两者都走 snapshot 的 prefix 校验。
                let i = self.item(&v, "message")?;
                let j = index(&v, "content_index")?;
                let part = &v["part"];
                let (channel, key) = match field(part, "type")? {
                    "output_text" => ("content", "text"),
                    "refusal" => ("refusal", "refusal"),
                    _ => return Err(bad("unsupported_upstream_content")),
                };
                let done = kind.ends_with(".done");
                c.emit(
                    &format!("r:{i}:{j}:{channel}"),
                    channel,
                    field(part, key)?,
                    true,
                    done,
                    d,
                )?;
            }
            "response.reasoning_summary_part.added" | "response.reasoning_summary_part.done" => {
                // 对 reasoning summary part 做与普通 content part 相同的
                // 首次登记/累计终值检查。
                let i = self.item(&v, "reasoning")?;
                let j = index(&v, "summary_index")?;
                let part = &v["part"];
                if field(part, "type")? != "summary_text" {
                    return Err(bad("unsupported_upstream_reasoning"));
                }
                let done = kind.ends_with(".done");
                c.emit(
                    &format!("r:{i}:{j}:reasoning_content"),
                    "reasoning_content",
                    field(part, "text")?,
                    true,
                    done,
                    d,
                )?;
            }
            "response.completed" | "response.incomplete" => {
                // 到这里仍然只说明协议给了 terminal event；transport 还必须干净
                // EOF，Decoder::finish 才会返回该 reconcile 产生的 Output。
                let response = v
                    .get("response")
                    .filter(|v| v.is_object())
                    .ok_or_else(|| bad("invalid_upstream_response"))?;
                if response["status"]
                    != if kind == "response.completed" {
                        "completed"
                    } else {
                        "incomplete"
                    }
                {
                    return Err(bad("inconsistent_upstream_terminal"));
                }
                if self
                    .response_id
                    .as_ref()
                    .is_some_and(|id| response.get("id").and_then(Value::as_str) != Some(id))
                {
                    return Err(bad("inconsistent_upstream_response"));
                }
                c.model(response, "model")?;
                let out = output::decode(UpstreamProtocol::OpenaiResponses, response.clone())?;
                c.reconcile(out, d)?;
                c.confirmed = true;
            }
            // Documented metadata with no text/tool output semantics.
            "response.output_text.annotation.added" => {
                // 注解元数据没有 Chat 正文或工具语义；只验证它属于已登记 message。
                self.item(&v, "message")?;
            }
            _ => return Err(bad("unsupported_upstream_event")),
        }
        Ok(())
    }
}

use super::*;
#[derive(Default)]
pub(super) struct State {
    items: BTreeMap<u64, (String, String)>,
    response_id: Option<String>,
}
impl State {
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
                self.item(&v, "message")?;
            }
            _ => return Err(bad("unsupported_upstream_event")),
        }
        Ok(())
    }
}

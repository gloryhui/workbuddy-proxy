use super::*;
#[derive(Default)]
pub(super) struct State {
    parts: Vec<Value>,
    reason: Option<String>,
    blocked: Option<Value>,
    usage: Value,
    ids: BTreeMap<String, usize>,
}
impl State {
    pub(super) fn terminal_seen(&self) -> bool {
        self.reason.is_some() || self.blocked.is_some()
    }
    pub(super) fn event(
        &mut self,
        event: &str,
        v: Value,
        c: &mut Common,
        d: &mut Map<String, Value>,
    ) -> Result<(), Error> {
        if event != "message" {
            return Err(bad("unsupported_upstream_event"));
        }
        if c.terminal {
            return Err(bad("event_after_terminal"));
        }
        c.model(&v, "modelVersion")?;
        if let Some(usage) = v.get("usageMetadata") {
            super::super::usage::merge_cumulative(
                &mut self.usage,
                usage,
                UpstreamProtocol::GeminiGenerateContent,
            );
            c.usage = super::super::usage::normalize(
                UpstreamProtocol::GeminiGenerateContent,
                &self.usage,
            );
        }
        if let Some(feedback) = v.get("promptFeedback")
            && feedback
                .get("blockReason")
                .is_some_and(|r| r.as_str() != Some("BLOCK_REASON_UNSPECIFIED"))
        {
            if !self.parts.is_empty() || self.reason.is_some() {
                return Err(bad("inconsistent_upstream_block"));
            }
            self.blocked = Some(feedback.clone());
            c.confirmed = true;
        }
        let Some(candidates) = v.get("candidates") else {
            if v.get("usageMetadata").is_none() && v.get("promptFeedback").is_none() {
                return Err(bad("invalid_upstream_event"));
            }
            return Ok(());
        };
        let candidates = candidates
            .as_array()
            .ok_or_else(|| bad("invalid_upstream_candidates"))?;
        if candidates.is_empty() {
            return Ok(());
        }
        if candidates.len() != 1
            || candidates[0]
                .get("index")
                .is_some_and(|i| i.as_u64() != Some(0))
        {
            return Err(bad("multiple_upstream_candidates"));
        }
        if self.blocked.is_some() {
            return Err(bad("content_after_safety_block"));
        }
        let candidate = &candidates[0];
        if let Some(content) = candidate.get("content") {
            if content.get("role").is_some_and(|r| r != "model") {
                return Err(bad("invalid_upstream_role"));
            }
            let parts = content
                .get("parts")
                .and_then(Value::as_array)
                .ok_or_else(|| bad("invalid_upstream_content"))?;
            for part in parts {
                if self.parts.len() >= 4096 {
                    return Err(bad("upstream_output_too_large"));
                }
                let p = part
                    .as_object()
                    .ok_or_else(|| bad("invalid_upstream_content"))?;
                if p.keys().any(|k| {
                    !matches!(
                        k.as_str(),
                        "text" | "thought" | "thoughtSignature" | "functionCall"
                    )
                }) || part.get("thought").is_some_and(|v| !v.is_boolean())
                    || part.get("thoughtSignature").is_some_and(|v| !v.is_string())
                {
                    return Err(bad("unsupported_upstream_content"));
                }
                let has_content = part.get("text").is_some() || part.get("functionCall").is_some();
                if self.reason.is_some() && has_content {
                    return Err(bad("content_after_terminal"));
                }
                if let Some(s) = part.get("text") {
                    if part.get("functionCall").is_some() {
                        return Err(bad("ambiguous_upstream_part"));
                    }
                    let s = s.as_str().ok_or_else(|| bad("invalid_upstream_content"))?;
                    let channel = if part["thought"] == true {
                        "reasoning_content"
                    } else {
                        "content"
                    };
                    c.emit(&format!("gemini:{channel}"), channel, s, false, false, d)?;
                }
                if let Some(call) = part.get("functionCall") {
                    if call.as_object().is_none_or(|call| {
                        call.keys()
                            .any(|k| !matches!(k.as_str(), "id" | "name" | "args"))
                    }) {
                        return Err(bad("unsupported_upstream_tool_call"));
                    }
                    if part["thought"] == true {
                        return Err(bad("invalid_upstream_tool_call"));
                    }
                    let name = field(call, "name")?;
                    let native_id = call
                        .get("id")
                        .map(|v| v.as_str().ok_or_else(|| bad("invalid_upstream_tool_id")))
                        .transpose()?
                        .unwrap_or("");
                    let args = call.get("args").cloned().unwrap_or_else(|| json!({}));
                    if !args.is_object() {
                        return Err(bad("invalid_upstream_tool_arguments"));
                    }
                    if let Some(&old) = self.ids.get(native_id).filter(|_| !native_id.is_empty()) {
                        let previous = &self.parts[old];
                        if previous["functionCall"]["name"] != name
                            || previous["functionCall"]
                                .get("args")
                                .cloned()
                                .unwrap_or_else(|| json!({}))
                                != args
                        {
                            return Err(bad("inconsistent_upstream_tool_arguments"));
                        }
                        if let Some(signature) = part.get("thoughtSignature") {
                            if previous
                                .get("thoughtSignature")
                                .is_some_and(|s| s != signature)
                            {
                                return Err(bad("inconsistent_thought_signature"));
                            }
                            c.charge(signature.as_str().expect("validated signature").len())?;
                            self.parts[old]["thoughtSignature"] = signature.clone();
                        }
                        continue;
                    }
                    let t =
                        c.start_tool(format!("gemini:{}", self.parts.len()), native_id, name, d)?;
                    c.args(
                        t,
                        &serde_json::to_string(&args)
                            .map_err(|_| bad("invalid_upstream_tool_arguments"))?,
                        true,
                        true,
                        d,
                    )?;
                    if !native_id.is_empty() {
                        self.ids.insert(native_id.into(), self.parts.len());
                    }
                }
                if let Some(signature) = part.get("thoughtSignature") {
                    c.charge(signature.as_str().expect("validated signature").len())?;
                }
                if !has_content && part.get("thoughtSignature").is_none() {
                    return Err(bad("unsupported_upstream_content"));
                }
                self.parts.push(part.clone());
                c.confirmed = true;
            }
        }
        if let Some(reason) = candidate.get("finishReason") {
            let reason = reason
                .as_str()
                .filter(|r| *r != "FINISH_REASON_UNSPECIFIED")
                .ok_or_else(|| bad("invalid_upstream_finish"))?;
            if self.reason.as_ref().is_some_and(|old| old != reason) {
                return Err(bad("inconsistent_upstream_finish"));
            }
            self.reason = Some(reason.into());
            c.confirmed = true;
        }
        Ok(())
    }
    pub(super) fn finish(&mut self, c: &mut Common) -> Result<(), Error> {
        let native = if let Some(block) = self.blocked.take() {
            json!({"promptFeedback":block,"usageMetadata":self.usage})
        } else {
            let reason = self
                .reason
                .take()
                .ok_or_else(|| bad("incomplete_upstream_stream"))?;
            json!({"candidates":[{"index":0,"content":{"role":"model","parts":std::mem::take(&mut self.parts)},"finishReason":reason}],"usageMetadata":self.usage})
        };
        let out = output::decode(UpstreamProtocol::GeminiGenerateContent, native)?;
        let mut delta = Map::new();
        c.reconcile(out, &mut delta)?;
        // Every visible Gemini part was already emitted as an increment. A
        // nonempty fallback here would be lost after EOF and must fail safely.
        if !delta.is_empty() {
            return Err(bad("inconsistent_upstream_terminal"));
        }
        Ok(())
    }
}

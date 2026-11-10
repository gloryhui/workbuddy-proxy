//! Gemini GenerateContent 的 SSE 响应状态机。
//!
//! Gemini 流没有 Responses/Anthropic 那样的 event header 名称；调用方以 `alt=sse`
//! 请求后，每条 SSE Event 的 header 都必须是缺省值 `message`，JSON 内部也没有
//! 统一 `type`。本 State 累积 candidate content parts、finishReason、blockReason
//! 和累计 usage，只有 transport clean EOF 时 `finish` 才组装最终 native JSON 并
//! 交给 `Common::reconcile`。State 属于单个 `streaming::Decoder`/响应 body，
//! 不创建线程，也不跨请求共享。
use super::*;
/// Gemini 一个响应的累计状态。
#[derive(Default)]
pub(super) struct State {
    /// 已接收的 native content parts。这里保留 part 是为了在 EOF 重建最终
    /// content/replay，尤其保住 thoughtSignature 和 functionCall；Common 的
    /// 文本 Track 本身仍不保存正文。
    parts: Vec<Value>,
    /// 已见到的 finishReason。出现后普通可见 text/tool 不得再追加；某些流仍可
    /// 在之后发送仅带 thoughtSignature 或 usageMetadata 的尾部事件。
    reason: Option<String>,
    /// promptFeedback 中的安全阻断元数据；它与正常 parts 互斥。
    blocked: Option<Value>,
    /// 从各事件累计合并的白名单 usage 字段；同名计数是最终累计值而非增量。
    usage: Value,
    /// 非空 native functionCall id -> parts 下标。用于识别同一工具调用的重复
    /// 快照；空 id 的 Gemini 工具调用只能按出现顺序保持本地稳定身份。
    ids: BTreeMap<String, usize>,
    /// final usage 是否已经出现。它决定 EOF 时是否隐藏 candidates/thought/total
    /// 这些未确认的输出计数，只保留已证明输入。
    usage_final: bool,
}
impl State {
    /// 是否已看到 finishReason 或 promptFeedback.blockReason。
    ///
    /// 这是协议 terminal 线索，可让外层缩短等待 EOF 的时间；它不是 transport
    /// EOF，也不等于 `Decoder::finish` 已成功。
    pub(super) fn terminal_seen(&self) -> bool {
        self.reason.is_some() || self.blocked.is_some()
    }
    /// 消费一条 Gemini SSE Event。
    ///
    /// 一条 Event 可同时更新 usage、安全阻断、多个 parts 和 finishReason；
    /// 顺序按字段检查。仅可公开的非空 text/thought 和首次完整工具调用会直接
    /// 形成 Chat delta；signature-only part 和重复 native tool 快照通常产生空
    /// delta。终态只重建完整 native snapshot 做一致性与 replay 校验，不会在
    /// EOF 再重复整段文本。
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
        // Gemini 的 modelVersion 可只出现在部分事件；出现后必须始终一致。
        c.model(&v, "modelVersion")?;
        if let Some(usage) = v.get("usageMetadata") {
            // 供应商可能把 usage 拆到多个事件，按累计快照白名单 merge。
            super::super::usage::merge_cumulative(
                &mut self.usage,
                usage,
                UpstreamProtocol::GeminiGenerateContent,
            );
            self.usage_final = self.usage_final
                || self.terminal_seen()
                || v.get("candidates")
                    .and_then(Value::as_array)
                    .is_some_and(|c| c.iter().any(|c| c.get("finishReason").is_some()))
                || v["promptFeedback"].get("blockReason").is_some();
            c.usage = if self.usage_final {
                // terminal 之后仍允许 trailing usage 事件；这里把它作为最终值
                // 规范化，下一帧若还有可见内容会被 reason/terminal 规则拒绝。
                super::super::usage::normalize(UpstreamProtocol::GeminiGenerateContent, &self.usage)
            } else {
                super::super::usage::normalize_partial(
                    UpstreamProtocol::GeminiGenerateContent,
                    &self.usage,
                )
            };
        }
        if let Some(feedback) = v.get("promptFeedback")
            && feedback
                .get("blockReason")
                .is_some_and(|r| r.as_str() != Some("BLOCK_REASON_UNSPECIFIED"))
        {
            // blockReason 是 prompt 级安全阻断；不能与已产生的 candidate 内容
            // 同时存在，也不能与另一终态混用。
            if !self.parts.is_empty() || self.reason.is_some() {
                return Err(bad("inconsistent_upstream_block"));
            }
            self.blocked = Some(feedback.clone());
            c.confirmed = true;
        }
        let Some(candidates) = v.get("candidates") else {
            // 单独的 usage 或 promptFeedback Event 合法；完全无业务字段才是错误。
            if v.get("usageMetadata").is_none() && v.get("promptFeedback").is_none() {
                return Err(bad("invalid_upstream_event"));
            }
            return Ok(());
        };
        let candidates = candidates
            .as_array()
            .ok_or_else(|| bad("invalid_upstream_candidates"))?;
        if candidates.is_empty() {
            // 某些 usage-only 流会保留 candidates: []。
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
                // parts 数量、字段名、类型都按白名单校验。part 可以只有
                // thoughtSignature：它用于补齐前文或工具调用的原生签名。
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
                let visible_content = part
                    .get("text")
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty())
                    || part.get("functionCall").is_some();
                if self.reason.is_some() && visible_content {
                    // finishReason 后，任何 functionCall 或非空 text 都被视为
                    // 新的可见内容；空 text 与 signature-only/usage 尾巴仍可通过
                    // 后续字段规则。
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
                    // Gemini 的 text part 在已审核 adapter 中是完整增量段，不是
                    // cumulative snapshot；重复全文不会自动去重。终态重建会检查
                    // 此前实际发出的拼接前缀是否与 native snapshot 一致。
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
                        // 同一非空 native id 再次出现时，函数名和 args 必须完全
                        // 相同；只允许补充/确认相同的 thoughtSignature。它不是
                        // offset-based JSON 字符串增量。
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
                    // Gemini 此处 args 已是 JSON object，adapter 将其序列化为
                    // Chat arguments String，并立即以完整 object 完成终值校验。
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
                    // signature 只保留给后续 native replay，计入驻留预算，
                    // 不进入普通 Chat delta。
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
    /// 在 transport clean EOF 后完成 Gemini 专属的最终快照。
    ///
    /// 若 output usage 从未被 terminal 后的最终快照确认，则先隐藏
    /// candidates/thought/total，只保留已证明输入。随后根据 blocked 或
    /// parts+finishReason 构造 native JSON，并调用非流式 decoder 与
    /// `Common::reconcile`。本阶段任何新增可见 delta 都说明增量与终态不一致，
    /// 必须失败。成功只表示协议终态完整；请求工具约束仍由
    /// `proxy::native::pending` 在 HTTP 层检查。
    pub(super) fn finish(&mut self, c: &mut Common) -> Result<(), Error> {
        if !self.usage_final
            && !super::super::usage::normalize(UpstreamProtocol::GeminiGenerateContent, &self.usage)
                .invalid
            && let Some(usage) = self.usage.as_object_mut()
        {
            for key in [
                "candidatesTokenCount",
                "thoughtsTokenCount",
                "totalTokenCount",
            ] {
                usage.remove(key);
            }
        }
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
        // Gemini 已逐 part 发出可见内容；终态允许补齐签名或工具参数一致性，
        // 但不应产生新的公开 text/tool delta。非空 delta 意味重复或错序。
        c.reconcile(out, &mut delta)?;
        // Every visible Gemini part was already emitted as an increment. A
        // nonempty fallback here would be lost after EOF and must fail safely.
        if !delta.is_empty() {
            return Err(bad("inconsistent_upstream_terminal"));
        }
        Ok(())
    }
}

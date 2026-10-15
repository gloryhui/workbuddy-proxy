//! Incremental native event state. Framing, transport and Chat serialization
//! remain pull-driven; this module never collects the original SSE stream.
mod anthropic;
mod gemini;
mod responses;
use super::{
    Error,
    config::UpstreamProtocol,
    output::{self, Output},
    request::{self, TOOL_ARGUMENT_LIMIT, TOOL_LIMIT},
    sse,
    usage::Usage,
};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Clone, Default)]
struct Track {
    len: usize,
    hash: Sha256,
    closed: bool,
}
impl Track {
    fn append(&mut self, s: &str) -> Result<(), Error> {
        if self.closed {
            return Err(bad("delta_after_block_done"));
        }
        self.len = self
            .len
            .checked_add(s.len())
            .filter(|n| *n <= output::OUTPUT_LIMIT)
            .ok_or_else(|| bad("upstream_output_too_large"))?;
        self.hash.update(s.as_bytes());
        Ok(())
    }
    fn tail<'a>(&self, full: &'a str) -> Result<&'a str, Error> {
        let prefix = full
            .get(..self.len)
            .ok_or_else(|| bad("inconsistent_upstream_snapshot"))?;
        if Sha256::digest(prefix.as_bytes())[..] != self.hash.clone().finalize()[..] {
            return Err(bad("inconsistent_upstream_snapshot"));
        }
        Ok(&full[self.len..])
    }
}
struct Tool {
    key: String,
    chat_id: String,
    native_id: String,
    name: String,
    args: String,
    done: bool,
}
#[derive(Default)]
struct Common {
    text: Track,
    reasoning: Track,
    refusal: Track,
    slots: BTreeMap<String, Track>,
    tools: Vec<Tool>,
    usage: Usage,
    model: Option<String>,
    size: usize,
    confirmed: bool,
    terminal: bool,
    final_output: Option<Output>,
}
fn bad(code: &'static str) -> Error {
    output::bad(code)
}
fn field<'a>(v: &'a Value, key: &str) -> Result<&'a str, Error> {
    output::text(v, key)
}
fn index(v: &Value, key: &str) -> Result<u64, Error> {
    v.get(key)
        .and_then(Value::as_u64)
        .filter(|n| *n < 4096)
        .ok_or_else(|| bad("invalid_upstream_index"))
}
fn add(delta: &mut Map<String, Value>, key: &str, s: &str) {
    if s.is_empty() {
        return;
    }
    delta
        .entry(key)
        .or_insert_with(|| json!(String::new()))
        .as_str()
        .expect("string delta");
    if let Value::String(old) = delta.get_mut(key).expect("inserted delta") {
        old.push_str(s);
    }
}
fn tool_delta(delta: &mut Map<String, Value>, v: Value) {
    let calls = delta
        .entry("tool_calls")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .expect("tools delta");
    if let Some(old) = calls.iter_mut().find(|old| old["index"] == v["index"]) {
        for key in ["id", "type"] {
            if let Some(value) = v.get(key) {
                old[key] = value.clone();
            }
        }
        if let Some(name) = v["function"].get("name") {
            old["function"]["name"] = name.clone();
        }
        if let Some(args) = v["function"]["arguments"].as_str() {
            let mut all = old["function"]["arguments"]
                .as_str()
                .unwrap_or("")
                .to_owned();
            all.push_str(args);
            old["function"]["arguments"] = json!(all);
        }
    } else {
        calls.push(v);
    }
}
impl Common {
    fn charge(&mut self, n: usize) -> Result<(), Error> {
        self.size = self
            .size
            .checked_add(n)
            .filter(|n| *n <= output::OUTPUT_LIMIT)
            .ok_or_else(|| bad("upstream_output_too_large"))?;
        Ok(())
    }
    fn model(&mut self, v: &Value, key: &str) -> Result<(), Error> {
        if let Some(value) = v.get(key) {
            let model = value
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control))
                .ok_or_else(|| bad("invalid_upstream_model"))?;
            if self.model.as_ref().is_some_and(|old| old != model) {
                return Err(bad("inconsistent_upstream_model"));
            }
            self.model = Some(model.into());
        }
        Ok(())
    }
    fn emit(
        &mut self,
        slot: &str,
        channel: &str,
        text: &str,
        snapshot: bool,
        close: bool,
        delta: &mut Map<String, Value>,
    ) -> Result<(), Error> {
        if self.terminal {
            return Err(bad("content_after_terminal"));
        }
        if !self.slots.contains_key(slot) {
            if self.slots.len() >= 4096 {
                return Err(bad("upstream_output_too_large"));
            }
            self.charge(slot.len() + 128)?;
        }
        let current = self.slots.entry(slot.into()).or_default();
        let text = if snapshot { current.tail(text)? } else { text };
        if !text.is_empty() {
            current.append(text)?;
            match channel {
                "content" => self.text.append(text)?,
                "reasoning_content" => self.reasoning.append(text)?,
                "refusal" => self.refusal.append(text)?,
                _ => return Err(bad("invalid_delta_channel")),
            }
            self.charge(text.len())?;
            add(delta, channel, text);
        }
        if close {
            self.slots.get_mut(slot).expect("registered slot").closed = true;
        }
        Ok(())
    }
    fn start_tool(
        &mut self,
        key: String,
        native_id: &str,
        name: &str,
        delta: &mut Map<String, Value>,
    ) -> Result<usize, Error> {
        if self.terminal {
            return Err(bad("content_after_terminal"));
        }
        if let Some(i) = self.tools.iter().position(|t| t.key == key) {
            let t = &self.tools[i];
            if t.native_id != native_id || t.name != name {
                return Err(bad("inconsistent_upstream_tool"));
            }
            return Ok(i);
        }
        if self.tools.len() >= TOOL_LIMIT
            || name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            || native_id.len() > 256
            || !native_id.is_empty() && self.tools.iter().any(|t| t.native_id == native_id)
        {
            return Err(bad("invalid_upstream_tool_call"));
        }
        self.charge(key.len() + native_id.len() + name.len() + 128)?;
        let id = super::replay::tool_id();
        let i = self.tools.len();
        tool_delta(
            delta,
            json!({"index":i,"id":id,"type":"function","function":{"name":name,"arguments":""}}),
        );
        self.tools.push(Tool {
            key,
            chat_id: id,
            native_id: native_id.into(),
            name: name.into(),
            args: String::new(),
            done: false,
        });
        Ok(i)
    }
    fn args(
        &mut self,
        i: usize,
        s: &str,
        snapshot: bool,
        done: bool,
        delta: &mut Map<String, Value>,
    ) -> Result<(), Error> {
        let t = self
            .tools
            .get_mut(i)
            .ok_or_else(|| bad("invalid_upstream_tool_index"))?;
        let tail = if snapshot {
            s.strip_prefix(&t.args)
                .ok_or_else(|| bad("inconsistent_upstream_tool_arguments"))?
        } else {
            s
        };
        if t.done && !tail.is_empty() {
            return Err(bad("tool_delta_after_done"));
        }
        if t.args.len() + tail.len() > TOOL_ARGUMENT_LIMIT {
            return Err(bad("upstream_tool_arguments_too_large"));
        }
        t.args.push_str(tail);
        if !tail.is_empty() {
            tool_delta(delta, json!({"index":i,"function":{"arguments":tail}}));
        }
        if done {
            if !request::strict_json(t.args.as_bytes(), TOOL_ARGUMENT_LIMIT)
                .map_err(|_| bad("invalid_upstream_tool_arguments"))?
                .is_object()
            {
                return Err(bad("invalid_upstream_tool_arguments"));
            }
            t.done = true;
        }
        let n = tail.len();
        self.charge(n)?;
        Ok(())
    }
    /// Terminal full snapshots validate already-emitted prefixes. Only their
    /// unseen suffix is emitted; tool IDs/indexes keep their first assignment.
    fn reconcile(&mut self, mut out: Output, delta: &mut Map<String, Value>) -> Result<(), Error> {
        let text = out.assistant.text();
        for (channel, full) in [
            ("content", text.as_str()),
            ("reasoning_content", out.reasoning.as_str()),
            ("refusal", out.refusal.as_deref().unwrap_or("")),
        ] {
            let track = match channel {
                "content" => &mut self.text,
                "reasoning_content" => &mut self.reasoning,
                _ => &mut self.refusal,
            };
            let tail = track.tail(full)?;
            track.append(tail)?;
            add(delta, channel, tail);
            self.charge(tail.len())?;
        }
        let mut used = Vec::new();
        if let Some(replay) = out.replay.as_mut() {
            let mut rebound = BTreeMap::new();
            for (ordinal, call) in out.assistant.calls.iter_mut().enumerate() {
                let native = replay
                    .calls
                    .remove(&call.id)
                    .ok_or_else(|| bad("invalid_replay_state"))?;
                let found = if native.id.is_empty() {
                    self.tools
                        .iter()
                        .enumerate()
                        .find(|(i, t)| {
                            !used.contains(i) && t.native_id.is_empty() && t.name == call.name
                        })
                        .map(|(i, _)| i)
                } else {
                    self.tools.iter().position(|t| t.native_id == native.id)
                };
                let i = match found {
                    Some(i) => i,
                    None => self.start_tool(
                        format!("terminal:{ordinal}"),
                        &native.id,
                        &call.name,
                        delta,
                    )?,
                };
                if used.contains(&i) || self.tools[i].name != call.name {
                    return Err(bad("inconsistent_upstream_tool"));
                }
                if let Ok(existing) =
                    request::strict_json(self.tools[i].args.as_bytes(), TOOL_ARGUMENT_LIMIT)
                {
                    if existing != call.arguments {
                        return Err(bad("inconsistent_upstream_tool_arguments"));
                    }
                    self.tools[i].done = true;
                } else {
                    // Responses arguments are a JSON string. Preserve its
                    // original whitespace when a terminal snapshot completes
                    // an earlier partial delta; never append a compacted copy.
                    let original = replay
                        .assistant
                        .as_array()
                        .and_then(|items| {
                            items
                                .iter()
                                .find(|v| v["type"] == "function_call" && v["call_id"] == native.id)
                        })
                        .and_then(|v| v["arguments"].as_str())
                        .unwrap_or(&call.arguments_text);
                    self.args(i, original, true, true, delta)?;
                }
                call.id = self.tools[i].chat_id.clone();
                used.push(i);
                rebound.insert(call.id.clone(), native);
            }
            replay.calls = rebound;
        }
        if used.len() != self.tools.len() {
            return Err(bad("incomplete_upstream_tool_call"));
        }
        out.assistant.calls.sort_by_key(|c| {
            self.tools
                .iter()
                .position(|t| t.chat_id == c.id)
                .expect("rebound tool")
        });
        self.usage = out.usage.clone();
        self.terminal = true;
        self.final_output = Some(out);
        Ok(())
    }
}

enum Native {
    Responses(responses::State),
    Anthropic(anthropic::State),
    Gemini(gemini::State),
}
pub struct Decoder {
    common: Common,
    native: Native,
}
pub struct Step {
    pub confirmed: bool,
    pub delta: Value,
}
impl Decoder {
    pub fn new(protocol: UpstreamProtocol) -> Result<Self, Error> {
        Ok(Self {
            common: Common::default(),
            native: match protocol {
                UpstreamProtocol::OpenaiResponses => Native::Responses(responses::State::default()),
                UpstreamProtocol::AnthropicMessages => {
                    Native::Anthropic(anthropic::State::default())
                }
                UpstreamProtocol::GeminiGenerateContent => Native::Gemini(gemini::State::default()),
                UpstreamProtocol::OpenaiChat => return Err(bad("invalid_protocol")),
            },
        })
    }
    pub fn model(&self) -> Option<&str> {
        self.common.model.as_deref()
    }
    pub fn usage(&self) -> &Usage {
        &self.common.usage
    }
    pub fn terminal_seen(&self) -> bool {
        self.common.terminal || matches!(&self.native,Native::Gemini(s) if s.terminal_seen())
    }
    pub fn event(&mut self, event: sse::Event) -> Result<Step, Error> {
        let v = request::strict_json(event.data.as_bytes(), sse::FRAME_LIMIT)
            .map_err(|_| bad("invalid_upstream_event_json"))?;
        if !v.is_object() {
            return Err(bad("invalid_upstream_event_json"));
        }
        if v.get("error").is_some_and(|v| !v.is_null()) {
            return Err(bad("upstream_stream_error"));
        }
        let mut delta = Map::new();
        match &mut self.native {
            Native::Responses(s) => s.event(&event.kind, v, &mut self.common, &mut delta)?,
            Native::Anthropic(s) => s.event(&event.kind, v, &mut self.common, &mut delta)?,
            Native::Gemini(s) => s.event(&event.kind, v, &mut self.common, &mut delta)?,
        }
        Ok(Step {
            confirmed: self.common.confirmed,
            delta: Value::Object(delta),
        })
    }
    /// Called only after the frame parser and transport reached clean EOF.
    /// A terminal marker alone never turns malformed/truncated tails into DONE.
    pub fn finish(mut self) -> Result<Output, Error> {
        if let Native::Gemini(s) = &mut self.native {
            s.finish(&mut self.common)?;
        }
        self.common
            .final_output
            .take()
            .ok_or_else(|| bad("incomplete_upstream_stream"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wire(events: &[Value], named: bool) -> Vec<u8> {
        events
            .iter()
            .map(|v| {
                format!(
                    "{}data: {}\r\n\r\n",
                    if named {
                        format!("event: {}\r\n", v["type"].as_str().unwrap())
                    } else {
                        String::new()
                    },
                    v
                )
            })
            .collect::<String>()
            .into_bytes()
    }
    fn consume(
        protocol: UpstreamProtocol,
        bytes: &[u8],
        size: usize,
    ) -> Result<(Vec<Value>, Output), Error> {
        let mut parser = sse::Parser::default();
        let mut decoder = Decoder::new(protocol)?;
        let mut deltas = vec![];
        for chunk in bytes.chunks(size) {
            let mut rest = chunk;
            while !rest.is_empty() {
                let (n, frame) = parser.feed(rest)?;
                rest = &rest[n..];
                if let Some(frame) = frame
                    && let Some(event) = frame.into_event()?
                {
                    let step = decoder.event(event)?;
                    if !step.delta.as_object().unwrap().is_empty() {
                        deltas.push(step.delta);
                    }
                }
            }
        }
        parser.finish()?;
        Ok((deltas, decoder.finish()?))
    }
    fn visible(deltas: &[Value], channel: &str) -> String {
        deltas.iter().filter_map(|v| v[channel].as_str()).collect()
    }
    fn tools(deltas: &[Value]) -> BTreeMap<u64, (String, String, String)> {
        let mut calls: BTreeMap<u64, (String, String, String)> = BTreeMap::new();
        for delta in deltas {
            if let Some(rows) = delta["tool_calls"].as_array() {
                let mut in_event = std::collections::BTreeSet::new();
                for row in rows {
                    let index = row["index"].as_u64().unwrap();
                    assert!(in_event.insert(index), "duplicate index in one Chat delta");
                    let call = calls.entry(index).or_default();
                    if let Some(id) = row["id"].as_str() {
                        assert!(call.0.is_empty());
                        call.0 = id.into();
                    }
                    if let Some(name) = row["function"]["name"].as_str() {
                        call.1.push_str(name);
                    }
                    if let Some(args) = row["function"]["arguments"].as_str() {
                        call.2.push_str(args);
                    }
                }
            }
        }
        calls
    }
    fn response_events() -> Vec<Value> {
        let message = json!({"id":"msg","type":"message","role":"assistant","content":[{"type":"output_text","text":"你好","annotations":[]}]});
        let call = json!({"id":"fc","type":"function_call","call_id":"native-a","name":"find","arguments":"{ \"q\": 1 }"});
        vec![
            json!({"type":"response.created","response":{"id":"resp","model":"real"}}),
            json!({"type":"response.output_item.added","output_index":0,"item":{"id":"msg","type":"message","role":"assistant","content":[]}}),
            json!({"type":"response.content_part.added","output_index":0,"item_id":"msg","content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),
            json!({"type":"response.output_text.delta","output_index":0,"item_id":"msg","content_index":0,"delta":"你"}),
            json!({"type":"response.output_text.delta","output_index":0,"item_id":"msg","content_index":0,"delta":"好"}),
            json!({"type":"response.output_text.done","output_index":0,"item_id":"msg","content_index":0,"text":"你好"}),
            json!({"type":"response.content_part.done","output_index":0,"item_id":"msg","content_index":0,"part":{"type":"output_text","text":"你好","annotations":[]}}),
            json!({"type":"response.output_item.done","output_index":0,"item":message}),
            json!({"type":"response.output_item.added","output_index":1,"item":{"id":"fc","type":"function_call","call_id":"native-a","name":"find","arguments":""}}),
            json!({"type":"response.function_call_arguments.delta","output_index":1,"item_id":"fc","delta":"{ \"q\":"}),
            json!({"type":"response.function_call_arguments.delta","output_index":1,"item_id":"fc","delta":" 1 }"}),
            json!({"type":"response.function_call_arguments.done","output_index":1,"item_id":"fc","arguments":"{ \"q\": 1 }"}),
            json!({"type":"response.output_item.done","output_index":1,"item":call}),
            json!({"type":"response.completed","response":{"id":"resp","model":"real","status":"completed","output":[message,call],"usage":{"input_tokens":10,"output_tokens":4,"total_tokens":14}}}),
        ]
    }
    fn anthropic_events() -> Vec<Value> {
        vec![
            json!({"type":"message_start","message":{"id":"msg","type":"message","role":"assistant","model":"real","content":[],"stop_reason":null,"usage":{"input_tokens":10,"cache_read_input_tokens":40,"cache_creation_input_tokens":5,"output_tokens":0}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"plan"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"private"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"你好"}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"native-a","name":"find","input":{}}}),
            json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{ \"q\": 1 }"}}),
            json!({"type":"content_block_stop","index":2}),
            json!({"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"native-b","name":"find","input":{}}}),
            json!({"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"{\"q\":2}"}}),
            json!({"type":"content_block_stop","index":3}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":7}}),
            json!({"type":"message_stop"}),
        ]
    }
    fn gemini_events() -> Vec<Value> {
        vec![
            json!({"modelVersion":"real","candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"plan","thought":true,"thoughtSignature":"private-text"},{"text":"你好"},{"functionCall":{"name":"find","args":{"q":1}},"thoughtSignature":"private-call"}]}}]}),
            json!({"candidates":[{"content":{"parts":[{"functionCall":{"name":"find","args":{"q":2}}}]},"finishReason":"STOP"}]}),
            json!({"candidates":[{"content":{"parts":[{"thoughtSignature":"private-tail"}]}}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":4,"thoughtsTokenCount":3,"totalTokenCount":17}}),
        ]
    }
    #[test]
    fn responses_delta_done_and_full_snapshots_do_not_duplicate_text_or_arguments() {
        let bytes = wire(&response_events(), true);
        for size in [1, 2, 7, 257, bytes.len()] {
            let (deltas, out) = consume(UpstreamProtocol::OpenaiResponses, &bytes, size).unwrap();
            assert_eq!(visible(&deltas, "content"), "你好");
            assert_eq!(out.finish, output::Finish::Tools);
            let calls = tools(&deltas);
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[&0].2, "{ \"q\": 1 }");
            assert_eq!(out.assistant.calls[0].id, calls[&0].0);
            assert_eq!(out.usage.chat["total_tokens"], 14);
        }
    }
    #[test]
    fn response_terminal_only_fallback_emits_once_and_keeps_stable_tool_indexes() {
        let events = response_events();
        let bytes = wire(&events[events.len() - 1..], true);
        let (deltas, out) = consume(UpstreamProtocol::OpenaiResponses, &bytes, 1).unwrap();
        assert_eq!(visible(&deltas, "content"), "你好");
        let calls = tools(&deltas);
        assert_eq!(calls.len(), 1);
        assert_eq!(out.assistant.calls[0].id, calls[&0].0);
    }
    #[test]
    fn anthropic_thinking_parallel_calls_and_cumulative_usage_are_separate() {
        let bytes = wire(&anthropic_events(), true);
        for size in [1, 11, 1024, bytes.len()] {
            let (deltas, out) = consume(UpstreamProtocol::AnthropicMessages, &bytes, size).unwrap();
            assert_eq!(visible(&deltas, "content"), "你好");
            assert_eq!(visible(&deltas, "reasoning_content"), "plan");
            assert!(!serde_json::to_string(&deltas).unwrap().contains("private"));
            let calls = tools(&deltas);
            assert_eq!(calls.len(), 2);
            assert_ne!(calls[&0].0, calls[&1].0);
            assert_eq!(calls[&0].1, "find");
            assert_eq!(out.assistant.calls[1].id, calls[&1].0);
            assert_eq!(out.usage.chat["prompt_tokens"], 55);
            assert_eq!(out.usage.chat["completion_tokens"], 7);
            assert_eq!(out.replay.unwrap().assistant[0]["signature"], "private");
        }
    }
    #[test]
    fn gemini_waits_for_trailing_usage_and_preserves_any_part_signatures() {
        let bytes = wire(&gemini_events(), false);
        for size in [1, 13, bytes.len()] {
            let (deltas, out) =
                consume(UpstreamProtocol::GeminiGenerateContent, &bytes, size).unwrap();
            assert_eq!(visible(&deltas, "content"), "你好");
            assert_eq!(visible(&deltas, "reasoning_content"), "plan");
            assert!(!serde_json::to_string(&deltas).unwrap().contains("private"));
            let calls = tools(&deltas);
            assert_eq!(calls.len(), 2);
            assert_ne!(calls[&0].0, calls[&1].0);
            assert_eq!(out.usage.chat["completion_tokens"], 7);
            let replay = out.replay.unwrap();
            assert_eq!(
                replay.assistant["parts"][0]["thoughtSignature"],
                "private-text"
            );
            assert_eq!(
                replay.assistant["parts"][4]["thoughtSignature"],
                "private-tail"
            );
        }
    }
    #[test]
    fn missing_terminal_truncated_frames_mismatched_types_and_unknown_content_fail() {
        for (protocol, events, named) in [
            (UpstreamProtocol::OpenaiResponses, response_events(), true),
            (
                UpstreamProtocol::AnthropicMessages,
                anthropic_events(),
                true,
            ),
            (
                UpstreamProtocol::GeminiGenerateContent,
                gemini_events(),
                false,
            ),
        ] {
            let first = wire(&events[..1], named);
            assert!(consume(protocol, &first, 1).is_err());
            let mut full = wire(&events, named);
            full.pop();
            full.pop();
            assert!(consume(protocol, &full, 1).is_err());
        }
        let mut d = Decoder::new(UpstreamProtocol::AnthropicMessages).unwrap();
        assert!(
            d.event(sse::Event {
                kind: "message_start".into(),
                data: json!({"type":"content_block_delta"}).to_string()
            })
            .is_err()
        );
        let mut d = Decoder::new(UpstreamProtocol::OpenaiResponses).unwrap();
        assert!(
            d.event(sse::Event {
                kind: "message".into(),
                data: json!({"type":"response.image.delta","delta":"secret"}).to_string()
            })
            .is_err()
        );
        let mut events = gemini_events();
        events.push(json!({"candidates":[{"content":{"parts":[{"text":"late"}]}}]}));
        assert!(
            consume(
                UpstreamProtocol::GeminiGenerateContent,
                &wire(&events, false),
                1
            )
            .is_err()
        );
    }

    #[test]
    fn standard_chat_history_replays_two_tool_rounds_without_private_fields() {
        use super::super::{
            config::ProtocolSettings,
            encode,
            replay::{Cache, Pending},
            request::ChatRequest,
        };
        use std::time::Instant;
        for (protocol, events, named, secret) in [
            (
                UpstreamProtocol::OpenaiResponses,
                response_events(),
                true,
                "private-reasoning",
            ),
            (
                UpstreamProtocol::AnthropicMessages,
                anthropic_events(),
                true,
                "private",
            ),
            (
                UpstreamProtocol::GeminiGenerateContent,
                gemini_events(),
                false,
                "private-tail",
            ),
        ] {
            let mut cache = Cache::default();
            let now = Instant::now();
            let route = [7; 32];
            let mut messages = json!([{ "role":"user","content":"hello" }]);
            let declared_tools = json!([{ "type":"function","function":{"name":"find","parameters":{"type":"object","properties":{"q":{"type":"integer"}}}} }]);
            for round in 0..2 {
                let current = json!({"model":"local","messages":messages,"tools":declared_tools});
                let request = ChatRequest::parse(&serde_json::to_vec(&current).unwrap()).unwrap();
                let mut events = events.clone();
                if protocol == UpstreamProtocol::OpenaiResponses {
                    events.last_mut().unwrap()["response"]["output"].as_array_mut().unwrap().push(json!({"id":"reasoning","type":"reasoning","summary":[],"encrypted_content":secret}));
                }
                let wire = String::from_utf8(wire(&events, named))
                    .unwrap()
                    .replace("native-a", &format!("native-a-{round}"))
                    .replace("native-b", &format!("native-b-{round}"));
                let (_, out) = consume(protocol, wire.as_bytes(), 3).unwrap();
                out.validate_request(&request).unwrap();
                cache
                    .commit(
                        Pending::new(
                            route,
                            &request.messages,
                            &out.assistant,
                            out.replay.clone().unwrap(),
                        )
                        .unwrap(),
                        now,
                    )
                    .unwrap();
                // Independent standard Chat consumer: keep no reasoning_content,
                // signatures, native IDs or protocol-specific JSON properties.
                let reply = out.json(&output::Identity::new("real"));
                let m = &reply["choices"][0]["message"];
                let assistant =
                    json!({"role":"assistant","content":m["content"],"tool_calls":m["tool_calls"]});
                messages.as_array_mut().unwrap().push(assistant);
                for call in m["tool_calls"].as_array().unwrap().iter().rev() {
                    messages.as_array_mut().unwrap().push(json!({"role":"tool","tool_call_id":call["id"],"content":format!("non-JSON result {round}")}));
                }
                let next = ChatRequest::parse(
                    &serde_json::to_vec(
                        &json!({"model":"local","messages":messages,"tools":declared_tools}),
                    )
                    .unwrap(),
                )
                .unwrap();
                let resolved = cache.resolve(&route, &next.messages, now).unwrap();
                assert_eq!(resolved.len(), round + 1);
                let native = encode::encode(
                    protocol,
                    "real",
                    &ProtocolSettings::default(),
                    &next,
                    &resolved,
                )
                .unwrap();
                assert!(native.to_string().contains(secret));
                if protocol == UpstreamProtocol::GeminiGenerateContent {
                    for turn in native["contents"].as_array().unwrap() {
                        for part in turn["parts"].as_array().unwrap() {
                            if let Some(result) = part.get("functionResponse") {
                                assert!(result.get("id").is_none());
                                assert!(
                                    result["response"]["result"]
                                        .as_str()
                                        .unwrap()
                                        .starts_with("non-JSON")
                                );
                            }
                        }
                    }
                }
            }
            assert!(!messages.to_string().contains("private"));
            let final_native = match protocol {
                UpstreamProtocol::OpenaiResponses => {
                    json!({"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]}]})
                }
                UpstreamProtocol::AnthropicMessages => {
                    json!({"type":"message","role":"assistant","content":[{"type":"text","text":"done"}],"stop_reason":"end_turn"})
                }
                _ => {
                    json!({"candidates":[{"content":{"role":"model","parts":[{"text":"done"}]},"finishReason":"STOP"}]})
                }
            };
            let final_output = output::decode(protocol, final_native).unwrap();
            assert_eq!(final_output.finish, output::Finish::Stop);
            assert_eq!(final_output.assistant.text(), "done");
        }
    }

    #[test]
    fn invalid_usage_does_not_abort_valid_native_output() {
        use crate::stats::dto::UsageStatus;
        let mut events = anthropic_events();
        let n = events.len();
        events[n - 2]["usage"] = json!({"output_tokens":-1});
        let (_, out) =
            consume(UpstreamProtocol::AnthropicMessages, &wire(&events, true), 5).unwrap();
        assert_eq!(out.finish, output::Finish::Tools);
        assert_eq!(out.usage.outcome().status, UsageStatus::Invalid);
        let mut events = gemini_events();
        let n = events.len();
        events[n - 1]["usageMetadata"] = json!("bad-metadata");
        let (_, out) = consume(
            UpstreamProtocol::GeminiGenerateContent,
            &wire(&events, false),
            5,
        )
        .unwrap();
        assert_eq!(out.finish, output::Finish::Tools);
        assert_eq!(out.usage.outcome().status, UsageStatus::Invalid);
    }

    #[test]
    fn empty_delta_slots_and_tool_arguments_have_real_resident_bounds() {
        let mut common = Common::default();
        let mut delta = Map::new();
        for i in 0..4096 {
            common
                .emit(
                    &format!("slot-{i}"),
                    "content",
                    "",
                    false,
                    false,
                    &mut delta,
                )
                .unwrap();
        }
        assert!(
            common
                .emit("extra-slot", "content", "", false, false, &mut delta)
                .is_err()
        );
        let t = common
            .start_tool("tool".into(), "native", "find", &mut delta)
            .unwrap();
        assert_eq!(
            common
                .args(
                    t,
                    &"x".repeat(TOOL_ARGUMENT_LIMIT + 1),
                    false,
                    false,
                    &mut delta
                )
                .unwrap_err()
                .code,
            "upstream_tool_arguments_too_large"
        );
        assert!(common.tools[t].args.is_empty());
        let mut decoder = Decoder::new(UpstreamProtocol::OpenaiResponses).unwrap();
        assert!(decoder.event(sse::Event{kind:"message".into(),data:json!({"type":"response.output_item.added","output_index":0,"item":{"id":"x".repeat(257),"type":"message"}}).to_string()}).is_err());
    }
}

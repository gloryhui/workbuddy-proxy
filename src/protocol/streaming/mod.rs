//! Incremental native event state. Framing, transport and Chat serialization
//! remain pull-driven; this module never collects the original SSE stream.
//!
//! 本模块拥有一次流式响应的协议状态。上游是 `sse::Event`（已经完成网络
//! chunk -> SSE Frame -> Event 的转换），下游是代理响应流：`Step.delta` 被
//! `proxy::native::DeltaCursor` 有界切片并包装成 Chat chunk，最终 `Output`
//! 再生成 finish、可选 usage 与 `[DONE]`。真实流驱动类型是
//! `proxy::native::NativeStream`；本模块的公共类型只有 `streaming::Decoder`
//! 和它返回的 `Step`，不存在名为 `Stream` 的适配器类型。
//!
//! `Decoder` 与 `NativeStream` 都由单个响应请求拥有，沿着 body future 可变
//! 借用前进；未启动独立线程，也不跨客户端共享。网络 chunk、SSE Frame、原生
//! Event、统一 Chat delta 是四种不同层次：一个网络 chunk 可含多个 Event，一个
//! Event 也可跨多个网络 chunk；一个原生 Event 还可能只更新 usage、只关闭 block，
//! 因而产生空 delta。
//!
//! `Common` 只保存跨协议语义：每一段已发出文本的长度/SHA-256 前缀证明、
//! 工具参数终值、usage、模型和最终 Output。它不把全部正文一直聚合成 String。
//! 为生成真实 replay/final snapshot，Anthropic 和 Gemini 适配器会保留各自的
//! native block/part 状态，Responses 会保留 item 身份和工具结构；这些保留范围
//! 受 `OUTPUT_LIMIT`、tool 参数上限和 slot 上限约束，不能笼统说整个原始 SSE
//! 流都被收集。
//!
//! 协议 marker、传输 EOF 和成功 DONE 也不是同一件事：`terminal_seen()` 只说明
//! 已看到终态线索。调用方必须先检查 Parser/transport clean EOF，再调用
//! `Decoder::finish`；Decoder 不持有 Parser，只负责协议 final snapshot 和工具
//! 参数闭合。工具选择约束再由 `proxy::native::pending` 在 finish 之后校验。
//! 任何协议/transport 错误都应由调用方发送错误事件并结束，不能补发成功
//! finish 或 DONE。
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

/// 一个文本/推理/refusal 通道的已观测前缀证明。
///
/// 本结构**不保存正文**，只保存已向客户端发出内容的字节长度和 SHA-256 增量
/// 摘要。终态 full snapshot 到来时，可用它确认 snapshot 的前缀正是此前发出的
/// 内容，再只补发未见后缀，避免重复文本。`len` 的单位是 UTF-8 字节。
#[derive(Clone, Default)]
struct Track {
    /// 已确认发出的 UTF-8 字节数。
    len: usize,
    /// 这些字节的增量 SHA-256；用于校验 snapshot 前缀一致。
    hash: Sha256,
    /// 对应协议 block/part 已关闭后，不允许继续向该 slot 追加内容。
    closed: bool,
}
impl Track {
    /// 记录一段已发出的文本前缀。
    ///
    /// 超限或已关闭时失败；失败不能让调用方继续把截断内容视为成功。
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
    /// 比较 full snapshot 的前缀并借用其未见后缀。
    ///
    /// 返回的 `&str` 生命周期绑定 `full`，调用方只在本事件内使用，不长期持有；
    /// 这样无需把整段 snapshot 再复制到 Common。
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
/// 跨协议统一后的一个函数调用累积状态。
struct Tool {
    /// 适配器自己的稳定 key，例如 Responses 的 output_index 或 Anthropic
    /// content block index，用于把后续事件归到同一个调用。
    key: String,
    /// 本代理生成并已发给客户端的 Chat tool id（`wbpt_...`）。
    chat_id: String,
    /// 上游 call id；Gemini/部分响应可能没有，因此允许空字符串。
    native_id: String,
    /// 已校验的 ASCII 函数名。
    name: String,
    /// 按协议增量拼接后的 JSON 文本；这是为了最终严格解析和 replay，而不是
    /// 每帧都把半截 JSON 当对象执行。
    args: String,
    /// 是否已经收到该工具块的明确完成信号并验证 args 为 JSON object。
    done: bool,
}
/// 三个原生协议共享的流式语义状态。
#[derive(Default)]
struct Common {
    /// 已发出的可见正文前缀证明，不保留正文 String。
    text: Track,
    /// 已发出的 reasoning summary 前缀证明，不保留正文。
    reasoning: Track,
    /// 已发出的 refusal 文本前缀证明，不保留正文。
    refusal: Track,
    /// 原生 item/content/part slot -> 前缀证明。用于按独立块判断 delta/done
    /// 顺序和 snapshot 一致性；仍然只保存长度与 hash。
    slots: BTreeMap<String, Track>,
    /// 已开始的工具调用。这里确实保留 args JSON 文本，因为 tool stop 时要严格
    /// 解析成 object，后续 replay 也需要原始参数。
    tools: Vec<Tool>,
    /// 当前经过协议规则规范化的 usage。流式累计值在适配器中替换更新，不累加
    /// 重复快照。
    usage: Usage,
    /// 各事件中一致的上游模型名；冲突会使流失败。
    model: Option<String>,
    /// 已知持久 state 的保守字节预算，用于限制 slot key、工具字段和文本等新增
    /// 驻留量；不是上游传输的总字节计数器。
    size: usize,
    /// 是否已收到足以确定 assistant 身份/内容的协议确认。代理用它决定何时
    /// 先发 role chunk；它不代表整个响应成功完成。
    confirmed: bool,
    /// 是否已由完整 terminal snapshot 构建并校验 `final_output`。设置后禁止
    /// 继续出现可见内容/工具事件。
    terminal: bool,
    /// `reconcile` 产出的最终统一结果；由 `Decoder::finish` 取走一次。
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
    /// 增加驻留 state 预算。超限即协议失败，避免只有网络 chunk 有限而解析状态
    /// 仍无限增长。
    fn charge(&mut self, n: usize) -> Result<(), Error> {
        self.size = self
            .size
            .checked_add(n)
            .filter(|n| *n <= output::OUTPUT_LIMIT)
            .ok_or_else(|| bad("upstream_output_too_large"))?;
        Ok(())
    }
    /// 读取并统一各协议的模型名；一旦已有值就必须完全一致。
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
    /// 把一个协议 slot 的新增可见文本追加到统一通道和当前 Chat delta。
    ///
    /// `snapshot=true` 表示输入是该 slot 的累计全文，并非新增后缀；先用
    /// Track 校验此前已发前缀，只追加未见部分。`close=true` 在本次处理后
    /// 关闭该 slot；关闭后再次提交非空文本会失败，空 snapshot 不会触发 append。
    /// 即使文本为空，也可能借此登记/关闭 slot，因此 slot 数量本身也受 4096
    /// 上限约束。
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
    /// 登记一个跨协议工具调用，并固定客户端看到的 index/id。
    ///
    /// 同一协议 key 可被后续事件再次提到，但 native id 和 name 必须一致。
    /// 首次出现时生成本地 `wbpt_` id，并立即形成 Chat tool call 头；后续参数
    /// 只按固定 index 追加。原生 ID 为空时仍用协议索引保证本地身份稳定。
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
    /// 累积一个工具的 JSON 参数字符串。
    ///
    /// `snapshot=true` 表示 `s` 是该工具的累计参数全文，必须以前面已保存文本
    /// 为前缀，只取后缀；否则 `s` 是真正增量。只有 `done=true`（Responses/
    /// Anthropic 的完成事件或 Gemini 的完整对象）才严格解析整个 args，并要求
    /// 顶层是 JSON object。半截 JSON 可以中转给客户端，但不能进入 replay。
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
    ///
    /// 调用方是三个适配器的 `completed/message_stop/Gemini EOF finish` 路径。
    /// 步骤是：逐通道校验 snapshot 前缀并补发后缀；按 native ID 或协议顺序
    /// 重绑定 terminal tool calls，补齐早先未完成的参数；拒绝漏工具/重复工具/
    /// 参数不一致；用同一个 Output 的 usage 覆盖当前 usage；设置 terminal 与
    /// final_output。只有完成这些校验，本次流才有最终成功候选。
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

/// 当前响应选中的协议状态机。
///
/// 每个 State 只保存该协议自己的 event 顺序、native item/block/part 和终态；
/// `Common` 只接收已归一化的语义结果。新增协议必须同时实现非流式 Output 解码
/// 和这里的增量状态，不能假定三种事件名称或工具参数形式相同。
enum Native {
    Responses(responses::State),
    Anthropic(anthropic::State),
    Gemini(gemini::State),
}
/// 一次流式原生响应的跨协议解码器。
///
/// 它不拥有网络流或 SSE Parser，只拥有解析后的协议状态；调用方在 clean EOF
/// 后用 `finish` 取走最终 Output。请求任务可跨 await 持有它，但同一时刻只有
/// 一个可变借用进入 `event`，无需额外锁。
pub struct Decoder {
    common: Common,
    native: Native,
}
/// 一个原生 Event 转换后的 Chat 增量结果。
pub struct Step {
    /// 从流开始到本事件为止，是否已能从协议内容确认 assistant；一旦为 true
    /// 通常保持 true。它用于发一次 role chunk，不表示 terminal。
    pub confirmed: bool,
    /// 可直接转成 Chat delta 的字段。可为空 object（例如 usage/stop-only 事件）；
    /// 调用方不应把空的 `{}` 当作用户可见内容。
    pub delta: Value,
}
impl Decoder {
    /// 按 destination protocol 创建对应状态机。
    ///
    /// `OpenaiChat` 走原有字节转发，不由 native adapter 解码，因此返回输入错误。
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
    /// 当前已确认的上游模型名，供 role chunk 和统计观察使用。
    pub fn model(&self) -> Option<&str> {
        self.common.model.as_deref()
    }
    /// 当前流式 usage 快照。调用方只借用，不取得所有权；它可能仍是 partial。
    pub fn usage(&self) -> &Usage {
        &self.common.usage
    }
    /// 是否已经看到协议 terminal marker 或完整 terminal snapshot。
    ///
    /// Gemini 在 finishReason/blockReason 出现时即返回 true，Anthropic/Responses
    /// 主要由 `Common.terminal` 决定。该值用于缩短 terminal 后等待 EOF 的时间，
    /// 不能单独作为成功条件。
    pub fn terminal_seen(&self) -> bool {
        self.common.terminal || matches!(&self.native,Native::Gemini(s) if s.terminal_seen())
    }
    /// 消费一个已分帧 Event 并推进对应协议状态。
    ///
    /// Event data 必须是 1 MiB Frame 上限内的 object JSON；非 null `error`
    /// 立即失败。适配器可能返回空 delta，也可能更新 usage/model/terminal。
    /// 本函数验证单个事件的类型、身份、顺序和增量一致性，但不知道 transport
    /// 是否已经干净结束，因此不会在这里发送 DONE。
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
    ///
    /// Gemini 必须在这里用累计 parts、finishReason 和 usage 组装 final native
    /// snapshot；Responses/Anthropic 的 terminal event 已调用 `reconcile`。随后
    /// `final_output` 必须存在，否则尾部被截断。该方法消耗 Decoder，不会自行
    /// 调用 HTTP 层 `Output::validate_request`；后者在
    /// `proxy::native::pending` 中完成，并决定能否发布 replay。
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
                let mut next = ChatRequest::parse(
                    &serde_json::to_vec(
                        &json!({"model":"local","messages":messages,"tools":declared_tools}),
                    )
                    .unwrap(),
                )
                .unwrap();
                // A standard client may reorder an older parallel result
                // group too; identical ID/content associations stay canonical.
                let mut cursor = 0;
                while cursor < next.messages.len() {
                    if next.messages[cursor].role == super::super::request::Role::Tool {
                        let end = cursor
                            + next.messages[cursor..]
                                .iter()
                                .take_while(|m| m.role == super::super::request::Role::Tool)
                                .count();
                        next.messages[cursor..end].reverse();
                        cursor = end;
                    } else {
                        cursor += 1;
                    }
                }
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

    #[test]
    fn initial_counters_are_not_a_final_zero_output_bill() {
        for (protocol, first) in [
            (
                UpstreamProtocol::AnthropicMessages,
                anthropic_events().remove(0),
            ),
            (
                UpstreamProtocol::GeminiGenerateContent,
                json!({"candidates":[{"content":{"parts":[{"text":"early"}]}}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":0,"totalTokenCount":10}}),
            ),
        ] {
            let mut decoder = Decoder::new(protocol).unwrap();
            let kind = first
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("message")
                .to_owned();
            decoder
                .event(sse::Event {
                    kind,
                    data: first.to_string(),
                })
                .unwrap();
            assert!(!decoder.usage().complete);
            assert!(decoder.usage().chat.get("completion_tokens").is_none());
            assert!(decoder.usage().chat.get("total_tokens").is_none());
            assert!(decoder.usage().outcome().numbers.output.is_none());
        }
        let events = vec![
            json!({"candidates":[{"content":{"parts":[{"text":"early"}]}}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":0,"totalTokenCount":10}}),
            json!({"candidates":[{"finishReason":"STOP"}]}),
            json!({"candidates":[{"content":{"parts":[{"text":"","thoughtSignature":"tail"}]}}]}),
        ];
        let (_, out) = consume(
            UpstreamProtocol::GeminiGenerateContent,
            &wire(&events, false),
            1,
        )
        .unwrap();
        assert!(!out.usage.complete);
        assert!(out.usage.chat.get("completion_tokens").is_none());
    }
}

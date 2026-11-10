//! Bounded, allocation-conscious observation of upstream response bodies.
//!
//! The collector never owns the response: [`Collector::observe`] looks at the
//! bytes that are already being forwarded and only keeps a small amount of
//! parser state. Chat text, tool arguments and images are parsed for framing but
//! immediately dropped; nothing but counters, model names and usage numbers can
//! leave this module.
//!
//! 上游调用方是 `stats::event::RequestGuard::observe`，下游只产生
//! `UsageOutcome`、首内容标记和响应模型名，再由 writer 写入 SQLite。collector 位于
//! `proxy-io` 请求任务内，不拥有响应流、不创建线程，也不修改正在转发的字节。压缩响应
//! 会被降级为 `Protocol::Unknown`，因为这里不猜测解码后的协议边界。
use crate::stats::dto::{
    InputBreakdown, MAX_TOKEN_VALUE, MAX_TOTAL_TOKENS, UsageReason, UsageStatus,
};
use serde_json::Value;

/// Largest single SSE event (or waiting line) that is still parsed.
pub const MAX_EVENT_BYTES: usize = 64 * 1024;
/// Largest total resident parser state per request.
pub const MAX_PENDING_BYTES: usize = 128 * 1024;
/// Largest non-streaming JSON body kept for a usage side copy.
pub const MAX_JSON_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// 可识别的上游 body framing。
///
/// `Sse` 需要看到终止事件才算协议完整；`Json` 只保留有界副本，在 EOF 时解析一次；
/// `Unknown` 不保存 body，也不猜测 usage。枚举值不是“成功/失败”终态，终态由
/// `RequestGuard` 结合 HTTP 状态、transport EOF 和取消路径决定。
pub enum Protocol {
    /// `text/event-stream`
    Sse,
    /// `application/json`
    Json,
    /// Anything else: framing cannot be trusted, usage stays missing.
    Unknown,
}

impl Protocol {
    pub fn classify(content_type: Option<&str>) -> Self {
        let Some(value) = content_type else {
            return Protocol::Unknown;
        };
        let value = value
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if value == "text/event-stream" {
            Protocol::Sse
        } else if value == "application/json" || value.ends_with("+json") {
            Protocol::Json
        } else {
            Protocol::Unknown
        }
    }

    pub fn is_encoded(encoding: Option<&str>) -> bool {
        encoding.is_some_and(|value| {
            let value = value.trim().to_ascii_lowercase();
            !value.is_empty() && value != "identity"
        })
    }
}

/// 一个响应中已经通过类型和范围校验的 token 用量。
///
/// 所有字段的单位都是 token。`None` 表示当前没有得到可信值，不能当作 0；`Some(0)`
/// 表示上游或推导逻辑确认该字段为零。这里保存的是上游发来的用量快照或由已知父字段推导
/// 出的值，不是 collector 根据响应正文长度估算的 token 数。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageNumbers {
    /// 输入（prompt）token 总数，包含缓存读取、缓存写入等输入明细。
    pub input: Option<i64>,
    /// 输出（completion）token 总数，包含 reasoning 等输出明细。
    pub output: Option<i64>,
    /// 上游给出的总 token 数；缺失且 input/output 都可信时可由两者相加得到。
    pub total: Option<i64>,
    /// 输入中命中缓存读取的 token 数，是 `input` 的子集。
    pub cache_read: Option<i64>,
    /// 输入中写入缓存的 token 数，也是 `input` 的子集。
    pub cache_write: Option<i64>,
    /// 输出中用于推理的 token 数，是 `output` 的子集，不重复计入 `total`。
    pub reasoning: Option<i64>,
}

impl UsageNumbers {
    /// 判断两个事件的全部六个 `Option` 字段是否逐一相同；字段不要求都已知。
    fn same_as(&self, other: &Self) -> bool {
        self.input == other.input
            && self.output == other.output
            && self.total == other.total
            && self.cache_read == other.cache_read
            && self.cache_write == other.cache_write
            && self.reasoning == other.reasoning
    }

    /// True when both sides know a parent field and disagree about it.
    fn conflicts_parent(&self, other: &Self) -> bool {
        differs(self.input, other.input)
            || differs(self.output, other.output)
            || differs(self.total, other.total)
    }

    /// True when both sides know an optional sub-field and disagree about it.
    fn conflicts_subfield(&self, other: &Self) -> bool {
        differs(self.cache_read, other.cache_read)
            || differs(self.cache_write, other.cache_write)
            || differs(self.reasoning, other.reasoning)
    }

    /// True when the three parent values contradict each other.
    fn total_mismatch(&self) -> bool {
        matches!(
            (self.input, self.output, self.total),
            (Some(input), Some(output), Some(total)) if total != input + output
        )
    }

    /// Copies fields the current candidate does not know yet. Known values are
    /// never overwritten, so a conflict surfaces instead of being silently
    /// replaced.
    fn fill_missing_from(&mut self, other: &Self) {
        macro_rules! fill {
            ($($field:ident),+) => { $( if self.$field.is_none() { self.$field = other.$field; } )+ };
        }
        fill!(input, output, total, cache_read, cache_write, reasoning);
    }
}

fn differs<T: PartialEq>(left: Option<T>, right: Option<T>) -> bool {
    matches!((left, right), (Some(a), Some(b)) if a != b)
}

/// Final usage answer handed to the writer thread.
///
/// `status` 表达数值完整度，`reason` 解释为何缺失或无效；`numbers` 中的每个 `None`
/// 都表示该字段未知，不能用 0 填充推测值。`breakdown`/`uncached` 只在解析规则能
/// 证明字段关系时给出，否则保持未知。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageOutcome {
    pub status: UsageStatus,
    pub reason: Option<UsageReason>,
    pub numbers: UsageNumbers,
    pub breakdown: InputBreakdown,
    pub uncached: Option<i64>,
}

impl UsageOutcome {
    pub fn missing(reason: UsageReason) -> Self {
        Self {
            status: UsageStatus::Missing,
            reason: Some(reason),
            numbers: UsageNumbers::default(),
            breakdown: InputBreakdown::Unknown,
            uncached: None,
        }
    }

    pub fn not_applicable() -> Self {
        Self {
            status: UsageStatus::NotApplicable,
            reason: Some(UsageReason::NotSent),
            ..Self::missing(UsageReason::NotSent)
        }
    }
}

/// 增量合并上游在不同事件中发送的 usage 快照。
///
/// 这些事件表示同一请求的累计快照或互补片段，不是需要逐项相加的增量。合并时只补
/// 当前未知的字段：重复快照忽略；父字段 `prompt_tokens`、`completion_tokens`、
/// `total_tokens` 一旦出现两个不同的可信值，就丢弃整次用量并永久标记无效。缓存和
/// reasoning 明细属于子字段：新事件中的子字段无法解析时只忽略该值；两个事件都已知
/// 同一子字段却不同，则保留先到值并标记 `subfield_invalid`，不会因此单独推翻可信的
/// input/output。
///
/// Java 读者可以把它理解为一个仅在当前响应内使用的可变累积器对象；Rust 中它由
/// 每个 `Collector` 按值持有，不需要共享实例，也没有跨请求状态。
#[derive(Debug, Default)]
pub struct UsageAccumulator {
    /// 已知字段的合并快照。父字段冲突后会被清空，因此 `is_some()` 不能表示
    /// “曾经见过 usage”，只能表示当前仍有一组可继续合并的候选值。
    candidate: Option<UsageNumbers>,
    /// 粘滞标志：父字段三元组自相矛盾，或不同事件给出了冲突的父字段。
    /// 置位后后续事件不能恢复该请求的 usage；`finalize` 会优先返回 Invalid。
    parent_invalid: bool,
    /// 粘滞标志：至少一个缓存/reasoning 子字段无法解析或与先到值冲突。
    /// 父字段仍可保持 Complete/Partial，最终通过 reason 报告明细异常。
    subfield_invalid: bool,
}

/// 按 `keys` 指定的对象路径读取一个非负整数 token 字段。
///
/// 返回 `Ok(None)` 只表示字段不存在或显式为 `null`；返回 `Err(())` 表示字段存在但
/// 不可信，包括负数、小数、字符串、布尔值、指数形式的 JSON number，或超过 `max`
/// 的值。函数借用瞬态 `serde_json::Value`，只复制验证后的 `i64`，不会把正文 JSON
/// 保存到 accumulator。
fn read_int(value: &Value, keys: &[&str], max: i64) -> Result<Option<i64>, ()> {
    let mut current = value;
    for key in keys {
        match current.get(*key) {
            Some(next) => current = next,
            None => return Ok(None),
        }
    }
    match current {
        Value::Null => Ok(None),
        Value::Number(number) => match number.as_i64() {
            Some(number) => {
                if number < 0 || number > max {
                    Err(())
                } else {
                    Ok(Some(number))
                }
            }
            None => Err(()),
        },
        _ => Err(()),
    }
}

impl UsageAccumulator {
    /// 吸收一个从响应事件中识别到的 `usage` JSON 对象。
    ///
    /// 调用来源是 `inspect`：SSE 每解析出一个完整 data 事件，或非流式 JSON body 在
    /// EOF 解析成功时，都可能调用这里。输入只是 `&Value` 借用，本函数不拥有响应或
    /// JSON，也不产生返回值；它按以下顺序修改当前 accumulator：
    ///
    /// 1. 校验 input/output/total 三个父字段，并拒绝单事件内部 `total != input + output`
    ///    的矛盾；total 缺失时可由两个已知父字段推导。
    /// 2. 校验缓存写入、缓存读取和 reasoning 子字段；超过父字段的值会被丢弃，当前
    ///    实现不会仅因这种“上界过滤”设置 `subfield_invalid`。
    /// 3. 父字段非法时清空候选并留下粘滞 Invalid；否则忽略完全相同快照、拒绝冲突的
    ///    父字段，并用新事件补齐仍然未知的字段。
    ///
    /// 这是一次同步内存更新，不跨 `await`，也没有 I/O。失败通过最终状态表达，不返回
    /// `Result`；一旦父字段冲突，本请求没有“恢复为之前快照”的路径。
    pub fn observe(&mut self, usage: &Value) {
        let mut invalid_parent = false;
        let mut invalid_subfield = false;
        let input = match read_int(usage, &["prompt_tokens"], MAX_TOKEN_VALUE) {
            Ok(value) => value,
            Err(()) => {
                invalid_parent = true;
                None
            }
        };
        let output = match read_int(usage, &["completion_tokens"], MAX_TOKEN_VALUE) {
            Ok(value) => value,
            Err(()) => {
                invalid_parent = true;
                None
            }
        };
        let mut total = match read_int(usage, &["total_tokens"], MAX_TOTAL_TOKENS) {
            Ok(value) => value,
            Err(()) => {
                invalid_parent = true;
                None
            }
        };
        if matches!((input, output, total), (Some(i), Some(o), Some(t)) if t != i + o) {
            // The event contradicts itself, so no value from it can be trusted.
            invalid_parent = true;
        }
        if total.is_none()
            && let (Some(input), Some(output)) = (input, output)
        {
            total = Some(input + output);
        }
        // Optional sub-fields never invalidate a trustworthy parent pair.
        let cache_read = match read_int(
            usage,
            &["prompt_tokens_details", "cached_tokens"],
            MAX_TOKEN_VALUE,
        ) {
            Ok(value) => value.filter(|v| input.is_none_or(|parent| *v <= parent)),
            Err(()) => {
                invalid_subfield = true;
                None
            }
        };
        let cache_write = match read_int(
            usage,
            &["prompt_tokens_details", "cache_write_tokens"],
            MAX_TOKEN_VALUE,
        ) {
            Ok(value) => value.filter(|v| input.is_none_or(|parent| *v <= parent)),
            Err(()) => {
                invalid_subfield = true;
                None
            }
        };
        let reasoning = match read_int(
            usage,
            &["completion_tokens_details", "reasoning_tokens"],
            MAX_TOKEN_VALUE,
        ) {
            Ok(value) => value.filter(|v| output.is_none_or(|parent| *v <= parent)),
            Err(()) => {
                invalid_subfield = true;
                None
            }
        };
        if invalid_subfield {
            self.subfield_invalid = true;
        }
        if invalid_parent {
            // Keep the "invalid" verdict without any numbers; the writer must
            // never receive values that violate its own constraints.
            self.parent_invalid = true;
            self.candidate = None;
            return;
        }
        let incoming = UsageNumbers {
            input,
            output,
            total,
            cache_read,
            cache_write,
            reasoning,
        };
        if incoming.input.is_none() && incoming.output.is_none() && incoming.total.is_none() {
            // An empty `usage` object (or only rejected details) carries nothing.
            return;
        }
        match &mut self.candidate {
            None => self.candidate = Some(incoming),
            Some(existing) => {
                if existing.same_as(&incoming) {
                    // Byte-identical repeats (some providers resend the final
                    // usage) are ignored, never summed.
                    return;
                }
                if existing.conflicts_parent(&incoming) {
                    self.parent_invalid = true;
                    self.candidate = None;
                    return;
                }
                if existing.conflicts_subfield(&incoming) {
                    // First value wins; the parent pair stays trustworthy.
                    self.subfield_invalid = true;
                }
                existing.fill_missing_from(&incoming);
                if existing.total_mismatch() {
                    self.parent_invalid = true;
                    self.candidate = None;
                }
            }
        }
    }

    /// 当前 `candidate` 是否仍是 `Some`；它不判断这组 usage 最终是否有效。
    ///
    /// 父字段冲突发生时会清空候选，因此该次调用后可能返回 `false`。但 `observe` 没有
    /// 在入口检查粘滞的 `parent_invalid`，后续正常事件仍可能重新填入 `candidate`，使
    /// 此函数再次返回 `true`；最终仍由 `finalize` 优先依据 `parent_invalid` 返回
    /// Invalid。`Collector::finish` 只用它判断当前是否留有候选，不能把它解释为
    /// “从未见过 usage”“usage 有效”或“父字段冲突后永远为 false”。
    pub fn saw_usage(&self) -> bool {
        self.candidate.is_some()
    }

    /// 在响应 body 结束时把累积状态转换成可入统计队列的最终判定。
    ///
    /// 优先级固定为：父字段冲突 -> Invalid；没有候选 -> Missing；input/output 都
    /// 已知 -> Complete；否则 Partial。父字段 Invalid 时所有数值均为 `None`，避免
    /// 把相互矛盾的值写入 SQLite。`fallback_reason` 只用于“从未形成候选”的 Missing，
    /// 不会覆盖 Partial 的 `NoUsage`/`InvalidNumbers`，也不会把 `None` 改成零。
    ///
    /// 本函数只克隆少量整数字段用于构造输出，不做 I/O，不改 accumulator。当前
    /// OpenAI Chat 风格解析能证明 cached tokens 是 input 的子集，但证明不了缓存读/写
    /// 与未缓存输入互斥，因此这里最多标成 `ReadSubset`，`uncached` 始终为 `None`。
    pub fn finalize(&self, fallback_reason: UsageReason) -> UsageOutcome {
        if self.parent_invalid {
            return UsageOutcome {
                status: UsageStatus::Invalid,
                reason: Some(UsageReason::ConflictingUsage),
                numbers: UsageNumbers::default(),
                breakdown: InputBreakdown::Unknown,
                uncached: None,
            };
        }
        let Some(mut numbers) = self.candidate.clone() else {
            return UsageOutcome::missing(fallback_reason);
        };
        if numbers.total.is_none()
            && let (Some(input), Some(output)) = (numbers.input, numbers.output)
        {
            // A missing total may be derived only from two valid parents.
            numbers.total = Some(input + output);
        }
        let (status, reason) = match (numbers.input, numbers.output) {
            (Some(_), Some(_)) => (
                UsageStatus::Complete,
                self.subfield_invalid.then_some(UsageReason::InvalidNumbers),
            ),
            _ => (
                UsageStatus::Partial,
                Some(if self.subfield_invalid {
                    UsageReason::InvalidNumbers
                } else {
                    UsageReason::NoUsage
                }),
            ),
        };
        // `openai_chat_v1` can prove that cache reads are a subset of the input,
        // never that reads and writes are disjoint, so `uncached` stays unknown.
        let breakdown = if numbers.cache_read.is_some() {
            InputBreakdown::ReadSubset
        } else {
            InputBreakdown::Unknown
        };
        UsageOutcome {
            status,
            reason,
            breakdown,
            uncached: None,
            numbers,
        }
    }
}

/// Fields extracted from one SSE event or a complete JSON body.
#[derive(Debug, Default)]
struct Observation {
    first_content: bool,
    error: bool,
    saw_choice: bool,
    model: Option<String>,
}

fn sanitize_model(value: &str) -> Option<String> {
    let cleaned: String = value
        .chars()
        .filter(|c| !c.is_control())
        .take(160)
        .collect();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// Returns whether a `choices` entry carries real generated content.
fn choice_has_content(choice: &Value) -> bool {
    let delta = choice.get("delta").or_else(|| choice.get("message"));
    let Some(delta) = delta else {
        return choice
            .get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty());
    };
    if !delta.is_object() {
        return false;
    }
    for key in [
        "content",
        "reasoning_content",
        "reasoning",
        "refusal",
        "tool_calls",
    ] {
        match delta.get(key) {
            Some(Value::String(text)) => {
                if !text.is_empty() {
                    return true;
                }
            }
            Some(Value::Array(items)) => {
                if !items.is_empty() {
                    return true;
                }
            }
            Some(Value::Object(map)) if !map.is_empty() => return true,
            _ => {}
        }
    }
    false
}

/// Inspects one parsed JSON object; only whitelisted fields are read.
fn inspect(value: &Value, usage: &mut UsageAccumulator, observation: &mut Observation) {
    if let Some(model) = value.get("model").and_then(Value::as_str) {
        observation.model = sanitize_model(model);
    }
    if value.get("error").is_some_and(|e| !e.is_null()) {
        observation.error = true;
    }
    if let Some(choices) = value.get("choices").and_then(Value::as_array)
        && !choices.is_empty()
    {
        observation.saw_choice = true;
        if choices.iter().any(choice_has_content) {
            observation.first_content = true;
        }
    }
    if let Some(usage_value) = value.get("usage")
        && usage_value.is_object()
    {
        usage.observe(usage_value);
    }
}

#[derive(Debug, Default)]
/// 有界的 SSE framing parser。
///
/// `line`/`data` 是跨网络 chunk 保留的少量字节；`event_bytes` 表示当前事件已见大小。
/// 发生超限后 `start_discarding` 会跳过错位数据直到下一个空行边界，避免把半截 JSON
/// 当成后续事件，同时保持 `overflowed` 供 `finish` 报告解析限制。
struct SseParser {
    line: Vec<u8>,
    data: Vec<u8>,
    event_bytes: usize,
    discarding: bool,
    /// Length of the line currently being skipped while `discarding`.
    discard_line_len: usize,
    pending_cr: bool,
    overflowed: bool,
}

impl SseParser {
    fn start_discarding(&mut self) {
        self.discarding = true;
        self.overflowed = true;
        self.discard_line_len = 0;
        self.line.clear();
        self.data.clear();
        self.event_bytes = 0;
    }

    fn push_data(&mut self, line: &[u8]) {
        let Some(payload) = line.strip_prefix(b"data:") else {
            return;
        };
        let payload = payload.strip_prefix(b" ").unwrap_or(payload);
        if !self.data.is_empty() {
            self.data.push(b'\n');
        }
        self.data.extend_from_slice(payload);
    }

    fn end_line(&mut self, sink: &mut impl FnMut(&[u8])) {
        if self.line.is_empty() {
            // A blank line terminates the current event.
            if !self.data.is_empty() {
                let data = std::mem::take(&mut self.data);
                sink(&data);
            }
            self.event_bytes = 0;
            return;
        }
        let line = std::mem::take(&mut self.line);
        if !line.starts_with(b":") {
            self.push_data(&line);
        }
        self.line = line;
        self.line.clear();
    }

    /// Feeds one network chunk; completed events are handed to `sink`.
    ///
    /// 支持跨 chunk 的 LF、CRLF 和 CR framing；`sink` 只会收到由同一事件的多条
    /// `data:` 行拼接成的 payload。若当前事件过大，parser 进入丢弃模式，在本函数内
    /// 不再分配与事件成比例的缓冲。
    fn feed_chunk(&mut self, bytes: &[u8], sink: &mut impl FnMut(&[u8])) {
        for &byte in bytes {
            if self.pending_cr {
                self.pending_cr = false;
                if byte == b'\n' {
                    continue;
                }
            }
            if self.discarding {
                match byte {
                    b'\n' => {
                        if self.discard_line_len == 0 {
                            self.discarding = false;
                        }
                        self.discard_line_len = 0;
                    }
                    b'\r' => {
                        if self.discard_line_len == 0 {
                            self.discarding = false;
                        }
                        self.discard_line_len = 0;
                        self.pending_cr = true;
                    }
                    _ => self.discard_line_len += 1,
                }
                continue;
            }
            match byte {
                b'\n' => self.end_line(sink),
                b'\r' => {
                    self.end_line(sink);
                    self.pending_cr = true;
                }
                _ => {
                    self.line.push(byte);
                    self.event_bytes += 1;
                    if self.line.len() > MAX_EVENT_BYTES || self.event_bytes > MAX_EVENT_BYTES {
                        self.start_discarding();
                    }
                }
            }
        }
    }
}

/// Verdict of a non-streaming JSON body.
///
/// The bounded copy is parsed once, at EOF, before any terminal classification
/// happens: an `error` object inside an HTTP 200 response and malformed JSON are
/// both failures even though the status line looked successful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JsonVerdict {
    /// Not parsed yet (streaming response, or the body has not ended).
    #[default]
    Pending,
    /// Valid JSON without an `error` member.
    Ok,
    /// Valid JSON carrying an `error` object.
    ErrorObject,
    /// The body was not valid JSON at all.
    Malformed,
}

/// True when a decoded body is an error envelope rather than a result.
fn is_error_envelope(value: &Value) -> bool {
    value.get("error").is_some_and(|error| !error.is_null())
}

/// Per-request response observer.
///
/// 每个已准入请求持有一个实例。`observe` 消费的是代理本来就要转发给客户端的字节；
/// collector 不取得 stream 所有权，也不延迟 yield。成功、HTTP/JSON 错误、transport
/// 中断和客户端取消可能发生在不同层，最终如何归类由 `RequestGuard` 决定。
pub struct Collector {
    protocol: Protocol,
    parser: SseParser,
    usage: UsageAccumulator,
    json: Option<Vec<u8>>,
    json_overflow: bool,
    /// Set once the non-streaming copy has been parsed.
    json_settled: bool,
    json_verdict: JsonVerdict,
    pending_bytes: usize,
    done: bool,
    protocol_error: bool,
    parse_error: bool,
    bytes_seen: u64,
    pub first_content: bool,
    pub response_model: Option<String>,
}

impl std::fmt::Debug for Collector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Collector")
            .field("protocol", &self.protocol)
            .field("bytes_seen", &self.bytes_seen)
            .field("done", &self.done)
            .finish()
    }
}

impl Collector {
    /// Creates a collector for the response framing known at this point.
    ///
    /// `encoded=true` 会把协议降为 `Unknown`，因为压缩 body 无法在这里按明文的 SSE/
    /// JSON 边界解析；聊天响应仍按原字节转发，只是 usage 留给缺失原因。
    pub fn new(protocol: Protocol, encoded: bool) -> Self {
        let protocol = if encoded { Protocol::Unknown } else { protocol };
        Self {
            protocol,
            parser: SseParser::default(),
            usage: UsageAccumulator::default(),
            json: (protocol == Protocol::Json).then(Vec::new),
            json_overflow: false,
            json_settled: false,
            json_verdict: JsonVerdict::default(),
            pending_bytes: 0,
            done: false,
            protocol_error: false,
            parse_error: false,
            bytes_seen: 0,
            first_content: false,
            response_model: None,
        }
    }

    /// 解析有界的非流式 JSON 副本一次，并记住 body 判定。
    ///
    /// 可由终态处理、`finish` 或 `Drop` 路径重复调用；`json_settled` 保证真正的解析
    /// 和 `usage.observe` 最多发生一次。空副本不额外标成 malformed，非空但 JSON
    /// 解析失败会设置 `parse_error`；`error` 成员存在且值不是 `null` 时设置协议错误。
    pub fn settle_json(&mut self) {
        if self.json_settled {
            return;
        }
        self.json_settled = true;
        let Some(buffer) = self.json.take() else {
            return;
        };
        match serde_json::from_slice::<Value>(&buffer) {
            Ok(value) => {
                if is_error_envelope(&value) {
                    self.json_verdict = JsonVerdict::ErrorObject;
                    self.parse_error = true;
                } else {
                    let mut observation = Observation::default();
                    inspect(&value, &mut self.usage, &mut observation);
                    self.apply(observation);
                    self.json_verdict = JsonVerdict::Ok;
                }
            }
            Err(_) if !buffer.is_empty() => {
                self.parse_error = true;
                self.json_verdict = JsonVerdict::Malformed;
            }
            Err(_) => {}
        }
    }

    /// Classification of the non-streaming body (see [`JsonVerdict`]).
    pub fn json_verdict(&self) -> JsonVerdict {
        self.json_verdict
    }

    /// 响应是否已满足其 framing 的完整结束条件。
    ///
    /// SSE 必须看到 `[DONE]` 且没有协议错误。JSON 和 Unknown 的结束由 transport
    /// 负责，因此这里对两者直接返回 `true`，本函数本身不证明已经读到 EOF。此标志只
    /// 描述 collector 能看到的 framing 条件，不等于请求业务成功，也不保证 usage 完整。
    pub fn stream_complete(&self) -> bool {
        match self.protocol {
            Protocol::Sse => self.done && !self.protocol_error,
            Protocol::Json | Protocol::Unknown => true,
        }
    }

    /// True when the stream carried an explicit protocol-level error.
    pub fn saw_protocol_error(&self) -> bool {
        self.protocol_error
    }

    /// Observes forwarded bytes without keeping a copy of the chat content.
    ///
    /// SSE 只保留 parser 状态；JSON 最多保留 `MAX_JSON_BYTES` 的副本；Unknown 不保存
    /// body。超限、malformed frame 等只影响统计结果，不在此处生成 HTTP 响应，也不会
    /// 改变调用方接着 yield 的 chunk。
    pub fn observe(&mut self, chunk: &[u8]) {
        self.bytes_seen += chunk.len() as u64;
        match self.protocol {
            Protocol::Sse => {
                // Only the parser's own resident state counts against the
                // budget. The incoming slice is scanned incrementally and is
                // never buffered, so charging `chunk.len()` here would discard
                // perfectly legal large bodies (for example a big heartbeat
                // block that arrives in one read).
                if self.parser.line.len() + self.parser.data.len() > MAX_PENDING_BYTES {
                    // Never grow parser state without bound: skip ahead to the
                    // next event boundary and keep watching for later usage.
                    self.parser.start_discarding();
                    self.parse_error = true;
                }
                let mut parser = std::mem::take(&mut self.parser);
                parser.feed_chunk(chunk, &mut |data| self.handle_event(data));
                self.parser = parser;
                self.pending_bytes = self.parser.line.len() + self.parser.data.len();
            }
            Protocol::Json => {
                if let Some(buffer) = self.json.as_mut() {
                    if buffer.len() + chunk.len() > MAX_JSON_BYTES {
                        self.json = None;
                        self.json_overflow = true;
                    } else {
                        buffer.extend_from_slice(chunk);
                    }
                }
            }
            Protocol::Unknown => {}
        }
    }

    fn handle_event(&mut self, data: &[u8]) {
        let trimmed = trim_ascii(data);
        if trimmed == b"[DONE]" {
            self.done = true;
            return;
        }
        if trimmed.is_empty() {
            return;
        }
        let Ok(value) = serde_json::from_slice::<Value>(trimmed) else {
            self.parse_error = true;
            return;
        };
        let mut observation = Observation::default();
        inspect(&value, &mut self.usage, &mut observation);
        self.apply(observation);
    }

    fn apply(&mut self, observation: Observation) {
        if observation.first_content {
            self.first_content = true;
        }
        if observation.error {
            self.protocol_error = true;
        }
        if let Some(model) = observation.model {
            self.response_model = Some(model);
        }
    }

    /// Called when the response body ends (EOF, error or cancellation).
    ///
    /// 该函数会结算尾部 SSE、解析非流式 JSON 并按 `fallback` 生成缺失原因。它只返回
    /// usage 判定；调用方仍负责决定请求的 `Outcome`。若 body 只是被客户端取消，
    /// collector 看到的最后状态不会自动改写成“成功”，也不会把缺失数值填成 0。
    pub fn finish(&mut self, fallback: UsageReason) -> UsageOutcome {
        match self.protocol {
            Protocol::Json => self.settle_json(),
            Protocol::Sse => {
                // A final line without a newline still carries data, and a
                // trailing event without its blank line may still be parsed;
                // either way the response is not protocol-complete.
                let trailing_line = !self.parser.line.is_empty();
                if trailing_line {
                    let line = std::mem::take(&mut self.parser.line);
                    if !line.starts_with(b":") {
                        self.parser.push_data(&line);
                    }
                    self.protocol_error = true;
                }
                if !self.parser.data.is_empty() {
                    let data = std::mem::take(&mut self.parser.data);
                    self.handle_event(&data);
                    self.protocol_error = true;
                }
            }
            Protocol::Unknown => {}
        }
        if self.json_overflow {
            return UsageOutcome::missing(UsageReason::BodyTooLarge);
        }
        if self.parser.overflowed && !self.usage.saw_usage() {
            return UsageOutcome::missing(UsageReason::EventTooLarge);
        }
        if let Protocol::Unknown = self.protocol {
            return UsageOutcome::missing(if self.protocol_error {
                UsageReason::StreamError
            } else {
                UsageReason::UnsupportedEncoding
            });
        }
        let mut outcome = self.usage.finalize(fallback);
        if outcome.status == UsageStatus::Missing {
            if self.parse_error {
                outcome.reason = Some(UsageReason::ParseError);
            } else if self.protocol_error {
                outcome.reason = Some(UsageReason::StreamError);
            } else if self.protocol == Protocol::Sse && !self.done {
                outcome.reason = Some(UsageReason::NoUsage);
            }
        }
        outcome
    }
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |i| i + 1);
    &bytes[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sse_collector() -> Collector {
        Collector::new(Protocol::Sse, false)
    }

    fn feed_all(collector: &mut Collector, data: &[u8], chunk: usize) {
        for part in data.chunks(chunk.max(1)) {
            collector.observe(part);
        }
    }

    #[test]
    fn protocol_classification_ignores_charset_and_rejects_unknown() {
        assert_eq!(
            Protocol::classify(Some("text/event-stream; charset=utf-8")),
            Protocol::Sse
        );
        assert_eq!(
            Protocol::classify(Some("application/json;charset=UTF-8")),
            Protocol::Json
        );
        assert_eq!(Protocol::classify(Some("text/plain")), Protocol::Unknown);
        assert_eq!(Protocol::classify(None), Protocol::Unknown);
        assert!(Protocol::is_encoded(Some("gzip")));
        assert!(!Protocol::is_encoded(Some("identity")));
        assert!(!Protocol::is_encoded(None));
    }

    #[test]
    fn sse_usage_is_parsed_across_every_byte_boundary() {
        let body = concat!(
            ": keep-alive\n\n",
            "event: message\n",
            "data: {\"model\":\"model-x\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1000,",
            "\"completion_tokens\":100,\"total_tokens\":1100,",
            "\"prompt_tokens_details\":{\"cached_tokens\":800}}}\n\n",
            "data: [DONE]\n\n"
        );
        for chunk in 1..=17 {
            let mut collector = sse_collector();
            feed_all(&mut collector, body.as_bytes(), chunk);
            assert!(collector.stream_complete(), "chunk={chunk}");
            assert!(collector.first_content, "chunk={chunk}");
            let outcome = collector.finish(UsageReason::NoUsage);
            assert_eq!(outcome.status, UsageStatus::Complete, "chunk={chunk}");
            assert_eq!(outcome.numbers.input, Some(1000));
            assert_eq!(outcome.numbers.output, Some(100));
            assert_eq!(outcome.numbers.total, Some(1100));
            assert_eq!(outcome.numbers.cache_read, Some(800));
            assert_eq!(outcome.breakdown, InputBreakdown::ReadSubset);
            assert_eq!(collector.response_model.as_deref(), Some("model-x"));
        }
    }

    #[test]
    fn multiple_data_lines_join_with_newline_and_crlf_is_supported() {
        let body = "data: {\"usage\":\r\ndata: {\"prompt_tokens\":10,\"completion_tokens\":2}}\r\n\r\ndata: [DONE]\r\n\r\n";
        let mut collector = sse_collector();
        feed_all(&mut collector, body.as_bytes(), 3);
        assert!(collector.stream_complete());
        let outcome = collector.finish(UsageReason::NoUsage);
        assert_eq!(outcome.status, UsageStatus::Complete);
        assert_eq!(outcome.numbers.total, Some(12));
    }

    #[test]
    fn repeated_identical_usage_is_counted_once_and_conflicts_are_invalid() {
        let repeated = concat!(
            "data: {\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1,\"total_tokens\":6}}\n\n",
            "data: {\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1,\"total_tokens\":6}}\n\n",
            "data: [DONE]\n\n"
        );
        let mut collector = sse_collector();
        feed_all(&mut collector, repeated.as_bytes(), 7);
        let outcome = collector.finish(UsageReason::NoUsage);
        assert_eq!(outcome.status, UsageStatus::Complete);
        assert_eq!(outcome.numbers.total, Some(6));

        let conflicting = concat!(
            "data: {\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1,\"total_tokens\":6}}\n\n",
            "data: {\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":1,\"total_tokens\":10}}\n\n",
            "data: [DONE]\n\n"
        );
        let mut collector = sse_collector();
        feed_all(&mut collector, conflicting.as_bytes(), 1000);
        let outcome = collector.finish(UsageReason::NoUsage);
        assert_eq!(outcome.status, UsageStatus::Invalid);
        assert_eq!(outcome.reason, Some(UsageReason::ConflictingUsage));
    }

    #[test]
    fn partial_candidates_merge_but_never_sum() {
        let body = concat!(
            "data: {\"usage\":{\"prompt_tokens\":100}}\n\n",
            "data: {\"usage\":{\"completion_tokens\":20}}\n\n",
            "data: [DONE]\n\n"
        );
        let mut collector = sse_collector();
        feed_all(&mut collector, body.as_bytes(), 5);
        let outcome = collector.finish(UsageReason::NoUsage);
        assert_eq!(outcome.status, UsageStatus::Complete);
        assert_eq!(outcome.numbers.input, Some(100));
        assert_eq!(outcome.numbers.output, Some(20));
        assert_eq!(outcome.numbers.total, Some(120));
    }

    #[test]
    fn conflicting_partial_parents_never_merge_into_an_invalid_complete() {
        // Regression for F01: `{prompt_tokens:100}` followed by
        // `{prompt_tokens:200, completion_tokens:10, total_tokens:210}` used to
        // merge into Complete{100, 10, 210}, which violates the SQLite total
        // constraint and rolled back the whole write batch.
        let mut collector = sse_collector();
        let body = concat!(
            "data: {\"usage\":{\"prompt_tokens\":100}}\n\n",
            "data: {\"usage\":{\"prompt_tokens\":200,\"completion_tokens\":10,\"total_tokens\":210}}\n\n",
            "data: [DONE]\n\n"
        );
        feed_all(&mut collector, body.as_bytes(), 3);
        let outcome = collector.finish(UsageReason::NoUsage);
        assert_eq!(outcome.status, UsageStatus::Invalid, "{outcome:?}");
        assert!(
            outcome.numbers.input.is_none()
                && outcome.numbers.output.is_none()
                && outcome.numbers.total.is_none(),
            "an invalid usage must not carry numbers: {outcome:?}"
        );
    }

    #[test]
    fn a_later_cache_detail_extends_a_complete_parent_instead_of_conflicting() {
        // Regression for F01: the same parent with a previously unknown cache
        // detail is compatible and must stay Complete.
        let mut collector = sse_collector();
        let body = concat!(
            "data: {\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":10,\"total_tokens\":110}}\n\n",
            "data: {\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":10,\"total_tokens\":110,",
            "\"prompt_tokens_details\":{\"cached_tokens\":80}}}\n\n",
            "data: [DONE]\n\n"
        );
        feed_all(&mut collector, body.as_bytes(), 4);
        let outcome = collector.finish(UsageReason::NoUsage);
        assert_eq!(outcome.status, UsageStatus::Complete, "{outcome:?}");
        assert_eq!(outcome.numbers.input, Some(100));
        assert_eq!(outcome.numbers.total, Some(110));
        assert_eq!(outcome.numbers.cache_read, Some(80));
        assert_eq!(outcome.breakdown, InputBreakdown::ReadSubset);
    }

    #[test]
    fn complementary_and_repeated_usage_events_still_behave() {
        // Complementary halves complete; byte-identical repeats count once.
        let mut collector = sse_collector();
        let body = concat!(
            "data: {\"usage\":{\"prompt_tokens\":100}}\n\n",
            "data: {\"usage\":{\"completion_tokens\":10}}\n\n",
            "data: {\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":10,\"total_tokens\":110}}\n\n",
            "data: {\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":10,\"total_tokens\":110}}\n\n",
            "data: [DONE]\n\n"
        );
        feed_all(&mut collector, body.as_bytes(), 5);
        let outcome = collector.finish(UsageReason::NoUsage);
        assert_eq!(outcome.status, UsageStatus::Complete, "{outcome:?}");
        assert_eq!(outcome.numbers.total, Some(110));
    }

    #[test]
    fn conflicting_subfield_drops_only_the_subfield() {
        // A conflicting cache detail must not discard a trustworthy parent.
        let mut collector = sse_collector();
        let body = concat!(
            "data: {\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":10,\"total_tokens\":110,",
            "\"prompt_tokens_details\":{\"cached_tokens\":80}}}\n\n",
            "data: {\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":10,\"total_tokens\":110,",
            "\"prompt_tokens_details\":{\"cached_tokens\":90}}}\n\n",
            "data: [DONE]\n\n"
        );
        feed_all(&mut collector, body.as_bytes(), 6);
        let outcome = collector.finish(UsageReason::NoUsage);
        assert_eq!(outcome.status, UsageStatus::Complete, "{outcome:?}");
        assert_eq!(outcome.numbers.total, Some(110));
        assert_eq!(outcome.numbers.cache_read, Some(80));
        assert_eq!(outcome.reason, Some(UsageReason::InvalidNumbers));
    }

    #[test]
    fn invalid_numbers_are_rejected_without_zeroing_the_parent() {
        for bad in ["\"10\"", "10.5", "-3", "true"] {
            let body = format!(
                "data: {{\"usage\":{{\"prompt_tokens\":10,\"completion_tokens\":2,\"total_tokens\":12,\"prompt_tokens_details\":{{\"cached_tokens\":{bad}}}}}}}\n\ndata: [DONE]\n\n"
            );
            let mut collector = sse_collector();
            feed_all(&mut collector, body.as_bytes(), 4);
            let outcome = collector.finish(UsageReason::NoUsage);
            assert_eq!(outcome.status, UsageStatus::Complete, "bad={bad}");
            assert_eq!(outcome.numbers.cache_read, None, "bad={bad}");
            assert_eq!(outcome.reason, Some(UsageReason::InvalidNumbers));
        }
    }

    #[test]
    fn inconsistent_total_and_subfield_over_parent_are_invalid() {
        let body = concat!(
            "data: {\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2,\"total_tokens\":99}}\n\n",
            "data: [DONE]\n\n"
        );
        let mut collector = sse_collector();
        feed_all(&mut collector, body.as_bytes(), 2);
        let outcome = collector.finish(UsageReason::NoUsage);
        assert_eq!(outcome.status, UsageStatus::Invalid);

        let over = concat!(
            "data: {\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2,\"total_tokens\":12,\"prompt_tokens_details\":{\"cached_tokens\":50}}}\n\n",
            "data: [DONE]\n\n"
        );
        let mut collector = sse_collector();
        feed_all(&mut collector, over.as_bytes(), 9);
        let outcome = collector.finish(UsageReason::NoUsage);
        assert_eq!(outcome.status, UsageStatus::Complete);
        assert_eq!(outcome.numbers.cache_read, None);
    }

    /// Feeds the same bytes with a fixed chunk size and returns the verdict.
    fn outcome_for(body: &[u8], chunk: usize) -> UsageOutcome {
        let mut collector = Collector::new(Protocol::Sse, false);
        for part in body.chunks(chunk.max(1)) {
            collector.observe(part);
        }
        collector.finish(UsageReason::NoUsage)
    }

    #[test]
    fn any_chunking_of_the_same_bytes_yields_the_same_usage() {
        // F05 regression: a legal 195 KB SSE body (usage first, then many
        // heartbeats) used to be discarded whole because the incoming chunk
        // length was charged against the parser's resident budget. Only real
        // parser state may count.
        let mut body = Vec::new();
        body.extend_from_slice(
            b"data: {\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":100,\"total_tokens\":1100}}\n\n",
        );
        for _ in 0..15000 {
            body.extend_from_slice(b": heartbeat\n\n");
        }
        body.extend_from_slice(b"data: [DONE]\n\n");
        assert!(
            body.len() > MAX_PENDING_BYTES,
            "fixture must exceed the budget"
        );

        let expected = outcome_for(&body, body.len());
        assert_eq!(expected.status, UsageStatus::Complete, "{expected:?}");
        assert_eq!(expected.numbers.total, Some(1100));
        for chunk in [1usize, 7, 4096, 65536, body.len()] {
            assert_eq!(outcome_for(&body, chunk), expected, "chunk size {chunk}");
        }

        // Random chunking must agree as well.
        let mut collector = Collector::new(Protocol::Sse, false);
        let mut seed = 0x5eed_1234_9abc_def0u64;
        let mut offset = 0usize;
        while offset < body.len() {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let size = ((seed >> 33) as usize % 5000) + 1;
            let end = (offset + size).min(body.len());
            collector.observe(&body[offset..end]);
            offset = end;
        }
        assert_eq!(collector.finish(UsageReason::NoUsage), expected);
    }

    #[test]
    fn an_oversized_single_event_is_still_bounded() {
        // The fix must not remove the real limit: one event larger than the
        // per-event budget is dropped, and parsing recovers afterwards.
        let mut body = Vec::new();
        body.extend_from_slice(b"data: {\"x\":\"");
        body.extend_from_slice(&vec![b'a'; MAX_EVENT_BYTES + 4096]);
        body.extend_from_slice(b"\"}\n\n");
        body.extend_from_slice(
            b"data: {\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":3,\"total_tokens\":10}}\n\n",
        );
        body.extend_from_slice(b"data: [DONE]\n\n");
        let outcome = outcome_for(&body, 4096);
        assert_eq!(outcome.status, UsageStatus::Complete, "{outcome:?}");
        assert_eq!(outcome.numbers.total, Some(10));
    }

    #[test]
    fn oversized_event_is_dropped_but_later_usage_still_parses() {
        let mut body = String::from("data: {\"choices\":[{\"delta\":{\"content\":\"");
        body.push_str(&"x".repeat(MAX_EVENT_BYTES + 4096));
        body.push_str("\"}}]}\n\n");
        body.push_str("data: {\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":3,\"total_tokens\":10}}\n\n");
        body.push_str("data: [DONE]\n\n");
        let mut collector = sse_collector();
        feed_all(&mut collector, body.as_bytes(), 8192);
        assert!(collector.stream_complete());
        let outcome = collector.finish(UsageReason::NoUsage);
        assert_eq!(outcome.status, UsageStatus::Complete);
        assert_eq!(outcome.numbers.total, Some(10));
    }

    #[test]
    fn missing_done_marks_protocol_incomplete_but_keeps_known_usage() {
        let body = "data: {\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":1,\"total_tokens\":5}}\n\n";
        let mut collector = sse_collector();
        feed_all(&mut collector, body.as_bytes(), 6);
        assert!(!collector.stream_complete());
        let outcome = collector.finish(UsageReason::NoUsage);
        assert_eq!(outcome.status, UsageStatus::Complete);
        assert_eq!(outcome.numbers.total, Some(5));
    }

    #[test]
    fn error_events_are_flagged_without_keeping_their_body() {
        let body = "data: {\"error\":{\"message\":\"secret upstream detail\"}}\n\n";
        let mut collector = sse_collector();
        feed_all(&mut collector, body.as_bytes(), 12);
        assert!(collector.saw_protocol_error());
        let outcome = collector.finish(UsageReason::NoUsage);
        assert_eq!(outcome.status, UsageStatus::Missing);
        assert_eq!(outcome.reason, Some(UsageReason::StreamError));
    }

    #[test]
    fn role_only_and_empty_deltas_are_not_first_content() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{}}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\n",
            "data: [DONE]\n\n"
        );
        let mut collector = sse_collector();
        feed_all(&mut collector, body.as_bytes(), 8);
        assert!(!collector.first_content);
        let body_with_content = "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"thinking\"}}]}\n\ndata: [DONE]\n\n";
        let mut collector = sse_collector();
        feed_all(&mut collector, body_with_content.as_bytes(), 8);
        assert!(collector.first_content);
    }

    #[test]
    fn json_bodies_are_parsed_and_oversized_copies_are_released() {
        let body = json!({
            "model": "model-y",
            "choices": [{"message": {"role": "assistant", "content": "hello"}}],
            "usage": {"prompt_tokens": 20, "completion_tokens": 4, "total_tokens": 24}
        })
        .to_string();
        let mut collector = Collector::new(Protocol::Json, false);
        feed_all(&mut collector, body.as_bytes(), 3);
        assert!(collector.stream_complete());
        let outcome = collector.finish(UsageReason::NoUsage);
        assert_eq!(outcome.status, UsageStatus::Complete);
        assert_eq!(outcome.numbers.total, Some(24));
        assert_eq!(collector.response_model.as_deref(), Some("model-y"));
        assert!(collector.first_content);

        let huge = format!("{{\"padding\":\"{}\"}}", "y".repeat(MAX_JSON_BYTES + 16));
        let mut collector = Collector::new(Protocol::Json, false);
        feed_all(&mut collector, huge.as_bytes(), 4096);
        let outcome = collector.finish(UsageReason::NoUsage);
        assert_eq!(outcome.status, UsageStatus::Missing);
        assert_eq!(outcome.reason, Some(UsageReason::BodyTooLarge));
    }

    #[test]
    fn encoded_or_unknown_content_type_never_guesses_usage() {
        let mut collector = Collector::new(Protocol::Json, true);
        collector.observe(
            b"{\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}",
        );
        let outcome = collector.finish(UsageReason::NoUsage);
        assert_eq!(outcome.status, UsageStatus::Missing);
        assert_eq!(outcome.reason, Some(UsageReason::UnsupportedEncoding));

        let mut collector = Collector::new(Protocol::Unknown, false);
        collector.observe(b"prompt_tokens=5");
        let outcome = collector.finish(UsageReason::NoUsage);
        assert_eq!(outcome.status, UsageStatus::Missing);
    }

    #[test]
    fn trailing_event_without_terminator_is_flagged() {
        let body =
            "data: {\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":1,\"total_tokens\":4}}";
        let mut collector = sse_collector();
        feed_all(&mut collector, body.as_bytes(), 5);
        let outcome = collector.finish(UsageReason::NoUsage);
        assert_eq!(outcome.status, UsageStatus::Complete);
        assert!(collector.saw_protocol_error() || !collector.stream_complete());
    }

    #[test]
    fn over_long_line_does_not_block_later_events() {
        let mut body = String::from("data: ");
        body.push_str(&"z".repeat(MAX_EVENT_BYTES * 2));
        body.push_str("\n\ndata: {\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1,\"total_tokens\":3}}\n\ndata: [DONE]\n\n");
        let mut collector = sse_collector();
        feed_all(&mut collector, body.as_bytes(), 4096);
        let outcome = collector.finish(UsageReason::NoUsage);
        assert_eq!(outcome.status, UsageStatus::Complete);
        assert_eq!(outcome.numbers.total, Some(3));
    }
}

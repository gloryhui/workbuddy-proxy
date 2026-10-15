//! Bounded, allocation-conscious observation of upstream response bodies.
//!
//! The collector never owns the response: [`Collector::observe`] looks at the
//! bytes that are already being forwarded and only keeps a small amount of
//! parser state. Chat text, tool arguments and images are parsed for framing but
//! immediately dropped; nothing but counters, model names and usage numbers can
//! leave this module.
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

/// Validated usage numbers for one request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageNumbers {
    pub input: Option<i64>,
    pub output: Option<i64>,
    pub total: Option<i64>,
    pub cache_read: Option<i64>,
    pub cache_write: Option<i64>,
    pub reasoning: Option<i64>,
}

impl UsageNumbers {
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

/// Incrementally merges upstream usage snapshots.
///
/// Only missing fields may be filled in. A parent field (`prompt_tokens`,
/// `completion_tokens`, `total_tokens`) that two events disagree about
/// invalidates the whole usage, while a conflicting or unparsable *sub-field*
/// (cache/reasoning details) is dropped on its own and never discards a
/// trustworthy parent pair.
#[derive(Debug, Default)]
pub struct UsageAccumulator {
    candidate: Option<UsageNumbers>,
    /// The parent triple is missing, contradictory or conflicts across events.
    parent_invalid: bool,
    /// An optional sub-field was rejected; the parent stays complete.
    subfield_invalid: bool,
}

/// Reads one integer field. Returns `Err(())` for values that are present but
/// cannot be trusted (negative, fractional, string, exponential, too large).
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
    /// Records one `usage` object seen in the response.
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

    pub fn saw_usage(&self) -> bool {
        self.candidate.is_some()
    }

    /// Resolves the accumulated state into a final answer.
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

    /// True when the response ended the way its protocol requires.
    ///
    /// A non-streaming body is complete once EOF arrives; an unknown content
    /// type is judged at the transport level, while usage stays missing.
    /// Parses the bounded non-streaming copy exactly once and remembers the
    /// verdict. Safe to call from `terminal`, `finish` and `Drop`.
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

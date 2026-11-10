//! Request lifecycle events, the bounded hand-off queue and the per-request
//! guard that publishes at most one terminal event.
//!
//! Admission reserves two slots up front (`Start` and `Finish`) so a guard can
//! always publish its terminal event from `Drop` without blocking, awaiting or
//! spawning a background task.
//!
//! 上游是 `proxy::relay_to`、原生协议 `relay` 和连接测试；下游是单线程
//! `stats-writer`。请求任务在 `proxy-io` Tokio runtime 上同步写入有界队列，
//! writer 再批量落库。`RequestGuard` 同时保存该请求的计时、协议观察状态和队列预约，
//! 因此它随响应 body stream 移动；客户端取消、网络 runtime 销毁都会触发其 `Drop`。
//!
//! 可选数值的 `None` 表示未知或未观察到；`Some(0)` 表示已经观察到一个合法的零值。
//! “代理已经把上游声明的全部字节交给 HTTP body 层”仍不等于客户端 socket 已确认收完。
use crate::stats::collector::{Collector, JsonVerdict, Protocol, UsageOutcome};
use crate::stats::dto::{
    ErrorKind, InputBreakdown, Outcome, Source, UsageReason, UsageSource, UsageStatus,
};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Live counters shared with the health endpoint. Only atomics are used so the
/// hot path never waits on the writer thread.
#[derive(Debug, Default)]
pub struct Counters {
    pub admitted: std::sync::atomic::AtomicU64,
    pub dropped: std::sync::atomic::AtomicU64,
    pub write_failures: std::sync::atomic::AtomicU64,
    pub pending: std::sync::atomic::AtomicI64,
    pub commits: std::sync::atomic::AtomicU64,
    pub last_commit_at_ms: std::sync::atomic::AtomicI64,
    pub closing: AtomicBool,
    pub enabled: AtomicBool,
    pub accepting: AtomicBool,
}

/// Exact, allowlisted WorkBuddy correlation. Store hashes, never raw headers.
#[derive(Clone, Debug)]
pub struct WorkbuddyIdentity {
    pub session_hash: String,
    pub message_hash: String,
}
impl WorkbuddyIdentity {
    pub fn from_headers(headers: &axum::http::HeaderMap) -> Option<Self> {
        let read = |name| {
            let mut values = headers.get_all(name).iter();
            let value = values.next()?.to_str().ok()?;
            if values.next().is_some()
                || value.is_empty()
                || value.chars().count() > 160
                || value.chars().any(char::is_control)
            {
                return None;
            }
            Some(crate::stats::workbuddy_import::parser::hash_id(value))
        };
        Some(Self {
            session_hash: read("x-conversation-id")?,
            message_hash: read("x-conversation-message-id")?,
        })
    }
}

/// Immutable snapshot of the routing decision, cloned while it is still valid.
#[derive(Clone, Debug)]
pub struct RequestMeta {
    pub workbuddy_identity: Option<crate::stats::event::WorkbuddyIdentity>,
    pub request_id: String,
    pub run_id: String,
    pub source: Source,
    pub provider_id: String,
    pub provider_name: String,
    pub route_id: String,
    pub route_name: String,
    pub requested_model: String,
    pub upstream_model: String,
    pub upstream_protocol: String,
    pub stream: bool,
    pub started_at_ms: i64,
    /// Unit prices copied from the published cache when the request was
    /// admitted. The writer stores it at `Start` and settles from it at
    /// `Finish`, so a later price edit or delete cannot change this request.
    pub price: crate::stats::store::PriceSnapshot,
}

/// Everything the writer needs to close a request row.
///
/// `*_at_ms` 是 Unix epoch 毫秒；`duration_ms`、`ttfb_ms` 和 `first_content_ms`
/// 是基于进程内单调时钟测得的毫秒时长。可选的 token、状态、时延和模型字段为 `None`
/// 时表示未观察到、未知或该项不适用；已经确认的数值零仍保留为 `Some(0)`。
#[derive(Clone, Debug)]
pub struct FinishRecord {
    pub ended_at_ms: i64,
    pub duration_ms: i64,
    pub ttfb_ms: Option<i64>,
    pub first_content_ms: Option<i64>,
    pub upstream_started: bool,
    pub response_model: Option<String>,
    pub http_status: Option<i64>,
    pub outcome: Outcome,
    pub error_kind: Option<ErrorKind>,
    pub usage_status: UsageStatus,
    pub usage_reason: Option<UsageReason>,
    pub usage_source: UsageSource,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub input_uncached_tokens: Option<i64>,
    pub input_breakdown: InputBreakdown,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug)]
/// 一条已准入请求的生命周期消息。
///
/// writer 通常先收到携带冻结 `RequestMeta` 的 `Start`，再收到用同一 `request_id`
/// 关闭记录的 `Finish`。两者被拆分入队是为了让 admission 和终态都能非阻塞提交；数据库
/// 侧负责把两条事件合成为请求记录，因此不能只根据内存中看到 `Start` 就断言已经落库。
pub enum StatsEvent {
    Start(Box<RequestMeta>),
    Finish {
        request_id: String,
        finish: Box<FinishRecord>,
    },
}

impl StatsEvent {
    /// Machine-readable event name used in diagnostics (never request content).
    pub fn kind(&self) -> &'static str {
        match self {
            StatsEvent::Start(_) => "start",
            StatsEvent::Finish { .. } => "finish",
        }
    }
}

#[derive(Debug, Default)]
struct QueueState {
    items: VecDeque<StatsEvent>,
    reserved: usize,
    closed: bool,
}

/// Bounded multi-producer/single-consumer queue with explicit slot reservation.
///
/// 每个 admission 先预约两个空位：一个立即写 `Start`，另一个留给之后的
/// `Finish`。预约在准入阶段一次性计入容量，所以即使 writer 延迟处理，队列也能保证
/// 已准入的 guard 最后无需等待即可提交终态。队列满时 reservation 立即失败，聊天请求
/// 继续执行并把这次统计记为 dropped。
#[derive(Debug)]
pub struct EventQueue {
    state: Mutex<QueueState>,
    ready: Condvar,
    capacity: usize,
}

impl EventQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            state: Mutex::new(QueueState::default()),
            ready: Condvar::new(),
            capacity,
        }
    }

    /// Non-blocking reservation of `slots` future events.
    ///
    /// 成功返回的 `Reservation` 持有 queue 的 `Arc` 和尚未使用的槽位数。
    /// 这里是同步临界区，不等待 writer，也不创建数据库连接。失败只表示队列已关闭或
    /// 已排队事件与已预约槽位之和放不下 `slots`；调用方据此跳过统计。
    pub fn reserve(self: &Arc<Self>, slots: usize) -> Option<Reservation> {
        let mut state = self.state.lock().unwrap();
        if state.closed || state.items.len() + state.reserved + slots > self.capacity {
            return None;
        }
        state.reserved += slots;
        Some(Reservation {
            queue: self.clone(),
            slots,
        })
    }

    fn push_raw(&self, event: StatsEvent) {
        let mut state = self.state.lock().unwrap();
        // The reservation is converted into a queued item.
        state.reserved = state.reserved.saturating_sub(1);
        state.items.push_back(event);
        drop(state);
        self.ready.notify_one();
    }

    fn release(&self, slots: usize) {
        let mut state = self.state.lock().unwrap();
        state.reserved = state.reserved.saturating_sub(slots);
        drop(state);
        self.ready.notify_all();
    }

    /// Waits up to `wait` for at least one event, then drains up to `max`.
    ///
    /// 返回值可能为空：既可能因等待超时，也可能因队列已关闭且没有剩余事件。
    /// reservation 只占预算，不能作为事件被 `pop_batch` 取出；writer 只消费已经提交的
    /// `StatsEvent`。
    pub fn pop_batch(&self, max: usize, wait: Duration) -> Vec<StatsEvent> {
        let mut state = self.state.lock().unwrap();
        if state.items.is_empty() && !state.closed {
            let (next, _) = self.ready.wait_timeout(state, wait).unwrap();
            state = next;
        }
        let mut batch = Vec::new();
        while batch.len() < max {
            match state.items.pop_front() {
                Some(event) => batch.push(event),
                None => break,
            }
        }
        batch
    }

    /// Wakes the writer without publishing an event (used by control traffic).
    pub fn wake(&self) {
        self.ready.notify_all();
    }

    pub fn close(&self) {
        let mut state = self.state.lock().unwrap();
        state.closed = true;
        drop(state);
        self.ready.notify_all();
    }

    pub fn is_closed(&self) -> bool {
        self.state.lock().unwrap().closed
    }

    pub fn depth(&self) -> usize {
        self.state.lock().unwrap().items.len()
    }

    #[allow(dead_code)] // Reported by `stats_health` through the shared constant.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

/// Reserved queue slots held by a guard.
///
/// `drop` 时释放仍未消费的预约。`RequestGuard` 正常提交 `Start` 后还剩一个槽位，
/// 这个槽位专门保证终态能在 `Drop` 中发布。
#[derive(Debug)]
pub struct Reservation {
    queue: Arc<EventQueue>,
    slots: usize,
}

impl Reservation {
    /// Publishes one event using one reserved slot.
    pub fn push(&mut self, event: StatsEvent) {
        if self.slots == 0 {
            return;
        }
        self.slots -= 1;
        self.queue.push_raw(event);
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.slots > 0 {
            self.queue.release(self.slots);
        }
    }
}

/// Terminal accounting for one admitted request.
///
/// `meta` 是准入时冻结的路由、模型、价格和时间点；后续配置或价格修改不会重算这次
/// 请求。下面几个可选字段的 `None` 都有业务含义：`upstream_at`、`ttfb_ms`、
/// `first_content_ms` 未被观察，`http_status`/`http_ok` 尚未取得上游状态，
/// `expected_bytes` 没有可用的 `Content-Length`。`observed_bytes == 0` 则是已经累计到
/// 零字节，与“未知”不同。
#[derive(Debug)]
pub struct RequestGuard {
    pub meta: RequestMeta,
    reservation: Reservation,
    started: Instant,
    upstream_at: Option<Instant>,
    ttfb_ms: Option<i64>,
    first_content_ms: Option<i64>,
    collector: Collector,
    native: Option<NativeObservation>,
    http_status: Option<i64>,
    http_ok: Option<bool>,
    /// `Content-Length` of the upstream body when the upstream declared one.
    ///
    /// `None` 表示头缺失或解析失败，不能假定 body 一定为空；`Some(0)` 才是上游明确
    /// 声明了零长度。
    expected_bytes: Option<u64>,
    observed_bytes: u64,
    upstream_started: bool,
    finished: bool,
    shared: Arc<Counters>,
}
#[derive(Debug)]
struct NativeObservation {
    usage: UsageOutcome,
    complete: bool,
}

impl RequestGuard {
    /// Reserves queue capacity and publishes the `Start` event.
    ///
    /// Returns `None` when the queue is full; the caller counts one dropped
    /// request and continues serving the chat request unchanged.
    ///
    /// 输入中的 `meta` 必须来自已经成功的路由解析，`protocol` 可先为 `Unknown`，
    /// 等上游 `Content-Type` 已知后再调用 `attach_collector`。成功会同时预约
    /// `Start`/`Finish` 两个配额、发布 `Start` 并增加 pending；失败不会发布半个事件，
    /// 也不会阻断实际代理请求。时间基准在成功准入时建立。
    pub fn begin(
        queue: &Arc<EventQueue>,
        counters: &Arc<Counters>,
        meta: RequestMeta,
        protocol: Protocol,
        encoded: bool,
    ) -> Option<Self> {
        let mut reservation = queue.reserve(2)?;
        // One reserved slot is spent on `Start`; the second one is kept so the
        // guard can always publish `Finish`, even from `Drop`.
        reservation.push(StatsEvent::Start(Box::new(meta.clone())));
        counters.pending.fetch_add(1, Ordering::Relaxed);
        Some(Self {
            meta,
            reservation,
            started: Instant::now(),
            upstream_at: None,
            ttfb_ms: None,
            first_content_ms: None,
            collector: Collector::new(protocol, encoded),
            native: None,
            http_status: None,
            http_ok: None,
            expected_bytes: None,
            observed_bytes: 0,
            upstream_started: false,
            finished: false,
            shared: counters.clone(),
        })
    }

    /// Marks the moment the upstream call is dispatched (monotonic clock).
    pub fn mark_upstream_started(&mut self) {
        if self.upstream_at.is_none() {
            self.upstream_at = Some(Instant::now());
            self.upstream_started = true;
        }
    }

    pub fn set_http_status(&mut self, status: u16) {
        self.http_status = Some(i64::from(status));
        self.http_ok = Some((200..300).contains(&status));
    }

    /// Records the declared upstream body length, when the upstream sent one.
    pub fn set_content_length(&mut self, length: Option<u64>) {
        self.expected_bytes = length;
    }

    /// Replaces the collector once the real content type is known.
    pub fn attach_collector(&mut self, protocol: Protocol, encoded: bool) {
        self.collector = Collector::new(protocol, encoded);
    }

    /// Observes bytes that are about to be yielded toward the HTTP body layer.
    ///
    /// 本函数先累计字节、TTFB 和首个内容时间，再把同一 slice 交给 collector 做有界解析；
    /// 不接管或替换响应 buffer；collector 可能保留有界解析副本。观察结果不能证明
    /// 客户端 socket 已经收到这些字节。
    pub fn observe(&mut self, chunk: &[u8]) {
        self.observed_bytes = self.observed_bytes.saturating_add(chunk.len() as u64);
        if self.ttfb_ms.is_none() {
            self.ttfb_ms = Some(self.elapsed_ms());
        }
        self.collector.observe(chunk);
        if self.first_content_ms.is_none() && self.collector.first_content {
            self.first_content_ms = Some(self.elapsed_ms());
        }
    }

    /// Native adapters supply only normalized counters and semantic timing.
    /// Raw messages, images, tools and opaque signatures never enter the stats
    /// collector or queue. Updates replace cumulative usage, never add it.
    pub fn observe_native(
        &mut self,
        bytes: usize,
        content: bool,
        usage: Option<UsageOutcome>,
        model: Option<&str>,
    ) {
        self.observed_bytes = self.observed_bytes.saturating_add(bytes as u64);
        if bytes > 0 && self.ttfb_ms.is_none() {
            self.ttfb_ms = Some(self.elapsed_ms());
        }
        if content && self.first_content_ms.is_none() {
            self.first_content_ms = Some(self.elapsed_ms());
        }
        let state = self.native.get_or_insert_with(|| NativeObservation {
            usage: UsageOutcome::missing(UsageReason::NoUsage),
            complete: false,
        });
        if let Some(usage) = usage {
            state.usage = usage;
        }
        if let Some(model) = model {
            self.collector.response_model = Some(model.into());
        }
    }

    /// Called after the adapter's terminal state and clean transport EOF agree.
    pub fn native_complete(&mut self) {
        if let Some(state) = self.native.as_mut() {
            state.complete = true;
        }
    }

    fn elapsed_ms(&self) -> i64 {
        match self.upstream_at {
            Some(start) => start.elapsed().as_millis() as i64,
            None => self.started.elapsed().as_millis() as i64,
        }
    }

    /// True when every byte declared by `Content-Length` was observed and yielded
    /// by this proxy stream.
    ///
    /// HTTP servers may release a known-length body stream without polling it
    /// to `None`, so completion cannot rely on the stream alone. This still does
    /// not prove that the client socket acknowledged those bytes.
    pub fn body_fully_forwarded(&self) -> bool {
        self.expected_bytes
            .is_some_and(|expected| self.observed_bytes >= expected)
    }

    /// Terminal classification shared by the stream and the guard's `Drop`.
    pub fn terminal(&mut self) -> (Outcome, Option<ErrorKind>) {
        if self.http_ok == Some(false) {
            return (Outcome::Failed, Some(ErrorKind::UpstreamHttp));
        }
        if let Some(native) = &self.native {
            return if native.complete {
                (Outcome::Completed, None)
            } else {
                (Outcome::Interrupted, Some(ErrorKind::UpstreamStream))
            };
        }
        if self.collector.saw_protocol_error() {
            return (Outcome::Failed, Some(ErrorKind::SseError));
        }
        // A non-streaming body is classified only after its bounded copy
        // reached EOF and parsed: an error envelope or malformed JSON failed
        // even though the HTTP status was 200.
        self.collector.settle_json();
        if matches!(
            self.collector.json_verdict(),
            JsonVerdict::ErrorObject | JsonVerdict::Malformed
        ) {
            return (Outcome::Failed, Some(ErrorKind::UpstreamStream));
        }
        if self.collector.stream_complete() {
            (Outcome::Completed, None)
        } else {
            (Outcome::Interrupted, Some(ErrorKind::UpstreamStream))
        }
    }

    /// Publishes the terminal event exactly once.
    ///
    /// 首次调用会冻结 usage、耗时和状态，使用 admission 留下的第二个配额发布
    /// `Finish`，并把 pending 减一。重复调用直接返回。这里只负责把事件放入内存队列；
    /// 数据库写入由 writer 异步完成，写入失败也不会反向改变已经发送的聊天响应。
    pub fn finish(&mut self, outcome: Outcome, error_kind: Option<ErrorKind>) {
        if self.finished {
            return;
        }
        self.finished = true;
        let now_ms = crate::stats::now_ms();
        let mut finish = self.build_finish(outcome, error_kind, now_ms);
        finish.updated_at_ms = now_ms.max(self.meta.started_at_ms);
        self.reservation.push(StatsEvent::Finish {
            request_id: self.meta.request_id.clone(),
            finish: Box::new(finish),
        });
        self.shared.pending.fetch_sub(1, Ordering::Relaxed);
    }

    fn build_finish(
        &mut self,
        outcome: Outcome,
        error_kind: Option<ErrorKind>,
        now_ms: i64,
    ) -> FinishRecord {
        let duration_ms = self.started.elapsed().as_millis() as i64;
        let fallback = match outcome {
            Outcome::Cancelled => UsageReason::ClientDrop,
            Outcome::Interrupted => UsageReason::UncleanShutdown,
            Outcome::Failed | Outcome::Rejected => UsageReason::StreamError,
            _ => UsageReason::NoUsage,
        };
        let mut usage = self
            .native
            .as_ref()
            .map(|state| state.usage.clone())
            .unwrap_or_else(|| self.collector.finish(fallback));
        if self.native.is_some()
            && (usage.status == UsageStatus::Missing
                || usage.status == UsageStatus::Partial
                    && usage.reason == Some(UsageReason::NoUsage))
        {
            usage.reason = Some(fallback);
        }
        let usage = adjust_usage(usage, outcome, self.upstream_started);
        let (status, source, numbers, breakdown, uncached) = decompose(&usage, outcome);
        FinishRecord {
            ended_at_ms: now_ms.max(self.meta.started_at_ms),
            duration_ms: duration_ms.max(0),
            ttfb_ms: self.ttfb_ms,
            first_content_ms: self.first_content_ms,
            upstream_started: self.upstream_started,
            response_model: self.collector.response_model.take().map(|model| {
                model
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(160)
                    .collect()
            }),
            http_status: self.http_status,
            outcome,
            error_kind,
            usage_status: status,
            usage_reason: usage.reason,
            usage_source: source,
            input_tokens: numbers.0,
            output_tokens: numbers.1,
            total_tokens: numbers.2,
            cache_read_tokens: numbers.3,
            cache_write_tokens: numbers.4,
            reasoning_tokens: numbers.5,
            input_uncached_tokens: uncached,
            input_breakdown: breakdown,
            updated_at_ms: now_ms,
        }
    }

    /// Marks the whole process as shutting down so open guards are classified
    /// as interrupted rather than client cancellations.
    pub fn mark_closing(counters: &Counters) {
        counters.closing.store(true, Ordering::Relaxed);
    }
}

/// Applies request-level rules that override the raw parser answer.
fn adjust_usage(mut usage: UsageOutcome, outcome: Outcome, upstream_started: bool) -> UsageOutcome {
    if !upstream_started {
        // Nothing was sent, so usage is not applicable even if a stray parse
        // produced numbers.
        return UsageOutcome::not_applicable();
    }
    if outcome == Outcome::Interrupted && usage.status != UsageStatus::Complete {
        usage.status = UsageStatus::Missing;
        usage.reason = Some(UsageReason::UncleanShutdown);
    }
    usage
}

type Numbers = (
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
);

fn decompose(
    usage: &UsageOutcome,
    outcome: Outcome,
) -> (
    UsageStatus,
    UsageSource,
    Numbers,
    InputBreakdown,
    Option<i64>,
) {
    let empty = (None, None, None, None, None, None);
    match usage.status {
        UsageStatus::Complete => {
            let numbers = &usage.numbers;
            // `Complete` is only produced when both parent fields are valid.
            let (Some(input), Some(output)) = (numbers.input, numbers.output) else {
                return (
                    UsageStatus::Partial,
                    UsageSource::Upstream,
                    empty,
                    InputBreakdown::Unknown,
                    None,
                );
            };
            let total = numbers.total.unwrap_or(input + output);
            (
                UsageStatus::Complete,
                UsageSource::Upstream,
                (
                    Some(input),
                    Some(output),
                    Some(total),
                    numbers.cache_read,
                    numbers.cache_write,
                    numbers.reasoning,
                ),
                usage.breakdown,
                usage.uncached,
            )
        }
        UsageStatus::Partial => {
            let numbers = &usage.numbers;
            (
                UsageStatus::Partial,
                UsageSource::Upstream,
                (
                    numbers.input,
                    numbers.output,
                    None,
                    numbers.cache_read,
                    numbers.cache_write,
                    numbers.reasoning,
                ),
                InputBreakdown::Unknown,
                None,
            )
        }
        UsageStatus::Invalid => {
            // Conflicting numbers are recorded as a status only: the schema
            // forbids writing an inconsistent total next to its parts.
            let _ = outcome;
            (
                UsageStatus::Invalid,
                UsageSource::None,
                empty,
                InputBreakdown::Unknown,
                None,
            )
        }
        UsageStatus::Missing => (
            UsageStatus::Missing,
            UsageSource::None,
            empty,
            InputBreakdown::Unknown,
            None,
        ),
        UsageStatus::NotApplicable => (
            UsageStatus::NotApplicable,
            UsageSource::None,
            empty,
            InputBreakdown::Unknown,
            None,
        ),
    }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        if self.shared.closing.load(Ordering::Relaxed) {
            self.finish(Outcome::Interrupted, Some(ErrorKind::Shutdown));
        } else if self.body_fully_forwarded() {
            // Every declared byte was observed and yielded by the response stream:
            // the response can be classified even if the stream was released
            // before polling `None`. This is not client socket acknowledgement.
            let (outcome, kind) = self.terminal();
            self.finish(outcome, kind);
        } else {
            // 没有完整长度证据且流未主动提交终态时，只能记录为客户端取消；不能把
            // “上游某个 future 已结束”推断成客户端已收到完整响应。
            self.finish(Outcome::Cancelled, Some(ErrorKind::ClientDrop));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::dto::UsageStatus;

    #[test]
    fn native_cumulative_usage_reuses_the_exactly_once_finish_queue_with_disjoint_input() {
        use crate::protocol::{config::UpstreamProtocol, usage};
        let queue = Arc::new(EventQueue::new(8));
        let counters = Arc::new(Counters::default());
        let mut guard =
            RequestGuard::begin(&queue, &counters, meta("native"), Protocol::Unknown, false)
                .unwrap();
        guard.mark_upstream_started();
        guard.set_http_status(200);
        for output in [0, 7] {
            let u = usage::normalize(
                UpstreamProtocol::AnthropicMessages,
                &serde_json::json!({"input_tokens":10,"cache_read_input_tokens":40,"cache_creation_input_tokens":5,"output_tokens":output}),
            );
            guard.observe_native(100, output > 0, Some(u.outcome()), Some("native-model"));
        }
        assert_eq!(guard.terminal().0, Outcome::Interrupted);
        guard.native_complete();
        let (outcome, kind) = guard.terminal();
        assert_eq!(outcome, Outcome::Completed);
        guard.finish(outcome, kind);
        guard.finish(Outcome::Failed, Some(ErrorKind::Other));
        drop(guard);
        let events = queue.pop_batch(16, Duration::from_millis(1));
        assert_eq!(events.len(), 2);
        let finish = finish_record(&events);
        assert_eq!(finish.input_tokens, Some(55));
        assert_eq!(finish.output_tokens, Some(7));
        assert_eq!(finish.total_tokens, Some(62));
        assert_eq!(finish.input_breakdown, InputBreakdown::Disjoint);
        assert_eq!(finish.input_uncached_tokens, Some(10));
        assert_eq!(finish.cache_read_tokens, Some(40));
        assert_eq!(finish.response_model.as_deref(), Some("native-model"));
        assert!(finish.first_content_ms.is_some());
        assert_eq!(counters.pending.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn native_cancellation_never_claims_a_valid_terminal() {
        let queue = Arc::new(EventQueue::new(8));
        let counters = Arc::new(Counters::default());
        let mut guard = RequestGuard::begin(
            &queue,
            &counters,
            meta("native-cancel"),
            Protocol::Unknown,
            false,
        )
        .unwrap();
        guard.mark_upstream_started();
        guard.set_http_status(200);
        guard.observe_native(12, true, None, None);
        drop(guard);
        let events = queue.pop_batch(16, Duration::from_millis(1));
        let finish = finish_record(&events);
        assert_eq!(finish.outcome, Outcome::Cancelled);
        assert_eq!(finish.error_kind, Some(ErrorKind::ClientDrop));
        assert_eq!(events.len(), 2);
    }

    fn meta(id: &str) -> RequestMeta {
        RequestMeta {
            workbuddy_identity: None,
            request_id: id.into(),
            run_id: "run-a".into(),
            source: Source::Relay,
            provider_id: "p1".into(),
            provider_name: "provider".into(),
            route_id: "r1".into(),
            route_name: "route".into(),
            requested_model: "model-x".into(),
            upstream_model: "model-x".into(),
            upstream_protocol: "openai_chat".into(),
            stream: true,
            started_at_ms: crate::stats::now_ms(),
            price: crate::stats::store::PriceSnapshot::default_zero("CNY", crate::stats::now_ms()),
        }
    }

    fn counts(events: &[StatsEvent]) -> (usize, usize) {
        let starts = events
            .iter()
            .filter(|e| matches!(e, StatsEvent::Start(_)))
            .count();
        let finishes = events
            .iter()
            .filter(|e| matches!(e, StatsEvent::Finish { .. }))
            .count();
        (starts, finishes)
    }

    #[test]
    fn queue_reserves_two_slots_and_rejects_when_full() {
        let queue = Arc::new(EventQueue::new(4));
        let counters = Arc::new(Counters::default());
        let a = RequestGuard::begin(&queue, &counters, meta("a"), Protocol::Sse, false);
        assert!(a.is_some());
        let b = RequestGuard::begin(&queue, &counters, meta("b"), Protocol::Sse, false);
        assert!(b.is_some());
        // Four slots are reserved (two per guard); the queue is full.
        let c = RequestGuard::begin(&queue, &counters, meta("c"), Protocol::Sse, false);
        assert!(c.is_none());
        assert_eq!(counters.pending.load(Ordering::Relaxed), 2);
        drop(a);
        drop(b);
        assert_eq!(counters.pending.load(Ordering::Relaxed), 0);
        let events = queue.pop_batch(16, Duration::from_millis(1));
        assert_eq!(counts(&events), (2, 2));
    }

    #[test]
    fn guard_finishes_exactly_once_for_every_terminal_path() {
        for outcome in [Outcome::Completed, Outcome::Failed, Outcome::Cancelled] {
            let queue = Arc::new(EventQueue::new(8));
            let counters = Arc::new(Counters::default());
            let mut guard =
                RequestGuard::begin(&queue, &counters, meta("x"), Protocol::Sse, false).unwrap();
            guard.mark_upstream_started();
            guard.finish(outcome, None);
            guard.finish(outcome, None);
            drop(guard);
            let events = queue.pop_batch(16, Duration::from_millis(1));
            assert_eq!(counts(&events), (1, 1), "outcome={outcome:?}");
            assert_eq!(counters.pending.load(Ordering::Relaxed), 0);
        }
    }

    fn json_guard(id: &str, body: &[u8]) -> (RequestGuard, Arc<EventQueue>, Arc<Counters>) {
        let queue = Arc::new(EventQueue::new(8));
        let counters = Arc::new(Counters::default());
        let mut guard =
            RequestGuard::begin(&queue, &counters, meta(id), Protocol::Json, false).unwrap();
        guard.mark_upstream_started();
        guard.set_content_length(Some(body.len() as u64));
        guard.observe(body);
        (guard, queue, counters)
    }

    fn finish_record(events: &[StatsEvent]) -> FinishRecord {
        events
            .iter()
            .find_map(|event| match event {
                StatsEvent::Finish { finish, .. } => Some((**finish).clone()),
                _ => None,
            })
            .expect("a finish event")
    }

    /// F06: an HTTP 200 body carrying an `error` object is a failed request,
    /// not a completed one.
    #[test]
    fn a_json_error_object_is_failed_even_with_http_200() {
        let body = br#"{"error":{"message":"fixture upstream error"}}"#;
        let (mut guard, queue, _counters) = json_guard("json-error", body);
        let (outcome, kind) = guard.terminal();
        guard.finish(outcome, kind);
        drop(guard);
        let events = queue.pop_batch(16, Duration::from_millis(1));
        let finish = finish_record(&events);
        assert_eq!(finish.outcome, Outcome::Failed, "{finish:?}");
        assert_ne!(finish.outcome, Outcome::Completed);
    }

    /// F06: malformed JSON must never be reported as completed.
    #[test]
    fn malformed_json_is_never_completed() {
        let (mut guard, queue, _counters) = json_guard("json-broken", b"{\"broken\":");
        let (outcome, kind) = guard.terminal();
        guard.finish(outcome, kind);
        drop(guard);
        let events = queue.pop_batch(16, Duration::from_millis(1));
        let finish = finish_record(&events);
        assert_eq!(finish.outcome, Outcome::Failed, "{finish:?}");
        assert_eq!(finish.usage_status, UsageStatus::Missing);
    }

    /// F06: a healthy non-streaming body still completes and keeps its usage.
    #[test]
    fn a_healthy_json_body_still_completes_with_usage() {
        let body = br#"{"model":"model-y","usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#;
        let (mut guard, queue, _counters) = json_guard("json-ok", body);
        let (outcome, kind) = guard.terminal();
        guard.finish(outcome, kind);
        drop(guard);
        let events = queue.pop_batch(16, Duration::from_millis(1));
        let finish = finish_record(&events);
        assert_eq!(finish.outcome, Outcome::Completed, "{finish:?}");
        assert_eq!(finish.total_tokens, Some(15));
        assert_eq!(finish.usage_status, UsageStatus::Complete);
    }

    /// F08: a guard dropped after `mark_closing` (a slow stream during normal
    /// shutdown) is `interrupted/shutdown`, not a fake client cancellation.
    #[test]
    fn closing_turns_a_dropped_stream_into_a_shutdown_interruption() {
        let queue = Arc::new(EventQueue::new(8));
        let counters = Arc::new(Counters::default());
        let mut guard =
            RequestGuard::begin(&queue, &counters, meta("slow"), Protocol::Sse, false).unwrap();
        guard.mark_upstream_started();
        // A declared body that never fully arrived, with no cancellation.
        guard.set_content_length(Some(4096));
        guard.observe(b"data: {\"cho");
        RequestGuard::mark_closing(&counters);
        drop(guard);
        let events = queue.pop_batch(16, Duration::from_millis(1));
        let finish = finish_record(&events);
        assert_eq!(finish.outcome, Outcome::Interrupted, "{finish:?}");
        assert_eq!(finish.error_kind, Some(ErrorKind::Shutdown));
    }

    /// F08: without `mark_closing` the very same drop is a real client
    /// cancellation, and it still releases its pending slot.
    #[test]
    fn a_real_client_cancellation_stays_cancelled() {
        let queue = Arc::new(EventQueue::new(8));
        let counters = Arc::new(Counters::default());
        let mut guard =
            RequestGuard::begin(&queue, &counters, meta("gone"), Protocol::Sse, false).unwrap();
        guard.mark_upstream_started();
        guard.set_content_length(Some(4096));
        guard.observe(b"data: {\"cho");
        drop(guard);
        let events = queue.pop_batch(16, Duration::from_millis(1));
        let finish = finish_record(&events);
        assert_eq!(finish.outcome, Outcome::Cancelled, "{finish:?}");
        assert_eq!(finish.error_kind, Some(ErrorKind::ClientDrop));
        assert_eq!(counters.pending.load(Ordering::Relaxed), 0);
    }

    /// F08: a fully forwarded body still completes even while closing.
    #[test]
    fn closing_does_not_rewrite_a_completed_response() {
        let queue = Arc::new(EventQueue::new(8));
        let counters = Arc::new(Counters::default());
        let mut guard =
            RequestGuard::begin(&queue, &counters, meta("done"), Protocol::Json, false).unwrap();
        guard.mark_upstream_started();
        let body = br#"{"usage":{"prompt_tokens":5,"completion_tokens":5,"total_tokens":10}}"#;
        guard.set_content_length(Some(body.len() as u64));
        guard.observe(body);
        let (outcome, kind) = guard.terminal();
        guard.finish(outcome, kind);
        RequestGuard::mark_closing(&counters);
        drop(guard);
        let events = queue.pop_batch(16, Duration::from_millis(1));
        let finish = finish_record(&events);
        assert_eq!(finish.outcome, Outcome::Completed, "{finish:?}");
        assert_eq!(finish.total_tokens, Some(10));
    }

    #[test]
    fn dropped_guard_records_cancellation_and_releases_pending() {
        let queue = Arc::new(EventQueue::new(8));
        let counters = Arc::new(Counters::default());
        {
            let _guard =
                RequestGuard::begin(&queue, &counters, meta("d"), Protocol::Sse, false).unwrap();
        }
        let events = queue.pop_batch(16, Duration::from_millis(1));
        let finish = events
            .iter()
            .find_map(|event| match event {
                StatsEvent::Finish { finish, .. } => Some(finish.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(finish.outcome, Outcome::Cancelled);
        assert_eq!(finish.error_kind, Some(ErrorKind::ClientDrop));
        assert!(finish.duration_ms >= 0);
        assert_eq!(counters.pending.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn local_rejection_never_claims_upstream_usage() {
        let queue = Arc::new(EventQueue::new(8));
        let counters = Arc::new(Counters::default());
        let mut guard =
            RequestGuard::begin(&queue, &counters, meta("r"), Protocol::Unknown, false).unwrap();
        guard.finish(Outcome::Rejected, Some(ErrorKind::MissingKey));
        let events = queue.pop_batch(16, Duration::from_millis(1));
        let finish = events
            .iter()
            .find_map(|event| match event {
                StatsEvent::Finish { finish, .. } => Some(finish.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(finish.usage_status, UsageStatus::NotApplicable);
        assert_eq!(finish.usage_source, UsageSource::None);
        assert!(!finish.upstream_started);
        assert_eq!(finish.input_tokens, None);
        assert_eq!(finish.total_tokens, None);
    }

    #[test]
    fn complete_usage_is_kept_even_when_the_client_cancels() {
        let queue = Arc::new(EventQueue::new(8));
        let counters = Arc::new(Counters::default());
        let mut guard =
            RequestGuard::begin(&queue, &counters, meta("c"), Protocol::Sse, false).unwrap();
        guard.mark_upstream_started();
        guard.observe(b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n");
        guard.observe(b"data: {\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2,\"total_tokens\":12}}\n\n");
        guard.finish(Outcome::Cancelled, Some(ErrorKind::ClientDrop));
        let events = queue.pop_batch(16, Duration::from_millis(1));
        let finish = events
            .iter()
            .find_map(|event| match event {
                StatsEvent::Finish { finish, .. } => Some(finish.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(finish.outcome, Outcome::Cancelled);
        assert_eq!(finish.usage_status, UsageStatus::Complete);
        assert_eq!(finish.total_tokens, Some(12));
        assert!(finish.first_content_ms.is_some());
        assert!(finish.ttfb_ms.is_some());
    }

    #[test]
    fn shutdown_classifies_open_guards_as_interrupted() {
        let queue = Arc::new(EventQueue::new(8));
        let counters = Arc::new(Counters::default());
        {
            let _guard =
                RequestGuard::begin(&queue, &counters, meta("s"), Protocol::Sse, false).unwrap();
            RequestGuard::mark_closing(&counters);
        }
        let events = queue.pop_batch(16, Duration::from_millis(1));
        let finish = events
            .iter()
            .find_map(|event| match event {
                StatsEvent::Finish { finish, .. } => Some(finish.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(finish.outcome, Outcome::Interrupted);
        assert_eq!(finish.error_kind, Some(ErrorKind::Shutdown));
        assert!(finish.duration_ms >= 0);
    }

    #[test]
    fn invalid_usage_is_recorded_without_inconsistent_numbers() {
        let queue = Arc::new(EventQueue::new(8));
        let counters = Arc::new(Counters::default());
        let mut guard =
            RequestGuard::begin(&queue, &counters, meta("i"), Protocol::Sse, false).unwrap();
        guard.mark_upstream_started();
        guard.observe(b"data: {\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1,\"total_tokens\":6}}\n\n");
        guard.observe(b"data: {\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":1,\"total_tokens\":10}}\n\n");
        guard.finish(Outcome::Completed, None);
        let events = queue.pop_batch(16, Duration::from_millis(1));
        let finish = events
            .iter()
            .find_map(|event| match event {
                StatsEvent::Finish { finish, .. } => Some(finish.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(finish.usage_status, UsageStatus::Invalid);
        assert_eq!(finish.usage_source, UsageSource::None);
        assert!(finish.input_tokens.is_none() && finish.output_tokens.is_none());
        assert!(finish.total_tokens.is_none());
    }
}

#[cfg(test)]
mod workbuddy_identity_tests {
    use super::WorkbuddyIdentity;
    #[test]
    fn exact_headers_are_hashed_and_ambiguous_or_partial_identity_is_ignored() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("X-Conversation-ID", "session-a".parse().unwrap());
        assert!(WorkbuddyIdentity::from_headers(&headers).is_none());
        headers.insert("X-Conversation-Message-ID", "message-a".parse().unwrap());
        let value = WorkbuddyIdentity::from_headers(&headers).unwrap();
        assert_eq!(
            value.session_hash,
            crate::stats::workbuddy_import::parser::hash_id("session-a")
        );
        assert_eq!(
            value.message_hash,
            crate::stats::workbuddy_import::parser::hash_id("message-a")
        );
        headers.append("X-Conversation-Message-ID", "message-b".parse().unwrap());
        assert!(WorkbuddyIdentity::from_headers(&headers).is_none());
    }
}

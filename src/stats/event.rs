//! Request lifecycle events, the bounded hand-off queue and the per-request
//! guard that guarantees exactly one terminal event.
//!
//! Admission reserves two slots up front (`Start` and `Finish`) so a guard can
//! always publish its terminal event from `Drop` without blocking, awaiting or
//! spawning a background task.
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

/// Immutable snapshot of the routing decision, cloned while it is still valid.
#[derive(Clone, Debug)]
pub struct RequestMeta {
    pub request_id: String,
    pub run_id: String,
    pub source: Source,
    pub provider_id: String,
    pub provider_name: String,
    pub route_id: String,
    pub route_name: String,
    pub requested_model: String,
    pub upstream_model: String,
    pub stream: bool,
    pub started_at_ms: i64,
    /// Unit prices copied from the published cache when the request was
    /// admitted. The writer stores it at `Start` and settles from it at
    /// `Finish`, so a later price edit or delete cannot change this request.
    pub price: crate::stats::store::PriceSnapshot,
}

/// Everything the writer needs to close a request row.
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
#[derive(Debug)]
pub struct RequestGuard {
    pub meta: RequestMeta,
    reservation: Reservation,
    started: Instant,
    upstream_at: Option<Instant>,
    ttfb_ms: Option<i64>,
    first_content_ms: Option<i64>,
    collector: Collector,
    http_status: Option<i64>,
    http_ok: Option<bool>,
    /// `Content-Length` of the upstream body when the upstream declared one.
    expected_bytes: Option<u64>,
    observed_bytes: u64,
    upstream_started: bool,
    finished: bool,
    shared: Arc<Counters>,
}

impl RequestGuard {
    /// Reserves queue capacity and publishes the `Start` event.
    ///
    /// Returns `None` when the queue is full; the caller counts one dropped
    /// request and continues serving the chat request unchanged.
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

    /// Observes forwarded bytes; the caller still yields the same buffer.
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

    fn elapsed_ms(&self) -> i64 {
        match self.upstream_at {
            Some(start) => start.elapsed().as_millis() as i64,
            None => self.started.elapsed().as_millis() as i64,
        }
    }

    /// True when the upstream body was fully handed to the client.
    ///
    /// HTTP servers may release a known-length body stream without polling it
    /// to `None`, so completion cannot rely on the stream alone.
    pub fn body_fully_forwarded(&self) -> bool {
        self.expected_bytes
            .is_some_and(|expected| self.observed_bytes >= expected)
    }

    /// Terminal classification shared by the stream and the guard's `Drop`.
    pub fn terminal(&mut self) -> (Outcome, Option<ErrorKind>) {
        if self.http_ok == Some(false) {
            return (Outcome::Failed, Some(ErrorKind::UpstreamHttp));
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
        let usage = self.collector.finish(fallback);
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
            // Every declared byte reached the client: the response completed
            // even though the stream was released before polling `None`.
            let (outcome, kind) = self.terminal();
            self.finish(outcome, kind);
        } else {
            self.finish(Outcome::Cancelled, Some(ErrorKind::ClientDrop));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::dto::UsageStatus;

    fn meta(id: &str) -> RequestMeta {
        RequestMeta {
            request_id: id.into(),
            run_id: "run-a".into(),
            source: Source::Relay,
            provider_id: "p1".into(),
            provider_name: "provider".into(),
            route_id: "r1".into(),
            route_name: "route".into(),
            requested_model: "model-x".into(),
            upstream_model: "model-x".into(),
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

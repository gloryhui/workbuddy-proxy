//! Request lifecycle events, the bounded hand-off queue and the per-request
//! guard that publishes at most one terminal event.
//!
//! Admission reserves two slots up front (`Start` and `Finish`). The remaining
//! slot lets `Drop` enqueue its terminal event without competing for capacity,
//! awaiting a database, or spawning a background task; the queue mutex is still
//! acquired synchronously.
//!
//! 上游是 `proxy::relay_to`、原生协议 `relay` 和连接测试；下游是单线程
//! `stats-writer`。请求任务在 `proxy-io` Tokio runtime 上同步写入有界队列，
//! writer 再批量落库。`RequestGuard` 同时保存该请求的计时、协议观察状态和队列预约，
//! 因此它随响应 body stream 移动；客户端取消、网络 runtime 销毁都会触发其 `Drop`。
//!
//! 可选数值的 `None` 表示未知或未观察到；`Some(0)` 表示已经观察到一个合法的零值。
//! “代理已经把上游声明的全部字节交给 HTTP body 层”仍不等于客户端 socket 已确认收完。
//!
//! 统计是旁路观察：队列预约失败、writer 延迟或数据库失败都不会让聊天请求改走失败
//! 响应。这里能保证的是同一个活动 `RequestGuard` 至多把一条 `Finish` 放进内存队列；
//! 这条本地事件仍可能因为进程直接退出、writer 写库失败或关闭时排水预算耗尽而丢失，
//! 因而不能把该保证扩展成“数据库 exactly-once 持久化”。
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

/// 当前统计 run 的运行态计数器，通过原子量共享给健康接口和 writer。
///
/// `pending` 是已创建且尚未 finished 的 guard 数，队列深度另由 `EventQueue::depth`
/// 表示；`dropped` 是已通过上层 `accepting` 门禁、但 `RequestGuard::begin` 无法预约
/// 队列槽位而未记录准入的请求数；`write_failures` 是 writer 无法完成持久化所影响的
/// 计数。`commits` 按 writer 批次数增加，不是请求数或事务数。
/// 这些原子量让代理热路径可以直接更新健康数据，无需先获得 writer 的数据库锁。
#[derive(Debug, Default)]
pub struct Counters {
    /// 当前 run 成功获得统计 guard 的请求数。
    pub admitted: std::sync::atomic::AtomicU64,
    /// 已通过 `StatsHandle::begin` 的 accepting 门禁，但队列预约失败而跳过统计的
    /// 请求数；门禁本身直接返回 `None` 的路径不会在这里增加。
    pub dropped: std::sync::atomic::AtomicU64,
    /// writer 无法持久化时，按受影响范围累计的失败数；它不是 HTTP 请求失败数。
    pub write_failures: std::sync::atomic::AtomicU64,
    /// 已准入但尚未发布 `Finish` 的 guard 数；不是队列 item 数。
    pub pending: std::sync::atomic::AtomicI64,
    /// writer 已处理批次的单调计数，不等同于已持久化的请求数。
    pub commits: std::sync::atomic::AtomicU64,
    /// 最近一次正常 writer 循环中 `apply_batch` 返回 `Ok` 后的 Unix epoch 毫秒。
    /// 失败尝试和 shutdown 专用排水路径不会更新该值，因此不是所有写入尝试的时间。
    pub last_commit_at_ms: std::sync::atomic::AtomicI64,
    /// 进程退出已开始；它改变未完成 guard 的 Drop 判定，但不直接关闭事件队列。
    pub closing: AtomicBool,
    /// 统计功能总开关的共享快照。
    pub enabled: AtomicBool,
    /// 是否允许新请求预约统计槽位；关闭后既有预约仍可发布终态。
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

/// 准入时冻结的请求元数据，供 Start 和 Finish 使用同一份路由快照。
///
/// `Start` 事件持有它的克隆，`RequestGuard` 保留原值继续补充终态字段。因此后续配置
/// 修改不会改写已经准入请求的 provider、route、model 或价格语义。
#[derive(Clone, Debug)]
pub struct RequestMeta {
    /// 只有两个白名单身份头都严格合法时才存在；内部保存的是哈希而非原始头值。
    pub workbuddy_identity: Option<crate::stats::event::WorkbuddyIdentity>,
    /// 关联同一次请求 `Start`/`Finish` 的随机 ID，不等同于上游请求 ID。
    pub request_id: String,
    /// 当前统计 writer run 的标识，用于隔离不同进程运行。
    pub run_id: String,
    pub source: Source,
    pub provider_id: String,
    pub provider_name: String,
    pub route_id: String,
    pub route_name: String,
    pub requested_model: String,
    pub upstream_model: String,
    pub upstream_protocol: String,
    /// 客户端是否请求流式响应；反映请求契约，不代表 writer 使用流式写入。
    pub stream: bool,
    /// 请求被统计准入时的 Unix epoch 毫秒，使用墙钟表达可展示时间。
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
    /// 终态产生时的 Unix epoch 毫秒；数据库排序和展示使用墙钟时间。
    pub ended_at_ms: i64,
    /// guard 建立到终态的单调时钟时长，不受系统墙钟回拨影响。
    pub duration_ms: i64,
    /// 首次字节观察点的耗时；已标记上游派发时从该时刻起算，否则从 guard 创建起算。
    /// 透传 observe 即使收到空 chunk 也可设置此值，native 路径要求 bytes > 0。
    pub ttfb_ms: Option<i64>,
    /// 首个语义内容出现的耗时，起点同 TTFB；与仅收到协议控制帧不同。
    pub first_content_ms: Option<i64>,
    /// 是否在执行上游请求前调用过 `mark_upstream_started`；本地拒绝通常为 false。
    pub upstream_started: bool,
    /// 已过滤控制字符并截断到 160 个字符的响应模型；未知时为 `None`。
    pub response_model: Option<String>,
    /// 上游 HTTP 状态；尚未取得响应时为 `None`，不能与任意确认的 0 混淆。
    pub http_status: Option<i64>,
    pub outcome: Outcome,
    /// `outcome` 的补充原因；没有更具体原因时可为 `None`。
    pub error_kind: Option<ErrorKind>,
    pub usage_status: UsageStatus,
    pub usage_reason: Option<UsageReason>,
    pub usage_source: UsageSource,
    /// token 字段中的 `None` 均表示未知或未解析；确认是零时才是 `Some(0)`。
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub input_uncached_tokens: Option<i64>,
    pub input_breakdown: InputBreakdown,
    /// guard 构建 Finish 时的 Unix epoch 毫秒快照。`finish_request` 落库时使用 writer
    /// 自己取得的当前时间，本字段不是数据库更新的权威时间来源。
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug)]
/// 一条已准入请求的生命周期消息。
///
/// writer 通常先收到携带冻结 `RequestMeta` 的 `Start`，再收到用同一 `request_id`
/// 关闭记录的 `Finish`。两者被拆分入队是为了让 admission 和终态都只需等待内存队列，
/// 不等待数据库；writer 侧负责把两条事件合成为请求记录，因此不能只根据内存中看到
/// `Start` 就断言已经落库。
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
    /// 已经发布、等待 writer 消费的 Start/Finish 事件，不包含仅预约的空位。
    items: VecDeque<StatsEvent>,
    /// 已承诺给活动 guard、但尚未通过 `Reservation::push` 变成 item 的未来槽位。
    reserved: usize,
    /// 是否拒绝新的 `reserve`；关闭不清空已有 items，也不回收既有预约。
    closed: bool,
}

/// 有界多生产者/单消费者事件队列，容量按“item + 未来预约”共同计算。
///
/// 每个 admission 先预约两个空位：一个立即写 `Start`，另一个留给之后的
/// `Finish`。预约在准入阶段一次性计入容量，所以即使 writer 延迟处理，队列也能保证
/// 已准入的 guard 最后无需等待即可提交终态。队列满时 reservation 立即失败，聊天请求
/// 继续执行并把这次统计记为 dropped。
///
/// `Mutex<QueueState>` 只保护内存簿记，`Condvar` 唤醒单线程 writer。这里没有数据库
/// I/O，也不会为每个请求创建任务或线程。成功预约保证后续两次本地 push 不会再次因容量
/// 失败，但不保证共享内存永远保留到落库：进程终止或 writer 停排仍可丢失统计。
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

    /// 尝试预约 `slots` 个未来事件槽位，不等待 writer 或数据库。
    ///
    /// 成功返回的 `Reservation` 持有 queue 的 `Arc` 和尚未使用的槽位数。
    /// 这里是同步临界区，可能短暂等待其他调用者释放队列 `Mutex`，但不等待 writer，
    /// 也不创建数据库连接。失败只表示队列已关闭，或已排队事件与既有预约之和放不下
    /// `slots`；调用方据此跳过统计并继续代理。成功只承诺这些槽位，不承诺事件已经落库。
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

    /// 消费一个既有预约并把事件追加到 writer 可见队列。
    ///
    /// 输入必须来自已经计数的 reservation；函数把 `reserved` 减一并增加 `items`，
    /// 所以总量不增加。它不检查 `closed`：关闭之后，已经预约的 guard 仍要把自己的
    /// `Finish` 发布出来。该函数只做内存操作，随后通知 writer。
    fn push_raw(&self, event: StatsEvent) {
        let mut state = self.state.lock().unwrap();
        // The reservation is converted into a queued item.
        state.reserved = state.reserved.saturating_sub(1);
        state.items.push_back(event);
        drop(state);
        self.ready.notify_one();
    }

    /// 释放不再使用的预约，不发布事件。
    ///
    /// `Reservation` 被提前丢弃或正常事件少于原预约数时调用。归还容量后唤醒 writer，
    /// 让它在没有待消费 item 的情况下重新检查关闭状态和等待超时。
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

    /// 不发布事件，仅唤醒 writer（供控制流量和关闭通知使用）。
    pub fn wake(&self) {
        self.ready.notify_all();
    }

    /// 关闭新统计预约并唤醒 writer。
    ///
    /// `items` 和 `reserved` 都保留：已发布事件可由 writer 排水，既有 guard 也仍能
    /// 消费自己的 Finish 预约。关闭后的新请求会在 `reserve` 处失败。关闭本身不等待
    /// writer、不执行 flush，也不证明既有事件最终已经持久化。
    pub fn close(&self) {
        let mut state = self.state.lock().unwrap();
        state.closed = true;
        drop(state);
        self.ready.notify_all();
    }

    /// 由 writer 判断是否已收到关闭信号；不代表所有已有事件已经写完。
    pub fn is_closed(&self) -> bool {
        self.state.lock().unwrap().closed
    }

    /// 仅返回已发布 item 数；不包含 `reserved`，也不等同于活动请求数。
    pub fn depth(&self) -> usize {
        self.state.lock().unwrap().items.len()
    }

    #[allow(dead_code)] // Reported by `stats_health` through the shared constant.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

/// 活动 guard 持有的队列预约令牌。
///
/// `drop` 时释放仍未消费的预约。`RequestGuard` 正常提交 `Start` 后还剩一个槽位，
/// 这个槽位专门保证终态能在 `Drop` 中发布。
///
/// 该类型持有队列的 `Arc`，因此只要有 guard 存在，队列状态就能被访问；它不是数据库
/// 事务、事件 ID 或重试凭据。预约只解决“原有容量被其他请求抢走”的问题。
#[derive(Debug)]
pub struct Reservation {
    queue: Arc<EventQueue>,
    /// 尚未转换成一个 `StatsEvent` 的槽位数；正常生命周期从 2 开始，随后只减不增。
    slots: usize,
}

impl Reservation {
    /// 用一个既有预约发布一条事件。
    ///
    /// `RequestGuard::begin` 用第一个槽位发布 Start，`finish` 用第二个槽位发布 Finish。
    /// 槽位为 0 时函数静默返回，避免越过容量；正常双槽预约不应走到该分支。
    /// 发布只进入内存队列，不会等待或确认 SQLite 写入。
    pub fn push(&mut self, event: StatsEvent) {
        if self.slots == 0 {
            return;
        }
        self.slots -= 1;
        self.queue.push_raw(event);
    }
}

impl Drop for Reservation {
    /// 把没有用完的预约归还队列；消费掉的槽位已经转成 item，不会在此重复归还。
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
///
/// `RequestGuard` 不实现 `Clone`，其所有权随一次请求移动。透传响应把 guard 放进 body
/// `unfold` 状态，原生响应把它放进 `NativeStream`；只要 body 或 stream 状态被保留，
/// guard 就存活。显式 `finish` 与 `Drop` 共用 `finished` 标志，保证最多一条内存
/// Finish 事件，但该事件之后仍可能因 writer/关闭/进程退出而未落库。
#[derive(Debug)]
pub struct RequestGuard {
    /// 准入时冻结的请求元数据；Start 事件持有它的克隆。
    pub meta: RequestMeta,
    /// 已预留 Start/Finish 两个容量位置；Start 已消费一个，正常剩一个给 Finish。
    reservation: Reservation,
    /// guard 建立时的单调时钟，用作 duration 起点，也是未标记上游时的 elapsed 基准。
    started: Instant,
    /// 首次派发上游请求时的单调时刻；重复标记不会覆盖它。
    upstream_at: Option<Instant>,
    /// 首次字节观察点相对 `elapsed_ms` 起点的毫秒数；未观察为 `None`。
    ttfb_ms: Option<i64>,
    /// 首次语义内容相对 `elapsed_ms` 起点的毫秒数；未观察为 `None`。
    first_content_ms: Option<i64>,
    /// 透传协议的有界 body 解析器；encoded/unknown body 可能只做字节观察。
    collector: Collector,
    /// 原生协议的语义观察状态；存在后，terminal 以 complete 标志判定协议终态。
    native: Option<NativeObservation>,
    /// 上游 HTTP 状态；拿到响应头前为 `None`。
    http_status: Option<i64>,
    /// 上游 HTTP 是否 2xx；它与 status 同步设置，供 terminal 优先分类。
    http_ok: Option<bool>,
    /// `Content-Length` of the upstream body when the upstream declared one.
    ///
    /// `None` 表示头缺失或解析失败，不能假定 body 一定为空；`Some(0)` 才是上游明确
    /// 声明了零长度。
    expected_bytes: Option<u64>,
    /// 已由代理观察到、随后准备交给 response body 层的累计字节；使用饱和加法避免
    /// 回绕，且不代表客户端 socket 已确认收到。
    observed_bytes: u64,
    /// 是否已经记录过上游派发时刻；供 usage 规则区分本地拒绝与真实上游调用。
    upstream_started: bool,
    /// 是否已经把唯一 Finish 放入队列；主动 finish 和 Drop 都检查它。
    finished: bool,
    /// 与健康接口共享的原子计数器；finish 时负责把 pending 减一。
    shared: Arc<Counters>,
}
#[derive(Debug)]
struct NativeObservation {
    /// 原生 adapter 规范化后的累计 usage 快照；后一次观察替换前一次，不做相加。
    usage: UsageOutcome,
    /// adapter 终态与干净 transport EOF 是否都已完成。
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
    ///
    /// 具体步骤是：先同步尝试向队列预约两个槽位；再用第一个槽位入队 `Start`；随后把
    /// guard 的活动数加一，并建立单调时钟和透传 collector。FINISH 槽位没有在此立即
    /// 发布，而是随返回的 guard 移动。`meta.started_at_ms` 是调用方先前取得的墙钟
    /// 时间，`started: Instant` 则在本函数成功构造 guard 时取得，两者用途不同。
    /// `admitted` 由上层 `StatsHandle::begin` 在拿到 `Some(guard)` 后增加。
    pub fn begin(
        queue: &Arc<EventQueue>,
        counters: &Arc<Counters>,
        meta: RequestMeta,
        protocol: Protocol,
        encoded: bool,
    ) -> Option<Self> {
        let mut reservation = queue.reserve(2)?;
        // 第一个预约转成 Start；第二个继续留在 reservation 中，作为之后主动 finish
        // 或 Drop 兜底的本地入队容量。
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

    /// 记录即将派发上游调用的单调时刻。
    ///
    /// 调用来源通常是连接建立前的请求分支；重复调用不会覆盖第一次时间。TTFB 和
    /// first-content 的 elapsed 基准在此后从 guard 建立时刻切换到 `upstream_at`。
    /// 该函数不发送网络请求，也不设置 HTTP 状态。
    pub fn mark_upstream_started(&mut self) {
        if self.upstream_at.is_none() {
            self.upstream_at = Some(Instant::now());
            self.upstream_started = true;
        }
    }

    /// 记录已收到的上游 HTTP 状态码，并缓存是否为 2xx。
    ///
    /// 状态码采用无符号 `u16` 转为数据库使用的 `i64`。这里只记录事实，不发布终态；
    /// caller 仍可在 HTTP 错误、body 错误或正常 EOF 时决定何时调用 `finish`。
    pub fn set_http_status(&mut self, status: u16) {
        self.http_status = Some(i64::from(status));
        self.http_ok = Some((200..300).contains(&status));
    }

    /// 记录上游 `Content-Length` 的解析结果。
    ///
    /// 头缺失或解析失败时为 `None`；上游明确声明长度 0 时为 `Some(0)`。该值只用于
    /// 判断代理是否已观察完整声明长度，不能证明客户端 socket 已收完。
    pub fn set_content_length(&mut self, length: Option<u64>) {
        self.expected_bytes = length;
    }

    /// 在拿到真实响应头后，用 Content-Type/Content-Encoding 重建透传 collector。
    ///
    /// 新建 collector 会丢弃构造前尚未解析的状态，所以调用方应在转发首个 body chunk
    /// 前 attach。encoded body 会被 collector 降级为 Unknown；原始响应字节不受影响。
    pub fn attach_collector(&mut self, protocol: Protocol, encoded: bool) {
        self.collector = Collector::new(protocol, encoded);
    }

    /// Observes bytes that are about to be yielded toward the HTTP body layer.
    ///
    /// 本函数先累计字节、TTFB 和首个内容时间，再把同一 slice 交给 collector 做有界解析；
    /// 不接管或替换响应 buffer；collector 可能保留有界解析副本。观察结果不能证明
    /// 客户端 socket 已经收到这些字节。
    ///
    /// 这是普通 Chat/透传路径：原始 chunk 仍由调用方原样 yield。`ttfb_ms` 只看第一个
    /// 被观察到的 chunk，不要求它已经是业务内容；`first_content_ms` 由 collector
    /// 发现首个非空语义内容后设置。所有时延都从单调时钟计算，缺失保持 `None`。
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
    ///
    /// 调用来源是 `proxy::native::observe`：native decoder 已经把 OpenAI Responses、
    /// Anthropic 或 Gemini 事件转成统一语义，因此这里不再次喂原始 SSE/JSON 给
    /// `Collector`。`bytes` 累计已经观察的原始 chunk 长度；`content` 表示本次是否出现
    /// 可见内容；`usage` 是该协议当前累计快照，存在新值时替换旧值。第一次调用即使参数
    /// 为零也会建立 `NativeObservation { complete: false }`。仅 `bytes > 0` 时可能
    /// 设置 TTFB，`content == true` 时可能设置首内容时间；只有后续 `native_complete`
    /// 才能把它判为成功。`model` 先保存为响应模型候选，构建 Finish 时再过滤控制字符
    /// 并截断；缺失时保持 `None`。
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

    /// 在 native adapter 终态与干净 transport EOF 都完成后标记语义完成。
    ///
    /// 本函数只修改已有 `NativeObservation`；若从未调用过 `observe_native` 则不创建
    /// 状态。它不是通用成功开关：调用方必须先确认协议 finish 和终止边界均成功。
    pub fn native_complete(&mut self) {
        if let Some(state) = self.native.as_mut() {
            state.complete = true;
        }
    }

    /// 按单调时钟计算自相关起点以来的毫秒数。
    ///
    /// 已标记上游派发时从 `upstream_at` 起算，否则从 guard 建立时的 `started` 起算。
    /// 该数值用于 TTFB 和 first-content；请求总 duration 始终从 `started` 计算。
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
    ///
    /// 返回 true 的前提是 `expected_bytes` 为 `Some(expected)` 且累计观察量达到
    /// expected；`observed_bytes` 使用饱和加法，所以不会因整数回绕。没有
    /// Content-Length 时即使流已正常结束也返回 false，需要正常 body 分支显式
    /// `finish`。该判断只到 HTTP body 层，不是客户端 ACK。
    pub fn body_fully_forwarded(&self) -> bool {
        self.expected_bytes
            .is_some_and(|expected| self.observed_bytes >= expected)
    }

    /// 根据当前已记录事实推导终态，不直接发布事件。
    ///
    /// 判定顺序固定为：
    ///
    /// 1. 已取得 HTTP 响应且不是 2xx，判为 `Failed/UpstreamHttp`。
    /// 2. 走过 native 观察路径时，adapter+EOF 已完成才是 `Completed`，否则是
    ///    `Interrupted/UpstreamStream`。
    /// 3. 普通 collector 看到显式 SSE 协议错误时判为 `Failed/SseError`。
    /// 4. 非流式 JSON 在 EOF 后结算；error envelope 或 malformed JSON 判为失败。
    /// 5. collector 判定协议完整则完成，否则是上游流中断。
    ///
    /// `settle_json` 可能在此改变 collector 的内部结算标志；本函数仍不设置
    /// `RequestGuard.finished`，也不入队。显式客户端取消通常由调用方直接传入
    /// `Outcome::Cancelled`，或由后续 `Drop` 分类。
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
        // 非流式 body 在 EOF 后才对有界副本结算。HTTP 200 本身不够：error envelope
        // 或 malformed JSON 仍属于失败。
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

    /// 至多向内存队列发布一次 Finish，并冻结终态字段。
    ///
    /// 首次调用会冻结 usage、耗时和状态，使用 admission 留下的第二个配额发布
    /// `Finish`，并把 pending 减一。重复调用直接返回。这里只负责把事件放入内存队列；
    /// 数据库写入由 writer 异步完成，写入失败也不会反向改变已经发送的聊天响应。
    ///
    /// “exactly once”只描述本 guard 的本地 `finished` 标志与 reservation 槽位：主动
    /// finish 后 Drop 不再重复，重复 finish 也立即返回。它不表示事件已经写入 SQLite，
    /// 更不提供跨进程、跨重启的恰好一次语义。正常双槽预约下，第一次 finish 不会因队列
    /// 满而等待 writer；若 reservation 已被错误耗尽，`push` 会无事件返回，因此调用方
    /// 必须保持“一次两槽、Start 一次、Finish 一次”的内部约束。
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
        // duration 始终从 guard 建立算起；wall-clock 时间只用于数据库展示/排序。
        let duration_ms = self.started.elapsed().as_millis() as i64;
        let fallback = match outcome {
            Outcome::Cancelled => UsageReason::ClientDrop,
            Outcome::Interrupted => UsageReason::UncleanShutdown,
            Outcome::Failed | Outcome::Rejected => UsageReason::StreamError,
            _ => UsageReason::NoUsage,
        };
        // native 保存的是累计快照，直接取最新值；普通路径由 collector 在结束时结算。
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
        // 本地未发出上游请求时 usage 不适用；中断若没有完整用量则不能猜成零。
        let usage = adjust_usage(usage, outcome, self.upstream_started);
        let (status, source, numbers, breakdown, uncached) = decompose(&usage, outcome);
        FinishRecord {
            ended_at_ms: now_ms.max(self.meta.started_at_ms),
            duration_ms: duration_ms.max(0),
            ttfb_ms: self.ttfb_ms,
            first_content_ms: self.first_content_ms,
            upstream_started: self.upstream_started,
            // response_model 进入数据库前过滤控制字符并最多保留 160 个字符。
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

    /// 标记整个统计 run 正在关闭，供尚未 finished 的 guard 在 Drop 时识别。
    ///
    /// 本函数只写共享原子量，不关闭队列也不等待 writer。`main::run` 在网络 runtime
    /// 销毁前调用它，使 teardown 释放的慢流记录为 `Interrupted/Shutdown`，避免误报
    /// 客户端取消。已经成功 finish 的 Completed 记录不会被后续 closing 改写。
    pub fn mark_closing(counters: &Counters) {
        counters.closing.store(true, Ordering::Relaxed);
    }
}

/// 在 parser 结果之上应用请求级 usage 规则。
///
/// 若请求根本没有发送到上游，即使局部状态中残留数值也应记录为
/// `NotApplicable`，不能把本地拒绝算成上游 token。若请求被中断且 usage 不完整，则
/// 缺失原因改为 `UncleanShutdown`；已经完整的累计 usage 即使客户端随后取消也保留，
/// 因为那是已经观察到的上游事实。
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

/// 把统一 usage 结果拆成数据库列，并约束哪些状态可以携带数值。
///
/// `Complete` 必须有可信的 input/output；total 缺失时由两者相加得到。`Partial` 只保留
/// 已观察的分量而不伪造 total；`Invalid` 只记录状态，不把互相矛盾的数值写入列。
/// `Missing` 与 `NotApplicable` 都保持所有数值为 `None`，与确认值 `Some(0)` 区分。
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
    /// body 未被主动 finish 时的兜底终态路径。
    ///
    /// 已经 finished 的 guard 什么都不做。否则 `closing` 优先：只要尚未完成就记为
    /// `Interrupted/Shutdown`，即使此时 body 已转发完但尚未显式 finish。随后才检查
    /// `body_fully_forwarded()`；只有存在 Content-Length 且已观察完全部声明字节时才
    /// 调用 `terminal()` 按协议事实分类。没有足够长度证据时，Drop 不会仅凭协议终止
    /// 标记推断 Completed，而会记为 `Cancelled/ClientDrop`；正常 EOF 分支应在此之前
    /// 显式调用 `finish`。该兜底仍使用预留的第二槽位，并同样只保证本地至多一条
    /// Finish，不保证 writer 已将其写入数据库。
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        if self.shared.closing.load(Ordering::Relaxed) {
            self.finish(Outcome::Interrupted, Some(ErrorKind::Shutdown));
        } else if self.body_fully_forwarded() {
            // 已观察完全部声明长度时，即使 stream 在再次 poll 到 None 前被释放，
            // 也可以按协议痕迹分类。这仍不是客户端 socket ACK。
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

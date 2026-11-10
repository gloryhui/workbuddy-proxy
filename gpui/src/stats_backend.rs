//! Dedicated statistics transport for the settings window.
//!
//! The shared [`crate::backend::Backend`] reloads the provider configuration and
//! requests a snapshot for every job, which is wrong for read-only statistics.
//! This backend keeps its own bounded queue, its own short timeout and never
//! touches the configuration, the snapshot or the proxy status.
//!
//! 上游是 `statistics::Statistics` 的筛选、翻页和设置操作；下游是本机 Core
//! 的同一个 `/_control/action` 管理接口。`StatsScheduler::new` 启动
//! `statistics-control` 标准线程，而 `Statistics` 的 GPUI Entity 仍只在 UI
//! 上下文修改：后台线程把结果写入 `StatsReply` channel，UI tick 读取后先
//! 调用 `complete` 释放资源，再由控制器判断是否为当前结果。
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, SyncSender},
    time::Duration,
};

/// Query budget for statistics calls (the shared control default is 100s).
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(3);
/// Longest accepted control reply, matching the core's own limit.
pub const MAX_REPLY_BYTES: u64 = 1024 * 1024;

/// What a statistics job is for.
///
/// The kind drives scheduler policy: read intents are deduplicated by their
/// more specific `JobKey` (a stale dashboard or page is worthless), while write
/// intents must never be dropped silently because the user asked for a state
/// change.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum JobKind {
    Dashboard,
    Rows,
    Recent,
    Groups,
    Filters,
    Detail,
    Health,
    Prices,
    Settings,
    Maintenance,
    ClearPreview,
    PriceSave,
    PriceDelete,
    PriceSync,
    SettingsUpdate,
    ProviderMode,
    ClearStart,
    /// An import read: status, pending list or a preview.
    WorkbuddyRead,
    WorkbuddyWrite,
}

impl JobKind {
    /// Read-only intents may be replaced by a newer one.
    pub fn is_read(self) -> bool {
        matches!(
            self,
            JobKind::Dashboard
                | JobKind::Rows
                | JobKind::Recent
                | JobKind::Groups
                | JobKind::Filters
                | JobKind::Detail
                | JobKind::Health
                | JobKind::Prices
                | JobKind::Settings
                | JobKind::Maintenance
                | JobKind::WorkbuddyRead
        )
    }
}

/// Identity of one read intent.
///
/// Merging by `JobKind` alone made two different filter requests overwrite each
/// other (provider filters were dropped as soon as model filters were
/// submitted). The key is scoped to the resource, so only a refresh of *the
/// same* resource is collapsed.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum JobKey {
    Dashboard,
    Rows,
    Recent,
    Groups,
    Detail(String),
    Health,
    Prices,
    Settings,
    Maintenance,
    ClearPreview,
    /// Usage mode of one provider.
    ProviderMode(String),
    /// The provider option list.
    ProviderFilters,
    /// The model option list, scoped to the provider and search it was built for.
    ModelFilters {
        provider_id: String,
        query: String,
        cursor: String,
    },
    /// An import read, deduplicated by the resource it reads. Two requests for
    /// the same resource collapse; two different ones never do.
    WorkbuddyRead {
        resource: String,
    },
}

impl JobKey {
    /// The key for one import read.
    ///
    /// Reads are keyed by resource, so refreshing the status twice in a row
    /// collapses into one request while a pending page and a preview never
    /// overwrite each other.
    pub fn workbuddy_read(resource: &str) -> Self {
        JobKey::WorkbuddyRead {
            resource: resource.to_owned(),
        }
    }

    /// Coarse kind used for reply dispatch.
    pub fn kind(&self) -> JobKind {
        match self {
            JobKey::Dashboard => JobKind::Dashboard,
            JobKey::Rows => JobKind::Rows,
            JobKey::Recent => JobKind::Recent,
            JobKey::Groups => JobKind::Groups,
            JobKey::Detail(_) => JobKind::Detail,
            JobKey::Health => JobKind::Health,
            JobKey::Prices => JobKind::Prices,
            JobKey::Settings => JobKind::Settings,
            JobKey::Maintenance => JobKind::Maintenance,
            JobKey::ClearPreview => JobKind::ClearPreview,
            JobKey::ProviderMode(_) => JobKind::ProviderMode,
            JobKey::ProviderFilters | JobKey::ModelFilters { .. } => JobKind::Filters,
            JobKey::WorkbuddyRead { .. } => JobKind::WorkbuddyRead,
        }
    }
}

pub struct StatsJob {
    /// scheduler 单调递增序号，Reply 原样带回；它是去旧的第二道身份，页面
    /// 还会再校验筛选 generation、tab 和资源。
    pub seq: u64,
    /// 读资源的身份。相同 `JobKey` 的读 intent 可以合并；`in_flight` 也按它跟踪。
    pub key: JobKey,
    /// 从 `key` 得到的粗粒度种类，决定读/写策略和 UI 分派。
    pub kind: JobKind,
    /// Core `request_tag` 及 UI 回复标签；价格、导入等还靠它区分同一 kind。
    pub tag: String,
    /// 已构造的管理命令；worker 不再重新读取 UI 状态。
    pub request: Value,
}

pub struct StatsReply {
    /// 与提交给 worker 的 `StatsJob.seq` 相同。
    pub seq: u64,
    /// 用于在完成 Reply 时释放同一资源的 `in_flight` 计数。
    pub key: JobKey,
    pub kind: JobKind,
    pub tag: String,
    /// `call/parse_message` 已把管理 envelope 解开为统计 JSON；格式错误保留
    /// 为 Err，不能伪装成空列表或零值。
    pub result: Result<Value, String>,
}

/// Newest read intent per `JobKey` plus a bounded FIFO of write intents.
#[derive(Default)]
struct Pending {
    /// 每个资源最多保留一个尚未发出的最新读请求；旧请求即使被删除，其已发出
    /// Reply 也因 seq/identity 不匹配而被 UI 忽略。
    reads: std::collections::HashMap<JobKey, StatsJob>,
    /// 写操作必须保序且不能被新请求覆盖，因此使用有界 FIFO。
    writes: std::collections::VecDeque<StatsJob>,
    /// 每个资源尚有多少任务在 worker 中，防止同资源并发覆盖。
    in_flight: std::collections::HashMap<JobKey, usize>,
    /// scheduler 内部分配 seq 的计数器；默认从 0 开始，提交时先加一再使用。
    next_seq: u64,
}

/// Bounded write backlog; beyond this the caller is told instead of losing it.
pub const MAX_WRITE_BACKLOG: usize = 32;

/// Statistics job scheduler.
///
/// Read intents collapse to the newest one per resource `JobKey`, write intents
/// are queued and retried until the worker takes them, and `pump` moves work
/// over only when the channel has room. Every reply carries its `seq`, so a
/// late answer can never be mistaken for the current one.
pub struct StatsScheduler {
    /// UI 向 worker 投递任务的有界同步 channel，容量 8。
    jobs: SyncSender<StatsJob>,
    /// worker 向 UI 投递回执的有界 channel，容量 64。
    replies: Receiver<StatsReply>,
    /// UI 线程与 tick/pump 共享的待发队列；每次访问只持有短暂互斥锁，
    /// 锁本身不把任何后台 I/O 包进 UI 线程。
    pending: std::sync::Mutex<Pending>,
    /// 新任务和完成回复都会触发的单槽异步唤醒 channel。
    wake: async_channel::Sender<()>,
    wake_receiver: async_channel::Receiver<()>,
}

impl StatsScheduler {
    pub fn new(dir: PathBuf) -> Self {
        // 任务队列保持较短（8），避免后台积压大量已经过时的读请求；回执
        // 队列稍大（64）给 UI 在 250ms tick 之间排空结果。`wake` 容量 1，
        // 一批通知会合并为一个唤醒，不要求 UI 每 250ms 才能发现新结果。
        let (jobs, requests) = mpsc::sync_channel::<StatsJob>(8);
        let (results, replies) = mpsc::sync_channel(64);
        let (wake, wake_receiver) = async_channel::bounded(1);
        let reply_wake = wake.clone();
        std::thread::Builder::new()
            .name("statistics-control".into())
            .stack_size(512 * 1024)
            .spawn(move || {
                // 该客户端线程逐条调用 Core，因此同一次统计查询不会阻塞
                // GPUI 渲染线程。请求仍先由 Core 的网络 runtime 接收并完成
                // 鉴权/解析，再交给 Core 的统计处理；这里不能据此认为管理
                // HTTP 请求拥有与普通请求隔离的 Core 线程。
                while let Ok(job) = requests.recv() {
                    let result = call(&dir, &job.request);
                    if results
                        .send(StatsReply {
                            seq: job.seq,
                            kind: job.kind,
                            key: job.key,
                            tag: job.tag,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                    let _ = reply_wake.try_send(());
                }
            })
            .expect("Cannot start statistics worker");
        Self {
            jobs,
            replies,
            pending: std::sync::Mutex::new(Pending::default()),
            wake,
            wake_receiver,
        }
    }

    /// Wake the window on new work and completed replies instead of waiting
    /// for its periodic maintenance timer. Bursts collapse to a single wake.
    pub fn wake_receiver(&self) -> async_channel::Receiver<()> {
        // `clone` 共享同一个异步 channel 的接收端能力；它不复制任何统计
        // Entity 或后台结果。
        self.wake_receiver.clone()
    }

    /// Queues a read intent, replacing any older intent for the *same resource*.
    pub fn submit_read(&self, key: JobKey, tag: String, request: Value) -> u64 {
        // 先分配新的 seq 再覆盖同 key 的 intent。即便旧任务已在 worker 中，
        // UI 之后收到旧 seq 时也会拒绝；`in_flight` 会阻止同 key 的新任务
        // 同时发出，直到旧 Reply 被 `complete`。
        let mut pending = self.pending.lock().unwrap();
        pending.next_seq += 1;
        let seq = pending.next_seq;
        let kind = key.kind();
        pending.reads.insert(
            key.clone(),
            StatsJob {
                seq,
                key,
                kind,
                tag,
                request,
            },
        );
        let _ = self.wake.try_send(());
        seq
    }

    /// Queues a write intent. Returns `Err` only when the bounded backlog is
    /// full, so the caller can report it instead of losing a save or a clear.
    pub fn submit_write(&self, kind: JobKind, tag: String, request: Value) -> Result<u64, String> {
        // 写操作不做“latest wins”。队列达到 32 时明确返回错误，让价格、
        // 设置或清理操作的用户知道没有提交成功。
        let mut pending = self.pending.lock().unwrap();
        if pending.writes.len() >= MAX_WRITE_BACKLOG {
            return Err("statistics are busy; try again".to_owned());
        }
        pending.next_seq += 1;
        let seq = pending.next_seq;
        pending.writes.push_back(StatsJob {
            seq,
            key: JobKey::Dashboard, // unused for writes; replies carry `kind`
            kind,
            tag,
            request,
        });
        let _ = self.wake.try_send(());
        Ok(seq)
    }

    /// Moves as much pending work as the worker channel accepts. Call from the
    /// UI tick: a full channel leaves the work queued for the next tick instead
    /// of dropping it.
    #[cfg(test)]
    pub fn pump(&self) {
        self.pump_with_reads(true);
    }

    pub fn pump_with_reads(&self, allow_reads: bool) {
        // 每次循环只短暂锁定 pending，随后在不持锁时 `try_send`。写请求优先
        // 于读请求；读请求中 rows/recent/groups/detail 优先于后台资源。
        // `allow_reads=false` 时仍可泵出已授权的写操作。
        loop {
            let next = {
                let mut pending = self.pending.lock().unwrap();
                let job = if let Some(write) = pending.writes.pop_front() {
                    Some(write)
                } else if !allow_reads {
                    None
                } else if let Some(key) = pending
                    .reads
                    .keys()
                    .filter(|key| !pending.in_flight.contains_key(*key))
                    .min_by_key(|key| match key {
                        JobKey::Rows | JobKey::Recent | JobKey::Groups | JobKey::Detail(_) => 0,
                        _ => 1,
                    })
                    .cloned()
                {
                    pending.reads.remove(&key)
                } else {
                    None
                };
                if let Some(job) = &job {
                    *pending.in_flight.entry(job.key.clone()).or_insert(0) += 1;
                }
                job
            };
            let Some(job) = next else { return };
            let key = job.key.clone();
            let is_read = job.kind.is_read();
            if let Err(returned) = self.jobs.try_send(job) {
                // Channel full: put the intent back and retry on the next tick.
                // 放回时保留期间新提交的同 key 读 intent，避免把更新的筛选
                // 结果替换回旧 job。
                let std::sync::mpsc::TrySendError::Full(job) = returned else {
                    return;
                };
                let mut pending = self.pending.lock().unwrap();
                if let Some(count) = pending.in_flight.get_mut(&key) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        pending.in_flight.remove(&key);
                    }
                }
                if is_read {
                    pending.reads.entry(key).or_insert(job);
                } else {
                    pending.writes.push_front(job);
                }
                return;
            }
        }
    }

    /// Marks a reply as handled so its resource can be scheduled again.
    pub fn complete(&self, key: &JobKey) {
        // 这必须在 UI 处理 Reply 前/同一步调用：它释放 in-flight 名额，
        // 允许等待中的同资源最新 intent 随后发出。完成本身不保证展示结果，
        // 过期结果仍由 Statistics 的 identity/generation 门禁丢弃。
        let mut pending = self.pending.lock().unwrap();
        if let Some(count) = pending.in_flight.get_mut(key) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                pending.in_flight.remove(key);
            }
        }
        // A newer intent for the same resource could not leave while this
        // one was in flight. Dispatch it now instead of waiting for the timer.
        if !pending.reads.is_empty() || !pending.writes.is_empty() {
            let _ = self.wake.try_send(());
        }
    }

    /// Cancel only reads that have not left the scheduler. Authorized writes
    /// retain their order even while the statistics page is hidden.
    pub fn cancel_reads(&self) {
        // 只清掉尚未从 scheduler 发出的读 intent。已经进入 worker 的请求
        // 无法撤回；返回后具体由 `Statistics` 各资源的 visibility/tag/seq/
        // generation 门禁决定，本函数不保证所有隐藏页面的 Reply 都走同一
        // 分支。写请求不清空，否则用户可能失去已授权的价格、设置或清理操作。
        self.pending.lock().unwrap().reads.clear();
    }

    /// True while that exact resource is queued or running. Used to keep the
    /// maintenance poll and the provider-mode walk from re-queueing themselves.
    pub fn is_busy(&self, key: &JobKey) -> bool {
        let pending = self.pending.lock().unwrap();
        pending.in_flight.contains_key(key) || pending.reads.contains_key(key)
    }

    pub fn replies(&self) -> &Receiver<StatsReply> {
        &self.replies
    }

    /// True when a write intent is still waiting for a worker slot.
    #[allow(dead_code)] // Asserted by the scheduler tests; also useful for UI hints.
    pub fn has_pending_writes(&self) -> bool {
        !self.pending.lock().unwrap().writes.is_empty()
    }
}

/// Sends one statistics command through the local control API.
///
/// Redirects and proxies are disabled on purpose: the call must stay on the
/// loopback interface and must never be replayed by a proxy.
///
/// `directory/instance.json` 决定 loopback 端口和本次 Core 管理 key。所有
/// 统计查询通常为 3 秒超时；价格目录更新需要访问外部价格源，单独放宽到
/// 45 秒。响应体最多 1 MiB，禁用重定向和代理，防止管理请求被转发。
pub fn call(directory: &Path, request: &Value) -> Result<Value, String> {
    let info: Value = serde_json::from_slice(
        &std::fs::read(directory.join("instance.json")).map_err(|_| "Core is not responding")?,
    )
    .map_err(|_| "Invalid core response")?;
    let port = info["port"]
        .as_u64()
        .filter(|port| *port > 0 && *port <= 65535)
        .ok_or("Invalid core response")?;
    let key = info["management_key"]
        .as_str()
        .ok_or("Invalid core response")?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(if request["command"] == "stats_prices_update" {
            Duration::from_secs(45)
        } else {
            QUERY_TIMEOUT
        }))
        .build()
        .into();
    let mut response = agent
        .post(format!("http://127.0.0.1:{port}/_control/action"))
        .header("Authorization", format!("Bearer {key}"))
        .send_json(request)
        .map_err(|_| "Core is not responding")?;
    let success = response.status().is_success();
    let data: Value = response
        .body_mut()
        .with_config()
        .limit(MAX_REPLY_BYTES)
        .read_json()
        .map_err(|_| "Invalid core response")?;
    parse_message(success, &data)
}

/// Turns a control reply into its payload.
///
/// A malformed payload is an error, never an empty value: showing an empty page
/// would hide a protocol mismatch behind data that merely looks plausible.
///
/// 管理层先返回 `{ "message": "<JSON string>" }` 的 envelope；失败时也兼容
/// `error.message`。只有成功 envelope 才继续解析内层 JSON，畸形内容会显示
/// 为错误而不是伪造空数据。
pub fn parse_message(success: bool, data: &Value) -> Result<Value, String> {
    let message = data["message"]
        .as_str()
        .or_else(|| data["error"]["message"].as_str())
        .unwrap_or("Invalid core response")
        .to_owned();
    if !success {
        return Err(message);
    }
    serde_json::from_str(&message).map_err(|_| "Invalid statistics response".to_owned())
}

/// Builds a statistics command with the shared request tag.
pub fn command(name: &str, tag: &str, fields: Value) -> Value {
    // `fields` 必须能合并进 JSON 对象才生效。调用方传入的 page/filter 等字段
    // 会附加到统一 command 与 request_tag 上；此函数只构造值，不入队。
    let mut request = json!({"command": name, "request_tag": tag});
    if let Some(object) = fields.as_object()
        && let Some(target) = request.as_object_mut()
    {
        for (key, value) in object {
            target.insert(key.clone(), value.clone());
        }
    }
    request
}

#[cfg(test)]
impl StatsScheduler {
    /// Scheduler with no worker thread, so the queueing policy can be asserted
    /// directly. Returns the receiving end of the job channel.
    fn for_tests() -> (Self, Receiver<StatsJob>) {
        let (jobs, requests) = mpsc::sync_channel::<StatsJob>(2);
        let (_results, replies) = mpsc::sync_channel(8);
        let (wake, wake_receiver) = async_channel::bounded(1);
        (
            Self {
                jobs,
                replies,
                pending: std::sync::Mutex::new(Pending::default()),
                wake,
                wake_receiver,
            },
            requests,
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn completion_wakes_a_newer_intent_waiting_on_the_same_resource() {
        let (scheduler, jobs) = super::StatsScheduler::for_tests();
        let wake = scheduler.wake_receiver();
        let first = scheduler.submit_read(super::JobKey::Rows, "old".into(), serde_json::json!({}));
        wake.try_recv().unwrap();
        scheduler.pump();
        assert_eq!(jobs.try_recv().unwrap().seq, first);
        let newest =
            scheduler.submit_read(super::JobKey::Rows, "new".into(), serde_json::json!({}));
        wake.try_recv().unwrap();
        scheduler.pump();
        assert!(jobs.try_recv().is_err());
        scheduler.complete(&super::JobKey::Rows);
        wake.try_recv()
            .expect("completion must wake the queued replacement");
        scheduler.pump();
        assert_eq!(jobs.try_recv().unwrap().seq, newest);
    }

    #[test]
    fn interactive_pages_wake_immediately_and_precede_background_reads() {
        let (scheduler, jobs) = super::StatsScheduler::for_tests();
        let wake = scheduler.wake_receiver();
        scheduler.submit_read(
            super::JobKey::Dashboard,
            "dash".into(),
            serde_json::json!({}),
        );
        for _ in 0..100 {
            scheduler.submit_read(super::JobKey::Rows, "rows".into(), serde_json::json!({}));
        }
        assert!(wake.try_recv().is_ok());
        assert!(wake.try_recv().is_err(), "one wake is enough for a burst");
        scheduler.pump();
        assert_eq!(jobs.try_recv().unwrap().key, super::JobKey::Rows);
        assert_eq!(jobs.try_recv().unwrap().key, super::JobKey::Dashboard);
    }

    #[test]
    fn recent_reads_do_not_replace_the_paginated_request_intent() {
        let (scheduler, jobs) = super::StatsScheduler::for_tests();
        scheduler.submit_read(
            super::JobKey::Rows,
            "page-2".into(),
            serde_json::json!({"cursor":"page-two"}),
        );
        scheduler.submit_read(
            super::JobKey::Recent,
            "old-recent".into(),
            serde_json::json!({"page_size":10}),
        );
        scheduler.submit_read(
            super::JobKey::Recent,
            "new-recent".into(),
            serde_json::json!({"page_size":10}),
        );
        scheduler.pump();
        let mut tags = Vec::new();
        while let Ok(job) = jobs.try_recv() {
            tags.push(job.tag);
        }
        tags.sort();
        assert_eq!(tags, ["new-recent", "page-2"]);
        assert!(super::JobKind::Recent.is_read());
    }

    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Read intents collapse: a stale dashboard must never run after a newer
    /// one was requested (F10).
    #[test]
    fn leaving_cancels_reads_but_preserves_and_prioritizes_authorized_writes() {
        let (scheduler, jobs) = StatsScheduler::for_tests();
        scheduler.submit_read(JobKey::Rows, "rows".into(), json!({}));
        scheduler
            .submit_write(JobKind::PriceSave, "save".into(), json!({}))
            .unwrap();
        scheduler.pump_with_reads(false);
        assert_eq!(jobs.try_recv().unwrap().tag, "save");
        assert!(jobs.try_recv().is_err());
        scheduler.cancel_reads();
        scheduler.pump();
        assert!(jobs.try_recv().is_err());
    }

    #[test]
    fn read_intents_keep_only_the_newest_request() {
        let (scheduler, jobs) = StatsScheduler::for_tests();
        for index in 0..5 {
            scheduler.submit_read(
                JobKey::Dashboard,
                format!("dash-{index}"),
                json!({"index": index}),
            );
        }
        scheduler.submit_read(JobKey::Rows, "rows-0".into(), json!({}));
        scheduler.pump();
        let mut seen = Vec::new();
        while let Ok(job) = jobs.try_recv() {
            seen.push((job.kind, job.tag));
        }
        assert_eq!(seen.len(), 2, "exactly one intent per kind: {seen:?}");
        let mut seen = seen
            .into_iter()
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(
            seen.remove(&JobKind::Dashboard).as_deref(),
            Some("dash-4"),
            "only the newest dashboard may run"
        );
        assert_eq!(seen.remove(&JobKind::Rows).as_deref(), Some("rows-0"));
    }

    /// Write intents are never collapsed away, and the backlog is bounded so a
    /// caller learns about back pressure instead of losing a save.
    #[test]
    fn write_intents_queue_in_order_and_report_a_full_backlog() {
        let (scheduler, jobs) = StatsScheduler::for_tests();
        for index in 0..MAX_WRITE_BACKLOG {
            scheduler
                .submit_write(JobKind::PriceSave, format!("save-{index}"), json!({}))
                .expect("the backlog accepts up to its bound");
        }
        let failure = scheduler
            .submit_write(JobKind::PriceSave, "save-overflow".into(), json!({}))
            .expect_err("a full backlog must be reported");
        assert!(failure.contains("busy"), "{failure}");

        // Draining the channel lets `pump` hand over the queued writes in order.
        let mut order = Vec::new();
        for _ in 0..64 {
            scheduler.pump();
            while let Ok(job) = jobs.try_recv() {
                order.push(job.tag);
            }
        }
        assert_eq!(order.len(), MAX_WRITE_BACKLOG, "{order:?}");
        assert_eq!(order.first().map(String::as_str), Some("save-0"));
        assert_eq!(order.last().map(String::as_str), Some("save-31"));
    }

    /// A full worker channel must keep the work for the next tick instead of
    /// dropping it.
    #[test]
    fn a_full_channel_defers_work_to_the_next_tick() {
        let (scheduler, jobs) = StatsScheduler::for_tests();
        for index in 0..MAX_WRITE_BACKLOG {
            scheduler
                .submit_write(JobKind::ClearStart, format!("clear-{index}"), json!({}))
                .unwrap();
        }
        // The channel holds two jobs, so the first pump fills it and leaves the
        // rest queued for later ticks.
        scheduler.pump();
        let mut delivered = 0;
        while jobs.try_recv().is_ok() {
            delivered += 1;
        }
        assert_eq!(delivered, 2, "the channel capacity bounds one tick");
        assert!(
            scheduler.has_pending_writes(),
            "the remaining writes must stay queued, not be dropped"
        );
        // Draining the channel lets the next tick hand over more work.
        scheduler.pump();
        assert!(jobs.try_recv().is_ok());
    }

    /// `is_busy` keeps a poll from queueing itself twice.
    #[test]
    fn busy_tracking_deduplicates_polls() {
        let (scheduler, jobs) = StatsScheduler::for_tests();
        assert!(!scheduler.is_busy(&JobKey::Maintenance));
        scheduler.submit_read(JobKey::Maintenance, "m-0".into(), json!({}));
        // Queued counts as busy, so a second poll is not scheduled.
        assert!(scheduler.is_busy(&JobKey::Maintenance));
        scheduler.pump();
        assert!(scheduler.is_busy(&JobKey::Maintenance));
        let job = jobs.try_recv().unwrap();
        assert_eq!(job.kind, JobKind::Maintenance);
        scheduler.complete(&JobKey::Maintenance);
        assert!(!scheduler.is_busy(&JobKey::Maintenance));
    }

    /// R01 regression: two different filter resources must both be dispatched.
    /// Merging by `JobKind` alone dropped the provider list as soon as the model
    /// list was submitted, which blocked the provider selector and the price
    /// form entirely.
    #[test]
    fn different_filter_resources_are_not_collapsed_into_one_intent() {
        let (scheduler, jobs) = StatsScheduler::for_tests();
        scheduler.submit_read(
            JobKey::ProviderFilters,
            "filters-0".into(),
            json!({"kind": "provider"}),
        );
        scheduler.submit_read(
            JobKey::ModelFilters {
                provider_id: String::new(),
                query: String::new(),
                cursor: String::new(),
            },
            "models-0".into(),
            json!({"kind": "model"}),
        );
        scheduler.pump();
        let mut tags = Vec::new();
        while let Ok(job) = jobs.try_recv() {
            tags.push(job.tag);
        }
        assert!(
            tags.contains(&"filters-0".to_owned()) && tags.contains(&"models-0".to_owned()),
            "both resources must be dispatched: {tags:?}"
        );
    }

    /// The same resource may still collapse to its newest intent, and a model
    /// list scoped to another provider is a different resource.
    #[test]
    fn the_same_resource_collapses_but_other_scopes_survive() {
        let (scheduler, jobs) = StatsScheduler::for_tests();
        for index in 0..3 {
            scheduler.submit_read(
                JobKey::ProviderFilters,
                format!("filters-{index}"),
                json!({}),
            );
        }
        scheduler.submit_read(
            JobKey::ModelFilters {
                provider_id: "provider-a".into(),
                query: String::new(),
                cursor: String::new(),
            },
            "models-a".into(),
            json!({}),
        );
        scheduler.submit_read(
            JobKey::ModelFilters {
                provider_id: "provider-b".into(),
                query: String::new(),
                cursor: String::new(),
            },
            "models-b".into(),
            json!({}),
        );
        scheduler.submit_read(
            JobKey::ProviderMode("provider-a".into()),
            "mode-a".into(),
            json!({}),
        );
        scheduler.submit_read(
            JobKey::ProviderMode("provider-b".into()),
            "mode-b".into(),
            json!({}),
        );
        let mut tags = Vec::new();
        for _ in 0..8 {
            scheduler.pump();
            while let Ok(job) = jobs.try_recv() {
                tags.push(job.tag);
            }
        }
        assert!(tags.contains(&"filters-2".to_owned()), "{tags:?}");
        assert!(!tags.contains(&"filters-0".to_owned()), "{tags:?}");
        for expected in ["models-a", "models-b", "mode-a", "mode-b"] {
            assert!(
                tags.contains(&expected.to_owned()),
                "{expected} missing: {tags:?}"
            );
        }
    }

    /// A full channel must not lose a different resource, and writes must not be
    /// starved by an endless stream of refreshes.
    #[test]
    fn a_full_channel_keeps_every_resource_and_never_starves_writes() {
        let (scheduler, jobs) = StatsScheduler::for_tests();
        scheduler.submit_read(JobKey::ProviderFilters, "filters-0".into(), json!({}));
        scheduler.submit_read(
            JobKey::ModelFilters {
                provider_id: String::new(),
                query: String::new(),
                cursor: String::new(),
            },
            "models-0".into(),
            json!({}),
        );
        scheduler
            .submit_write(JobKind::PriceSave, "save-0".into(), json!({}))
            .unwrap();
        let mut tags = Vec::new();
        for _ in 0..8 {
            scheduler.pump();
            while let Ok(job) = jobs.try_recv() {
                tags.push(job.tag);
            }
        }
        for expected in ["filters-0", "models-0", "save-0"] {
            assert!(
                tags.contains(&expected.to_owned()),
                "{expected} missing: {tags:?}"
            );
        }
    }

    #[test]
    fn statistics_calls_use_the_management_key_and_unwrap_the_message() {
        let dir = tempfile::tempdir().unwrap();
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        std::fs::write(
            dir.path().join("instance.json"),
            json!({"port": port, "management_key": "fixture-management"}).to_string(),
        )
        .unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = socket.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 1024];
            loop {
                let size = stream.read(&mut buffer).unwrap();
                assert!(size > 0);
                bytes.extend_from_slice(&buffer[..size]);
                if let Some(end) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let length = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse::<usize>()
                        .unwrap();
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let request = String::from_utf8_lossy(&bytes).to_lowercase();
            assert!(
                request.contains("authorization: bearer fixture-management"),
                "headers were not authenticated: {request}"
            );
            assert!(
                request.contains("stats_health"),
                "request body was not received: {request}"
            );
            let body = r#"{"message":"{\"api_version\":1,\"state\":\"ready\"}"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        let value = call(dir.path(), &json!({"command": "stats_health"})).unwrap();
        assert_eq!(value["state"], "ready");
        worker.join().unwrap();
    }

    /// Reads one HTTP request completely, headers plus the declared body.
    fn read_full_request(stream: &mut std::net::TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 1024];
        loop {
            let length = match stream.read(&mut buffer) {
                Ok(0) | Err(_) => return String::from_utf8_lossy(&bytes).to_lowercase(),
                Ok(size) => size,
            };
            bytes.extend_from_slice(&buffer[..length]);
            let Some(head) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
                continue;
            };
            let length: usize = String::from_utf8_lossy(&bytes[..head])
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|value| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            if bytes.len() >= head + 4 + length {
                return String::from_utf8_lossy(&bytes).to_lowercase();
            }
        }
    }

    #[test]
    fn statistics_errors_keep_the_core_message() {
        let dir = tempfile::tempdir().unwrap();
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        std::fs::write(
            dir.path().join("instance.json"),
            json!({"port": port, "management_key": "fixture-management"}).to_string(),
        )
        .unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = socket.accept().unwrap();
            // Read the whole request before answering. A single `read` can stop
            // mid-body, and replying then closing the socket makes the client
            // see a broken connection instead of the core's message.
            let request = read_full_request(&mut stream);
            assert!(
                request.contains("stats_dashboard"),
                "request body was not received: {request}"
            );
            let body = r#"{"message":"stats_invalid_argument: bad filter"}"#;
            write!(
                stream,
                "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        let failure = call(dir.path(), &json!({"command": "stats_dashboard"})).unwrap_err();
        assert!(failure.starts_with("stats_invalid_argument"), "{failure}");
        worker.join().unwrap();
    }
}

#[cfg(test)]
mod reply_tests {
    use super::*;

    #[test]
    fn a_malformed_payload_is_an_error_and_never_an_empty_page() {
        // AC063: a reply that cannot be parsed must surface as an error. Showing
        // an empty page would hide a protocol mismatch behind plausible data.
        let envelope = serde_json::json!({
            "success": true,
            "message": "{ this is not json",
        });
        let error = parse_message(true, &envelope).unwrap_err();
        assert_eq!(error, "Invalid statistics response");
    }

    #[test]
    fn a_failed_envelope_keeps_the_core_message() {
        let envelope = serde_json::json!({
            "success": false,
            "error": { "message": "stats_unavailable: busy" },
        });
        let error = parse_message(false, &envelope).unwrap_err();
        assert!(error.contains("stats_unavailable"), "{error}");
    }

    #[test]
    fn a_well_formed_payload_still_parses() {
        let envelope = serde_json::json!({
            "success": true,
            "message": "{\"ok\":true}",
        });
        let value = parse_message(true, &envelope).unwrap();
        assert_eq!(value["ok"], true);
    }
}

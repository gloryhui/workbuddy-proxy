//! WorkBuddy 历史导入的生产运行骨架。
//!
//! 读与写被拆成两条明确的路径：
//!
//! * `workbuddy-import` 线程只扫描 WorkBuddy 源文件，并持有只读 SQLite 连接读取
//!   设置、checkpoint 和待补处理状态。它不直接打开写事务。
//! * 读出的 `Work` 被装入 `Envelope`，通过有界通道交给 Core 原有的统计 writer。
//!   writer 在自己的线程中串行执行 `apply_work`，事务提交成功后才回 ACK。
//!
//! 这接近 Spring Boot 中“Reader Service 产生命令，单一 Writer Service 处理命令”
//! 的分工，但它们不是两个事务管理器：数据库写连接和最终写事务都由 writer 独占，
//! import 线程只拥有文件句柄、只读连接和内存快照。当前代码把 `FileBatch` 移入
//! `Work` 后，在这条同步通道路径上不能再用原所有者读取同一批数据；这是编译期
//! 所有权约束，并不保证 ACK 回调一定送达、一定正确或在进程退出后仍可观察。
//!
//! 成功 ACK 表示导入事务已经提交；失败 ACK 表示这次调用没有得到成功返回，可能
//! 来自事务回滚，也可能来自写事务开始前的禁用、busy 或 epoch 检查。它不是“文件
//! 已解析”或“消息已排队”的通知，也不能在确认丢失时充当提交状态回执。writer 可能
//! 在取得报告/导入门禁前暂缓；reply 通道断开、进程在发送 ACK 前退出或调用方未
//! 观察到结果时，具体提交状态可能未知。

use super::{reader, store, types};
use crate::stats::{Inner, now_ms};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::{
    path::Path,
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

const SLICE_BYTES: u64 = 8 * 1024 * 1024;
const SLICE_TIME: Duration = Duration::from_millis(50);

/// 各 import 子系统共享的进程内协调状态。
///
/// `Shared` 不保存业务事实，也不是数据库缓存；它保存启停、唤醒、扫描进度、
/// 同步/观察租约和进程内遥测。`Arc<Shared>` 可跨线程共享，但每次读取或修改字段
/// 仍要遵守 atomic 或 `Mutex` 提供的同步边界。
#[derive(Default)]
pub(in crate::stats) struct Shared {
    #[cfg(test)]
    idle_snapshot_reads: AtomicU64,
    #[cfg(test)]
    test_batch_failure: Mutex<Option<&'static str>>,
    clock: OnceLock<metrics::Clock>,
    telemetry: Mutex<telemetry::Telemetry>,
    enabled: AtomicBool,
    legacy_sync_allowed: AtomicBool,
    catchup_backlog: AtomicBool,
    stopped: AtomicBool,
    wake: Condvar,
    wake_epoch: AtomicU64,
    progress: Mutex<Progress>,
    sync_in_flight: Mutex<Option<Arc<AtomicBool>>>,
    observation_requested: AtomicBool,
    observation_epoch: AtomicU64,
}

/// 供状态 API 和调度器读取的可变进度快照。
///
/// 这里的 `last_source_commit`、扫描任务和错误码都是运行时观察值。持久化的文件
/// 位置仍以 checkpoint 为准；重启后可以从数据库重建调度，而不依赖该结构是否
/// 曾经存在于内存。
#[derive(Default)]
struct Progress {
    last_source_commit: Option<metrics::SourceCommit>,
    recent_source_commits: std::collections::VecDeque<metrics::SourceCommit>,
    requested: bool,
    root_found: bool,
    last_incremental_at_ms: Option<i64>,
    last_error_code: Option<&'static str>,
    body_bytes_read: u64,
    discovering: bool,
    discovery_complete: bool,
    scan: Option<ScanTask>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanMode {
    Incremental,
    Full,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanState {
    Queued,
    Running,
    Completed,
    Failed,
}

/// 一次手动扫描请求在内存中的身份与状态。
///
/// 相同模式的排队/运行请求会合并并复用 `id`。已有 incremental 时再申请 full
/// 会返回 busy；已有 full 时申请 incremental 会复用当前 full 任务。请求本身不会
/// 改写已有任务的 mode，因此这里没有把 incremental 原地“升级”成 full 的动作。
struct ScanTask {
    id: String,
    mode: ScanMode,
    state: ScanState,
}

impl Shared {
    fn notify(&self) {
        self.wake_epoch.fetch_add(1, Ordering::AcqRel);
        self.wake.notify_all();
    }

    pub(in crate::stats) fn publish_import_policy(&self, requested: bool) {
        self.legacy_sync_allowed
            .store(!requested, Ordering::Release);
    }

    pub(in crate::stats) fn legacy_sync_allowed(&self) -> bool {
        self.legacy_sync_allowed.load(Ordering::Acquire)
    }

    pub(in crate::stats) fn register_sync(&self, lease: Arc<AtomicBool>) {
        *self.sync_in_flight.lock().unwrap() = Some(lease);
        self.request_observation();
    }

    pub(in crate::stats) fn request_observation(&self) {
        self.observation_epoch.fetch_add(1, Ordering::AcqRel);
        self.observation_requested.store(true, Ordering::Release);
        self.notify();
    }

    fn sync_active(&self) -> bool {
        let mut guard = self.sync_in_flight.lock().unwrap();
        if guard
            .as_ref()
            .is_some_and(|lease| lease.load(Ordering::Acquire))
        {
            return true;
        }
        if guard.take().is_some() {
            self.request_observation();
        }
        false
    }

    pub(in crate::stats) fn request_scan(&self, mode: &str) -> Result<String, String> {
        let mode = match mode {
            "incremental" => ScanMode::Incremental,
            "full" => ScanMode::Full,
            _ => return Err("stats_workbuddy_unsupported_format".into()),
        };
        if !self.active() {
            return Err("stats_workbuddy_import_disabled".into());
        }
        let mut progress = self.progress.lock().unwrap();
        let pending = progress
            .scan
            .as_ref()
            .filter(|scan| matches!(scan.state, ScanState::Queued | ScanState::Running));
        let id = if let Some(scan) = pending {
            if scan.mode != mode && scan.mode != ScanMode::Full {
                return Err("stats_workbuddy_scan_busy".into());
            }
            scan.id.clone()
        } else {
            let id = crate::config::token();
            progress.scan = Some(ScanTask {
                id: id.clone(),
                mode,
                state: ScanState::Queued,
            });
            progress.requested = true;
            id
        };
        if mode == ScanMode::Full {
            self.request_observation();
        }
        self.notify();
        Ok(serde_json::json!({"api_version":crate::stats::dto::API_VERSION,
            "scan_id":id,"state":"queued","mode":if mode==ScanMode::Full {"full"} else {"incremental"}}).to_string())
    }

    pub(in crate::stats) fn publish_status(&self, text: &str) -> Result<String, String> {
        let mut value: serde_json::Value =
            serde_json::from_str(text).map_err(|_| "stats_unavailable".to_owned())?;
        let progress = self.progress.lock().unwrap();
        value["root_found"] = progress.root_found.into();
        value["last_incremental_at_ms"] = progress.last_incremental_at_ms.into();
        if let Some(code) = progress.last_error_code {
            value["last_error_code"] = code.into();
        }
        value["source_body_bytes_read"] = progress.body_bytes_read.into();
        value["progress"]["total_known"] =
            (progress.discovery_complete && !progress.discovering && progress.root_found).into();
        metrics::publish(self, &progress, &mut value);
        value["runtime_metrics"] = telemetry::snapshot(self);
        if let Some(scan) = progress.scan.as_ref() {
            let state = match scan.state {
                ScanState::Queued => "queued",
                ScanState::Running => "running",
                ScanState::Completed => "completed",
                ScanState::Failed => "failed",
            };
            value["scan_id"] = scan.id.clone().into();
            value["scan_state"] = state.into();
            value["scan_mode"] = (if scan.mode == ScanMode::Full {
                "full"
            } else {
                "incremental"
            })
            .into();
            if value["effective_enabled"] == true
                && scan.state == ScanState::Running
                && scan.mode == ScanMode::Full
            {
                value["state"] = "scanning".into();
            }
        }
        if progress.discovering && value["effective_enabled"] == true {
            value["state"] = "discovering".into();
        }
        let task_active = progress
            .scan
            .as_ref()
            .is_some_and(|scan| matches!(scan.state, ScanState::Queued | ScanState::Running));
        if !value["last_error_code"].is_null() && value["effective_enabled"] == true {
            value["state"] = "degraded".into();
        }
        value["complete"] = (value["complete"] == true
            && progress.root_found
            && progress.last_error_code.is_none()
            && progress.last_incremental_at_ms.is_some()
            && !progress.discovering
            && !task_active)
            .into();
        Ok(value.to_string())
    }
    pub(in crate::stats) fn configure(&self, enabled: bool) {
        let previous = self.enabled.swap(enabled, Ordering::AcqRel);
        if enabled && !previous {
            self.progress.lock().unwrap().requested = true;
            self.request_observation();
        }
        self.notify();
    }

    fn error(&self, code: &'static str) {
        self.progress.lock().unwrap().last_error_code = Some(code);
    }

    fn active(&self) -> bool {
        self.enabled.load(Ordering::Acquire) && !self.stopped.load(Ordering::Acquire)
    }
}

/// 从一份源文件快照读出的一个待提交批次。
///
/// `settings_revision` 与 `mapping_revision` 是读取时的双版本快照；writer 启事务后
/// 必须重新读取当前值并逐项比较。`expected` 是读取前看到的 checkpoint，
/// `reset`/`offset`/`fingerprint` 描述本批从哪读到哪以及下一 checkpoint 应记录什么。
///
/// `namespace_id` 参与文件身份和候选归属隔离。`at_eof` 表示本片看到了当前文件
/// 末尾；普通末行只有看见换行才算完整记录，半行会留在下一轮重新读取。
struct FileBatch {
    namespace_id: String,
    settings_revision: i64,
    mapping_revision: i64,
    file_key: String,
    relative_path: String,
    expected: Option<store::Checkpoint>,
    reset: bool,
    offset: i64,
    observed_size: i64,
    modified_ms: i64,
    fingerprint: reader::FileFingerprint,
    discarding: bool,
    at_eof: bool,
    candidates: Vec<types::Candidate>,
    diagnostics: types::DiagnosticCounts,
    scan_offset: Option<i64>,
}

struct FullCursor {
    relative_path: String,
    offset: Option<i64>,
    discarding: bool,
    generation: i64,
}

struct DiscoveredFile {
    file_key: String,
    relative_path: String,
    size: i64,
    modified_ms: i64,
    expected_revision: Option<i64>,
    needs_read: bool,
}

struct DiscoveryBatch {
    namespace_id: String,
    settings_revision: i64,
    files: Vec<DiscoveredFile>,
}

/// import 读者提交给 writer 的有界命令。
///
/// 命令既包含源文件批次，也包含发现、补偿、模型观察、全量扫描控制、错误和
/// touch。它们全部走同一个 writer 通道，避免 import 为自己另开一条写 SQLite 的
/// 路径。Java 读者可把它理解为封闭的 command enum；Rust 的 match 要求每个变体
/// 都在写入分派处显式处理。
enum Work {
    Discover(DiscoveryBatch),
    CatchUp,
    Reconcile {
        mapping_id: String,
    },
    Observe {
        observations: Vec<super::classify::ConfigObservation>,
        epoch: u64,
        at_ms: i64,
        final_batch: bool,
    },
    Batch(Box<FileBatch>),
    Error {
        file_key: String,
        code: &'static str,
    },
    Touch {
        file_key: String,
    },
    BeginFull,
    CompleteFull,
}

/// 一个等待 writer 处理的命令及其结果通道。
///
/// 当前生产实现让 `reply` 容量为 1，worker 发出 Envelope 后等待该次结果；成功
/// 结果只在 `apply_work` 已提交后发送。所有权只保证这条代码路径不能同时复用
/// `Work`，并不保证回调一定送达或内容一定可观察。reply 断开表示调用方没有收到
/// 成功确认，不能当作成功；进程退出等情况下也不存在独立的持久化回调凭证。
struct Envelope {
    work: Work,
    reply: SyncSender<Result<(), &'static str>>,
}

/// 统计 writer 线程内持有的 import 端点。
///
/// `receiver` 对应该 writer 启动时创建的 Envelope 通道；`pending` 最多持有一个
/// 已收到但尚未通过 read/report 门禁的命令。`worker` 是独立源读取线程的 join
/// 句柄。写连接没有存放在这里，导入事务只在 `poll`/`apply_envelope` 中借用 writer
/// 自己的连接。
pub(in crate::stats) struct Runtime {
    shared: Arc<Shared>,
    receiver: Receiver<Envelope>,
    worker: Option<JoinHandle<()>>,
    pending: Option<Envelope>,
    import_yield: crate::stats::report_scheduling::ImportYield,
}

impl Runtime {
    pub(in crate::stats) fn start(inner: Arc<Inner>, root: reader::ReaderRoot) -> Self {
        // 启动一个命名为 `workbuddy-import` 的标准线程。它循环运行
        // `run_worker`；遇到可恢复错误时设置统一错误码并指数退避，停止标志则退出。
        // 通道有界，源线程与 writer 之间最多积压少量 Envelope，源文件内容本身不进入
        // 无界内存队列。
        let shared = inner.import_runtime.clone();
        let (sender, receiver) = mpsc::sync_channel(super::IMPORT_CHANNEL_CAPACITY);
        let worker_shared = shared.clone();
        let worker = std::thread::Builder::new()
            .name("workbuddy-import".into())
            .spawn(move || {
                let mut backoff = Duration::from_secs(1);
                while !worker_shared.stopped.load(Ordering::Acquire) {
                    match run_worker(&inner, &root, &sender, &worker_shared) {
                        Ok(()) => break,
                        Err(code) => {
                            worker_shared.error(code);
                            let guard = worker_shared.progress.lock().unwrap();
                            let _ = worker_shared.wake.wait_timeout(guard, backoff);
                            backoff = (backoff * 2).min(Duration::from_secs(30));
                        }
                    }
                }
            });
        let worker = match worker {
            Ok(worker) => Some(worker),
            Err(_) => {
                shared.error("stats_workbuddy_source_read_failed");
                None
            }
        };
        Self {
            shared,
            receiver,
            worker,
            pending: None,
            import_yield: crate::stats::report_scheduling::ImportYield::default(),
        }
    }

    /// 在 writer 一轮代理事件之后推进至多一个 import Envelope。
    ///
    /// 第一次调用先非阻塞收取一条消息并放在 `pending`；之后只有通过准入门禁才
    /// 执行它。取不到门禁时保留同一个 Envelope 并立即返回，既不丢弃，也不继续
    /// 从通道排更多消息。这样 writer 仍优先处理代理事件、控制命令和关闭请求。
    ///
    /// `pending` 在调用 `apply_envelope` 前被移出。无论写入成功还是失败，
    /// `apply_envelope` 都会向等待中的 worker 发送一次结果；发送后源线程才可能继续。
    pub(in crate::stats) fn poll(
        &mut self,
        writer: &mut crate::stats::store::Writer,
        inner: &Arc<Inner>,
    ) {
        if self.pending.is_none() {
            let Ok(envelope) = self.receiver.try_recv() else {
                return;
            };
            telemetry::received(&self.shared);
            // One in-flight message, still unacknowledged. Do not drain more
            // messages while report admission defers this transaction.
            self.pending = Some(envelope);
        }
        let Ok(_import_activity) = self.admit(writer, inner) else {
            return;
        };
        let envelope = self.pending.take().expect("pending import envelope");
        self.apply_envelope(writer, inner, envelope);
    }

    fn admit<'a>(
        &mut self,
        writer: &crate::stats::store::Writer,
        inner: &'a Inner,
    ) -> Result<Option<crate::stats::report_scheduling::ImportGuard<'a>>, ()> {
        // read/report 门禁只协调“报表读取”和“导入的一次有界写”，不是全局数据库锁。
        // 会改变事实的 Batch、确有 backlog 的 CatchUp 和 Reconcile 还要遵守
        // ImportYield 的节流，避免持续报表饿死导入。纯元数据命令跳过 400ms 节流，
        // 但仍要取得 ImportGuard，因为提交也可能触发 WAL checkpoint。
        //
        // 若当前 reader 正活跃或节流未放行，本函数返回 Err，`pending` 因而留在
        // 下轮重试。真正禁用时返回 Ok(None) 并撤销尚未接手的预约，随后 poll 会立即
        // 用 disabled ACK 结束当前 Envelope，不能让 worker 无限等待。
        if !self.shared.active() || !writer.settings.enabled {
            inner.report_reads.cancel_import_reservation();
            return Ok(None);
        }
        let changes_facts = match &self.pending.as_ref().ok_or(())?.work {
            Work::Batch(batch) => !batch.candidates.is_empty(),
            Work::CatchUp => writer
                .conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM workbuddy_import_ledger
                 WHERE state='pending_classification' AND candidate_json IS NOT NULL)",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .unwrap_or(true),
            Work::Reconcile { .. } => true,
            _ => false,
        };
        // Metadata skips the400ms pacing, but still hands off after the
        // active report: even a metadata commit can start a WAL checkpoint.
        if changes_facts
            && !inner.report_reads.import_reserved()
            && !self
                .import_yield
                .allows_import(inner.report_reads.is_active(), Instant::now())
        {
            return Err(());
        }
        inner.report_reads.try_enter_import().map(Some).ok_or(())
    }

    fn apply_envelope(
        &mut self,
        writer: &mut crate::stats::store::Writer,
        inner: &Inner,
        envelope: Envelope,
    ) {
        // 这里运行在统计 writer 线程。若 Observe 的 epoch 已过期或有 live sync，
        // 该命令以 busy 结束；其余命令交给 `apply_work` 在自己的事务中执行。
        //
        // `apply_work` 返回 Ok 时事务已经提交。Err 可能来自事务创建失败、
        // 已经开启的事务因提前返回而随栈展开回滚，或其他提交失败；不能把每个
        // Err 都描述成“已经完成回滚”。只有到这里才更新遥测并尝试发送结果；
        // 缺 ACK 只代表调用方没有观察到确认，不能反过来证明事务没有提交。
        let candidates = match &envelope.work {
            Work::Batch(batch) => Some(batch.candidates.len()),
            _ => None,
        };
        let source_commit = metrics::pending(&envelope.work);
        let reconcile_id = match &envelope.work {
            Work::Reconcile { mapping_id } => Some(mapping_id.clone()),
            _ => None,
        };
        let reconciliation = reconcile_id.is_some();
        let catchup = matches!(&envelope.work, Work::CatchUp);
        let before = Instant::now();
        let result = if !self.shared.active() || !writer.settings.enabled {
            Err("stats_workbuddy_import_disabled")
        } else if matches!(&envelope.work, Work::Observe { epoch, .. } if self.shared.sync_active() || *epoch != self.shared.observation_epoch.load(Ordering::Acquire))
        {
            Err("stats_workbuddy_scan_busy")
        } else {
            // The injection exists only in unit-test builds and traverses the
            // real writer ACK and reader retry path without changing SQLite.
            #[cfg(test)]
            let injected =
                candidates.and_then(|_| self.shared.test_batch_failure.lock().unwrap().take());
            #[cfg(not(test))]
            let injected: Option<&'static str> = None;
            match injected {
                Some(code) => Err(code),
                None => apply_work(&mut writer.conn, envelope.work),
            }
        };
        if let Some(candidates) = candidates {
            if let Err(code) = result.as_ref() {
                eprintln!("workbuddy import batch rejected: {code}");
            }
            telemetry::batch_finished(&self.shared, before.elapsed(), candidates, result.is_ok());
        }
        if let Err(code) = result {
            self.shared.error(code);
            if let Some(id) = reconcile_id {
                let _=writer.conn.execute("UPDATE workbuddy_model_history SET reconcile_error_code=?2 WHERE mapping_id=?1",params![id,code]);
            }
        }
        if let Ok(report) = result {
            if let Some(receipt) = source_commit {
                metrics::committed(&self.shared, receipt);
            }
            if catchup {
                self.shared
                    .catchup_backlog
                    .store(report.catchup_more, Ordering::Release);
            }
            // Append-only commits preserve pagination's frozen watermark.
            // Only a withdrawal invalidates a previously issued cursor.
            if report.revised {
                inner.maintenance_revision.fetch_add(1, Ordering::AcqRel);
            }
            inner.counters.commits.fetch_add(1, Ordering::Relaxed);
        }
        if (reconciliation || catchup) && result.is_ok() {
            let missing: bool = writer
                .conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM workbuddy_import_ledger
                WHERE state='pending_classification' AND candidate_json IS NULL)",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(false);
            if missing {
                let _ = self.shared.request_scan("full");
            }
        }
        let _ = envelope.reply.send(result.map(|_| ()));
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        // 关闭时先发布 stopped 并唤醒等待中的源线程，再 join。源线程会在批次之间
        // 和 ACK 等待中观察该标志，因此不会继续打开新的源快照。它不拥有 writer
        // 连接，也不会遗留自己的未提交事务；但正常关闭本身不保证把尚未开始读取的
        // 文件全部导入，恢复仍依赖 durable checkpoint 和下次启动后的扫描。
        self.shared.stopped.store(true, Ordering::Release);
        self.shared.notify();
        // The worker tests stopped during ACK waits and each source slice. No
        // writer connection or uncommitted checkpoint is owned by that thread.
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

mod metrics;
mod observer;
mod telemetry;
mod worker;
mod writer;
use worker::run_worker;
use writer::apply_work;
#[cfg(test)]
mod tests;

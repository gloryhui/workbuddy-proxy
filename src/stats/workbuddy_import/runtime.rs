//! Production import wiring. Only this worker reads source files; all durable
//! changes are submitted to the existing statistics writer and acknowledged
//! after commit. The worker's SQLite connection is strictly read-only.

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

#[derive(Default)]
pub(in crate::stats) struct Shared {
    #[cfg(test)]
    idle_snapshot_reads: AtomicU64,
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

struct Envelope {
    work: Work,
    reply: SyncSender<Result<(), &'static str>>,
}

pub(in crate::stats) struct Runtime {
    shared: Arc<Shared>,
    receiver: Receiver<Envelope>,
    worker: Option<JoinHandle<()>>,
    pending: Option<Envelope>,
    import_yield: crate::stats::report_scheduling::ImportYield,
}

impl Runtime {
    pub(in crate::stats) fn start(inner: Arc<Inner>, root: reader::ReaderRoot) -> Self {
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

    /// One import message after the proxy batch; never consume the proxy queue.
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
            apply_work(&mut writer.conn, envelope.work)
        };
        if let Some(candidates) = candidates {
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

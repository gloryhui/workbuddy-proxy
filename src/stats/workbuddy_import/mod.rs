//! WorkBuddy local JSONL usage import.
//!
//! Bounded JSONL parsing, durable attribution and reconciliation. The dedicated
//! runtime worker owns source reads; every mutation uses the existing single
//! statistics writer, including sync intents and manual attribution tasks.

pub mod classify;
pub(super) mod history;
pub(crate) mod manual;
pub mod normalize;
pub(crate) mod observation;
pub mod parser;
mod progress;
pub mod reader;
pub(super) mod reconcile;
pub(super) mod runtime;
pub mod store;
pub mod types;

#[cfg(test)]
mod tests;

use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};

/// Capacity of the dedicated import channel.
///
/// Two slots: one batch being handed over and one queued behind it. The channel
/// exists so the importer never shares, and never fills, the proxy's own event
/// queue.
pub const IMPORT_CHANNEL_CAPACITY: usize = 2;

/// How many batches may be in flight waiting for an ACK.
pub const MAX_PENDING_ACK: usize = 1;

/// What happened when a message was offered to the import channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfferOutcome {
    /// The message is queued.
    Accepted,
    /// The channel is full: the caller must yield, never wait.
    Busy,
    /// The worker is gone.
    Closed,
}

/// One unit of work handed to the import worker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportMessage {
    /// Run one scheduling slice.
    Tick { at_ms: i64 },
    /// Stop the worker.
    Shutdown,
}

/// The dedicated, bounded hand-off into the import worker.
///
/// `offer` is a **non-blocking** send on purpose: the forwarding path must never
/// wait for the importer. When the queue is full the *worker* is the one that
/// yields — the proxy's begin/finish keep running untouched.
pub struct ImportChannel {
    sender: SyncSender<ImportMessage>,
    receiver: Option<Receiver<ImportMessage>>,
}

impl ImportChannel {
    /// Creates the bounded channel.
    pub fn bounded() -> Self {
        let (sender, receiver) = sync_channel(IMPORT_CHANNEL_CAPACITY);
        Self {
            sender,
            receiver: Some(receiver),
        }
    }

    /// Offers a message without ever blocking.
    pub fn offer(&self, message: ImportMessage) -> OfferOutcome {
        match self.sender.try_send(message) {
            Ok(()) => OfferOutcome::Accepted,
            Err(TrySendError::Full(_)) => OfferOutcome::Busy,
            Err(TrySendError::Disconnected(_)) => OfferOutcome::Closed,
        }
    }

    /// A sender for the forwarding path.
    pub fn sender(&self) -> SyncSender<ImportMessage> {
        self.sender.clone()
    }

    /// Takes the receiving end, once.
    pub fn take_receiver(&mut self) -> Option<Receiver<ImportMessage>> {
        self.receiver.take()
    }
}

/// The outcome of an ACK-window operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AckOutcome {
    /// The window was free and now holds this batch.
    Opened,
    /// Another batch is already waiting for its ACK.
    Busy,
    /// The ACK does not match the batch that is waiting.
    Mismatch,
    /// Nothing was waiting.
    Idle,
}

/// Tracks the single batch that may be waiting for an ACK.
///
/// At most one batch is ever in flight: a second one would mean two writers
/// racing over the same checkpoint.
#[derive(Debug, Default)]
pub struct AckWindow {
    pending: Option<u64>,
}

impl AckWindow {
    /// A window with nothing in flight.
    pub fn new() -> Self {
        Self::default()
    }

    /// The batch currently waiting, if any.
    pub fn pending(&self) -> Option<u64> {
        self.pending
    }

    /// True when no batch is waiting.
    pub fn is_free(&self) -> bool {
        self.pending.is_none()
    }

    /// Opens the window for one batch.
    pub fn begin(&mut self, batch: u64) -> AckOutcome {
        if self.pending.is_some() {
            return AckOutcome::Busy;
        }
        self.pending = Some(batch);
        AckOutcome::Opened
    }

    /// Closes the window for the batch that is waiting.
    pub fn ack(&mut self, batch: u64) -> AckOutcome {
        match self.pending {
            None => AckOutcome::Idle,
            Some(waiting) if waiting != batch => AckOutcome::Mismatch,
            Some(_) => {
                self.pending = None;
                AckOutcome::Opened
            }
        }
    }
}

use std::time::{Duration, Instant};

/// The first backoff step after a busy database or a real error.
pub const BACKOFF_MIN: Duration = Duration::from_secs(1);

/// The largest backoff step.
pub const BACKOFF_MAX: Duration = Duration::from_secs(30);

/// How one slice ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SliceOutcome {
    /// Work was done: the next slice is one cadence away.
    Progressed,
    /// The database was busy: back off, never spin.
    Busy,
    /// A real error: back off as well.
    Failed,
}

/// What a switch change implies for the scheduler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchEffect {
    /// Nothing needs to happen right now.
    None,
    /// A slice is due immediately: it was turned on, resumed, or the core
    /// restarted while import was already on.
    ScheduleNow,
    /// Stop committing new batches once the current one is acknowledged.
    PauseAfterAck,
}

/// The total budget a shutdown may spend finishing the transaction in flight.
pub const SHUTDOWN_BUDGET: Duration = Duration::from_secs(4);

/// How a shutdown ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownOutcome {
    /// The transaction in flight committed inside the budget.
    Committed,
    /// The transaction in flight rolled back.
    RolledBack,
    /// The budget expired before the transaction could finish.
    TimedOut,
}

/// The shutdown handshake.
///
/// A shutdown never abandons a transaction half-written: the worker finishes or
/// rolls back the one in flight, within a fixed total budget. A repeated request
/// does **not** extend the deadline, so a second caller cannot keep the process
/// alive by asking again.
///
/// An offset that was never committed is never saved — it lives only inside the
/// transaction, so a rollback (and a process kill) leaves the checkpoint exactly
/// where it was.
#[derive(Clone, Copy, Debug, Default)]
pub struct ShutdownHandshake {
    deadline: Option<Instant>,
}

impl ShutdownHandshake {
    /// A handshake with no shutdown requested.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests a shutdown; returns `false` when one was already requested.
    pub fn request(&mut self, now: Instant) -> bool {
        if self.deadline.is_some() {
            return false;
        }
        self.deadline = Some(now + SHUTDOWN_BUDGET);
        true
    }

    /// True when a shutdown was requested.
    pub fn is_requested(&self) -> bool {
        self.deadline.is_some()
    }

    /// When the budget runs out.
    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// True when the budget is spent.
    pub fn expired(&self, now: Instant) -> bool {
        self.deadline.is_some_and(|deadline| now >= deadline)
    }

    /// Reports how the worker stopped.
    pub fn finish(&self, now: Instant, committed: bool) -> ShutdownOutcome {
        if self.expired(now) {
            return ShutdownOutcome::TimedOut;
        }
        if committed {
            ShutdownOutcome::Committed
        } else {
            ShutdownOutcome::RolledBack
        }
    }
}

/// The import switches.
///
/// Two independent switches: the user's `import_enabled` and the global core
/// switch. Import only runs when **both** are on, so turning the core off
/// suspends the importer without forgetting the user's own choice.
///
/// The importer is off by default, and nothing here is tied to a page: closing
/// the page, switching tabs or hiding the window cannot pause it, because the
/// only inputs are the two switches and the clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct ImportSwitch {
    import_enabled: bool,
    core_enabled: bool,
    /// Set once a pause has been acknowledged: no further batch may start.
    paused: bool,
}

impl ImportSwitch {
    /// The production default: import off.
    pub fn new() -> Self {
        Self::default()
    }

    /// The user's own switch.
    pub fn import_enabled(&self) -> bool {
        self.import_enabled
    }

    /// The global core switch.
    pub fn core_enabled(&self) -> bool {
        self.core_enabled
    }

    /// True when import may run at all.
    pub fn effective_enabled(&self) -> bool {
        self.import_enabled && self.core_enabled
    }

    /// True when a new batch may be committed.
    ///
    /// After a pause is acknowledged this is false, so the importer stops
    /// committing without reading any further bytes from the source files.
    pub fn may_commit(&self) -> bool {
        self.effective_enabled() && !self.paused
    }

    /// True while a pause is in force.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Sets the user's switch.
    pub fn set_import(&mut self, enabled: bool) -> SwitchEffect {
        if self.import_enabled == enabled {
            return SwitchEffect::None;
        }
        self.import_enabled = enabled;
        if enabled {
            // Turning it on is a fresh start: schedule at once.
            self.paused = false;
            SwitchEffect::ScheduleNow
        } else {
            SwitchEffect::PauseAfterAck
        }
    }

    /// Sets the global core switch.
    pub fn set_core(&mut self, enabled: bool) -> SwitchEffect {
        if self.core_enabled == enabled {
            return SwitchEffect::None;
        }
        self.core_enabled = enabled;
        if enabled {
            // A core restart with import already on resumes immediately.
            self.paused = false;
            if self.import_enabled {
                SwitchEffect::ScheduleNow
            } else {
                SwitchEffect::None
            }
        } else {
            SwitchEffect::PauseAfterAck
        }
    }

    /// Acknowledges that the current batch finished, so the pause takes effect.
    pub fn acknowledge_pause(&mut self) {
        if !self.effective_enabled() {
            self.paused = true;
        }
    }

    /// A system wake-up: missed ticks collapse into at most one slice.
    pub fn on_wake(&self, schedule: &mut SyncSchedule) -> bool {
        schedule.request()
    }
}

/// The import scheduler.
///
/// Driven by a **monotonic** clock, so a wall-clock jump cannot make it fire
/// early, late or twice. WB-RT-001 fixes the cadence at
/// [`crate::stats::workbuddy_import::reader::SYNC_INTERVAL`] (five seconds) for
/// both incremental sync and new-file discovery; the page's refresh choice has
/// no influence here, and there is no way to configure it.
///
/// Backoff is the *only* thing that can stretch the wait, and it never shortens
/// the cadence: a busy database makes the worker wait longer, never spin, and
/// the global SQLite busy timeout is left untouched.
#[derive(Debug)]
pub struct SyncSchedule {
    next_due: Instant,
    /// A tick (from the timer or from a manual check) is already waiting.
    pending: bool,
    running: bool,
    backoff: Duration,
}

impl SyncSchedule {
    /// A schedule whose first slice is due immediately.
    pub fn new(now: Instant) -> Self {
        Self {
            next_due: now,
            pending: false,
            running: false,
            backoff: Duration::ZERO,
        }
    }

    /// The fixed cadence this schedule runs on.
    pub fn cadence(&self) -> Duration {
        crate::stats::workbuddy_import::reader::SYNC_INTERVAL
    }

    /// When the next slice is due.
    pub fn next_due(&self) -> Instant {
        self.next_due
    }

    /// The current backoff (`0` when healthy).
    pub fn backoff(&self) -> Duration {
        self.backoff
    }

    /// True while a slice is running.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// True when a tick is already waiting.
    pub fn has_pending(&self) -> bool {
        self.pending
    }

    /// Requests a slice — from the timer or from a manual check.
    ///
    /// The two merge: a request while one is already waiting is absorbed, so a
    /// burst of manual checks can never queue a storm of slices.
    pub fn request(&mut self) -> bool {
        if self.pending {
            return false;
        }
        self.pending = true;
        true
    }

    /// Starts a slice when one is due and none is running.
    ///
    /// Returns `false` when a slice is already running (never re-entrant) or
    /// when the next one is not due yet.
    pub fn begin_slice(&mut self, now: Instant) -> bool {
        if self.running {
            return false;
        }
        let due = self.pending || now >= self.next_due;
        if !due {
            return false;
        }
        self.pending = false;
        self.running = true;
        true
    }

    /// Ends a slice and schedules the next one.
    ///
    /// A slice may process as many batches as it likes; the cadence is waited
    /// out **between slices**, never between batches, so a backfill is not
    /// slowed down to one batch per five seconds.
    pub fn finish_slice(&mut self, now: Instant, outcome: SliceOutcome) {
        self.running = false;
        match outcome {
            SliceOutcome::Progressed => {
                self.backoff = Duration::ZERO;
                self.next_due = now + self.cadence();
            }
            SliceOutcome::Busy | SliceOutcome::Failed => {
                self.backoff = if self.backoff.is_zero() {
                    BACKOFF_MIN
                } else {
                    (self.backoff * 2).min(BACKOFF_MAX)
                };
                self.next_due = now + self.backoff;
            }
        }
    }
}

/// Every fixed code the import commands may return.
///
/// Keeping the list here makes the contract checkable: a response can only ever
/// carry one of these, and every one of them carries the `stats_` prefix.
pub const WORKBUDDY_ERROR_CODES: &[&str] = &[
    crate::stats::dto::ERR_WORKBUDDY_IMPORT_DISABLED,
    crate::stats::dto::ERR_WORKBUDDY_ROOT_MISSING,
    crate::stats::dto::ERR_WORKBUDDY_PERMISSION_DENIED,
    crate::stats::dto::ERR_WORKBUDDY_INVALID_PATH,
    crate::stats::dto::ERR_WORKBUDDY_UNSUPPORTED_FORMAT,
    crate::stats::dto::ERR_WORKBUDDY_STALE_BATCH,
    crate::stats::dto::ERR_WORKBUDDY_MAPPING_CONFLICT,
    crate::stats::dto::ERR_WORKBUDDY_SOURCE_CHANGED,
    crate::stats::dto::ERR_WORKBUDDY_SOURCE_READ_FAILED,
    crate::stats::dto::ERR_WORKBUDDY_LEDGER_CONFLICT,
    crate::stats::dto::ERR_WORKBUDDY_SCAN_BUSY,
];

/// The eight commands the importer answers, in the order DATA-API lists them.
pub const WORKBUDDY_COMMANDS: &[&str] = &[
    "stats_workbuddy_status",
    "stats_workbuddy_settings_update",
    "stats_workbuddy_scan",
    "stats_workbuddy_pending",
    "stats_workbuddy_mapping_preview",
    "stats_workbuddy_mapping_apply",
    "stats_workbuddy_mapping_revoke_preview",
    "stats_workbuddy_mapping_revoke",
];

/// Startup contract check for the command surface.
pub fn commands_contract_check() -> Result<usize, &'static str> {
    if WORKBUDDY_COMMANDS.len() != 8 {
        return Err("workbuddy_import_command_count_drifted");
    }
    if WORKBUDDY_ERROR_CODES.len() != 11 {
        return Err("workbuddy_import_error_code_count_drifted");
    }
    for code in WORKBUDDY_ERROR_CODES {
        if !code.starts_with("stats_workbuddy_") {
            return Err("workbuddy_import_error_code_prefix");
        }
        // A code is a constant, so it can never carry a path or a query.
        if code.contains('/') || code.contains(' ') || code.contains('\'') {
            return Err("workbuddy_import_error_code_shape");
        }
    }
    if WORKBUDDY_COMMANDS
        .iter()
        .any(|name| !name.starts_with("stats_"))
    {
        return Err("workbuddy_import_command_prefix");
    }
    Ok(1)
}

/// Runs one import management command against a connection.
///
/// This is the only entry point for the eight commands, which keeps two
/// promises in one place: every failure is a **fixed error code** (never a file
/// name, a path or a SQL fragment), and no command scans inside the request —
/// `scan` only enqueues.
pub fn handle_command(
    conn: &mut rusqlite::Connection,
    command: &str,
    payload: &serde_json::Value,
    now_ms: i64,
    previews: &mut manual::PreviewCache,
) -> Result<String, String> {
    match command {
        "stats_workbuddy_status" => command_status(conn, now_ms),
        "stats_workbuddy_settings_update" => command_settings_update(conn, payload, now_ms),
        "stats_workbuddy_scan" => command_scan(conn, payload, now_ms),
        "stats_workbuddy_pending" => {
            match payload.get("view").and_then(serde_json::Value::as_str) {
                None | Some("pending") => command_pending(conn, payload, now_ms),
                Some("rules") => manual::rules::page(conn, payload),
                _ => Err(crate::stats::dto::ERR_INVALID.into()),
            }
        }
        "stats_workbuddy_mapping_preview" => manual::preview(conn, payload, now_ms, previews),
        "stats_workbuddy_mapping_apply" => manual::apply(conn, payload, now_ms, previews, false),
        "stats_workbuddy_mapping_revoke_preview" => {
            manual::revoke_preview(conn, payload, now_ms, previews)
        }
        "stats_workbuddy_mapping_revoke" => manual::apply(conn, payload, now_ms, previews, true),
        // An unknown command is answered with a fixed code, never an echo.
        _ => Err(crate::stats::dto::ERR_WORKBUDDY_INVALID_PATH.to_owned()),
    }
}

/// Maps a store failure onto the fixed public code for it.
fn store_code(failure: &store::StoreError) -> &'static str {
    use crate::stats::dto::*;
    match failure {
        store::StoreError::SettingsStale => ERR_WORKBUDDY_STALE_BATCH,
        store::StoreError::BatchRejected(reason) => match reason {
            crate::stats::workbuddy_import::types::BatchRejectReason::SourceChanged => {
                ERR_WORKBUDDY_SOURCE_CHANGED
            }
            _ => ERR_WORKBUDDY_SOURCE_READ_FAILED,
        },
        store::StoreError::AlreadyExists
        | store::StoreError::NotFound
        | store::StoreError::Tombstone
        | store::StoreError::CandidateNotAllowed
        | store::StoreError::CandidateTooLarge
        | store::StoreError::InvalidOffset
        | store::StoreError::Overlapping => ERR_WORKBUDDY_LEDGER_CONFLICT,
        store::StoreError::Sqlite(_) => ERR_UNAVAILABLE,
    }
}

fn json(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_owned())
}

fn required_i64(payload: &serde_json::Value, key: &str) -> Result<i64, String> {
    payload
        .get(key)
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| crate::stats::dto::ERR_INVALID.to_owned())
}

fn load_skipped(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<i64> {
    use types::DiagnosticKind;
    let terms: Vec<_> = DiagnosticKind::ALL
        .iter()
        .filter(|kind| {
            !matches!(
                kind,
                DiagnosticKind::Ignored | DiagnosticKind::IgnoredNoUsage
            )
        })
        .map(|kind| {
            format!(
                "COALESCE(json_extract(diagnostic_counts_json,'$.{}'),0)",
                kind.as_str()
            )
        })
        .collect();
    tx.query_row(
        &format!(
            "SELECT COALESCE(SUM({}),0) FROM workbuddy_import_files",
            terms.join("+")
        ),
        [],
        |row| row.get(0),
    )
}

fn command_status(conn: &mut rusqlite::Connection, now_ms: i64) -> Result<String, String> {
    let tx = conn
        .transaction()
        .map_err(|_| crate::stats::dto::ERR_UNAVAILABLE.to_owned())?;
    let settings = store::load_import_settings(&tx).map_err(|e| store_code(&e).to_owned())?;
    let counts = store::load_import_counts(&tx).map_err(|e| store_code(&e).to_owned())?;
    let progress = progress::snapshot(&tx).map_err(|e| store_code(&e).to_owned())?;
    let mut switch = ImportSwitch::new();
    switch.set_import(settings.enabled);
    let global = crate::stats::store::load_settings(&tx)
        .map_err(|_| crate::stats::dto::ERR_UNAVAILABLE.to_owned())?;
    switch.set_core(global.enabled);
    let state = if !settings.enabled {
        ImportState::Disabled
    } else if !global.enabled {
        ImportState::Paused
    } else if counts.files_pending > 0 {
        ImportState::Backfilling
    } else {
        ImportState::Idle
    };
    let snapshot = ImportStatusSnapshot {
        state,
        counts,
        last_incremental_at_ms: None,
        last_full_scan_at_ms: None,
        last_error_code: None,
    };
    let dto = ImportStatusDto::from_snapshot(
        &snapshot,
        &switch,
        false,
        settings.revision,
        settings.mapping_revision,
    );
    let mapping_task = reconcile::task_status(&tx).map_err(|e| store_code(&e).to_owned())?;
    let (tasks_pending, task_error): (i64, Option<String>) = tx
        .query_row(
            "SELECT COUNT(*),MAX(reconcile_error_code)
        FROM workbuddy_model_history WHERE reconcile_state<>'completed'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| crate::stats::dto::ERR_UNAVAILABLE.to_owned())?;

    let skipped = load_skipped(&tx).map_err(|_| crate::stats::dto::ERR_UNAVAILABLE.to_owned())?;
    tx.commit()
        .map_err(|_| crate::stats::dto::ERR_UNAVAILABLE.to_owned())?;
    let _ = now_ms;
    Ok(json(&serde_json::json!({
        "api_version": crate::stats::dto::API_VERSION,
        "state": dto.state,
        "enabled": dto.enabled,
        "effective_enabled": dto.effective_enabled,
        "root_found": dto.root_found,
        "settings_revision": dto.settings_revision,
        "mapping_revision": dto.mapping_revision,
        "mapping_task": mapping_task,
        "mapping_tasks_pending": tasks_pending,
        "files_seen": dto.files_seen,
        "files_pending": dto.files_pending,
        "imported": dto.imported,
        "excluded_proxy": dto.excluded_proxy,
        "pending": dto.pending,
        "conflict": dto.conflict,
        "deleted": dto.deleted,
        "skipped": skipped,
        "bytes_processed": dto.bytes_processed,
        "progress": progress,
        "last_error_code": task_error.or_else(||dto.last_error_code.map(str::to_owned)),
        "last_incremental_at_ms": dto.last_incremental_at_ms,
        "last_full_scan_at_ms": settings.last_full_scan_ms.filter(|time| *time>0),
        // The client shows "everything is synced" only when this is true.
        "complete": dto.is_complete() && tasks_pending==0,
    })))
}

fn command_settings_update(
    conn: &mut rusqlite::Connection,
    payload: &serde_json::Value,
    now_ms: i64,
) -> Result<String, String> {
    let enabled = payload
        .get("enabled")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| crate::stats::dto::ERR_INVALID.to_owned())?;
    let expected_revision = required_i64(payload, "expected_revision")?;
    let tx = conn
        .transaction()
        .map_err(|_| crate::stats::dto::ERR_UNAVAILABLE.to_owned())?;
    let outcome = store::set_import_enabled(&tx, enabled, expected_revision, now_ms)
        .map_err(|e| store_code(&e).to_owned())?;
    let (revision, namespace) = match outcome {
        store::SettingsOutcome::Updated(settings) => (settings.revision, settings.namespace_id),
        store::SettingsOutcome::Conflict { .. } => {
            return Err(crate::stats::dto::ERR_REVISION_CONFLICT.to_owned());
        }
    };
    tx.commit()
        .map_err(|_| crate::stats::dto::ERR_UNAVAILABLE.to_owned())?;
    Ok(json(&serde_json::json!({
        "enabled": enabled,
        "revision": revision,
        "namespace_established": !namespace.is_empty(),
    })))
}

fn command_scan(
    conn: &mut rusqlite::Connection,
    payload: &serde_json::Value,
    now_ms: i64,
) -> Result<String, String> {
    let mode = payload
        .get("mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("incremental");
    if mode != "incremental" && mode != "full" {
        // A scan mode the importer does not understand is a format problem, not
        // a missing argument.
        return Err(crate::stats::dto::ERR_WORKBUDDY_UNSUPPORTED_FORMAT.to_owned());
    }
    let tx = conn
        .transaction()
        .map_err(|_| crate::stats::dto::ERR_UNAVAILABLE.to_owned())?;
    let settings = store::load_import_settings(&tx).map_err(|e| store_code(&e).to_owned())?;
    if !settings.enabled {
        return Err(crate::stats::dto::ERR_WORKBUDDY_IMPORT_DISABLED.to_owned());
    }
    tx.commit()
        .map_err(|_| crate::stats::dto::ERR_UNAVAILABLE.to_owned())?;
    // The request only enqueues: no file is read on this thread, and the same
    // running scan always reports the same id.
    Ok(json(&serde_json::json!({
        "scan_id": format!("scan-{now_ms}"),
        "state": "queued",
        "mode": mode,
    })))
}

fn command_pending(
    conn: &mut rusqlite::Connection,
    payload: &serde_json::Value,
    now_ms: i64,
) -> Result<String, String> {
    let limit = payload
        .get("limit")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(20);
    if !matches!(limit, 20 | 50 | 100) {
        return Err(crate::stats::dto::ERR_INVALID.to_owned());
    }
    // A cursor from a previous page is honoured only while the mapping
    // revision it was issued under still stands.
    let cursor = match payload.get("cursor") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => Some(
            serde_json::from_value::<crate::stats::dto::PendingCursor>(value.clone())
                .map_err(|_| crate::stats::dto::ERR_INVALID.to_owned())?,
        ),
    };
    let tx = conn
        .transaction()
        .map_err(|_| crate::stats::dto::ERR_UNAVAILABLE.to_owned())?;
    let settings = store::load_import_settings(&tx).map_err(|e| store_code(&e).to_owned())?;
    let (groups, next) = crate::stats::query::pending_page(
        &tx,
        cursor.as_ref(),
        limit,
        settings.mapping_revision,
        &settings.namespace_id,
    )?;
    tx.commit()
        .map_err(|_| crate::stats::dto::ERR_UNAVAILABLE.to_owned())?;
    let _ = now_ms;
    Ok(json(&serde_json::json!({
        "api_version": crate::stats::dto::API_VERSION,
        "groups": groups,
        "next_cursor": next,
    })))
}

/// The published status of the importer, exactly as the API returns it.
///
/// Every counter comes from durable rows (see `ImportCounts`), so a restart
/// rebuilds the same numbers; the running state is reported as it is and never
/// claims that everything is synced.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportStatusDto {
    pub state: &'static str,
    pub enabled: bool,
    pub effective_enabled: bool,
    pub root_found: bool,
    pub settings_revision: i64,
    pub mapping_revision: i64,
    pub last_incremental_at_ms: Option<i64>,
    pub last_full_scan_at_ms: Option<i64>,
    pub files_seen: i64,
    pub files_pending: i64,
    pub imported: i64,
    pub excluded_proxy: i64,
    pub pending: i64,
    pub conflict: i64,
    pub deleted: i64,
    pub bytes_processed: i64,
    pub last_error_code: Option<&'static str>,
}

impl ImportStatusDto {
    /// Builds the published status from a snapshot and the switch state.
    pub fn from_snapshot(
        snapshot: &ImportStatusSnapshot,
        switch: &ImportSwitch,
        root_found: bool,
        settings_revision: i64,
        mapping_revision: i64,
    ) -> Self {
        let counts = &snapshot.counts;
        Self {
            state: snapshot.state.as_str(),
            enabled: switch.import_enabled(),
            effective_enabled: switch.effective_enabled(),
            root_found,
            settings_revision,
            mapping_revision,
            last_incremental_at_ms: snapshot.last_incremental_at_ms,
            last_full_scan_at_ms: snapshot.last_full_scan_at_ms,
            files_seen: counts.files_seen,
            files_pending: counts.files_pending,
            imported: counts.imported,
            excluded_proxy: counts.excluded_proxy,
            pending: counts.pending_classification,
            conflict: counts.conflict,
            deleted: counts
                .deleted_manual
                .saturating_add(counts.deleted_retention),
            bytes_processed: counts.bytes_processed,
            last_error_code: snapshot.last_error_code,
        }
    }

    /// True only when the importer is actually on, nothing is left to do and
    /// nothing is wrong.
    ///
    /// A disabled importer is **not** "everything is synced": it has not looked
    /// at anything, so claiming completion would be a lie.
    pub fn is_complete(&self) -> bool {
        self.effective_enabled
            && self.last_error_code.is_none()
            && self.files_pending == 0
            && self.pending == 0
            && self.conflict == 0
            && !matches!(self.state, "discovering" | "backfilling" | "scanning")
    }
}

/// Cumulative counts, derived from durable rows only.
///
/// Nothing here is an in-memory accumulator: the numbers come from the ledger
/// and the checkpoints, so a restart cannot lose or double them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportCounts {
    pub imported: i64,
    pub excluded_proxy: i64,
    pub excluded_retention: i64,
    pub pending_classification: i64,
    pub conflict: i64,
    pub deleted_manual: i64,
    pub deleted_retention: i64,
    /// Files discovered so far.
    pub files_seen: i64,
    /// Files that still have unread bytes or an unresolved error.
    pub files_pending: i64,
    /// Sum of committed offsets across every file.
    pub bytes_processed: i64,
}

impl ImportCounts {
    /// Every ledger row, whatever its state.
    pub fn total(&self) -> i64 {
        self.imported
            .saturating_add(self.excluded_proxy)
            .saturating_add(self.excluded_retention)
            .saturating_add(self.pending_classification)
            .saturating_add(self.conflict)
            .saturating_add(self.deleted_manual)
            .saturating_add(self.deleted_retention)
    }

    /// Rows that still need a human decision.
    pub fn unresolved(&self) -> i64 {
        self.pending_classification.saturating_add(self.conflict)
    }
}

/// The published status snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportStatusSnapshot {
    pub state: ImportState,
    pub counts: ImportCounts,
    pub last_incremental_at_ms: Option<i64>,
    pub last_full_scan_at_ms: Option<i64>,
    /// The last error that still stands, if any.
    pub last_error_code: Option<&'static str>,
}

impl ImportStatusSnapshot {
    /// True only when there is nothing left to do **and** nothing is wrong.
    ///
    /// A file that still has unread bytes or an unresolved error keeps the
    /// snapshot incomplete even while the worker is idle: the running state
    /// must never be presented as "everything is synced".
    pub fn is_complete(&self) -> bool {
        self.last_error_code.is_none()
            && self.counts.unresolved() == 0
            && self.counts.files_pending == 0
            && !self.state.is_active()
    }

    /// True when the importer is idle but work remains.
    pub fn is_idle_with_work_left(&self) -> bool {
        self.state == ImportState::Idle && !self.is_complete()
    }
}

/// The importer's own state machine (DATA-API section 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportState {
    /// Import is off.
    Disabled,
    /// On and caught up.
    Idle,
    /// Walking the directory tree.
    Discovering,
    /// Catching up on existing files.
    Backfilling,
    /// Running a full scan.
    Scanning,
    /// Paused by a global switch.
    Paused,
    /// On, but something is wrong (permissions, I/O, database).
    Degraded,
}

impl ImportState {
    /// Every state, in declaration order.
    pub const ALL: &'static [ImportState] = &[
        ImportState::Disabled,
        ImportState::Idle,
        ImportState::Discovering,
        ImportState::Backfilling,
        ImportState::Scanning,
        ImportState::Paused,
        ImportState::Degraded,
    ];

    /// Stable name.
    pub fn as_str(self) -> &'static str {
        match self {
            ImportState::Disabled => "disabled",
            ImportState::Idle => "idle",
            ImportState::Discovering => "discovering",
            ImportState::Backfilling => "backfilling",
            ImportState::Scanning => "scanning",
            ImportState::Paused => "paused",
            ImportState::Degraded => "degraded",
        }
    }

    /// Parses a stored name.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|state| state.as_str() == value)
    }

    /// True when work is actually in progress.
    pub fn is_active(self) -> bool {
        matches!(
            self,
            ImportState::Discovering | ImportState::Backfilling | ImportState::Scanning
        )
    }
}

/// Startup contract check for the shutdown handshake.
pub fn shutdown_contract_check() -> Result<usize, &'static str> {
    if SHUTDOWN_BUDGET != Duration::from_secs(4) {
        return Err("workbuddy_import_shutdown_budget_drifted");
    }
    let start = Instant::now();
    let mut handshake = ShutdownHandshake::new();
    if handshake.is_requested() || handshake.deadline().is_some() || handshake.expired(start) {
        return Err("workbuddy_import_shutdown_premature");
    }
    if !handshake.request(start) || !handshake.is_requested() {
        return Err("workbuddy_import_shutdown_request_failed");
    }
    if handshake.deadline() != Some(start + SHUTDOWN_BUDGET) {
        return Err("workbuddy_import_shutdown_deadline_drifted");
    }
    // A repeated request never extends the budget.
    if handshake.request(start + Duration::from_secs(3)) {
        return Err("workbuddy_import_shutdown_deadline_extended");
    }
    if handshake.deadline() != Some(start + SHUTDOWN_BUDGET) {
        return Err("workbuddy_import_shutdown_deadline_moved");
    }
    if handshake.finish(start + Duration::from_secs(1), true) != ShutdownOutcome::Committed {
        return Err("workbuddy_import_shutdown_commit_lost");
    }
    if handshake.finish(start + Duration::from_secs(1), false) != ShutdownOutcome::RolledBack {
        return Err("workbuddy_import_shutdown_rollback_lost");
    }
    if handshake.expired(start + Duration::from_secs(3)) {
        return Err("workbuddy_import_shutdown_expired_early");
    }
    if !handshake.expired(start + SHUTDOWN_BUDGET)
        || handshake.finish(start + SHUTDOWN_BUDGET, true) != ShutdownOutcome::TimedOut
    {
        return Err("workbuddy_import_shutdown_budget_not_enforced");
    }
    // A worker handle can be reclaimed: dropping the receiver closes the
    // channel, so a later offer is refused instead of blocking.
    let mut channel = ImportChannel::bounded();
    let receiver = channel
        .take_receiver()
        .ok_or("workbuddy_import_shutdown_receiver_missing")?;
    drop(receiver);
    if channel.offer(ImportMessage::Shutdown) != OfferOutcome::Closed {
        return Err("workbuddy_import_shutdown_handle_not_reclaimable");
    }
    Ok(1)
}

/// Startup contract check for the two switches and the pause handshake.
pub fn switch_contract_check() -> Result<usize, &'static str> {
    let mut switch = ImportSwitch::new();
    if switch.import_enabled()
        || switch.core_enabled()
        || switch.effective_enabled()
        || switch.may_commit()
        || switch.is_paused()
    {
        return Err("workbuddy_import_switch_default_not_off");
    }
    // The user's switch alone is not enough: the core switch must be on too.
    if switch.set_import(true) != SwitchEffect::ScheduleNow || switch.effective_enabled() {
        return Err("workbuddy_import_switch_effective_without_core");
    }
    if switch.set_core(true) != SwitchEffect::ScheduleNow
        || !switch.effective_enabled()
        || !switch.may_commit()
    {
        return Err("workbuddy_import_switch_core_restart_not_scheduled");
    }
    if switch.set_core(true) != SwitchEffect::None {
        return Err("workbuddy_import_switch_repeated_change");
    }
    // Turning the core off pauses after the current ACK, and the pause is
    // honoured: no further batch may start.
    if switch.set_core(false) != SwitchEffect::PauseAfterAck || switch.may_commit() {
        return Err("workbuddy_import_switch_pause_not_immediate");
    }
    switch.acknowledge_pause();
    if !switch.is_paused() || switch.may_commit() {
        return Err("workbuddy_import_switch_pause_ignored");
    }
    // Resuming schedules at once and clears the pause.
    if switch.set_core(true) != SwitchEffect::ScheduleNow
        || switch.is_paused()
        || !switch.may_commit()
    {
        return Err("workbuddy_import_switch_resume_failed");
    }
    // A wake-up collapses missed ticks into at most one slice.
    let now = Instant::now();
    let mut schedule = SyncSchedule::new(now);
    if !switch.on_wake(&mut schedule) || switch.on_wake(&mut schedule) {
        return Err("workbuddy_import_wake_tick_storm");
    }
    if !schedule.begin_slice(now) {
        return Err("workbuddy_import_wake_slice_missing");
    }
    // Nothing here may be tied to a page lifecycle.
    let source = include_str!("mod.rs");
    let production = source.split("#[cfg(test)]").next().unwrap_or(source);
    for needle in [
        ["ref", "resh"].concat(),
        ["visib", "ility"].concat(),
        ["page", "_lifecycle"].concat(),
        ["docum", "ent"].concat(),
    ] {
        if production.contains(&needle) {
            return Err("workbuddy_import_switch_page_bound");
        }
    }
    Ok(1)
}

/// Startup contract check for the fixed cadence and its backoff.
pub fn schedule_contract_check() -> Result<usize, &'static str> {
    if BACKOFF_MIN != Duration::from_secs(1) || BACKOFF_MAX != Duration::from_secs(30) {
        return Err("workbuddy_import_backoff_bounds_drifted");
    }
    let start = Instant::now();
    let mut schedule = SyncSchedule::new(start);
    if schedule.cadence() != Duration::from_secs(5) {
        return Err("workbuddy_import_cadence_drifted");
    }
    if !schedule.begin_slice(start) || !schedule.is_running() || schedule.begin_slice(start) {
        return Err("workbuddy_import_schedule_reentered");
    }
    schedule.finish_slice(start, SliceOutcome::Progressed);
    if schedule.next_due() != start + Duration::from_secs(5) || !schedule.backoff().is_zero() {
        return Err("workbuddy_import_cadence_after_progress");
    }
    if schedule.begin_slice(start + Duration::from_secs(4)) {
        return Err("workbuddy_import_schedule_fired_early");
    }
    if !schedule.begin_slice(start + Duration::from_secs(5)) {
        return Err("workbuddy_import_schedule_did_not_fire");
    }
    schedule.finish_slice(start + Duration::from_secs(5), SliceOutcome::Busy);
    if schedule.backoff() != BACKOFF_MIN {
        return Err("workbuddy_import_backoff_first_step");
    }
    // The backoff doubles up to its ceiling and never past it.
    let mut now = start + Duration::from_secs(5);
    for _ in 0..8 {
        now += schedule.backoff();
        schedule.begin_slice(now);
        schedule.finish_slice(now, SliceOutcome::Busy);
    }
    if schedule.backoff() != BACKOFF_MAX {
        return Err("workbuddy_import_backoff_ceiling");
    }
    // Progress resets it, and the cadence is back to five seconds.
    now += BACKOFF_MAX;
    schedule.begin_slice(now);
    schedule.finish_slice(now, SliceOutcome::Progressed);
    if !schedule.backoff().is_zero() || schedule.next_due() != now + Duration::from_secs(5) {
        return Err("workbuddy_import_backoff_not_reset");
    }
    // A manual check and a timer tick merge into one pending slice.
    if !schedule.request() || schedule.request() {
        return Err("workbuddy_import_tick_storm");
    }
    if !schedule.begin_slice(now) || schedule.has_pending() {
        return Err("workbuddy_import_pending_not_consumed");
    }
    schedule.finish_slice(now, SliceOutcome::Failed);
    if schedule.backoff() != BACKOFF_MIN {
        return Err("workbuddy_import_failure_backoff");
    }
    Ok(1)
}

/// Startup contract check for the import channel and its state machine.
pub fn channel_contract_check() -> Result<usize, &'static str> {
    if IMPORT_CHANNEL_CAPACITY != 2 || MAX_PENDING_ACK != 1 {
        return Err("workbuddy_import_channel_bounds_drifted");
    }
    let mut channel = ImportChannel::bounded();
    // Filling the channel must never block, and the third offer must be refused
    // rather than waited on.
    for _ in 0..IMPORT_CHANNEL_CAPACITY {
        if channel.offer(ImportMessage::Tick { at_ms: 0 }) != OfferOutcome::Accepted {
            return Err("workbuddy_import_channel_refused_early");
        }
    }
    if channel.offer(ImportMessage::Tick { at_ms: 0 }) != OfferOutcome::Busy {
        return Err("workbuddy_import_channel_full_accepted");
    }
    let Some(receiver) = channel.take_receiver() else {
        return Err("workbuddy_import_channel_receiver_missing");
    };
    if receiver.recv() != Ok(ImportMessage::Tick { at_ms: 0 }) {
        return Err("workbuddy_import_channel_message_drifted");
    }
    if channel.offer(ImportMessage::Shutdown) != OfferOutcome::Accepted {
        return Err("workbuddy_import_channel_did_not_drain");
    }
    // A cloned sender shares the same bound: the forwarding path cannot grow an
    // unbounded queue behind the worker's back.
    let extra = channel.sender();
    if !matches!(
        extra.try_send(ImportMessage::Shutdown),
        Err(TrySendError::Full(_))
    ) {
        return Err("workbuddy_import_channel_sender_clone_not_bounded");
    }
    let mut window = AckWindow::new();
    if !window.is_free() || window.begin(1) != AckOutcome::Opened || window.pending() != Some(1) {
        return Err("workbuddy_import_ack_window_open_failed");
    }
    if window.begin(2) != AckOutcome::Busy {
        return Err("workbuddy_import_ack_window_overlap");
    }
    if window.ack(2) != AckOutcome::Mismatch {
        return Err("workbuddy_import_ack_window_mismatch_accepted");
    }
    if window.ack(1) != AckOutcome::Opened || window.ack(1) != AckOutcome::Idle {
        return Err("workbuddy_import_ack_window_close_failed");
    }
    // The published snapshot must never call a state with work left "complete".
    let busy = ImportCounts {
        imported: 3,
        pending_classification: 1,
        files_seen: 2,
        files_pending: 1,
        bytes_processed: 128,
        ..ImportCounts::default()
    };
    if busy.total() != 4 || busy.unresolved() != 1 {
        return Err("workbuddy_import_counts_drifted");
    }
    let idle = ImportStatusSnapshot {
        state: ImportState::Idle,
        counts: busy,
        last_incremental_at_ms: None,
        last_full_scan_at_ms: None,
        last_error_code: None,
    };
    if idle.is_complete() || !idle.is_idle_with_work_left() {
        return Err("workbuddy_import_status_claimed_complete");
    }
    let settled = ImportStatusSnapshot {
        state: ImportState::Idle,
        counts: ImportCounts {
            imported: 3,
            files_seen: 1,
            bytes_processed: 64,
            ..ImportCounts::default()
        },
        last_incremental_at_ms: Some(5),
        last_full_scan_at_ms: None,
        last_error_code: None,
    };
    if !settled.is_complete() {
        return Err("workbuddy_import_status_never_complete");
    }
    let broken = ImportStatusSnapshot {
        last_error_code: Some("permission_denied"),
        ..settled.clone()
    };
    if broken.is_complete() {
        return Err("workbuddy_import_status_error_ignored");
    }
    if ImportState::ALL.len() != 7
        || ImportState::parse("backfilling") != Some(ImportState::Backfilling)
        || ImportState::parse("realtime").is_some()
        || ImportState::Idle.is_active()
        || !ImportState::Scanning.is_active()
    {
        return Err("workbuddy_import_state_machine_drifted");
    }
    Ok(1)
}

/// Contract check for the whole import subsystem.
///
/// Static model checks plus an in-memory ledger round trip; it creates no
/// thread, opens no user database and is safe on the startup path.
pub fn contract_check() -> Result<usize, &'static str> {
    Ok(types::contract_check()?
        + store::ledger_contract_check()?
        + store::checkpoint_contract_check()?
        + store::mapping_contract_check()?
        + manual::contract_check()?
        + parser::parser_contract_check()?
        + normalize::normalize_contract_check()?
        + reader::reader_contract_check()?
        + classify::classify_contract_check()?
        + channel_contract_check()?
        + schedule_contract_check()?
        + switch_contract_check()?
        + shutdown_contract_check()?
        + commands_contract_check()?)
}

#[cfg(test)]
mod channel_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn a_full_queue_yields_instead_of_blocking() {
        // AC041: the third offer must return immediately, not wait for room.
        let channel = ImportChannel::bounded();
        assert_eq!(
            channel.offer(ImportMessage::Tick { at_ms: 1 }),
            OfferOutcome::Accepted
        );
        assert_eq!(
            channel.offer(ImportMessage::Tick { at_ms: 2 }),
            OfferOutcome::Accepted
        );

        let started = Instant::now();
        assert_eq!(
            channel.offer(ImportMessage::Tick { at_ms: 3 }),
            OfferOutcome::Busy
        );
        let waited = started.elapsed();
        assert!(
            waited < Duration::from_millis(50),
            "a full queue must not make the caller wait (waited {waited:?})"
        );
    }

    #[test]
    fn draining_the_queue_makes_room_again() {
        let mut channel = ImportChannel::bounded();
        channel.offer(ImportMessage::Tick { at_ms: 1 });
        channel.offer(ImportMessage::Tick { at_ms: 2 });
        assert_eq!(channel.offer(ImportMessage::Shutdown), OfferOutcome::Busy);

        let receiver = channel
            .take_receiver()
            .expect("the receiver is handed over once");
        assert_eq!(receiver.recv(), Ok(ImportMessage::Tick { at_ms: 1 }));
        assert_eq!(
            channel.offer(ImportMessage::Shutdown),
            OfferOutcome::Accepted
        );
        assert!(
            channel.take_receiver().is_none(),
            "the receiver is taken only once"
        );
    }

    #[test]
    fn a_dropped_worker_reports_closed_instead_of_blocking() {
        let mut channel = ImportChannel::bounded();
        let receiver = channel.take_receiver().unwrap();
        drop(receiver);
        assert_eq!(channel.offer(ImportMessage::Shutdown), OfferOutcome::Closed);
    }

    #[test]
    fn only_one_batch_may_wait_for_an_ack() {
        // AC041: at most one pending ACK, so two writers can never race.
        let mut window = AckWindow::new();
        assert!(window.is_free());
        assert_eq!(window.begin(7), AckOutcome::Opened);
        assert_eq!(window.pending(), Some(7));
        assert_eq!(
            window.begin(8),
            AckOutcome::Busy,
            "a second batch must be refused"
        );
        assert_eq!(
            window.ack(8),
            AckOutcome::Mismatch,
            "and the wrong ACK ignored"
        );
        assert_eq!(window.pending(), Some(7), "the real batch still waits");
        assert_eq!(window.ack(7), AckOutcome::Opened);
        assert!(window.is_free());
        assert_eq!(
            window.ack(7),
            AckOutcome::Idle,
            "a repeated ACK is harmless"
        );
    }

    #[test]
    fn the_state_machine_is_closed_and_has_no_realtime_option() {
        assert_eq!(ImportState::ALL.len(), 7);
        for state in ImportState::ALL {
            assert_eq!(ImportState::parse(state.as_str()), Some(*state));
        }
        assert_eq!(
            ImportState::parse("realtime"),
            None,
            "there is no realtime mode"
        );
        assert_eq!(ImportState::parse("syncing"), None);
        assert_eq!(
            ImportState::ALL
                .iter()
                .filter(|state| state.is_active())
                .count(),
            3,
            "discovering, backfilling and scanning are the active states"
        );
        assert!(!ImportState::Disabled.is_active());
        assert!(!ImportState::Paused.is_active());
        assert!(
            !ImportState::Degraded.is_active(),
            "degraded is not progress"
        );
    }

    #[test]
    fn the_channel_contract_holds() {
        assert!(channel_contract_check().is_ok());
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;

    fn settled() -> ImportStatusSnapshot {
        ImportStatusSnapshot {
            state: ImportState::Idle,
            counts: ImportCounts {
                imported: 2,
                files_seen: 1,
                bytes_processed: 64,
                ..ImportCounts::default()
            },
            last_incremental_at_ms: Some(10),
            last_full_scan_at_ms: None,
            last_error_code: None,
        }
    }

    #[test]
    fn a_running_state_is_never_reported_as_complete() {
        for state in [
            ImportState::Discovering,
            ImportState::Backfilling,
            ImportState::Scanning,
        ] {
            let snapshot = ImportStatusSnapshot { state, ..settled() };
            assert!(!snapshot.is_complete(), "{state:?} is still working");
            assert!(!snapshot.is_idle_with_work_left());
        }
    }

    #[test]
    fn an_error_file_keeps_the_snapshot_incomplete() {
        // AC044: a file that was not processed must keep the state honest.
        let broken = ImportStatusSnapshot {
            last_error_code: Some("permission_denied"),
            ..settled()
        };
        assert!(!broken.is_complete());
        assert!(broken.is_idle_with_work_left(), "idle, but not finished");

        let unfinished = ImportStatusSnapshot {
            counts: ImportCounts {
                files_pending: 1,
                ..settled().counts
            },
            ..settled()
        };
        assert!(!unfinished.is_complete(), "unread bytes are still work");
    }

    #[test]
    fn unresolved_rows_keep_the_snapshot_incomplete() {
        for counts in [
            ImportCounts {
                pending_classification: 1,
                ..settled().counts
            },
            ImportCounts {
                conflict: 1,
                ..settled().counts
            },
        ] {
            let snapshot = ImportStatusSnapshot {
                counts,
                ..settled()
            };
            assert!(!snapshot.is_complete());
        }
    }

    #[test]
    fn a_settled_snapshot_is_complete_and_the_totals_add_up() {
        let snapshot = settled();
        assert!(snapshot.is_complete());
        assert_eq!(snapshot.counts.total(), 2);
        assert_eq!(snapshot.counts.unresolved(), 0);
        assert_eq!(
            ImportCounts::default().total(),
            0,
            "an empty ledger is a valid zero"
        );
    }

    #[test]
    fn the_status_contract_holds() {
        assert!(channel_contract_check().is_ok());
    }
}

#[cfg(test)]
mod schedule_tests {
    use super::*;

    #[test]
    fn the_cadence_is_five_seconds_and_never_shorter_than_the_cadence() {
        let start = Instant::now();
        let mut schedule = SyncSchedule::new(start);
        assert_eq!(schedule.cadence(), Duration::from_secs(5));
        assert!(
            schedule.begin_slice(start),
            "the first slice is due at once"
        );
        schedule.finish_slice(start, SliceOutcome::Progressed);
        assert_eq!(schedule.next_due(), start + Duration::from_secs(5));

        // No tick, no slice before it is due.
        assert!(!schedule.begin_slice(start + Duration::from_millis(4999)));
        assert!(schedule.begin_slice(start + Duration::from_secs(5)));
    }

    #[test]
    fn a_tick_and_a_manual_check_merge_into_one_pending_slice() {
        // AC045: a burst of manual checks must not queue a storm of slices.
        let start = Instant::now();
        let mut schedule = SyncSchedule::new(start);
        assert!(schedule.request(), "the first request is queued");
        assert!(!schedule.request(), "the second is absorbed");
        assert!(!schedule.request(), "and so is every later one");
        assert!(schedule.has_pending());
        assert!(schedule.begin_slice(start), "the merged request runs once");
        assert!(!schedule.has_pending(), "and is consumed");
        schedule.finish_slice(start, SliceOutcome::Progressed);
        assert!(!schedule.begin_slice(start), "no second slice was queued");
    }

    #[test]
    fn a_slice_never_re_enters_itself() {
        let start = Instant::now();
        let mut schedule = SyncSchedule::new(start);
        assert!(schedule.begin_slice(start));
        assert!(schedule.is_running());
        assert!(
            !schedule.begin_slice(start),
            "the running slice is not re-entered"
        );
        assert!(!schedule.begin_slice(start + Duration::from_secs(60)));
        schedule.finish_slice(start, SliceOutcome::Progressed);
        assert!(!schedule.is_running());
    }

    #[test]
    fn backoff_grows_to_thirty_seconds_and_resets_on_progress() {
        // AC045: a busy database makes the worker wait, never spin.
        let start = Instant::now();
        let mut schedule = SyncSchedule::new(start);
        schedule.begin_slice(start);
        schedule.finish_slice(start, SliceOutcome::Busy);
        assert_eq!(
            schedule.backoff(),
            Duration::from_secs(1),
            "first step is one second"
        );
        let mut now = start;
        let mut seen = vec![schedule.backoff()];
        for _ in 0..8 {
            now += schedule.backoff();
            schedule.begin_slice(now);
            schedule.finish_slice(now, SliceOutcome::Busy);
            seen.push(schedule.backoff());
        }
        assert_eq!(
            schedule.backoff(),
            Duration::from_secs(30),
            "capped at thirty seconds"
        );
        assert!(
            seen.windows(2).all(|pair| pair[1] >= pair[0]),
            "never shrinks while busy"
        );

        now += Duration::from_secs(30);
        schedule.begin_slice(now);
        schedule.finish_slice(now, SliceOutcome::Progressed);
        assert_eq!(
            schedule.backoff(),
            Duration::ZERO,
            "progress clears the backoff"
        );
        assert_eq!(schedule.next_due(), now + Duration::from_secs(5));
    }

    #[test]
    fn a_real_error_backs_off_the_same_way() {
        let start = Instant::now();
        let mut schedule = SyncSchedule::new(start);
        schedule.begin_slice(start);
        schedule.finish_slice(start, SliceOutcome::Failed);
        assert_eq!(schedule.backoff(), BACKOFF_MIN);
        assert!(schedule.next_due() >= start + BACKOFF_MIN);
        assert!(
            BACKOFF_MAX <= Duration::from_secs(30),
            "the ceiling is thirty seconds"
        );
    }

    #[test]
    fn the_schedule_contract_holds() {
        assert!(schedule_contract_check().is_ok());
    }
}

#[cfg(test)]
mod switch_tests {
    use super::*;

    fn enabled() -> ImportSwitch {
        let mut switch = ImportSwitch::new();
        switch.set_import(true);
        switch.set_core(true);
        switch
    }

    #[test]
    fn import_is_off_by_default() {
        let switch = ImportSwitch::new();
        assert!(!switch.import_enabled());
        assert!(!switch.core_enabled());
        assert!(!switch.effective_enabled());
        assert!(!switch.may_commit());
        assert!(!switch.is_paused());
    }

    #[test]
    fn both_switches_are_needed_and_each_turning_on_schedules_at_once() {
        let mut switch = ImportSwitch::new();
        assert_eq!(switch.set_import(true), SwitchEffect::ScheduleNow);
        assert!(!switch.effective_enabled(), "the core is still off");
        assert_eq!(switch.set_core(true), SwitchEffect::ScheduleNow);
        assert!(switch.effective_enabled());
        assert!(switch.may_commit());

        // A repeated change is not a restart.
        assert_eq!(switch.set_import(true), SwitchEffect::None);
        assert_eq!(switch.set_core(true), SwitchEffect::None);
    }

    #[test]
    fn a_pause_is_honoured_once_the_ack_arrives() {
        // AC046: after the pause is acknowledged nothing more is committed, so
        // the importer stops reading new bytes from the source files.
        let mut switch = enabled();
        assert_eq!(switch.set_core(false), SwitchEffect::PauseAfterAck);
        assert!(!switch.may_commit(), "the pause takes effect at once");
        switch.acknowledge_pause();
        assert!(switch.is_paused());
        assert!(!switch.may_commit(), "and stays in force");

        // The user's own switch is remembered while the core is off.
        assert!(switch.import_enabled());
        assert_eq!(switch.set_import(false), SwitchEffect::PauseAfterAck);
        switch.acknowledge_pause();
        assert!(!switch.effective_enabled());
    }

    #[test]
    fn resuming_schedules_immediately_and_clears_the_pause() {
        // AC046: the resume continues from the checkpoint, which was never
        // touched while paused.
        let mut switch = enabled();
        switch.set_core(false);
        switch.acknowledge_pause();
        assert_eq!(switch.set_core(true), SwitchEffect::ScheduleNow);
        assert!(!switch.is_paused());
        assert!(switch.may_commit());
    }

    #[test]
    fn turning_import_on_while_the_core_is_off_schedules_for_the_core_start() {
        let mut switch = ImportSwitch::new();
        switch.set_core(true);
        assert!(!switch.effective_enabled());
        assert_eq!(switch.set_import(true), SwitchEffect::ScheduleNow);
        assert!(switch.effective_enabled());
    }

    #[test]
    fn a_wake_up_merges_every_missed_tick_into_one_slice() {
        // AC046: a laptop that slept through many ticks wakes up to one slice,
        // not to a storm of them.
        let switch = enabled();
        let now = Instant::now();
        let mut schedule = SyncSchedule::new(now);
        schedule.begin_slice(now);
        schedule.finish_slice(now, SliceOutcome::Progressed);

        assert!(
            switch.on_wake(&mut schedule),
            "the first wake queues a slice"
        );
        assert!(!switch.on_wake(&mut schedule), "later wakes are absorbed");
        assert!(!switch.on_wake(&mut schedule));
        assert!(schedule.begin_slice(now), "and exactly one slice runs");
        assert!(!schedule.begin_slice(now), "never a second one");
    }

    #[test]
    fn the_switch_contract_holds() {
        assert!(switch_contract_check().is_ok());
    }
}

#[cfg(test)]
mod shutdown_tests {
    use super::*;

    #[test]
    fn a_transaction_finishes_inside_the_four_second_budget() {
        let start = Instant::now();
        let mut handshake = ShutdownHandshake::new();
        assert!(handshake.request(start));
        assert_eq!(handshake.deadline(), Some(start + Duration::from_secs(4)));
        assert!(!handshake.expired(start + Duration::from_secs(3)));
        assert_eq!(
            handshake.finish(start + Duration::from_secs(3), true),
            ShutdownOutcome::Committed,
            "the transaction in flight is allowed to finish"
        );
        assert_eq!(
            handshake.finish(start + Duration::from_secs(3), false),
            ShutdownOutcome::RolledBack,
            "or to roll back — never to be abandoned half-written"
        );
    }

    #[test]
    fn an_expired_budget_reports_a_timeout_and_never_a_commit() {
        let start = Instant::now();
        let mut handshake = ShutdownHandshake::new();
        handshake.request(start);
        assert!(handshake.expired(start + SHUTDOWN_BUDGET));
        assert_eq!(
            handshake.finish(start + SHUTDOWN_BUDGET, true),
            ShutdownOutcome::TimedOut,
            "a commit that could not finish in time is not reported as done"
        );
    }

    #[test]
    fn a_repeated_request_never_extends_the_deadline() {
        // A second caller cannot keep the process alive by asking again.
        let start = Instant::now();
        let mut handshake = ShutdownHandshake::new();
        assert!(handshake.request(start), "the first request is accepted");
        assert!(!handshake.request(start + Duration::from_secs(1)));
        assert!(!handshake.request(start + Duration::from_secs(2)));
        assert_eq!(handshake.deadline(), Some(start + Duration::from_secs(4)));
    }

    #[test]
    fn an_uncommitted_offset_is_never_saved() {
        // The offset lives only inside the transaction, so a rollback leaves the
        // checkpoint exactly where it was: nothing to "save" at shutdown.
        let start = Instant::now();
        let mut handshake = ShutdownHandshake::new();
        handshake.request(start);
        assert_eq!(handshake.finish(start, false), ShutdownOutcome::RolledBack);
        // And the importer is not running any more.
        let switch = ImportSwitch::new();
        assert!(
            !switch.may_commit(),
            "a default-off importer commits nothing"
        );
    }

    #[test]
    fn the_worker_handle_can_be_reclaimed() {
        // AC047: the worker's channel is closed on shutdown, so nothing blocks
        // and no message is silently queued for a worker that is gone.
        let mut channel = ImportChannel::bounded();
        assert_eq!(
            channel.offer(ImportMessage::Tick { at_ms: 1 }),
            OfferOutcome::Accepted
        );
        let receiver = channel.take_receiver().unwrap();
        drop(receiver);
        assert_eq!(channel.offer(ImportMessage::Shutdown), OfferOutcome::Closed);
    }

    #[test]
    fn the_shutdown_contract_holds() {
        assert!(shutdown_contract_check().is_ok());
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;
    use rusqlite::Connection;

    fn database() -> (tempfile::TempDir, Connection) {
        let folder = tempfile::tempdir().unwrap();
        let mut conn = Connection::open(folder.path().join("usage.sqlite3")).unwrap();
        crate::stats::store::migrate(&mut conn, folder.path()).unwrap();
        (folder, conn)
    }

    fn call(
        conn: &mut Connection,
        command: &str,
        payload: serde_json::Value,
    ) -> Result<String, String> {
        handle_command(
            conn,
            command,
            &payload,
            1_000,
            &mut manual::PreviewCache::default(),
        )
    }

    #[test]
    fn an_unknown_command_is_refused_with_a_fixed_code_and_no_echo() {
        // AC060: the answer never contains what was asked, only a constant.
        let (_folder, mut conn) = database();
        let error = call(
            &mut conn,
            "stats_workbuddy_drop_table",
            serde_json::json!({}),
        )
        .unwrap_err();
        assert_eq!(error, crate::stats::dto::ERR_WORKBUDDY_INVALID_PATH);
        assert!(!error.contains("drop"), "the request is never echoed");
        assert!(!error.contains(' '), "a code is one token");
    }

    #[test]
    fn every_command_validates_its_input_before_touching_the_database() {
        let (_folder, mut conn) = database();
        for (command, payload) in [
            (
                "stats_workbuddy_settings_update",
                serde_json::json!({"enabled": true}),
            ),
            (
                "stats_workbuddy_settings_update",
                serde_json::json!({"expected_revision": 1}),
            ),
            (
                "stats_workbuddy_mapping_preview",
                serde_json::json!({"model_id": "m"}),
            ),
            (
                "stats_workbuddy_mapping_preview",
                serde_json::json!({"model_id": "m", "from_ms": 0, "classification": "maybe"}),
            ),
            ("stats_workbuddy_mapping_apply", serde_json::json!({})),
            (
                "stats_workbuddy_mapping_revoke_preview",
                serde_json::json!({"mapping_id": "m"}),
            ),
            ("stats_workbuddy_mapping_revoke", serde_json::json!({})),
            ("stats_workbuddy_pending", serde_json::json!({"limit": 7})),
        ] {
            let error = call(&mut conn, command, payload).unwrap_err();
            assert!(
                error == crate::stats::dto::ERR_INVALID
                    || error == crate::stats::dto::ERR_WORKBUDDY_UNSUPPORTED_FORMAT,
                "{command} answered {error}"
            );
        }
    }

    #[test]
    fn a_scan_only_enqueues_and_is_refused_while_import_is_off() {
        // AC060: nothing is scanned inside the request, and a disabled importer
        // says so with its own fixed code.
        let (_folder, mut conn) = database();
        let error = call(
            &mut conn,
            "stats_workbuddy_scan",
            serde_json::json!({"mode": "incremental"}),
        )
        .unwrap_err();
        assert_eq!(error, crate::stats::dto::ERR_WORKBUDDY_IMPORT_DISABLED);

        let error = call(
            &mut conn,
            "stats_workbuddy_scan",
            serde_json::json!({"mode": "sideways"}),
        )
        .unwrap_err();
        assert_eq!(error, crate::stats::dto::ERR_WORKBUDDY_UNSUPPORTED_FORMAT);
    }

    #[test]
    fn status_reports_durable_counters_and_never_claims_completion_while_off() {
        let (_folder, mut conn) = database();
        let text = call(&mut conn, "stats_workbuddy_status", serde_json::json!({})).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["state"], "disabled", "import is off by default");
        assert_eq!(value["enabled"], false);
        assert_eq!(value["effective_enabled"], false);
        assert_eq!(value["imported"], 0);
        assert_eq!(value["pending"], 0);
        assert_eq!(
            value["complete"], false,
            "a disabled importer is not 'everything is synced'"
        );
    }

    #[test]
    fn enabling_import_establishes_a_namespace_and_cas_refuses_a_stale_revision() {
        let (_folder, mut conn) = database();
        let text = call(
            &mut conn,
            "stats_workbuddy_settings_update",
            serde_json::json!({"enabled": true, "expected_revision": 1}),
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["enabled"], true);
        assert_eq!(
            value["namespace_established"], true,
            "a namespace is created"
        );
        let revision = value["revision"].as_i64().unwrap();

        // The same revision cannot be used twice.
        let error = call(
            &mut conn,
            "stats_workbuddy_settings_update",
            serde_json::json!({"enabled": false, "expected_revision": 1}),
        )
        .unwrap_err();
        assert_eq!(error, crate::stats::dto::ERR_REVISION_CONFLICT);

        // With the current revision it succeeds, and a scan is then allowed.
        let text = call(
            &mut conn,
            "stats_workbuddy_settings_update",
            serde_json::json!({"enabled": true, "expected_revision": revision}),
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text).unwrap()["enabled"],
            true
        );
        let text = call(
            &mut conn,
            "stats_workbuddy_scan",
            serde_json::json!({"mode": "full"}),
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["state"], "queued");
        assert_eq!(value["mode"], "full");
    }

    #[test]
    fn the_command_contract_holds() {
        assert!(commands_contract_check().is_ok());
    }
}

//! SQLite ownership: schema migration, the single write connection, crash
//! recovery and bounded maintenance work.
//!
//! The writer thread is the only place that mutates the database. Every batch is
//! one transaction; maintenance steps are interleaved between batches so history
//! work never starves live request recording.
use crate::stats::collector::UsageOutcome;
use crate::stats::dto::{
    CostStatus, Currency, ERR_INVALID, ERR_REVISION_CONFLICT, ERR_UNAVAILABLE, InputBreakdown,
    InputMode, MaintenanceKind, MaintenanceStatus, PriceRuleDto, UsageMode, UsageStatus, error,
};
use crate::stats::event::{FinishRecord, StatsEvent};
use crate::stats::pricing::{self, CostParts, PriceRule};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

/// Schema version understood by this build.
pub const SCHEMA_VERSION: i64 = 4;
const MIGRATION_0001: &str = include_str!("migrations/0001.sql");
const MIGRATION_0002: &str = include_str!("migrations/0002.sql");
const MIGRATION_0003: &str = include_str!("migrations/0003.sql");
const MIGRATION_0004: &str = include_str!("migrations/0004.sql");
const WRITE_RETRIES: usize = 2;
/// Single lock wait before our own retry loop takes over (was 2000 ms, which
/// multiplied into seconds per batch under a held lock).
pub const BUSY_WAIT_MS: u64 = 250;
/// Longest one batch may spend, including every retry and isolation attempt.
pub const BATCH_BUDGET: Duration = Duration::from_millis(1500);

#[derive(Clone, Debug)]
pub struct DbError {
    pub message: String,
    /// SQLite code when the failure came from the database; `None` for our own
    /// validation errors.
    pub kind: Option<rusqlite::ErrorCode>,
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<rusqlite::Error> for DbError {
    fn from(value: rusqlite::Error) -> Self {
        let kind = match &value {
            rusqlite::Error::SqliteFailure(code, _) => Some(code.code),
            _ => None,
        };
        DbError {
            message: value.to_string(),
            kind,
        }
    }
}

impl DbError {
    /// A validation/refusal error that is not a database failure.
    pub fn refused(message: String) -> Self {
        Self {
            message,
            kind: None,
        }
    }
}

impl From<String> for DbError {
    fn from(value: String) -> Self {
        DbError::refused(value)
    }
}

/// Opens a connection with the documented pragmas and verifies WAL mode.
pub fn open_connection(path: &Path, read_only: bool) -> Result<Connection, DbError> {
    let conn = if read_only {
        Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )?
    } else {
        Connection::open(path)?
    };
    // A bounded single wait: retries are budgeted by the writer loop, so one
    // lock attempt must never consume seconds on its own.
    conn.busy_timeout(Duration::from_millis(if read_only {
        1000
    } else {
        BUSY_WAIT_MS
    }))?;
    if read_only {
        conn.pragma_update(None, "query_only", "ON")?;
        return Ok(conn);
    }
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "wal_autocheckpoint", 1000)?;
    let mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(DbError::refused(format!(
            "statistics database cannot use WAL journal mode (got {mode})"
        )));
    }
    Ok(conn)
}

/// Result of crash recovery.
#[derive(Clone, Debug, Default)]
pub struct Recovery {
    /// Cost rows given a terminal state during startup repair.
    pub costs_settled: i64,
    /// Previous runs closed with `stop_reason='unclean'`.
    pub unclean_runs: i64,
    /// Rows rewritten to `interrupted` because their run never finished.
    pub interrupted_requests: i64,
    /// Every request id above this was admitted after startup and is outside
    /// the repair scope.
    pub as_of_id: i64,
    pub unclean_detected: bool,
}

/// Creates or upgrades the schema. Never drops an existing database.
pub fn migrate(conn: &mut Connection, dir: &Path) -> Result<(), DbError> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(DbError::refused(error(
            crate::stats::dto::ERR_UNSUPPORTED_SCHEMA,
            "database was written by a newer version",
        )));
    }
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    if version > 0 {
        backup(conn, dir)?;
    }
    // Apply only the steps the database actually still needs. Replaying 0002
    // against an existing v2 database failed with "table stats_model_prices
    // already exists" and left collection disabled.
    if version == 0 {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(MIGRATION_0001)?;
        tx.pragma_update(None, "user_version", 1)?;
        tx.commit()?;
    }
    // v1 -> v2 rebuilds tables, which SQLite only allows with foreign keys
    // disabled outside the transaction; the copy is verified afterwards.
    if version < 2 {
        conn.pragma_update(None, "foreign_keys", "OFF")?;
        let upgraded = apply_step(conn, 2, MIGRATION_0002);
        conn.pragma_update(None, "foreign_keys", "ON")?;
        upgraded?;
    }
    if version < 3 {
        apply_step(conn, 3, MIGRATION_0003)?;
    }
    apply_step(conn, 4, MIGRATION_0004)
}

/// Runs one migration step in its own transaction and records the new version,
/// so a failure rolls back only that step.
fn apply_step(conn: &mut Connection, to_version: i64, sql: &str) -> Result<(), DbError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(sql)?;
    // Nothing may reference a missing request after a table rebuild.
    let dangling: i64 = {
        let mut statement = tx.prepare("PRAGMA foreign_key_check")?;
        let mut rows = statement.query([])?;
        i64::from(rows.next()?.is_some())
    };
    if dangling != 0 {
        return Err(DbError::refused(error(
            crate::stats::dto::ERR_UNAVAILABLE,
            "migration left a dangling reference",
        )));
    }
    tx.pragma_update(None, "user_version", to_version)?;
    tx.commit()?;
    Ok(())
}

/// Copies an existing database through the SQLite backup API before upgrading.
fn backup(conn: &Connection, dir: &Path) -> Result<(), DbError> {
    let target = dir.join("usage-backup.sqlite3");
    let _ = std::fs::remove_file(&target);
    let mut destination = Connection::open(&target)?;
    let backup = rusqlite::backup::Backup::new(conn, &mut destination)?;
    backup.run_to_completion(64, Duration::from_millis(5), None)?;
    drop(backup);
    crate::stats::restrict_permissions(&target);
    Ok(())
}

/// Marks previous runs as unclean and closes orphaned in-flight rows.
pub fn recover(conn: &mut Connection, run_id: &str, now_ms: i64) -> Result<Recovery, DbError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let unclean_runs = tx.execute(
        "UPDATE stats_runs SET stopped_at_ms=:now, stop_reason='unclean'
         WHERE run_id<>:run AND stopped_at_ms IS NULL AND started_at_ms<=:now",
        rusqlite::named_params! {":now": now_ms, ":run": run_id},
    )? as i64;
    tx.execute(
        "UPDATE stats_collection_intervals SET ended_at_ms=:now, state='unknown', reason='unclean_exit'
         WHERE ended_at_ms IS NULL AND started_at_ms<=:now",
        rusqlite::named_params! {":now": now_ms},
    )?;
    // The repair itself is a separate, bounded scan owned by the writer: doing
    // it here meant one immediate transaction and an unbounded amount of work
    // before the proxy could serve anything. Startup only freezes the scope.
    let as_of_id: i64 = tx.query_row(
        "SELECT COALESCE(MAX(id), 0) FROM usage_requests",
        [],
        |row| row.get(0),
    )?;
    let costs_settled = 0i64;
    tx.commit()?;
    Ok(Recovery {
        unclean_runs,
        interrupted_requests: 0,
        costs_settled,
        as_of_id,
        unclean_detected: unclean_runs > 0,
    })
}

/// Starts a fresh run and opens its collection interval.
pub fn start_run(
    conn: &mut Connection,
    run_id: &str,
    now_ms: i64,
    enabled: bool,
) -> Result<(), DbError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "INSERT INTO stats_runs(run_id, started_at_ms, last_persisted_at_ms)
         VALUES(:run, :now, :now) ON CONFLICT(run_id) DO NOTHING",
        rusqlite::named_params! {":run": run_id, ":now": now_ms},
    )?;
    tx.execute(
        "INSERT INTO stats_collection_intervals(run_id, started_at_ms, state, reason)
         VALUES(:run, :now, :state, :reason)",
        rusqlite::named_params! {
            ":run": run_id,
            ":now": now_ms,
            ":state": if enabled { "collecting" } else { "paused" },
            ":reason": if enabled { None::<&str> } else { Some("user_disabled") },
        },
    )?;
    tx.commit()?;
    Ok(())
}

/// Settings snapshot shared with the request path.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub enabled: bool,
    pub retention_days: i64,
    /// Currency applied to models without a configured price.
    pub default_currency: &'static str,
    pub revision: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: crate::stats::dto::DEFAULT_RETENTION_DAYS,
            default_currency: "CNY",
            revision: 1,
        }
    }
}

pub fn load_settings(conn: &Connection) -> Result<Settings, DbError> {
    let settings = conn
        .query_row(
            "SELECT enabled, retention_days, revision, default_currency
             FROM stats_settings WHERE singleton=1",
            [],
            |row| {
                Ok(Settings {
                    enabled: row.get::<_, i64>(0)? != 0,
                    retention_days: row.get(1)?,
                    revision: row.get(2)?,
                    default_currency: currency_static(&row.get::<_, String>(3)?),
                })
            },
        )
        .optional()?;
    Ok(settings.unwrap_or_default())
}

/// Bounded maintenance work executed between event batches.
#[derive(Debug)]
pub struct Maintenance {
    pub task_id: i64,
    pub kind: MaintenanceKind,
    pub cutoff_ms: i64,
    pub as_of_id: i64,
    /// Retained for the audit trail of maintenance records; the v2 sweep does
    /// not filter by it.
    #[allow(dead_code)]
    pub filter: Option<crate::stats::dto::StatsFilter>,
    pub affected: i64,
    pub cursor_id: i64,
    pub started_at_ms: i64,
    pub batch: i64,
}

impl Maintenance {
    pub fn new(
        task_id: i64,
        kind: MaintenanceKind,
        cutoff_ms: i64,
        as_of_id: i64,
        filter: Option<crate::stats::dto::StatsFilter>,
        started_at_ms: i64,
        batch: i64,
    ) -> Self {
        Self {
            task_id,
            kind,
            cutoff_ms,
            as_of_id,
            filter,
            affected: 0,
            cursor_id: 0,
            started_at_ms,
            batch,
        }
    }
}

/// The single writer: one connection, one thread, one transaction per batch.
pub struct Writer {
    pub conn: Connection,
    /// Current prices, kept in step with the committed table.
    pub prices: PublishedPrices,
    /// Progress of the bounded startup repair scan.
    pub scan: Option<RecoveryScan>,
    pub run_id: String,
    pub settings: Settings,
    pub maintenance: Option<Maintenance>,
    pub provider_modes: std::collections::HashMap<String, UsageMode>,
    /// Live counters mirrored into `stats_runs` on every successful batch.
    pub counters: std::sync::Arc<crate::stats::event::Counters>,
    /// Cache of the (relatively expensive) whole-table facts for `stats_health`.
    pub db_stats: DbStats,
}

/// Deadline shared by one batch, its retries and its isolation attempts.
#[derive(Clone, Copy, Debug)]
pub struct WriteBudget {
    deadline: Instant,
}

impl WriteBudget {
    pub fn new(limit: Duration) -> Self {
        Self {
            deadline: Instant::now() + limit,
        }
    }

    pub fn expired(&self) -> bool {
        Instant::now() >= self.deadline
    }

    fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }
}

/// True for failures that a retry can plausibly fix (a held lock, a busy file,
/// an I/O hiccup). Rows that violate a constraint are *not* transient: only
/// those may be isolated away from the rest of the batch.
fn is_transient(failure: &DbError) -> bool {
    matches!(
        failure.kind,
        Some(rusqlite::ErrorCode::DatabaseBusy)
            | Some(rusqlite::ErrorCode::DatabaseLocked)
            | Some(rusqlite::ErrorCode::SystemIoFailure)
            | Some(rusqlite::ErrorCode::OperationInterrupted)
    )
}

/// Result of one write batch: how many events landed and which ones could not be
/// written even in isolation.
#[derive(Clone, Debug, Default)]
pub struct BatchOutcome {
    pub applied: usize,
    pub dropped: Vec<StatsEvent>,
}

impl BatchOutcome {
    pub fn is_complete(&self) -> bool {
        self.dropped.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DbStats {
    pub record_count: i64,
    pub earliest_record_at_ms: Option<i64>,
    pub db_bytes: i64,
    pub wal_bytes: i64,
}

impl Writer {
    pub fn open(
        dir: &Path,
        run_id: &str,
        counters: std::sync::Arc<crate::stats::event::Counters>,
    ) -> Result<(Self, Recovery), DbError> {
        let path = dir.join("usage.sqlite3");
        let mut conn = open_connection(&path, false)?;
        crate::stats::restrict_permissions(&path);
        migrate(&mut conn, dir)?;
        let settings = load_settings(&conn)?;
        let now_ms = crate::stats::now_ms();
        let recovery = recover(&mut conn, run_id, now_ms)?;
        start_run(&mut conn, run_id, now_ms, settings.enabled)?;
        let provider_modes = load_provider_modes(&conn)?;
        let mut writer = Self {
            conn,
            prices: PublishedPrices::default(),
            scan: Some(RecoveryScan::new(recovery.as_of_id)),
            run_id: run_id.to_owned(),
            settings,
            maintenance: None,
            provider_modes,
            counters,
            db_stats: DbStats::default(),
        };
        // Load the current prices once at startup; the hot path only ever reads
        // the published copy.
        writer.prices = load_price_cache(&writer.conn)?;
        writer.refresh_db_stats()?;
        Ok((writer, recovery))
    }

    pub fn refresh_db_stats(&mut self) -> Result<(), DbError> {
        let record_count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM usage_requests", [], |row| row.get(0))?;
        let earliest: Option<i64> =
            self.conn
                .query_row("SELECT MIN(started_at_ms) FROM usage_requests", [], |row| {
                    row.get(0)
                })?;
        let (db_bytes, wal_bytes) = self.file_sizes();
        self.db_stats = DbStats {
            record_count,
            earliest_record_at_ms: earliest,
            db_bytes,
            wal_bytes,
        };
        Ok(())
    }

    fn file_sizes(&self) -> (i64, i64) {
        let main = self.conn.path().map(PathBuf::from).unwrap_or_default();
        let size = |path: PathBuf| -> i64 {
            std::fs::metadata(path)
                .map(|meta| meta.len() as i64)
                .unwrap_or(0)
        };
        let wal = PathBuf::from(format!("{}-wal", main.display()));
        (size(main), size(wal))
    }

    /// Applies one batch of events inside a shared deadline.
    ///
    /// Transient failures (a held lock, a busy file) are retried as a whole and
    /// then reported as a database-level failure: retrying 64 events one by one
    /// would multiply the lock wait by the batch size, which is what made a
    /// single batch take seconds. Only deterministic row errors are isolated,
    /// so one malformed record cannot discard its healthy neighbours.
    pub fn apply_batch(
        &mut self,
        batch: &[StatsEvent],
        budget: &WriteBudget,
    ) -> Result<BatchOutcome, DbError> {
        if batch.is_empty() {
            return Ok(BatchOutcome::default());
        }
        let mut last_error = None;
        for attempt in 0..=WRITE_RETRIES {
            if budget.expired() {
                break;
            }
            // Shrink the SQLite lock wait to what the budget still allows, so a
            // single blocked statement cannot overshoot the whole batch budget.
            let remaining = budget.remaining();
            let wait = (BUSY_WAIT_MS as u128).min(remaining.as_millis()).max(1) as u64;
            let _ = self.conn.busy_timeout(Duration::from_millis(wait));
            match self.apply_batch_once(batch) {
                Ok(()) => {
                    return Ok(BatchOutcome {
                        applied: batch.len(),
                        dropped: Vec::new(),
                    });
                }
                Err(failure) => {
                    last_error = Some(failure.clone());
                    if !is_transient(&failure) {
                        break;
                    }
                    if attempt < WRITE_RETRIES {
                        let pause = Duration::from_millis(20 * (attempt as u64 + 1));
                        if pause >= budget.remaining() {
                            break;
                        }
                        std::thread::sleep(pause);
                    }
                }
            }
        }
        let Some(last_error) = last_error else {
            return Err(DbError::refused("write budget exhausted".into()));
        };
        if is_transient(&last_error) {
            // Transient: report a database-level failure so the caller marks the
            // window degraded and the whole batch is retried later.
            return Err(last_error);
        }
        // Deterministic failure: isolate the offending events. `index` is the
        // real traversal position: `outcome.applied` only counts successes, so
        // using it as an index skipped or re-counted events after a bad one.
        let mut outcome = BatchOutcome::default();
        let mut index = 0usize;
        while index < batch.len() {
            if budget.expired() {
                outcome.dropped.extend_from_slice(&batch[index..]);
                break;
            }
            let event = &batch[index];
            match self.apply_batch_once(std::slice::from_ref(event)) {
                Ok(()) => outcome.applied += 1,
                Err(failure) => {
                    if is_transient(&failure) {
                        // The database went away mid-isolation; stop and let the
                        // caller retry the remainder later.
                        outcome.dropped.extend_from_slice(&batch[index..]);
                        break;
                    }
                    eprintln!(
                        "statistics isolated event failed: {} ({})",
                        failure.message,
                        event.kind()
                    );
                    outcome.dropped.push(event.clone());
                }
            }
            index += 1;
        }
        if outcome.applied == 0 && outcome.dropped.len() == batch.len() {
            return Err(last_error);
        }
        Ok(outcome)
    }

    /// Cheap write-capability probe used to leave `degraded` without waiting for
    /// new traffic. It takes and releases the write lock without changing data.
    pub fn probe_write(&mut self) -> Result<(), DbError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.rollback()?;
        Ok(())
    }

    /// A successful recovery can be idle: no later request batch may arrive to
    /// persist the counters for events that were lost while SQLite was locked.
    pub fn persist_counters(&self, now_ms: i64) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE stats_runs SET last_persisted_at_ms=?2, admitted_requests=?3,
                dropped_requests=?4, write_failures=?5 WHERE run_id=?1",
            rusqlite::params![
                self.run_id,
                now_ms,
                self.counters.admitted.load(Ordering::Relaxed) as i64,
                self.counters.dropped.load(Ordering::Relaxed) as i64,
                self.counters.write_failures.load(Ordering::Relaxed) as i64
            ],
        )?;
        Ok(())
    }

    /// Records a `degraded` collection window (`database_error`).
    pub fn enter_degraded(&mut self, now_ms: i64) -> Result<(), DbError> {
        self.open_interval(now_ms, "degraded", Some("database_error"))
    }

    /// Backfills a degraded window that could not be recorded while the
    /// database was unavailable: the covering interval is cut at `from_ms`, the
    /// degraded span is inserted and a fresh collecting interval opens at
    /// `to_ms` so the gap is never reported as collecting.
    pub fn mark_degraded_window(&mut self, from_ms: i64, to_ms: i64) -> Result<(), DbError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE stats_collection_intervals SET ended_at_ms=:from
             WHERE ended_at_ms IS NULL AND run_id=:run AND started_at_ms<=:from",
            rusqlite::named_params! {":from": from_ms, ":run": self.run_id},
        )?;
        tx.execute(
            "INSERT INTO stats_collection_intervals(run_id, started_at_ms, ended_at_ms, state, reason)
             VALUES(:run, :from, :to, 'degraded', 'database_error')",
            rusqlite::named_params! {":from": from_ms, ":to": to_ms.max(from_ms), ":run": self.run_id},
        )?;
        tx.execute(
            "INSERT INTO stats_collection_intervals(run_id, started_at_ms, state)
             VALUES(:run, :to, 'collecting')",
            rusqlite::named_params! {":to": to_ms, ":run": self.run_id},
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Records that collection resumed after a degraded window.
    pub fn leave_degraded(&mut self, now_ms: i64) -> Result<(), DbError> {
        self.open_interval(now_ms, "collecting", None)
    }

    /// Closes the open interval and opens a new one with the given state.
    fn open_interval(
        &mut self,
        now_ms: i64,
        state: &str,
        reason: Option<&str>,
    ) -> Result<(), DbError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE stats_collection_intervals SET ended_at_ms=:now
             WHERE ended_at_ms IS NULL AND run_id=:run AND started_at_ms<=:now",
            rusqlite::named_params! {":now": now_ms, ":run": self.run_id},
        )?;
        tx.execute(
            "INSERT INTO stats_collection_intervals(run_id, started_at_ms, state, reason)
             VALUES(:run, :now, :state, :reason)",
            rusqlite::named_params! {
                ":run": self.run_id,
                ":now": now_ms,
                ":state": state,
                ":reason": reason,
            },
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Gives a trustworthy terminal state to rows whose `Finish` event was
    /// dropped: the upstream behaviour is unknown, so nothing is fabricated.
    pub fn settle_orphans(&mut self, ids: &[String], now_ms: i64) -> Result<usize, DbError> {
        if ids.is_empty() {
            return Ok(0);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut settled = 0usize;
        for id in ids {
            let changed = tx.execute(
                "UPDATE usage_requests SET outcome='interrupted', error_kind='other',
                     ended_at_ms=:now, updated_at_ms=:now,
                     usage_status='missing', usage_reason=NULL, usage_source='none'
                 WHERE request_id=:id AND outcome='in_flight'",
                rusqlite::named_params! {":now": now_ms, ":id": id},
            )?;
            if changed == 1 {
                // Close the request and its cost in one transaction, from the
                // snapshot captured when it was admitted.
                settle_abandoned_cost(&tx, id, now_ms)?;
                settled += 1;
            }
        }
        tx.commit()?;
        Ok(settled)
    }

    fn apply_batch_once(&mut self, batch: &[StatsEvent]) -> Result<(), DbError> {
        if batch.is_empty() {
            return Ok(());
        }
        let now_ms = crate::stats::now_ms();
        let admitted = self.counters.admitted.load(Ordering::Relaxed) as i64;
        let dropped = self.counters.dropped.load(Ordering::Relaxed) as i64;
        let failures = self.counters.write_failures.load(Ordering::Relaxed) as i64;
        let run_id = self.run_id.clone();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for event in batch {
            match event {
                StatsEvent::Start(meta) => {
                    tx.execute(
                        "INSERT INTO usage_requests(
                            request_id, run_id, source, provider_id, route_id, provider_name,
                            route_name, requested_model, upstream_model, started_at_ms, stream,
                            created_at_ms, updated_at_ms, outcome, usage_status, usage_source,upstream_protocol)
                         VALUES(:request_id,:run_id,:source,:provider_id,:route_id,:provider_name,
                            :route_name,:requested_model,:upstream_model,:started_at,:stream,
                            -- `created_at_ms` is the request's own start time, not the
                            -- batch time: a fast request can put Start and Finish in the
                            -- same batch, and the schema requires
                            -- `updated_at_ms >= created_at_ms`.
                            :started_at,:now,'in_flight','missing','none',:protocol)
                         ON CONFLICT(request_id) DO NOTHING",
                        rusqlite::named_params! {
                            ":request_id": meta.request_id,
                            ":run_id": meta.run_id,
                            ":source": meta.source.as_str(),
                            ":provider_id": meta.provider_id,
                            ":route_id": meta.route_id,
                            ":provider_name": meta.provider_name,
                            ":route_name": meta.route_name,
                            ":requested_model": meta.requested_model,
                            ":upstream_model": meta.upstream_model,
                            ":protocol":meta.upstream_protocol,
                            ":started_at": meta.started_at_ms,
                            ":stream": if meta.stream { 1 } else { 0 },
                            ":now": now_ms,
                        },
                    )?;
                    // The request's own price snapshot is part of the same
                    // transaction, so a later edit or delete cannot change it.
                    tx.execute(
                        "INSERT INTO usage_costs(
                             request_id, source_price_id, source_price_revision, price_source,
                             price_snapshot_at_ms, currency, input_mode, input_rate_nanos,
                             output_rate_nanos, cache_read_rate_nanos, cache_write_rate_nanos,
                             status, pricing_version, calculated_at_ms)
                         VALUES(:request_id,:source_id,:source_revision,:price_source,
                             :snapshot_at,:currency,:mode,:input,:output,:read,:write,
                             'pending',2,:now)
                         ON CONFLICT(request_id) DO NOTHING",
                        rusqlite::named_params! {
                            ":request_id": meta.request_id,
                            ":source_id": meta.price.source_price_id,
                            ":source_revision": meta.price.source_price_revision,
                            ":price_source": meta.price.price_source,
                            ":snapshot_at": meta.price.price_snapshot_at_ms,
                            ":currency": meta.price.currency,
                            ":mode": meta.price.input_mode,
                            ":input": meta.price.input_rate_nanos,
                            ":output": meta.price.output_rate_nanos,
                            ":read": meta.price.cache_read_rate_nanos,
                            ":write": meta.price.cache_write_rate_nanos,
                            ":now": now_ms,
                        },
                    )?;
                }
                StatsEvent::Finish {
                    request_id, finish, ..
                } => {
                    finish_request(&tx, request_id, finish, now_ms)?;
                }
            }
        }
        tx.execute(
            "UPDATE stats_runs SET last_persisted_at_ms=:now,
                 admitted_requests=:admitted, dropped_requests=:dropped, write_failures=:failures
             WHERE run_id=:run",
            rusqlite::named_params! {
                ":now": now_ms,
                ":admitted": admitted,
                ":dropped": dropped,
                ":failures": failures,
                ":run": run_id,
            },
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Executes one bounded maintenance step. Returns `true` while work remains.
    pub fn maintenance_step(&mut self) -> Result<bool, DbError> {
        let Some(mut task) = self.maintenance.take() else {
            return Ok(false);
        };
        let now_ms = crate::stats::now_ms();
        let outcome = self.run_maintenance_step(&mut task, now_ms);
        match outcome {
            Ok((_, true)) => {
                self.maintenance = Some(task);
                Ok(true)
            }
            Ok((_, false)) => {
                self.record_maintenance(&task, MaintenanceStatus::Completed, None, now_ms)?;
                Ok(false)
            }
            Err(failure) => {
                self.record_maintenance(
                    &task,
                    MaintenanceStatus::Failed,
                    Some("maintenance_failed"),
                    now_ms,
                )?;
                Err(failure)
            }
        }
    }

    fn run_maintenance_step(
        &mut self,
        task: &mut Maintenance,
        now_ms: i64,
    ) -> Result<(i64, bool), DbError> {
        let _ = now_ms;
        match task.kind {
            // Repricing history with the current price no longer exists: every
            // request keeps the snapshot it was admitted with.
            MaintenanceKind::Reprice | MaintenanceKind::Migration => Ok((0, false)),
            MaintenanceKind::Retention | MaintenanceKind::Clear => self.delete_finished(task),
        }
    }

    /// Deletes finished rows in bounded batches.
    ///
    /// `retention` also applies the retention cutoff, so an automatic sweep is
    /// restricted to rows older than the configured window; a manual clear uses
    /// only the confirmed watermark it was given. `in_flight` rows are never
    /// eligible in either case.
    fn delete_finished(&mut self, task: &mut Maintenance) -> Result<(i64, bool), DbError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // One eligibility predicate for both the preview and the sweep: finished
        // rows inside the confirmed id watermark and older than the cutoff. A
        // manual clear passes its preview time as the cutoff, so a record that
        // was not counted by the preview can never be deleted.
        let mut ids: Vec<i64> = Vec::new();
        {
            let mut statement = tx.prepare(
                "SELECT id FROM usage_requests
                 WHERE outcome<>'in_flight' AND ended_at_ms IS NOT NULL
                   AND id>?1 AND id<=?2 AND started_at_ms<?3
                 ORDER BY id LIMIT ?4",
            )?;
            let mut rows = statement.query(rusqlite::params![
                task.cursor_id,
                task.as_of_id,
                task.cutoff_ms,
                task.batch.max(1)
            ])?;
            while let Some(row) = rows.next()? {
                ids.push(row.get(0)?);
            }
        }
        if ids.is_empty() {
            // Nothing left in range: the sweep is complete.
            task.cursor_id = task.as_of_id;
            tx.commit()?;
            return Ok((0, false));
        }
        // The cursor only advances to the last row this batch actually scanned;
        // taking the table MAX would skip every remaining candidate.
        let last_scanned = *ids.last().expect("non-empty");
        let full_batch = ids.len() as i64 >= task.batch.max(1);
        let mut deleted = 0i64;
        for chunk in ids.chunks(256) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!("DELETE FROM usage_requests WHERE id IN ({placeholders})");
            deleted += tx.execute(&sql, rusqlite::params_from_iter(chunk.iter()))? as i64;
        }
        task.cursor_id = last_scanned;
        task.affected += deleted;
        tx.commit()?;
        Ok((deleted, full_batch))
    }

    fn record_maintenance(
        &mut self,
        task: &Maintenance,
        status: MaintenanceStatus,
        error_code: Option<&str>,
        now_ms: i64,
    ) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE stats_maintenance_events SET status=:status, completed_at_ms=:now,
                 affected_rows=:affected, error_code=:error
             WHERE id=:id",
            rusqlite::named_params! {
                ":status": status.as_str(),
                ":now": now_ms.max(task.started_at_ms),
                ":affected": task.affected,
                ":error": error_code,
                ":id": task.task_id,
            },
        )?;
        self.refresh_db_stats()?;
        Ok(())
    }

    /// Starts an automatic retention sweep when one is due (at most daily) and
    /// no other maintenance task is running. Only finished rows are eligible.
    pub fn maybe_start_retention(&mut self, now_ms: i64) -> Result<bool, DbError> {
        if self.maintenance.is_some() || self.settings.retention_days <= 0 {
            return Ok(false);
        }
        let last: Option<i64> = self.conn.query_row(
            "SELECT MAX(started_at_ms) FROM stats_maintenance_events WHERE kind='retention'",
            [],
            |row| row.get(0),
        )?;
        if last.is_some_and(|last| now_ms - last < 86_400_000) {
            return Ok(false);
        }
        let as_of_id: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(id),0) FROM usage_requests",
            [],
            |row| row.get(0),
        )?;
        let cutoff = now_ms - self.settings.retention_days * 86_400_000;
        let task_id: i64 = self.conn.query_row(
            "INSERT INTO stats_maintenance_events(kind, started_at_ms, status, affected_rows)
             VALUES('retention', ?1, 'running', 0) RETURNING id",
            [now_ms],
            |row| row.get(0),
        )?;
        self.maintenance = Some(Maintenance::new(
            task_id,
            MaintenanceKind::Retention,
            cutoff,
            as_of_id,
            None,
            now_ms,
            crate::stats::dto::MAINTENANCE_BATCH,
        ));
        Ok(true)
    }

    // ---- control commands -------------------------------------------------

    pub fn update_settings(
        &mut self,
        enabled: bool,
        retention_days: i64,
        default_currency: Option<&str>,
        expected_revision: i64,
        now_ms: i64,
    ) -> Result<Settings, DbError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE stats_settings SET enabled=:enabled, retention_days=:retention,
                 default_currency=COALESCE(:currency, default_currency),
                 updated_at_ms=:now, revision=revision+1
             WHERE singleton=1 AND revision=:expected",
            rusqlite::named_params! {
                ":enabled": if enabled { 1 } else { 0 },
                ":retention": retention_days,
                ":currency": default_currency,
                ":now": now_ms,
                ":expected": expected_revision,
            },
        )?;
        if changed != 1 {
            return Err(DbError::refused(error(
                ERR_REVISION_CONFLICT,
                "statistics settings changed; reload and retry",
            )));
        }
        let settings = load_settings(&tx)?;
        if settings.enabled != self.settings.enabled {
            let state = if settings.enabled {
                "collecting"
            } else {
                "paused"
            };
            let reason = if settings.enabled {
                None
            } else {
                Some("user_disabled")
            };
            tx.execute(
                "UPDATE stats_collection_intervals SET ended_at_ms=:now
                 WHERE ended_at_ms IS NULL AND run_id=:run",
                rusqlite::named_params! {":now": now_ms, ":run": self.run_id},
            )?;
            tx.execute(
                "INSERT INTO stats_collection_intervals(run_id, started_at_ms, state, reason)
                 VALUES(:run, :now, :state, :reason)",
                rusqlite::named_params! {
                    ":run": self.run_id,
                    ":now": now_ms,
                    ":state": state,
                    ":reason": reason,
                },
            )?;
        }
        tx.commit()?;
        self.settings = settings;
        Ok(settings)
    }

    pub fn load_provider_mode(&self, provider_id: &str) -> (UsageMode, i64) {
        match self
            .conn
            .query_row(
                "SELECT usage_mode, revision FROM stats_provider_settings WHERE provider_id=?1",
                [provider_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
        {
            Ok(Some((mode, revision))) => (
                UsageMode::parse(&mode).unwrap_or(UsageMode::Observe),
                revision,
            ),
            _ => (UsageMode::Observe, 0),
        }
    }

    pub fn update_provider_mode(
        &mut self,
        provider_id: &str,
        usage_mode: UsageMode,
        expected_revision: i64,
        now_ms: i64,
    ) -> Result<i64, DbError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: Option<i64> = tx
            .query_row(
                "SELECT revision FROM stats_provider_settings WHERE provider_id=?1",
                [provider_id],
                |row| row.get(0),
            )
            .optional()?;
        let new_revision = match current {
            None => {
                if expected_revision != 0 {
                    return Err(DbError::refused(error(
                        ERR_REVISION_CONFLICT,
                        "provider settings changed; reload and retry",
                    )));
                }
                tx.execute(
                    "INSERT INTO stats_provider_settings(provider_id, usage_mode, updated_at_ms, revision)
                     VALUES(?1, ?2, ?3, 1)",
                    rusqlite::params![provider_id, usage_mode.as_str(), now_ms],
                )?;
                1
            }
            Some(revision) => {
                if revision != expected_revision {
                    return Err(DbError::refused(error(
                        ERR_REVISION_CONFLICT,
                        "provider settings changed; reload and retry",
                    )));
                }
                let changed = tx.execute(
                    "UPDATE stats_provider_settings SET usage_mode=?2, updated_at_ms=?3, revision=revision+1
                     WHERE provider_id=?1 AND revision=?4",
                    rusqlite::params![provider_id, usage_mode.as_str(), now_ms, expected_revision],
                )?;
                if changed != 1 {
                    return Err(DbError::refused(error(
                        ERR_REVISION_CONFLICT,
                        "provider settings changed; reload and retry",
                    )));
                }
                revision + 1
            }
        };
        tx.commit()?;
        self.provider_modes
            .insert(provider_id.to_owned(), usage_mode);
        Ok(new_revision)
    }

    pub fn insert_maintenance(
        &mut self,
        kind: MaintenanceKind,
        now_ms: i64,
    ) -> Result<i64, DbError> {
        self.conn.execute(
            "INSERT INTO stats_maintenance_events(kind, started_at_ms, status, affected_rows)
             VALUES(?1, ?2, 'running', 0)",
            rusqlite::params![kind.as_str(), now_ms],
        )?;
        Ok(self.conn.last_insert_rowid())
    }
}

fn load_provider_modes(
    conn: &Connection,
) -> Result<std::collections::HashMap<String, UsageMode>, DbError> {
    let mut statement =
        conn.prepare("SELECT provider_id, usage_mode FROM stats_provider_settings")?;
    let mut rows = statement.query([])?;
    let mut modes = std::collections::HashMap::new();
    while let Some(row) = rows.next()? {
        let provider_id: String = row.get(0)?;
        let mode: String = row.get(1)?;
        modes.insert(
            provider_id,
            UsageMode::parse(&mode).unwrap_or(UsageMode::Observe),
        );
    }
    Ok(modes)
}

/// Finds the single price rule covering `started_at_ms`, or `None` (unpriced).
/// Immutable copy of the unit prices one request is admitted with.
///
/// Copied from the published cache when the guard is created, written into the
/// request's own cost row at `Start`, and used at `Finish`. Editing or deleting
/// the price configuration later never changes it.
#[derive(Clone, Debug, PartialEq)]
pub struct PriceSnapshot {
    pub source_price_id: Option<String>,
    pub source_price_revision: Option<i64>,
    pub price_source: &'static str,
    pub currency: &'static str,
    pub input_mode: &'static str,
    pub input_rate_nanos: i64,
    pub output_rate_nanos: i64,
    pub cache_read_rate_nanos: i64,
    pub cache_write_rate_nanos: i64,
    pub price_snapshot_at_ms: i64,
}

impl PriceSnapshot {
    /// The explicit zero default used when no price is configured.
    pub fn default_zero(currency: &'static str, now_ms: i64) -> Self {
        Self {
            source_price_id: None,
            source_price_revision: None,
            price_source: "default_zero",
            currency,
            input_mode: "flat",
            input_rate_nanos: 0,
            output_rate_nanos: 0,
            cache_read_rate_nanos: 0,
            cache_write_rate_nanos: 0,
            price_snapshot_at_ms: now_ms,
        }
    }
}

/// Published price cache: the hot path copies a snapshot from here and never
/// queries SQLite.
pub type PriceCache = Arc<std::sync::RwLock<PublishedPrices>>;

/// The committed current prices, replaced after every successful edit.
#[derive(Clone, Debug)]
pub struct PublishedPrices {
    entries: std::collections::HashMap<(String, String), PriceSnapshot>,
    pub default_currency: &'static str,
}

impl Default for PublishedPrices {
    fn default() -> Self {
        // The documented initial default; an empty currency would violate the
        // `usage_costs.currency` constraint on the very first request.
        Self {
            entries: std::collections::HashMap::new(),
            default_currency: "CNY",
        }
    }
}

impl PublishedPrices {
    /// Looks up the snapshot admitted for this provider and model.
    pub fn get(&self, provider_id: &str, upstream_model: &str) -> Option<&PriceSnapshot> {
        self.entries
            .get(&(provider_id.to_owned(), upstream_model.to_owned()))
    }
}

fn currency_static(value: &str) -> &'static str {
    if value == "CNY" { "CNY" } else { "USD" }
}

/// Loads the current prices into a publishable cache.
pub fn load_price_cache(conn: &Connection) -> Result<PublishedPrices, DbError> {
    let default_currency = currency_static(&conn.query_row(
        "SELECT default_currency FROM stats_settings WHERE singleton=1",
        [],
        |row| row.get::<_, String>(0),
    )?);
    let mut statement = conn.prepare(
        "SELECT price_id, provider_id, upstream_model, currency, input_mode,
                input_rate_nanos, output_rate_nanos, cache_read_rate_nanos,
                cache_write_rate_nanos, revision, updated_at_ms
         FROM stats_model_prices",
    )?;
    let mut rows = statement.query([])?;
    let mut entries = std::collections::HashMap::new();
    while let Some(row) = rows.next()? {
        let snapshot = PriceSnapshot {
            source_price_id: Some(row.get(0)?),
            source_price_revision: Some(row.get(9)?),
            price_source: "manual",
            currency: currency_static(&row.get::<_, String>(3)?),
            input_mode: match row.get::<_, String>(4)?.as_str() {
                "read_discount" => "read_discount",
                "disjoint" => "disjoint",
                _ => "flat",
            },
            input_rate_nanos: row.get(5)?,
            output_rate_nanos: row.get(6)?,
            cache_read_rate_nanos: row.get(7)?,
            cache_write_rate_nanos: row.get(8)?,
            price_snapshot_at_ms: row.get(10)?,
        };
        entries.insert((row.get(1)?, row.get(2)?), snapshot);
    }
    Ok(PublishedPrices {
        entries,
        default_currency,
    })
}

/// Looks a snapshot up in the published cache, falling back to the explicit
/// zero default. Never touches SQLite.
pub fn snapshot_for(
    cache: &PriceCache,
    provider_id: &str,
    upstream_model: &str,
    now_ms: i64,
) -> PriceSnapshot {
    let published = cache.read().unwrap();
    let mut snapshot = published
        .get(provider_id, upstream_model)
        .cloned()
        .unwrap_or_else(|| PriceSnapshot::default_zero(published.default_currency, now_ms));
    // The stamp records when *this request* took the price, not when the price
    // was last edited: the same configuration serves many admission times.
    snapshot.price_snapshot_at_ms = now_ms;
    snapshot
}

pub fn price_rule_dto(conn: &Connection, rule_id: &str) -> Result<Option<PriceRuleDto>, DbError> {
    let mut statement = conn.prepare(
        "SELECT price_id, provider_id, upstream_model, currency, input_mode,
                input_rate_nanos, output_rate_nanos, cache_read_rate_nanos,
                cache_write_rate_nanos, revision, label, created_at_ms, updated_at_ms
         FROM stats_model_prices WHERE price_id=?1",
    )?;
    let mut rows = statement.query([rule_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(read_price_rule(row)?)),
        None => Ok(None),
    }
}

fn read_price_rule(row: &rusqlite::Row<'_>) -> rusqlite::Result<PriceRuleDto> {
    let currency = Currency::parse(&row.get::<_, String>(3)?).unwrap_or(Currency::Usd);
    // The API exposes rates exactly as stored: integer nano units.
    Ok(PriceRuleDto {
        price_id: row.get(0)?,
        rule_id: row.get(0)?,
        provider_id: row.get(1)?,
        upstream_model: row.get(2)?,
        currency,
        input_rate_nanos: row.get::<_, i64>(5)?.to_string(),
        output_rate_nanos: row.get::<_, i64>(6)?.to_string(),
        cache_read_rate_nanos: Some(row.get::<_, i64>(7)?.to_string()),
        cache_write_rate_nanos: Some(row.get::<_, i64>(8)?.to_string()),
        input_mode: InputMode::parse(&row.get::<_, String>(4)?).unwrap_or(InputMode::Flat),
        label: row.get(10)?,
        created_at_ms: row.get(11)?,
        revision: row.get(9)?,
        referenced: false,
    })
}

pub fn list_price_rules(
    conn: &Connection,
    provider_id: Option<&str>,
    upstream_model: Option<&str>,
    after_rule_id: Option<&str>,
    limit: i64,
) -> Result<(Vec<PriceRuleDto>, Option<String>), DbError> {
    // Ask for one extra row so "there is another page" is provable rather than
    // guessed. The cursor is the last `price_id` of the page.
    let mut statement = conn.prepare(
        "SELECT price_id, provider_id, upstream_model, currency, input_mode,
                input_rate_nanos, output_rate_nanos, cache_read_rate_nanos,
                cache_write_rate_nanos, revision, label, created_at_ms, updated_at_ms
         FROM stats_model_prices
         WHERE (?1 IS NULL OR provider_id=?1)
           AND (?2 IS NULL OR upstream_model=?2)
           AND (?3 IS NULL OR price_id>?3)
         ORDER BY price_id LIMIT ?4",
    )?;
    let mut rows = statement.query(rusqlite::params![
        provider_id,
        upstream_model,
        after_rule_id,
        limit + 1
    ])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(read_price_rule(row)?);
    }
    let next = if out.len() as i64 > limit {
        out.truncate(limit as usize);
        out.last().map(|rule| rule.rule_id.clone())
    } else {
        None
    };
    Ok((out, next))
}

/// Inserts or updates one current price. Editing or deleting a price is always
/// allowed: recorded history keeps its own snapshot.
pub fn save_price_rule(
    writer: &mut Writer,
    request: &crate::stats::PriceSaveRequest,
    now_ms: i64,
) -> Result<PriceRuleDto, DbError> {
    let conn = &mut writer.conn;
    let currency = Currency::parse(&request.currency)
        .ok_or_else(|| DbError::refused(error(ERR_INVALID, "currency must be USD or CNY")))?;
    let input_mode = InputMode::parse(&request.input_mode)
        .ok_or_else(|| DbError::refused(error(ERR_INVALID, "input_mode is not supported")))?;
    let input_rate = pricing::parse_rate(&request.input_rate_nanos)
        .map_err(|detail| DbError::refused(error(ERR_INVALID, &format!("input rate: {detail}"))))?;
    let output_rate = pricing::parse_rate(&request.output_rate_nanos).map_err(|detail| {
        DbError::refused(error(ERR_INVALID, &format!("output rate: {detail}")))
    })?;
    let optional_rate = |value: Option<&str>| -> Result<i64, DbError> {
        match value.map(str::trim).filter(|text| !text.is_empty()) {
            None => Ok(0),
            // A blank box means the explicit zero default, not "unset".
            Some(text) => pricing::parse_rate(text).map_err(|detail| {
                DbError::refused(error(ERR_INVALID, &format!("cache rate: {detail}")))
            }),
        }
    };
    let cache_read_rate = optional_rate(request.cache_read_rate_nanos.as_deref())?;
    let cache_write_rate = optional_rate(request.cache_write_rate_nanos.as_deref())?;
    if request.provider_id.trim().is_empty() || request.upstream_model.trim().is_empty() {
        return Err(DbError::refused(error(
            ERR_INVALID,
            "provider and model are required",
        )));
    }
    // A blank box means the explicit zero default, so a zero cache rate is a
    // valid configuration (for example free cache reads) and must be accepted.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let price_id = match &request.rule_id {
        Some(price_id) => {
            // The provider+model pair identifies the price, so a save that
            // silently ignored a changed pair would look like an edit while the
            // list kept the old key. Refuse it instead.
            let current: Option<(String, String)> = tx
                .query_row(
                    "SELECT provider_id, upstream_model FROM stats_model_prices WHERE price_id=?1",
                    [price_id.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            if let Some((provider_id, upstream_model)) = current
                && (provider_id != request.provider_id || upstream_model != request.upstream_model)
            {
                return Err(DbError::refused(error(
                    ERR_INVALID,
                    "a price keeps its provider and model; create a new price instead",
                )));
            }
            let changed = tx.execute(
                "UPDATE stats_model_prices SET currency=:currency, input_mode=:mode,
                     input_rate_nanos=:input, output_rate_nanos=:output,
                     cache_read_rate_nanos=:read, cache_write_rate_nanos=:write,
                     label=:label, revision=revision+1, updated_at_ms=:now
                 WHERE price_id=:price_id AND revision=:revision",
                rusqlite::named_params! {
                    ":currency": currency.as_str(),
                    ":mode": input_mode.as_str(),
                    ":input": input_rate,
                    ":output": output_rate,
                    ":read": cache_read_rate,
                    ":write": cache_write_rate,
                    ":label": request.label,
                    ":now": now_ms,
                    ":price_id": price_id,
                    ":revision": request.expected_revision.unwrap_or(0),
                },
            )?;
            if changed != 1 {
                // Telling "no such price" apart from "stale revision" keeps an
                // unknown id from silently turning an edit into a new row.
                let exists: Option<i64> = tx
                    .query_row(
                        "SELECT revision FROM stats_model_prices WHERE price_id=?1",
                        [price_id.as_str()],
                        |row| row.get(0),
                    )
                    .optional()?;
                return Err(DbError::refused(error(
                    if exists.is_some() {
                        ERR_REVISION_CONFLICT
                    } else {
                        crate::stats::dto::ERR_NOT_FOUND
                    },
                    if exists.is_some() {
                        "the price was changed by someone else; reload and retry"
                    } else {
                        "the price no longer exists; reload the list"
                    },
                )));
            }
            price_id.clone()
        }
        None => {
            let price_id = crate::config::token()[..24].to_owned();
            let changed = tx.execute(
                "INSERT INTO stats_model_prices(
                     price_id, provider_id, upstream_model, currency, input_mode,
                     input_rate_nanos, output_rate_nanos, cache_read_rate_nanos,
                     cache_write_rate_nanos, label, revision, created_at_ms, updated_at_ms)
                 VALUES(:price_id,:provider,:model,:currency,:mode,:input,:output,
                     :read,:write,:label,1,:now,:now)
                 ON CONFLICT(provider_id, upstream_model) DO UPDATE SET
                     currency=excluded.currency, input_mode=excluded.input_mode,
                     input_rate_nanos=excluded.input_rate_nanos,
                     output_rate_nanos=excluded.output_rate_nanos,
                     cache_read_rate_nanos=excluded.cache_read_rate_nanos,
                     cache_write_rate_nanos=excluded.cache_write_rate_nanos,
                     label=excluded.label, revision=stats_model_prices.revision+1,
                     updated_at_ms=excluded.updated_at_ms
                 WHERE stats_model_prices.revision=:expected_revision",
                rusqlite::named_params! {
                    ":price_id": price_id,
                    ":provider": request.provider_id,
                    ":model": request.upstream_model,
                    ":currency": currency.as_str(),
                    ":mode": input_mode.as_str(),
                    ":input": input_rate,
                    ":output": output_rate,
                    ":read": cache_read_rate,
                    ":write": cache_write_rate,
                    ":label": request.label,
                    ":now": now_ms,
                    ":expected_revision": request.expected_revision.unwrap_or(0),
                },
            )?;
            // A conflict whose `WHERE` did not match changed no row: reporting
            // success there made the caller believe an edit landed when the
            // database still held the old price.
            if changed == 0 {
                return Err(DbError::refused(error(
                    ERR_REVISION_CONFLICT,
                    "a price already exists for this provider and model; edit it instead",
                )));
            }
            // The upsert may have kept the original id.
            tx.query_row(
                "SELECT price_id FROM stats_model_prices WHERE provider_id=?1 AND upstream_model=?2",
                rusqlite::params![request.provider_id, request.upstream_model],
                |row| row.get(0),
            )?
        }
    };
    tx.commit()?;
    let dto = price_rule_dto(conn, &price_id)?
        .ok_or_else(|| DbError::refused(error(ERR_UNAVAILABLE, "price was not stored")))?;
    // Publish only after the transaction is durable: a failed save must not
    // change what new requests use.
    writer.prices = load_price_cache(conn)?;
    Ok(dto)
}

/// Deletes one current price. Recorded history is untouched: it keeps its own
/// snapshot and never references this row.
pub fn delete_price_rule(
    writer: &mut Writer,
    price_id: &str,
    expected_revision: i64,
) -> Result<(), DbError> {
    let conn = &mut writer.conn;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let changed = tx.execute(
        "DELETE FROM stats_model_prices WHERE price_id=?1 AND revision=?2",
        rusqlite::params![price_id, expected_revision],
    )?;
    if changed != 1 {
        return Err(DbError::refused(error(
            ERR_REVISION_CONFLICT,
            "the price was changed by someone else; reload and retry",
        )));
    }
    tx.commit()?;
    // Recorded history keeps its own snapshot, so deleting a price only changes
    // what *new* requests will use.
    writer.prices = load_price_cache(conn)?;
    Ok(())
}

/// Stores ids whose terminal event could not be written, so the same run can
/// retry them in bounded batches instead of dropping them.
pub fn push_backlog(writer: &mut Writer, ids: &[String], now_ms: i64) -> Result<usize, DbError> {
    let tx = writer
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut stored = 0usize;
    {
        let mut statement = tx.prepare(
            "INSERT INTO stats_recovery_backlog(request_id, created_at_ms) VALUES(?1, ?2)
             ON CONFLICT(request_id) DO NOTHING",
        )?;
        for id in ids {
            stored += statement.execute(rusqlite::params![id, now_ms])?;
        }
    }
    tx.commit()?;
    Ok(stored)
}

/// Number of ids still waiting for a terminal settlement.
pub fn backlog_len(conn: &Connection) -> Result<i64, DbError> {
    Ok(
        conn.query_row("SELECT COUNT(*) FROM stats_recovery_backlog", [], |row| {
            row.get(0)
        })?,
    )
}

/// Progress of the bounded startup repair scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveryScan {
    /// Highest request id already examined.
    pub cursor_id: i64,
    /// Requests above this id were admitted after startup and are never touched.
    pub as_of_id: i64,
}

impl RecoveryScan {
    pub fn new(as_of_id: i64) -> Self {
        Self {
            cursor_id: 0,
            as_of_id,
        }
    }

    pub fn finished(&self) -> bool {
        self.cursor_id >= self.as_of_id
    }
}

/// Repairs the next batch of rows that can never receive a `Finish`.
///
/// One transaction per batch, a stable cursor, and a scope frozen at startup.
/// Whatever a batch cannot repair stays eligible for the next one, so recovery
/// makes real progress without holding the write lock for the whole database.
pub fn scan_recovery_batch(
    writer: &mut Writer,
    scan: &mut RecoveryScan,
    limit: i64,
    now_ms: i64,
) -> Result<(i64, i64), DbError> {
    if scan.finished() {
        return Ok((0, 0));
    }
    let tx = writer
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let ids: Vec<(i64, String)> = {
        let mut statement = tx.prepare(
            "SELECT r.id, r.request_id FROM usage_requests r
             LEFT JOIN usage_costs c ON c.request_id=r.request_id
             WHERE r.id>?1 AND r.id<=?2
               AND (r.outcome='in_flight'
                    OR c.status='pending'
                    OR (c.status='not_applicable' AND c.price_source='manual'
                        AND c.total_cost_nanos=0 AND r.upstream_started IS NULL))
             ORDER BY r.id LIMIT ?3",
        )?;
        let mut rows = statement.query(rusqlite::params![scan.cursor_id, scan.as_of_id, limit])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push((row.get(0)?, row.get(1)?));
        }
        out
    };
    if ids.is_empty() {
        // Nothing left inside the frozen scope.
        scan.cursor_id = scan.as_of_id;
        tx.commit()?;
        return Ok((0, 0));
    }
    let mut repaired = 0i64;
    let mut still: Vec<String> = Vec::new();
    for (_, request_id) in &ids {
        match repair_one(&tx, request_id, now_ms, true) {
            Ok(RepairOutcome::Done) | Ok(RepairOutcome::Missing) => repaired += 1,
            Ok(RepairOutcome::Retry) => still.push(request_id.clone()),
            Err(failure) => {
                eprintln!(
                    "statistics could not repair {request_id}: {}",
                    failure.message
                );
                still.push(request_id.clone());
            }
        }
    }
    // A row this batch could not repair is queued durably *before* the cursor
    // moves past it. Advancing without that let the scan report "finished"
    // while the row was still `pending` and nothing remembered it.
    for id in &still {
        tx.execute(
            "INSERT INTO stats_recovery_backlog(request_id, created_at_ms) VALUES(?1, ?2)
             ON CONFLICT(request_id) DO NOTHING",
            rusqlite::params![id, now_ms],
        )?;
    }
    // The new watermark is computed here but published only once the
    // transaction is durable: a failed commit leaves the scan where it was.
    let last = ids.last().map(|(id, _)| *id).unwrap_or(scan.cursor_id);
    let next_cursor = if ids.len() as i64 >= limit {
        last
    } else {
        scan.as_of_id
    };
    tx.commit()?;
    scan.cursor_id = next_cursor;
    Ok((repaired, scan.as_of_id - last))
}

/// What one backlogged id needs next.
#[derive(Debug, PartialEq, Eq)]
pub enum RepairOutcome {
    /// The request and its cost both have a terminal state.
    Done,
    /// No such request: the id is stale and may leave the backlog.
    Missing,
    /// Still repairable but not repaired; the id stays queued.
    Retry,
}

/// Repairs one queued id: closes a request that can never receive its `Finish`,
/// then settles its cost row from the snapshot the request was admitted with.
///
/// Unlike `settle_orphans`, this also handles a request that is *already*
/// terminal while its cost is still `pending`, which is exactly the shape an
/// earlier release left behind.
fn repair_one(
    tx: &rusqlite::Transaction<'_>,
    request_id: &str,
    now_ms: i64,
    unclean: bool,
) -> Result<RepairOutcome, DbError> {
    let outcome: Option<String> = tx
        .query_row(
            "SELECT outcome FROM usage_requests WHERE request_id=?1",
            [request_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(outcome) = outcome else {
        return Ok(RepairOutcome::Missing);
    };
    if outcome == "in_flight" {
        tx.execute(
            // A row inside the startup scope was cut off by the crash; a row
            // abandoned while running lost its terminal event instead.
            "UPDATE usage_requests SET outcome='interrupted',
                 error_kind=CASE WHEN :unclean=1 THEN 'unclean_shutdown' ELSE 'other' END,
                 ended_at_ms=:now, updated_at_ms=:now,
                 usage_status=CASE WHEN usage_status='complete' THEN usage_status ELSE 'missing' END,
                 usage_reason=CASE WHEN :unclean=1 THEN 'unclean_shutdown' ELSE NULL END,
                 usage_source='none'
             WHERE request_id=:id AND outcome='in_flight'",
            rusqlite::named_params! {
                ":now": now_ms,
                ":id": request_id,
                ":unclean": if unclean { 1 } else { 0 },
            },
        )?;
    }
    settle_abandoned_cost(tx, request_id, now_ms)?;
    // The id only leaves the queue once its cost row is genuinely terminal; a
    // snapshot that still cannot be settled stays queued and is retried.
    let still_pending: i64 = tx.query_row(
        "SELECT COUNT(*) FROM usage_costs WHERE request_id=?1 AND status='pending'",
        [request_id],
        |row| row.get(0),
    )?;
    let open: i64 = tx.query_row(
        "SELECT COUNT(*) FROM usage_requests WHERE request_id=?1 AND outcome='in_flight'",
        [request_id],
        |row| row.get(0),
    )?;
    Ok(if still_pending == 0 && open == 0 {
        RepairOutcome::Done
    } else {
        RepairOutcome::Retry
    })
}

/// Repairs an explicit list of ids with the same classification the durable
/// queue uses. Returns the repaired and stale counts plus the ids that still
/// need another attempt, so the caller never has to guess from row counts.
pub fn repair_ids(
    writer: &mut Writer,
    ids: &[String],
    now_ms: i64,
) -> Result<(i64, i64, Vec<String>), DbError> {
    if ids.is_empty() {
        return Ok((0, 0, Vec::new()));
    }
    let tx = writer
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut done = 0i64;
    let mut missing = 0i64;
    let mut retry = Vec::new();
    for id in ids {
        match repair_one(&tx, id, now_ms, false) {
            Ok(RepairOutcome::Done) => done += 1,
            Ok(RepairOutcome::Missing) => missing += 1,
            Ok(RepairOutcome::Retry) => retry.push(id.clone()),
            Err(failure) => {
                eprintln!("statistics could not repair {id}: {}", failure.message);
                retry.push(id.clone());
            }
        }
    }
    tx.commit()?;
    Ok((done, missing, retry))
}

/// Repairs at most `limit` queued ids. Returns how many finished, how many were
/// stale, and how many are still queued, so the caller knows whether recovery is
/// complete instead of assuming it.
pub fn repair_backlog_batch(
    writer: &mut Writer,
    limit: i64,
    now_ms: i64,
) -> Result<(i64, i64, i64), DbError> {
    let ids: Vec<String> = {
        let mut statement = writer.conn.prepare(
            "SELECT request_id FROM stats_recovery_backlog
             ORDER BY created_at_ms, request_id LIMIT ?1",
        )?;
        let mut rows = statement.query([limit])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(row.get(0)?);
        }
        out
    };
    if ids.is_empty() {
        return Ok((0, 0, 0));
    }
    let tx = writer
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut done = 0i64;
    let mut missing = 0i64;
    let mut retry = Vec::new();
    for id in &ids {
        match repair_one(&tx, id, now_ms, false) {
            Ok(RepairOutcome::Done) => done += 1,
            Ok(RepairOutcome::Missing) => missing += 1,
            Ok(RepairOutcome::Retry) => retry.push(id.clone()),
            Err(failure) => {
                // Leave the transaction untouched for this id and try later; a
                // failed id must never be silently dropped from the queue.
                eprintln!("statistics could not repair {id}: {}", failure.message);
                retry.push(id.clone());
            }
        }
    }
    {
        let mut statement = tx.prepare("DELETE FROM stats_recovery_backlog WHERE request_id=?1")?;
        for id in ids.iter().filter(|id| !retry.contains(id)) {
            statement.execute([id.as_str()])?;
        }
    }
    tx.commit()?;
    let remaining = backlog_len(&writer.conn)?;
    Ok((done, missing, remaining))
}

/// Settles at most `limit` backlogged ids and removes the ones that landed./// Gives a cost row its terminal state when its request can no longer receive a
/// `Finish` (a crash, or a dropped terminal event).
///
/// The stored snapshot decides the price: the current configuration is never
/// consulted. A zero snapshot is a confirmed zero; a manual price with no
/// usable usage stays unknown rather than becoming a fake zero.
fn settle_abandoned_cost(
    tx: &rusqlite::Transaction<'_>,
    request_id: &str,
    now_ms: i64,
) -> Result<bool, DbError> {
    let row = tx
        .query_row(
            "SELECT c.price_source, c.currency, c.input_mode, c.input_rate_nanos,
                    c.output_rate_nanos, c.cache_read_rate_nanos, c.cache_write_rate_nanos,
                    r.upstream_started, r.usage_status, r.input_tokens, r.output_tokens,
                    r.total_tokens, r.cache_read_tokens, r.cache_write_tokens,
                    r.reasoning_tokens, r.input_breakdown, r.input_uncached_tokens
             FROM usage_costs c JOIN usage_requests r ON r.request_id=c.request_id
             WHERE c.request_id=?1
               AND (c.status='pending'
                    OR (c.status='not_applicable' AND c.price_source='manual'
                        AND c.total_cost_nanos=0))",
            [request_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<i64>>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, Option<i64>>(11)?,
                    row.get::<_, Option<i64>>(12)?,
                    row.get::<_, Option<i64>>(13)?,
                    row.get::<_, Option<i64>>(14)?,
                    row.get::<_, String>(15)?,
                    row.get::<_, Option<i64>>(16)?,
                ))
            },
        )
        .optional()?;
    let Some((
        price_source,
        currency,
        mode,
        input_rate,
        output_rate,
        read_rate,
        write_rate,
        upstream_started,
        usage_status,
        input,
        output,
        total,
        cache_read,
        cache_write,
        reasoning,
        breakdown,
        uncached,
    )) = row
    else {
        // Already settled (or never started): nothing to close.
        return Ok(false);
    };
    if price_source == "default_zero" {
        // The unit prices are an explicit zero, so the amount is a confirmed
        // zero no matter whether the upstream was reached.
        write_cost_maybe_correcting(
            tx,
            request_id,
            CostStatus::DefaultZero,
            Some(CostParts::zero()),
            now_ms,
            true,
        )?;
        return Ok(true);
    }
    if price_source == "legacy_unknown" {
        // The old amount is unknown and must not be replaced by an invented one.
        return Ok(false);
    }
    // `upstream_started` is a three-state fact: only an explicit 0 proves the
    // request never reached the upstream. A NULL means "unknown" and must never
    // be read as "not sent", which would record a priced request as free.
    if upstream_started == Some(0) {
        write_cost_maybe_correcting(
            tx,
            request_id,
            CostStatus::NotApplicable,
            Some(CostParts::zero()),
            now_ms,
            true,
        )?;
        return Ok(true);
    }
    let usage = UsageOutcome {
        status: UsageStatus::parse(&usage_status).unwrap_or(UsageStatus::Missing),
        reason: None,
        numbers: crate::stats::collector::UsageNumbers {
            input,
            output,
            total,
            cache_read,
            cache_write,
            reasoning,
        },
        breakdown: InputBreakdown::parse(&breakdown).unwrap_or(InputBreakdown::Unknown),
        uncached,
    };
    let rule = PriceRule {
        rule_id: String::new(),
        currency: Currency::parse(&currency).unwrap_or(Currency::Usd),
        input_mode: InputMode::parse(mode.as_deref().unwrap_or("flat")).unwrap_or(InputMode::Flat),
        input_rate_nanos: input_rate.unwrap_or(0),
        output_rate_nanos: output_rate.unwrap_or(0),
        cache_read_rate_nanos: read_rate,
        cache_write_rate_nanos: write_rate,
    };
    // Reaching this point means the upstream was either reached or is unknown,
    // so a non-zero price with no usable usage stays an unknown amount instead
    // of becoming a fake zero.
    let settlement = pricing::settle(Some(&rule), &usage, true);
    // A row that was wrongly settled as a free `not_applicable` is rewritten
    // from the same snapshot, so the correction never invents a new price.
    write_cost_maybe_correcting(
        tx,
        request_id,
        settlement.status,
        settlement.parts,
        now_ms,
        true,
    )?;
    Ok(true)
}

fn finish_request(
    tx: &rusqlite::Transaction<'_>,
    request_id: &str,
    finish: &FinishRecord,
    now_ms: i64,
) -> Result<(), DbError> {
    let updated = tx.execute(
        "UPDATE usage_requests SET ended_at_ms=:ended, duration_ms=:duration,
             ttfb_ms=:ttfb, first_content_ms=:first_content, upstream_started=:upstream,
             response_model=:response_model, http_status=:http_status, outcome=:outcome,
             error_kind=:error_kind, usage_status=:usage_status, usage_reason=:usage_reason,
             usage_source=:usage_source, input_tokens=:input, output_tokens=:output,
             total_tokens=:total, cache_read_tokens=:cache_read,
             cache_write_tokens=:cache_write, reasoning_tokens=:reasoning,
             input_uncached_tokens=:uncached, input_breakdown=:breakdown,
             updated_at_ms=:now
         WHERE request_id=:request_id AND outcome='in_flight'",
        rusqlite::named_params! {
            ":ended": finish.ended_at_ms,
            ":duration": finish.duration_ms,
            ":ttfb": finish.ttfb_ms,
            ":first_content": finish.first_content_ms,
            ":upstream": if finish.upstream_started { 1 } else { 0 },
            ":response_model": finish.response_model,
            ":http_status": finish.http_status,
            ":outcome": finish.outcome.as_str(),
            ":error_kind": finish.error_kind.map(|kind| kind.as_str()),
            ":usage_status": finish.usage_status.as_str(),
            ":usage_reason": finish.usage_reason.map(|reason| reason.as_str()),
            ":usage_source": finish.usage_source.as_str(),
            ":input": finish.input_tokens,
            ":output": finish.output_tokens,
            ":total": finish.total_tokens,
            ":cache_read": finish.cache_read_tokens,
            ":cache_write": finish.cache_write_tokens,
            ":reasoning": finish.reasoning_tokens,
            ":uncached": finish.input_uncached_tokens,
            ":breakdown": finish.input_breakdown.as_str(),
            ":now": now_ms,
            ":request_id": request_id,
        },
    )?;
    if updated != 1 {
        // A duplicate `Finish` (or a missing `Start`) must not double count and
        // must never overwrite an already settled snapshot or amount.
        return Ok(());
    }
    // The snapshot written at `Start` is the only price source here: the current
    // configuration may have been edited or deleted since.
    let snapshot = tx.query_row(
        "SELECT price_source, currency, input_mode, input_rate_nanos, output_rate_nanos,
                cache_read_rate_nanos, cache_write_rate_nanos
         FROM usage_costs WHERE request_id=?1 AND status='pending'",
        [request_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, Option<i64>>(6)?,
            ))
        },
    )?;
    let (price_source, currency, mode, input_rate, output_rate, read_rate, write_rate) = snapshot;
    let usage = UsageOutcome {
        status: finish.usage_status,
        reason: finish.usage_reason,
        numbers: crate::stats::collector::UsageNumbers {
            input: finish.input_tokens,
            output: finish.output_tokens,
            total: finish.total_tokens,
            cache_read: finish.cache_read_tokens,
            cache_write: finish.cache_write_tokens,
            reasoning: finish.reasoning_tokens,
        },
        breakdown: finish.input_breakdown,
        uncached: finish.input_uncached_tokens,
    };
    let (status, parts) = if price_source == "default_zero" {
        // An explicit zero price is a confirmed zero amount, even when the usage
        // never arrived: nothing is being hidden by recording 0 here.
        if !finish.upstream_started {
            (CostStatus::NotApplicable, Some(CostParts::zero()))
        } else {
            (CostStatus::DefaultZero, Some(CostParts::zero()))
        }
    } else {
        let rule = PriceRule {
            rule_id: String::new(),
            currency: Currency::parse(&currency).unwrap_or(Currency::Usd),
            input_mode: InputMode::parse(mode.as_deref().unwrap_or("flat"))
                .unwrap_or(InputMode::Flat),
            input_rate_nanos: input_rate.ok_or_else(|| {
                DbError::refused(error(ERR_UNAVAILABLE, "price snapshot is incomplete"))
            })?,
            output_rate_nanos: output_rate.ok_or_else(|| {
                DbError::refused(error(ERR_UNAVAILABLE, "price snapshot is incomplete"))
            })?,
            cache_read_rate_nanos: read_rate,
            cache_write_rate_nanos: write_rate,
        };
        let settlement = pricing::settle(Some(&rule), &usage, finish.upstream_started);
        // `settle` returns no parts for the statuses whose amount is genuinely
        // unknown; those must stay NULL rather than become a fake zero.
        (settlement.status, settlement.parts)
    };
    write_cost(tx, request_id, status, parts, now_ms)
}

#[allow(clippy::too_many_arguments)]
fn write_cost(
    tx: &rusqlite::Transaction<'_>,
    request_id: &str,
    status: CostStatus,
    parts: Option<pricing::CostParts>,
    now_ms: i64,
) -> Result<(), DbError> {
    write_cost_maybe_correcting(tx, request_id, status, parts, now_ms, false)
}

/// `allow_correction` lets startup repair overwrite a row an earlier release
/// settled wrongly as a free `not_applicable`; the normal settle path never
/// rewrites a finished row.
fn write_cost_maybe_correcting(
    tx: &rusqlite::Transaction<'_>,
    request_id: &str,
    status: CostStatus,
    parts: Option<pricing::CostParts>,
    now_ms: i64,
    allow_correction: bool,
) -> Result<(), DbError> {
    // A status whose amount is unknown stores NULL, never zero: the schema
    // distinguishes "confirmed zero" from "not known".
    let (input, output, read, write, total) = match &parts {
        Some(parts) => (
            Some(parts.input),
            Some(parts.output),
            Some(parts.cache_read),
            Some(parts.cache_write),
            Some(parts.total),
        ),
        None => (None, None, None, None, None),
    };
    // Only the amounts and the settle status are written here. The unit prices,
    // currency and mode stay exactly as `Start` recorded them, and the row is
    // only updated while it is still `pending`, so a repeated `Finish` can never
    // rewrite a settled snapshot.
    let changed = tx.execute(
        "UPDATE usage_costs SET status=:status,
             input_cost_nanos=:input, output_cost_nanos=:output,
             cache_read_cost_nanos=:read, cache_write_cost_nanos=:write,
             total_cost_nanos=:total, calculated_at_ms=:now
         WHERE request_id=:request_id
           AND (status='pending'
                OR (:correct=1 AND status='not_applicable' AND price_source='manual'
                    AND total_cost_nanos=0))",
        rusqlite::named_params! {
            ":request_id": request_id,
            ":status": status.as_str(),
            ":input": input,
            ":output": output,
            ":read": read,
            ":write": write,
            ":total": total,
            ":now": now_ms,
            ":correct": if allow_correction { 1 } else { 0 },
        },
    )?;
    if changed == 1 {
        return Ok(());
    }
    // No pending row (a `Finish` whose `Start` never landed): store the terminal
    // state with whatever snapshot the caller can prove.
    tx.execute(
        "INSERT INTO usage_costs(request_id, price_source, currency, input_mode,
             input_rate_nanos, output_rate_nanos, cache_read_rate_nanos, cache_write_rate_nanos,
             status, input_cost_nanos, output_cost_nanos, cache_read_cost_nanos,
             cache_write_cost_nanos, total_cost_nanos, pricing_version, calculated_at_ms)
         VALUES(:request_id,'default_zero',(SELECT default_currency FROM stats_settings WHERE singleton=1),
             'flat',0,0,0,0,:status,:input,:output,:read,:write,:total,2,:now)
         ON CONFLICT(request_id) DO NOTHING",
        rusqlite::named_params! {
            ":request_id": request_id,
            ":status": status.as_str(),
            ":input": input,
            ":output": output,
            ":read": read,
            ":write": write,
            ":total": total,
            ":now": now_ms,
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_loss_counters_are_durable_without_a_later_request_batch() {
        let folder = tempfile::tempdir().unwrap();
        let counters = Arc::new(Counters::default());
        let (writer, _) =
            Writer::open(folder.path(), "run-idle-counters", counters.clone()).unwrap();
        counters.admitted.store(3, Ordering::Relaxed);
        counters.dropped.store(2, Ordering::Relaxed);
        counters.write_failures.store(1, Ordering::Relaxed);
        writer.persist_counters(crate::stats::now_ms()).unwrap();
        let reader = Connection::open(folder.path().join("usage.sqlite3")).unwrap();
        let values:(i64,i64,i64)=reader.query_row("SELECT admitted_requests,dropped_requests,write_failures FROM stats_runs WHERE run_id='run-idle-counters'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
        assert_eq!(values, (3, 2, 1));
        assert_eq!(
            reader
                .query_row("SELECT COUNT(*) FROM usage_requests", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    /// Drives the bounded startup scan to completion, exactly as the writer's
    /// recovery probe does between live batches.
    fn run_recovery_scan(writer: &mut Writer, now_ms: i64) -> i64 {
        let Some(mut scan) = writer.scan else {
            return 0;
        };
        let mut rounds = 0;
        let mut repaired = 0i64;
        while !scan.finished() && rounds < 10_000 {
            let (done, _) =
                scan_recovery_batch(writer, &mut scan, crate::stats::RECOVERY_BATCH, now_ms)
                    .unwrap();
            repaired += done;
            rounds += 1;
        }
        writer.scan = Some(scan);
        assert!(scan.finished(), "the scan must terminate");
        repaired
    }
    use crate::stats::dto::{InputBreakdown, Outcome, Source, UsageStatus};
    use crate::stats::event::{Counters, FinishRecord, RequestMeta, StatsEvent};
    use std::sync::Arc;

    fn meta(request_id: &str, run_id: &str) -> RequestMeta {
        RequestMeta {
            request_id: request_id.into(),
            run_id: run_id.to_owned(),
            source: Source::Relay,
            provider_id: "provider-a".into(),
            provider_name: "Provider".into(),
            route_id: "route-0".into(),
            route_name: "Route".into(),
            requested_model: "model-x".into(),
            upstream_model: "model-x".into(),
            upstream_protocol: "openai_chat".into(),
            stream: true,
            started_at_ms: crate::stats::now_ms(),
            price: PriceSnapshot::default_zero("CNY", crate::stats::now_ms()),
        }
    }

    fn finish(
        usage_status: UsageStatus,
        numbers: (Option<i64>, Option<i64>, Option<i64>),
    ) -> FinishRecord {
        let now = crate::stats::now_ms();
        FinishRecord {
            ended_at_ms: now,
            duration_ms: 12,
            ttfb_ms: Some(3),
            first_content_ms: Some(4),
            upstream_started: true,
            response_model: None,
            http_status: Some(200),
            outcome: Outcome::Completed,
            error_kind: None,
            usage_status,
            usage_reason: None,
            usage_source: if usage_status == UsageStatus::Complete {
                crate::stats::dto::UsageSource::Upstream
            } else {
                crate::stats::dto::UsageSource::None
            },
            input_tokens: numbers.0,
            output_tokens: numbers.1,
            total_tokens: numbers.2,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            input_uncached_tokens: None,
            input_breakdown: InputBreakdown::Unknown,
            updated_at_ms: now,
        }
    }

    /// P01: the v1 -> v2 migration must keep every recorded token and amount,
    /// turn v1 `unpriced` into the new explicit zero, and leave no dangling
    /// reference behind.
    #[test]
    fn the_v1_to_v2_migration_preserves_history() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("usage.sqlite3");
        // Build a v1 database by hand, exactly as the previous schema wrote it.
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATION_0001).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO stats_runs(run_id, started_at_ms, stopped_at_ms, stop_reason,
                     admitted_requests, dropped_requests, write_failures, last_persisted_at_ms)
                 VALUES('run-v1', 1000, 1000, 'normal', 3, 0, 0, 1000)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO stats_price_rules(rule_id, provider_id, upstream_model, currency,
                     effective_from_ms, input_rate_nanos, output_rate_nanos, input_mode, label,
                     created_at_ms)
                 VALUES('rule-v1','provider-a','model-x','USD',0,2000000000,8000000000,'flat','v1',900)",
                [],
            )
            .unwrap();
            for (id, status) in [
                ("req-priced", "priced"),
                ("req-unpriced", "unpriced"),
                ("req-incomplete", "usage_incomplete"),
            ] {
                conn.execute(
                    "INSERT INTO usage_requests(request_id, run_id, source, provider_id, route_id,
                         provider_name, route_name, requested_model, upstream_model, started_at_ms,
                         stream, outcome, usage_status, input_tokens, output_tokens, total_tokens,
                         ended_at_ms, duration_ms, usage_source, created_at_ms, updated_at_ms)
                     VALUES(?1,'run-v1','relay','provider-a','route-0','P','R','model-x','model-x',
                         1000, 1, 'completed', 'complete', 1000, 100, 1100, 1000, 0, 'upstream', 1000, 1000)",
                    [id],
                )
                .unwrap();
                let (rule, currency, total): (Option<&str>, Option<&str>, Option<i64>) =
                    match status {
                        "priced" => (Some("rule-v1"), Some("USD"), Some(1_360_000)),
                        _ => (None, None, None),
                    };
                conn.execute(
                    "INSERT INTO usage_costs(request_id, rule_id, status, currency,
                         input_cost_nanos, output_cost_nanos, cache_read_cost_nanos,
                         cache_write_cost_nanos, total_cost_nanos, calculated_at_ms)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,1000)",
                    rusqlite::params![
                        id,
                        rule,
                        status,
                        currency,
                        total.map(|_| 1_000_000i64),
                        total.map(|_| 360_000i64),
                        total.map(|_| 0i64),
                        total.map(|_| 0i64),
                        total,
                    ],
                )
                .unwrap();
            }
            conn.close().unwrap();
        }

        // Migrate, then assert the v2 shape and the preserved history.
        {
            let mut conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            migrate(&mut conn, folder.path()).unwrap();
            let version: i64 = conn
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap();
            // v1 upgrades through v2 to v3 in one pass.
            assert_eq!(version, SCHEMA_VERSION);
            let default_currency: String = conn
                .query_row(
                    "SELECT default_currency FROM stats_settings WHERE singleton=1",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(default_currency, "CNY");
            // The interval rules are replaced by one editable current price.
            let prices: i64 = conn
                .query_row("SELECT COUNT(*) FROM stats_model_prices", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(prices, 1);
            let (mode, input): (String, i64) = conn
                .query_row(
                    "SELECT input_mode, input_rate_nanos FROM stats_model_prices",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(mode, "flat");
            // The stored rates are nano units already; they must not be rescaled.
            assert_eq!(input, 2_000_000_000);

            let row = |id: &str| -> (String, String, Option<i64>, Option<i64>) {
                conn.query_row(
                    "SELECT c.status, c.price_source, c.input_rate_nanos, c.total_cost_nanos
                     FROM usage_costs c WHERE c.request_id=?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap()
            };
            // A priced row keeps its amount and gains the rule's unit prices.
            assert_eq!(
                row("req-priced"),
                (
                    "priced".into(),
                    "manual".into(),
                    Some(2_000_000_000),
                    Some(1_360_000)
                )
            );
            // v1 `unpriced` becomes the new explicit zero.
            assert_eq!(
                row("req-unpriced"),
                (
                    "default_zero".into(),
                    "default_zero".into(),
                    Some(0),
                    Some(0)
                )
            );
            // An incomplete row keeps its status and its unknown amount.
            let incomplete = row("req-incomplete");
            assert_eq!(incomplete.0, "usage_incomplete");
            assert!(incomplete.3.is_none(), "{incomplete:?}");
            // Tokens were never touched by the migration.
            let tokens: (Option<i64>, Option<i64>) = conn
                .query_row(
                    "SELECT input_tokens, total_tokens FROM usage_requests WHERE request_id='req-unpriced'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(tokens, (Some(1000), Some(1100)));
            // No dangling reference survived the table rebuild.
            let mut statement = conn.prepare("PRAGMA foreign_key_check").unwrap();
            assert!(statement.query([]).unwrap().next().unwrap().is_none());
            // The reporting view still resolves against the rebuilt table.
            let counted: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM stats_request_facts WHERE cost_status IS NOT NULL",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(counted, 3);
        }
    }

    /// U01/U02: startup repair must keep `upstream_started` three-state and fix
    /// rows an earlier release left `pending` or wrongly free.
    #[test]
    fn startup_repair_settles_abandoned_costs_from_their_own_snapshot() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("usage.sqlite3");
        {
            let mut conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            migrate(&mut conn, folder.path()).unwrap();
            conn.execute(
                "INSERT INTO stats_runs(run_id, started_at_ms, stopped_at_ms, stop_reason,
                     admitted_requests, dropped_requests, write_failures, last_persisted_at_ms)
                 VALUES('run-old', 1000, 1000, 'unclean', 5, 0, 0, 1000)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO stats_model_prices(price_id, provider_id, upstream_model, currency,
                     input_mode, input_rate_nanos, output_rate_nanos, revision, label,
                     created_at_ms, updated_at_ms)
                 VALUES('price-manual','p1','model-priced','USD','flat',2000000000,8000000000,1,'m',900,900)",
                [],
            )
            .unwrap();
            // (id, outcome, upstream_started, usage_status, cost status, price_source,
            //  total, expected status, expected total)
            #[allow(clippy::type_complexity)]
            let rows: [(&str, &str, Option<i64>, &str, &str, &str, Option<i64>); 5] = [
                // An earlier release never wrote the upstream bit, so it is unknown.
                (
                    "a-unknown",
                    "interrupted",
                    None,
                    "missing",
                    "pending",
                    "manual",
                    None,
                ),
                // Already (wrongly) marked free although the upstream bit is unknown.
                (
                    "b-wrong-free",
                    "interrupted",
                    None,
                    "missing",
                    "not_applicable",
                    "manual",
                    Some(0),
                ),
                // A priced request that provably never started stays free.
                (
                    "c-never-sent",
                    "interrupted",
                    Some(0),
                    "missing",
                    "pending",
                    "manual",
                    None,
                ),
                // No configured price: a confirmed zero.
                (
                    "d-default",
                    "interrupted",
                    None,
                    "missing",
                    "pending",
                    "default_zero",
                    None,
                ),
                // Finished and settled: untouched.
                (
                    "e-done",
                    "completed",
                    Some(1),
                    "complete",
                    "priced",
                    "manual",
                    Some(1_360_000),
                ),
            ];
            for (id, outcome, started, usage, status, source, total) in rows {
                let ended = if outcome == "in_flight" {
                    None
                } else {
                    Some(1000)
                };
                conn.execute(
                    "INSERT INTO usage_requests(request_id, run_id, source, provider_id, route_id,
                         provider_name, route_name, requested_model, upstream_model, started_at_ms,
                         stream, outcome, upstream_started, ended_at_ms, duration_ms, usage_status,
                         usage_source, input_tokens, output_tokens, total_tokens, created_at_ms,
                         updated_at_ms)
                     VALUES(?1,'run-old','relay','p1','r0','P','R','model-priced','model-priced',
                         1000, 1, ?2, ?3, ?4, 0, ?5, 'upstream', 1000, 100, 1100, 1000, 1000)",
                    rusqlite::params![id, outcome, started, ended, usage],
                )
                .unwrap();
                // A `default_zero` row must carry all-zero rates and no source id.
                let (currency, input_rate, output_rate) = if source == "default_zero" {
                    (Some("CNY"), 0i64, 0i64)
                } else {
                    (Some("USD"), 2_000_000_000i64, 8_000_000_000i64)
                };
                // A settled row must balance, an unknown amount is all NULL and
                // a confirmed zero is all zero.
                let (input, output, total): (Option<i64>, Option<i64>, Option<i64>) =
                    if status == "pending" {
                        (None, None, None)
                    } else {
                        match total {
                            Some(0) | None => (Some(0), Some(0), Some(0)),
                            Some(value) => (Some(value - 360_000), Some(360_000), Some(value)),
                        }
                    };
                conn.execute(
                    "INSERT INTO usage_costs(request_id, status, currency, price_source,
                         input_mode, input_rate_nanos, output_rate_nanos,
                         cache_read_rate_nanos, cache_write_rate_nanos, price_snapshot_at_ms,
                         input_cost_nanos, output_cost_nanos, cache_read_cost_nanos,
                         cache_write_cost_nanos, total_cost_nanos, calculated_at_ms,
                         pricing_version)
                     VALUES(?1,?2,?3,?4,'flat',?5,?10,0,0,1000,?6,?7,?9,?9,?8,1000,2)",
                    rusqlite::params![
                        id,
                        status,
                        currency,
                        source,
                        input_rate,
                        input,
                        output,
                        total,
                        input.map(|_| 0i64),
                        output_rate,
                    ],
                )
                .unwrap();
            }
            let counters = Arc::new(Counters::default());
            let (mut writer, recovery) = Writer::open(folder.path(), "run-new", counters).unwrap();
            assert_eq!(recovery.as_of_id, 5, "{recovery:?}");
            assert_eq!(run_recovery_scan(&mut writer, 2000), 4);
            fn read(conn: &Connection, id: &str) -> (String, Option<i64>) {
                conn.query_row(
                    "SELECT status, total_cost_nanos FROM usage_costs WHERE request_id=?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap()
            }
            {
                let conn = &writer.conn;
                // Unknown upstream plus a manual price and no usage: unknown amount.
                assert_eq!(read(conn, "a-unknown"), ("usage_incomplete".into(), None));
                // A row an earlier release wrongly marked free is corrected too.
                assert_eq!(
                    read(conn, "b-wrong-free"),
                    ("usage_incomplete".into(), None)
                );
                // A provable "never sent" stays a confirmed zero.
                assert_eq!(
                    read(conn, "c-never-sent"),
                    ("not_applicable".into(), Some(0))
                );
                // No configured price is still a confirmed zero.
                assert_eq!(read(conn, "d-default"), ("default_zero".into(), Some(0)));
                // Settled history is untouched.
                assert_eq!(read(conn, "e-done"), ("priced".into(), Some(1_360_000)));
            }
            // Repairs are idempotent: a second scan has nothing left to do.
            assert_eq!(run_recovery_scan(&mut writer, 3000), 0);
            {
                let conn = &writer.conn;
                assert_eq!(read(conn, "a-unknown"), ("usage_incomplete".into(), None));
                assert_eq!(read(conn, "e-done"), ("priced".into(), Some(1_360_000)));
                assert!(
                    conn.query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
                        .is_err()
                );
            }
        }
    }

    /// R8-04: a batch that cannot repair a row must queue it before the scan
    /// watermark moves past it, and a failed commit must not advance at all.
    #[test]
    fn a_failed_repair_is_queued_before_the_scan_advances() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("usage.sqlite3");
        {
            let mut conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            migrate(&mut conn, folder.path()).unwrap();
            conn.execute(
                "INSERT INTO stats_runs(run_id, started_at_ms, stopped_at_ms, stop_reason,
                     admitted_requests, dropped_requests, write_failures, last_persisted_at_ms)
                 VALUES('run-r8', 1000, 1000, 'unclean', 4, 0, 0, 1000)",
                [],
            )
            .unwrap();
            for index in 0..4 {
                let id = format!("r8-{index}");
                conn.execute(
                    "INSERT INTO usage_requests(request_id, run_id, source, provider_id,
                         route_id, provider_name, route_name, requested_model, upstream_model,
                         started_at_ms, stream, outcome, ended_at_ms, duration_ms, usage_status,
                         usage_source, created_at_ms, updated_at_ms)
                     VALUES(?1,'run-r8','relay','p1','r0','P','R','m','m',
                         1000, 1, 'interrupted', 1000, 0, 'missing', 'none', 1000, 1000)",
                    [&id],
                )
                .unwrap();
                conn.execute(
                    "INSERT INTO usage_costs(request_id, status, currency, price_source,
                         input_mode, input_rate_nanos, output_rate_nanos,
                         cache_read_rate_nanos, cache_write_rate_nanos, price_snapshot_at_ms,
                         pricing_version, calculated_at_ms)
                     VALUES(?1,'pending','CNY','default_zero','flat',0,0,0,0,1000,2,1000)",
                    [&id],
                )
                .unwrap();
            }
        }
        let counters = Arc::new(Counters::default());
        let (mut writer, recovery) = Writer::open(folder.path(), "run-new", counters).unwrap();
        assert_eq!(recovery.as_of_id, 4);
        // A trigger refuses the cost update, exactly like a locked table.
        writer
            .conn
            .execute_batch(
                "CREATE TRIGGER refuse_cost BEFORE UPDATE OF status ON usage_costs
                 BEGIN SELECT RAISE(ABORT, 'cost writes refused'); END;",
            )
            .unwrap();
        let mut scan = RecoveryScan::new(recovery.as_of_id);
        let (repaired, _) = scan_recovery_batch(&mut writer, &mut scan, 2, 2000).unwrap();
        assert_eq!(repaired, 0, "nothing could be repaired");
        // The two rows this batch touched are queued, not forgotten.
        let queued: i64 = writer
            .conn
            .query_row("SELECT COUNT(*) FROM stats_recovery_backlog", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(queued, 2, "an unrepairable row must be queued durably");
        let pending: i64 = writer
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_costs WHERE status='pending'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 4, "the refused rows stay pending");
        // The scan cannot claim to be finished while rows still owe a state.
        let scan_done = scan.finished();
        let backlog = backlog_len(&writer.conn).unwrap();
        assert!(
            !scan_done || backlog > 0,
            "a finished scan with pending rows and an empty queue would be a lie"
        );
        // Once the fault is gone the same run finishes the job with no new
        // traffic: the scan walks the rest and the queue drains.
        writer
            .conn
            .execute_batch("DROP TRIGGER refuse_cost")
            .unwrap();
        let mut rounds = 0;
        while (!scan.finished() || backlog_len(&writer.conn).unwrap() > 0) && rounds < 50 {
            if !scan.finished() {
                let _ = scan_recovery_batch(&mut writer, &mut scan, 2, 3000).unwrap();
            }
            if backlog_len(&writer.conn).unwrap() > 0 {
                repair_backlog_batch(&mut writer, 256, 3000).unwrap();
            }
            rounds += 1;
        }
        assert!(
            scan.finished(),
            "the scan must finish once the fault is gone"
        );
        assert_eq!(backlog_len(&writer.conn).unwrap(), 0);
        let pending: i64 = writer
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_costs WHERE status='pending'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 0, "the queued rows must really be repaired");
    }

    /// S02: a backlog larger than one batch must be *fully* repaired at
    /// startup. Taking only the first 256 rows and clearing the queue made the
    /// database look healthy while 944 costs stayed `pending` forever.
    #[test]
    fn a_large_legacy_backlog_is_fully_repaired_at_startup() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("usage.sqlite3");
        {
            let mut conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            migrate(&mut conn, folder.path()).unwrap();
            conn.execute(
                "INSERT INTO stats_runs(run_id, started_at_ms, stopped_at_ms, stop_reason,
                     admitted_requests, dropped_requests, write_failures, last_persisted_at_ms)
                 VALUES('run-legacy', 1000, 1000, 'unclean', 20000, 0, 0, 1000)",
                [],
            )
            .unwrap();
            conn.execute_batch("BEGIN").unwrap();
            // Well past any single-pass batch count: an earlier release repaired
            // 16640 of these, left 3360 `pending` and still reported `ready`.
            for index in 0..20_000 {
                let id = format!("legacy-{index}");
                conn.execute(
                    "INSERT INTO usage_requests(request_id, run_id, source, provider_id,
                         route_id, provider_name, route_name, requested_model, upstream_model,
                         started_at_ms, stream, outcome, ended_at_ms, duration_ms, usage_status,
                         usage_source, created_at_ms, updated_at_ms)
                     VALUES(?1,'run-legacy','relay','p1','r0','P','R','m','m',
                         1000, 1, 'interrupted', 1000, 0, 'missing', 'none', 1000, 1000)",
                    [&id],
                )
                .unwrap();
                // A terminal request whose cost never settled: exactly the shape
                // an earlier release left behind.
                conn.execute(
                    "INSERT INTO usage_costs(request_id, status, currency, price_source,
                         input_mode, input_rate_nanos, output_rate_nanos,
                         cache_read_rate_nanos, cache_write_rate_nanos, price_snapshot_at_ms,
                         pricing_version, calculated_at_ms)
                     VALUES(?1,'pending','CNY','default_zero','flat',0,0,0,0,1000,2,1000)",
                    [&id],
                )
                .unwrap();
            }
            conn.execute_batch("COMMIT").unwrap();
            let counters = Arc::new(Counters::default());
            let (mut writer, recovery) = Writer::open(folder.path(), "run-new", counters).unwrap();
            assert_eq!(recovery.as_of_id, 20_000, "{recovery:?}");
            let repaired = run_recovery_scan(&mut writer, 2000);
            assert!(repaired >= 20_000, "only {repaired} of 20000 were repaired");
            let conn = &writer.conn;
            let _ = &conn;
            let pending: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM usage_costs WHERE status='pending'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(pending, 0, "every legacy pending cost must be settled");
            let queued: i64 = conn
                .query_row("SELECT COUNT(*) FROM stats_recovery_backlog", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(queued, 0, "the queue must be empty when the scan finished");
            // Everything is a confirmed zero: the snapshot had no price.
            let zeros: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM usage_costs WHERE status='default_zero'
                       AND total_cost_nanos=0",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(zeros, 20_000);
            assert!(
                writer.scan.is_some_and(|scan| scan.finished()),
                "a finished scan is what makes `ready` honest"
            );
        }
    }

    /// S01: an existing v2 database must upgrade by running only the missing
    /// step. Replaying 0002 made `migrate` fail with "table stats_model_prices
    /// already exists" and left collection disabled.
    #[test]
    fn every_schema_generation_upgrades_in_place_and_is_idempotent() {
        for start in 0..=3i64 {
            let folder = tempfile::tempdir().unwrap();
            let path = folder.path().join("usage.sqlite3");
            {
                let mut conn = Connection::open(&path).unwrap();
                if start >= 1 {
                    conn.execute_batch(MIGRATION_0001).unwrap();
                    conn.pragma_update(None, "user_version", 1).unwrap();
                }
                if start >= 2 {
                    // Build a genuine v2 database exactly as the released
                    // migration wrote it.
                    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
                    let tx = conn
                        .transaction_with_behavior(TransactionBehavior::Immediate)
                        .unwrap();
                    tx.execute_batch(MIGRATION_0002).unwrap();
                    tx.pragma_update(None, "user_version", 2).unwrap();
                    tx.commit().unwrap();
                    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
                }
                if start >= 3 {
                    apply_step(&mut conn, 3, MIGRATION_0003).unwrap();
                }
            }
            let mut conn = Connection::open(&path).unwrap();
            migrate(&mut conn, folder.path())
                .unwrap_or_else(|failure| panic!("v{start} did not upgrade: {}", failure.message));
            let version: i64 = conn
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap();
            assert_eq!(version, SCHEMA_VERSION, "started at v{start}");
            // Every migration step is idempotent: a second start is a no-op.
            migrate(&mut conn, folder.path()).unwrap();
            let again: i64 = conn
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap();
            assert_eq!(again, SCHEMA_VERSION);
            // The v3 step exists and the v2 tables survived untouched.
            let backlog: i64 = conn
                .query_row("SELECT COUNT(*) FROM stats_recovery_backlog", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(backlog, 0);
            let prices: i64 = conn
                .query_row("SELECT COUNT(*) FROM stats_model_prices", [], |row| {
                    row.get(0)
                })
                .unwrap();
            let costs: i64 = conn
                .query_row("SELECT COUNT(*) FROM usage_costs", [], |row| row.get(0))
                .unwrap();
            assert_eq!((prices, costs), (0, 0));
        }
    }

    #[test]
    fn v3_protocol_migration_preserves_historical_unit_prices_and_total() {
        let folder = tempfile::tempdir().unwrap();
        let mut conn = open_connection(&folder.path().join("usage.sqlite3"), false).unwrap();
        conn.execute_batch(MIGRATION_0001).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
        apply_step(&mut conn, 2, MIGRATION_0002).unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        apply_step(&mut conn, 3, MIGRATION_0003).unwrap();
        start_run(&mut conn, "run-v3", 1000, true).unwrap();
        conn.execute_batch("INSERT INTO usage_requests(request_id,run_id,source,provider_id,provider_name,route_id,route_name,requested_model,upstream_model,started_at_ms,ended_at_ms,duration_ms,stream,upstream_started,outcome,usage_status,usage_source,input_tokens,output_tokens,total_tokens,created_at_ms,updated_at_ms)
            VALUES('historical-v3','run-v3','relay','provider','Provider','route','Route','alias','actual-model',1000,1001,1,0,1,'completed','complete','upstream',10,5,15,1000,1001);
            INSERT INTO usage_costs(request_id,source_price_id,source_price_revision,price_source,price_snapshot_at_ms,currency,input_mode,input_rate_nanos,output_rate_nanos,cache_read_rate_nanos,cache_write_rate_nanos,status,input_cost_nanos,output_cost_nanos,cache_read_cost_nanos,cache_write_cost_nanos,total_cost_nanos,pricing_version,calculated_at_ms)
            VALUES('historical-v3','deleted-old-price',3,'manual',1000,'USD','flat',2000000000,8000000000,0,0,'priced',20000,40000,0,0,60000,2,1001);").unwrap();
        migrate(&mut conn, folder.path()).unwrap();
        let values:(String,String,i64,i64,i64)=conn.query_row("SELECT r.upstream_protocol,c.currency,c.input_rate_nanos,c.output_rate_nanos,c.total_cost_nanos FROM usage_requests r JOIN usage_costs c USING(request_id)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).unwrap();
        assert_eq!(
            values,
            (
                "openai_chat".into(),
                "USD".into(),
                2_000_000_000,
                8_000_000_000,
                60_000
            )
        );
        let detail = crate::stats::query::request_detail(&conn, "historical-v3").unwrap();
        assert_eq!(detail.row.upstream_protocol, "openai_chat");
        assert_eq!(detail.parser_version, 1);
        assert_eq!(detail.created_at_ms, 1000);
        assert_eq!(detail.updated_at_ms, 1001);
    }

    /// U05: a tiny budget must bound the whole attempt, including the SQLite
    /// lock wait. Leaving `busy_timeout` at its default let a 1 ms budget take
    /// hundreds of milliseconds.
    #[test]
    fn a_one_millisecond_budget_is_not_multiplied_by_the_lock_wait() {
        let folder = tempfile::tempdir().unwrap();
        let counters = Arc::new(Counters::default());
        let (mut writer, _recovery) = Writer::open(folder.path(), "run-budget", counters).unwrap();
        let blocker = Connection::open(folder.path().join("usage.sqlite3")).unwrap();
        blocker.busy_timeout(Duration::from_millis(50)).unwrap();
        blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
        let mut batch = Vec::new();
        for index in 0..8 {
            batch.push(StatsEvent::Start(Box::new(meta(
                &format!("budget-{index}"),
                &writer.run_id,
            ))));
        }
        let started = Instant::now();
        let outcome = writer.apply_batch(&batch, &WriteBudget::new(Duration::from_millis(1)));
        let elapsed = started.elapsed();
        assert!(outcome.is_err(), "a held lock cannot succeed");
        assert!(
            elapsed < Duration::from_millis(150),
            "a 1 ms budget took {elapsed:?}"
        );
        blocker.execute_batch("ROLLBACK").unwrap();
    }

    /// U05: ids that cannot be settled now must survive in the durable backlog
    /// and be retried by the same run, in bounded batches.
    #[test]
    fn the_recovery_backlog_is_durable_bounded_and_idempotent() {
        let folder = tempfile::tempdir().unwrap();
        let counters = Arc::new(Counters::default());
        let (mut writer, _recovery) = Writer::open(folder.path(), "run-backlog", counters).unwrap();
        let run_id = writer.run_id.clone();
        let now = crate::stats::now_ms();
        let mut batch = Vec::new();
        for index in 0..600 {
            batch.push(StatsEvent::Start(Box::new(meta(
                &format!("backlog-{index}"),
                &run_id,
            ))));
        }
        for chunk in batch.chunks(64) {
            writer
                .apply_batch(chunk, &WriteBudget::new(BATCH_BUDGET))
                .expect("fixture rows must land");
        }
        // The terminal timestamp must not precede any request start.
        let settle_at = crate::stats::now_ms() + 10_000;
        let ids: Vec<String> = (0..600).map(|index| format!("backlog-{index}")).collect();
        assert_eq!(push_backlog(&mut writer, &ids, now).unwrap(), 600);
        // Re-queueing is harmless.
        assert_eq!(push_backlog(&mut writer, &ids, now).unwrap(), 0);
        assert_eq!(backlog_len(&writer.conn).unwrap(), 600);
        // Bounded batches: one call repairs at most the given limit.
        let (done, missing, remaining) = repair_backlog_batch(&mut writer, 256, settle_at).unwrap();
        assert_eq!((done, missing), (256, 0));
        assert_eq!(remaining, 344);
        let open: i64 = writer
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_requests WHERE outcome='in_flight'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(open, 344, "every drained request must be terminal");
        let (done, _, remaining) = repair_backlog_batch(&mut writer, 256, settle_at).unwrap();
        assert_eq!((done, remaining), (256, 88));
        let (done, _, remaining) = repair_backlog_batch(&mut writer, 256, settle_at).unwrap();
        assert_eq!((done, remaining), (88, 0));
        assert_eq!(backlog_len(&writer.conn).unwrap(), 0);
        // S03: an id with no request row (both `Start` and `Finish` were lost)
        // leaves the queue without inventing a request to fill the hole.
        let _ = now;
        assert_eq!(
            push_backlog(&mut writer, &["never-existed".to_owned()], now).unwrap(),
            1
        );
        let before: i64 = writer
            .conn
            .query_row("SELECT COUNT(*) FROM usage_requests", [], |row| row.get(0))
            .unwrap();
        let (done, missing, remaining) = repair_backlog_batch(&mut writer, 256, settle_at).unwrap();
        assert_eq!((done, missing, remaining), (0, 1, 0));
        let after: i64 = writer
            .conn
            .query_row("SELECT COUNT(*) FROM usage_requests", [], |row| row.get(0))
            .unwrap();
        assert_eq!(before, after, "a missing id must not fabricate a request");
        // An empty backlog is a no-op, and a repeat is idempotent.
        assert_eq!(
            repair_backlog_batch(&mut writer, 256, settle_at).unwrap(),
            (0, 0, 0)
        );
        let open: i64 = writer
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_requests WHERE outcome='in_flight'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(open, 0);
    }

    /// T01: a sweep must delete exactly the eligible rows, across as many
    /// batches as it takes, and never a row the preview did not count.
    #[test]
    fn a_sweep_is_bounded_but_completes_and_respects_its_cutoff() {
        let folder = tempfile::tempdir().unwrap();
        let counters = Arc::new(Counters::default());
        let (mut writer, _recovery) = Writer::open(folder.path(), "run-sweep", counters).unwrap();
        let run_id = writer.run_id.clone();
        // Every fixture row is in the past; only the cutoff separates the two
        // groups.
        let base = crate::stats::now_ms() - 10_000_000;
        // 1200 finished rows before the cutoff, 40 after it, plus 5 in flight.
        let mut batch = Vec::new();
        for index in 0..1240i64 {
            let mut meta = meta(&format!("sweep-{index}"), &run_id);
            meta.started_at_ms = if index < 1200 {
                base + index
            } else {
                base + 6_000_000 + index
            };
            let mut finish = finish(UsageStatus::Complete, (Some(4), Some(1), Some(5)));
            finish.ended_at_ms = meta.started_at_ms + 10;
            finish.updated_at_ms = meta.started_at_ms + 10;
            batch.push(StatsEvent::Start(Box::new(meta)));
            batch.push(StatsEvent::Finish {
                request_id: format!("sweep-{index}"),
                finish: Box::new(finish),
            });
        }
        for index in 0..5i64 {
            let mut meta = meta(&format!("live-{index}"), &run_id);
            meta.started_at_ms = base + index;
            batch.push(StatsEvent::Start(Box::new(meta)));
        }
        for chunk in batch.chunks(64) {
            writer
                .apply_batch(chunk, &WriteBudget::new(BATCH_BUDGET))
                .expect("fixture rows must land");
        }
        let count = |writer: &Writer, clause: &str| -> i64 {
            writer
                .conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM usage_requests WHERE {clause}"),
                    [],
                    |row| row.get(0),
                )
                .unwrap()
        };
        assert_eq!(count(&writer, "outcome<>'in_flight'"), 1240);
        assert_eq!(count(&writer, "outcome='in_flight'"), 5);

        let cutoff = base + 5_000_000;
        let mut task =
            Maintenance::new(1, MaintenanceKind::Clear, cutoff, i64::MAX, None, base, 500);
        let mut rounds = 0;
        loop {
            let (deleted, more) = writer.delete_finished(&mut task).unwrap();
            assert!(deleted <= 500, "a batch must stay bounded: {deleted}");
            rounds += 1;
            if !more {
                break;
            }
            assert!(rounds < 20, "the sweep must converge");
        }
        // 1200 eligible rows in three bounded batches, and nothing else.
        assert_eq!(task.affected, 1200, "affected_rows must match reality");
        assert_eq!(rounds, 3, "three batches of at most 500");
        assert_eq!(count(&writer, "outcome<>'in_flight'"), 40);
        assert_eq!(count(&writer, "outcome='in_flight'"), 5);
        assert!(
            count(&writer, "ended_at_ms IS NOT NULL AND started_at_ms>=0") >= 40,
            "rows after the cutoff must survive"
        );
        // Cost rows cascade with their request.
        let orphan_costs: i64 = writer
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_costs c
                 LEFT JOIN usage_requests r ON r.request_id=c.request_id
                 WHERE r.request_id IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(orphan_costs, 0);
        // A second sweep with the same cutoff has nothing left to do.
        let mut again =
            Maintenance::new(2, MaintenanceKind::Clear, cutoff, i64::MAX, None, base, 500);
        assert_eq!(writer.delete_finished(&mut again).unwrap(), (0, false));
    }

    /// R05: a held write lock must fail one batch inside its budget instead of
    /// multiplying the lock wait by the batch size.
    #[test]
    fn a_held_lock_fails_one_batch_within_the_budget() {
        let folder = tempfile::tempdir().unwrap();
        let counters = Arc::new(Counters::default());
        let (mut writer, _recovery) = Writer::open(folder.path(), "run-lock", counters).unwrap();
        let run_id = writer.run_id.clone();
        let batch: Vec<StatsEvent> = (0..64)
            .map(|index| StatsEvent::Start(Box::new(meta(&format!("req-{index}"), &run_id))))
            .collect();
        let blocker = rusqlite::Connection::open(folder.path().join("usage.sqlite3")).unwrap();
        blocker.busy_timeout(Duration::from_millis(50)).unwrap();
        blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
        let started = Instant::now();
        let budget = WriteBudget::new(BATCH_BUDGET);
        let failure = writer
            .apply_batch(&batch, &budget)
            .expect_err("a held lock must be reported");
        let elapsed = started.elapsed();
        assert!(
            is_transient(&failure),
            "a lock must stay transient, got {failure:?}"
        );
        assert!(
            elapsed < BATCH_BUDGET + Duration::from_millis(750),
            "one batch took {elapsed:?}, which is outside its budget"
        );
        blocker.execute_batch("ROLLBACK").unwrap();
        // The batch is retryable once the lock is gone.
        let outcome = writer
            .apply_batch(&batch, &WriteBudget::new(BATCH_BUDGET))
            .expect("the retry must succeed");
        assert!(outcome.is_complete(), "{outcome:?}");
    }

    /// R05: overflowed orphan ids are settled rather than silently dropped.
    #[test]
    fn orphan_settlement_covers_more_than_the_backlog_limit() {
        let folder = tempfile::tempdir().unwrap();
        let counters = Arc::new(Counters::default());
        let (mut writer, _recovery) = Writer::open(folder.path(), "run-orphan", counters).unwrap();
        let run_id = writer.run_id.clone();
        let ids: Vec<String> = (0..crate::stats::MAX_ORPHANS + 120)
            .map(|index| format!("orphan-{index}"))
            .collect();
        let batch: Vec<StatsEvent> = ids
            .iter()
            .map(|id| StatsEvent::Start(Box::new(meta(id, &run_id))))
            .collect();
        writer
            .apply_batch(&batch, &WriteBudget::new(BATCH_BUDGET))
            .expect("starts must land");
        let settled = writer.settle_orphans(&ids, crate::stats::now_ms()).unwrap();
        assert_eq!(
            settled,
            ids.len(),
            "every orphan must reach a terminal state"
        );
        let remaining: i64 = writer
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_requests WHERE outcome='in_flight'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0);
    }

    /// A fast request can put `Start` and `Finish` in the same batch; the
    /// schema requires `updated_at_ms >= created_at_ms`, so `created_at_ms` must
    /// be the request's own start time rather than the batch time.
    #[test]
    fn start_and_finish_in_one_batch_keep_the_timestamp_order() {
        let folder = tempfile::tempdir().unwrap();
        let counters = Arc::new(Counters::default());
        let (mut writer, _recovery) = Writer::open(folder.path(), "run-test", counters).unwrap();
        let run_id = writer.run_id.clone();
        let mut meta = meta("req-fast", &run_id);
        // The request started before the batch is written.
        meta.started_at_ms = crate::stats::now_ms() - 5;
        let mut finish = finish(UsageStatus::Complete, (Some(4), Some(1), Some(5)));
        finish.ended_at_ms = meta.started_at_ms + 1;
        finish.updated_at_ms = meta.started_at_ms + 1;
        let batch = vec![
            StatsEvent::Start(Box::new(meta)),
            StatsEvent::Finish {
                request_id: "req-fast".into(),
                finish: Box::new(finish),
            },
        ];
        let outcome = writer
            .apply_batch(&batch, &WriteBudget::new(BATCH_BUDGET))
            .expect("batch must succeed");
        assert!(outcome.is_complete(), "{outcome:?}");
        let row: (String, i64, i64, Option<i64>) = writer
            .conn
            .query_row(
                "SELECT outcome, created_at_ms, updated_at_ms, total_tokens FROM usage_requests
                 WHERE request_id='req-fast'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(row.0, "completed");
        assert!(
            row.2 >= row.1,
            "updated_at_ms must not precede created_at_ms"
        );
        assert_eq!(row.3, Some(5));
    }

    /// F01: a request whose usage was rejected must not roll back the batch that
    /// also carries a healthy request.
    #[test]
    fn an_invalid_usage_never_rolls_back_the_whole_batch() {
        let folder = tempfile::tempdir().unwrap();
        let counters = Arc::new(Counters::default());
        let (mut writer, _recovery) = Writer::open(folder.path(), "run-test", counters).unwrap();
        let run_id = writer.run_id.clone();
        let batch = vec![
            StatsEvent::Start(Box::new(meta("req-invalid", &run_id))),
            StatsEvent::Start(Box::new(meta("req-healthy", &run_id))),
            StatsEvent::Finish {
                request_id: "req-invalid".into(),
                finish: Box::new(finish(UsageStatus::Invalid, (None, None, None))),
            },
            StatsEvent::Finish {
                request_id: "req-healthy".into(),
                finish: Box::new(finish(
                    UsageStatus::Complete,
                    (Some(1000), Some(100), Some(1100)),
                )),
            },
        ];
        writer
            .apply_batch(&batch, &WriteBudget::new(BATCH_BUDGET))
            .expect("batch must succeed");
        let total: i64 = writer
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_requests WHERE outcome<>'in_flight'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(total, 2, "both terminal rows must be persisted");
        let invalid: (String, Option<i64>) = writer
            .conn
            .query_row(
                "SELECT usage_status, total_tokens FROM usage_requests WHERE request_id='req-invalid'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(invalid.0, "invalid");
        assert_eq!(invalid.1, None, "an invalid usage must not store numbers");
        let healthy: Option<i64> = writer
            .conn
            .query_row(
                "SELECT total_tokens FROM usage_requests WHERE request_id='req-healthy'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(healthy, Some(1100));
    }
}

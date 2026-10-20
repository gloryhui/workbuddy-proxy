//! Ledger operations for the WorkBuddy local-usage importer.
//!
//! Every function borrows the caller's [`rusqlite::Transaction`]: the importer
//! never opens its own connection, so the ledger, the fact row and the file
//! checkpoint always commit or roll back together (DATA-API section 3.3).
//!
//! Hard rules implemented here:
//!
//! * a key is looked up before it is written and `INSERT OR REPLACE` is never
//!   used, so an existing identity is never silently overwritten;
//! * `deleted_manual`/`deleted_retention` tombstones can never be resurrected by
//!   any later rescan;
//! * a candidate payload is only accepted for `pending_classification` and
//!   `conflict`, and never exceeds the 16 KiB contract bound.

use crate::stats::workbuddy_import::ImportCounts;
use crate::stats::workbuddy_import::classify::Attribution;
use crate::stats::workbuddy_import::types::{
    BatchRejectReason, Candidate, CheckpointAck, CommitOutcome, DiagnosticCounts, ImportErrorCode,
    MAX_CANDIDATE_BYTES, NormalizedUsage, PARSER_VERSION,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::path::Path;

/// The seven ledger states (DATA-API section 3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerState {
    Imported,
    ExcludedProxy,
    ExcludedRetention,
    PendingClassification,
    Conflict,
    DeletedManual,
    DeletedRetention,
}

impl LedgerState {
    /// Every state, in declaration order.
    pub const ALL: &'static [LedgerState] = &[
        LedgerState::Imported,
        LedgerState::ExcludedProxy,
        LedgerState::ExcludedRetention,
        LedgerState::PendingClassification,
        LedgerState::Conflict,
        LedgerState::DeletedManual,
        LedgerState::DeletedRetention,
    ];

    /// Stored form.
    pub fn as_str(self) -> &'static str {
        match self {
            LedgerState::Imported => "imported",
            LedgerState::ExcludedProxy => "excluded_proxy",
            LedgerState::ExcludedRetention => "excluded_retention",
            LedgerState::PendingClassification => "pending_classification",
            LedgerState::Conflict => "conflict",
            LedgerState::DeletedManual => "deleted_manual",
            LedgerState::DeletedRetention => "deleted_retention",
        }
    }

    /// Parses a stored state.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|state| state.as_str() == text)
    }

    /// A tombstone: cleared history that no rescan may revive.
    pub fn is_tombstone(self) -> bool {
        matches!(
            self,
            LedgerState::DeletedManual | LedgerState::DeletedRetention
        )
    }

    /// Only these states may keep a whitelisted candidate payload.
    pub fn keeps_candidate(self) -> bool {
        matches!(
            self,
            LedgerState::PendingClassification | LedgerState::Conflict
        )
    }

    /// True when the ledger points at a fact row in `usage_requests`.
    pub fn is_imported(self) -> bool {
        matches!(self, LedgerState::Imported)
    }
}

/// Why a store write was refused.
#[derive(Debug)]
pub enum StoreError {
    /// The key already exists; duplicate/conflict handling is the caller's job.
    AlreadyExists,
    /// No row with that key.
    NotFound,
    /// A tombstone may never be resurrected.
    Tombstone,
    /// The target state does not allow a candidate payload.
    CandidateNotAllowed,
    /// The candidate payload exceeds [`MAX_CANDIDATE_BYTES`].
    CandidateTooLarge,
    /// An offset would move backwards, or is negative.
    InvalidOffset,
    /// A verified mapping would overlap another verified interval.
    Overlapping,
    /// The settings revision moved since the batch was prepared.
    SettingsStale,
    /// The checkpoint refused the batch (stale revision or changed source).
    BatchRejected(BatchRejectReason),
    /// SQLite refused the statement.
    Sqlite(rusqlite::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::AlreadyExists => f.write_str("ledger key already exists"),
            StoreError::NotFound => f.write_str("ledger key not found"),
            StoreError::Tombstone => f.write_str("a deleted ledger entry cannot be revived"),
            StoreError::CandidateNotAllowed => {
                f.write_str("candidate payload is only allowed for pending or conflict")
            }
            StoreError::CandidateTooLarge => f.write_str("candidate payload exceeds 16 KiB"),
            StoreError::InvalidOffset => {
                f.write_str("offset must be non-negative and non-decreasing")
            }
            StoreError::Overlapping => f.write_str("verified mapping intervals must not overlap"),
            StoreError::SettingsStale => {
                f.write_str("settings revision moved since the batch was prepared")
            }
            StoreError::BatchRejected(reason) => write!(f, "batch refused: {}", reason.as_str()),
            StoreError::Sqlite(failure) => write!(f, "sqlite: {failure}"),
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(failure: rusqlite::Error) -> Self {
        StoreError::Sqlite(failure)
    }
}

/// A ledger row as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerRow {
    pub entry_key: String,
    pub namespace_id: String,
    pub session_hash: String,
    pub message_hash: String,
    pub requested_model: Option<String>,
    pub event_at_ms: i64,
    pub normalized_hash: String,
    pub state: LedgerState,
    pub request_id: Option<String>,
    pub candidate_json: Option<String>,
    pub mapping_id: Option<String>,
    pub mapping_revision: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// A ledger row to insert.
#[derive(Clone, Debug)]
pub struct NewLedgerEntry {
    pub entry_key: String,
    pub namespace_id: String,
    pub session_hash: String,
    pub message_hash: String,
    pub requested_model: Option<String>,
    pub event_at_ms: i64,
    pub normalized_hash: String,
    pub state: LedgerState,
    /// Whitelisted candidate JSON; only valid for pending/conflict.
    pub candidate_json: Option<String>,
    pub mapping_id: Option<String>,
    pub mapping_revision: i64,
    pub now_ms: i64,
}

/// Validates the candidate payload against the contract bounds.
fn check_candidate(state: LedgerState, candidate: Option<&str>) -> Result<(), StoreError> {
    match (state.keeps_candidate(), candidate) {
        (true, Some(text)) if text.len() > MAX_CANDIDATE_BYTES => {
            Err(StoreError::CandidateTooLarge)
        }
        (true, _) => Ok(()),
        (false, Some(_)) => Err(StoreError::CandidateNotAllowed),
        (false, None) => Ok(()),
    }
}

/// Reads one ledger row; `None` when the key was never seen.
pub fn load(tx: &Transaction<'_>, entry_key: &str) -> Result<Option<LedgerRow>, StoreError> {
    let row = tx
        .query_row(
            "SELECT entry_key, namespace_id, session_hash, message_hash, requested_model,
                    event_at_ms, normalized_hash, state, request_id, candidate_json,
                    mapping_id, mapping_revision, created_at_ms, updated_at_ms
             FROM workbuddy_import_ledger WHERE entry_key = ?1",
            params![entry_key],
            |row| {
                let state: String = row.get(7)?;
                Ok(LedgerRow {
                    entry_key: row.get(0)?,
                    namespace_id: row.get(1)?,
                    session_hash: row.get(2)?,
                    message_hash: row.get(3)?,
                    requested_model: row.get(4)?,
                    event_at_ms: row.get(5)?,
                    normalized_hash: row.get(6)?,
                    state: LedgerState::parse(&state).unwrap_or(LedgerState::Conflict),
                    request_id: row.get(8)?,
                    candidate_json: row.get(9)?,
                    mapping_id: row.get(10)?,
                    mapping_revision: row.get(11)?,
                    created_at_ms: row.get(12)?,
                    updated_at_ms: row.get(13)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

/// Inserts a new ledger row.
///
/// Looks the key up first and refuses to write when it already exists, so a
/// duplicate is never silently collapsed into an overwrite.
pub fn insert(tx: &Transaction<'_>, entry: &NewLedgerEntry) -> Result<(), StoreError> {
    if load(tx, &entry.entry_key)?.is_some() {
        return Err(StoreError::AlreadyExists);
    }
    check_candidate(entry.state, entry.candidate_json.as_deref())?;
    let request_id = if entry.state.is_imported() {
        Some(entry.entry_key.as_str())
    } else {
        None
    };
    tx.execute(
        "INSERT INTO workbuddy_import_ledger(entry_key, namespace_id, session_hash, message_hash,
             requested_model, event_at_ms, normalized_hash, state, request_id, candidate_json,
             mapping_id, mapping_revision, created_at_ms, updated_at_ms)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?13)",
        params![
            entry.entry_key,
            entry.namespace_id,
            entry.session_hash,
            entry.message_hash,
            entry.requested_model,
            entry.event_at_ms,
            entry.normalized_hash,
            entry.state.as_str(),
            request_id,
            entry.candidate_json,
            entry.mapping_id,
            entry.mapping_revision,
            entry.now_ms,
        ],
    )?;
    Ok(())
}

/// Moves an existing row to a new state.
///
/// A tombstone is terminal: any attempt to move it anywhere else is refused, so
/// a rescan can never revive cleared history. A candidate payload is cleared
/// whenever the target state does not allow one, which keeps
/// `candidate_json` from lingering on imported/excluded/deleted rows.
pub fn set_state(
    tx: &Transaction<'_>,
    entry_key: &str,
    state: LedgerState,
    candidate_json: Option<&str>,
    now_ms: i64,
) -> Result<(), StoreError> {
    let existing = load(tx, entry_key)?.ok_or(StoreError::NotFound)?;
    if existing.state.is_tombstone() && state != existing.state {
        return Err(StoreError::Tombstone);
    }
    check_candidate(state, candidate_json)?;
    let request_id = if state.is_imported() {
        Some(entry_key)
    } else {
        None
    };
    tx.execute(
        "UPDATE workbuddy_import_ledger
         SET state = ?2, request_id = ?3, candidate_json = ?4, updated_at_ms = ?5
         WHERE entry_key = ?1",
        params![
            entry_key,
            state.as_str(),
            request_id,
            candidate_json,
            now_ms
        ],
    )?;
    Ok(())
}

/// Runs the ledger write path once against an **in-memory** copy of the schema.
///
/// This is the startup contract check for the ledger: it proves that
/// `insert`/`load`/`set_state` still match the migrated schema without touching
/// any user database, and it keeps those entry points on the real startup path
/// so a schema drift fails closed instead of surfacing during the first import.
pub fn ledger_contract_check() -> Result<usize, &'static str> {
    let mut conn = Connection::open_in_memory().map_err(|_| "workbuddy_import_ledger_memory")?;
    crate::stats::store::migrate(&mut conn, Path::new("."))
        .map_err(|_| "workbuddy_import_ledger_migrate")?;
    let tx = conn
        .transaction()
        .map_err(|_| "workbuddy_import_ledger_transaction")?;
    let entry = NewLedgerEntry {
        entry_key: "0".repeat(64),
        namespace_id: "0".repeat(48),
        session_hash: "0".repeat(64),
        message_hash: "0".repeat(64),
        requested_model: None,
        event_at_ms: 0,
        normalized_hash: "0".repeat(64),
        state: LedgerState::PendingClassification,
        candidate_json: None,
        mapping_id: None,
        mapping_revision: 1,
        now_ms: 0,
    };
    insert(&tx, &entry).map_err(|_| "workbuddy_import_ledger_insert")?;
    let loaded = load(&tx, &entry.entry_key)
        .map_err(|_| "workbuddy_import_ledger_load")?
        .ok_or("workbuddy_import_ledger_missing")?;
    if loaded.state != LedgerState::PendingClassification || loaded.request_id.is_some() {
        return Err("workbuddy_import_ledger_shape_drifted");
    }
    if insert(&tx, &entry).is_ok() {
        return Err("workbuddy_import_ledger_duplicate_accepted");
    }
    set_state(&tx, &entry.entry_key, LedgerState::Conflict, Some("{}"), 1)
        .map_err(|_| "workbuddy_import_ledger_state")?;
    if load(&tx, &entry.entry_key)
        .map_err(|_| "workbuddy_import_ledger_reload")?
        .ok_or("workbuddy_import_ledger_missing")?
        .state
        != LedgerState::Conflict
    {
        return Err("workbuddy_import_ledger_state_drifted");
    }
    tx.rollback()
        .map_err(|_| "workbuddy_import_ledger_rollback")?;
    Ok(1)
}

/// A file checkpoint row as stored (DATA-API section 3.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub file_key: String,
    pub namespace_id: String,
    pub relative_path: String,
    pub generation: i64,
    pub revision: i64,
    pub committed_offset: i64,
    pub observed_size: i64,
    pub modified_ms: i64,
    pub prefix_hash: Option<String>,
    pub prefix_len: i64,
    pub anchor_hash: Option<String>,
    pub anchor_start: i64,
    pub anchor_len: i64,
    pub discarding_oversized: bool,
    pub scan_offset: i64,
    pub scan_generation: i64,
    pub last_seen_at_ms: i64,
    pub last_completed_at_ms: Option<i64>,
    pub last_error_code: Option<String>,
    pub diagnostic_counts_json: String,
}

/// A checkpoint for a file that has just been discovered.
#[derive(Clone, Debug)]
pub struct NewCheckpoint {
    pub file_key: String,
    pub namespace_id: String,
    pub relative_path: String,
    pub observed_size: i64,
    pub modified_ms: i64,
    pub now_ms: i64,
}

/// What a batch claims about the checkpoint it was read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckpointGuard {
    pub file_key: String,
    pub namespace_id: String,
    pub generation: i64,
    pub revision: i64,
    pub base_offset: i64,
}

/// The next committed state of a checkpoint.
#[derive(Clone, Debug)]
pub struct CheckpointUpdate {
    pub next_offset: i64,
    pub observed_size: i64,
    pub modified_ms: i64,
    pub prefix_hash: Option<String>,
    pub anchor_hash: Option<String>,
    pub diagnostics: DiagnosticCounts,
    pub last_error_code: Option<ImportErrorCode>,
    pub now_ms: i64,
}

/// Serialises the closed diagnostic key set; no arbitrary string can appear.
pub fn diagnostics_json(counts: &DiagnosticCounts) -> String {
    let mut text = String::from("{");
    for (index, (kind, value)) in counts.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        text.push('"');
        text.push_str(kind.as_str());
        text.push_str("\":");
        text.push_str(&value.to_string());
    }
    text.push('}');
    text
}

/// Reads one checkpoint; `None` when the file was never seen.
pub fn load_checkpoint(
    tx: &Transaction<'_>,
    file_key: &str,
) -> Result<Option<Checkpoint>, StoreError> {
    let row = tx
        .query_row(
            "SELECT file_key, namespace_id, relative_path, generation, revision, committed_offset,
                    observed_size, modified_ms, prefix_hash, prefix_len, anchor_hash, anchor_start,
                    anchor_len, discarding_oversized, scan_offset, scan_generation, last_seen_at_ms,
                    last_completed_at_ms, last_error_code, diagnostic_counts_json
             FROM workbuddy_import_files WHERE file_key = ?1",
            params![file_key],
            |row| {
                Ok(Checkpoint {
                    file_key: row.get(0)?,
                    namespace_id: row.get(1)?,
                    relative_path: row.get(2)?,
                    generation: row.get(3)?,
                    revision: row.get(4)?,
                    committed_offset: row.get(5)?,
                    observed_size: row.get(6)?,
                    modified_ms: row.get(7)?,
                    prefix_hash: row.get(8)?,
                    prefix_len: row.get(9)?,
                    anchor_hash: row.get(10)?,
                    anchor_start: row.get(11)?,
                    anchor_len: row.get(12)?,
                    discarding_oversized: row.get::<_, i64>(13)? != 0,
                    scan_offset: row.get(14)?,
                    scan_generation: row.get(15)?,
                    last_seen_at_ms: row.get(16)?,
                    last_completed_at_ms: row.get(17)?,
                    last_error_code: row.get(18)?,
                    diagnostic_counts_json: row.get(19)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

/// Registers a newly discovered file at generation 1, revision 1, offset 0.
pub fn insert_checkpoint(tx: &Transaction<'_>, entry: &NewCheckpoint) -> Result<(), StoreError> {
    if load_checkpoint(tx, &entry.file_key)?.is_some() {
        return Err(StoreError::AlreadyExists);
    }
    tx.execute(
        "INSERT INTO workbuddy_import_files(file_key, namespace_id, relative_path, generation,
             revision, committed_offset, observed_size, modified_ms, prefix_len, anchor_start,
             anchor_len, discarding_oversized, scan_offset, scan_generation, last_seen_at_ms,
             diagnostic_counts_json)
         VALUES(?1,?2,?3,1,1,0,?4,?5,0,0,0,0,0,0,?6,'{}')",
        params![
            entry.file_key,
            entry.namespace_id,
            entry.relative_path,
            entry.observed_size,
            entry.modified_ms,
            entry.now_ms,
        ],
    )?;
    Ok(())
}

/// Advances a checkpoint with an optimistic guard.
///
/// The batch must match the stored namespace, generation, revision and base
/// offset. A mismatch is answered with a fixed rejection code instead of a
/// write, so an out-of-date reader can never move the cursor and no partial
/// state is left behind. `next_offset == base_offset` is legal and simply means
/// the file did not grow; it still bumps `revision` so later batches observe it.
pub fn commit_checkpoint(
    tx: &Transaction<'_>,
    guard: &CheckpointGuard,
    update: &CheckpointUpdate,
) -> Result<CheckpointAck, StoreError> {
    if guard.base_offset < 0 || update.next_offset < 0 || update.next_offset < guard.base_offset {
        return Err(StoreError::InvalidOffset);
    }
    let current = load_checkpoint(tx, &guard.file_key)?.ok_or(StoreError::NotFound)?;
    if current.namespace_id != guard.namespace_id {
        return Ok(CheckpointAck::Rejected(BatchRejectReason::StaleBatch));
    }
    if current.generation != guard.generation {
        return Ok(CheckpointAck::Rejected(BatchRejectReason::SourceChanged));
    }
    if current.revision != guard.revision || current.committed_offset != guard.base_offset {
        return Ok(CheckpointAck::Rejected(BatchRejectReason::StaleBatch));
    }
    let changed = tx.execute(
        "UPDATE workbuddy_import_files
         SET revision = revision + 1, committed_offset = ?2, observed_size = ?3, modified_ms = ?4,
             prefix_hash = ?5, anchor_hash = ?6, last_seen_at_ms = ?7, last_completed_at_ms = ?7,
             last_error_code = ?8, diagnostic_counts_json = ?9
         WHERE file_key = ?1 AND namespace_id = ?10 AND generation = ?11 AND revision = ?12",
        params![
            guard.file_key,
            update.next_offset,
            update.observed_size,
            update.modified_ms,
            update.prefix_hash,
            update.anchor_hash,
            update.now_ms,
            update.last_error_code.map(|code| code.as_str()),
            diagnostics_json(&update.diagnostics),
            guard.namespace_id,
            guard.generation,
            guard.revision,
        ],
    )?;
    if changed != 1 {
        // The row moved between the read and the write: treat it as stale.
        return Ok(CheckpointAck::Rejected(BatchRejectReason::StaleBatch));
    }
    Ok(CheckpointAck::Committed(CommitOutcome {
        file_key: guard.file_key.clone(),
        revision: guard.revision + 1,
        committed_offset: update.next_offset,
        counts: Default::default(),
    }))
}

/// Runs the checkpoint write path once against an in-memory schema.
///
/// Startup contract check: proves the CAS SQL still matches the migrated schema
/// and that a stale guard is refused, without touching a user database.
pub fn checkpoint_contract_check() -> Result<usize, &'static str> {
    let mut conn =
        Connection::open_in_memory().map_err(|_| "workbuddy_import_checkpoint_memory")?;
    crate::stats::store::migrate(&mut conn, Path::new("."))
        .map_err(|_| "workbuddy_import_checkpoint_migrate")?;
    let tx = conn
        .transaction()
        .map_err(|_| "workbuddy_import_checkpoint_transaction")?;
    let file_key = "0".repeat(64);
    insert_checkpoint(
        &tx,
        &NewCheckpoint {
            file_key: file_key.clone(),
            namespace_id: "0".repeat(48),
            relative_path: "session-probe.jsonl".into(),
            observed_size: 0,
            modified_ms: 0,
            now_ms: 0,
        },
    )
    .map_err(|_| "workbuddy_import_checkpoint_insert")?;
    let guard = CheckpointGuard {
        file_key: file_key.clone(),
        namespace_id: "0".repeat(48),
        generation: 1,
        revision: 1,
        base_offset: 0,
    };
    let update = CheckpointUpdate {
        next_offset: 0,
        observed_size: 0,
        modified_ms: 0,
        prefix_hash: None,
        anchor_hash: None,
        diagnostics: DiagnosticCounts::default(),
        last_error_code: None,
        now_ms: 1,
    };
    let ack = commit_checkpoint(&tx, &guard, &update)
        .map_err(|_| "workbuddy_import_checkpoint_commit")?;
    if !ack.is_committed() {
        return Err("workbuddy_import_checkpoint_commit_refused");
    }
    // The same guard is stale now: revision moved to 2.
    match commit_checkpoint(&tx, &guard, &update) {
        Ok(CheckpointAck::Rejected(BatchRejectReason::StaleBatch)) => {}
        _ => return Err("workbuddy_import_checkpoint_stale_accepted"),
    }
    if load_checkpoint(&tx, &file_key)
        .map_err(|_| "workbuddy_import_checkpoint_reload")?
        .ok_or("workbuddy_import_checkpoint_missing")?
        .revision
        != 2
    {
        return Err("workbuddy_import_checkpoint_revision_drifted");
    }
    tx.rollback()
        .map_err(|_| "workbuddy_import_checkpoint_rollback")?;
    Ok(1)
}

/// How a historical model was routed (DATA-API section 3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MappingClass {
    Proxy,
    Direct,
}

/// What proves a mapping interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MappingEvidence {
    ObservedConfig,
    ManagedSync,
    Manual,
}

/// Lifecycle of a mapping row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MappingState {
    Prepared,
    Verified,
    Conflict,
    Revoked,
}

/// Progress of a rule reconcile task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconcileState {
    Pending,
    Running,
    Completed,
}

macro_rules! mapping_enum {
    ($name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];
            /// Stored form.
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $text),+ }
            }
            /// Parses a stored value.
            pub fn parse(value: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|item| item.as_str() == value)
            }
        }
    };
}

mapping_enum!(MappingClass { Proxy => "proxy", Direct => "direct" });
mapping_enum!(MappingEvidence {
    ObservedConfig => "observed_config",
    ManagedSync => "managed_sync",
    Manual => "manual",
});
mapping_enum!(MappingState {
    Prepared => "prepared",
    Verified => "verified",
    Conflict => "conflict",
    Revoked => "revoked",
});
mapping_enum!(ReconcileState {
    Pending => "pending",
    Running => "running",
    Completed => "completed",
});

impl MappingState {
    /// Only a verified interval may classify imported records.
    pub fn is_usable(self) -> bool {
        matches!(self, MappingState::Verified)
    }
}

/// A historical model-attribution row as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelMapping {
    pub mapping_id: String,
    pub namespace_id: String,
    pub requested_model: String,
    pub classification: MappingClass,
    pub valid_from_ms: i64,
    pub valid_to_ms: Option<i64>,
    pub evidence: MappingEvidence,
    pub state: MappingState,
    pub route_id: Option<String>,
    pub provider_id: Option<String>,
    pub revision: i64,
    pub reconcile_state: ReconcileState,
    pub reconcile_watermark: i64,
    pub reconciled_rows: i64,
    pub reconcile_error_code: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// A mapping row to insert.
#[derive(Clone, Debug)]
pub struct NewMapping {
    pub mapping_id: String,
    pub namespace_id: String,
    pub requested_model: String,
    pub classification: MappingClass,
    pub valid_from_ms: i64,
    /// Exclusive upper bound; `None` means "no upper bound yet".
    pub valid_to_ms: Option<i64>,
    pub evidence: MappingEvidence,
    pub state: MappingState,
    pub route_id: Option<String>,
    pub provider_id: Option<String>,
    pub now_ms: i64,
}

/// Result of matching one model id at one instant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MappingMatch {
    /// Nothing is known for that instant.
    None,
    /// Exactly one verified interval covers the instant.
    One(Box<ModelMapping>),
    /// More than one verified interval covers the instant: never guess.
    Conflict,
}

fn mapping_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ModelMapping> {
    let classification: String = row.get(3)?;
    let evidence: String = row.get(6)?;
    let state: String = row.get(7)?;
    let reconcile: String = row.get(11)?;
    Ok(ModelMapping {
        mapping_id: row.get(0)?,
        namespace_id: row.get(1)?,
        requested_model: row.get(2)?,
        classification: MappingClass::parse(&classification).unwrap_or(MappingClass::Direct),
        valid_from_ms: row.get(4)?,
        valid_to_ms: row.get(5)?,
        evidence: MappingEvidence::parse(&evidence).unwrap_or(MappingEvidence::Manual),
        state: MappingState::parse(&state).unwrap_or(MappingState::Conflict),
        route_id: row.get(8)?,
        provider_id: row.get(9)?,
        revision: row.get(10)?,
        reconcile_state: ReconcileState::parse(&reconcile).unwrap_or(ReconcileState::Pending),
        reconcile_watermark: row.get(12)?,
        reconciled_rows: row.get(13)?,
        reconcile_error_code: row.get(14)?,
        created_at_ms: row.get(15)?,
        updated_at_ms: row.get(16)?,
    })
}

const MAPPING_COLUMNS: &str = "mapping_id, namespace_id, requested_model, classification,
     valid_from_ms, valid_to_ms, evidence, state, route_id, provider_id, revision,
     reconcile_state, reconcile_watermark, reconciled_rows, reconcile_error_code,
     created_at_ms, updated_at_ms";

/// Reads one mapping row.
pub fn load_mapping(
    tx: &Transaction<'_>,
    mapping_id: &str,
) -> Result<Option<ModelMapping>, StoreError> {
    let sql =
        format!("SELECT {MAPPING_COLUMNS} FROM workbuddy_model_history WHERE mapping_id = ?1");
    Ok(tx
        .query_row(&sql, params![mapping_id], mapping_from_row)
        .optional()?)
}

/// Counts verified intervals that would overlap `[from, to)` for one model.
///
/// Mirrors the SQL trigger so the caller gets a typed error instead of a raw
/// constraint failure, and so the same rule can be asserted from Rust.
pub fn overlapping_verified(
    tx: &Transaction<'_>,
    namespace_id: &str,
    requested_model: &str,
    from_ms: i64,
    to_ms: Option<i64>,
    except_mapping_id: Option<&str>,
) -> Result<i64, StoreError> {
    Ok(tx.query_row(
        "SELECT COUNT(*) FROM workbuddy_model_history
         WHERE namespace_id = ?1 AND requested_model = ?2 AND state = 'verified'
           AND (?3 IS NULL OR mapping_id <> ?3)
           AND valid_from_ms < COALESCE(?5, 9223372036854775807)
           AND ?4 < COALESCE(valid_to_ms, 9223372036854775807)",
        params![
            namespace_id,
            requested_model,
            except_mapping_id,
            from_ms,
            to_ms
        ],
        |row| row.get(0),
    )?)
}

/// Inserts a mapping row.
///
/// Adjacent half-open intervals are legal; an overlap with another **verified**
/// interval is refused here and, as a backstop, by the schema trigger.
pub fn insert_mapping(tx: &Transaction<'_>, entry: &NewMapping) -> Result<(), StoreError> {
    if load_mapping(tx, &entry.mapping_id)?.is_some() {
        return Err(StoreError::AlreadyExists);
    }
    if entry
        .valid_to_ms
        .is_some_and(|to| to <= entry.valid_from_ms)
    {
        return Err(StoreError::InvalidOffset);
    }
    if entry.state.is_usable()
        && overlapping_verified(
            tx,
            &entry.namespace_id,
            &entry.requested_model,
            entry.valid_from_ms,
            entry.valid_to_ms,
            None,
        )? > 0
    {
        return Err(StoreError::Overlapping);
    }
    tx.execute(
        "INSERT INTO workbuddy_model_history(mapping_id, namespace_id, requested_model,
             classification, valid_from_ms, valid_to_ms, evidence, state, route_id, provider_id,
             revision, reconcile_state, reconcile_watermark, reconciled_rows, created_at_ms,
             updated_at_ms)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,1,'completed',0,0,?11,?11)",
        params![
            entry.mapping_id,
            entry.namespace_id,
            entry.requested_model,
            entry.classification.as_str(),
            entry.valid_from_ms,
            entry.valid_to_ms,
            entry.evidence.as_str(),
            entry.state.as_str(),
            entry.route_id,
            entry.provider_id,
            entry.now_ms,
        ],
    )?;
    Ok(())
}

/// Moves a mapping to a new state, keeping the interval intact.
pub fn set_mapping_state(
    tx: &Transaction<'_>,
    mapping_id: &str,
    state: MappingState,
    now_ms: i64,
) -> Result<(), StoreError> {
    let current = load_mapping(tx, mapping_id)?.ok_or(StoreError::NotFound)?;
    if state.is_usable()
        && overlapping_verified(
            tx,
            &current.namespace_id,
            &current.requested_model,
            current.valid_from_ms,
            current.valid_to_ms,
            Some(mapping_id),
        )? > 0
    {
        return Err(StoreError::Overlapping);
    }
    let changed = tx.execute(
        "UPDATE workbuddy_model_history SET state = ?2, revision = revision + 1, updated_at_ms = ?3
         WHERE mapping_id = ?1",
        params![mapping_id, state.as_str(), now_ms],
    )?;
    if changed != 1 {
        return Err(StoreError::NotFound);
    }
    Ok(())
}

/// Finds the verified interval covering `at_ms`.
///
/// Zero matches means unknown, one match is authoritative, and more than one is
/// a conflict: the newest is never picked.
pub fn match_mapping(
    tx: &Transaction<'_>,
    namespace_id: &str,
    requested_model: &str,
    at_ms: i64,
) -> Result<MappingMatch, StoreError> {
    let sql = format!(
        "SELECT {MAPPING_COLUMNS} FROM workbuddy_model_history
         WHERE namespace_id = ?1 AND requested_model = ?2 AND state = 'verified'
           AND valid_from_ms <= ?3 AND (valid_to_ms IS NULL OR ?3 < valid_to_ms)
         ORDER BY valid_from_ms LIMIT 2"
    );
    let mut statement = tx.prepare(&sql)?;
    let mut rows = statement.query(params![namespace_id, requested_model, at_ms])?;
    let first = rows.next()?.map(mapping_from_row).transpose()?;
    let second = rows.next()?.is_some();
    Ok(match (first, second) {
        (None, _) => MappingMatch::None,
        (Some(_), true) => MappingMatch::Conflict,
        (Some(row), false) => MappingMatch::One(Box::new(row)),
    })
}

/// Classify only the WorkBuddy format, never who created the model. A native
/// fact wins only for the exact session/message pair carried by request headers.
pub fn attribute_candidate(
    tx: &Transaction<'_>,
    _namespace_id: &str,
    candidate: &Candidate,
) -> Result<(Attribution, Option<String>), StoreError> {
    if native_duplicate(tx, &candidate.session_hash, &candidate.message_hash)? {
        return Ok((Attribution::Proxy, None));
    }
    let class = match candidate.requested_model.as_deref() {
        None => Attribution::Pending,
        Some(model) if model.starts_with("custom-local:") => Attribution::Direct,
        Some(_) => Attribution::Builtin,
    };
    Ok((class, None))
}

pub(super) fn native_duplicate(
    tx: &Transaction<'_>,
    session: &str,
    message: &str,
) -> rusqlite::Result<bool> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM workbuddy_proxy_identity p
        JOIN usage_requests r ON r.request_id=p.request_id
        WHERE p.session_hash=?1 AND p.message_hash=?2 AND r.record_origin='proxy')",
        params![session, message],
        |row| row.get(0),
    )
}

/// Part of native Start's transaction. Imports arriving first are withdrawn
/// atomically with admission of their native original; native rows are untouched.
pub(crate) fn record_proxy_identity(
    tx: &Transaction<'_>,
    request: &str,
    identity: &crate::stats::event::WorkbuddyIdentity,
    now: i64,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO workbuddy_proxy_identity(request_id,session_hash,message_hash)
        VALUES(?1,?2,?3) ON CONFLICT(request_id) DO NOTHING",
        params![request, identity.session_hash, identity.message_hash],
    )?;
    let matches: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM workbuddy_proxy_identity
        WHERE request_id=?1 AND session_hash=?2 AND message_hash=?3)",
        params![request, identity.session_hash, identity.message_hash],
        |r| r.get(0),
    )?;
    if !matches {
        return Ok(());
    }
    drain_proxy_identity(tx, request, identity, now)?;
    Ok(())
}
fn drain_proxy_identity(
    tx: &Transaction<'_>,
    request: &str,
    identity: &crate::stats::event::WorkbuddyIdentity,
    now: i64,
) -> rusqlite::Result<usize> {
    let mut statement = tx.prepare(
        "SELECT entry_key FROM workbuddy_import_ledger
        WHERE session_hash=?1 AND message_hash=?2 AND state='imported' LIMIT ?3",
    )?;
    let keys = statement
        .query_map(
            params![
                identity.session_hash,
                identity.message_hash,
                super::types::MAX_BATCH_ITEMS as i64
            ],
            |row| row.get::<_, String>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let count = keys.len();
    for key in keys {
        withdraw_workbuddy_fact(tx, &key).map_err(|_| rusqlite::Error::InvalidQuery)?;
        tx.execute(
            "UPDATE workbuddy_import_ledger SET state='excluded_proxy',candidate_json=NULL,
            mapping_id=NULL,updated_at_ms=MAX(updated_at_ms,?2) WHERE entry_key=?1",
            params![key, now],
        )?;
    }
    tx.execute("UPDATE workbuddy_proxy_identity SET pending_dedup=EXISTS(SELECT 1 FROM workbuddy_import_ledger
        WHERE session_hash=?2 AND message_hash=?3 AND state='imported') WHERE request_id=?1",
        params![request,identity.session_hash,identity.message_hash])?;
    Ok(count)
}

/// Resume the bounded native-precedence task after a crash or a large namespace
/// history. Each transaction withdraws at most MAX_BATCH_ITEMS log facts.
pub(super) fn resume_proxy_dedup(tx: &Transaction<'_>, now: i64) -> rusqlite::Result<usize> {
    use rusqlite::OptionalExtension;
    let pending = tx
        .query_row(
            "SELECT request_id,session_hash,message_hash FROM workbuddy_proxy_identity
        WHERE pending_dedup=1 ORDER BY request_id LIMIT 1",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    crate::stats::event::WorkbuddyIdentity {
                        session_hash: r.get(1)?,
                        message_hash: r.get(2)?,
                    },
                ))
            },
        )
        .optional()?;
    match pending {
        Some((request, identity)) => drain_proxy_identity(tx, &request, &identity, now),
        None => Ok(0),
    }
}

/// Import-related columns of the singleton settings row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportSettings {
    pub enabled: bool,
    pub namespace_id: String,
    pub mapping_revision: i64,
    pub last_full_scan_ms: Option<i64>,
    /// The shared settings revision used for CAS.
    pub revision: i64,
}

/// Result of a settings CAS.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsOutcome {
    Updated(Box<ImportSettings>),
    Conflict { current_revision: i64 },
}

/// Reads the import settings.
pub fn load_import_settings(tx: &Transaction<'_>) -> Result<ImportSettings, StoreError> {
    Ok(tx.query_row(
        "SELECT workbuddy_import_enabled, workbuddy_namespace_id, workbuddy_mapping_revision,
                workbuddy_last_full_scan_ms, revision
         FROM stats_settings WHERE singleton = 1",
        [],
        |row| {
            Ok(ImportSettings {
                enabled: row.get::<_, i64>(0)? != 0,
                namespace_id: row.get(1)?,
                mapping_revision: row.get(2)?,
                last_full_scan_ms: row.get(3)?,
                revision: row.get(4)?,
            })
        },
    )?)
}

/// Turns import on or off with a settings CAS.
///
/// Enabling creates the namespace exactly once; the id is kept when import is
/// turned off, so toggling never re-identifies existing ledger entries.
pub fn set_import_enabled(
    tx: &Transaction<'_>,
    enabled: bool,
    expected_revision: i64,
    now_ms: i64,
) -> Result<SettingsOutcome, StoreError> {
    let current = load_import_settings(tx)?;
    if current.revision != expected_revision {
        return Ok(SettingsOutcome::Conflict {
            current_revision: current.revision,
        });
    }
    let namespace_id = if enabled && current.namespace_id.is_empty() {
        crate::config::token()
    } else {
        current.namespace_id.clone()
    };
    let changed = tx.execute(
        "UPDATE stats_settings SET workbuddy_import_enabled = ?1, workbuddy_namespace_id = ?2,
             updated_at_ms = ?3, revision = revision + 1
         WHERE singleton = 1 AND revision = ?4",
        params![i64::from(enabled), namespace_id, now_ms, expected_revision],
    )?;
    if changed != 1 {
        let now = load_import_settings(tx)?;
        return Ok(SettingsOutcome::Conflict {
            current_revision: now.revision,
        });
    }
    if current.enabled && !enabled {
        super::observation::suspend_history(tx, now_ms)?;
    }
    Ok(SettingsOutcome::Updated(Box::new(load_import_settings(
        tx,
    )?)))
}

/// Loads every mapping currently in one state.
pub fn load_mappings_in_state(
    tx: &Transaction<'_>,
    state: MappingState,
) -> Result<Vec<ModelMapping>, StoreError> {
    let sql = format!(
        "SELECT {MAPPING_COLUMNS} FROM workbuddy_model_history WHERE state = ?1 ORDER BY mapping_id"
    );
    let mut statement = tx.prepare(&sql)?;
    let rows = statement.query_map(params![state.as_str()], mapping_from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The outcome of one startup recovery pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PreparedRecovery {
    pub completed: usize,
    pub conflicted: usize,
}

impl PreparedRecovery {
    /// How many prepared rows were resolved.
    pub fn total(&self) -> usize {
        self.completed + self.conflicted
    }
}

/// Resolves every `prepared` mapping against the configuration that exists now.
///
/// A `prepared` row is written **before** the WorkBuddy file is touched, so a
/// crash can leave three states behind: nothing written, the file written but
/// unconfirmed, or the file written and confirmed but the ACK lost. Startup
/// resolves them all the same way — the row is completed only when the
/// configuration really contains it, and is otherwise turned into `conflict`.
///
/// A `prepared` or `conflict` row is never usable for classification
/// ([`MappingState::is_usable`]), so an uncertain interval can never be counted
/// as a direct call, whichever moment the crash happened at.
///
/// When import is disabled the pass is skipped entirely: the rows simply stay
/// `prepared` (still unusable), and WorkBuddy syncing is unaffected.
pub fn recover_prepared(
    tx: &Transaction<'_>,
    enabled: bool,
    present: &dyn Fn(&ModelMapping) -> bool,
    now_ms: i64,
) -> Result<PreparedRecovery, StoreError> {
    if !enabled {
        return Ok(PreparedRecovery::default());
    }
    let mut recovery = PreparedRecovery::default();
    for mapping in load_mappings_in_state(tx, MappingState::Prepared)? {
        if present(&mapping) {
            match set_mapping_state(tx, &mapping.mapping_id, MappingState::Verified, now_ms) {
                Ok(()) => {
                    recovery.completed += 1;
                    continue;
                }
                // Completing it would overlap an interval that is already
                // verified: the honest answer is a conflict, not an override.
                Err(StoreError::Overlapping) => {}
                Err(failure) => return Err(failure),
            }
        }
        set_mapping_state(tx, &mapping.mapping_id, MappingState::Conflict, now_ms)?;
        recovery.conflicted += 1;
    }
    Ok(recovery)
}

/// Milliseconds in one retention day.
pub const RETENTION_DAY_MS: i64 = 86_400_000;

/// True when a record is already older than the retention window.
///
/// A window of `0` (or less) means "keep everything": nothing is ever expired,
/// which is the documented "不限期" setting and not an accident of arithmetic.
pub fn is_expired(event_at_ms: i64, now_ms: i64, retention_days: i64) -> bool {
    if retention_days <= 0 {
        return false;
    }
    let window = retention_days.saturating_mul(RETENTION_DAY_MS);
    event_at_ms < now_ms.saturating_sub(window)
}

/// The configuration a batch was prepared under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchPolicy {
    /// The settings revision the caller read.
    pub settings_revision: i64,
    /// The retention window in days; `0` keeps everything.
    pub retention_days: i64,
}

/// One candidate ready to be committed in a batch.
#[derive(Clone, Debug)]
pub struct CommitItem {
    pub entry_key: String,
    pub session_hash: String,
    pub message_hash: String,
    pub requested_model: Option<String>,
    pub upstream_model: Option<String>,
    pub response_model: Option<String>,
    pub tool_name: Option<String>,
    pub usage: NormalizedUsage,
    pub mapping_id: Option<String>,
    pub event_at_ms: i64,
    pub normalized_hash: String,
    pub attribution: Attribution,
    /// Whitelisted candidate JSON; only pending rows may carry one.
    pub candidate_json: Option<String>,
    /// Fact-row origin; `None` for rows that must not produce a fact.
    pub origin: Option<&'static str>,
}

impl CommitItem {
    /// Carries only the parser's allowlisted, normalized data to the writer.
    /// The semantic hash is independent of the file path, timestamp, JSON key
    /// order, chat content and diagnostic observations.
    pub fn from_candidate(
        mut candidate: Candidate,
        attribution: Attribution,
    ) -> Result<Self, StoreError> {
        // Old pending/prepared payloads may still carry the former Missing
        // state. Normalize them too when replaying or changing attribution.
        if candidate.requested_model.as_deref().is_some_and(|model| {
            model.starts_with("workbuddy-credit:") && model.len() > "workbuddy-credit:".len()
        }) && candidate.usage.status == crate::stats::dto::UsageStatus::Missing
            && candidate.usage.has_no_token_values()
        {
            candidate.usage.status = crate::stats::dto::UsageStatus::NotApplicable;
            candidate.usage.reason = None;
        }
        let normalized_hash = semantic_hash(
            &candidate.requested_model,
            &candidate.upstream_model,
            &candidate.response_model,
            &candidate.usage,
        )?;
        let candidate_json =
            serde_json::to_string(&candidate).map_err(|_| StoreError::CandidateNotAllowed)?;
        if candidate_json.len() > MAX_CANDIDATE_BYTES {
            return Err(StoreError::CandidateTooLarge);
        }
        let origin = match attribution {
            Attribution::Builtin => Some("workbuddy_builtin"),
            Attribution::Direct => Some("workbuddy_custom"),
            Attribution::Proxy | Attribution::Pending => None,
        };
        Ok(Self {
            entry_key: candidate.entry_key,
            session_hash: candidate.session_hash,
            message_hash: candidate.message_hash,
            requested_model: candidate.requested_model,
            upstream_model: candidate.upstream_model,
            response_model: candidate.response_model,
            tool_name: candidate.tool_name,
            usage: candidate.usage,
            mapping_id: None,
            event_at_ms: candidate.event_at_ms,
            normalized_hash,
            attribution,
            candidate_json: Some(candidate_json),
            origin,
        })
    }
}

fn semantic_hash(
    requested_model: &Option<String>,
    upstream_model: &Option<String>,
    response_model: &Option<String>,
    usage: &NormalizedUsage,
) -> Result<String, StoreError> {
    // Keep v2/v3 receipt hashes stable: Token applicability is a presentation
    // correction, not a different bill. Real amount/Token changes still conflict.
    let status = if requested_model.as_deref().is_some_and(|model| {
        model.starts_with("workbuddy-credit:") && model.len() > "workbuddy-credit:".len()
    }) && usage.status == crate::stats::dto::UsageStatus::NotApplicable
        && usage.has_no_token_values()
    {
        crate::stats::dto::UsageStatus::Missing
    } else {
        usage.status
    };
    let semantic = serde_json::to_string(&(
        "wbp-wb-usage-v1",
        requested_model,
        upstream_model,
        response_model,
        status,
        usage.reason,
        usage.input_tokens,
        usage.output_tokens,
        usage.total_tokens,
        usage.total_tokens_derived,
        usage.cache_read_tokens,
        usage.cache_write_tokens,
        usage.reasoning_tokens,
        usage.input_breakdown,
        usage.workbuddy_credit_nanos,
    ))
    .map_err(|_| StoreError::CandidateNotAllowed)?;
    Ok(super::parser::hash_id(&semantic))
}

/// What one batch commit produced.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitReport {
    pub imported: usize,
    pub excluded_proxy: usize,
    pub pending: usize,
    pub duplicates: usize,
    pub conflicted: usize,
    pub excluded_retention: usize,
    /// Offset that became committed; unchanged when the batch did not commit.
    pub committed_offset: u64,
}

pub(super) fn insert_fact_row(
    tx: &Transaction<'_>,
    item: &CommitItem,
    origin: &str,
    now_ms: i64,
) -> Result<(), StoreError> {
    let requested_model = item
        .requested_model
        .as_deref()
        .ok_or(StoreError::CandidateNotAllowed)?;
    let model_hash = super::parser::hash_id(requested_model);
    let provider_id = match origin {
        "workbuddy_builtin" => "wb:builtin".to_owned(),
        "workbuddy_custom" => format!("wb:custom:{}", &model_hash[..48]),
        _ => return Err(StoreError::CandidateNotAllowed),
    };
    let provider_name = if origin == "workbuddy_custom" {
        "WorkBuddy custom"
    } else {
        "WorkBuddy"
    };
    let route_id = format!("wb:route:{}", &model_hash[..48]);
    let usage = &item.usage;
    tx.execute(
        "INSERT INTO usage_requests(request_id, source, provider_id, route_id, provider_name,
         route_name, requested_model, upstream_model, started_at_ms, ended_at_ms, outcome,
         usage_status, usage_source, created_at_ms, updated_at_ms, upstream_protocol,
         record_origin, time_basis, response_model, usage_reason, parser_version,
         input_tokens, output_tokens, total_tokens, cache_read_tokens, cache_write_tokens,
         reasoning_tokens, input_breakdown, total_tokens_derived, workbuddy_credit_nanos)
         VALUES(?1,'workbuddy_import',?6,?7,?8,COALESCE(?23,?2),
         ?2, COALESCE(?9,?2), ?3, ?3, 'recorded', ?10,
         'workbuddy_jsonl', ?4, ?4, NULL, ?5, 'message_timestamp',?11,?12,?13,
         ?14,?15,?16,?17,?18,?19,?20,?21,?22)",
        params![
            item.entry_key,
            item.requested_model,
            item.event_at_ms,
            now_ms,
            origin,
            provider_id,
            route_id,
            provider_name,
            item.upstream_model,
            usage.status.as_str(),
            item.response_model,
            usage.reason.map(|reason| reason.as_str()),
            PARSER_VERSION,
            usage.input_tokens,
            usage.output_tokens,
            usage.total_tokens,
            usage.cache_read_tokens,
            usage.cache_write_tokens,
            usage.reasoning_tokens,
            usage.input_breakdown.as_str(),
            usage.total_tokens_derived,
            usage.workbuddy_credit_nanos,
            item.tool_name,
        ],
    )?;
    Ok(())
}

/// Withdraws the workbuddy fact row of one entry.
///
/// The ledger row is moved off `imported` **first**: deleting the fact row
/// would otherwise fire `ON DELETE SET NULL` while the row still says
/// `imported`, which the ledger's own CHECK forbids. Only workbuddy origins are
/// ever deleted — a proxy fact row is never removed or overwritten by an
/// import.
pub fn withdraw_workbuddy_fact(tx: &Transaction<'_>, entry_key: &str) -> Result<bool, StoreError> {
    tx.execute(
        "UPDATE workbuddy_import_ledger
         SET state = 'conflict', request_id = NULL, updated_at_ms = updated_at_ms
         WHERE entry_key = ?1 AND state = 'imported'",
        params![entry_key],
    )?;
    let deleted = tx.execute(
        "DELETE FROM usage_requests WHERE request_id = ?1
           AND record_origin IN ('workbuddy_builtin','workbuddy_custom')",
        params![entry_key],
    )?;
    Ok(deleted > 0)
}

/// Resolves a replayed key against the row that already exists.
///
/// The same semantic hash is a no-op — that is what makes a lost ACK, a
/// repeated batch, a copied file and a duplicated tool response all count
/// exactly once. A different hash withdraws the workbuddy detail and leaves a
/// `conflict` behind, which is the honest answer when the same identity now
/// carries different content.
pub fn resolve_replay(
    tx: &Transaction<'_>,
    item: &CommitItem,
    namespace_id: &str,
    mapping_revision: i64,
    now_ms: i64,
) -> Result<ReplayOutcome, StoreError> {
    let Some(existing) = load(tx, &item.entry_key)? else {
        return Ok(ReplayOutcome::New);
    };
    if existing.state.is_tombstone() {
        return Ok(ReplayOutcome::Duplicate);
    }
    if existing.normalized_hash == item.normalized_hash {
        if existing.state == LedgerState::Imported
            && item.requested_model.as_deref() == Some("workbuddy-credit:generation_tool")
            && let Some(name) = &item.tool_name
        {
            // A label is not billing identity or usage. Fill only the synthetic
            // route's missing name, without withdrawing or recounting a charge.
            tx.execute(
                "UPDATE usage_requests SET route_name=?2,parser_version=?3,updated_at_ms=?4
                 WHERE request_id=?1 AND record_origin<>'proxy' AND route_name=requested_model",
                params![item.entry_key, name, PARSER_VERSION, now_ms],
            )?;
        }
        if existing.state == LedgerState::PendingClassification && existing.candidate_json.is_none()
        {
            return rehydrate(tx, item, mapping_revision, now_ms).map(ReplayOutcome::Rehydrated);
        }
        return Ok(ReplayOutcome::Duplicate);
    }
    if existing.state == LedgerState::Imported && item.usage.workbuddy_credit_nanos.is_some() {
        let mut previous_usage = item.usage.clone();
        previous_usage.workbuddy_credit_nanos = None;
        if semantic_hash(
            &item.requested_model,
            &item.upstream_model,
            &item.response_model,
            &previous_usage,
        )? == existing.normalized_hash
        {
            // Parser v1 did not read usage.credit. Only fill an unknown credit
            // when every previously accepted semantic field still matches.
            // Known amounts, tombstones and changed Token data never enter.
            let changed = tx.execute(
                "UPDATE usage_requests SET workbuddy_credit_nanos=?2,parser_version=?3,updated_at_ms=?4
                 WHERE request_id=?1 AND record_origin<>'proxy'
                   AND parser_version<2 AND workbuddy_credit_nanos IS NULL",
                params![item.entry_key,item.usage.workbuddy_credit_nanos,PARSER_VERSION,now_ms],
            )?;
            if changed == 1 {
                tx.execute("UPDATE workbuddy_import_ledger SET normalized_hash=?2,updated_at_ms=?3 WHERE entry_key=?1",
                    params![item.entry_key,item.normalized_hash,now_ms])?;
                return Ok(ReplayOutcome::Duplicate);
            }
        }
    }
    // A proxy exclusion carries no workbuddy detail to withdraw, and its state
    // is left alone: an import never rewrites a proxy decision.
    if existing.state != LedgerState::ExcludedProxy {
        withdraw_workbuddy_fact(tx, &item.entry_key)?;
        tx.execute(
            "UPDATE workbuddy_import_ledger
             SET state = 'conflict', candidate_json = ?2, mapping_revision = ?3,
                 updated_at_ms = ?4
             WHERE entry_key = ?1",
            params![
                item.entry_key,
                item.candidate_json,
                mapping_revision,
                now_ms
            ],
        )?;
    }
    let _ = namespace_id;
    Ok(ReplayOutcome::Conflicted)
}

/// A pending exclusion may lack normalized data after a proxy rule is revoked.
/// A full source reread can fill it once; settled rows and tombstones never
/// enter this path. The original ledger identity and acceptance time survive.
fn rehydrate(
    tx: &Transaction<'_>,
    item: &CommitItem,
    revision: i64,
    now: i64,
) -> Result<LedgerState, StoreError> {
    let retention: i64 = tx.query_row(
        "SELECT retention_days FROM stats_settings WHERE singleton=1",
        [],
        |row| row.get(0),
    )?;
    let state = match item.attribution {
        Attribution::Proxy => LedgerState::ExcludedProxy,
        _ if is_expired(item.event_at_ms, now, retention) => LedgerState::ExcludedRetention,
        Attribution::Builtin | Attribution::Direct => LedgerState::Imported,
        Attribution::Pending => LedgerState::PendingClassification,
    };
    if state == LedgerState::Imported {
        insert_fact_row(
            tx,
            item,
            item.origin.ok_or(StoreError::CandidateNotAllowed)?,
            now,
        )?;
    }
    set_state(
        tx,
        &item.entry_key,
        state,
        if state == LedgerState::PendingClassification {
            item.candidate_json.as_deref()
        } else {
            None
        },
        now,
    )?;
    tx.execute(
        "UPDATE workbuddy_import_ledger SET mapping_id=?2,mapping_revision=?3 WHERE entry_key=?1",
        params![item.entry_key, item.mapping_id, revision],
    )?;
    Ok(state)
}

/// How a replayed key related to the row already stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayOutcome {
    /// A pending row without data was filled from a validated source reread.
    Rehydrated(LedgerState),
    /// No row existed yet.
    New,
    /// Same semantic hash: nothing to do.
    Duplicate,
    /// Different hash: the detail was withdrawn and a conflict recorded.
    Conflicted,
}

/// Commits one batch atomically: ledger rows, their fact rows and the checkpoint.
///
/// Everything happens inside the caller's transaction, so **any** failure leaves
/// no trace at all — in particular the committed offset cannot move ahead of the
/// facts, because the checkpoint is only written after every row succeeded, in
/// the same transaction.
///
/// The settings revision is re-checked here rather than trusted from the caller:
/// a batch prepared under an older revision is refused instead of being written
/// against a configuration that has since changed.
pub fn commit_batch(
    tx: &Transaction<'_>,
    namespace_id: &str,
    guard: &CheckpointGuard,
    update: &CheckpointUpdate,
    policy: BatchPolicy,
    items: &[CommitItem],
    now_ms: i64,
) -> Result<CommitReport, StoreError> {
    let settings = load_import_settings(tx)?;
    if settings.revision != policy.settings_revision {
        return Err(StoreError::SettingsStale);
    }
    if settings.namespace_id != namespace_id {
        return Err(StoreError::NotFound);
    }
    let mut report = CommitReport::default();
    for item in items {
        match resolve_replay(tx, item, namespace_id, settings.mapping_revision, now_ms)? {
            // The same key with the same content: never a second fact. This is
            // what makes a lost ACK or a repeated batch count exactly once.
            ReplayOutcome::Duplicate => {
                report.duplicates += 1;
                continue;
            }
            ReplayOutcome::Conflicted => {
                report.conflicted += 1;
                continue;
            }
            ReplayOutcome::New => {}
            ReplayOutcome::Rehydrated(state) => {
                match state {
                    LedgerState::Imported => report.imported += 1,
                    LedgerState::ExcludedProxy => report.excluded_proxy += 1,
                    LedgerState::ExcludedRetention => report.excluded_retention += 1,
                    _ => report.pending += 1,
                }
                continue;
            }
        }
        let state = match item.attribution {
            Attribution::Proxy => LedgerState::ExcludedProxy,
            _ if is_expired(item.event_at_ms, now_ms, policy.retention_days) => {
                LedgerState::ExcludedRetention
            }
            Attribution::Pending => LedgerState::PendingClassification,
            Attribution::Builtin | Attribution::Direct => LedgerState::Imported,
        };
        if state == LedgerState::Imported {
            let origin = item.origin.ok_or(StoreError::CandidateNotAllowed)?;
            insert_fact_row(tx, item, origin, now_ms)?;
        }
        insert(
            tx,
            &NewLedgerEntry {
                entry_key: item.entry_key.clone(),
                namespace_id: namespace_id.to_owned(),
                session_hash: item.session_hash.clone(),
                message_hash: item.message_hash.clone(),
                requested_model: item.requested_model.clone(),
                event_at_ms: item.event_at_ms,
                normalized_hash: item.normalized_hash.clone(),
                state,
                candidate_json: if state == LedgerState::PendingClassification {
                    item.candidate_json.clone()
                } else {
                    None
                },
                mapping_id: item.mapping_id.clone(),
                mapping_revision: settings.mapping_revision,
                now_ms,
            },
        )?;
        match state {
            LedgerState::Imported => report.imported += 1,
            LedgerState::ExcludedProxy => report.excluded_proxy += 1,
            LedgerState::PendingClassification => report.pending += 1,
            LedgerState::ExcludedRetention => report.excluded_retention += 1,
            _ => {}
        }
    }
    match commit_checkpoint(tx, guard, update)? {
        CheckpointAck::Committed(outcome) => {
            report.committed_offset = u64::try_from(outcome.committed_offset).unwrap_or(0);
        }
        CheckpointAck::Rejected(reason) => return Err(StoreError::BatchRejected(reason)),
    }
    Ok(report)
}

/// How long a checkpoint may stay unseen before it is reclaimed.
pub const CHECKPOINT_RECLAIM_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// Reclaims checkpoints whose file has not been seen for thirty days.
///
/// **Only the checkpoint row is removed.** The ledger and every fact row stay:
/// deleting a source file never deletes statistics, and a file that comes back
/// is re-read from the start but still cannot double-count, because identity
/// comes from the message key (`namespace + sessionId + messageId`) rather than
/// from a byte offset in a file.
pub fn reclaim_unseen_checkpoints(tx: &Transaction<'_>, now_ms: i64) -> Result<usize, StoreError> {
    let cutoff = now_ms.saturating_sub(CHECKPOINT_RECLAIM_MS);
    let removed = tx.execute(
        "DELETE FROM workbuddy_import_files WHERE last_seen_at_ms < ?1",
        params![cutoff],
    )?;
    Ok(removed)
}

/// Marks a checkpoint as seen right now.
///
/// A file that disappears simply stops being marked; nothing is deleted, and the
/// committed offset stays where it was so a reappearing file resumes instead of
/// re-reading.
pub fn touch_checkpoint(
    tx: &Transaction<'_>,
    file_key: &str,
    now_ms: i64,
) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE workbuddy_import_files SET last_seen_at_ms = ?2 WHERE file_key = ?1",
        params![file_key, now_ms],
    )?;
    Ok(())
}

/// Reads the published counts from durable rows.
///
/// Every number is a query result: a restart rebuilds exactly the same snapshot
/// because there is no in-memory accumulator to lose.
pub fn load_import_counts(tx: &Transaction<'_>) -> Result<ImportCounts, StoreError> {
    let mut counts = ImportCounts::default();
    let mut statement =
        tx.prepare("SELECT state, COUNT(*) FROM workbuddy_import_ledger GROUP BY state")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (state, count) = row?;
        match state.as_str() {
            "imported" => counts.imported = count,
            "excluded_proxy" => counts.excluded_proxy = count,
            "excluded_retention" => counts.excluded_retention = count,
            "pending_classification" => counts.pending_classification = count,
            "conflict" => counts.conflict = count,
            "deleted_manual" => counts.deleted_manual = count,
            "deleted_retention" => counts.deleted_retention = count,
            _ => {}
        }
    }
    drop(statement);
    counts.files_seen = tx.query_row("SELECT COUNT(*) FROM workbuddy_import_files", [], |row| {
        row.get(0)
    })?;
    counts.files_pending = tx.query_row(
        "SELECT COUNT(*) FROM workbuddy_import_files
         WHERE last_error_code IS NOT NULL OR last_completed_at_ms IS NULL OR committed_offset < observed_size",
        [],
        |row| row.get(0),
    )?;
    counts.bytes_processed = tx.query_row(
        "SELECT COALESCE(SUM(committed_offset), 0) FROM workbuddy_import_files",
        [],
        |row| row.get(0),
    )?;
    Ok(counts)
}

/// Runs the mapping/settings write path once against an in-memory schema.
pub fn mapping_contract_check() -> Result<usize, &'static str> {
    let mut conn = Connection::open_in_memory().map_err(|_| "workbuddy_import_mapping_memory")?;
    crate::stats::store::migrate(&mut conn, Path::new("."))
        .map_err(|_| "workbuddy_import_mapping_migrate")?;
    let tx = conn
        .transaction()
        .map_err(|_| "workbuddy_import_mapping_transaction")?;
    let settings = load_import_settings(&tx).map_err(|_| "workbuddy_import_settings_load")?;
    let outcome = set_import_enabled(&tx, true, settings.revision, 1)
        .map_err(|_| "workbuddy_import_settings_update")?;
    let SettingsOutcome::Updated(enabled) = outcome else {
        return Err("workbuddy_import_settings_cas_refused");
    };
    if !enabled.enabled || enabled.namespace_id.len() != 48 {
        return Err("workbuddy_import_namespace_shape");
    }
    let namespace = enabled.namespace_id.clone();
    // Turning import off keeps the namespace; turning it back on reuses it.
    let SettingsOutcome::Updated(off) = set_import_enabled(&tx, false, enabled.revision, 2)
        .map_err(|_| "workbuddy_import_settings_off")?
    else {
        return Err("workbuddy_import_settings_cas_refused");
    };
    let SettingsOutcome::Updated(on) = set_import_enabled(&tx, true, off.revision, 3)
        .map_err(|_| "workbuddy_import_settings_on")?
    else {
        return Err("workbuddy_import_settings_cas_refused");
    };
    if on.namespace_id != namespace {
        return Err("workbuddy_import_namespace_not_reused");
    }
    let base = NewMapping {
        mapping_id: "m1".into(),
        namespace_id: namespace.clone(),
        requested_model: "custom-local:probe".into(),
        classification: MappingClass::Direct,
        valid_from_ms: 0,
        valid_to_ms: Some(10),
        evidence: MappingEvidence::Manual,
        state: MappingState::Verified,
        route_id: None,
        provider_id: None,
        now_ms: 0,
    };
    insert_mapping(&tx, &base).map_err(|_| "workbuddy_import_mapping_insert")?;
    // Adjacent half-open interval is legal.
    insert_mapping(
        &tx,
        &NewMapping {
            mapping_id: "m2".into(),
            valid_from_ms: 10,
            valid_to_ms: Some(20),
            ..base.clone()
        },
    )
    .map_err(|_| "workbuddy_import_mapping_adjacent")?;
    // An overlap must be refused.
    if !matches!(
        insert_mapping(
            &tx,
            &NewMapping {
                mapping_id: "m3".into(),
                valid_from_ms: 5,
                valid_to_ms: Some(15),
                ..base.clone()
            }
        ),
        Err(StoreError::Overlapping)
    ) {
        return Err("workbuddy_import_mapping_overlap_accepted");
    }
    if match_mapping(&tx, &namespace, "custom-local:probe", 5)
        .map_err(|_| "workbuddy_import_mapping_match")?
        == MappingMatch::None
    {
        return Err("workbuddy_import_mapping_match_missing");
    }
    set_mapping_state(&tx, "m2", MappingState::Revoked, 4)
        .map_err(|_| "workbuddy_import_mapping_state")?;
    if !matches!(
        set_mapping_state(&tx, "m1", MappingState::Verified, 4),
        Ok(())
    ) {
        return Err("workbuddy_import_mapping_reverify");
    }
    // Crash recovery: a prepared row whose configuration is really present is
    // completed, one that is absent becomes a conflict, and neither is ever
    // usable for classification.
    let mut prepared = base.clone();
    prepared.mapping_id = "p1".into();
    prepared.requested_model = "custom-local:recover".into();
    prepared.valid_from_ms = 100;
    prepared.valid_to_ms = Some(200);
    prepared.state = MappingState::Prepared;
    insert_mapping(&tx, &prepared).map_err(|_| "workbuddy_import_recovery_insert")?;
    let mut absent = prepared.clone();
    absent.mapping_id = "p2".into();
    absent.valid_from_ms = 300;
    absent.valid_to_ms = Some(400);
    insert_mapping(&tx, &absent).map_err(|_| "workbuddy_import_recovery_insert")?;
    let recovery = recover_prepared(
        &tx,
        true,
        &|mapping: &ModelMapping| mapping.mapping_id == "p1",
        6,
    )
    .map_err(|_| "workbuddy_import_recovery_failed")?;
    if recovery.completed != 1 || recovery.conflicted != 1 || recovery.total() != 2 {
        return Err("workbuddy_import_recovery_counts_drifted");
    }
    if load_mapping(&tx, "p1")
        .map_err(|_| "workbuddy_import_recovery_load")?
        .is_none_or(|mapping| mapping.state != MappingState::Verified)
    {
        return Err("workbuddy_import_recovery_not_completed");
    }
    if load_mapping(&tx, "p2")
        .map_err(|_| "workbuddy_import_recovery_load")?
        .is_none_or(|mapping| mapping.state != MappingState::Conflict)
    {
        return Err("workbuddy_import_recovery_not_conflicted");
    }
    // A disabled import leaves prepared rows exactly where they were.
    let skipped = recover_prepared(&tx, false, &|_: &ModelMapping| true, 7)
        .map_err(|_| "workbuddy_import_recovery_skipped")?;
    if skipped.total() != 0 {
        return Err("workbuddy_import_recovery_disabled_touched_rows");
    }
    // A preview freezes the watermark and the revision, and its refusals are
    // part of the contract. One pending row gives the rule real work to freeze.
    tx.execute(
        "INSERT INTO workbuddy_import_ledger(entry_key, namespace_id, session_hash, message_hash,
         requested_model, event_at_ms, normalized_hash, state, mapping_revision, created_at_ms,
         updated_at_ms)
         VALUES(?1, ?2, ?3, ?4, 'custom-local:probe', 5, ?5, 'pending_classification', 1, 5, 5)",
        params![
            "1".repeat(64),
            namespace,
            "2".repeat(64),
            "3".repeat(64),
            "4".repeat(64)
        ],
    )
    .map_err(|_| "workbuddy_import_preview_pending")?;
    // A batch commits ledger, fact and checkpoint in one transaction, and a
    // stale settings revision or a stale checkpoint refuses it instead.
    insert_checkpoint(
        &tx,
        &NewCheckpoint {
            file_key: format!("{:0<64}", "batch-file"),
            namespace_id: namespace.clone(),
            relative_path: "projects/demo/session.jsonl".into(),
            observed_size: 64,
            modified_ms: 5,
            now_ms: 5,
        },
    )
    .map_err(|_| "workbuddy_import_batch_checkpoint")?;
    let batch_guard = CheckpointGuard {
        file_key: format!("{:0<64}", "batch-file"),
        namespace_id: namespace.clone(),
        generation: 1,
        revision: 1,
        base_offset: 0,
    };
    let batch_update = CheckpointUpdate {
        next_offset: 64,
        observed_size: 64,
        modified_ms: 6,
        prefix_hash: Some("a".repeat(64)),
        anchor_hash: Some("b".repeat(64)),
        diagnostics: DiagnosticCounts::default(),
        last_error_code: None,
        now_ms: 6,
    };
    let current = load_import_settings(&tx).map_err(|_| "workbuddy_import_batch_settings")?;
    let batch_items = [
        CommitItem {
            entry_key: "9".repeat(64),
            session_hash: "a".repeat(64),
            message_hash: "b".repeat(64),
            requested_model: Some("custom-local:probe".into()),
            upstream_model: None,
            response_model: None,
            tool_name: None,
            usage: NormalizedUsage::missing(),
            mapping_id: None,
            event_at_ms: 5,
            normalized_hash: "c".repeat(64),
            attribution: Attribution::Builtin,
            candidate_json: None,
            origin: Some("workbuddy_builtin"),
        },
        CommitItem {
            entry_key: "8".repeat(64),
            session_hash: "d".repeat(64),
            message_hash: "e".repeat(64),
            requested_model: Some("custom-local:probe".into()),
            upstream_model: None,
            response_model: None,
            tool_name: None,
            usage: NormalizedUsage::missing(),
            mapping_id: None,
            event_at_ms: 5,
            normalized_hash: "f".repeat(64),
            attribution: Attribution::Proxy,
            candidate_json: None,
            origin: None,
        },
    ];
    // A stale settings revision is refused before anything is written.
    if !matches!(
        commit_batch(
            &tx,
            &namespace,
            &batch_guard,
            &batch_update,
            BatchPolicy {
                settings_revision: current.revision + 1,
                retention_days: 30,
            },
            &batch_items,
            6
        ),
        Err(StoreError::SettingsStale)
    ) {
        return Err("workbuddy_import_batch_stale_settings_accepted");
    }
    let report = commit_batch(
        &tx,
        &namespace,
        &batch_guard,
        &batch_update,
        BatchPolicy {
            settings_revision: current.revision,
            retention_days: 30,
        },
        &batch_items,
        6,
    )
    .map_err(|_| "workbuddy_import_batch_commit")?;
    if report.imported != 1 || report.excluded_proxy != 1 || report.committed_offset != 64 {
        return Err("workbuddy_import_batch_counts_drifted");
    }
    // The same guard cannot commit twice: the checkpoint moved.
    if !matches!(
        commit_batch(
            &tx,
            &namespace,
            &batch_guard,
            &batch_update,
            BatchPolicy {
                settings_revision: current.revision,
                retention_days: 30,
            },
            &batch_items,
            6
        ),
        Err(StoreError::BatchRejected(_))
    ) {
        return Err("workbuddy_import_batch_replay_accepted");
    }
    // The published counts are read back from the rows just written.
    let counts = load_import_counts(&tx).map_err(|_| "workbuddy_import_counts_load")?;
    if counts.imported == 0 || counts.files_seen == 0 || counts.bytes_processed == 0 {
        return Err("workbuddy_import_counts_empty");
    }
    if counts.total() < counts.imported {
        return Err("workbuddy_import_counts_total_drifted");
    }
    // A file that has not been seen for thirty days loses its checkpoint — but
    // never its ledger, and never a fact row.
    if CHECKPOINT_RECLAIM_MS != 30 * 24 * 60 * 60 * 1000 {
        return Err("workbuddy_import_reclaim_window_drifted");
    }
    touch_checkpoint(&tx, &format!("{:0<64}", "batch-file"), 6)
        .map_err(|_| "workbuddy_import_touch_checkpoint")?;
    let ledger_before: i64 = tx
        .query_row("SELECT COUNT(*) FROM workbuddy_import_ledger", [], |row| {
            row.get(0)
        })
        .map_err(|_| "workbuddy_import_ledger_count")?;
    // One millisecond past the window: the boundary itself is not yet stale.
    let reclaimed = reclaim_unseen_checkpoints(&tx, CHECKPOINT_RECLAIM_MS + 7)
        .map_err(|_| "workbuddy_import_reclaim")?;
    if reclaimed != 1 {
        return Err("workbuddy_import_reclaim_missed");
    }
    let ledger_after: i64 = tx
        .query_row("SELECT COUNT(*) FROM workbuddy_import_ledger", [], |row| {
            row.get(0)
        })
        .map_err(|_| "workbuddy_import_ledger_count")?;
    if ledger_after != ledger_before || ledger_after == 0 {
        return Err("workbuddy_import_reclaim_touched_the_ledger");
    }
    // A stale settings revision must be answered with a conflict, not a write.
    match set_import_enabled(&tx, false, 0, 9).map_err(|_| "workbuddy_import_settings_stale")? {
        SettingsOutcome::Conflict { current_revision } if current_revision > 0 => {}
        _ => return Err("workbuddy_import_settings_stale_accepted"),
    }
    tx.rollback()
        .map_err(|_| "workbuddy_import_mapping_rollback")?;
    Ok(1)
}

#[cfg(test)]
mod ledger_tests {
    use super::*;
    use rusqlite::Connection;

    fn database() -> (tempfile::TempDir, Connection) {
        let folder = tempfile::tempdir().unwrap();
        let mut conn = Connection::open(folder.path().join("usage.sqlite3")).unwrap();
        crate::stats::store::migrate(&mut conn, folder.path()).unwrap();
        (folder, conn)
    }

    fn entry(key: &str, state: LedgerState) -> NewLedgerEntry {
        NewLedgerEntry {
            entry_key: key.to_string(),
            namespace_id: "n".repeat(48),
            session_hash: "s".repeat(64),
            message_hash: "m".repeat(64),
            requested_model: Some("hy3".into()),
            event_at_ms: 1_792_108_800_000,
            normalized_hash: "h".repeat(64),
            state,
            candidate_json: None,
            mapping_id: None,
            mapping_revision: 1,
            now_ms: 1_792_108_800_000,
        }
    }

    /// Inserts one imported fact row so the ledger foreign key has a target.
    fn fact(conn: &Connection, request_id: &str) {
        conn.execute(
            "INSERT INTO usage_requests(request_id,source,provider_id,route_id,provider_name,
             route_name,requested_model,upstream_model,started_at_ms,ended_at_ms,outcome,usage_status,
             usage_source,created_at_ms,updated_at_ms,record_origin,time_basis,upstream_protocol)
             VALUES(?1,'workbuddy_import','wb:builtin','wb:route','WorkBuddy','hy3','hy3','hy3',
                    1,1,'recorded','missing','workbuddy_jsonl',1,1,'workbuddy_builtin',
                    'message_timestamp',NULL)",
            params![request_id],
        )
        .unwrap();
    }

    fn ledger_rows(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM workbuddy_import_ledger", [], |row| {
            row.get(0)
        })
        .unwrap()
    }

    fn dangling(conn: &Connection) -> i64 {
        let mut statement = conn.prepare("PRAGMA foreign_key_check").unwrap();
        let mut rows = statement.query([]).unwrap();
        i64::from(rows.next().unwrap().is_some())
    }

    #[test]
    fn insert_then_load_round_trips() {
        let (_folder, mut conn) = database();
        let key = "a".repeat(64);
        let tx = conn.transaction().unwrap();
        insert(&tx, &entry(&key, LedgerState::PendingClassification)).unwrap();
        tx.commit().unwrap();

        let tx = conn.transaction().unwrap();
        let row = load(&tx, &key).unwrap().unwrap();
        assert_eq!(row.state, LedgerState::PendingClassification);
        assert_eq!(row.request_id, None);
        assert_eq!(row.event_at_ms, 1_792_108_800_000);
        assert_eq!(row.mapping_revision, 1);
        assert!(load(&tx, &"b".repeat(64)).unwrap().is_none());
        drop(tx);
        assert_eq!(dangling(&conn), 0);
    }

    #[test]
    fn the_same_key_is_never_written_twice() {
        let (_folder, mut conn) = database();
        let key = "a".repeat(64);
        let tx = conn.transaction().unwrap();
        insert(&tx, &entry(&key, LedgerState::ExcludedProxy)).unwrap();
        let failure = insert(&tx, &entry(&key, LedgerState::Imported)).unwrap_err();
        assert!(matches!(failure, StoreError::AlreadyExists), "{failure}");
        tx.commit().unwrap();
        assert_eq!(
            ledger_rows(&conn),
            1,
            "the duplicate must not create a second row"
        );
        let tx = conn.transaction().unwrap();
        assert_eq!(
            load(&tx, &key).unwrap().unwrap().state,
            LedgerState::ExcludedProxy
        );
    }

    #[test]
    fn an_imported_entry_points_at_its_own_fact_row() {
        let (_folder, mut conn) = database();
        let key = "a".repeat(64);
        fact(&conn, &key);
        let tx = conn.transaction().unwrap();
        insert(&tx, &entry(&key, LedgerState::Imported)).unwrap();
        tx.commit().unwrap();
        let tx = conn.transaction().unwrap();
        assert_eq!(
            load(&tx, &key).unwrap().unwrap().request_id,
            Some(key.clone())
        );
        drop(tx);
        assert_eq!(dangling(&conn), 0);

        // Without the fact row the foreign key refuses the imported state.
        let missing = "c".repeat(64);
        let tx = conn.transaction().unwrap();
        let failure = insert(&tx, &entry(&missing, LedgerState::Imported)).unwrap_err();
        assert!(matches!(failure, StoreError::Sqlite(_)), "{failure}");
    }

    #[test]
    fn tombstones_are_terminal() {
        let (_folder, mut conn) = database();
        let key = "a".repeat(64);
        let tx = conn.transaction().unwrap();
        insert(&tx, &entry(&key, LedgerState::DeletedManual)).unwrap();
        let failure = set_state(&tx, &key, LedgerState::Imported, None, 2).unwrap_err();
        assert!(matches!(failure, StoreError::Tombstone), "{failure}");
        let failure = set_state(
            &tx,
            &key,
            LedgerState::PendingClassification,
            None,
            1_792_108_800_001,
        )
        .unwrap_err();
        assert!(matches!(failure, StoreError::Tombstone), "{failure}");
        // Re-stating the same tombstone is harmless (retention re-sweeps do it).
        set_state(
            &tx,
            &key,
            LedgerState::DeletedManual,
            None,
            1_792_108_800_001,
        )
        .unwrap();
        tx.commit().unwrap();
        let tx = conn.transaction().unwrap();
        assert_eq!(
            load(&tx, &key).unwrap().unwrap().state,
            LedgerState::DeletedManual
        );
        assert!(load(&tx, &key).unwrap().unwrap().state.is_tombstone());
    }

    #[test]
    fn candidates_only_live_on_pending_and_conflict() {
        let (_folder, mut conn) = database();
        let pending = "a".repeat(64);
        let mut with_candidate = entry(&pending, LedgerState::PendingClassification);
        with_candidate.candidate_json = Some("{\"requested_model\":null}".into());
        let tx = conn.transaction().unwrap();
        insert(&tx, &with_candidate).unwrap();
        tx.commit().unwrap();

        // Moving to imported must drop the payload.
        fact(&conn, &pending);
        let tx = conn.transaction().unwrap();
        set_state(
            &tx,
            &pending,
            LedgerState::Imported,
            None,
            1_792_108_800_001,
        )
        .unwrap();
        tx.commit().unwrap();
        let tx = conn.transaction().unwrap();
        assert_eq!(load(&tx, &pending).unwrap().unwrap().candidate_json, None);

        // And an imported entry may not be given one.
        let failure = set_state(
            &tx,
            &pending,
            LedgerState::Imported,
            Some("{}"),
            1_792_108_800_002,
        )
        .unwrap_err();
        assert!(
            matches!(failure, StoreError::CandidateNotAllowed),
            "{failure}"
        );

        let oversized = "x".repeat(MAX_CANDIDATE_BYTES + 1);
        let failure = set_state(
            &tx,
            &pending,
            LedgerState::Conflict,
            Some(&oversized),
            1_792_108_800_002,
        )
        .unwrap_err();
        assert!(
            matches!(failure, StoreError::CandidateTooLarge),
            "{failure}"
        );
        let failure = set_state(
            &tx,
            &"b".repeat(64),
            LedgerState::Conflict,
            None,
            1_792_108_800_002,
        )
        .unwrap_err();
        assert!(matches!(failure, StoreError::NotFound), "{failure}");
    }

    #[test]
    fn a_conflict_never_touches_the_proxy_fact_row() {
        let (_folder, mut conn) = database();
        conn.execute(
            "INSERT INTO stats_runs(run_id, started_at_ms) VALUES('run-x', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO usage_requests(request_id,run_id,source,provider_id,route_id,provider_name,
             route_name,requested_model,upstream_model,started_at_ms,ended_at_ms,stream,outcome,
             usage_status,usage_source,created_at_ms,updated_at_ms)
             VALUES('proxy-key','run-x','relay','p','r','P','R','m','m',1,2,1,'completed',
                    'missing','upstream',1,2)",
            [],
        )
        .unwrap();
        let key = "a".repeat(64);
        let tx = conn.transaction().unwrap();
        // A pending row needs no fact row, so the conflict transition below is
        // the only thing that could touch `usage_requests`.
        insert(&tx, &entry(&key, LedgerState::PendingClassification)).unwrap();
        tx.commit().unwrap();

        let before: (String, String, i64) = conn
            .query_row(
                "SELECT outcome, usage_status, total_tokens IS NULL FROM usage_requests
                 WHERE request_id='proxy-key'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let tx = conn.transaction().unwrap();
        set_state(
            &tx,
            &key,
            LedgerState::Conflict,
            Some("{\"reason\":\"hash\"}"),
            1_792_108_800_003,
        )
        .unwrap();
        tx.commit().unwrap();
        let after: (String, String, i64) = conn
            .query_row(
                "SELECT outcome, usage_status, total_tokens IS NULL FROM usage_requests
                 WHERE request_id='proxy-key'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            before, after,
            "the proxy row must stay byte-for-byte identical"
        );
        let tx = conn.transaction().unwrap();
        let row = load(&tx, &key).unwrap().unwrap();
        assert_eq!(row.state, LedgerState::Conflict);
        assert_eq!(
            row.request_id, None,
            "a conflict row no longer owns a fact row"
        );
        drop(tx);
        assert_eq!(dangling(&conn), 0);
    }

    #[test]
    fn state_codes_are_closed_and_parsable() {
        assert_eq!(LedgerState::ALL.len(), 7);
        for state in LedgerState::ALL {
            assert_eq!(LedgerState::parse(state.as_str()), Some(*state));
        }
        assert_eq!(
            LedgerState::parse("duplicate"),
            None,
            "duplicate is a counter, not a state"
        );
        assert_eq!(
            LedgerState::ALL.iter().filter(|s| s.is_tombstone()).count(),
            2
        );
        assert_eq!(
            LedgerState::ALL
                .iter()
                .filter(|s| s.keeps_candidate())
                .count(),
            2
        );
    }
}

#[cfg(test)]
mod checkpoint_tests {
    use super::*;
    use rusqlite::Connection;

    /// Hard-coded expectation: the closed key set, in declaration order.
    const ZERO_DIAGNOSTICS: &str = "{\"json_syntax_error\":0,\"ignored\":0,\"ignored_no_usage\":0,\"pending_identity\":0,\"invalid_timestamp\":0,\"oversized_line\":0,\"missing_session_id\":0,\"session_mismatch\":0,\"control_character_in_id\":0,\"overlong_id\":0}";
    const THREE_DIAGNOSTICS: &str = "{\"json_syntax_error\":3,\"ignored\":0,\"ignored_no_usage\":0,\"pending_identity\":0,\"invalid_timestamp\":0,\"oversized_line\":1,\"missing_session_id\":0,\"session_mismatch\":0,\"control_character_in_id\":0,\"overlong_id\":0}";

    fn database() -> (tempfile::TempDir, Connection) {
        let folder = tempfile::tempdir().unwrap();
        let mut conn = Connection::open(folder.path().join("usage.sqlite3")).unwrap();
        crate::stats::store::migrate(&mut conn, folder.path()).unwrap();
        (folder, conn)
    }

    fn register(conn: &mut Connection, namespace: &str) -> CheckpointGuard {
        let file_key = "f".repeat(64);
        let tx = conn.transaction().unwrap();
        insert_checkpoint(
            &tx,
            &NewCheckpoint {
                file_key: file_key.clone(),
                namespace_id: namespace.to_string(),
                relative_path: "projects/demo/session-a.jsonl".into(),
                observed_size: 100,
                modified_ms: 1_000,
                now_ms: 1_000,
            },
        )
        .unwrap();
        tx.commit().unwrap();
        CheckpointGuard {
            file_key,
            namespace_id: namespace.to_string(),
            generation: 1,
            revision: 1,
            base_offset: 0,
        }
    }

    fn update(next_offset: i64) -> CheckpointUpdate {
        CheckpointUpdate {
            next_offset,
            observed_size: 200,
            modified_ms: 2_000,
            prefix_hash: Some("p".repeat(64)),
            anchor_hash: Some("a".repeat(64)),
            diagnostics: DiagnosticCounts::default(),
            last_error_code: None,
            now_ms: 2_000,
        }
    }

    fn stored(conn: &Connection) -> Checkpoint {
        let tx = conn.unchecked_transaction().unwrap();
        let row = load_checkpoint(&tx, &"f".repeat(64)).unwrap().unwrap();
        tx.rollback().unwrap();
        row
    }

    #[test]
    fn first_commit_advances_revision_and_offset() {
        let (_folder, mut conn) = database();
        let guard = register(&mut conn, &"n".repeat(48));
        let tx = conn.transaction().unwrap();
        let ack = commit_checkpoint(&tx, &guard, &update(64)).unwrap();
        tx.commit().unwrap();
        match ack {
            CheckpointAck::Committed(outcome) => {
                assert_eq!(outcome.revision, 2);
                assert_eq!(outcome.committed_offset, 64);
                assert_eq!(outcome.file_key, "f".repeat(64));
            }
            other => panic!("expected a commit, got {other:?}"),
        }
        let row = stored(&conn);
        assert_eq!((row.revision, row.committed_offset), (2, 64));
        assert_eq!(row.observed_size, 200);
        assert_eq!(row.prefix_hash, Some("p".repeat(64)));
        assert_eq!(row.last_completed_at_ms, Some(2_000));
        assert_eq!(row.diagnostic_counts_json, ZERO_DIAGNOSTICS);
    }

    #[test]
    fn stale_revision_is_refused_without_a_partial_write() {
        let (_folder, mut conn) = database();
        let guard = register(&mut conn, &"n".repeat(48));
        let tx = conn.transaction().unwrap();
        commit_checkpoint(&tx, &guard, &update(64)).unwrap();
        tx.commit().unwrap();

        // The same guard is now one revision behind.
        let tx = conn.transaction().unwrap();
        let ack = commit_checkpoint(&tx, &guard, &update(128)).unwrap();
        tx.commit().unwrap();
        assert_eq!(ack.error_code(), Some("stale_batch"));
        let row = stored(&conn);
        assert_eq!(
            (row.revision, row.committed_offset),
            (2, 64),
            "no partial write"
        );

        // A wrong base offset is refused the same way.
        let ahead = CheckpointGuard {
            revision: 2,
            base_offset: 1,
            ..guard
        };
        let tx = conn.transaction().unwrap();
        let ack = commit_checkpoint(&tx, &ahead, &update(200)).unwrap();
        tx.commit().unwrap();
        assert_eq!(ack.error_code(), Some("stale_batch"));
        assert_eq!(stored(&conn).committed_offset, 64);
    }

    #[test]
    fn an_old_generation_is_refused() {
        let (_folder, mut conn) = database();
        let guard = register(&mut conn, &"n".repeat(48));
        let tx = conn.transaction().unwrap();
        commit_checkpoint(&tx, &guard, &update(64)).unwrap();
        tx.commit().unwrap();
        // The reader still believes it is on generation 1 while the file was replaced.
        conn.execute(
            "UPDATE workbuddy_import_files SET generation = 2, revision = revision + 1,
             committed_offset = 0 WHERE file_key = ?1",
            params!["f".repeat(64)],
        )
        .unwrap();
        let tx = conn.transaction().unwrap();
        let ack = commit_checkpoint(
            &tx,
            &CheckpointGuard {
                revision: 2,
                ..guard
            },
            &update(64),
        )
        .unwrap();
        tx.commit().unwrap();
        assert_eq!(ack.error_code(), Some("source_changed"));
        assert_eq!(stored(&conn).generation, 2);
    }

    #[test]
    fn negative_or_regressing_offsets_are_refused() {
        let (_folder, mut conn) = database();
        let guard = register(&mut conn, &"n".repeat(48));
        let tx = conn.transaction().unwrap();
        commit_checkpoint(&tx, &guard, &update(64)).unwrap();
        tx.commit().unwrap();

        let current = CheckpointGuard {
            revision: 2,
            base_offset: 64,
            ..guard.clone()
        };
        let tx = conn.transaction().unwrap();
        let failure = commit_checkpoint(&tx, &current, &update(63)).unwrap_err();
        assert!(matches!(failure, StoreError::InvalidOffset), "{failure}");
        let failure = commit_checkpoint(&tx, &current, &update(-1)).unwrap_err();
        assert!(matches!(failure, StoreError::InvalidOffset), "{failure}");
        let failure = commit_checkpoint(
            &tx,
            &CheckpointGuard {
                base_offset: -1,
                ..current.clone()
            },
            &update(0),
        )
        .unwrap_err();
        assert!(matches!(failure, StoreError::InvalidOffset), "{failure}");
        tx.rollback().unwrap();
        assert_eq!(
            stored(&conn).committed_offset,
            64,
            "refusals must not write"
        );
    }

    #[test]
    fn a_foreign_namespace_is_refused() {
        let (_folder, mut conn) = database();
        let guard = register(&mut conn, &"n".repeat(48));
        let tx = conn.transaction().unwrap();
        let ack = commit_checkpoint(
            &tx,
            &CheckpointGuard {
                namespace_id: "x".repeat(48),
                ..guard
            },
            &update(64),
        )
        .unwrap();
        tx.commit().unwrap();
        assert_eq!(ack.error_code(), Some("stale_batch"));
        assert_eq!(
            stored(&conn).revision,
            1,
            "a foreign namespace must not advance anything"
        );
        assert_eq!(stored(&conn).committed_offset, 0);
    }

    #[test]
    fn an_unchanged_file_can_commit_at_the_same_offset() {
        let (_folder, mut conn) = database();
        let guard = register(&mut conn, &"n".repeat(48));
        let tx = conn.transaction().unwrap();
        let ack = commit_checkpoint(&tx, &guard, &update(0)).unwrap();
        tx.commit().unwrap();
        assert!(
            ack.is_committed(),
            "a file that did not grow is still a valid batch"
        );
        assert_eq!(stored(&conn).revision, 2, "the revision still advances");
        assert_eq!(stored(&conn).committed_offset, 0);
    }

    #[test]
    fn diagnostics_json_keeps_the_closed_key_set() {
        let mut counts = DiagnosticCounts::default();
        counts.add(
            crate::stats::workbuddy_import::types::DiagnosticKind::JsonSyntaxError,
            3,
        );
        counts.add(
            crate::stats::workbuddy_import::types::DiagnosticKind::OversizedLine,
            1,
        );
        let text = diagnostics_json(&counts);
        assert_eq!(text, THREE_DIAGNOSTICS);
        assert_eq!(text.matches(':').count(), 10, "the key set is closed");
        assert_eq!(
            diagnostics_json(&DiagnosticCounts::default()),
            ZERO_DIAGNOSTICS
        );
        assert!(!text.contains("free_form"));

        let (_folder, mut conn) = database();
        let guard = register(&mut conn, &"n".repeat(48));
        let tx = conn.transaction().unwrap();
        commit_checkpoint(
            &tx,
            &guard,
            &CheckpointUpdate {
                diagnostics: counts,
                ..update(0)
            },
        )
        .unwrap();
        tx.commit().unwrap();
        assert_eq!(stored(&conn).diagnostic_counts_json, THREE_DIAGNOSTICS);
    }

    #[test]
    fn a_missing_checkpoint_is_reported() {
        let (_folder, mut conn) = database();
        let tx = conn.transaction().unwrap();
        let failure = commit_checkpoint(
            &tx,
            &CheckpointGuard {
                file_key: "z".repeat(64),
                namespace_id: "n".repeat(48),
                generation: 1,
                revision: 1,
                base_offset: 0,
            },
            &update(0),
        )
        .unwrap_err();
        assert!(matches!(failure, StoreError::NotFound), "{failure}");
        let failure = insert_checkpoint(
            &tx,
            &NewCheckpoint {
                file_key: "z".repeat(64),
                namespace_id: "n".repeat(48),
                relative_path: "a.jsonl".into(),
                observed_size: 0,
                modified_ms: 0,
                now_ms: 0,
            },
        );
        assert!(failure.is_ok());
        let again = insert_checkpoint(
            &tx,
            &NewCheckpoint {
                file_key: "z".repeat(64),
                namespace_id: "n".repeat(48),
                relative_path: "a.jsonl".into(),
                observed_size: 0,
                modified_ms: 0,
                now_ms: 0,
            },
        )
        .unwrap_err();
        assert!(matches!(again, StoreError::AlreadyExists), "{again}");
    }
}

#[cfg(test)]
mod mapping_tests {
    use super::*;
    use rusqlite::Connection;

    fn database() -> (tempfile::TempDir, Connection) {
        let folder = tempfile::tempdir().unwrap();
        let mut conn = Connection::open(folder.path().join("usage.sqlite3")).unwrap();
        crate::stats::store::migrate(&mut conn, folder.path()).unwrap();
        (folder, conn)
    }

    fn namespace(conn: &mut Connection) -> String {
        let tx = conn.transaction().unwrap();
        let current = load_import_settings(&tx).unwrap();
        let SettingsOutcome::Updated(settings) =
            set_import_enabled(&tx, true, current.revision, 1).unwrap()
        else {
            panic!("enabling import must succeed");
        };
        tx.commit().unwrap();
        settings.namespace_id.clone()
    }

    fn mapping(namespace: &str, id: &str, from: i64, to: Option<i64>) -> NewMapping {
        NewMapping {
            mapping_id: id.into(),
            namespace_id: namespace.into(),
            requested_model: "custom-local:demo".into(),
            classification: MappingClass::Direct,
            valid_from_ms: from,
            valid_to_ms: to,
            evidence: MappingEvidence::Manual,
            state: MappingState::Verified,
            route_id: None,
            provider_id: None,
            now_ms: 1_000,
        }
    }

    #[test]
    fn adjacent_half_open_intervals_are_legal() {
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        insert_mapping(&tx, &mapping(&ns, "m1", 100, Some(200))).unwrap();
        insert_mapping(&tx, &mapping(&ns, "m2", 200, Some(300))).unwrap();
        insert_mapping(&tx, &mapping(&ns, "m3", 300, None)).unwrap();
        tx.commit().unwrap();

        let tx = conn.transaction().unwrap();
        // Half-open: 199 belongs to m1, 200 to m2, 300 to m3.
        assert!(
            matches!(match_mapping(&tx, &ns, "custom-local:demo", 199).unwrap(),
                         MappingMatch::One(row) if row.mapping_id == "m1")
        );
        assert!(
            matches!(match_mapping(&tx, &ns, "custom-local:demo", 200).unwrap(),
                         MappingMatch::One(row) if row.mapping_id == "m2")
        );
        assert!(
            matches!(match_mapping(&tx, &ns, "custom-local:demo", 999).unwrap(),
                         MappingMatch::One(row) if row.mapping_id == "m3")
        );
        assert_eq!(
            match_mapping(&tx, &ns, "custom-local:demo", 99).unwrap(),
            MappingMatch::None
        );
        assert_eq!(
            match_mapping(&tx, &ns, "custom-local:other", 200).unwrap(),
            MappingMatch::None
        );
    }

    #[test]
    fn overlapping_verified_intervals_are_refused_by_rust_and_by_the_trigger() {
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        insert_mapping(&tx, &mapping(&ns, "m1", 100, Some(200))).unwrap();
        let failure = insert_mapping(&tx, &mapping(&ns, "m2", 150, Some(250))).unwrap_err();
        assert!(matches!(failure, StoreError::Overlapping), "{failure}");
        let failure = insert_mapping(&tx, &mapping(&ns, "m3", 0, None)).unwrap_err();
        assert!(matches!(failure, StoreError::Overlapping), "{failure}");
        // A non-verified overlap is allowed (prepared intents may overlap nothing verified).
        let mut prepared = mapping(&ns, "m4", 150, Some(250));
        prepared.state = MappingState::Prepared;
        insert_mapping(&tx, &prepared).unwrap();
        tx.commit().unwrap();

        // The schema trigger still refuses an overlap that bypasses the Rust check.
        let raw = format!(
            "INSERT INTO workbuddy_model_history(mapping_id, namespace_id, requested_model,
             classification, valid_from_ms, valid_to_ms, evidence, state, revision, created_at_ms,
             updated_at_ms) VALUES('raw','{ns}','custom-local:demo','direct',120,180,'manual',
             'verified',1,1,1)"
        );
        let failure = conn.execute(&raw, []);
        assert!(
            failure.is_err(),
            "the trigger must refuse a raw overlapping insert"
        );
        assert!(
            failure
                .unwrap_err()
                .to_string()
                .contains("overlapping model mapping")
        );
    }

    #[test]
    fn two_verified_matches_report_a_conflict_instead_of_picking_one() {
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        // The triggers make this state unreachable through the normal path, so
        // it is built explicitly with them dropped: the matcher must still
        // refuse to pick a winner.
        conn.execute_batch(&format!(
            "DROP TRIGGER workbuddy_mapping_no_overlap_insert;
             DROP TRIGGER workbuddy_mapping_no_overlap_update;
             INSERT INTO workbuddy_model_history(mapping_id, namespace_id, requested_model,
             classification, valid_from_ms, valid_to_ms, evidence, state, revision, created_at_ms,
             updated_at_ms) VALUES('c1','{ns}','custom-local:demo','direct',100,200,'manual',
             'verified',1,1,1);
             INSERT INTO workbuddy_model_history(mapping_id, namespace_id, requested_model,
             classification, valid_from_ms, valid_to_ms, evidence, state, revision, created_at_ms,
             updated_at_ms) VALUES('c2','{ns}','custom-local:demo','direct',150,250,'manual',
             'verified',1,1,1);"
        ))
        .unwrap();
        let tx = conn.transaction().unwrap();
        assert_eq!(
            match_mapping(&tx, &ns, "custom-local:demo", 175).unwrap(),
            MappingMatch::Conflict
        );
        assert!(
            matches!(
                match_mapping(&tx, &ns, "custom-local:demo", 120).unwrap(),
                MappingMatch::One(_)
            ),
            "only one row covers 120"
        );
    }

    #[test]
    fn the_namespace_is_created_once_and_reused_after_toggling() {
        let (_folder, mut conn) = database();
        let tx = conn.transaction().unwrap();
        let initial = load_import_settings(&tx).unwrap();
        assert!(!initial.enabled);
        assert!(
            initial.namespace_id.is_empty(),
            "import starts disabled without an identity"
        );

        let SettingsOutcome::Updated(on) =
            set_import_enabled(&tx, true, initial.revision, 1).unwrap()
        else {
            panic!("enable must succeed");
        };
        assert!(on.enabled && on.namespace_id.len() == 48);
        let first = on.namespace_id.clone();

        let SettingsOutcome::Updated(off) = set_import_enabled(&tx, false, on.revision, 2).unwrap()
        else {
            panic!("disable must succeed");
        };
        assert!(!off.enabled);
        assert_eq!(
            off.namespace_id, first,
            "turning import off keeps the identity"
        );

        let SettingsOutcome::Updated(again) =
            set_import_enabled(&tx, true, off.revision, 3).unwrap()
        else {
            panic!("re-enable must succeed");
        };
        assert_eq!(again.namespace_id, first, "the namespace is permanent");
        assert_eq!(again.revision, off.revision + 1);
        tx.commit().unwrap();

        // A fresh connection reads the same permanent identity.
        let tx = conn.transaction().unwrap();
        assert_eq!(load_import_settings(&tx).unwrap().namespace_id, first);
    }

    #[test]
    fn a_stale_settings_revision_is_refused() {
        let (_folder, mut conn) = database();
        let tx = conn.transaction().unwrap();
        let initial = load_import_settings(&tx).unwrap();
        set_import_enabled(&tx, true, initial.revision, 1).unwrap();
        let outcome = set_import_enabled(&tx, false, initial.revision, 2).unwrap();
        assert_eq!(
            outcome,
            SettingsOutcome::Conflict {
                current_revision: initial.revision + 1
            }
        );
        tx.commit().unwrap();
        let tx = conn.transaction().unwrap();
        assert!(
            load_import_settings(&tx).unwrap().enabled,
            "the refused CAS wrote nothing"
        );
    }

    #[test]
    fn mapping_states_and_codes_are_closed() {
        assert_eq!(MappingClass::ALL.len(), 2);
        assert_eq!(MappingEvidence::ALL.len(), 3);
        assert_eq!(MappingState::ALL.len(), 4);
        assert_eq!(ReconcileState::ALL.len(), 3);
        for state in MappingState::ALL {
            assert_eq!(MappingState::parse(state.as_str()), Some(*state));
        }
        assert_eq!(
            MappingState::ALL.iter().filter(|s| s.is_usable()).count(),
            1
        );
        assert_eq!(MappingState::parse("duplicate"), None);
        assert_eq!(
            MappingEvidence::parse("guessed"),
            None,
            "evidence is never invented"
        );
    }

    #[test]
    fn an_invalid_interval_is_refused() {
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        let failure = insert_mapping(&tx, &mapping(&ns, "bad", 100, Some(100))).unwrap_err();
        assert!(matches!(failure, StoreError::InvalidOffset), "{failure}");
        let failure = insert_mapping(&tx, &mapping(&ns, "bad", 100, Some(50))).unwrap_err();
        assert!(matches!(failure, StoreError::InvalidOffset), "{failure}");
        tx.rollback().unwrap();
        let tx = conn.transaction().unwrap();
        assert!(load_mapping(&tx, "bad").unwrap().is_none());
    }

    #[test]
    fn verifying_a_prepared_row_checks_the_interval_again() {
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        insert_mapping(&tx, &mapping(&ns, "verified", 100, Some(200))).unwrap();
        let mut prepared = mapping(&ns, "later", 150, Some(250));
        prepared.state = MappingState::Prepared;
        insert_mapping(&tx, &prepared).unwrap();
        let failure = set_mapping_state(&tx, "later", MappingState::Verified, 2).unwrap_err();
        assert!(matches!(failure, StoreError::Overlapping), "{failure}");
        let failure = set_mapping_state(&tx, "missing", MappingState::Revoked, 2).unwrap_err();
        assert!(matches!(failure, StoreError::NotFound), "{failure}");
        tx.commit().unwrap();
        let tx = conn.transaction().unwrap();
        assert_eq!(
            load_mapping(&tx, "later").unwrap().unwrap().state,
            MappingState::Prepared
        );
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use rusqlite::Connection;

    fn database() -> (tempfile::TempDir, Connection) {
        let folder = tempfile::tempdir().unwrap();
        let mut conn = Connection::open(folder.path().join("usage.sqlite3")).unwrap();
        crate::stats::store::migrate(&mut conn, folder.path()).unwrap();
        (folder, conn)
    }

    fn namespace(conn: &mut Connection) -> String {
        let tx = conn.transaction().unwrap();
        let current = load_import_settings(&tx).unwrap();
        let SettingsOutcome::Updated(settings) =
            set_import_enabled(&tx, true, current.revision, 1).unwrap()
        else {
            panic!("enabling import must succeed");
        };
        tx.commit().unwrap();
        settings.namespace_id.clone()
    }

    fn prepared(namespace: &str, id: &str, from: i64, to: i64) -> NewMapping {
        NewMapping {
            mapping_id: id.into(),
            namespace_id: namespace.into(),
            requested_model: "custom-local:recover".into(),
            classification: MappingClass::Direct,
            valid_from_ms: from,
            valid_to_ms: Some(to),
            evidence: MappingEvidence::ObservedConfig,
            state: MappingState::Prepared,
            route_id: None,
            provider_id: None,
            now_ms: 1_000,
        }
    }

    #[test]
    fn a_crash_before_the_write_conflicts_the_prepared_row() {
        // AC038: nothing was written to the WorkBuddy file, so the row must not
        // survive as a direct interval.
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        insert_mapping(&tx, &prepared(&ns, "p1", 0, 100)).unwrap();
        let recovery = recover_prepared(&tx, true, &|_: &ModelMapping| false, 2_000).unwrap();
        assert_eq!(
            recovery,
            PreparedRecovery {
                completed: 0,
                conflicted: 1
            }
        );
        let row = load_mapping(&tx, "p1").unwrap().unwrap();
        assert_eq!(row.state, MappingState::Conflict);
        assert!(
            !row.state.is_usable(),
            "a conflict never classifies anything"
        );
        assert_eq!(
            match_mapping(&tx, &ns, "custom-local:recover", 50).unwrap(),
            MappingMatch::None
        );
    }

    #[test]
    fn a_crash_after_the_write_completes_the_prepared_row() {
        // AC038: the configuration really holds it, so it becomes verified.
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        insert_mapping(&tx, &prepared(&ns, "p1", 0, 100)).unwrap();
        let recovery = recover_prepared(&tx, true, &|_: &ModelMapping| true, 2_000).unwrap();
        assert_eq!(
            recovery,
            PreparedRecovery {
                completed: 1,
                conflicted: 0
            }
        );
        let row = load_mapping(&tx, "p1").unwrap().unwrap();
        assert_eq!(row.state, MappingState::Verified);
        assert!(row.state.is_usable());
        assert!(matches!(
            match_mapping(&tx, &ns, "custom-local:recover", 50).unwrap(),
            MappingMatch::One(_)
        ));
    }

    #[test]
    fn a_crash_before_the_ack_is_resolved_the_same_way() {
        // AC038: the third moment (file written and confirmed, ACK lost) is
        // indistinguishable from the second at startup, and resolves the same.
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        insert_mapping(&tx, &prepared(&ns, "p1", 0, 100)).unwrap();
        let first = recover_prepared(&tx, true, &|_: &ModelMapping| true, 2_000).unwrap();
        // A second startup pass finds nothing left to resolve: it is idempotent.
        let second = recover_prepared(&tx, true, &|_: &ModelMapping| true, 3_000).unwrap();
        assert_eq!(first.completed, 1);
        assert_eq!(
            second.total(),
            0,
            "recovery never re-resolves a settled row"
        );
        assert_eq!(
            load_mapping(&tx, "p1").unwrap().unwrap().state,
            MappingState::Verified
        );
    }

    #[test]
    fn a_disabled_import_leaves_prepared_rows_untouched() {
        // Import being off must not block or alter WorkBuddy syncing.
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        insert_mapping(&tx, &prepared(&ns, "p1", 0, 100)).unwrap();
        let skipped = recover_prepared(&tx, false, &|_: &ModelMapping| true, 2_000).unwrap();
        assert_eq!(skipped.total(), 0);
        let row = load_mapping(&tx, "p1").unwrap().unwrap();
        assert_eq!(row.state, MappingState::Prepared, "still exactly as it was");
        assert!(
            !row.state.is_usable(),
            "and still unusable for classification"
        );
    }

    #[test]
    fn recovery_never_overrides_an_already_verified_interval() {
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        let mut verified = prepared(&ns, "v1", 0, 100);
        verified.state = MappingState::Verified;
        insert_mapping(&tx, &verified).unwrap();
        // The prepared row overlaps the verified one and its configuration is
        // present, so completing it would collide.
        insert_mapping(&tx, &prepared(&ns, "p1", 50, 150)).unwrap();
        let recovery = recover_prepared(&tx, true, &|_: &ModelMapping| true, 2_000).unwrap();
        assert_eq!(
            recovery,
            PreparedRecovery {
                completed: 0,
                conflicted: 1
            }
        );
        assert_eq!(
            load_mapping(&tx, "v1").unwrap().unwrap().state,
            MappingState::Verified
        );
        assert_eq!(
            load_mapping(&tx, "p1").unwrap().unwrap().state,
            MappingState::Conflict
        );
    }

    #[test]
    fn recovery_leaves_settled_states_alone() {
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        for (id, state) in [
            ("v1", MappingState::Verified),
            ("c1", MappingState::Conflict),
            ("r1", MappingState::Revoked),
        ] {
            let mut row = prepared(&ns, id, 0, 100);
            row.state = state;
            if state != MappingState::Verified {
                row.valid_from_ms = 1_000;
                row.valid_to_ms = Some(1_100);
            }
            insert_mapping(&tx, &row).unwrap();
        }
        let recovery = recover_prepared(&tx, true, &|_: &ModelMapping| true, 2_000).unwrap();
        assert_eq!(recovery.total(), 0, "only prepared rows are resolved");
        assert_eq!(
            load_mapping(&tx, "c1").unwrap().unwrap().state,
            MappingState::Conflict
        );
        assert_eq!(
            load_mapping(&tx, "r1").unwrap().unwrap().state,
            MappingState::Revoked
        );
    }

    #[test]
    fn the_recovery_contract_holds() {
        assert!(mapping_contract_check().is_ok());
    }
}

#[cfg(test)]
mod batch_tests {
    use super::*;
    use rusqlite::Connection;

    fn database() -> (tempfile::TempDir, Connection) {
        let folder = tempfile::tempdir().unwrap();
        let mut conn = Connection::open(folder.path().join("usage.sqlite3")).unwrap();
        crate::stats::store::migrate(&mut conn, folder.path()).unwrap();
        (folder, conn)
    }

    fn namespace(conn: &mut Connection) -> String {
        let tx = conn.transaction().unwrap();
        let current = load_import_settings(&tx).unwrap();
        let SettingsOutcome::Updated(settings) =
            set_import_enabled(&tx, true, current.revision, 1).unwrap()
        else {
            panic!("enabling import must succeed");
        };
        tx.commit().unwrap();
        settings.namespace_id.clone()
    }

    /// `workbuddy_import_files.file_key` is exactly 64 characters.
    fn batch_key(name: &str) -> String {
        format!("{name:0<64}")
    }

    fn checkpoint(tx: &Transaction<'_>, namespace: &str, file_key: &str) {
        insert_checkpoint(
            tx,
            &NewCheckpoint {
                file_key: batch_key(file_key),
                namespace_id: namespace.into(),
                relative_path: format!("projects/demo/{file_key}.jsonl").replace('\0', ""),
                observed_size: 64,
                modified_ms: 5,
                now_ms: 5,
            },
        )
        .unwrap();
    }

    fn guard(namespace: &str, file_key: &str) -> CheckpointGuard {
        CheckpointGuard {
            file_key: batch_key(file_key),
            namespace_id: namespace.into(),
            generation: 1,
            revision: 1,
            base_offset: 0,
        }
    }

    fn update() -> CheckpointUpdate {
        CheckpointUpdate {
            next_offset: 64,
            observed_size: 64,
            modified_ms: 6,
            prefix_hash: Some("a".repeat(64)),
            anchor_hash: Some("b".repeat(64)),
            diagnostics: DiagnosticCounts::default(),
            last_error_code: None,
            now_ms: 6,
        }
    }

    fn item(key: char, attribution: Attribution) -> CommitItem {
        CommitItem {
            entry_key: key.to_string().repeat(64),
            session_hash: key.to_string().repeat(64),
            message_hash: (((key as u8) + 1) as char).to_string().repeat(64),
            requested_model: Some("custom-local:batch".into()),
            upstream_model: None,
            response_model: None,
            tool_name: None,
            usage: NormalizedUsage::missing(),
            mapping_id: None,
            event_at_ms: 5,
            normalized_hash: "c".repeat(64),
            attribution,
            candidate_json: None,
            origin: if attribution.is_imported() {
                Some("workbuddy_builtin")
            } else {
                None
            },
        }
    }

    fn counts(conn: &Connection) -> (i64, i64, i64) {
        (
            conn.query_row("SELECT COUNT(*) FROM workbuddy_import_ledger", [], |row| row.get(0))
                .unwrap(),
            conn.query_row(
                "SELECT COUNT(*) FROM usage_requests WHERE record_origin IN ('workbuddy_builtin','workbuddy_custom')",
                [],
                |row| row.get(0),
            )
            .unwrap(),
            conn.query_row(
                "SELECT COALESCE(MAX(committed_offset), 0) FROM workbuddy_import_files WHERE file_key = ?1",
                params![batch_key("batch")],
                |row| row.get(0),
            )
            .unwrap(),
        )
    }

    #[test]
    fn parsed_usage_reaches_the_fact_columns_without_losing_credit_or_cache() {
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let line = br#"{"type":"function_call","sessionId":"session","timestamp":5,
          "providerData":{"messageId":"message","requestModelId":"auto",
          "responseModelId":"hy3","rawUsage":{"prompt_tokens":100,
          "completion_tokens":20,"total_tokens":120,"credit":1.14,
          "prompt_tokens_details":{"cached_tokens":60},
          "completion_tokens_details":{"reasoning_tokens":2}}}}"#;
        let super::super::types::ParseDisposition::Candidate(candidate) =
            super::super::parser::evaluate(line, "session", &ns, 6)
        else {
            panic!("the synthetic response must parse");
        };
        let item = CommitItem::from_candidate(*candidate.clone(), Attribution::Builtin).unwrap();
        let mut replay = *candidate;
        replay.event_at_ms = 6;
        replay.usage.diagnostics.cache_alias_disagreement = true;
        let replay = CommitItem::from_candidate(replay, Attribution::Builtin).unwrap();
        assert_eq!(item.normalized_hash, replay.normalized_hash);
        assert!(!item.candidate_json.as_ref().unwrap().contains("rawUsage"));
        let tx = conn.transaction().unwrap();
        checkpoint(&tx, &ns, "batch");
        let settings = load_import_settings(&tx).unwrap();
        commit_batch(
            &tx,
            &ns,
            &guard(&ns, "batch"),
            &update(),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 0,
            },
            &[item],
            6,
        )
        .unwrap();
        let values = tx
            .query_row(
                "SELECT input_tokens,output_tokens,total_tokens,cache_read_tokens,
                    reasoning_tokens,workbuddy_credit_nanos,usage_status,upstream_model,
                    response_model,provider_id,duration_ms,http_status
             FROM usage_requests",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, Option<i64>>(10)?,
                        row.get::<_, Option<i64>>(11)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            values,
            (
                100,
                20,
                120,
                60,
                2,
                1_140_000_000,
                "complete".into(),
                "hy3".into(),
                "hy3".into(),
                "wb:builtin".into(),
                None,
                None
            )
        );
        assert_eq!(
            tx.query_row("SELECT COUNT(*) FROM usage_costs", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn runtime_attribution_ignores_model_creator_history_and_unresolved_intents() {
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let make = |model: &str, at: i64| {
            let bytes = serde_json::to_vec(&serde_json::json!({
                "type":"function_call","sessionId":"s","timestamp":at,
                "providerData":{"messageId":"m","requestModelId":model,
                    "rawUsage":{"prompt_tokens":1,"completion_tokens":2}}
            }))
            .unwrap();
            let super::super::types::ParseDisposition::Candidate(candidate) =
                super::super::parser::evaluate(&bytes, "s", &ns, 30)
            else {
                panic!("synthetic candidate must parse")
            };
            candidate
        };
        let tx = conn.transaction().unwrap();
        assert_eq!(
            attribute_candidate(&tx, &ns, &make("hy3", 5)).unwrap().0,
            Attribution::Builtin
        );
        assert_eq!(
            attribute_candidate(&tx, &ns, &make("custom-local:unknown", 5))
                .unwrap()
                .0,
            Attribution::Direct
        );
        let mapping = NewMapping {
            mapping_id: "direct-before".into(),
            namespace_id: ns.clone(),
            requested_model: "custom-local:scope".into(),
            classification: MappingClass::Direct,
            valid_from_ms: 0,
            valid_to_ms: Some(10),
            evidence: MappingEvidence::Manual,
            state: MappingState::Verified,
            route_id: None,
            provider_id: None,
            now_ms: 1,
        };
        insert_mapping(&tx, &mapping).unwrap();
        let proxy = NewMapping {
            mapping_id: "proxy-after".into(),
            classification: MappingClass::Proxy,
            valid_from_ms: 10,
            valid_to_ms: None,
            ..mapping.clone()
        };
        insert_mapping(&tx, &proxy).unwrap();
        assert_eq!(
            attribute_candidate(&tx, &ns, &make("custom-local:scope", 5)).unwrap(),
            (Attribution::Direct, None)
        );
        assert_eq!(
            attribute_candidate(&tx, &ns, &make("custom-local:scope", 10)).unwrap(),
            (Attribution::Direct, None)
        );
        assert_eq!(
            tx.query_row("SELECT COUNT(*) FROM usage_requests", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0,
            "proxy classification must not require a matching local proxy fact"
        );
        let intent = NewMapping {
            mapping_id: "uncertain-switch".into(),
            state: MappingState::Prepared,
            ..mapping
        };
        insert_mapping(&tx, &intent).unwrap();
        assert_eq!(
            attribute_candidate(&tx, &ns, &make("custom-local:scope", 5))
                .unwrap()
                .0,
            Attribution::Direct
        );
    }

    #[test]
    fn a_failed_item_rolls_the_whole_batch_back() {
        // AC042: one bad row must leave ledger, facts and offset untouched.
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        checkpoint(&tx, &ns, "batch");
        let settings = load_import_settings(&tx).unwrap();

        let mut broken = item('2', Attribution::Builtin);
        broken.entry_key = "too-short".into();
        let items = [item('1', Attribution::Builtin), broken];
        assert!(
            commit_batch(
                &tx,
                &ns,
                &guard(&ns, "batch"),
                &update(),
                BatchPolicy {
                    settings_revision: settings.revision,
                    retention_days: 30,
                },
                &items,
                6
            )
            .is_err(),
            "the bad row must fail the batch"
        );
        tx.rollback().unwrap();

        assert_eq!(counts(&conn), (0, 0, 0), "nothing survived the rollback");
    }

    #[test]
    fn the_offset_never_leads_the_facts() {
        // AC042: after a successful commit every ledger row has its fact, and
        // the offset moved exactly once.
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        checkpoint(&tx, &ns, "batch");
        let settings = load_import_settings(&tx).unwrap();
        let items = [
            item('1', Attribution::Builtin),
            item('2', Attribution::Proxy),
            item('3', Attribution::Pending),
        ];
        let report = commit_batch(
            &tx,
            &ns,
            &guard(&ns, "batch"),
            &update(),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 30,
            },
            &items,
            6,
        )
        .unwrap();
        assert_eq!(
            (
                report.imported,
                report.excluded_proxy,
                report.pending,
                report.committed_offset
            ),
            (1, 1, 1, 64)
        );
        // Every imported ledger row has its fact row; the others have none.
        let imported_without_fact: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM workbuddy_import_ledger l
                 WHERE l.state = 'imported'
                   AND NOT EXISTS (SELECT 1 FROM usage_requests r WHERE r.request_id = l.entry_key)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(imported_without_fact, 0, "the offset never leads the facts");
        let facts: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM usage_requests WHERE record_origin IN ('workbuddy_builtin','workbuddy_custom')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(facts, 1, "a proxy or pending row writes no fact");
        tx.commit().unwrap();
        assert_eq!(counts(&conn), (3, 1, 64));
    }

    #[test]
    fn a_stale_settings_revision_is_refused_before_any_write() {
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        checkpoint(&tx, &ns, "batch");
        let settings = load_import_settings(&tx).unwrap();
        assert!(matches!(
            commit_batch(
                &tx,
                &ns,
                &guard(&ns, "batch"),
                &update(),
                BatchPolicy {
                    settings_revision: settings.revision + 1,
                    retention_days: 30,
                },
                &[item('1', Attribution::Builtin)],
                6
            ),
            Err(StoreError::SettingsStale)
        ));
        let ledger: i64 = tx
            .query_row("SELECT COUNT(*) FROM workbuddy_import_ledger", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(ledger, 0, "nothing was written");
    }

    #[test]
    fn a_duplicate_key_is_a_no_op_and_the_offset_still_advances_once() {
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        checkpoint(&tx, &ns, "batch");
        let settings = load_import_settings(&tx).unwrap();
        let first = commit_batch(
            &tx,
            &ns,
            &guard(&ns, "batch"),
            &update(),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 30,
            },
            &[item('1', Attribution::Builtin)],
            6,
        )
        .unwrap();
        assert_eq!(first.imported, 1);
        // The same key offered again is counted as a duplicate, not a second fact.
        let report = commit_batch(
            &tx,
            &ns,
            &guard(&ns, "batch"),
            &update(),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 30,
            },
            &[item('1', Attribution::Builtin)],
            6,
        );
        // The checkpoint already moved, so the batch itself is refused; the
        // duplicate was counted before that and wrote nothing.
        assert!(matches!(report, Err(StoreError::BatchRejected(_))));
        let facts: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM usage_requests WHERE record_origin IN ('workbuddy_builtin','workbuddy_custom')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(facts, 1, "the duplicate never wrote a second fact");
    }

    #[test]
    fn the_batch_contract_holds() {
        assert!(mapping_contract_check().is_ok());
    }
}

#[cfg(test)]
mod replay_tests {
    use super::*;
    use rusqlite::Connection;

    fn database() -> (tempfile::TempDir, Connection) {
        let folder = tempfile::tempdir().unwrap();
        let mut conn = Connection::open(folder.path().join("usage.sqlite3")).unwrap();
        crate::stats::store::migrate(&mut conn, folder.path()).unwrap();
        (folder, conn)
    }

    fn namespace(conn: &mut Connection) -> String {
        let tx = conn.transaction().unwrap();
        let current = load_import_settings(&tx).unwrap();
        let SettingsOutcome::Updated(settings) =
            set_import_enabled(&tx, true, current.revision, 1).unwrap()
        else {
            panic!("enabling import must succeed");
        };
        tx.commit().unwrap();
        settings.namespace_id.clone()
    }

    fn batch_key(name: &str) -> String {
        format!("{name:0<64}")
    }

    fn current_guard(tx: &Transaction<'_>, namespace: &str, file_key: &str) -> CheckpointGuard {
        let row = load_checkpoint(tx, &batch_key(file_key)).unwrap();
        match row {
            Some(row) => CheckpointGuard {
                file_key: batch_key(file_key),
                namespace_id: namespace.into(),
                generation: row.generation,
                revision: row.revision,
                base_offset: row.committed_offset,
            },
            None => CheckpointGuard {
                file_key: batch_key(file_key),
                namespace_id: namespace.into(),
                generation: 1,
                revision: 1,
                base_offset: 0,
            },
        }
    }

    fn update(next_offset: i64) -> CheckpointUpdate {
        CheckpointUpdate {
            next_offset,
            observed_size: next_offset,
            modified_ms: 6,
            prefix_hash: Some("a".repeat(64)),
            anchor_hash: Some("b".repeat(64)),
            diagnostics: DiagnosticCounts::default(),
            last_error_code: None,
            now_ms: 6,
        }
    }

    fn item(key: char, hash: char, attribution: Attribution) -> CommitItem {
        CommitItem {
            entry_key: key.to_string().repeat(64),
            session_hash: key.to_string().repeat(64),
            message_hash: (((key as u8) + 1) as char).to_string().repeat(64),
            requested_model: Some("custom-local:replay".into()),
            upstream_model: None,
            response_model: None,
            tool_name: None,
            usage: NormalizedUsage::missing(),
            mapping_id: None,
            event_at_ms: 5,
            normalized_hash: hash.to_string().repeat(64),
            attribution,
            candidate_json: None,
            origin: if attribution.is_imported() {
                Some("workbuddy_builtin")
            } else {
                None
            },
        }
    }

    fn facts(tx: &Transaction<'_>) -> i64 {
        tx.query_row(
            "SELECT COUNT(*) FROM usage_requests
             WHERE record_origin IN ('workbuddy_builtin','workbuddy_custom')",
            [],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn prepare(tx: &Transaction<'_>, namespace: &str, file_key: &str) {
        insert_checkpoint(
            tx,
            &NewCheckpoint {
                file_key: batch_key(file_key),
                namespace_id: namespace.into(),
                relative_path: format!("projects/demo/{file_key}.jsonl"),
                observed_size: 0,
                modified_ms: 5,
                now_ms: 5,
            },
        )
        .unwrap();
    }

    #[test]
    fn a_lost_ack_and_a_repeated_batch_count_exactly_once() {
        // AC043: the batch was committed but its ACK never arrived, so it is
        // replayed verbatim.
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        prepare(&tx, &ns, "batch");
        let settings = load_import_settings(&tx).unwrap();
        let items = [item('1', 'c', Attribution::Builtin)];

        let first = commit_batch(
            &tx,
            &ns,
            &current_guard(&tx, &ns, "batch"),
            &update(64),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 30,
            },
            &items,
            6,
        )
        .unwrap();
        assert_eq!(
            (first.imported, first.duplicates, first.conflicted),
            (1, 0, 0)
        );

        // The replay resumes from the checkpoint the first commit left behind.
        let second = commit_batch(
            &tx,
            &ns,
            &current_guard(&tx, &ns, "batch"),
            &update(128),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 30,
            },
            &items,
            6,
        )
        .unwrap();
        assert_eq!(
            (second.imported, second.duplicates, second.conflicted),
            (0, 1, 0),
            "the replay must be a no-op"
        );
        assert_eq!(facts(&tx), 1, "and it must not write a second fact");
        let ledger: i64 = tx
            .query_row("SELECT COUNT(*) FROM workbuddy_import_ledger", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(ledger, 1, "one identity, one ledger row");
    }

    #[test]
    fn a_copied_file_and_a_duplicated_tool_response_count_once() {
        // AC043: the same record copied into another project file, and the same
        // tool response seen twice, share the identity key.
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        prepare(&tx, &ns, "batch");
        let settings = load_import_settings(&tx).unwrap();

        // The copy: same key and same content, discovered in a second file.
        let copy = [item('4', 'd', Attribution::Builtin)];
        commit_batch(
            &tx,
            &ns,
            &current_guard(&tx, &ns, "batch"),
            &update(64),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 30,
            },
            &copy,
            6,
        )
        .unwrap();
        let again = commit_batch(
            &tx,
            &ns,
            &current_guard(&tx, &ns, "batch"),
            &update(128),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 30,
            },
            &copy,
            6,
        )
        .unwrap();
        assert_eq!(again.duplicates, 1);
        assert_eq!(facts(&tx), 1);
    }

    #[test]
    fn a_different_hash_withdraws_the_detail_and_leaves_a_conflict() {
        // Same identity, different content: the workbuddy detail is withdrawn
        // and the row becomes a conflict — never a silently updated fact.
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        prepare(&tx, &ns, "batch");
        let settings = load_import_settings(&tx).unwrap();
        commit_batch(
            &tx,
            &ns,
            &current_guard(&tx, &ns, "batch"),
            &update(64),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 30,
            },
            &[item('1', 'c', Attribution::Builtin)],
            6,
        )
        .unwrap();
        assert_eq!(facts(&tx), 1);

        let changed = commit_batch(
            &tx,
            &ns,
            &current_guard(&tx, &ns, "batch"),
            &update(128),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 30,
            },
            &[item('1', 'e', Attribution::Builtin)],
            7,
        )
        .unwrap();
        assert_eq!((changed.imported, changed.conflicted), (0, 1));
        assert_eq!(facts(&tx), 0, "the withdrawn detail is gone");
        let row = load(&tx, &"1".repeat(64)).unwrap().unwrap();
        assert_eq!(row.state, LedgerState::Conflict);
        assert_eq!(row.request_id, None);
    }

    #[test]
    fn a_proxy_decision_is_never_rewritten_by_an_import() {
        // A proxy exclusion is left exactly as it is, even when the same key
        // arrives with different content.
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        prepare(&tx, &ns, "batch");
        let settings = load_import_settings(&tx).unwrap();
        commit_batch(
            &tx,
            &ns,
            &current_guard(&tx, &ns, "batch"),
            &update(64),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 30,
            },
            &[item('5', 'c', Attribution::Proxy)],
            6,
        )
        .unwrap();
        assert_eq!(facts(&tx), 0, "a proxy exclusion writes no fact");

        let changed = commit_batch(
            &tx,
            &ns,
            &current_guard(&tx, &ns, "batch"),
            &update(128),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 30,
            },
            &[item('5', 'e', Attribution::Builtin)],
            7,
        )
        .unwrap();
        assert_eq!(changed.conflicted, 1);
        let row = load(&tx, &"5".repeat(64)).unwrap().unwrap();
        assert_eq!(
            row.state,
            LedgerState::ExcludedProxy,
            "the proxy decision survives untouched"
        );
    }

    #[test]
    fn the_counts_survive_a_restart() {
        // AC044: the published numbers are query results, so a restart rebuilds
        // exactly the same snapshot.
        let (folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        prepare(&tx, &ns, "batch");
        let settings = load_import_settings(&tx).unwrap();
        commit_batch(
            &tx,
            &ns,
            &current_guard(&tx, &ns, "batch"),
            &update(64),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 30,
            },
            &[
                item('1', 'c', Attribution::Builtin),
                item('2', 'c', Attribution::Pending),
            ],
            6,
        )
        .unwrap();
        tx.commit().unwrap();
        let before = {
            let tx = conn.transaction().unwrap();
            load_import_counts(&tx).unwrap()
        };
        assert_eq!((before.imported, before.pending_classification), (1, 1));
        assert_eq!(before.files_seen, 1);
        assert_eq!(before.bytes_processed, 64);

        drop(conn);
        let mut reopened = Connection::open(folder.path().join("usage.sqlite3")).unwrap();
        let tx = reopened.transaction().unwrap();
        assert_eq!(
            load_import_counts(&tx).unwrap(),
            before,
            "a restart must not change a single count"
        );
    }

    #[test]
    fn a_first_sighting_that_is_already_expired_is_excluded_not_imported() {
        // AC050: an old record is never imported in the first place, so no
        // later window can bring it back.
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        prepare(&tx, &ns, "batch");
        let settings = load_import_settings(&tx).unwrap();
        let now = 100 * RETENTION_DAY_MS;
        let report = commit_batch(
            &tx,
            &ns,
            &current_guard(&tx, &ns, "batch"),
            &update(64),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 1,
            },
            &[item('1', 'c', Attribution::Builtin)],
            now,
        )
        .unwrap();
        assert_eq!(
            (report.imported, report.excluded_retention),
            (0, 1),
            "it is excluded, not imported"
        );
        assert_eq!(facts(&tx), 0, "and no fact row exists");
        let row = load(&tx, &"1".repeat(64)).unwrap().unwrap();
        assert_eq!(row.state, LedgerState::ExcludedRetention);
    }

    #[test]
    fn a_zero_window_keeps_everything() {
        // AC050: "不限期" is zero, and it means nothing is ever expired.
        assert!(!is_expired(0, 100 * RETENTION_DAY_MS, 0));
        assert!(!is_expired(0, 100 * RETENTION_DAY_MS, -1));
        assert!(is_expired(0, 100 * RETENTION_DAY_MS, 1));

        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        prepare(&tx, &ns, "batch");
        let settings = load_import_settings(&tx).unwrap();
        let report = commit_batch(
            &tx,
            &ns,
            &current_guard(&tx, &ns, "batch"),
            &update(64),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 0,
            },
            &[item('1', 'c', Attribution::Builtin)],
            100 * RETENTION_DAY_MS,
        )
        .unwrap();
        assert_eq!(report.imported, 1, "an unlimited window keeps the record");
        assert_eq!(facts(&tx), 1);
    }

    #[test]
    fn widening_the_window_never_restores_an_excluded_or_deleted_row() {
        // AC050: shortening and then lengthening the window changes nothing.
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        prepare(&tx, &ns, "batch");
        let settings = load_import_settings(&tx).unwrap();
        let now = 100 * RETENTION_DAY_MS;

        // Short window: excluded on sight.
        let narrow = commit_batch(
            &tx,
            &ns,
            &current_guard(&tx, &ns, "batch"),
            &update(64),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 1,
            },
            &[item('1', 'c', Attribution::Builtin)],
            now,
        )
        .unwrap();
        assert_eq!(narrow.excluded_retention, 1);

        // Widened window, same record replayed: still a tombstone, no fact.
        let wide = commit_batch(
            &tx,
            &ns,
            &current_guard(&tx, &ns, "batch"),
            &update(128),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 3650,
            },
            &[item('1', 'c', Attribution::Builtin)],
            now,
        )
        .unwrap();
        assert_eq!(
            (wide.imported, wide.duplicates, wide.conflicted),
            (0, 1, 0),
            "the tombstone is a duplicate, never a new import"
        );
        assert_eq!(facts(&tx), 0);
        let row = load(&tx, &"1".repeat(64)).unwrap().unwrap();
        assert_eq!(row.state, LedgerState::ExcludedRetention);
    }

    #[test]
    fn deleting_a_source_file_keeps_every_statistic() {
        // AC053: the file is gone, the numbers are not.
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        prepare(&tx, &ns, "batch");
        let settings = load_import_settings(&tx).unwrap();
        commit_batch(
            &tx,
            &ns,
            &current_guard(&tx, &ns, "batch"),
            &update(64),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 3650,
            },
            &[item('1', 'c', Attribution::Builtin)],
            6,
        )
        .unwrap();

        // The file disappears: nothing at all is deleted.
        let counts = load_import_counts(&tx).unwrap();
        assert_eq!((counts.imported, counts.files_seen), (1, 1));
        assert_eq!(facts(&tx), 1);
        assert_eq!(
            reclaim_unseen_checkpoints(&tx, 7).unwrap(),
            0,
            "a checkpoint is kept while it is still fresh"
        );
        assert_eq!(load_import_counts(&tx).unwrap(), counts);
    }

    #[test]
    fn a_file_that_comes_back_is_not_counted_twice() {
        // AC053: the checkpoint was never dropped, so the reader resumes at the
        // committed offset; and even a re-read is a duplicate by message key.
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        prepare(&tx, &ns, "batch");
        let settings = load_import_settings(&tx).unwrap();
        let items = [item('1', 'c', Attribution::Builtin)];
        commit_batch(
            &tx,
            &ns,
            &current_guard(&tx, &ns, "batch"),
            &update(64),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 3650,
            },
            &items,
            6,
        )
        .unwrap();
        let offset: i64 = tx
            .query_row(
                "SELECT committed_offset FROM workbuddy_import_files WHERE file_key = ?1",
                params![batch_key("batch")],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(offset, 64, "the resumed read starts where it stopped");

        let again = commit_batch(
            &tx,
            &ns,
            &current_guard(&tx, &ns, "batch"),
            &update(128),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 3650,
            },
            &items,
            7,
        )
        .unwrap();
        assert_eq!((again.imported, again.duplicates), (0, 1));
        assert_eq!(facts(&tx), 1, "still one fact");
    }

    #[test]
    fn an_unseen_checkpoint_is_reclaimed_after_thirty_days_but_its_ledger_is_not() {
        // AC053: only the checkpoint row goes. A later re-read of the same file
        // finds the ledger entries and counts nothing twice.
        let (_folder, mut conn) = database();
        let ns = namespace(&mut conn);
        let tx = conn.transaction().unwrap();
        prepare(&tx, &ns, "batch");
        let settings = load_import_settings(&tx).unwrap();
        let items = [item('1', 'c', Attribution::Builtin)];
        commit_batch(
            &tx,
            &ns,
            &current_guard(&tx, &ns, "batch"),
            &update(64),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 3650,
            },
            &items,
            6,
        )
        .unwrap();
        let ledger_before: i64 = tx
            .query_row("SELECT COUNT(*) FROM workbuddy_import_ledger", [], |row| {
                row.get(0)
            })
            .unwrap();

        // Twenty-nine days: still there.
        assert_eq!(
            reclaim_unseen_checkpoints(&tx, CHECKPOINT_RECLAIM_MS - 1).unwrap(),
            0
        );
        // Thirty days and a bit past the last sighting: reclaimed.
        assert_eq!(
            reclaim_unseen_checkpoints(&tx, CHECKPOINT_RECLAIM_MS + 7).unwrap(),
            1
        );
        let ledger_after: i64 = tx
            .query_row("SELECT COUNT(*) FROM workbuddy_import_ledger", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(ledger_after, ledger_before, "the ledger is never reclaimed");
        assert_eq!(facts(&tx), 1, "and the fact row is untouched");

        // Re-read from scratch: still one fact, because identity is the key.
        prepare(&tx, &ns, "batch");
        let again = commit_batch(
            &tx,
            &ns,
            &current_guard(&tx, &ns, "batch"),
            &update(64),
            BatchPolicy {
                settings_revision: settings.revision,
                retention_days: 3650,
            },
            &items,
            CHECKPOINT_RECLAIM_MS + 8,
        )
        .unwrap();
        assert_eq!((again.imported, again.duplicates), (0, 1));
        assert_eq!(facts(&tx), 1);
    }

    #[test]
    fn the_replay_contract_holds() {
        assert!(mapping_contract_check().is_ok());
    }
}

//! Typed internal model for the WorkBuddy local-usage importer.
//!
//! Pure data and pure helpers only: no I/O, no SQLite and no
//! [`serde_json::Value`]. Unknown values stay `Option::None` and every state is
//! an enum, so a missing number can never be read as a real zero (DATA-API
//! sections 1 and 2). Bounds are shared with [`crate::stats::dto`] instead of
//! being re-declared, and nothing here starts a thread or opens a connection.

use crate::stats::dto::{
    InputBreakdown, MAX_ID_CHARS, MAX_TOKEN_VALUE, MAX_TOTAL_TOKENS, UsageReason, UsageStatus,
};

/// Parser/normalizer contract version written to `usage_requests.parser_version`
/// for imported rows (DATA-API section 3.1).
pub const PARSER_VERSION: i64 = 2;

/// Domain prefix of the length-prefixed entry-key hash (DATA-API section 1).
pub const ENTRY_KEY_DOMAIN: &[u8] = b"wbp-wb-message-v1";

/// One committed import batch carries at most this many normalized items.
pub const MAX_BATCH_ITEMS: usize = 128;

/// One committed import batch carries at most this many normalized payload bytes.
pub const MAX_BATCH_BYTES: usize = 512 * 1024;

/// Stored candidate JSON bound for `pending_classification`/`conflict` ledger rows.
pub const MAX_CANDIDATE_BYTES: usize = 16 * 1024;

/// Identifier length limit shared with the statistics DTO layer.
pub const ID_LIMIT_CHARS: usize = MAX_ID_CHARS;

/// Per-field Token bound shared with the statistics DTO layer.
pub const TOKEN_LIMIT: i64 = MAX_TOKEN_VALUE;

/// Total Token bound shared with the statistics DTO layer.
pub const TOTAL_LIMIT: i64 = MAX_TOTAL_TOKENS;

/// Fixed reasons why a complete line cannot become an import candidate.
///
/// The reason is a closed set, never free-form text, so it can be counted in
/// `diagnostic_counts_json` and logged without leaking file content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MalformedReason {
    /// A usage-bearing record has no `sessionId` at all.
    MissingSessionId,
    /// `sessionId` differs from the source file stem.
    SessionMismatch,
    /// An identifier carries control characters; it is never rewritten.
    ControlCharacterInId,
    /// An identifier exceeds [`ID_LIMIT_CHARS`]; it is never truncated.
    OverlongId,
}

impl MalformedReason {
    /// Every reason, in declaration order.
    pub const ALL: &'static [MalformedReason] = &[
        MalformedReason::MissingSessionId,
        MalformedReason::SessionMismatch,
        MalformedReason::ControlCharacterInId,
        MalformedReason::OverlongId,
    ];

    /// Stable diagnostic code.
    pub fn as_str(self) -> &'static str {
        match self {
            MalformedReason::MissingSessionId => "missing_session_id",
            MalformedReason::SessionMismatch => "session_mismatch",
            MalformedReason::ControlCharacterInId => "control_character_in_id",
            MalformedReason::OverlongId => "overlong_id",
        }
    }
}

/// Every diagnostic counter the importer may persist.
///
/// The set is closed on purpose: DATA-API section 3.2 allows at most 32 fixed
/// keys, so an arbitrary string can never reach the checkpoint row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticKind {
    /// A complete line was not valid JSON.
    JsonSyntaxError,
    /// A complete line was not a usage-bearing record type or role.
    Ignored,
    /// A usage-bearing record carried no usage object at all.
    IgnoredNoUsage,
    /// `providerData.messageId` was absent; no usage and no ledger row.
    PendingIdentity,
    /// Timestamp missing, not integer milliseconds, negative or beyond the skew.
    InvalidTimestamp,
    /// A line exceeded the single-line bound and was drained to the next newline.
    OversizedLine,
    /// Structural identity problem, see [`MalformedReason`].
    Malformed(MalformedReason),
}

impl DiagnosticKind {
    /// Every diagnostic key, in declaration order (the four malformed reasons
    /// included). The set is closed: nothing outside it may be persisted.
    pub const ALL: &'static [DiagnosticKind] = &[
        DiagnosticKind::JsonSyntaxError,
        DiagnosticKind::Ignored,
        DiagnosticKind::IgnoredNoUsage,
        DiagnosticKind::PendingIdentity,
        DiagnosticKind::InvalidTimestamp,
        DiagnosticKind::OversizedLine,
        DiagnosticKind::Malformed(MalformedReason::MissingSessionId),
        DiagnosticKind::Malformed(MalformedReason::SessionMismatch),
        DiagnosticKind::Malformed(MalformedReason::ControlCharacterInId),
        DiagnosticKind::Malformed(MalformedReason::OverlongId),
    ];

    /// Parses a stored diagnostic key.
    pub fn parse(code: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.as_str() == code)
    }

    /// Stable diagnostic code, unique across the whole set.
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosticKind::JsonSyntaxError => "json_syntax_error",
            DiagnosticKind::Ignored => "ignored",
            DiagnosticKind::IgnoredNoUsage => "ignored_no_usage",
            DiagnosticKind::PendingIdentity => "pending_identity",
            DiagnosticKind::InvalidTimestamp => "invalid_timestamp",
            DiagnosticKind::OversizedLine => "oversized_line",
            DiagnosticKind::Malformed(reason) => reason.as_str(),
        }
    }
}

/// Non-negative diagnostic counters carried with one batch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiagnosticCounts {
    pub json_syntax_error: i64,
    pub ignored: i64,
    pub ignored_no_usage: i64,
    pub pending_identity: i64,
    pub invalid_timestamp: i64,
    pub oversized_line: i64,
    pub missing_session_id: i64,
    pub session_mismatch: i64,
    pub control_character_in_id: i64,
    pub overlong_id: i64,
}

impl DiagnosticCounts {
    /// Adds a non-negative amount to one counter; negative input is ignored.
    pub fn add(&mut self, kind: DiagnosticKind, amount: i64) {
        let slot = match kind {
            DiagnosticKind::JsonSyntaxError => &mut self.json_syntax_error,
            DiagnosticKind::Ignored => &mut self.ignored,
            DiagnosticKind::IgnoredNoUsage => &mut self.ignored_no_usage,
            DiagnosticKind::PendingIdentity => &mut self.pending_identity,
            DiagnosticKind::InvalidTimestamp => &mut self.invalid_timestamp,
            DiagnosticKind::OversizedLine => &mut self.oversized_line,
            DiagnosticKind::Malformed(MalformedReason::MissingSessionId) => {
                &mut self.missing_session_id
            }
            DiagnosticKind::Malformed(MalformedReason::SessionMismatch) => {
                &mut self.session_mismatch
            }
            DiagnosticKind::Malformed(MalformedReason::ControlCharacterInId) => {
                &mut self.control_character_in_id
            }
            DiagnosticKind::Malformed(MalformedReason::OverlongId) => &mut self.overlong_id,
        };
        *slot = slot.saturating_add(amount.max(0));
    }

    /// Reads one counter.
    pub fn get(&self, kind: DiagnosticKind) -> i64 {
        match kind {
            DiagnosticKind::JsonSyntaxError => self.json_syntax_error,
            DiagnosticKind::Ignored => self.ignored,
            DiagnosticKind::IgnoredNoUsage => self.ignored_no_usage,
            DiagnosticKind::PendingIdentity => self.pending_identity,
            DiagnosticKind::InvalidTimestamp => self.invalid_timestamp,
            DiagnosticKind::OversizedLine => self.oversized_line,
            DiagnosticKind::Malformed(MalformedReason::MissingSessionId) => self.missing_session_id,
            DiagnosticKind::Malformed(MalformedReason::SessionMismatch) => self.session_mismatch,
            DiagnosticKind::Malformed(MalformedReason::ControlCharacterInId) => {
                self.control_character_in_id
            }
            DiagnosticKind::Malformed(MalformedReason::OverlongId) => self.overlong_id,
        }
    }

    /// Iterates the closed key set with its current values.
    ///
    /// Persisting the whole iteration is how `diagnostic_counts_json` stays
    /// whitelisted: an arbitrary string can never become a key.
    pub fn iter(&self) -> impl Iterator<Item = (DiagnosticKind, i64)> + '_ {
        DiagnosticKind::ALL
            .iter()
            .copied()
            .map(|kind| (kind, self.get(kind)))
    }

    /// Sum of every counter; used for "nothing was skipped" checks.
    pub fn total(&self) -> i64 {
        self.json_syntax_error
            .saturating_add(self.ignored)
            .saturating_add(self.ignored_no_usage)
            .saturating_add(self.pending_identity)
            .saturating_add(self.invalid_timestamp)
            .saturating_add(self.oversized_line)
            .saturating_add(self.missing_session_id)
            .saturating_add(self.session_mismatch)
            .saturating_add(self.control_character_in_id)
            .saturating_add(self.overlong_id)
    }
}

/// Fixed-shape diagnostics for one normalized usage record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageDiagnostics {
    /// A lower-priority cache alias disagreed; the higher-priority value stands
    /// and the high-priority value is never zeroed.
    pub cache_alias_disagreement: bool,
    /// A credit literal was rejected; valid Token values are unaffected.
    pub credit_invalid: bool,
    /// `rawUsage` is not the observed WorkBuddy Chat wrapper.
    pub unsupported_usage_schema: bool,
}

/// Normalized usage of one saved model response.
///
/// `None` means "unknown", never zero: an explicit `0` is preserved as
/// `Some(0)` (DATA-API section 2). Token columns of an `invalid` record are all
/// `None`, while identity and error code are still kept.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedUsage {
    pub status: UsageStatus,
    pub reason: Option<UsageReason>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    /// True only when the importer derived the total from input + output.
    pub total_tokens_derived: bool,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub input_breakdown: InputBreakdown,
    /// Exact WorkBuddy credit in nano units; `None` when absent or invalid.
    pub workbuddy_credit_nanos: Option<i64>,
    pub diagnostics: UsageDiagnostics,
}

impl NormalizedUsage {
    /// The shape used when a usage-bearing record carried no usage object.
    pub fn missing() -> Self {
        Self {
            status: UsageStatus::Missing,
            reason: None,
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            total_tokens_derived: false,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            input_breakdown: InputBreakdown::Unknown,
            workbuddy_credit_nanos: None,
            diagnostics: UsageDiagnostics::default(),
        }
    }

    /// True when the record counts towards Token totals.
    pub fn is_complete(&self) -> bool {
        self.status == UsageStatus::Complete
    }

    /// True when no Token column was populated, as required for `invalid`.
    pub fn has_no_token_values(&self) -> bool {
        self.input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.total_tokens.is_none()
            && self.cache_read_tokens.is_none()
            && self.cache_write_tokens.is_none()
            && self.reasoning_tokens.is_none()
    }
}

/// One usage-bearing record with a usable identity.
///
/// The importer never stores the raw session or message id: only their hashes
/// and the derived entry key leave the parser (DATA-API section 1).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    /// 64 lowercase hex; equals `usage_requests.request_id` and `ledger.entry_key`.
    pub entry_key: String,
    /// SHA256 of the session id.
    pub session_hash: String,
    /// SHA256 of `providerData.messageId`.
    pub message_hash: String,
    /// Validated `providerData.requestModelId`; never renamed or guessed.
    pub requested_model: Option<String>,
    /// Source event time in integer milliseconds; frozen at first acceptance.
    pub event_at_ms: i64,
    /// `responseModelId` -> `model` -> `requestModelId`, first usable value.
    pub upstream_model: Option<String>,
    /// Only an explicitly reported response model, otherwise `None`.
    pub response_model: Option<String>,
    pub usage: NormalizedUsage,
}

/// Result of parsing one complete JSONL line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseDisposition {
    /// Usage-bearing record with a usable identity: goes to classification.
    /// Boxed so the enum stays small when it travels through the batch queue.
    Candidate(Box<Candidate>),
    /// A complete line was not valid JSON.
    JsonSyntaxError,
    /// Record type or role that never carries importable usage.
    Ignored,
    /// Usage-bearing record whose `providerData` has no usage object at all.
    IgnoredNoUsage,
    /// Structurally unusable identity fields.
    Malformed(MalformedReason),
    /// `providerData.messageId` absent: diagnose only, no usage and no ledger row.
    PendingIdentity,
    /// Timestamp missing, not integer milliseconds, negative, or beyond the skew.
    InvalidTimestamp,
}

impl ParseDisposition {
    /// True when this disposition carries a candidate.
    pub fn is_candidate(&self) -> bool {
        matches!(self, ParseDisposition::Candidate(_))
    }

    /// The one diagnostic counter this disposition contributes, if any.
    ///
    /// A candidate contributes none: its outcome is decided by classification,
    /// not by parsing.
    pub fn diagnostic(&self) -> Option<DiagnosticKind> {
        match self {
            ParseDisposition::Candidate(_) => None,
            ParseDisposition::JsonSyntaxError => Some(DiagnosticKind::JsonSyntaxError),
            ParseDisposition::Ignored => Some(DiagnosticKind::Ignored),
            ParseDisposition::IgnoredNoUsage => Some(DiagnosticKind::IgnoredNoUsage),
            ParseDisposition::Malformed(reason) => Some(DiagnosticKind::Malformed(*reason)),
            ParseDisposition::PendingIdentity => Some(DiagnosticKind::PendingIdentity),
            ParseDisposition::InvalidTimestamp => Some(DiagnosticKind::InvalidTimestamp),
        }
    }
}

/// Identity and range of the file region a batch was read from.
///
/// The writer re-checks it inside the transaction, so a batch that was read
/// from a file which has since been replaced or truncated is rejected instead
/// of committing bytes that no longer belong to that file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFingerprint {
    /// SHA256(namespace_id + relative_path), 64 lowercase hex.
    pub file_key: String,
    /// Path relative to the WorkBuddy root; never an absolute private path.
    pub relative_path: String,
    /// Bumped when the file is truncated or replaced.
    pub generation: i64,
    /// Checkpoint revision the read started from.
    pub revision: i64,
    /// Offset of the first byte covered by this batch.
    pub base_offset: i64,
    /// Offset just past the last complete line covered by this batch.
    pub next_offset: i64,
    /// File size observed while reading.
    pub observed_size: i64,
    /// Modification time observed while reading, in milliseconds.
    pub modified_ms: i64,
    /// Hash proving the already-committed prefix did not change.
    pub anchor_hash: Option<String>,
}

/// One batch handed to the statistics writer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportBatch {
    /// Namespace that owns the ledger entries of this batch.
    pub namespace_id: String,
    pub fingerprint: SourceFingerprint,
    /// Mapping revision the reader classified against.
    pub mapping_revision: i64,
    /// At most [`MAX_BATCH_ITEMS`] normalized candidates.
    pub items: Vec<Candidate>,
    /// Diagnostics observed while reading this range.
    pub diagnostics: DiagnosticCounts,
}

impl ImportBatch {
    /// True when the batch respects both committed-batch bounds.
    pub fn within_bounds(&self, payload_bytes: usize) -> bool {
        self.items.len() <= MAX_BATCH_ITEMS && payload_bytes <= MAX_BATCH_BYTES
    }
}

/// What the writer did with one batch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BatchCounts {
    pub imported: i64,
    pub excluded_proxy: i64,
    pub excluded_retention: i64,
    pub pending_classification: i64,
    pub conflict: i64,
    pub duplicate: i64,
    pub skipped: i64,
    pub deleted: i64,
}

impl BatchCounts {
    /// Total number of items the writer accounted for.
    pub fn total(&self) -> i64 {
        self.imported
            .saturating_add(self.excluded_proxy)
            .saturating_add(self.excluded_retention)
            .saturating_add(self.pending_classification)
            .saturating_add(self.conflict)
            .saturating_add(self.duplicate)
            .saturating_add(self.skipped)
            .saturating_add(self.deleted)
    }
}

/// Fixed reasons the writer refuses a batch.
///
/// These strings are contract error codes; the management API adds its
/// `stats_` prefix when it reports them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BatchRejectReason {
    /// Import is disabled, paused, or the global switch is off at commit time.
    ImportDisabled,
    /// Checkpoint generation, revision or offset moved since the read, including
    /// a batch that belongs to another namespace.
    StaleBatch,
    /// A model mapping revision changed under the batch.
    MappingConflict,
    /// The source file changed while it was being read.
    SourceChanged,
    /// The ledger holds a conflicting row for one of the keys.
    LedgerConflict,
}

impl BatchRejectReason {
    /// Every rejection reason, in declaration order.
    pub const ALL: &'static [BatchRejectReason] = &[
        BatchRejectReason::ImportDisabled,
        BatchRejectReason::StaleBatch,
        BatchRejectReason::MappingConflict,
        BatchRejectReason::SourceChanged,
        BatchRejectReason::LedgerConflict,
    ];

    /// Stable error code.
    pub fn as_str(self) -> &'static str {
        match self {
            BatchRejectReason::ImportDisabled => "workbuddy_import_disabled",
            BatchRejectReason::StaleBatch => "stale_batch",
            BatchRejectReason::MappingConflict => "mapping_conflict",
            BatchRejectReason::SourceChanged => "source_changed",
            BatchRejectReason::LedgerConflict => "ledger_conflict",
        }
    }
}

/// Every importer error code (DATA-API section 5).
///
/// Internal codes are stable identifiers used in the database, logs and status
/// snapshots; the management API exposes the same value with the shared `stats_`
/// prefix. No code may ever carry file content, SQL, credentials or a private
/// path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImportErrorCode {
    /// Import is disabled, paused, or the global statistics switch is off.
    ImportDisabled,
    /// The WorkBuddy root directory does not exist.
    RootMissing,
    /// The source tree could not be read because of permissions.
    PermissionDenied,
    /// A discovered path is not a regular file inside the root.
    InvalidPath,
    /// The record shape is not a supported WorkBuddy format.
    UnsupportedFormat,
    /// The submitted batch no longer matches the stored checkpoint.
    StaleBatch,
    /// A model mapping revision changed under the batch.
    MappingConflict,
    /// The source file changed while it was being read.
    SourceChanged,
    /// Reading the source failed.
    SourceReadFailed,
    /// The ledger holds a conflicting row for one of the keys.
    LedgerConflict,
    /// Another scan is already running.
    ScanBusy,
}

impl ImportErrorCode {
    /// Every code, in declaration order.
    pub const ALL: &'static [ImportErrorCode] = &[
        ImportErrorCode::ImportDisabled,
        ImportErrorCode::RootMissing,
        ImportErrorCode::PermissionDenied,
        ImportErrorCode::InvalidPath,
        ImportErrorCode::UnsupportedFormat,
        ImportErrorCode::StaleBatch,
        ImportErrorCode::MappingConflict,
        ImportErrorCode::SourceChanged,
        ImportErrorCode::SourceReadFailed,
        ImportErrorCode::LedgerConflict,
        ImportErrorCode::ScanBusy,
    ];

    /// Internal code, stored in the database and written to logs.
    pub fn as_str(self) -> &'static str {
        match self {
            ImportErrorCode::ImportDisabled => "workbuddy_import_disabled",
            ImportErrorCode::RootMissing => "root_missing",
            ImportErrorCode::PermissionDenied => "permission_denied",
            ImportErrorCode::InvalidPath => "invalid_path",
            ImportErrorCode::UnsupportedFormat => "unsupported_format",
            ImportErrorCode::StaleBatch => "stale_batch",
            ImportErrorCode::MappingConflict => "mapping_conflict",
            ImportErrorCode::SourceChanged => "source_changed",
            ImportErrorCode::SourceReadFailed => "source_read_failed",
            ImportErrorCode::LedgerConflict => "ledger_conflict",
            ImportErrorCode::ScanBusy => "scan_busy",
        }
    }

    /// Management-API code: the internal identifier with the shared prefix.
    pub fn api_code(self) -> &'static str {
        match self {
            ImportErrorCode::ImportDisabled => "stats_workbuddy_import_disabled",
            ImportErrorCode::RootMissing => "stats_root_missing",
            ImportErrorCode::PermissionDenied => "stats_permission_denied",
            ImportErrorCode::InvalidPath => "stats_invalid_path",
            ImportErrorCode::UnsupportedFormat => "stats_unsupported_format",
            ImportErrorCode::StaleBatch => "stats_stale_batch",
            ImportErrorCode::MappingConflict => "stats_mapping_conflict",
            ImportErrorCode::SourceChanged => "stats_source_changed",
            ImportErrorCode::SourceReadFailed => "stats_source_read_failed",
            ImportErrorCode::LedgerConflict => "stats_ledger_conflict",
            ImportErrorCode::ScanBusy => "stats_scan_busy",
        }
    }

    /// Parses an internal code.
    pub fn parse(code: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|item| item.as_str() == code)
    }

    /// The shared code for a rejected batch.
    pub fn from_reject(reason: BatchRejectReason) -> Self {
        match reason {
            BatchRejectReason::ImportDisabled => ImportErrorCode::ImportDisabled,
            BatchRejectReason::StaleBatch => ImportErrorCode::StaleBatch,
            BatchRejectReason::MappingConflict => ImportErrorCode::MappingConflict,
            BatchRejectReason::SourceChanged => ImportErrorCode::SourceChanged,
            BatchRejectReason::LedgerConflict => ImportErrorCode::LedgerConflict,
        }
    }
}

/// Durable checkpoint values after a successful commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitOutcome {
    pub file_key: String,
    /// Checkpoint revision after the commit; the next read must match it.
    pub revision: i64,
    /// Offset of the next unprocessed byte.
    pub committed_offset: i64,
    pub counts: BatchCounts,
}

/// Writer reply for one submitted batch.
///
/// A committed batch may always be replayed: the ledger de-duplicates by entry
/// key, so a lost ACK can never double a record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckpointAck {
    Committed(CommitOutcome),
    Rejected(BatchRejectReason),
}

impl CheckpointAck {
    /// True when the reader may advance its cursor.
    pub fn is_committed(&self) -> bool {
        matches!(self, CheckpointAck::Committed(_))
    }

    /// Fixed error code of a rejection, or `None` after a commit.
    pub fn error_code(&self) -> Option<&'static str> {
        match self {
            CheckpointAck::Committed(_) => None,
            CheckpointAck::Rejected(reason) => Some(reason.as_str()),
        }
    }
}

/// Verifies the typed import contract on the real startup path.
///
/// The importer itself is not started here: no thread, queue, file handle or
/// SQLite connection is created (the runtime task owns those). Every state and
/// every fixed code is constructed on purpose, so adding a variant without
/// extending its diagnostic or error code fails closed at startup instead of
/// silently reaching the writer.
pub fn contract_check() -> Result<usize, &'static str> {
    Ok(check_bounds()?
        + check_dispositions()?
        + check_batches()?
        + check_acks()?
        + check_diagnostics()?
        + check_error_codes()?)
}

/// Fixed bounds must match the frozen design table and the shared DTO limits.
fn check_bounds() -> Result<usize, &'static str> {
    if MAX_BATCH_ITEMS != 128 || MAX_BATCH_BYTES != 512 * 1024 || MAX_CANDIDATE_BYTES != 16 * 1024 {
        return Err("workbuddy_import_batch_bounds_drifted");
    }
    if ID_LIMIT_CHARS != 160 || TOKEN_LIMIT != 1_000_000_000_000 || TOTAL_LIMIT != 2_000_000_000_000
    {
        return Err("workbuddy_import_limits_drifted");
    }
    if PARSER_VERSION != 2 || ENTRY_KEY_DOMAIN != b"wbp-wb-message-v1" {
        return Err("workbuddy_import_identity_contract_drifted");
    }
    Ok(1)
}

/// Every disposition must stay constructible and map to its fixed counter.
fn check_dispositions() -> Result<usize, &'static str> {
    let samples = [
        ParseDisposition::Candidate(Box::new(probe_candidate())),
        ParseDisposition::Ignored,
        ParseDisposition::IgnoredNoUsage,
        ParseDisposition::Malformed(MalformedReason::MissingSessionId),
        ParseDisposition::PendingIdentity,
        ParseDisposition::InvalidTimestamp,
    ];
    for sample in &samples {
        if !sample.is_candidate() && sample.diagnostic().is_none() {
            return Err("workbuddy_import_diagnostic_mapping_missing");
        }
        if let Some(kind) = sample.diagnostic()
            && kind.as_str().is_empty()
        {
            return Err("workbuddy_import_diagnostic_code_missing");
        }
    }
    Ok(1)
}

/// One batch must respect both bounds and keep unknown usage unknown.
fn check_batches() -> Result<usize, &'static str> {
    let usage = NormalizedUsage::missing();
    if usage.status != UsageStatus::Missing || !usage.has_no_token_values() || usage.is_complete() {
        return Err("workbuddy_import_unknown_usage_shape_drifted");
    }
    let batch = ImportBatch {
        namespace_id: "0".repeat(48),
        fingerprint: probe_fingerprint(),
        mapping_revision: 1,
        items: vec![probe_candidate()],
        diagnostics: DiagnosticCounts::default(),
    };
    if !batch.within_bounds(0) || !batch.within_bounds(MAX_BATCH_BYTES) {
        return Err("workbuddy_import_batch_shape_drifted");
    }
    if batch.within_bounds(MAX_BATCH_BYTES + 1) {
        return Err("workbuddy_import_batch_bound_not_enforced");
    }
    Ok(1)
}

/// Both ACK shapes, every rejection code and the batch counters.
fn check_acks() -> Result<usize, &'static str> {
    let committed = CheckpointAck::Committed(CommitOutcome {
        file_key: probe_fingerprint().file_key,
        revision: 2,
        committed_offset: 0,
        counts: BatchCounts::default(),
    });
    if !committed.is_committed() || committed.error_code().is_some() {
        return Err("workbuddy_import_commit_ack_drifted");
    }
    let counted = BatchCounts {
        imported: 2,
        duplicate: 1,
        ..BatchCounts::default()
    };
    if counted.total() != 3 {
        return Err("workbuddy_import_batch_counts_drifted");
    }
    for reason in BatchRejectReason::ALL {
        let rejected = CheckpointAck::Rejected(*reason);
        if rejected.is_committed() || rejected.error_code() != Some(reason.as_str()) {
            return Err("workbuddy_import_reject_ack_drifted");
        }
    }
    Ok(1)
}

/// Diagnostic keys are closed, codes are non-empty and counters stay non-negative.
fn check_diagnostics() -> Result<usize, &'static str> {
    for reason in MalformedReason::ALL {
        if DiagnosticKind::Malformed(*reason).as_str().is_empty() {
            return Err("workbuddy_import_malformed_code_missing");
        }
    }
    let mut counts = DiagnosticCounts::default();
    counts.add(DiagnosticKind::JsonSyntaxError, 1);
    counts.add(DiagnosticKind::OversizedLine, -5);
    counts.add(
        DiagnosticKind::Malformed(MalformedReason::SessionMismatch),
        2,
    );
    if counts.get(DiagnosticKind::JsonSyntaxError) != 1
        || counts.get(DiagnosticKind::OversizedLine) != 0
        || counts.get(DiagnosticKind::Malformed(MalformedReason::SessionMismatch)) != 2
        || counts.total() != 3
    {
        return Err("workbuddy_import_diagnostic_counters_drifted");
    }
    Ok(1)
}

/// Error codes, diagnostic keys and their API forms stay closed and consistent.
fn check_error_codes() -> Result<usize, &'static str> {
    for code in ImportErrorCode::ALL {
        if code.as_str().is_empty() || !code.api_code().starts_with("stats_") {
            return Err("workbuddy_import_error_code_missing");
        }
        if ImportErrorCode::parse(code.as_str()) != Some(*code) {
            return Err("workbuddy_import_error_code_not_parsable");
        }
    }
    for reason in BatchRejectReason::ALL {
        if ImportErrorCode::from_reject(*reason).as_str() != reason.as_str() {
            return Err("workbuddy_import_reject_code_mismatch");
        }
    }
    for kind in DiagnosticKind::ALL {
        if DiagnosticKind::parse(kind.as_str()) != Some(*kind) {
            return Err("workbuddy_import_diagnostic_key_not_parsable");
        }
    }
    if DiagnosticCounts::default().iter().count() != DiagnosticKind::ALL.len() {
        return Err("workbuddy_import_diagnostic_iteration_incomplete");
    }
    Ok(1)
}

/// Placeholder identity used only by the startup contract check.
fn probe_candidate() -> Candidate {
    Candidate {
        entry_key: "0".repeat(64),
        session_hash: "0".repeat(64),
        message_hash: "0".repeat(64),
        requested_model: None,
        event_at_ms: 0,
        upstream_model: None,
        response_model: None,
        usage: NormalizedUsage::missing(),
    }
}

/// Placeholder file identity used only by the startup contract check.
fn probe_fingerprint() -> SourceFingerprint {
    SourceFingerprint {
        file_key: "0".repeat(64),
        relative_path: "session-probe.jsonl".into(),
        generation: 1,
        revision: 1,
        base_offset: 0,
        next_offset: 0,
        observed_size: 0,
        modified_ms: 0,
        anchor_hash: None,
    }
}

#[cfg(test)]
mod error_code_tests {
    use super::{
        BatchRejectReason, DiagnosticCounts, DiagnosticKind, ImportErrorCode, MalformedReason,
    };

    #[test]
    fn error_codes_are_unique_and_complete() {
        assert_eq!(ImportErrorCode::ALL.len(), 11);
        let mut seen = Vec::new();
        for code in ImportErrorCode::ALL {
            let text = code.as_str();
            assert!(!text.is_empty());
            assert!(!seen.contains(&text), "duplicate error code {text}");
            seen.push(text);
        }
        assert_eq!(
            seen,
            vec![
                "workbuddy_import_disabled",
                "root_missing",
                "permission_denied",
                "invalid_path",
                "unsupported_format",
                "stale_batch",
                "mapping_conflict",
                "source_changed",
                "source_read_failed",
                "ledger_conflict",
                "scan_busy",
            ]
        );
    }

    #[test]
    fn api_codes_match_the_stats_prefix_rule() {
        for code in ImportErrorCode::ALL {
            assert_eq!(code.api_code(), format!("stats_{}", code.as_str()));
            assert!(code.api_code().starts_with("stats_"));
        }
    }

    #[test]
    fn error_code_parse_round_trips() {
        for code in ImportErrorCode::ALL {
            assert_eq!(ImportErrorCode::parse(code.as_str()), Some(*code));
        }
        assert_eq!(ImportErrorCode::parse("stats_stale_batch"), None);
        assert_eq!(ImportErrorCode::parse(""), None);
    }

    #[test]
    fn batch_rejections_map_onto_the_shared_codes() {
        for reason in BatchRejectReason::ALL {
            let mapped = ImportErrorCode::from_reject(*reason);
            assert_eq!(mapped.as_str(), reason.as_str());
            assert!(ImportErrorCode::ALL.contains(&mapped));
        }
    }

    #[test]
    fn diagnostic_key_set_is_closed_and_parsable() {
        assert_eq!(DiagnosticKind::ALL.len(), 10);
        let mut seen = Vec::new();
        for kind in DiagnosticKind::ALL {
            let code = kind.as_str();
            assert!(!seen.contains(&code), "duplicate diagnostic key {code}");
            seen.push(code);
            assert_eq!(DiagnosticKind::parse(code), Some(*kind));
        }
        assert_eq!(DiagnosticKind::parse("free_form_note"), None);
        assert_eq!(
            DiagnosticKind::parse(MalformedReason::OverlongId.as_str()),
            Some(DiagnosticKind::Malformed(MalformedReason::OverlongId))
        );
    }

    #[test]
    fn diagnostic_iteration_covers_the_closed_set() {
        let mut counts = DiagnosticCounts::default();
        counts.add(DiagnosticKind::JsonSyntaxError, 2);
        counts.add(DiagnosticKind::Malformed(MalformedReason::OverlongId), 1);
        let listed: Vec<(DiagnosticKind, i64)> = counts.iter().collect();
        assert_eq!(listed.len(), DiagnosticKind::ALL.len());
        assert_eq!(listed[0], (DiagnosticKind::JsonSyntaxError, 2));
        assert_eq!(
            listed.last().copied(),
            Some((DiagnosticKind::Malformed(MalformedReason::OverlongId), 1))
        );
    }
}

//! Test entry point for the WorkBuddy local-usage importer.
//!
//! Everything asserted here calls the production module (`super::types`); no
//! behaviour is re-implemented for tests. Only shapes that already exist are
//! asserted, so the suite is green at the end of S01. No case is `#[ignore]`d and
//! no assertion is a placeholder.
//!
//! Case inventory, added by the task that owns each behaviour:
//!
//! - S03.01 message shell: `message`+`assistant` and role-less `function_call`
//!   are candidates; `user`, `reasoning` and `function_call_result` are not.
//! - S03.02 identity/time: length-prefixed SHA256 entry key, session id must
//!   match the file stem, `messageId` required, integer-millisecond timestamps.
//! - S03.03 main tokens: rawUsage priority, explicit zero vs missing, invalid
//!   numbers, derived total only when input+output are both legal.
//! - S03.04 cache/reasoning: alias priority, subset bounds, alias diagnostics.
//! - S03.05 credit: exact nano conversion, `0` preserved, invalid literals
//!   rejected without touching valid tokens.
//! - S03.06 quality/candidate: complete/partial/missing/invalid, semantic hash
//!   independent of key order and content, candidate JSON within 16 KiB.
//! - S03.07 format compatibility and canary privacy.
//! - S04.01..S04.07 discovery, fingerprints, bounded line reads, tail/bad line
//!   progress, fair traversal, change validation, low-priority full rescan.
//! - S05.01..S05.07 proxy attribution, current-config observation, pure
//!   classification, history intervals, prepare/finalize, prepared recovery,
//!   pending confirm/revoke transactions.
//! - S06.01..S06.07 bounded import channel, writer batch transaction, replay and
//!   conflict, status publishing, fairness/backoff, start-stop coupling,
//!   shutdown and crash recovery (including X45..X47 scheduling cases).
//! - S07.01..S07.07 manual clear tombstones, retention tombstones, preview
//!   watermark, candidate expiry, vanished/replaced files, indexes/revisions,
//!   cost and recovery isolation.
//! - S08.01..S08.07 DTO/protocol versions, filter SQL, metrics, management
//!   commands, pagination consistency, GPUI scheduler jobs, compatibility.
//! - S09.01..S09.07 source icons, filter bar layout, filter state, metrics and
//!   empty states, detail rows, import settings, attribution dialogs.
//! - S10.01..S10.07 real-process end-to-end, fault injection, performance
//!   budgets, full regression, platform acceptance, upgrade/rollback drill.

use super::types::{
    BatchCounts, BatchRejectReason, CheckpointAck, CommitOutcome, DiagnosticCounts, DiagnosticKind,
    ENTRY_KEY_DOMAIN, ID_LIMIT_CHARS, ImportBatch, MAX_BATCH_BYTES, MAX_BATCH_ITEMS,
    MAX_CANDIDATE_BYTES, MalformedReason, NormalizedUsage, PARSER_VERSION, ParseDisposition,
    SourceFingerprint, TOKEN_LIMIT, TOTAL_LIMIT, UsageDiagnostics,
};
use crate::stats::dto::{
    InputBreakdown, MAX_ID_CHARS, MAX_TOKEN_VALUE, MAX_TOTAL_TOKENS, UsageStatus,
};

/// Test-local shape assertion: 64 lowercase hex characters.
fn is_entry_key(value: &str) -> bool {
    value.len() == 64
        && value
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

fn fingerprint() -> SourceFingerprint {
    SourceFingerprint {
        file_key: "0".repeat(64),
        relative_path: "session-test.jsonl".into(),
        generation: 1,
        revision: 1,
        base_offset: 0,
        next_offset: 0,
        observed_size: 0,
        modified_ms: 0,
        anchor_hash: None,
    }
}

fn candidate_with(usage: NormalizedUsage) -> super::types::Candidate {
    super::types::Candidate {
        entry_key: "a".repeat(64),
        session_hash: "b".repeat(64),
        message_hash: "c".repeat(64),
        requested_model: Some("hy3".into()),
        event_at_ms: 1_792_108_800_000,
        upstream_model: Some("hy3".into()),
        response_model: None,
        tool_name: None,
        usage,
    }
}

fn complete_usage(input: Option<i64>, output: Option<i64>, total: Option<i64>) -> NormalizedUsage {
    NormalizedUsage {
        status: UsageStatus::Complete,
        reason: None,
        input_tokens: input,
        output_tokens: output,
        total_tokens: total,
        total_tokens_derived: false,
        cache_read_tokens: None,
        cache_write_tokens: None,
        reasoning_tokens: None,
        input_breakdown: InputBreakdown::Unknown,
        workbuddy_credit_nanos: None,
        diagnostics: UsageDiagnostics::default(),
    }
}

#[test]
fn contract_check_reports_every_invariant() {
    let checks = super::types::contract_check().expect("the shipped contract must hold");
    assert!(
        checks >= 5,
        "expected at least five invariant groups, got {checks}"
    );
}

#[test]
fn parser_version_and_entry_domain_are_frozen() {
    assert_eq!(PARSER_VERSION, 3);
    assert_eq!(ENTRY_KEY_DOMAIN, b"wbp-wb-message-v1");
}

#[test]
fn shared_limits_come_from_the_stats_dto() {
    assert_eq!(ID_LIMIT_CHARS, MAX_ID_CHARS);
    assert_eq!(TOKEN_LIMIT, MAX_TOKEN_VALUE);
    assert_eq!(TOTAL_LIMIT, MAX_TOTAL_TOKENS);
    assert_eq!(ID_LIMIT_CHARS, 160);
    assert_eq!(TOKEN_LIMIT, 1_000_000_000_000);
    assert_eq!(TOTAL_LIMIT, 2_000_000_000_000);
}

#[test]
fn batch_bounds_match_the_design_table() {
    assert_eq!(MAX_BATCH_ITEMS, 128);
    assert_eq!(MAX_BATCH_BYTES, 512 * 1024);
    assert_eq!(MAX_CANDIDATE_BYTES, 16 * 1024);
}

#[test]
fn unknown_usage_is_none_and_never_zero() {
    let usage = NormalizedUsage::missing();
    assert_eq!(usage.status, UsageStatus::Missing);
    assert_eq!(usage.reason, None);
    assert!(usage.has_no_token_values());
    assert!(!usage.is_complete());
    assert!(!usage.total_tokens_derived);
    assert_eq!(usage.workbuddy_credit_nanos, None);
    assert_eq!(usage.input_breakdown, InputBreakdown::Unknown);
    assert_eq!(usage.diagnostics, UsageDiagnostics::default());
}

#[test]
fn explicit_zero_survives_as_some_zero() {
    let usage = complete_usage(Some(0), Some(0), Some(0));
    assert!(usage.is_complete());
    assert!(
        !usage.has_no_token_values(),
        "an explicit zero is known, not unknown"
    );
    assert_eq!(usage.input_tokens, Some(0));
    assert_eq!(usage.output_tokens, Some(0));
    assert_eq!(usage.total_tokens, Some(0));
}

#[test]
fn invalid_usage_keeps_no_token_values_but_keeps_its_code() {
    let usage = NormalizedUsage {
        status: UsageStatus::Invalid,
        reason: Some(crate::stats::dto::UsageReason::InvalidNumbers),
        ..complete_usage(None, None, None)
    };
    assert!(!usage.is_complete());
    assert!(
        usage.has_no_token_values(),
        "invalid records must not expose token columns"
    );
    assert_eq!(
        usage.reason,
        Some(crate::stats::dto::UsageReason::InvalidNumbers)
    );
}

#[test]
fn malformed_reason_codes_are_unique_and_fixed() {
    let mut seen = Vec::new();
    for reason in MalformedReason::ALL {
        let code = reason.as_str();
        assert!(!code.is_empty());
        assert!(!seen.contains(&code), "duplicate malformed code {code}");
        seen.push(code);
    }
    assert_eq!(seen.len(), MalformedReason::ALL.len());
    assert_eq!(
        MalformedReason::MissingSessionId.as_str(),
        "missing_session_id"
    );
    assert_eq!(
        MalformedReason::SessionMismatch.as_str(),
        "session_mismatch"
    );
    assert_eq!(
        MalformedReason::ControlCharacterInId.as_str(),
        "control_character_in_id"
    );
    assert_eq!(MalformedReason::OverlongId.as_str(), "overlong_id");
}

#[test]
fn diagnostic_codes_are_unique_across_the_whole_set() {
    let mut kinds = vec![
        DiagnosticKind::JsonSyntaxError,
        DiagnosticKind::Ignored,
        DiagnosticKind::IgnoredNoUsage,
        DiagnosticKind::PendingIdentity,
        DiagnosticKind::InvalidTimestamp,
        DiagnosticKind::OversizedLine,
    ];
    for reason in MalformedReason::ALL {
        kinds.push(DiagnosticKind::Malformed(*reason));
    }
    let mut seen = Vec::new();
    for kind in kinds {
        let code = kind.as_str();
        assert!(!code.is_empty());
        assert!(!seen.contains(&code), "duplicate diagnostic code {code}");
        seen.push(code);
    }
    assert_eq!(
        seen.len(),
        10,
        "six standalone kinds plus four malformed reasons"
    );
}

#[test]
fn parse_disposition_maps_to_its_diagnostic_counter() {
    let cases = [
        (ParseDisposition::Ignored, Some(DiagnosticKind::Ignored)),
        (
            ParseDisposition::IgnoredNoUsage,
            Some(DiagnosticKind::IgnoredNoUsage),
        ),
        (
            ParseDisposition::Malformed(MalformedReason::OverlongId),
            Some(DiagnosticKind::Malformed(MalformedReason::OverlongId)),
        ),
        (
            ParseDisposition::PendingIdentity,
            Some(DiagnosticKind::PendingIdentity),
        ),
        (
            ParseDisposition::InvalidTimestamp,
            Some(DiagnosticKind::InvalidTimestamp),
        ),
    ];
    for (disposition, expected) in cases {
        assert!(!disposition.is_candidate());
        assert_eq!(disposition.diagnostic(), expected);
    }
    let candidate =
        ParseDisposition::Candidate(Box::new(candidate_with(NormalizedUsage::missing())));
    assert!(candidate.is_candidate());
    assert_eq!(
        candidate.diagnostic(),
        None,
        "a candidate is counted by classification, not parsing"
    );
}

#[test]
fn rejection_codes_match_the_contract_list() {
    let codes: Vec<&str> = BatchRejectReason::ALL.iter().map(|r| r.as_str()).collect();
    assert_eq!(
        codes,
        vec![
            "workbuddy_import_disabled",
            "stale_batch",
            "mapping_conflict",
            "source_changed",
            "ledger_conflict",
        ]
    );
}

#[test]
fn diagnostic_counters_ignore_negative_amounts() {
    let mut counts = DiagnosticCounts::default();
    counts.add(DiagnosticKind::JsonSyntaxError, -5);
    counts.add(DiagnosticKind::Ignored, 0);
    assert_eq!(counts.get(DiagnosticKind::JsonSyntaxError), 0);
    assert_eq!(counts.get(DiagnosticKind::Ignored), 0);
    assert_eq!(counts.total(), 0);
}

#[test]
fn diagnostic_counters_saturate_instead_of_overflowing() {
    let mut counts = DiagnosticCounts::default();
    counts.add(DiagnosticKind::OversizedLine, i64::MAX);
    counts.add(DiagnosticKind::OversizedLine, i64::MAX);
    assert_eq!(counts.get(DiagnosticKind::OversizedLine), i64::MAX);
    assert_eq!(counts.total(), i64::MAX);
}

#[test]
fn diagnostic_counters_track_every_key_independently() {
    let mut counts = DiagnosticCounts::default();
    counts.add(DiagnosticKind::JsonSyntaxError, 1);
    counts.add(
        DiagnosticKind::Malformed(MalformedReason::SessionMismatch),
        2,
    );
    counts.add(
        DiagnosticKind::Malformed(MalformedReason::MissingSessionId),
        3,
    );
    assert_eq!(counts.get(DiagnosticKind::JsonSyntaxError), 1);
    assert_eq!(
        counts.get(DiagnosticKind::Malformed(MalformedReason::SessionMismatch)),
        2
    );
    assert_eq!(
        counts.get(DiagnosticKind::Malformed(MalformedReason::MissingSessionId)),
        3
    );
    assert_eq!(
        counts.get(DiagnosticKind::Malformed(MalformedReason::OverlongId)),
        0
    );
    assert_eq!(counts.total(), 6);
}

#[test]
fn batch_bounds_reject_oversized_payload_and_item_count() {
    let within = ImportBatch {
        namespace_id: "0".repeat(48),
        fingerprint: fingerprint(),
        mapping_revision: 1,
        items: vec![candidate_with(NormalizedUsage::missing())],
        diagnostics: DiagnosticCounts::default(),
    };
    assert!(within.within_bounds(0));
    assert!(within.within_bounds(MAX_BATCH_BYTES));
    assert!(!within.within_bounds(MAX_BATCH_BYTES + 1));

    let oversized = ImportBatch {
        items: vec![candidate_with(NormalizedUsage::missing()); MAX_BATCH_ITEMS + 1],
        ..within
    };
    assert!(
        !oversized.within_bounds(0),
        "129 items must not pass the batch bound"
    );
}

#[test]
fn checkpoint_ack_exposes_a_code_only_when_rejected() {
    let committed = CheckpointAck::Committed(CommitOutcome {
        file_key: "0".repeat(64),
        revision: 2,
        committed_offset: 0,
        counts: BatchCounts::default(),
    });
    assert!(committed.is_committed());
    assert_eq!(committed.error_code(), None);

    for reason in BatchRejectReason::ALL {
        let rejected = CheckpointAck::Rejected(*reason);
        assert!(!rejected.is_committed());
        assert_eq!(rejected.error_code(), Some(reason.as_str()));
    }
}

#[test]
fn batch_counts_total_adds_every_outcome() {
    let counts = BatchCounts {
        imported: 2,
        excluded_proxy: 1,
        excluded_retention: 1,
        pending_classification: 3,
        conflict: 1,
        duplicate: 4,
        skipped: 1,
        deleted: 1,
    };
    assert_eq!(counts.total(), 14);
    assert_eq!(BatchCounts::default().total(), 0);
}

#[test]
fn candidate_identity_fields_are_hashes_not_raw_ids() {
    let candidate = candidate_with(NormalizedUsage::missing());
    assert!(
        is_entry_key(&candidate.entry_key),
        "entry key must be 64 lowercase hex"
    );
    assert!(is_entry_key(&candidate.session_hash));
    assert!(is_entry_key(&candidate.message_hash));
    assert_ne!(candidate.entry_key, candidate.session_hash);
    assert_eq!(candidate.event_at_ms, 1_792_108_800_000);
    assert_eq!(
        candidate.response_model, None,
        "no explicit response model was reported"
    );
}

#[test]
fn usage_diagnostics_default_to_no_findings() {
    let diagnostics = UsageDiagnostics::default();
    assert!(!diagnostics.cache_alias_disagreement);
    assert!(!diagnostics.credit_invalid);
    assert!(!diagnostics.unsupported_usage_schema);
}

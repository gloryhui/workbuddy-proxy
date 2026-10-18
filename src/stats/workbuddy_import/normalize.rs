//! Main token normalization for the WorkBuddy local-usage importer.
//!
//! The rules are the fixed priority list of DATA-API section 2:
//!
//! * `input`: `rawUsage.prompt_tokens` → `rawUsage.input_tokens` → `usage.inputTokens`
//! * `output`: `rawUsage.completion_tokens` → `rawUsage.output_tokens` → `usage.outputTokens`
//! * `total`: `rawUsage.total_tokens` → `usage.totalTokens`, otherwise derived with
//!   a checked addition when both input and output are known
//!
//! A value that is present but illegal is **never** masked by a lower-priority
//! alias, a missing value stays `None` (never zero), and two wrappers that
//! disagree about the same fact make the record `invalid` with
//! `conflicting_usage` instead of picking a winner. Cache, reasoning and credit
//! are filled by their own rules; this module only sets the main three fields
//! plus the status and reason.

use crate::stats::dto::{InputBreakdown, UsageReason, UsageStatus};
use crate::stats::workbuddy_import::parser::{NormalizedUsageFields, NumberLiteral, RawUsage};
use crate::stats::workbuddy_import::types::{NormalizedUsage, TOKEN_LIMIT, TOTAL_LIMIT};

/// Why a main token field was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenRejection {
    /// Present, but not a plain integer (float, string, bool, out of range).
    NotAnInteger,
    /// A negative count.
    Negative,
    /// Above the per-field or total bound.
    TooLarge,
    /// An explicit total does not equal `input + output`.
    InconsistentTotal,
    /// `rawUsage` and `usage` disagree about the same fact.
    ConflictingSources,
    /// Deriving the total overflowed `i64`.
    Overflow,
}

impl TokenRejection {
    /// The fixed usage reason this rejection maps to.
    pub fn reason(self) -> UsageReason {
        match self {
            TokenRejection::InconsistentTotal | TokenRejection::ConflictingSources => {
                UsageReason::ConflictingUsage
            }
            _ => UsageReason::InvalidNumbers,
        }
    }
}

/// Accepts one literal against a bound.
fn accept(literal: NumberLiteral, limit: i64) -> Result<i64, TokenRejection> {
    match literal {
        NumberLiteral::Other => Err(TokenRejection::NotAnInteger),
        NumberLiteral::Integer(value) if value < 0 => Err(TokenRejection::Negative),
        NumberLiteral::Integer(value) if value > limit => Err(TokenRejection::TooLarge),
        NumberLiteral::Integer(value) => Ok(value),
    }
}

/// The first alias that is actually present, in priority order.
fn first_present(candidates: [Option<NumberLiteral>; 3]) -> Option<NumberLiteral> {
    candidates.into_iter().flatten().next()
}

/// Resolves one fact across both wrappers.
///
/// `raw` is the highest-priority `rawUsage` alias, `normalized` the `usage`
/// counterpart. When both are present they must agree, otherwise the record is
/// a conflict. An illegal high-priority value is an error, never a fallback.
fn resolve(
    raw: Option<NumberLiteral>,
    normalized: Option<NumberLiteral>,
    limit: i64,
) -> Result<Option<i64>, TokenRejection> {
    match (raw, normalized) {
        (Some(raw), Some(normalized)) => {
            let raw_value = accept(raw, limit)?;
            let normalized_value = accept(normalized, limit)?;
            if raw_value != normalized_value {
                return Err(TokenRejection::ConflictingSources);
            }
            Ok(Some(raw_value))
        }
        (Some(raw), None) => Ok(Some(accept(raw, limit)?)),
        (None, Some(normalized)) => Ok(Some(accept(normalized, limit)?)),
        (None, None) => Ok(None),
    }
}

/// Normalizes the three main token fields into a usage record.
///
/// Only `status`, `reason`, `input_tokens`, `output_tokens`, `total_tokens`,
/// `total_tokens_derived` and `input_breakdown` are set here; cache, reasoning
/// and credit stay unknown for their own rules to fill.
pub fn normalize_usage(
    raw: Option<&RawUsage>,
    normalized: Option<&NormalizedUsageFields>,
) -> NormalizedUsage {
    let mut usage = NormalizedUsage::missing();
    usage.input_breakdown = InputBreakdown::Unknown;

    let input_raw = first_present([
        raw.and_then(|usage| usage.prompt_tokens),
        raw.and_then(|usage| usage.input_tokens),
        None,
    ]);
    let output_raw = first_present([
        raw.and_then(|usage| usage.completion_tokens),
        raw.and_then(|usage| usage.output_tokens),
        None,
    ]);
    let total_raw = raw.and_then(|usage| usage.total_tokens);

    let input_normalized = normalized.and_then(|usage| usage.input_tokens);
    let output_normalized = normalized.and_then(|usage| usage.output_tokens);
    let total_normalized = normalized.and_then(|usage| usage.total_tokens);

    let input = match resolve(input_raw, input_normalized, TOKEN_LIMIT) {
        Ok(value) => value,
        Err(rejection) => return invalid(usage, rejection),
    };
    let output = match resolve(output_raw, output_normalized, TOKEN_LIMIT) {
        Ok(value) => value,
        Err(rejection) => return invalid(usage, rejection),
    };

    let (total, derived) = match resolve(total_raw, total_normalized, TOTAL_LIMIT) {
        Ok(Some(total)) => (Some(total), false),
        Ok(None) => match (input, output) {
            (Some(input), Some(output)) => match input.checked_add(output) {
                // Both halves are already bounded, so the sum can only exceed
                // the total bound, never `i64`.
                Some(sum) if sum <= TOTAL_LIMIT => (Some(sum), true),
                Some(_) => return invalid(usage, TokenRejection::TooLarge),
                None => return invalid(usage, TokenRejection::Overflow),
            },
            _ => (None, false),
        },
        Err(rejection) => return invalid(usage, rejection),
    };

    // An explicit total must equal the sum; it is never "corrected" to it, and a
    // missing half is never recovered by subtraction.
    if let (Some(total), Some(input), Some(output)) = (total, input, output)
        && !derived
        && total != input.saturating_add(output)
    {
        return invalid(usage, TokenRejection::InconsistentTotal);
    }

    usage.input_tokens = input;
    usage.output_tokens = output;
    usage.total_tokens = total;
    usage.total_tokens_derived = derived;
    usage.status = match (input, output, total) {
        (Some(_), Some(_), Some(_)) => UsageStatus::Complete,
        (None, None, None) => UsageStatus::Missing,
        _ => UsageStatus::Partial,
    };
    usage
}

/// Builds the `invalid` shape: every token column stays `None`.
fn invalid(mut usage: NormalizedUsage, rejection: TokenRejection) -> NormalizedUsage {
    usage.status = UsageStatus::Invalid;
    usage.reason = Some(rejection.reason());
    usage.input_tokens = None;
    usage.output_tokens = None;
    usage.total_tokens = None;
    usage.total_tokens_derived = false;
    usage
}

/// The outcome of picking one fact from an alias list.
struct AliasPick {
    /// The first legal value, in priority order.
    chosen: Option<i64>,
    /// A lower-priority alias carried a different legal value.
    disagreement: bool,
    /// At least one alias was present but illegal.
    illegal: bool,
}

/// Picks the first legal alias, remembering disagreements and illegal aliases.
///
/// The cache compatibility rule of DATA-API section 2 is explicit here: the
/// highest-priority **legal** alias wins, a lower alias that merely disagrees is
/// recorded as `cache_alias_disagreement` and never zeroes the winner, and a
/// value that is present but illegal is never silently used. When no legal alias
/// exists but an illegal one does, the caller invalidates the record.
fn pick_alias(candidates: [Option<NumberLiteral>; 4], limit: i64) -> AliasPick {
    let mut chosen = None;
    let mut disagreement = false;
    let mut illegal = false;
    for literal in candidates.into_iter().flatten() {
        match accept(literal, limit) {
            Ok(value) => match chosen {
                None => chosen = Some(value),
                Some(first) if first != value => disagreement = true,
                Some(_) => {}
            },
            Err(_) => illegal = true,
        }
    }
    AliasPick {
        chosen,
        disagreement,
        illegal,
    }
}

fn detail_cache_read(raw: Option<&RawUsage>) -> Option<NumberLiteral> {
    raw.and_then(|usage| usage.prompt_tokens_details.as_ref())
        .and_then(|details| details.cached_tokens)
}

fn detail_reasoning(raw: Option<&RawUsage>) -> Option<NumberLiteral> {
    raw.and_then(|usage| usage.completion_tokens_details.as_ref())
        .and_then(|details| details.reasoning_tokens)
}

/// Fills the cache and reasoning fields of an already normalized record.
///
/// Priority lists come from DATA-API section 2. Cache counts are never added to
/// the input count, `reasoning` may not exceed `output`, `cache_read` and
/// `cache_write` may not exceed `input`, and the input composition is only ever
/// labelled `read_subset` or `unknown` — never `disjoint`.
pub fn normalize_cache_and_reasoning(
    usage: &mut NormalizedUsage,
    raw: Option<&RawUsage>,
    normalized: Option<&NormalizedUsageFields>,
) {
    if usage.status == UsageStatus::Invalid {
        return;
    }
    let first_detail = |usage: Option<&NormalizedUsageFields>| -> Option<NumberLiteral> {
        usage
            .and_then(|fields| fields.input_tokens_details.as_ref())
            .and_then(|details| details.first())
            .and_then(|detail| detail.cached_tokens)
    };
    let first_creation = |usage: Option<&NormalizedUsageFields>| -> Option<NumberLiteral> {
        usage
            .and_then(|fields| fields.input_tokens_details.as_ref())
            .and_then(|details| details.first())
            .and_then(|detail| detail.cache_creation_input_tokens)
    };
    let first_output_reasoning = |usage: Option<&NormalizedUsageFields>| -> Option<NumberLiteral> {
        usage
            .and_then(|fields| fields.output_tokens_details.as_ref())
            .and_then(|details| details.first())
            .and_then(|detail| detail.reasoning_tokens)
    };

    let cache_read = pick_alias(
        [
            detail_cache_read(raw),
            raw.and_then(|usage| usage.prompt_cache_hit_tokens),
            raw.and_then(|usage| usage.cache_read_input_tokens),
            first_detail(normalized),
        ],
        TOKEN_LIMIT,
    );
    let cache_write = pick_alias(
        [
            raw.and_then(|usage| usage.prompt_cache_write_tokens),
            raw.and_then(|usage| usage.cache_creation_input_tokens),
            first_creation(normalized),
            None,
        ],
        TOKEN_LIMIT,
    );
    let reasoning = pick_alias(
        [
            detail_reasoning(raw),
            raw.and_then(|usage| usage.completion_thinking_tokens),
            first_output_reasoning(normalized),
            None,
        ],
        TOKEN_LIMIT,
    );

    // An alias list whose only entries were illegal cannot be ignored.
    if (cache_read.illegal && cache_read.chosen.is_none())
        || (cache_write.illegal && cache_write.chosen.is_none())
        || (reasoning.illegal && reasoning.chosen.is_none())
    {
        *usage = invalid(usage.clone(), TokenRejection::NotAnInteger);
        return;
    }

    // Subset constraints: a cache count is part of the input, reasoning is part
    // of the output. Violations make the whole record invalid.
    if let (Some(read), Some(input)) = (cache_read.chosen, usage.input_tokens)
        && read > input
    {
        *usage = invalid(usage.clone(), TokenRejection::TooLarge);
        return;
    }
    if let (Some(write), Some(input)) = (cache_write.chosen, usage.input_tokens)
        && write > input
    {
        *usage = invalid(usage.clone(), TokenRejection::TooLarge);
        return;
    }
    if let (Some(reasoning), Some(output)) = (reasoning.chosen, usage.output_tokens)
        && reasoning > output
    {
        *usage = invalid(usage.clone(), TokenRejection::TooLarge);
        return;
    }

    usage.cache_read_tokens = cache_read.chosen;
    usage.cache_write_tokens = cache_write.chosen;
    usage.reasoning_tokens = reasoning.chosen;
    usage.diagnostics.cache_alias_disagreement =
        cache_read.disagreement || cache_write.disagreement || reasoning.disagreement;
    // The composition of the input is only ever a known subset or unknown.
    usage.input_breakdown = if usage.cache_read_tokens.is_some() {
        InputBreakdown::ReadSubset
    } else {
        InputBreakdown::Unknown
    };
    // Cache-only records carry information, so they are no longer "missing".
    if usage.status == UsageStatus::Missing
        && (usage.cache_read_tokens.is_some()
            || usage.cache_write_tokens.is_some()
            || usage.reasoning_tokens.is_some())
    {
        usage.status = UsageStatus::Partial;
    }
}

/// Nano units in one WorkBuddy credit.
pub const CREDIT_NANOS_PER_UNIT: i64 = 1_000_000_000;

/// The largest number of fractional digits a credit literal may carry.
pub const MAX_CREDIT_FRACTION_DIGITS: usize = 9;

/// Why a credit literal was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreditRejection {
    /// Not a plain decimal literal.
    NotDecimal,
    /// Exponent notation: the exact value cannot be recovered safely.
    ScientificNotation,
    /// A negative credit.
    Negative,
    /// More fractional digits than [`MAX_CREDIT_FRACTION_DIGITS`].
    TooManyFractionDigits,
    /// The scaled value does not fit in `i64` nanos.
    TooLarge,
}

/// Parses a credit literal into exact nano units.
///
/// The literal is handled as text: no `f64` ever touches it, so `1.14` becomes
/// exactly `1_140_000_000`. Exponent notation, signs, extra dots, more than nine
/// fractional digits and anything that overflows `i64` nanos are refused.
pub fn parse_credit_nanos(literal: &str) -> Result<i64, CreditRejection> {
    if literal.is_empty() {
        return Err(CreditRejection::NotDecimal);
    }
    if literal.contains(['e', 'E']) {
        return Err(CreditRejection::ScientificNotation);
    }
    if literal.starts_with('-') {
        return Err(CreditRejection::Negative);
    }
    if !literal
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return Err(CreditRejection::NotDecimal);
    }
    let mut parts = literal.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = match parts.next() {
        None => "",
        Some("") => return Err(CreditRejection::NotDecimal),
        Some(value) => value,
    };
    if parts.next().is_some() || whole.is_empty() {
        return Err(CreditRejection::NotDecimal);
    }
    if fraction.len() > MAX_CREDIT_FRACTION_DIGITS {
        return Err(CreditRejection::TooManyFractionDigits);
    }
    let whole_value: i64 = whole.parse().map_err(|_| CreditRejection::TooLarge)?;
    let mut nanos = whole_value
        .checked_mul(CREDIT_NANOS_PER_UNIT)
        .ok_or(CreditRejection::TooLarge)?;
    if !fraction.is_empty() {
        let digits: i64 = fraction.parse().map_err(|_| CreditRejection::TooLarge)?;
        let mut scale = 1i64;
        for _ in fraction.len()..MAX_CREDIT_FRACTION_DIGITS {
            scale *= 10;
        }
        let fraction_nanos = digits.checked_mul(scale).ok_or(CreditRejection::TooLarge)?;
        nanos = nanos
            .checked_add(fraction_nanos)
            .ok_or(CreditRejection::TooLarge)?;
    }
    Ok(nanos)
}

/// Fills the credit field of a normalized record.
///
/// A refused literal only raises `credit_invalid`: it never invalidates the
/// record and never touches the Token values (DATA-API section 2).
pub fn normalize_credit(usage: &mut NormalizedUsage, raw: Option<&RawUsage>) {
    let Some(literal) = raw.and_then(|usage| usage.credit.as_ref()) else {
        return;
    };
    match parse_credit_nanos(literal.as_str()) {
        Ok(nanos) => usage.workbuddy_credit_nanos = Some(nanos),
        Err(_) => {
            usage.workbuddy_credit_nanos = None;
            usage.diagnostics.credit_invalid = true;
        }
    }
}

/// Reads a credit literal the way the importer does: through the real parser.
///
/// Used by the startup check and the tests so the credit fixtures cannot drift
/// away from what `providerData.rawUsage.credit` actually produces.
fn credit_literal(line: &[u8]) -> Option<crate::stats::workbuddy_import::parser::NumberText> {
    match crate::stats::workbuddy_import::parser::parse_shell(line) {
        Ok(crate::stats::workbuddy_import::parser::Shell::Usage(shell)) => shell
            .raw_usage
            .as_ref()
            .and_then(|usage| usage.credit.clone()),
        _ => None,
    }
}

/// Startup contract check for the main token rules.
///
/// Runs fixed synthetic wrappers on the real startup path, so a rule change
/// that would start inventing counts fails closed.
pub fn normalize_contract_check() -> Result<usize, &'static str> {
    let complete = RawUsage {
        prompt_tokens: Some(NumberLiteral::Integer(100)),
        completion_tokens: Some(NumberLiteral::Integer(20)),
        total_tokens: Some(NumberLiteral::Integer(120)),
        ..RawUsage::default()
    };
    let usage = normalize_usage(Some(&complete), None);
    if usage.status != UsageStatus::Complete
        || usage.input_tokens != Some(100)
        || usage.total_tokens_derived
    {
        return Err("workbuddy_import_tokens_complete_drifted");
    }
    let derived = RawUsage {
        total_tokens: None,
        ..complete.clone()
    };
    let usage = normalize_usage(Some(&derived), None);
    if usage.total_tokens != Some(120) || !usage.total_tokens_derived {
        return Err("workbuddy_import_tokens_derived_drifted");
    }
    let zero = RawUsage {
        prompt_tokens: Some(NumberLiteral::Integer(0)),
        completion_tokens: Some(NumberLiteral::Integer(0)),
        total_tokens: Some(NumberLiteral::Integer(0)),
        ..RawUsage::default()
    };
    let usage = normalize_usage(Some(&zero), None);
    if usage.input_tokens != Some(0) || usage.status != UsageStatus::Complete {
        return Err("workbuddy_import_tokens_zero_drifted");
    }
    let broken = RawUsage {
        completion_tokens: Some(NumberLiteral::Other),
        ..complete.clone()
    };
    let usage = normalize_usage(Some(&broken), None);
    if usage.status != UsageStatus::Invalid
        || usage.reason != Some(UsageReason::InvalidNumbers)
        || !usage.has_no_token_values()
    {
        return Err("workbuddy_import_tokens_invalid_drifted");
    }
    let conflicting = RawUsage {
        total_tokens: Some(NumberLiteral::Integer(999)),
        ..complete.clone()
    };
    let usage = normalize_usage(Some(&conflicting), None);
    if usage.status != UsageStatus::Invalid || usage.reason != Some(UsageReason::ConflictingUsage) {
        return Err("workbuddy_import_tokens_conflict_drifted");
    }
    let mut usage = normalize_usage(Some(&complete), None);
    let with_cache = RawUsage {
        prompt_cache_hit_tokens: Some(NumberLiteral::Integer(40)),
        cache_read_input_tokens: Some(NumberLiteral::Integer(0)),
        completion_thinking_tokens: Some(NumberLiteral::Integer(5)),
        ..complete.clone()
    };
    normalize_cache_and_reasoning(&mut usage, Some(&with_cache), None);
    if usage.cache_read_tokens != Some(40)
        || usage.reasoning_tokens != Some(5)
        || !usage.diagnostics.cache_alias_disagreement
        || usage.input_breakdown != InputBreakdown::ReadSubset
        || usage.input_tokens != Some(100)
    {
        return Err("workbuddy_import_cache_rules_drifted");
    }
    let mut usage = normalize_usage(Some(&complete), None);
    let with_credit = RawUsage {
        credit: credit_literal(
            br#"{"type":"message","role":"assistant","providerData":{"rawUsage":{"credit":1.14}}}"#,
        ),
        ..complete.clone()
    };
    normalize_credit(&mut usage, Some(&with_credit));
    if usage.workbuddy_credit_nanos != Some(1_140_000_000)
        || usage.diagnostics.credit_invalid
        || usage.status != UsageStatus::Complete
    {
        return Err("workbuddy_import_credit_exact_drifted");
    }
    let mut usage = normalize_usage(Some(&complete), None);
    let bad_credit = RawUsage {
        credit: credit_literal(
            br#"{"type":"message","role":"assistant","providerData":{"rawUsage":{"credit":1e3}}}"#,
        ),
        ..complete.clone()
    };
    normalize_credit(&mut usage, Some(&bad_credit));
    if usage.workbuddy_credit_nanos.is_some()
        || !usage.diagnostics.credit_invalid
        || usage.status != UsageStatus::Complete
        || usage.input_tokens != Some(100)
    {
        return Err("workbuddy_import_credit_invalid_drifted");
    }
    Ok(1)
}

#[cfg(test)]
mod token_tests {
    use super::*;

    fn raw(
        prompt: Option<NumberLiteral>,
        completion: Option<NumberLiteral>,
        total: Option<NumberLiteral>,
    ) -> RawUsage {
        RawUsage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: total,
            ..RawUsage::default()
        }
    }

    fn integer(value: i64) -> Option<NumberLiteral> {
        Some(NumberLiteral::Integer(value))
    }

    #[test]
    fn an_explicit_zero_is_preserved() {
        let usage = normalize_usage(Some(&raw(integer(0), integer(0), integer(0))), None);
        assert_eq!(usage.status, UsageStatus::Complete);
        assert_eq!(usage.input_tokens, Some(0));
        assert_eq!(usage.output_tokens, Some(0));
        assert_eq!(usage.total_tokens, Some(0));
        assert!(!usage.total_tokens_derived);
    }

    #[test]
    fn missing_fields_stay_unknown_and_are_never_invented() {
        // X06: only a prompt count is present; nothing may be derived from it.
        let usage = normalize_usage(Some(&raw(integer(7), None, None)), None);
        assert_eq!(usage.status, UsageStatus::Partial);
        assert_eq!(usage.input_tokens, Some(7));
        assert_eq!(usage.output_tokens, None);
        assert_eq!(usage.total_tokens, None);
        assert_eq!(usage.reason, None);

        // A missing output can never be recovered by subtracting from a total.
        let usage = normalize_usage(Some(&raw(None, None, integer(120))), None);
        assert_eq!(usage.status, UsageStatus::Partial);
        assert_eq!(usage.input_tokens, None);
        assert_eq!(usage.total_tokens, Some(120));
    }

    #[test]
    fn an_empty_usage_object_is_missing_not_zero() {
        let usage = normalize_usage(Some(&RawUsage::default()), None);
        assert_eq!(usage.status, UsageStatus::Missing);
        assert!(usage.has_no_token_values());
        assert_eq!(normalize_usage(None, None).status, UsageStatus::Missing);
    }

    #[test]
    fn negative_values_invalidate_the_whole_record() {
        // X06: -1 must never be truncated into a count.
        let usage = normalize_usage(Some(&raw(integer(10), integer(-1), integer(9))), None);
        assert_eq!(usage.status, UsageStatus::Invalid);
        assert_eq!(usage.reason, Some(UsageReason::InvalidNumbers));
        assert!(
            usage.has_no_token_values(),
            "an invalid record exposes no counts"
        );
    }

    #[test]
    fn floats_strings_and_booleans_are_never_coerced() {
        for broken in [
            NumberLiteral::Other,
            NumberLiteral::Other,
            NumberLiteral::Other,
        ] {
            let usage = normalize_usage(Some(&raw(integer(10), Some(broken), integer(20))), None);
            assert_eq!(usage.status, UsageStatus::Invalid, "{broken:?}");
            assert_eq!(usage.reason, Some(UsageReason::InvalidNumbers));
        }
    }

    #[test]
    fn the_per_field_bound_is_inclusive_and_the_total_bound_too() {
        let at_bound = normalize_usage(
            Some(&raw(
                integer(TOKEN_LIMIT),
                integer(TOKEN_LIMIT),
                integer(TOTAL_LIMIT),
            )),
            None,
        );
        assert_eq!(at_bound.status, UsageStatus::Complete);

        let over = normalize_usage(Some(&raw(integer(TOKEN_LIMIT + 1), None, None)), None);
        assert_eq!(over.status, UsageStatus::Invalid);
        assert_eq!(over.reason, Some(UsageReason::InvalidNumbers));

        let over_total = normalize_usage(Some(&raw(None, None, integer(TOTAL_LIMIT + 1))), None);
        assert_eq!(over_total.status, UsageStatus::Invalid);
        assert_eq!(over_total.reason, Some(UsageReason::InvalidNumbers));
    }

    #[test]
    fn a_missing_total_is_derived_with_a_checked_add() {
        // X07: total absent, both halves known.
        let usage = normalize_usage(Some(&raw(integer(100), integer(20), None)), None);
        assert_eq!(usage.status, UsageStatus::Complete);
        assert_eq!(usage.total_tokens, Some(120));
        assert!(usage.total_tokens_derived, "the derived flag must be set");

        // The derived sum must also respect the total bound.
        let at_bound = normalize_usage(
            Some(&raw(integer(TOKEN_LIMIT), integer(TOKEN_LIMIT), None)),
            None,
        );
        assert_eq!(at_bound.total_tokens, Some(TOTAL_LIMIT));
        assert!(at_bound.total_tokens_derived);
    }

    #[test]
    fn a_contradicting_total_is_invalid_and_never_corrected() {
        // X07: total says 9 while 10 + 20 = 30.
        let usage = normalize_usage(Some(&raw(integer(10), integer(20), integer(9))), None);
        assert_eq!(usage.status, UsageStatus::Invalid);
        assert_eq!(usage.reason, Some(UsageReason::ConflictingUsage));
        assert!(usage.has_no_token_values());
    }

    #[test]
    fn raw_usage_wins_and_must_agree_with_the_normalized_wrapper() {
        let normalized = NormalizedUsageFields {
            input_tokens: integer(100),
            output_tokens: integer(20),
            total_tokens: integer(120),
            ..NormalizedUsageFields::default()
        };
        let usage = normalize_usage(
            Some(&raw(integer(100), integer(20), integer(120))),
            Some(&normalized),
        );
        assert_eq!(usage.status, UsageStatus::Complete);
        assert_eq!(usage.input_tokens, Some(100));

        // Only the normalized wrapper carries a fact: it is used.
        let usage = normalize_usage(Some(&RawUsage::default()), Some(&normalized));
        assert_eq!(usage.status, UsageStatus::Complete);
        assert_eq!(usage.total_tokens, Some(120));

        // Disagreeing wrappers are a conflict, not a preference.
        let disagreeing = NormalizedUsageFields {
            input_tokens: integer(200),
            ..normalized.clone()
        };
        let usage = normalize_usage(
            Some(&raw(integer(100), integer(20), integer(120))),
            Some(&disagreeing),
        );
        assert_eq!(usage.status, UsageStatus::Invalid);
        assert_eq!(usage.reason, Some(UsageReason::ConflictingUsage));
    }

    #[test]
    fn an_illegal_high_priority_value_is_not_masked_by_a_lower_alias() {
        // rawUsage.prompt_tokens is broken; usage.inputTokens is fine. The
        // record is still invalid: a broken value is never a fallback trigger.
        let raw_usage = RawUsage {
            prompt_tokens: Some(NumberLiteral::Other),
            ..RawUsage::default()
        };
        let normalized = NormalizedUsageFields {
            input_tokens: integer(100),
            ..NormalizedUsageFields::default()
        };
        let usage = normalize_usage(Some(&raw_usage), Some(&normalized));
        assert_eq!(usage.status, UsageStatus::Invalid);
        assert_eq!(usage.reason, Some(UsageReason::InvalidNumbers));

        // The same rule inside rawUsage: prompt_tokens beats input_tokens.
        let raw_usage = RawUsage {
            prompt_tokens: Some(NumberLiteral::Other),
            input_tokens: integer(50),
            ..RawUsage::default()
        };
        let usage = normalize_usage(Some(&raw_usage), None);
        assert_eq!(usage.status, UsageStatus::Invalid);

        // When the higher alias is absent the lower one is used.
        let raw_usage = RawUsage {
            input_tokens: integer(50),
            ..RawUsage::default()
        };
        let usage = normalize_usage(Some(&raw_usage), None);
        assert_eq!(usage.input_tokens, Some(50));
        assert_eq!(usage.status, UsageStatus::Partial);
    }

    #[test]
    fn the_normalized_wrapper_uses_its_own_camel_case_fields() {
        let normalized = NormalizedUsageFields {
            input_tokens: integer(11),
            output_tokens: integer(22),
            total_tokens: integer(33),
            ..NormalizedUsageFields::default()
        };
        let usage = normalize_usage(None, Some(&normalized));
        assert_eq!(
            (usage.input_tokens, usage.output_tokens, usage.total_tokens),
            (Some(11), Some(22), Some(33))
        );
        assert_eq!(usage.status, UsageStatus::Complete);
    }

    #[test]
    fn a_partial_record_keeps_every_known_value() {
        let usage = normalize_usage(Some(&raw(integer(5), integer(6), None)), None);
        assert_eq!(
            usage.status,
            UsageStatus::Complete,
            "a derived total completes the trio"
        );
        let usage = normalize_usage(Some(&raw(None, integer(6), None)), None);
        assert_eq!(usage.status, UsageStatus::Partial);
        assert_eq!(usage.output_tokens, Some(6));
        assert_eq!(usage.total_tokens, None);
        assert!(!usage.total_tokens_derived);
    }

    #[test]
    fn the_contract_check_holds() {
        assert!(normalize_contract_check().is_ok());
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    fn complete_raw() -> RawUsage {
        RawUsage {
            prompt_tokens: Some(NumberLiteral::Integer(1000)),
            completion_tokens: Some(NumberLiteral::Integer(200)),
            total_tokens: Some(NumberLiteral::Integer(1200)),
            ..RawUsage::default()
        }
    }

    fn normalized_with(usage: RawUsage) -> NormalizedUsage {
        let mut record = normalize_usage(Some(&usage), None);
        normalize_cache_and_reasoning(&mut record, Some(&usage), None);
        record
    }

    fn integer(value: i64) -> Option<NumberLiteral> {
        Some(NumberLiteral::Integer(value))
    }

    #[test]
    fn the_highest_priority_legal_alias_wins_and_never_gets_zeroed() {
        // X08: the observed WorkBuddy shape — detail 4096 beats the top-level 0.
        let usage = RawUsage {
            prompt_tokens_details: Some(
                crate::stats::workbuddy_import::parser::PromptTokensDetails {
                    cached_tokens: integer(4096),
                },
            ),
            prompt_tokens: integer(8192),
            completion_tokens: integer(200),
            total_tokens: integer(8392),
            cache_read_input_tokens: integer(0),
            ..complete_raw()
        };
        let record = normalized_with(usage);
        assert_eq!(record.cache_read_tokens, Some(4096));
        assert!(
            record.diagnostics.cache_alias_disagreement,
            "the top-level alias is present and disagrees, so the diagnostic is raised"
        );

        let usage = RawUsage {
            prompt_cache_hit_tokens: integer(4096),
            prompt_tokens: integer(8192),
            completion_tokens: integer(200),
            total_tokens: integer(8392),
            cache_read_input_tokens: integer(0),
            ..complete_raw()
        };
        let record = normalized_with(usage);
        assert_eq!(
            record.cache_read_tokens,
            Some(4096),
            "a disagreeing lower alias must not zero it"
        );
        assert!(record.diagnostics.cache_alias_disagreement);
        assert_eq!(record.status, UsageStatus::Complete);
    }

    #[test]
    fn a_matching_lower_alias_is_not_a_disagreement() {
        let usage = RawUsage {
            prompt_cache_hit_tokens: integer(40),
            cache_read_input_tokens: integer(40),
            ..complete_raw()
        };
        let record = normalized_with(usage);
        assert_eq!(record.cache_read_tokens, Some(40));
        assert!(!record.diagnostics.cache_alias_disagreement);
    }

    #[test]
    fn cache_counts_are_never_added_to_the_input() {
        let usage = RawUsage {
            prompt_tokens_details: Some(
                crate::stats::workbuddy_import::parser::PromptTokensDetails {
                    cached_tokens: integer(400),
                },
            ),
            prompt_cache_write_tokens: integer(100),
            completion_tokens_details: Some(
                crate::stats::workbuddy_import::parser::CompletionTokensDetails {
                    reasoning_tokens: integer(50),
                },
            ),
            ..complete_raw()
        };
        let record = normalized_with(usage);
        assert_eq!(
            record.input_tokens,
            Some(1000),
            "the input count is untouched"
        );
        assert_eq!(
            record.output_tokens,
            Some(200),
            "the output count is untouched"
        );
        assert_eq!(record.total_tokens, Some(1200), "the total is untouched");
        assert_eq!(record.cache_read_tokens, Some(400));
        assert_eq!(record.cache_write_tokens, Some(100));
        assert_eq!(record.reasoning_tokens, Some(50));
    }

    #[test]
    fn cache_read_and_write_may_not_exceed_the_input() {
        // X09.
        let usage = RawUsage {
            prompt_cache_hit_tokens: integer(1001),
            ..complete_raw()
        };
        let record = normalized_with(usage);
        assert_eq!(record.status, UsageStatus::Invalid);
        assert_eq!(record.reason, Some(UsageReason::InvalidNumbers));
        assert!(record.has_no_token_values());

        let usage = RawUsage {
            prompt_cache_write_tokens: integer(1001),
            ..complete_raw()
        };
        assert_eq!(normalized_with(usage).status, UsageStatus::Invalid);

        // Exactly the input is legal: a cache count is a subset, not a strict one.
        let usage = RawUsage {
            prompt_cache_hit_tokens: integer(1000),
            ..complete_raw()
        };
        let record = normalized_with(usage);
        assert_eq!(record.status, UsageStatus::Complete);
        assert_eq!(record.cache_read_tokens, Some(1000));
    }

    #[test]
    fn reasoning_may_not_exceed_the_output() {
        // X09.
        let usage = RawUsage {
            completion_thinking_tokens: integer(201),
            ..complete_raw()
        };
        let record = normalized_with(usage);
        assert_eq!(record.status, UsageStatus::Invalid);
        assert_eq!(record.reason, Some(UsageReason::InvalidNumbers));

        let usage = RawUsage {
            completion_thinking_tokens: integer(200),
            ..complete_raw()
        };
        let record = normalized_with(usage);
        assert_eq!(record.status, UsageStatus::Complete);
        assert_eq!(record.reasoning_tokens, Some(200));
    }

    #[test]
    fn reasoning_aliases_follow_their_own_priority() {
        let usage = RawUsage {
            completion_tokens_details: Some(
                crate::stats::workbuddy_import::parser::CompletionTokensDetails {
                    reasoning_tokens: integer(7),
                },
            ),
            completion_thinking_tokens: integer(9),
            ..complete_raw()
        };
        let record = normalized_with(usage);
        assert_eq!(record.reasoning_tokens, Some(7));
        assert!(record.diagnostics.cache_alias_disagreement);

        let normalized = NormalizedUsageFields {
            output_tokens_details: Some(vec![
                crate::stats::workbuddy_import::parser::OutputTokensDetails {
                    reasoning_tokens: integer(11),
                },
            ]),
            ..NormalizedUsageFields::default()
        };
        let mut record = normalize_usage(Some(&complete_raw()), None);
        normalize_cache_and_reasoning(&mut record, Some(&complete_raw()), Some(&normalized));
        assert_eq!(
            record.reasoning_tokens,
            Some(11),
            "the normalized wrapper is the last resort"
        );
    }

    #[test]
    fn the_normalized_wrapper_supplies_cache_details() {
        let normalized = NormalizedUsageFields {
            input_tokens_details: Some(vec![
                crate::stats::workbuddy_import::parser::InputTokensDetails {
                    cached_tokens: integer(64),
                    cache_creation_input_tokens: integer(16),
                },
            ]),
            ..NormalizedUsageFields::default()
        };
        let mut record = normalize_usage(Some(&complete_raw()), None);
        normalize_cache_and_reasoning(&mut record, Some(&complete_raw()), Some(&normalized));
        assert_eq!(record.cache_read_tokens, Some(64));
        assert_eq!(record.cache_write_tokens, Some(16));
    }

    #[test]
    fn a_cache_only_record_is_partial_not_missing() {
        let usage = RawUsage {
            prompt_cache_hit_tokens: integer(10),
            ..RawUsage::default()
        };
        let record = normalized_with(usage);
        assert_eq!(record.status, UsageStatus::Partial);
        assert_eq!(record.cache_read_tokens, Some(10));
        assert_eq!(record.input_tokens, None);
    }

    #[test]
    fn an_illegal_alias_is_skipped_for_a_legal_lower_one() {
        // The cache rule of DATA-API section 2 selects the first *legal* alias.
        let usage = RawUsage {
            prompt_cache_hit_tokens: Some(NumberLiteral::Other),
            cache_read_input_tokens: integer(30),
            ..complete_raw()
        };
        let record = normalized_with(usage);
        assert_eq!(record.cache_read_tokens, Some(30));
        assert_eq!(record.status, UsageStatus::Complete);
    }

    #[test]
    fn a_field_whose_only_aliases_are_illegal_invalidates_the_record() {
        let usage = RawUsage {
            prompt_cache_hit_tokens: Some(NumberLiteral::Other),
            ..complete_raw()
        };
        let record = normalized_with(usage);
        assert_eq!(record.status, UsageStatus::Invalid);
        assert_eq!(record.reason, Some(UsageReason::InvalidNumbers));
        assert!(record.has_no_token_values());
    }

    #[test]
    fn an_already_invalid_record_is_left_alone() {
        let usage = RawUsage {
            completion_tokens: Some(NumberLiteral::Other),
            prompt_cache_hit_tokens: integer(10),
            ..complete_raw()
        };
        let mut record = normalize_usage(Some(&usage), None);
        assert_eq!(record.status, UsageStatus::Invalid);
        normalize_cache_and_reasoning(&mut record, Some(&usage), None);
        assert_eq!(record.status, UsageStatus::Invalid);
        assert_eq!(
            record.cache_read_tokens, None,
            "an invalid record gains no cache counts"
        );
    }

    #[test]
    fn the_input_breakdown_is_only_read_subset_or_unknown() {
        let without_cache = normalized_with(complete_raw());
        assert_eq!(without_cache.input_breakdown, InputBreakdown::Unknown);
        let with_cache = normalized_with(RawUsage {
            prompt_cache_hit_tokens: integer(0),
            ..complete_raw()
        });
        assert_eq!(with_cache.input_breakdown, InputBreakdown::ReadSubset);
        assert_ne!(with_cache.input_breakdown, InputBreakdown::Disjoint);
        assert!(
            !with_cache.diagnostics.unsupported_usage_schema,
            "the observed wrapper is supported"
        );
    }

    #[test]
    fn the_cache_contract_check_holds() {
        assert!(normalize_contract_check().is_ok());
    }
}

#[cfg(test)]
mod credit_tests {
    use super::*;

    fn complete_raw() -> RawUsage {
        RawUsage {
            prompt_tokens: Some(NumberLiteral::Integer(1000)),
            completion_tokens: Some(NumberLiteral::Integer(200)),
            total_tokens: Some(NumberLiteral::Integer(1200)),
            ..RawUsage::default()
        }
    }

    fn with_credit(literal: &str) -> RawUsage {
        let line = format!(
            r#"{{"type":"message","role":"assistant","providerData":{{"rawUsage":{{"credit":{literal}}}}}}}"#
        );
        RawUsage {
            credit: credit_literal(line.as_bytes()),
            ..complete_raw()
        }
    }

    #[test]
    fn a_two_decimal_credit_is_exact() {
        // X10: 1.14 must never become 1.1399999999999999 nanos.
        assert_eq!(parse_credit_nanos("1.14"), Ok(1_140_000_000));
        assert_eq!(parse_credit_nanos("0.01"), Ok(10_000_000));
        assert_eq!(parse_credit_nanos("123.456"), Ok(123_456_000_000));
    }

    #[test]
    fn trailing_zeros_and_integers_are_legal() {
        assert_eq!(parse_credit_nanos("1.140"), Ok(1_140_000_000));
        assert_eq!(parse_credit_nanos("3"), Ok(3_000_000_000));
        assert_eq!(parse_credit_nanos("0"), Ok(0));
        assert_eq!(parse_credit_nanos("0.0"), Ok(0));
    }

    #[test]
    fn nine_fractional_digits_is_the_limit() {
        assert_eq!(parse_credit_nanos("0.000000001"), Ok(1));
        assert_eq!(
            parse_credit_nanos("0.0000000001"),
            Err(CreditRejection::TooManyFractionDigits)
        );
    }

    #[test]
    fn scientific_notation_and_signs_are_refused() {
        assert_eq!(
            parse_credit_nanos("1e3"),
            Err(CreditRejection::ScientificNotation)
        );
        assert_eq!(
            parse_credit_nanos("1E3"),
            Err(CreditRejection::ScientificNotation)
        );
        assert_eq!(
            parse_credit_nanos("1.14e0"),
            Err(CreditRejection::ScientificNotation)
        );
        assert_eq!(parse_credit_nanos("-1"), Err(CreditRejection::Negative));
    }

    #[test]
    fn junk_is_refused() {
        assert_eq!(parse_credit_nanos(""), Err(CreditRejection::NotDecimal));
        assert_eq!(
            parse_credit_nanos("1.2.3"),
            Err(CreditRejection::NotDecimal)
        );
        assert_eq!(parse_credit_nanos(".5"), Err(CreditRejection::NotDecimal));
        assert_eq!(parse_credit_nanos("5."), Err(CreditRejection::NotDecimal));
        assert_eq!(parse_credit_nanos("1,14"), Err(CreditRejection::NotDecimal));
        assert_eq!(parse_credit_nanos("0x10"), Err(CreditRejection::NotDecimal));
        assert_eq!(parse_credit_nanos(" "), Err(CreditRejection::NotDecimal));
    }

    #[test]
    fn an_oversized_credit_is_refused_not_wrapped() {
        assert_eq!(
            parse_credit_nanos("99999999999999999999"),
            Err(CreditRejection::TooLarge)
        );
        assert_eq!(
            parse_credit_nanos("9223372036.854775808"),
            Err(CreditRejection::TooLarge),
            "one nano above the i64 limit"
        );
        assert_eq!(parse_credit_nanos("9223372036.854775807"), Ok(i64::MAX));
    }

    #[test]
    fn an_invalid_credit_never_damages_the_token_values() {
        // X10 / AC021: the record stays complete, only the diagnostic is raised.
        let mut usage = normalize_usage(Some(&complete_raw()), None);
        normalize_cache_and_reasoning(&mut usage, Some(&complete_raw()), None);
        normalize_credit(&mut usage, Some(&with_credit("1e3")));
        assert_eq!(usage.status, UsageStatus::Complete);
        assert_eq!(usage.reason, None);
        assert_eq!(usage.input_tokens, Some(1000));
        assert_eq!(usage.output_tokens, Some(200));
        assert_eq!(usage.total_tokens, Some(1200));
        assert_eq!(usage.workbuddy_credit_nanos, None);
        assert!(usage.diagnostics.credit_invalid);
    }

    #[test]
    fn a_valid_credit_is_stored_without_touching_anything_else() {
        let mut usage = normalize_usage(Some(&complete_raw()), None);
        let before = usage.clone();
        normalize_credit(&mut usage, Some(&with_credit("1.14")));
        assert_eq!(usage.workbuddy_credit_nanos, Some(1_140_000_000));
        assert!(!usage.diagnostics.credit_invalid);
        assert_eq!(usage.status, before.status);
        assert_eq!(usage.reason, before.reason);
        assert_eq!(usage.input_tokens, before.input_tokens);
        assert_eq!(usage.total_tokens, before.total_tokens);
        assert_eq!(usage.total_tokens_derived, before.total_tokens_derived);
    }

    #[test]
    fn an_absent_credit_is_not_a_diagnostic() {
        let mut usage = normalize_usage(Some(&complete_raw()), None);
        normalize_credit(&mut usage, Some(&complete_raw()));
        assert_eq!(usage.workbuddy_credit_nanos, None);
        assert!(!usage.diagnostics.credit_invalid, "absent is not invalid");
        normalize_credit(&mut usage, None);
        assert!(!usage.diagnostics.credit_invalid);
    }

    #[test]
    fn the_parser_keeps_the_exact_literal_for_the_credit_rule() {
        // The whole point of NumberText: the rule sees "1.140", not a float.
        let usage = with_credit("1.140");
        assert_eq!(
            usage.credit.as_ref().map(|text| text.as_str()),
            Some("1.140")
        );
        let mut record = normalize_usage(Some(&usage), None);
        normalize_credit(&mut record, Some(&usage));
        assert_eq!(record.workbuddy_credit_nanos, Some(1_140_000_000));
    }

    #[test]
    fn an_invalid_credit_leaves_an_invalid_record_invalid() {
        let broken = RawUsage {
            completion_tokens: Some(NumberLiteral::Other),
            ..complete_raw()
        };
        let mut usage = normalize_usage(Some(&broken), None);
        assert_eq!(usage.status, UsageStatus::Invalid);
        normalize_credit(&mut usage, Some(&with_credit("1e3")));
        assert_eq!(
            usage.status,
            UsageStatus::Invalid,
            "credit never repairs a record"
        );
        assert_eq!(usage.workbuddy_credit_nanos, None);
    }

    #[test]
    fn the_credit_contract_check_holds() {
        assert!(normalize_contract_check().is_ok());
    }
}

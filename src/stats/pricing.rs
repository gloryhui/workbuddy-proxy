//! Manual price rules and integer cost estimation.
//!
//! Money never touches floating point: a rate is "nano currency units per
//! 1,000,000 tokens" stored as an `i64`, every part is rounded half-up on its
//! own and summed with checked `i128` arithmetic before it is narrowed back.
use crate::stats::collector::UsageOutcome;
use crate::stats::dto::{CostStatus, Currency, InputBreakdown, InputMode, UsageStatus};

/// Largest accepted rate: 1e15 nano units per million tokens.
pub const MAX_RATE_NANOS: i64 = 1_000_000_000_000_000;
/// Decimal places accepted in a rate string (nano units).

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PriceRule {
    /// Traceability only: the editable current price is never joined for money.
    pub rule_id: String,
    pub currency: Currency,
    pub input_mode: InputMode,
    pub input_rate_nanos: i64,
    pub output_rate_nanos: i64,
    pub cache_read_rate_nanos: Option<i64>,
    pub cache_write_rate_nanos: Option<i64>,
}

/// Per-request money split; every field is a non-negative nano amount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CostParts {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    pub total: i64,
}

impl CostParts {
    /// A confirmed zero: used for the explicit "no price configured" default.
    pub fn zero() -> Self {
        Self {
            input: 0,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            total: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settlement {
    pub status: CostStatus,
    pub parts: Option<CostParts>,
}

impl Settlement {
    fn status(status: CostStatus) -> Self {
        Self {
            status,
            parts: None,
        }
    }
}

/// `round_half_up(tokens * rate / 1_000_000)` in checked `i128`.
fn part(tokens: i64, rate: i64) -> Option<i64> {
    let product = (tokens as i128).checked_mul(rate as i128)?;
    let rounded = (product + 500_000) / 1_000_000;
    i64::try_from(rounded).ok()
}

/// Estimates the cost of one request.
///
/// `rule` is the price rule that matches the request `started_at_ms`; `None`
/// means the request is unpriced, which is never the same as free.
pub fn settle(
    rule: Option<&PriceRule>,
    usage: &UsageOutcome,
    upstream_started: bool,
) -> Settlement {
    if !upstream_started {
        // The upstream was never called, so this request is a confirmed zero
        // rather than an unknown amount. A NULL here would violate the v2
        // `not_applicable` constraint and fail the whole settle transaction.
        return Settlement {
            status: CostStatus::NotApplicable,
            parts: Some(CostParts::zero()),
        };
    }
    if usage.status != UsageStatus::Complete {
        return Settlement::status(CostStatus::UsageIncomplete);
    }
    let Some(rule) = rule else {
        return Settlement::status(CostStatus::Unpriced);
    };
    let numbers = &usage.numbers;
    let (Some(input), Some(output)) = (numbers.input, numbers.output) else {
        return Settlement::status(CostStatus::UsageIncomplete);
    };
    let mut cache_read = 0i64;
    let mut cache_write = 0i64;
    let input_cost = match rule.input_mode {
        InputMode::Flat => {
            // Reads and writes are already covered by the flat input price.
            part(input, rule.input_rate_nanos)
        }
        InputMode::ReadDiscount => {
            let Some(read) = numbers.cache_read else {
                // A discount cannot be applied to an unknown split.
                return Settlement::status(CostStatus::UnsupportedBreakdown);
            };
            let Some(read_rate) = rule.cache_read_rate_nanos else {
                return Settlement::status(CostStatus::UnsupportedBreakdown);
            };
            let Some(uncached) = input.checked_sub(read).filter(|v| *v >= 0) else {
                return Settlement::status(CostStatus::UnsupportedBreakdown);
            };
            cache_read = match part(read, read_rate) {
                Some(value) => value,
                None => return Settlement::status(CostStatus::Overflow),
            };
            part(uncached, rule.input_rate_nanos)
        }
        InputMode::Disjoint => {
            if usage.breakdown != InputBreakdown::Disjoint {
                return Settlement::status(CostStatus::UnsupportedBreakdown);
            }
            let (Some(read), Some(write), Some(uncached)) =
                (numbers.cache_read, numbers.cache_write, usage.uncached)
            else {
                return Settlement::status(CostStatus::UnsupportedBreakdown);
            };
            let (Some(read_rate), Some(write_rate)) =
                (rule.cache_read_rate_nanos, rule.cache_write_rate_nanos)
            else {
                return Settlement::status(CostStatus::UnsupportedBreakdown);
            };
            cache_read = match part(read, read_rate) {
                Some(value) => value,
                None => return Settlement::status(CostStatus::Overflow),
            };
            cache_write = match part(write, write_rate) {
                Some(value) => value,
                None => return Settlement::status(CostStatus::Overflow),
            };
            part(uncached, rule.input_rate_nanos)
        }
    };
    let Some(input_cost) = input_cost else {
        return Settlement::status(CostStatus::Overflow);
    };
    let Some(output_cost) = part(output, rule.output_rate_nanos) else {
        return Settlement::status(CostStatus::Overflow);
    };
    let total =
        (input_cost as i128) + (output_cost as i128) + (cache_read as i128) + (cache_write as i128);
    let Ok(total) = i64::try_from(total) else {
        return Settlement::status(CostStatus::Overflow);
    };
    Settlement {
        status: CostStatus::Priced,
        parts: Some(CostParts {
            input: input_cost,
            output: output_cost,
            cache_read,
            cache_write,
            total,
        }),
    }
}

/// Parses an API rate string.
///
/// `API.md` fixes `*_rate_nanos` as a plain non-negative integer count of nano
/// currency units per 1,000,000 tokens: `"2000000000"` is $2/M and `"1"` is a
/// single nano unit. Fractional nano values, signs and scientific notation are
/// rejected instead of being guessed or rounded, and the settings form converts
/// its human amount with [`crate::stats::dto::parse_human_amount`] exactly once.
pub fn parse_rate(text: &str) -> Result<i64, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("rate must not be empty".into());
    }
    if !text.chars().all(|c| c.is_ascii_digit()) {
        return Err("rate must be a whole number of nano units".into());
    }
    let nanos: i64 = text.parse().map_err(|_| "rate is too large".to_string())?;
    if nanos > MAX_RATE_NANOS {
        return Err("rate is out of range".into());
    }
    Ok(nanos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::collector::UsageNumbers;

    fn usage(input: i64, output: i64, cache_read: Option<i64>) -> UsageOutcome {
        UsageOutcome {
            status: UsageStatus::Complete,
            reason: None,
            numbers: UsageNumbers {
                input: Some(input),
                output: Some(output),
                total: Some(input + output),
                cache_read,
                cache_write: None,
                reasoning: None,
            },
            breakdown: if cache_read.is_some() {
                InputBreakdown::ReadSubset
            } else {
                InputBreakdown::Unknown
            },
            uncached: None,
        }
    }

    fn rule(mode: InputMode, input: &str, output: &str, read: Option<&str>) -> PriceRule {
        PriceRule {
            rule_id: "price-a".into(),

            currency: Currency::Usd,
            input_mode: mode,
            input_rate_nanos: crate::stats::dto::parse_human_amount(input).unwrap(),
            output_rate_nanos: crate::stats::dto::parse_human_amount(output).unwrap(),
            cache_read_rate_nanos: read
                .map(|rate| crate::stats::dto::parse_human_amount(rate).unwrap()),
            cache_write_rate_nanos: None,
        }
    }

    #[test]
    fn api_rate_strings_are_nano_units_per_million_tokens() {
        // F03 regression: `API.md` fixes `*_rate_nanos` as an integer count of
        // nano currency units per million tokens, so "2000000000" means $2/M
        // and "1" means one nano unit (not one dollar).
        assert_eq!(parse_rate("2000000000").unwrap(), 2_000_000_000);
        assert_eq!(parse_rate("1").unwrap(), 1);
        assert_eq!(parse_rate("0").unwrap(), 0);
        assert_eq!(parse_rate("1000000000").unwrap(), 1_000_000_000);
        assert_eq!(
            parse_rate(&MAX_RATE_NANOS.to_string()).unwrap(),
            MAX_RATE_NANOS
        );
        // Fractional nano units are rejected instead of being guessed.
        for bad in [
            "0.2",
            "2.5",
            "1.0000000001",
            "-1",
            "1e3",
            "",
            "abc",
            " 1 2 ",
        ] {
            assert!(parse_rate(bad).is_err(), "{bad} must be rejected");
        }
        assert!(parse_rate(&(MAX_RATE_NANOS + 1).to_string()).is_err());
    }

    #[test]
    fn the_settings_form_converts_a_human_amount_exactly_once() {
        assert_eq!(
            crate::stats::dto::parse_human_amount("2").unwrap(),
            2_000_000_000
        );
        assert_eq!(
            crate::stats::dto::parse_human_amount("0.2").unwrap(),
            200_000_000
        );
        assert_eq!(
            crate::stats::dto::parse_human_amount("1.234567891").unwrap(),
            1_234_567_891
        );
        assert!(crate::stats::dto::parse_human_amount("1.2345678912").is_err());
        assert!(crate::stats::dto::parse_human_amount("-1").is_err());
        assert!(crate::stats::dto::parse_human_amount("1e3").is_err());
        // Round trip through the display helper must be lossless.
        for nanos in [0i64, 1, 200_000_000, 2_000_000_000, 1_234_567_891] {
            let human = crate::stats::dto::format_amount(nanos);
            let reparsed = if human.starts_with('<') {
                0
            } else {
                crate::stats::dto::parse_human_amount(&human).unwrap()
            };
            assert!(
                (reparsed - nanos).abs() <= 100_000,
                "{nanos} -> {human} -> {reparsed}"
            );
        }
    }

    #[test]
    fn read_discount_matches_the_documented_example() {
        let discount = rule(InputMode::ReadDiscount, "2", "8", Some("0.2"));
        let settlement = settle(Some(&discount), &usage(1000, 100, Some(800)), true);
        assert_eq!(settlement.status, CostStatus::Priced);
        let parts = settlement.parts.unwrap();
        assert_eq!(parts.input, 400_000);
        assert_eq!(parts.cache_read, 160_000);
        assert_eq!(parts.output, 800_000);
        assert_eq!(parts.total, 1_360_000);
        assert_eq!(crate::stats::dto::format_amount(1_360_000), "0.00136");
    }

    #[test]
    fn flat_prices_every_input_token_once() {
        let flat = rule(InputMode::Flat, "2", "8", Some("0.2"));
        let parts = settle(Some(&flat), &usage(1000, 100, Some(800)), true)
            .parts
            .unwrap();
        assert_eq!(parts.input, 2_000_000);
        assert_eq!(parts.cache_read, 0);
        assert_eq!(parts.total, 2_800_000);
    }

    #[test]
    fn disjoint_requires_a_proven_split() {
        let disjoint = rule(InputMode::Disjoint, "2", "8", Some("0.2"));
        let subset = settle(Some(&disjoint), &usage(1000, 100, Some(800)), true);
        assert_eq!(subset.status, CostStatus::UnsupportedBreakdown);

        let mut split = usage(1000, 100, Some(800));
        split.breakdown = InputBreakdown::Disjoint;
        split.numbers.cache_write = Some(100);
        split.uncached = Some(100);
        let mut disjoint = disjoint;
        disjoint.cache_write_rate_nanos =
            Some(crate::stats::dto::parse_human_amount("2.5").unwrap());
        let parts = settle(Some(&disjoint), &split, true).parts.unwrap();
        assert_eq!(parts.input, 200_000);
        assert_eq!(parts.cache_read, 160_000);
        assert_eq!(parts.cache_write, 250_000);
        assert_eq!(parts.total, 1_410_000);
    }

    #[test]
    fn missing_inputs_never_become_free_or_zero() {
        // No rule at all.
        assert_eq!(
            settle(None, &usage(10, 1, None), true).status,
            CostStatus::Unpriced
        );
        // Usage was never confirmed.
        let mut partial = usage(10, 1, None);
        partial.status = UsageStatus::Partial;
        let flat = rule(InputMode::Flat, "2", "8", None);
        assert_eq!(
            settle(Some(&flat), &partial, true).status,
            CostStatus::UsageIncomplete
        );
        // Local rejection before the upstream call.
        assert_eq!(
            settle(Some(&flat), &usage(10, 1, None), false).status,
            CostStatus::NotApplicable
        );
        // read_discount with unknown cache read.
        let discount = rule(InputMode::ReadDiscount, "2", "8", Some("0.2"));
        assert_eq!(
            settle(Some(&discount), &usage(10, 1, None), true).status,
            CostStatus::UnsupportedBreakdown
        );
    }

    #[test]
    fn overflow_is_reported_instead_of_wrapping() {
        let mut flat = rule(InputMode::Flat, "2", "8", None);
        flat.input_rate_nanos = MAX_RATE_NANOS;
        flat.output_rate_nanos = MAX_RATE_NANOS;
        let settlement = settle(
            Some(&flat),
            &usage(1_000_000_000_000, 1_000_000_000_000, None),
            true,
        );
        assert_eq!(settlement.status, CostStatus::Overflow);
    }

    #[test]
    fn rounding_is_half_up_per_part() {
        // 1 token at 5 nanos per million -> 0.5 nanos -> 1 (half up).
        let thumb = PriceRule {
            input_mode: InputMode::Flat,
            input_rate_nanos: 5,
            output_rate_nanos: 0,
            ..rule(InputMode::Flat, "0", "0", None)
        };
        let parts = settle(Some(&thumb), &usage(100_000, 0, None), true)
            .parts
            .unwrap();
        assert_eq!(parts.input, 1);
        // 1 token at 4 nanos per million -> 0.4 nanos -> 0.
        let thumb = PriceRule {
            input_rate_nanos: 4,
            ..thumb
        };
        let parts = settle(Some(&thumb), &usage(100_000, 0, None), true)
            .parts
            .unwrap();
        assert_eq!(parts.input, 0);
    }
}

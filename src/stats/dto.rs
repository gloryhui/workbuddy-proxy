//! Statistics DTOs shared by the core proxy and the native settings binary.
//!
//! This module is deliberately limited to `serde`/`std` so `gpui` can include it
//! with `#[path = "../../src/stats/dto.rs"]` without linking SQLite, chrono-tz or
//! any other core-only dependency. SQL, timezone resolution and pricing live in
//! the sibling modules of `crate::stats`.
use serde::{Deserialize, Serialize};

/// Inner protocol version for every statistics response.
/// Statistics API version. Bumped to 2 with the per-call price snapshot: the
/// request detail no longer returns a mutable `rule` and every place is a
/// decimal string of nano units.
pub const API_VERSION: u32 = 2;
/// Longest accepted `[from_ms, to_ms)` range.
pub const MAX_RANGE_MS: i64 = 366 * 86_400_000;
/// Upper bound for a single statistical query, above which the core asks the
/// caller to narrow the range instead of degrading to floating point.
pub const MAX_MATCHING_ROWS: i64 = 1_000_000;
/// Largest allowed serialized statistics response.
pub const MAX_RESULT_BYTES: usize = 1024 * 1024;
/// Largest accepted single token field.
pub const MAX_TOKEN_VALUE: i64 = 1_000_000_000_000;
/// Largest accepted `total_tokens`.
pub const MAX_TOTAL_TOKENS: i64 = 2_000_000_000_000;
/// Bucket ceiling for the trend series.
pub const MAX_BUCKETS: usize = 400;
/// Default retention window in days; `0` means keep everything.
pub const DEFAULT_RETENTION_DAYS: i64 = 180;
/// Row batch size for maintenance work.
pub const MAINTENANCE_BATCH: i64 = 1000;
pub const ERR_INVALID: &str = "stats_invalid_argument";
pub const ERR_TIMEOUT: &str = "stats_query_timeout";
pub const ERR_BUSY: &str = "stats_busy";
pub const ERR_UNAVAILABLE: &str = "stats_unavailable";
pub const ERR_CURSOR_EXPIRED: &str = "stats_cursor_expired";
/// A command that used to exist and was deliberately withdrawn: repricing
/// history with the *current* price contradicts the per-call price snapshot.
pub const ERR_OPERATION_REMOVED: &str = "stats_operation_removed";
pub const ERR_REVISION_CONFLICT: &str = "stats_revision_conflict";
pub const ERR_NOT_FOUND: &str = "stats_not_found";
pub const ERR_RESULT_TOO_LARGE: &str = "stats_result_too_large";
pub const ERR_QUERY_TOO_LARGE: &str = "stats_query_too_large";
pub const ERR_UNSUPPORTED_SCHEMA: &str = "stats_unsupported_schema";

/// Formats a nano amount as a decimal currency string with at least four
/// decimals, or `<0.0001` when the value is smaller than the display precision.
#[allow(dead_code)] // Shared with the native settings UI.
pub fn format_amount(nanos: i64) -> String {
    format_amount_text(&nanos.to_string())
}

/// Formats a decimal nano amount of any size.
///
/// The API sends amounts as decimal strings because a total can exceed `i64`;
/// parsing them into `i64` and falling back to 0 turned a real amount into
/// "unknown" on the page. `i128` covers every value the contract allows.
pub fn format_amount_text(text: &str) -> String {
    let text = text.trim();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return "—".into();
    }
    let Ok(value) = digits.parse::<i128>() else {
        return "—".into();
    };
    if value > 0 && value < 100_000 {
        return if negative {
            ">-0.0001".into()
        } else {
            "<0.0001".into()
        };
    }
    let whole = value / 1_000_000_000;
    let fraction = value % 1_000_000_000;
    if fraction == 0 {
        // A whole amount is shown as a whole number: "0", never "0.0000".
        return if negative {
            format!("-{whole}")
        } else {
            whole.to_string()
        };
    }
    let mut rendered = format!("{whole}.{fraction:09}");
    while rendered.ends_with('0') && rendered.len() - rendered.find('.').unwrap_or(0) > 5 {
        rendered.pop();
    }
    if negative {
        format!("-{rendered}")
    } else {
        rendered
    }
}

/// Parses a human amount per million tokens (`"2"`, `"0.2"`) into the integer
/// nano units the API stores. The settings form calls this exactly once on
/// submit; the value never round-trips through `f64`.
#[allow(dead_code)] // Shared with the native settings UI.
pub fn parse_human_amount(text: &str) -> Result<i64, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("amount must not be empty".into());
    }
    let (whole, fraction) = match text.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (text, ""),
    };
    if whole.is_empty() && fraction.is_empty() {
        return Err("amount is not a number".into());
    }
    if !whole.chars().all(|c| c.is_ascii_digit()) || !fraction.chars().all(|c| c.is_ascii_digit()) {
        return Err("amount must be a plain decimal number".into());
    }
    if fraction.len() > 9 {
        return Err("amount accepts at most 9 decimals".into());
    }
    let whole: i64 = if whole.is_empty() {
        0
    } else {
        whole
            .parse()
            .map_err(|_| "amount is too large".to_string())?
    };
    let mut padded = fraction.to_string();
    while padded.len() < 9 {
        padded.push('0');
    }
    let fraction: i64 = if padded.is_empty() {
        0
    } else {
        padded
            .parse()
            .map_err(|_| "amount is too large".to_string())?
    };
    let nanos = whole
        .checked_mul(1_000_000_000)
        .and_then(|value| value.checked_add(fraction))
        .ok_or("amount is too large")?;
    if nanos < 0 {
        return Err("amount must not be negative".into());
    }
    Ok(nanos)
}

/// Formats a localized, non-leaking error message with a stable machine prefix.
pub fn error(code: &str, detail: &str) -> String {
    format!("{code}: {detail}")
}

macro_rules! stats_enum {
    ($(#[$enum_attr:meta])* $name:ident { $($(#[$variant_attr:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$enum_attr])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($(#[$variant_attr])* $variant),+
        }
        impl $name {
            /// Every variant, in declaration order (used by UI option lists).
            #[allow(dead_code)]
            pub const ALL: &'static [$name] = &[$($name::$variant),+];
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $text),+ }
            }
            /// Parses the wire form; used by SQL row mapping and the settings UI.
            #[allow(dead_code)]
            pub fn parse(value: &str) -> Option<Self> {
                match value { $($text => Some($name::$variant),)+ _ => None }
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

stats_enum!(Source {
    Relay => "relay",
    ConnectionTest => "connection_test",
});

stats_enum!(Outcome {
    InFlight => "in_flight",
    Completed => "completed",
    Failed => "failed",
    Cancelled => "cancelled",
    Interrupted => "interrupted",
    Rejected => "rejected",
});

stats_enum!(ErrorKind {
    MissingKey => "missing_key",
    Connect => "connect",
    UpstreamHttp => "upstream_http",
    UpstreamStream => "upstream_stream",
    SseError => "sse_error",
    ClientDrop => "client_drop",
    UncleanShutdown => "unclean_shutdown",
    Shutdown => "shutdown",
    Other => "other",
});

stats_enum!(UsageStatus {
    Complete => "complete",
    Partial => "partial",
    Missing => "missing",
    Invalid => "invalid",
    NotApplicable => "not_applicable",
});

stats_enum!(UsageReason {
    NoUsage => "no_usage",
    ClientDrop => "client_drop",
    StreamError => "stream_error",
    ParseError => "parse_error",
    EventTooLarge => "event_too_large",
    BodyTooLarge => "body_too_large",
    UnsupportedEncoding => "unsupported_encoding",
    InvalidNumbers => "invalid_numbers",
    Inconsistent => "inconsistent",
    ConflictingUsage => "conflicting_usage",
    UncleanShutdown => "unclean_shutdown",
    NotSent => "not_sent",
});

stats_enum!(UsageSource {
    None => "none",
    Upstream => "upstream",
});

stats_enum!(InputBreakdown {
    Unknown => "unknown",
    ReadSubset => "read_subset",
    Disjoint => "disjoint",
});

stats_enum!(#[derive(Default)] UsageMode {
    Observe => "observe",
    #[default]
    Request => "request",
});

stats_enum!(PriceSource {
    Manual => "manual",
    DefaultZero => "default_zero",
    LegacyUnknown => "legacy_unknown",
});

stats_enum!(CostStatus {
    Priced => "priced",
    DefaultZero => "default_zero",
    Unpriced => "unpriced",
    LegacyUnknown => "legacy_unknown",
    Pending => "pending",
    UsageIncomplete => "usage_incomplete",
    UnsupportedBreakdown => "unsupported_breakdown",
    NotApplicable => "not_applicable",
    Overflow => "overflow",
});

stats_enum!(InputMode {
    Flat => "flat",
    ReadDiscount => "read_discount",
    Disjoint => "disjoint",
});

/// Currencies keep their uppercase ISO-style wire form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Currency {
    #[serde(rename = "USD")]
    Usd,
    #[serde(rename = "CNY")]
    Cny,
}

impl Currency {
    /// Every currency, in declaration order (used by UI option lists).
    #[allow(dead_code)]
    pub const ALL: &'static [Currency] = &[Currency::Usd, Currency::Cny];
    pub fn as_str(self) -> &'static str {
        match self {
            Currency::Usd => "USD",
            Currency::Cny => "CNY",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "USD" => Some(Currency::Usd),
            "CNY" => Some(Currency::Cny),
            _ => None,
        }
    }
}

impl std::fmt::Display for Currency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

stats_enum!(Granularity {
    Auto => "auto",
    Hour => "hour",
    Day => "day",
    Week => "week",
});

stats_enum!(SeriesQuality {
    Known => "known",
    Partial => "partial",
    Unknown => "unknown",
    Empty => "empty",
});

stats_enum!(GroupBy {
    Provider => "provider",
    Model => "model",
});

stats_enum!(SortMetric {
    TotalTokens => "total_tokens",
    RequestCount => "request_count",
});

stats_enum!(WarningCode {
    UsageMissing => "usage_missing",
    UsagePartial => "usage_partial",
    UsageInvalid => "usage_invalid",
    CollectionDisabled => "collection_disabled",
    CollectorLoss => "collector_loss",
    UncleanExit => "unclean_exit",
    RetentionLimited => "retention_limited",
    ProviderObserveMode => "provider_observe_mode",
});

stats_enum!(HealthState {
    Ready => "ready",
    Disabled => "disabled",
    Degraded => "degraded",
    Migrating => "migrating",
});

stats_enum!(LossScope {
    NoLossObservedInCurrentRun => "no_loss_observed_in_current_run",
    LossObserved => "loss_observed",
    UnknownAfterUncleanExit => "unknown_after_unclean_exit",
});

stats_enum!(MaintenanceKind {
    Migration => "migration",
    Retention => "retention",
    Clear => "clear",
    Reprice => "reprice",
});

stats_enum!(MaintenanceStatus {
    Running => "running",
    Completed => "completed",
    Failed => "failed",
});

stats_enum!(FilterKind {
    Provider => "provider",
    Model => "model",
});

/// Maximum accepted length for identifier-like strings (`provider_id`, ...).
pub const MAX_ID_CHARS: usize = 160;
fn clean_id(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("must not be empty".into());
    }
    if trimmed.chars().count() > MAX_ID_CHARS {
        return Err(format!("must be at most {MAX_ID_CHARS} characters"));
    }
    if trimmed.chars().any(char::is_control) {
        return Err("must not contain control characters".into());
    }
    Ok(trimmed.to_owned())
}

/// Common read filter shared by dashboard, requests, breakdown and reprice.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatsFilter {
    pub from_ms: i64,
    pub to_ms: i64,
    /// IANA timezone name. Empty asks the core for the local zone.
    pub timezone: String,
    pub provider_id: Option<String>,
    pub upstream_model: Option<String>,
    pub route_id: Option<String>,
    pub outcome: Option<Outcome>,
    pub usage_status: Option<UsageStatus>,
    pub include_tests: bool,
}

impl StatsFilter {
    /// Validates every field once; the result is reused by the SQL builder.
    pub fn validate(&self) -> Result<(), String> {
        if self.from_ms < 0 || self.to_ms < 0 {
            return Err(error(ERR_INVALID, "from_ms and to_ms must not be negative"));
        }
        if self.from_ms >= self.to_ms {
            return Err(error(ERR_INVALID, "from_ms must be smaller than to_ms"));
        }
        if self.to_ms - self.from_ms > MAX_RANGE_MS {
            return Err(error(
                ERR_INVALID,
                "range must not exceed 366 days; narrow the range",
            ));
        }
        if self.to_ms / 1000 > i32::MAX as i64 * 4 {
            return Err(error(ERR_INVALID, "to_ms is out of range"));
        }
        if self.timezone.chars().count() > 64 || self.timezone.chars().any(char::is_control) {
            return Err(error(ERR_INVALID, "timezone is not a valid IANA name"));
        }
        for (name, value) in [
            ("provider_id", &self.provider_id),
            ("upstream_model", &self.upstream_model),
            ("route_id", &self.route_id),
        ] {
            if let Some(value) = value {
                clean_id(value).map_err(|e| error(ERR_INVALID, &format!("{name} {e}")))?;
            }
        }
        Ok(())
    }

    /// Normalized key used for cursor consistency checks, never for auth.
    pub fn key(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"wbp-stats-filter-v1\n");
        let mut push = |name: &str, value: &str| {
            hasher.update(name.as_bytes());
            hasher.update(b"=");
            hasher.update(value.as_bytes());
            hasher.update(b"\n");
        };
        push("from", &self.from_ms.to_string());
        push("to", &self.to_ms.to_string());
        push("tz", &self.timezone);
        push("provider", self.provider_id.as_deref().unwrap_or(""));
        push("model", self.upstream_model.as_deref().unwrap_or(""));
        push("route", self.route_id.as_deref().unwrap_or(""));
        push("outcome", self.outcome.map_or("", |v| v.as_str()));
        push("usage", self.usage_status.map_or("", |v| v.as_str()));
        push("tests", if self.include_tests { "1" } else { "0" });
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

/// One currency subtotal. Amounts are decimal-encoded nano units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CostTotalDto {
    pub currency: Currency,
    pub total_cost_nanos: String,
    pub priced_request_count: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SummaryDto {
    pub request_count: i64,
    pub finished_count: i64,
    pub in_flight_count: i64,
    pub completed_count: i64,
    pub failed_count: i64,
    pub rejected_count: i64,
    pub cancelled_count: i64,
    pub interrupted_count: i64,
    pub usage_eligible_count: i64,
    pub usage_complete_count: i64,
    pub usage_partial_count: i64,
    pub usage_missing_count: i64,
    pub usage_invalid_count: i64,
    pub usage_not_applicable_count: i64,
    /// `None` means "no confirmed usage", never zero.
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_read_known_count: i64,
    pub cache_read_denominator_input_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub cache_write_known_count: i64,
    pub input_uncached_tokens: Option<i64>,
    pub input_breakdown_known_count: i64,
    pub reasoning_tokens: Option<i64>,
    pub reasoning_known_count: i64,
    pub success_rate: Option<f64>,
    pub usage_coverage: Option<f64>,
    pub cache_hit_rate: Option<f64>,
    pub avg_duration_ms: Option<f64>,
    pub costs: Vec<CostTotalDto>,
    pub unpriced_request_count: i64,
    /// Requests priced by the explicit "no price configured" default.
    pub default_zero_request_count: i64,
    /// Requests whose amount is decided and therefore included in `costs`.
    pub settled_cost_request_count: i64,
    pub cost_incomplete_request_count: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeriesPointDto {
    pub start_ms: i64,
    pub end_ms: i64,
    pub request_count: i64,
    pub finished_count: i64,
    pub usage_complete_count: i64,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub cache_read_known_count: i64,
    pub cache_write_known_count: i64,
    pub cache_read_denominator_input_tokens: Option<i64>,
    pub costs: Vec<CostTotalDto>,
    pub quality: SeriesQuality,
    pub collection_warning: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WarningDto {
    pub code: WarningCode,
    pub request_count: i64,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DashboardDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    /// Timezone actually used for bucketing (resolved from the filter).
    pub timezone: String,
    pub filter_key: String,
    pub granularity: Granularity,
    pub summary: SummaryDto,
    pub series: Vec<SeriesPointDto>,
    pub warnings: Vec<WarningDto>,
}

/// Cost of one request as settled at finish time or by a later reprice.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CostDto {
    pub status: CostStatus,
    pub currency: Option<Currency>,
    pub total_cost_nanos: Option<String>,
    /// Traceability only: the price configuration this request copied. It is
    /// never joined to compute an amount, so it keeps working after the
    /// configuration is edited or deleted.
    pub source_price_id: Option<String>,
    pub pricing_version: Option<i64>,
}

/// The unit prices one request was admitted with, as stored on its own row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PriceSnapshotDto {
    pub price_source: PriceSource,
    pub price_snapshot_at_ms: Option<i64>,
    pub input_mode: Option<InputMode>,
    pub input_rate_nanos: Option<String>,
    pub output_rate_nanos: Option<String>,
    pub cache_read_rate_nanos: Option<String>,
    pub cache_write_rate_nanos: Option<String>,
    pub source_price_id: Option<String>,
    pub source_price_revision: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestRowDto {
    pub id: i64,
    pub request_id: String,
    pub source: Source,
    pub provider_id: String,
    pub provider_name: String,
    pub route_id: String,
    pub route_name: String,
    pub requested_model: String,
    pub upstream_model: String,
    #[serde(default = "default_upstream_protocol")]
    pub upstream_protocol: String,
    pub response_model: Option<String>,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub stream: bool,
    /// `None` while a started record cannot prove whether the call was sent.
    pub upstream_started: Option<bool>,
    pub http_status: Option<i64>,
    pub outcome: Outcome,
    pub error_kind: Option<ErrorKind>,
    pub duration_ms: Option<i64>,
    pub ttfb_ms: Option<i64>,
    pub first_content_ms: Option<i64>,
    pub usage_status: UsageStatus,
    pub usage_reason: Option<UsageReason>,
    pub usage_source: UsageSource,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub input_uncached_tokens: Option<i64>,
    pub input_breakdown: InputBreakdown,
    pub cost: Option<CostDto>,
}
fn default_upstream_protocol() -> String {
    "openai_chat".into()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestCursor {
    pub last_started_at_ms: i64,
    pub last_id: i64,
    pub as_of_id: i64,
    pub filter_key: String,
    pub run_id: String,
    pub maintenance_revision: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestPageDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub timezone: String,
    pub filter_key: String,
    pub as_of_id: i64,
    pub total_rows: i64,
    pub page_size: i64,
    pub items: Vec<RequestRowDto>,
    pub next_cursor: Option<RequestCursor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PriceRuleDto {
    /// Canonical v2 identifier.
    pub price_id: String,
    /// Transitional alias of `price_id`, kept so older callers keep working.
    /// Both fields always carry the same value.
    pub rule_id: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub currency: Currency,
    pub input_rate_nanos: String,
    pub output_rate_nanos: String,
    pub cache_read_rate_nanos: Option<String>,
    pub cache_write_rate_nanos: Option<String>,
    pub input_mode: InputMode,
    pub label: String,
    pub created_at_ms: i64,
    /// Optimistic-concurrency token for the editable current price.
    pub revision: i64,
    /// Kept for wire compatibility; v2 prices are never "in use".
    pub referenced: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestDetailDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub request: RequestRowDto,
    pub parser_version: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub run_id: String,
    pub cost_parts: Option<CostPartsDto>,
    /// The prices this request was admitted with, read from its own row.
    pub price_snapshot: Option<PriceSnapshotDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CostPartsDto {
    pub status: CostStatus,
    pub currency: Option<Currency>,
    pub input_cost_nanos: Option<String>,
    pub output_cost_nanos: Option<String>,
    pub cache_read_cost_nanos: Option<String>,
    pub cache_write_cost_nanos: Option<String>,
    pub total_cost_nanos: Option<String>,
    pub calculated_at_ms: i64,
    pub pricing_version: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BreakdownItemDto {
    pub key: String,
    pub label: String,
    pub provider_count: i64,
    pub summary: SummaryDto,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BreakdownCursor {
    /// The grouping this cursor was issued for; a page from another grouping is
    /// not reproducible.
    pub group_by: GroupBy,
    pub sort: SortMetric,
    /// The last group *value* (a provider id or a model name), never a column
    /// name: the keyset compares it against the aggregated group key.
    pub last_group_key: String,
    /// The last group's sort metric; `None` means the NULL tail.
    pub last_metric: Option<i64>,
    pub as_of_id: i64,
    pub filter_key: String,
    pub run_id: String,
    pub maintenance_revision: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BreakdownDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub timezone: String,
    pub filter_key: String,
    pub group_by: GroupBy,
    pub sort: SortMetric,
    pub as_of_id: i64,
    pub total_groups: i64,
    pub items: Vec<BreakdownItemDto>,
    pub next_cursor: Option<BreakdownCursor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FilterOptionDto {
    pub key: String,
    pub label: String,
    /// True when the option only exists in historical rows.
    pub historical: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FilterOptionsDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub kind: FilterKind,
    pub items: Vec<FilterOptionDto>,
    pub next_key: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HealthDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub state: HealthState,
    pub enabled: bool,
    pub schema_version: i64,
    pub collection_started_at_ms: Option<i64>,
    pub earliest_record_at_ms: Option<i64>,
    pub retention_days: i64,
    pub record_count: i64,
    pub db_bytes: i64,
    pub wal_bytes: i64,
    pub queue_depth: i64,
    pub queue_capacity: i64,
    pub pending_requests: i64,
    pub dropped_requests_current_run: i64,
    pub write_failures_current_run: i64,
    pub last_commit_at_ms: Option<i64>,
    pub loss_scope: LossScope,
    pub error_code: Option<String>,
    pub settings_revision: i64,
    pub timezone: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettingsDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub enabled: bool,
    pub retention_days: i64,
    /// Currency used for models that have no configured price.
    pub default_currency: Currency,
    pub revision: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderSettingsDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub provider_id: String,
    pub usage_mode: UsageMode,
    pub revision: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PriceListDto {
    #[serde(default)]
    pub catalog_sync: Option<PriceSyncDto>,
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub items: Vec<PriceRuleDto>,
    pub next_rule_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PriceSyncDto {
    pub last_success_day: Option<String>,
    pub updated_at_ms: Option<i64>,
    pub updated_count: i64,
    pub unmatched_count: i64,
    pub catalog_count: i64,
    pub targets_key: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaintenanceDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub task_id: i64,
    pub kind: MaintenanceKind,
    pub status: MaintenanceStatus,
    pub affected_rows: i64,
    pub started_at_ms: i64,
    pub completed_at_ms: Option<i64>,
    pub error_code: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClearPreviewDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub before_ms: i64,
    pub as_of_id: i64,
    pub row_count: i64,
    pub confirmed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R8-06: amounts travel as decimal strings because a total can exceed
    /// `i64`. Parsing them into `i64` and falling back turned a real amount into
    /// "0" or "—" on the page.
    #[test]
    fn amounts_beyond_i64_still_format_exactly() {
        // 2^63 nano units is already past `i64::MAX`.
        let huge = "9223372036854775808";
        assert_eq!(format_amount_text(huge), "9223372036.854775808");
        let bigger = "123456789012345678901234567890";
        assert_eq!(format_amount_text(bigger), "123456789012345678901.23456789");
        assert_eq!(format_amount_text("0"), "0");
        // Below the display floor the page says so instead of showing 0.
        assert_eq!(format_amount_text("1"), "<0.0001");
        assert_eq!(format_amount_text("99999"), "<0.0001");
        assert_eq!(format_amount_text("100000"), "0.0001");
        assert_eq!(format_amount_text("-1000000000"), "-1");
        assert_eq!(format_amount_text("-1"), ">-0.0001");
        // Anything that is not a decimal amount is unknown, never zero.
        assert_eq!(format_amount_text(""), "—");
        assert_eq!(format_amount_text("1.5"), "—");
        assert_eq!(format_amount_text("abc"), "—");
        // The i64 entry point agrees with the shared formatter.
        assert_eq!(format_amount(1_360_000), format_amount_text("1360000"));
        assert_eq!(
            format_amount(i64::MAX),
            format_amount_text(&i64::MAX.to_string())
        );
    }

    fn filter() -> StatsFilter {
        StatsFilter {
            from_ms: 1_790_784_000_000,
            to_ms: 1_791_993_600_000,
            timezone: "Asia/Shanghai".into(),
            ..Default::default()
        }
    }

    #[test]
    fn filter_validation_bounds_range_and_identifiers() {
        let mut f = filter();
        assert!(f.validate().is_ok());
        f.to_ms = f.from_ms;
        assert!(f.validate().unwrap_err().starts_with(ERR_INVALID));
        let mut f = filter();
        f.to_ms = f.from_ms + MAX_RANGE_MS + 1;
        assert!(f.validate().unwrap_err().contains("366 days"));
        let mut f = filter();
        f.provider_id = Some("bad\u{7}id".into());
        assert!(f.validate().is_err());
        let mut f = filter();
        f.provider_id = Some("  ".into());
        assert!(f.validate().is_err());
    }

    #[test]
    fn filter_key_is_stable_and_field_sensitive() {
        let a = filter();
        let b = filter();
        assert_eq!(a.key(), b.key());
        assert_eq!(a.key().len(), 64);
        let mut c = filter();
        c.include_tests = true;
        assert_ne!(a.key(), c.key());
        let mut d = filter();
        d.timezone = "UTC".into();
        assert_ne!(a.key(), d.key());
    }

    #[test]
    fn enum_text_round_trips() {
        for outcome in Outcome::ALL {
            assert_eq!(Outcome::parse(outcome.as_str()), Some(*outcome));
        }
        for status in UsageStatus::ALL {
            assert_eq!(UsageStatus::parse(status.as_str()), Some(*status));
        }
        assert_eq!(
            serde_json::to_string(&Outcome::InFlight).unwrap(),
            "\"in_flight\""
        );
        assert_eq!(serde_json::to_string(&Currency::Usd).unwrap(), "\"USD\"");
        assert!(Outcome::parse("unknown").is_none());
    }

    #[test]
    fn unknown_usage_stays_null_through_serialization() {
        let summary = SummaryDto::default();
        let json = serde_json::to_value(&summary).unwrap();
        assert!(json["input_tokens"].is_null());
        assert_eq!(json["request_count"], 0);
    }
}

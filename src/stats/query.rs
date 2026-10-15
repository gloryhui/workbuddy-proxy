//! Read-only statistics queries: filters, buckets, dashboard, pagination,
//! breakdowns and option lists.
//!
//! Every query runs on the dedicated reader connection with a 2 second budget
//! enforced through `progress_handler`. Amounts are accumulated with `i128` in
//! Rust, never with floating point or `SUM()` of money columns.
use crate::stats::dto::{
    BreakdownCursor, BreakdownItemDto, CostDto, CostStatus, CostTotalDto, Currency, DashboardDto,
    ERR_BUSY, ERR_CURSOR_EXPIRED, ERR_INVALID, ERR_NOT_FOUND, ERR_QUERY_TOO_LARGE, ERR_TIMEOUT,
    ERR_UNAVAILABLE, FilterKind, FilterOptionDto, Granularity, GroupBy, InputBreakdown, InputMode,
    MAX_BUCKETS, MAX_MATCHING_ROWS, Outcome, PriceSnapshotDto, PriceSource, RequestCursor,
    RequestRowDto, SeriesPointDto, SeriesQuality, SortMetric, Source, StatsFilter, SummaryDto,
    UsageReason, UsageSource, UsageStatus, WarningCode, WarningDto, error,
};
use chrono::{Datelike, Duration as ChronoDuration, NaiveDate, TimeZone};
use chrono_tz::Tz;
use rusqlite::{Connection, OptionalExtension, params_from_iter, types::Value};
use std::{
    str::FromStr,
    time::{Duration, Instant},
};

/// Hard budget for a single statistical query.
pub const QUERY_BUDGET: Duration = Duration::from_secs(2);
/// Rows allowed to match one query before the caller must narrow the range.
pub const ROW_BUDGET: i64 = MAX_MATCHING_ROWS;
/// Groups reported for one breakdown page; the total is capped at this + 1.
pub const MAX_GROUPS: i64 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bucket {
    pub start_ms: i64,
    pub end_ms: i64,
}

/// Resolves the IANA timezone requested by the client. An empty name means
/// "use this machine's zone", which is what the native UI defaults to.
pub fn resolve_timezone(name: &str) -> Result<(Tz, String), String> {
    let name = name.trim();
    if name.is_empty() || name.eq_ignore_ascii_case("local") {
        let local = iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".into());
        return match Tz::from_str(&local) {
            Ok(tz) => Ok((tz, local)),
            Err(_) => Ok((chrono_tz::UTC, "UTC".into())),
        };
    }
    Tz::from_str(name)
        .map(|tz| (tz, name.to_owned()))
        .map_err(|_| error(ERR_INVALID, "timezone is not a known IANA name"))
}

pub fn choose_granularity(range_ms: i64, requested: Granularity) -> Granularity {
    match requested {
        Granularity::Auto if range_ms <= 48 * 3_600_000 => Granularity::Hour,
        Granularity::Auto if range_ms <= 90 * 86_400_000 => Granularity::Day,
        Granularity::Auto => Granularity::Week,
        explicit => explicit,
    }
}

/// Local midnight for `date`, choosing the earlier instant on DST ambiguity and
/// the first valid instant when the local midnight does not exist.
fn local_midnight(tz: &Tz, date: NaiveDate) -> i64 {
    let naive = date.and_hms_opt(0, 0, 0).expect("valid midnight");
    if let Some(value) = tz.from_local_datetime(&naive).earliest() {
        return value.timestamp_millis();
    }
    let mut candidate = naive;
    for _ in 0..(48 * 60) {
        candidate += ChronoDuration::minutes(1);
        if let Some(value) = tz.from_local_datetime(&candidate).earliest() {
            return value.timestamp_millis();
        }
    }
    tz.from_utc_datetime(&naive).timestamp_millis()
}

fn local_date(tz: &Tz, ms: i64) -> NaiveDate {
    tz.timestamp_millis_opt(ms)
        .single()
        .map(|value| value.date_naive())
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(1970, 1, 1).unwrap())
}

/// Builds strictly increasing, non-overlapping buckets covering `[from, to)`.
pub fn build_buckets(
    filter: &StatsFilter,
    granularity: Granularity,
) -> Result<(Vec<Bucket>, Granularity), String> {
    let (tz, _) = resolve_timezone(&filter.timezone)?;
    let granularity = choose_granularity(filter.to_ms - filter.from_ms, granularity);
    let mut edges = vec![filter.from_ms];
    match granularity {
        Granularity::Hour | Granularity::Auto => {
            let mut cursor = filter.from_ms + 3_600_000;
            while cursor < filter.to_ms {
                edges.push(cursor);
                cursor += 3_600_000;
                if edges.len() > MAX_BUCKETS + 1 {
                    return Err(error(
                        ERR_INVALID,
                        "range needs more than 400 hourly buckets; widen the granularity",
                    ));
                }
            }
        }
        Granularity::Day | Granularity::Week => {
            let mut date = local_date(&tz, filter.from_ms);
            loop {
                date += ChronoDuration::days(if granularity == Granularity::Week {
                    // Monday-based weeks.
                    7 - date.weekday().num_days_from_monday() as i64
                } else {
                    1
                });
                let boundary = local_midnight(&tz, date);
                if boundary >= filter.to_ms {
                    break;
                }
                if boundary > filter.from_ms {
                    edges.push(boundary);
                }
                if edges.len() > MAX_BUCKETS + 1 {
                    return Err(error(
                        ERR_INVALID,
                        "range needs more than 400 buckets; widen the granularity",
                    ));
                }
            }
        }
    }
    edges.push(filter.to_ms);
    edges.dedup();
    let buckets: Vec<Bucket> = edges
        .windows(2)
        .filter(|window| window[0] < window[1])
        .map(|window| Bucket {
            start_ms: window[0],
            end_ms: window[1],
        })
        .collect();
    if buckets.len() > MAX_BUCKETS {
        return Err(error(
            ERR_INVALID,
            "range needs more than 400 buckets; widen the granularity",
        ));
    }
    Ok((buckets, granularity))
}

/// Builds the shared `WHERE` clause. Values are always bound, never formatted.
struct FilterSql {
    clause: String,
    params: Vec<Value>,
}

impl FilterSql {
    fn build(filter: &StatsFilter, alias: &str) -> Self {
        let prefix = if alias.is_empty() {
            String::new()
        } else {
            format!("{alias}.")
        };
        let mut clause = format!(
            "{prefix}started_at_ms>=? AND {prefix}started_at_ms<? AND ({prefix}source='relay' OR ?=1)"
        );
        let mut params = vec![
            Value::Integer(filter.from_ms),
            Value::Integer(filter.to_ms),
            Value::Integer(if filter.include_tests { 1 } else { 0 }),
        ];
        for (column, value) in [
            ("provider_id", filter.provider_id.as_deref()),
            ("upstream_model", filter.upstream_model.as_deref()),
            ("route_id", filter.route_id.as_deref()),
            ("outcome", filter.outcome.map(|value| value.as_str())),
            (
                "usage_status",
                filter.usage_status.map(|value| value.as_str()),
            ),
        ] {
            if let Some(value) = value {
                clause.push_str(&format!(" AND {prefix}{column}=?"));
                params.push(Value::Text(value.to_owned()));
            }
        }
        Self { clause, params }
    }
}

/// Maps a database failure to a stable, non-leaking API error.
///
/// The rusqlite message embeds the full statement (column list, WHERE clause,
/// bound parameter text), so it must never reach the client: API.md promises a
/// fixed code plus a short hint. Classification uses the SQLite error code, not
/// the rendered text.
fn budget_error(failure: rusqlite::Error) -> String {
    if let rusqlite::Error::SqliteFailure(code, _) = &failure {
        return match code.code {
            rusqlite::ErrorCode::OperationInterrupted => {
                error(ERR_TIMEOUT, "query exceeded its time budget")
            }
            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked => {
                error(ERR_BUSY, "statistics database is busy")
            }
            _ => error(ERR_UNAVAILABLE, "statistics query failed"),
        };
    }
    match failure {
        rusqlite::Error::QueryReturnedNoRows => error(ERR_NOT_FOUND, "no matching row"),
        _ => error(ERR_UNAVAILABLE, "statistics query failed"),
    }
}

/// Installs the deadline handler for the duration of one query and always
/// removes it again, including on early error returns.
struct Budget<'a> {
    conn: &'a Connection,
}

impl<'a> Budget<'a> {
    fn install(conn: &'a Connection) -> Self {
        let deadline = Instant::now() + QUERY_BUDGET;
        let _ = conn.progress_handler(2000, Some(move || Instant::now() >= deadline));
        Self { conn }
    }
}

impl Drop for Budget<'_> {
    fn drop(&mut self) {
        let _ = self.conn.progress_handler(0, None::<fn() -> bool>);
    }
}

/// Aggregated numbers produced by the shared summary SQL.
#[derive(Default)]
struct RawSummary {
    request_count: i64,
    finished_count: i64,
    in_flight_count: i64,
    completed_count: i64,
    failed_count: i64,
    rejected_count: i64,
    cancelled_count: i64,
    interrupted_count: i64,
    usage_eligible_count: i64,
    usage_complete_count: i64,
    usage_partial_count: i64,
    usage_missing_count: i64,
    usage_invalid_count: i64,
    usage_not_applicable_count: i64,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_read_known_count: i64,
    cache_read_denominator_input_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
    cache_write_known_count: i64,
    input_uncached_tokens: Option<i64>,
    input_breakdown_known_count: i64,
    reasoning_tokens: Option<i64>,
    reasoning_known_count: i64,
    avg_duration_ms: Option<f64>,
}

const SUMMARY_COLUMNS: &str = "
  COUNT(*),
  SUM(CASE WHEN outcome<>'in_flight' THEN 1 ELSE 0 END),
  SUM(CASE WHEN outcome='in_flight' THEN 1 ELSE 0 END),
  SUM(CASE WHEN outcome='completed' THEN 1 ELSE 0 END),
  SUM(CASE WHEN outcome IN ('failed','rejected') THEN 1 ELSE 0 END),
  SUM(CASE WHEN outcome='rejected' THEN 1 ELSE 0 END),
  SUM(CASE WHEN outcome='cancelled' THEN 1 ELSE 0 END),
  SUM(CASE WHEN outcome='interrupted' THEN 1 ELSE 0 END),
  SUM(CASE WHEN outcome<>'in_flight' AND upstream_started=1 THEN 1 ELSE 0 END),
  SUM(CASE WHEN (usage_status='complete' AND outcome<>'in_flight') THEN 1 ELSE 0 END),
  SUM(CASE WHEN outcome<>'in_flight' AND usage_status='partial' THEN 1 ELSE 0 END),
  SUM(CASE WHEN outcome<>'in_flight' AND usage_status='missing' THEN 1 ELSE 0 END),
  SUM(CASE WHEN outcome<>'in_flight' AND usage_status='invalid' THEN 1 ELSE 0 END),
  SUM(CASE WHEN outcome<>'in_flight' AND usage_status='not_applicable' THEN 1 ELSE 0 END),
  SUM(CASE WHEN (usage_status='complete' AND outcome<>'in_flight') THEN input_tokens END),
  SUM(CASE WHEN (usage_status='complete' AND outcome<>'in_flight') THEN output_tokens END),
  SUM(CASE WHEN (usage_status='complete' AND outcome<>'in_flight') THEN total_tokens END),
  SUM(CASE WHEN (usage_status='complete' AND outcome<>'in_flight') THEN cache_read_tokens END),
  SUM(CASE WHEN (usage_status='complete' AND outcome<>'in_flight') AND cache_read_tokens IS NOT NULL THEN 1 ELSE 0 END),
  SUM(CASE WHEN (usage_status='complete' AND outcome<>'in_flight') AND cache_read_tokens IS NOT NULL THEN input_tokens END),
  SUM(CASE WHEN (usage_status='complete' AND outcome<>'in_flight') THEN cache_write_tokens END),
  SUM(CASE WHEN (usage_status='complete' AND outcome<>'in_flight') AND cache_write_tokens IS NOT NULL THEN 1 ELSE 0 END),
  SUM(CASE WHEN input_breakdown='disjoint' THEN input_uncached_tokens END),
  SUM(CASE WHEN input_breakdown='disjoint' THEN 1 ELSE 0 END),
  SUM(CASE WHEN (usage_status='complete' AND outcome<>'in_flight') THEN reasoning_tokens END),
  SUM(CASE WHEN (usage_status='complete' AND outcome<>'in_flight') AND reasoning_tokens IS NOT NULL THEN 1 ELSE 0 END),
  AVG(CASE WHEN outcome='completed' THEN duration_ms END)
";

fn read_summary(row: &rusqlite::Row<'_>, base: usize) -> rusqlite::Result<RawSummary> {
    let at = |index: usize| base + index;
    Ok(RawSummary {
        request_count: row.get::<_, Option<i64>>(at(0))?.unwrap_or(0),
        finished_count: row.get::<_, Option<i64>>(at(1))?.unwrap_or(0),
        in_flight_count: row.get::<_, Option<i64>>(at(2))?.unwrap_or(0),
        completed_count: row.get::<_, Option<i64>>(at(3))?.unwrap_or(0),
        failed_count: row.get::<_, Option<i64>>(at(4))?.unwrap_or(0),
        rejected_count: row.get::<_, Option<i64>>(at(5))?.unwrap_or(0),
        cancelled_count: row.get::<_, Option<i64>>(at(6))?.unwrap_or(0),
        interrupted_count: row.get::<_, Option<i64>>(at(7))?.unwrap_or(0),
        usage_eligible_count: row.get::<_, Option<i64>>(at(8))?.unwrap_or(0),
        usage_complete_count: row.get::<_, Option<i64>>(at(9))?.unwrap_or(0),
        usage_partial_count: row.get::<_, Option<i64>>(at(10))?.unwrap_or(0),
        usage_missing_count: row.get::<_, Option<i64>>(at(11))?.unwrap_or(0),
        usage_invalid_count: row.get::<_, Option<i64>>(at(12))?.unwrap_or(0),
        usage_not_applicable_count: row.get::<_, Option<i64>>(at(13))?.unwrap_or(0),
        input_tokens: row.get(at(14))?,
        output_tokens: row.get(at(15))?,
        total_tokens: row.get(at(16))?,
        cache_read_tokens: row.get(at(17))?,
        cache_read_known_count: row.get::<_, Option<i64>>(at(18))?.unwrap_or(0),
        cache_read_denominator_input_tokens: row.get(at(19))?,
        cache_write_tokens: row.get(at(20))?,
        cache_write_known_count: row.get::<_, Option<i64>>(at(21))?.unwrap_or(0),
        input_uncached_tokens: row.get(at(22))?,
        input_breakdown_known_count: row.get::<_, Option<i64>>(at(23))?.unwrap_or(0),
        reasoning_tokens: row.get(at(24))?,
        reasoning_known_count: row.get::<_, Option<i64>>(at(25))?.unwrap_or(0),
        avg_duration_ms: row.get(at(26))?,
    })
}

/// Per-currency accumulation with checked 128-bit addition.
#[derive(Default, Debug, Clone)]
struct CostAccumulator {
    usd: i128,
    cny: i128,
    usd_count: i64,
    cny_count: i64,
}

impl CostAccumulator {
    fn add(&mut self, currency: Currency, nanos: i64) {
        match currency {
            Currency::Usd => {
                self.usd += nanos as i128;
                self.usd_count += 1;
            }
            Currency::Cny => {
                self.cny += nanos as i128;
                self.cny_count += 1;
            }
        }
    }

    fn into_dto(self) -> Vec<CostTotalDto> {
        let mut out = Vec::new();
        if self.usd_count > 0 {
            out.push(CostTotalDto {
                currency: Currency::Usd,
                total_cost_nanos: self.usd.to_string(),
                priced_request_count: self.usd_count,
            });
        }
        if self.cny_count > 0 {
            out.push(CostTotalDto {
                currency: Currency::Cny,
                total_cost_nanos: self.cny.to_string(),
                priced_request_count: self.cny_count,
            });
        }
        out
    }
}

fn ratio(value: i64, denominator: i64) -> Option<f64> {
    (denominator > 0).then(|| value as f64 / denominator as f64)
}

fn to_summary(
    raw: &RawSummary,
    costs: Vec<CostTotalDto>,
    unpriced: i64,
    incomplete: i64,
    default_zero: i64,
    settled: i64,
) -> SummaryDto {
    let success_rate = ratio(raw.completed_count, raw.finished_count);
    let usage_coverage = ratio(raw.usage_complete_count, raw.usage_eligible_count);
    let cache_hit_rate = match (
        raw.cache_read_tokens,
        raw.cache_read_denominator_input_tokens,
    ) {
        (Some(read), Some(input)) if input > 0 => ratio(read, input),
        _ => None,
    };
    SummaryDto {
        request_count: raw.request_count,
        finished_count: raw.finished_count,
        in_flight_count: raw.in_flight_count,
        completed_count: raw.completed_count,
        failed_count: raw.failed_count,
        rejected_count: raw.rejected_count,
        cancelled_count: raw.cancelled_count,
        interrupted_count: raw.interrupted_count,
        usage_eligible_count: raw.usage_eligible_count,
        usage_complete_count: raw.usage_complete_count,
        usage_partial_count: raw.usage_partial_count,
        usage_missing_count: raw.usage_missing_count,
        usage_invalid_count: raw.usage_invalid_count,
        usage_not_applicable_count: raw.usage_not_applicable_count,
        input_tokens: raw.input_tokens,
        output_tokens: raw.output_tokens,
        total_tokens: raw.total_tokens,
        cache_read_tokens: raw.cache_read_tokens,
        cache_read_known_count: raw.cache_read_known_count,
        cache_read_denominator_input_tokens: raw.cache_read_denominator_input_tokens,
        cache_write_tokens: raw.cache_write_tokens,
        cache_write_known_count: raw.cache_write_known_count,
        input_uncached_tokens: raw.input_uncached_tokens,
        input_breakdown_known_count: raw.input_breakdown_known_count,
        reasoning_tokens: raw.reasoning_tokens,
        reasoning_known_count: raw.reasoning_known_count,
        success_rate,
        usage_coverage,
        cache_hit_rate,
        avg_duration_ms: raw.avg_duration_ms,
        costs,
        unpriced_request_count: unpriced,
        default_zero_request_count: default_zero,
        settled_cost_request_count: settled,
        cost_incomplete_request_count: incomplete,
    }
}

/// True when at least one cost row is *not* plain `unpriced`.
///
/// Only in that case may the counts be derived from the summary. A priced row
/// is the common reason, but `unsupported_breakdown`, `overflow`,
/// `usage_incomplete` and `not_applicable` rows matter just as much: assuming
/// "no prices anywhere means everything is unpriced" silently mislabels them.
fn has_classified_costs(conn: &Connection) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM usage_costs WHERE status<>'unpriced' LIMIT 1)",
        [],
        |row| row.get::<_, i64>(0),
    )
    .map(|value| value != 0)
    .map_err(budget_error)
}

/// Money totals plus optional per-bucket splits, accumulated with `i128` in
/// Rust because SQLite `SUM` over nano amounts can overflow `i64`.
struct CostScan {
    /// False when the money scan was skipped; the caller then derives the
    /// unpriced/incomplete counts from the request summary instead.
    scanned: bool,
    totals: CostAccumulator,
    unpriced: i64,
    incomplete: i64,
    /// Requests whose price is the explicit zero default.
    default_zero: i64,
    /// Requests whose amount is decided (any settled status), i.e. the ones the
    /// money totals actually cover.
    settled: i64,
    buckets: Vec<CostAccumulator>,
}

/// Money per group key, accumulated in one pass over the matching rows.
/// Per-group money plus the same counters the summary reports, so a grouping
/// row and the page summary never disagree about what is settled.
#[derive(Default)]
struct GroupCosts {
    totals: CostAccumulator,
    unpriced: i64,
    incomplete: i64,
    default_zero: i64,
    settled: i64,
}

fn scan_costs_grouped(
    conn: &Connection,
    filter: &StatsFilter,
    group_by: GroupBy,
    as_of_id: i64,
) -> Result<std::collections::HashMap<String, GroupCosts>, String> {
    if !has_classified_costs(conn)? {
        return Ok(std::collections::HashMap::new());
    }
    let column = match group_by {
        GroupBy::Provider => "provider_id",
        GroupBy::Model => "upstream_model",
    };
    let sql = FilterSql::build(filter, "");
    // Money must come from exactly the same row set as the request and token
    // counts: the same filter *and* the same frozen id watermark. Without the
    // watermark a page issued at id 3 charged for requests created afterwards.
    let query = format!(
        "SELECT {column}, cost_status, currency, total_cost_nanos
         FROM stats_request_facts
         WHERE {} AND id<=? AND cost_status IS NOT NULL",
        sql.clause
    );
    let mut groups: std::collections::HashMap<String, GroupCosts> =
        std::collections::HashMap::new();
    let mut statement = conn.prepare(&query).map_err(budget_error)?;
    let mut bound: Vec<rusqlite::types::Value> = sql.params.clone();
    bound.push(rusqlite::types::Value::Integer(as_of_id));
    let mut rows = statement
        .query(params_from_iter(bound))
        .map_err(budget_error)?;
    while let Some(row) = rows.next().map_err(budget_error)? {
        let key: String = row.get(0).map_err(budget_error)?;
        let status: String = row.get(1).map_err(budget_error)?;
        let entry = groups.entry(key).or_default();
        match CostStatus::parse(&status) {
            Some(CostStatus::Priced)
            | Some(CostStatus::DefaultZero)
            | Some(CostStatus::NotApplicable)
            | Some(CostStatus::LegacyUnknown) => {
                let currency: String = row.get(2).map_err(budget_error)?;
                let nanos: Option<i64> = row.get(3).map_err(budget_error)?;
                if CostStatus::parse(&status) == Some(CostStatus::DefaultZero) {
                    entry.default_zero += 1;
                }
                if let Some(nanos) = nanos {
                    entry.settled += 1;
                    entry
                        .totals
                        .add(Currency::parse(&currency).unwrap_or(Currency::Usd), nanos);
                }
            }
            Some(CostStatus::Unpriced) => entry.unpriced += 1,
            Some(CostStatus::UsageIncomplete)
            | Some(CostStatus::UnsupportedBreakdown)
            | Some(CostStatus::Overflow)
            | Some(CostStatus::Pending) => entry.incomplete += 1,
            None => {}
        }
    }
    Ok(groups)
}

fn scan_costs(
    conn: &Connection,
    filter: &StatsFilter,
    buckets: Option<&[Bucket]>,
) -> Result<CostScan, String> {
    let mut sql = FilterSql::build(filter, "");
    let query = format!(
        "SELECT cost_status, currency, total_cost_nanos, started_at_ms
         FROM stats_request_facts WHERE {} AND cost_status IS NOT NULL",
        sql.clause
    );
    let mut scan = CostScan {
        scanned: false,
        totals: CostAccumulator::default(),
        unpriced: 0,
        incomplete: 0,
        default_zero: 0,
        settled: 0,
        buckets: vec![CostAccumulator::default(); buckets.map_or(0, <[Bucket]>::len)],
    };
    if !has_classified_costs(conn)? {
        return Ok(scan);
    }
    scan.scanned = true;
    let mut statement = conn.prepare(&query).map_err(budget_error)?;
    let mut rows = statement
        .query(params_from_iter(sql.params.iter()))
        .map_err(budget_error)?;
    while let Some(row) = rows.next().map_err(budget_error)? {
        let status: String = row.get(0).map_err(budget_error)?;
        match CostStatus::parse(&status) {
            // Every settled status contributes its stored amount: a confirmed
            // zero from the "no price configured" default and a
            // `legacy_unknown` amount are money the user must still see.
            Some(CostStatus::Priced)
            | Some(CostStatus::DefaultZero)
            | Some(CostStatus::NotApplicable)
            | Some(CostStatus::LegacyUnknown) => {
                let currency: String = row.get(1).map_err(budget_error)?;
                let nanos: Option<i64> = row.get(2).map_err(budget_error)?;
                if CostStatus::parse(&status) == Some(CostStatus::DefaultZero) {
                    scan.default_zero += 1;
                }
                if let Some(nanos) = nanos {
                    scan.settled += 1;
                    let currency = Currency::parse(&currency).unwrap_or(Currency::Usd);
                    scan.totals.add(currency, nanos);
                    if let Some(buckets) = buckets
                        && !buckets.is_empty()
                    {
                        let started: i64 = row.get(3).map_err(budget_error)?;
                        let index = buckets.partition_point(|bucket| bucket.end_ms <= started);
                        if let Some(bucket) = buckets.get(index)
                            && bucket.start_ms <= started
                            && started < bucket.end_ms
                        {
                            scan.buckets[index].add(currency, nanos);
                        }
                    }
                }
            }
            Some(CostStatus::Unpriced) => scan.unpriced += 1,
            Some(CostStatus::UsageIncomplete)
            | Some(CostStatus::UnsupportedBreakdown)
            | Some(CostStatus::Overflow)
            | Some(CostStatus::Pending) => scan.incomplete += 1,
            None => {}
        }
    }
    let _ = &mut sql;
    Ok(scan)
}

/// Counts the unpriced/incomplete buckets from an already-computed summary.
fn derived_cost_counts(raw: &RawSummary) -> (i64, i64) {
    let incomplete = raw.usage_partial_count + raw.usage_missing_count + raw.usage_invalid_count;
    let unpriced = (raw.finished_count - raw.usage_not_applicable_count - incomplete).max(0);
    (unpriced, incomplete)
}

/// Runtime facts the dashboard needs but the database does not hold.
#[derive(Debug)]
pub struct RuntimeFlags {
    pub enabled: bool,
    pub dropped_current_run: i64,
    pub unclean_exit: bool,
    pub observe_mode_providers: i64,
}

#[allow(clippy::too_many_arguments)]
pub fn dashboard(
    conn: &Connection,
    filter: &StatsFilter,
    granularity: Granularity,
    flags: &RuntimeFlags,
    generation: &str,
    request_tag: &str,
    generated_at_ms: i64,
) -> Result<DashboardDto, String> {
    // The summary aggregation already counts the matching rows, so the row
    // budget is checked there instead of with a second full scan.
    let _budget = Budget::install(conn);
    dashboard_inner(
        conn,
        filter,
        granularity,
        flags,
        generation,
        request_tag,
        generated_at_ms,
    )
}

#[allow(clippy::too_many_arguments)]
fn dashboard_inner(
    conn: &Connection,
    filter: &StatsFilter,
    granularity: Granularity,
    flags: &RuntimeFlags,
    generation: &str,
    request_tag: &str,
    generated_at_ms: i64,
) -> Result<DashboardDto, String> {
    let (buckets, granularity_used) = build_buckets(filter, granularity)?;
    let (_, timezone) = resolve_timezone(&filter.timezone)?;
    let sql = FilterSql::build(filter, "");
    // Summary, money and series share one read transaction, so the cards and
    // the chart are always derived from the same database snapshot.
    let transaction = conn.unchecked_transaction().map_err(budget_error)?;
    let summary_sql = format!(
        "SELECT {SUMMARY_COLUMNS} FROM usage_requests WHERE {}",
        sql.clause
    );
    let raw = transaction
        .query_row(&summary_sql, params_from_iter(sql.params.iter()), |row| {
            read_summary(row, 0)
        })
        .map_err(budget_error)?;
    if raw.request_count > ROW_BUDGET {
        return Err(error(
            ERR_QUERY_TOO_LARGE,
            "more than 1,000,000 rows match; narrow the range",
        ));
    }
    // One money scan fills the totals and every bucket at once.
    let mut scan = scan_costs(&transaction, filter, Some(&buckets))?;
    if !scan.scanned {
        let (unpriced, incomplete) = derived_cost_counts(&raw);
        scan.unpriced = unpriced;
        scan.incomplete = incomplete;
    }
    let summary = to_summary(
        &raw,
        std::mem::take(&mut scan.totals).into_dto(),
        scan.unpriced,
        scan.incomplete,
        scan.default_zero,
        scan.settled,
    );
    let series = series_inner(&transaction, filter, &buckets, &mut scan.buckets)?;
    let warnings = warnings(filter, &raw, flags)?;
    transaction.commit().map_err(budget_error)?;
    Ok(DashboardDto {
        api_version: crate::stats::dto::API_VERSION,
        request_tag: request_tag.to_owned(),
        generated_at_ms,
        generation: generation.to_owned(),
        timezone,
        filter_key: filter.key(),
        granularity: granularity_used,
        summary,
        series,
        warnings,
    })
}

type SeriesRow = (
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    i64,
    i64,
    i64,
    i64,
    Option<i64>,
    i64,
    i64,
);

fn series_inner(
    conn: &Connection,
    filter: &StatsFilter,
    buckets: &[Bucket],
    bucket_costs: &mut [CostAccumulator],
) -> Result<Vec<SeriesPointDto>, String> {
    let mut params = Vec::new();
    for bucket in buckets {
        params.push(Value::Integer(bucket.start_ms));
        params.push(Value::Integer(bucket.end_ms));
    }
    let placeholders = buckets
        .iter()
        .enumerate()
        .map(|(index, _)| format!("(?{},?{})", index * 2 + 1, index * 2 + 2))
        .collect::<Vec<_>>()
        .join(",");
    let mut extra = String::new();
    params.push(Value::Integer(if filter.include_tests { 1 } else { 0 }));
    for (column, value) in [
        ("provider_id", filter.provider_id.as_deref()),
        ("upstream_model", filter.upstream_model.as_deref()),
        ("route_id", filter.route_id.as_deref()),
        ("outcome", filter.outcome.map(|value| value.as_str())),
        (
            "usage_status",
            filter.usage_status.map(|value| value.as_str()),
        ),
    ] {
        if let Some(value) = value {
            extra.push_str(&format!(" AND r.{column}=?"));
            params.push(Value::Text(value.to_owned()));
        }
    }
    params.push(Value::Integer(filter.from_ms));
    params.push(Value::Integer(filter.to_ms));
    let complete = "(r.usage_status='complete' AND r.outcome<>'in_flight')";
    let sql = format!(
        "WITH buckets(bucket_start,bucket_end) AS (VALUES {placeholders})
         SELECT b.bucket_start,
           COUNT(r.id),
           SUM(CASE WHEN r.outcome<>'in_flight' THEN 1 ELSE 0 END),
           SUM(CASE WHEN {complete} THEN 1 ELSE 0 END),
           SUM(CASE WHEN {complete} THEN r.input_tokens END),
           SUM(CASE WHEN {complete} THEN r.output_tokens END),
           SUM(CASE WHEN {complete} THEN r.total_tokens END),
           SUM(CASE WHEN {complete} THEN r.cache_read_tokens END),
           SUM(CASE WHEN {complete} THEN r.cache_write_tokens END),
           SUM(CASE WHEN {complete} AND r.cache_read_tokens IS NOT NULL THEN 1 ELSE 0 END),
           SUM(CASE WHEN {complete} AND r.cache_write_tokens IS NOT NULL THEN 1 ELSE 0 END),
           SUM(CASE WHEN r.outcome<>'in_flight' AND r.usage_status='partial' THEN 1 ELSE 0 END),
           SUM(CASE WHEN r.outcome<>'in_flight' AND r.upstream_started=1 THEN 1 ELSE 0 END),
           SUM(CASE WHEN {complete} AND r.cache_read_tokens IS NOT NULL THEN r.input_tokens END),
           SUM(CASE WHEN r.outcome='in_flight' THEN 1 ELSE 0 END),
           SUM(CASE WHEN r.outcome<>'in_flight' AND r.usage_status IN ('missing','invalid') THEN 1 ELSE 0 END)
         FROM buckets b LEFT JOIN usage_requests r
           ON r.started_at_ms>=b.bucket_start AND r.started_at_ms<b.bucket_end
          AND (r.source='relay' OR ?=1){extra}
          AND r.started_at_ms>=? AND r.started_at_ms<?
         GROUP BY b.bucket_start ORDER BY b.bucket_start"
    );
    let mut statement = conn.prepare(&sql).map_err(budget_error)?;
    let mut rows = statement
        .query(params_from_iter(params.iter()))
        .map_err(budget_error)?;
    let mut seen: std::collections::HashMap<i64, SeriesRow> = std::collections::HashMap::new();
    while let Some(row) = rows.next().map_err(budget_error)? {
        let bucket_start: i64 = row.get(0).map_err(budget_error)?;
        let value: SeriesRow = (
            bucket_start,
            row.get(1).map_err(budget_error)?,
            row.get::<_, Option<i64>>(2)
                .map_err(budget_error)?
                .unwrap_or(0),
            row.get::<_, Option<i64>>(3)
                .map_err(budget_error)?
                .unwrap_or(0),
            row.get::<_, Option<i64>>(4)
                .map_err(budget_error)?
                .unwrap_or(0),
            row.get::<_, Option<i64>>(5)
                .map_err(budget_error)?
                .unwrap_or(0),
            row.get(6).map_err(budget_error)?,
            row.get(7).map_err(budget_error)?,
            row.get(8).map_err(budget_error)?,
            row.get::<_, Option<i64>>(9)
                .map_err(budget_error)?
                .unwrap_or(0),
            row.get::<_, Option<i64>>(10)
                .map_err(budget_error)?
                .unwrap_or(0),
            row.get::<_, Option<i64>>(11)
                .map_err(budget_error)?
                .unwrap_or(0),
            row.get::<_, Option<i64>>(12)
                .map_err(budget_error)?
                .unwrap_or(0),
            row.get(13).map_err(budget_error)?,
            row.get::<_, Option<i64>>(14)
                .map_err(budget_error)?
                .unwrap_or(0),
            row.get::<_, Option<i64>>(15)
                .map_err(budget_error)?
                .unwrap_or(0),
        );
        seen.insert(bucket_start, value);
    }
    drop(rows);
    drop(statement);
    let coverage = collection_coverage(conn, filter.from_ms, filter.to_ms)?;
    let mut out = Vec::with_capacity(buckets.len());
    for (position, bucket) in buckets.iter().enumerate() {
        let value = seen.get(&bucket.start_ms);
        let costs = std::mem::take(&mut bucket_costs[position]).into_dto();
        let (
            request_count,
            finished,
            complete_count,
            input,
            output,
            total,
            cache_read,
            cache_write,
            read_known,
            write_known,
            partial,
            eligible,
            denominator,
            in_flight,
            missing_or_invalid,
        ) = match value {
            Some(value) => (
                value.1, value.2, value.3, value.4, value.5, value.6, value.7, value.8, value.9,
                value.10, value.11, value.12, value.13, value.14, value.15,
            ),
            None => (
                0i64, 0i64, 0i64, 0i64, 0i64, None, None, None, 0i64, 0i64, 0i64, 0i64, None, 0i64,
                0i64,
            ),
        };
        let fully_covered = coverage.covers(bucket.start_ms, bucket.end_ms);
        let (quality, collection_warning) = if request_count == 0 {
            // An empty bucket is a proven zero only while collection is known
            // to have been running; otherwise it stays unknown.
            if fully_covered {
                (SeriesQuality::Empty, false)
            } else {
                (SeriesQuality::Unknown, true)
            }
        } else if complete_count >= eligible && eligible > 0 {
            (SeriesQuality::Known, !fully_covered)
        } else if complete_count > 0 || partial > 0 {
            (SeriesQuality::Partial, !fully_covered)
        } else {
            // Requests exist but no complete usage: never claim a value.
            let _ = (in_flight, missing_or_invalid);
            (SeriesQuality::Unknown, !fully_covered)
        };
        let has_usage = complete_count > 0;
        // A bucket that is provably inside a collecting window and saw no
        // request is a real zero, not an unknown. Anything else (uncovered
        // window, degraded/paused collection, or requests without usage) stays
        // `null` so the chart can draw a gap instead of inventing a value.
        let proven_zero = request_count == 0 && fully_covered && !collection_warning;
        let (input_out, output_out, total_out) = if proven_zero {
            (Some(0), Some(0), Some(0))
        } else {
            (
                has_usage.then_some(input),
                has_usage.then_some(output),
                total,
            )
        };
        out.push(SeriesPointDto {
            start_ms: bucket.start_ms,
            end_ms: bucket.end_ms,
            request_count,
            finished_count: finished,
            usage_complete_count: complete_count,
            input_tokens: input_out,
            output_tokens: output_out,
            total_tokens: total_out,
            cache_read_tokens: cache_read,
            cache_write_tokens: cache_write,
            cache_read_known_count: read_known,
            cache_write_known_count: write_known,
            cache_read_denominator_input_tokens: denominator,
            costs,
            quality,
            collection_warning,
        });
    }
    Ok(out)
}

fn warnings(
    filter: &StatsFilter,
    raw: &RawSummary,
    flags: &RuntimeFlags,
) -> Result<Vec<WarningDto>, String> {
    let mut out = Vec::new();
    let push = |out: &mut Vec<WarningDto>, code: WarningCode, count: i64| {
        if count > 0 {
            out.push(WarningDto {
                code,
                request_count: count,
                from_ms: Some(filter.from_ms),
                to_ms: Some(filter.to_ms),
            });
        }
    };
    push(&mut out, WarningCode::UsageMissing, raw.usage_missing_count);
    push(&mut out, WarningCode::UsagePartial, raw.usage_partial_count);
    push(&mut out, WarningCode::UsageInvalid, raw.usage_invalid_count);
    if !flags.enabled {
        out.push(WarningDto {
            code: WarningCode::CollectionDisabled,
            request_count: 0,
            from_ms: Some(filter.from_ms),
            to_ms: Some(filter.to_ms),
        });
    }
    push(
        &mut out,
        WarningCode::CollectorLoss,
        flags.dropped_current_run,
    );
    if flags.unclean_exit {
        out.push(WarningDto {
            code: WarningCode::UncleanExit,
            request_count: 0,
            from_ms: None,
            to_ms: None,
        });
    }
    if flags.observe_mode_providers > 0 {
        out.push(WarningDto {
            code: WarningCode::ProviderObserveMode,
            request_count: flags.observe_mode_providers,
            from_ms: None,
            to_ms: None,
        });
    }
    Ok(out)
}

/// Merged collection intervals used to decide whether an empty bucket is a
/// proven zero or an unknown gap.
struct Coverage {
    intervals: Vec<(i64, i64)>,
}

impl Coverage {
    fn covers(&self, start: i64, end: i64) -> bool {
        let mut cursor = start;
        for (from, to) in &self.intervals {
            if *from > cursor {
                return false;
            }
            if *to > cursor {
                cursor = *to;
            }
            if cursor >= end {
                return true;
            }
        }
        cursor >= end
    }
}

fn collection_coverage(conn: &Connection, from_ms: i64, to_ms: i64) -> Result<Coverage, String> {
    let mut statement = conn
        .prepare(
            "SELECT started_at_ms, COALESCE(ended_at_ms, ?2) FROM stats_collection_intervals
             WHERE state='collecting' AND started_at_ms<?2 AND COALESCE(ended_at_ms, ?2)>?1
             ORDER BY started_at_ms",
        )
        .map_err(budget_error)?;
    let mut rows = statement
        .query(rusqlite::params![from_ms, to_ms])
        .map_err(budget_error)?;
    let mut intervals = Vec::new();
    while let Some(row) = rows.next().map_err(budget_error)? {
        let start: i64 = row.get(0).map_err(budget_error)?;
        let end: i64 = row.get(1).map_err(budget_error)?;
        let start = start.max(from_ms);
        let end = end.min(to_ms);
        if end > start {
            match intervals.last_mut() {
                Some((_, last_end)) if start <= *last_end => *last_end = (*last_end).max(end),
                _ => intervals.push((start, end)),
            }
        }
    }
    Ok(Coverage { intervals })
}

pub struct RequestPage {
    pub as_of_id: i64,
    pub total_rows: i64,
    pub items: Vec<RequestRowDto>,
    pub next_cursor: Option<RequestCursor>,
}

pub fn requests(
    conn: &Connection,
    filter: &StatsFilter,
    page_size: i64,
    cursor: Option<&RequestCursor>,
) -> Result<RequestPage, String> {
    // The first page counts the matching rows anyway, so that count doubles as
    // the row-budget check instead of adding a second full scan.
    let _budget = Budget::install(conn);
    requests_inner(conn, filter, page_size, cursor)
}

const ROW_COLUMNS: &str =
    "r.id, r.request_id, r.source, r.provider_id, r.provider_name, r.route_id,
    r.route_name, r.requested_model, r.upstream_model, r.response_model, r.started_at_ms,
    r.ended_at_ms, r.stream, r.upstream_started, r.http_status, r.outcome, r.error_kind,
    r.duration_ms, r.ttfb_ms, r.first_content_ms, r.usage_status, r.usage_reason, r.usage_source,
    r.input_tokens, r.output_tokens, r.total_tokens, r.cache_read_tokens, r.cache_write_tokens,
    r.reasoning_tokens, r.input_uncached_tokens, r.input_breakdown,
    c.status, c.currency, c.total_cost_nanos, c.source_price_id, c.pricing_version,r.upstream_protocol";
const ROW_COLUMN_COUNT: usize = 37;

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RequestRowDto> {
    let cost_status: Option<String> = row.get(31)?;
    let cost = cost_status.map(|status| CostDto {
        status: CostStatus::parse(&status).unwrap_or(CostStatus::Unpriced),
        currency: row
            .get::<_, Option<String>>(32)
            .ok()
            .flatten()
            .and_then(|value| Currency::parse(&value)),
        total_cost_nanos: row
            .get::<_, Option<i64>>(33)
            .ok()
            .flatten()
            .map(|value| value.to_string()),
        source_price_id: row.get(34).ok().flatten(),
        pricing_version: row.get(35).ok().flatten(),
    });
    Ok(RequestRowDto {
        id: row.get(0)?,
        request_id: row.get(1)?,
        source: Source::parse(&row.get::<_, String>(2)?).unwrap_or(Source::Relay),
        provider_id: row.get(3)?,
        provider_name: row.get(4)?,
        route_id: row.get(5)?,
        route_name: row.get(6)?,
        requested_model: row.get(7)?,
        upstream_model: row.get(8)?,
        upstream_protocol: row.get(36)?,
        response_model: row.get(9)?,
        started_at_ms: row.get(10)?,
        ended_at_ms: row.get(11)?,
        stream: row.get::<_, i64>(12)? != 0,
        upstream_started: row.get::<_, Option<i64>>(13)?.map(|value| value != 0),
        http_status: row.get(14)?,
        outcome: Outcome::parse(&row.get::<_, String>(15)?).unwrap_or(Outcome::Failed),
        error_kind: row
            .get::<_, Option<String>>(16)?
            .as_deref()
            .and_then(crate::stats::dto::ErrorKind::parse),
        duration_ms: row.get(17)?,
        ttfb_ms: row.get(18)?,
        first_content_ms: row.get(19)?,
        usage_status: UsageStatus::parse(&row.get::<_, String>(20)?)
            .unwrap_or(UsageStatus::Missing),
        usage_reason: row
            .get::<_, Option<String>>(21)?
            .as_deref()
            .and_then(UsageReason::parse),
        usage_source: UsageSource::parse(&row.get::<_, String>(22)?).unwrap_or(UsageSource::None),
        input_tokens: row.get(23)?,
        output_tokens: row.get(24)?,
        total_tokens: row.get(25)?,
        cache_read_tokens: row.get(26)?,
        cache_write_tokens: row.get(27)?,
        reasoning_tokens: row.get(28)?,
        input_uncached_tokens: row.get(29)?,
        input_breakdown: InputBreakdown::parse(&row.get::<_, String>(30)?)
            .unwrap_or(InputBreakdown::Unknown),
        cost,
    })
}

/// Rejects a cursor whose identity no longer matches the query it addresses.
///
/// A cursor embeds the filter, the collection run and the maintenance revision
/// it was issued under; if any of them changed (or the numeric fields are out
/// of range) the page it points at is not reproducible any more and the caller
/// must restart pagination instead of silently receiving a wrong page.
pub fn validate_cursor(
    cursor: &RequestCursor,
    filter: &StatsFilter,
    run_id: &str,
    maintenance_revision: i64,
) -> Result<(), String> {
    if cursor.last_started_at_ms < 0
        || cursor.last_id < 0
        || cursor.as_of_id < 0
        || cursor.maintenance_revision < 0
    {
        return Err(error(ERR_CURSOR_EXPIRED, "cursor contains negative fields"));
    }
    if cursor.filter_key != filter.key() {
        return Err(error(
            ERR_CURSOR_EXPIRED,
            "cursor was issued for a different filter",
        ));
    }
    if cursor.run_id != run_id {
        return Err(error(
            ERR_CURSOR_EXPIRED,
            "cursor was issued for a different collection run",
        ));
    }
    if cursor.maintenance_revision != maintenance_revision {
        return Err(error(
            ERR_CURSOR_EXPIRED,
            "cursor was invalidated by maintenance",
        ));
    }
    Ok(())
}

/// The same checks for a grouping cursor.
pub fn validate_breakdown_cursor(
    cursor: &BreakdownCursor,
    filter: &StatsFilter,
    group_by: GroupBy,
    sort: SortMetric,
    run_id: &str,
    maintenance_revision: i64,
) -> Result<(), String> {
    if cursor.as_of_id < 0 || cursor.maintenance_revision < 0 {
        return Err(error(ERR_CURSOR_EXPIRED, "cursor contains negative fields"));
    }
    if cursor.filter_key != filter.key() {
        return Err(error(
            ERR_CURSOR_EXPIRED,
            "cursor was issued for a different filter",
        ));
    }
    if cursor.run_id != run_id {
        return Err(error(
            ERR_CURSOR_EXPIRED,
            "cursor was issued for a different collection run",
        ));
    }
    if cursor.maintenance_revision != maintenance_revision {
        return Err(error(
            ERR_CURSOR_EXPIRED,
            "cursor was invalidated by maintenance",
        ));
    }
    if cursor.group_by != group_by || cursor.sort != sort {
        return Err(error(
            ERR_CURSOR_EXPIRED,
            "cursor was issued for a different grouping",
        ));
    }
    if cursor.last_group_key.is_empty() {
        return Err(error(ERR_CURSOR_EXPIRED, "cursor has no group key"));
    }
    Ok(())
}

fn requests_inner(
    conn: &Connection,
    filter: &StatsFilter,
    page_size: i64,
    cursor: Option<&RequestCursor>,
) -> Result<RequestPage, String> {
    let sql = FilterSql::build(filter, "");
    let transaction = conn.unchecked_transaction().map_err(budget_error)?;
    let as_of_id = match cursor {
        Some(cursor) => cursor.as_of_id,
        None => transaction
            .query_row(
                "SELECT COALESCE(MAX(id),0) FROM usage_requests",
                [],
                |row| row.get(0),
            )
            .map_err(budget_error)?,
    };
    let total_rows = match cursor {
        Some(_) => -1,
        None => {
            let count_sql = format!(
                "SELECT COUNT(*) FROM usage_requests WHERE {} AND id<=?",
                sql.clause
            );
            let mut params = sql.params.clone();
            params.push(Value::Integer(as_of_id));
            transaction
                .query_row(&count_sql, params_from_iter(params.iter()), |row| {
                    row.get(0)
                })
                .map_err(budget_error)?
        }
    };
    let mut clause = format!("{} AND r.id<=?", sql.clause);
    let mut params = sql.params.clone();
    params.push(Value::Integer(as_of_id));
    if let Some(cursor) = cursor {
        clause.push_str(" AND (r.started_at_ms<? OR (r.started_at_ms=? AND r.id<?))");
        params.push(Value::Integer(cursor.last_started_at_ms));
        params.push(Value::Integer(cursor.last_started_at_ms));
        params.push(Value::Integer(cursor.last_id));
    }
    let query = format!(
        "SELECT {ROW_COLUMNS} FROM usage_requests r
         LEFT JOIN usage_costs c ON c.request_id=r.request_id
         WHERE {clause} ORDER BY r.started_at_ms DESC, r.id DESC LIMIT ?"
    );
    params.push(Value::Integer(page_size + 1));
    let mut statement = transaction.prepare(&query).map_err(budget_error)?;
    let mut rows = statement
        .query(params_from_iter(params.iter()))
        .map_err(budget_error)?;
    let mut items = Vec::new();
    while let Some(row) = rows.next().map_err(budget_error)? {
        items.push(read_row(row).map_err(budget_error)?);
    }
    drop(rows);
    drop(statement);
    let has_more = items.len() as i64 > page_size;
    if has_more {
        items.truncate(page_size as usize);
    }
    let next_cursor = if has_more {
        items.last().map(|last| RequestCursor {
            last_started_at_ms: last.started_at_ms,
            last_id: last.id,
            as_of_id,
            filter_key: filter.key(),
            run_id: String::new(),
            maintenance_revision: 0,
        })
    } else {
        None
    };
    transaction.commit().map_err(budget_error)?;
    Ok(RequestPage {
        as_of_id,
        total_rows,
        items,
        next_cursor,
    })
}

pub struct RequestDetail {
    pub row: RequestRowDto,
    pub parser_version: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    /// The unit prices this request was admitted with. It is read from the
    /// request's own row, so it survives editing or deleting the configuration.
    pub price_snapshot: Option<PriceSnapshotDto>,
    pub cost_parts: Option<CostPartsRow>,
}

pub struct CostPartsRow {
    pub status: CostStatus,
    pub currency: Option<Currency>,
    pub input: Option<i64>,
    pub output: Option<i64>,
    pub cache_read: Option<i64>,
    pub cache_write: Option<i64>,
    pub total: Option<i64>,
    pub calculated_at_ms: i64,
    pub pricing_version: i64,
}

pub fn request_detail(conn: &Connection, request_id: &str) -> Result<RequestDetail, String> {
    let _budget = Budget::install(conn);
    request_detail_inner(conn, request_id)
}

fn request_detail_inner(conn: &Connection, request_id: &str) -> Result<RequestDetail, String> {
    let query = format!(
        "SELECT {ROW_COLUMNS}, r.parser_version, r.created_at_ms, r.updated_at_ms
         FROM usage_requests r LEFT JOIN usage_costs c ON c.request_id=r.request_id
         WHERE r.request_id=?"
    );
    let mut row = conn
        .query_row(&query, [request_id], |row| {
            let parsed = read_row(row)?;
            Ok((
                parsed,
                row.get::<_, i64>(ROW_COLUMN_COUNT)?,
                row.get::<_, i64>(ROW_COLUMN_COUNT + 1)?,
                row.get::<_, i64>(ROW_COLUMN_COUNT + 2)?,
            ))
        })
        .optional()
        .map_err(budget_error)?
        .ok_or_else(|| error(ERR_NOT_FOUND, "request was not found"))?;
    let detail = conn
        .query_row(
            "SELECT source_price_id, status, currency, input_cost_nanos, output_cost_nanos,
                    cache_read_cost_nanos, cache_write_cost_nanos, total_cost_nanos,
                    calculated_at_ms, pricing_version, price_source, price_snapshot_at_ms,
                    input_mode, input_rate_nanos, output_rate_nanos,
                    cache_read_rate_nanos, cache_write_rate_nanos, source_price_revision
             FROM usage_costs WHERE request_id=?",
            [request_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, Option<i64>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, Option<i64>>(13)?,
                    row.get::<_, Option<i64>>(14)?,
                    row.get::<_, Option<i64>>(15)?,
                    row.get::<_, Option<i64>>(16)?,
                    row.get::<_, Option<i64>>(17)?,
                ))
            },
        )
        .optional()
        .map_err(budget_error)?;
    let (snapshot, cost_parts) = match detail {
        Some((
            source_price_id,
            status,
            currency,
            input,
            output,
            read,
            write,
            total,
            at,
            version,
            price_source,
            snapshot_at,
            mode,
            input_rate,
            output_rate,
            read_rate,
            write_rate,
            source_revision,
        )) => (
            Some(PriceSnapshotDto {
                price_source: PriceSource::parse(&price_source)
                    .unwrap_or(PriceSource::LegacyUnknown),
                price_snapshot_at_ms: snapshot_at,
                input_mode: mode.as_deref().and_then(InputMode::parse),
                input_rate_nanos: input_rate.map(|value| value.to_string()),
                output_rate_nanos: output_rate.map(|value| value.to_string()),
                cache_read_rate_nanos: read_rate.map(|value| value.to_string()),
                cache_write_rate_nanos: write_rate.map(|value| value.to_string()),
                source_price_id: source_price_id.clone(),
                source_price_revision: source_revision,
            }),
            Some(CostPartsRow {
                status: CostStatus::parse(&status).unwrap_or(CostStatus::Unpriced),
                currency: currency.as_deref().and_then(Currency::parse),
                input,
                output,
                cache_read: read,
                cache_write: write,
                total,
                calculated_at_ms: at,
                pricing_version: version,
            }),
        ),
        None => (None, None),
    };
    let (row_ref, parser_version, created_at_ms, updated_at_ms) = &mut row;
    Ok(RequestDetail {
        row: std::mem::replace(
            row_ref,
            RequestRowDto {
                id: 0,
                request_id: String::new(),
                source: Source::Relay,
                provider_id: String::new(),
                provider_name: String::new(),
                route_id: String::new(),
                route_name: String::new(),
                requested_model: String::new(),
                upstream_model: String::new(),
                upstream_protocol: "openai_chat".into(),
                response_model: None,
                started_at_ms: 0,
                ended_at_ms: None,
                stream: false,
                upstream_started: None,
                http_status: None,
                outcome: Outcome::Failed,
                error_kind: None,
                duration_ms: None,
                ttfb_ms: None,
                first_content_ms: None,
                usage_status: UsageStatus::Missing,
                usage_reason: None,
                usage_source: UsageSource::None,
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
                reasoning_tokens: None,
                input_uncached_tokens: None,
                input_breakdown: InputBreakdown::Unknown,
                cost: None,
            },
        ),
        parser_version: *parser_version,
        created_at_ms: *created_at_ms,
        updated_at_ms: *updated_at_ms,
        price_snapshot: snapshot,
        cost_parts,
    })
}

pub struct BreakdownPage {
    pub as_of_id: i64,
    pub total_groups: i64,
    pub items: Vec<BreakdownItemDto>,
    pub next_cursor: Option<BreakdownCursor>,
}

pub fn breakdown(
    conn: &Connection,
    filter: &StatsFilter,
    group_by: GroupBy,
    sort: SortMetric,
    limit: i64,
    cursor: Option<&BreakdownCursor>,
) -> Result<BreakdownPage, String> {
    // The grouped aggregation is bounded by `limit`; an extra guard scan would
    // double the cost without changing the answer.
    let _budget = Budget::install(conn);
    breakdown_inner(conn, filter, group_by, sort, limit, cursor)
}

fn breakdown_inner(
    conn: &Connection,
    filter: &StatsFilter,
    group_by: GroupBy,
    sort: SortMetric,
    limit: i64,
    cursor: Option<&BreakdownCursor>,
) -> Result<BreakdownPage, String> {
    let key = match group_by {
        GroupBy::Provider => "provider_id",
        GroupBy::Model => "upstream_model",
    };
    let sql = FilterSql::build(filter, "");
    let transaction = conn.unchecked_transaction().map_err(budget_error)?;
    let as_of_id = match cursor {
        Some(cursor) => cursor.as_of_id,
        None => transaction
            .query_row(
                "SELECT COALESCE(MAX(id),0) FROM usage_requests",
                [],
                |row| row.get(0),
            )
            .map_err(budget_error)?,
    };
    let total_groups = match cursor {
        Some(_) => -1,
        None => {
            // Bounded group count: the total is reported as `MAX_GROUPS + 1`
            // when a range really has more groups than one page can show.
            let count_sql = format!(
                "SELECT COUNT(*) FROM (SELECT 1 FROM usage_requests WHERE {} AND id<=? GROUP BY {key} LIMIT {})",
                sql.clause,
                MAX_GROUPS + 1
            );
            let mut params = sql.params.clone();
            params.push(Value::Integer(as_of_id));
            transaction
                .query_row(&count_sql, params_from_iter(params.iter()), |row| {
                    row.get(0)
                })
                .map_err(budget_error)?
        }
    };
    let clause = format!("{} AND r.id<=?", sql.clause);
    let mut params = sql.params.clone();
    params.push(Value::Integer(as_of_id));
    let metric = match sort {
        SortMetric::TotalTokens => {
            "SUM(CASE WHEN (r.usage_status='complete' AND r.outcome<>'in_flight') THEN r.total_tokens END)"
        }
        SortMetric::RequestCount => "COUNT(*)",
    };
    let label = match group_by {
        GroupBy::Provider => {
            "(SELECT r2.provider_name FROM usage_requests r2 WHERE r2.provider_id=r.provider_id
               ORDER BY r2.started_at_ms DESC, r2.id DESC LIMIT 1)"
        }
        GroupBy::Model => "r.upstream_model",
    };
    // The keyset runs on the aggregated rows. Putting `SUM(...)`/`COUNT(*)` in
    // the inner WHERE raised "misuse of aggregate"; the CTE makes the metric an
    // ordinary column that can be compared.
    let mut outer = String::new();
    if let Some(cursor) = cursor {
        match cursor.last_metric {
            Some(last_metric) => {
                // Ordering is `metric DESC, key ASC` with NULL metrics last, so
                // the next page is "smaller metric, or the same metric with a
                // larger key, or the whole NULL tail".
                outer.push_str(
                    "WHERE (metric_value<? OR (metric_value=? AND group_key>?) OR metric_value IS NULL)",
                );
                params.push(Value::Integer(last_metric));
                params.push(Value::Integer(last_metric));
                params.push(Value::Text(cursor.last_group_key.clone()));
            }
            None => {
                // Already in the NULL tail.
                outer.push_str("WHERE metric_value IS NULL AND group_key>?");
                params.push(Value::Text(cursor.last_group_key.clone()));
            }
        }
    }
    let query = format!(
        "WITH grouped AS (
            SELECT r.{key} AS group_key, {label} AS group_label,
                COUNT(DISTINCT r.provider_id) AS provider_count,
                {metric} AS metric_value,
                {SUMMARY_COLUMNS}
            FROM usage_requests r WHERE {clause}
            GROUP BY r.{key}
         )
         SELECT * FROM grouped {outer}
         ORDER BY (metric_value IS NULL) ASC, metric_value DESC, group_key ASC
         LIMIT ?"
    );
    params.push(Value::Integer(limit + 1));
    let mut statement = transaction.prepare(&query).map_err(budget_error)?;
    let mut rows = statement
        .query(params_from_iter(params.iter()))
        .map_err(budget_error)?;
    let mut items = Vec::new();
    let mut metrics = Vec::new();
    while let Some(row) = rows.next().map_err(budget_error)? {
        let group_key: String = row.get(0).map_err(budget_error)?;
        let label: Option<String> = row.get(1).map_err(budget_error)?;
        let provider_count: i64 = row
            .get::<_, Option<i64>>(2)
            .map_err(budget_error)?
            .unwrap_or(0);
        let metric_value: Option<i64> = row.get(3).map_err(budget_error)?;
        let raw = read_summary_at(row, 4).map_err(budget_error)?;
        items.push(BreakdownItemDto {
            key: group_key.clone(),
            label: label.unwrap_or_else(|| group_key.clone()),
            provider_count,
            summary: to_summary(&raw, Vec::new(), 0, 0, 0, 0),
        });
        metrics.push(metric_value);
    }
    drop(rows);
    drop(statement);
    let has_more = items.len() as i64 > limit;
    if has_more {
        items.truncate(limit as usize);
        metrics.truncate(limit as usize);
    }
    // Money is accumulated per group with 128-bit addition, in one pass.
    let grouped = scan_costs_grouped(&transaction, filter, group_by, as_of_id)?;
    for item in items.iter_mut() {
        match grouped.get(&item.key) {
            Some(group) => {
                item.summary.costs = group.totals.clone().into_dto();
                item.summary.unpriced_request_count = group.unpriced;
                item.summary.default_zero_request_count = group.default_zero;
                item.summary.settled_cost_request_count = group.settled;
                item.summary.cost_incomplete_request_count = group.incomplete;
            }
            None => {
                // No priced rows anywhere: derive the split from the summary.
                let incomplete = item.summary.usage_partial_count
                    + item.summary.usage_missing_count
                    + item.summary.usage_invalid_count;
                item.summary.unpriced_request_count = (item.summary.finished_count
                    - item.summary.usage_not_applicable_count
                    - incomplete)
                    .max(0);
                item.summary.cost_incomplete_request_count = incomplete;
                item.summary.default_zero_request_count = 0;
                item.summary.settled_cost_request_count = 0;
            }
        }
    }
    let next_cursor = if has_more {
        items.last().map(|last| BreakdownCursor {
            group_by,
            sort,
            last_group_key: last.key.clone(),
            last_metric: metrics.last().copied().flatten(),
            as_of_id,
            filter_key: filter.key(),
            run_id: String::new(),
            maintenance_revision: 0,
        })
    } else {
        None
    };
    transaction.commit().map_err(budget_error)?;
    Ok(BreakdownPage {
        as_of_id,
        total_groups,
        items,
        next_cursor,
    })
}

fn read_summary_at(row: &rusqlite::Row<'_>, offset: usize) -> rusqlite::Result<RawSummary> {
    read_summary(row, offset)
}

pub struct FilterOptions {
    pub items: Vec<FilterOptionDto>,
    pub next_key: Option<String>,
}

pub fn filter_options(
    conn: &Connection,
    kind: FilterKind,
    query: &str,
    provider_id: Option<&str>,
    limit: i64,
    after_key: Option<&str>,
    configured: &[(String, String)],
) -> Result<FilterOptions, String> {
    let _budget = Budget::install(conn);
    filter_options_inner(conn, kind, query, provider_id, limit, after_key, configured)
}

fn filter_options_inner(
    conn: &Connection,
    kind: FilterKind,
    query: &str,
    provider_id: Option<&str>,
    limit: i64,
    after_key: Option<&str>,
    configured: &[(String, String)],
) -> Result<FilterOptions, String> {
    let column = match kind {
        FilterKind::Provider => "provider_id",
        FilterKind::Model => "upstream_model",
    };
    let mut clause = String::from("1=1");
    let mut params: Vec<Value> = Vec::new();
    if let Some(provider_id) = provider_id {
        clause.push_str(" AND provider_id=?");
        params.push(Value::Text(provider_id.to_owned()));
    }
    if !query.trim().is_empty() {
        // `%` and `_` are escaped so the search stays literal.
        clause.push_str(&format!(" AND {column} LIKE ? ESCAPE '\\'"));
        let escaped = query
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        params.push(Value::Text(format!("%{escaped}%")));
    }
    if let Some(after_key) = after_key {
        clause.push_str(&format!(" AND {column}>?"));
        params.push(Value::Text(after_key.to_owned()));
    }
    let label = match kind {
        FilterKind::Provider => {
            "(SELECT r2.provider_name FROM usage_requests r2 WHERE r2.provider_id=r.provider_id
               ORDER BY r2.started_at_ms DESC, r2.id DESC LIMIT 1)"
        }
        FilterKind::Model => "r.upstream_model",
    };
    let sql = format!(
        "SELECT r.{column}, {label}, MAX(r.started_at_ms) FROM usage_requests r
         WHERE {clause} GROUP BY r.{column} ORDER BY r.{column} ASC LIMIT ?"
    );
    params.push(Value::Integer(limit + 1));
    let mut statement = conn.prepare(&sql).map_err(budget_error)?;
    let mut rows = statement
        .query(params_from_iter(params.iter()))
        .map_err(budget_error)?;
    let mut items = Vec::new();
    while let Some(row) = rows.next().map_err(budget_error)? {
        let key: String = row.get(0).map_err(budget_error)?;
        let label: Option<String> = row.get(1).map_err(budget_error)?;
        items.push(FilterOptionDto {
            key,
            label: label.unwrap_or_default(),
            historical: true,
        });
    }
    drop(rows);
    drop(statement);
    let has_more = items.len() as i64 > limit;
    if has_more {
        items.truncate(limit as usize);
    }
    // Provider options also include the currently configured providers so a
    // provider with no traffic can still be selected.
    if let FilterKind::Provider = kind {
        for (id, name) in configured {
            if let Some(existing) = items.iter_mut().find(|item| &item.key == id) {
                existing.historical = false;
                if existing.label.is_empty() {
                    existing.label = name.clone();
                }
                continue;
            }
            let matches =
                query.trim().is_empty() || id.contains(query.trim()) || name.contains(query.trim());
            if matches && after_key.is_none_or(|after| id.as_str() > after) {
                items.push(FilterOptionDto {
                    key: id.clone(),
                    label: name.clone(),
                    historical: false,
                });
            }
        }
        items.sort_by(|a, b| a.key.cmp(&b.key));
        items.dedup_by(|a, b| a.key == b.key);
        if items.len() as i64 > limit {
            items.truncate(limit as usize);
        }
    }
    let next_key = has_more
        .then(|| items.last().map(|item| item.key.clone()))
        .flatten();
    Ok(FilterOptions { items, next_key })
}

#[derive(Clone, Copy, Debug)]
pub struct HealthFacts {
    pub record_count: i64,
    pub earliest_record_at_ms: Option<i64>,
}

/// Record count for cursor/`as_of_id` bookkeeping after maintenance.
pub fn max_request_id(conn: &Connection) -> Result<i64, String> {
    conn.query_row(
        "SELECT COALESCE(MAX(id),0) FROM usage_requests",
        [],
        |row| row.get(0),
    )
    .map_err(budget_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    fn filter(range_ms: i64) -> StatsFilter {
        StatsFilter {
            from_ms: 1_790_784_000_000,
            to_ms: 1_790_784_000_000 + range_ms,
            timezone: "Asia/Shanghai".into(),
            ..Default::default()
        }
    }

    #[test]
    fn hourly_buckets_cover_the_range_without_gaps() {
        let filter = filter(5 * 3_600_000 + 10);
        let (buckets, granularity) = build_buckets(&filter, Granularity::Auto).unwrap();
        assert_eq!(granularity, Granularity::Hour);
        assert_eq!(buckets.first().unwrap().start_ms, filter.from_ms);
        assert_eq!(buckets.last().unwrap().end_ms, filter.to_ms);
        for pair in buckets.windows(2) {
            assert_eq!(pair[0].end_ms, pair[1].start_ms);
        }
        assert_eq!(buckets.len(), 6);
    }

    #[test]
    fn daily_buckets_follow_local_calendar_boundaries() {
        let filter = filter(4 * 86_400_000);
        let (buckets, granularity) = build_buckets(&filter, Granularity::Auto).unwrap();
        assert_eq!(granularity, Granularity::Day);
        assert_eq!(buckets.first().unwrap().start_ms, filter.from_ms);
        assert_eq!(buckets.last().unwrap().end_ms, filter.to_ms);
        for pair in buckets.windows(2) {
            assert_eq!(pair[0].end_ms, pair[1].start_ms);
            assert!(pair[0].start_ms < pair[0].end_ms);
        }
    }

    #[test]
    fn dst_days_are_shorter_or_longer_but_still_strictly_increasing() {
        // America/New_York spring forward (2026-03-08) and fall back (2026-11-01).
        for tz in ["America/New_York", "Australia/Lord_Howe", "Asia/Kolkata"] {
            let start = chrono::Utc
                .with_ymd_and_hms(2026, 3, 1, 12, 0, 0)
                .unwrap()
                .timestamp_millis();
            let mut filter = StatsFilter {
                from_ms: start,
                to_ms: start + 60 * 86_400_000,
                timezone: tz.into(),
                ..Default::default()
            };
            filter.to_ms = start + 60 * 86_400_000;
            let (buckets, _) = build_buckets(&filter, Granularity::Day).unwrap();
            for pair in buckets.windows(2) {
                assert!(pair[0].start_ms < pair[0].end_ms, "tz={tz}");
                assert_eq!(pair[0].end_ms, pair[1].start_ms, "tz={tz}");
            }
            assert_eq!(buckets.last().unwrap().end_ms, filter.to_ms);
            if tz != "Asia/Kolkata" {
                // DST days are not 24 hours long; Lord Howe shifts by 30 minutes.
                let unusual = buckets.iter().any(|bucket| {
                    let length = bucket.end_ms - bucket.start_ms;
                    length != 24 * 3_600_000 && length > 20 * 3_600_000
                });
                assert!(unusual, "tz={tz} should contain a DST transition day");
            }
        }
    }

    #[test]
    fn weekly_buckets_start_on_monday() {
        let start = chrono::Utc
            .with_ymd_and_hms(2026, 3, 4, 0, 0, 0)
            .unwrap()
            .timestamp_millis();
        let filter = StatsFilter {
            from_ms: start,
            to_ms: start + 30 * 86_400_000,
            timezone: "UTC".into(),
            ..Default::default()
        };
        let (buckets, _) = build_buckets(&filter, Granularity::Week).unwrap();
        for bucket in buckets.iter().skip(1) {
            let date = chrono::Utc
                .timestamp_millis_opt(bucket.start_ms)
                .unwrap()
                .date_naive();
            assert_eq!(date.weekday().num_days_from_monday(), 0, "{date}");
            assert_eq!(
                chrono::Utc
                    .timestamp_millis_opt(bucket.start_ms)
                    .unwrap()
                    .time()
                    .hour(),
                0
            );
        }
    }

    #[test]
    fn too_many_buckets_is_rejected_instead_of_sampled() {
        let filter = filter(401 * 3_600_000);
        assert!(build_buckets(&filter, Granularity::Hour).is_err());
        let filter = StatsFilter {
            from_ms: 0,
            to_ms: 500 * 86_400_000,
            timezone: "UTC".into(),
            ..Default::default()
        };
        assert!(build_buckets(&filter, Granularity::Day).is_err());
        let filter = StatsFilter {
            from_ms: 0,
            to_ms: 366 * 86_400_000,
            timezone: "UTC".into(),
            ..Default::default()
        };
        let (buckets, _) = build_buckets(&filter, Granularity::Week).unwrap();
        assert!(buckets.len() <= MAX_BUCKETS);
    }

    #[test]
    fn timezone_resolution_accepts_iana_and_rejects_expressions() {
        assert!(resolve_timezone("Asia/Shanghai").is_ok());
        assert!(resolve_timezone("UTC").is_ok());
        assert!(resolve_timezone("").is_ok());
        assert!(resolve_timezone("Mars/Olympus").is_err());
        assert!(resolve_timezone("UTC'; DROP TABLE usage_requests;--").is_err());
    }

    #[test]
    fn granularity_auto_scales_with_range() {
        assert_eq!(
            choose_granularity(3_600_000, Granularity::Auto),
            Granularity::Hour
        );
        assert_eq!(
            choose_granularity(30 * 86_400_000, Granularity::Auto),
            Granularity::Day
        );
        assert_eq!(
            choose_granularity(300 * 86_400_000, Granularity::Auto),
            Granularity::Week
        );
    }
}

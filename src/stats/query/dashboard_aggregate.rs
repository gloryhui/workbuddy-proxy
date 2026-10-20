//! One bounded result per bucket feeds both overview and series. Imported
//! records use the schema's recorded/unknown-transport invariant; proxy rows
//! retain the complete original summary expression. No result cache is used.
use super::*;

const IMPORT_COLUMNS: &str = "
 COUNT(*), 0, 0, 0, 0, 0, 0, 0, COUNT(*) FILTER(WHERE usage_status<>'not_applicable'),
 COUNT(*) FILTER(WHERE usage_status='complete'),
 COUNT(*) FILTER(WHERE usage_status='partial'),
 COUNT(*) FILTER(WHERE usage_status='missing'),
 COUNT(*) FILTER(WHERE usage_status='invalid'),
 COUNT(*) FILTER(WHERE usage_status='not_applicable'),
 SUM(input_tokens) FILTER(WHERE usage_status='complete'),
 SUM(output_tokens) FILTER(WHERE usage_status='complete'),
 SUM(total_tokens) FILTER(WHERE usage_status='complete'),
 SUM(cache_read_tokens) FILTER(WHERE usage_status='complete'),
 COUNT(cache_read_tokens) FILTER(WHERE usage_status='complete'),
 SUM(input_tokens) FILTER(WHERE usage_status='complete' AND cache_read_tokens IS NOT NULL),
 SUM(cache_write_tokens) FILTER(WHERE usage_status='complete'),
 COUNT(cache_write_tokens) FILTER(WHERE usage_status='complete'),
 SUM(input_uncached_tokens) FILTER(WHERE input_breakdown='disjoint'),
 COUNT(*) FILTER(WHERE input_breakdown='disjoint'),
 SUM(reasoning_tokens) FILTER(WHERE usage_status='complete'),
 COUNT(reasoning_tokens) FILTER(WHERE usage_status='complete'), NULL, 0, 0,
 COUNT(workbuddy_credit_nanos),
 COALESCE(SUM(workbuddy_credit_nanos / 1000000000),0),
 COALESCE(SUM(workbuddy_credit_nanos % 1000000000),0)";

#[derive(Default)]
struct Accumulator {
    raw: RawSummary,
    origin: OriginCounts,
    duration_weight: f64,
    duration_count: i64,
}

fn add(left: i64, right: i64) -> Result<i64, String> {
    left.checked_add(right)
        .ok_or_else(|| error(ERR_UNAVAILABLE, "statistics total exceeds supported range"))
}

fn nullable_add(left: Option<i64>, right: Option<i64>) -> Result<Option<i64>, String> {
    match (left, right) {
        (Some(left), Some(right)) => add(left, right).map(Some),
        (left, right) => Ok(left.or(right)),
    }
}

impl Accumulator {
    fn merge_raw(&mut self, row: &RawSummary, duration_count: i64) -> Result<(), String> {
        if add(self.raw.request_count, row.request_count)? > ROW_BUDGET {
            return Err(error(
                ERR_QUERY_TOO_LARGE,
                "more than 1,000,000 rows match; narrow the range",
            ));
        }
        macro_rules! counts {
            ($($field:ident),+ $(,)?) => { $(self.raw.$field = add(self.raw.$field, row.$field)?;)+ };
        }
        counts!(
            request_count,
            finished_count,
            in_flight_count,
            completed_count,
            failed_count,
            rejected_count,
            cancelled_count,
            interrupted_count,
            usage_eligible_count,
            usage_complete_count,
            usage_partial_count,
            usage_missing_count,
            usage_invalid_count,
            usage_not_applicable_count,
            cache_read_known_count,
            cache_write_known_count,
            input_breakdown_known_count,
            reasoning_known_count,
            proxy_cost_incomplete_count,
            proxy_token_not_applicable_count
        );
        macro_rules! nullable {
            ($($field:ident),+ $(,)?) => { $(self.raw.$field = nullable_add(self.raw.$field, row.$field)?;)+ };
        }
        nullable!(
            input_tokens,
            output_tokens,
            total_tokens,
            cache_read_tokens,
            cache_read_denominator_input_tokens,
            cache_write_tokens,
            input_uncached_tokens,
            reasoning_tokens
        );
        self.duration_weight += row.avg_duration_ms.unwrap_or(0.0) * duration_count as f64;
        self.duration_count = add(self.duration_count, duration_count)?;
        self.raw.avg_duration_ms =
            (self.duration_count > 0).then(|| self.duration_weight / self.duration_count as f64);
        Ok(())
    }

    fn merge_origin(&mut self, origin: &OriginCounts) -> Result<(), String> {
        self.origin.proxy = add(self.origin.proxy, origin.proxy)?;
        self.origin.imported = add(self.origin.imported, origin.imported)?;
        self.origin.imported_unpriced =
            add(self.origin.imported_unpriced, origin.imported_unpriced)?;
        self.origin.credit_count = add(self.origin.credit_count, origin.credit_count)?;
        self.origin.credit_nanos = self
            .origin
            .credit_nanos
            .checked_add(origin.credit_nanos)
            .ok_or_else(|| error(ERR_UNAVAILABLE, "credit total exceeds supported range"))?;
        Ok(())
    }
}

fn proxy_bucket(conn: &Connection, filter: &StatsFilter) -> Result<(RawSummary, i64), String> {
    if !filter.source_scope.includes_proxy()
        || filter.capture_method == crate::stats::dto::CaptureMethod::Workbuddy
    {
        return Ok((RawSummary::default(), 0));
    }
    let sql = FilterSql::build(filter, "");
    let query = format!(
        "SELECT {SUMMARY_COLUMNS},
        COUNT(duration_ms) FILTER(WHERE outcome='completed') FROM usage_requests
        WHERE {} AND record_origin='proxy'",
        sql.clause
    );
    conn.prepare_cached(&query)
        .map_err(budget_error)?
        .query_row(params_from_iter(sql.params.iter()), |row| {
            Ok((read_summary(row, 0)?, row.get(29)?))
        })
        .map_err(budget_error)
}

fn imported_bucket(
    conn: &Connection,
    filter: &StatsFilter,
) -> Result<(RawSummary, OriginCounts), String> {
    if !filter.source_scope.includes_imported()
        || filter.capture_method == crate::stats::dto::CaptureMethod::Proxy
        || filter
            .outcome
            .is_some_and(|outcome| outcome != Outcome::Recorded)
    {
        return Ok((RawSummary::default(), OriginCounts::default()));
    }
    let (query, params) = imported_query(filter);
    conn.prepare_cached(&query)
        .map_err(budget_error)?
        .query_row(params_from_iter(params.iter()), |row| {
            let raw = read_summary(row, 0)?;
            let origin = OriginCounts {
                imported: raw.request_count,
                // Schema v7 prohibits both inserting and updating costs for imports.
                imported_unpriced: raw.request_count,
                credit_count: row.get(29)?,
                credit_nanos: i128::from(row.get::<_, i64>(30)?) * 1_000_000_000
                    + i128::from(row.get::<_, i64>(31)?),
                ..OriginCounts::default()
            };
            Ok((raw, origin))
        })
        .map_err(budget_error)
}

fn imported_query(filter: &StatsFilter) -> (String, Vec<Value>) {
    let sql = FilterSql::build(filter, "");
    // The origin/time index looks selective, but every matching aggregate row
    // then requires the wide fact table. Cold reads during backfill can exceed
    // the query deadline. All predicates/aggregates below are covered when no
    // provider/model/route filter is set; use the existing time-covering index.
    // Selective identity filters retain the planner's original index choices.
    let table = if filter.provider_id.is_none()
        && filter.record_kind == crate::stats::dto::RecordKind::All
        && filter.upstream_model.is_none()
        && filter.route_id.is_none()
    {
        "usage_requests INDEXED BY usage_requests_dashboard_cover"
    } else {
        "usage_requests"
    };
    (
        format!(
            "SELECT {IMPORT_COLUMNS} FROM {table}
            WHERE {} AND record_origin<>'proxy'",
            sql.clause
        ),
        sql.params,
    )
}

/// Reads each bucket once per origin family, with at most MAX_BUCKETS summaries
/// in memory. The existing caller owns the snapshot and two-second deadline.
pub(super) fn aggregate(
    conn: &Connection,
    filter: &StatsFilter,
    buckets: &[Bucket],
) -> Result<(RawSummary, OriginCounts, Vec<RawSummary>), String> {
    let mut total = Accumulator::default();
    let mut rows = Vec::with_capacity(buckets.len());
    for bucket in buckets {
        let mut fields = filter.clone();
        fields.from_ms = bucket.start_ms;
        fields.to_ms = bucket.end_ms;
        let (proxy, durations) = proxy_bucket(conn, &fields)?;
        let (imported, mut origin) = imported_bucket(conn, &fields)?;
        origin.proxy = proxy.request_count;
        let mut combined = Accumulator::default();
        combined.merge_raw(&proxy, durations)?;
        combined.merge_raw(&imported, 0)?;
        total.merge_raw(&combined.raw, durations)?;
        total.merge_origin(&origin)?;
        rows.push(combined.raw);
    }
    total.origin.pending = pending_import_count(conn, filter)?;
    Ok((total.raw, total.origin, rows))
}

fn pending_import_count(conn: &Connection, filter: &StatsFilter) -> Result<i64, String> {
    if filter.source_scope != SourceScope::All
        || filter.record_kind != crate::stats::dto::RecordKind::All
        || filter.capture_method == crate::stats::dto::CaptureMethod::Proxy
        || filter.provider_id.is_some()
        || filter.upstream_model.is_some()
        || filter.route_id.is_some()
        || filter.outcome.is_some()
        || filter.usage_status.is_some()
    {
        return Ok(0);
    }
    conn.query_row(
        "SELECT COUNT(*) FROM workbuddy_import_ledger
        WHERE state='pending_classification' AND event_at_ms>=?1 AND event_at_ms<?2",
        rusqlite::params![filter.from_ms, filter.to_ms],
        |row| row.get(0),
    )
    .map_err(budget_error)
}

fn quality(raw: &RawSummary, covered: bool) -> (SeriesQuality, bool) {
    let warning = !covered;
    if raw.request_count == 0 {
        (
            if covered {
                SeriesQuality::Empty
            } else {
                SeriesQuality::Unknown
            },
            warning,
        )
    } else if raw.usage_complete_count >= raw.usage_eligible_count && raw.usage_eligible_count > 0 {
        (SeriesQuality::Known, warning)
    } else if raw.usage_complete_count > 0 || raw.usage_partial_count > 0 {
        (SeriesQuality::Partial, warning)
    } else {
        (SeriesQuality::Unknown, warning)
    }
}

fn point(
    bucket: &Bucket,
    raw: &RawSummary,
    covered: bool,
    costs: Vec<CostTotalDto>,
) -> SeriesPointDto {
    let (quality, collection_warning) = quality(raw, covered);
    let proven_zero = raw.request_count == 0 && covered;
    let usage = raw.usage_complete_count > 0;
    let tokens = |value: Option<i64>| {
        if proven_zero {
            Some(0)
        } else if usage {
            value
        } else {
            None
        }
    };
    SeriesPointDto {
        start_ms: bucket.start_ms,
        end_ms: bucket.end_ms,
        request_count: raw.request_count,
        finished_count: raw.finished_count,
        usage_complete_count: raw.usage_complete_count,
        input_tokens: tokens(raw.input_tokens),
        output_tokens: tokens(raw.output_tokens),
        total_tokens: tokens(raw.total_tokens),
        cache_read_tokens: raw.cache_read_tokens,
        cache_write_tokens: raw.cache_write_tokens,
        cache_read_known_count: raw.cache_read_known_count,
        cache_write_known_count: raw.cache_write_known_count,
        cache_read_denominator_input_tokens: raw.cache_read_denominator_input_tokens,
        costs,
        quality,
        collection_warning,
    }
}

/// Reuses the aggregate's identical snapshot instead of scanning source facts
/// again. Empty/partial/unknown semantics still use collection coverage.
pub(super) fn series(
    conn: &Connection,
    filter: &StatsFilter,
    buckets: &[Bucket],
    rows: &[RawSummary],
    costs: &mut [CostAccumulator],
) -> Result<Vec<SeriesPointDto>, String> {
    let coverage = collection_coverage(conn, filter.from_ms, filter.to_ms)?;
    Ok(buckets
        .iter()
        .zip(rows)
        .zip(costs)
        .map(|((bucket, raw), cost)| {
            point(
                bucket,
                raw,
                coverage.covers(bucket.start_ms, bucket.end_ms),
                std::mem::take(cost).into_dto(),
            )
        })
        .collect())
}

#[cfg(test)]
mod tests;

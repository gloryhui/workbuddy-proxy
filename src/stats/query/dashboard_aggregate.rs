//! dashboard 的桶级共享聚合。
//!
//! `aggregate` 在同一个 SQLite 读事务中逐桶查询代理与导入记录；每个桶只保留
//! 一份有界 `RawSummary`。总览累加这些同一批摘要，`series` 再直接使用它们，
//! 所以两条展示路径不各扫一遍原始请求。这里没有跨请求缓存。
//!
//! 代理记录保留原始 outcome/transport/usage 摘要；导入记录采用 schema 固定的
//! `recorded` 与未知传输语义。导入行的信用额单独累计，不与供应商货币费用相加。
use super::*;

/// 导入来源的桶内聚合列，列位置必须与 `read_summary` 对齐。
///
/// 导入记录没有代理请求特有的 completed/failed/rejected/cancelled/interrupted
/// 计数，所以这些位置填 0；请求总数、usage 状态、token 和 WorkBuddy credit
/// 仍按真实行聚合。credit 的商和余数分两列求 SUM，随后在 Rust 用 `i128`
/// 重组，避免 SQLite i64 累加溢出。
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

/// 跨桶累计总览时使用的可变累加器。
///
/// `raw` 保存业务摘要，`origin` 保存代理/导入/信用等来源拆分。耗时不能直接平
/// 均各桶 `AVG`：`duration_weight` 先还原每个桶的“均值 × 样本数”总和，最后
/// 再除以真实样本数。
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
    // None 表示该桶没有可信值；Some(0) 表示可信的零。两者不能互换。
    // 只有一侧有时保留该侧；两侧都有的值才做 checked i64 加法。
    match (left, right) {
        (Some(left), Some(right)) => add(left, right).map(Some),
        (left, right) => Ok(left.or(right)),
    }
}

impl Accumulator {
    fn merge_raw(&mut self, row: &RawSummary, duration_count: i64) -> Result<(), String> {
        // 这是 ROW_BUDGET 当前真正生效的位置：逐桶摘要累计后超过上限即返回
        // stats_query_too_large。先检查请求数，再合并其余计数、None-aware 指标
        // 和耗时；所有 checked_add 失败都映射为稳定的统计错误。来源拆分由
        // merge_origin 另行合并。
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
    // 只聚合 record_origin='proxy'，并额外返回 completed 且 duration_ms 非空的
    // 样本数，供跨桶加权平均。来源范围或 capture 条件排除代理时直接返回空摘要。
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
    // 导入事实的 outcome 固定按 recorded 解释；筛选了其他 outcome 时无需查库。
    // 返回值中的 imported_unpriced 来自来源计数本身，而不是代理费用扫描。
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
    // 顺序遍历每个 bucket：代理和导入各查一次，再把两份摘要合成该桶结果。
    // `rows` 只保存最多 MAX_BUCKETS 份聚合摘要；总览使用同一个 combined.raw
    // 累加，所以 overview 与 series 来自完全相同的桶边界和数据快照。
    //
    // pending import 数最后单独读取，只进入来源告警字段，不混入请求数、token
    // 或成本；它表达尚未完成分类的导入候选。
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
    // 质量与采集覆盖是两个维度：完整覆盖的空桶是 Empty；未覆盖的空桶是
    // Unknown。有请求时，只有 eligible usage 全部 complete 才是 Known；
    // 有 complete/partial 但未全量则为 Partial；其余没有可信 usage 时为
    // Unknown。collection_warning 直接由是否完整覆盖产生。
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
    // 只有“请求数确实为 0 且整桶已覆盖采集”才输出 token 0。已经有请求但
    // usage 不 complete 时保留 None，让图表显示缺口。费用由同桶费用累加器
    // 消耗后转成 DTO；未知或零金额由 CostTotalDto 的币种条目另行表达。
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
    // 本函数不重扫 usage_requests：`rows` 是 aggregate 已算好的同序桶摘要，
    // `costs` 是 scan_costs 填充的同序桶费用。只额外读取 collection coverage，
    // 然后按桶 zip 生成最终点。`std::mem::take` 把金额所有权移入 DTO，避免
    // 再复制一份累加器。
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

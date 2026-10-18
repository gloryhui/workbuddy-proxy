//! Compare optimized aggregates with the original independent full SQL and
//! series implementation, using real v7 constraints rather than mock tables.
use super::*;
use crate::stats::dto::CaptureMethod;
use rusqlite::params;

fn database() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    crate::stats::store::migrate(&mut conn, std::path::Path::new(".")).unwrap();
    conn.execute(
        "INSERT INTO stats_runs(run_id,started_at_ms) VALUES('test',0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO stats_collection_intervals(run_id,started_at_ms,ended_at_ms,state)
        VALUES('test',0,3600000,'collecting'),('test',7200000,10800000,'collecting')",
        [],
    )
    .unwrap();
    for index in 1..=48 {
        insert(&conn, index, "proxy");
        insert(&conn, 100 + index, "workbuddy_builtin");
        insert(&conn, 200 + index, "workbuddy_custom");
    }
    conn
}

fn insert(conn: &Connection, index: i64, origin: &str) {
    let proxy = origin == "proxy";
    let at = (index % 3) * 3_600_000 + 1000;
    let status = ["complete", "partial", "missing", "invalid"][(index % 4) as usize];
    let outcome = if proxy {
        [
            "completed",
            "failed",
            "cancelled",
            "interrupted",
            "rejected",
            "in_flight",
        ][(index % 6) as usize]
    } else {
        "recorded"
    };
    let duration = (proxy && outcome != "in_flight" && index % 3 != 0).then_some(index * 7);
    let known = matches!(status, "complete" | "partial");
    let cache = (known && index % 3 != 0).then_some(if index % 3 == 1 { 0 } else { 7 });
    conn.execute(
        "INSERT INTO usage_requests(request_id,run_id,source,provider_id,route_id,
        provider_name,route_name,requested_model,upstream_model,started_at_ms,ended_at_ms,
        duration_ms,stream,upstream_started,outcome,usage_status,usage_source,input_tokens,
        output_tokens,total_tokens,cache_read_tokens,cache_write_tokens,reasoning_tokens,
        input_breakdown,upstream_protocol,record_origin,time_basis,workbuddy_credit_nanos,
        created_at_ms,updated_at_ms)
        VALUES(?1,?2,?3,?4,?5,'Provider','Route','model',?6,?7,?8,?9,?10,?11,?12,?13,?14,
        ?15,?16,?17,?18,?19,?20,'unknown',?21,?22,?23,?24,0,0)",
        params![
            format!("row-{index}"),
            proxy.then_some("test"),
            if !proxy {
                "workbuddy_import"
            } else if index % 5 == 0 {
                "connection_test"
            } else {
                "relay"
            },
            if index % 2 == 0 { "p1" } else { "p2" },
            if index % 3 == 0 { "r1" } else { "r2" },
            if index % 2 == 0 { "m1" } else { "m2" },
            at,
            (outcome != "in_flight").then_some(at + duration.unwrap_or(0)),
            duration,
            proxy.then_some(0),
            proxy.then_some(1),
            outcome,
            status,
            if proxy { "upstream" } else { "workbuddy_jsonl" },
            known.then_some(100),
            known.then_some(20),
            known.then_some(120),
            cache,
            (known && index % 4 == 0).then_some(0),
            (known && index % 2 == 0).then_some(2),
            proxy.then_some("openai_chat"),
            origin,
            if proxy {
                "request_start"
            } else {
                "message_timestamp"
            },
            (!proxy && index % 2 == 0).then_some(1_000_000_000_000_000_000_i64),
        ],
    )
    .unwrap();
}

fn filter(scope: SourceScope, capture: CaptureMethod, tests: bool) -> StatsFilter {
    StatsFilter {
        from_ms: 0,
        to_ms: 14_400_000,
        timezone: "UTC".into(),
        source_scope: scope,
        capture_method: capture,
        include_tests: tests,
        ..StatsFilter::default()
    }
}

fn compare(conn: &Connection, fields: &StatsFilter) {
    let (buckets, _) = build_buckets(fields, Granularity::Hour).unwrap();
    let sql = FilterSql::build(fields, "");
    let reference = conn
        .query_row(
            &format!(
                "SELECT {SUMMARY_COLUMNS} FROM usage_requests WHERE {}",
                sql.clause
            ),
            params_from_iter(sql.params.iter()),
            |row| read_summary(row, 0),
        )
        .unwrap();
    let reference_origin = origin_counts(conn, fields).unwrap();
    let (actual, origin, rows) = aggregate(conn, fields, &buckets).unwrap();
    let convert = |raw: &RawSummary, counts: &OriginCounts| {
        serde_json::to_value(to_summary(
            raw,
            Vec::new(),
            counts.imported_unpriced,
            0,
            0,
            0,
            counts,
        ))
        .unwrap()
    };
    let mut expected = convert(&reference, &reference_origin);
    let mut result = convert(&actual, &origin);
    let avg_expected = expected
        .as_object_mut()
        .unwrap()
        .remove("avg_duration_ms")
        .unwrap();
    let avg_actual = result
        .as_object_mut()
        .unwrap()
        .remove("avg_duration_ms")
        .unwrap();
    assert_eq!(result, expected, "filter: {fields:?}");
    match (avg_expected.as_f64(), avg_actual.as_f64()) {
        (Some(left), Some(right)) => assert!((left - right).abs() <= 1e-9),
        (left, right) => assert_eq!(left, right),
    }
    let mut original_costs = vec![CostAccumulator::default(); buckets.len()];
    let original = series_inner(conn, fields, &buckets, &mut original_costs).unwrap();
    let mut costs = vec![CostAccumulator::default(); buckets.len()];
    assert_eq!(
        series(conn, fields, &buckets, &rows, &mut costs).unwrap(),
        original
    );
}

#[test]
fn all_source_capture_filters_match_original_summary_and_series_with_real_constraints() {
    let conn = database();
    for scope in [
        SourceScope::All,
        SourceScope::Builtin,
        SourceScope::Supplier,
    ] {
        for capture in [
            CaptureMethod::All,
            CaptureMethod::Proxy,
            CaptureMethod::Workbuddy,
        ] {
            if scope == SourceScope::Builtin && capture == CaptureMethod::Proxy {
                continue;
            }
            for tests in [false, true] {
                let base = filter(scope, capture, tests);
                for variant in 0..10 {
                    let mut fields = base.clone();
                    match variant {
                        1 => fields.provider_id = Some("p1".into()),
                        2 => fields.route_id = Some("r2".into()),
                        3 => fields.upstream_model = Some("m1".into()),
                        4 => fields.outcome = Some(Outcome::Recorded),
                        5 => fields.outcome = Some(Outcome::Completed),
                        6 => fields.usage_status = Some(UsageStatus::Complete),
                        7 => fields.usage_status = Some(UsageStatus::Partial),
                        8 => fields.from_ms = 3_600_500,
                        9 => fields.from_ms = 12_000_000,
                        _ => {}
                    }
                    compare(&conn, &fields);
                }
            }
        }
    }
}

#[test]
fn exact_credit_above_i64_and_missing_cache_are_not_zero_or_currency() {
    let conn = database();
    let fields = filter(SourceScope::Builtin, CaptureMethod::All, false);
    let (buckets, _) = build_buckets(&fields, Granularity::Hour).unwrap();
    let (_, origin, _) = aggregate(&conn, &fields, &buckets).unwrap();
    assert_eq!(origin.credit_nanos, 24_000_000_000_000_000_000_i128);
    assert_eq!(origin.credit_count, 24);
    assert_eq!(origin.imported_unpriced, 48);
    assert!(
        conn.execute(
            "INSERT INTO usage_costs(request_id,price_source,currency,status,calculated_at_ms)
        VALUES('row-101','legacy_unknown','USD','pending',0)",
            []
        )
        .is_err()
    );
}

#[test]
fn cover_index_is_used_and_existing_v7_startup_adds_it_without_touching_facts() {
    let mut conn = database();
    let original: i64 = conn
        .query_row("SELECT COUNT(*) FROM usage_requests", [], |row| row.get(0))
        .unwrap();
    conn.execute("DROP INDEX usage_requests_dashboard_cover", [])
        .unwrap();
    crate::stats::store::migrate(&mut conn, std::path::Path::new(".")).unwrap();
    crate::stats::store::migrate(&mut conn, std::path::Path::new(".")).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        crate::stats::store::SCHEMA_VERSION
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM usage_requests", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        original
    );
    let sql = FilterSql::build(&filter(SourceScope::All, CaptureMethod::All, false), "");
    let plan: String = conn
        .query_row(
            &format!(
                "EXPLAIN QUERY PLAN SELECT {IMPORT_COLUMNS}
        FROM usage_requests WHERE {} AND record_origin<>'proxy'",
                sql.clause
            ),
            params_from_iter(sql.params.iter()),
            |row| row.get(3),
        )
        .unwrap();
    assert!(
        plan.contains("COVERING INDEX usage_requests_dashboard_cover"),
        "{plan}"
    );
}

#[test]
fn imported_dashboard_scopes_use_the_covering_index_without_fact_lookups() {
    let conn = database();
    for scope in [
        SourceScope::All,
        SourceScope::Builtin,
        SourceScope::Supplier,
    ] {
        for capture in [CaptureMethod::All, CaptureMethod::Workbuddy] {
            let fields = filter(scope, capture, false);
            let (query, values) = imported_query(&fields);
            let plan: String = conn
                .query_row(
                    &format!("EXPLAIN QUERY PLAN {query}"),
                    params_from_iter(values.iter()),
                    |row| row.get(3),
                )
                .unwrap();
            assert!(
                plan.contains("COVERING INDEX usage_requests_dashboard_cover"),
                "{scope:?}/{capture:?}: {plan}"
            );
            // Compare the complete production dashboard with the independent
            // original SQL, including tokens, credit, money and every bucket.
            compare(&conn, &fields);
        }
    }
}

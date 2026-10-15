#!/usr/bin/env python3
"""Validate design DDL and the SQL printed in DEVELOPMENT.md, using a temporary DB only."""

from pathlib import Path
import re
import sqlite3
import tempfile


ROOT = Path(__file__).resolve().parent


def rejects(db, sql, args=()):
    try:
        db.execute(sql, args)
    except sqlite3.IntegrityError:
        return
    raise AssertionError(f"Expected integrity rejection: {sql}")


def add_request(db, request_id, started, **fields):
    row = dict(
        request_id=request_id, run_id="run-a", source="relay", provider_id="p1",
        route_id="r1", provider_name="供应商A", route_name="日常模型",
        requested_model="alias/model-x", upstream_model="model-x",
        started_at_ms=started, ended_at_ms=started + 100, duration_ms=100,
        stream=1, upstream_started=1, http_status=200, outcome="completed",
        usage_status="complete", usage_source="upstream", input_tokens=1000,
        output_tokens=100, total_tokens=1100, cache_read_tokens=800,
        input_breakdown="read_subset", created_at_ms=started,
        updated_at_ms=started + 100,
    )
    row.update(fields)
    columns = ",".join(row)
    values = ",".join("?" for _ in row)
    db.execute(f"INSERT INTO usage_requests({columns}) VALUES({values})", tuple(row.values()))


def main():
    with tempfile.TemporaryDirectory(prefix="wbp-stats-design-") as folder:
        db = sqlite3.connect(str(Path(folder) / "usage.sqlite3"))
        db.row_factory = sqlite3.Row
        db.executescript((ROOT / "schema.sql").read_text())
        assert db.execute("PRAGMA user_version").fetchone()[0] == 2
        tables = {r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE type='table'")}
        assert len(tables) == 8, tables
        db.execute("INSERT INTO stats_runs(run_id,started_at_ms) VALUES('run-a',0)")
        db.execute("INSERT INTO stats_collection_intervals(run_id,started_at_ms,state) VALUES('run-a',0,'collecting')")
        rejects(db, "INSERT INTO stats_collection_intervals(run_id,started_at_ms,state) VALUES('run-a',1,'collecting')")

        add_request(db, "r1", 1000)
        unknown = dict(input_tokens=None, output_tokens=None, total_tokens=None,
                       cache_read_tokens=None, input_breakdown="unknown", usage_source="none")
        add_request(db, "r2", 2000, outcome="cancelled", usage_status="missing", **unknown)
        add_request(db, "r3", 3000, input_tokens=0, output_tokens=0, total_tokens=0, cache_read_tokens=0)
        add_request(db, "r4", 4000, source="connection_test", input_tokens=50,
                    output_tokens=5, total_tokens=55, cache_read_tokens=0)
        add_request(db, "r5", 5000, outcome="failed", error_kind="upstream_stream",
                    input_tokens=150, output_tokens=15, total_tokens=165,
                    cache_read_tokens=None, input_breakdown="unknown")
        add_request(db, "r6", 6000, outcome="rejected", http_status=503,
                    error_kind="missing_key", upstream_started=0,
                    usage_status="not_applicable", usage_reason="not_sent", **unknown)
        add_request(db, "r7", 7000, outcome="interrupted", usage_status="partial",
                    input_tokens=20, output_tokens=None, total_tokens=None,
                    cache_read_tokens=None, input_breakdown="unknown")
        add_request(db, "r8", 8000, outcome="in_flight", usage_status="missing",
                    ended_at_ms=None, duration_ms=None, **unknown)
        add_request(db, "r9", 9000, input_tokens=100, output_tokens=10,
                    total_tokens=110, cache_read_tokens=0)

        # SQL comes from the development document, not a second copied implementation.
        blocks = re.findall(r"```sql\n(.*?)\n```", (ROOT / "DEVELOPMENT.md").read_text(), re.S)
        summary_sql = next(s for s in blocks if s.startswith("SELECT COUNT(*) AS request_count"))
        page_sql = next(s for s in blocks if s.startswith("SELECT * FROM stats_request_facts"))
        bucket_sql = next(s for s in blocks if s.startswith("WITH buckets"))
        cleanup_sql = next(s for s in blocks if s.startswith("DELETE FROM usage_requests"))
        params = dict(from_ms=0, to_ms=10000, include_tests=0)
        summary = dict(db.execute(summary_sql, params).fetchone())
        expected = dict(request_count=8, finished_count=7, in_flight_count=1,
                        completed_count=3, failed_count=2, rejected_count=1,
                        cancelled_count=1, interrupted_count=1, usage_eligible_count=6,
                        usage_complete_count=4, input_tokens=1250, output_tokens=125,
                        total_tokens=1375, cache_read_tokens=800,
                        cache_read_known_count=3, cache_read_denominator_input_tokens=1100)
        for key, value in expected.items():
            assert summary[key] == value, (key, summary[key], value)
        assert summary["avg_duration_ms"] == 100
        with_tests = db.execute(summary_sql, dict(params, include_tests=1)).fetchone()
        assert with_tests["total_tokens"] == 1430
        missing = db.execute(summary_sql, dict(from_ms=2000, to_ms=3000, include_tests=0)).fetchone()
        assert missing["request_count"] == 1 and missing["total_tokens"] is None
        empty = db.execute(summary_sql, dict(from_ms=10000, to_ms=11000, include_tests=0)).fetchone()
        assert empty["request_count"] == 0 and empty["input_tokens"] is None

        page_params = dict(params, as_of_id=9, last_time=10000, last_id=100, limit_plus_one=3)
        first = db.execute(page_sql, page_params).fetchall()
        assert [r["request_id"] for r in first] == ["r9", "r8", "r7"]
        second = db.execute(page_sql, dict(page_params, last_time=8000, last_id=8)).fetchall()
        assert [r["request_id"] for r in second] == ["r7", "r6", "r5"]
        buckets = db.execute(bucket_sql, dict(b0=0, e0=5000, b1=10000, e1=11000, include_tests=0)).fetchall()
        assert buckets[0]["requests"] == 3 and buckets[0]["input_tokens"] == 1000
        assert buckets[1]["requests"] == 0 and buckets[1]["input_tokens"] is None

        try:
            add_request(db, "r1", 1001)
        except sqlite3.IntegrityError:
            pass
        else:
            raise AssertionError("Duplicate request_id accepted")
        for request_id, changes in (
            ("bad-total", dict(total_tokens=999)),
            ("negative", dict(input_tokens=-1)),
            ("bad-cache", dict(cache_read_tokens=1001)),
            ("bad-reasoning", dict(reasoning_tokens=101)),
            ("complete-null", dict(input_tokens=None)),
            ("bad-breakdown", dict(input_breakdown="disjoint", cache_write_tokens=100, input_uncached_tokens=200)),
        ):
            try:
                add_request(db, request_id, 10000, **changes)
            except sqlite3.IntegrityError:
                pass
            else:
                raise AssertionError(f"Invalid row accepted: {request_id}")

        # Current model prices are freely editable; per-request snapshots are independent.
        db.execute("""INSERT INTO stats_model_prices
          (price_id,provider_id,upstream_model,currency,input_mode,input_rate_nanos,
           output_rate_nanos,cache_read_rate_nanos,label,created_at_ms,updated_at_ms)
          VALUES('price-a','p1','model-x','USD','read_discount',2000000000,8000000000,
                 200000000,'fixture',0,0)""")
        rejects(db, """INSERT INTO stats_model_prices
          (price_id,provider_id,upstream_model,currency,input_mode,label,created_at_ms,updated_at_ms)
          VALUES('duplicate-key','p1','model-x','USD','flat','duplicate',0,0)""")

        def amount(tokens, rate):
            return (tokens * rate + 500000) // 1000000

        components = (amount(200, 2000000000), amount(100, 8000000000), amount(800, 200000000), 0)
        assert sum(components) == 1360000
        assert amount(1, 500000) == 1  # Exactly half a nano rounds up.
        cost_sql = """INSERT INTO usage_costs(request_id,source_price_id,source_price_revision,
            price_source,price_snapshot_at_ms,currency,input_mode,
            input_rate_nanos,output_rate_nanos,cache_read_rate_nanos,cache_write_rate_nanos,
            status,input_cost_nanos,output_cost_nanos,cache_read_cost_nanos,cache_write_cost_nanos,
            total_cost_nanos,calculated_at_ms)
            VALUES(?,'price-a',1,'manual',1000,'USD','read_discount',
                   2000000000,8000000000,200000000,0,'priced',?,?,?,?,?,0)"""
        rejects(db, cost_sql, ("r1", *components, None))
        rejects(db, cost_sql, ("r1", *components, 1))
        db.execute(cost_sql, ("r1", *components, sum(components)))
        original = tuple(db.execute("SELECT currency,input_rate_nanos,output_rate_nanos,total_cost_nanos FROM usage_costs WHERE request_id='r1'").fetchone())
        db.execute("UPDATE stats_model_prices SET input_rate_nanos=9000000000, currency='CNY', revision=revision+1,updated_at_ms=1 WHERE price_id='price-a'")
        assert tuple(db.execute("SELECT currency,input_rate_nanos,output_rate_nanos,total_cost_nanos FROM usage_costs WHERE request_id='r1'").fetchone()) == original
        db.execute("DELETE FROM stats_model_prices WHERE price_id='price-a'")
        assert tuple(db.execute("SELECT currency,input_rate_nanos,output_rate_nanos,total_cost_nanos FROM usage_costs WHERE request_id='r1'").fetchone()) == original

        zero_sql = """INSERT INTO usage_costs(request_id,price_source,price_snapshot_at_ms,
            currency,input_mode,input_rate_nanos,output_rate_nanos,cache_read_rate_nanos,
            cache_write_rate_nanos,status,input_cost_nanos,output_cost_nanos,cache_read_cost_nanos,
            cache_write_cost_nanos,total_cost_nanos,calculated_at_ms)
            VALUES(?,'default_zero',1000,'CNY','flat',0,0,0,0,'default_zero',0,0,0,0,0,0)"""
        db.execute(zero_sql, ('r9',))
        db.execute(zero_sql, ('r2',))  # Missing token usage does not prevent known zero price/fee.
        assert db.execute("SELECT input_tokens FROM usage_requests WHERE request_id='r2'").fetchone()[0] is None
        rejects(db, "UPDATE usage_costs SET input_rate_nanos=1 WHERE request_id='r9'")
        rejects(db, "UPDATE usage_costs SET input_cost_nanos=1,total_cost_nanos=1 WHERE request_id='r9'")
        db.execute("""INSERT INTO stats_model_prices
          (price_id,provider_id,upstream_model,currency,input_mode,input_rate_nanos,label,created_at_ms,updated_at_ms)
          VALUES('later-price','p1','model-x','CNY','flat',9000000000,'added later',0,0)""")
        assert db.execute("SELECT total_cost_nanos FROM usage_costs WHERE request_id='r9'").fetchone()[0] == 0
        assert tuple(db.execute("SELECT currency,input_rate_nanos,output_rate_nanos,total_cost_nanos FROM usage_costs WHERE request_id='r1'").fetchone()) == original
        assert db.execute("SELECT cache_write_rate_nanos FROM stats_model_prices WHERE price_id='later-price'").fetchone()[0] == 0

        # Idempotent terminal update: retry cannot touch the finished record.
        assert db.execute("UPDATE usage_requests SET outcome='failed' WHERE request_id='r1' AND outcome='in_flight'").rowcount == 0
        db.execute(cleanup_sql, dict(cutoff=10000, as_of_id=9))
        remaining = [r[0] for r in db.execute("SELECT request_id FROM usage_requests")]
        assert remaining == ["r8"], remaining
        assert db.execute("SELECT COUNT(*) FROM usage_costs").fetchone()[0] == 0
        assert not db.execute("PRAGMA foreign_key_check").fetchall()
        assert db.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
        db.close()
    print("PASS: 8 tables; constraints; documented summary/bucket/keyset/cleanup SQL; NULL vs zero; test filtering; mutable prices; immutable independent cost snapshots; missing-price zero; deduplication; FK integrity")


if __name__ == "__main__":
    main()

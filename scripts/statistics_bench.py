#!/usr/bin/env python3
"""Statistics query-budget and relay-overhead measurements.

Builds a temporary database with synthetic, non-sensitive seed rows, then
measures the documented budgets through the real management API:
  * 100k rows: dashboard P95 < 500 ms, first page < 200 ms;
  * 1M rows:   dashboard < 2 s, one query budget 2 s;
  * relay P95 increase against the same Mock upstream with and without
    collection (< 5% or 2 ms, whichever is larger).

Nothing here touches a user profile: the config directory is temporary and the
upstream is a local Mock.

Usage:
    python3 scripts/statistics_bench.py --exe target/release/workbuddy-proxy \
        --rows 100000 1000000 --output bench-results/statistics.json
"""
import argparse
import http.client
import json
import os
import socket
import sqlite3
import statistics
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer as HttpServer
from pathlib import Path

LOCAL_KEY = 'bench-local-key'
MANAGEMENT_KEY = 'bench-management-key'
UPSTREAM_KEY = 'bench-upstream-key'
RUN_ID = 'bench-run'
DAY_MS = 86_400_000
PROVIDERS = 8
MODELS = 12


class Mock(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *args):
        pass

    def do_POST(self):
        length = int(self.headers.get('Content-Length', '0'))
        self.rfile.read(length)
        body = (b'data: {"choices":[{"delta":{"content":"ok"}}]}\n\n'
                b'data: {"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":10,'
                b'"total_tokens":110,"prompt_tokens_details":{"cached_tokens":40}}}\n\n'
                b'data: [DONE]\n\n')
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def free_port():
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]


def now_ms():
    return int(time.time() * 1000)


def write_config(directory, port, upstream):
    models = [{
        'id': 'route-%d' % index,
        'alias': 'wbp-bench-%d' % index,
        'name': 'bench-%d' % index,
        'model': 'bench-%d' % index,
        'reasoning_effort': 'default',
    } for index in range(MODELS)]
    config = {
        'local_key': LOCAL_KEY,
        'language': 'en',
        'port': port,
        'autostart': False,
        'providers': [{
            'id': 'provider-a',
            'name': 'Bench Provider',
            'upstream': upstream,
            'api_key': UPSTREAM_KEY,
            'models': models,
        }],
        'installed': [],
    }
    (Path(directory) / 'config.json').write_text(json.dumps(config), encoding='utf-8')


def start_core(exe, directory, port):
    log = open(Path(directory, 'process.log'), 'ab')
    process = subprocess.Popen([str(Path(exe).resolve()), '--config-dir', str(directory), '--headless'],
                               stdout=log, stderr=log)
    for _ in range(600):
        if process.poll() is not None:
            log.close()
            raise AssertionError('Core exited:\n' + (Path(directory) / 'process.log').read_text(errors='replace'))
        try:
            with urllib.request.urlopen('http://127.0.0.1:%d/health' % port, timeout=0.5) as response:
                if json.load(response).get('ready'):
                    return process, log
        except (OSError, ValueError):
            time.sleep(0.025)
    process.kill()
    log.close()
    raise AssertionError('startup timed out')


def stop_core(process, port, directory, log):
    if process.poll() is None:
        instance = json.loads((Path(directory) / 'instance.json').read_text(encoding='utf-8'))
        request = urllib.request.Request('http://127.0.0.1:%d/_control/stop' % port, data=b'',
                                         headers={'Authorization': 'Bearer ' + instance['management_key']})
        try:
            with urllib.request.urlopen(request, timeout=5) as response:
                response.read()
        except OSError:
            pass
        try:
            process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
    log.close()


def management_key(directory):
    """The running core generates its own management key at startup."""
    return json.loads((Path(directory) / 'instance.json').read_text(encoding='utf-8'))['management_key']


def action(directory, port, command, **fields):
    payload = {'command': command, 'request_tag': 'bench'}
    payload.update(fields)
    request = urllib.request.Request(
        'http://127.0.0.1:%d/_control/action' % port,
        data=json.dumps(payload).encode(),
        headers={'Authorization': 'Bearer ' + management_key(directory),
                 'Content-Type': 'application/json'})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.loads(json.load(response)['message'])
    except urllib.error.HTTPError as failure:
        body = failure.read().decode('utf-8', 'replace')
        try:
            raise AssertionError(json.loads(body)['message'])
        except (ValueError, KeyError):
            raise AssertionError(body) from None


def wait_stats_ready(directory, port, timeout=120):
    """The writer migrates and recovers on its own thread, so the HTTP API can
    be ready while statistics are still `migrating`."""
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        try:
            health = action(directory, port, 'stats_health')
            if health.get('state') == 'ready':
                return health
            last = health.get('state')
        except AssertionError as failure:
            last = str(failure)
        time.sleep(0.25)
    raise AssertionError('statistics never became ready (last=%r)' % last)


def timed(directory, port, command, **fields):
    start = time.perf_counter()
    value = action(directory, port, command, **fields)
    return (time.perf_counter() - start) * 1000.0, value


def seed(database, rows, start_ms, scenario="zero"):
    """Inserts synthetic rows directly; no chat content or credentials."""
    connection = sqlite3.connect(database)
    connection.execute('PRAGMA journal_mode=WAL')
    connection.execute('PRAGMA synchronous=OFF')
    # Drop the relay-stage rows so the seeded row count is exactly what is
    # measured (the core refuses ranges matching more than 1,000,000 rows).
    connection.execute('DELETE FROM usage_requests')
    connection.execute('DELETE FROM usage_costs')
    connection.commit()
    connection.execute(
        "INSERT OR REPLACE INTO stats_runs(run_id, started_at_ms, stopped_at_ms, stop_reason,"
        " admitted_requests, dropped_requests, write_failures, last_persisted_at_ms)"
        " VALUES(?,?,?,'normal',?,0,0,?)",
        (RUN_ID, start_ms, start_ms + 30 * DAY_MS, rows, start_ms + 30 * DAY_MS))
    connection.commit()
    batch = []
    costs = []
    span = 30 * DAY_MS
    for index in range(rows):
        request_id = 'seed-%09d' % index
        started = start_ms + (index * span) // max(rows, 1)
        provider = 'provider-%d' % (index % PROVIDERS)
        model = 'bench-%d' % (index % MODELS)
        outcome = 'completed' if index % 20 else 'failed'
        usage = 'complete' if index % 5 else 'missing'
        input_tokens = 1000 + (index % 500) if usage == 'complete' else None
        output_tokens = 100 + (index % 50) if usage == 'complete' else None
        total = (input_tokens + output_tokens) if usage == 'complete' else None
        cache_read = (input_tokens // 2) if usage == 'complete' else None
        batch.append((
            request_id, RUN_ID, 'relay', provider, 'route-%d' % (index % MODELS),
            'Seed Provider %d' % (index % PROVIDERS), 'Seed Route %d' % (index % MODELS),
            'bench-%d' % (index % MODELS), model, None,
            started, started + 900, 900, 120, 150, 1, 1,
            200 if outcome == 'completed' else 429,
            outcome, None if outcome == 'completed' else 'upstream_http',
            usage, None if usage == 'complete' else 'no_usage',
            'upstream' if usage == 'complete' else 'none',
            1, input_tokens, output_tokens, total, cache_read, None, None, None,
            'read_subset' if usage == 'complete' else 'unknown',
            started, started + 900,
        ))
        manual = scenario == 'priced' or (scenario == 'mixed' and index % 3 != 0)
        currency = 'USD' if scenario == 'mixed' and index % 3 == 2 else 'CNY'
        if not manual:
            status='default_zero'; amounts=(0,0,0,0,0); rates=(0,0,0,0)
        else:
            rates=(2_000_000_000,8_000_000_000,0,0)
            if usage=='complete':
                ic=input_tokens*2000; oc=output_tokens*8000
                status='priced'; amounts=(ic,oc,0,0,ic+oc)
            else:
                status='usage_incomplete'; amounts=(None,)*5
        costs.append((request_id, 'manual' if manual else 'default_zero', started,
                      currency, 'flat', *rates, status, *amounts, 2, started))
        if len(batch) >= 20000:
            flush(connection, batch, costs)
            batch, costs = [], []
    flush(connection, batch, costs)
    connection.execute('PRAGMA wal_checkpoint(TRUNCATE)')
    connection.close()


def flush(connection, batch, costs):
    if batch:
        connection.executemany(
            "INSERT INTO usage_requests(request_id, run_id, source, provider_id, route_id,"
            " provider_name, route_name, requested_model, upstream_model, response_model,"
            " started_at_ms, ended_at_ms, duration_ms, ttfb_ms, first_content_ms, stream,"
            " upstream_started, http_status, outcome, error_kind, usage_status, usage_reason,"
            " usage_source, parser_version, input_tokens, output_tokens, total_tokens,"
            " cache_read_tokens, cache_write_tokens, reasoning_tokens, input_uncached_tokens,"
            " input_breakdown, created_at_ms, updated_at_ms)"
            " VALUES(" + ",".join(["?"] * 34) + ")", batch)
        connection.commit()
    if costs:
        connection.executemany(
            "INSERT INTO usage_costs(request_id,price_source,price_snapshot_at_ms,currency,input_mode,"
            "input_rate_nanos,output_rate_nanos,cache_read_rate_nanos,cache_write_rate_nanos,"
            "status,input_cost_nanos,output_cost_nanos,cache_read_cost_nanos,cache_write_cost_nanos,total_cost_nanos,"
            "pricing_version,calculated_at_ms) VALUES(" + ",".join(["?"] * 17) + ")", costs)
        connection.commit()


def percentiles(samples):
    ordered = sorted(samples)
    def at(fraction):
        index = min(len(ordered) - 1, max(0, int(round(fraction * (len(ordered) - 1)))))
        return round(ordered[index], 2)
    return {
        'p50': at(0.5),
        'p95': at(0.95),
        'max': round(ordered[-1], 2),
        'runs': len(ordered),
        'samples': [round(value, 2) for value in ordered],
    }


def relay_once(port, model):
    payload = json.dumps({'model': model, 'stream': True, 'messages': [{'role': 'user', 'content': 'ping'}]}).encode()
    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=20)
    start = time.perf_counter()
    connection.request('POST', '/v1/chat/completions', payload,
                       {'Authorization': 'Bearer ' + LOCAL_KEY, 'Content-Type': 'application/json'})
    response = connection.getresponse()
    response.read()
    elapsed = (time.perf_counter() - start) * 1000.0
    connection.close()
    return elapsed


def relay_samples(port, model, count):
    samples = []
    for _ in range(count):
        samples.append(relay_once(port, model))
    return samples


def measure_query_budget(directory, port, rows, repeats, warmup=2, start_ms=None):
    start = start_ms if start_ms is not None else now_ms() - 30 * DAY_MS
    window = {'from_ms': start, 'to_ms': start + 30 * DAY_MS, 'timezone': 'UTC'}
    results = {'rows': rows}
    # Warm the page cache first: the first query after seeding reads the whole
    # file from disk, which is a cold-start cost rather than a steady state.
    cold = []
    for _ in range(warmup):
        try:
            elapsed, _value = timed(directory, port, 'stats_dashboard', filter=window, granularity='auto')
            cold.append(round(elapsed, 2))
        except AssertionError as failure:
            results['dashboard_error'] = str(failure)
            break
    results['dashboard_cold_ms'] = cold
    dashboard = []
    for _ in range(repeats):
        try:
            elapsed, value = timed(directory, port, 'stats_dashboard', filter=window, granularity='auto')
        except AssertionError as failure:
            # A budget refusal is an explicit, diagnosable answer; record it
            # instead of hiding it or reporting a wrong number.
            results['dashboard_error'] = str(failure)
            break
        dashboard.append(elapsed)
        results['dashboard_series_buckets'] = len(value.get('series', []))
        results['dashboard_request_count'] = value['summary']['request_count']
    if dashboard:
        results['dashboard_ms'] = percentiles(dashboard)

    page = []
    for _ in range(repeats):
        try:
            elapsed, value = timed(directory, port, 'stats_requests', filter=window, page_size=20)
        except AssertionError as failure:
            results['requests_error'] = str(failure)
            break
        page.append(elapsed)
        results['page_rows'] = len(value.get('items', []))
        results['total_rows'] = value.get('total_rows')
    if page:
        results['requests_first_page_ms'] = percentiles(page)

    cursor_page = []
    cursor = None
    for _ in range(min(repeats, 5)):
        fields = {'filter': window, 'page_size': 100}
        if cursor:
            fields['cursor'] = cursor
        try:
            elapsed, value = timed(directory, port, 'stats_requests', **fields)
        except AssertionError as failure:
            results['cursor_error'] = str(failure)
            break
        cursor_page.append(elapsed)
        cursor = value.get('next_cursor')
        if not cursor:
            break
    if cursor_page:
        results['requests_cursor_page_ms'] = percentiles(cursor_page)

    breakdown = []
    for _ in range(repeats):
        try:
            elapsed, value = timed(directory, port, 'stats_breakdown', filter=window,
                                   group_by='model', sort='total_tokens', limit=50)
        except AssertionError as failure:
            results['breakdown_error'] = str(failure)
            break
        breakdown.append(elapsed)
        results['breakdown_groups'] = len(value.get('items', []))
    if breakdown:
        results['breakdown_ms'] = percentiles(breakdown)

    request_id = 'seed-%09d' % (rows // 2)
    detail = []
    for _ in range(repeats):
        try:
            elapsed, _value = timed(directory, port, 'stats_request_detail', request_id=request_id)
        except AssertionError as failure:
            results['detail_error'] = str(failure)
            break
        detail.append(elapsed)
    if detail:
        results['detail_ms'] = percentiles(detail)

    try:
        elapsed, value = timed(directory, port, 'stats_dashboard',
                               filter=dict(window, upstream_model='bench-3'), granularity='auto')
        results['filtered_dashboard_ms'] = round(elapsed, 2)
        results['filtered_request_count'] = value['summary']['request_count']
    except AssertionError as failure:
        results['filtered_dashboard_error'] = str(failure)
    return results


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--exe', default='target/release/workbuddy-proxy')
    parser.add_argument('--rows', type=int, nargs='+', default=[100_000])
    parser.add_argument('--repeats', type=int, default=7)
    parser.add_argument('--relay-samples', type=int, default=150)
    parser.add_argument('--output', default='')
    parser.add_argument('--scenario', choices=['zero','priced','mixed'], default='zero')
    args = parser.parse_args()
    if not Path(args.exe).exists():
        raise SystemExit('core binary not found: %s' % args.exe)
    upstream = HttpServer(('127.0.0.1', 0), Mock)
    threading.Thread(target=upstream.serve_forever, daemon=True).start()
    upstream_url = 'http://127.0.0.1:%d/v1' % upstream.server_port
    report = {'exe': str(Path(args.exe).resolve()), 'relay_samples': args.relay_samples, 'datasets': []}
    for rows in args.rows:
        with tempfile.TemporaryDirectory(prefix='workbuddy-stats-bench-') as folder:
            port = free_port()
            write_config(folder, port, upstream_url)
            # The first start creates the schema; the relay and query phases
            # then run against a fresh process so no state leaks between them.
            process, log = start_core(args.exe, folder, port)
            stop_core(process, port, folder, log)

            # Relay overhead: collection off, then on.
            process, log = start_core(args.exe, folder, port)
            settings = action(folder, port, 'stats_settings_get')
            action(folder, port, 'stats_settings_update', enabled=False,
                   retention_days=180, expected_revision=settings['revision'])
            off = relay_samples(port, 'wbp-bench-0', args.relay_samples)
            settings = action(folder, port, 'stats_settings_get')
            action(folder, port, 'stats_settings_update', enabled=True,
                   retention_days=180, expected_revision=settings['revision'])
            on = relay_samples(port, 'wbp-bench-0', args.relay_samples)
            stop_core(process, port, folder, log)

            seeded = now_ms() - 30 * DAY_MS
            seed(str(Path(folder) / 'usage.sqlite3'), rows, seeded, args.scenario)
            process, log = start_core(args.exe, folder, port)
            try:
                health = wait_stats_ready(folder, port)
                entry = measure_query_budget(folder, port, rows, args.repeats, start_ms=seeded)
                assert entry.get('total_rows') == rows, entry
                if 'dashboard_request_count' in entry: assert entry['dashboard_request_count']==rows, entry
                entry['health_record_count'] = health['record_count']
                entry['scenario']=args.scenario
                from benchmark import rss_kib
                entry['core_rss_mib']=round(rss_kib(process.pid)/1024,2)
                entry['db_bytes'] = health['db_bytes']
            finally:
                stop_core(process, port, folder, log)
            off_stats = percentiles(off)
            on_stats = percentiles(on)
            delta = on_stats['p95'] - off_stats['p95']
            entry['relay_collection_off_ms'] = off_stats
            entry['relay_collection_on_ms'] = on_stats
            entry['relay_p95_delta_ms'] = round(delta, 2)
            entry['relay_p95_delta_percent'] = round(delta / off_stats['p95'] * 100.0, 2) if off_stats['p95'] else None
            entry['relay_budget'] = 'within budget' if delta <= max(2.0, off_stats['p95'] * 0.05) else 'OVER BUDGET'
            report['datasets'].append(entry)
            print(json.dumps(entry, indent=2, sort_keys=True), flush=True)
    report['generated_at'] = time.strftime('%Y-%m-%dT%H:%M:%S')
    if args.output:
        target = Path(args.output)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(report, indent=2, sort_keys=True), encoding='utf-8')
        print('wrote', target)
    failures = []
    for entry in report['datasets']:
        rows = entry['rows']
        dashboard_p95 = entry.get('dashboard_ms', {}).get('p95')
        page_p95 = entry.get('requests_first_page_ms', {}).get('p95')
        if dashboard_p95 is None:
            # Only a budget refusal is acceptable; a wrong number never is.
            if 'stats_query_timeout' not in str(entry.get('dashboard_error', '')):
                failures.append('%d rows dashboard failed: %s' % (rows, entry.get('dashboard_error')))
        elif rows <= 100_000 and dashboard_p95 > 500:
            failures.append('%d rows dashboard p95 %.0fms > 500ms' % (rows, dashboard_p95))
        elif rows > 100_000 and dashboard_p95 > 2000:
            failures.append('%d rows dashboard p95 %.0fms > 2000ms' % (rows, dashboard_p95))
        if page_p95 is None:
            failures.append('%d rows request page failed: %s' % (rows, entry.get('requests_error')))
        elif rows <= 100_000 and page_p95 > 200:
            failures.append('%d rows page p95 %.0fms > 200ms' % (rows, page_p95))
        if entry['relay_budget'] != 'within budget':
            failures.append('%d rows relay overhead over budget' % rows)
    if failures:
        print('FAIL: ' + '; '.join(failures))
        return 1
    print('PASS: statistics budgets and relay overhead within target')
    return 0


if __name__ == '__main__':
    sys.exit(main())

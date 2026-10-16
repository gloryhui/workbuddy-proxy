#!/usr/bin/env python3
"""Isolated real-core tests for daily public-price synchronization."""
from contextlib import closing
import argparse
import json
import os
import sqlite3
import tempfile
import threading
import time
from datetime import date, timedelta
from pathlib import Path
import statistics_test as fixture


class Upstream(fixture.Mock):
    input_price = 2
    output_price = 4
    failing = False

    def do_GET(self):
        if self.path != '/catalog':
            payload = {'data': [{'id': 'model-ok'}]}
            status = 200
        elif self.failing:
            payload, status = {'error': 'fixture failure'}, 503
        else:
            payload, status = {'lab': {'api': 'https://lab.test/v1', 'models': {
                'model-ok': {'canonical_model_id': 'lab/model-ok', 'cost': {
                    'input': self.input_price, 'output': self.output_price}},
                'model-json': {'cost': {'input': 1e-7, 'output': 0, 'cache_read': 0}},
            }}}, 200
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--exe', default='target/debug/workbuddy-proxy')
    args = parser.parse_args()
    server = fixture.ThreadingHTTPServer(('127.0.0.1', 0), Upstream)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    os.environ['WORKBUDDY_PROXY_CATALOG_TEST_URL'] = f'http://127.0.0.1:{server.server_port}/catalog'
    checks = fixture.Checks()
    with tempfile.TemporaryDirectory(prefix='workbuddy-price-sync-') as directory:
        port = fixture.free_port()
        fixture.write_config(directory, port, f'http://127.0.0.1:{server.server_port}/v1')
        process, log, _ = fixture.start_core(args.exe, directory, port)
        db = Path(directory) / 'usage.sqlite3'

        def page():
            return fixture.stats(directory, port, 'stats_prices_list', provider_id='provider-a')

        def current():
            return next((r for r in page()['items'] if r['upstream_model'] == 'model-ok'), None)

        def stored_cost():
            with closing(sqlite3.connect(db)) as conn, conn:
                return conn.execute("SELECT input_rate_nanos,total_cost_nanos FROM usage_costs WHERE status='priced' ORDER BY rowid LIMIT 1").fetchone()

        try:
            price = fixture.wait_for(current, 'startup did not initialize prices')
            assert price['input_rate_nanos'] == '2000000000', price
            assert price['currency'] == 'USD' and price['label'].startswith('models.dev'), price
            assert page()['catalog_sync']['last_success_day'] == str(date.today())
            checks.ok('startup imports prices and persists the successful local day')
            fixture.relay(port, 'wbp-ok')
            old_cost = fixture.wait_for(stored_cost, 'priced request not settled')
            assert old_cost == (2_000_000_000, 2_400_000), old_cost
            checks.ok('a real forwarded request freezes the imported USD price')

            fixture.stats(directory, port, 'stats_price_save', price_id=price['rule_id'],
                          provider_id='provider-a', upstream_model='model-ok', currency='CNY',
                          input_mode='flat', input_rate_nanos='9000000000',
                          output_rate_nanos='9000000000', label='manual', expected_revision=price['revision'])
            fixture.stop_core(process, port, directory, log)
            Upstream.input_price = 3.5
            Upstream.output_price = 7
            process, log, _ = fixture.start_core(args.exe, directory, port)
            time.sleep(0.5)
            assert current()['input_rate_nanos'] == '9000000000'
            checks.ok('same-day restart preserves edits without another automatic update')

            result = fixture.stats(directory, port, 'stats_prices_update')
            assert result['catalog_sync']['updated_count'] == 2, result
            changed = current()
            assert changed['input_rate_nanos'] == '3500000000' and changed['currency'] == 'USD', changed
            assert changed['revision'] > price['revision']
            assert stored_cost() == old_cost
            checks.ok('manual refresh silently overwrites manual prices and keeps historical costs')
            rejected = fixture.action(directory, port, 'stats_prices_update',
                                      auth_token=fixture.LOCAL_KEY, expect_error=True)
            assert rejected
            checks.ok('the forwarding key cannot trigger price updates')

            status_before = page()['catalog_sync']
            Upstream.failing = True
            fixture.stats(directory, port, 'stats_prices_update', expect_error=True)
            assert current() == changed and page()['catalog_sync'] == status_before
            checks.ok('failed downloads keep prices and the successful-day marker unchanged')
            Upstream.failing = False

            fixture.stop_core(process, port, directory, log)
            with closing(sqlite3.connect(db)) as conn, conn:
                conn.execute('UPDATE stats_price_catalog SET last_success_day=?',
                             (str(date.today() - timedelta(days=1)),))
            Upstream.input_price = 5
            process, log, _ = fixture.start_core(args.exe, directory, port)
            fixture.wait_for(lambda: current()['input_rate_nanos'] == '5000000000',
                             'yesterday marker was not refreshed at startup')
            assert stored_cost() == old_cost
            checks.ok('a previous-day marker triggers one new startup update')
        finally:
            fixture.stop_core(process, port, directory, log)
    server.shutdown()
    print(f'PASS: {len(checks.passed)} price catalogue integration checks')


if __name__ == '__main__':
    main()

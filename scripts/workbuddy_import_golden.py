#!/usr/bin/env python3
"""Three nonempty origins through real HTTP, sync intents, JSONL and commands.

SQLite is opened read-only for assertions. This module never seeds facts,
settings, classifications or checkpoints with SQL writes.
"""
from contextlib import closing
import json
import subprocess
import shutil
import hashlib
import threading
import time
import urllib.request
import urllib.error
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from workbuddy_import_test import free_port, mixed_line


class MockUpstream(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_GET(self):
        self.reply({'data': [{'id': 'golden-model'}]})

    def do_POST(self):
        self.rfile.read(int(self.headers.get('Content-Length', 0)))
        self.reply({'id': 'golden-response', 'object': 'chat.completion',
                    'model': 'golden-model', 'choices': [{'index': 0, 'message': {
                        'role': 'assistant', 'content': 'synthetic'}, 'finish_reason': 'stop'}],
                    'usage': {'prompt_tokens': 500, 'completion_tokens': 50, 'total_tokens': 550}})

    def reply(self, value):
        body = json.dumps(value).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


class Golden:
    def __init__(self, sandbox, process_options=None, handler_class=MockUpstream):
        self.sandbox = sandbox
        self.process_options = process_options or {}
        self.port = free_port()
        self.server = ThreadingHTTPServer(('127.0.0.1', 0), handler_class)
        self.server.daemon_threads = True
        self.server_started = False
        self.process = None
        self.log = None
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        self.database = sandbox.config_dir / 'usage.sqlite3'
        self.key = None
        self.at = None
        self.project = sandbox.source_root / 'projects/golden'

    def start(self, exe):
        self.exe = self.sandbox.root / ('core-under-test' + exe.suffix)
        shutil.copy2(exe, self.exe)
        self.binary_sha256 = hashlib.sha256(self.exe.read_bytes()).hexdigest()
        self.sandbox.write_core_config(self.port)
        config_path = self.sandbox.config_dir / 'config.json'
        config = json.loads(config_path.read_text())
        config['providers'] = [{'id': 'golden-provider', 'name': 'Golden',
            'upstream': 'http://127.0.0.1:%d/v1' % self.server.server_port,
            'api_key': 'fixture-upstream-key', 'models': [{'id': 'golden-route',
                'alias': 'Golden/golden-model', 'name': 'golden-model',
                'model': 'golden-model', 'reasoning_effort': 'default'}]}]
        config_path.write_text(json.dumps(config))
        self.sandbox.source_root.mkdir(parents=True, exist_ok=True)
        (self.sandbox.source_root / 'models.json').write_text('[]')
        self.log = (self.sandbox.root / 'golden-core.log').open('wb')
        self.process = subprocess.Popen([str(self.exe), '--headless', '--config-dir',
            str(self.sandbox.config_dir)], cwd=self.sandbox.root,
            env=self.sandbox.environment(), stdout=self.log, stderr=self.log, **self.process_options)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.server_started = True
        self.until(self.ready, 'statistics writer readiness')

    def ready(self):
        if self.process.poll() is not None:
            raise AssertionError('isolated golden core exited')
        path = self.sandbox.config_dir / 'instance.json'
        if not path.exists():
            return False
        try:
            self.key = json.loads(path.read_text())['management_key']
            return self.action('stats_health')['state'] == 'ready'
        except (OSError, ValueError, KeyError):
            return False

    @staticmethod
    def until(predicate, description, timeout=30):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if predicate():
                return
            time.sleep(0.1)
        raise AssertionError('golden timeout: ' + description)

    def post(self, path, payload, key, headers=None):
        request = urllib.request.Request('http://127.0.0.1:%d%s' % (self.port, path),
            data=json.dumps(payload).encode(), headers={'Content-Type': 'application/json',
                'Authorization': 'Bearer ' + key, **(headers or {})})
        try:
            response = self.opener.open(request, timeout=10)
        except urllib.error.HTTPError as failure:
            response = failure
        with response:
            raw = response.read()
            if path == '/_control/stop':
                value = {'message': raw.decode()}
            else:
                value = json.loads(raw)
            value['_http_status'] = response.code
            return value

    def raw_action(self, command, **fields):
        value = self.post('/_control/action', dict(command=command, **fields), self.key)
        value['ok'] = 200 <= value.pop('_http_status') < 300
        return value

    def action(self, command, **fields):
        response = self.raw_action(command, **fields)
        if not response.get('ok', False):
            message = response.get('message', '')
            code = message.split(':', 1)[0] if isinstance(message, str) else ''
            allowed = {'stats_query_timeout', 'stats_query_too_large', 'stats_busy',
                       'stats_unavailable', 'stats_invalid_argument', 'stats_cursor_expired'}
            raise AssertionError('golden command failed: %s (%s)' %
                                 (command, code if code in allowed else 'unclassified_error'))
        return json.loads(response['message'])

    def rows(self, sql):
        import sqlite3
        with closing(sqlite3.connect('file:%s?mode=ro' % self.database, uri=True)) as connection:
            return connection.execute(sql).fetchall()

    def wait_rows(self, sql, expected):
        self.until(lambda: self.rows(sql) == expected, sql.split('FROM')[0].strip())

    def enable(self):
        current = self.action('stats_settings_get')
        current = self.action('stats_settings_update', enabled=True, retention_days=0,
                              expected_revision=current['revision'])
        self.action('stats_workbuddy_settings_update', enabled=True,
                    expected_revision=current['revision'])

    def sync_proxy(self):
        response = self.raw_action('install')
        if not response.get('ok') or 'attribution_unconfirmed' in response.get('message', ''):
            raise AssertionError('production sync was not confirmed')
        profile = json.loads((self.sandbox.source_root / 'models.json').read_text())
        if not isinstance(profile, list) or len(profile) != 1:
            raise AssertionError('unexpected isolated profile shape')
        model = 'custom-local:' + profile[0]['id']
        evidence = self.rows("SELECT requested_model FROM workbuddy_model_history "
                             "WHERE evidence='managed_sync' AND state='verified' AND classification='proxy'")
        if (model,) not in evidence:
            raise AssertionError('production sync did not persist the actual model identity')
        return model

    def proxy_request(self):
        response = self.post('/r/golden-route/v1/chat/completions', {
            'model': 'golden-model', 'messages': [{'role': 'user', 'content': 'synthetic'}],
            'stream': False}, 'fixture-local-key', headers={
                'X-Conversation-ID': 'mirror', 'X-Conversation-Message-ID': 'mirror'})
        if response['usage']['total_tokens'] != 550:
            raise AssertionError('mock proxy response usage mismatch')
        self.wait_rows("SELECT COUNT(*) FROM usage_requests WHERE record_origin='proxy'", [(1,)])

    def write_logs(self, proxy_model):
        self.project.mkdir(parents=True, exist_ok=True)
        self.at = int(time.time() * 1000) + 2
        specs = [('builtin', 'hy3', 100, 20, 1.14),
                 ('direct', 'custom-local:golden/direct', 200, 40, 2.28),
                 ('mirror', proxy_model, 500, 50, 0)]
        for session, model, prompt, completion, credit in specs:
            line = json.loads(mixed_line(session, session, prompt, completion, credit))
            line['timestamp'] = self.at
            line['providerData']['requestModelId'] = model
            line['providerData']['model'] = 'golden-model' if session != 'builtin' else 'hy3'
            # The same identity repeated really reaches the reader; ledger dedup
            # must not create a second fact or resurrect a proxy mirror.
            copies = 2 if session in ('builtin', 'mirror') else 1
            (self.project / (session + '.jsonl')).write_text((json.dumps(line) + '\n') * copies)
        self.wait_rows("SELECT state,COUNT(*) FROM workbuddy_import_ledger GROUP BY state ORDER BY state",
                       [('excluded_proxy', 1), ('imported', 2)])

    def check_automatic_custom(self):
        # No creator/source attribution is needed. The mirror is excluded only
        # because its exact WorkBuddy session/message identity reached the proxy.
        self.wait_rows("SELECT record_origin,input_tokens,output_tokens,total_tokens,workbuddy_credit_nanos "
                       "FROM usage_requests ORDER BY record_origin", [
                           ('proxy', 500, 50, 550, None),
                           ('workbuddy_builtin', 100, 20, 120, 1_140_000_000),
                           ('workbuddy_custom', 200, 40, 240, 2_280_000_000)])

    def filter(self, scope, capture):
        return {'from_ms': self.at - 60_000, 'to_ms': self.at + 60_000,
                'timezone': '', 'include_tests': False, 'source_scope': scope,
                'capture_method': capture}

    def check_reports(self, report):
        expected = [('all', 'all', 3, 910, '3420000000'),
                    ('builtin', 'all', 1, 120, '1140000000'),
                    ('supplier', 'all', 2, 790, '2280000000'),
                    ('all', 'workbuddy', 2, 360, '3420000000'),
                    ('all', 'proxy', 1, 550, None)]
        for scope, capture, count, tokens, credits in expected:
            query_filter = self.filter(scope, capture)
            dashboard = self.action('stats_dashboard', filter=query_filter, granularity='auto')
            summary = dashboard['summary']
            measured = (summary['request_count'], summary['total_tokens'], summary['workbuddy_credit_nanos'])
            report.check('golden: series %s/%s matches overview count and token totals' % (scope, capture),
                sum(point['request_count'] for point in dashboard['series']) == count
                and sum(point['total_tokens'] or 0 for point in dashboard['series']) == tokens)

            report.check('golden: dashboard %s/%s exact totals' % (scope, capture),
                         measured == (count, tokens, credits), repr(measured))
            if capture == 'workbuddy' or scope == 'builtin':
                report.check('golden: imported usage does not invent HTTP success',
                             summary['success_rate'] is None, repr(summary['success_rate']))
            page = self.action('stats_requests', filter=query_filter, page_size=20)
            report.check('golden: request list %s/%s exact count' % (scope, capture),
                         len(page['items']) == count and page['total_rows'] == count)
        self.check_details(report)

    def check_details(self, report):
        page = self.action('stats_requests', filter=self.filter('all', 'workbuddy'), page_size=20)
        for row in page['items']:
            detail = self.action('stats_request_detail', request_id=row['request_id'])
            data = detail['request']
            report.check('golden: imported detail has precise credit and unknown transport',
                data['record_origin'] in ('workbuddy_builtin', 'workbuddy_custom')
                and data['workbuddy_credit_nanos'] in ('1140000000', '2280000000')
                and data['stream'] is None and data['http_status'] is None
                and data['duration_ms'] is None and data['upstream_protocol'] is None
                and data['outcome'] == 'recorded' and detail['price_snapshot'] is None)

    def full_check(self, report):
        self.action('stats_workbuddy_scan', mode='full')
        self.until(lambda: self.action('stats_workbuddy_status')['scan_state'] == 'completed',
                   'full reconciliation completion')
        facts = self.rows('SELECT COUNT(*),SUM(total_tokens),SUM(workbuddy_credit_nanos) FROM usage_requests')
        report.check('golden: full rescan preserves exact facts, token and credit totals',
                     facts == [(3, 910, 3_420_000_000)], repr(facts))
        states = self.rows('SELECT state,COUNT(*) FROM workbuddy_import_ledger GROUP BY state ORDER BY state')
        report.check('golden: actual proxy mirror excluded; all three ledger identities unique',
                     states == [('excluded_proxy', 1), ('imported', 2)], repr(states))

    def close(self):
        if self.process is not None and self.process.poll() is None:
            try:
                self.post('/_control/stop', {}, self.key)
                self.process.wait(timeout=5)
            except (OSError, subprocess.TimeoutExpired, ValueError):
                self.process.terminate()
                try:
                    self.process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    self.process.wait()
        if self.log is not None:
            self.log.close()
        if self.server_started:
            self.server.shutdown()
        self.server.server_close()


def run(sandbox, exe, report):
    golden = Golden(sandbox)
    try:
        golden.start(exe)
        golden.enable()
        proxy_model = golden.sync_proxy()
        golden.proxy_request()
        golden.write_logs(proxy_model)
        golden.check_automatic_custom()
        report.check('golden: real proxy, builtin and direct each have one nonempty fact', True)
        golden.check_reports(report)
        golden.full_check(report)
    finally:
        golden.close()

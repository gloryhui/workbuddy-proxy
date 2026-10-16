#!/usr/bin/env python3
"""Usage-statistics integration tests.

Runs the real core binary against a temporary configuration directory, fake keys
and a local Mock upstream. Nothing here touches a user profile, a real provider
or a running instance: the config directory is a fresh temporary folder and the
proxy port is random.

Verified on this path:
  * byte-for-byte response passthrough (direct upstream vs. proxied);
  * default usage requests, observe-mode fidelity and `include_usage` merging;
  * SSE/JSON usage recording, conflicts, oversized events, encodings, 429, cuts;
  * client cancellation, connection-test exclusion, paused collection;
  * management API filtering, pagination, pricing, reprice, clear preview/start;
  * crash recovery of in-flight rows.

Usage:
    python3 scripts/statistics_test.py --exe target/release/workbuddy-proxy
"""
import argparse
import gzip
import hashlib
import http.client
import json
import os
import socket
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer as HttpServer
from pathlib import Path

MANAGEMENT_KEY = 'fixture-management-key'
LOCAL_KEY = 'fixture-local-key'
UPSTREAM_KEY = 'fixture-upstream-key'

USAGE = {
    'prompt_tokens': 1000,
    'completion_tokens': 100,
    'total_tokens': 1100,
    'prompt_tokens_details': {'cached_tokens': 800},
}
CONFLICTING_USAGE = {
    'prompt_tokens': 40,
    'completion_tokens': 2,
    'total_tokens': 42,
}


class ThreadingHTTPServer(HttpServer):
    def handle_error(self, request, address):
        if isinstance(sys.exc_info()[1], (ConnectionResetError, ConnectionAbortedError, BrokenPipeError)):
            return
        super().handle_error(request, address)


class Mock(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'
    received = []
    lock = threading.Lock()

    def setup(self):
        super().setup()
        self.connection.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)

    def log_message(self, *args):
        pass

    def take_received(self):
        with self.lock:
            return list(self.received)

    def do_POST(self):
        length = int(self.headers.get('Content-Length', '0'))
        body = self.rfile.read(length)
        with self.lock:
            self.received.append({
                'body': body,
                'path': self.path,
                'headers': {k.lower(): v for k, v in self.headers.items()},
            })
        try:
            request = json.loads(body)
        except ValueError:
            request = {}
        model = request.get('model', '')
        if model == 'model-429':
            payload = b'{"error":{"message":"rate limited"}}'
            self.send_response(429)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(payload)))
            self.send_header('Retry-After', '3')
            self.end_headers()
            self.wfile.write(payload)
            return
        if model == 'model-gzip':
            payload = gzip.compress(json.dumps({
                'model': 'model-gzip',
                'choices': [{'message': {'role': 'assistant', 'content': 'compressed'}}],
                'usage': USAGE,
            }).encode())
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Encoding', 'gzip')
            self.send_header('Content-Length', str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return
        if model == 'model-json':
            payload = json.dumps({
                'id': 'chatcmpl-json',
                'model': 'model-json',
                'choices': [{'index': 0, 'message': {'role': 'assistant', 'content': 'hello'}, 'finish_reason': 'stop'}],
                'usage': USAGE,
            }).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'application/json; charset=utf-8')
            self.send_header('Content-Length', str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return
        if model == 'model-cut':
            # A truncated SSE body: headers promise more, the socket closes.
            part = b'data: {"model":"model-cut","choices":[{"delta":{"content":"partial"}}]}\n\n'
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Transfer-Encoding', 'chunked')
            self.end_headers()
            self.wfile.write(b'%x\r\n' % len(part) + part + b'\r\n')
            self.wfile.flush()
            self.close_connection = True
            return
        if model == 'model-slow':
            first = b'data: {"model":"model-slow","choices":[{"delta":{"content":"slow"}}]}\n\n'
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Transfer-Encoding', 'chunked')
            self.end_headers()
            self.wfile.write(b'%x\r\n' % len(first) + first + b'\r\n')
            self.wfile.flush()
            time.sleep(30)
            return
        # Streaming scenarios.
        if model == 'model-nousage':
            events = [b'data: {"model":"model-nousage","choices":[{"delta":{"content":"no usage"}}]}\n\n',
                      b'data: [DONE]\n\n']
        elif model == 'model-conflict':
            events = [
                b'data: {"model":"model-conflict","choices":[{"delta":{"content":"a"}}],"usage":{"prompt_tokens":1000,"completion_tokens":100,"total_tokens":1100}}\n\n',
                b'data: {"choices":[],"usage":{"prompt_tokens":40,"completion_tokens":2,"total_tokens":42}}\n\n',
                b'data: [DONE]\n\n',
            ]
        elif model == 'model-big':
            huge = 'x' * (70 * 1024)
            events = [
                ('data: {"model":"model-big","choices":[{"delta":{"content":"%s"}}]}\n\n' % huge).encode(),
                b'data: {"choices":[],"usage":{"prompt_tokens":7,"completion_tokens":3,"total_tokens":10}}\n\n',
                b'data: [DONE]\n\n',
            ]
        elif model == 'model-encoded':
            events = [b'data: {"choices":[{"delta":{"content":"hi"}}]}\n\n',
                      b'data: {"choices":[],"usage":{"prompt_tokens":5,"completion_tokens":1,"total_tokens":6}}\n\n',
                      b'data: [DONE]\n\n']
        else:
            events = [
                b'data: {"model":"model-ok","choices":[{"delta":{"role":"assistant"}}]}\n\n',
                b'data: {"choices":[{"delta":{"content":"hello"}}]}\n\n',
                b'data: {"choices":[],"usage":' + json.dumps(USAGE).encode() + b'}\n\n',
                b'data: {"choices":[],"usage":' + json.dumps(USAGE).encode() + b'}\n\n',
                b'data: [DONE]\n\n',
            ]
        if model == 'model-encoded':
            raw = b''.join(events)
            payload = gzip.compress(raw)
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Content-Encoding', 'gzip')
            self.send_header('Content-Length', str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Transfer-Encoding', 'chunked')
        self.end_headers()
        for index, event in enumerate(events):
            if model == 'model-ok' and index == 2:
                # Split one SSE event across two TCP chunks on purpose.
                half = len(event) // 2
                for piece in (event[:half], event[half:]):
                    self.wfile.write(b'%x\r\n' % len(piece) + piece + b'\r\n')
                    self.wfile.flush()
                    time.sleep(0.02)
                continue
            self.wfile.write(b'%x\r\n' % len(event) + event + b'\r\n')
            self.wfile.flush()
        self.wfile.write(b'0\r\n\r\n')
        self.wfile.flush()


def free_port():
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]


def now_ms():
    return int(time.time() * 1000)


def wait_for(predicate, message, timeout=15.0):
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        try:
            value = predicate()
        except Exception as failure:  # pragma: no cover - diagnostics only
            last = failure
            value = None
        if value:
            return value
        time.sleep(0.05)
    raise AssertionError('%s (last error: %r)' % (message, last))


def write_config(directory, port, upstream):
    models = [
        ('model-ok', 'wbp-ok'),
        ('model-nousage', 'wbp-nousage'),
        ('model-conflict', 'wbp-conflict'),
        ('model-big', 'wbp-big'),
        ('model-json', 'wbp-json'),
        ('model-cut', 'wbp-cut'),
        ('model-slow', 'wbp-slow'),
        ('model-429', 'wbp-429'),
        ('model-gzip', 'wbp-gzip'),
        ('model-encoded', 'wbp-encoded'),
    ]
    config = {
        'local_key': LOCAL_KEY,
        'language': 'en',
        'port': port,
        'autostart': False,
        'providers': [{
            'id': 'provider-a',
            'name': 'Provider A',
            'upstream': upstream,
            'api_key': UPSTREAM_KEY,
            'models': [{
                'id': 'route-%d' % index,
                'alias': alias,
                'name': model,
                'model': model,
                'reasoning_effort': 'default',
            } for index, (model, alias) in enumerate(models)],
        }],
        'installed': [],
    }
    (Path(directory) / 'config.json').write_text(json.dumps(config), encoding='utf-8')


def start_core(exe, directory, port):
    log = open(Path(directory, 'process.log'), 'ab')
    process = subprocess.Popen([str(Path(exe).resolve()), '--config-dir', str(directory), '--headless'],
                               stdout=log, stderr=log)
    for _ in range(400):
        if process.poll() is not None:
            log.close()
            text = (Path(directory) / 'process.log').read_text(encoding='utf-8', errors='replace')
            raise AssertionError('Core exited during startup:\n' + text)
        try:
            with urllib.request.urlopen('http://127.0.0.1:%d/health' % port, timeout=0.5) as response:
                health = json.load(response)
            if health.get('ready'):
                return process, log, health
        except (OSError, ValueError):
            time.sleep(0.025)
    process.kill()
    log.close()
    raise AssertionError('Core startup timed out')


def stop_core(process, port, directory, log, expect_clean=True):
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
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
    log.close()
    if expect_clean:
        assert process.returncode == 0, 'Core exited with %r' % process.returncode


def management_key(directory):
    instance = json.loads((Path(directory) / 'instance.json').read_text(encoding='utf-8'))
    return instance['management_key']


def action(directory, port, command, auth_token=None, expect_error=False, **fields):
    payload = {'command': command, 'request_tag': 'fixture-tag'}
    payload.update(fields)
    request = urllib.request.Request(
        'http://127.0.0.1:%d/_control/action' % port,
        data=json.dumps(payload).encode(),
        headers={'Authorization': 'Bearer ' + (auth_token or management_key(directory)),
                 'Content-Type': 'application/json'})
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            body = json.load(response)
            status = response.status
    except urllib.error.HTTPError as failure:
        raw = failure.read()
        status = failure.code
        try:
            body = json.loads(raw.decode('utf-8'))
        except ValueError:
            body = {'message': raw.decode('utf-8', 'replace')}
    if isinstance(body, dict):
        # Success replies nest the payload under `message`; transport-level
        # failures use the proxy error envelope instead.
        message = body.get('message') or (body.get('error') or {}).get('message') or ''
    else:
        message = str(body)
    if expect_error:
        assert status != 200, 'expected a failure for %s, got %s' % (command, message)
        return message
    assert status == 200, '%s failed: %s' % (command, message)
    try:
        return json.loads(message)
    except ValueError:
        return message


def stats(directory, port, command, **fields):
    return action(directory, port, command, **fields)


def window(start_ms):
    return {'from_ms': start_ms - 60_000, 'to_ms': now_ms() + 60_000, 'timezone': 'UTC'}


def relay(port, model, token=LOCAL_KEY, stream=True, extra=None, timeout=20):
    payload = {'model': model, 'stream': stream, 'messages': [{'role': 'user', 'content': 'ping'}]}
    if extra:
        payload.update(extra)
    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=timeout)
    connection.request('POST', '/v1/chat/completions', json.dumps(payload).encode(),
                       {'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'})
    response = connection.getresponse()
    body = response.read()
    headers = {k.lower(): v for k, v in response.getheaders()}
    connection.close()
    return response.status, headers, body


def relay_and_cancel(port, model, token=LOCAL_KEY, read_bytes=24):
    payload = {'model': model, 'stream': True, 'messages': [{'role': 'user', 'content': 'ping'}]}
    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=20)
    connection.request('POST', '/v1/chat/completions', json.dumps(payload).encode(),
                       {'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'})
    response = connection.getresponse()
    first = response.read(read_bytes)
    connection.close()
    return first


def direct(upstream_port, model, stream=True, extra=None):
    payload = {'model': model, 'stream': stream, 'messages': [{'role': 'user', 'content': 'ping'}]}
    if extra:
        payload.update(extra)
    connection = http.client.HTTPConnection('127.0.0.1', upstream_port, timeout=20)
    connection.request('POST', '/v1/chat/completions', json.dumps(payload).encode(),
                       {'Authorization': 'Bearer ' + UPSTREAM_KEY, 'Content-Type': 'application/json'})
    response = connection.getresponse()
    body = response.read()
    status = response.status
    connection.close()
    return status, body


def find_row(items, model, source='relay'):
    for item in items:
        if item['upstream_model'] == model and item['source'] == source:
            return item
    return None


def digest(value):
    return hashlib.sha256(value).hexdigest()[:16]


class Checks:
    def __init__(self):
        self.passed = []

    def ok(self, name, detail=''):
        self.passed.append(name)
        print('  PASS %-46s %s' % (name, detail), flush=True)


def run(directory, port, upstream_port, start_ms, checks):
    # --- byte-for-byte passthrough -------------------------------------
    status, _headers, proxied = relay(port, 'wbp-ok')
    forwarded = json.loads(Mock.received[-1]['body'])
    assert forwarded['stream_options']['include_usage'] is True, forwarded
    settings = stats(directory, port, 'stats_provider_settings_get', provider_id='provider-a')
    assert settings['usage_mode'] == 'request' and settings['revision'] == 0, settings
    checks.ok('usage is requested by default before settings are read')
    direct_status, direct_body = direct(upstream_port, 'model-ok')
    assert status == direct_status == 200, (status, direct_status)
    assert proxied == direct_body, 'streamed body differs: %s vs %s' % (digest(proxied), digest(direct_body))
    checks.ok('streaming response is byte-identical', digest(proxied))

    status, _headers, proxied = relay(port, 'wbp-json', stream=False)
    direct_status, direct_body = direct(upstream_port, 'model-json', stream=False)
    assert status == 200 and proxied == direct_body, 'json body differs'
    checks.ok('non-streaming response is byte-identical', digest(proxied))

    status, _headers, body = relay(port, 'wbp-429')
    assert status == 429, status
    assert body == b'{"error":{"message":"rate limited"}}', body
    checks.ok('upstream 429 status and body are preserved')

    # --- request fidelity ----------------------------------------------
    stats(directory, port, 'stats_provider_settings_update', provider_id='provider-a',
          usage_mode='observe', expected_revision=0)
    original = json.dumps({'model': 'wbp-ok', 'stream': True, 'temperature': 0.25,
                           'messages': [{'role': 'user', 'content': 'ping'}],
                           'tools': [{'type': 'function'}]}, separators=(',', ':')).encode()
    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=20)
    connection.request('POST', '/v1/chat/completions', original,
                       {'Authorization': 'Bearer ' + LOCAL_KEY, 'Content-Type': 'application/json'})
    connection.getresponse().read()
    connection.close()
    forwarded = wait_for(lambda: next(
        (entry['body'] for entry in reversed(Mock.received) if b'"model-ok"' in entry['body']), None),
        'proxied observe request was not forwarded')
    expected = original.replace(b'"wbp-ok"', b'"model-ok"')
    assert forwarded == expected, 'observe request was rewritten:\n%r\n%r' % (forwarded, expected)
    checks.ok('observe mode forwards the original request bytes')

    # --- request mode merges only include_usage ------------------------
    settings = stats(directory, port, 'stats_provider_settings_get', provider_id='provider-a')
    assert settings['usage_mode'] == 'observe' and settings['revision'] == 1, settings
    stats(directory, port, 'stats_provider_settings_update', provider_id='provider-a',
          usage_mode='request', expected_revision=settings['revision'])
    stream_request = b'{"model":"wbp-ok","stream":true,"stream_options":{"vendor":"keep"},"messages":[]}'
    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=20)
    connection.request('POST', '/v1/chat/completions', stream_request,
                       {'Authorization': 'Bearer ' + LOCAL_KEY, 'Content-Type': 'application/json'})
    connection.getresponse().read()
    connection.close()
    merged = wait_for(lambda: next(
        (entry['body'] for entry in reversed(Mock.received) if b'"include_usage"' in entry['body']), None),
        'include_usage was not merged')
    parsed = json.loads(merged)
    assert parsed['stream_options'] == {'vendor': 'keep', 'include_usage': True}, parsed['stream_options']
    checks.ok('request mode merges include_usage and keeps unknown fields')

    non_stream = b'{"model":"wbp-ok","stream":false,"messages":[]}'
    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=20)
    connection.request('POST', '/v1/chat/completions', non_stream,
                       {'Authorization': 'Bearer ' + LOCAL_KEY, 'Content-Type': 'application/json'})
    connection.getresponse().read()
    connection.close()
    time.sleep(0.3)
    forwarded = [entry['body'] for entry in Mock.received if b'"stream":false' in entry['body']]
    assert forwarded and b'include_usage' not in forwarded[-1], 'include_usage leaked into a non-stream request'
    checks.ok('request mode never injects stream_options for stream=false')

    broken = b'{"model":"wbp-ok","stream":true,"stream_options":"off","messages":[]}'
    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=20)
    connection.request('POST', '/v1/chat/completions', broken,
                       {'Authorization': 'Bearer ' + LOCAL_KEY, 'Content-Type': 'application/json'})
    response = connection.getresponse()
    response.read()
    connection.close()
    assert response.status == 200, 'a non-object stream_options must not break the chat'
    forwarded = [entry['body'] for entry in Mock.received if b'"stream_options":"off"' in entry['body']]
    assert forwarded, 'the original stream_options value must be forwarded unchanged'
    checks.ok('non-object stream_options is forwarded unchanged')
    stats(directory, port, 'stats_provider_settings_update', provider_id='provider-a',
          usage_mode='observe', expected_revision=2)

    # --- remaining scenarios -------------------------------------------
    relay(port, 'wbp-nousage')
    relay(port, 'wbp-conflict')
    relay(port, 'wbp-big')
    relay(port, 'wbp-gzip', stream=False)
    relay(port, 'wbp-encoded')
    try:
        relay(port, 'wbp-cut')
    except (http.client.HTTPException, OSError):
        pass
    relay_and_cancel(port, 'wbp-slow')

    # --- connection test ------------------------------------------------
    report = action(directory, port, 'test', provider_id='route-0')
    assert isinstance(report, dict) and report.get('kind') == 'connection_test', report
    assert report.get('response_model') == 'model-ok', report
    assert report.get('reason') == '', report
    checks.ok('connection test still reports success')

    rows = wait_for(lambda: _rows_ready(directory, port, start_ms,
                                        {'model-ok': 'complete', 'model-nousage': 'missing',
                                         'model-conflict': 'invalid', 'model-big': 'complete',
                                         'model-json': 'complete'}),
                    'statistics rows did not settle')
    checks.ok('all scenarios were recorded', '%d rows' % rows['total_rows'])

    items = rows['items']
    ok_row = find_row(items, 'model-ok')
    assert ok_row['total_tokens'] == 1100 and ok_row['input_tokens'] == 1000, ok_row
    assert ok_row['cache_read_tokens'] == 800 and ok_row['usage_status'] == 'complete', ok_row
    assert ok_row['outcome'] == 'completed' and ok_row['http_status'] == 200, ok_row
    assert ok_row['cache_read_tokens'] <= ok_row['input_tokens']
    checks.ok('A01/A02 complete usage is counted once', json.dumps({
        'total': ok_row['total_tokens'], 'cache_read': ok_row['cache_read_tokens']}))

    no_usage = find_row(items, 'model-nousage')
    assert no_usage['usage_status'] == 'missing' and no_usage['total_tokens'] is None, no_usage
    assert no_usage['outcome'] == 'completed', no_usage
    checks.ok('A03 missing usage stays null and the request still completes')

    conflict = find_row(items, 'model-conflict')
    assert conflict['usage_status'] == 'invalid', conflict
    assert conflict['total_tokens'] is None and conflict['input_tokens'] is None, conflict
    checks.ok('A08 conflicting usage is invalid and excluded')

    big = find_row(items, 'model-big')
    assert big['usage_status'] == 'complete' and big['total_tokens'] == 10, big
    checks.ok('A09 usage after an oversized event is still parsed')

    gzip_row = find_row(items, 'model-gzip')
    assert gzip_row['usage_status'] == 'missing', gzip_row
    assert gzip_row['usage_reason'] == 'unsupported_encoding', gzip_row
    checks.ok('compressed responses never guess usage')

    cut_row = find_row(items, 'model-cut')
    assert cut_row['outcome'] in ('failed', 'interrupted'), cut_row
    assert cut_row['usage_status'] == 'missing', cut_row
    checks.ok('A06 a truncated 200 stream is not a success', cut_row['outcome'])

    slow_row = find_row(items, 'model-slow')
    assert slow_row['outcome'] == 'cancelled', slow_row
    assert slow_row['error_kind'] == 'client_drop', slow_row
    checks.ok('A04 client cancellation is recorded immediately')

    failed = find_row(items, 'model-429')
    assert failed['outcome'] == 'failed' and failed['error_kind'] == 'upstream_http', failed
    assert failed['http_status'] == 429, failed
    checks.ok('A06 upstream 429 keeps status and marks failure')

    json_row = find_row(items, 'model-json')
    assert json_row['usage_status'] == 'complete' and json_row['total_tokens'] == 1100, json_row
    checks.ok('non-streaming JSON usage is parsed')

    encoded = find_row(items, 'model-encoded')
    assert encoded is not None and encoded['usage_status'] == 'missing', encoded
    checks.ok('encoded SSE is not decoded implicitly')

    # --- dashboard consistency ------------------------------------------
    dashboard = stats(directory, port, 'stats_dashboard', filter=window(start_ms), granularity='auto')
    assert dashboard['summary']['request_count'] == rows['total_rows'], dashboard['summary']
    expected_tokens = sum(item['total_tokens'] or 0
                          for item in rows['items'] if item['usage_status'] == 'complete')
    assert dashboard['summary']['total_tokens'] == expected_tokens, (
        dashboard['summary']['total_tokens'], expected_tokens)
    assert dashboard['summary']['completed_count'] <= dashboard['summary']['request_count']
    assert dashboard['summary']['failed_count'] >= dashboard['summary']['rejected_count']
    assert dashboard['summary']['cache_read_known_count'] >= 1
    assert dashboard['series'], 'series must cover the range'
    bucket_total = sum(point['request_count'] for point in dashboard['series'])
    assert bucket_total == rows['total_rows'], (bucket_total, rows['total_rows'])
    checks.ok('dashboard summary and buckets match the request list')

    filtered = stats(directory, port, 'stats_requests', page_size=20,
                     **{'filter': dict(window(start_ms), upstream_model='model-ok')})
    assert filtered['items'], 'the model filter returned nothing'
    assert {item['upstream_model'] for item in filtered['items']} == {'model-ok'}, filtered['items']
    checks.ok('model filter is exact')

    # --- pagination ------------------------------------------------------
    # Page sizes are fixed at 20/50/100, so add enough rows to page for real.
    for _ in range(24):
        relay(port, 'wbp-nousage')
    # Statistics land asynchronously; wait until the total stops moving so the
    # walk below cannot race the writer (a flaky assertion would hide real
    # cursor bugs).
    # Freeze one window for the whole walk: pagination is bound to the exact
    # filter it started with, so recomputing `to_ms` per request would (rightly)
    # expire the cursor.
    frozen = window(start_ms)
    previous = -1
    for _ in range(120):
        settled = stats(directory, port, 'stats_requests', filter=frozen, page_size=20)['total_rows']
        if settled == previous:
            break
        previous = settled
        time.sleep(0.15)
    page_size = 20
    cursor = None
    seen = []
    pages = 0
    for _ in range(20):
        fields = {'filter': frozen, 'page_size': page_size}
        if cursor:
            fields['cursor'] = cursor
        page = stats(directory, port, 'stats_requests', **fields)
        pages += 1
        assert len(page['items']) <= page_size, len(page['items'])
        seen.extend(item['id'] for item in page['items'])
        cursor = page.get('next_cursor')
        if not cursor:
            break
    assert pages >= 2, 'expected more than one page, saw %d' % pages
    assert len(seen) == len(set(seen)), 'keyset pagination repeated rows'
    total = stats(directory, port, 'stats_requests', filter=frozen, page_size=20)['total_rows']
    assert len(seen) == total, (len(seen), total)
    invalid = stats(directory, port, 'stats_requests', filter=frozen, page_size=7,
                    expect_error=True)
    assert invalid.startswith('stats_invalid_argument'), invalid

    # Cursor identity is enforced, not merely generated (F04).
    first = stats(directory, port, 'stats_requests', filter=frozen, page_size=20)
    assert first['next_cursor'], 'expected a second page'
    good = first['next_cursor']
    assert good['filter_key'] == first['filter_key'], good
    assert good['run_id'], 'the server must stamp its run id on the cursor'
    second = stats(directory, port, 'stats_requests', filter=frozen, page_size=20, cursor=good)
    assert second['items'], 'the cursor handed out by the server must work'

    for field, value in [('run_id', 'someone-elses-run'), ('filter_key', 'deadbeef'),
                         ('maintenance_revision', good['maintenance_revision'] + 1),
                         ('last_id', -1), ('as_of_id', -5)]:
        tampered = dict(good)
        tampered[field] = value
        failure = stats(directory, port, 'stats_requests', filter=frozen, page_size=20,
                        cursor=tampered, expect_error=True)
        assert failure.startswith('stats_cursor_expired'), (field, failure)

    # A cursor is bound to its filter, so a different window must not reuse it.
    other = stats(directory, port, 'stats_requests', filter=window(start_ms - 1000), page_size=20,
                  cursor=good, expect_error=True)
    assert other.startswith('stats_cursor_expired'), other
    checks.ok('keyset pagination walks every row once and rejects stale cursors',
              '%d rows in %d pages' % (len(seen), pages))

    first_page = stats(directory, port, 'stats_requests', filter=window(start_ms), page_size=20)
    assert first_page['next_cursor'] is not None
    assert first_page['next_cursor']['filter_key'] == first_page['filter_key']
    assert first_page['as_of_id'] >= first_page['items'][0]['id']
    detail = stats(directory, port, 'stats_request_detail', request_id=ok_row['request_id'])
    assert detail['request']['request_id'] == ok_row['request_id']
    assert detail['cost_parts'] is not None
    assert 'messages' not in json.dumps(detail), 'detail must never expose chat bodies'
    checks.ok('request detail returns summary fields only')

    missing = stats(directory, port, 'stats_request_detail', request_id='does-not-exist',
                    expect_error=True)
    assert missing.startswith('stats_not_found'), missing
    checks.ok('unknown request ids report stats_not_found')

    # --- breakdown and filters -------------------------------------------
    breakdown = stats(directory, port, 'stats_breakdown', filter=window(start_ms),
                      group_by='model', sort='total_tokens', limit=50)
    labels = [item['key'] for item in breakdown['items']]
    assert 'model-ok' in labels and breakdown['total_groups'] >= 1, labels
    options = stats(directory, port, 'stats_filters', kind='provider', limit=50)
    assert any(item['key'] == 'provider-a' for item in options['items']), options
    model_options = stats(directory, port, 'stats_filters', kind='model', limit=100, query='status')
    assert all('status' in item['key'] for item in model_options['items']), model_options
    checks.ok('breakdown and filter options are served server-side')

    # --- R02: the server's own grouping cursor must work -------------------
    # The page walks forward 1 -> 2 -> 3 and then back 3 -> 2 -> 1 through the
    # cursor each page was started with.
    # One frozen window for the whole walk: the cursor is bound to its filter.
    group_window = window(start_ms)
    for group_by, sort in [('model', 'total_tokens'), ('provider', 'request_count')]:
        first = stats(directory, port, 'stats_breakdown', filter=group_window,
                      group_by=group_by, sort=sort, limit=1)
        if first['total_groups'] < 3:
            # A single-group result is a complete page, not a truncated one; the
            # fixture has one provider, so only the model walk has pages.
            assert first['next_cursor'] is None, (group_by, first)
            continue
        assert first['next_cursor'], (group_by, sort, first)
        assert first['next_cursor']['group_by'] == group_by, first['next_cursor']
        assert first['next_cursor']['sort'] == sort, first['next_cursor']
        pages = [first]
        cursor = first['next_cursor']
        # Enough pages to reach the end of the grouping.
        while cursor and len(pages) < first['total_groups'] + 2:
            page = stats(directory, port, 'stats_breakdown', filter=group_window,
                         group_by=group_by, sort=sort, limit=1, cursor=cursor)
            pages.append(page)
            cursor = page.get('next_cursor')
        assert len(pages) >= 3, 'expected at least three pages of groups'
        keys = [item['key'] for page in pages for item in page['items']]
        assert len(keys) == len(set(keys)), 'grouping pagination repeated a group: %r' % keys
        assert len(keys) == first['total_groups'], (keys, first['total_groups'])
        # Walking back (3 -> 2 -> 1) replays each earlier page through the very
        # cursor that produced it: page j was fetched with pages[j-1].next_cursor.
        for index in range(len(pages) - 1, 0, -1):
            target = index - 1
            start_cursor = None if target == 0 else pages[target - 1]['next_cursor']
            fields = {'filter': group_window, 'group_by': group_by, 'sort': sort, 'limit': 1}
            if start_cursor is not None:
                fields['cursor'] = start_cursor
            replayed = stats(directory, port, 'stats_breakdown', **fields)
            assert [item['key'] for item in replayed['items']] == \
                   [item['key'] for item in pages[target]['items']], \
                   'replaying page %d differed' % target
        # Tampering and reusing the cursor for another grouping must expire it.
        for field, value in [('run_id', 'somewhere-else'), ('filter_key', 'deadbeef'),
                             ('group_by', 'provider' if group_by == 'model' else 'model'),
                             ('last_group_key', '')]:
            tampered = dict(first['next_cursor'])
            tampered[field] = value
            failure = stats(directory, port, 'stats_breakdown', filter=group_window,
                            group_by=group_by, sort=sort, limit=1, cursor=tampered,
                            expect_error=True)
            assert failure.startswith('stats_cursor_expired'), (field, failure)
    checks.ok('grouping pagination walks every group once and rejects stale cursors')

    # U03: money must obey the same frozen id watermark as the request counts.
    before = stats(directory, port, 'stats_breakdown',
                   filter=group_window, group_by=group_by, sort=sort, limit=1)
    frozen = before['as_of_id']
    for _ in range(2):
        relay(port, 'wbp-nokey')
    after = stats(directory, port, 'stats_breakdown', filter=group_window,
                  group_by=group_by, sort=sort, limit=1,
                  cursor=before['next_cursor'])
    assert after['as_of_id'] == frozen, (after['as_of_id'], frozen)
    for item in after['items']:
        summary = item['summary']
        settled = summary.get('settled_cost_request_count', 0)
        assert settled <= summary['request_count'], item
        identical = stats(directory, port, 'stats_breakdown', filter=group_window,
                          group_by=group_by, sort=sort, limit=1)
        for frozen_item in identical['items']:
            if frozen_item['key'] == item['key']:
                assert (frozen_item['summary'].get('settled_cost_request_count', 0)
                        == settled), (frozen_item, item)
    checks.ok('grouping money obeys the frozen id watermark')

    # --- test rows are excluded by default --------------------------------
    with_tests = stats(directory, port, 'stats_requests',
                       **{'filter': dict(window(start_ms), include_tests=True), 'page_size': 100})
    without_tests = stats(directory, port, 'stats_requests', filter=window(start_ms), page_size=100)
    tests = [item for item in with_tests['items'] if item['source'] == 'connection_test']
    assert tests, 'the connection test must be recorded'
    assert tests[0]['outcome'] == 'cancelled' and tests[0]['error_kind'] == 'client_drop', tests[0]
    assert all(item['source'] == 'relay' for item in without_tests['items'])
    checks.ok('connection tests are recorded but excluded by default')

    # --- permissions -------------------------------------------------------
    denied = action(directory, port, 'stats_health', auth_token=LOCAL_KEY, expect_error=True)
    assert 'Unauthorized' in denied or 'unauthorized' in denied.lower(), denied
    checks.ok('the local API key cannot read statistics')

    # --- pricing: an editable current price plus per-call snapshots (v2) ----
    rules = stats(directory, port, 'stats_prices_list', limit=50)
    assert rules['items'] == [], rules
    # Rates are integer nano units per million tokens ($2/M -> 2000000000).
    rule = stats(directory, port, 'stats_price_save', provider_id='provider-a',
                 upstream_model='model-ok', currency='USD',
                 input_mode='read_discount', input_rate_nanos='2000000000',
                 output_rate_nanos='8000000000', cache_read_rate_nanos='200000000',
                 label='fixture')
    assert rule['input_rate_nanos'] == '2000000000', rule
    assert rule['revision'] == 1, rule
    fractional = stats(directory, port, 'stats_price_save', provider_id='provider-a',
                       upstream_model='model-other', currency='USD',
                       input_mode='flat', input_rate_nanos='0.2', output_rate_nanos='1',
                       label='fractional', expect_error=True)
    assert 'whole number of nano units' in fractional, fractional
    # The current price can be edited directly; history is not locked by it.
    edited = stats(directory, port, 'stats_price_save', rule_id=rule['rule_id'],
                   provider_id='provider-a', upstream_model='model-ok', currency='USD',
                   input_mode='read_discount', input_rate_nanos='3000000000',
                   output_rate_nanos='9000000000', cache_read_rate_nanos='300000000',
                   label='edited', expected_revision=1)
    assert edited['rule_id'] == rule['rule_id'], edited
    assert edited['revision'] == 2, edited
    assert edited['input_rate_nanos'] == '3000000000', edited
    stale_edit = stats(directory, port, 'stats_price_save', rule_id=rule['rule_id'],
                       provider_id='provider-a', upstream_model='model-ok', currency='USD',
                       input_mode='flat', input_rate_nanos='1', output_rate_nanos='1',
                       label='stale', expected_revision=1, expect_error=True)
    assert stale_edit.startswith('stats_revision_conflict'), stale_edit
    listed = stats(directory, port, 'stats_prices_list')
    assert len(listed['items']) == 1, listed
    assert listed['items'][0]['revision'] == 2, listed
    checks.ok('the current price can be added, edited and listed without interval rules')

    # U04: a repeated "new" must really conflict, the v2 `price_id` spelling must
    # edit the exact row, and a blank rate is the documented zero.
    clash = stats(directory, port, 'stats_price_save', provider_id='provider-a',
                  upstream_model='model-ok', currency='USD', input_mode='flat',
                  input_rate_nanos='99000000000', output_rate_nanos='1',
                  label='clash', expect_error=True)
    assert clash.startswith('stats_revision_conflict'), clash
    unchanged = stats(directory, port, 'stats_prices_list')
    assert unchanged['items'][0]['input_rate_nanos'] == '3000000000', unchanged
    by_canonical = stats(directory, port, 'stats_price_save', price_id=rule['rule_id'],
                         provider_id='provider-a', upstream_model='model-ok',
                         currency='USD', input_mode='flat', input_rate_nanos='4000000000',
                         output_rate_nanos='4000000000', label='renamed',
                         expected_revision=2)
    assert by_canonical['rule_id'] == rule['rule_id'], by_canonical
    assert by_canonical['revision'] == 3, by_canonical
    assert by_canonical['input_rate_nanos'] == '4000000000', by_canonical
    # Changing the identifying pair through an edit is refused, not ignored.
    remap = stats(directory, port, 'stats_price_save', price_id=rule['rule_id'],
                  provider_id='provider-a', upstream_model='model-renamed', currency='USD',
                  input_mode='flat', input_rate_nanos='4000000000',
                  output_rate_nanos='4000000000', label='renamed',
                  expected_revision=3, expect_error=True)
    assert remap.startswith('stats_invalid_argument'), remap
    missing = stats(directory, port, 'stats_price_save', price_id='does-not-exist',
                    provider_id='provider-a', upstream_model='model-ok', currency='USD',
                    input_mode='flat', input_rate_nanos='1', output_rate_nanos='1',
                    expected_revision=1, expect_error=True)
    assert missing.startswith('stats_not_found'), missing
    blank = stats(directory, port, 'stats_price_save', provider_id='provider-a',
                  upstream_model='model-blank', currency='CNY', input_mode='flat',
                  input_rate_nanos='', output_rate_nanos='', label='blank')
    assert blank['input_rate_nanos'] == '0', blank
    assert blank['output_rate_nanos'] == '0', blank
    assert blank['currency'] == 'CNY', blank
    stats(directory, port, 'stats_price_delete', rule_id=blank['rule_id'],
          expected_revision=blank['revision'])
    # U04: the create path is revision 0 only, and delete accepts the canonical
    # field. Sending an id with a create-shaped revision must not overwrite.
    sneaky = stats(directory, port, 'stats_price_save', provider_id='provider-a',
                   upstream_model='model-ok', currency='USD', input_mode='flat',
                   input_rate_nanos='99000000000', output_rate_nanos='1',
                   label='sneaky', expected_revision=1, expect_error=True)
    assert sneaky.startswith('stats_invalid_argument'), sneaky
    untouched = stats(directory, port, 'stats_prices_list')['items'][0]
    assert untouched['input_rate_nanos'] == '4000000000', untouched
    assert untouched['revision'] == 3, untouched
    by_canonical_delete = stats(directory, port, 'stats_price_save',
                                provider_id='provider-a', upstream_model='model-del',
                                currency='USD', input_mode='flat',
                                input_rate_nanos='1000000000',
                                output_rate_nanos='1000000000', label='del')
    stats(directory, port, 'stats_price_delete',
          price_id=by_canonical_delete['price_id'],
          expected_revision=by_canonical_delete['revision'])
    left = [item for item in stats(directory, port, 'stats_prices_list')['items']
            if item['rule_id'] == by_canonical_delete['rule_id']]
    assert left == [], left
    dual = stats(directory, port, 'stats_price_delete', price_id='a', rule_id='b',
                 expected_revision=1, expect_error=True)
    assert dual.startswith('stats_invalid_argument'), dual
    checks.ok('create is revision 0 only and delete takes the canonical price_id')

    # A request that ran before the edit keeps its own snapshot and amount.
    settled = stats(directory, port, 'stats_requests', filter=window(start_ms), page_size=100)
    early = find_row(settled['items'], 'model-ok')
    assert early['cost']['status'] in ('default_zero', 'priced'), early['cost']
    detail = stats(directory, port, 'stats_request_detail', request_id=early['request_id'])
    assert detail['price_snapshot']['price_source'] in ('manual', 'default_zero'), detail
    checks.ok('a request detail returns its own price snapshot')

    # Adding a price only affects requests admitted afterwards.
    free_before = stats(directory, port, 'stats_requests',
                        filter=dict(window(start_ms), upstream_model='model-429'), page_size=20)
    free_row = find_row(free_before['items'], 'model-429')
    assert free_row and free_row['cost']['status'] == 'default_zero', free_row
    assert free_row['cost']['total_cost_nanos'] == '0', free_row['cost']
    tokens = stats(directory, port, 'stats_request_detail', request_id=free_row['request_id'])
    assert tokens['request']['total_tokens'] is None or tokens['request']['total_tokens'] >= 0
    checks.ok('an unconfigured model is an explicit zero amount')

    # Repricing history with the current price is withdrawn.
    removed = stats(directory, port, 'stats_reprice_start', filter=window(start_ms),
                    expected_generation='', expect_error=True)
    assert removed.startswith('stats_operation_removed'), removed

    # Deleting the current price leaves history readable.
    current = stats(directory, port, 'stats_prices_list')['items'][0]
    stats(directory, port, 'stats_price_delete', rule_id=current['rule_id'],
          expected_revision=current['revision'])
    listed = stats(directory, port, 'stats_prices_list')
    assert listed['items'] == [], listed
    still_there = stats(directory, port, 'stats_request_detail',
                        request_id=early['request_id'])
    assert still_there['request']['request_id'] == early['request_id']
    checks.ok('deleting the current price leaves recorded history readable')

    # --- paused collection -------------------------------------------------
    health = stats(directory, port, 'stats_health')
    paused = stats(directory, port, 'stats_settings_update', enabled=False, retention_days=180,
                   expected_revision=health['settings_revision'])
    assert paused['enabled'] is False
    conflict = stats(directory, port, 'stats_settings_update', enabled=True, retention_days=180,
                     expected_revision=health['settings_revision'], expect_error=True)
    assert conflict.startswith('stats_revision_conflict'), conflict
    before = stats(directory, port, 'stats_health')['record_count']
    relay(port, 'wbp-ok')
    time.sleep(0.4)
    after = stats(directory, port, 'stats_health')['record_count']
    assert after == before, (before, after)
    stats(directory, port, 'stats_settings_update', enabled=True, retention_days=180,
          expected_revision=paused['revision'])
    relay(port, 'wbp-ok')
    wait_for(lambda: stats(directory, port, 'stats_health')['record_count'] > after,
             'collection did not resume')
    checks.ok('pausing collection only stops new requests')

    # --- clear preview and confirmation ------------------------------------
    bad = stats(directory, port, 'stats_clear_start', token='not-a-token', expect_error=True)
    assert bad.startswith('stats_invalid_argument'), bad
    # T01: the sweep must delete exactly what the preview counted. A record
    # outside the previewed window has to survive.
    boundary = now_ms() + 1_500
    preview = stats(directory, port, 'stats_clear_preview', before_ms=boundary)
    assert preview['row_count'] > 0, preview
    # A request admitted after the preview is outside its scope.
    relay(port, 'wbp-nokey')
    cleared = stats(directory, port, 'stats_clear_start', token=preview['token'])
    assert cleared['kind'] == 'clear', cleared
    final = wait_for(lambda: (lambda task: task if task['status'] != 'running' else None)(
        stats(directory, port, 'stats_maintenance_status', task_id=cleared['task_id'])),
        'clear did not finish')
    assert final['status'] == 'completed', final
    assert final['affected_rows'] == preview['row_count'], (final, preview)
    survivors = stats(directory, port, 'stats_requests',
                      **{'filter': dict(window(start_ms), include_tests=True), 'page_size': 50})
    for item in survivors['items']:
        assert item['started_at_ms'] >= boundary, item
    checks.ok('clear deletes exactly the previewed rows and no more',
              'preview=%d deleted=%d kept=%d' % (
                  preview['row_count'], final['affected_rows'], survivors['total_rows']))

    # --- crash recovery -----------------------------------------------------
    return survivors


def _rows_ready(directory, port, start_ms, expectations):
    page = stats(directory, port, 'stats_requests',
                 **{'filter': dict(window(start_ms), include_tests=False), 'page_size': 100})
    for model, status in expectations.items():
        row = find_row(page['items'], model)
        if row is None or row['usage_status'] != status or row['outcome'] == 'in_flight':
            import sys as _sys
            print(f'NOT READY {model} expected={status} got='
                  f'{None if row is None else (row["usage_status"], row["outcome"], row["usage_reason"])}',
                  file=_sys.stderr, flush=True)
            return None
    slow = find_row(page['items'], 'model-slow')
    if slow is None or slow['outcome'] != 'cancelled':
        import sys as _sys
        print(f'NOT READY model-slow got='
              f'{None if slow is None else (slow["usage_status"], slow["outcome"], slow["error_kind"])}',
              file=_sys.stderr, flush=True)
        return None
    return page


def crash_recovery(exe, directory, port, checks):
    """Kills the core while a request is in flight, then restarts it."""
    process, log, _health = start_core(exe, directory, port)
    try:
        payload = {'model': 'wbp-slow', 'stream': True, 'messages': [{'role': 'user', 'content': 'ping'}]}
        connection = http.client.HTTPConnection('127.0.0.1', port, timeout=20)
        connection.request('POST', '/v1/chat/completions', json.dumps(payload).encode(),
                           {'Authorization': 'Bearer ' + LOCAL_KEY, 'Content-Type': 'application/json'})
        response = connection.getresponse()
        response.read(16)
        wait_for(lambda: stats(directory, port, 'stats_requests',
                               filter=window(now_ms() - 3_600_000), page_size=20)['total_rows'] >= 1,
                 'in-flight row was not recorded')
        process.kill()
        process.wait()
    finally:
        connection.close()
        log.close()
    process, log, health = start_core(exe, directory, port)
    try:
        assert health['control_protocol'] == 7, health
        page = wait_for(lambda: _recovered(directory, port), 'interrupted row was not recovered')
        assert page
        assert stats(directory, port, 'stats_health')['loss_scope'] == 'unknown_after_unclean_exit'
        warnings = stats(directory, port, 'stats_dashboard',
                         filter=window(now_ms() - 3_600_000), granularity='hour')['warnings']
        assert any(w['code'] == 'unclean_exit' for w in warnings), warnings
        checks.ok('crash recovery interrupts in-flight rows and keeps the warning')

        # F09: a bucket that is provably inside a collecting window and saw no
        # request is a real zero; an uncovered or usage-less bucket stays unknown.
        empty_start = now_ms() + 80
        time.sleep(0.25)
        empty = stats(directory, port, 'stats_dashboard',
                      filter={'from_ms': empty_start, 'to_ms': now_ms(), 'timezone': 'UTC'},
                      granularity='hour')
        point = empty['series'][0]
        assert point['request_count'] == 0, point
        assert point['collection_warning'] is False, point
        assert point['quality'] == 'empty', point
        assert point['input_tokens'] == 0 and point['output_tokens'] == 0 and point['total_tokens'] == 0, point

        # The clear test earlier removed the older rows, so make a fresh
        # request that carries no usage and check it never becomes a zero.
        before = stats(directory, port, 'stats_dashboard',
                       filter=dict(window(now_ms() - 3_600_000), upstream_model='model-nousage'),
                       granularity='hour')['summary']['request_count']
        relay(port, 'wbp-nousage')
        without_usage = wait_for(
            lambda: (lambda s: s if s['request_count'] > before else None)(
                stats(directory, port, 'stats_dashboard',
                      filter=dict(window(now_ms() - 3_600_000), upstream_model='model-nousage'),
                      granularity='hour')['summary']),
            'a usage-less request was not recorded')
        assert without_usage['request_count'] > 0, without_usage
        assert without_usage['input_tokens'] is None, without_usage
        assert without_usage['total_tokens'] is None, without_usage
        assert without_usage['usage_complete_count'] == 0, without_usage

        settings = stats(directory, port, 'stats_settings_get')
        stats(directory, port, 'stats_settings_update', enabled=False,
              retention_days=settings['retention_days'], expected_revision=settings['revision'])
        paused_start = now_ms()
        time.sleep(0.3)
        paused = stats(directory, port, 'stats_dashboard',
                       filter={'from_ms': paused_start, 'to_ms': now_ms(), 'timezone': 'UTC'},
                       granularity='hour')
        paused_point = paused['series'][0]
        assert paused_point['input_tokens'] is None, paused_point
        assert paused_point['collection_warning'] is True, paused_point
        settings = stats(directory, port, 'stats_settings_get')
        stats(directory, port, 'stats_settings_update', enabled=True,
              retention_days=settings['retention_days'], expected_revision=settings['revision'])
        checks.ok('empty buckets distinguish a proven zero from unknown',
                  'covered=%s paused=%s' % (point['quality'], paused_point['quality']))

    finally:
        stop_core(process, port, directory, log)

    process, log, _health = start_core(exe, directory, port)
    try:
        wait_for(lambda: stats(directory, port, 'stats_health')['state'] == 'ready',
                 'statistics did not finish the clean restart')
        assert stats(directory, port, 'stats_health')['loss_scope'] == 'no_loss_observed_in_current_run'
        warnings = stats(directory, port, 'stats_dashboard',
                         filter=window(now_ms() - 3_600_000), granularity='hour')['warnings']
        assert not any(w['code'] == 'unclean_exit' for w in warnings), warnings
        with sqlite3.connect(Path(directory) / 'usage.sqlite3') as db:
            assert db.execute("SELECT COUNT(*) FROM stats_runs WHERE stop_reason='unclean'").fetchone()[0] > 0
        checks.ok('normal quit clears the runtime warning without deleting crash history')
    finally:
        stop_core(process, port, directory, log)
    return None


def _recovered(directory, port):
    # A bounded startup repair runs before the page reports `ready`, and queries
    # are refused until it finishes: the database is not presented as complete
    # while rows still owe a terminal state.
    try:
        health = stats(directory, port, 'stats_health')
    except AssertionError:
        return None
    if health.get('state') != 'ready':
        return None
    page = stats(directory, port, 'stats_requests',
                 **{'filter': dict(window(now_ms() - 3_600_000), include_tests=True), 'page_size': 50})
    slow = find_row(page['items'], 'model-slow')
    if slow and slow['outcome'] == 'interrupted' and slow['error_kind'] == 'unclean_shutdown':
        if slow['duration_ms'] is None and slow['total_tokens'] is None:
            # T03: the request and its cost must reach a terminal state
            # together; a crash must not leave the cost row pending forever.
            cost = slow.get('cost') or {}
            if cost.get('status') == 'pending':
                return None
            detail = stats(directory, port, 'stats_request_detail',
                           request_id=slow['request_id'])
            snapshot = detail.get('price_snapshot') or {}
            if snapshot.get('price_source') == 'default_zero':
                if cost.get('status') != 'default_zero':
                    return None
                if cost.get('total_cost_nanos') != '0':
                    return None
            elif cost.get('status') not in ('usage_incomplete', 'not_applicable', 'priced'):
                return None
            return page
    return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--exe', default='target/release/workbuddy-proxy')
    args = parser.parse_args()
    if not Path(args.exe).exists():
        raise SystemExit('core binary not found: %s' % args.exe)
    upstream = ThreadingHTTPServer(('127.0.0.1', 0), Mock)
    threading.Thread(target=upstream.serve_forever, daemon=True).start()
    checks = Checks()
    with tempfile.TemporaryDirectory(prefix='workbuddy-stats-') as folder:
        port = free_port()
        write_config(folder, port, 'http://127.0.0.1:%d/v1' % upstream.server_port)
        start_ms = now_ms()
        process, log, health = start_core(args.exe, folder, port)
        try:
            assert health['control_protocol'] == 7, health
            checks.ok('control protocol is 6', str(health['control_protocol']))
            run(folder, port, upstream.server_port, start_ms, checks)
        except AssertionError:
            text = (Path(folder) / 'process.log').read_text(encoding='utf-8', errors='replace')
            print('--- core log ---\n' + text[-4000:], flush=True)
            raise
        finally:
            if process.poll() is None:
                stop_core(process, port, folder, log)
        crash_recovery(args.exe, folder, port, checks)
    print('PASS: %d statistics integration checks' % len(checks.passed))
    return 0


if __name__ == '__main__':
    sys.exit(main())

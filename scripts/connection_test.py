#!/usr/bin/env python3
"""Offline detailed diagnostics, error boundaries and single-owner journaling."""
import argparse
from http.server import BaseHTTPRequestHandler
import json
from pathlib import Path
import tempfile
import threading
import time
import urllib.error
import urllib.request
from smoke import ThreadingHTTPServer, start, stop

class DiagnosticUpstream(BaseHTTPRequestHandler):
    def log_message(self, *args): pass
    def do_POST(self):
        payload = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        assert payload['stream'] and payload['max_tokens'] == 1
        mode = self.path.split('/')[1]
        status = int(mode) if mode.isdigit() else 200
        if status != 200:
            body = b'{"error":"fixture-upstream-key fixture-secret-body"}'
        elif mode == 'invalid':
            body = b'not an SSE response: fixture-secret-body'
        elif mode == 'stream-error':
            body = b'data: {"error":{"message":"fixture-secret-body"}}\n\n'
        elif mode == 'empty':
            body = b'data: {"choices":[]}\n\ndata: [DONE]\n\n'
        else:
            event = {"model": self.headers.get("Authorization", "").removeprefix("Bearer "), "choices": [{"delta": {"role": "assistant"}}]}
            body = ("data: " + json.dumps(event) + "\n\n").encode()
        self.send_response(status)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        time.sleep(.02)
        self.wfile.write(body)

def main():
    parser = argparse.ArgumentParser(); parser.add_argument('--exe', required=True); args = parser.parse_args()
    server = ThreadingHTTPServer(('127.0.0.1', 0), DiagnosticUpstream)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{server.server_port}'
    with tempfile.TemporaryDirectory(prefix='workbuddy-diagnostics-') as directory:
        process, port, log = start(args.exe, directory, base + '/v1')
        try:
            instance = json.loads(Path(directory, 'instance.json').read_text(encoding='utf-8'))
            def action(payload):
                req = urllib.request.Request(f'http://127.0.0.1:{port}/_control/action', data=json.dumps(payload).encode(), headers={'Authorization': 'Bearer ' + instance['management_key'], 'Content-Type': 'application/json'})
                try:
                    with urllib.request.urlopen(req, timeout=5) as response: return response.status, json.load(response)['message']
                except urllib.error.HTTPError as error: return error.code, json.load(error)['message']
            before = len(json.loads(action({'command': 'snapshot'})[1])['logs'])
            status, message = action({'command': 'test'})
            report = json.loads(message)
            assert status == 200 and report['kind'] == 'connection_test' and report['http'] == 'HTTP 200'
            assert report['url'] == base + '/v1/chat/completions'
            assert report['model'] == 'fixture-model' and report['provider'] and report['name']
            assert 0 <= report['headers_ms'] <= report['first_event_ms'] <= report['elapsed_ms']
            assert report['reason'] == '' and report['response_model'] == '[redacted]'
            snapshot = json.loads(action({'command': 'snapshot'})[1])
            assert len(snapshot['logs']) == before + 1
            assert all(row['time'] > 1_700_000_000_000 for row in snapshot['logs'])
            assert [row['time'] for row in snapshot['logs']] == sorted(row['time'] for row in snapshot['logs'])
            before = len(snapshot['logs'])
            assert action({'command': 'test', 'ui': True})[0] == 200
            assert len(json.loads(action({'command': 'snapshot'})[1])['logs']) == before, 'UI-owned tests were duplicated in the core journal'
            # The asynchronous native reporter writes exactly one canonical core entry.
            assert action({'command': 'record', 'message': 'UI test result', 'ui': True})[0] == 200
            entries = json.loads(action({'command': 'snapshot'})[1])['logs']
            assert len(entries) == before + 1 and sum(row['message'] == 'UI test result' for row in entries) == 1
            for mode in ['401', '403', '404', '429', '503', 'invalid', 'stream-error', 'empty']:
                assert action({'command': 'save', 'upstream': base + '/' + mode})[0] == 200
                status, message = action({'command': 'test'})
                report = json.loads(message)
                assert status == 400 and report['reason'] and report['headers_ms'] is not None and report['first_event_ms'] is None
                assert report['http'] == 'HTTP ' + (mode if mode.isdigit() else '200')
                assert 'fixture-secret-body' not in message and 'fixture-upstream-key' not in message
            assert action({'command': 'save', 'upstream': 'http://127.0.0.1:1/v1'})[0] == 200
            status, message = action({'command': 'test'})
            report = json.loads(message)
            assert status == 400 and report['http'] == '' and report['headers_ms'] is None and report['reason']
            text = action({'command': 'snapshot'})[1]
            assert 'fixture-secret-body' not in text and 'fixture-upstream-key' not in text and instance['management_key'] not in text
            print('PASS: detailed success timings, first valid SSE, empty/error/malformed SSE, HTTP 401/403/404/429/503, network failure, redacted metadata, chronological journal and no duplicate UI-owned entries')
        finally: stop(process, port, directory, log)
    server.shutdown(); server.server_close()

if __name__ == '__main__': main()

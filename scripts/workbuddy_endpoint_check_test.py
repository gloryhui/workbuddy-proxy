#!/usr/bin/env python3
"""Read-only startup check and repair/port integration, using a private fake HOME."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from statistics_test import action, free_port, stop_core, write_config


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--exe', required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='wbp-endpoint-check-') as folder:
        root = Path(folder)
        profile = root / 'profile'
        profile.mkdir()
        home = root / 'home'
        models = home / '.workbuddy' / 'models.json'
        models.parent.mkdir(parents=True)

        class Upstream(BaseHTTPRequestHandler):
            def log_message(self, *args): pass
            def do_GET(self):
                body = b'{"data":[]}'
                self.send_response(200)
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)

        upstream = ThreadingHTTPServer(('127.0.0.1', 0), Upstream)
        threading.Thread(target=upstream.serve_forever, daemon=True).start()
        configured_port, actual_port = free_port(), free_port()
        while configured_port == actual_port:
            actual_port = free_port()
        write_config(profile, configured_port, f'http://127.0.0.1:{upstream.server_port}/v1')
        env = dict(os.environ, HOME=str(home), USERPROFILE=str(home))
        log = (profile / 'process.log').open('ab')
        core = subprocess.Popen([str(Path(args.exe).resolve()), '--headless', '--config-dir', str(profile), '--port', str(actual_port)], env=env, stdout=log, stderr=log)
        try:
            for _ in range(400):
                assert core.poll() is None, 'Core exited'
                try:
                    with urllib.request.urlopen(f'http://127.0.0.1:{actual_port}/health', timeout=.5) as response:
                        assert json.load(response)['control_protocol'] == 8
                    break
                except OSError:
                    time.sleep(.025)
            else:
                raise AssertionError('Startup timed out')
            check = lambda: action(profile, actual_port, 'workbuddy_check')
            assert check()['checked_count'] == 0 and not models.exists()
            cfg = json.loads((profile / 'config.json').read_text())
            route = cfg['providers'][0]['models'][0]['id']
            managed = {'id': 'old-synced-model', 'vendor': 'Custom', 'url': f'http://192.0.2.1:{configured_port}/r/{route}/v1', 'apiKey': cfg['local_key'], '_workbuddyProxy': {'managed': True, 'route': route}}
            unrelated = {'id': 'user-owned', 'vendor': 'Custom', 'url': 'https://manual.example/v1', 'apiKey': 'private-fixture-key'}
            models.write_text(json.dumps([managed, unrelated]))
            before = models.read_bytes()
            cfg_before = (profile / 'config.json').read_bytes()
            report = check()
            assert report['expected_address'] == f'http://127.0.0.1:{actual_port}', report
            assert report['checked_count'] == report['mismatch_count'] == 1, report
            assert report['actual_addresses'] == [f'http://192.0.2.1:{configured_port}'], report
            assert models.read_bytes() == before and (profile / 'config.json').read_bytes() == cfg_before
            assert not list(models.parent.glob('*.bak'))
            action(profile, actual_port, 'install')
            assert check()['mismatch_count'] == 0
            assert unrelated in json.loads(models.read_text())
            assert list(models.parent.glob('*.bak'))
            next_port = free_port()
            while next_port == actual_port:
                next_port = free_port()
            instance = json.loads((profile / 'instance.json').read_text())
            request = urllib.request.Request(f'http://127.0.0.1:{actual_port}/_control/action',
                data=json.dumps({'command': 'set_port', 'port': next_port}).encode(),
                headers={'Authorization': 'Bearer ' + instance['management_key'], 'Content-Type': 'application/json'})
            with urllib.request.urlopen(request, timeout=10) as response:
                assert json.loads(json.load(response)['message'])['port'] == next_port
            actual_port = next_port
            assert check()['mismatch_count'] == 0
            assert json.loads((profile / 'config.json').read_text())['port'] == next_port
            assert unrelated in json.loads(models.read_text())
            snapshot = json.dumps(action(profile, actual_port, 'snapshot'))
            assert 'private-fixture-key' not in snapshot
            print('PASS: absent file unchanged, actual listener override, managed-only IP/port mismatch, read-only check, repair backup, unrelated entries preserved, port rebind addresses')
        finally:
            stop_core(core, actual_port, profile, log)
            upstream.shutdown()
            upstream.server_close()


if __name__ == '__main__':
    main()

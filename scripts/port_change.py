#!/usr/bin/env python3
"""Real socket migration with occupied ports, WorkBuddy entries and live SSE."""
import argparse
import http.client
from http.server import BaseHTTPRequestHandler
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request
from smoke import ThreadingHTTPServer, free_port

FIRST = b'data: {"choices":[{"delta":{"content":"first"}}]}\n\n'
LAST = b'data: {"choices":[{"delta":{"content":"last"}}]}\n\ndata: [DONE]\n\n'
RELEASE = threading.Event()


class Upstream(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        assert body['model'] == 'fixture-model'
        assert self.headers['Authorization'] == 'Bearer fixture-upstream-key'
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Content-Length', str(len(FIRST) + len(LAST)))
        self.end_headers()
        self.wfile.write(FIRST)
        self.wfile.flush()
        if body.get('slow'):
            assert RELEASE.wait(20)
        self.wfile.write(LAST)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--exe', required=True)
    args = parser.parse_args()
    server = ThreadingHTTPServer(('127.0.0.1', 0), Upstream)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    with tempfile.TemporaryDirectory(prefix='workbuddy-port-change-') as directory:
        root = Path(directory)
        home = root / 'home'
        home.mkdir()
        models_file = home / '.workbuddy/models.json'
        models_file.parent.mkdir()
        old_port = free_port()
        config = {'local_key': 'fixture-local-key', 'port': old_port,
                  'api_key': 'fixture-upstream-key', 'model': 'fixture-model',
                  'upstream': f'http://127.0.0.1:{server.server_port}/v1', 'installed': None}
        config_file = root / 'config.json'
        instance_file = root / 'instance.json'
        config_file.write_text(json.dumps(config))
        env = dict(os.environ)
        env['USERPROFILE' if os.name == 'nt' else 'HOME'] = str(home)
        command = [str(Path(args.exe).resolve()), '--headless', '--config-dir', str(root)]
        process = subprocess.Popen(command, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        live = None
        try:
            def wait_ready():
                for _ in range(300):
                    if process.poll() is not None:
                        raise AssertionError('Core exited during port migration test')
                    try:
                        current = json.loads(instance_file.read_text())
                        with urllib.request.urlopen(f"http://127.0.0.1:{current['port']}/health", timeout=.3) as response:
                            assert json.load(response)['ready']
                        return current
                    except (OSError, ValueError):
                        time.sleep(.025)
                raise AssertionError('Core did not become ready')

            instance = wait_ready()
            port = old_port

            def action(payload):
                request = urllib.request.Request(f'http://127.0.0.1:{port}/_control/action',
                    data=json.dumps(payload).encode(), headers={'Content-Type': 'application/json', 'Authorization': 'Bearer ' + instance['management_key']})
                try:
                    with urllib.request.urlopen(request, timeout=8) as response:
                        return response.status, json.load(response)['message']
                except urllib.error.HTTPError as error:
                    return error.code, json.load(error)['message']

            assert action({'command': 'install'})[0] == 200
            owned = json.loads(models_file.read_text())[0]
            manual = {'id': 'manual', 'name': 'Keep name', 'url': f'http://localhost:{old_port}/v1',
                      'apiKey': 'fixture-local-key', 'reasoning': {'defaultEffort': 'max'}, 'extra': 7}
            other = {'id': 'other', 'name': 'Other vendor', 'url': f'http://127.0.0.1:{old_port}/v1', 'apiKey': 'other-key'}
            models_file.write_text(json.dumps({'custom': 'keep-root', 'models': [owned, manual, other]}))
            before = [p.read_bytes() for p in [config_file, instance_file, models_file]]
            with socket.socket() as occupied:
                occupied.bind(('127.0.0.1', 0))
                occupied.listen(1)
                assert action({'command': 'set_port', 'port': occupied.getsockname()[1]})[0] == 400
            for invalid in [0, -1, 65536, 'not-a-port']:
                assert action({'command': 'set_port', 'port': invalid})[0] == 400
            assert [p.read_bytes() for p in [config_file, instance_file, models_file]] == before
            status, message = action({'command': 'set_port', 'port': old_port})
            assert status == 200 and not json.loads(message)['changed']
            assert [p.read_bytes() for p in [config_file, instance_file, models_file]] == before
            live = http.client.HTTPConnection('127.0.0.1', old_port, timeout=8)
            live.request('POST', '/v1/chat/completions', json.dumps({'model': owned['id'], 'messages': [], 'stream': True, 'slow': True}),
                         {'Authorization': 'Bearer fixture-local-key', 'Content-Type': 'application/json'})
            response = live.getresponse()
            assert response.status == 200 and response.read(len(FIRST)) == FIRST
            new_port = free_port()
            status, message = action({'command': 'set_port', 'port': new_port})
            assert status == 200, message
            assert json.loads(message) == {'port': new_port, 'updated': 2, 'changed': True}
            port = new_port
            changed = json.loads(instance_file.read_text())
            assert changed == {**instance, 'port': new_port}
            assert json.loads(config_file.read_text())['port'] == new_port
            doc = json.loads(models_file.read_text())
            assert doc['custom'] == 'keep-root'
            assert doc['models'][0] == {**owned, 'url': owned['url'].replace(f':{old_port}/', f':{new_port}/')}
            assert doc['models'][1] == {**manual, 'url': manual['url'].replace(f':{old_port}/', f':{new_port}/')}
            assert doc['models'][2] == other
            assert list(models_file.parent.glob('models.json.workbuddy-proxy-*.bak'))
            RELEASE.set()
            assert response.read() == LAST, 'An in-flight stream was interrupted by the port change'
            live.close()
            live = None
            with urllib.request.urlopen(f'http://127.0.0.1:{port}/health', timeout=3) as current:
                assert json.load(current)['ready']
            try:
                with socket.create_connection(('127.0.0.1', old_port), timeout=.5):
                    raise AssertionError('The old listener is still accepting new connections')
            except OSError:
                pass
            # Invalid WorkBuddy JSON must reject the migration before any setting changes.
            valid = models_file.read_bytes()
            models_file.write_bytes(b'not-json')
            config_before, metadata_before = config_file.read_bytes(), instance_file.read_bytes()
            assert action({'command': 'set_port', 'port': free_port()})[0] == 400
            assert config_file.read_bytes() == config_before and instance_file.read_bytes() == metadata_before
            assert models_file.read_bytes() == b'not-json'
            models_file.write_bytes(valid)
            assert action({'command': 'restore'})[0] == 200, 'Expected-entry tracking was not migrated'
            assert [entry['id'] for entry in json.loads(models_file.read_text())['models']] == ['manual', 'other']
            request = urllib.request.Request(f'http://127.0.0.1:{port}/_control/stop', data=b'', headers={'Authorization': 'Bearer ' + instance['management_key']})
            with urllib.request.urlopen(request, timeout=3) as current:
                current.read()
            assert process.wait(timeout=5) == 0
            process = subprocess.Popen(command, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            restarted = wait_ready()
            assert restarted['port'] == new_port
            instance = restarted
            provider = json.loads(config_file.read_text())['providers'][0]['id']
            assert action({'command': 'delete', 'provider_id': provider})[0] == 200
            empty_port = free_port()
            status, message = action({'command': 'set_port', 'port': empty_port})
            assert status == 200 and json.loads(message)['port'] == empty_port
            port = empty_port
            assert json.loads(config_file.read_text())['providers'] == []
            print('PASS: occupied/invalid ports keep all files and old listener; same-port no-op; same core and live SSE survive rebinding; owned/manual WorkBuddy URLs migrate without changing other entries; malformed configuration is rejected; restore and persisted restart work; empty-provider migration works')
        finally:
            RELEASE.set()
            if live:
                live.close()
            if process.poll() is None:
                try:
                    current = json.loads(instance_file.read_text())
                    request = urllib.request.Request(f"http://127.0.0.1:{current['port']}/_control/stop", data=b'', headers={'Authorization': 'Bearer ' + current['management_key']})
                    with urllib.request.urlopen(request, timeout=3) as response:
                        response.read()
                    process.wait(timeout=5)
                finally:
                    if process.poll() is None:
                        process.kill()
                        process.wait(timeout=5)
    server.shutdown()
    server.server_close()


if __name__ == '__main__':
    main()

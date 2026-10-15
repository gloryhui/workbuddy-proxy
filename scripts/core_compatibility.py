#!/usr/bin/env python3
"""Replace an incompatible same-version core before opening native settings."""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import time
import tomllib
import urllib.request


def check(exe, version, protocol):
    with tempfile.TemporaryDirectory(prefix='workbuddy-core-compat-') as directory:
        root = Path(directory)
        stopped = threading.Event()

        class LegacyCore(BaseHTTPRequestHandler):
            protocol_version = 'HTTP/1.1'

            def setup(self):
                super().setup()
                self.connection.settimeout(2)

            def log_message(self, *args):
                pass

            def handle(self):
                try:
                    super().handle()
                except (ConnectionResetError, BrokenPipeError):
                    # Startup probes may discard a response and close keep-alive.
                    pass

            def reply(self, value, status=200):
                body = json.dumps(value).encode()
                self.send_response(status)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def do_GET(self):
                assert self.path == '/health', self.path
                data = {'app': 'workbuddy-proxy', 'version': version, 'ready': True}
                if protocol is not None:
                    data['control_protocol'] = protocol
                self.reply(data)

            def do_POST(self):
                self.rfile.read(int(self.headers.get('Content-Length', '0')))
                assert self.headers.get('Authorization') == 'Bearer fixture-management'
                if self.path == '/_control/ping':
                    self.reply({'message': 'OK'})
                elif self.path == '/_control/stop':
                    self.reply({'message': 'OK'})
                    stopped.set()
                else:
                    self.reply({'message': 'Unknown command'}, 400)

        server = ThreadingHTTPServer(('127.0.0.1', 0), LegacyCore)
        server.timeout = .1
        port = server.server_port
        instance_path = root / 'instance.json'
        instance_path.write_text(json.dumps({'pid': os.getpid(), 'port': port,
                                             'management_key': 'fixture-management'}))
        config = {'api_key': 'fixture-upstream', 'local_key': 'fixture-local',
                  'upstream': 'http://127.0.0.1:1/v1', 'model': 'fixture-model',
                  'port': port, 'installed': None}
        (root / 'config.json').write_text(json.dumps(config))
        current_version = tomllib.loads(Path('Cargo.toml').read_text())['package']['version']
        (root / 'update-cache.json').write_text(json.dumps({
            'tag': 'v' + current_version, 'checked_at': int(time.time())}))

        def serve():
            try:
                while not stopped.is_set():
                    server.handle_request()
            finally:
                server.server_close()
                instance_path.unlink(missing_ok=True)

        thread = threading.Thread(target=serve, daemon=True)
        thread.start()
        env = os.environ.copy()
        home = root / 'home'
        home.mkdir()
        env['HOME'] = env['USERPROFILE'] = str(home)
        process = subprocess.Popen([exe, '--config-dir', directory], env=env,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        running = None

        def action(payload):
            request = urllib.request.Request(
                f'http://127.0.0.1:{running["port"]}/_control/action',
                data=json.dumps(payload).encode(),
                headers={'Authorization': 'Bearer ' + running['management_key'],
                         'Content-Type': 'application/json'})
            with urllib.request.urlopen(request, timeout=3) as response:
                return json.load(response)['message']

        try:
            for _ in range(400):
                assert process.poll() is None, 'Native controller exited during core replacement'
                try:
                    candidate = json.loads(instance_path.read_text())
                    frontend = json.loads((root / 'frontend.json').read_text())
                    if candidate['pid'] != os.getpid() and frontend['pid'] == process.pid:
                        running = candidate
                        break
                except (OSError, ValueError):
                    pass
                time.sleep(.025)
            else:
                raise AssertionError('Same-version legacy core was reused instead of replaced')
            assert stopped.is_set(), 'Legacy core did not receive a graceful stop'
            thread.join(timeout=2)
            assert not thread.is_alive()
            assert running['port'] == port, 'Core replacement changed the configured port'
            with urllib.request.urlopen(f'http://127.0.0.1:{port}/health', timeout=3) as response:
                health = json.load(response)
            assert health['version'] == current_version and health['control_protocol'] == 3
            saved = json.loads((root / 'config.json').read_text())
            assert saved['providers'][0]['models'][0]['model'] == 'fixture-model'
            assert saved['providers'][0]['api_key'] == 'fixture-upstream'
            action({'command': 'record', 'message': 'fixture compatibility log'})
            assert json.loads(action({'command': 'snapshot'}))['logs']
            assert action({'command': 'clear_logs'}) == 'Logs cleared / 日志已清空'
            assert json.loads(action({'command': 'snapshot'}))['logs'] == []
        finally:
            if running is None:
                try:
                    candidate = json.loads(instance_path.read_text())
                    if candidate['pid'] != os.getpid():
                        running = candidate
                except (OSError, ValueError):
                    pass
            if running:
                request = urllib.request.Request(
                    f'http://127.0.0.1:{running["port"]}/_control/stop', data=b'',
                    headers={'Authorization': 'Bearer ' + running['management_key']})
                with urllib.request.urlopen(request, timeout=3) as response:
                    response.read()
                for _ in range(100):
                    if not instance_path.exists():
                        break
                    time.sleep(.025)
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=5)
            stopped.set()
            thread.join(timeout=2)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--exe', required=True)
    args = parser.parse_args()
    exe = str(Path(args.exe).resolve())
    version = tomllib.loads(Path('Cargo.toml').read_text())['package']['version']
    for legacy_version, protocol in [(version, None), (version, 0), (version, 1), (version, 2), ('0.3.0', 1)]:
        check(exe, legacy_version, protocol)
    print('PASS: same-version cores without/currently incompatible protocol and older-version cores '
          'are gracefully replaced; supplier configuration and port survive; new log clear works')


if __name__ == '__main__':
    main()

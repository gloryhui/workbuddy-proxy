#!/usr/bin/env python3
"""Automatic metadata discovery, precedence, sync and offline recovery. Mock only."""
import argparse
from http.server import BaseHTTPRequestHandler
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request
from smoke import ThreadingHTTPServer, free_port


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--exe', required=True)
    exe = Path(parser.parse_args().exe).resolve()

    class Upstream(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_GET(self):
            server.hits.append((self.path, self.headers.get('Authorization')))
            if server.offline:
                self.send_response(503)
                self.send_header('Content-Length', '0')
                self.end_headers()
                return
            if self.path == '/catalog':
                # Catalogue requests must never carry the provider's API key.
                assert self.headers.get('Authorization') is None
                data = {
                    'author': {'models': {'vision-model': {
                        'canonical_model_id': 'author/vision-model',
                        'cost': {'input': 1, 'output': 2},
                        'tool_call': True, 'reasoning': True,
                        'reasoning_options': [{'type': 'effort', 'values': ['low', 'high']}],
                        'modalities': {'input': ['text', 'image']},
                        'limit': {'context': 1000000, 'output': 32000}}}},
                    'hosted-plan': {'api': base + '/plan/v1', 'models': {'vision-model': {
                        'canonical_model_id': 'author/vision-model',
                        'modalities': {'input': ['text']}, 'limit': {'context': 128000}}}},
                }
            elif self.path.endswith('/models'):
                assert self.headers.get('Authorization') in ('Bearer fixture-upstream', 'Bearer draft-secret')
                row = {'id': 'vision-model'}
                if server.enriched or self.path.startswith('/gateway/'):
                    row.update({'modalities': {'input': ['text', 'image']}, 'tool_call': False,
                                'context_length': 64000, 'max_output_tokens': 7777,
                                'supported_reasoning_levels': ['none', 'low', 'high']})
                data = {'data': [row, {'id': 'another-model'}]}
            else:
                raise AssertionError(self.path)
            body = json.dumps(data).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    server = ThreadingHTTPServer(('127.0.0.1', 0), Upstream)
    server.hits = []
    server.offline = False
    server.enriched = False
    base = f'http://127.0.0.1:{server.server_port}'
    threading.Thread(target=server.serve_forever, daemon=True).start()
    with tempfile.TemporaryDirectory(prefix='workbuddy-capabilities-') as folder:
        root = Path(folder)
        data = root / 'data'
        data.mkdir()
        home = root / 'home'
        workbuddy = home / '.workbuddy/models.json'
        workbuddy.parent.mkdir(parents=True)
        unrelated = {'id': 'user-model', 'extra': 'keep'}
        workbuddy.write_text(json.dumps([unrelated]), encoding='utf-8')
        models = [
            {'id': 'auto', 'alias': 'wbp-auto', 'name': 'Automatic', 'model': 'vision-model', 'reasoning_effort': 'default'},
            {'id': 'manual', 'alias': 'wbp-manual', 'name': 'Manual', 'model': 'vision-model',
             'reasoning_effort': 'default', 'capabilities': {'supportsImages': True, 'maxOutputTokens': 1234}},
        ]
        cfg = {'local_key': 'fixture-local', 'port': free_port(), 'providers': [{
            'id': 'plan', 'name': 'Hosted plan', 'api_key': 'fixture-upstream',
            'upstream': base + '/plan/v1', 'models': models}], 'installed': []}
        (data / 'config.json').write_text(json.dumps(cfg), encoding='utf-8')
        env = dict(os.environ, HOME=str(home), USERPROFILE=str(home),
                   WORKBUDDY_PROXY_CATALOG_TEST_URL=base + '/catalog')
        process = None
        instance = None

        def start():
            nonlocal process, instance
            process = subprocess.Popen([str(exe), '--headless', '--config-dir', str(data)], env=env,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            for _ in range(400):
                assert process.poll() is None, 'Core exited'
                try:
                    instance = json.loads((data / 'instance.json').read_text(encoding='utf-8'))
                    return
                except (OSError, ValueError):
                    time.sleep(.025)
            raise AssertionError('Core did not start')

        def stop():
            nonlocal process
            if process is None:
                return
            try:
                request = urllib.request.Request(f'http://127.0.0.1:{instance["port"]}/_control/stop',
                    data=b'', headers={'Authorization': 'Bearer ' + instance['management_key']})
                urllib.request.urlopen(request, timeout=3).close()
                process.wait(timeout=8)
            finally:
                if process.poll() is None:
                    process.terminate()
                    process.wait(timeout=5)
                process = None

        def action(command, **args):
            payload = dict(command=command, provider_id='plan', **args)
            request = urllib.request.Request(f'http://127.0.0.1:{instance["port"]}/_control/action',
                data=json.dumps(payload).encode(), headers={'Content-Type': 'application/json',
                'Authorization': 'Bearer ' + instance['management_key']})
            with urllib.request.urlopen(request, timeout=12) as response:
                return json.load(response)['message']

        def metadata(model='vision-model', upstream=None):
            return json.loads(action('model_metadata', model=model, upstream=upstream or base + '/plan/v1'))['metadata']

        def synced():
            action('install')
            rows = json.loads(workbuddy.read_text(encoding='utf-8'))
            assert unrelated in rows
            return {row['_workbuddyProxy']['route']: row for row in rows if '_workbuddyProxy' in row}

        try:
            start()
            for _ in range(200):
                resolved = metadata()
                if 'models.dev:provider' in resolved['sources']:
                    break
                time.sleep(.025)
            else:
                raise AssertionError('Background catalogue did not load')
            assert resolved['image_known'] and not resolved['capabilities']['supportsImages']
            assert resolved['capabilities']['maxInputTokens'] == 128000
            assert resolved['capabilities']['maxOutputTokens'] == 32000
            assert metadata(upstream=base + '/other/v1')['capabilities']['supportsImages']
            assert not metadata('unlisted-model')['image_known']
            rows = synced()
            assert rows['auto']['supportsImages'] is False
            assert rows['manual']['supportsImages'] is True
            assert rows['manual']['maxOutputTokens'] == 1234

            # Discovery uses the unsaved endpoint/key, without contaminating the
            # saved plan's metadata. The old list-of-IDs API remains compatible.
            assert json.loads(action('fetch', upstream=base + '/gateway/v1', key='draft-secret')) == ['another-model', 'vision-model']
            assert metadata(upstream=base + '/gateway/v1')['capabilities']['supportsImages']
            assert not metadata()['capabilities']['supportsImages']
            server.enriched = True
            action('fetch')
            resolved = metadata()
            assert resolved['sources'][-1] == 'upstream'
            caps = resolved['capabilities']
            assert caps['supportsImages'] and not caps['supportsToolCall']
            assert caps['maxInputTokens'] == 64000 and caps['maxOutputTokens'] == 7777
            assert caps['reasoning']['supportedEfforts'] == ['low', 'high']
            rows = synced()
            assert rows['auto']['supportsImages'] and rows['auto']['maxOutputTokens'] == 7777
            assert rows['manual']['maxOutputTokens'] == 1234

            models[0]['capabilities'] = None
            models[1]['capabilities'] = {'supportsImages': False}
            action('save', models=models, reasoning_effort='keep')
            rows = synced()
            assert rows['auto']['supportsImages'] and not rows['manual']['supportsImages']
            saved = json.loads((data / 'config.json').read_text(encoding='utf-8'))['providers'][0]['models']
            assert saved[0]['capabilities'] is None, 'Automatic values became manual overrides'
            models[1]['capabilities'] = None
            action('save', models=models, reasoning_effort='keep')
            assert synced()['manual']['supportsImages']
            models[0]['model'] = 'brand-new-id'
            action('save', models=models, reasoning_effort='keep')
            assert not synced()['auto']['supportsImages'], 'Renamed model inherited stale capabilities'
            models[0]['model'] = 'vision-model'
            action('save', models=models, reasoning_effort='keep')

            # Pricing uses the same public catalogue. Seed its successful-day
            # marker before the offline restart so an independent startup price
            # fetch cannot be mistaken for an immediate capability retry.
            deadline = time.monotonic() + 10
            while True:
                try:
                    price_sync = json.loads(action('stats_prices_update'))['catalog_sync']
                    break
                except urllib.error.HTTPError as failure:
                    body = failure.read().decode()
                    assert failure.code == 400 and 'stats_busy' in body, body
                    assert time.monotonic() < deadline, 'Price refresh stayed busy'
                    time.sleep(.025)
            assert price_sync['last_success_day'] == time.strftime('%Y-%m-%d')

            cache_path = data / 'model-catalog.json'
            text = cache_path.read_text(encoding='utf-8')
            assert all(secret not in text for secret in ['fixture-upstream', 'draft-secret', instance['management_key']])
            stop()
            # Expire both caches and refuse refreshes: the last good metadata
            # must remain available and failures must be rate-limited.
            cache = json.loads(cache_path.read_text(encoding='utf-8'))
            cache['fetched_at'] = int(time.time()) - 90000
            for item in cache['live'].values():
                item['fetched_at'] = int(time.time()) - 90000
            cache_path.write_text(json.dumps(cache), encoding='utf-8')
            server.offline = True
            start()
            assert synced()['auto']['supportsImages']
            hits = len(server.hits)
            assert synced()['auto']['maxOutputTokens'] == 7777
            assert len(server.hits) == hits, 'Failed lookups were retried immediately'
            stop()
            cache_path.write_text('{', encoding='utf-8')
            start()
            assert metadata('deepseek-flash')['capabilities']['supportsImages']
            assert metadata('unlisted-model')['sources'] == ['unknown']
            assert synced()['auto']['supportsImages'] is False
            action('restore')
            assert json.loads(workbuddy.read_text(encoding='utf-8')) == [unrelated]
            print('PASS: automatic background metadata, provider/plan isolation, canonical fallback, live precedence, '
                  'manual override/reset, rename, WorkBuddy sync, offline cache, failure backoff, corrupt-cache recovery, '
                  'compatible discovery and credential-free public requests/cache')
        finally:
            stop()
    server.shutdown()
    server.server_close()


if __name__ == '__main__':
    main()

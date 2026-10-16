#!/usr/bin/env python3
"""Media passthrough and WorkBuddy capability sync against an isolated mock."""
import argparse
import http.client
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

PNG = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aE1sAAAAASUVORK5CYII='
SSE = b'data: {"choices":[{"delta":{"content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,Zml4dHVyZQ=="}}],"audio":{"data":"AAEC","format":"wav"}}}]}\n\ndata: [DONE]\n\n'
JSON_RESPONSE = b'{ "choices": [{"message":{"role":"assistant","content":[{"type":"text","text":"mock only"},{"type":"image_url","image_url":{"url":"https://example.invalid/out.png"}}]}}] }'


class Upstream(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        body = self.rfile.read(int(self.headers['Content-Length']))
        self.server.received.append((self.path, body))
        streamed = json.loads(body).get('stream', False)
        response = SSE if streamed else JSON_RESPONSE
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream' if streamed else 'application/json')
        self.send_header('Content-Length', str(len(response)))
        self.end_headers()
        self.wfile.write(response)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--exe', required=True)
    exe = Path(parser.parse_args().exe).resolve()
    server = ThreadingHTTPServer(('127.0.0.1', 0), Upstream)
    server.received = []
    threading.Thread(target=server.serve_forever, daemon=True).start()
    with tempfile.TemporaryDirectory(prefix='workbuddy-media-') as folder:
        root = Path(folder)
        data = root / 'data'
        data.mkdir()
        home = root / 'home'
        models_file = home / '.workbuddy/models.json'
        models_file.parent.mkdir(parents=True)
        unrelated = {'id': 'user-model', 'supportsImages': True, 'extra': 'keep'}
        models_file.write_text(json.dumps([unrelated]))
        cfg = {'local_key': 'fixture-local', 'language': 'en', 'port': free_port(),
               'providers': [{'id': 'media-provider', 'name': 'media',
                              'api_key': 'fixture-upstream',
                              'upstream': f'http://127.0.0.1:{server.server_port}/v1',
                              'models': [{'id': 'vision-route', 'alias': 'wbp-vision-route',
                                          'name': 'Vision', 'model': 'deepseek-v4.1-flash',
                                          'reasoning_effort': 'default'},
                                         {'id': 'other-route', 'alias': 'wbp-other-route',
                                          'name': 'Other', 'model': 'custom-media-model',
                                          'reasoning_effort': 'default'}]}], 'installed': []}
        (data / 'config.json').write_text(json.dumps(cfg))
        env = dict(os.environ, HOME=str(home), USERPROFILE=str(home))
        process = subprocess.Popen([str(exe), '--headless', '--config-dir', str(data)],
                                   env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        instance = None
        try:
            for _ in range(400):
                assert process.poll() is None, 'Core exited'
                try:
                    instance = json.loads((data / 'instance.json').read_text())
                    with urllib.request.urlopen(f'http://127.0.0.1:{instance["port"]}/health', timeout=.5) as res:
                        assert json.load(res)['ready']
                    break
                except (OSError, ValueError):
                    time.sleep(.025)
            else:
                raise AssertionError('Core did not start')
            port = instance['port']

            def action(payload, success=True):
                req = urllib.request.Request(f'http://127.0.0.1:{port}/_control/action',
                    data=json.dumps(payload).encode(), headers={
                        'Authorization': 'Bearer ' + instance['management_key'],
                        'Content-Type': 'application/json'})
                try:
                    with urllib.request.urlopen(req, timeout=5) as response:
                        value = json.load(response)['message']
                    assert success, 'Expected invalid configuration to fail'
                    return value
                except urllib.error.HTTPError as error:
                    assert not success and error.code == 400
                    return None

            def entry(route):
                return next(e for e in json.loads(models_file.read_text())
                            if e.get('_workbuddyProxy', {}).get('route') == route)

            action({'command': 'install'})
            # Media byte transparency is checked with usage injection disabled;
            # the default request-mode behavior has its own statistics fixture.
            action({'command': 'stats_provider_settings_update',
                    'provider_id': 'media-provider', 'usage_mode': 'observe',
                    'expected_revision': 0})
            assert entry('vision-route')['supportsImages'] is True
            assert entry('vision-route')['reasoning']['supportedEfforts'] == ['low', 'high', 'max']
            assert entry('other-route')['supportsImages'] is False
            assert 'reasoning' not in entry('other-route')
            custom = {'supportsToolCall': False, 'supportsImages': True,
                      'supportsReasoning': True, 'onlyReasoning': False,
                      'useCustomProtocol': True,
                      'reasoning': {'defaultEffort': 'medium', 'supportedEfforts': ['low', 'medium', 'high'],
                                    'canDisableThinking': True},
                      'maxInputTokens': 200000, 'maxOutputTokens': 16000,
                      'contextWindow': {'defaultLength': 128000, 'supportedLengths': [64000, 128000]}}
            rows = cfg['providers'][0]['models']
            rows[1]['capabilities'] = custom
            action({'command': 'save', 'provider_id': 'media-provider', 'models': rows})
            action({'command': 'install'})
            synced = entry('other-route')
            for key, value in custom.items():
                assert synced[key] == value, key
            assert synced['url'].endswith('/r/other-route/v1/chat/completions')
            # A name/URL save from an older client must not silently erase metadata.
            action({'command': 'save', 'provider_id': 'media-provider',
                    'models': [{k: v for k, v in row.items() if k != 'capabilities'} for row in rows]})
            assert json.loads((data / 'config.json').read_text())['providers'][0]['models'][1]['capabilities'] == custom
            before = (data / 'config.json').read_bytes()
            invalid = json.loads(json.dumps(rows))
            invalid[1]['capabilities']['reasoning']['defaultEffort'] = 'max'
            action({'command': 'save', 'provider_id': 'media-provider', 'models': invalid}, success=False)
            assert (data / 'config.json').read_bytes() == before

            media = [{'type': 'text', 'text': 'synthetic media fixture'},
                     {'type': 'image_url', 'image_url': {'url': PNG, 'detail': 'high'}},
                     {'type': 'image_url', 'image_url': {'url': 'https://example.invalid/input.png'}},
                     {'type': 'video_url', 'video_url': {'url': 'https://example.invalid/video.mp4'}, 'fps': 1},
                     {'type': 'input_audio', 'input_audio': {'data': 'AAEC', 'format': 'wav'}}]
            messages = [{'role': 'user', 'content': media},
                        {'role': 'tool', 'tool_call_id': 'fixture', 'content': [media[1]]}]
            for effort in ['default', 'high']:
                action({'command': 'save', 'provider_id': 'media-provider', 'reasoning_effort': effort})
                action({'command': 'install'})
                for route, model in [('vision-route', 'deepseek-v4.1-flash'), ('other-route', 'custom-media-model')]:
                    for streamed in [False, True]:
                        for caller_model in [entry(route)['id'], model]:
                            payload = json.dumps({'model': caller_model, 'messages': messages,
                                'stream': streamed, 'vendor_media': {'keep': [1, True, None]}},
                                ensure_ascii=False, separators=(',', ':')).encode()
                            endpoint = entry(route)['url'].removeprefix(f'http://127.0.0.1:{port}')
                            if not endpoint.endswith('/chat/completions'):
                                endpoint += '/chat/completions'
                            conn = http.client.HTTPConnection('127.0.0.1', port, timeout=5)
                            try:
                                conn.request('POST', endpoint, payload, {'Authorization': 'Bearer fixture-local', 'Content-Type': 'application/json'})
                                response = conn.getresponse()
                                assert response.status == 200
                                assert response.read() == (SSE if streamed else JSON_RESPONSE)
                            finally:
                                conn.close()
                            path, received = server.received[-1]
                            parsed = json.loads(received)
                            assert path == '/v1/chat/completions'
                            assert parsed['model'] == model and parsed['messages'] == messages
                            assert parsed['vendor_media'] == {'keep': [1, True, None]}
                            if effort == 'default':
                                assert received == payload.replace(json.dumps(caller_model).encode(), json.dumps(model).encode(), 1)
                            else:
                                assert parsed['reasoning_effort'] == 'high'
                                assert json.dumps(messages, ensure_ascii=False, separators=(',', ':')).encode() in received
            # Explicit image opt-out is respected, and null returns to automatic defaults.
            rows[0]['capabilities'] = {'supportsImages': False}
            action({'command': 'save', 'provider_id': 'media-provider', 'models': rows})
            action({'command': 'install'})
            assert entry('vision-route')['supportsImages'] is False
            rows[0]['capabilities'] = None
            action({'command': 'save', 'provider_id': 'media-provider', 'models': rows})
            action({'command': 'install'})
            assert entry('vision-route')['supportsImages'] is True
            rows[1]['capabilities'] = None
            action({'command': 'save', 'provider_id': 'media-provider', 'models': rows})
            action({'command': 'install'})
            reset = entry('other-route')
            assert reset['supportsImages'] is False and reset['useCustomProtocol'] is False
            assert reset['url'].endswith('/r/other-route/v1')
            assert not any(field in reset for field in ['maxInputTokens', 'maxOutputTokens', 'contextWindow', 'reasoning'])
            action({'command': 'restore'})
            assert json.loads(models_file.read_text()) == [unrelated]
            print('PASS: image URL/base64, audio/video JSON and tool media; byte-preserving model rewrite; thinking overrides; SSE/JSON response bytes; per-model capability defaults/overrides/persistence/validation; custom-protocol endpoint; sync/unsync isolation')
        finally:
            if instance:
                req = urllib.request.Request(f'http://127.0.0.1:{instance["port"]}/_control/stop',
                    data=b'', headers={'Authorization': 'Bearer ' + instance['management_key']})
                try:
                    urllib.request.urlopen(req, timeout=3).close()
                except OSError:
                    pass
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            process.stderr.close()
            server.shutdown()
            server.server_close()


if __name__ == '__main__':
    main()

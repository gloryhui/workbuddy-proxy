#!/usr/bin/env python3
"""Offline real-core regression for Anthropic and Azure API-key providers."""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import tempfile
import threading
import urllib.parse
import urllib.request
import urllib.error

import statistics_test as t

KEY = 'synthetic-provider-key'
MODEL = 'my-deployment'


class Vendor(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def reply(self, value, stream=False, status=200):
        raw = value.encode() if isinstance(value, str) else json.dumps(value).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'text/event-stream' if stream else 'application/json')
        self.send_header('Content-Length', str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def check_auth(self):
        path = urllib.parse.urlparse(self.path).path
        azure = '/openai/' in path
        anthropic = '/anthropic/' in path
        assert self.headers.get('api-key') == (KEY if azure else None), dict(self.headers)
        assert self.headers.get('Authorization') == (None if azure or anthropic else 'Bearer ' + KEY)
        assert self.headers.get('x-api-key') == (KEY if anthropic else None)
        assert self.headers.get('x-goog-api-key') is None
        if anthropic:
            assert self.headers.get('anthropic-version') == '2023-06-01'
        self.server.seen.append((self.command, self.path))
        return path

    def do_GET(self):
        try:
            if self.path == '/catalog':
                return self.reply({})
            path = self.check_auth()
            if path.endswith('/deployments'):
                assert urllib.parse.parse_qs(urllib.parse.urlparse(self.path).query) == {'api-version': ['2022-12-01']}
                if path.startswith('/denied/'):
                    return self.reply({}, status=401)
                if path.startswith('/unavailable-'):
                    return self.reply({}, status=int(path.split('/')[1].removeprefix('unavailable-')))
                if path.startswith('/empty/'):
                    return self.reply({'data': []})
                if path.startswith('/invalid/'):
                    return self.reply({'data': [{'model': 'gpt-4.1'}]})
                if path.startswith('/malformed/'):
                    return self.reply('not-json')
            else:
                assert path.endswith('/models'), path
                if '/openai/' in path:
                    return self.reply({'data': [{'id': 'gpt-4.1'}]})
            data = {'data': [{'id': MODEL, 'model': 'gpt-4.1', 'supportsImages': True}], 'has_more': False}
            self.reply(data)
        except (BrokenPipeError, ConnectionResetError):
            pass
        except Exception as error:
            self.server.failures.append(repr(error))
            self.reply({}, status=500)

    def do_POST(self):
        try:
            path = self.check_auth()
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            assert body['model'] == MODEL, body
            stream = body.get('stream', False)
            if path.endswith('/chat/completions'):
                value = {'id': 'chat-fixture', 'model': MODEL, 'object': 'chat.completion',
                         'choices': [{'index': 0, 'message': {'role': 'assistant', 'content': 'OK'}, 'finish_reason': 'stop'}],
                         'usage': {'prompt_tokens': 1, 'completion_tokens': 1, 'total_tokens': 2}}
                if stream:
                    return self.reply('data: ' + json.dumps({'choices': [{'index': 0, 'delta': {'role': 'assistant', 'content': 'OK'}, 'finish_reason': 'stop'}]}) + '\n\ndata: [DONE]\n\n', True)
                return self.reply(value)
            if path.endswith('/messages'):
                value = {'id': 'msg-fixture', 'type': 'message', 'role': 'assistant', 'model': MODEL,
                         'content': [{'type': 'text', 'text': 'OK'}], 'stop_reason': 'end_turn',
                         'usage': {'input_tokens': 1, 'output_tokens': 1}}
                events = [
                    {'type': 'message_start', 'message': dict(value, content=[], stop_reason=None, usage={'input_tokens': 1, 'output_tokens': 0})},
                    {'type': 'content_block_start', 'index': 0, 'content_block': {'type': 'text', 'text': ''}},
                    {'type': 'content_block_delta', 'index': 0, 'delta': {'type': 'text_delta', 'text': 'OK'}},
                    {'type': 'content_block_stop', 'index': 0},
                    {'type': 'message_delta', 'delta': {'stop_reason': 'end_turn'}, 'usage': {'output_tokens': 1}},
                    {'type': 'message_stop'},
                ]
            else:
                assert path.endswith('/responses'), path
                item = {'id': 'message-fixture', 'type': 'message', 'role': 'assistant', 'content': [{'type': 'output_text', 'text': 'OK', 'annotations': []}]}
                value = {'id': 'resp-fixture', 'model': MODEL, 'status': 'completed', 'output': [item],
                         'usage': {'input_tokens': 1, 'output_tokens': 1, 'total_tokens': 2}}
                events = [
                    {'type': 'response.created', 'response': {'id': value['id'], 'model': MODEL}},
                    {'type': 'response.output_item.added', 'output_index': 0, 'item': dict(item, content=[])},
                    {'type': 'response.output_text.delta', 'output_index': 0, 'item_id': item['id'], 'content_index': 0, 'delta': 'OK'},
                    {'type': 'response.output_item.done', 'output_index': 0, 'item': item},
                    {'type': 'response.completed', 'response': value},
                ]
            self.reply(''.join('data: ' + json.dumps(e) + '\n\n' for e in events), True) if stream else self.reply(value)
        except (BrokenPipeError, ConnectionResetError):
            pass  # A connection test stops after its first valid event.
        except Exception as error:
            self.server.failures.append(repr(error))
            self.reply({}, status=500)


def relay(port, model_id, stream):
    request = urllib.request.Request(
        f'http://127.0.0.1:{port}/r/{model_id}/v1/chat/completions',
        data=json.dumps({'model': 'wbp-' + model_id, 'messages': [{'role': 'user', 'content': 'OK?'}], 'stream': stream}).encode(),
        headers={'Authorization': 'Bearer ' + t.LOCAL_KEY, 'Content-Type': 'application/json',
                 'api-key': 'client-key-must-not-leak', 'x-api-key': 'client-key-must-not-leak',
                 'x-goog-api-key': 'client-key-must-not-leak'})
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            raw = response.read().decode()
    except urllib.error.HTTPError as error:
        raise AssertionError((model_id, stream, error.code, error.read().decode())) from error
    if stream:
        assert 'OK' in raw and '[DONE]' in raw, raw
    else:
        assert json.loads(raw)['choices'][0]['message']['content'] == 'OK', raw


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--exe', default='target/debug/workbuddy-proxy')
    args = parser.parse_args()
    server = ThreadingHTTPServer(('127.0.0.1', 0), Vendor)
    server.seen, server.failures = [], []
    threading.Thread(target=server.serve_forever, daemon=True).start()
    origin = f'http://127.0.0.1:{server.server_port}'
    old_catalog = os.environ.get('WORKBUDDY_PROXY_CATALOG_TEST_URL')
    os.environ['WORKBUDDY_PROXY_CATALOG_TEST_URL'] = origin + '/catalog'
    cases = [
        ('anthropic', 'anthropic_messages', '/anthropic/v1', {}),
        ('azure-chat', 'openai_chat', '', {'authentication': 'azure_api_key'}),
        ('azure-responses', 'openai_responses', '/openai/v1', {'authentication': 'azure_api_key'}),
        ('default', 'openai_chat', '/default/v1', {}),
    ]
    try:
        with tempfile.TemporaryDirectory(prefix='wbp-api-key-providers-') as tmp:
            directory = Path(tmp)
            port = t.free_port()
            config = {'local_key': t.LOCAL_KEY, 'port': port, 'providers': [
                {'id': ident, 'name': ident, 'upstream': '', 'models': []} for ident, *_ in cases]}
            (directory / 'config.json').write_text(json.dumps(config))
            process, log, health = t.start_core(args.exe, directory, port)
            model_ids = {}
            try:
                assert health['control_protocol'] == 10, health
                for ident, protocol, path, settings in cases:
                    fields = {'provider_id': ident, 'upstream': origin + path, 'key': KEY,
                              'protocol': protocol, 'protocol_settings': settings}
                    before = (directory / 'config.json').read_bytes()
                    assert t.action(directory, port, 'fetch', **fields) == [MODEL]
                    assert (directory / 'config.json').read_bytes() == before
                    t.action(directory, port, 'save', name=ident,
                             models=[{'id': 'model-' + ident, 'name': MODEL, 'model': MODEL}], **fields)
                    saved_config = json.loads((directory / 'config.json').read_text())
                    model_ids[ident] = next(p for p in saved_config['providers'] if p['id'] == ident)['models'][0]['id']
                    for stream in [False, True]:
                        relay(port, model_ids[ident], stream)
                    report = t.action(directory, port, 'test', provider_id=ident)
                    assert report['http'] == 'HTTP 200' and not report['reason'], report
                    if settings:
                        assert '/openai/v1/' in report['url'], report
                saved = (directory / 'config.json').read_bytes()
                for protocol in ['openai_chat', 'openai_responses']:
                    for prefix in ['unavailable-400', 'unavailable-403', 'unavailable-404', 'unavailable-410',
                                   'unavailable-429', 'empty', 'invalid', 'malformed']:
                        error = t.action(directory, port, 'fetch', provider_id='azure-chat',
                                         upstream=origin + '/' + prefix + '/openai/v1', key=KEY,
                                         protocol=protocol, protocol_settings={'authentication': 'azure_api_key'}, expect_error=True)
                        assert ('GET', '/' + prefix + '/openai/v1/models') not in server.seen
                        assert (directory / 'config.json').read_bytes() == saved
                        if prefix in ['unavailable-400', 'unavailable-403', 'unavailable-404', 'unavailable-410', 'empty']:
                            assert 'deployment name manually' in error and '手动填写部署名称' in error, error
                        if prefix == 'unavailable-429':
                            assert error == 'Upstream HTTP 429', error
                t.action(directory, port, 'fetch', provider_id='azure-chat',
                         upstream=origin + '/denied/openai/v1', key=KEY,
                         protocol='openai_chat', protocol_settings={'authentication': 'azure_api_key'}, expect_error=True)
                assert ('GET', '/denied/openai/v1/models') not in server.seen
                # Manual deployment names must remain usable without discovery.
                manual = {'provider_id': 'azure-chat', 'upstream': origin + '/unavailable-404/openai/v1',
                          'key': KEY, 'protocol': 'openai_chat',
                          'protocol_settings': {'authentication': 'azure_api_key'}}
                t.action(directory, port, 'save', name='azure-chat',
                         models=[{'id': model_ids['azure-chat'], 'name': MODEL, 'model': MODEL}], **manual)
                relay(port, model_ids['azure-chat'], False)
                relay(port, model_ids['azure-chat'], True)
                saved = (directory / 'config.json').read_bytes()
                t.action(directory, port, 'save', provider_id='azure-chat', protocol='anthropic_messages', expect_error=True)
                assert (directory / 'config.json').read_bytes() == saved
                # A metadata cache entry must not cross Azure/default auth or credentials.
                same = t.action(directory, port, 'model_metadata', provider_id='azure-chat',
                                upstream=origin + '/openai/v1', key=KEY, protocol='openai_chat',
                                protocol_settings={'authentication': 'azure_api_key'}, model=MODEL)
                assert 'upstream' in same['metadata']['sources'], same
                for key, settings in [(KEY, {}), ('other-synthetic-key', {'authentication': 'azure_api_key'})]:
                    result = t.action(directory, port, 'model_metadata', provider_id='azure-chat',
                                      upstream=origin + '/openai/v1', key=key, protocol='openai_chat', protocol_settings=settings, model=MODEL)
                    assert result['metadata']['sources'] == ['unknown'], result
                snapshot = t.action(directory, port, 'snapshot')
                assert KEY not in json.dumps(snapshot['logs'])
                assert not server.failures, server.failures
            finally:
                t.stop_core(process, port, directory, log)
            # Reopen saved configuration and exercise every authentication path again.
            process, log, _ = t.start_core(args.exe, directory, port)
            try:
                for ident, *_ in cases:
                    relay(port, model_ids[ident], False)
                assert not server.failures, server.failures
            finally:
                t.stop_core(process, port, directory, log)
        print('PASS: protocol 10, Anthropic/Azure/default auth, deployment names without catalog fallback, manual entry, save/reopen, Chat/Responses JSON+SSE, probes, header and metadata isolation')
    finally:
        server.shutdown()
        server.server_close()
        if old_catalog is None:
            os.environ.pop('WORKBUDDY_PROXY_CATALOG_TEST_URL', None)
        else:
            os.environ['WORKBUDDY_PROXY_CATALOG_TEST_URL'] = old_catalog


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Independent loopback vendor fixtures + ordinary Chat consumer + real SQLite.

No user profile, provider key or upstream is accessed. Fixtures spell the native
REST contracts directly; they do not import or call the Rust conversion code.
"""
from contextlib import closing
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import sqlite3
import tempfile
import threading
import urllib.error
import urllib.request
import statistics_test as t

KEY = 'synthetic-native-provider-key'
SECRET = 'NATIVE_PRIVATE_OPAQUE_MARKER'
SYSTEM = 'NATIVE_SYSTEM_SENTINEL'
PNG = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jMZkAAAAASUVORK5CYII='
PROTOCOLS = ['openai_responses', 'anthropic_messages', 'gemini_generate_content']


def dumps(v):
    return json.dumps(v, ensure_ascii=False, separators=(',', ':'))


class Vendor(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *_):
        pass

    def handle(self):
        try:
            super().handle()
        except ConnectionError:
            pass  # Expected when the shared connection probe stops early.

    def reply(self, payload, stream=False, status=200, encoding='identity'):
        self.send_response(status)
        self.send_header('Content-Type', 'text/event-stream' if stream else 'application/json')
        self.send_header('Content-Length', str(len(payload)))
        self.send_header('Content-Encoding', encoding)
        self.send_header('Set-Cookie', 'provider-secret-cookie')
        self.send_header('ETag', 'provider-private-etag')
        self.send_header('Digest', 'provider-private-digest')
        self.send_header('X-Upstream-Untrusted', 'private-metadata')
        self.send_header('X-Request-Id', 'safe-fixture-id')
        self.send_header('Retry-After', '2')
        self.send_header('X-Ratelimit-Remaining-Requests', '99')
        self.end_headers()
        if stream:
            for i in range(0, len(payload), 11):
                self.wfile.write(payload[i:i + 11])
                self.wfile.flush()
        else:
            self.wfile.write(payload)

    def do_GET(self):
        self.reply(b'{"data":[]}', status=404)

    def do_POST(self):
        try:
            self.server.seen_posts += 1
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            self.serve(body)
        except ConnectionError:
            pass  # Connection probes intentionally stop at the first valid event.
        except Exception as exc:
            self.server.failures.append(str(exc))
            try:
                self.reply(b'{"error":{"message":"fixture assertion failed"}}', status=500)
            except OSError:
                pass

    def serve(self, body):
        if self.path == '/gateway/v1/responses':
            protocol = PROTOCOLS[0]
            assert self.headers.get('Authorization') == 'Bearer ' + KEY
            assert self.headers.get('x-api-key') is None
            assert self.headers.get('x-goog-api-key') is None
            history = body['input']
            stage = sum(item.get('type') == 'function_call' for item in history)
            stream = body['stream']
        elif self.path == '/gateway/v1/messages':
            protocol = PROTOCOLS[1]
            assert self.headers.get('x-api-key') == KEY
            assert self.headers.get('Authorization') is None
            assert self.headers.get('x-goog-api-key') is None
            assert self.headers.get('anthropic-version') == '2023-06-01'
            history = body['messages']
            stage = sum(part.get('type') == 'tool_use' for m in history for part in m['content'])
            stream = body['stream']
        elif self.path in ['/gateway/v1beta/models/gemini-test:generateContent',
                           '/gateway/v1beta/models/gemini-test:streamGenerateContent?alt=sse']:
            protocol = PROTOCOLS[2]
            assert self.headers.get('x-goog-api-key') == KEY
            assert self.headers.get('Authorization') is None
            assert self.headers.get('x-api-key') is None
            history = body['contents']
            stage = sum('functionCall' in part for m in history for part in m['parts'])
            stream = ':streamGenerateContent?' in self.path
        else:
            raise AssertionError('unexpected endpoint ' + self.path)
        assert self.headers.get('Accept-Encoding') == 'identity'
        assert self.headers.get('Cookie') is None
        assert self.headers.get('Anthropic-Beta') is None
        assert self.headers.get('Proxy-Authorization') is None
        raw = dumps(body)
        if 'NATIVE_CASE_HTTP_429' in raw:
            self.reply(('<html>' + SECRET + KEY + '</html>').encode(), status=429)
            return
        if protocol == PROTOCOLS[0] and 'ordinary assistant' in raw:
            assert {'role': 'assistant', 'content': 'ordinary assistant'} in history
        probe = 'Reply with OK.' in raw
        if not probe:
            assert SYSTEM in raw and 'developer-two' in raw
            assert PNG in raw
            if protocol == PROTOCOLS[0]:
                assert body['model'] == 'responses-test'
                assert body['reasoning']['effort'] == 'high'
                assert body['max_output_tokens'] == 1600
                assert body['store'] is False
            elif protocol == PROTOCOLS[1]:
                assert body['model'] == 'claude-test'
                assert body['thinking'] == {'type': 'enabled', 'budget_tokens': 1024}
                assert body['max_tokens'] == 1600
            else:
                assert body['generationConfig']['thinkingConfig']['thinkingBudget'] == 512
                assert body['generationConfig']['maxOutputTokens'] == 1600
            if stage:
                assert SECRET in raw, 'opaque state missing in replay'
                assert 'non-JSON tool result' in raw
                assert stage in (2, 4)
                assert 'wbpt_' not in raw, 'proxy IDs must be translated back'
                if protocol == PROTOCOLS[0]:
                    items = [item for item in history if 'id' in item]
                    assert len(set(item['id'] for item in items)) == len(items)
                    calls_by_id = {item['call_id']: json.loads(item['arguments']) for item in history if item.get('type') == 'function_call'}
                    for item in history:
                        if item.get('type') == 'function_call_output':
                            args = calls_by_id[item['call_id']]
                            assert item['output'] == 'non-JSON tool result n=%s round=%s' % (args['n'], args['round'])
                elif protocol == PROTOCOLS[1]:
                    calls_by_id = {part['id']: part['input'] for m in history for part in m['content'] if part.get('type') == 'tool_use'}
                    for m in history:
                        for part in m['content']:
                            if part.get('type') == 'tool_result':
                                args = calls_by_id[part['tool_use_id']]
                                assert part['content'] == 'non-JSON tool result n=%s round=%s' % (args['n'], args['round'])
        turn = stage // 2
        calls = [] if turn >= 2 or probe else [
            {'id': 'native-%s-%s-%s-%s' % (protocol, stream, turn, i), 'name': 'find',
             'args': {'q': '中"文', 'n': i, 'round': turn}} for i in range(2)]
        answer = 'OK' if probe else ('checking' if calls else 'done')
        if protocol == PROTOCOLS[0]:
            items = [{'id': 'reason-%s-%s' % (stream, turn), 'type': 'reasoning', 'summary': [{'type': 'summary_text', 'text': 'plan'}], 'encrypted_content': SECRET},
                     {'id': 'message-%s-%s' % (stream, turn), 'type': 'message', 'role': 'assistant', 'content': [{'type': 'output_text', 'text': answer, 'annotations': []}]}]
            items += [{'id': 'item-' + call['id'], 'type': 'function_call', 'call_id': call['id'],
                       'name': call['name'], 'arguments': json.dumps(call['args'], ensure_ascii=False)} for call in calls]
            native = {'id': 'resp-%s-%s' % (stream, turn), 'model': 'responses-test', 'status': 'completed', 'output': items,
                      'usage': {'input_tokens': 100, 'output_tokens': 20, 'total_tokens': 120,
                                'input_tokens_details': {'cached_tokens': 30}, 'output_tokens_details': {'reasoning_tokens': 5}}}
            events = [{'type': 'response.created', 'response': {'id': native['id'], 'model': native['model']}}]
            for index, item in enumerate(items):
                initial = dict(item)
                if item['type'] == 'function_call': initial['arguments'] = ''
                if item['type'] == 'message': initial['content'] = []
                if item['type'] == 'reasoning': initial['summary'] = []
                events.append({'type': 'response.output_item.added', 'output_index': index, 'item': initial})
                if item['type'] == 'message':
                    events.append({'type': 'response.output_text.delta', 'output_index': index, 'item_id': item['id'], 'content_index': 0, 'delta': answer})
                if item['type'] == 'function_call':
                    args = item['arguments']
                    for piece in (args[:5], args[5:]):
                        events.append({'type': 'response.function_call_arguments.delta', 'output_index': index, 'item_id': item['id'], 'delta': piece})
                events.append({'type': 'response.output_item.done', 'output_index': index, 'item': item})
            events.append({'type': 'response.completed', 'response': native})
        elif protocol == PROTOCOLS[1]:
            blocks = [{'type': 'thinking', 'thinking': 'plan', 'signature': SECRET}, {'type': 'text', 'text': answer}]
            blocks += [{'type': 'tool_use', 'id': call['id'], 'name': call['name'], 'input': call['args']} for call in calls]
            usage = {'input_tokens': 60, 'cache_read_input_tokens': 30, 'cache_creation_input_tokens': 10, 'output_tokens': 20}
            native = {'id': 'msg-fixture', 'type': 'message', 'role': 'assistant', 'model': 'claude-test',
                      'content': blocks, 'stop_reason': 'tool_use' if calls else 'end_turn', 'usage': usage}
            initial = dict(native, content=[], stop_reason=None, usage=dict(usage, output_tokens=0))
            events = [{'type': 'message_start', 'message': initial}]
            for index, block in enumerate(blocks):
                initial = dict(block)
                if block['type'] == 'thinking': initial.update(thinking='', signature='')
                if block['type'] == 'text': initial['text'] = ''
                if block['type'] == 'tool_use': initial['input'] = {}
                events.append({'type': 'content_block_start', 'index': index, 'content_block': initial})
                deltas = {'thinking': [{'type': 'thinking_delta', 'thinking': 'plan'}, {'type': 'signature_delta', 'signature': SECRET}],
                          'text': [{'type': 'text_delta', 'text': answer}],
                          'tool_use': [{'type': 'input_json_delta', 'partial_json': json.dumps(block.get('input', {}), ensure_ascii=False)}]}[block['type']]
                events += [{'type': 'content_block_delta', 'index': index, 'delta': delta} for delta in deltas]
                events.append({'type': 'content_block_stop', 'index': index})
            events += [{'type': 'message_delta', 'delta': {'stop_reason': native['stop_reason']}, 'usage': {'output_tokens': 20}}, {'type': 'message_stop'}]
        else:
            parts = [{'text': 'plan', 'thought': True, 'thoughtSignature': SECRET}, {'text': answer, 'thoughtSignature': SECRET}]
            # No native IDs: the proxy must keep two same-name calls distinct
            # and send result parts in the original call order without fake IDs.
            parts += [{'functionCall': {'name': call['name'], 'args': call['args']}, 'thoughtSignature': SECRET} for call in calls]
            native = {'modelVersion': 'gemini-test', 'candidates': [{'index': 0, 'content': {'role': 'model', 'parts': parts}, 'finishReason': 'STOP'}],
                      'usageMetadata': {'promptTokenCount': 100, 'candidatesTokenCount': 15, 'thoughtsTokenCount': 5, 'totalTokenCount': 120, 'cachedContentTokenCount': 30}}
            events = [{'modelVersion': 'gemini-test', 'candidates': [{'content': {'role': 'model', 'parts': [part]}}]} for part in parts]
            events += [{'candidates': [{'finishReason': 'STOP'}]}, {'usageMetadata': native['usageMetadata']}]
            if stage:
                results = [p['functionResponse'] for m in history for p in m['parts'] if 'functionResponse' in p]
                assert all('id' not in result for result in results)
                assert all(set(result['response']) == {'result'} for result in results)
                calls_in_order = [p['functionCall']['args'] for m in history for p in m['parts'] if 'functionCall' in p]
                assert len(calls_in_order) == len(results)
                for args, result in zip(calls_in_order, results):
                    assert result['response']['result'] == 'non-JSON tool result n=%s round=%s' % (args['n'], args['round'])
        self.server.requests.append((protocol, stream, stage, probe))
        if 'NATIVE_CASE_SLOW' in raw:
            self.send_response(200); self.send_header('Content-Type', 'text/event-stream'); self.send_header('Connection', 'close'); self.end_headers()
            try:
                import time
                for e in events:
                    payload = ('' if protocol == PROTOCOLS[2] else 'event: %s\n' % e['type']) + 'data: ' + dumps(e) + '\n\n'
                    self.wfile.write(payload.encode()); self.wfile.flush()
                    if (e.get('type') == 'response.output_text.delta' or (e.get('type') == 'content_block_delta' and e.get('delta', {}).get('type') == 'text_delta') or any(any(p.get('text') == answer for p in c.get('content', {}).get('parts', [])) for c in e.get('candidates', []))):
                        break
                for _ in range(200):
                    time.sleep(.03); self.wfile.write(b': heartbeat\n\n'); self.wfile.flush()
            except ConnectionError:
                self.server.cancelled.add(protocol)
            self.close_connection = True
            return
        if 'NATIVE_CASE_ERROR_EVENT' in raw:
            events = events[:1] + [{'type': 'error', 'error': {'message': SECRET + KEY}}]
        if 'NATIVE_CASE_TRUNCATED' in raw:
            events = [e for e in events if e.get('type') not in ['response.completed', 'message_stop']
                      and not any('finishReason' in c for c in e.get('candidates', []))]
        if 'NATIVE_CASE_PRICE' in raw:
            self.server.price_started.set()
            assert self.server.price_release.wait(5), 'price test release timed out'
        if stream:
            payload = ''.join(('' if protocol == PROTOCOLS[2] else 'event: %s\r\n' % e['type']) + 'data: ' + dumps(e) + '\r\n\r\n' for e in events).encode()
        else:
            payload = dumps(native).encode()
        if 'NATIVE_CASE_GZIP' in raw:
            import gzip
            self.reply(gzip.compress(payload), stream, encoding='gzip')
        else:
            self.reply(payload, stream)


def chat(port, alias, messages, stream):
    payload = {'model': alias, 'messages': messages, 'stream': stream, 'max_tokens': 1600,
               'reasoning_effort': 'high', 'tools': [{'type': 'function', 'function': {'name': 'find', 'parameters': {'type': 'object', 'properties': {'q': {'type': 'string'}, 'n': {'type': 'integer'}, 'round': {'type': 'integer'}}}}}]}
    if stream: payload['stream_options'] = {'include_usage': True}
    req = urllib.request.Request('http://127.0.0.1:%s/v1/chat/completions' % port, data=dumps(payload).encode(), headers={
        'Authorization': 'Bearer ' + t.LOCAL_KEY, 'Content-Type': 'application/json',
        'X-Api-Key': 'client-vendor-secret', 'X-Goog-Api-Key': 'client-other-secret',
        'Cookie': 'client-cookie-secret', 'Anthropic-Beta': 'client-beta', 'Proxy-Authorization': 'client-proxy-secret'})
    with urllib.request.urlopen(req, timeout=20) as response:
        for key in ['Set-Cookie', 'ETag', 'Digest', 'X-Upstream-Untrusted', 'Content-Encoding', 'Content-Length']:
            assert response.headers.get(key) is None, (key, dict(response.headers))
        assert response.headers['X-Request-Id'] == 'safe-fixture-id'
        raw = response.read()
    assert SECRET.encode() not in raw
    if not stream:
        value = json.loads(raw)
        assert value['usage']['prompt_tokens'] == 100 and value['usage']['completion_tokens'] == 20
        return value['choices'][0]['message'], value['choices'][0]['finish_reason']
    message = {'role': 'assistant', 'content': ''}; calls = {}; ids = set(); finishes = []; done = 0; usage = None
    for frame in raw.decode().split('\n\n'):
        if not frame.strip(): continue
        assert frame.startswith('data: '), frame
        data = frame[6:]
        if data == '[DONE]': done += 1; continue
        value = json.loads(data)
        assert 'error' not in value, value
        ids.add((value['id'], value['created'], value['model']))
        if not value['choices']:
            assert finishes, 'usage must follow finish'
            usage = value['usage']; continue
        choice = value['choices'][0]; delta = choice['delta']
        if choice['finish_reason'] is not None: finishes.append(choice['finish_reason'])
        message['content'] += delta.get('content', '')
        for call in delta.get('tool_calls', []):
            current = calls.setdefault(call['index'], {'id': '', 'type': 'function', 'function': {'name': '', 'arguments': ''}})
            if 'id' in call: assert not current['id']; current['id'] = call['id']
            for key in ['name', 'arguments']: current['function'][key] += call.get('function', {}).get(key, '')
    assert done == 1 and len(finishes) == 1 and len(ids) == 1, (done, finishes, ids)
    assert usage['prompt_tokens'] == 100 and usage['completion_tokens'] == 20
    if calls: message['tool_calls'] = [calls[i] for i in sorted(calls)]
    return message, finishes[0]


def error_request(port, alias, messages, stream=False, extra=None, query=''):
    payload = {'model': alias, 'messages': messages, 'stream': stream, 'max_tokens': 1600, 'reasoning_effort': 'high'}
    if extra: payload.update(extra)
    request = urllib.request.Request('http://127.0.0.1:%s/v1/chat/completions%s' % (port, query), data=dumps(payload).encode(),
                                     headers={'Authorization': 'Bearer ' + t.LOCAL_KEY, 'Content-Type': 'application/json'})
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            return response.status, response.read(), response.headers
    except urllib.error.HTTPError as e:
        return e.code, e.read(), e.headers


def main():
    parser = argparse.ArgumentParser(); parser.add_argument('--exe', default='target/debug/workbuddy-proxy'); args = parser.parse_args()
    server = ThreadingHTTPServer(('127.0.0.1', 0), Vendor); server.failures = []; server.requests = []; server.seen_posts = 0; server.cancelled = set(); server.price_started = threading.Event(); server.price_release = threading.Event()
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix='wbp-native-contracts-') as tmp:
            folder = Path(tmp); port = t.free_port(); providers = []
            for i, protocol in enumerate(PROTOCOLS):
                settings = {}
                if i: settings['reasoning_profile'] = {'mode': 'anthropic_budget' if i == 1 else 'gemini_budget', 'budgets': {'high': 1024 if i == 1 else 512}}
                providers.append({'id': 'provider-%s' % i, 'name': protocol, 'protocol': protocol, 'protocol_settings': settings,
                    'upstream': 'http://127.0.0.1:%s/gateway/%s' % (server.server_port, 'v1beta' if i == 2 else 'v1'), 'api_key': KEY,
                    'models': [{'id': 'route-%s' % i, 'alias': 'wbp-native-%s' % i, 'name': 'Native %s' % i,
                                'model': ['responses-test', 'claude-test', 'models/gemini-test'][i], 'reasoning_effort': 'default'}]})
            config = {'local_key': t.LOCAL_KEY, 'port': port, 'language': 'en', 'providers': providers, 'installed': []}
            (folder / 'config.json').write_text(dumps(config))
            process, log, health = t.start_core(args.exe, folder, port)
            try:
                assert health['control_protocol'] == 8
                t.wait_for(lambda: t.stats(folder, port, 'stats_health')['state'] == 'ready', 'stats ready')
                for i, protocol in enumerate(PROTOCOLS):
                    t.stats(folder, port, 'stats_price_save', provider_id='provider-%s' % i, upstream_model=providers[i]['models'][0]['model'],
                            currency='CNY', input_mode='flat', input_rate_nanos='2000000000', output_rate_nanos='8000000000')
                    for stream in [False, True]:
                        messages = [{'role': 'system', 'content': SYSTEM}, {'role': 'developer', 'content': 'developer-two'},
                                    {'role': 'user', 'content': [{'type': 'text', 'text': 'Run tools'}, {'type': 'image_url', 'image_url': {'url': 'data:image/png;base64,' + PNG}}]}]
                        for round in range(3):
                            message, finish = chat(port, 'wbp-native-%s' % i, messages, stream)
                            if round < 2:
                                assert finish == 'tool_calls' and len(message['tool_calls']) == 2
                                assert len(set(call['id'] for call in message['tool_calls'])) == 2
                                for call in message['tool_calls']:
                                    assert call['id'].startswith('wbpt_'); assert json.loads(call['function']['arguments'])['round'] == round
                                # Keep ordinary Chat fields only, no native reasoning/signatures.
                                messages.append({k: message[k] for k in ['role', 'content', 'tool_calls']})
                                for call in reversed(message['tool_calls']):
                                    messages.append({'role': 'tool', 'tool_call_id': call['id'], 'content': 'non-JSON tool result n=%s round=%s' % (json.loads(call['function']['arguments'])['n'], round)})
                                if round == 0:
                                    tampered = json.loads(dumps(messages)); tampered[0]['content'] += ' tampered'
                                    before = server.seen_posts
                                    status, raw, _ = error_request(port, 'wbp-native-%s' % i, tampered)
                                    assert status == 400 and json.loads(raw)['error']['code'] == 'conversation_state_mismatch'
                                    assert server.seen_posts == before
                                    t.action(folder, port, 'save', provider_id='provider-%s' % i, name=protocol + '-renamed')
                                    saved = json.loads((folder / 'config.json').read_text())['providers'][i]
                                    assert saved['protocol'] == protocol and saved['protocol_settings'].get('reasoning_profile', {}).get('budgets', {}) == providers[i]['protocol_settings'].get('reasoning_profile', {}).get('budgets', {})
                            else:
                                assert finish == 'stop' and message['content'] == 'done'
                        print('PASS real relay %s %s: image, thinking, two parallel tool rounds, final answer' % (protocol, 'SSE' if stream else 'JSON'), flush=True)
                def recorded():
                    with closing(sqlite3.connect(folder / 'usage.sqlite3')) as db, db:
                        return db.execute("SELECT COUNT(*) FROM usage_requests WHERE outcome='completed'").fetchone()[0] == 18
                t.wait_for(recorded, 'all native request finishes persisted')
                with closing(sqlite3.connect(folder / 'usage.sqlite3')) as db, db:
                    assert db.execute('PRAGMA user_version').fetchone()[0] == 6
                    rows = db.execute("SELECT upstream_protocol,input_tokens,output_tokens,total_tokens,input_breakdown,input_uncached_tokens FROM usage_requests WHERE outcome='completed'").fetchall()
                    assert len(rows) == 18 and set(r[0] for r in rows) == set(PROTOCOLS)
                    assert all(r[1:4] == (100, 20, 120) for r in rows), rows
                    assert all(r[4:] == ('disjoint', 60) for r in rows if r[0] == PROTOCOLS[1])
                    assert db.execute("SELECT COUNT(*) FROM usage_costs c JOIN usage_requests r USING(request_id) WHERE r.outcome='completed' AND c.total_cost_nanos=360000 AND c.input_rate_nanos=2000000000 AND c.output_rate_nanos=8000000000").fetchone()[0] == 18
                    dump = '\n'.join(db.iterdump())
                    assert SECRET not in dump and SYSTEM not in dump and PNG not in dump and KEY not in dump
                    request_id = db.execute('SELECT request_id FROM usage_requests LIMIT 1').fetchone()[0]
                detail = t.stats(folder, port, 'stats_request_detail', request_id=request_id)
                assert detail['request']['upstream_protocol'] in PROTOCOLS
                for i, protocol in enumerate(PROTOCOLS):
                    base = [{'role': 'system', 'content': SYSTEM}, {'role': 'developer', 'content': 'developer-two'},
                            {'role': 'user', 'content': [{'type': 'text', 'text': 'NATIVE_CASE_HTTP_429'}, {'type': 'image_url', 'image_url': {'url': 'data:image/png;base64,' + PNG}}]}]
                    status, raw, headers = error_request(port, 'wbp-native-%s' % i, base)
                    assert status == 429 and headers['Retry-After'] == '2'
                    assert json.loads(raw)['error']['code'] == 'upstream_http_error' and SECRET.encode() not in raw and KEY.encode() not in raw
                    for marker, code in [('NATIVE_CASE_GZIP', 'unsupported_content_encoding'), ('NATIVE_CASE_TRUNCATED', None), ('NATIVE_CASE_ERROR_EVENT', None)]:
                        base[2]['content'][0]['text'] = marker
                        status, raw, _ = error_request(port, 'wbp-native-%s' % i, base, stream=True)
                        if code:
                            assert status == 502 and json.loads(raw)['error']['code'] == code
                        else:
                            assert status == 200 and b'"error"' in raw and b'[DONE]' not in raw and b'"finish_reason":"stop"' not in raw
                        assert SECRET.encode() not in raw and KEY.encode() not in raw
                    before = server.seen_posts
                    status, raw, _ = error_request(port, 'wbp-native-%s' % i, base, extra={'seed': 0})
                    assert status == 400 and json.loads(raw)['error']['code'] == 'unsupported_parameter'
                    status, raw, _ = error_request(port, 'wbp-native-%s' % i, base, query='?unsupported=1')
                    assert status == 400 and json.loads(raw)['error']['code'] == 'unsupported_query'
                    expired = [{'role': 'user', 'content': 'hello'}, {'role': 'assistant', 'content': None, 'tool_calls': [
                        {'id': 'wbpt_' + '0' * 32, 'type': 'function', 'function': {'name': 'find', 'arguments': '{}'}}]},
                        {'role': 'tool', 'tool_call_id': 'wbpt_' + '0' * 32, 'content': 'result'}]
                    status, raw, _ = error_request(port, 'wbp-native-%s' % i, expired)
                    assert status == 409 and json.loads(raw)['error']['code'] == 'conversation_state_expired'
                    assert server.seen_posts == before
                    report = t.action(folder, port, 'test', connection_id='route-%s' % i)
                    assert report['http'] == 'HTTP 200' and report['first_event_ms'] is not None, report
                    assert report['protocol'] == protocol
                    if i == 2: assert report['url'].endswith(':streamGenerateContent?alt=sse')
                    base[2]['content'][0]['text'] = 'NATIVE_CASE_SLOW'
                    payload = {'model': 'wbp-native-%s' % i, 'messages': base, 'stream': True, 'max_tokens': 1600, 'reasoning_effort': 'high'}
                    request = urllib.request.Request('http://127.0.0.1:%s/v1/chat/completions' % port, data=dumps(payload).encode(), headers={'Authorization': 'Bearer ' + t.LOCAL_KEY, 'Content-Type': 'application/json'})
                    with urllib.request.urlopen(request, timeout=3) as response:
                        while True:
                            line = response.readline()
                            assert line, 'stream closed before incremental text'
                            if line.startswith(b'data: '):
                                value = json.loads(line[6:])
                                if value.get('choices') and value['choices'][0]['delta'].get('content') == 'checking': break
                    t.wait_for(lambda: protocol in server.cancelled, 'native source is dropped when Chat client cancels')
                    print('PASS %s: HTTP 429, encoding, truncation, error event, unsupported inputs, expired state, shared connection probe' % protocol, flush=True)
                # Freeze price at request admission, including updates while a native
                # vendor is still computing. This uses real control calls and SQLite.
                import concurrent.futures
                for i, protocol in enumerate(PROTOCOLS):
                    provider_id='provider-%s' % i; model=providers[i]['models'][0]['model']
                    with closing(sqlite3.connect(folder / 'usage.sqlite3')) as db, db:
                        rule_id, revision=db.execute('SELECT price_id,revision FROM stats_model_prices WHERE provider_id=?',(provider_id,)).fetchone()
                        before=db.execute('SELECT COUNT(*) FROM usage_requests WHERE provider_id=?',(provider_id,)).fetchone()[0]
                    server.price_started.clear();server.price_release.clear()
                    messages=[{'role':'system','content':SYSTEM},{'role':'developer','content':'developer-two'},
                        {'role':'assistant','content':'ordinary assistant'},
                        {'role':'user','content':[{'type':'text','text':'NATIVE_CASE_PRICE'},{'type':'image_url','image_url':{'url':'data:image/png;base64,'+PNG}}]}]
                    with concurrent.futures.ThreadPoolExecutor() as pool:
                        result=pool.submit(chat,port,'wbp-native-%s' % i,messages,False)
                        assert server.price_started.wait(5)
                        try:
                            updated=t.stats(folder,port,'stats_price_save',rule_id=rule_id,expected_revision=revision,provider_id=provider_id,upstream_model=model,
                                currency='CNY',input_mode='flat',input_rate_nanos='10000000000',output_rate_nanos='20000000000')
                        finally:server.price_release.set()
                        result.result()
                    def settled():
                        with closing(sqlite3.connect(folder / 'usage.sqlite3')) as db, db:
                            return db.execute("SELECT COUNT(*) FROM usage_requests WHERE provider_id=? AND outcome='completed'",(provider_id,)).fetchone()[0]>=7
                    t.wait_for(settled,'native price request settled')
                    with closing(sqlite3.connect(folder / 'usage.sqlite3')) as db, db:
                        old=db.execute('SELECT c.input_rate_nanos,c.output_rate_nanos,c.total_cost_nanos FROM usage_requests r JOIN usage_costs c USING(request_id) WHERE r.provider_id=? ORDER BY r.started_at_ms DESC LIMIT 1',(provider_id,)).fetchone()
                        assert old==(2000000000,8000000000,360000),old
                    rule=updated
                    t.stats(folder,port,'stats_price_delete',rule_id=rule_id,expected_revision=rule['revision'])
                    messages[-1]['content'][0]['text']='missing price'
                    chat(port,'wbp-native-%s' % i,messages,False)
                    def zero_price():
                        with closing(sqlite3.connect(folder / 'usage.sqlite3')) as db, db:
                            return db.execute("SELECT COUNT(*) FROM usage_requests r JOIN usage_costs c USING(request_id) WHERE r.provider_id=? AND c.total_cost_nanos=0 AND c.price_source='default_zero'",(provider_id,)).fetchone()[0]==1
                    t.wait_for(zero_price,'native missing model price is zero')
                    print('PASS %s: ordinary assistant history, mid-flight frozen rates, deleted price defaults to zero' % protocol,flush=True)
                def probes():
                    with closing(sqlite3.connect(folder / 'usage.sqlite3')) as db, db:
                        return db.execute("SELECT COUNT(*) FROM usage_requests WHERE source='connection_test' AND outcome='cancelled'").fetchone()[0] == 3
                t.wait_for(probes, 'native probes recorded once as cancelled')
                def cancelled():
                    with closing(sqlite3.connect(folder / 'usage.sqlite3')) as db, db:
                        return db.execute("SELECT COUNT(*) FROM usage_requests WHERE source='relay' AND outcome='cancelled'").fetchone()[0] == 3
                t.wait_for(cancelled, 'native stream cancellations persisted')
                with closing(sqlite3.connect(folder / 'usage.sqlite3')) as db, db:
                    for protocol, output, total, cost in db.execute("SELECT r.upstream_protocol,r.output_tokens,r.total_tokens,c.total_cost_nanos FROM usage_requests r JOIN usage_costs c USING(request_id) WHERE r.outcome='cancelled'"):
                        assert output is None and total is None and cost is None, (protocol, output, total, cost)
                    dump = '\n'.join(db.iterdump())
                    assert SECRET not in dump and SYSTEM not in dump and PNG not in dump and KEY not in dump
                assert not server.failures, server.failures
                print('PASS native usage + immutable price rows + protocol DTO + SQLite privacy', flush=True)
            finally:
                t.stop_core(process, port, folder, log)
    finally:
        server.shutdown(); server.server_close()
    print('PASS native protocol loopback contracts')


if __name__ == '__main__': main()

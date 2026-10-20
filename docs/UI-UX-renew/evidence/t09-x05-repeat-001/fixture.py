#!/usr/bin/env python3
"""Isolated fixture for UI-T08-RESIDUAL-003 (pricing / storage / price dialogs).

Creates a throw-away config with 25 providers, starts the current core on
127.0.0.1 with a random port and a local offline price-catalogue mock, inserts
20 finished synthetic usage rows, and creates 25 price rules through the real
`stats_price_save` control command. No real profile, credential, history file,
API key or upstream network is read.

Commands:
  setup    start mock + core, insert rows, create prices, write run.json
  status   print recorded state plus live API counts
  mode ok|slow|fail   switch the mock catalogue reply
  stop     stop the core and the mock
  cleanup  stop then remove only this task's temporary run directory
"""
import argparse
import json
import os
import random
import shutil
import signal
import socket
import sqlite3
import subprocess
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime
from pathlib import Path

ROOT = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign')
EV = ROOT / 'docs/UI-UX-renew/evidence/t09-x05-repeat-001'
CAP = Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/'
           '01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t09-x05-repeat-001')
RUN = CAP / 'run'
CORE = ROOT / 'target/release/workbuddy-proxy'
MOCK = EV / 'mock_catalog.py'
PROVIDERS = 25

PROVIDER_NAMES = ['opencode', 'Provider B', 'Provider C'] + [
    'Provider %02d' % i for i in range(4, PROVIDERS + 1)
]
MODELS = ['deepseek-v4.1-flash'] + ['model-%02d' % i for i in range(2, PROVIDERS + 1)]
PROVIDER_IDS = ['a%011d' % i for i in range(1, PROVIDERS + 1)]
MODEL_IDS = ['b%011d' % i for i in range(1, PROVIDERS + 1)]


def now_ms():
    return int(time.time() * 1000)


def today_start_ms():
    lt = time.localtime()
    return int(time.mktime((lt.tm_year, lt.tm_mon, lt.tm_mday, 0, 0, 0, 0, 0, -1)) * 1000)


def free_port():
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]


def env_for(config, mock_port):
    env = dict(os.environ)
    env.update({
        'HOME': str(RUN / 'home'), 'TMPDIR': str(CAP / 'tmp'),
        'TMP': str(CAP / 'tmp'), 'TEMP': str(CAP / 'tmp'),
        'WORKBUDDY_PROXY_CONFIG_DIR': str(config),
        'WORKBUDDY_PROXY_CATALOG_TEST_URL':
            'http://127.0.0.1:%d/api.json' % mock_port,
        'XDG_CONFIG_HOME': str(RUN / 'home/.config'),
        'XDG_DATA_HOME': str(RUN / 'home/.local/share'),
        'XDG_CACHE_HOME': str(RUN / 'home/.cache'),
    })
    return env


def write_config(config):
    providers = []
    for index, (name, model) in enumerate(zip(PROVIDER_NAMES, MODELS), start=1):
        providers.append({
            'id': PROVIDER_IDS[index - 1],
            'name': name,
            'upstream': ('https://opencode.test/v1' if index == 1
                         else 'https://provider%02d.test/v1' % index),
            'protocol': 'openai_chat',
            'api_key': '',
            'models': [{
                'id': MODEL_IDS[index - 1],
                'alias': model,
                'name': model,
                'model': model,
                'reasoning_effort': 'default',
            }],
        })
    payload = {
        'local_key': 't09-supp-local-key',
        'language': 'zh',
        'port': 0,
        'autostart': False,
        'providers': providers,
        'installed': [],
    }
    config.mkdir(parents=True, exist_ok=True)
    path = config / 'config.json'
    path.write_text(json.dumps(payload, ensure_ascii=False), encoding='utf-8')
    os.chmod(path, 0o600)


def instance(config):
    return json.loads((config / 'instance.json').read_text(encoding='utf-8'))


def command(config, port, payload, timeout=20):
    key = instance(config)['management_key']
    request = urllib.request.Request(
        'http://127.0.0.1:%d/_control/action' % port,
        data=json.dumps(payload).encode(),
        headers={'Content-Type': 'application/json',
                 'Authorization': 'Bearer ' + key})
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    try:
        with opener.open(request, timeout=timeout) as response:
            body = json.loads(response.read())
    except urllib.error.HTTPError as failure:
        detail = failure.read().decode('utf-8', 'replace')
        raise RuntimeError('HTTP %s for %s: %s'
                           % (failure.code, payload.get('command'), detail)) from None
    message = body.get('message', '')
    return json.loads(message) if isinstance(message, str) else message


def stats(config, port, name, **fields):
    payload = {'command': name, 'request_tag': 't08'}
    payload.update(fields)
    return command(config, port, payload)


def wait_ready(config, port, timeout=40):
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        try:
            if stats(config, port, 'stats_health').get('state') == 'ready':
                return
        except Exception as failure:
            last = failure
        time.sleep(0.15)
    raise RuntimeError('core stats not ready: %r' % (last,))


def stop_pid(pid):
    if not pid:
        return
    try:
        os.kill(pid, signal.SIGTERM)
    except (ProcessLookupError, PermissionError):
        return
    for _ in range(100):
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return
        time.sleep(0.05)
    try:
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass


def start_mock():
    port = free_port()
    (RUN / 'mock-mode').write_text('ok')
    process = subprocess.Popen(
        [sys.executable, str(MOCK), str(RUN), str(port)],
        env=env_for(RUN / 'config', port), cwd=str(RUN),
        stdout=open(RUN / 'mock.log', 'ab'), stderr=subprocess.STDOUT)
    for _ in range(100):
        if (RUN / 'mock-ready').exists():
            break
        if process.poll() is not None:
            raise RuntimeError('mock exited')
        time.sleep(0.05)
    return process, port


def start_core(mock_port, write=True):
    config = RUN / 'config'
    port = free_port()
    if write:
        write_config(config)
    log = open(RUN / 'core.log', 'ab')
    process = subprocess.Popen(
        [str(CORE), '--headless', '--config-dir', str(config), '--port', str(port)],
        env=env_for(config, mock_port), stdout=log, stderr=log, cwd=str(RUN))
    for _ in range(600):
        if process.poll() is not None:
            raise RuntimeError('core exited; see %s' % (RUN / 'core.log'))
        try:
            if instance(config).get('port') == port:
                break
        except (OSError, ValueError, KeyError):
            pass
        time.sleep(0.05)
    else:
        process.kill()
        raise RuntimeError('instance.json did not appear')
    wait_ready(config, port)
    return process, port


def insert_rows(config):
    db_path = config / 'usage.sqlite3'
    base = now_ms() - 5 * 60_000
    columns = (
        'id,request_id,run_id,source,provider_id,route_id,provider_name,route_name,'
        'requested_model,upstream_model,response_model,started_at_ms,ended_at_ms,'
        'duration_ms,ttfb_ms,first_content_ms,stream,upstream_started,http_status,'
        'outcome,usage_status,usage_source,parser_version,input_tokens,output_tokens,'
        'total_tokens,cache_read_tokens,cache_write_tokens,reasoning_tokens,'
        'input_uncached_tokens,input_breakdown,record_origin,time_basis,'
        'total_tokens_derived,upstream_protocol,created_at_ms,updated_at_ms')
    with sqlite3.connect(db_path) as db:
        run = db.execute(
            'SELECT run_id FROM stats_runs ORDER BY started_at_ms DESC LIMIT 1').fetchone()
        if run is None:
            run = ('t08-run',)
            db.execute('INSERT INTO stats_runs(run_id,started_at_ms,admitted_requests) '
                       'VALUES(?,?,0)', (run[0], now_ms() - 7_200_000))
        run_id = run[0]
        for index in range(1, 21):
            started = base + index * 10_000
            value = {
                'id': index, 'request_id': 't08-%04d' % index, 'run_id': run_id,
                'source': 'relay', 'provider_id': 'p01', 'route_id': 'route-01',
                'provider_name': 'opencode', 'route_name': 'Route 01',
                'requested_model': 'deepseek-v4.1-flash',
                'upstream_model': 'deepseek-v4.1-flash',
                'response_model': 'deepseek-v4.1-flash',
                'started_at_ms': started, 'ended_at_ms': started + 500,
                'duration_ms': 500, 'ttfb_ms': 100, 'first_content_ms': 200,
                'stream': 0, 'upstream_started': 1, 'http_status': 200,
                'outcome': 'completed', 'usage_status': 'complete',
                'usage_source': 'upstream', 'parser_version': 1,
                'input_tokens': 1000 + index, 'output_tokens': 100,
                'total_tokens': 1100 + index, 'cache_read_tokens': 0,
                'cache_write_tokens': 0, 'reasoning_tokens': 0,
                'input_uncached_tokens': 1000 + index,
                'input_breakdown': 'disjoint', 'record_origin': 'proxy',
                'time_basis': 'request_start', 'total_tokens_derived': 0,
                'upstream_protocol': 'openai_chat', 'created_at_ms': started,
                'updated_at_ms': started + 500,
            }
            db.execute('INSERT INTO usage_requests(%s) VALUES(%s)'
                       % (columns, ','.join('?' for _ in columns.split(','))),
                       [value[key] for key in columns.split(',')])
            db.execute(
                'INSERT INTO usage_costs(request_id,price_source,price_snapshot_at_ms,'
                'currency,input_mode,input_rate_nanos,output_rate_nanos,'
                'cache_read_rate_nanos,cache_write_rate_nanos,status,'
                'input_cost_nanos,output_cost_nanos,cache_read_cost_nanos,'
                'cache_write_cost_nanos,total_cost_nanos,pricing_version,calculated_at_ms) '
                "VALUES(?,'default_zero',?,'CNY','flat',0,0,0,0,'default_zero',0,0,0,0,0,2,?)",
                (value['request_id'], started, started + 500))
        db.commit()
        count = db.execute('SELECT COUNT(*) FROM usage_requests').fetchone()[0]
    return count


def create_prices(config, port):
    rules = []
    for index in range(1, PROVIDERS + 1):
        currency = 'CNY' if index % 2 else 'USD'
        mode = ('flat', 'read_discount', 'disjoint')[index % 3]
        payload = {
            'command': 'stats_price_save',
            'request_tag': 't08-price-%02d' % index,
            'provider_id': PROVIDER_IDS[index - 1],
            'upstream_model': MODELS[index - 1],
            'currency': currency,
            'input_mode': mode,
            'input_rate_nanos': str(100_000_000 + index * 10_000_000),
            'output_rate_nanos': str(200_000_000 + index * 10_000_000),
            'cache_read_rate_nanos': str(30_000_000 + index * 1_000_000),
            'cache_write_rate_nanos': str(40_000_000 + index * 1_000_000),
            'label': ('models.dev · %s/%s' % (PROVIDER_NAMES[index - 1], MODELS[index - 1])
                      if index % 2 else 'manual'),
            'expected_revision': 0,
        }
        result = command(config, port, payload)
        rules.append({'index': index, 'provider_id': PROVIDER_IDS[index - 1],
                      'model': MODELS[index - 1], 'currency': currency,
                      'mode': mode, 'ruled': result})
    page = stats(config, port, 'stats_prices_list', limit=20)
    return rules, page


def record_expected(config, port, count, rules, page, mock_port):
    settings = stats(config, port, 'stats_settings_get')
    provider_modes = {}
    for index in range(1, 4):
        provider_modes[PROVIDER_IDS[index - 1]] = stats(
            config, port, 'stats_provider_settings_get',
            provider_id=PROVIDER_IDS[index - 1])
    expected = {
        'providers': PROVIDER_NAMES,
        'models': MODELS,
        'usage_rows': count,
        'price_rules': rules,
        'first_price_page': {
            'total': len(page.get('items', [])) if isinstance(page, dict) else None,
            'items': [{'rule_id': item.get('rule_id'),
                       'upstream_model': item.get('upstream_model'),
                       'currency': item.get('currency')}
                      for item in (page.get('items', []) if isinstance(page, dict) else [])],
            'next_rule_id': page.get('next_rule_id') if isinstance(page, dict) else None,
        },
        'settings': settings,
        'provider_settings': provider_modes,
        'mock_catalog_url': 'http://127.0.0.1:%d/api.json' % mock_port,
        'page_size': 20,
    }
    (EV / 'expected.json').write_text(
        json.dumps(expected, ensure_ascii=False, indent=2), encoding='utf-8')
    return expected


def stop_run():
    state_file = RUN / 'run.json'
    if not state_file.exists():
        return
    state = json.loads(state_file.read_text(encoding='utf-8'))
    stop_pid(state.get('gpui_pid'))
    stop_pid(state.get('mock_pid'))
    port = state.get('port')
    config = RUN / 'config'
    if port:
        try:
            key = instance(config)['management_key']
            request = urllib.request.Request(
                'http://127.0.0.1:%d/_control/stop' % port, data=b'',
                headers={'Authorization': 'Bearer ' + key})
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
            with opener.open(request, timeout=3) as response:
                response.read()
        except Exception:
            pass
    stop_pid(state.get('core_pid'))


def kill_stale():
    result = subprocess.run(['ps', '-eo', 'pid=,command='], capture_output=True,
                            text=True)
    pids = []
    for line in result.stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        if str(RUN / 'config') in line and 'workbuddy-proxy' in line:
            pids.append(int(line.split()[0]))
        elif str(MOCK) in line and str(RUN) in line:
            pids.append(int(line.split()[0]))
    for pid in pids:
        stop_pid(pid)
    return pids


def setup():
    stop_run()
    stale = kill_stale()
    if stale:
        print('stopped stale:', stale)
    if RUN.exists():
        shutil.rmtree(RUN)
    RUN.mkdir(parents=True)
    (RUN / 'home').mkdir()
    (CAP / 'tmp').mkdir(parents=True, exist_ok=True)
    mock, mock_port = start_mock()
    process, port = start_core(mock_port)
    try:
        config = RUN / 'config'
        count = insert_rows(config)
        rules, page = create_prices(config, port)
        expected = record_expected(config, port, count, rules, page, mock_port)
    except Exception:
        stop_pid(process.pid)
        stop_pid(mock.pid)
        raise
    state = {
        'core_pid': process.pid, 'mock_pid': mock.pid, 'mock_port': mock_port,
        'gpui_pid': None, 'port': port, 'config': str(config),
        'db': str(config / 'usage.sqlite3'), 'usage_rows': count,
        'prices': len(rules), 'mock_mode': 'ok',
        'created_ms': now_ms(),
    }
    (RUN / 'run.json').write_text(json.dumps(state, indent=2), encoding='utf-8')
    print(json.dumps({'state': state,
                      'price_page': expected['first_price_page']['total'],
                      'next_rule_id': expected['first_price_page']['next_rule_id'] is not None},
                     ensure_ascii=False, indent=2))


def status():
    state = json.loads((RUN / 'run.json').read_text(encoding='utf-8'))
    config = RUN / 'config'
    live = {}
    try:
        live['health'] = stats(config, state['port'], 'stats_health')
        live['prices'] = stats(config, state['port'], 'stats_prices_list', limit=20)
        live['prices'].pop('items', None)
        live['settings'] = stats(config, state['port'], 'stats_settings_get')
    except Exception as failure:
        live['error'] = repr(failure)
    print(json.dumps({'state': state, 'live': live}, ensure_ascii=False, indent=2))


def mode(value):
    (RUN / 'mock-mode').write_text(value)
    print('mock mode', value)


def cleanup():
    stop_run()
    shutil.rmtree(RUN, ignore_errors=True)
    print('removed', RUN)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('command',
                        choices=['setup', 'status', 'mode', 'stop', 'cleanup'])
    parser.add_argument('value', nargs='?')
    args = parser.parse_args()
    if args.command == 'setup':
        setup()
    elif args.command == 'status':
        status()
    elif args.command == 'mode':
        mode(args.value or 'ok')
    elif args.command == 'stop':
        stop_run()
    else:
        cleanup()

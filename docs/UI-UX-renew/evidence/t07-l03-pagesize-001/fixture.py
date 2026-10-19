#!/usr/bin/env python3
"""Isolated fixture for UI-T07-L03-PAGESIZE-001.

Creates a throw-away config directory, starts the current-head core on
127.0.0.1 with a random port, inserts exactly 60 synthetic request-log rows
into that core's SQLite database, and records the backend pagination replies
for 20/50/100 rows per page. No real profile, credential or upstream is read.

Commands:
  setup    start core, insert 60 rows, record backend samples, write run.json
  status   print the recorded run state and the live API totals
  stop     stop the GPUI and core processes recorded in run.json
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
import time
import urllib.request
from pathlib import Path

ROOT = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign')
EV = ROOT / 'docs/UI-UX-renew/evidence/t07-l03-pagesize-001'
CAP = Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/'
           '01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp')
RUN = CAP / 't07-l03-pagesize-001'
CORE = ROOT / 'target/release/workbuddy-proxy'
GPUI = ROOT / 'gpui/target/release/workbuddy-proxy-gpui'
ROWS = 60


def now_ms():
    return int(time.time() * 1000)


def today_start_ms():
    lt = time.localtime()
    return int(time.mktime((lt.tm_year, lt.tm_mon, lt.tm_mday, 0, 0, 0, 0, 0, -1)) * 1000)


def free_port():
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]


def env_for(config):
    env = dict(os.environ)
    env.update({
        'HOME': str(RUN / 'home'),
        'TMPDIR': str(RUN / 'tmp'),
        'TMP': str(RUN / 'tmp'),
        'TEMP': str(RUN / 'tmp'),
        'WORKBUDDY_PROXY_CONFIG_DIR': str(config),
        'XDG_CONFIG_HOME': str(RUN / 'home/.config'),
        'XDG_DATA_HOME': str(RUN / 'home/.local/share'),
        'XDG_CACHE_HOME': str(RUN / 'home/.cache'),
    })
    return env


def write_config(config, port):
    payload = {
        'local_key': 't07-l03-local-key',
        'language': 'zh',
        'port': port,
        'autostart': False,
        'providers': [],
        'installed': [],
    }
    config.mkdir(parents=True, exist_ok=True)
    path = config / 'config.json'
    path.write_text(json.dumps(payload, ensure_ascii=False), encoding='utf-8')
    os.chmod(path, 0o600)


def instance(config):
    return json.loads((config / 'instance.json').read_text(encoding='utf-8'))


def command(config, port, payload, timeout=10):
    key = instance(config)['management_key']
    request = urllib.request.Request(
        'http://127.0.0.1:%d/_control/action' % port,
        data=json.dumps(payload).encode(),
        headers={'Content-Type': 'application/json',
                 'Authorization': 'Bearer ' + key})
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with opener.open(request, timeout=timeout) as response:
        body = json.loads(response.read())
    message = body.get('message', '')
    return json.loads(message) if isinstance(message, str) else message


def stats(config, port, command_name, **fields):
    payload = {'command': command_name, 'request_tag': 't07-l03'}
    payload.update(fields)
    return command(config, port, payload)


def wait_ready(config, port, timeout=30):
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        try:
            value = stats(config, port, 'stats_health')
            if value.get('state') == 'ready':
                return value
        except Exception as failure:  # core not listening yet
            last = failure
        time.sleep(0.1)
    raise RuntimeError('core stats not ready: %r' % (last,))


def stop_pid(pid, name):
    if not pid:
        return
    try:
        os.kill(pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    except PermissionError:
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


def stop_run():
    state_file = RUN / 'run.json'
    if not state_file.exists():
        return
    state = json.loads(state_file.read_text(encoding='utf-8'))
    stop_pid(state.get('gpui_pid'), 'gpui')
    config = RUN / 'config'
    port = state.get('port')
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
    stop_pid(state.get('core_pid'), 'core')


def start_core():
    config = RUN / 'config'
    port = free_port()
    write_config(config, port)
    log = open(RUN / 'core.log', 'ab')
    process = subprocess.Popen(
        [str(CORE), '--headless', '--config-dir', str(config), '--port', str(port)],
        env=env_for(config), stdout=log, stderr=log, cwd=str(RUN))
    for _ in range(600):
        if process.poll() is not None:
            raise RuntimeError('core exited during startup; see %s' % (RUN / 'core.log'))
        try:
            if instance(config).get('port') == port:
                break
        except (OSError, ValueError, KeyError):
            pass
        time.sleep(0.05)
    else:
        process.kill()
        raise RuntimeError('core instance.json did not appear')
    wait_ready(config, port)
    return process, port


def insert_rows(config):
    db_path = config / 'usage.sqlite3'
    now = now_ms()
    base = now - 3_600_000
    with sqlite3.connect(db_path) as db:
        db.execute('PRAGMA foreign_keys=ON')
        run_id = db.execute(
            'SELECT run_id FROM stats_runs ORDER BY started_at_ms DESC LIMIT 1').fetchone()
        if run_id is None:
            run_id = ('t07-l03-run',)
            db.execute(
                'INSERT INTO stats_runs(run_id,started_at_ms,admitted_requests) VALUES(?,?,0)',
                (run_id[0], now - 7_200_000))
        run_id = run_id[0]
        columns = (
            'id,request_id,run_id,source,provider_id,route_id,provider_name,route_name,'
            'requested_model,upstream_model,response_model,started_at_ms,ended_at_ms,'
            'duration_ms,ttfb_ms,first_content_ms,stream,upstream_started,http_status,'
            'outcome,usage_status,usage_source,parser_version,input_tokens,output_tokens,'
            'total_tokens,cache_read_tokens,cache_write_tokens,reasoning_tokens,'
            'input_uncached_tokens,input_breakdown,record_origin,time_basis,'
            'total_tokens_derived,upstream_protocol,created_at_ms,updated_at_ms')
        for index in range(1, ROWS + 1):
            started = base + index * 30_000
            input_tokens = 1_000 + index
            output_tokens = 100
            value = {
                'id': index,
                'request_id': 't07l03-%04d' % index,
                'run_id': run_id,
                'source': 'relay',
                'provider_id': 'synth-provider',
                'route_id': 'synth-route',
                'provider_name': 'Synthetic Provider',
                'route_name': 'Synthetic Route',
                'requested_model': 'synth-model-%s' % ('a' if index % 2 else 'b'),
                'upstream_model': 'synth-model-%s' % ('a' if index % 2 else 'b'),
                'response_model': 'synth-model-%s' % ('a' if index % 2 else 'b'),
                'started_at_ms': started,
                'ended_at_ms': started + 500,
                'duration_ms': 500,
                'ttfb_ms': 100,
                'first_content_ms': 200,
                'stream': 0,
                'upstream_started': 1,
                'http_status': 200,
                'outcome': 'completed',
                'usage_status': 'complete',
                'usage_source': 'upstream',
                'parser_version': 1,
                'input_tokens': input_tokens,
                'output_tokens': output_tokens,
                'total_tokens': input_tokens + output_tokens,
                'cache_read_tokens': 0,
                'cache_write_tokens': 0,
                'reasoning_tokens': 0,
                'input_uncached_tokens': None,
                'input_breakdown': 'unknown',
                'record_origin': 'proxy',
                'time_basis': 'request_start',
                'total_tokens_derived': 0,
                'upstream_protocol': 'openai_chat',
                'created_at_ms': started,
                'updated_at_ms': started + 500,
            }
            db.execute(
                'INSERT INTO usage_requests(%s) VALUES(%s)'
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
        span = db.execute('SELECT MIN(started_at_ms),MAX(started_at_ms) FROM usage_requests').fetchone()
    return count, span, run_id


def record_backend(config, port):
    window = {'from_ms': today_start_ms(), 'to_ms': now_ms() + 60_000,
              'timezone': '', 'include_tests': False}
    samples = []
    for size in (20, 50, 100):
        page = stats(config, port, 'stats_requests', filter=window, page_size=size)
        samples.append({
            'page_size': size,
            'total_rows': page['total_rows'],
            'returned': len(page['items']),
            'next_cursor': page.get('next_cursor'),
            'first_request_id': page['items'][0]['request_id'] if page['items'] else None,
            'last_request_id': page['items'][-1]['request_id'] if page['items'] else None,
            'ids': [row['request_id'] for row in page['items']],
        })
    # Also walk pages the way the GUI does: page 1 -> 2 -> 3 with size 20, then
    # the same query at size 50 from page 1.
    walk = []
    cursor = None
    for page_number in range(1, 4):
        fields = {'filter': window, 'page_size': 20}
        if cursor is not None:
            fields['cursor'] = cursor
        page = stats(config, port, 'stats_requests', **fields)
        walk.append({
            'page_size': 20,
            'page_number': page_number,
            'returned': len(page['items']),
            'first_request_id': page['items'][0]['request_id'],
            'last_request_id': page['items'][-1]['request_id'],
            'next_cursor_present': page.get('next_cursor') is not None,
        })
        cursor = page.get('next_cursor')
        if cursor is None:
            break
    result = {'window': window, 'samples': samples, 'walk_size_20': walk}
    (EV / 'core-api-samples.json').write_text(
        json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
    return result


def setup():
    if RUN.exists():
        stop_run()
        shutil.rmtree(RUN)
    RUN.mkdir(parents=True)
    (RUN / 'home').mkdir()
    (RUN / 'tmp').mkdir()
    process, port = start_core()
    config = RUN / 'config'
    count, span, run_id = insert_rows(config)
    samples = record_backend(config, port)
    state = {
        'core_pid': process.pid,
        'gpui_pid': None,
        'port': port,
        'config': str(config),
        'db': str(config / 'usage.sqlite3'),
        'rows': count,
        'run_id': run_id,
        'started_ms': span[0],
        'ended_ms': span[1],
        'started_at': time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(span[0] / 1000)),
        'ended_at': time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(span[1] / 1000)),
        'created_ms': now_ms(),
    }
    (RUN / 'run.json').write_text(json.dumps(state, indent=2), encoding='utf-8')
    print(json.dumps({'state': state, 'count': count,
                      'samples': [{'page_size': s['page_size'], 'total_rows': s['total_rows'],
                                   'returned': s['returned']} for s in samples['samples']]},
                     ensure_ascii=False, indent=2))


def status():
    state = json.loads((RUN / 'run.json').read_text(encoding='utf-8'))
    config = RUN / 'config'
    port = state['port']
    live = {}
    try:
        live['health'] = stats(config, port, 'stats_health')
        live['requests'] = stats(config, port, 'stats_requests',
                                 filter={'from_ms': today_start_ms(),
                                         'to_ms': now_ms() + 60_000,
                                         'timezone': '', 'include_tests': False},
                                 page_size=20)
        live['requests'].pop('items', None)
    except Exception as failure:
        live['error'] = repr(failure)
    print(json.dumps({'state': state, 'live': live}, ensure_ascii=False, indent=2))


def stop():
    stop_run()


def cleanup():
    stop_run()
    shutil.rmtree(RUN, ignore_errors=True)
    print('removed', RUN)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('command', choices=['setup', 'status', 'stop', 'cleanup'])
    args = parser.parse_args()
    globals()[args.command]()

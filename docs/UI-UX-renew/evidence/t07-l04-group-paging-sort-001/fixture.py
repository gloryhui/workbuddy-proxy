#!/usr/bin/env python3
"""Isolated fixture for UI-T07-L04-GROUP-PAGING-SORT-001.

Creates a throw-away config directory under the session TMPDIR, starts the
current-head core on 127.0.0.1 with a random port, inserts a synthetic usage
matrix with 25 distinct provider groups and 27 distinct model groups into that
core's SQLite database, then records the backend grouping replies for the
acceptance walk (providers/models x sort=tokens/requests x page sizes).

No real profile, credential, database or upstream is read or written.

Commands:
  setup    start core, insert the synthetic matrix, record API samples, write run.json
  status   print the recorded run state and the live API grouping totals
  stop     stop the GPUI and core processes recorded in run.json
  cleanup  stop then remove only this task's temporary run directory
"""
import argparse
import json
import os
import shutil
import signal
import socket
import sqlite3
import subprocess
import time
import urllib.request
from pathlib import Path

ROOT = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign')
EV = ROOT / 'docs/UI-UX-renew/evidence/t07-l04-group-paging-sort-001'
CAP = Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/'
           '01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t07-l04-group-paging-sort-001')
RUN = CAP / 'run'
CORE = ROOT / 'target/release/workbuddy-proxy'
GPUI = ROOT / 'gpui/target/release/workbuddy-proxy-gpui'
TOTAL_ROWS = 60

# Provider row counts: 25 distinct providers (5 + 7*3 + 9*2 + 8*2 = 60).
PROVIDER_COUNTS = {25: 5}
PROVIDER_COUNTS.update({p: 3 for p in range(10, 17)})
PROVIDER_COUNTS.update({p: 2 for p in range(1, 10)})
PROVIDER_COUNTS.update({p: 2 for p in range(17, 25)})
# Model row counts: 27 distinct models (6*3 + 21*2 = 60).
MODEL_COUNTS = {m: 3 for m in range(1, 7)}
MODEL_COUNTS.update({m: 2 for m in range(7, 28)})

# Fixed (provider, model) pairs keep the two special token stories apart:
# provider 7 / 3 must lead sort=tokens without inflating a model group past
# model 20, and model 20 / 2 must lead model sort=tokens without inflating a
# provider group past provider 7.
SPECIAL_PAIRS = [(7, 15), (7, 16), (3, 13), (3, 14),
                 (1, 20), (2, 20), (5, 2), (6, 2), (24, 2)]


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
        'local_key': 't07-l04-local-key',
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
    payload = {'command': command_name, 'request_tag': 't07-l04'}
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


def stop_pid(pid):
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
    stop_pid(state.get('gpui_pid'))
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
    stop_pid(state.get('core_pid'))


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


def build_matrix():
    """Return 60 rows of (provider_id, provider_name, model, total_tokens)."""
    prov_slots = [p for p, count in sorted(PROVIDER_COUNTS.items()) for _ in range(count)]
    model_slots = [m for m, count in sorted(MODEL_COUNTS.items()) for _ in range(count)]
    assert len(prov_slots) == TOTAL_ROWS and len(model_slots) == TOTAL_ROWS
    for provider, model in SPECIAL_PAIRS:
        prov_slots.remove(provider)
        model_slots.remove(model)
    # Pair the remaining occurrences deterministically.
    pairs = list(SPECIAL_PAIRS) + list(zip(sorted(prov_slots), sorted(model_slots)))
    assert len(pairs) == TOTAL_ROWS
    rows = []
    for index, (provider, model) in enumerate(pairs):
        if provider == 7:
            total = 1_500_000
        elif provider == 3:
            total = 1_250_000
        elif model == 20:
            total = 2_000_000
        elif model == 2:
            total = 300_000
        else:
            total = 1_000 + index * 37
        rows.append({
            'provider_id': 'prov-%02d' % provider,
            'provider_name': 'Provider %02d' % provider,
            'model': 'model-%02d' % model,
            'total_tokens': total,
        })
    return rows


def insert_rows(config):
    db_path = config / 'usage.sqlite3'
    now = now_ms()
    base = now - 3_600_000
    rows = build_matrix()
    with sqlite3.connect(db_path) as db:
        db.execute('PRAGMA foreign_keys=ON')
        run_id = db.execute(
            'SELECT run_id FROM stats_runs ORDER BY started_at_ms DESC LIMIT 1').fetchone()
        if run_id is None:
            run_id = ('t07-l04-run',)
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
        for index, spec in enumerate(rows, start=1):
            started = base + index * 30_000
            total = spec['total_tokens']
            input_tokens = total // 2
            output_tokens = total - input_tokens
            value = {
                'id': index,
                'request_id': 't07l04-%04d' % index,
                'run_id': run_id,
                'source': 'relay',
                'provider_id': spec['provider_id'],
                'route_id': 'route-' + spec['provider_id'],
                'provider_name': spec['provider_name'],
                'route_name': 'Route ' + spec['provider_id'],
                'requested_model': spec['model'],
                'upstream_model': spec['model'],
                'response_model': spec['model'],
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
                'total_tokens': total,
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
        providers = db.execute(
            'SELECT COUNT(DISTINCT provider_id) FROM usage_requests').fetchone()[0]
        models = db.execute(
            'SELECT COUNT(DISTINCT upstream_model) FROM usage_requests').fetchone()[0]
        span = db.execute('SELECT MIN(started_at_ms),MAX(started_at_ms) FROM usage_requests').fetchone()
    return count, providers, models, span, run_id


def page_fields(window, group_by, sort, limit, cursor=None):
    fields = {'filter': window, 'group_by': group_by, 'sort': sort, 'limit': limit}
    if cursor is not None:
        fields['cursor'] = cursor
    return fields


def compact_page(page):
    return {
        'group_by': page['group_by'],
        'sort': page['sort'],
        'total_groups': page['total_groups'],
        'returned': len(page['items']),
        'keys': [item['key'] for item in page['items']],
        'labels': [item['label'] for item in page['items']],
        'first': page['items'][0] if page['items'] else None,
        'last': page['items'][-1] if page['items'] else None,
        'next_cursor': page.get('next_cursor'),
    }


def walk(config, port, window, group_by, sort, limit):
    cursor = None
    pages = []
    number = 0
    while True:
        number += 1
        page = stats(config, port, 'stats_breakdown',
                     **page_fields(window, group_by, sort, limit, cursor))
        pages.append(dict(compact_page(page), page_number=number))
        cursor = page.get('next_cursor')
        if cursor is None or number >= 10:
            break
    return pages


def record_backend(config, port, expected):
    window = {'from_ms': today_start_ms(), 'to_ms': now_ms() + 60_000,
              'timezone': '', 'include_tests': False}
    samples = {'window': window, 'expected': expected, 'groups': {}}
    for group_by in ('provider', 'model'):
        entry = {'sorts': {}, 'page_sizes': {}, 'walk_20': {}}
        for sort in ('tokens', 'requests'):
            net_sort = 'total_tokens' if sort == 'tokens' else 'request_count'
            page = stats(config, port, 'stats_breakdown',
                         **page_fields(window, group_by, net_sort, 20))
            entry['sorts'][sort] = compact_page(page)
            entry['walk_20'][sort] = walk(config, port, window, group_by, net_sort, 20)
        for size in (20, 50, 100):
            page = stats(config, port, 'stats_breakdown',
                         **page_fields(window, group_by, 'total_tokens', size))
            entry['page_sizes'][str(size)] = compact_page(page)
        samples['groups'][group_by] = entry

    # A provider cursor must not be accepted for a model listing (and the
    # reverse): the core validates grouping + sort in validate_breakdown_cursor.
    provider_cursor = samples['groups']['provider']['sorts']['tokens']['next_cursor']
    model_cursor = samples['groups']['model']['sorts']['tokens']['next_cursor']
    cross = {}
    for name, group_by, cursor in (
            ('provider_cursor_on_model', 'model', provider_cursor),
            ('model_cursor_on_provider', 'provider', model_cursor)):
        try:
            value = stats(config, port, 'stats_breakdown',
                          **page_fields(window, group_by, 'total_tokens', 20, cursor))
            cross[name] = {'accepted': True, 'returned': len(value['items'])}
        except Exception as failure:
            body = getattr(failure, 'read', lambda: b'')()
            cross[name] = {'accepted': False, 'error': body.decode('utf-8', 'replace')[:400]}
    # A cursor from sort=tokens must not be accepted for sort=requests.
    try:
        value = stats(config, port, 'stats_breakdown',
                      **page_fields(window, 'provider', 'request_count', 20,
                                    samples['groups']['provider']['sorts']['tokens']['next_cursor']))
        cross['tokens_cursor_on_requests'] = {'accepted': True, 'returned': len(value['items'])}
    except Exception as failure:
        body = getattr(failure, 'read', lambda: b'')()
        cross['tokens_cursor_on_requests'] = {
            'accepted': False, 'error': body.decode('utf-8', 'replace')[:400]}
    samples['cross_cursor'] = cross
    (EV / 'core-api-samples.json').write_text(
        json.dumps(samples, ensure_ascii=False, indent=2), encoding='utf-8')

    # Independent SQL expectation for the first page of each sort.
    db_path = config / 'usage.sqlite3'
    with sqlite3.connect(db_path) as db:
        sql_expected = {}
        for group_by, key in (('provider', 'provider_id'), ('model', 'upstream_model')):
            sql_expected[group_by] = {
                'groups': db.execute(
                    'SELECT COUNT(DISTINCT %s) FROM usage_requests' % key).fetchone()[0],
                'tokens': [list(row) for row in db.execute(
                    'SELECT %s, SUM(total_tokens) AS m FROM usage_requests GROUP BY %s '
                    'ORDER BY m DESC, %s ASC LIMIT 20' % (key, key, key)).fetchall()],
                'requests': [list(row) for row in db.execute(
                    'SELECT %s, COUNT(*) AS m FROM usage_requests GROUP BY %s '
                    'ORDER BY m DESC, %s ASC LIMIT 20' % (key, key, key)).fetchall()],
            }
    samples['sql_expected'] = sql_expected
    (EV / 'core-api-samples.json').write_text(
        json.dumps(samples, ensure_ascii=False, indent=2), encoding='utf-8')
    return samples


def setup():
    if RUN.exists():
        stop_run()
        shutil.rmtree(RUN)
    RUN.mkdir(parents=True)
    (RUN / 'home').mkdir()
    (RUN / 'tmp').mkdir()
    process, port = start_core()
    config = RUN / 'config'
    count, providers, models, span, run_id = insert_rows(config)
    expected = {'rows': count, 'providers': providers, 'models': models,
                'provider_token_leader': 'prov-07',
                'provider_request_leader': 'prov-25',
                'model_token_leader': 'model-20',
                'model_request_leader': 'model-01'}
    samples = record_backend(config, port, expected)
    checks = {
        'rows_60': count == 60,
        'providers_25': providers == 25,
        'models_27': models == 27,
        'provider_total_groups': samples['groups']['provider']['sorts']['tokens']['total_groups'] == 25,
        'model_total_groups': samples['groups']['model']['sorts']['tokens']['total_groups'] == 27,
        'provider_tokens_leader': samples['groups']['provider']['sorts']['tokens']['keys'][0] == 'prov-07',
        'provider_requests_leader': samples['groups']['provider']['sorts']['requests']['keys'][0] == 'prov-25',
        'model_tokens_leader': samples['groups']['model']['sorts']['tokens']['keys'][0] == 'model-20',
        'model_requests_leader': samples['groups']['model']['sorts']['requests']['keys'][0] == 'model-01',
        'provider_page2_count': len(samples['groups']['provider']['walk_20']['tokens']) == 2 and samples['groups']['provider']['walk_20']['tokens'][1]['returned'] == 5,
        'model_page2_count': len(samples['groups']['model']['walk_20']['tokens']) == 2 and samples['groups']['model']['walk_20']['tokens'][1]['returned'] == 7,
    }
    state = {
        'core_pid': process.pid,
        'gpui_pid': None,
        'port': port,
        'config': str(config),
        'db': str(config / 'usage.sqlite3'),
        'rows': count,
        'providers': providers,
        'models': models,
        'run_id': run_id,
        'started_at': time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(span[0] / 1000)),
        'ended_at': time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(span[1] / 1000)),
        'checks': checks,
        'created_ms': now_ms(),
    }
    (RUN / 'run.json').write_text(json.dumps(state, indent=2), encoding='utf-8')
    print(json.dumps({'state': state,
                      'provider_first_keys': samples['groups']['provider']['sorts']['tokens']['keys'][:3],
                      'provider_request_first_keys': samples['groups']['provider']['sorts']['requests']['keys'][:3],
                      'model_first_keys': samples['groups']['model']['sorts']['tokens']['keys'][:3],
                      'model_request_first_keys': samples['groups']['model']['sorts']['requests']['keys'][:3],
                      'cross_cursor': samples['cross_cursor']},
                     ensure_ascii=False, indent=2))


def status():
    state = json.loads((RUN / 'run.json').read_text(encoding='utf-8'))
    config = RUN / 'config'
    port = state['port']
    live = {}
    window = {'from_ms': today_start_ms(), 'to_ms': now_ms() + 60_000,
              'timezone': '', 'include_tests': False}
    try:
        live['health'] = stats(config, port, 'stats_health')
        for group_by in ('provider', 'model'):
            page = stats(config, port, 'stats_breakdown',
                         **page_fields(window, group_by, 'total_tokens', 20))
            live[group_by] = {'total_groups': page['total_groups'],
                              'first': page['items'][0]['key'],
                              'returned': len(page['items'])}
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

#!/usr/bin/env python3
"""Isolated synthetic fixture for UI-T07-L02-RESIDUAL-002.

Creates a throw-away config directory under this task's own TMPDIR, starts the
current-HEAD core on 127.0.0.1 with a random port, and inserts exactly 45
synthetic request-log rows into that core's SQLite database.  No real profile,
credential, history database, API key or upstream network is read or written.

Rows are ordered oldest -> newest; the GUI lists newest first.  The newest 11
rows (ids 35-45) carry the FX03 / FX05 / FX07 representative values so the first
table screen already shows ordinary, large and unknown values:

  45  large token counts (1.23亿 / 9877万 / 2.22亿) + ¥2.2200       FX07/FX05
  44  over-long Chinese provider + long English model id            FX03
  43  in-flight (unknown token/duration/status)                     FX05
  42  failed and unpriced (HTTP 429, 未定价)                         FX05
  41  partial usage (用量不全)                                       FX05
  40  zero usage (0 / — hit rate, ¥0)                               FX05
  39  WorkBuddy built-in import, credits 1.14                       FX05 source
  38  WorkBuddy custom import, credits 0                            FX05 source
  37  below-floor amount ¥0.00005 + long duration 123.46 s          FX07
  36  cancelled, 499, unsupported cache breakdown                   FX05
  35  illegal hit rate (cache read > input), unknown hit rate       FX05
  1-34 ordinary priced/default-zero rows for pages 2-3              FX07 paging

Commands:
  setup    start core, insert rows, record expectations + API samples
  status   print recorded state and live API totals
  stop     stop the core recorded in run.json
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
import urllib.error
import urllib.request
from datetime import datetime
from pathlib import Path

ROOT = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign')
EV = ROOT / 'docs/UI-UX-renew/evidence/t07-l02-residual-002'
CAP = Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/'
           '01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t07-l02-residual-002')
RUN = CAP / 'run'
CORE = ROOT / 'target/release/workbuddy-proxy'
ROWS = 45
LONG_PROVIDER = '供应商名称超长测试-ABCDEFGHIJKLMNOPQRSTUVWXYZ-0123456789'
LONG_MODEL = 'model-extremely-long-name-for-truncation-check-0123456789'


# ---------------------------------------------------------------- fixture rows
def proxy_row(rid, tag, provider, model, inp, out, total, cr, cw, uncached,
              breakdown, dur, ttfb, first, stream, http, outcome, usage,
              cost, error_kind=None, credit=None):
    return dict(id=rid, tag=tag, kind='proxy', provider_id='prov-%02d' % rid,
                provider_name=provider, model=model, inp=inp, out=out,
                total=total, cr=cr, cw=cw, uncached=uncached,
                breakdown=breakdown, dur=dur, ttfb=ttfb, first=first,
                stream=stream, http=http, outcome=outcome, usage=usage,
                cost=cost, error_kind=error_kind, credit=credit)


def default_cost(total=0, status='default_zero', currency='CNY', mode='flat',
                 source='default_zero'):
    return dict(status=status, currency=currency, total=total, mode=mode,
                price_source=source, rates=(0, 0, 0, 0))


def priced_cost(total, currency='CNY', rates=(2000000, 8000000, 200000, 4000000)):
    return dict(status='priced', currency=currency, total=total,
                mode='disjoint', price_source='manual', rates=rates)


def build_rows():
    rows = []
    # Ordinary rows 1-34: two cost shapes, alternating providers/models. They
    # fill pages 2 and 3 and prove the pagination/isolation of the log.
    for rid in range(1, 35):
        rows.append(proxy_row(
            rid, 'r%02d-ordinary' % rid,
            'Provider %02d' % rid, 'model-%02d' % rid,
            1000 + rid, 100, 1100 + rid, 0, 0, None, 'unknown',
            500, 100, 200, 0, 200, 'completed', 'complete',
            priced_cost(1234500000) if rid % 3 == 0 else default_cost()))
    rows.append(proxy_row(
        35, 'r35-illegal-hit-rate', 'Provider 35', 'model-hit-rate-illegal',
        0, 50, 50, 0, 0, None, 'unknown', 1000, 200, 300, 0, 200,
        'completed', 'complete', priced_cost(100000000)))
    rows.append(proxy_row(
        36, 'r36-cancelled', 'Provider 36', 'model-cancelled',
        None, None, None, None, None, None, 'unknown', 700, 300, 300, 0, 499,
        'cancelled', 'invalid',
        dict(status='unsupported_breakdown', currency='CNY', total=None,
             mode='read_discount', price_source='manual',
             rates=(3000000, 9000000, 300000, 6000000))))
    rows.append(proxy_row(
        37, 'r37-small-amount-long-duration', 'Provider 37', 'model-long-duration',
        10, 5, 15, 5, 5, 0, 'disjoint', 123456, 1000, 2000, 0, 200,
        'completed', 'complete', priced_cost(50000)))
    # Imported rows: record_origin drives the source pill; builtin/custom are
    # only visible after switching the source filter. They carry credits.
    rows.append(dict(
        id=38, tag='r38-import-custom', kind='imported', origin='workbuddy_custom',
        provider_id='workbuddy-custom', provider_name='WorkBuddy 自定义',
        model='wb-model-beta', inp=300, out=None, total=None, cr=None, cw=None,
        uncached=None, breakdown='unknown', dur=None, ttfb=None, first=None,
        stream=None, http=None, outcome='recorded', usage='partial', cost=None,
        error_kind=None, credit=0))
    rows.append(dict(
        id=39, tag='r39-import-builtin', kind='imported', origin='workbuddy_builtin',
        provider_id='workbuddy-builtin', provider_name='WorkBuddy 内置',
        model='wb-model-alpha', inp=2000, out=1000, total=3000, cr=500, cw=0,
        uncached=1500, breakdown='disjoint', dur=None, ttfb=None, first=None,
        stream=None, http=None, outcome='recorded', usage='complete', cost=None,
        error_kind=None, credit=1140000000))
    rows.append(proxy_row(
        40, 'r40-zero-usage', 'Provider 40', 'model-zero',
        0, 0, 0, 0, 0, None, 'unknown', 900, 200, 200, 0, 200,
        'completed', 'complete', default_cost()))
    rows.append(proxy_row(
        41, 'r41-partial-usage', 'Provider 41', 'model-partial',
        1000, None, None, None, None, None, 'unknown', 1200, 400, 400, 0, 200,
        'completed', 'partial',
        dict(status='usage_incomplete', currency='CNY', total=None,
             mode='flat', price_source='manual',
             rates=(3000000, 9000000, 300000, 6000000))))
    rows.append(proxy_row(
        42, 'r42-failed-unpriced', 'Provider 42', 'model-failed',
        None, None, None, None, None, None, 'unknown', 800, 300, 300, 0, 429,
        'failed', 'missing', None, error_kind='upstream_http'))
    rows.append(proxy_row(
        43, 'r43-in-flight', 'Provider 43', 'model-in-flight',
        None, None, None, None, None, None, 'unknown', None, None, None, 1, None,
        'in_flight', 'missing', None))
    rows.append(proxy_row(
        44, 'r44-long-names', LONG_PROVIDER, LONG_MODEL,
        999, 1, 1000, 500, 499, 0, 'disjoint', 2000, 300, 300, 0, 200,
        'completed', 'complete', priced_cost(1234500000)))
    rows.append(proxy_row(
        45, 'r45-large-tokens', 'Provider 45', 'model-large',
        123456789, 98765432, 222222221, 61728394, 0, 61728395, 'disjoint',
        5000, 1000, 1000, 1, 200, 'completed', 'complete',
        priced_cost(2222222211)))
    return rows


ROWS_DEF = build_rows()


# ---------------------------------------------------------------- formatting
def compact(value):
    if value is None:
        return '—'
    magnitude = abs(value)
    if magnitude >= 100_000_000:
        unit, scale = '亿', 100_000_000.0
    elif magnitude >= 10_000:
        unit, scale = '万', 10_000.0
    else:
        return str(value)
    scaled = value / scale
    if abs(scaled) >= 100.:
        text = '%.0f' % scaled
    elif abs(scaled) >= 10.:
        text = '%.1f' % scaled
    else:
        text = '%.2f' % scaled
    return text + unit


def percent(value):
    return '—' if value is None else '%.1f%%' % (value * 100.)


def cache_rate(read, inp):
    if read is None or inp is None or inp <= 0 or read < 0 or read > inp:
        return None
    return read / inp


def duration(value):
    if value is None:
        return '—'
    return '%d ms' % value if value < 1000 else '%.2f s' % (value / 1000.)


def rate_text(row):
    out, dur, first, stream = row.get('out'), row.get('dur'), row.get('first'), row.get('stream')
    if stream is None or out is None or out < 0 or dur is None or dur <= 0:
        return '—'
    elapsed = (dur - first) if stream else dur
    if elapsed is None or elapsed <= 0:
        return '—'
    return '%.1f' % (out * 1000. / elapsed)


def format_amount_text(text):
    value = int(text)
    if 0 < value < 100_000:
        return '<0.0001'
    whole, fraction = divmod(value, 1_000_000_000)
    if fraction == 0:
        return str(whole)
    rendered = ('%d.%09d' % (whole, fraction)).rstrip('0')
    if rendered.endswith('.'):
        rendered = rendered[:-1]
    return rendered


def amount(nanos, currency):
    if nanos is None:
        return '—'
    symbol = {'USD': '$', 'CNY': '¥'}.get(currency, '')
    return symbol + format_amount_text(nanos)


def format_credit(value):
    if value is None:
        return '—'
    whole, fraction = divmod(int(value), 1_000_000_000)
    if fraction == 0:
        return str(whole)
    return ('%d.%09d' % (whole, fraction)).rstrip('0')


def origin_text(origin):
    return {'proxy': '代理', 'workbuddy_builtin': '内置',
            'workbuddy_custom': '自定义'}[origin]


def status_text(outcome, recorded):
    if recorded:
        return '已记录'
    return {'in_flight': '进行中', 'completed': '完成', 'failed': '失败',
            'cancelled': '已取消', 'interrupted': '中断'}[outcome]


def usage_label(status):
    return {'complete': '完整', 'partial': '部分', 'missing': '缺失',
            'invalid': '无效', 'not_applicable': '不适用'}[status]


def cost_label(status):
    return {'priced': '', 'default_zero': '未配置价格，按 0 计算',
            'unpriced': '未定价', 'usage_incomplete': '用量不全',
            'unsupported_breakdown': '缓存计费数据不足'}.get(status, status)


def cost_text(row):
    cost = row.get('cost')
    status = cost['status'] if cost else 'unpriced'
    label = cost_label(status)
    if not label or status in ('default_zero', 'not_applicable'):
        return amount(None if not cost else (
            None if cost.get('total') is None else str(cost['total'])),
            cost.get('currency') if cost else None)
    return label


def timestamp(ms):
    return datetime.fromtimestamp(ms / 1000.).strftime('%m-%d %H:%M:%S')


def started_ms(rid):
    base = int(time.mktime((time.localtime().tm_year, time.localtime().tm_mon,
                            time.localtime().tm_mday, 8, 0, 0, 0, 0, -1)) * 1000)
    return base + rid * 120_000


def display_expectations(row, start):
    hit = cache_rate(row.get('cr'), row.get('inp'))
    recorded = row['kind'] == 'imported'
    http = '—' if row.get('http') is None else str(row['http'])
    tokens_line1 = '总Token %s · 输入 %s · 输出 %s' % (
        compact(row.get('total')), compact(row.get('inp')), compact(row.get('out')))
    tokens_line2 = '命中率 %s · 缓存读 %s' % (
        percent(hit), compact(row.get('cr')))
    return {
        'request_id': 't07l02-%04d-%s' % (row['id'], row['tag'].split('-')[0]),
        'time': timestamp(start),
        'source': origin_text(row.get('origin', 'proxy')),
        'provider': row['provider_name'],
        'model': row['model'],
        'token_line1': tokens_line1,
        'token_line2': tokens_line2,
        'hit_rate': percent(hit),
        'total_tokens': compact(row.get('total')),
        'input_tokens': compact(row.get('inp')),
        'output_tokens': compact(row.get('out')),
        'cache_read': compact(row.get('cr')),
        'cost': cost_text(row),
        'credit': format_credit(row.get('credit')),
        'latency': duration(row.get('dur')),
        'speed': rate_text(row),
        'http': http,
        'status': '%s · %s' % (http, status_text(row['outcome'], recorded)),
        'usage': usage_label(row['usage']),
    }


# ---------------------------------------------------------------- core control
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
        'HOME': str(RUN / 'home'), 'TMPDIR': str(CAP / 'tmp'),
        'TMP': str(CAP / 'tmp'), 'TEMP': str(CAP / 'tmp'),
        'WORKBUDDY_PROXY_CONFIG_DIR': str(config),
        'XDG_CONFIG_HOME': str(RUN / 'home/.config'),
        'XDG_DATA_HOME': str(RUN / 'home/.local/share'),
        'XDG_CACHE_HOME': str(RUN / 'home/.cache'),
    })
    return env


def write_config(config, port):
    payload = {'local_key': 't07-l02-local-key', 'language': 'zh', 'port': port,
               'autostart': False, 'providers': [], 'installed': []}
    config.mkdir(parents=True, exist_ok=True)
    path = config / 'config.json'
    path.write_text(json.dumps(payload, ensure_ascii=False), encoding='utf-8')
    os.chmod(path, 0o600)


def instance(config):
    return json.loads((config / 'instance.json').read_text(encoding='utf-8'))


def command(config, port, payload, timeout=15):
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
    payload = {'command': name, 'request_tag': 't07-l02'}
    payload.update(fields)
    return command(config, port, payload)


def wait_ready(config, port, timeout=30):
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        try:
            if stats(config, port, 'stats_health').get('state') == 'ready':
                return
        except Exception as failure:
            last = failure
        time.sleep(0.1)
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


def kill_stale_config():
    """Stop any core still running against this task's config directory."""
    config = RUN / 'config'
    result = subprocess.run(['ps', '-eo', 'pid=,command='], capture_output=True,
                            text=True)
    pids = []
    for line in result.stdout.splitlines():
        line = line.strip()
        if str(config) in line and 'workbuddy-proxy' in line:
            try:
                pids.append(int(line.split()[0]))
            except ValueError:
                pass
    for pid in pids:
        stop_pid(pid)
    return pids


def stop_run():
    state_file = RUN / 'run.json'
    if not state_file.exists():
        return
    state = json.loads(state_file.read_text(encoding='utf-8'))
    stop_pid(state.get('gpui_pid'))
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
    base = today_start_ms()
    with sqlite3.connect(db_path) as db:
        db.execute('PRAGMA foreign_keys=ON')
        run = db.execute(
            'SELECT run_id FROM stats_runs ORDER BY started_at_ms DESC LIMIT 1').fetchone()
        if run is None:
            run = ('t07-l02-run',)
            db.execute('INSERT INTO stats_runs(run_id,started_at_ms,admitted_requests) '
                       'VALUES(?,?,0)', (run[0], now_ms() - 7_200_000))
        run_id = run[0]
        recorded = {}
        for row in ROWS_DEF:
            start = started_ms(row['id'])
            recorded[row['id']] = start
            imported = row['kind'] == 'imported'
            value = {
                'id': row['id'],
                'request_id': 't07l02-%04d-%s' % (row['id'], row['tag'].split('-')[0]),
                'run_id': None if imported else run_id,
                'source': 'workbuddy_import' if imported else 'relay',
                'provider_id': row['provider_id'],
                'route_id': 'route-%02d' % row['id'],
                'provider_name': row['provider_name'],
                'route_name': 'Route %02d' % row['id'],
                'requested_model': row['model'],
                'upstream_model': row['model'],
                'response_model': row['model'],
                'started_at_ms': start,
                'ended_at_ms': start if imported else (
                    None if row['outcome'] == 'in_flight' else start + (row['dur'] or 0)),
                'duration_ms': None if imported else row['dur'],
                'ttfb_ms': None if imported else row['ttfb'],
                'first_content_ms': None if imported else row['first'],
                'stream': None if imported else row['stream'],
                'upstream_started': None if imported else 1,
                'http_status': None if imported else row['http'],
                'outcome': row['outcome'],
                'error_kind': None if imported else row.get('error_kind'),
                'usage_status': row['usage'],
                'usage_source': ('workbuddy_jsonl' if imported else
                                 'none' if row['usage'] in ('missing', 'invalid',
                                                            'not_applicable')
                                 else 'upstream'),
                'parser_version': 1,
                'input_tokens': row['inp'],
                'output_tokens': row['out'],
                'total_tokens': row['total'],
                'cache_read_tokens': row['cr'],
                'cache_write_tokens': row['cw'],
                'reasoning_tokens': None,
                'input_uncached_tokens': row['uncached'],
                'input_breakdown': row['breakdown'],
                'record_origin': row.get('origin', 'proxy'),
                'time_basis': 'message_timestamp' if imported else 'request_start',
                'total_tokens_derived': 0,
                'upstream_protocol': None if imported else 'openai_chat',
                'workbuddy_credit_nanos': row.get('credit'),
                'created_at_ms': start,
                'updated_at_ms': start if imported else start + (row['dur'] or 0),
            }
            columns = list(value)
            db.execute('INSERT INTO usage_requests(%s) VALUES(%s)'
                       % (','.join(columns), ','.join('?' for _ in columns)),
                       [value[key] for key in columns])
            cost = row.get('cost')
            if cost:
                rates = cost.get('rates') or (None, None, None, None)
                total = cost['total']
                parts = (None, None, None, None) if total is None else (
                    total - 3, 1, 1, 1) if total else (0, 0, 0, 0)
                db.execute(
                    'INSERT INTO usage_costs(request_id,price_source,price_snapshot_at_ms,'
                    'currency,input_mode,input_rate_nanos,output_rate_nanos,'
                    'cache_read_rate_nanos,cache_write_rate_nanos,status,'
                    'input_cost_nanos,output_cost_nanos,cache_read_cost_nanos,'
                    'cache_write_cost_nanos,total_cost_nanos,pricing_version,calculated_at_ms) '
                    'VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,2,?)',
                    (value['request_id'], cost['price_source'], start,
                     cost['currency'], cost['mode'], rates[0], rates[1],
                     rates[2], rates[3], cost['status'], parts[0], parts[1],
                     parts[2], parts[3], total, start))
        db.commit()
        count = db.execute('SELECT COUNT(*) FROM usage_requests').fetchone()[0]
        origins = dict(db.execute(
            'SELECT record_origin, COUNT(*) FROM usage_requests GROUP BY record_origin'
        ).fetchall())
    return count, origins, run_id, recorded


def filter_payload(scope='all', capture='proxy'):
    return {'from_ms': today_start_ms(), 'to_ms': now_ms() + 60_000,
            'timezone': '', 'provider_id': None, 'upstream_model': None,
            'route_id': None, 'outcome': None, 'usage_status': None,
            'include_tests': False, 'source_scope': scope,
            'capture_method': capture}


def record_backend(config, port, recorded):
    expected = {'rows': [], 'views': [], 'page_walk': []}
    for name, scope, capture in (('all-sources', 'all', 'proxy'),
                                 ('builtin', 'builtin', 'all'),
                                 ('custom', 'supplier', 'all')):
        page = stats(config, port, 'stats_requests',
                     filter=filter_payload(scope, capture), page_size=20)
        expected['views'].append({
            'view': name, 'source_scope': scope, 'capture_method': capture,
            'filter': filter_payload(scope, capture),
            'api_total_rows': page['total_rows'],
            'api_returned': len(page['items']),
            'request_ids': [item['request_id'] for item in page['items']],
            'api_rows': [{
                'request_id': item['request_id'],
                'started_at_ms': item['started_at_ms'],
                'record_origin': item['record_origin'],
                'provider_name': item['provider_name'],
                'upstream_model': item['upstream_model'],
                'input_tokens': item['input_tokens'],
                'output_tokens': item['output_tokens'],
                'total_tokens': item['total_tokens'],
                'cache_read_tokens': item['cache_read_tokens'],
                'duration_ms': item['duration_ms'],
                'http_status': item['http_status'],
                'outcome': item['outcome'],
                'usage_status': item['usage_status'],
                'workbuddy_credit_nanos': item['workbuddy_credit_nanos'],
                'cost': item['cost'],
            } for item in page['items']],
        })
    cursor = None
    walk_filter = filter_payload()
    for number in range(1, 4):
        fields = {'filter': walk_filter, 'page_size': 20}
        if cursor:
            fields['cursor'] = cursor
        page = stats(config, port, 'stats_requests', **fields)
        expected['page_walk'].append({
            'page': number, 'page_size': 20,
            'returned': len(page['items']),
            'first_request_id': page['items'][0]['request_id'] if page['items'] else None,
            'last_request_id': page['items'][-1]['request_id'] if page['items'] else None,
            'next_cursor_present': page.get('next_cursor') is not None,
        })
        cursor = page.get('next_cursor')
    for row in ROWS_DEF:
        expected['rows'].append({
            'id': row['id'], 'tag': row['tag'], 'kind': row['kind'],
            'origin': row.get('origin', 'proxy'),
            'request_id': 't07l02-%04d-%s' % (row['id'], row['tag'].split('-')[0]),
            'started_at_ms': recorded[row['id']],
            'expected': display_expectations(row, recorded[row['id']]),
            'db': {key: row.get(key) for key in
                   ('provider_name', 'model', 'http', 'outcome', 'stream',
                    'usage', 'inp', 'out', 'total', 'cr', 'cw', 'dur',
                    'ttfb', 'first', 'credit')},
        })
    (EV / 'expected.json').write_text(
        json.dumps(expected, ensure_ascii=False, indent=2), encoding='utf-8')
    return expected


def setup():
    stop_run()
    stale = kill_stale_config()
    if stale:
        print('stopped stale cores:', stale)
    if RUN.exists():
        shutil.rmtree(RUN)
    RUN.mkdir(parents=True)
    (RUN / 'home').mkdir()
    (CAP / 'tmp').mkdir(parents=True, exist_ok=True)
    process, port = start_core()
    try:
        config = RUN / 'config'
        count, origins, run_id, recorded = insert_rows(config)
        expected = record_backend(config, port, recorded)
    except Exception:
        stop_pid(process.pid)
        raise
    span = (min(recorded.values()), max(recorded.values()))
    state = {
        'core_pid': process.pid, 'gpui_pid': None, 'port': port,
        'config': str(config), 'db': str(config / 'usage.sqlite3'),
        'rows': count, 'origins': origins, 'run_id': run_id,
        'started_at': timestamp(span[0]), 'ended_at': timestamp(span[1]),
        'views': [{'view': v['view'], 'api_total_rows': v['api_total_rows'],
                   'api_returned': v['api_returned']} for v in expected['views']],
        'created_ms': now_ms(),
    }
    (RUN / 'run.json').write_text(json.dumps(state, indent=2), encoding='utf-8')
    print(json.dumps(state, ensure_ascii=False, indent=2))


def status():
    state = json.loads((RUN / 'run.json').read_text(encoding='utf-8'))
    live = {}
    try:
        live['health'] = stats(RUN / 'config', state['port'], 'stats_health')
        for name, scope, capture in (('all-sources', 'all', 'proxy'),
                                     ('builtin', 'builtin', 'all'),
                                     ('custom', 'supplier', 'all')):
            page = stats(RUN / 'config', state['port'], 'stats_requests',
                         filter=filter_payload(scope, capture), page_size=20)
            live[name] = {'total_rows': page['total_rows'],
                          'returned': len(page['items'])}
    except Exception as failure:
        live['error'] = repr(failure)
    print(json.dumps({'state': state, 'live': live}, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('command', choices=['setup', 'status', 'stop', 'cleanup'])
    args = parser.parse_args()
    if args.command == 'setup':
        setup()
    elif args.command == 'status':
        status()
    elif args.command == 'stop':
        stop_run()
    else:
        stop_run()
        shutil.rmtree(RUN, ignore_errors=True)
        print('removed', RUN)

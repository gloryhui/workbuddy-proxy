#!/usr/bin/env python3
"""Isolated fixture for UI-T07-L05-FULL-VALUES-001.

Builds one synthetic SQLite dataset inside a throw-away config directory under
this task's own TMPDIR, starts the current-head core on 127.0.0.1 with a random
port, and records the per-row backend values. The same dataset is later shown to
the pre-T07 GPUI binary and to the current GPUI binary, so every visible value
can be compared row by row on identical data.

The 16 fixture rows cover: complete success with the no-price default, priced
success (CNY and USD, streaming and not), a below-floor amount, zero usage,
partial usage, failure without a price, a call never sent upstream, an in-flight
row, a cancelled row with an unsupported cache breakdown, two imported
WorkBuddy records (built-in and custom) carrying credits, a large compact-number
row, an interrupted row, an earliest-of-day row, and a legacy-amount row with
over-long provider/model names.

No real profile, credential, database or upstream is read or written.

Commands:
  setup    start core, insert the synthetic rows, record expectations, write run.json
  status   print the recorded state plus the live row count from the core API
  stop     stop the core process recorded in run.json
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
from datetime import datetime
from pathlib import Path

ROOT = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign')
EV = ROOT / 'docs/UI-UX-renew/evidence/t07-l05-full-values-001'
CAP = Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/'
           '01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t07-l05-full-values-001')
RUN = CAP / 'run'
CORE = CAP / 'bin/core-current'

# ---------------------------------------------------------------- fixture rows
# Every value is chosen so the rendered text is unambiguous and distinct per row.
LONG_PROVIDER = '供应商名称超长测试-ABCDEFGHIJKLMNOPQRSTUVWXYZ-0123456789'
LONG_MODEL = 'model-extremely-long-name-for-truncation-check-0123456789'

# kind: proxy | imported
ROWS = [
    dict(id=1, tag='r01-complete-default-zero', kind='proxy',
         provider_id='prov-01', provider_name='Provider 01', model='model-01',
         outcome='completed', error_kind=None, http=200, stream=0,
         upstream_started=1, usage='complete', usage_source='upstream',
         inp=1200, out=300, total=1500, cr=0, cw=0, uncached=None,
         breakdown='unknown', dur=500, ttfb=100, first=200,
         cost=dict(status='default_zero', currency='CNY', total=0,
                   rates=(0, 0, 0, 0), mode='flat', price_source='default_zero'),
         credit=None, minute=1),
    dict(id=2, tag='r02-priced-streaming', kind='proxy',
         provider_id='prov-02', provider_name='Provider 02', model='model-02',
         outcome='completed', error_kind=None, http=200, stream=1,
         upstream_started=1, usage='complete', usage_source='upstream',
         inp=1000, out=500, total=1500, cr=600, cw=0, uncached=400,
         breakdown='disjoint', dur=2500, ttfb=200, first=300,
         cost=dict(status='priced', currency='CNY', total=1234500000,
                   rates=(2000000, 8000000, 200000, 4000000), mode='disjoint',
                   price_source='manual'),
         credit=None, minute=2),
    dict(id=3, tag='r03-priced-usd-full-hit', kind='proxy',
         provider_id='prov-03', provider_name='Provider 03', model='model-03',
         outcome='completed', error_kind=None, http=200, stream=0,
         upstream_started=1, usage='complete', usage_source='upstream',
         inp=800, out=200, total=1000, cr=800, cw=0, uncached=0,
         breakdown='disjoint', dur=1000, ttfb=150, first=300,
         cost=dict(status='priced', currency='USD', total=123450000,
                   rates=(1000000, 5000000, 100000, 2000000), mode='disjoint',
                   price_source='manual'),
         credit=None, minute=3),
    dict(id=4, tag='r04-tiny-amount-cache-write', kind='proxy',
         provider_id='prov-04', provider_name='Provider 04', model='model-04',
         outcome='completed', error_kind=None, http=200, stream=0,
         upstream_started=1, usage='complete', usage_source='upstream',
         inp=10, out=5, total=15, cr=5, cw=5, uncached=0,
         breakdown='disjoint', dur=100, ttfb=20, first=50,
         cost=dict(status='priced', currency='CNY', total=50000,
                   rates=(1000, 2000, 500, 1000), mode='disjoint',
                   price_source='manual'),
         credit=None, minute=4),
    dict(id=5, tag='r05-zero-usage', kind='proxy',
         provider_id='prov-05', provider_name='Provider 05', model='model-05',
         outcome='completed', error_kind=None, http=200, stream=0,
         upstream_started=1, usage='complete', usage_source='upstream',
         inp=0, out=0, total=0, cr=0, cw=0, uncached=None,
         breakdown='unknown', dur=900, ttfb=100, first=200,
         cost=dict(status='default_zero', currency='CNY', total=0,
                   rates=(0, 0, 0, 0), mode='flat', price_source='default_zero'),
         credit=None, minute=5),
    dict(id=6, tag='r06-partial-usage', kind='proxy',
         provider_id='prov-06', provider_name='Provider 06', model='model-06',
         outcome='completed', error_kind=None, http=200, stream=1,
         upstream_started=1, usage='partial', usage_source='upstream',
         inp=1000, out=None, total=None, cr=None, cw=None, uncached=None,
         breakdown='unknown', dur=1200, ttfb=300, first=400,
         cost=dict(status='usage_incomplete', currency='CNY', total=None,
                   rates=(3000000, 9000000, 300000, 6000000), mode='flat',
                   price_source='manual'),
         credit=None, minute=6),
    dict(id=7, tag='r07-failed-unpriced', kind='proxy',
         provider_id='prov-07', provider_name='Provider 07', model='model-07',
         outcome='failed', error_kind='upstream_http', http=429, stream=0,
         upstream_started=1, usage='missing', usage_source='none',
         inp=None, out=None, total=None, cr=None, cw=None, uncached=None,
         breakdown='unknown', dur=800, ttfb=150, first=300,
         cost=None, credit=None, minute=7),
    dict(id=8, tag='r08-failed-not-sent', kind='proxy',
         provider_id='prov-08', provider_name='Provider 08', model='model-08',
         outcome='failed', error_kind='connect', http=None, stream=0,
         upstream_started=0, usage='not_applicable', usage_source='none',
         inp=None, out=None, total=None, cr=None, cw=None, uncached=None,
         breakdown='unknown', dur=300, ttfb=50, first=100,
         cost=dict(status='not_applicable', currency='CNY', total=0,
                   rates=(0, 0, 0, 0), mode='flat', price_source='manual'),
         credit=None, minute=8),
    dict(id=9, tag='r09-in-flight', kind='proxy',
         provider_id='prov-09', provider_name='Provider 09', model='model-09',
         outcome='in_flight', error_kind=None, http=None, stream=1,
         upstream_started=None, usage='missing', usage_source='none',
         inp=None, out=None, total=None, cr=None, cw=None, uncached=None,
         breakdown='unknown', dur=None, ttfb=None, first=None,
         cost=None, credit=None, minute=9),
    dict(id=10, tag='r10-cancelled-unsupported', kind='proxy',
         provider_id='prov-10', provider_name='Provider 10', model='model-10',
         outcome='cancelled', error_kind=None, http=499, stream=1,
         upstream_started=1, usage='invalid', usage_source='upstream',
         inp=None, out=None, total=None, cr=None, cw=None, uncached=None,
         breakdown='unknown', dur=700, ttfb=200, first=300,
         cost=dict(status='unsupported_breakdown', currency='CNY', total=None,
                   rates=(3000000, 9000000, 300000, 6000000), mode='read_discount',
                   price_source='manual'),
         credit=None, minute=10),
    dict(id=11, tag='r11-import-builtin-credit', kind='imported',
         origin='workbuddy_builtin', provider_id='wb:builtin',
         provider_name='WorkBuddy 内置', model='wb-model-alpha',
         outcome='recorded', usage='complete', usage_source='workbuddy_jsonl',
         inp=2000, out=1000, total=3000, cr=500, cw=0, uncached=1500,
         breakdown='disjoint', cost=None, credit=1140000000, minute=11),
    dict(id=12, tag='r12-import-custom-credit-zero', kind='imported',
         origin='workbuddy_custom', provider_id='wb:custom',
         provider_name='WorkBuddy 自定义', model='wb-model-beta',
         outcome='recorded', usage='partial', usage_source='workbuddy_jsonl',
         inp=300, out=None, total=None, cr=None, cw=None, uncached=None,
         breakdown='unknown', cost=None, credit=0, minute=12),
    dict(id=13, tag='r13-large-compact', kind='proxy',
         provider_id='prov-13', provider_name='Provider 13', model='model-13',
         outcome='completed', error_kind=None, http=200, stream=0,
         upstream_started=1, usage='complete', usage_source='upstream',
         inp=123456789, out=98765432, total=222222221, cr=61728394, cw=0,
         uncached=61728395, breakdown='disjoint', dur=5000, ttfb=500, first=1000,
         cost=dict(status='priced', currency='CNY', total=2220000000,
                   rates=(1000000, 3000000, 100000, 2000000), mode='disjoint',
                   price_source='manual'),
         credit=None, minute=13),
    dict(id=14, tag='r14-interrupted', kind='proxy',
         provider_id='prov-14', provider_name='Provider 14', model='model-14',
         outcome='interrupted', error_kind='client_drop', http=500, stream=1,
         upstream_started=1, usage='partial', usage_source='upstream',
         inp=500, out=100, total=600, cr=0, cw=0, uncached=None,
         breakdown='unknown', dur=160, ttfb=60, first=80,
         cost=dict(status='usage_incomplete', currency='CNY', total=None,
                   rates=(1000000, 3000000, 100000, 2000000), mode='flat',
                   price_source='manual'),
         credit=None, minute=14),
    dict(id=15, tag='r15-earliest-below-floor', kind='proxy',
         provider_id='prov-15', provider_name='Provider 15', model='model-15',
         outcome='completed', error_kind=None, http=201, stream=0,
         upstream_started=1, usage='complete', usage_source='upstream',
         inp=42, out=8, total=50, cr=0, cw=0, uncached=None,
         breakdown='unknown', dur=1000, ttfb=0, first=500,
         cost=dict(status='priced', currency='CNY', total=5000,
                   rates=(1000, 2000, 500, 1000), mode='flat',
                   price_source='manual'),
         credit=None, minute=15),
    dict(id=16, tag='r16-legacy-long-names', kind='proxy',
         provider_id='prov-16', provider_name=LONG_PROVIDER, model=LONG_MODEL,
         outcome='completed', error_kind=None, http=200, stream=1,
         upstream_started=1, usage='complete', usage_source='upstream',
         inp=999, out=1, total=1000, cr=500, cw=499, uncached=0,
         breakdown='disjoint', dur=2000, ttfb=100, first=300,
         cost=dict(status='legacy_unknown', currency='CNY', total=777000000,
                   rates=(None, None, None, None), mode=None,
                   price_source='legacy_unknown'),
         credit=None, minute=16),
]

# ------------------------------------------------------- display formatters
# Independent Python re-implementation of the display rules both binaries share
# (usage_table.rs). The pre-T07 and post-T07 sources agree on every one of them;
# the point of the exercise is to check the rendered pixels against this.


def compact(value, zh=True):
    if value is None:
        return '—'
    magnitude = abs(value)
    if zh:
        if magnitude >= 100_000_000:
            unit, scale = '亿', 100_000_000.0
        elif magnitude >= 10_000:
            unit, scale = '万', 10_000.0
        else:
            return str(value)
    else:
        if magnitude >= 1_000_000_000:
            unit, scale = 'B', 1_000_000_000.0
        elif magnitude >= 1_000_000:
            unit, scale = 'M', 1_000_000.0
        elif magnitude >= 1_000:
            unit, scale = 'k', 1_000.0
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


def output_rate(out, dur, first, stream):
    if out is None or out < 0:
        return None
    if dur is None or dur <= 0:
        return None
    if stream:
        if first is None or first < 0:
            return None
        elapsed = dur - first
        if elapsed <= 0:
            return None
    else:
        elapsed = dur
    return out * 1000. / elapsed


def rate_text(row):
    stream = row.get('stream')
    if stream is None:
        return '—'
    rate = output_rate(row.get('out'), row.get('dur'), row.get('first'), stream)
    return '—' if rate is None else '%.1f' % rate


def duration(value):
    if value is None:
        return '—'
    if value < 1000:
        return '%d ms' % value
    return '%.2f s' % (value / 1000.)


def format_amount_text(text):
    if text is None:
        return None
    text = text.strip()
    negative = text.startswith('-')
    digits = text[1:] if negative else text
    if not digits or not digits.isdigit():
        return '—'
    value = int(digits)
    if 0 < value < 100_000:
        return '>-0.0001' if negative else '<0.0001'
    whole, fraction = divmod(value, 1_000_000_000)
    if fraction == 0:
        return ('-' if negative else '') + str(whole)
    rendered = '%d.%09d' % (whole, fraction)
    while rendered.endswith('0') and len(rendered) - rendered.find('.') > 5:
        rendered = rendered[:-1]
    return ('-' if negative else '') + rendered


def amount(nanos, currency):
    if nanos is None:
        return '—'
    symbol = {'USD': '$', 'CNY': '¥'}.get(currency, '')
    return symbol + format_amount_text(nanos)


def format_credit_nanos(text):
    if text is None or text == '' or not text.isdigit():
        return None
    nanos = int(text)
    whole, fraction = divmod(nanos, 1_000_000_000)
    if fraction == 0:
        return str(whole)
    return ('%d.%09d' % (whole, fraction)).rstrip('0')


def origin_text(origin):
    return {'proxy': '代理', 'workbuddy_builtin': '内置',
            'workbuddy_custom': '自定义'}[origin]


def outcome_label(outcome, error_kind):
    if outcome == 'completed':
        return '已完成'
    if outcome == 'failed':
        return {'upstream_http': '上游拒绝', 'sse_error': '流内错误',
                'connect': '连接失败', 'missing_key': '缺少密钥'}.get(
                    error_kind, '失败')
    if outcome == 'cancelled':
        return '已取消'
    if outcome == 'interrupted':
        return '被中断'
    if outcome == 'rejected':
        return '未发上游' if error_kind == 'missing_key' else '本地拒绝'
    if outcome == 'in_flight':
        return '进行中'
    if outcome == 'recorded':
        return '已记录'
    raise KeyError(outcome)


def status_text(row):
    """The status column's text (usage_table::status_text)."""
    if row['kind'] == 'imported':
        return '已记录'
    return {'in_flight': '进行中', 'completed': '完成', 'failed': '失败',
            'cancelled': '已取消', 'interrupted': '中断', 'rejected': '拒绝',
            'recorded': '已记录'}[row['outcome']]


def usage_label(status):
    return {'complete': '完整', 'partial': '部分', 'missing': '缺失',
            'invalid': '无效', 'not_applicable': '不适用'}[status]


def cost_label(status):
    return {
        'priced': '', 'default_zero': '未配置价格，按 0 计算',
        'pending': '结算中', 'legacy_unknown': '历史金额（来源不明）',
        'unpriced': '未定价', 'usage_incomplete': '用量不全',
        'unsupported_breakdown': '缓存计费数据不足',
        'not_applicable': '未发上游', 'overflow': '金额溢出',
    }[status]


def cost_hint(status):
    """The cost cell's hover/详情 hint (usage_table::cost_hint)."""
    if status == 'unsupported_breakdown':
        return ('本次价格快照按普通输入、缓存读取或缓存写入分别计费，但本次调用的用量拆分或'
                '缓存单价不足/不兼容，无法可靠估算费用。这不代表请求失败，也不代表费用为 0。'
                '修改当前价格不会重算历史明细。')
    return cost_label(status)


def cost_cell(row):
    """Return (primary_text, muted, tooltip_note) for the cost column."""
    cost = row.get('cost')
    status = cost['status'] if cost else 'unpriced'
    label = cost_label(status)
    if not label or status in ('default_zero', 'not_applicable'):
        nanos = None if not cost else (
            None if cost.get('total') is None else str(cost['total']))
        text = amount(nanos, cost.get('currency') if cost else None)
        return text, False, '估算值，不是实际账单'
    return label, True, label


def timestamp(ms):
    return datetime.fromtimestamp(ms / 1000.).strftime('%m-%d %H:%M:%S')


def display_expectations(row, started_ms):
    """Every value the request log can show for one row (zh, 20/row, today)."""
    http = '—' if row.get('http') is None else str(row['http'])
    outcome = status_text(row)
    hit = cache_rate(row.get('cr'), row.get('inp'))
    cost_text, muted, cost_note = cost_cell(row)
    if cost_text == '':
        cost_text = amount(None, None)
    credit = None
    if row.get('credit') is not None:
        credit = format_credit_nanos(str(row['credit']))
    credit_text = credit if credit is not None else '—'
    return {
        'request_id': row['request_id'],
        'request_id_shown': (row['request_id'][:12] + '…'
                             if len(row['request_id']) > 12 else row['request_id']),
        'time': timestamp(started_ms),
        'source': origin_text(row.get('origin', 'proxy')),
        'provider': row['provider_name'],
        'model': row['model'],
        'total_tokens': compact(row.get('total')),
        'input_tokens': compact(row.get('inp')),
        'output_tokens': compact(row.get('out')),
        'cache_read': compact(row.get('cr')),
        'cache_write': compact(row.get('cw')),
        'hit_rate': percent(hit),
        'cache_hit_rate_float': hit,
        'amount': cost_text,
        'amount_muted': muted,
        'cost_note': cost_note,
        'cost_status': (row['cost']['status'] if row.get('cost') else 'unpriced'),
        'currency': (row['cost'].get('currency') if row.get('cost') else None),
        'latency': duration(row.get('dur')),
        'first_content': duration(row.get('first')),
        'ttfb': duration(row.get('ttfb')),
        'speed': rate_text(row),
        'http': http,
        'outcome': outcome,
        'status_cell': '%s · %s' % (http, outcome),
        'usage_label': usage_label(row['usage']),
        'credit': credit_text,
        'stream': row.get('stream'),
    }


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
        'local_key': 't07-l05-local-key',
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


def command(config, port, payload, timeout=15):
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
    payload = {'command': command_name, 'request_tag': 't07-l05'}
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


def started_ms(row, base):
    """Row 1 sits at 00:00:05 local; the rest spread over the morning."""
    if row['id'] == 1:
        return base + 5_000
    return base + (row['minute'] * 60 + 15) * 60_000


def insert_rows(config):
    db_path = config / 'usage.sqlite3'
    now = now_ms()
    base = today_start_ms()
    with sqlite3.connect(db_path) as db:
        db.execute('PRAGMA foreign_keys=ON')
        run_id = db.execute(
            'SELECT run_id FROM stats_runs ORDER BY started_at_ms DESC LIMIT 1').fetchone()
        if run_id is None:
            run_id = ('t07-l05-run',)
            db.execute(
                'INSERT INTO stats_runs(run_id,started_at_ms,admitted_requests) VALUES(?,?,0)',
                (run_id[0], now - 7_200_000))
        run_id = run_id[0]
        recorded = {}
        for row in ROWS:
            row['request_id'] = 't07l05-%04d-%s' % (row['id'], row['tag'].split('-')[0])
            start = started_ms(row, base)
            recorded[row['id']] = start
            imported = row['kind'] == 'imported'
            value = {
                'id': row['id'], 'request_id': row['request_id'],
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
                'upstream_started': None if imported else row['upstream_started'],
                'http_status': None if imported else row['http'],
                'outcome': row['outcome'],
                'error_kind': None if imported else row.get('error_kind'),
                'usage_status': row['usage'],
                'usage_source': row['usage_source'],
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
            columns = list(value.keys())
            db.execute(
                'INSERT INTO usage_requests(%s) VALUES(%s)'
                % (','.join(columns), ','.join('?' for _ in columns)),
                [value[key] for key in columns])
            cost = row.get('cost')
            if cost:
                rates = cost.get('rates') or (None, None, None, None)
                legacy = cost['price_source'] == 'legacy_unknown'
                total = cost['total']
                parts = (0, 0, 0, 0)
                if total:
                    parts = (total - 3, 1, 1, 1)
                db.execute(
                    'INSERT INTO usage_costs(request_id,source_price_id,source_price_revision,'
                    'price_source,price_snapshot_at_ms,currency,input_mode,input_rate_nanos,'
                    'output_rate_nanos,cache_read_rate_nanos,cache_write_rate_nanos,status,'
                    'input_cost_nanos,output_cost_nanos,cache_read_cost_nanos,'
                    'cache_write_cost_nanos,total_cost_nanos,pricing_version,calculated_at_ms) '
                    'VALUES(?,NULL,NULL,?,?,?,?,?,?,?,?,?,?,?,?,?,?,2,?)',
                    (row['request_id'], cost['price_source'],
                     None if legacy else start, cost['currency'],
                     None if legacy else cost['mode'],
                     None if legacy else rates[0], None if legacy else rates[1],
                     None if legacy else rates[2], None if legacy else rates[3],
                     cost['status'],
                     None if total is None else parts[0],
                     None if total is None else parts[1],
                     None if total is None else parts[2],
                     None if total is None else parts[3],
                     total, start))
        db.commit()
        count = db.execute('SELECT COUNT(*) FROM usage_requests').fetchone()[0]
        origins = dict(db.execute(
            'SELECT record_origin, COUNT(*) FROM usage_requests GROUP BY record_origin'
        ).fetchall())
        span = db.execute(
            'SELECT MIN(started_at_ms),MAX(started_at_ms) FROM usage_requests').fetchone()
    return count, origins, span, run_id, recorded


def filter_payload(from_ms, to_ms, scope='all', capture='proxy'):
    return {'from_ms': from_ms, 'to_ms': to_ms, 'timezone': '',
            'provider_id': None, 'upstream_model': None, 'route_id': None,
            'outcome': None, 'usage_status': None, 'include_tests': False,
            'source_scope': scope, 'capture_method': capture}


# The three views the GUI reaches with its source switch, in the same order the
# acceptance run uses. "all sources" keeps the default proxy capture; switching
# to "built-in models" reconciles the capture filter to `all`.
VIEWS = [
    ('all-sources', 'all', 'proxy'),
    ('builtin', 'builtin', 'all'),
    ('custom', 'supplier', 'all'),
]


def record_backend(config, port, recorded):
    expected = {'rows': [], 'views': []}
    base = today_start_ms()
    for name, scope, capture in VIEWS:
        window = filter_payload(base, now_ms() + 60_000, scope, capture)
        page = stats(config, port, 'stats_requests', filter=window, page_size=50)
        expected['views'].append({
            'view': name, 'source_scope': scope, 'capture_method': capture,
            'filter': window, 'api_returned': len(page['items']),
            'api_total_rows': page['total_rows'],
            'api_page_size': page['page_size'],
            'request_ids': [item['request_id'] for item in page['items']],
            'api_rows': [{
                'request_id': item['request_id'], 'started_at_ms': item['started_at_ms'],
                'http_status': item['http_status'], 'outcome': item['outcome'],
                'usage_status': item['usage_status'],
                'input_tokens': item['input_tokens'],
                'output_tokens': item['output_tokens'],
                'total_tokens': item['total_tokens'],
                'cache_read_tokens': item['cache_read_tokens'],
                'duration_ms': item['duration_ms'],
                'first_content_ms': item['first_content_ms'],
                'workbuddy_credit_nanos': item['workbuddy_credit_nanos'],
                'cost': item['cost'], 'record_origin': item['record_origin'],
                'recorded': item['recorded'],
                'provider_name': item['provider_name'],
                'upstream_model': item['upstream_model'],
                'stream': item['stream'],
                'error_kind': item['error_kind'],
            } for item in page['items']],
        })
    for row in ROWS:
        start = recorded[row['id']]
        expected['rows'].append({
            'id': row['id'], 'tag': row['tag'], 'request_id': row['request_id'],
            'started_at_ms': start, 'started_local': timestamp(start),
            'view': ('all-sources' if row['kind'] == 'proxy'
                     else ('builtin' if row.get('origin') == 'workbuddy_builtin'
                           else 'custom')),
            'expected': display_expectations(row, start),
            'db': {key: row.get(key) for key in
                   ('kind', 'origin', 'provider_name', 'model', 'http', 'outcome',
                    'error_kind', 'stream', 'usage', 'inp', 'out', 'total', 'cr',
                    'cw', 'dur', 'ttfb', 'first', 'credit')},
        })
    (EV / 'expected.json').write_text(
        json.dumps(expected, ensure_ascii=False, indent=2), encoding='utf-8')
    return expected


def setup():
    if RUN.exists():
        stop_run()
        shutil.rmtree(RUN)
    RUN.mkdir(parents=True)
    (RUN / 'home').mkdir()
    (RUN / 'tmp').mkdir()
    process, port = start_core()
    config = RUN / 'config'
    count, origins, span, run_id, recorded = insert_rows(config)
    expected = record_backend(config, port, recorded)
    state = {
        'core_pid': process.pid, 'gpui_pid': None, 'port': port,
        'config': str(config), 'db': str(config / 'usage.sqlite3'),
        'rows': count, 'origins': origins, 'run_id': run_id,
        'started_at': timestamp(span[0]), 'ended_at': timestamp(span[1]),
        'views': [{'view': view['view'], 'api_returned': view['api_returned'],
                   'api_total_rows': view['api_total_rows']}
                  for view in expected['views']],
        'created_ms': now_ms(),
    }
    (RUN / 'run.json').write_text(json.dumps(state, indent=2), encoding='utf-8')
    print(json.dumps(state, ensure_ascii=False, indent=2))


def status():
    state = json.loads((RUN / 'run.json').read_text(encoding='utf-8'))
    config = RUN / 'config'
    live = {}
    base = today_start_ms()
    try:
        live['health'] = stats(config, state['port'], 'stats_health')
        for name, scope, capture in VIEWS:
            page = stats(config, state['port'], 'stats_requests',
                         filter=filter_payload(base, now_ms() + 60_000, scope, capture),
                         page_size=50)
            live[name] = {'total_rows': page['total_rows'],
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

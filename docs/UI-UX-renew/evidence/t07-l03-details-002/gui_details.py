#!/usr/bin/env python3
"""T07-L03 detail binding GUI run (macOS, isolated 60-row fixture)."""
import json, re, subprocess, sys, time
from pathlib import Path

ROOT = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign')
EV = ROOT / 'docs/UI-UX-renew/evidence/t07-l03-details-002'
CAP = Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t07-l03-details-002')
AX = CAP / 'bin/axdrive'; OCR = CAP / 'bin/ocr'
PID = int((CAP / 'gpui.pid').read_text().strip())
WID = int((CAP / 'wid').read_text().strip())
EXPECTED = json.loads((EV / 'expected.json').read_text())
ROWS = {r['request_id']: r for r in EXPECTED['rows']}
DETAIL_API = EXPECTED['detail_api']

def run(*args, timeout=30):
    return subprocess.run([str(a) for a in args], capture_output=True, text=True, timeout=timeout)

def dump(name):
    p = EV / 'logs' / ('ax-' + name + '.txt')
    run(AX, 'dump', PID, p)
    return p.read_text(encoding='utf-8')

def shot(name):
    p = EV / 'shots' / (name + '.png')
    run(AX, 'shot', WID, p)
    o = EV / 'ocr' / (name + '.json')
    with open(o, 'w') as f:
        subprocess.run([str(OCR), str(p)], stdout=f, stderr=subprocess.PIPE, text=True, timeout=60)
    return p, o

def ocr_text(o):
    try:
        return [x['text'] for x in json.loads(o.read_text()) if x['confidence'] >= 0.15]
    except Exception:
        return []

def page_text(o):
    for t in ocr_text(o):
        m = re.search(r'第\s*(\d+)\s*/\s*(\d+)', t)
        if m: return t
    return ''

def frames(text, kind='AXRow'):
    out = []
    for line in text.splitlines():
        if line.strip().startswith(kind + ' '):
            m = re.search(r'\[(-?\d+),(-?\d+) (\d+)x(\d+)\]', line)
            if m: out.append(tuple(map(int, m.groups())))
    return out

def button_y(text, title):
    for line in text.splitlines():
        if ('AXButton title=%s ' % title) in line:
            m = re.search(r'\[(-?\d+),(-?\d+) ', line)
            if m: return int(m.group(2))
    return None

def ensure(x, y):
    """click with System Events re-activation fallback once."""
    run(AX, 'click', x, y)
    time.sleep(0.8)

def activate():
    subprocess.run(['osascript', '-e',
        'tell application "System Events" to set frontmost of first process whose unix id is %d to true' % PID],
        capture_output=True)
    time.sleep(0.8)

def detail_fields(text):
    fields = {}
    for line in text.splitlines():
        m = re.search(r'AXGroup title=(.*?) desc=', line)
        if not m: continue
        title = m.group(1)
        if '：' in title:
            k, v = title.split('：', 1)
            if k not in fields: fields[k] = v
    return fields

def close_detail(name):
    run(AX, 'escape', PID); time.sleep(0.5)
    t = dump(name + '-closed')
    return '请求标识' not in t

def click_row(text, index=0):
    rows = frames(text)
    if index >= len(rows): return None
    x, y, w, h = rows[index]
    run(AX, 'click', x + w // 2, y + h // 2); time.sleep(0.9)
    return (x, y, w, h)

def compare(rid, fields):
    exp = ROWS[rid]
    api = DETAIL_API.get(rid, {}).get('request', {})
    checks = {
        'request_id': (fields.get('请求标识', ''), rid),
        'provider_route': ('Synthetic Provider · Synthetic Route' in fields.get('供应商 / 路由', ''), True),
        'requested_model': (fields.get('请求模型', ''), exp['requested_model']),
        'upstream_model': (fields.get('上游模型', ''), exp['upstream_model']),
        'status': ('HTTP 200 · 已完成' in fields.get('状态', ''), True),
        'tokens': ('↓%d ↑%d' % (exp['input_tokens'], exp['output_tokens']) in fields.get('Token', ''), True),
        'time_present': (bool(re.search(r'\d{2}-\d{2} \d{2}:\d{2}:\d{2}', fields.get('时间', ''))), True),
        'cost_present': ('估算费用' in fields, True),
        'api_request_id': (api.get('request_id'), rid),
    }
    return checks

summary = {'pid': PID, 'wid': WID, 'expected_page1': EXPECTED['walk_size_20'][0]['ids'][:2],
           'steps': [], 'page_sizes': []}

# Current detail (0060) may be open from the manual check: close it.
close_detail('pre')

cases = [
    ('01-page1-row1', 1, 0, 't07l03-0060'),
    ('02-page1-row2', 1, 1, 't07l03-0059'),
    ('03-page2-row1', 2, 0, 't07l03-0040'),
    ('04-page3-row1', 3, 0, 't07l03-0020'),
    ('05-page1-row2-again', 1, 1, 't07l03-0059'),
]
page = 1
for name, target_page, row_index, rid in cases:
    while page < target_page:
        t = dump(name + '-pre-next')
        y = button_y(t, '下一页')
        if y is None or y > 900:
            run(AX, 'scroll', 280, 300, -700); time.sleep(0.4)
            t = dump(name + '-pre-next')
            y = button_y(t, '下一页')
        run(AX, 'press', PID, '下一页'); time.sleep(1.2); page += 1
    while page > target_page:
        run(AX, 'press', PID, '上一页'); time.sleep(1.2); page -= 1
    t = dump(name + '-list')
    frame = click_row(t, row_index)
    d = dump(name + '-detail')
    fields = detail_fields(d)
    p, o = shot(name)
    step = {'name': name, 'page': page, 'row_index': row_index, 'row_frame': frame,
            'expected_request_id': rid, 'actual_fields': fields,
            'checks': compare(rid, fields), 'screenshot': str(p.relative_to(EV)),
            'ocr': str(o.relative_to(EV)), 'page_text': page_text(o)}
    step['all_match'] = all(v[0] == v[1] for v in step['checks'].values())
    summary['steps'].append(step)
    print(json.dumps({k: step[k] for k in ('name', 'page', 'expected_request_id', 'all_match', 'page_text')}, ensure_ascii=False))
    step['closed_clean'] = close_detail(name)
    # verify close
    summary['steps'][-1]['closed_clean'] = step['closed_clean']

# page-size checks (REUSE old L03 for screenshots if menu selection not reachable)
t = dump('page-size-pre')
y = button_y(t, '20 条/页')
if y is not None and y <= 900:
    run(AX, 'press', PID, '20 条/页'); time.sleep(0.8)
    menu = dump('page-size-menu')
    menu_titles = re.findall(r'AX(?:Button|MenuItem|Cell) title=([^\n]+)', menu)
    summary['page_size_menu'] = menu_titles
    for target in ('50 条/页', '100 条/页'):
        if target in menu_titles:
            run(AX, 'press', PID, target); time.sleep(1.2)
            p, o = shot('06-page-size-' + target.split()[0])
            summary['page_sizes'].append({'selected': target, 'page_text': page_text(o),
                                          'screenshot': str(p.relative_to(EV))})
            t2 = dump('page-size-after-' + target.split()[0])
            y2 = button_y(t2, target)
            if y2 is not None and y2 <= 900:
                run(AX, 'press', PID, target); time.sleep(1.0)
                menu = dump('page-size-menu-2')
                menu_titles = re.findall(r'AX(?:Button|MenuItem|Cell) title=([^\n]+)', menu)

(EV / 'gui-results.json').write_text(json.dumps(summary, ensure_ascii=False, indent=2))
print('DONE', len(summary['steps']), 'page_sizes', summary['page_sizes'])

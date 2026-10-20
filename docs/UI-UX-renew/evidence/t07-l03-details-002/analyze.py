#!/usr/bin/env python3
import json, re, time
from pathlib import Path
EV = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign/docs/UI-UX-renew/evidence/t07-l03-details-002')
exp = json.loads((EV/'expected.json').read_text())
gui = json.loads((EV/'gui-results.json').read_text())
rows = {r['request_id']: r for r in exp['rows']}
api = {k: v['request'] for k, v in exp['detail_api'].items()}

def local(ms):
    return time.strftime('%m-%d %H:%M:%S', time.localtime(ms/1000))

def tokens(s):
    m = re.search(r'↓(\d+) ↑(\d+) R(\d+) W(\d+) Σ(\d+)', s)
    return tuple(map(int, m.groups())) if m else None

def ocr_page(name):
    p = EV/'ocr'/(name + '.json')
    texts = [x['text'] for x in json.loads(p.read_text()) if x['confidence'] > 0.15]
    page = next((t for t in texts if '第' in t and '/' in t), '')
    size = next((t for t in texts if '条/页' in t), '')
    return page, size

steps = []
for s in gui['steps']:
    rid = s['expected_request_id']; r = rows[rid]; f = s['actual_fields']
    t = tokens(f.get('Token', ''))
    api_ok = all(api.get(rid, {}).get(k) == r.get(k) for k in
                 ('request_id','provider_name','route_name','requested_model','upstream_model',
                  'http_status','input_tokens','output_tokens','total_tokens'))
    checks = {
        'request_id': f.get('请求标识') == rid,
        'provider_route': f.get('供应商 / 路由') == 'Synthetic Provider · Synthetic Route',
        'requested_model': f.get('请求模型') == r['requested_model'],
        'upstream_model': f.get('上游模型') == r['upstream_model'],
        'status': f.get('状态') == 'HTTP 200 · 已完成',
        'time': f.get('时间') == local(r['started_at_ms']),
        'tokens': t is not None and t[0] == r['input_tokens'] and t[1] == r['output_tokens'] and t[4] == r['total_tokens'],
        'cost': '估算费用' in f and '¥0' in f.get('估算费用', ''),
        'db_api_consistent': api_ok,
        'closed_clean': bool(s.get('closed_clean')),
    }
    steps.append({'name': s['name'], 'page': s['page'], 'request_id': rid,
                  'expected_model': r['requested_model'], 'actual_model': f.get('请求模型'),
                  'expected_time': local(r['started_at_ms']), 'actual_time': f.get('时间'),
                  'expected_tokens': [r['input_tokens'], r['output_tokens'], r['total_tokens']],
                  'actual_token_text': f.get('Token'), 'checks': checks,
                  'all_pass': all(checks.values()), 'page_text': s['page_text'],
                  'screenshot': s['screenshot'], 'ax_detail': 'logs/ax-' + s['name'] + '-detail.txt'})

pages = []
for name, expected in (('06-page-size-50','第1/2'), ('07-page-size-100','第1/1'), ('08-page-size-20','第1/3'),
                       ('09-page3-20','第3/3'), ('10-page3-next-disabled','第3/3'), ('11-page1-prev-disabled','第1/3')):
    page, size = ocr_page(name)
    pages.append({'name': name, 'page_text': page, 'page_size_text': size,
                  'expected_contains': expected, 'pass': expected in page.replace(' ', ''),
                  'screenshot': 'shots/' + name + '.png'})

out = {'head': gui.get('head'), 'steps': steps, 'pages': pages,
       'summary': {'detail_cases': len(steps), 'all_pass': all(s['all_pass'] for s in steps),
                   'pages_all_pass': all(p['pass'] for p in pages)}}
(EV/'acceptance-summary.json').write_text(json.dumps(out, ensure_ascii=False, indent=2))
print(json.dumps(out['summary'], ensure_ascii=False))
for s in steps:
    print(s['name'], s['request_id'], s['all_pass'], {k:v for k,v in s['checks'].items() if not v})
for p in pages:
    print(p['name'], repr(p['page_text']), p['pass'])

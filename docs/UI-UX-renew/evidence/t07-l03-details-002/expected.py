#!/usr/bin/env python3
"""Export expected rows and per-page mapping from the isolated T07-L03 fixture."""
import json, sqlite3, sys, time, datetime
from pathlib import Path
ROOT = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign')
EV = ROOT / 'docs/UI-UX-renew/evidence/t07-l03-details-002'
RUN = Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t07-l03-details-002')
sys.path.insert(0, str(EV))
import fixture

state = json.loads((RUN / 'run.json').read_text())
cfg, port = RUN / 'config', state['port']
db = sqlite3.connect(cfg / 'usage.sqlite3')
db.row_factory = sqlite3.Row
rows = []
for r in db.execute('''SELECT u.request_id,u.provider_name,u.route_name,u.requested_model,
        u.upstream_model,u.started_at_ms,u.ended_at_ms,u.http_status,u.outcome,u.usage_status,
        u.input_tokens,u.output_tokens,u.total_tokens,c.status cost_status,
        c.total_cost_nanos,c.currency
        FROM usage_requests u LEFT JOIN usage_costs c ON c.request_id=u.request_id
        ORDER BY u.started_at_ms DESC'''):
    rows.append({k: r[k] for k in r.keys()})
db.close()
window = {'from_ms': fixture.today_start_ms(), 'to_ms': fixture.now_ms() + 60000,
          'timezone': '', 'include_tests': False}
pages = {}
for size in (20, 50):
    page = fixture.stats(cfg, port, 'stats_requests',
        filter=window, page_size=size)
    pages[str(size)] = [x['request_id'] for x in page['items']]
# page-1/2/3 20-row cursor walk (same as GUI)
walk = []
cursor = None
for n in range(1, 4):
    fields = {'filter': window, 'page_size': 20}
    if cursor: fields['cursor'] = cursor
    page = fixture.stats(cfg, port, 'stats_requests', **fields)
    walk.append({'page': n, 'ids': [x['request_id'] for x in page['items']],
                 'next_cursor': page.get('next_cursor')})
    cursor = page.get('next_cursor')
    if not cursor: break
detail_samples = {}
for rid in ('t07l03-0060', 't07l03-0059', 't07l03-0040',
            't07l03-0020', 't07l03-0001'):
    detail_samples[rid] = fixture.stats(cfg, port, 'stats_request_detail', request_id=rid)
out = {'state': state, 'rows': rows, 'pages_size_20': pages['20'],
       'pages_size_50': pages['50'], 'walk_size_20': walk, 'detail_api': detail_samples}
(EV / 'expected.json').write_text(json.dumps(out, ensure_ascii=False, indent=2))
print('rows', len(rows), 'walk pages', [w['page'] for w in walk],
      'first page id', pages['20'][0], 'page2 first', walk[1]['ids'][0])
print('detail sample keys', list(detail_samples)[:2], 'status',
      detail_samples['t07l03-0060'].get('http_status'),
      detail_samples['t07l03-0060'].get('input_tokens'))

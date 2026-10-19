#!/usr/bin/env python3
"""Regenerate expected.json from the live fixture database and core.

The acceptance run was captured against this database; this script only
re-reads it (through the core API exactly like the table does) and rewrites the
expectation file with the three source views, without touching any row.

usage: regen_expected.py
"""
import json
import sqlite3
import sys
from pathlib import Path

EV = Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign/'
          'docs/UI-UX-renew/evidence/t07-l05-full-values-001')
CAP = Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/'
           '01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t07-l05-full-values-001')
RUN = CAP / 'run'
sys.path.insert(0, str(EV))
import fixture  # noqa: E402

state = json.loads((RUN / 'run.json').read_text(encoding='utf-8'))
config = Path(state['config'])
port = state['port']

for row in fixture.ROWS:
    row['request_id'] = 't07l05-%04d-%s' % (row['id'], row['tag'].split('-')[0])

with sqlite3.connect(config / 'usage.sqlite3') as db:
    db.row_factory = sqlite3.Row
    recorded = {row['id']: row['started_at_ms'] for row in
                db.execute('SELECT id, started_at_ms FROM usage_requests')}

expected = fixture.record_backend(config, port, recorded)
print(json.dumps({
    'rows': len(expected['rows']),
    'views': [{'view': view['view'], 'scope': view['source_scope'],
               'capture': view['capture_method'],
               'returned': view['api_returned'],
               'total': view['api_total_rows']} for view in expected['views']],
    'first_row': expected['rows'][0]['expected'],
}, ensure_ascii=False, indent=2))

"""Insert one synthetic row into this exact disposable fixture only."""
import os
from pathlib import Path
cap=Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/d15-gui-001')
assert (cap/'owner.txt').read_text()=='Codex D15 2026-10-20\n'
os.environ.update(TMPDIR=str(cap),TMP=str(cap),TEMP=str(cap))
import sqlite3,time,json
from contextlib import closing
ev=Path(__file__).resolve().parent
with closing(sqlite3.connect(cap/'run/config/usage.sqlite3')) as db:
 db.row_factory=sqlite3.Row
 before=db.execute('SELECT COUNT(*) FROM usage_requests').fetchone()[0]
 row=dict(db.execute('SELECT * FROM usage_requests ORDER BY id DESC LIMIT 1').fetchone())
 old=row['request_id'];row['id']=db.execute('SELECT MAX(id)+1 FROM usage_requests').fetchone()[0]
 row['request_id']=f"d15-{row['id']}";now=int(time.time()*1000)
 row.update(started_at_ms=now-600,ended_at_ms=now-100,created_at_ms=now-600,updated_at_ms=now-100)
 cols=','.join(row);db.execute('INSERT INTO usage_requests('+cols+') VALUES('+','.join('?' for _ in row)+')',list(row.values()))
 cost=dict(db.execute('SELECT * FROM usage_costs WHERE request_id=?',(old,)).fetchone());cost['request_id']=row['request_id']
 if 'id' in cost:cost.pop('id')
 cols=','.join(cost);db.execute('INSERT INTO usage_costs('+cols+') VALUES('+','.join('?' for _ in cost)+')',list(cost.values()))
 db.commit();after=db.execute('SELECT COUNT(*) FROM usage_requests').fetchone()[0]
 record={'at_ms':now,'before':before,'after':after,'request_id':row['request_id']}
 with (ev/'logs/insertions.jsonl').open('a') as out:out.write(json.dumps(record)+'\n')
 print(record)

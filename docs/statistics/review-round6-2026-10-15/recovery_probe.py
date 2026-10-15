import helper as r
import json,time,sqlite3,tempfile
from pathlib import Path
t=r.t;out=[]
def emit(case,**data):
 v={'case':case,**data};out.append(v);print(json.dumps(v,ensure_ascii=False),flush=True)
# A real v2 database, built from the unchanged formal 0001 + 0002.
with tempfile.TemporaryDirectory(prefix='wbp-review6-v2-') as tmp:
 d=Path(tmp);p=t.free_port();t.write_config(d,p,'http://127.0.0.1:9/v1')
 with sqlite3.connect(d/'usage.sqlite3') as db:
  db.executescript((r.WORKTREE/'src/stats/migrations/0001.sql').read_text())
  db.executescript((r.WORKTREE/'src/stats/migrations/0002.sql').read_text())
  db.execute('PRAGMA user_version=2')
 process,log,_=t.start_core(r.EXE,d,p)
 try:
  time.sleep(.3);h=t.stats(d,p,'stats_health')
  with sqlite3.connect(d/'usage.sqlite3') as db:version=db.execute('PRAGMA user_version').fetchone()[0]
  emit('existing_v2_upgrade',state=h['state'],error=h.get('error_code'),user_version=version)
 finally:t.stop_core(process,p,d,log)
# Use a fresh core to produce a valid template, then build 1200 old terminal+pending rows.
with r.fixture() as (d,p,process,log):
 t.relay(p,'wbp-ok');time.sleep(.3);t.stop_core(process,p,d,log)
 with sqlite3.connect(d/'usage.sqlite3') as db:
  db.execute("PRAGMA foreign_keys=ON");db.row_factory=sqlite3.Row
  req=dict(db.execute('SELECT * FROM usage_requests LIMIT 1').fetchone());req.pop('id')
  cost=dict(db.execute('SELECT * FROM usage_costs LIMIT 1').fetchone())
  cost.update(status='pending',input_cost_nanos=None,output_cost_nanos=None,cache_read_cost_nanos=None,cache_write_cost_nanos=None,total_cost_nanos=None)
  db.execute('DELETE FROM usage_requests')
  def insert(table,row):
   names=list(row);db.execute('INSERT INTO '+table+' ('+','.join(names)+') VALUES ('+','.join('?' for _ in names)+')',[row[n] for n in names])
  for i in range(1200):
   req['request_id']=cost['request_id']='old-pending-%04d'%i;insert('usage_requests',req);insert('usage_costs',cost)
 process2,log2,_=t.start_core(r.EXE,d,p)
 try:
  time.sleep(1.5)
  def snap():
   h=t.stats(d,p,'stats_health')
   with sqlite3.connect(d/'usage.sqlite3') as db:
    counts=dict(db.execute('SELECT status,COUNT(*) FROM usage_costs GROUP BY status').fetchall());backlog=db.execute('SELECT COUNT(*) FROM stats_recovery_backlog').fetchone()[0]
   return {'state':h['state'],'cost_counts':counts,'backlog':backlog}
  first=snap();time.sleep(1.5)
  emit('large_startup_repair',seed_terminal_pending=1200,first=first,after_wait=snap())
 finally:t.stop_core(process2,p,d,log2)
Path(__file__).with_name('recovery-results.json').write_text(json.dumps(out,ensure_ascii=False,indent=2)+'\n')

import helper as r
import time,sqlite3,json
from pathlib import Path
results=[]
for count in [1200,20000]:
 with r.fixture() as (d,p,process,log):
  t=r.t;t.relay(p,'wbp-ok');time.sleep(.3);t.stop_core(process,p,d,log)
  with sqlite3.connect(d/'usage.sqlite3') as db:
   db.execute('PRAGMA foreign_keys=ON');db.row_factory=sqlite3.Row
   req=dict(db.execute('SELECT * FROM usage_requests LIMIT 1').fetchone());req.pop('id')
   cost=dict(db.execute('SELECT * FROM usage_costs LIMIT 1').fetchone());cost.update(status='pending',input_cost_nanos=None,output_cost_nanos=None,cache_read_cost_nanos=None,cache_write_cost_nanos=None,total_cost_nanos=None)
   db.execute('DELETE FROM usage_requests')
   def insert(table,row):
    names=list(row);db.execute('INSERT INTO '+table+' ('+','.join(names)+') VALUES ('+','.join('?' for _ in names)+')',[row[n] for n in names])
   for i in range(count):
    req['request_id']=cost['request_id']='old-pending-%05d'%i;insert('usage_requests',req);insert('usage_costs',cost)
  start=time.monotonic();process2,log2,_=t.start_core(r.EXE,d,p);samples=[]
  try:
   for target in ([.8,2,4.5,9,18] if count==1200 else [.8,2,4.5,9]):
    time.sleep(max(0,target-(time.monotonic()-start)))
    h=t.stats(d,p,'stats_health')
    with sqlite3.connect(d/'usage.sqlite3') as db:
     pending=db.execute("SELECT COUNT(*) FROM usage_costs WHERE status='pending'").fetchone()[0]
    samples.append(dict(elapsed_s=round(time.monotonic()-start,2),state=h['state'],pending=pending))
   item=dict(seed=count,samples=samples);results.append(item);print(json.dumps(item),flush=True)
  finally:t.stop_core(process2,p,d,log2)
Path(__file__).with_name('runtime-scan-results.json').write_text(json.dumps(results,indent=2)+'\n')

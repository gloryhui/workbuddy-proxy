import helper as r
import sqlite3,time,json
from pathlib import Path
t=r.t
with r.fixture() as (d,p,process,log):
 db=sqlite3.connect(d/'usage.sqlite3');db.execute('BEGIN IMMEDIATE')
 status,_,_=t.relay(p,'wbp-ok');time.sleep(3.5)
 db.rollback();db.close();time.sleep(1.4)
 def snap():
  h=t.stats(d,p,'stats_health')
  with sqlite3.connect(d/'usage.sqlite3') as x:
   return {'state':h['state'],'write_failures':h['write_failures_current_run'],'requests':x.execute('select count(*) from usage_requests').fetchone()[0],'backlog':x.execute('select count(*) from stats_recovery_backlog').fetchone()[0]}
 a=snap();time.sleep(1.6)
 out={'case':'both_start_finish_lost','http':status,'first':a,'after_wait':snap()}
 print(json.dumps(out),flush=True);Path(__file__).with_name('missing-start-results.json').write_text(json.dumps(out,indent=2)+'\n')

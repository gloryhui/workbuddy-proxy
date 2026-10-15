import helper as r
import time,sqlite3,json
from pathlib import Path
t=r.t
with r.fixture() as (d,p,process,log):
 t.relay(p,'wbp-ok');time.sleep(.3);t.stop_core(process,p,d,log)
 with sqlite3.connect(d/'usage.sqlite3') as db:
  db.execute("UPDATE usage_costs SET status='pending',input_cost_nanos=NULL,output_cost_nanos=NULL,cache_read_cost_nanos=NULL,cache_write_cost_nanos=NULL,total_cost_nanos=NULL")
  db.execute("CREATE TRIGGER refuse_settle BEFORE UPDATE ON usage_costs BEGIN SELECT RAISE(ABORT,'review settlement refusal'); END")
 process2,log2,_=t.start_core(r.EXE,d,p)
 try:
  time.sleep(1.2)
  h=t.stats(d,p,'stats_health')
  with sqlite3.connect(d/'usage.sqlite3') as db:
   before=db.execute("SELECT COUNT(*) FROM usage_costs WHERE status='pending'").fetchone()[0]
   db.execute('DROP TRIGGER refuse_settle')
  time.sleep(2)
  h2=t.stats(d,p,'stats_health')
  with sqlite3.connect(d/'usage.sqlite3') as db:
   after=db.execute("SELECT COUNT(*) FROM usage_costs WHERE status='pending'").fetchone()[0]
   backlog=db.execute('SELECT COUNT(*) FROM stats_recovery_backlog').fetchone()[0]
  out=dict(first_state=h['state'],pending_before=before,state_after_trigger_removed=h2['state'],pending_after=after,backlog=backlog)
  print(json.dumps(out),flush=True);Path(__file__).with_name('startup-failure-results.json').write_text(json.dumps(out,indent=2)+'\n')
 finally:t.stop_core(process2,p,d,log2)

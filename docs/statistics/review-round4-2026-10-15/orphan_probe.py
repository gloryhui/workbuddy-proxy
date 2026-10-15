import helper as r
import time,json,sqlite3,http.client
from pathlib import Path
t=r.t
with r.fixture() as (d,p,process,log):
 connection=http.client.HTTPConnection('127.0.0.1',p,timeout=4)
 connection.request('POST','/v1/chat/completions',json.dumps({'model':'wbp-slow','stream':True,'messages':[{'role':'user','content':'fixture'}]}),{'Authorization':'Bearer '+t.LOCAL_KEY,'Content-Type':'application/json'})
 response=connection.getresponse();response.read(1);time.sleep(.3)
 with sqlite3.connect(d/'usage.sqlite3') as db:
  db.execute("CREATE TRIGGER deny_orphan_cost BEFORE UPDATE OF status ON usage_costs BEGIN SELECT RAISE(FAIL,'fixture orphan cost failure'); END")
 response.close();connection.close();time.sleep(1.5)
 def rows():
  with sqlite3.connect(d/'usage.sqlite3') as db:return db.execute('select r.outcome,c.status,c.total_cost_nanos from usage_requests r join usage_costs c on c.request_id=r.request_id').fetchall()
 out={'case':'orphan_settlement_failure_ready','during':{'health':t.stats(d,p,'stats_health'),'rows':rows()}}
 with sqlite3.connect(d/'usage.sqlite3') as db:db.execute('DROP TRIGGER deny_orphan_cost')
 time.sleep(1.5)
 out['after_trigger_removed']={'health':t.stats(d,p,'stats_health'),'rows':rows()}
 print(json.dumps(out,ensure_ascii=False),flush=True)
 Path(__file__).with_name('orphan-results.json').write_text(json.dumps(out,ensure_ascii=False,indent=2)+'\n')

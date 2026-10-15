import sys,json,time,sqlite3,http.client
from pathlib import Path
import helper as r
t=r.t
out=[]
def emit(name,**values):
 value={'case':name,**values};out.append(value);print(json.dumps(value,ensure_ascii=False),flush=True)
with r.fixture() as (d,p,process,log):
 for model in ['model-json-error','model-json-invalid','model-partial-conflict']:
  status,_,_=t.relay(p,model,stream=False if 'json' in model else True);time.sleep(.25)
  with sqlite3.connect(d/'usage.sqlite3') as db:row=db.execute('SELECT outcome,error_kind,usage_status,input_tokens,total_tokens FROM usage_requests WHERE upstream_model=?',(model,)).fetchone()
  emit(model,http=status,record=row)
 emit('collector_after_bad_usage',state=t.stats(d,p,'stats_health')['state'])
 t.relay(p,'wbp-ok');time.sleep(.25)
 with sqlite3.connect(d/'usage.sqlite3') as db:
  missing=db.execute("SELECT c.status,c.total_cost_nanos FROM usage_costs c JOIN usage_requests r ON r.request_id=c.request_id WHERE r.upstream_model='model-ok'").fetchall()
  columns=[row[1] for row in db.execute('PRAGMA table_info(usage_costs)')]
 emit('price_decoupling_not_implemented',missing_price_cost=missing,has_rate_snapshot='input_rate_nanos' in columns,has_price_snapshot_time='price_snapshot_at_ms' in columns)
 fields=dict(provider_id='provider-a',upstream_model='model-ok',currency='USD',effective_from_ms=0,input_mode='flat',label='round2')
 saved=t.stats(d,p,'stats_price_save',**fields,input_rate_nanos='2000000000',output_rate_nanos='8000000000')
 t.relay(p,'wbp-ok');time.sleep(.25)
 try:
  t.stats(d,p,'stats_price_save',**fields,rule_id=saved['rule_id'],input_rate_nanos='1000000000',output_rate_nanos='8000000000')
  emit('price_still_locked_after_use',editable=True)
 except AssertionError as error:emit('price_still_locked_after_use',editable=False,error=str(error))
 start=t.now_ms()+80;time.sleep(.20)
 dash=t.stats(d,p,'stats_dashboard',filter={'from_ms':start,'to_ms':t.now_ms(),'timezone':'UTC'},granularity='hour')
 emit('empty_bucket',series=dash['series'])
with r.fixture() as (d,p,process,log):
 connection=http.client.HTTPConnection('127.0.0.1',p,timeout=4)
 connection.request('POST','/v1/chat/completions',json.dumps({'model':'wbp-slow','stream':True,'messages':[{'role':'user','content':'fixture'}]}),{'Authorization':'Bearer '+t.LOCAL_KEY,'Content-Type':'application/json'})
 response=connection.getresponse();response.read(1);time.sleep(.25)
 t.stop_core(process,p,d,log);connection.close()
 with sqlite3.connect(d/'usage.sqlite3') as db:row=db.execute('select outcome,error_kind from usage_requests').fetchall()
 emit('normal_stop',records=row)
with r.fixture() as (d,p,process,log):
 connection=http.client.HTTPConnection('127.0.0.1',p,timeout=4)
 connection.request('POST','/v1/chat/completions',json.dumps({'model':'wbp-slow','stream':True,'messages':[{'role':'user','content':'fixture'}]}),{'Authorization':'Bearer '+t.LOCAL_KEY,'Content-Type':'application/json'})
 response=connection.getresponse();response.read(1);time.sleep(.25)
 db=sqlite3.connect(d/'usage.sqlite3');db.execute('BEGIN IMMEDIATE')
 response.close();connection.close();time.sleep(10.5);db.rollback();db.close()
 def recovered():
  h=t.stats(d,p,'stats_health')
  return h if h['state']=='ready' and h['write_failures_current_run']>0 else None
 health=t.wait_for(recovered,'recover after finish lock',timeout=15)
 t.relay(p,'wbp-ok');time.sleep(.4)
 with sqlite3.connect(d/'usage.sqlite3') as db:
  intervals=db.execute('select state,reason from stats_collection_intervals').fetchall()
  rows=db.execute('select upstream_model,outcome from usage_requests').fetchall()
 emit('transient_finish_lock',state=health['state'],write_failures=health['write_failures_current_run'],rows=rows,intervals=intervals)
Path(__file__).with_name('api-results.json').write_text(json.dumps(out,ensure_ascii=False,indent=2)+'\n')

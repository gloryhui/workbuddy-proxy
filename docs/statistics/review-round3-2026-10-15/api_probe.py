import json,time,sqlite3,http.client
from pathlib import Path
import helper as r
t=r.t
results=[]
def emit(case,**data):
 value={'case':case,**data};results.append(value);print(json.dumps(value,ensure_ascii=False),flush=True)
def dbrows(d,sql,args=()):
 with sqlite3.connect(d/'usage.sqlite3') as db:return db.execute(sql,args).fetchall()
def save(d,p,model='model-ok',**extra):
 values=dict(provider_id='provider-a',upstream_model=model,currency='USD',input_mode='flat',input_rate_nanos='2000000000',output_rate_nanos='8000000000',label='review')
 values.update(extra);return t.stats(d,p,'stats_price_save',**values)
with r.fixture() as (d,p,process,log):
 t.relay(p,'wbp-ok');time.sleep(.3)
 f=r.filter_now();dash=t.stats(d,p,'stats_dashboard',filter=f,granularity='hour');groups=t.stats(d,p,'stats_breakdown',filter=f,group_by='model',sort='total_tokens',limit=20)
 emit('default_zero_aggregation',row=dbrows(d,'select status,currency,total_cost_nanos from usage_costs'),summary=dash['summary'],series_costs=[x.get('costs') for x in dash['series']],groups=[x['summary'] for x in groups['items']])
 a=save(d,p);save(d,p,'model-json-error');save(d,p,'model-json-invalid',provider_id='provider-b')
 first=t.stats(d,p,'stats_prices_list',provider_id='provider-b',limit=1)
 after=t.stats(d,p,'stats_prices_list',limit=1,after_rule_id=a['rule_id'])
 emit('price_list_ignores_filter_cursor',filter_provider_b=first,after_rule=after)
 try:save(d,p,'model-free-cache',input_mode='read_discount',cache_read_rate_nanos='0');emit('free_cache_price',accepted=True)
 except AssertionError as e:emit('free_cache_price',accepted=False,error=str(e))
 time.sleep(.15);t.relay(p,'wbp-ok');time.sleep(.3)
 emit('snapshot_timestamp',rows=dbrows(d,'select r.started_at_ms,c.price_snapshot_at_ms,p.updated_at_ms from usage_requests r join usage_costs c on c.request_id=r.request_id join stats_model_prices p on p.price_id=c.source_price_id'))
 saved_again=save(d,p,input_rate_nanos='9000000000')
 emit('duplicate_create_bypasses_revision',first_revision=a['revision'],second_revision=saved_again['revision'])
with r.fixture() as (d,p,process,log):
 t.relay(p,'wbp-ok');time.sleep(.25);cutoff=t.now_ms();time.sleep(.04)
 t.relay(p,'model-json-error',stream=False);time.sleep(.3)
 before=dbrows(d,'select upstream_model,started_at_ms from usage_requests order by id')
 preview=t.stats(d,p,'stats_clear_preview',before_ms=cutoff)
 task=t.stats(d,p,'stats_clear_start',token=preview['token']);time.sleep(.3)
 emit('clear_deletes_outside_preview',cutoff=cutoff,before=before,preview_count=preview['row_count'],after=dbrows(d,'select upstream_model,started_at_ms from usage_requests order by id'),maintenance=t.stats(d,p,'stats_maintenance_status',task_id=task['task_id']))
with r.fixture(missing_key=True) as (d,p,process,log):
 save(d,p);status,_,_=t.relay(p,'wbp-ok');time.sleep(.45)
 emit('manual_price_no_upstream',http=status,rows=dbrows(d,'select r.outcome,r.upstream_started,c.status,c.total_cost_nanos from usage_requests r join usage_costs c on c.request_id=r.request_id'),health=t.stats(d,p,'stats_health'))
with r.fixture() as (d,p,process,log):
 connection=http.client.HTTPConnection('127.0.0.1',p,timeout=4)
 connection.request('POST','/v1/chat/completions',json.dumps({'model':'wbp-slow','stream':True,'messages':[{'role':'user','content':'fixture'}]}),{'Authorization':'Bearer '+t.LOCAL_KEY,'Content-Type':'application/json'})
 response=connection.getresponse();response.read(1);time.sleep(.3)
 t.stop_core(process,p,d,log);connection.close()
 emit('shutdown_cost_unsettled',rows=dbrows(d,'select r.outcome,c.status,c.total_cost_nanos from usage_requests r join usage_costs c on c.request_id=r.request_id'))
 process2,log2,_=t.start_core(r.EXE,d,p)
 try:
  time.sleep(.2);emit('restart_cost_unsettled',rows=dbrows(d,'select r.outcome,c.status,c.total_cost_nanos from usage_requests r join usage_costs c on c.request_id=r.request_id'))
 finally:t.stop_core(process2,p,d,log2)
# Abrupt termination bypasses normal Guard finalization.
import tempfile,threading
with tempfile.TemporaryDirectory(prefix='wbp-review3-crash-') as tmp:
 d=Path(tmp);server=t.ThreadingHTTPServer(('127.0.0.1',0),r.Mock)
 threading.Thread(target=server.serve_forever,daemon=True).start();p=t.free_port()
 t.write_config(d,p,'http://127.0.0.1:%s/v1'%server.server_port)
 process,log,_=t.start_core(r.EXE,d,p)
 connection=http.client.HTTPConnection('127.0.0.1',p,timeout=4)
 try:
  t.wait_for(lambda:t.stats(d,p,'stats_health')['state']=='ready','ready')
  connection.request('POST','/v1/chat/completions',json.dumps({'model':'wbp-slow','stream':True,'messages':[{'role':'user','content':'fixture'}]}),{'Authorization':'Bearer '+t.LOCAL_KEY,'Content-Type':'application/json'})
  response=connection.getresponse();response.read(1);time.sleep(.3)
  process.kill();process.wait();connection.close();log.close()
  process,log,_=t.start_core(r.EXE,d,p);time.sleep(.3)
  emit('crash_recovery_pending_cost',rows=dbrows(d,'select r.outcome,c.price_source,c.status,c.total_cost_nanos from usage_requests r join usage_costs c on c.request_id=r.request_id'))
 finally:
  if process.poll() is None:t.stop_core(process,p,d,log)
  server.shutdown();server.server_close()
with r.fixture() as (d,p,process,log):
 connection=http.client.HTTPConnection('127.0.0.1',p,timeout=4)
 connection.request('POST','/v1/chat/completions',json.dumps({'model':'wbp-slow','stream':True,'messages':[{'role':'user','content':'fixture'}]}),{'Authorization':'Bearer '+t.LOCAL_KEY,'Content-Type':'application/json'})
 response=connection.getresponse();response.read(1);time.sleep(.3)
 with sqlite3.connect(d/'usage.sqlite3') as db:
  db.execute("CREATE TRIGGER deny_recovery_interval BEFORE INSERT ON stats_collection_intervals WHEN NEW.state='collecting' BEGIN SELECT RAISE(FAIL,'fixture recovery metadata failure'); END")
 db=sqlite3.connect(d/'usage.sqlite3');db.execute('BEGIN IMMEDIATE')
 response.close();connection.close();time.sleep(1.3);db.rollback();db.close();time.sleep(1.2)
 emit('recovery_metadata_failure',health=t.stats(d,p,'stats_health'),intervals=dbrows(d,'select state,reason,ended_at_ms from stats_collection_intervals'),diagnostic_present='could not record the degraded window' in (d/'process.log').read_text(errors='replace'))

Path(__file__).with_name('api-results.json').write_text(json.dumps(results,ensure_ascii=False,indent=2)+'\n')

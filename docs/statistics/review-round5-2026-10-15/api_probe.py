import json,time,sqlite3,tempfile,threading,http.client
from pathlib import Path
import helper as r
t=r.t;out=[]
def emit(case,**data):
 v={'case':case,**data};out.append(v);print(json.dumps(v,ensure_ascii=False),flush=True)
def rows(d,sql,args=()):
 with sqlite3.connect(d/'usage.sqlite3') as db:return db.execute(sql,args).fetchall()
def save(d,p,model='model-ok',**more):
 v=dict(provider_id='provider-a',upstream_model=model,currency='USD',input_mode='flat',input_rate_nanos='2000000000',output_rate_nanos='8000000000',label='round4')
 v.update(more);return t.stats(d,p,'stats_price_save',**v)
with r.fixture() as (d,p,process,log):
 t.relay(p,'wbp-ok');time.sleep(.25);dash=t.stats(d,p,'stats_dashboard',filter=r.filter_now(),granularity='hour')
 emit('default_zero_summary_now',costs=dash['summary']['costs'],default_zero=dash['summary'].get('default_zero_request_count'),settled=dash['summary'].get('settled_cost_request_count'))
 a=save(d,p);b=save(d,p,'model-json-error');c=save(d,p,'model-json-invalid',provider_id='provider-b')
 emit('price_list_now',provider_b=t.stats(d,p,'stats_prices_list',provider_id='provider-b',limit=1),first=t.stats(d,p,'stats_prices_list',limit=1))
 try:
  duplicate=save(d,p,input_rate_nanos='99000000000')
  emit('duplicate_new_no_revision',accepted=True,returned=duplicate,stored=rows(d,"select input_rate_nanos,revision from stats_model_prices where upstream_model='model-ok'"))
 except AssertionError as e:emit('duplicate_new_no_revision',accepted=False,error=str(e))
 try:
  mismatch=save(d,p,'model-other',price_id=a['rule_id'],expected_revision=a['revision'])
  emit('canonical_price_id_edit',accepted=True,returned=mismatch,original=a['rule_id'],price_rows=rows(d,'select price_id,upstream_model,revision from stats_model_prices order by upstream_model'))
 except AssertionError as e:emit('canonical_price_id_edit',accepted=False,error=str(e))
 try:emit('explicit_zero_cache_now',saved=save(d,p,'model-free-cache',input_mode='read_discount',cache_read_rate_nanos='0'))
 except AssertionError as e:emit('explicit_zero_cache_now',error=str(e))
 try:emit('blank_input_rate',saved=save(d,p,'model-blank',input_rate_nanos=''))
 except AssertionError as e:emit('blank_input_rate',error=str(e))
with r.fixture(missing_key=True) as (d,p,process,log):
 save(d,p);status,_,_=t.relay(p,'wbp-ok');time.sleep(.35)
 emit('manual_no_upstream_now',http=status,rows=rows(d,'select r.outcome,r.upstream_started,c.status,c.total_cost_nanos from usage_requests r join usage_costs c on c.request_id=r.request_id'))
with r.fixture() as (d,p,process,log):
 t.relay(p,'wbp-ok');time.sleep(.2);cutoff=t.now_ms();time.sleep(.03);t.relay(p,'model-json-error',stream=False);time.sleep(.2)
 pre=t.stats(d,p,'stats_clear_preview',before_ms=cutoff);task=t.stats(d,p,'stats_clear_start',token=pre['token']);time.sleep(.3)
 emit('clear_cutoff_now',preview=pre['row_count'],left=rows(d,'select upstream_model from usage_requests'),task=t.stats(d,p,'stats_maintenance_status',task_id=task['task_id']))
# Crash a request whose upstream has already emitted actual bytes, under a manual price.
with tempfile.TemporaryDirectory(prefix='wbp-review4-crash-') as tmp:
 d=Path(tmp);server=t.ThreadingHTTPServer(('127.0.0.1',0),r.Mock)
 threading.Thread(target=server.serve_forever,daemon=True).start();p=t.free_port();t.write_config(d,p,'http://127.0.0.1:%s/v1'%server.server_port)
 process,log,_=t.start_core(r.EXE,d,p);connection=http.client.HTTPConnection('127.0.0.1',p,timeout=4)
 try:
  t.wait_for(lambda:t.stats(d,p,'stats_health')['state']=='ready','ready');save(d,p,'model-slow')
  connection.request('POST','/v1/chat/completions',json.dumps({'model':'wbp-slow','stream':True,'messages':[{'role':'user','content':'fixture'}]}),{'Authorization':'Bearer '+t.LOCAL_KEY,'Content-Type':'application/json'})
  response=connection.getresponse();response.read(1);time.sleep(.3)
  emit('manual_before_crash',rows=rows(d,'select r.outcome,r.upstream_started,c.status,c.price_source from usage_requests r join usage_costs c on c.request_id=r.request_id'))
  process.kill();process.wait();connection.close();log.close();process,log,_=t.start_core(r.EXE,d,p);time.sleep(.3)
  emit('manual_after_crash',rows=rows(d,'select r.outcome,r.upstream_started,c.price_source,c.status,c.total_cost_nanos from usage_requests r join usage_costs c on c.request_id=r.request_id'))
  # Emulate a terminal+pending row left by the previous v2 build, without changing prices/tokens.
  t.stop_core(process,p,d,log)
  with sqlite3.connect(d/'usage.sqlite3') as db:
   db.execute("UPDATE usage_costs SET status='pending',input_cost_nanos=NULL,output_cost_nanos=NULL,cache_read_cost_nanos=NULL,cache_write_cost_nanos=NULL,total_cost_nanos=NULL")
  process,log,_=t.start_core(r.EXE,d,p);time.sleep(.3)
  emit('old_v2_terminal_pending_after_restart',rows=rows(d,'select r.outcome,c.status,c.total_cost_nanos from usage_requests r join usage_costs c on c.request_id=r.request_id'))
 finally:
  if process.poll() is None:t.stop_core(process,p,d,log)
  server.shutdown();server.server_close()
Path(__file__).with_name('api-results.json').write_text(json.dumps(out,ensure_ascii=False,indent=2)+'\n')

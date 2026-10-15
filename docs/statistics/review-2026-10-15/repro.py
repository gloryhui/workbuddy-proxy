#!/usr/bin/env python3
"""Audit fixtures only. Outputs observed facts; these are not passing regression assertions."""
from pathlib import Path
import sys,json,time,tempfile,threading,http.client,sqlite3,contextlib
import argparse
parser=argparse.ArgumentParser(description='Read-only statistics audit against isolated temporary fixtures')
parser.add_argument('--worktree',type=Path,required=True)
parser.add_argument('--exe',type=Path,required=True)
parser.add_argument('--output',type=Path,required=True)
ARGS=parser.parse_args()
sys.path.insert(0,str(ARGS.worktree.resolve()/'scripts'))
import statistics_test as t
EXE=str(ARGS.exe.resolve())
RESULTS=[]
class Mock(t.BaseHTTPRequestHandler):
    protocol_version='HTTP/1.1'
    def log_message(self,*a):pass
    def do_POST(self):
        request=json.loads(self.rfile.read(int(self.headers.get('Content-Length',0))))
        model=request.get('model')
        if model=='model-slow':
            self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Transfer-Encoding','chunked');self.end_headers()
            data=b'data: {"choices":[{"delta":{"content":"hello"}}]}\n\n'
            self.wfile.write(b'%x\r\n'%len(data)+data+b'\r\n');self.wfile.flush();time.sleep(12);self.close_connection=True;return
        if model=='model-json-error':
            content_type='application/json';body=b'{"error":{"message":"fixture upstream error"}}'
        elif model=='model-json-invalid':
            content_type='application/json';body=b'{"broken":'
        elif model=='model-partial-conflict':
            content_type='text/event-stream'
            body=b'data: {"usage":{"prompt_tokens":100}}\n\ndata: {"usage":{"prompt_tokens":200,"completion_tokens":10,"total_tokens":210}}\n\ndata: [DONE]\n\n'
        else:
            content_type='text/event-stream'
            body=b'data: {"choices":[{"delta":{"content":"hello"}}]}\n\ndata: {"usage":{"prompt_tokens":1000,"completion_tokens":100,"total_tokens":1100}}\n\ndata: [DONE]\n\n'
        self.send_response(200);self.send_header('Content-Type',content_type);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body);self.wfile.flush()
@contextlib.contextmanager
def fixture(missing_key=False):
    with tempfile.TemporaryDirectory(prefix='wbp-review-') as tmp:
        directory=Path(tmp);server=t.ThreadingHTTPServer(('127.0.0.1',0),Mock)
        threading.Thread(target=server.serve_forever,daemon=True).start()
        port=t.free_port();t.write_config(directory,port,'http://127.0.0.1:%s/v1'%server.server_port)
        cfg=json.loads((directory/'config.json').read_text())
        for index,name in enumerate(['model-json-error','model-json-invalid','model-partial-conflict']):
            cfg['providers'][0]['models'].append({'id':'extra-%s'%index,'alias':name,'model':name,'name':name,'reasoning_effort':'default'})

        if missing_key:cfg['providers'][0]['api_key']=''
        (directory/'config.json').write_text(json.dumps(cfg))
        process,log,_=t.start_core(EXE,directory,port)
        t.wait_for(lambda:t.stats(directory,port,'stats_health')['state']=='ready','statistics ready')
        try:yield directory,port,process,log
        finally:
            t.stop_core(process,port,directory,log);server.shutdown();server.server_close()
def emit(name,**data):
    r={'case':name,**data};RESULTS.append(r);print(json.dumps(r,ensure_ascii=False),flush=True)
def rows(d,p):
    return t.stats(d,p,'stats_requests',filter={'from_ms':0,'to_ms':t.now_ms()+60000,'timezone':'UTC'})
def filter_now():return {'from_ms':t.now_ms()-60000,'to_ms':t.now_ms()+60000,'timezone':'UTC'}
def main():
    # Exact responses are delivered normally but protocol-error classification differs.
    with fixture() as (d,p,process,log):
        for model in ['model-json-error','model-json-invalid']:
            status,_,body=t.relay(p,model,stream=False);time.sleep(.25)
            with sqlite3.connect(d/'usage.sqlite3') as db:
                record=db.execute('select outcome,error_kind,usage_status,usage_reason from usage_requests where upstream_model=?',(model,)).fetchone()
            emit(model,http=status,body=body.decode(),record=record)
        # API contract rate_nanos is integer nanos, not currency units.
        fields=dict(provider_id='provider-a',upstream_model='model-ok',currency='USD',effective_from_ms=0,input_mode='flat',label='review')
        try:
            r=t.stats(d,p,'stats_price_save',**fields,input_rate_nanos='2000000000',output_rate_nanos='8000000000')
            emit('documented_nano_rates',accepted=True,response=r)
        except AssertionError as e:emit('documented_nano_rates',accepted=False,error=str(e))
        r=t.stats(d,p,'stats_price_save',**fields,input_rate_nanos='1',output_rate_nanos='1')
        with sqlite3.connect(d/'usage.sqlite3') as db:stored=db.execute('select input_rate_nanos,output_rate_nanos from stats_price_rules').fetchone()
        emit('one_nano_rate',sent='1',stored=stored)
        for _ in range(21):t.relay(p,'wbp-ok')
        time.sleep(.4);f=filter_now();first=t.stats(d,p,'stats_requests',filter=f,page_size=20)
        cursor=dict(first['next_cursor']);cursor.update(run_id='wrong-run',filter_key='wrong-filter',maintenance_revision=999999)
        try:
            second=t.stats(d,p,'stats_requests',filter=f,page_size=20,cursor=cursor)
            emit('invalid_cursor',accepted=True,items=len(second['items']))
        except AssertionError as e:emit('invalid_cursor',accepted=False,error=str(e))
        # A fully covered empty bucket should be a confirmed zero, not NULL.
        start=t.now_ms()+80;time.sleep(.20)
        dash=t.stats(d,p,'stats_dashboard',filter={'from_ms':start,'to_ms':t.now_ms(),'timezone':'UTC'},granularity='hour')
        emit('covered_empty_bucket',series=dash['series'])
    with fixture() as (d,p,process,log):
        t.relay(p,'model-partial-conflict');time.sleep(.5)
        health=t.stats(d,p,'stats_health')
        with sqlite3.connect(d/'usage.sqlite3') as db:record=db.execute('select outcome,usage_status,total_tokens from usage_requests').fetchall()

        with sqlite3.connect(d/'usage.sqlite3') as db:before=db.execute('select count(*) from usage_requests').fetchone()[0]
        t.relay(p,'wbp-ok');time.sleep(.5)

        with sqlite3.connect(d/'usage.sqlite3') as db:after=db.execute('select count(*) from usage_requests').fetchone()[0]
        emit('conflicting_partial_usage',state=health['state'],error=health['error_code'],rows=record,subsequent_request_recorded=after>before)
    with fixture() as (d,p,process,log):
        # A transient external writer lock outlives all 3 two-second attempts.
        db=sqlite3.connect(d/'usage.sqlite3');db.execute('BEGIN IMMEDIATE')
        t.relay(p,'wbp-ok');time.sleep(6.5)
        db.rollback();db.close();time.sleep(.25)
        h1=t.stats(d,p,'stats_health');t.relay(p,'wbp-ok');time.sleep(.4);h2=t.stats(d,p,'stats_health')
        with sqlite3.connect(d/'usage.sqlite3') as db:intervals=db.execute('select state,reason from stats_collection_intervals').fetchall()
        emit('transient_database_lock',state_after_unlock=h1['state'],state_after_new_request=h2['state'],write_failures=h2['write_failures_current_run'],loss_scope=h2['loss_scope'],intervals=intervals)
    with fixture() as (d,p,process,log):
        connection=http.client.HTTPConnection('127.0.0.1',p,timeout=4)
        connection.request('POST','/v1/chat/completions',json.dumps({'model':'wbp-slow','stream':True,'messages':[{'role':'user','content':'fixture'}]}),{'Authorization':'Bearer '+t.LOCAL_KEY,'Content-Type':'application/json'})
        response=connection.getresponse();response.read(1);time.sleep(.25)
        t.stop_core(process,p,d,log);connection.close()
        with sqlite3.connect(d/'usage.sqlite3') as db:record=db.execute('select outcome,error_kind from usage_requests').fetchall()
        emit('normal_shutdown_active_stream',record=record)


    r=sys.modules[__name__]
    with r.fixture() as (d,p,process,log):
        connection=http.client.HTTPConnection('127.0.0.1',p,timeout=4)
        connection.request('POST','/v1/chat/completions',json.dumps({'model':'wbp-slow','stream':True,'messages':[{'role':'user','content':'fixture'}]}),{'Authorization':'Bearer '+t.LOCAL_KEY,'Content-Type':'application/json'})
        response=connection.getresponse();response.read(1);time.sleep(.25)
        db=sqlite3.connect(d/'usage.sqlite3');db.execute('BEGIN IMMEDIATE')
        response.close();connection.close();time.sleep(6.5);db.rollback();db.close();time.sleep(.2)
        h1=t.stats(d,p,'stats_health');t.relay(p,'wbp-ok');time.sleep(.4);h2=t.stats(d,p,'stats_health')
        with sqlite3.connect(d/'usage.sqlite3') as db:
            intervals=db.execute('select state,reason from stats_collection_intervals').fetchall()
            rows=db.execute('select outcome,error_kind from usage_requests').fetchall()
        r.emit('failed_finish_lock_released',state_after_unlock=h1['state'],state_after_new_request=h2['state'],rows=rows,intervals=intervals,write_failures=h2['write_failures_current_run'])
    with r.fixture() as (d,p,process,log):
        t.stats(d,p,'stats_price_save',provider_id='provider-a',upstream_model='model-ok',currency='USD',effective_from_ms=0,input_mode='read_discount',label='review',input_rate_nanos='2',output_rate_nanos='8',cache_read_rate_nanos='0.2')
        t.relay(p,'wbp-ok');time.sleep(.3)
        dash=t.stats(d,p,'stats_dashboard',filter=r.filter_now())
        with sqlite3.connect(d/'usage.sqlite3') as db:costs=db.execute('select status from usage_costs').fetchall()
        r.emit('unsupported_breakdown_counts',actual_costs=costs,summary={key:dash['summary'][key] for key in ['unpriced_request_count','cost_incomplete_request_count']})
    with r.fixture(missing_key=True) as (d,p,process,log):
        t.relay(p,'wbp-ok');time.sleep(.3)
        with sqlite3.connect(d/'usage.sqlite3') as db:before=db.execute('select status from usage_costs').fetchall()
        task=t.stats(d,p,'stats_reprice_start',filter=r.filter_now(),expected_generation='')
        def done():
            value=t.stats(d,p,'stats_maintenance_status',task_id=task['task_id'])
            return value if value['status']!='running' else None
        t.wait_for(done,'reprice finished')
        with sqlite3.connect(d/'usage.sqlite3') as db:after=db.execute('select status from usage_costs').fetchall()
        r.emit('reprice_not_sent_request',before=before,after=after)
    ARGS.output.parent.mkdir(parents=True,exist_ok=True)
    ARGS.output.write_text(json.dumps(RESULTS,ensure_ascii=False,indent=2)+"\n")

if __name__=='__main__':main()

from pathlib import Path
import os,sys,json,time,tempfile,threading,http.client,sqlite3,contextlib
WORKTREE=Path(os.environ.get('WBP_REVIEW_WORKTREE','/Volumes/1TB/Downloads/git/workbuddy-proxy-usage-statistics'))
sys.path.insert(0,str(WORKTREE/'scripts'))
import statistics_test as t
EXE=os.environ.get('WBP_REVIEW_EXE','/Volumes/1TB/Downloads/git/workbuddy-proxy/target/release/workbuddy-proxy')
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

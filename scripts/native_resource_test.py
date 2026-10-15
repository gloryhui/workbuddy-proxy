#!/usr/bin/env python3
"""Native HTTP resource guards with real slow consumers and process RSS."""
import argparse
import json
from pathlib import Path
import tempfile
import threading
import time
import urllib.request
import urllib.error
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
import statistics_test as t
from benchmark import rss_kib

class Vendor(BaseHTTPRequestHandler):
    protocol_version='HTTP/1.1'
    def log_message(self,*_):pass
    def do_GET(self):
        self.send_response(404);self.send_header('Content-Length','0');self.end_headers()
    def do_POST(self):
        body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        case=body['messages'][-1]['content'][0]['text']
        assert self.path=='/gateway/v1/messages' and self.headers['x-api-key']=='fixture-native'
        with self.server.lock:self.server.started+=1
        self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Connection','close');self.end_headers()
        def emit(v):
            self.wfile.write(('event: '+v['type']+'\ndata: '+json.dumps(v)+'\n\n').encode());self.wfile.flush()
        try:
            emit({'type':'message_start','message':{'id':'m','type':'message','role':'assistant','model':'native-model','content':[],'stop_reason':None,'usage':{'input_tokens':10,'cache_read_input_tokens':0,'cache_creation_input_tokens':0,'output_tokens':0}}})
            emit({'type':'content_block_start','index':0,'content_block':{'type':'text','text':''}})
            emit({'type':'content_block_delta','index':0,'delta':{'type':'text_delta','text':'started'}})
            if case=='frame':
                emit({'type':'content_block_delta','index':0,'delta':{'type':'text_delta','text':'x'*1048577}})
            else:
                # 16-MiB aggregate budget is lower than this intentionally endless output.
                for _ in range(400):
                    emit({'type':'content_block_delta','index':0,'delta':{'type':'text_delta','text':'x'*65536}})
                    if case=='slow':time.sleep(.001)
            emit({'type':'content_block_stop','index':0})
            emit({'type':'message_delta','delta':{'stop_reason':'end_turn'},'usage':{'output_tokens':20}})
            emit({'type':'message_stop'})
        except (BrokenPipeError,ConnectionResetError):
            with self.server.lock:self.server.cancelled+=1
        self.close_connection=True

def main():
    p=argparse.ArgumentParser();p.add_argument('--exe',default='target/release/workbuddy-proxy');a=p.parse_args()
    server=ThreadingHTTPServer(('127.0.0.1',0),Vendor);server.lock=threading.Lock();server.started=0;server.cancelled=0
    threading.Thread(target=server.serve_forever,daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix='wbp-native-resource-') as tmp:
            folder=Path(tmp);port=t.free_port()
            folder.joinpath('config.json').write_text(json.dumps({'local_key':t.LOCAL_KEY,'port':port,'providers':[{'id':'native-provider','name':'Native','upstream':'http://127.0.0.1:%s/gateway/v1'%server.server_port,'protocol':'anthropic_messages','api_key':'fixture-native','models':[{'id':'native-route','alias':'native-resource','name':'Native','model':'native-model','reasoning_effort':'default'}]}]}))
            process,log,_=t.start_core(a.exe,folder,port)
            def request(case,padding=0):
                data=json.dumps({'model':'native-resource','stream':True,'messages':[{'role':'user','content':case+' '*padding}]}).encode()
                r=urllib.request.Request('http://127.0.0.1:%s/v1/chat/completions'%port,data=data,headers={'Authorization':'Bearer '+t.LOCAL_KEY,'Content-Type':'application/json'})
                return urllib.request.urlopen(r,timeout=15)
            try:
                baseline=rss_kib(process.pid);peak=baseline
                for case,code in [('frame',b'sse_frame_too_large'),('aggregate',b'upstream_output_too_large')]:
                    with request(case) as response:raw=response.read()
                    assert b'"error"' in raw and b'[DONE]' not in raw and b'"finish_reason":"stop"' not in raw,raw[-300:]
                    assert code in raw,raw[-300:]
                    print('PASS native %s limit: explicit safe stream error without success terminal'%case,flush=True)
                cancelled_before=server.cancelled
                clients=[]
                try:
                    for _ in range(4):clients.append(request('slow'))
                    t.wait_for(lambda:server.started>=6,'four native upstreams started')
                    try:request('fifth');raise AssertionError('Fifth native request was not rejected')
                    except urllib.error.HTTPError as e:assert e.code==429
                    before=server.started
                    # Do not consume these bodies: network backpressure must stop
                    # the producer without an unbounded resident event backlog.
                    deadline=time.monotonic()+1
                    while time.monotonic()<deadline:
                        peak=max(peak,rss_kib(process.pid));time.sleep(.05)
                    assert server.started==before
                finally:
                    for c in clients:c.close()
                t.wait_for(lambda:server.cancelled>=cancelled_before+4,'native sources dropped after slow clients close')
                # A new request proves the permits were returned.
                with request('frame') as response:assert b'sse_frame_too_large' in response.read()
                before=server.started
                try:request('oversized',8*1024*1024);raise AssertionError('Oversized native request accepted')
                except urllib.error.HTTPError as e:assert e.code==413
                assert server.started==before
                growth=(peak-baseline)/1024
                assert growth<160,(baseline,peak)
                print('PASS native 4 in flight, fifth 429, slow-consumer backpressure, cancellation/reuse, 8-MiB input guard; baseline %.1f MiB / sampled peak %.1f MiB / growth %.1f MiB'%(baseline/1024,peak/1024,growth),flush=True)
            finally:t.stop_core(process,port,folder,log)
    finally:server.shutdown();server.server_close()
if __name__=='__main__':main()

#!/usr/bin/env python3
"""Real core: draft URL/key discovery, editable model rows, short routes and bounded logs."""
import argparse
from http.server import BaseHTTPRequestHandler
import http.client
import json
from pathlib import Path
import tempfile
import threading
import time
import urllib.error
import urllib.request
from smoke import ThreadingHTTPServer, start, stop
class Models(BaseHTTPRequestHandler):
    def log_message(self,*args):pass
    def do_GET(self):
        self.server.seen.append((self.path,self.headers.get('Authorization')))
        mode=self.path.split('/')[1]
        status=401 if mode=='denied' else 200
        body=({'data':[{'id':'z-model'},{'id':'a-model'},{'id':'a-model'}]} if mode=='v1' else
              {'data':[{'id':f'model-{i}'} for i in range(257)]} if mode=='many' else
              {'data':[{'id':f'model-{i}'} for i in range(4097)]} if mode=='oversized' else
              {'data':[{'name':'missing-id'}]} if mode=='bad' else {'error':'fixture-secret-must-not-leak'})
        data=json.dumps(body).encode();self.send_response(status);self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
    def do_POST(self):
        body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));assert body['model'] in ['a-model','z-model']
        data=b'data: {"choices":[{"delta":{"content":"OK"}}]}\n\ndata: [DONE]\n\n'
        self.send_response(200);self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
def main():
    p=argparse.ArgumentParser();p.add_argument('--exe',required=True);args=p.parse_args()
    server=ThreadingHTTPServer(('127.0.0.1',0),Models);server.seen=[];threading.Thread(target=server.serve_forever,daemon=True).start();base=f'http://127.0.0.1:{server.server_port}'
    with tempfile.TemporaryDirectory() as directory:
        process,port,log=start(args.exe,directory,base+'/v1')
        try:
            cfg=Path(directory,'config.json');before=cfg.read_bytes();profile=json.loads(before);pid=profile['providers'][0]['id'];key=json.loads(Path(directory,'instance.json').read_text())['management_key']
            def action(payload):
                payload['provider_id']=pid
                request=urllib.request.Request(f'http://127.0.0.1:{port}/_control/action',data=json.dumps(payload).encode(),headers={'Authorization':'Bearer '+key,'Content-Type':'application/json'})
                try:
                    with urllib.request.urlopen(request,timeout=5) as r:return r.status,json.load(r)['message']
                except urllib.error.HTTPError as e:return e.code,json.load(e)['message']
            Path(directory,'update-cache.json').write_text(json.dumps({'checked_at':int(time.time()),'tag':'v99.0.0'}))
            code,text=action({'command':'check_update','force':False});update=json.loads(text)
            assert code==200 and update['available'] and update['cached'] and update['url']=='https://github.com/gloryhui/workbuddy-proxy/releases/tag/v99.0.0'
            code,text=action({'command':'fetch','upstream':base+'/v1','key':'draft-secret'})
            assert code==200 and json.loads(text)==['a-model','z-model']
            assert server.seen[-1]==('/v1/models','Bearer draft-secret')
            assert cfg.read_bytes()==before,'Fetching must not save draft URL/key/models'
            assert action({'command':'fetch','upstream':base+'/v1','key':''})[0]==200
            assert server.seen[-1][1]=='Bearer fixture-upstream-key'
            code,text=action({'command':'fetch','upstream':base+'/many','key':'draft-secret'})
            assert code==200 and len(json.loads(text))==257
            assert cfg.read_bytes()==before,'Large discovery must not change configuration'
            for path in ['denied','oversized','bad']:
                code,text=action({'command':'fetch','upstream':base+'/'+path,'key':'draft-secret'})
                assert code==400 and 'fixture-secret-must-not-leak' not in text and 'draft-secret' not in text
            assert action({'command':'save','models':[{'name':'','model':''}]})[0]==400
            assert cfg.read_bytes()==before
            rows=[{'name':'Flash','model':'a-model'},{'name':'','model':'z-model'}]
            assert action({'command':'save','models':rows,'reasoning_effort':'high'})[0]==200
            saved=json.loads(cfg.read_text());models=saved['providers'][0]['models'];assert models[1]['name']=='z-model'
            original=models[0]['id'];rows=[{'id':original,'name':'Renamed','model':'a-model'},rows[1]]
            assert action({'command':'save','models':rows,'reasoning_effort':'keep'})[0]==200
            assert json.loads(cfg.read_text())['providers'][0]['models'][0]['id']==original
            conn=http.client.HTTPConnection('127.0.0.1',port,timeout=5)
            conn.request('POST',f'/r/{original}/v1/chat/completions',json.dumps({'model':'wbp-'+original,'messages':[]}),{'Authorization':'Bearer fixture-local-key'})
            r=conn.getresponse();assert r.status==200;r.read();conn.close()
            assert action({'command':'language','language':'en'})[0]==200
            assert json.loads(cfg.read_text())['language']=='en'
            for _ in range(105):action({'command':'language','language':'zh'})
            code,text=action({'command':'snapshot'});snapshot=json.loads(text)
            assert code==200 and len(snapshot['logs'])==100 and snapshot['requests']==1
            assert 'draft-secret' not in text and 'fixture-upstream-key' not in text and key not in text
            print('PASS: draft discovery, saved-key fallback, large catalogs, invalid/401/oversized lists, editable/default names, stable route IDs, compact model aliases, persisted language, bounded secret-free system logs, cached release check')
        finally:stop(process,port,directory,log)
    server.shutdown();server.server_close()
if __name__=='__main__':main()

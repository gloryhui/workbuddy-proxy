#!/usr/bin/env python3
"""Real binary: distinct and identical providers/models/keys, aliases and list sync."""
import argparse
import concurrent.futures
from http.server import BaseHTTPRequestHandler
import json
import http.client
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import time
import urllib.request
from smoke import ThreadingHTTPServer, free_port, stop

class Mock(BaseHTTPRequestHandler):
    protocol_version='HTTP/1.1'
    def log_message(self,*args):pass
    def do_POST(self):
        data=self.rfile.read(int(self.headers['Content-Length']));body=json.loads(data)
        key=self.headers.get('Authorization','')
        assert key in self.server.allowed_keys
        assert body['model'] in ['deepseek-v4.1-flash','other-model']
        assert 'tools' in body or body['max_tokens']==1
        if 'tools' in body:
            assert body['extra']=={'keep':123}
            assert body['messages'][0]['content']=='fixture'
            assert body['tools']==[{'type':'function','function':{'name':'fixture','parameters':{'type':'object'}}}]
            self.server.seen.append((key,body['model'],body.get('reasoning_effort'),body.get('thinking')))
            self.server.raw.append(data)
            assert self.headers['X-Fixture']=='transparent'
            assert self.path.endswith('?fixture=a%2Fb')
            assert self.headers.get('X-Hop') is None
        tag=self.server.label+'|'+key+'|'+body['model']
        result=('data: '+json.dumps({'choices':[{'delta':{'content':tag}}]})+'\n\ndata: [DONE]\n\n').encode()
        self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(result)));self.send_header('X-Upstream','preserved');self.send_header('Connection','X-Hop');self.send_header('X-Hop','remove');self.end_headers();self.wfile.write(result);self.wfile.flush()

def main():
    p=argparse.ArgumentParser();p.add_argument('--exe',required=True);args=p.parse_args()
    servers=[]
    for label,keys in [('A',['Bearer key-a','Bearer key-a2']),('B',['Bearer key-b'])]:
        server=ThreadingHTTPServer(('127.0.0.1',0),Mock);server.label=label;server.allowed_keys=keys;server.seen=[];server.raw=[]
        threading.Thread(target=server.serve_forever,daemon=True).start();servers.append(server)
    with tempfile.TemporaryDirectory(prefix='workbuddy-multi-') as folder:
        directory=Path(folder);port=free_port();connections=[]
        cases=[(0,'deepseek-v4.1-flash','key-a'),(0,'deepseek-v4.1-flash','key-a'),(0,'other-model','key-a'),(1,'deepseek-v4.1-flash','key-b'),(1,'other-model','key-b'),(0,'deepseek-v4.1-flash','key-a2')]
        for i,(supplier,model,key) in enumerate(cases):
            connections.append({'id':f'route-{i}','alias':f'wbp-route-{i}/{model}','name':f'账号 {i}','provider':servers[supplier].label,'upstream':f'http://127.0.0.1:{servers[supplier].server_port}/v1','api_key':key,'model':model,'reasoning_effort':'high' if i==0 else 'default'})
        config={'local_key':'local-fixture','port':port,'connections':connections,'installed':[]};(directory/'config.json').write_text(json.dumps(config))
        home=directory/'home';(home/'.workbuddy').mkdir(parents=True);models=home/'.workbuddy/models.json';unrelated={'id':'existing','apiKey':'keep','name':'keep'};models.write_text(json.dumps([unrelated]))
        env=os.environ.copy();env['HOME']=str(home);env['USERPROFILE']=str(home)
        log=(directory/'process.log').open('wb');process=subprocess.Popen([str(Path(args.exe).resolve()),'--headless','--config-dir',folder],env=env,stdout=log,stderr=log)
        try:
            for _ in range(200):
                try:
                    with urllib.request.urlopen(f'http://127.0.0.1:{port}/health',timeout=.5) as r:r.read()
                    break
                except OSError:time.sleep(.025)
            else:raise AssertionError('Core did not start')
            instance=json.loads((directory/'instance.json').read_text(encoding='utf-8'))
            def action(obj):
                req=urllib.request.Request(f'http://127.0.0.1:{port}/_control/action',data=json.dumps(obj).encode(),headers={'Authorization':'Bearer '+instance['management_key'],'Content-Type':'application/json'})
                with urllib.request.urlopen(req,timeout=5) as r:return json.load(r)['message']
            def call(c,scoped,bare=False):
                body={'model':c['model'] if bare else c['alias'],'messages':[{'role':'user','content':'fixture'}],'stream':True,'tools':[{'type':'function','function':{'name':'fixture','parameters':{'type':'object'}}}],'extra':{'keep':123}}
                path=f"/r/{c['id']}/v1/chat/completions" if scoped else '/v1/chat/completions'
                conn=http.client.HTTPConnection('127.0.0.1',port,timeout=5)
                conn.request('POST',path+'?fixture=a%2Fb',json.dumps(body).encode(),{'Authorization':'Bearer local-fixture','Content-Type':'application/json','X-Fixture':'transparent','Connection':'X-Hop','X-Hop':'remove'})
                r=conn.getresponse();result=r.read().decode();assert r.status==200
                assert r.getheader('X-Upstream')=='preserved';assert r.getheader('X-Hop') is None;conn.close()
                assert c['provider']+'|Bearer '+c['api_key']+'|'+c['model'] in result
                if c['reasoning_effort']=='default':
                    expected=json.dumps(body).encode().replace(json.dumps(body['model']).encode(),json.dumps(c['model']).encode(),1)
                    assert expected in servers[cases[connections.index(c)][0]].raw
            migrated=json.loads((directory/'config.json').read_text(encoding='utf-8'))
            assert len(migrated['providers'])==3 and [len(p['models']) for p in migrated['providers']]==[3,2,1]
            assert 'connections' not in migrated
            for scoped,bare in [(False,False),(True,False),(True,True)]:
                for c in connections:call(c,scoped,bare)
            # Two simultaneous requests with the same raw model reach different suppliers.
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                for f in [pool.submit(call,connections[i],True,True) for i in [0,3]]:f.result()
            for c in connections:assert 'HTTP 200' in action({'command':'test','connection_id':c['id']})
            for path in ['/v1/models',f"/r/{connections[0]['id']}/v1/models"]:
                req=urllib.request.Request(f'http://127.0.0.1:{port}'+path,headers={'Authorization':'Bearer local-fixture'})
                with urllib.request.urlopen(req) as r:items=json.load(r)['data'];assert len(items)==(6 if path=='/v1/models' else 1)
                assert all(item['id'].startswith('wbp-route-') for item in items)
            provider=migrated['providers'][0]
            req=urllib.request.Request(f"http://127.0.0.1:{port}/r/{provider['id']}/v1/models",headers={'Authorization':'Bearer local-fixture'})
            with urllib.request.urlopen(req) as r:assert len(json.load(r)['data'])==3
            # Changing a supplier credential updates every child model, without changing aliases.
            action({'command':'save','provider_id':provider['id'],'key':'key-a2'})
            for original in connections[:3]:
                original['api_key']='key-a2';call(original,True)
            action({'command':'save','provider_id':provider['id'],'key':'key-a'})
            for original in connections[:3]:original['api_key']='key-a'
            assert '6' in action({'command':'install'});listed=json.loads(models.read_text(encoding='utf-8'));assert listed[0]==unrelated;assert len(listed)==7;assert len({m['id'] for m in listed})==7
            assert all(m['apiKey']=='local-fixture' for m in listed[1:])
            assert len({m['url'] for m in listed[1:]})==6
            provider=migrated['providers'][0]
            action({'command':'save','provider_id':provider['id'],'models':'deepseek-v4.1-flash | 账号 0\nother-model | 账号 2','reasoning_effort':'keep'})
            action({'command':'install'});assert len(json.loads(models.read_text(encoding='utf-8')))==6
            action({'command':'restore'});assert json.loads(models.read_text(encoding='utf-8'))==[unrelated]
            # A deleted/unknown/mismatched route must never silently select another upstream.
            for path,model in [('/v1/chat/completions','deepseek-v4.1-flash'),('/v1/chat/completions','unknown'),('/r/route-1/v1/chat/completions',connections[1]['alias']),('/r/route-0/v1/chat/completions','other-model')]:
                req=urllib.request.Request(f'http://127.0.0.1:{port}'+path,data=json.dumps({'model':model,'messages':[{'role':'user','content':'fixture'}]}).encode(),headers={'Authorization':'Bearer local-fixture'})
                try:urllib.request.urlopen(req);raise AssertionError('Invalid routing was accepted')
                except urllib.error.HTTPError as e:assert e.code==400;e.read()
            assert any(effort=='high' and thinking=={'type':'enabled'} for _,_,effort,thinking in servers[0].seen)
            print('PASS: 6 connection combinations, credentials, alias/model rewrite, scoped URLs, concurrent supplier isolation, 6 tests, model discovery, batch sync/delete/restore, no fallback')
        finally:stop(process,port,folder,log)
    for server in servers:server.shutdown();server.server_close()
if __name__=='__main__':main()

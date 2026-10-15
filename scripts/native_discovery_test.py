#!/usr/bin/env python3
"""Real-core, independent native discovery pagination/auth/metadata fixtures."""
import argparse
import json
from pathlib import Path
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, parse_qs
import statistics_test as t

KEY='synthetic-discovery-key'
class Vendor(BaseHTTPRequestHandler):
    def log_message(self,*_): pass
    def do_GET(self):
        try:
            u=urlparse(self.path); q=parse_qs(u.query); self.server.seen.append(self.path)
            assert 'key' not in q and self.headers.get('Accept-Encoding')=='identity'
            if '/claude/' in u.path:
                assert self.headers.get('x-api-key')==KEY and self.headers.get('anthropic-version')=='2023-06-01'
                assert self.headers.get('Authorization') is None and self.headers.get('x-goog-api-key') is None
                assert q['limit']==['1000']
                cursor=q.get('after_id',[''])[0]
                row={'id':'claude-'+('two' if cursor else 'one'),'max_input_tokens':10000,'max_tokens':2048,
                    'capabilities':{'image_input':{'supported':True},'thinking':{'supported':True,'types':{'enabled':{'supported':True},'disabled':{'supported':True}}}}}
                data={'data':[row],'has_more':not bool(cursor),'last_id':row['id']}
                if '/loop/' in u.path: data.update(has_more=True,last_id='same')
                if '/missing/' in u.path: data={'data':[],'has_more':True}
                if '/pages/' in u.path: data.update(has_more=True,last_id=str(int(cursor or '0')+1))
            elif '/gemini/' in u.path:
                assert self.headers.get('x-goog-api-key')==KEY and self.headers.get('Authorization') is None and self.headers.get('x-api-key') is None
                assert q['pageSize']==['1000']
                cursor=q.get('pageToken',[''])[0]
                row={'name':'models/gemini-'+('two' if cursor else 'one'),'supportedGenerationMethods':['generateContent'],'thinking':True,'inputTokenLimit':10000,'outputTokenLimit':2048}
                data={'models':[row,{'name':'models/embedding','supportedGenerationMethods':['embedContent']}]}
                if not cursor: data['nextPageToken']='next+/='  # Must be query-escaped and decoded unchanged.
                else: assert cursor=='next+/='
                if '/loop/' in u.path:data['nextPageToken']='next+/='
            else:
                assert self.headers.get('Authorization')=='Bearer '+KEY
                assert self.headers.get('x-api-key') is None and self.headers.get('x-goog-api-key') is None
                assert u.path.endswith('/models')
                data={'data':[{'id':'responses-one','supported_reasoning_levels':['low','high'],'supportsReasoning':True}]}
            raw=json.dumps(data).encode()
            if '/huge/' in u.path: raw=b' '*1048577
            if '/many/' in u.path: raw=json.dumps({'data':[{'id':'m-'+str(i)} for i in range(4097)]}).encode()
            self.send_response(200);self.send_header('Content-Length',str(len(raw)));self.end_headers();self.wfile.write(raw)
        except (BrokenPipeError,ConnectionResetError):pass
        except Exception as e:
            self.server.failures.append(str(e));self.send_response(500);self.end_headers()

def main():
    p=argparse.ArgumentParser();p.add_argument('--exe',default='target/debug/workbuddy-proxy');a=p.parse_args()
    server=ThreadingHTTPServer(('127.0.0.1',0),Vendor);server.seen=[];server.failures=[]
    threading.Thread(target=server.serve_forever,daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix='wbp-native-discovery-') as tmp:
            folder=Path(tmp);port=t.free_port()
            folder.joinpath('config.json').write_text(json.dumps({'local_key':t.LOCAL_KEY,'port':port,'providers':[{'id':'provider-draft','name':'draft','upstream':'','models':[]}]}))
            process,log,_=t.start_core(a.exe,folder,port)
            before=folder.joinpath('config.json').read_bytes()
            try:
                origin='http://127.0.0.1:'+str(server.server_port)
                for protocol,path,expected,settings in [
                    ('openai_responses','/responses/v1/responses',['responses-one'],{}),
                    ('anthropic_messages','/claude/gateway/v1/messages',['claude-one','claude-two'],{'reasoning_profile':{'mode':'anthropic_budget','budgets':{'high':1024}}}),
                    ('gemini_generate_content','/gemini/gateway/v1beta',['gemini-one','gemini-two'],{'reasoning_profile':{'mode':'gemini_budget','budgets':{'high':512}}})]:
                    fields={'provider_id':'provider-draft','protocol':protocol,'protocol_settings':settings,'upstream':origin+path,'key':KEY}
                    assert t.action(folder,port,'fetch',**fields)==expected
                    assert folder.joinpath('config.json').read_bytes()==before
                    meta=t.action(folder,port,'model_metadata',model=expected[0],**fields)['metadata']
                    assert meta['capabilities']['useCustomProtocol'] is False
                    assert meta['capabilities']['supportsReasoning'] is True,meta
                    if protocol!='openai_responses':assert meta['capabilities']['maxOutputTokens']==2048
                    if protocol=='anthropic_messages':assert meta['capabilities']['supportsImages'] is True
                    # A different account cannot inherit this account's live model hints.
                    other_key=dict(fields,key='different-synthetic-key')
                    assert t.action(folder,port,'model_metadata',model=expected[0],**other_key)['metadata']['sources']==['unknown']
                    # Same URL/model with another wire protocol must not reuse live metadata.
                    other=dict(fields,protocol='openai_chat',protocol_settings={})
                    assert t.action(folder,port,'model_metadata',model=expected[0],**other)['metadata']['sources']==['unknown']
                    if protocol!='openai_responses':
                        for kind in ['loop','huge']+(['missing','pages'] if protocol=='anthropic_messages' else []):
                            bad=dict(fields,upstream=origin+'/'+kind+path)
                            error=t.action(folder,port,'fetch',expect_error=True,**bad)
                            assert KEY not in str(error)
                t.action(folder,port,'fetch',provider_id='provider-draft',upstream=origin+'/many/v1',key=KEY,expect_error=True)
                assert not server.failures,server.failures
                print('PASS: protocol auth, full endpoints/prefixes, escaped cursors, multiple pages, generateContent filter, unsaved drafts, metadata isolation, repeated/missing cursors, 16-page/4096-model/1-MiB limits')
            finally:t.stop_core(process,port,folder,log)
    finally:server.shutdown();server.server_close()
if __name__=='__main__':main()

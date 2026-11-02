#!/usr/bin/env python3
"""Authenticated updater API and visible-journal regression using synthetic data."""
import argparse
import json
from pathlib import Path
import tempfile
import threading
import time
import urllib.error
import urllib.request
from smoke import Mock, ThreadingHTTPServer, request, start, stop

def main():
    p=argparse.ArgumentParser();p.add_argument('--exe',required=True);p.add_argument('--output',type=Path);p.add_argument('--live-check',action='store_true');args=p.parse_args()
    server=ThreadingHTTPServer(('127.0.0.1',0),Mock);threading.Thread(target=server.serve_forever,daemon=True).start()
    result={}
    try:
        with tempfile.TemporaryDirectory(prefix='online-update-api-') as directory:
            process,port,log=start(args.exe,directory,f'http://127.0.0.1:{server.server_port}/v1')
            try:
                key=json.loads(Path(directory,'instance.json').read_text())['management_key']
                def action(payload,authenticated=True,timeout=12):
                    headers={'Content-Type':'application/json'}
                    if authenticated:headers['Authorization']='Bearer '+key
                    req=urllib.request.Request(f'http://127.0.0.1:{port}/_control/action',data=json.dumps(payload).encode(),headers=headers)
                    try:
                        with urllib.request.urlopen(req,timeout=timeout) as response:return response.status,json.load(response)['message']
                    except urllib.error.HTTPError as error:
                        data=json.load(error);return error.code,data.get('message',data.get('error'))
                for cmd in ['update_status','update_start','update_apply','update_cancel','update_ack']:
                    assert action({'command':cmd,'tag':'v99.0.0'},False)[0]==401,cmd
                Path(directory,'update-cache.json').write_text(json.dumps({'checked_at':int(time.time()),'tag':'v99.0.0','release':{'tag_name':'v99.0.0','name':'Synthetic release','body':'Fixture release notes','published_at':'2026-11-01T11:35:34Z','assets':[]}}))
                code,text=action({'command':'check_update','force':False});info=json.loads(text)
                assert code==200 and info['available'] and info['cached'] and info['name']=='Synthetic release' and info['body']=='Fixture release notes'
                assert info['url']=='https://github.com/gloryhui/workbuddy-proxy/releases/tag/v99.0.0'
                assert info['online_supported'] is False and info['online_error']
                for payload in [{'command':'update_start','tag':'../bad'},{'command':'update_start','tag':info['current']},{'command':'update_apply','tag':'v99.0.0'}]:assert action(payload)[0]==400
                code,text=action({'command':'update_status'});progress=json.loads(text)
                assert code==200 and progress['phase']=='idle' and any('Checking GitHub Releases' in s for s in progress['logs']) and any('v99.0.0' in s for s in progress['logs'])
                action({'command':'update_ack'});assert json.loads(action({'command':'update_status'})[1])['logs']==progress['logs'],'Closing a notice must retain its check log'
                snapshot=json.loads(action({'command':'snapshot'})[1]);journal=json.dumps(snapshot['logs'])
                assert 'Checking GitHub Releases' in journal and 'v99.0.0' in journal
                for secret in [key,'fixture-upstream-key','fixture-local-key']:assert secret not in journal
                conn,response=request(port,json.dumps({'model':'fixture-model','messages':[]}));assert response.status==200;response.read();conn.close()
                result={'authenticated_commands':True,'release_details':True,'unsafe_or_non_newer_versions_rejected':True,'apply_requires_verified_download':True,'visible_secret_free_check_log':True,'dismiss_retains_log':True,'invalid_apply_does_not_pause_relay':True}
                if args.live_check:
                    code,text=action({'command':'check_update','force':True});result['live_check']={'http':code}
                    if code==200:
                        live=json.loads(text);result['live_check'].update(tag=live['tag'],name=live['name'],published_at=live['published_at'],body_bytes=len(live['body'].encode()),cached=live['cached']);assert not live['cached']
                    else:result['live_check']['error']=text
                    logs=json.loads(action({'command':'update_status'})[1])['logs'];assert len(logs)>=4
                print(json.dumps(result,ensure_ascii=False,indent=2))
            finally:stop(process,port,directory,log)
    finally:server.shutdown();server.server_close()
    if args.output:args.output.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
if __name__=='__main__':main()

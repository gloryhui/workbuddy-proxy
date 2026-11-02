#!/usr/bin/env python3
"""Exercise the production updater helper only inside an explicitly owned fixture root.

The supplied old/current executables are never installed outside that root.
Network/download contracts are tested separately by Rust loopback tests.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import secrets
import shutil
import socket
import sqlite3
import subprocess
import sys
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from workbuddy_import_ui_sandbox import write_profile, command, kernel_protection

APP_ID = 'io.github.gloryhui.workbuddy-proxy'
class Mock(BaseHTTPRequestHandler):
    def log_message(self, *_): pass
    def do_GET(self):
        body=json.dumps({'data':[{'id':'fixture-model'}]}).encode();self.send_response(200);self.end_headers();self.wfile.write(body)
    def do_POST(self):
        self.rfile.read(int(self.headers.get('Content-Length',0)))
        body=json.dumps({'id':'synthetic','object':'chat.completion','model':'fixture-model','choices':[{'index':0,'message':{'role':'assistant','content':'fixture'},'finish_reason':'stop'}],'usage':{'prompt_tokens':12000,'completion_tokens':50,'total_tokens':12050}}).encode()
        self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)

def bundle(path, core, ui, version, broken=False):
    main=path/'Contents/MacOS';main.mkdir(parents=True)
    helper=path/'Contents/Helpers/WorkBuddy Proxy Settings.app'
    (helper/'Contents/MacOS').mkdir(parents=True)
    shutil.copy2(core,main/'workbuddy-proxy');shutil.copy2(ui,helper/'Contents/MacOS/workbuddy-proxy-gpui')
    if broken:
        source=path.parent/'startup-failure.c'
        source.write_text('#include <stdio.h>\n#include <string.h>\nint main(int n,char **v){if(n>1&&!strcmp(v[1],"--version")){puts("'+version+'");return 0;}return 7;}\n')
        subprocess.run(['xcrun','clang',str(source),'-o',str(main/'workbuddy-proxy')],check=True)
    for root,exe,identity in [(path,'workbuddy-proxy',APP_ID),(helper,'workbuddy-proxy-gpui',APP_ID+'.settings')]:
        with (root/'Contents/Info.plist').open('wb') as stream:
            plistlib.dump({'CFBundleIdentifier':identity,'CFBundleExecutable':exe,'CFBundleName':'Isolated update test','CFBundlePackageType':'APPL','CFBundleVersion':version,'CFBundleShortVersionString':version},stream)
    for root in [helper,path]:subprocess.run(['codesign','--force','--sign','-',str(root)],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    return path

def main():
    p=argparse.ArgumentParser();p.add_argument('--core',required=True,type=Path);p.add_argument('--ui',required=True,type=Path);p.add_argument('--before-core',required=True,type=Path);p.add_argument('--before-ui',required=True,type=Path);p.add_argument('--temporary-root',required=True,type=Path);p.add_argument('--output',required=True,type=Path);args=p.parse_args()
    if sys.platform!='darwin':raise SystemExit('This harness requires macOS; Windows runtime is not validated here.')
    base=args.temporary_root.resolve(strict=True)
    assert base.is_dir() and base.stat().st_uid==os.getuid() and base.parent==Path(os.environ.get('TMPDIR',str(base.parent))).resolve()
    version=subprocess.check_output([str(args.core.resolve()),'--version'],text=True).strip()
    results=[]
    opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
    for scenario in ['success','startup-rollback']:
        root=base/('engine-'+scenario);root.mkdir();config=root/'profile';config.mkdir()
        policy=write_profile(root/'protect-real-profiles.sb',Path.home())
        environment=dict(os.environ,TMPDIR=str(root),WORKBUDDY_PROXY_CATALOG_TEST_URL='http://127.0.0.1:1/catalog')
        server=ThreadingHTTPServer(('127.0.0.1',0),Mock);threading.Thread(target=server.serve_forever,daemon=True).start()
        with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
        cfg={'local_key':'synthetic-update-local','language':'zh','port':port,'autostart':False,'installed':[],'providers':[{'id':'update-fixture','name':'Synthetic update fixture','upstream':f'http://127.0.0.1:{server.server_port}/v1','api_key':'synthetic-update-upstream','models':[{'id':'update-route','model':'fixture-model','name':'Fixture','alias':'fixture'}]}]}
        (config/'config.json').write_text(json.dumps(cfg));(config/'config.json').chmod(0o600)
        app=bundle(root/'WorkBuddy Proxy.app',args.before_core,args.before_ui,version)
        token=secrets.token_hex(24);session=config/'updates'/token;session.mkdir(parents=True);session.chmod(0o700)
        stage=root/('.workbuddy-update-'+token);stage.mkdir();stage.chmod(0o700)
        staged=bundle(stage/'new.app',args.core,args.ui,version,broken=scenario=='startup-rollback')
        package=session/'verified-fixture.dmg';package.write_bytes(b'owned helper integration fixture; streaming SHA validation has independent tests')
        plan={'target':{'Mac':str(app)},'staged':str(staged),'backup':str(stage/'previous'),'stage_root':str(stage),'session':str(session),'config_dir':str(config),'tag':'v'+version,'package':str(package),'package_size':package.stat().st_size,'package_digest':hashlib.sha256(package.read_bytes()).hexdigest(),'data_backup':str(session/'profile-backup')}
        (session/'plan.json').write_text(json.dumps(plan));shutil.copy2(args.core,session/'update-helper')
        old=None;helper=None;handles=[];record={'scenario':scenario,'synthetic_only':True,'same_version_different_protocol_fixture':True}
        record['input_sha256']={name:hashlib.sha256(path.read_bytes()).hexdigest() for name,path in [('core',args.core),('gpui',args.ui),('before_core',args.before_core),('before_gpui',args.before_ui)]}
        def info():return json.loads((config/'instance.json').read_text())
        def action(payload):
            value=info();req=urllib.request.Request(f'http://127.0.0.1:{value["port"]}/_control/action',data=json.dumps(payload).encode(),headers={'Authorization':'Bearer '+value['management_key'],'Content-Type':'application/json'})
            with opener.open(req,timeout=4) as response:return json.load(response)['message']
        def ready(deadline=20):
            until=time.monotonic()+deadline
            while time.monotonic()<until:
                try:
                    value=info()
                    with opener.open(f'http://127.0.0.1:{value["port"]}/health',timeout=2) as response:health=json.load(response)
                    if health['ready'] and json.loads(action({'command':'stats_health'}))['state']=='ready':return health
                except (OSError,ValueError,KeyError):pass
                time.sleep(.1)
            raise AssertionError('Fixture core did not become ready')
        def stop_owned():
            pids=[]
            for name in ['instance.json','frontend.json','gpui.json']:
                try:pids.append(json.loads((config/name).read_text())['pid'])
                except (OSError,ValueError,KeyError):pass
            for name in ['gpui-exit-host','gpui-stop.json']:(config/name).write_text('true')
            try:
                value=info();req=urllib.request.Request(f'http://127.0.0.1:{value["port"]}/_control/stop',data=b'{}',headers={'Authorization':'Bearer '+value['management_key']})
                with opener.open(req,timeout=3) as response:response.read()
            except (OSError,ValueError,KeyError):pass
            until=time.monotonic()+12
            while time.monotonic()<until:
                live=[]
                for pid in pids:
                    result=subprocess.run(['ps','-o','state=,command=','-p',str(pid)],text=True,stdout=subprocess.PIPE)
                    if result.returncode==0 and result.stdout.strip() and not result.stdout.strip().startswith('Z'):live.append((pid,result.stdout.strip()))
                if not live:return
                time.sleep(.1)
            for pid,cmd in live:
                assert str(root) in cmd,(pid,cmd)
                os.kill(pid,15)
            raise AssertionError('Fixture required fallback termination: '+str([pid for pid,_ in live]))
        try:
            log=(root/'old-core.log').open('wb');handles.append(log)
            old=subprocess.Popen(command(policy,app/'Contents/MacOS/workbuddy-proxy',['--headless','--config-dir',config]),env=environment,stdout=log,stderr=log)
            record['before_health']=ready();record['sandbox']=kernel_protection(Path.home(),root,pid=old.pid)
            assert record['sandbox']['enforced']
            for _ in range(3):
                req=urllib.request.Request(f'http://127.0.0.1:{port}/r/update-route/v1/chat/completions',data=json.dumps({'model':'fixture-model','messages':[]}).encode(),headers={'Authorization':'Bearer synthetic-update-local','Content-Type':'application/json'})
                with opener.open(req,timeout=3) as response:assert json.load(response)['usage']['total_tokens']==12050
            before=(config/'config.json').read_bytes()
            log=(root/'helper.log').open('wb');handles.append(log)
            helper=subprocess.Popen(command(policy,session/'update-helper',['--apply-update',session/'plan.json']),env=environment,stdout=log,stderr=log)
            until=time.monotonic()+15
            while not (session/'helper-ready').exists() and time.monotonic()<until:
                assert helper.poll() is None,'Helper exited before readiness';time.sleep(.05)
            assert (session/'helper-ready').exists()
            # Production helper must wait while the old core still owns its lock.
            time.sleep(.15);assert old.poll() is None;assert not (stage/'previous').exists()
            value=info();req=urllib.request.Request(f'http://127.0.0.1:{port}/_control/stop',data=b'{}',headers={'Authorization':'Bearer '+value['management_key']})
            with opener.open(req,timeout=3) as response:response.read()
            old.wait(timeout=10);assert helper.wait(timeout=110)==0
            record['after_health']=ready();result=json.loads((config/'update-install-result.json').read_text())
            assert result['phase']==('complete' if scenario=='success' else 'failed'),result
            assert record['after_health']['control_protocol']==(13 if scenario=='success' else 12),record
            assert not (config/'update-maintenance').exists(), 'Maintenance gate remained after verified recovery'
            assert not (session/'profile-backup/update-maintenance').exists(), 'Maintenance marker must not enter the data backup'
            record['maintenance_released_after_health']=True
            assert (config/'config.json').read_bytes()==before
            connection=sqlite3.connect((config/'usage.sqlite3').as_uri()+'?mode=ro',uri=True)
            connection.execute('PRAGMA query_only=ON');rows,tokens=connection.execute('select count(*),sum(total_tokens) from usage_requests').fetchone();connection.close()
            assert (rows,tokens)==(3,36150),(rows,tokens)
            record.update({'phase':result['phase'],'preserved_config_sha256':hashlib.sha256(before).hexdigest(),'usage_rows':rows,'usage_tokens':tokens,'data_backup_exists':(session/'profile-backup/usage.sqlite3').is_file(),'application_backup_retained':(stage/'previous').is_dir(),'log_entries':len(result['logs']),'passed':True})
            results.append(record);print(scenario+': PASS',flush=True)
        finally:
            stop_owned()
            for process in [old,helper]:
                if process is not None:
                    if process.poll() is None:process.terminate()
                    process.wait(timeout=10)
            server.shutdown();server.server_close()
            for handle in handles:handle.close()
            args.output.mkdir(parents=True,exist_ok=True)
            shutil.copy2(root/'helper.log',args.output/(scenario+'-helper.log'))
            (args.output/'engine-results.json').write_text(json.dumps(results,indent=2)+'\n')
if __name__=='__main__':main()

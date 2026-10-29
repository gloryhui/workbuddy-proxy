#!/usr/bin/env python3
"""Verify UI provider draft transactions and discovery against an isolated real core.

Example: python3 scripts/studio_provider_draft_test.py --core-exe target/release/workbuddy-proxy --output report.json
CARGO_TARGET_DIR may select an existing compatible UI cache. No production profile is opened.
"""
import argparse
import gzip
import http.client
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--core-exe', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    core_exe = args.core_exe.resolve(); output = args.output.resolve()
    real_home = Path.home()
    cargo = shutil.which('cargo')
    if not cargo: raise RuntimeError('Cargo is required for the native UI test harness')
    original = dict(os.environ)
    with tempfile.TemporaryDirectory(prefix='studio-draft-discovery-') as temporary:
        folder = Path(temporary).resolve()
        for key, child in {'HOME':'home','USERPROFILE':'home','XDG_CONFIG_HOME':'xdg-config','XDG_DATA_HOME':'xdg-data','XDG_CACHE_HOME':'xdg-cache','XDG_STATE_HOME':'xdg-state','TMPDIR':'tmp','TMP':'tmp','TEMP':'tmp'}.items():
            path=folder/child;path.mkdir(exist_ok=True);os.environ[key]=str(path)
        os.environ.setdefault('CARGO_HOME',str(real_home/'.cargo'))
        os.environ.setdefault('RUSTUP_HOME',str(real_home/'.rustup'))
        os.environ['WORKBUDDY_PROXY_CATALOG_TEST_URL']='http://127.0.0.1:1/catalog'
        sys.path.insert(0,str(ROOT/'scripts'))
        import smoke
        import native_discovery_test as vendor
        import workbuddy_import_ui_sandbox as guard
        current_request=threading.local()
        fail_warm=folder/'fail-warm'
        class Failures(list):
            def append(self,error):super().append(str(getattr(current_request,'path','unknown'))+': '+repr(error))
        class Vendor(vendor.Vendor):
            def do_GET(self):
                current_request.path=self.path
                if '/fresh-transfer-true/' in self.path and fail_warm.exists():
                    self.send_response(503);self.end_headers();return
                special = any(marker in self.path for marker in ['/azure','/encoding/','/builtin/'])
                if not special: return super().do_GET()
                self.server.seen.append(self.path)
                try:
                    if '/azure' in self.path:
                        assert self.headers.get('api-key') == vendor.KEY
                        assert not self.headers.get('Authorization')
                        assert '/deployments?' in self.path and 'api-version=2022-12-01' in self.path
                        if '/azure404/' in self.path:
                            self.send_response(404);self.end_headers();return
                        data={'data':[{'id':'azure-one','supportsReasoning':True,'supported_reasoning_levels':['low','high']}]}
                    else:
                        assert self.headers.get('Authorization') == 'Bearer '+vendor.KEY
                        data={'data':[{'id':'gpt-4o','supports_images':False,'reasoning':False,'limit':{'context':8192,'input':4000,'output':1000}}]}
                    raw=json.dumps(data).encode()
                    self.send_response(200)
                    if '/encoding/' in self.path:
                        raw=gzip.compress(raw);self.send_header('Content-Encoding','gzip')
                    self.send_header('Content-Length',str(len(raw)));self.end_headers();self.wfile.write(raw)
                except (BrokenPipeError, ConnectionResetError): pass
                except Exception as error:
                    self.server.failures.append(str(error));self.send_response(500);self.end_headers()
        server=ThreadingHTTPServer(('127.0.0.1',0),Vendor);server.seen=[];server.failures=Failures()
        thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
        core=folder/core_exe.name;shutil.copy2(core_exe,core)
        config=folder/'config';config.mkdir()
        port=vendor.t.free_port()
        (config/'config.json').write_text(json.dumps({'local_key':'fixture-local-key','language':'en','port':port,'autostart':False,'providers':[{'id':'source-provider','name':'Source','upstream':'http://127.0.0.1:1/v1','api_key':vendor.KEY,'models':[{'id':'source-one','alias':'wbp-source-one','name':'one','model':'one','reasoning_effort':'low'},{'id':'source-two','alias':'wbp-source-two','name':'two','model':'two','reasoning_effort':'high'}]}],'installed':[]}))
        os.chmod(config/'config.json',0o600)
        rpc_counts=folder/'rpc-counts.json';rpc_counts.write_text(json.dumps({'creates':0,'saves':0}))
        fail_save=folder/'fail-save';ambiguous=folder/'ambiguous-create'
        proxy_server=None;proxy_thread=None

        proof={'enforced':False,'reason':'non-macOS: synthetic environment and explicit config directory'}
        original_popen=smoke.subprocess.Popen
        children=[]
        policy=guard.write_profile(folder/'policy.sb',real_home) if sys.platform=='darwin' else None
        def launch(command,*a,**kw):
            if policy is not None and command and str(command[0])==str(core):command=guard.command(policy,core,command[1:])
            child=original_popen(command,*a,**kw);children.append(child);return child
        smoke.subprocess.Popen=launch
        process=None
        try:
            process,log,_=vendor.t.start_core(core,config,port)
            instance=json.loads((config/'instance.json').read_text())
            if policy is not None:
                proof=guard.kernel_protection(real_home,folder,pid=instance['pid'])
                assert proof['enforced'], 'Core sandbox attestation failed'
            saved=json.loads((config/'config.json').read_text())
            provider=saved['providers'][0]['id']
            before=hashlib.sha256((config/'config.json').read_bytes()).hexdigest()
            class Management(BaseHTTPRequestHandler):
                def log_message(self,*_):pass
                def do_POST(self):
                    raw=self.rfile.read(int(self.headers.get('Content-Length',0)));payload=json.loads(raw)
                    command=payload.get('command');counts=json.loads(rpc_counts.read_text())
                    if command in ('add','duplicate'):counts['creates']+=1
                    if command=='save':counts['saves']+=1
                    rpc_counts.write_text(json.dumps(counts))
                    def forward(body):
                        connection=http.client.HTTPConnection('127.0.0.1',port,timeout=105)
                        try:
                            connection.request('POST','/_control/action',body,{'Authorization':self.headers.get('Authorization',''),'Content-Type':'application/json'})
                            response=connection.getresponse();return response.status,response.read()
                        finally:connection.close()
                    if command=='save' and payload.get('provider_id')!=provider and fail_save.exists():
                        fail_save.unlink();status,data=400,json.dumps({'message':'synthetic post-create save failure'}).encode()
                    else:
                        status,data=forward(raw)
                        if command=='add' and status==200 and ambiguous.exists():
                            ambiguous.unlink();extra_status,_=forward(json.dumps({'command':'add','ui':True}).encode());assert extra_status==200
                    self.send_response(status);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
            proxy_server=ThreadingHTTPServer(('127.0.0.1',0),Management)
            proxy_thread=threading.Thread(target=proxy_server.serve_forever,daemon=True);proxy_thread.start()
            redirected=dict(instance,port=proxy_server.server_port)
            (config/'instance.json').write_text(json.dumps(redirected))

            origin='http://127.0.0.1:'+str(server.server_port)
            cases=[]
            def case(name,protocol,path,settings=None,error=False):
                cases.append({'name':name,'error':error,'payload':{'provider_id':provider,'upstream':origin+path,'key':vendor.KEY,'protocol':protocol,'protocol_settings':settings or {}}})
            case('chat','openai_chat','/chat/v1')
            case('responses','openai_responses','/responses/v1/responses')
            case('responses-map','openai_responses','/responses/v1',{'responses_efforts':{'minimal':'low','max':'xhigh'}})
            anthropic={'reasoning_profile':{'mode':'anthropic_budget','budgets':{'high':1024}}}
            gemini={'reasoning_profile':{'mode':'gemini_budget','budgets':{'high':512}}}
            case('anthropic-pages','anthropic_messages','/claude/gateway/v1/messages',anthropic)
            case('anthropic-adaptive-evidence','anthropic_messages','/claude/gateway/v1',{'reasoning_profile':{'mode':'anthropic_adaptive','levels':{'high':'high'}}})
            case('gemini-pages-filter','gemini_generate_content','/gemini/gateway/v1beta',gemini)
            case('gemini-level','gemini_generate_content','/gemini/gateway/v1beta',{'reasoning_profile':{'mode':'gemini_level','levels':{'high':'high'}}})
            for protocol in ['openai_chat','openai_responses']:
                case('azure-'+protocol,protocol,'/azure/openai/v1',{'authentication':'azure_api_key'})
            case('azure-no-fallback','openai_chat','/azure404/openai/v1',{'authentication':'azure_api_key'},True)
            case('builtin-live-override','openai_chat','/builtin/v1')
            case('encoded-rejected','openai_chat','/encoding/v1',error=True)
            case('4097-rejected','openai_chat','/many/v1',error=True)
            for kind in ['loop','huge','missing','pages']:
                case('anthropic-'+kind,'anthropic_messages','/'+kind+'/claude/gateway/v1',anthropic,True)
            for kind in ['loop','huge']:
                case('gemini-'+kind,'gemini_generate_content','/'+kind+'/gemini/gateway/v1beta',gemini,True)
            report=folder/'result.json';manifest=folder/'fixture.json'
            creation_report=folder/'creation.json'
            manifest.write_text(json.dumps({'core_dir':str(config),'output':str(report),'creation_output':str(creation_report),'fail_save_marker':str(fail_save),'fail_warm_marker':str(fail_warm),'ambiguous_marker':str(ambiguous),'rpc_counts':str(rpc_counts),'cases':cases}))
            env=dict(os.environ,WORKBUDDY_PROXY_DISCOVERY_PARITY_FIXTURE=str(manifest))
            result=subprocess.run([cargo,'test','--manifest-path',str(ROOT/'gpui/Cargo.toml'),'draft_discovery::tests::core_parity','--','--ignored','--exact'],cwd=ROOT,env=env)
            assert result.returncode==0, 'UI / core parity test failed'
            assert not server.failures,server.failures
            assert hashlib.sha256((config/'config.json').read_bytes()).hexdigest()==before
            creation_result=subprocess.run([cargo,'test','--manifest-path',str(ROOT/'gpui/Cargo.toml'),'provider_create::tests::real_core_contract','--','--ignored','--exact'],cwd=ROOT,env=env)
            assert creation_result.returncode==0, 'Real-core creation contract failed'
            data=json.loads(report.read_text());data['creation']=json.loads(creation_report.read_text());data.update(core_sha256=hashlib.sha256(core.read_bytes()).hexdigest(),core_sandbox_enforced=proof['enforced'],config_sha256_unchanged_for_discovery=True,test_harness_exit_code=result.returncode)
        finally:
            try:
                if process is not None:smoke.stop(process,port,config,log)
            finally:
                server.shutdown();thread.join(timeout=5);server.server_close()
                if proxy_server is not None:proxy_server.shutdown();proxy_thread.join(timeout=5);proxy_server.server_close();assert not proxy_thread.is_alive()
                for child in children:
                    if child.poll() is None:
                        child.terminate()
                        try:child.wait(timeout=5)
                        except subprocess.TimeoutExpired:child.kill();child.wait(timeout=5)
                    else:child.wait(timeout=5)
                smoke.subprocess.Popen=original_popen
                os.environ.clear();os.environ.update(original)
                assert not thread.is_alive()
        data.update(core_exit_code=process.returncode,all_owned_children_waited=True,mock_server_joined=True)
        output.parent.mkdir(parents=True,exist_ok=True);output.write_text(json.dumps(data,indent=2)+'\n')
        print(json.dumps({'report':str(output),'cases':len(data['cases']),'core_exit_code':process.returncode}))

if __name__=='__main__': main()

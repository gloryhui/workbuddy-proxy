# -*- coding: utf-8 -*-
"""Isolated GUI fixture. No real profiles, credentials or upstream endpoints."""
import argparse, atexit, hashlib, json, os, pathlib, plistlib, shutil, signal, socket, sqlite3, subprocess, tempfile, threading, time, urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from workbuddy_import_test import real_user_home
from workbuddy_import_macho import verify_code_identity
from workbuddy_import_ui_sandbox import bootstrap_environment, command as sandbox_command, write_profile
args=argparse.ArgumentParser(description=__doc__)
args.add_argument('--guard-gui',action='store_true',help='Diagnostic native sandbox bootstrap; current CUA cannot bind that sandboxed AppKit process')
args.add_argument('--sign-bundle',action='store_true',help='Ad-hoc sign the complete unique QA bundle and prove stripped code matches the input')
args.add_argument('--output',type=pathlib.Path,help='Evidence/fixture folder; defaults to configured TMPDIR')
args.add_argument('--exe',type=pathlib.Path,help='Explicit core under test')
args.add_argument('--gpui',type=pathlib.Path,help='Explicit GPUI under test')
args.add_argument('--keep-core-after-ui-exit',action='store_true',help='Keep the isolated core for close/reopen checks until native-acceptance-finished is created')
options=args.parse_args()
ROOT=pathlib.Path(__file__).resolve().parents[1]
review=ROOT/'dist/workbuddy-import-review'
folder=options.output.resolve() if options.output else pathlib.Path(tempfile.gettempdir())/('ui-release-'+str(time.time_ns()))
config=folder/'config'; home=folder/'home'; project=home/'.workbuddy/projects/demo'
config.mkdir(parents=True); project.mkdir(parents=True)
class Mock(BaseHTTPRequestHandler):
    def log_message(self,*args): pass
    def do_POST(self):
        self.rfile.read(int(self.headers.get('Content-Length',0)))
        data=json.dumps({'id':'synthetic-response','object':'chat.completion','model':'mock-model','choices':[{'index':0,'message':{'role':'assistant','content':'synthetic'},'finish_reason':'stop'}],
            'usage':{'prompt_tokens':500,'completion_tokens':50,'total_tokens':550}}).encode()
        self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
server=ThreadingHTTPServer(('127.0.0.1',0),Mock)
threading.Thread(target=server.serve_forever,daemon=True).start()
with socket.socket() as socket_: socket_.bind(('127.0.0.1',0)); port=socket_.getsockname()[1]
cfg={'local_key':'fixture-local-key','language':'zh','port':port,'autostart':False,'providers':[{'id':'review-provider','name':'Review','upstream':'http://127.0.0.1:%d/v1'%server.server_port,
    'api_key':'fixture-upstream-key','models':[{'id':'proxy-route','alias':'Review/mock-model','name':'mock-model','model':'mock-model','reasoning_effort':'default'}]}],'installed':[]}
(config/'config.json').write_text(json.dumps(cfg));os.chmod(config/'config.json',0o600)
(home/'.workbuddy/models.json').write_text('[]')
at=int(time.time()*1000)
for session,model,prompt,output,credit in [('builtin','hy3',100,20,1.14),('direct','custom-local:review/direct',200,40,2.28)]:
    row={'type':'function_call','sessionId':session,'timestamp':at,'providerData':{'messageId':session,'requestModelId':model,'model':model,
        'rawUsage':{'prompt_tokens':prompt,'completion_tokens':output,'total_tokens':prompt+output,'credit':credit}}}
    (project/(session+'.jsonl')).write_text(json.dumps(row)+'\n')
profile_paths={'HOME':home,'USERPROFILE':home,'APPDATA':folder/'appdata',
    'LOCALAPPDATA':folder/'localappdata','XDG_CONFIG_HOME':folder/'xdg-config',
    'XDG_DATA_HOME':folder/'xdg-data','XDG_CACHE_HOME':folder/'xdg-cache',
    'XDG_STATE_HOME':folder/'xdg-state','TMPDIR':folder/'tmp','TMP':folder/'tmp','TEMP':folder/'tmp'}
for path in profile_paths.values():path.mkdir(parents=True,exist_ok=True)
env=dict(os.environ,**{name:str(path) for name,path in profile_paths.items()},
    WORKBUDDY_PROXY_CONFIG_DIR=str(config),WORKBUDDY_PROXY_UI_EVENT_METRICS='1',WORKBUDDY_PROXY_CATALOG_TEST_URL='http://127.0.0.1:1/catalog')
# Native app lookup may launch a duplicate without CLI arguments. Keep that
# LaunchServices path inside this same fixture instead of the default profile.
launch_environment={name:env[name] for name in [*profile_paths,
    'WORKBUDDY_PROXY_CONFIG_DIR','WORKBUDDY_PROXY_CATALOG_TEST_URL','WORKBUDDY_PROXY_UI_EVENT_METRICS']}
log=(folder/'core.log').open('wb')
shutil.copy2(options.exe or ROOT/'target/release/workbuddy-proxy', folder/'review-core')
policy=write_profile(folder/'source-protection.sb',real_user_home())
core=subprocess.Popen(sandbox_command(policy,folder/'review-core',['--headless','--config-dir',str(config),'--port',str(port)]),env=env,stdout=log,stderr=log)
ui=None
def cleanup():
    if ui is not None and ui.poll() is None:
        ui.terminate()
        try: ui.wait(timeout=5)
        except subprocess.TimeoutExpired:
            ui.kill(); ui.wait(timeout=5)
    if core.poll() is None:
        try:
            if not globals().get('key'):
                raise RuntimeError('core key not ready')
            request=urllib.request.Request('http://127.0.0.1:%d/_control/stop'%port,
                data=b'{}',headers={'Content-Type':'application/json','Authorization':'Bearer '+key})
            opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
            with opener.open(request,timeout=3) as response:response.read()
            core.wait(timeout=5)
        except (OSError,RuntimeError,subprocess.TimeoutExpired):
            core.terminate()
            try: core.wait(timeout=5)
            except subprocess.TimeoutExpired:
                core.kill(); core.wait(timeout=5)
def stop_fixture(signum, frame):
    raise SystemExit(128+signum)
atexit.register(cleanup)
signal.signal(signal.SIGTERM,stop_fixture)
key=None
for _ in range(1200):
    if core.poll() is not None: raise RuntimeError('isolated core exited; inspect scoped core.log')
    try: key=json.loads((config/'instance.json').read_text())['management_key'];break
    except (OSError,KeyError,ValueError): time.sleep(.05)

if not key:
    core.terminate()
    raise AssertionError('isolated core readiness timed out')

def command(payload):
    request=urllib.request.Request('http://127.0.0.1:%d/_control/action'%port,data=json.dumps(payload).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer '+key})
    with urllib.request.urlopen(request,timeout=10) as response: result=json.loads(response.read())
    return json.loads(result['message'])
for _ in range(1200):
    try:
        if command({'command':'stats_health'})['state']=='ready':break
    except (OSError,ValueError,KeyError):pass
    time.sleep(.05)
settings=command({'command':'stats_settings_get'})
settings=command({'command':'stats_settings_update','enabled':True,'retention_days':0,'expected_revision':settings['revision']})
command({'command':'stats_workbuddy_settings_update','enabled':True,'expected_revision':settings['revision']})
def number(sql):
    with sqlite3.connect('file:%s?mode=ro'%(config/'usage.sqlite3'),uri=True) as conn:return conn.execute(sql).fetchone()[0]
def wait(sql,expected):
    for _ in range(300):
        if number(sql)==expected:return
        time.sleep(.05)
    raise AssertionError((sql,number(sql),expected))
wait("SELECT COUNT(*) FROM workbuddy_import_ledger WHERE state='pending_classification'",1)
preview=command({'command':'stats_workbuddy_mapping_preview','model_id':'custom-local:review/direct','from_ms':at-1000,'to_ms':at+1000,'classification':'direct'})
command({'command':'stats_workbuddy_mapping_apply','token':preview['token']})
request=urllib.request.Request('http://127.0.0.1:%d/r/proxy-route/v1/chat/completions'%port,data=json.dumps({'model':'mock-model','messages':[{'role':'user','content':'synthetic'}],'stream':False}).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer fixture-local-key'})
with urllib.request.urlopen(request,timeout=10) as response: assert json.loads(response.read())['usage']['total_tokens']==550
wait('SELECT COUNT(*) FROM usage_requests',3)
assert number('SELECT SUM(total_tokens) FROM usage_requests')==910
assert number('SELECT SUM(workbuddy_credit_nanos) FROM usage_requests')==3420000000
row={'type':'function_call','sessionId':'pending','timestamp':at+1,'providerData':{'messageId':'pending','requestModelId':'custom-local:review/pending','model':'mock-pending','rawUsage':{'prompt_tokens':300,'completion_tokens':60,'total_tokens':360,'credit':3.42}}}
(project/'pending.jsonl').write_text(json.dumps(row)+'\n')
wait("SELECT COUNT(*) FROM workbuddy_import_ledger WHERE state='pending_classification'",1)
bundle=folder/'WorkBuddy Proxy Review.app'; macos=bundle/'Contents/MacOS';macos.mkdir(parents=True)
shutil.copy2(options.gpui or ROOT/'gpui/target/release/workbuddy-proxy-gpui',macos/'workbuddy-proxy-gpui')
ui_env=bootstrap_environment(folder,policy,env) if options.guard_gui else env
if options.guard_gui:
    launch_environment.update({name:ui_env[name] for name in [
        'DYLD_INSERT_LIBRARIES','WORKBUDDY_PROXY_QA_POLICY','WORKBUDDY_PROXY_QA_PROOF_DIR']})
with (bundle/'Contents/Info.plist').open('wb') as file:
    plistlib.dump({'CFBundleExecutable':'workbuddy-proxy-gpui','CFBundleIdentifier':'com.workbuddy.proxy.codexreview.t'+folder.name.removeprefix('ui-release-'),'CFBundleName':'WorkBuddy Proxy Review','CFBundleDisplayName':'WorkBuddy Proxy Review','CFBundlePackageType':'APPL','NSHighResolutionCapable':True,'LSEnvironment':launch_environment},file)
ui_code=macos/'workbuddy-proxy-gpui'
signature_identity=None
if options.sign_bundle:
    subprocess.run(['codesign','--force','--sign','-',str(bundle)],check=True)
    subprocess.run(['codesign','--verify','--strict',str(bundle)],check=True)
    signature_identity=verify_code_identity(options.gpui or ROOT/'gpui/target/release/workbuddy-proxy-gpui',ui_code)
    (folder/'signature-identity.json').write_text(json.dumps(signature_identity,indent=2)+'\n')
ui_log=(folder/'gpui.log').open('wb')
ui=subprocess.Popen([str(ui_code),'--config-dir',str(config)],env=ui_env,stdout=ui_log,stderr=ui_log)
proof=folder/'source-guard-proofs'/('%d.enforced'%ui.pid)
for _ in range(1200):
    if ui.poll() is not None:raise RuntimeError('protected GUI exited before readiness')
    if (not options.guard_gui or proof.exists()) and (config/'gpui.json').exists():break
    time.sleep(.05)
else:raise RuntimeError('native GUI source protection was not verified')
manifest={'folder':str(folder),'bundle':str(bundle),'core_pid':core.pid,'ui_pid':ui.pid,'fixture_pid':os.getpid(),'port':port,'bundle_id':'com.workbuddy.proxy.codexreview.t'+folder.name.removeprefix('ui-release-'),'runtime_ui_binary':str(ui_code.relative_to(folder)),'sandbox_policy_sha256':hashlib.sha256(policy.read_bytes()).hexdigest(),'source_guard_library_sha256':hashlib.sha256((folder/'source-guard.dylib').read_bytes()).hexdigest() if options.guard_gui else None,'sandbox_ui_pid':ui.pid,'gui_kernel_guard':options.guard_gui,'native_binding_rule':'Unique app identity and isolated LSEnvironment apply to initial and duplicate native launches. Core has kernel profile protection; GUI bootstrap mode is a separate CUA binding diagnostic.','binary_sha256':{'core':hashlib.sha256((folder/'review-core').read_bytes()).hexdigest(),'ui':hashlib.sha256(ui_code.read_bytes()).hexdigest()},'expected':{'all':910,'builtin':120,'supplier':790,'credits':'3.42'}}
manifest.update(complete_bundle_signed=options.sign_bundle,signature_identity=signature_identity)
(folder/'fixture.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2))
(review/'ui-live-current.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2))
print(json.dumps(manifest,ensure_ascii=False),flush=True)
try:
    while core.poll() is None:
        if (folder/'native-acceptance-finished').exists():break
        if ui.poll() is not None and not options.keep_core_after_ui_exit:break
        time.sleep(.1)
finally:
    terminal={'ui_exit_before_cleanup':ui.poll(),'core_exit_before_cleanup':core.poll()}
    cleanup()
    server.shutdown()
    terminal.update(ui_exit_after_cleanup=ui.poll(),core_exit_after_cleanup=core.poll())
    (folder/'terminal-status.json').write_text(json.dumps(terminal,indent=2)+'\n')

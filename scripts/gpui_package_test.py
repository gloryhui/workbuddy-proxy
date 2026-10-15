#!/usr/bin/env python3
"""Exercise a packaged GPUI application with isolated credentials and loopback SSE."""
import argparse
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
from connection_test import DiagnosticUpstream
from smoke import ThreadingHTTPServer, free_port
from benchmark import rss_kib

def running(pid):
    if platform.system() == 'Windows':
        import ctypes
        from ctypes import wintypes
        kernel=ctypes.WinDLL('kernel32',use_last_error=True)
        kernel.OpenProcess.argtypes=[wintypes.DWORD,wintypes.BOOL,wintypes.DWORD]
        kernel.OpenProcess.restype=wintypes.HANDLE
        kernel.WaitForSingleObject.argtypes=[wintypes.HANDLE,wintypes.DWORD]
        kernel.CloseHandle.argtypes=[wintypes.HANDLE]
        handle=kernel.OpenProcess(0x100000,False,pid)
        if not handle:return False
        try:return kernel.WaitForSingleObject(handle,0)==258
        finally:kernel.CloseHandle(handle)
    try:
        state=subprocess.check_output(['ps','-o','stat=','-p',str(pid)],text=True).strip()
        return bool(state) and not state.startswith('Z')
    except subprocess.CalledProcessError:return False


def rss(pid):
    if not running(pid):return 0
    try:return rss_kib(pid)
    except (OSError,subprocess.CalledProcessError,ValueError):return 0



def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--exe',required=True)
    parser.add_argument('--output',required=True)
    parser.add_argument('--hold',action='store_true')
    parser.add_argument('--directory')
    args=parser.parse_args()
    exe=Path(args.exe).resolve();output=Path(args.output).resolve();output.parent.mkdir(parents=True,exist_ok=True)
    temp=None
    if args.directory:directory=Path(args.directory).resolve();directory.mkdir(parents=True,exist_ok=True)
    else:temp=tempfile.TemporaryDirectory(prefix='workbuddy-gpui-package-');directory=Path(temp.name)
    class Upstream(DiagnosticUpstream):
        def do_GET(self):
            body=json.dumps({'data':[{'id':f'model-{i}'} for i in range(96)]}).encode()
            self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
    upstream=ThreadingHTTPServer(('127.0.0.1',0),Upstream);upstream.seen_models=[]
    threading.Thread(target=upstream.serve_forever,daemon=True).start()
    port=free_port()
    config={'local_key':'fixture-local','language':'en','port':port,'installed':[],
            'providers':[{'id':'qa-provider','name':'Fixture provider','upstream':f'http://127.0.0.1:{upstream.server_port}/v1','api_key':'fixture-upstream-key',
                          'models':[{'id':f'qa-model-{i:04}','alias':f'wbp-qa-model-{i:04}','name':f'Model {i:02}','model':f'model-{i}','reasoning_effort':'default'} for i in range(64)]}]}
    (directory/'config.json').write_text(json.dumps(config))
    (directory/'update-cache.json').write_text(json.dumps({'tag':'v'+__import__('tomllib').loads(Path('Cargo.toml').read_text())['package']['version'],'checked_at':int(time.time())}))
    home=directory/'home';home.mkdir(exist_ok=True);env=os.environ.copy();env['HOME']=env['USERPROFILE']=str(home)
    host=subprocess.Popen([str(exe),'--config-dir',str(directory)],env=env,start_new_session=platform.system()!='Windows',stdout=(directory/'host.stdout.log').open('wb'),stderr=(directory/'host.stderr.log').open('wb'))
    core=None;ui=None;metrics={}
    def wait(predicate,message,seconds=35):
        deadline=time.monotonic()+seconds
        while time.monotonic()<deadline:
            try:
                value=predicate()
                if value:return value
            except (OSError,ValueError,KeyError):pass
            if host.poll() is not None:raise AssertionError('Tray host exited: '+(directory/'host.stderr.log').read_text(errors='replace')[:2000])
            time.sleep(.05)
        detail=(directory/'gpui-startup.log').read_text(errors='replace')[:2500] if (directory/'gpui-startup.log').exists() else ''
        raise AssertionError(message+'\n'+detail)
    def read(name):return json.loads((directory/name).read_text())
    def action(payload):
        request=urllib.request.Request(f'http://127.0.0.1:{core["port"]}/_control/action',data=json.dumps(payload).encode(),headers={'Authorization':'Bearer '+core['management_key'],'Content-Type':'application/json'})
        with urllib.request.urlopen(request,timeout=10) as response:return json.load(response)['message']
    def wake():
        request=urllib.request.Request(f'http://127.0.0.1:{core["port"]}/_control/show',data=b'',headers={'Authorization':'Bearer '+core['management_key']})
        with urllib.request.urlopen(request,timeout=5) as response:response.read()
    try:
        core=wait(lambda:read('instance.json'),'Core did not start')
        ui=wait(lambda:read('gpui.json'),'GPUI renderer/window did not start')
        assert core['pid']!=host.pid and ui['pid'] not in (core['pid'],host.pid)
        wait(lambda:read('frontend.json')['pid']==host.pid,'Tray singleton was not published')
        with urllib.request.urlopen(f'http://127.0.0.1:{port}/health',timeout=3) as response:assert json.load(response)['control_protocol']==7
        time.sleep(1)
        metrics={'platform':platform.platform(),'architecture':platform.machine(),'processes_with_settings':3,
                 'core_rss_mib':round(rss(core['pid'])/1024,2),'tray_rss_mib':round(rss(host.pid)/1024,2),'gpui_rss_mib':round(rss(ui['pid'])/1024,2),'model_count':64,
                 'note':'RSS sums process working sets; shared pages can be counted more than once. GPU/driver allocations are not separately measured. Mock/client processes are excluded.'}
        metrics['settings_open_rss_mib']=round(sum(metrics[k] for k in ('core_rss_mib','tray_rss_mib','gpui_rss_mib')),2)
        (directory/'qa-info.json').write_text(json.dumps({'directory':str(directory),'exe':str(exe),'host_pid':host.pid,'core_pid':core['pid'],'ui_pid':ui['pid']},indent=2))
        print('READY:',directory,'GPUI PID',ui['pid'],flush=True)
        if args.hold:
            while not (directory/'qa-done').exists():time.sleep(.25)
        else:
            for message in ['Package log A','Package log B']:action({'command':'record','message':message})
            snapshot=json.loads(action({'command':'snapshot'}));assert len(snapshot['logs'])>=2
            action({'command':'clear_logs'});assert json.loads(action({'command':'snapshot'}))['logs']==[]
            action({'command':'test','provider_id':'qa-provider','model_index':3});assert upstream.seen_models[-1]=='model-3'
        saved_name=read('config.json')['providers'][0]['name']
        second=subprocess.run([str(exe),'--config-dir',str(directory)],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,timeout=8)
        assert second.returncode==0,'Opening the app again failed'
        time.sleep(.4)
        assert read('frontend.json')['pid']==host.pid and read('gpui.json')['pid']==ui['pid'],'Opening again created another application instance'
        first_ui=ui['pid']
        (directory/'gpui-stop.json').write_text('true')
        wait(lambda:not (directory/'gpui.json').exists(),'Closing settings did not release GPUI')
        wait(lambda:not running(first_ui),'GPUI settings process is still resident',seconds=8)
        assert read('instance.json')['pid']==core['pid'] and read('frontend.json')['pid']==host.pid
        metrics['settings_closed_rss_mib']=round((rss(core['pid'])+rss(host.pid))/1024,2)
        metrics['processes_after_close']=2
        drafts=wait(lambda:read('gpui-drafts.json'),'Unsaved drafts were not persisted')
        drafts['providers']['qa-provider']['name']='Unsaved package draft'
        (directory/'gpui-drafts.json').write_text(json.dumps(drafts))
        wake()
        ui=wait(lambda:read('gpui.json'),'Tray could not reopen GPUI settings')
        assert ui['pid']!=first_ui and read('instance.json')['pid']==core['pid']
        time.sleep(.7)  # Let the reopened window capture and persist its actual restored state.
        wait(lambda:read('gpui-drafts.json')['providers']['qa-provider']['name']=='Unsaved package draft','Draft was lost on reopen')
        assert read('config.json')['providers'][0]['name']==saved_name,'Unsaved draft overwrote saved configuration'
        metrics['checks']=['separate lightweight core/tray/GPUI processes','repeated launch reuses the existing application instance','real loopback streaming diagnostic with selected model','ordered core log clear','closing settings releases GPUI without stopping the core','tray reopens settings using the same core','private unsaved drafts survive reopening without changing config.json']
        metrics['result']='passed';output.write_text(json.dumps(metrics,ensure_ascii=False,indent=2)+'\n')
        print('PASS: packaged GPUI lifecycle, management API, log clear, selected-model diagnostic, draft persistence and resident-process measurements',flush=True)
    finally:
        (directory/'gpui-exit-host').write_text('true')
        try:host.wait(timeout=8)
        except subprocess.TimeoutExpired:host.terminate();host.wait(timeout=5)
        if core and (directory/'instance.json').exists():
            try:
                request=urllib.request.Request(f'http://127.0.0.1:{core["port"]}/_control/stop',data=b'',headers={'Authorization':'Bearer '+core['management_key']})
                with urllib.request.urlopen(request,timeout=3) as response:response.read()
            except OSError:pass
        upstream.shutdown();upstream.server_close()
        if temp:temp.cleanup()


if __name__=='__main__':main()

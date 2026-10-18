#!/usr/bin/env python3
"""Reproducible loopback measurements, not an Internet-model speed claim."""
import argparse
import concurrent.futures
import http.client
import contextlib
import subprocess
import sys
import json
import platform
from pathlib import Path
import statistics
import tempfile
import threading
import time
import urllib.request
from smoke import Mock, ThreadingHTTPServer, start, stop, request


def mock_process(marker):
    # Keep client and server in separate interpreters: a shared Python GIL can
    # add scheduler waits to pooled requests, particularly on Windows.
    class NullList:
        def append(self,value):pass
    Mock.received=NullList()
    server=ThreadingHTTPServer(('127.0.0.1',0),Mock)
    Path(marker).write_text(str(server.server_port),encoding='utf-8')
    try:server.serve_forever()
    finally:server.server_close()


@contextlib.contextmanager
def mock_server(directory):
    marker=Path(directory)/'mock-port';log=Path(directory)/'mock-start.log'
    with log.open('wb') as out:
        child=subprocess.Popen([sys.executable,str(Path(__file__).resolve()),'--mock-server',str(marker)],stdin=subprocess.DEVNULL,stdout=out,stderr=out)
        try:
            for _ in range(600):
                if child.poll() is not None:raise AssertionError('Mock exited: '+log.read_text(encoding='utf-8',errors='replace')[:8192])
                try:
                    port=int(marker.read_text(encoding='utf-8'));break
                except (OSError,ValueError):time.sleep(.05)
            else:raise AssertionError('Mock process did not start')
            yield port
        finally:
            if child.poll() is None:child.terminate()
            child.wait(timeout=5)


def rss_kib(pid):
    import subprocess
    if sys.platform == 'darwin':
        from workbuddy_import_resources import usage
        return usage(pid)[0] / 1024
    if platform.system() == 'Windows':
        import ctypes
        from ctypes import wintypes
        class Memory(ctypes.Structure):
            _fields_=[('cb',wintypes.DWORD),('PageFaultCount',wintypes.DWORD),
                      ('PeakWorkingSetSize',ctypes.c_size_t),('WorkingSetSize',ctypes.c_size_t),
                      ('QuotaPeakPagedPoolUsage',ctypes.c_size_t),('QuotaPagedPoolUsage',ctypes.c_size_t),
                      ('QuotaPeakNonPagedPoolUsage',ctypes.c_size_t),('QuotaNonPagedPoolUsage',ctypes.c_size_t),
                      ('PagefileUsage',ctypes.c_size_t),('PeakPagefileUsage',ctypes.c_size_t)]
        kernel=ctypes.WinDLL('kernel32',use_last_error=True);psapi=ctypes.WinDLL('psapi',use_last_error=True)
        kernel.OpenProcess.restype=wintypes.HANDLE
        kernel.OpenProcess.argtypes=[wintypes.DWORD,wintypes.BOOL,wintypes.DWORD]
        kernel.CloseHandle.argtypes=[wintypes.HANDLE]
        psapi.GetProcessMemoryInfo.argtypes=[wintypes.HANDLE,ctypes.POINTER(Memory),wintypes.DWORD]
        handle=kernel.OpenProcess(0x410,False,pid)
        if not handle:raise OSError(ctypes.get_last_error())
        try:
            mem=Memory();mem.cb=ctypes.sizeof(mem)
            if not psapi.GetProcessMemoryInfo(handle,ctypes.byref(mem),mem.cb):raise OSError(ctypes.get_last_error())
            return mem.WorkingSetSize/1024
        finally:kernel.CloseHandle(handle)
    return float(subprocess.check_output(['ps','-o','rss=','-p',str(pid)],text=True).strip())


def percentiles(values):
    values=sorted(values);return {'p50':round(statistics.median(values),3),
        'p95':round(values[min(len(values)-1,int(len(values)*.95))],3)}


def main():
    p=argparse.ArgumentParser();p.add_argument('--exe',required=True);p.add_argument('--output',required=True)
    p.add_argument('--requests',type=int,default=200);p.add_argument('--models',type=int,default=64);p.add_argument('--gui',action='store_true');args=p.parse_args()
    with tempfile.TemporaryDirectory(prefix='workbuddy-proxy-bench-') as folder, mock_server(folder) as upstream_port:
        process,port,log=start(args.exe,folder,f'http://127.0.0.1:{upstream_port}/v1',args.gui)
        try:
            instance=json.loads(Path(folder,'instance.json').read_text(encoding='utf-8'))
            assert 1<=args.models<=256
            req=urllib.request.Request(f'http://127.0.0.1:{port}/_control/action',data=json.dumps({'command':'save','models':'\n'.join(f'bench-model-{i}' for i in range(args.models))}).encode(),headers={'Authorization':'Bearer '+instance['management_key'],'Content-Type':'application/json'})
            with urllib.request.urlopen(req,timeout=5) as r:r.read()
            saved=json.loads(Path(folder,'config.json').read_text(encoding='utf-8'))['providers'][0]['models'][-1]
            real_model=saved['model'];alias=saved['alias']
            pids=set([process.pid,instance.get('pid',process.pid)])
            total_rss=lambda:sum(rss_kib(pid) for pid in pids)
            time.sleep(1);idle=total_rss();core_idle=rss_kib(instance.get('pid',process.pid));peak=[idle];done=threading.Event()
            def sample():
                while not done.wait(.15):
                    try: peak.append(total_rss())
                    except Exception: break
            watcher=threading.Thread(target=sample,daemon=True);watcher.start()
            payload=json.dumps({'model':real_model,'stream':True,'messages':[{'role':'user','content':'bench'}]}).encode()
            def measured(proxy,payload):
                started=time.perf_counter()
                if proxy: conn,res=request(port,payload.replace(json.dumps(real_model).encode(),json.dumps(alias).encode(),1))
                else:
                    conn=http.client.HTTPConnection('127.0.0.1',upstream_port,timeout=5)
                    conn.request('POST','/v1/chat/completions',payload,{'Content-Type':'application/json'});res=conn.getresponse()
                assert res.status==200;res.read();conn.close();return (time.perf_counter()-started)*1000
            for _ in range(10): measured(True,payload)
            direct=[measured(False,payload) for _ in range(args.requests)]
            proxied=[measured(True,payload) for _ in range(args.requests)]
            large=json.dumps({'model':real_model,'stream':True,'messages':[{'role':'user','content':'x'*(1024*1024)}]}).encode()
            # Four concurrent 1 MiB requests, repeated, while RSS is sampled.
            start_time=time.perf_counter()
            with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
                list(pool.map(lambda _:measured(True,large),range(80)))
            large_seconds=time.perf_counter()-start_time
            time.sleep(.5);settled=total_rss();done.set();watcher.join()
            result={'platform':platform.platform(),'architecture':platform.machine(),'mode':'native GUI' if args.gui else 'headless',
                'configured_providers':1,'configured_models':args.models,'alias_rewrite':True,
                'binary_bytes':Path(args.exe).stat().st_size,'idle_rss_mib':round(idle/1024,2),
                'core_idle_rss_mib':round(core_idle/1024,2),'measured_process_count':len(pids),
                'sampled_peak_rss_mib':round(max(peak)/1024,2),'post_load_rss_mib':round(settled/1024,2),
                'small_request_count':args.requests,'direct_loopback_ms':percentiles(direct),
                'proxied_loopback_ms':percentiles(proxied),
                'median_proxy_overhead_ms':round(statistics.median(proxied)-statistics.median(direct),3),
                'large_request_size_mib':1,'large_concurrency':4,'large_requests':80,'large_seconds':round(large_seconds,3),
                'notes':'RSS sums the core and settings PIDs while native GUI is open (shared pages can be counted twice); headless has one PID. Excludes mock/client processes. Loopback HTTP mock in a separate process, no shared client/server GIL; no TLS or Internet/model latency. Client opens new connections; upstream is pooled. No cross-platform ranking implied.'}
            Path(args.output).parent.mkdir(parents=True,exist_ok=True);Path(args.output).write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
        finally: stop(process,port,folder,log)


if __name__=='__main__':
    if len(sys.argv)==3 and sys.argv[1]=='--mock-server':mock_process(sys.argv[2])
    else:main()

#!/usr/bin/env python3
"""Close and reopen the real Win32 UI without restarting the proxy core."""
import argparse
import ctypes
from ctypes import wintypes
import json
from pathlib import Path
import subprocess
import tempfile
import time
import urllib.request
from smoke import free_port

def main():
    p=argparse.ArgumentParser();p.add_argument('--exe',required=True);args=p.parse_args()
    exe=str(Path(args.exe).resolve());user=ctypes.WinDLL('user32',use_last_error=True)
    cbtype=ctypes.WINFUNCTYPE(wintypes.BOOL,wintypes.HWND,wintypes.LPARAM)
    user.GetWindowThreadProcessId.argtypes=[wintypes.HWND,ctypes.POINTER(wintypes.DWORD)]
    user.IsWindowVisible.argtypes=[wintypes.HWND]
    user.PostMessageW.argtypes=[wintypes.HWND,wintypes.UINT,wintypes.WPARAM,wintypes.LPARAM]
    def window(pid):
        found=[]
        def callback(hwnd,param):
            value=wintypes.DWORD();user.GetWindowThreadProcessId(hwnd,ctypes.byref(value))
            if value.value==pid and user.IsWindowVisible(hwnd):found.append(hwnd)
            return True
        user.EnumWindows(cbtype(callback),0);return found[0] if found else None
    def wait_window(process):
        for _ in range(200):
            if process.poll() is not None:raise AssertionError('UI exited before showing a window')
            h=window(process.pid)
            if h:return h
            time.sleep(.025)
        raise AssertionError('UI window did not appear')
    with tempfile.TemporaryDirectory(prefix='workbuddy-lifecycle-') as directory:
        config={'api_key':'fixture-key','local_key':'fixture-local','upstream':'https://opencode.ai/zen/go/v1','model':'deepseek-v4.1-flash','port':free_port(),'installed':None}
        Path(directory,'config.json').write_text(json.dumps(config))
        command=[exe,'--config-dir',directory];first=subprocess.Popen(command)
        second=None;core=None
        try:
            h=wait_window(first);core=json.loads(Path(directory,'instance.json').read_text())
            assert core['pid']!=first.pid
            user.PostMessageW(h,0x0010,0,0);assert first.wait(timeout=5)==0
            with urllib.request.urlopen(f"http://127.0.0.1:{core['port']}/health",timeout=3) as r:assert json.load(r)['ready']
            second=subprocess.Popen(command);h=wait_window(second)
            assert json.loads(Path(directory,'instance.json').read_text())['pid']==core['pid']
            user.PostMessageW(h,0x0010,0,0);assert second.wait(timeout=5)==0
            print('PASS: native UI closes, core keeps running, reopened UI uses same core PID')
        finally:
            for process in [first,second]:
                if process is not None and process.poll() is None:process.terminate();process.wait(timeout=5)
            if core:
                req=urllib.request.Request(f"http://127.0.0.1:{core['port']}/_control/stop",data=b'',headers={'Authorization':'Bearer '+core['management_key']})
                with urllib.request.urlopen(req,timeout=3) as r:r.read()
                for _ in range(200):
                    if not Path(directory,'instance.json').exists():break
                    time.sleep(.025)
if __name__=='__main__':main()

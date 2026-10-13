#!/usr/bin/env python3
"""Close/minimize to tray, restore, and exit the real Win32 application."""
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
    user.GetDlgItem.argtypes=[wintypes.HWND,ctypes.c_int]
    user.GetDlgItem.restype=wintypes.HWND
    user.SendMessageW.argtypes=[wintypes.HWND,wintypes.UINT,wintypes.WPARAM,wintypes.LPARAM]
    user.SendMessageW.restype=wintypes.LPARAM
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
        def profile():
            for attempt in range(20):
                try:return json.loads(Path(directory,'config.json').read_text(encoding='utf-8'))
                except PermissionError:
                    if attempt==19:raise
                    time.sleep(.005) # MoveFileEx can hold a brief replacement lock.
        command=[exe,'--config-dir',directory];first=subprocess.Popen(command)
        second=None;core=None
        try:
            h=wait_window(first);core=json.loads(Path(directory,'instance.json').read_text(encoding='utf-8'))
            assert core['pid']!=first.pid
            # The real window, taskbar and header use embedded application icons.
            big=user.SendMessageW(h,0x007F,1,0);small=user.SendMessageW(h,0x007F,0,0)
            assert big and small,'Application icon is missing'
            logo=user.GetDlgItem(h,108);assert logo
            assert user.SendMessageW(logo,0x0171,0,0)==big,'Settings header icon differs'
            kernel=ctypes.WinDLL('kernel32');kernel.LoadLibraryExW.argtypes=[wintypes.LPCWSTR,wintypes.HANDLE,wintypes.DWORD];kernel.LoadLibraryExW.restype=wintypes.HMODULE
            kernel.FindResourceW.argtypes=[wintypes.HMODULE,wintypes.LPCWSTR,wintypes.LPCWSTR];kernel.FindResourceW.restype=wintypes.HANDLE
            kernel.FreeLibrary.argtypes=[wintypes.HMODULE]
            module=kernel.LoadLibraryExW(exe,None,0x2|0x20);assert module
            try:assert kernel.FindResourceW(module,ctypes.cast(ctypes.c_void_p(1),wintypes.LPCWSTR),ctypes.cast(ctypes.c_void_p(14),wintypes.LPCWSTR)),'Executable icon group is missing'
            finally:kernel.FreeLibrary(module)

            combo=user.GetDlgItem(h,104);assert combo
            assert user.SendMessageW(combo,0x0146,0,0)==6, 'Thinking selector is missing choices'
            user.SendMessageW(combo,0x014E,3,0) # select High
            user.PostMessageW(h,0x0111,201,0) # Save through actual native UI / FFI
            for _ in range(200):
                if profile()['providers'][0]['models'][0].get('reasoning_effort')=='high':break
                time.sleep(.025)
            else:raise AssertionError('Native Save did not persist thinking effort')
            # Edit the actual Win32 multiline model control, then save through FFI.
            model_edit=user.GetDlgItem(h,103);assert model_edit
            model_text=ctypes.c_wchar_p('deepseek-v4.1-flash | Flash\r\nother-model | Other')
            user.SendMessageW(model_edit,0x000C,0,ctypes.cast(model_text,ctypes.c_void_p).value)
            user.PostMessageW(h,0x0111,201,0)
            for _ in range(200):
                entries=profile()['providers'][0]['models']
                if len(entries)==2:break
                time.sleep(.025)
            else:raise AssertionError('Native multiline model save failed')
            assert entries[0]['name']=='Flash' and entries[1]['model']=='other-model'
            listing=user.GetDlgItem(h,107);assert listing
            assert user.SendMessageW(listing,0x018B,0,0)==1 # LB_GETCOUNT
            user.PostMessageW(h,0x0111,207,0) # Duplicate the selected configured connection
            for _ in range(200):
                saved=profile()['providers']
                if len(saved)==2:break
                time.sleep(.025)
            else:raise AssertionError('Native Duplicate did not add a connection')
            assert saved[0]['api_key']==saved[1]['api_key'] and saved[0]['models'][0]['alias']!=saved[1]['models'][0]['alias']
            user.PostMessageW(h,0x0111,208,0) # Delete duplicate, leaves original configured connection
            for _ in range(200):
                if len(profile()['providers'])==1:break
                time.sleep(.025)
            else:raise AssertionError('Native Delete failed')
            for message,wparam in [(0x0010,0),(0x0112,0xF020)]: # Close; minimize system command
                user.PostMessageW(h,message,wparam,0)
                for _ in range(200):
                    assert first.poll() is None,'Closing the window terminated the tray process'
                    if not user.IsWindowVisible(h):break
                    time.sleep(.025)
                else:raise AssertionError('Window did not hide to tray')
                with urllib.request.urlopen(f"http://127.0.0.1:{core['port']}/health",timeout=3) as r:assert json.load(r)['ready']
                user.PostMessageW(h,0x8001,1,0x0202) # tray left click
                assert wait_window(first)==h
            user.PostMessageW(h,0x0010,0,0)
            time.sleep(.1)
            second=subprocess.Popen(command);assert second.wait(timeout=5)==0
            assert wait_window(first)==h, 'Second launch did not restore original tray window'
            assert json.loads(Path(directory,'instance.json').read_text(encoding='utf-8'))['pid']==core['pid']
            user.PostMessageW(h,0x0111,302,0) # same command as tray Quit
            assert first.wait(timeout=5)==0
            for _ in range(200):
                if not Path(directory,'instance.json').exists():break
                time.sleep(.025)
            else:raise AssertionError('Tray Quit left the core running')
            core=None
            print('PASS: embedded executable/window/header icons; native effort Save; Close/minimize hide to tray; tray click/relaunch restore same UI and core; Quit stops both')
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

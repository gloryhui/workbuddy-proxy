#!/usr/bin/env python3
"""Close/minimize to tray, restore, and exit the real Win32 application."""
import argparse
import ctypes
from ctypes import wintypes
import json
import tomllib
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
        version=tomllib.loads(Path('Cargo.toml').read_text())['package']['version']
        Path(directory,'update-cache.json').write_text(json.dumps({'tag':'v'+version,'checked_at':int(time.time())}))
        def profile():
            for attempt in range(20):
                try:return json.loads(Path(directory,'config.json').read_text(encoding='utf-8'))
                except PermissionError:
                    if attempt==19:raise
                    time.sleep(.005) # MoveFileEx can hold a brief replacement lock.
        command=[exe,'--config-dir',directory];first=subprocess.Popen(command)
        second=None;core=None;duplicates=[]
        try:
            def caption(control):
                buffer=ctypes.create_unicode_buffer(256)
                # WM_GETTEXT marshals control text across processes; GetWindowText cannot read another process's Edit contents.
                user.SendMessageW(control,0x000D,256,ctypes.cast(buffer,ctypes.c_void_p).value);return buffer.value
            h=wait_window(first);core=json.loads(Path(directory,'instance.json').read_text(encoding='utf-8'))
            assert core['pid']!=first.pid
            for _ in range(200):
                ready=Path(directory,'frontend.json')
                if ready.exists() and json.loads(ready.read_text())['pid']==first.pid:break
                time.sleep(.025)
            else:raise AssertionError('Native UI readiness was not published')
            duplicates=[subprocess.Popen(command) for _ in range(5)]
            for process in duplicates:assert process.wait(timeout=12)==0,'Concurrent relaunch did not exit cleanly'
            assert first.poll() is None
            assert json.loads(ready.read_text())['pid']==first.pid
            assert json.loads(Path(directory,'instance.json').read_text())['pid']==core['pid']
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
            key_field=user.GetDlgItem(h,102);toggle=user.GetDlgItem(h,212);assert key_field and toggle
            assert caption(key_field)=='fixture-key','Saved key disappeared from the form'
            assert user.SendMessageW(key_field,0x00D2,0,0)==ord('*'),'Key must start masked'
            user.SendMessageW(h,0x0111,212,0)
            assert user.SendMessageW(key_field,0x00D2,0,0)==0 and caption(key_field)=='fixture-key'
            user.SendMessageW(h,0x0111,212,0)
            assert user.SendMessageW(key_field,0x00D2,0,0)==ord('*') and caption(key_field)=='fixture-key'
            user.SendMessageW(key_field,0x0203,1,4|(4<<16)) # Double-click selects the complete field.
            selection=user.SendMessageW(key_field,0x00B0,0,0)
            assert selection&0xffff==0 and selection>>16==len('fixture-key')
            # Edit the real table's first display name through its native cell editor.
            table=user.GetDlgItem(h,103);assert table
            user.FindWindowExW.argtypes=[wintypes.HWND,wintypes.HWND,wintypes.LPCWSTR,wintypes.LPCWSTR];user.FindWindowExW.restype=wintypes.HWND
            def editor():
                for _ in range(200):
                    edit=user.FindWindowExW(table,None,'Edit',None)
                    if edit:return edit
                    time.sleep(.025)
                raise AssertionError('Table cell editor did not open')
            def enter(edit,text):
                value=ctypes.c_wchar_p(text)
                user.SendMessageW(edit,0x000C,0,ctypes.cast(value,ctypes.c_void_p).value)
                user.PostMessageW(edit,0x0100,13,0)
                for _ in range(200):
                    if not user.FindWindowExW(table,None,'Edit',None):return
                    time.sleep(.025)
                raise AssertionError('Table edit did not commit')
            # Header is followed by row 0; double-click its name column.
            point=10 | (32<<16)
            user.PostMessageW(table,0x0201,1,point);user.PostMessageW(table,0x0202,0,point)
            user.PostMessageW(table,0x0203,1,point);user.PostMessageW(table,0x0202,0,point)
            enter(editor(),'Flash')
            user.SendMessageW(h,0x0111,210,0) # Add model and edit its ID.
            enter(editor(),'other-model')
            user.PostMessageW(h,0x0111,201,0)
            for _ in range(200):
                entries=profile()['providers'][0]['models']
                if len(entries)==2:break
                time.sleep(.025)
            else:raise AssertionError('Native model table save failed')
            assert entries[0]['name']=='Flash' and entries[1]['name']=='other-model' and entries[1]['model']=='other-model'
            for model in ['third-model','fourth-model']:
                user.SendMessageW(h,0x0111,210,0);enter(editor(),model)
            user.PostMessageW(h,0x0111,201,0)
            for _ in range(200):
                if len(profile()['providers'][0]['models'])==4:break
                time.sleep(.025)
            else:raise AssertionError('Extra test model rows did not save')
            # Switching languages persists a preference and uses one language per button.
            language=user.GetDlgItem(h,110);assert language
            for index,code,title in [(0,'zh','保存'),(1,'en','Save')]:
                user.SendMessageW(language,0x014E,index,0)
                user.PostMessageW(h,0x0111,110 | (1<<16),0)
                for _ in range(200):
                    if profile().get('language')==code and caption(user.GetDlgItem(h,201))==title:break
                    time.sleep(.025)
                else:raise AssertionError('Native language switch did not apply')
            assert user.GetDlgItem(h,112),'Scrollable log panel is missing'
            update=user.GetDlgItem(h,113);assert update
            for _ in range(200):
                if caption(update)=='Up to date':break
                time.sleep(.025)
            else:raise AssertionError('Automatic Release check did not update the header button')
            # Native modifier-click selection, then Backspace deletes exactly the selected rows.
            kernel=ctypes.WinDLL('kernel32',use_last_error=True)
            kernel.OpenProcess.argtypes=[wintypes.DWORD,wintypes.BOOL,wintypes.DWORD];kernel.OpenProcess.restype=wintypes.HANDLE
            kernel.VirtualAllocEx.argtypes=[wintypes.HANDLE,ctypes.c_void_p,ctypes.c_size_t,wintypes.DWORD,wintypes.DWORD];kernel.VirtualAllocEx.restype=ctypes.c_void_p
            kernel.WriteProcessMemory.argtypes=[wintypes.HANDLE,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_size_t,ctypes.c_void_p]
            kernel.ReadProcessMemory.argtypes=[wintypes.HANDLE,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_size_t,ctypes.c_void_p]
            kernel.VirtualFreeEx.argtypes=[wintypes.HANDLE,ctypes.c_void_p,ctypes.c_size_t,wintypes.DWORD]
            kernel.CloseHandle.argtypes=[wintypes.HANDLE]
            user.SetForegroundWindow.argtypes=[wintypes.HWND];user.SetForegroundWindow(h)
            user.ClientToScreen.argtypes=[wintypes.HWND,ctypes.POINTER(wintypes.POINT)]
            user.SetCursorPos.argtypes=[ctypes.c_int,ctypes.c_int]
            target=kernel.OpenProcess(0x438,False,first.pid);assert target
            memory=kernel.VirtualAllocEx(target,None,ctypes.sizeof(wintypes.RECT),0x3000,4);assert memory
            def click_row(index,modifier=None):
                rect=wintypes.RECT();assert kernel.WriteProcessMemory(target,memory,ctypes.byref(rect),ctypes.sizeof(rect),None)
                assert user.SendMessageW(table,0x100E,index,memory)
                assert kernel.ReadProcessMemory(target,memory,ctypes.byref(rect),ctypes.sizeof(rect),None)
                point=wintypes.POINT(12,(rect.top+rect.bottom)//2)
                assert user.ClientToScreen(table,ctypes.byref(point))
                assert user.SetCursorPos(point.x,point.y)
                if modifier:user.keybd_event(modifier,0,0,0)
                # Real input goes through the UI thread queue, including modifier state.
                user.mouse_event(2,0,0,0,0);user.mouse_event(4,0,0,0,0)
                time.sleep(.12)
                if modifier:user.keybd_event(modifier,0,2,0)
                time.sleep(.08)
            try:
                click_row(0);click_row(2,16)
                assert user.SendMessageW(table,0x1032,0,0)==3,'Shift-click did not select a range'
                click_row(1,17)
                assert user.SendMessageW(table,0x1032,0,0)==2,'Ctrl-click did not toggle one row'
                click_row(3,17)
                assert user.SendMessageW(table,0x1032,0,0)==3,'Ctrl-click did not add a separate row'
                from windows_preview import capture
                capture(h,'bench-results/native-windows-preview.png')
                user.PostMessageW(table,0x0100,8,0);user.PostMessageW(h,0x0111,201,0)
                for _ in range(200):
                    remaining=profile()['providers'][0]['models']
                    if len(remaining)==1:break
                    time.sleep(.025)
                else:raise AssertionError('Backspace did not delete selected rows')
                assert remaining[0]['model']=='other-model','Batch delete changed an unselected model'
                # Ctrl+A + Delete removes the remaining model; typed Backspace stays in the cell editor.
                user.keybd_event(17,0,0,0);time.sleep(.04);user.SendMessageW(table,0x0100,65,0);user.keybd_event(17,0,2,0)
                assert user.SendMessageW(table,0x1032,0,0)==1
                user.SendMessageW(table,0x0100,46,0);user.PostMessageW(h,0x0111,201,0)
                for _ in range(200):
                    if not profile()['providers'][0]['models']:break
                    time.sleep(.025)
                else:raise AssertionError('Delete did not remove the selected model')
                user.SendMessageW(h,0x0111,210,0);edit=editor()
                value=ctypes.c_wchar_p('temporary-model');user.SendMessageW(edit,0x000C,0,ctypes.cast(value,ctypes.c_void_p).value)
                user.SendMessageW(edit,0x00B1,len('temporary-model'),len('temporary-model'))
                user.keybd_event(8,0,0,0);user.keybd_event(8,0,2,0);time.sleep(.12)
                assert caption(edit)=='temporary-mode',f'Backspace did not edit text normally: {caption(edit)!r}'
                assert user.FindWindowExW(table,None,'Edit',None)==edit,'Editing Backspace removed its model row'
                enter(edit,'kept-model');user.PostMessageW(h,0x0111,201,0)
                for _ in range(200):
                    if len(profile()['providers'][0]['models'])==1:break
                    time.sleep(.025)
                else:raise AssertionError('Cell editing did not save the model')
            finally:
                kernel.VirtualFreeEx(target,memory,0,0x8000);kernel.CloseHandle(target)
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
            # Acknowledge the native delete dialog, then check the saved list.
            for _ in range(200):
                dialogs=[]
                def find_dialog(hwnd,param):
                    pid=wintypes.DWORD();user.GetWindowThreadProcessId(hwnd,ctypes.byref(pid))
                    if pid.value==first.pid and hwnd!=h and user.IsWindowVisible(hwnd):dialogs.append(hwnd)
                    return True
                user.EnumWindows(cbtype(find_dialog),0)
                if dialogs:
                    user.PostMessageW(dialogs[0],0x0111,1,0);break
                time.sleep(.025)
            else:raise AssertionError('Delete confirmation did not open')
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
            name_field=user.GetDlgItem(h,105);original_name=caption(name_field)
            draft=ctypes.c_wchar_p('Unsaved provider draft');user.SendMessageW(name_field,0x000C,0,ctypes.cast(draft,ctypes.c_void_p).value)
            second=subprocess.Popen(command);assert second.wait(timeout=12)==0
            assert wait_window(first)==h, 'Second launch did not restore original tray window'
            assert caption(name_field)=='Unsaved provider draft','Relaunch discarded unsaved form edits'
            original=ctypes.c_wchar_p(original_name);user.SendMessageW(name_field,0x000C,0,ctypes.cast(original,ctypes.c_void_p).value)
            assert json.loads(ready.read_text())['pid']==first.pid
            assert json.loads(Path(directory,'instance.json').read_text(encoding='utf-8'))['pid']==core['pid']
            user.PostMessageW(h,0x0111,302,0) # same command as tray Quit
            assert first.wait(timeout=5)==0
            for _ in range(200):
                if not Path(directory,'instance.json').exists():break
                time.sleep(.025)
            else:raise AssertionError('Tray Quit left the core running')
            core=None
            print('PASS: embedded executable/window/header icons; native range/toggle multiselect, batch Delete/Backspace, protected cell editing, automatic display names, language switch and log panel; effort Save; Close/minimize hide to tray; tray click/relaunch restore same UI and core; Quit stops both')
        finally:
            for process in [first,second]+duplicates:
                if process is not None and process.poll() is None:process.terminate();process.wait(timeout=5)
            if core:
                req=urllib.request.Request(f"http://127.0.0.1:{core['port']}/_control/stop",data=b'',headers={'Authorization':'Bearer '+core['management_key']})
                with urllib.request.urlopen(req,timeout=3) as r:r.read()
                for _ in range(200):
                    if not Path(directory,'instance.json').exists():break
                    time.sleep(.025)
if __name__=='__main__':main()

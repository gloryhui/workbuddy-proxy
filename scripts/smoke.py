#!/usr/bin/env python3
"""Executable smoke tests on macOS, Windows and Linux. No real credentials."""
import argparse
import concurrent.futures
import hashlib
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer as HttpServer
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time
import sys
import urllib.request


class ThreadingHTTPServer(HttpServer):
    def handle_error(self,request,address):
        if isinstance(sys.exc_info()[1],(ConnectionResetError,ConnectionAbortedError,BrokenPipeError)):return
        super().handle_error(request,address)

class Mock(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'
    received = []
    lock = threading.Lock()
    def setup(self):
        super().setup()
        self.connection.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    def log_message(self, *args):
        pass
    def do_POST(self):
        body = self.rfile.read(int(self.headers['Content-Length']))
        with self.lock:
            self.received.append((body, {k.lower():v for k,v in self.headers.items()}))
        request = json.loads(body)
        code = 429 if request['model'] == 'quota-test' else 200
        chunk = b'data: {"choices":[{"delta":{"tool_calls":[{"id":"ok"}]}}]}\n\ndata: [DONE]\n\n'
        if request['model'] == 'quota-test':
            chunk = b'{"error":"quota"}'
        self.send_response(code)
        self.send_header('Content-Type', 'text/event-stream' if code == 200 else 'application/json')
        self.send_header('Content-Length', str(len(chunk)))
        if code == 429:
            self.send_header('Retry-After', '5')
        self.end_headers()
        if request['model'] == 'slow-test':
            # Send part now and finish later; the proxy must not buffer the whole SSE.
            self.wfile.write(chunk[:10]); self.wfile.flush(); time.sleep(.5)
            self.wfile.write(chunk[10:]); self.wfile.flush()
        else:
            self.wfile.write(chunk); self.wfile.flush()


def free_port():
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0)); return s.getsockname()[1]


def start(exe, directory, upstream, gui=False):
    port = free_port()
    config = {'api_key': 'fixture-upstream-key', 'local_key': 'fixture-local-key',
              'model': 'fixture-model', 'upstream': upstream, 'port': port, 'installed': None}
    Path(directory, 'config.json').write_text(json.dumps(config))
    command = [str(Path(exe).resolve()), '--config-dir', str(directory)]
    if not gui: command.append('--headless')
    log = open(Path(directory, 'process.log'), 'wb')
    process = subprocess.Popen(command, stdout=log, stderr=log)
    for _ in range(200):
        if process.poll() is not None:
            log.close(); raise AssertionError('Process exited: ' + Path(directory, 'process.log').read_text(encoding='utf-8',errors='replace'))
        try:
            with urllib.request.urlopen(f'http://127.0.0.1:{port}/health', timeout=.5) as r:
                assert json.load(r)['ready']; return process, port, log
        except (OSError, ValueError): time.sleep(.025)
    process.kill(); log.close(); raise AssertionError('Startup timed out')


def request(port, payload, token='fixture-local-key', origin=None):
    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=5)
    headers = {'Authorization': 'Bearer '+token, 'Content-Type': 'application/json','User-Agent':'WorkBuddy-fixture-client'}
    if origin: headers['Origin'] = origin
    connection.request('POST', '/v1/chat/completions', payload, headers)
    return connection, connection.getresponse()


def stop(process, port, directory, log):
    controller = False
    if Path(directory, 'instance.json').exists():
        current=json.loads(Path(directory,'instance.json').read_text(encoding='utf-8'))
        controller=current.get('pid',process.pid)!=process.pid
    if process.poll() is None or controller:
        instance = json.loads(Path(directory, 'instance.json').read_text(encoding='utf-8'))
        req = urllib.request.Request(f'http://127.0.0.1:{port}/_control/stop', data=b'',
                                     headers={'Authorization': 'Bearer '+instance['management_key']})
        try:
            with urllib.request.urlopen(req, timeout=3) as response: response.read()
        finally:
            if controller:
                if os.name=='nt':
                    import ctypes
                    from ctypes import wintypes
                    user=ctypes.WinDLL('user32');cbtype=ctypes.WINFUNCTYPE(wintypes.BOOL,wintypes.HWND,wintypes.LPARAM)
                    user.GetWindowThreadProcessId.argtypes=[wintypes.HWND,ctypes.POINTER(wintypes.DWORD)]
                    user.PostMessageW.argtypes=[wintypes.HWND,wintypes.UINT,wintypes.WPARAM,wintypes.LPARAM]
                    def close(hwnd,param):
                        pid=wintypes.DWORD();user.GetWindowThreadProcessId(hwnd,ctypes.byref(pid))
                        if pid.value==process.pid:user.PostMessageW(hwnd,0x8004,0,0)
                        return True
                    user.EnumWindows(cbtype(close),0)
                else:
                    process.terminate()
            try: process.wait(timeout=5)
            except subprocess.TimeoutExpired: process.kill(); process.wait(); raise
    log.close()
    for _ in range(100):
        if not Path(directory,'instance.json').exists():break
        time.sleep(.025)
    else:raise AssertionError('Core did not release its instance record')
    assert process.returncode == 0 or (controller and os.name!='nt' and process.returncode==-15)


def main():
    parser = argparse.ArgumentParser(); parser.add_argument('--exe', required=True)
    parser.add_argument('--gui', action='store_true'); args = parser.parse_args()
    upstream = ThreadingHTTPServer(('127.0.0.1', 0), Mock)
    thread = threading.Thread(target=upstream.serve_forever, daemon=True); thread.start()
    with tempfile.TemporaryDirectory(prefix='workbuddy-proxy-smoke-') as folder:
        process, port, log = start(args.exe, folder, f'http://127.0.0.1:{upstream.server_port}/v1', args.gui)
        try:
            if args.gui and os.name == 'nt':
                import ctypes
                from ctypes import wintypes
                user = ctypes.WinDLL('user32', use_last_error=True)
                callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL,wintypes.HWND,wintypes.LPARAM)
                user.GetWindowThreadProcessId.argtypes=[wintypes.HWND,ctypes.POINTER(wintypes.DWORD)]
                user.IsWindowVisible.argtypes=[wintypes.HWND]
                def visible_window():
                    found=[]
                    def callback(hwnd,param):
                        pid=wintypes.DWORD();user.GetWindowThreadProcessId(hwnd,ctypes.byref(pid))
                        if pid.value==process.pid and user.IsWindowVisible(hwnd):found.append(hwnd)
                        return True
                    cb=callback_type(callback);user.EnumWindows(cb,0);return bool(found)
                for _ in range(100):
                    if visible_window():break
                    time.sleep(.02)
                else:raise AssertionError('Native Windows settings window did not appear')
            instance=json.loads(Path(folder,'instance.json').read_text(encoding='utf-8'))
            control=lambda obj:urllib.request.Request(f'http://127.0.0.1:{port}/_control/action',data=json.dumps(obj).encode(),headers={'Authorization':'Bearer '+instance['management_key'],'Content-Type':'application/json'})
            original_id=json.loads(Path(folder,'config.json').read_text(encoding='utf-8'))['providers'][0]['id']
            # Byte-for-byte passthrough is an explicit observe-mode contract.
            # Default request-mode usage injection is covered by statistics_test.
            with urllib.request.urlopen(control({'command':'stats_provider_settings_update','provider_id':original_id,'usage_mode':'observe','expected_revision':0}),timeout=5) as response:response.read()
            for model in ['quota-test','slow-test']:
                with urllib.request.urlopen(control({'command':'add'}),timeout=5) as response:response.read()
                route=json.loads(Path(folder,'config.json').read_text(encoding='utf-8'))['providers'][-1]['id']
                with urllib.request.urlopen(control({'command':'save','connection_id':route,'key':'fixture-upstream-key','models':model,'name':model,'upstream':f'http://127.0.0.1:{upstream.server_port}/v1'}),timeout=5) as response:response.read()
            payload = b'{"model":"fixture-model","stream":true,"messages":[{"role":"user","content":"test"}],"tools":[{"type":"function","function":{"name":"report","parameters":{"type":"object"}}}]}'
            conn, res = request(port, payload); received = res.read(); conn.close()
            assert res.status == 200 and b'tool_calls' in received and b'[DONE]' in received
            body, headers = Mock.received[-1]
            assert body == payload, 'Payload was rewritten'
            assert headers['authorization'] == 'Bearer fixture-upstream-key'
            assert headers['user-agent']=='WorkBuddy-fixture-client'
            conn, res = request(port, payload, token='wrong'); assert res.status == 401; res.read(); conn.close()
            conn, res = request(port, payload, origin='https://evil.example'); assert res.status == 403; res.read(); conn.close()
            quota = payload.replace(b'fixture-model', b'quota-test')
            conn, res = request(port, quota); assert res.status == 429 and res.getheader('Retry-After') == '5'; res.read(); conn.close()
            slow = payload.replace(b'fixture-model', b'slow-test')
            start_time = time.perf_counter(); conn, res = request(port, slow)
            assert res.read(10).startswith(b'data:')
            first_byte = time.perf_counter()-start_time
            assert first_byte < .4, 'SSE first bytes were buffered'
            res.read(); conn.close()
            streams=[]
            for _ in range(4):
                connection,response=request(port,slow);assert response.status==200;response.read(10)
                streams.append((connection,response))
            connection,response=request(port,payload);assert response.status==429;response.read();connection.close()
            for connection,response in streams:response.close();connection.close()
            time.sleep(.05)
            connection,response=request(port,payload);assert response.status==200,'Canceled streams did not release request slots';response.read();connection.close()
            huge = json.dumps({'model':'fixture-model','messages':[{'role':'user','content':'x'*(8*1024*1024)}]}).encode()
            conn, res = request(port, huge); assert res.status == 413; res.read(); conn.close()
            # Restarting an already-running instance must activate it, not bind a new port.
            second = subprocess.run([str(Path(args.exe).resolve()), '--headless','--config-dir',folder], capture_output=True, timeout=5)
            assert second.returncode == 0, second.stderr.decode(errors='replace')
            assert process.poll() is None
            instance=json.loads(Path(folder,'instance.json').read_text(encoding='utf-8'))
            control=lambda obj:urllib.request.Request(f'http://127.0.0.1:{port}/_control/action',data=json.dumps(obj).encode(),headers={'Authorization':'Bearer '+instance['management_key'],'Content-Type':'application/json'})
            with urllib.request.urlopen(control({'command':'test','connection_id':original_id}),timeout=5) as response:
                assert 'HTTP 200' in json.load(response)['message']
            with urllib.request.urlopen(control({'command':'copy','connection_id':original_id}),timeout=5) as response:
                message=json.load(response)['message'];assert 'fixture-local-key' in message and 'fixture-upstream-key' not in message
            # The saved setting must affect both actual forwarding and Test.
            for effort in ['low','high','max','none','default']:
                settings={'command':'save','connection_id':original_id,'model':'deepseek-v4.1-flash','upstream':f'http://127.0.0.1:{upstream.server_port}/v1','reasoning_effort':effort}
                with urllib.request.urlopen(control(settings),timeout=5) as response:response.read()
                assert json.loads(Path(folder,'config.json').read_text(encoding='utf-8'))['providers'][0]['models'][0]['reasoning_effort']==effort
                input_body=payload.replace(b'fixture-model',b'deepseek-v4.1-flash')[:-1]+b',"thinking":{"type":"disabled"},"reasoning_effort":"high","extra_field":{"keep":123}}'
                conn,res=request(port,input_body);assert res.status==200;res.read();conn.close()
                outgoing=json.loads(Mock.received[-1][0]);original=json.loads(input_body)
                assert outgoing['messages']==original['messages'] and outgoing['tools']==original['tools'] and outgoing['extra_field']==original['extra_field']
                if effort=='default':assert Mock.received[-1][0]==input_body
                elif effort=='none':assert outgoing['thinking']=={'type':'disabled'} and 'reasoning_effort' not in outgoing
                else:assert outgoing['thinking']=={'type':'enabled'} and outgoing['reasoning_effort']==effort
                with urllib.request.urlopen(control({'command':'test','connection_id':original_id}),timeout=5) as response:assert 'HTTP 200' in json.load(response)['message']
                tested=json.loads(Mock.received[-1][0])
                if effort in ['low','high','max']:assert tested['reasoning_effort']==effort and tested['thinking']['type']=='enabled'
                elif effort=='none':assert tested['thinking']['type']=='disabled' and 'reasoning_effort' not in tested
            print(json.dumps({'result':'PASS' ,'streaming_first_bytes_ms':round(first_byte*1000,3),
                              'checks':['startup','byte-identical tool payload','auth','origin','upstream 429',
                                        'first SSE chunk before completion','bounded concurrent streams','cancellation releases slots',
                                        '8 MiB request cap','single instance','persisted thinking levels affect forwarding and test','shutdown']}))
        finally:
            stop(process, port, folder, log)
    upstream.shutdown(); upstream.server_close()


if __name__ == '__main__': main()

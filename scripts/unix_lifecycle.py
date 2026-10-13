#!/usr/bin/env python3
"""The Mac core must survive settings/launcher process-group termination."""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import urllib.request
from smoke import free_port

def main():
    p=argparse.ArgumentParser();p.add_argument('--exe',required=True);args=p.parse_args()
    exe=str(Path(args.exe).resolve())
    with tempfile.TemporaryDirectory(prefix='workbuddy-unix-lifecycle-') as directory:
        config={'api_key':'fixture-key','local_key':'fixture-local','upstream':'https://opencode.ai/zen/go/v1','model':'deepseek-v4.1-flash','port':free_port(),'installed':None}
        Path(directory,'config.json').write_text(json.dumps(config));command=[exe,'--config-dir',directory]
        first=subprocess.Popen(command,start_new_session=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);second=None;core=None
        try:
            for _ in range(200):
                try:
                    core=json.loads(Path(directory,'instance.json').read_text(encoding='utf-8'))
                    with urllib.request.urlopen(f"http://127.0.0.1:{core['port']}/health",timeout=.5) as r:assert json.load(r)['ready']
                    break
                except (OSError,ValueError):time.sleep(.025)
            assert core and core['pid']!=first.pid
            assert os.getpgid(core['pid'])!=os.getpgid(first.pid),'Core shares the settings/launcher process group'
            os.killpg(os.getpgid(first.pid),signal.SIGTERM);first.wait(timeout=5)
            with urllib.request.urlopen(f"http://127.0.0.1:{core['port']}/health",timeout=3) as r:assert json.load(r)['ready']
            second=subprocess.Popen(command,start_new_session=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            time.sleep(.5);assert second.poll() is None
            assert json.loads(Path(directory,'instance.json').read_text(encoding='utf-8'))['pid']==core['pid']
            print('PASS: launcher/UI process-group termination leaves the same core running; reopened UI attaches')
        finally:
            for process in [first,second]:
                if process and process.poll() is None:process.terminate();process.wait(timeout=5)
            if core:
                req=urllib.request.Request(f"http://127.0.0.1:{core['port']}/_control/stop",data=b'',headers={'Authorization':'Bearer '+core['management_key']})
                with urllib.request.urlopen(req,timeout=3) as r:r.read()
                for _ in range(200):
                    if not Path(directory,'instance.json').exists():break
                    time.sleep(.025)
if __name__=='__main__':main()

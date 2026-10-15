#!/usr/bin/env python3
"""Isolated login registration and quiet-start lifecycle using the actual binary."""
import argparse
import json
import os
from pathlib import Path
import plistlib
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
from smoke import free_port

LABEL = 'io.github.gloryhui.workbuddy-proxy.autostart'
RUN = r'Software\Microsoft\Windows\CurrentVersion\Run'
NAME = 'WorkBuddyProxy'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--exe', required=True)
    args = parser.parse_args()
    exe = Path(args.exe).resolve()
    if os.name == 'nt':
        import winreg
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN) as key:
                winreg.QueryValueEx(key, NAME)
            raise AssertionError('Existing login registration detected; use an isolated Windows user')
        except FileNotFoundError:
            pass
    with tempfile.TemporaryDirectory(prefix='workbuddy-login-') as folder:
        root = Path(folder)
        data = root / '配置 & Space'
        home = root / 'home'
        data.mkdir()
        home.mkdir()
        agent = home / 'Library/LaunchAgents' / (LABEL + '.plist')
        config_file = data / 'config.json'
        # Older configs omit the option and must remain opted out.
        config_file.write_text(json.dumps({'local_key': 'fixture-local', 'providers': [],
                                          'installed': [], 'port': free_port()}))
        env = dict(os.environ, HOME=str(home), USERPROFILE=str(home))
        command = [str(exe), '--config-dir', str(data)]
        host = subprocess.Popen([*command, '--autostart'], env=env,
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        instance = None

        def read(name):
            return json.loads((data / name).read_text(encoding='utf-8'))

        def wait(check, message):
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                try:
                    if check():
                        return
                except (OSError, ValueError):
                    pass
                time.sleep(.05)
            raise AssertionError(message)

        def action(payload, success=True):
            request = urllib.request.Request(
                f'http://127.0.0.1:{instance["port"]}/_control/action',
                data=json.dumps(payload).encode(),
                headers={'Authorization': 'Bearer ' + instance['management_key'],
                         'Content-Type': 'application/json'})
            try:
                with urllib.request.urlopen(request, timeout=5) as response:
                    result = json.load(response)['message']
                assert success, 'Expected failure'
                return result
            except urllib.error.HTTPError as error:
                assert not success, error.read()
                assert error.code == 400
                return None

        def registered():
            if os.name != 'nt':
                return agent.exists()
            try:
                with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN) as key:
                    winreg.QueryValueEx(key, NAME)
                return True
            except FileNotFoundError:
                return False

        try:
            wait(lambda: read('frontend.json')['pid'] == host.pid, 'Quiet tray failed to start')
            instance = read('instance.json')
            assert not json.loads(action({'command': 'snapshot'}))['autostart']
            assert not registered(), 'Startup enabled without opting in'
            time.sleep(.7)
            assert not (data / 'gpui.json').exists(), 'Login startup opened settings'
            subprocess.run([*command, '--autostart'], env=env, check=True, timeout=10)
            assert not (data / 'gpui.json').exists(), 'Duplicate login launch opened settings'
            action({'command': 'autostart', 'enabled': 'true'}, success=False)
            assert not registered()
            action({'command': 'autostart', 'enabled': True})
            assert read('config.json')['autostart'] is True and registered()
            assert json.loads(action({'command': 'snapshot'}))['autostart'] is True
            if os.name != 'nt':
                plist = plistlib.loads(agent.read_bytes())
                assert plist['ProgramArguments'] == [str(exe), '--autostart', '--config-dir', str(data)]
                assert plist['RunAtLoad'] is True and not plist.get('KeepAlive', False)
                # Loading the XML is also a check that spaces, Unicode and & survived.
            else:
                with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN) as key:
                    value, kind = winreg.QueryValueEx(key, NAME)
                assert kind == winreg.REG_SZ
                assert value == f'"{exe}" --autostart --config-dir "{data}"'
            # Explicitly opening the app still opens settings on the same tray/core.
            subprocess.run(command, env=env, check=True, timeout=10)
            wait(lambda: read('gpui.json')['pid'] > 0, 'Manual launch did not open settings')
            assert read('frontend.json')['pid'] == host.pid
            assert read('instance.json')['pid'] == instance['pid']
            action({'command': 'autostart', 'enabled': False})
            action({'command': 'autostart', 'enabled': False})
            assert not read('config.json')['autostart'] and not registered()
            # A failed OS registration must roll the saved setting back to false.
            if os.name != 'nt':
                agents = agent.parent
                agents.rmdir()
                agents.write_text('blocked')
                action({'command': 'autostart', 'enabled': True}, success=False)
                assert not read('config.json')['autostart']
                assert not json.loads(action({'command': 'snapshot'}))['autostart']
                agents.unlink()
            action({'command': 'autostart', 'enabled': True})
            # Quitting/reopening must retain the explicit opt-in without opening settings.
            (data / 'gpui-exit-host').write_text('true')
            host.wait(timeout=10)
            wait(lambda: not (data / 'instance.json').exists(), 'Core did not stop')
            host = subprocess.Popen([*command, '--autostart'], env=env,
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            wait(lambda: read('frontend.json')['pid'] == host.pid, 'Quiet restart failed')
            instance = read('instance.json')
            assert read('config.json')['autostart'] is True and registered()
            time.sleep(.7)
            assert not (data / 'gpui.json').exists(), 'Quiet restart opened settings'
            print('PASS: old-config default off, actual opt-in registration, Unicode/space paths, '
                  'quiet and duplicate login launch, manual wake, repeated disable, '
                  'persistence, registration failure rollback (macOS)', flush=True)
        finally:
            if instance:
                try:
                    action({'command': 'autostart', 'enabled': False})
                except (OSError, AssertionError):
                    pass
            (data / 'gpui-exit-host').write_text('true')
            try:
                host.wait(timeout=8)
            except subprocess.TimeoutExpired:
                host.terminate()
                host.wait(timeout=5)
            if instance and (data / 'instance.json').exists():
                try:
                    request = urllib.request.Request(
                        f'http://127.0.0.1:{instance["port"]}/_control/stop', data=b'',
                        headers={'Authorization': 'Bearer ' + instance['management_key']})
                    with urllib.request.urlopen(request, timeout=3) as response:
                        response.read()
                except OSError:
                    pass
            if os.name == 'nt':
                try:
                    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN, 0, winreg.KEY_SET_VALUE) as key:
                        winreg.DeleteValue(key, NAME)
                except FileNotFoundError:
                    pass


if __name__ == '__main__':
    main()

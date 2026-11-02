#!/usr/bin/env python3
"""Test install to a custom Unicode path, re-install, launch and uninstall in CI."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import socket
import sqlite3
import subprocess
import tempfile
import time
import tomllib
import urllib.request
import zipfile


def wait_for_uninstall_log(path):
    """Inno's detached cleanup process can briefly retain the log handle."""
    deadline = time.monotonic() + 10
    while path.exists():
        try:
            path.unlink()
        except PermissionError:
            if time.monotonic() >= deadline:
                raise
            time.sleep(.1)


def wait_for(predicate, message, seconds=20):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            if value := predicate():
                return value
        except (OSError, ValueError, KeyError):
            pass
        time.sleep(.1)
    raise AssertionError(message)


def stop_fixture(process, directory):
    if process is None:
        return
    (directory / 'gpui-exit-host').write_text('true')
    (directory / 'gpui-stop.json').write_text('true')
    try:
        instance = json.loads((directory / 'instance.json').read_text())
        request = urllib.request.Request(f'http://127.0.0.1:{instance["port"]}/_control/stop',
                                         data=b'', headers={'Authorization': 'Bearer ' + instance['management_key']})
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        with opener.open(request, timeout=5) as response:
            response.read()
    except (OSError, ValueError, KeyError):
        pass
    try:
        process.wait(timeout=15)
    except subprocess.TimeoutExpired:
        process.terminate()
        process.wait(timeout=5)


def main():
    assert platform.system() == 'Windows', 'Windows installation test only'
    parser = argparse.ArgumentParser()
    parser.add_argument('--setup', required=True)
    parser.add_argument('--zip', required=True)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    setup = Path(args.setup).resolve()
    archive = Path(args.zip).resolve()
    version = tomllib.loads(Path('Cargo.toml').read_text())['package']['version']
    output = Path(args.output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='workbuddy-install-qa-') as temp:
        directory = Path(temp)
        install = directory / '自选目录 with spaces' / 'WorkBuddy Proxy'
        data = directory / 'private-fixture'
        # Do not overwrite a real installation registration on a developer machine.
        import winreg
        startup_key = r'Software\Microsoft\Windows\CurrentVersion\Run'
        def assert_startup_absent():
            try:
                with winreg.OpenKey(winreg.HKEY_CURRENT_USER, startup_key) as key:
                    winreg.QueryValueEx(key, 'WorkBuddyProxy')
                raise AssertionError('Unexpected login startup registration')
            except FileNotFoundError:
                pass
        assert_startup_absent()
        uninstall_key = r'Software\Microsoft\Windows\CurrentVersion\Uninstall\WorkBuddyProxy_is1'
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, uninstall_key):
                raise AssertionError('Existing installation detected; use isolated CI')
        except FileNotFoundError:
            pass
        installed = False
        active_host = None
        other_core = None
        other_data = directory / 'other-fixture'
        try:
            for attempt in range(2):
                subprocess.run([str(setup), '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART',
                                '/SP-', f'/DIR={install}',
                                f'/LOG={directory / (str(attempt) + "-install.log")}'], check=True, timeout=90)
                installed = True
                assert_startup_absent()
                log_bytes = (directory / (str(attempt) + '-install.log')).read_bytes()
                log_text = log_bytes.decode('utf-16' if log_bytes.startswith(b'\xff\xfe') else 'utf-8-sig')
                assert 'Installer language: chinesesimplified' in log_text
                assert 'Desktop task label: 创建桌面快捷方式' in log_text
                assert 'Desktop shortcut selected: yes' in log_text
                assert (install / 'workbuddy-proxy.exe').is_file()
                assert (install / 'internal/workbuddy-settings.exe').is_file()
                assert not (install / 'workbuddy-proxy-gpui.exe').exists()
            # Verify installed binary bytes are exactly those tested in the portable package.
            with zipfile.ZipFile(archive) as z:
                for tail in ('workbuddy-proxy.exe', 'internal/workbuddy-settings.exe'):
                    entries = [n for n in z.namelist() if n.endswith('/' + tail)]
                    assert len(entries) == 1
                    assert hashlib.sha256(z.read(entries[0])).digest() == hashlib.sha256((install / tail).read_bytes()).digest()
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, uninstall_key) as key:
                assert Path(winreg.QueryValueEx(key, 'InstallLocation')[0]).resolve() == install.resolve()
                assert winreg.QueryValueEx(key, 'DisplayName')[0] == 'WorkBuddy Proxy'
                assert winreg.QueryValueEx(key, 'DisplayVersion')[0] == version
            for binary in ('workbuddy-proxy.exe', 'internal/workbuddy-settings.exe'):
                assert subprocess.check_output([str(install / binary), '--version'], text=True).strip() == version
            # COM reads shortcut metadata; it does not send GUI input.
            env = os.environ.copy()
            env['WBP_INSTALL_QA'] = str(install)
            ps = '''$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$shell = New-Object -ComObject Shell.Application
$paths = @(
 (Join-Path ([Environment]::GetFolderPath('Programs')) 'WorkBuddy Proxy\\WorkBuddy Proxy.lnk'),
 (Join-Path ([Environment]::GetFolderPath('Desktop')) 'WorkBuddy Proxy.lnk')
)
$targets = foreach ($p in $paths) {
 if (!(Test-Path $p)) { throw "Missing shortcut: $p" }
 # WScript.Shell can return ANSI paths (???? on English Windows).
 # Shell.Application exposes the Unicode ShellLinkObject metadata.
 $s = $shell.NameSpace((Split-Path $p)).ParseName((Split-Path $p -Leaf)).GetLink
 [pscustomobject]@{ shortcut = $p; target = $s.Path; workingDirectory = $s.WorkingDirectory; arguments = $s.Arguments }
}
$targets | ConvertTo-Json -Compress
'''
            shortcuts = subprocess.run(['powershell', '-NoProfile', '-Command', ps], env=env,
                                       check=True, timeout=20, capture_output=True, encoding='utf-8')
            for shortcut in json.loads(shortcuts.stdout):
                # Windows may expand RUNNER~1 into runneradmin in .lnk metadata.
                # Compare actual file identity, not equivalent long/short path strings.
                assert os.path.samefile(shortcut['target'], install / 'workbuddy-proxy.exe'), shortcut
                assert os.path.samefile(shortcut['workingDirectory'], install), shortcut
                assert not shortcut['arguments'], shortcut
            lifecycle = directory / 'lifecycle.json'
            subprocess.run(['python', 'scripts/gpui_package_test.py', '--exe', str(install / 'workbuddy-proxy.exe'),
                            '--directory', str(data), '--output', str(lifecycle)], check=True, timeout=120)
            assert json.loads(lifecycle.read_text())['result'] == 'passed'
            original_config = (data / 'config.json').read_bytes()
            # The old test stopped every process before uninstall, missing the
            # actual locked-main/helper failure. Leave this installation live.
            env = dict(os.environ, WORKBUDDY_PROXY_CONFIG_DIR=str(data),
                       WORKBUDDY_PROXY_CATALOG_TEST_URL='http://127.0.0.1:1/catalog')
            active_host = subprocess.Popen([str(install / 'workbuddy-proxy.exe'),
                                           '--config-dir', str(data)], env=env)
            core = wait_for(lambda: json.loads((data / 'instance.json').read_text()),
                            'Installed core did not restart')
            ui = wait_for(lambda: json.loads((data / 'gpui.json').read_text()),
                          'Installed helper did not restart')
            wait_for(lambda: json.loads((data / 'frontend.json').read_text())['pid'] == active_host.pid,
                     'Installed tray did not restart')
            assert active_host.poll() is None
            from gpui_package_test import running
            assert running(core['pid']) and running(ui['pid'])
            # A same-named program in another directory must remain running.
            other_install = directory / 'other-install'
            other_install.mkdir()
            other_exe = other_install / 'workbuddy-proxy.exe'
            shutil.copy2(install / 'workbuddy-proxy.exe', other_exe)
            other_data.mkdir()
            other_config = json.loads(original_config)
            with socket.socket() as probe:
                probe.bind(('127.0.0.1', 0))
                other_config['port'] = probe.getsockname()[1]
            (other_data / 'config.json').write_text(json.dumps(other_config))
            other_core = subprocess.Popen([str(other_exe), '--headless', '--config-dir', str(other_data)],
                                          env=dict(env, WORKBUDDY_PROXY_CONFIG_DIR=str(other_data)))
            wait_for(lambda: json.loads((other_data / 'instance.json').read_text()),
                     'Unrelated same-named core did not start')
            # Untracked user content inside the selected install root also stays.
            sentinel = install / 'user-note.txt'
            sentinel.write_bytes(b'keep user content')
            # Simulate an explicit in-app opt-in before uninstall.
            with winreg.CreateKey(winreg.HKEY_CURRENT_USER, startup_key) as key:
                winreg.SetValueEx(key, 'WorkBuddyProxy', 0, winreg.REG_SZ,
                                 f'"{install / "workbuddy-proxy.exe"}" --autostart')
            subprocess.run([str(install / 'unins000.exe'), '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART',
                            f'/LOG={directory / "uninstall.log"}'], check=True, timeout=90)
            wait_for_uninstall_log(directory / 'uninstall.log')
            installed = False
            active_host.wait(timeout=15)
            assert active_host.returncode == 0
            wait_for(lambda: not running(core['pid']) and not running(ui['pid']),
                     'Uninstall left installed core/helper running')
            assert not (install / 'workbuddy-proxy.exe').exists()
            assert not (install / 'internal/workbuddy-settings.exe').exists()
            assert not (install / 'internal').exists()
            assert sentinel.read_bytes() == b'keep user content'
            assert other_core.poll() is None
            with sqlite3.connect(data / 'usage.sqlite3') as connection:
                assert all(reason == 'normal' for reason, in
                           connection.execute('SELECT stop_reason FROM stats_runs'))
            assert (data / 'config.json').read_bytes() == original_config
            assert_startup_absent()
            try:
                with winreg.OpenKey(winreg.HKEY_CURRENT_USER, uninstall_key):
                    raise AssertionError('Uninstall registration remains')
            except FileNotFoundError:
                pass
            result = json.loads(lifecycle.read_text())
            result['installer_checks'] = ['custom Unicode/space installation directory', 'per-user HKCU uninstall registration',
                                          'Simplified Chinese installer language and task text',
                                          'desktop shortcut created by default without TASKS override',
                                          'only one root entry point; renamed internal helper', 'repeat install to same directory',
                                          'main-only Start Menu and desktop shortcuts', 'installed bytes match portable package',
                                          'installed app real relay/settings lifecycle',
                                          'uninstall while tray/core/helper are running closes them and removes main/internal',
                                          'uninstall preserves unrelated same-named core and user content',
                                          'uninstall removes binaries/registration and preserves external config']
            output.write_text(json.dumps(result, indent=2) + '\n')
            print('PASS: selectable-directory Windows installer, shortcuts, lifecycle, reinstall and uninstall')
        finally:
            try:
                stop_fixture(active_host, data)
            finally:
                try:
                    stop_fixture(other_core, other_data)
                finally:
                    if installed and (install / 'unins000.exe').exists():
                        subprocess.run([str(install / 'unins000.exe'), '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART'], timeout=90)


if __name__ == '__main__':
    main()

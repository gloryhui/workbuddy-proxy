#!/usr/bin/env python3
"""Test install to a custom Unicode path, re-install, launch and uninstall in CI."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile
import time
import tomllib
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
            # Simulate an explicit in-app opt-in before uninstall.
            with winreg.CreateKey(winreg.HKEY_CURRENT_USER, startup_key) as key:
                winreg.SetValueEx(key, 'WorkBuddyProxy', 0, winreg.REG_SZ,
                                 f'"{install / "workbuddy-proxy.exe"}" --autostart')
            subprocess.run([str(install / 'unins000.exe'), '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART',
                            f'/LOG={directory / "uninstall.log"}'], check=True, timeout=90)
            wait_for_uninstall_log(directory / 'uninstall.log')
            installed = False
            assert not (install / 'workbuddy-proxy.exe').exists()
            assert not (install / 'internal/workbuddy-settings.exe').exists()
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
                                          'installed app real relay/settings lifecycle', 'uninstall removes binaries/registration and preserves external config']
            output.write_text(json.dumps(result, indent=2) + '\n')
            print('PASS: selectable-directory Windows installer, shortcuts, lifecycle, reinstall and uninstall')
        finally:
            if installed and (install / 'unins000.exe').exists():
                subprocess.run([str(install / 'unins000.exe'), '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART'], timeout=90)


if __name__ == '__main__':
    main()

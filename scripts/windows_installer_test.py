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
import zipfile


def main():
    assert platform.system() == 'Windows', 'Windows installation test only'
    parser = argparse.ArgumentParser()
    parser.add_argument('--setup', required=True)
    parser.add_argument('--zip', required=True)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    setup = Path(args.setup).resolve()
    archive = Path(args.zip).resolve()
    output = Path(args.output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='workbuddy-install-qa-') as temp:
        directory = Path(temp)
        install = directory / '自选目录 with spaces' / 'WorkBuddy Proxy'
        data = directory / 'private-fixture'
        # Do not overwrite a real installation registration on a developer machine.
        import winreg
        uninstall_key = r'Software\Microsoft\Windows\CurrentVersion\Uninstall\WorkBuddyProxy-GPUI-Preview_is1'
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, uninstall_key):
                raise AssertionError('Existing preview installation detected; use isolated CI')
        except FileNotFoundError:
            pass
        installed = False
        try:
            for attempt in range(2):
                subprocess.run([str(setup), '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART',
                                '/SP-', f'/DIR={install}', '/TASKS=desktopicon',
                                f'/LOG={directory / (str(attempt) + "-install.log")}'], check=True, timeout=90)
                installed = True
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
            # COM reads shortcut metadata; it does not send GUI input.
            env = os.environ.copy()
            env['WBP_INSTALL_QA'] = str(install)
            ps = '''$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$w = New-Object -ComObject WScript.Shell
$paths = @(
 (Join-Path ([Environment]::GetFolderPath('Programs')) 'WorkBuddy Proxy Preview\\WorkBuddy Proxy Preview.lnk'),
 (Join-Path ([Environment]::GetFolderPath('Desktop')) 'WorkBuddy Proxy Preview.lnk')
)
$targets = foreach ($p in $paths) {
 if (!(Test-Path $p)) { throw "Missing shortcut: $p" }
 $s = $w.CreateShortcut($p)
 [pscustomobject]@{ shortcut = $p; target = $s.TargetPath; workingDirectory = $s.WorkingDirectory; arguments = $s.Arguments }
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
            subprocess.run([str(install / 'unins000.exe'), '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART',
                            f'/LOG={directory / "uninstall.log"}'], check=True, timeout=90)
            installed = False
            assert not (install / 'workbuddy-proxy.exe').exists()
            assert not (install / 'internal/workbuddy-settings.exe').exists()
            assert (data / 'config.json').read_bytes() == original_config
            try:
                with winreg.OpenKey(winreg.HKEY_CURRENT_USER, uninstall_key):
                    raise AssertionError('Uninstall registration remains')
            except FileNotFoundError:
                pass
            result = json.loads(lifecycle.read_text())
            result['installer_checks'] = ['custom Unicode/space installation directory', 'per-user HKCU uninstall registration',
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

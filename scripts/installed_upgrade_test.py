#!/usr/bin/env python3
"""Install real legacy packages, replace them in place, then verify legacy data.

Windows registry/shortcut operations and the unattended GUI lifecycle are only
allowed on disposable GitHub-hosted runners. A local Mac run must explicitly
select --local-headless-only; it still copies/replaces real app bundles.
--directory must not exist. It belongs to the caller and is retained for audit
and exact cleanup after every child exits. Reports exclude credentials/configs.
"""
import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import plistlib
import shutil
import signal
import sqlite3
import stat
import struct
import subprocess
import sys
import time
from types import SimpleNamespace
import urllib.error
import urllib.parse
import urllib.request
import zipfile

import release_upgrade_test as upgrade
from gpui_package_test import running


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def clean_environment(environment=None):
    result = dict(os.environ if environment is None else environment)
    for key in ('GH_TOKEN', 'GITHUB_TOKEN', 'ACTIONS_RUNTIME_TOKEN',
                'ACTIONS_ID_TOKEN_REQUEST_TOKEN'):
        result.pop(key, None)
    return result


def api_json(url):
    assert urllib.parse.urlsplit(url).hostname == 'api.github.com'
    headers = {'Accept': 'application/vnd.github+json', 'User-Agent': 'WorkBuddy-upgrade-QA'}
    token = os.environ.get('GH_TOKEN') or os.environ.get('GITHUB_TOKEN')
    if token:
        headers['Authorization'] = 'Bearer ' + token
    with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=45) as response:
        return json.load(response)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, file, code, message, headers, new_url):
        return None


def download(url, path, expected, authenticated=False):
    assert not path.exists()
    request = urllib.request.Request(url, headers={'User-Agent': 'WorkBuddy-upgrade-QA'})
    if authenticated:
        assert urllib.parse.urlsplit(url).hostname == 'api.github.com'
        token = os.environ.get('GH_TOKEN') or os.environ.get('GITHUB_TOKEN')
        assert token, 'Artifact download needs the runner read-only token'
        request.add_header('Authorization', 'Bearer ' + token)
    try:
        response = urllib.request.build_opener(NoRedirect).open(request, timeout=45)
    except urllib.error.HTTPError as error:
        if error.code not in (301, 302, 303, 307, 308):
            raise
        redirect = error.headers['Location']
        assert urllib.parse.urlsplit(redirect).scheme == 'https'
        # Never forward the GitHub token to the artifact storage redirect.
        response = urllib.request.urlopen(redirect, timeout=45)
    with response, path.open('wb') as output:
        shutil.copyfileobj(response, output)
    assert path.stat().st_size == expected['bytes'], 'Download size mismatch'
    assert sha(path) == expected['sha256'], 'Download SHA256 mismatch'


def architecture(exe, target):
    blob = exe.read_bytes()
    if target == 'windows-x64':
        assert blob[:2] == b'MZ'
        offset = struct.unpack_from('<I', blob, 60)[0]
        assert blob[offset:offset + 4] == b'PE\0\0'
        assert struct.unpack_from('<H', blob, offset + 4)[0] == 0x8664
    else:
        assert struct.unpack_from('<I', blob)[0] == 0xfeedfacf
        expected = 0x100000c if target == 'macos-arm64' else 0x1000007
        assert struct.unpack_from('<I', blob, 4)[0] == expected


def tree_identity(root):
    files = {}
    for base, directories, names in os.walk(root, followlinks=False):
        for name in directories + names:
            path = Path(base) / name
            if path.is_symlink():
                assert path.resolve().is_relative_to(root.resolve())
                files[str(path.relative_to(root))] = {'link': os.readlink(path)}
            elif path.is_file():
                files[str(path.relative_to(root))] = {'sha256': sha(path)}
    return files


@contextmanager
def mounted_app(dmg, mount):
    mount.mkdir()
    subprocess.run(['hdiutil', 'attach', '-readonly', '-nobrowse', '-mountpoint',
                    str(mount), str(dmg)], check=True, stdout=subprocess.DEVNULL)
    try:
        apps = list(mount.glob('*.app'))
        assert len(apps) == 1
        assert (mount / 'Applications').is_symlink()
        assert os.readlink(mount / 'Applications') == '/Applications'
        yield apps[0]
    finally:
        subprocess.run(['hdiutil', 'detach', str(mount)], check=True,
                       stdout=subprocess.DEVNULL)


def mac_metadata(app, version, target):
    core = app / 'Contents/MacOS/workbuddy-proxy'
    helper = app / 'Contents/Helpers/WorkBuddy Proxy Settings.app'
    ui = helper / 'Contents/MacOS/workbuddy-proxy-gpui'
    for bundle in (app, helper):
        subprocess.run(['codesign', '--verify', '--strict', str(bundle)], check=True)
        metadata = plistlib.loads((bundle / 'Contents/Info.plist').read_bytes())
        assert metadata['CFBundleVersion'] == version
    for exe in (core, ui):
        architecture(exe, target)
        assert subprocess.check_output([str(exe), '--version'], env=clean_environment(),
                                       text=True).strip() == version
    return {'core_sha256': sha(core), 'ui_sha256': sha(ui),
            'strict_signatures': True, 'version': version, 'architecture': target}


def installed_lifecycle(exe, directory, environment, policy, report):
    """Use real installed host/Core/UI, without sending keyboard/mouse input."""
    assert os.environ.get('GITHUB_ACTIONS') == 'true', 'Native lifecycle is runner-only'
    command = [str(exe), '--config-dir', str(directory)]
    # The host must use LaunchServices normally. CI has no user's installation;
    # the CLI data cases separately attest the Core's Darwin sandbox policy.
    with (directory / 'installed-host.log').open('wb') as log:
        host = subprocess.Popen(command, env=clean_environment(environment), stdout=log,
                                stderr=subprocess.STDOUT)
        core = None
        ui_pids = []
        primary_error = None
        report['installed_lifecycle'] = {'host_pid': host.pid, 'core_pid': None,
                                         'ui_pids': ui_pids, 'input_sent': False}

        def read(name):
            return json.loads((directory / name).read_text(encoding='utf-8'))

        def wait(predicate, message, seconds=35):
            def observe():
                assert host.poll() is None, 'Installed host exited prematurely'
                try:
                    return predicate()
                except (OSError, ValueError, KeyError):
                    return None
            return upgrade.wait_for(observe, message, seconds)

        try:
            core = wait(lambda: read('instance.json'), 'Installed Core not ready')
            report['installed_lifecycle']['core_pid'] = core['pid']
            ui = wait(lambda: read('gpui.json'), 'Installed GPUI not ready')
            ui_pids.append(ui['pid'])
            assert len({host.pid, core['pid'], ui['pid']}) == 3
            assert wait(lambda: read('frontend.json')['pid'] == host.pid,
                        'Installed tray not published')
            # PID metadata alone is insufficient: both OS processes must exist.
            assert running(core['pid']) and running(ui['pid'])
            time.sleep(.5)
            (directory / 'gpui-stop.json').write_text('true')
            wait(lambda: not (directory / 'gpui.json').exists() and not running(ui['pid']),
                 'Closing installed settings did not release UI', 10)
            assert read('instance.json')['pid'] == core['pid'] and running(core['pid'])
            request = urllib.request.Request(
                f'http://127.0.0.1:{core["port"]}/_control/show', data=b'',
                headers={'Authorization': 'Bearer ' + core['management_key']})
            with upgrade.OPENER.open(request, timeout=5) as response:
                response.read()
            reopened = wait(lambda: read('gpui.json'), 'Installed settings did not reopen')
            ui_pids.append(reopened['pid'])
            assert reopened['pid'] != ui['pid'] and running(reopened['pid'])
            assert read('instance.json')['pid'] == core['pid']
            report['installed_lifecycle'] = {'host_pid': host.pid, 'core_pid': core['pid'],
                                             'ui_pids': ui_pids,
                                             'settings_close_kept_same_core': True,
                                             'settings_reopened_new_pid': True,
                                             'input_sent': False}
        except BaseException as error:
            primary_error = error
            raise
        finally:
            (directory / 'gpui-exit-host').write_text('true')
            try:
                host.wait(timeout=12)
            except subprocess.TimeoutExpired:
                host.terminate()
                host.wait(timeout=5)
            if core:
                if running(core['pid']):
                    request = urllib.request.Request(
                        f'http://127.0.0.1:{core["port"]}/_control/stop', data=b'',
                        headers={'Authorization': 'Bearer ' + core['management_key']})
                    try:
                        with upgrade.OPENER.open(request, timeout=3) as response:
                            response.read()
                    except OSError:
                        # A core already shutting down can close its HTTP listener.
                        # The process wait below still must prove actual termination.
                        pass
                upgrade.wait_for(lambda: not running(core['pid']),
                                 'Installed Core remains after quit', 10)
            for pid in ui_pids:
                upgrade.wait_for(lambda: not running(pid), 'Installed UI remains after quit', 10)
            if primary_error is None:
                assert host.returncode == 0, host.returncode
                assert not (directory / 'instance.json').exists()
                with sqlite3.connect(directory / 'usage.sqlite3') as db:
                    runs = db.execute('SELECT stopped_at_ms,stop_reason FROM stats_runs').fetchall()
                assert len(runs) == 3 and all(x is not None and reason == 'normal'
                                             for x, reason in runs), runs
                report['installed_lifecycle'].update(host_exit_code=host.returncode,
                                                     all_pids_gone=True,
                                                     three_runs_stopped_normally=True)


def windows_locations():
    script = """[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)
@{programs=[Environment]::GetFolderPath('Programs');desktop=[Environment]::GetFolderPath('Desktop')} | ConvertTo-Json -Compress
"""
    return json.loads(subprocess.check_output(['powershell', '-NoProfile', '-Command', script],
                                              text=True, encoding='utf-8'))


UNINSTALL = r'Software\Microsoft\Windows\CurrentVersion\Uninstall\WorkBuddyProxy_is1'
STARTUP = r'Software\Microsoft\Windows\CurrentVersion\Run'


def registration():
    import winreg
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, UNINSTALL) as key:
            return {name: winreg.QueryValueEx(key, name)[0]
                    for name in ('InstallLocation', 'DisplayName', 'DisplayVersion')}
    except FileNotFoundError:
        return None


def assert_no_startup():
    import winreg
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, STARTUP) as key:
            winreg.QueryValueEx(key, 'WorkBuddyProxy')
    except FileNotFoundError:
        return
    raise AssertionError('Unexpected WorkBuddyProxy login startup registration')


def windows_process_references(root):
    environment = clean_environment()
    environment['WBP_QA_ROOT'] = str(root)
    environment['WBP_QA_EXCLUDED_PIDS'] = f'{os.getpid()},{os.getppid()}'
    script = """[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)
$exclude=@($env:WBP_QA_EXCLUDED_PIDS.Split(',') | ForEach-Object {[int]$_}) + @($PID)
@(Get-CimInstance Win32_Process | Where-Object {$_.CommandLine -and $_.CommandLine.Contains($env:WBP_QA_ROOT) -and $_.ProcessId -notin $exclude} | Select-Object ProcessId,Name) | ConvertTo-Json -Compress
"""
    text = subprocess.check_output(['powershell', '-NoProfile', '-Command', script],
                                   env=environment, text=True, encoding='utf-8').strip()
    return json.loads(text) if text else []


def shortcut_targets(locations, install):
    script = """$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)
$shell=New-Object -ComObject Shell.Application
$paths=@((Join-Path ([Environment]::GetFolderPath('Programs')) 'WorkBuddy Proxy\\WorkBuddy Proxy.lnk'),(Join-Path ([Environment]::GetFolderPath('Desktop')) 'WorkBuddy Proxy.lnk'))
$result=foreach($p in $paths){$link=$shell.NameSpace((Split-Path $p)).ParseName((Split-Path $p -Leaf)).GetLink;[pscustomobject]@{target=$link.Path;directory=$link.WorkingDirectory;arguments=$link.Arguments}}
$result | ConvertTo-Json -Compress
"""
    targets = json.loads(subprocess.check_output(['powershell', '-NoProfile', '-Command', script],
                                                 encoding='utf-8', text=True))
    assert len(targets) == 2
    for target in targets:
        assert os.path.samefile(target['target'], install / 'workbuddy-proxy.exe')
        assert os.path.samefile(target['directory'], install) and not target['arguments']
    return {'main_only_shortcuts': 2, 'unicode_target_and_working_directory_verified': True}


def windows_case(root, old_setup, candidate_setup, candidate_zip, version, schema, report):
    assert os.environ.get('GITHUB_ACTIONS') == 'true', 'Installer test only on disposable CI'
    assert registration() is None, 'Existing installation: refuse to overwrite'
    assert_no_startup()
    locations = windows_locations()
    assert not (Path(locations['programs']) / 'WorkBuddy Proxy').exists()
    assert not (Path(locations['desktop']) / 'WorkBuddy Proxy.lnk').exists()
    install = root / '自选路径 with spaces' / 'WorkBuddy Proxy'
    directory = root / 'private-data'
    directory.mkdir()
    exe = install / 'workbuddy-proxy.exe'
    expected = {}
    with zipfile.ZipFile(candidate_zip) as archive:
        core_names = [n for n in archive.namelist() if n.endswith('/workbuddy-proxy.exe')]
        assert len(core_names) == 1
        base = str(PurePosixPath(core_names[0]).parent) + '/'
        for name in archive.namelist():
            if name.startswith(base) and not name.endswith('/'):
                relative = name[len(base):]
                assert '..' not in PurePosixPath(relative).parts
                expected[relative] = hashlib.sha256(archive.read(name)).hexdigest()
    assert 'internal/workbuddy-settings.exe' in expected

    def install_setup(setup, label):
        subprocess.run([str(setup), '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART',
                        '/SP-', f'/DIR={install}', f'/LOG={root / (label + "-install.log")}'],
                       env=clean_environment(), check=True, timeout=90)
        entry = registration()
        assert entry and Path(entry['InstallLocation']).resolve() == install.resolve()
        assert entry['DisplayName'] == 'WorkBuddy Proxy'
        assert_no_startup()
        shortcut_targets(locations, install)
        return entry

    def replace(directory, result):
        assert len(upgrade.snapshot(directory)['tables']['usage_requests']['rows']) == 12
        entry = install_setup(candidate_setup, 'candidate')
        assert entry['DisplayVersion'] == '0.5.7'
        for name, checksum in expected.items():
            assert sha(install / name) == checksum, ('Installed package differs', name)
        assert not (install / 'workbuddy-proxy-gpui.exe').exists()
        for path in (exe, install / 'internal/workbuddy-settings.exe'):
            architecture(path, 'windows-x64')
            assert subprocess.check_output([str(path), '--version'], env=clean_environment(),
                                           text=True).strip() == '0.5.7'
        result['replacement'] = {'method': 'Real old Inno setup → candidate Inno setup same directory',
                                  'legacy_12_rows_existed_before_setup': True,
                                  'candidate_all_portable_files_match': len(expected),
                                  'same_registration_and_shortcuts': True}

    attempted = False
    try:
        attempted = True
        entry = install_setup(old_setup, 'released')
        assert entry['DisplayVersion'] == version
        architecture(exe, 'windows-x64')
        report['old_exe_sha256'] = sha(exe)
        args = SimpleNamespace(directory=directory, old_exe=exe, candidate_exe=exe,
                               old_version=version, old_schema=schema, candidate_version='0.5.7')
        upgrade.run(args, report, replace, installed_lifecycle)
    finally:
        uninstaller = install / 'unins000.exe'
        if attempted and uninstaller.is_file():
            entry = registration()
            assert entry and Path(entry['InstallLocation']).resolve() == install.resolve()
            config_before = (directory / 'config.json').read_bytes() if (directory / 'config.json').exists() else None
            data_before = upgrade.snapshot(directory) if (directory / 'usage.sqlite3').exists() else None
            subprocess.run([str(uninstaller), '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART'],
                           env=clean_environment(), check=True, timeout=90)
            upgrade.wait_for(lambda: not uninstaller.exists(), 'Detached uninstaller not finished', 15)
            assert registration() is None
            assert_no_startup()
            assert not exe.exists() and not (install / 'internal/workbuddy-settings.exe').exists()
            assert not (Path(locations['desktop']) / 'WorkBuddy Proxy.lnk').exists()
            assert not (Path(locations['programs']) / 'WorkBuddy Proxy/WorkBuddy Proxy.lnk').exists()
            if config_before is not None:
                assert (directory / 'config.json').read_bytes() == config_before
            if data_before is not None:
                assert data_before == upgrade.snapshot(directory)
            report['uninstall'] = {'removed_files_registration_and_desktop_shortcut': True,
                                    'external_config_bytes_and_all_business_fields_preserved': True}


def mac_case(root, old_dmg, candidate_dmg, version, schema, target, report, local):
    install = root / '自选路径 with spaces' / 'WorkBuddy Proxy.app'
    install.parent.mkdir()
    directory = root / 'private-data'
    directory.mkdir()
    with mounted_app(old_dmg, root / 'old-volume') as source:
        subprocess.run(['ditto', str(source), str(install)], check=True)
    report['old_package'] = mac_metadata(install, version, target)
    report['old_exe_sha256'] = sha(install / 'Contents/MacOS/workbuddy-proxy')
    candidate_app = root / 'candidate-staged.app'
    with mounted_app(candidate_dmg, root / 'candidate-volume') as source:
        subprocess.run(['ditto', str(source), str(candidate_app)], check=True)
    report['candidate_package'] = mac_metadata(candidate_app, '0.5.7', target)
    candidate_tree = tree_identity(candidate_app)

    def replace(directory, result):
        assert len(upgrade.snapshot(directory)['tables']['usage_requests']['rows']) == 12
        previous = root / 'previous-app-retained.app'
        assert not previous.exists() and install.resolve().is_relative_to(root.resolve())
        install.rename(previous)
        subprocess.run(['ditto', str(candidate_app), str(install)], check=True)
        assert tree_identity(install) == candidate_tree
        mac_metadata(install, '0.5.7', target)
        result['replacement'] = {'method': 'Actual DMG app bundle replaced at identical install path',
                                  'previous_app_renamed_as_owned_backup': True,
                                  'legacy_12_rows_existed_before_replacement': True,
                                  'all_candidate_resources_and_links_equal': True,
                                  'Finder_drag_not_performed': True}

    exe = install / 'Contents/MacOS/workbuddy-proxy'
    args = SimpleNamespace(directory=directory, old_exe=exe, candidate_exe=exe,
                           old_version=version, old_schema=schema, candidate_version='0.5.7')
    upgrade.run(args, report, replace, None if local else installed_lifecycle)
    if local:
        report['installed_lifecycle'] = 'Not run locally: delegated computer use is separate'
    # Removing a copied .app is the Mac uninstall operation. Verify that only
    # this known package bundle is removed while its external profile survives.
    config_before = (directory / 'config.json').read_bytes()
    data_before = upgrade.snapshot(directory)
    assert install.resolve().is_relative_to(root.resolve()) and not install.is_symlink()
    assert not os.path.ismount(install) and tree_identity(install) == candidate_tree
    for path in [install, *install.parents]:
        if path == root:
            break
        assert not path.is_symlink() and path.resolve().is_relative_to(root.resolve())
    for path in install.rglob('*'):
        assert path.lstat().st_uid == os.getuid()
        assert path.resolve().is_relative_to(install.resolve()) and not os.path.ismount(path)
    handles = subprocess.run(['/usr/sbin/lsof', '-nP', '+D', str(install)],
                             capture_output=True, text=True)
    assert handles.returncode == 1 and not handles.stdout.strip(), 'Installed bundle is in use'
    print('UNINSTALL_PLAN:', json.dumps({'target': str(install), 'type': 'owned copied app bundle',
                                        'project': str(Path.cwd()),
                                        'regenerable': 'Verified candidate DMG; profile is outside app'}), flush=True)
    shutil.rmtree(install)
    assert not install.exists() and (directory / 'config.json').read_bytes() == config_before
    assert upgrade.snapshot(directory) == data_before
    report['uninstall'] = {'method': 'Removed only the verified installed app bundle',
                            'installed_path_absent': True,
                            'external_config_bytes_and_all_business_fields_preserved': True,
                            'Finder_Trash_interaction_not_performed': True}


def cleanup_fixture(root, output, target):
    assert os.environ.get('GITHUB_ACTIONS') == 'true', 'Local cleanup requires caller review'
    output.mkdir(parents=True, exist_ok=True)
    if not root.exists():
        (output / 'cleanup.json').write_text(json.dumps({'fixture_never_created': True}) + '\n')
        return
    runner_temp = Path(os.environ['RUNNER_TEMP']).resolve()
    assert root != runner_temp and root.is_relative_to(runner_temp)
    assert root not in (Path.home().resolve(), Path.cwd().resolve(), Path(root.anchor))
    assert not root.is_symlink() and not os.path.ismount(root)
    ownership = json.loads((root / 'ownership.json').read_text())
    assert ownership['root'] == str(root) and ownership['created_by_this_script'] is True
    assert ownership['run_id'] == os.environ['GITHUB_RUN_ID']
    assert ownership['job'] == os.environ['GITHUB_JOB'] and ownership['platform'] == target
    assert ownership['script_sha256'] == sha(Path(__file__))
    for path in output.glob('v*.json'):
        lifecycle = json.loads(path.read_text()).get('installed_lifecycle')
        if isinstance(lifecycle, dict):
            pids = [lifecycle.get('host_pid'), lifecycle.get('core_pid'),
                    *lifecycle.get('ui_pids', [])]
            assert all(not running(pid) for pid in pids if pid), 'Installed process remains'
    for path in root.rglob('instance.json'):
        assert not running(json.loads(path.read_text())['pid']), 'Core still owns fixture'
    for path in root.rglob('gpui.json'):
        assert not running(json.loads(path.read_text())['pid']), 'UI still owns fixture'
    if sys.platform == 'darwin':
        handles = subprocess.run(['/usr/sbin/lsof', '-nP', '+D', str(root)],
                                 capture_output=True, text=True)
        assert handles.returncode == 1 and not handles.stdout.strip(), 'Fixture files in use'
        ancestors = {os.getpid()}
        pid = os.getppid()
        while pid > 1 and pid not in ancestors:
            ancestors.add(pid)
            pid = int(subprocess.check_output(['ps', '-p', str(pid), '-o', 'ppid='], text=True))
        for line in subprocess.check_output(['ps', '-axo', 'pid=,command='], text=True).splitlines():
            values = line.strip().split(None, 1)
            if len(values) == 2 and int(values[0]) not in ancestors:
                assert str(root) not in values[1], 'Fixture process still active'
    else:
        assert registration() is None
        assert_no_startup()
        assert not windows_process_references(root), 'Fixture process still active'
    files = 0
    size = 0
    for base, directories, names in os.walk(root, followlinks=False):
        for name in directories + names:
            path = Path(base) / name
            attributes = path.lstat()
            assert not os.path.ismount(path)
            assert path.resolve().is_relative_to(root)
            if hasattr(os, 'getuid'):
                assert attributes.st_uid == os.getuid()
            assert stat.S_ISREG(attributes.st_mode) or stat.S_ISDIR(attributes.st_mode) or path.is_symlink()
            if path.is_file() and not path.is_symlink():
                files += 1
                size += attributes.st_size
    plan = {'target': str(root), 'type': 'owned dedicated CI test directory',
            'project': str(Path.cwd()), 'files': files, 'logical_bytes': size,
            'all_mounts_and_processes_finished': True,
            'regenerable': 'Verified package downloads/copies/synthetic profiles only; reports retained outside fixture',
            'free_before_bytes': shutil.disk_usage(root).free}
    print('CLEANUP_PLAN:', json.dumps(plan), flush=True)
    # This is the normal cleanup of this script's own verified CI fixture.
    shutil.rmtree(root)
    assert not root.exists()
    plan.update(removed=True, session_fixture_bytes_remaining=0,
                free_after_bytes=shutil.disk_usage(runner_temp).free)
    (output / 'cleanup.json').write_text(json.dumps(plan, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--platform', choices=['windows-x64', 'macos-arm64', 'macos-x64'], required=True)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--output-dir', type=Path, required=True)
    parser.add_argument('--candidate-archive', type=Path)
    parser.add_argument('--local-headless-only', action='store_true')
    parser.add_argument('--cleanup-only', action='store_true')
    args = parser.parse_args()
    if args.cleanup_only:
        cleanup_fixture(args.directory.resolve(), args.output_dir.resolve(), args.platform)
        return
    if args.local_headless_only:
        assert sys.platform == 'darwin'
    else:
        assert os.environ.get('GITHUB_ACTIONS') == 'true', 'Use disposable GitHub Actions runners'
    target = args.platform
    assert (sys.platform == 'win32') == target.startswith('windows')
    if target.startswith('macos'):
        assert platform.machine() == ('arm64' if target.endswith('arm64') else 'x86_64')
    root = args.directory.resolve()
    assert not root.exists(), 'Refuse existing test directory'
    root.mkdir()
    output = args.output_dir.resolve()
    assert not output.is_relative_to(root), 'Retain reports outside the fixture'
    output.mkdir(parents=True, exist_ok=True)
    manifest = json.loads(Path(__file__).with_name('release_upgrade_sources.json').read_text())
    artifact = next(x for x in manifest['candidate_artifacts'] if x['platform'] == target)
    report = {'result': 'FAIL', 'platform': target, 'runtime_source': manifest['runtime_source'],
              'candidate_run_id': manifest['candidate_run_id'], 'artifact_id': artifact['id'],
              'script_sha256': sha(Path(__file__)),
              'data_script_sha256': sha(Path(__file__).with_name('release_upgrade_test.py')),
              'fixture': str(root), 'cases': []}
    (root / 'ownership.json').write_text(json.dumps({'purpose': 'Actual package upgrades/syntheticdata',
                                                    'root': str(root), 'created_by_this_script': True,
                                                    'pid': os.getpid(), 'platform': target,
                                                    'run_id': os.environ.get('GITHUB_RUN_ID'),
                                                    'job': os.environ.get('GITHUB_JOB'),
                                                    'script_sha256': sha(Path(__file__))}, indent=2) + '\n')
    signal.signal(signal.SIGTERM, lambda signum, frame: (_ for _ in ()).throw(KeyboardInterrupt(signum)))
    try:
        if args.candidate_archive:
            archive = args.candidate_archive.resolve()
            assert archive.stat().st_size == artifact['bytes'] and sha(archive) == artifact['sha256']
        else:
            metadata = api_json(f'https://api.github.com/repos/{manifest["repository"]}/actions/artifacts/{artifact["id"]}')
            assert metadata['workflow_run']['head_sha'] == manifest['runtime_source']
            assert metadata['workflow_run']['id'] == manifest['candidate_run_id']
            assert metadata['digest'] == 'sha256:' + artifact['sha256']
            archive = root / 'candidate-artifact.zip'
            download(metadata['archive_download_url'], archive, artifact, authenticated=True)
        packages = root / 'candidate-packages'
        packages.mkdir()
        with zipfile.ZipFile(archive) as zipped:
            wanted = [n for n in zipped.namelist() if n.startswith('dist/') and
                      (n.endswith('.dmg') if target.startswith('macos') else
                       n.endswith(('-setup.exe', '-windows-x64.zip')))]
            assert len(wanted) == (1 if target.startswith('macos') else 2), wanted
            for name in wanted:
                assert '..' not in PurePosixPath(name).parts
                path = packages / PurePosixPath(name).name
                assert not path.exists()
                path.write_bytes(zipped.read(name))
        report['candidate_archive_verified'] = artifact
        report['candidate_packages'] = [{'name': p.name, 'sha256': sha(p), 'bytes': p.stat().st_size}
                                         for p in packages.iterdir()]
        for release in sorted(manifest['official_releases'], key=lambda x: x['tag']):
            version = release['tag'].removeprefix('v')
            case_root = root / release['tag']
            case_root.mkdir()
            suffix = target + ('.dmg' if target.startswith('macos') else '-setup.exe')
            legacy = next(a for a in release['assets'] if a['name'].endswith(suffix))
            old_package = case_root / legacy['name']
            download(legacy['url'], old_package, legacy)
            case = {'old_version': version, 'old_package_sha256': legacy['sha256'],
                    'old_package_size': legacy['bytes'], 'result': 'FAIL'}
            report['cases'].append(case)
            try:
                if target.startswith('macos'):
                    mac_case(case_root, old_package, next(packages.glob('*.dmg')), version,
                             6 if version == '0.5.6' else 11, target, case,
                             args.local_headless_only)
                else:
                    windows_case(case_root, old_package, next(packages.glob('*-setup.exe')),
                                 next(packages.glob('*.zip')), version,
                                 6 if version == '0.5.6' else 11, case)
                case['result'] = 'PASS'
            finally:
                (output / (release['tag'] + '.json')).write_text(json.dumps(case, indent=2) + '\n')
            print('PASS:', target, release['tag'], 'actual installed package/data upgrade', flush=True)
        report['result'] = 'PASS'
    except BaseException as error:
        report['error'] = type(error).__name__ + ': ' + str(error)
        raise
    finally:
        # Preserve original Core/installer diagnostics, never config/instance/env files.
        for path in root.rglob('*.log'):
            if path.name.endswith('.stdout.log') or path.name.endswith('-install.log'):
                destination = output / 'logs' / path.relative_to(root)
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(path, destination.with_suffix(destination.suffix + '.txt'))
        report['fixture_bytes_before_cleanup'] = sum(p.stat().st_size for p in root.rglob('*') if p.is_file())
        (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Run existing GDI/lifecycle checks with pinned, already built CI executables.

Only disposable Windows Actions runners are supported. No installer is run and
no public package is produced. Update the pins after a new UI/Core check; source
drift and expired/mismatched artifacts fail rather than silently rebuilding.
"""
import argparse
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import uuid
import zipfile

from installed_upgrade_test import api_json, architecture, clean_environment, download, sha
from windows_ci_screenshot import (ROOT, current_owner_matches, no_links,
                                   package_processes, tree_files)

REPOSITORY = 'gloryhui/workbuddy-proxy'
CORE_SOURCE = 'dd9debade83e91b0d11c6b28f18f97edb227abbd'
UI_SOURCE = CORE_SOURCE
CORE_EXE_SHA256 = 'ed1d6440180db79d5fe940488161aa2d84cb97c30682beceb9a545f37ab4640e'
PINS = {
    'core': {'id': 13072713023, 'run_id': 41278929532,
             'source': CORE_SOURCE, 'name': 'workbuddy-proxy-windows-x64',
             'package_version': '0.6.0', 'bytes': 61366295,
             'sha256': '88631e907155d2e4329221fabd61006fec73e4fee6cbabd77c5a0f2863fb29c4'},
    'ui': {'id': 13072713023, 'run_id': 41278929532,
           'source': UI_SOURCE, 'name': 'workbuddy-proxy-windows-x64',
           'layout': 'package', 'package_version': '0.6.0', 'bytes': 61366295,
           'sha256': '88631e907155d2e4329221fabd61006fec73e4fee6cbabd77c5a0f2863fb29c4'},
}


def checked_metadata(pin):
    base = f'https://api.github.com/repos/{REPOSITORY}/actions'
    metadata = api_json(f'{base}/artifacts/{pin["id"]}')
    assert metadata['id'] == pin['id'] and not metadata['expired']
    assert metadata['name'] == pin['name']
    assert metadata['size_in_bytes'] == pin['bytes']
    assert metadata['digest'] == 'sha256:' + pin['sha256']
    assert metadata['workflow_run']['id'] == pin['run_id']
    assert metadata['workflow_run']['head_sha'] == pin['source']
    run = api_json(f'{base}/runs/{pin["run_id"]}')
    assert run['head_sha'] == pin['source'] and run['conclusion'] == 'success'
    assert run['repository']['full_name'] == REPOSITORY
    return metadata, run


def unique_member(archive, name):
    matches = [info for info in archive.infolist() if info.filename == name]
    assert len(matches) == 1 and not matches[0].is_dir(), name
    # No archive paths are extracted. Only these explicitly named bytes are used.
    return archive.read(matches[0])


def package_binary(path, version, relative_path):
    prefix = f'workbuddy-proxy-{version}-windows-x64'
    with zipfile.ZipFile(path) as outer:
        blob = unique_member(outer, f'dist/{prefix}.zip')
    with zipfile.ZipFile(io.BytesIO(blob)) as inner:
        return unique_member(inner, f'{prefix}/{relative_path}')


def cleanup(root, marker, parent, report):
    no_links(root)
    assert root.parent == parent and not os.path.ismount(root)
    assert (root / '.owner').read_text() == marker
    assert current_owner_matches(root)
    entries = tree_files(root)
    for base, dirs, names in os.walk(root, followlinks=False):
        for name in dirs + names:
            assert current_owner_matches(Path(base) / name)
    assert not package_processes(root), 'A process still uses this fixture'
    size = sum(path.stat().st_size for path in entries)
    plan = {'path': str(root), 'type': 'session-owned CI downloads and two-binary test layout',
            'project': str(ROOT), 'files': len(entries), 'bytes': size,
            'regenerable_from': PINS, 'process_references': [],
            'free_before_bytes': shutil.disk_usage(parent).free}
    print('CLEANUP_PLAN:', json.dumps(plan), flush=True)
    # Exact root created in this invocation; every component/owner checked above.
    shutil.rmtree(root)
    assert not root.exists()
    report['cleanup'] = dict(plan, status='removed', temporary_bytes_remaining=0,
                             free_after_bytes=shutil.disk_usage(parent).free)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, required=True)
    args = parser.parse_args()
    assert sys.platform == 'win32' and os.environ.get('GITHUB_ACTIONS') == 'true'
    assert os.environ.get('RUNNER_OS') == 'Windows'
    output = no_links(args.output_dir)
    output.mkdir(parents=True, exist_ok=False)
    report = {'result': 'failed', 'pins': PINS, 'artifacts': {},
              'scope': 'Two GDI sizes and existing package lifecycle on a synthetic profile; '
                       'not a distribution package, installer, physical input/IME/HiDPI or full RC.',
              'script_sha256': sha(Path(__file__)), 'cleanup': {'status': 'not-created'}}
    root = None
    error = None
    try:
        for source, paths in [(CORE_SOURCE, ['src', 'Cargo.toml', 'Cargo.lock', 'packaging']),
                              (UI_SOURCE, ['gpui'])]:
            subprocess.run(['git', 'diff', '--exit-code', source, 'HEAD', '--', *paths],
                           cwd=ROOT, check=True)
        parent = no_links(Path(tempfile.gettempdir()))
        root = no_links(Path(tempfile.mkdtemp(prefix='studio-windows-frozen-', dir=parent)))
        marker = uuid.uuid4().hex
        (root / '.owner').write_text(marker)
        archives = {}
        downloads = {}
        for kind, pin in PINS.items():
            metadata, run = checked_metadata(pin)
            report['artifacts'][kind] = {'metadata': metadata, 'run': run}
            if pin['id'] not in downloads:
                path = root / f'{pin["id"]}.zip'
                download(metadata['archive_download_url'], path, pin, authenticated=True)
                downloads[pin['id']] = path
            path = downloads[pin['id']]
            assert path.stat().st_size == pin['bytes'] and sha(path) == pin['sha256']
            archives[kind] = path
            report['artifacts'][kind]['download_sha256'] = sha(path)
        package = root / 'package'
        (package / 'internal').mkdir(parents=True)
        core = package / 'workbuddy-proxy.exe'
        ui = package / 'internal' / 'workbuddy-settings.exe'
        core.write_bytes(package_binary(archives['core'], PINS['core']['package_version'],
                                        'workbuddy-proxy.exe'))
        if PINS['ui'].get('layout') == 'package':
            ui.write_bytes(package_binary(archives['ui'], PINS['ui']['package_version'],
                                          'internal/workbuddy-settings.exe'))
        else:
            with zipfile.ZipFile(archives['ui']) as archive:
                ui.write_bytes(unique_member(archive, 'workbuddy-proxy-gpui.exe'))
        for exe in (core, ui):
            architecture(exe, 'windows-x64')
        report['core_sha256'] = sha(core)
        assert report['core_sha256'] == CORE_EXE_SHA256
        report['ui_sha256'] = sha(ui)
        process = subprocess.run([sys.executable, str(ROOT / 'scripts/windows_ci_screenshot.py'),
                                  '--exe', str(core), '--output-dir', str(output / 'native'),
                                  '--ci-display-size', '1920x1080'],
                                 cwd=ROOT, env=clean_environment())
        report['capture_returncode'] = process.returncode
        assert process.returncode == 0, 'Native capture/lifecycle failed; preserve first failure'
        native = json.loads((output / 'native/windows-ci-screenshot.json').read_text())
        assert native['result'] == 'passed'
        assert native['core_sha256'] == report['core_sha256']
        assert native['ui_sha256'] == report['ui_sha256']
        assert native['cleanup']['status'] == 'removed'
        report['result'] = 'passed'
    except Exception as failure:
        error = f'{type(failure).__name__}: {failure}'
    finally:
        if root:
            try:
                cleanup(root, marker, parent, report)
            except Exception as failure:
                report['cleanup'] = {'status': 'retained', 'path': str(root),
                                     'reason': f'{type(failure).__name__}: {failure}'}
                error = (error + '; ' if error else '') + 'Safe cleanup blocked'
        if error:
            report.update(result='failed', error=error)
        (output / 'frozen-native.json').write_text(json.dumps(report, indent=2) + '\n')
    return 0 if report['result'] == 'passed' else 1


if __name__ == '__main__':
    sys.exit(main())

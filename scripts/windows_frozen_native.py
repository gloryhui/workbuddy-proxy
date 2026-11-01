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
CORE_SOURCE = '9570451e5006e25118cb2cafa6b3da69353836da'
UI_SOURCE = '5681103e5b05fb88c73f8ea4a32bafd11cbc3380'
CORE_EXE_SHA256 = '880016c6b5c3cf8c0b269e4ed5a74ab6bfaa6026a888b52850b19048a31590e6'
PINS = {
    'core': {'id': 13060545088, 'run_id': 41246381635,
             'source': CORE_SOURCE, 'name': 'workbuddy-proxy-windows-x64',
             'bytes': 60734196,
             'sha256': '8aba173b7b087c4add1832aa96220129b47f7c855d97afb7b506ac59f56c226f'},
    'ui': {'id': 13069217005, 'run_id': 41270728784,
           'source': UI_SOURCE, 'name': 'studio-ui-check-windows-x64',
           'bytes': 29808660,
           'sha256': 'e5f5ad012349d2d5081716c86fb82da8f0526d851df340d5b618696db20f7a33'},
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
        for kind, pin in PINS.items():
            metadata, run = checked_metadata(pin)
            report['artifacts'][kind] = {'metadata': metadata, 'run': run}
            path = root / f'{kind}.zip'
            download(metadata['archive_download_url'], path, pin, authenticated=True)
            archives[kind] = path
            report['artifacts'][kind]['download_sha256'] = sha(path)
        package = root / 'package'
        (package / 'internal').mkdir(parents=True)
        core = package / 'workbuddy-proxy.exe'
        ui = package / 'internal' / 'workbuddy-settings.exe'
        with zipfile.ZipFile(archives['core']) as outer:
            blob = unique_member(outer, 'dist/workbuddy-proxy-0.5.7-windows-x64.zip')
        with zipfile.ZipFile(io.BytesIO(blob)) as inner:
            core.write_bytes(unique_member(inner, 'workbuddy-proxy-0.5.7-windows-x64/workbuddy-proxy.exe'))
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

#!/usr/bin/env python3
"""Build a directory-selectable, per-user Inno Setup EXE from the portable ZIP."""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import zipfile


def compiler():
    explicit = os.environ.get('ISCC')
    if explicit:
        return explicit
    found = shutil.which('ISCC.exe')
    if found:
        return found
    for key in ('ProgramFiles(x86)', 'ProgramFiles'):
        path = Path(os.environ.get(key, 'C:/Program Files (x86)')) / 'Inno Setup 6/ISCC.exe'
        if path.is_file():
            return str(path)
    raise SystemExit('Inno Setup 6 is required only on the Windows build machine')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--zip', required=True)
    parser.add_argument('--output', default='dist')
    parser.add_argument('--display-version', default='0.5.3')
    args = parser.parse_args()
    archive = Path(args.zip).resolve()
    out = Path(args.output).resolve()
    out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='workbuddy-installer-build-') as temp:
        with zipfile.ZipFile(archive) as z:
            for info in z.infolist():
                dest = (Path(temp) / info.filename).resolve()
                if not dest.is_relative_to(Path(temp).resolve()):
                    raise ValueError('Invalid archive path')
            z.extractall(temp)
        roots = list(Path(temp).glob('*/workbuddy-proxy.exe'))
        assert len(roots) == 1, 'Archive must have one main entry point'
        root = roots[0].parent
        assert (root / 'internal/workbuddy-settings.exe').is_file()
        assert not (root / 'workbuddy-proxy-gpui.exe').exists()
        assert sorted(p.name for p in root.glob('*.exe')) == ['workbuddy-proxy.exe']
        subprocess.run([compiler(), '/Qp', f'/DSourceDir={root}', f'/DOutputDir={out}',
                        f'/DPackageName={archive.stem}', f'/DDisplayVersion={args.display_version}',
                        str(Path('packaging/windows/workbuddy-proxy.iss').resolve())], check=True)
    setup = out / (archive.stem + '-setup.exe')
    assert setup.is_file()
    setup.with_suffix('.exe.sha256').write_text(hashlib.sha256(setup.read_bytes()).hexdigest() + '  ' + setup.name + '\n')
    print(setup)


if __name__ == '__main__':
    main()

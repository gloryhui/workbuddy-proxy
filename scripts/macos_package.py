#!/usr/bin/env python3
"""Mount the release DMG and verify its drag-to-install application copy."""
import argparse
import os
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
from ds_store import DSStore


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--dmg', required=True)
    args = parser.parse_args()
    dmg = Path(args.dmg).resolve()
    with tempfile.TemporaryDirectory(prefix='workbuddy-dmg-check-') as directory:
        root = Path(directory)
        mount = root / 'volume'
        mount.mkdir()
        subprocess.run(['hdiutil', 'attach', '-readonly', '-nobrowse', '-mountpoint', str(mount), str(dmg)], check=True, stdout=subprocess.DEVNULL)
        try:
            app = mount / 'WorkBuddy Proxy.app'
            assert app.is_dir() and os.readlink(mount / 'Applications') == '/Applications'
            info = plistlib.loads((app / 'Contents/Info.plist').read_bytes())
            assert info['CFBundlePackageType'] == 'APPL'
            assert info['LSUIElement'] is False and not info.get('LSBackgroundOnly', False)
            assert info['LSApplicationCategoryType'] == 'public.app-category.utilities'
            assert (app / 'Contents/Resources/app-icon.icns').read_bytes() == Path('assets/app-icon.icns').read_bytes()
            assert (app / 'Contents/Resources/Documentation/LICENSE').is_file()
            with DSStore.open(str(mount / '.DS_Store'), 'r') as store:
                assert store['WorkBuddy Proxy.app']['Iloc'] == (160, 130)
                assert store['Applications']['Iloc'] == (480, 130)
            assert not any(p.name in {'config.json', 'instance.json', 'frontend.json', 'update-cache.json'} for p in app.rglob('*'))
            installed = root / 'Applications/WorkBuddy Proxy.app'
            installed.parent.mkdir()
            subprocess.run(['ditto', str(app), str(installed)], check=True)
            subprocess.run(['codesign', '--verify', '--strict', str(installed)], check=True)
            executable = installed / 'Contents/MacOS' / info['CFBundleExecutable']
            assert os.access(executable, os.X_OK)
            version = subprocess.check_output([str(executable), '--version'], text=True).strip()
            assert version == info['CFBundleShortVersionString']
            subprocess.run([sys.executable, 'scripts/core_compatibility.py', '--exe', str(executable)], check=True)
            subprocess.run([sys.executable, 'scripts/connection_test.py', '--exe', str(executable)], check=True)
            print('PASS: read-only DMG, Finder layout, Applications link, regular-app metadata, copied app signature/executable/version and no user configuration')
        finally:
            subprocess.run(['hdiutil', 'detach', str(mount)], check=True, stdout=subprocess.DEVNULL)


if __name__ == '__main__':
    main()

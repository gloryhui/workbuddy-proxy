#!/usr/bin/env python3
"""Verify the actual GPUI DMG, copy its app, then test the copied executables."""
import argparse
import os
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import tomllib
from ds_store import DSStore

def verify_staged_app(app, output):
    """Run copied executables inside the caller's protected fixture environment.

    A caller may mount/copy read-only outside a process sandbox when macOS
    refuses to delegate disk-image attachment from that sandbox. Image layout
    and packaged-code identity must then be attested by the staging caller.
    """
    core=app/'Contents/MacOS/workbuddy-proxy';helper=app/'Contents/Helpers/WorkBuddy Proxy Settings.app'
    for bundle in (app,helper):
        subprocess.run(['codesign','--verify','--strict',str(bundle)],check=True)
        info=plistlib.loads((bundle/'Contents/Info.plist').read_bytes());assert info['CFBundleVersion']==tomllib.loads(Path('Cargo.toml').read_text())['package']['version'] and info['LSUIElement'] is False
    assert not any(p.name in {'config.json','instance.json','gpui-drafts.json','frontend.json'} for p in app.rglob('*'))
    assert subprocess.check_output([str(helper/'Contents/MacOS/workbuddy-proxy-gpui'),'--version'],text=True).strip()==tomllib.loads(Path('Cargo.toml').read_text())['package']['version']
    subprocess.run([sys.executable,'scripts/core_compatibility.py','--exe',str(core)],check=True)
    subprocess.run([sys.executable,'scripts/model_capabilities_test.py','--exe',str(core)],check=True)
    subprocess.run([sys.executable,'scripts/autostart_test.py','--exe',str(core)],check=True)
    subprocess.run([sys.executable,'scripts/gpui_package_test.py','--exe',str(core),'--output',str(Path(output).resolve())],check=True)

def main():
    p=argparse.ArgumentParser();source=p.add_mutually_exclusive_group(required=True);source.add_argument('--dmg');source.add_argument('--staged-app',type=Path,help='already copied app; caller must separately attest the readonly image layout and code identity');p.add_argument('--output',required=True);a=p.parse_args()
    if a.staged_app:
        verify_staged_app(a.staged_app.resolve(),a.output)
        print('PASS: staged signed app metadata and protected executable lifecycle; image layout is a separate caller attestation')
        return
    with tempfile.TemporaryDirectory(prefix='workbuddy-gpui-dmg-') as directory:
        root=Path(directory);mount=root/'volume';mount.mkdir();app_name='WorkBuddy Proxy.app'
        subprocess.run(['hdiutil','attach','-readonly','-nobrowse','-mountpoint',str(mount),str(Path(a.dmg).resolve())],check=True,stdout=subprocess.DEVNULL)
        try:
            assert os.readlink(mount/'Applications')=='/Applications'
            with DSStore.open(str(mount/'.DS_Store'),'r') as store:assert store[app_name]['Iloc']==(160,130)
            app=root/app_name;subprocess.run(['ditto',str(mount/app_name),str(app)],check=True)
            verify_staged_app(app,a.output)
            print('PASS: readonly GPUI DMG, Finder drag layout, copied core/helper signatures, metadata, no user config, real packaged settings/relay lifecycle')
        finally:subprocess.run(['hdiutil','detach',str(mount)],check=True,stdout=subprocess.DEVNULL)

if __name__=='__main__':main()

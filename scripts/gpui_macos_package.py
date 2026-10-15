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

def main():
    p=argparse.ArgumentParser();p.add_argument('--dmg',required=True);p.add_argument('--output',required=True);a=p.parse_args()
    with tempfile.TemporaryDirectory(prefix='workbuddy-gpui-dmg-') as directory:
        root=Path(directory);mount=root/'volume';mount.mkdir();app_name='WorkBuddy Proxy.app'
        subprocess.run(['hdiutil','attach','-readonly','-nobrowse','-mountpoint',str(mount),str(Path(a.dmg).resolve())],check=True,stdout=subprocess.DEVNULL)
        try:
            assert os.readlink(mount/'Applications')=='/Applications'
            with DSStore.open(str(mount/'.DS_Store'),'r') as store:assert store[app_name]['Iloc']==(160,130)
            app=root/app_name;subprocess.run(['ditto',str(mount/app_name),str(app)],check=True)
            core=app/'Contents/MacOS/workbuddy-proxy';helper=app/'Contents/Helpers/WorkBuddy Proxy Settings.app'
            for bundle in (app,helper):
                subprocess.run(['codesign','--verify','--strict',str(bundle)],check=True)
                info=plistlib.loads((bundle/'Contents/Info.plist').read_bytes());assert info['CFBundleVersion']==tomllib.loads(Path('Cargo.toml').read_text())['package']['version'] and info['LSUIElement'] is False
            assert not any(p.name in {'config.json','instance.json','gpui-drafts.json','frontend.json'} for p in app.rglob('*'))
            assert subprocess.check_output([str(helper/'Contents/MacOS/workbuddy-proxy-gpui'),'--version'],text=True).strip()==tomllib.loads(Path('Cargo.toml').read_text())['package']['version']
            subprocess.run([sys.executable,'scripts/core_compatibility.py','--exe',str(core)],check=True)
            subprocess.run([sys.executable,'scripts/model_capabilities_test.py','--exe',str(core)],check=True)
            subprocess.run([sys.executable,'scripts/autostart_test.py','--exe',str(core)],check=True)
            subprocess.run([sys.executable,'scripts/gpui_package_test.py','--exe',str(core),'--output',str(Path(a.output).resolve())],check=True)
            print('PASS: readonly GPUI DMG, Finder drag layout, copied core/helper signatures, metadata, no user config, real packaged settings/relay lifecycle')
        finally:subprocess.run(['hdiutil','detach',str(mount)],check=True,stdout=subprocess.DEVNULL)

if __name__=='__main__':main()

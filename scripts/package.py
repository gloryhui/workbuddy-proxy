#!/usr/bin/env python3
"""Package only the executable and documentation; never user configuration."""
import argparse
import hashlib
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile
import zipfile

def main():
    p=argparse.ArgumentParser();p.add_argument('--exe',required=True);p.add_argument('--platform',choices=['macos-arm64','macos-x64','windows-x64'],required=True);p.add_argument('--output',default='dist');p.add_argument('--version',default='0.3.0');args=p.parse_args()
    out=Path(args.output);out.mkdir(parents=True,exist_ok=True)
    name=f'workbuddy-proxy-{args.version}-{args.platform}'
    with tempfile.TemporaryDirectory(prefix='workbuddy-release-') as temp:
        root=Path(temp)/name;root.mkdir()
        if args.platform.startswith('macos'):
            app=root/'WorkBuddy Proxy.app';contents=app/'Contents';binaries=contents/'MacOS';binaries.mkdir(parents=True)
            exe=binaries/'workbuddy-proxy';shutil.copy(args.exe,exe);exe.chmod(0o755)
            resources=contents/'Resources';resources.mkdir();shutil.copy('assets/app-icon.icns',resources/'app-icon.icns')
            with (contents/'Info.plist').open('wb') as f:
                plistlib.dump({'CFBundleName':'WorkBuddy Proxy','CFBundleDisplayName':'WorkBuddy Proxy','CFBundleIdentifier':'io.github.gloryhui.workbuddy-proxy','CFBundleExecutable':'workbuddy-proxy','CFBundlePackageType':'APPL','CFBundleIconFile':'app-icon.icns','CFBundleShortVersionString':args.version,'CFBundleVersion':'1','LSMinimumSystemVersion':'12.0','LSUIElement':True,'NSHighResolutionCapable':True},f)
            subprocess.run(['codesign','--force','--sign','-',str(app)],check=True)
            subprocess.run(['codesign','--verify','--strict',str(app)],check=True)
        else:
            shutil.copy(args.exe,root/'workbuddy-proxy.exe')
            (root/'Diagnose.cmd').write_bytes(b'@echo off\r\ncd /d "%~dp0"\r\necho Starting headless diagnostics. Close any existing WorkBuddy Proxy first.\r\n"workbuddy-proxy.exe" --headless\r\necho Exit code: %errorlevel%\r\npause\r\n')
        shutil.copy('README.md',root/'README.md');shutil.copy('LICENSE',root/'LICENSE')
        shutil.copytree('docs',root/'docs')
        archive=out/(name+'.zip')
        with zipfile.ZipFile(archive,'w',zipfile.ZIP_DEFLATED) as z:
            for path in sorted(root.rglob('*')):
                if path.is_file():z.write(path,path.relative_to(root.parent))
    (out/(name+'.sha256')).write_text(hashlib.sha256(archive.read_bytes()).hexdigest()+'  '+archive.name+'\n')
    print(archive)
if __name__=='__main__':main()

#!/usr/bin/env python3
"""Package executables/docs only: Windows ZIP, macOS drag-to-install DMG + ZIP."""
import argparse
import hashlib
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile
import zipfile

def main():
    p=argparse.ArgumentParser();p.add_argument('--exe',required=True);p.add_argument('--platform',choices=['macos-arm64','macos-x64','windows-x64'],required=True);p.add_argument('--output',default='dist');p.add_argument('--version',default='0.4.0');p.add_argument('--gpui');p.add_argument('--variant',default='');args=p.parse_args()
    out=Path(args.output);out.mkdir(parents=True,exist_ok=True)
    name=f'workbuddy-proxy-{args.version}'+(f'-{args.variant}' if args.variant else '')+f'-{args.platform}'
    app_name='WorkBuddy Proxy GPUI Preview.app' if args.gpui else 'WorkBuddy Proxy.app'
    with tempfile.TemporaryDirectory(prefix='workbuddy-release-') as temp:
        root=Path(temp)/name;root.mkdir()
        if args.platform.startswith('macos'):
            app=root/app_name;contents=app/'Contents';binaries=contents/'MacOS';binaries.mkdir(parents=True)
            exe=binaries/'workbuddy-proxy';shutil.copy(args.exe,exe);exe.chmod(0o755)
            resources=contents/'Resources';resources.mkdir();shutil.copy('assets/app-icon.icns',resources/'app-icon.icns')
            with (contents/'Info.plist').open('wb') as f:
                plistlib.dump({'CFBundleName':'WorkBuddy Proxy','CFBundleDisplayName':'WorkBuddy Proxy','CFBundleIdentifier':'io.github.gloryhui.workbuddy-proxy.gpui-preview' if args.gpui else 'io.github.gloryhui.workbuddy-proxy','CFBundleExecutable':'workbuddy-proxy','CFBundlePackageType':'APPL','CFBundleIconFile':'app-icon.icns','CFBundleShortVersionString':args.version,'CFBundleVersion':'10' if args.gpui else '9','LSMinimumSystemVersion':'12.0','LSUIElement':False,'LSApplicationCategoryType':'public.app-category.utilities','NSHighResolutionCapable':True},f)
        else:
            shutil.copy(args.exe,root/'workbuddy-proxy.exe')
            (root/'Diagnose.cmd').write_bytes(b'@echo off\r\ncd /d "%~dp0"\r\necho Starting headless diagnostics. Close any existing WorkBuddy Proxy first.\r\n"workbuddy-proxy.exe" --headless\r\necho Exit code: %errorlevel%\r\npause\r\n')
        if args.gpui:
            if args.platform.startswith('macos'):
                helper=contents/'Helpers/WorkBuddy Proxy Settings.app/Contents';helper_binary=helper/'MacOS';helper_binary.mkdir(parents=True)
                shutil.copy(args.gpui,helper_binary/'workbuddy-proxy-gpui');(helper_binary/'workbuddy-proxy-gpui').chmod(0o755)
                helper_resources=helper/'Resources';helper_resources.mkdir();shutil.copy('assets/app-icon.icns',helper_resources/'app-icon.icns')
                with (helper/'Info.plist').open('wb') as f:
                    plistlib.dump({'CFBundleName':'WorkBuddy Proxy','CFBundleDisplayName':'WorkBuddy Proxy','CFBundleIdentifier':'io.github.gloryhui.workbuddy-proxy.gpui-preview.settings','CFBundleExecutable':'workbuddy-proxy-gpui','CFBundlePackageType':'APPL','CFBundleIconFile':'app-icon.icns','CFBundleShortVersionString':args.version,'CFBundleVersion':'10','LSMinimumSystemVersion':'12.0','LSUIElement':False,'NSHighResolutionCapable':True},f)
                subprocess.run(['codesign','--force','--sign','-',str(helper.parent)],check=True)
            else:
                shutil.copy(args.gpui,root/'workbuddy-proxy-gpui.exe')
            shutil.copy('docs/gpui-preview.md',root/'GPUI-PREVIEW.md')
        shutil.copy('README.md',root/'README.md');shutil.copy('LICENSE',root/'LICENSE');shutil.copy('CHANGELOG.md',root/'CHANGELOG.md')
        (root/'assets').mkdir();shutil.copy('assets/icon-128.png',root/'assets/icon-128.png')
        shutil.copytree('docs',root/'docs')
        if args.platform.startswith('macos'):
            # Keep the installer window to two items. Documentation travels inside the app.
            documentation=resources/'Documentation';documentation.mkdir()
            for filename in ['README.md','LICENSE','CHANGELOG.md']+(['GPUI-PREVIEW.md'] if args.gpui else []):
                shutil.copy(root/filename,documentation/filename)
            shutil.copytree(root/'docs',documentation/'docs')
            subprocess.run(['codesign','--force','--sign','-',str(app)],check=True)
            subprocess.run(['codesign','--verify','--strict',str(app)],check=True)
            try:
                from dmgbuild import build_dmg
            except ImportError as error:
                raise SystemExit('Install build-only DMG dependencies: python3 -m pip install -r scripts/dmg-requirements.txt') from error
            dmg=out/(name+'.dmg')
            build_dmg(str(dmg.resolve()),'WorkBuddy Proxy',settings={
                'format':'UDZO','filesystem':'HFS+','files':[str(app.resolve())],
                'symlinks':{'Applications':'/Applications'},
                'icon':str(Path('assets/app-icon.icns').resolve()),
                'background':'builtin-arrow','window_rect':((200,200),(640,280)),
                'default_view':'icon-view','icon_size':96,'text_size':13,
                'icon_locations':{app_name:(160,130),'Applications':(480,130)},
                'show_toolbar':False,'show_sidebar':False,'show_status_bar':False,
            })
            subprocess.run(['hdiutil','verify',str(dmg)],check=True,stdout=subprocess.DEVNULL)
            Path(str(dmg)+'.sha256').write_text(hashlib.sha256(dmg.read_bytes()).hexdigest()+'  '+dmg.name+'\n')
            print(dmg)
        archive=out/(name+'.zip')
        with zipfile.ZipFile(archive,'w',zipfile.ZIP_DEFLATED) as z:
            for path in sorted(root.rglob('*')):
                if path.is_file():z.write(path,path.relative_to(root.parent))
    (out/(name+'.sha256')).write_text(hashlib.sha256(archive.read_bytes()).hexdigest()+'  '+archive.name+'\n')
    print(archive)
if __name__=='__main__':main()

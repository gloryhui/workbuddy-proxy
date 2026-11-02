#!/usr/bin/env python3
"""Package runtime files and licenses: Windows staging ZIP or macOS DMG."""
import argparse
import hashlib
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile
import zipfile
from release_version import release_version

LICENSE_FILES = (
    ('LICENSE', 'LICENSE'),
    ('docs/GPUI-NOTICES.md', 'THIRD-PARTY-NOTICES.txt'),
    ('docs/LICENSE-APACHE-GPUI.txt', 'LICENSE-APACHE-GPUI.txt'),
    ('docs/third-party/magpie-LICENSE.txt', 'magpie-LICENSE.txt'),
)


def copy_licenses(destination):
    # Explicit files only: never sweep docs, screenshots or historical evidence
    # into a distributable. Notices are legal attribution, not user guides.
    destination.mkdir(parents=True, exist_ok=True)
    for source, filename in LICENSE_FILES:
        shutil.copy(source, destination / filename)


def main():
    p=argparse.ArgumentParser();p.add_argument('--exe',required=True);p.add_argument('--platform',choices=['macos-arm64','macos-x64','windows-x64'],required=True);p.add_argument('--output',default='dist');p.add_argument('--version',help='Must match the Core/UI Cargo package version');p.add_argument('--gpui',required=True);p.add_argument('--variant',default='');args=p.parse_args()
    try:
        args.version=release_version(args.version)
    except ValueError as error:
        p.error(str(error))
    out=Path(args.output);out.mkdir(parents=True,exist_ok=True)
    name=f'workbuddy-proxy-{args.version}'+(f'-{args.variant}' if args.variant else '')+f'-{args.platform}'
    app_name='WorkBuddy Proxy.app'
    with tempfile.TemporaryDirectory(prefix='workbuddy-release-') as temp:
        root=Path(temp)/name;root.mkdir()
        if args.platform.startswith('macos'):
            app=root/app_name;contents=app/'Contents';binaries=contents/'MacOS';binaries.mkdir(parents=True)
            exe=binaries/'workbuddy-proxy';shutil.copy(args.exe,exe);exe.chmod(0o755)
            resources=contents/'Resources';resources.mkdir();shutil.copy('assets/app-icon.icns',resources/'app-icon.icns')
            with (contents/'Info.plist').open('wb') as f:
                plistlib.dump({'CFBundleName':'WorkBuddy Proxy','CFBundleDisplayName':'WorkBuddy Proxy','CFBundleIdentifier':'io.github.gloryhui.workbuddy-proxy','CFBundleExecutable':'workbuddy-proxy','CFBundlePackageType':'APPL','CFBundleIconFile':'app-icon.icns','CFBundleShortVersionString':args.version,'CFBundleVersion':args.version,'LSMinimumSystemVersion':'12.0','LSUIElement':False,'LSApplicationCategoryType':'public.app-category.utilities','NSHighResolutionCapable':True},f)
        else:
            shutil.copy(args.exe,root/'workbuddy-proxy.exe')
            (root/'Diagnose.cmd').write_bytes(b'@echo off\r\ncd /d "%~dp0"\r\necho Starting headless diagnostics. Close any existing WorkBuddy Proxy first.\r\n"workbuddy-proxy.exe" --headless\r\necho Exit code: %errorlevel%\r\npause\r\n')
        if args.gpui:
            if args.platform.startswith('macos'):
                helper=contents/'Helpers/WorkBuddy Proxy Settings.app/Contents';helper_binary=helper/'MacOS';helper_binary.mkdir(parents=True)
                shutil.copy(args.gpui,helper_binary/'workbuddy-proxy-gpui');(helper_binary/'workbuddy-proxy-gpui').chmod(0o755)
                helper_resources=helper/'Resources';helper_resources.mkdir();shutil.copy('assets/app-icon.icns',helper_resources/'app-icon.icns')
                with (helper/'Info.plist').open('wb') as f:
                    plistlib.dump({'CFBundleName':'WorkBuddy Proxy','CFBundleDisplayName':'WorkBuddy Proxy','CFBundleIdentifier':'io.github.gloryhui.workbuddy-proxy.settings','CFBundleExecutable':'workbuddy-proxy-gpui','CFBundlePackageType':'APPL','CFBundleIconFile':'app-icon.icns','CFBundleShortVersionString':args.version,'CFBundleVersion':args.version,'LSMinimumSystemVersion':'12.0','LSUIElement':False,'NSHighResolutionCapable':True},f)
                subprocess.run(['codesign','--force','--sign','-',str(helper.parent)],check=True)
            else:
                internal=root/'internal';internal.mkdir()
                shutil.copy(args.gpui,internal/'workbuddy-settings.exe')
        if args.platform.startswith('macos'):
            copy_licenses(resources/'Licenses')
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
        if args.platform.startswith('macos'):
            return
        (root/'assets').mkdir();shutil.copy('assets/icon-128.png',root/'assets/icon-128.png')
        copy_licenses(root/'licenses')
        archive=out/(name+'.zip')
        with zipfile.ZipFile(archive,'w',zipfile.ZIP_DEFLATED) as z:
            for path in sorted(root.rglob('*')):
                if path.is_file():z.write(path,path.relative_to(root.parent))
    (out/(name+'.sha256')).write_text(hashlib.sha256(archive.read_bytes()).hexdigest()+'  '+archive.name+'\n')
    print(archive)
if __name__=='__main__':main()

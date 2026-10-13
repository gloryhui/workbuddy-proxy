#!/usr/bin/env python3
"""Convert the supplied artwork to native icon formats on macOS (no runtime dependency)."""
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile

ROOT=Path(__file__).resolve().parents[1]
ASSETS=ROOT/'assets'

def main():
    with tempfile.TemporaryDirectory(prefix='workbuddy-icons-') as folder:
        temp=Path(folder);sizes=[16,24,32,48,64,128,256,512,1024]
        for size in sizes:
            subprocess.run(['sips','-z',str(size),str(size),str(ASSETS/'icon-source.png'),'--out',str(temp/f'{size}.png')],check=True,stdout=subprocess.DEVNULL)
        frames=[(size,(temp/f'{size}.png').read_bytes()) for size in sizes if size<=256]
        offset=6+16*len(frames);directory=bytearray(struct.pack('<HHH',0,1,len(frames)));payload=bytearray()
        for size,png in frames:
            directory.extend(struct.pack('<BBBBHHII',size if size<256 else 0,size if size<256 else 0,0,0,1,32,len(png),offset))
            payload.extend(png);offset+=len(png)
        (ASSETS/'app-icon.ico').write_bytes(directory+payload)
        shutil.copyfile(temp/'128.png',ASSETS/'icon-128.png')
        iconset=temp/'app-icon.iconset';iconset.mkdir()
        for size in [16,32,128,256,512]:
            for scale in [1,2]:
                name=f'icon_{size}x{size}'+('@2x' if scale==2 else '')+'.png'
                shutil.copyfile(temp/f'{size*scale}.png',iconset/name)
        subprocess.run(['iconutil','-c','icns',str(iconset),'-o',str(ASSETS/'app-icon.icns')],check=True)
    print('Generated ICO, ICNS and native UI PNG from the current icon artwork')
if __name__=='__main__':main()

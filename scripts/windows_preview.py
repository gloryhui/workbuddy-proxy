#!/usr/bin/env python3
"""Capture the real native fixture window in CI, using the Windows GDI APIs."""
import ctypes
from ctypes import wintypes
from pathlib import Path
import struct
import zlib

def capture(hwnd,path):
    user=ctypes.WinDLL('user32');gdi=ctypes.WinDLL('gdi32')
    user.GetWindowRect.argtypes=[wintypes.HWND,ctypes.POINTER(wintypes.RECT)]
    user.GetWindowDC.argtypes=[wintypes.HWND];user.GetWindowDC.restype=wintypes.HDC
    user.ReleaseDC.argtypes=[wintypes.HWND,wintypes.HDC]
    user.PrintWindow.argtypes=[wintypes.HWND,wintypes.HDC,wintypes.UINT]
    gdi.CreateCompatibleDC.argtypes=[wintypes.HDC];gdi.CreateCompatibleDC.restype=wintypes.HDC
    gdi.CreateCompatibleBitmap.argtypes=[wintypes.HDC,ctypes.c_int,ctypes.c_int];gdi.CreateCompatibleBitmap.restype=wintypes.HBITMAP
    gdi.SelectObject.argtypes=[wintypes.HDC,wintypes.HGDIOBJ];gdi.SelectObject.restype=wintypes.HGDIOBJ
    gdi.GetDIBits.argtypes=[wintypes.HDC,wintypes.HBITMAP,wintypes.UINT,wintypes.UINT,ctypes.c_void_p,ctypes.c_void_p,wintypes.UINT]
    gdi.DeleteObject.argtypes=[wintypes.HGDIOBJ];gdi.DeleteDC.argtypes=[wintypes.HDC]
    rect=wintypes.RECT();assert user.GetWindowRect(hwnd,ctypes.byref(rect));width=rect.right-rect.left;height=rect.bottom-rect.top
    source=user.GetWindowDC(hwnd);dc=gdi.CreateCompatibleDC(source);bitmap=gdi.CreateCompatibleBitmap(source,width,height);old=gdi.SelectObject(dc,bitmap)
    try:
        assert user.PrintWindow(hwnd,dc,2),'Cannot capture the real fixture window'
        gdi.SelectObject(dc,old)
        info=ctypes.create_string_buffer(struct.pack('<IiiHHIIiiII',40,width,-height,1,32,0,0,0,0,0,0))
        pixels=ctypes.create_string_buffer(width*height*4)
        assert gdi.GetDIBits(dc,bitmap,0,height,pixels,info,0)==height
        raw=pixels.raw;scan=bytearray()
        for y in range(height):
            scan.append(0)
            for x in range(width):
                offset=(y*width+x)*4;scan.extend((raw[offset+2],raw[offset+1],raw[offset]))
        def chunk(kind,data):return struct.pack('>I',len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data))
        image=b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',width,height,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(scan))+chunk(b'IEND',b'')
        Path(path).parent.mkdir(parents=True,exist_ok=True);Path(path).write_bytes(image)
    finally:
        gdi.DeleteObject(bitmap);gdi.DeleteDC(dc);user.ReleaseDC(hwnd,source)

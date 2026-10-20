#!/usr/bin/env python3
"""Save one visible Windows application window as PNG using GDI, not WGC."""
import argparse
import ctypes
from ctypes import wintypes as W
import hashlib
import json
from pathlib import Path
import sys
import time


def capture(pid, output, activate=False):
    if sys.platform != 'win32':
        raise RuntimeError('This capture helper requires Windows.')
    try:
        from PIL import ImageGrab
    except ImportError as error:
        raise RuntimeError('Use a Python environment with Pillow installed.') from error

    user32 = ctypes.WinDLL('user32', use_last_error=True)
    dwm = ctypes.WinDLL('dwmapi')
    # All screen and DWM coordinates must use the same physical-pixel space.
    user32.SetProcessDpiAwarenessContext.argtypes = [ctypes.c_void_p]
    user32.SetProcessDpiAwarenessContext.restype = W.BOOL
    if not user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4)):
        raise RuntimeError('Cannot enable per-monitor DPI awareness; use a fresh Python process.')
    user32.GetWindowThreadProcessId.argtypes = [W.HWND, ctypes.POINTER(W.DWORD)]
    user32.IsWindowVisible.argtypes = [W.HWND]
    user32.IsIconic.argtypes = [W.HWND]
    user32.GetWindowRect.argtypes = [W.HWND, ctypes.POINTER(W.RECT)]
    user32.GetForegroundWindow.restype = W.HWND
    user32.ShowWindow.argtypes = [W.HWND, ctypes.c_int]
    user32.SetForegroundWindow.argtypes = [W.HWND]
    user32.GetSystemMetrics.argtypes = [ctypes.c_int]
    dwm.DwmGetWindowAttribute.argtypes = [W.HWND, W.DWORD, ctypes.c_void_p, W.DWORD]
    dwm.DwmGetWindowAttribute.restype = ctypes.c_long
    callback_type = ctypes.WINFUNCTYPE(W.BOOL, W.HWND, W.LPARAM)
    windows = []

    @callback_type
    def enumerate_window(hwnd, _):
        owner = W.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        rect = W.RECT()
        if (owner.value == pid and user32.IsWindowVisible(hwnd)
                and user32.GetWindowRect(hwnd, ctypes.byref(rect))
                and rect.right > rect.left and rect.bottom > rect.top):
            windows.append(hwnd)
        return True

    user32.EnumWindows.argtypes = [callback_type, W.LPARAM]
    if not user32.EnumWindows(enumerate_window, 0):
        raise ctypes.WinError(ctypes.get_last_error())
    if len(windows) != 1:
        raise RuntimeError(f'PID {pid} has {len(windows)} visible windows; select one unambiguous target.')
    hwnd = windows[0]
    if activate:
        user32.ShowWindow(hwnd, 9)  # SW_RESTORE
        user32.SetForegroundWindow(hwnd)
        time.sleep(.6)
    if user32.IsIconic(hwnd) or user32.GetForegroundWindow() != hwnd:
        raise RuntimeError('Target must be restored and foreground; bring it forward or use --activate.')

    rect = W.RECT()
    if dwm.DwmGetWindowAttribute(hwnd, 9, ctypes.byref(rect), ctypes.sizeof(rect)) != 0:
        raise RuntimeError('Cannot read DWM extended frame bounds.')
    bounds = (rect.left, rect.top, rect.right, rect.bottom)
    screen_x, screen_y, screen_w, screen_h = [user32.GetSystemMetrics(i) for i in (76, 77, 78, 79)]
    if not (screen_x <= rect.left < rect.right <= screen_x+screen_w
            and screen_y <= rect.top < rect.bottom <= screen_y+screen_h):
        raise RuntimeError('Window is partly outside the virtual desktop; move it fully onto a display.')

    image = ImageGrab.grab(bbox=bounds, all_screens=True)
    if image.size != (rect.right-rect.left, rect.bottom-rect.top):
        raise RuntimeError('Captured dimensions do not match the native window bounds.')
    # A new file is required: never overwrite an earlier capture accidentally.
    with output.open('xb') as stream:
        try:
            image.save(stream, format='PNG')
        except BaseException:
            stream.close()
            output.unlink()
            raise
    return {
        'file': str(output), 'pid': pid, 'hwnd': hwnd,
        'rectangle': bounds, 'size': image.size,
        'capture_backend': 'Windows GDI via Pillow ImageGrab',
        'sha256': hashlib.sha256(output.read_bytes()).hexdigest(),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid', required=True, type=int, help='Current PID of the target application')
    parser.add_argument('--output', required=True, type=Path, help='New PNG file in an existing directory')
    parser.add_argument('--activate', action='store_true', help='Restore and activate the selected target window')
    args = parser.parse_args()
    if args.pid <= 0:
        parser.error('--pid must be positive')
    output = args.output.resolve()
    if output.suffix.lower() != '.png' or not output.parent.is_dir() or output.exists():
        parser.error('--output must be a new .png file in an existing directory')
    try:
        result = capture(args.pid, output, args.activate)
    except (RuntimeError, OSError) as error:
        parser.exit(1, f'Capture failed: {error}\n')
    print(json.dumps(result, ensure_ascii=False))


if __name__ == '__main__':
    main()

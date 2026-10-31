#!/usr/bin/env python3
"""Capture a disposable packaged GPUI window on an interactive Windows CI desktop.

Run from the repository root with --exe pointing at the extracted package's
workbuddy-proxy.exe and --output-dir pointing at a dedicated artifact directory.
No system DPI settings are changed. Captures are raw GDI/Pillow PNGs from
windows_screenshot.py. A blocked desktop or unsafe cleanup is reported as failure.
"""
import argparse
import ctypes
from ctypes import wintypes as W
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
HARNESS = ROOT / 'scripts' / 'gpui_package_test.py'
CAPTURE = ROOT / 'scripts' / 'windows_screenshot.py'
REPARSE = 0x400


# Fixed-width types and offsets follow the public Windows DEVMODEW layout:
# https://learn.microsoft.com/windows/win32/api/wingdi/ns-wingdi-devmodew
# https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-enumdisplaysettingsexw
# https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-changedisplaysettingsexw
class DEVMODEW(ctypes.Structure):
    _fields_ = [('device_name', ctypes.c_uint16 * 32), ('spec_version', ctypes.c_uint16),
                ('driver_version', ctypes.c_uint16), ('size', ctypes.c_uint16),
                ('driver_extra', ctypes.c_uint16), ('fields', ctypes.c_uint32),
                ('position_x', ctypes.c_int32), ('position_y', ctypes.c_int32),
                ('orientation', ctypes.c_uint32), ('fixed_output', ctypes.c_uint32),
                ('color', ctypes.c_int16), ('duplex', ctypes.c_int16),
                ('y_resolution', ctypes.c_int16), ('tt_option', ctypes.c_int16),
                ('collate', ctypes.c_int16), ('form_name', ctypes.c_uint16 * 32),
                ('log_pixels', ctypes.c_uint16), ('bpp', ctypes.c_uint32),
                ('width', ctypes.c_uint32), ('height', ctypes.c_uint32),
                ('display_flags', ctypes.c_uint32), ('frequency', ctypes.c_uint32),
                ('icm_method', ctypes.c_uint32), ('icm_intent', ctypes.c_uint32),
                ('media_type', ctypes.c_uint32), ('dither_type', ctypes.c_uint32),
                ('reserved1', ctypes.c_uint32), ('reserved2', ctypes.c_uint32),
                ('panning_width', ctypes.c_uint32), ('panning_height', ctypes.c_uint32)]


def validate_devmode_layout():
    expected = {'size': 68, 'driver_extra': 70, 'fields': 72, 'position_x': 76,
                'orientation': 84, 'log_pixels': 166, 'bpp': 168, 'width': 172,
                'height': 176, 'display_flags': 180, 'frequency': 184}
    if ctypes.sizeof(DEVMODEW) != 220 or any(
            getattr(DEVMODEW, name).offset != offset for name, offset in expected.items()):
        raise RuntimeError('Unexpected DEVMODEW ABI layout; refusing display changes')


def mode_summary(mode):
    return {'width': mode.width, 'height': mode.height, 'bpp': mode.bpp,
            'orientation': mode.orientation if mode.fields & 0x80 else None, 'frequency': mode.frequency,
            'position': [mode.position_x, mode.position_y], 'fields': mode.fields,
            'display_flags': mode.display_flags, 'driver_extra': mode.driver_extra,
            'public_devmode_hex': bytes(mode).hex()}


def same_mode(a, b):
    required = 0x5c0080  # bpp, width, height, frequency, orientation
    return (a.fields & required == required and b.fields & required == required and
            all(getattr(a, k) == getattr(b, k) for k in
               ('width', 'height', 'bpp', 'orientation', 'frequency'))
            )


def display_environment(user):
    """Return the sole monitor and its device name, geometry and effective DPI."""
    class MONITORINFOEXW(ctypes.Structure):
        _fields_ = [('size', W.DWORD), ('monitor', W.RECT), ('work', W.RECT),
                    ('flags', W.DWORD), ('device', W.WCHAR * 32)]
    callback = ctypes.WINFUNCTYPE(W.BOOL, W.HANDLE, W.HANDLE, ctypes.POINTER(W.RECT), W.LPARAM)
    monitors = []
    @callback
    def visit(handle, _dc, _rect, _data):
        monitors.append(handle)
        return True
    user.EnumDisplayMonitors.argtypes = [W.HANDLE, ctypes.c_void_p, callback, W.LPARAM]
    user.EnumDisplayMonitors.restype = W.BOOL
    if not user.EnumDisplayMonitors(None, None, visit, 0):
        raise ctypes.WinError(ctypes.get_last_error())
    if len(monitors) != 1:
        raise RuntimeError(f'CI display mode changes require one monitor, found {len(monitors)}')
    info = MONITORINFOEXW(); info.size = ctypes.sizeof(info)
    if not user.GetMonitorInfoW(monitors[0], ctypes.byref(info)):
        raise ctypes.WinError(ctypes.get_last_error())
    shcore = ctypes.WinDLL('shcore', use_last_error=True)
    shcore.GetDpiForMonitor.argtypes = [W.HANDLE, ctypes.c_int, ctypes.POINTER(W.UINT), ctypes.POINTER(W.UINT)]
    shcore.GetDpiForMonitor.restype = ctypes.c_long
    x = W.UINT(); y = W.UINT()
    result = shcore.GetDpiForMonitor(monitors[0], 0, ctypes.byref(x), ctypes.byref(y))
    if result != 0 or x.value < 96 or x.value > 384 or x.value != y.value:
        raise RuntimeError(f'Unexpected monitor DPI or GetDpiForMonitor failure: {result}, {x.value}x{y.value}')
    return {'device': info.device, 'monitor': rect_tuple(info.monitor),
            'work_area': rect_tuple(info.work), 'dpi': [x.value, y.value]}


def get_mode(user, device, index):
    mode = DEVMODEW(); mode.size = ctypes.sizeof(mode); mode.driver_extra = 0
    # No RAWMODE or ROTATEDMODE: only monitor-compatible, same-orientation modes.
    if not user.EnumDisplaySettingsExW(device, index, ctypes.byref(mode), 0):
        return None
    if mode.size != ctypes.sizeof(mode) or mode.driver_extra != 0:
        raise RuntimeError('Display mode has unsupported size or private driver data')
    return mode


def display_init(user, report):
    validate_devmode_layout()
    user.EnumDisplaySettingsExW.argtypes = [W.LPCWSTR, W.DWORD, ctypes.POINTER(DEVMODEW), W.DWORD]
    user.EnumDisplaySettingsExW.restype = W.BOOL
    user.ChangeDisplaySettingsExW.argtypes = [W.LPCWSTR, ctypes.POINTER(DEVMODEW), W.HWND, W.DWORD, ctypes.c_void_p]
    user.ChangeDisplaySettingsExW.restype = W.LONG
    env = display_environment(user)
    report['original_environment'] = env
    original = get_mode(user, env['device'], 0xffffffff)  # ENUM_CURRENT_SETTINGS
    if original is None:
        raise RuntimeError('Cannot query current display mode')
    report.update({'original_mode': mode_summary(original), 'available_modes': [],
                   'available_matching_modes': [], 'test': 'not-run', 'apply': 'not-run',
                   'restore': 'not-needed'})
    modes = []
    for index in range(10000):
        mode = get_mode(user, env['device'], index)
        if mode is None:
            break
        modes.append(mode)
        report['available_modes'].append({'index': index, 'width': mode.width,
                                          'height': mode.height, 'bpp': mode.bpp,
                                          'orientation': mode.orientation if mode.fields & 0x80 else None,
                                          'frequency': mode.frequency, 'fields': mode.fields})
    else:
        raise RuntimeError('Display mode enumeration did not terminate within 10000 modes')
    # A missing orientation field cannot be safely assumed to be the default.
    if original.fields & 0x5c0080 != 0x5c0080 or original.bpp < 32 or original.width <= 0 or original.height <= 0:
        raise RuntimeError('Current display mode lacks explicit bpp, dimensions, frequency or orientation')
    bounds = env['monitor']
    if (bounds[2]-bounds[0], bounds[3]-bounds[1]) != (original.width, original.height):
        raise RuntimeError('Current display mode dimensions do not match the sole monitor bounds')
    return env, original, modes


def display_prepare(user, hwnd, env, original, modes, report):
    """Switch only when measured window and requested client cannot fit work area."""
    check_input_desktop(user)
    current_env = display_environment(user)
    current_mode = get_mode(user, env['device'], 0xffffffff)
    if current_env != env or current_mode is None or not same_mode(current_mode, original):
        raise RuntimeError('Display environment changed since the original observation')
    dpi = user.GetDpiForWindow(hwnd)
    client = W.RECT(); outer = W.RECT()
    if dpi != env['dpi'][0] or not user.GetClientRect(hwnd, ctypes.byref(client)) or not user.GetWindowRect(hwnd, ctypes.byref(outer)):
        raise RuntimeError('Cannot establish matching window DPI and current geometry')
    inset = [outer.right-outer.left-client.right+client.left, outer.bottom-outer.top-client.bottom+client.top]
    if any(n < 0 or n > 128*dpi/96 for n in inset):
        raise RuntimeError(f'Unexpected measured nonclient dimensions: {inset}')
    requested = [round(1440*dpi/96)+inset[0], round(900*dpi/96)+inset[1]]
    work = env['work_area']
    report['prechange_required_outer_physical'] = requested
    if requested[0] <= work[2]-work[0] and requested[1] <= work[3]-work[1]:
        report['apply'] = 'not-needed: measured window fits work area'
        return False
    candidates = [mode for mode in modes
                  if mode.fields & 0x5c0000 == 0x5c0000
                  and (mode.width, mode.height, mode.bpp) == (1920, 1080, original.bpp)
                  and (not mode.fields & 0x80 or mode.orientation == original.orientation)]
    report['available_matching_modes'] = [mode_summary(m) for m in candidates]
    if not candidates:
        raise RuntimeError('Exact 1920x1080 mode with original bpp and orientation unavailable')
    candidates.sort(key=lambda mode: mode.frequency != original.frequency)
    selected = candidates[0]
    report['selected_mode'] = mode_summary(selected)
    report['selected_orientation_basis'] = ('explicit DEVMODE field' if selected.fields & 0x80
                                            else 'EnumDisplaySettingsExW flags=0 filters to current orientation')
    result = user.ChangeDisplaySettingsExW(env['device'], ctypes.byref(selected), None, 0x2, None)  # CDS_TEST
    report['test'] = result
    if result != 0:
        raise RuntimeError(f'CDS_TEST did not return DISP_CHANGE_SUCCESS: {result}')
    check_input_desktop(user)
    pre_apply = get_mode(user, env['device'], 0xffffffff)
    if display_environment(user) != env or pre_apply is None or not same_mode(pre_apply, original):
        raise RuntimeError('Display environment changed after CDS_TEST')
    # Set before invoking the API: even an exception or failure must attempt restoration.
    report['restore'] = 'required'
    result = user.ChangeDisplaySettingsExW(env['device'], ctypes.byref(selected), None, 0x4, None)  # CDS_FULLSCREEN
    report['apply'] = result
    if result != 0:
        raise RuntimeError(f'CDS_FULLSCREEN did not return DISP_CHANGE_SUCCESS: {result}')
    actual = get_mode(user, env['device'], 0xffffffff)
    current_env = display_environment(user)
    report['actual_mode'] = mode_summary(actual) if actual else None
    report['actual_environment'] = current_env
    if (actual is None or actual.fields & 0x5c0080 != 0x5c0080 or
            (actual.width, actual.height, actual.bpp, actual.orientation, actual.frequency) !=
            (selected.width, selected.height, selected.bpp, original.orientation, selected.frequency) or
            current_env['device'] != env['device'] or current_env['dpi'] != env['dpi']):
        raise RuntimeError('Applied display mode, device or DPI differs from requested values')
    return True


def display_restore(user, env, original, report):
    result = user.ChangeDisplaySettingsExW(env['device'], ctypes.byref(original), None, 0x4, None)
    report['restore'] = {'return_code': result}
    actual = get_mode(user, env['device'], 0xffffffff)
    report['restore']['actual_mode'] = mode_summary(actual) if actual else None
    report['restore']['environment'] = display_environment(user)
    if result != 0 or actual is None or not same_mode(original, actual):
        raise RuntimeError('Original display mode restoration failed or did not verify')
    restored_env = report['restore']['environment']
    if restored_env['device'] != env['device'] or restored_env['dpi'] != env['dpi'] or restored_env['monitor'] != env['monitor'] or restored_env['work_area'] != env['work_area']:
        raise RuntimeError('Original display environment did not restore exactly')
    report['restore']['verified'] = True


def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def no_links(path):
    """Reject symlinks/junctions in every existing component, not string prefixes."""
    path = Path(os.path.abspath(path))
    for component in reversed((path, *path.parents)):
        if not os.path.lexists(component):
            continue
        st = os.lstat(component)
        if component.is_symlink() or getattr(st, 'st_file_attributes', 0) & REPARSE:
            raise RuntimeError(f'reparse point or symlink in path: {component}')
    return path


def tree_files(root):
    """Enumerate only regular files/dirs with no links, junctions or mount changes."""
    root = no_links(root)
    root_dev = os.stat(root).st_dev
    files = []
    for base, dirs, names in os.walk(root, followlinks=False):
        for name in dirs + names:
            item = Path(base) / name
            st = os.lstat(item)
            if item.is_symlink() or getattr(st, 'st_file_attributes', 0) & REPARSE:
                raise RuntimeError(f'reparse point or symlink in tree: {item}')
            if st.st_dev != root_dev or not (item.is_dir() or item.is_file()):
                raise RuntimeError(f'mount or unexpected object in tree: {item}')
            if item.is_file():
                files.append(item)
    return files


def package_manifest(root):
    result = {p.relative_to(root).as_posix(): sha(p) for p in sorted(tree_files(root))}
    encoded = json.dumps(result, sort_keys=True, separators=(',', ':')).encode()
    return result, hashlib.sha256(encoded).hexdigest()


def current_owner_matches(path):
    """Accept the process token user or its configured default owner SID."""
    adv = ctypes.WinDLL('advapi32', use_last_error=True)
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    adv.GetNamedSecurityInfoW.argtypes = [W.LPWSTR, W.DWORD, W.DWORD,
        ctypes.POINTER(ctypes.c_void_p), ctypes.c_void_p, ctypes.c_void_p,
        ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)]
    adv.GetNamedSecurityInfoW.restype = W.DWORD
    adv.OpenProcessToken.argtypes = [W.HANDLE, W.DWORD, ctypes.POINTER(W.HANDLE)]
    adv.GetTokenInformation.argtypes = [W.HANDLE, W.DWORD, ctypes.c_void_p,
                                         W.DWORD, ctypes.POINTER(W.DWORD)]
    adv.EqualSid.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
    kernel.GetCurrentProcess.restype = W.HANDLE
    kernel.CloseHandle.argtypes = [W.HANDLE]
    kernel.LocalFree.argtypes = [ctypes.c_void_p]
    owner = ctypes.c_void_p(); sd = ctypes.c_void_p(); token = W.HANDLE()
    error = adv.GetNamedSecurityInfoW(str(path), 1, 1, ctypes.byref(owner),
                                      None, None, None, ctypes.byref(sd))
    if error:
        raise OSError(error, f'GetNamedSecurityInfoW failed: {path}')
    try:
        if not adv.OpenProcessToken(kernel.GetCurrentProcess(), 0x0008, ctypes.byref(token)):
            raise ctypes.WinError(ctypes.get_last_error())
        for info_class in (1, 4):  # TokenUser and TokenOwner start with a SID pointer.
            needed = W.DWORD()
            adv.GetTokenInformation(token, info_class, None, 0, ctypes.byref(needed))
            if not needed.value:
                raise ctypes.WinError(ctypes.get_last_error())
            data = ctypes.create_string_buffer(needed.value)
            if not adv.GetTokenInformation(token, info_class, data, needed, ctypes.byref(needed)):
                raise ctypes.WinError(ctypes.get_last_error())
            token_sid = ctypes.cast(data, ctypes.POINTER(ctypes.c_void_p))[0]
            if adv.EqualSid(owner, token_sid):
                return True
        return False
    finally:
        if token:
            kernel.CloseHandle(token)
        if sd:
            kernel.LocalFree(sd)


def process_image(pid):
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel.OpenProcess.argtypes = [W.DWORD, W.BOOL, W.DWORD]
    kernel.OpenProcess.restype = W.HANDLE
    kernel.QueryFullProcessImageNameW.argtypes = [W.HANDLE, W.DWORD, W.LPWSTR, ctypes.POINTER(W.DWORD)]
    kernel.CloseHandle.argtypes = [W.HANDLE]
    handle = kernel.OpenProcess(0x1000, False, pid)
    if not handle:
        raise RuntimeError(f'Cannot query process {pid}: {ctypes.get_last_error()}')
    try:
        size = W.DWORD(32768); buf = ctypes.create_unicode_buffer(size.value)
        if not kernel.QueryFullProcessImageNameW(handle, 0, buf, ctypes.byref(size)):
            raise ctypes.WinError(ctypes.get_last_error())
        return Path(buf.value).resolve()
    finally:
        kernel.CloseHandle(handle)


def pid_alive(pid):
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel.OpenProcess.argtypes = [W.DWORD, W.BOOL, W.DWORD]
    kernel.OpenProcess.restype = W.HANDLE
    kernel.WaitForSingleObject.argtypes = [W.HANDLE, W.DWORD]
    kernel.CloseHandle.argtypes = [W.HANDLE]
    handle = kernel.OpenProcess(0x100000, False, pid)
    if not handle:
        error = ctypes.get_last_error()
        if error == 87:  # ERROR_INVALID_PARAMETER: no such process
            return False
        raise RuntimeError(f'Cannot establish whether PID {pid} is alive: {error}')
    try:
        result = kernel.WaitForSingleObject(handle, 0)
        if result not in (0, 258):
            raise RuntimeError(f'Cannot wait on PID {pid}: {result}')
        return result == 258
    finally:
        kernel.CloseHandle(handle)


def package_processes(package):
    """Conservative process scan: uncertainty prevents deleting the fixture."""
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    class PROCESSENTRY32W(ctypes.Structure):
        _fields_ = [('dwSize', W.DWORD), ('cntUsage', W.DWORD), ('th32ProcessID', W.DWORD),
                    ('th32DefaultHeapID', ctypes.c_size_t), ('th32ModuleID', W.DWORD),
                    ('cntThreads', W.DWORD), ('th32ParentProcessID', W.DWORD),
                    ('pcPriClassBase', W.LONG), ('dwFlags', W.DWORD), ('szExeFile', W.WCHAR * 260)]
    kernel.CreateToolhelp32Snapshot.argtypes = [W.DWORD, W.DWORD]
    kernel.CreateToolhelp32Snapshot.restype = W.HANDLE
    kernel.Process32FirstW.argtypes = [W.HANDLE, ctypes.POINTER(PROCESSENTRY32W)]
    kernel.Process32NextW.argtypes = [W.HANDLE, ctypes.POINTER(PROCESSENTRY32W)]
    kernel.CloseHandle.argtypes = [W.HANDLE]
    snap = kernel.CreateToolhelp32Snapshot(2, 0)
    if snap == ctypes.c_void_p(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    matches = []
    try:
        entry = PROCESSENTRY32W(); entry.dwSize = ctypes.sizeof(entry)
        ok = kernel.Process32FirstW(snap, ctypes.byref(entry))
        if not ok:
            raise ctypes.WinError(ctypes.get_last_error())
        names = {'workbuddy-proxy.exe', 'workbuddy-settings.exe', 'workbuddy-proxy-gpui.exe'}
        while ok:
            if entry.szExeFile.lower() in names:
                try:
                    image = process_image(entry.th32ProcessID)
                except RuntimeError:
                    if pid_alive(entry.th32ProcessID):
                        raise
                else:
                    if image.is_relative_to(package.resolve()):
                        matches.append({'pid': entry.th32ProcessID, 'image': str(image)})
            ok = kernel.Process32NextW(snap, ctypes.byref(entry))
        if ctypes.get_last_error() != 18:  # ERROR_NO_MORE_FILES
            raise ctypes.WinError(ctypes.get_last_error())
    finally:
        kernel.CloseHandle(snap)
    return matches


def check_input_desktop(user):
    user.OpenInputDesktop.argtypes = [W.DWORD, W.BOOL, W.DWORD]
    user.OpenInputDesktop.restype = W.HANDLE
    user.CloseDesktop.argtypes = [W.HANDLE]
    user.GetUserObjectInformationW.argtypes = [W.HANDLE, ctypes.c_int, ctypes.c_void_p,
                                               W.DWORD, ctypes.POINTER(W.DWORD)]
    desktop = user.OpenInputDesktop(0, False, 0x0001)
    if not desktop:
        raise RuntimeError('Interactive input desktop unavailable or locked')
    try:
        name = ctypes.create_unicode_buffer(256); needed = W.DWORD()
        if not user.GetUserObjectInformationW(desktop, 2, name, ctypes.sizeof(name), ctypes.byref(needed)):
            raise ctypes.WinError(ctypes.get_last_error())
        if name.value.lower() != 'default':
            raise RuntimeError(f'Input desktop is not Default: {name.value!r}')
    finally:
        user.CloseDesktop(desktop)


def windows_api():
    user = ctypes.WinDLL('user32', use_last_error=True)
    dwm = ctypes.WinDLL('dwmapi', use_last_error=True)
    user.SetProcessDpiAwarenessContext.argtypes = [ctypes.c_void_p]
    user.SetProcessDpiAwarenessContext.restype = W.BOOL
    if not user.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4)):
        raise RuntimeError('Cannot establish per-monitor-v2 DPI awareness in fresh process')
    check_input_desktop(user)
    user.GetWindowThreadProcessId.argtypes = [W.HWND, ctypes.POINTER(W.DWORD)]
    user.IsWindowVisible.argtypes = [W.HWND]; user.IsIconic.argtypes = [W.HWND]
    user.GetWindowRect.argtypes = [W.HWND, ctypes.POINTER(W.RECT)]
    user.GetClientRect.argtypes = [W.HWND, ctypes.POINTER(W.RECT)]
    user.ClientToScreen.argtypes = [W.HWND, ctypes.POINTER(W.POINT)]
    user.GetForegroundWindow.restype = W.HWND
    user.GetDpiForWindow.argtypes = [W.HWND]; user.GetDpiForWindow.restype = W.UINT
    user.MonitorFromWindow.argtypes = [W.HWND, W.DWORD]; user.MonitorFromWindow.restype = W.HANDLE
    user.GetMonitorInfoW.argtypes = [W.HANDLE, ctypes.c_void_p]
    user.SetWindowPos.argtypes = [W.HWND, W.HWND, ctypes.c_int, ctypes.c_int,
                                  ctypes.c_int, ctypes.c_int, W.UINT]
    user.ShowWindow.argtypes = [W.HWND, ctypes.c_int]
    user.SetForegroundWindow.argtypes = [W.HWND]
    dwm.DwmGetWindowAttribute.argtypes = [W.HWND, W.DWORD, ctypes.c_void_p, W.DWORD]
    dwm.DwmGetWindowAttribute.restype = ctypes.c_long
    return user, dwm


def visible_window(user, pid):
    callback = ctypes.WINFUNCTYPE(W.BOOL, W.HWND, W.LPARAM)
    found = []
    @callback
    def visit(hwnd, _):
        owner = W.DWORD(); rect = W.RECT()
        user.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if (owner.value == pid and user.IsWindowVisible(hwnd) and not user.IsIconic(hwnd)
                and user.GetWindowRect(hwnd, ctypes.byref(rect))
                and rect.right > rect.left and rect.bottom > rect.top):
            found.append(hwnd)
        return True
    user.EnumWindows.argtypes = [callback, W.LPARAM]
    if not user.EnumWindows(visit, 0):
        raise ctypes.WinError(ctypes.get_last_error())
    if len(found) != 1:
        raise RuntimeError(f'Expected exactly one visible unminimized UI window for PID {pid}, found {len(found)}')
    return found[0]


def rect_tuple(r):
    return [r.left, r.top, r.right, r.bottom]


def assert_unobscured(user, dwm, target, frame):
    """Reject any visible, uncloaked top-level window overlapping the frame."""
    callback = ctypes.WINFUNCTYPE(W.BOOL, W.HWND, W.LPARAM)
    before = []
    found_target = False
    @callback
    def visit(hwnd, _):
        nonlocal found_target
        if hwnd == target:
            found_target = True
            return False
        before.append(hwnd)
        return True
    user.EnumWindows.argtypes = [callback, W.LPARAM]
    user.EnumWindows(visit, 0)  # callback deliberately stops at the target
    if not found_target:
        raise RuntimeError('Target not found in desktop Z order')
    for hwnd in before:
        if not user.IsWindowVisible(hwnd) or user.IsIconic(hwnd):
            continue
        cloaked = W.DWORD()
        if dwm.DwmGetWindowAttribute(hwnd, 14, ctypes.byref(cloaked), ctypes.sizeof(cloaked)) != 0:
            raise RuntimeError('Cannot determine whether a window above target is cloaked')
        if cloaked.value:
            continue
        rect = W.RECT()
        if not user.GetWindowRect(hwnd, ctypes.byref(rect)):
            raise ctypes.WinError(ctypes.get_last_error())
        if (rect.left < frame.right and rect.right > frame.left and
                rect.top < frame.bottom and rect.bottom > frame.top):
            raise RuntimeError(f'Visible window {hwnd} overlaps the target; capture blocked')


def configure_window(user, dwm, hwnd, logical, attempt):
    class MONITORINFO(ctypes.Structure):
        _fields_ = [('cbSize', W.DWORD), ('rcMonitor', W.RECT), ('rcWork', W.RECT), ('dwFlags', W.DWORD)]
    user.ShowWindow(hwnd, 9)  # restore only the unique owned test window
    time.sleep(.2)
    dpi = user.GetDpiForWindow(hwnd)
    attempt['dpi'] = dpi
    if dpi < 96 or dpi > 384:
        raise RuntimeError(f'Unexpected window DPI {dpi}')
    target = [round(n * dpi / 96) for n in logical]
    attempt['requested_physical_client'] = target
    before_client = W.RECT(); before_window = W.RECT()
    if (not user.GetClientRect(hwnd, ctypes.byref(before_client)) or
            not user.GetWindowRect(hwnd, ctypes.byref(before_window))):
        raise ctypes.WinError(ctypes.get_last_error())
    # GPUI handles WM_NCCALCSIZE for custom chrome, so the standard
    # AdjustWindowRectExForDpi caption formula would oversize the client.
    # Measure this restored window's actual nonclient area instead.
    insets = [before_window.right - before_window.left - before_client.right + before_client.left,
              before_window.bottom - before_window.top - before_client.bottom + before_client.top]
    if any(n < 0 or n > 128 * dpi / 96 for n in insets):
        raise RuntimeError(f'Unexpected measured nonclient dimensions: {insets}')
    width, height = [n + inset for n, inset in zip(target, insets)]
    mon = MONITORINFO(); mon.cbSize = ctypes.sizeof(mon)
    if not user.GetMonitorInfoW(user.MonitorFromWindow(hwnd, 2), ctypes.byref(mon)):
        raise ctypes.WinError(ctypes.get_last_error())
    work = mon.rcWork
    attempt.update({'monitor': rect_tuple(mon.rcMonitor), 'monitor_work_area': rect_tuple(work),
                    'measured_nonclient_physical': insets,
                    'requested_outer_physical': [width, height]})
    if width > work.right - work.left or height > work.bottom - work.top:
        raise RuntimeError(f'{logical} logical client at DPI {dpi} cannot fit the monitor work area')
    if not user.SetWindowPos(hwnd, None, work.left, work.top, width, height, 0x0014):
        raise ctypes.WinError(ctypes.get_last_error())  # NOZORDER | NOACTIVATE
    if not user.SetForegroundWindow(hwnd):
        raise RuntimeError('Windows denied foreground access for the isolated test window')
    time.sleep(.8)
    if user.GetForegroundWindow() != hwnd or user.IsIconic(hwnd):
        raise RuntimeError('Isolated window is not foreground and restored')
    if user.GetDpiForWindow(hwnd) != dpi:
        raise RuntimeError('Window DPI changed during resize; refusing ambiguous capture')
    client = W.RECT(); outer = W.RECT(); frame = W.RECT(); origin = W.POINT(0, 0)
    if not user.GetClientRect(hwnd, ctypes.byref(client)) or not user.GetWindowRect(hwnd, ctypes.byref(outer)):
        raise ctypes.WinError(ctypes.get_last_error())
    if not user.ClientToScreen(hwnd, ctypes.byref(origin)):
        raise ctypes.WinError(ctypes.get_last_error())
    if dwm.DwmGetWindowAttribute(hwnd, 9, ctypes.byref(frame), ctypes.sizeof(frame)) != 0:
        raise RuntimeError('DWM frame bounds unavailable')
    actual = [client.right-client.left, client.bottom-client.top]
    if any(abs(a-b) > 1 for a, b in zip(actual, target)):
        raise RuntimeError(f'Client dimensions {actual} differ from requested physical {target}')
    if not (mon.rcMonitor.left <= frame.left < frame.right <= mon.rcMonitor.right and
            mon.rcMonitor.top <= frame.top < frame.bottom <= mon.rcMonitor.bottom):
        raise RuntimeError('DWM frame is not fully inside one monitor')
    assert_unobscured(user, dwm, hwnd, frame)
    return {'requested_logical_client': list(logical), 'dpi': dpi,
            'measured_nonclient_physical': insets,
            'requested_physical_client': target, 'actual_physical_client': actual,
            'actual_logical_client': [round(n*96/dpi, 3) for n in actual],
            'client_origin_screen': [origin.x, origin.y], 'window_rect': rect_tuple(outer),
            'dwm_frame': rect_tuple(frame), 'monitor': rect_tuple(mon.rcMonitor),
            'monitor_work_area': rect_tuple(work)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True, help='Extracted package workbuddy-proxy.exe')
    parser.add_argument('--output-dir', type=Path, required=True, help='Dedicated output directory for new evidence')
    parser.add_argument('--ci-display-size', choices=['1920x1080'],
                        help='Allow a temporary 1920x1080 mode only on a single-monitor GitHub Actions Windows runner')
    args = parser.parse_args()
    output = no_links(args.output_dir)
    output.mkdir(parents=True, exist_ok=True)
    no_links(output)
    report_path = output / 'windows-ci-screenshot.json'
    targets = [report_path, output / 'windows-ci-1440x900.png', output / 'windows-ci-1100x700.png',
               output / 'windows-ci-lifecycle.json']
    if any(p.exists() for p in targets):
        parser.error('Evidence files must be new; use a dedicated empty output directory')
    report = {'result': 'failed', 'capture_backend': 'Windows GDI via existing windows_screenshot.py',
              'captures': [], 'helper_attempts': [], 'window_attempts': [],
              'display': {'requested': args.ci_display_size, 'change_policy': 'disabled by default'},
              'hold_mode_limit': 'Hold skips harness non-hold log-clear and selected-model diagnostic branch.',
              'cleanup': {'status': 'not-started'}}
    session = None; harness = None; known = []; marker = uuid.uuid4().hex
    failure = None; display_env = None; original_mode = None; display_modes = None; user = None
    try:
        if sys.platform != 'win32':
            raise RuntimeError('blocked: this helper requires an interactive Windows desktop')
        user, dwm = windows_api()
        if args.ci_display_size:
            if os.environ.get('GITHUB_ACTIONS', '').lower() != 'true' or os.environ.get('RUNNER_OS') != 'Windows':
                raise RuntimeError('Display mode changes are restricted to GitHub Actions Windows runners')
            display_env, original_mode, display_modes = display_init(user, report['display'])
        exe = no_links(args.exe)
        if exe.name.lower() != 'workbuddy-proxy.exe' or not exe.is_file():
            raise RuntimeError('--exe must name the extracted package workbuddy-proxy.exe')
        source = exe.parent
        ui = source / 'internal' / 'workbuddy-settings.exe'
        if not ui.is_file():
            raise RuntimeError('Packaged internal/workbuddy-settings.exe is missing')
        if output == source or output.is_relative_to(source):
            raise RuntimeError('Output directory cannot be inside the source package')
        manifest, package_hash = package_manifest(source)
        temp_base = no_links(Path(tempfile.gettempdir()))
        session = Path(tempfile.mkdtemp(prefix='workbuddy-ci-capture-', dir=temp_base))
        no_links(session)
        if session.parent.resolve() != temp_base.resolve() or not current_owner_matches(session):
            raise RuntimeError('Cannot verify temporary directory ownership or boundary')
        (session / '.capture-owner').write_text(marker, encoding='utf-8')
        package = session / 'package'
        # Preserve rather than follow a raced-in link; the post-copy audit rejects it.
        shutil.copytree(source, package, symlinks=True)
        copied, copied_hash = package_manifest(package)
        if manifest != copied or package_hash != copied_hash:
            raise RuntimeError('Copied package does not match source manifest')
        copied_exe = package / exe.name; copied_ui = package / 'internal' / 'workbuddy-settings.exe'
        report.update({'package_manifest_sha256': package_hash, 'package_files_sha256': manifest,
                       'core_sha256': sha(copied_exe), 'ui_sha256': sha(copied_ui),
                       'capture_helper_sha256': sha(CAPTURE), 'harness_sha256': sha(HARNESS),
                       'temporary_directory': str(session), 'source_exe': str(exe)})
        fixture = session / 'fixture'; fixture.mkdir()
        log_path = session / 'harness.log'
        with log_path.open('wb') as log:
            harness = subprocess.Popen([sys.executable, str(HARNESS), '--exe', str(copied_exe),
                '--output', str(targets[3]), '--directory', str(fixture), '--hold'],
                cwd=ROOT, stdout=log, stderr=subprocess.STDOUT)
        report['harness_pid'] = harness.pid
        deadline = time.monotonic() + 90
        info = None
        while time.monotonic() < deadline:
            if harness.poll() is not None:
                raise RuntimeError(f'Package harness exited before hold: {harness.returncode}')
            try:
                info = json.loads((fixture / 'qa-info.json').read_text(encoding='utf-8'))
                if 'READY:' in log_path.read_text(encoding='utf-8', errors='replace'):
                    break
            except (OSError, ValueError):
                pass
            time.sleep(.2)
        else:
            raise RuntimeError('Timed out waiting for the explicit harness hold marker')
        if Path(info['directory']).resolve() != fixture.resolve() or Path(info['exe']).resolve() != copied_exe.resolve():
            raise RuntimeError('Hold marker does not belong to this fixture/package')
        known = [int(info[k]) for k in ('host_pid', 'core_pid', 'ui_pid')]
        if len(set(known + [harness.pid])) != 4 or min(known) <= 0:
            raise RuntimeError('Hold marker process IDs are invalid or not distinct')
        if process_image(known[0]) != copied_exe.resolve() or process_image(known[1]) != copied_exe.resolve() or process_image(known[2]) != copied_ui.resolve():
            raise RuntimeError('Hold marker PID image paths do not match the copied binaries')
        report.update({'host_pid': known[0], 'core_pid': known[1], 'ui_pid': known[2],
                       'hold_marker': 'READY plus qa-info.json verified'})
        for logical, target in [((1440, 900), targets[1]), ((1100, 700), targets[2])]:
            hwnd = visible_window(user, known[2])
            window_attempt = {'requested_logical_client': list(logical)}
            report['window_attempts'].append(window_attempt)
            if logical == (1440, 900) and original_mode is not None:
                window_attempt.update({'dpi': display_env['dpi'][0],
                                       'monitor': display_env['monitor'],
                                       'monitor_work_area': display_env['work_area']})
                display_prepare(user, hwnd, display_env, original_mode, display_modes, report['display'])
            geometry = configure_window(user, dwm, hwnd, logical, window_attempt)
            # The existing helper establishes DPI awareness in a fresh process.
            helper = subprocess.Popen([sys.executable, str(CAPTURE), '--pid', str(known[2]),
                                       '--output', str(target)], cwd=ROOT,
                                      stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            attempt = {'pid': helper.pid, 'output': str(target), 'requested_logical_client': list(logical)}
            report['helper_attempts'].append(attempt)
            try:
                stdout, stderr = helper.communicate(timeout=30)
            except subprocess.TimeoutExpired:
                helper.terminate()  # only the helper subprocess created immediately above
                try:
                    helper.communicate(timeout=10)
                except subprocess.TimeoutExpired:
                    helper.kill()
                    helper.communicate()
                attempt['result'] = 'timeout'
                raise RuntimeError(f'Capture helper PID {helper.pid} timed out')
            attempt['returncode'] = helper.returncode
            if helper.returncode:
                raise RuntimeError(f'Capture helper PID {helper.pid} failed: {stderr.strip()}')
            result = json.loads(stdout)
            if result['pid'] != known[2] or result['hwnd'] != hwnd or result['rectangle'] != geometry['dwm_frame']:
                raise RuntimeError('Capture PID/window/frame mismatch with resize observation')
            if user.GetForegroundWindow() != hwnd:
                raise RuntimeError('Isolated window lost foreground during capture')
            frame = W.RECT(*geometry['dwm_frame'])
            assert_unobscured(user, dwm, hwnd, frame)
            from PIL import Image
            with Image.open(target) as im:
                im.load()
                if im.format != 'PNG' or im.size != tuple(result['size']) or not target.stat().st_size:
                    raise RuntimeError('Captured PNG is empty or has inconsistent dimensions')
                if sha(target) != result['sha256']:
                    raise RuntimeError('Captured PNG hash does not match helper report')
                extrema = im.convert('RGB').getextrema()
                if all(low == high for low, high in extrema):
                    raise RuntimeError('Captured image is uniform; desktop may be unavailable')
            report['captures'].append({'helper_pid': helper.pid, 'geometry': geometry, **result})
        report['result'] = 'captures-complete'
    except Exception as error:
        failure = f'{type(error).__name__}: {error}'
        report['error'] = failure
    finally:
        if session and harness:
            try:
                (session / 'fixture' / 'qa-done').write_text('capture complete', encoding='utf-8')
                report['harness_returncode'] = harness.wait(timeout=180)
                if report['harness_returncode'] != 0:
                    failure = failure or 'Package harness lifecycle assertions failed'
                if targets[3].exists():
                    report['lifecycle_report'] = str(targets[3])
                    lifecycle = json.loads(targets[3].read_text(encoding='utf-8'))
                    if lifecycle.get('result') != 'passed':
                        failure = failure or 'Package harness did not report lifecycle success'
                else:
                    failure = failure or 'Package harness lifecycle report is missing'
            except Exception as error:
                failure = failure or f'Harness release/wait failed: {error}'
        if original_mode is not None and report['display'].get('restore') == 'required':
            try:
                display_restore(user, display_env, original_mode, report['display'])
            except Exception as error:
                report['display']['restore_error'] = f'{type(error).__name__}: {error}'
                failure = (f'{failure}; ' if failure else '') + f'Display restore failed: {error}'
        if session:
            try:
                no_links(session)
                if session.parent.resolve() != Path(tempfile.gettempdir()).resolve():
                    raise RuntimeError('Temporary directory parent changed')
                if (session / '.capture-owner').read_text(encoding='utf-8') != marker:
                    raise RuntimeError('Temporary directory ownership marker changed')
                if not current_owner_matches(session):
                    raise RuntimeError('Temporary directory owner changed')
                entries = tree_files(session)
                for base, dirs, names in os.walk(session, followlinks=False):
                    for name in dirs + names:
                        if not current_owner_matches(Path(base) / name):
                            raise RuntimeError(f'Unexpected owner: {Path(base) / name}')
                if harness and harness.poll() is None:
                    raise RuntimeError('Harness remains running')
                if any(pid_alive(pid) for pid in known):
                    raise RuntimeError('A recorded fixture PID remains alive')
                remaining = package_processes(session / 'package') if (session / 'package').exists() else []
                if remaining:
                    raise RuntimeError(f'Copied package processes remain: {remaining}')
                byte_count = sum(p.stat().st_size for p in entries)
                print(json.dumps({'cleanup_preflight': {
                    'path': str(session), 'type': 'session-owned temporary directory',
                    'project': str(ROOT), 'owner_matches_process': True,
                    'files': len(entries), 'bytes': byte_count,
                    'impact': 'copied package and synthetic fixture only; captured evidence retained',
                    'regenerable_from': str(exe), 'owned_processes_remaining': []
                }}, ensure_ascii=False), flush=True)
                shutil.rmtree(session)  # exact session created above; verified links, owner, PIDs
                report['cleanup'] = {'status': 'removed', 'bytes': byte_count, 'path': str(session)}
            except Exception as error:
                report['cleanup'] = {'status': 'retained', 'path': str(session), 'reason': str(error)}
                failure = failure or f'Safe cleanup blocked: {error}'
        report['result'] = 'passed' if not failure and len(report['captures']) == 2 else 'failed'
        if failure:
            report['error'] = failure
        with report_path.open('x', encoding='utf-8') as stream:
            json.dump(report, stream, ensure_ascii=False, indent=2)
            stream.write('\n')
    print(json.dumps({'result': report['result'], 'report': str(report_path)}, ensure_ascii=False))
    return 0 if report['result'] == 'passed' else 1


if __name__ == '__main__':
    sys.exit(main())

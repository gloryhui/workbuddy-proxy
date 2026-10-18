"""Process RSS is a conservative upper bound, never called per-thread RSS."""
import ctypes
import errno
from functools import lru_cache
import subprocess
import sys
import threading


class _DarwinRusage(ctypes.Structure):
    # sys/resource.h rusage_info_v0. Its user/system times are Mach absolute
    # units; convert using this machine's timebase, never a fixed arm64 ratio.
    _fields_ = [('uuid', ctypes.c_uint8 * 16)] + [
        (name, ctypes.c_uint64) for name in (
            'user', 'system', 'package_wakeups', 'interrupt_wakeups', 'pageins',
            'wired', 'resident', 'footprint', 'start', 'exit')]


@lru_cache(maxsize=1)
def _darwin_api():
    class Timebase(ctypes.Structure):
        _fields_ = [('numer', ctypes.c_uint32), ('denom', ctypes.c_uint32)]

    library = ctypes.CDLL('/usr/lib/libproc.dylib', use_errno=True)
    read = library.proc_pid_rusage
    read.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
    read.restype = ctypes.c_int
    timebase = ctypes.CDLL('/usr/lib/libSystem.B.dylib').mach_timebase_info
    timebase.argtypes = [ctypes.POINTER(Timebase)]
    timebase.restype = ctypes.c_int
    ratio = Timebase()
    if timebase(ctypes.byref(ratio)) != 0 or ratio.denom == 0:
        raise OSError('macOS process timebase unavailable')
    return read, ratio.numer / ratio.denom / 1_000_000_000


def _darwin_usage(pid):
    read, seconds_per_tick = _darwin_api()
    info = _DarwinRusage()
    if read(pid, 0, ctypes.byref(info)) != 0:
        raise OSError(ctypes.get_errno(), 'macOS process resource query failed')
    return info.resident, (info.user + info.system) * seconds_per_tick


def darwin_process_running(pid):
    """No setuid ps execution in a sandbox; exclude exited/zombie processes.

    Permission errors remain errors, never evidence that a process stopped.
    rusage_info_v0 exposes the process exit timestamp even for a zombie.
    """
    read, _ = _darwin_api()
    info = _DarwinRusage()
    if read(pid, 0, ctypes.byref(info)) != 0:
        code = ctypes.get_errno()
        if code == errno.ESRCH:
            return False
        raise OSError(code, 'macOS process state query failed')
    return info.exit == 0


def usage(pid):
    if sys.platform == 'darwin':
        # macOS ps is setuid and cannot exec inside an inherited QA sandbox.
        # libproc measures the same process directly without a privileged tool.
        return _darwin_usage(pid)
    result = subprocess.run(['ps', '-p', str(pid), '-o', 'rss=', '-o', 'time='],
                            capture_output=True, text=True, check=True)
    rss, raw_time = result.stdout.strip().split()
    fields = raw_time.split(':')
    seconds = 0.0
    for field in fields:
        seconds = seconds * 60 + float(field)
    return int(rss) * 1024, seconds


class MemoryMonitor:
    def __init__(self, pid, interval=0.1):
        self.pid = pid
        self.interval = interval
        self.baseline, _ = usage(pid)
        self.peak = self.baseline
        self.samples = 0
        self.error = None
        self.stopped = threading.Event()
        self.thread = threading.Thread(target=self.run, daemon=True)

    def run(self):
        while not self.stopped.wait(self.interval):
            try:
                rss, _ = usage(self.pid)
                self.peak = max(self.peak, rss)
                self.samples += 1
            except (OSError, subprocess.SubprocessError, ValueError) as failure:
                self.error = type(failure).__name__
                return

    def start(self):
        self.thread.start()

    def stop(self):
        self.stopped.set()
        self.thread.join(timeout=2)
        return {'baseline_bytes': self.baseline, 'peak_bytes': self.peak,
                'net_rss_growth_bytes': max(0, self.peak - self.baseline),
                'samples': self.samples, 'error': self.error,
                'sampling_interval_seconds': self.interval,
                'method': 'whole-core peak and net RSS change; not per-thread RSS',
                'sampler': 'libproc rusage_info_v0 with Mach timebase' if sys.platform == 'darwin'
                           else 'ps rss/time'}

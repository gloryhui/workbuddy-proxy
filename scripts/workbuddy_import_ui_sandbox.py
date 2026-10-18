"""macOS-only source/profile protection for synthetic native GUI fixtures.

LaunchServices can start an app without the fixture's CLI arguments. Every QA
entry point must therefore apply this policy, including an extra UI instance.
Product binaries remain byte-identical. The core uses sandbox-exec; optional
GUI bootstrap is a separate diagnostic because CUA cannot currently bind it.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import time


def protected_locations(real_home):
    home = Path(real_home)
    return {'workbuddy': home / '.workbuddy',
            'proxy_macos': home / 'Library/Application Support/WorkBuddyProxy',
            'proxy_unix': home / '.config/workbuddy-proxy',
            'installed_app': Path('/Applications/WorkBuddy Proxy.app')}


def kernel_protection(real_home, allowed_root, pid=None):
    """Query actual process permissions; never open/write a protected file.

    A readable/writable synthetic path is the positive control. An ordinary
    unsandboxed process must fail this attestation. Policy inheritance applies
    to exec descendants, not apps launched through an external native service.
    SPI declarations: WebKit Source/WTF/wtf/spi/darwin/SandboxSPI.h.
    """
    if sys.platform != 'darwin':
        return {'enforced': False, 'reason': 'not_macos'}
    try:
        library = ctypes.CDLL('/usr/lib/libsandbox.1.dylib', use_errno=True)
        check = library.sandbox_check
        # Fixed argtypes are essential for variadic calls on Apple arm64.
        check.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int]
        check.restype = ctypes.c_int
        path_flag = 1 | ctypes.c_int.in_dll(library, 'SANDBOX_CHECK_NO_REPORT').value
        target = os.getpid() if pid is None else pid

        def permission(operation, path):
            return check(target, operation.encode(), path_flag,
                         ctypes.c_char_p(os.fsencode(Path(path).resolve())))

        operations = ('file-read-data', 'file-write-data', 'file-write-create', 'file-write-unlink')
        denied = {label: {kind: [permission(kind, root),
                                  permission(kind, root / '.qa-permission-query-only')]
                           for kind in operations}
                  for label, root in protected_locations(real_home).items()}
        # sandbox_check may deny a nonexistent path simply because pathname
        # resolution failed. The positive control must be an existing fixture
        # directory; nonexistent-child results alone are never a proof.
        allowed = {kind: permission(kind, Path(allowed_root))
                   for kind in operations}
        sandboxed = check(target, None, 0) == 1
        enforced = (sandboxed and all(value > 0 for group in denied.values()
                                     for values in group.values() for value in values)
                    and all(value == 0 for value in allowed.values()))
        return {'enforced': enforced, 'pid': target, 'sandboxed': sandboxed,
                'protected_permissions': denied, 'synthetic_permissions': allowed,
                'protected_root_exists': {label: root.exists()
                                          for label, root in protected_locations(real_home).items()},
                'method': 'sandbox_check permission queries, no protected file I/O'}
    except (OSError, AttributeError, ValueError) as failure:
        return {'enforced': False, 'reason': type(failure).__name__}


def profile_text(protected):
    # Match the attested CLI policy: protected contents remain unreadable and
    # all writes are denied. Metadata is available for guards/native lookup.
    rules = '\n'.join('    (subpath %s)' % json.dumps(str(Path(path).resolve()), ensure_ascii=False)
                      for path in protected)
    return '(version 1)\n(allow default)\n(deny file-read-data file-write*\n%s)\n' % rules


def write_profile(destination, real_home):
    if sys.platform != 'darwin' or not Path('/usr/bin/sandbox-exec').is_file():
        raise RuntimeError('native GUI fixture requires macOS sandbox-exec protection')
    protected = protected_locations(real_home).values()
    path = Path(destination).resolve()
    path.write_text(profile_text(protected))
    return path


def command(profile, executable, arguments=()):
    return ['/usr/bin/sandbox-exec', '-f', str(Path(profile).resolve()),
            str(Path(executable).resolve()), *map(str, arguments)]



def bootstrap_environment(folder, policy, environment):
    """Protect the original GUI Mach-O before main, retaining native app identity."""
    root = Path(folder).resolve()
    library = root / 'source-guard.dylib'
    source = Path(__file__).with_name('workbuddy_import_ui_guard.c')
    subprocess.run(['xcrun', 'clang', '-dynamiclib', '-Wall', '-Wextra', '-Werror',
                    str(source), '-o', str(library)], check=True)
    proof = root / 'source-guard-proofs'; proof.mkdir()
    return dict(environment, DYLD_INSERT_LIBRARIES=str(library),
                WORKBUDDY_PROXY_QA_POLICY=str(Path(policy).resolve()),
                WORKBUDDY_PROXY_QA_PROOF_DIR=str(proof))


def self_check(output):
    """Use synthetic denied files to prove read/write and descendant isolation."""
    if sys.platform != 'darwin':
        raise RuntimeError('macOS native fixture protection cannot be verified here')
    checks = []
    with tempfile.TemporaryDirectory(prefix='workbuddy-native-policy-') as folder:
        root = Path(folder)
        victim = root / 'protected'; victim.mkdir()
        allowed = root / 'allowed'; allowed.mkdir()
        secret = victim / 'synthetic.txt'; secret.write_text('SYNTHETIC_DENIED_READ')
        policy = root / 'policy.sb'; policy.write_text(profile_text([victim]))
        cases = [
            ('protected read denied', ['/bin/cat', str(secret)], False),
            ('protected metadata available', ['/usr/bin/stat', '-f', '%z', str(secret)], True),
            ('protected write denied', ['/usr/bin/touch', str(victim / 'new')], False),
            ('descendant write denied', ['/bin/sh', '-c',
              '/usr/bin/touch ' + shlex.quote(str(victim / 'child'))], False),
            ('isolated fixture write allowed', ['/usr/bin/touch', str(allowed / 'new')], True),
        ]
        for name, args, succeeds in cases:
            result = subprocess.run(command(policy, args[0], args[1:]), capture_output=True)
            checks.append({'name': name, 'exit_code': result.returncode,
                           'ok': (result.returncode == 0) == succeeds
                                 and b'SYNTHETIC_DENIED_READ' not in result.stdout})
        checks.append({'name': 'denied targets were never created',
                       'ok': not (victim / 'new').exists() and not (victim / 'child').exists()})
        checks.append({'name': 'allowed target really exists', 'ok': (allowed / 'new').exists()})
    report = {'result': 'PASS' if all(row['ok'] for row in checks) else 'FAIL', 'checks': checks,
              'scope': 'synthetic paths only; no real profile read/write attempted'}
    target = Path(output); target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(report, indent=2) + '\n')
    return 0 if report['result'] == 'PASS' else 1


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output')
    parser.add_argument('--attest-file')
    parser.add_argument('--allowed-root')
    parser.add_argument('child', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.attest_file:
        from workbuddy_import_test import real_user_home
        child = args.child[1:] if args.child[:1] == ['--'] else args.child
        if not child or not args.allowed_root:
            parser.error('attestation requires an isolated root and exec command')
        proof = kernel_protection(real_user_home(), args.allowed_root)
        proof.update(command=child, monotonic_ns=str(time.monotonic_ns()),
                     script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest())
        Path(args.attest_file).write_text(json.dumps(proof, indent=2) + '\n')
        if not proof['enforced']:
            raise SystemExit('kernel source/config protection is not enforced')
        os.execvpe(child[0], child, os.environ)
    if not args.output:
        parser.error('--output is required for synthetic self-check')
    raise SystemExit(self_check(args.output))

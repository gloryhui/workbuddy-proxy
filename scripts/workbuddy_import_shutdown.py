#!/usr/bin/env python3
"""Graceful five-second exit: idle, active import, and actual oversized drain."""
import argparse
import hashlib
import os
import sys
import time
import traceback
from pathlib import Path

from workbuddy_import_golden import Golden
from workbuddy_import_performance import line
from workbuddy_import_test import Report, Sandbox, resolve_exe


def source(fixture, mode):
    fixture.project.mkdir(parents=True, exist_ok=True)
    path = fixture.project / 'shutdown.jsonl'
    with path.open('wb') as output:
        if mode == 'oversized':
            block = b'x' * (1024 * 1024)
            for _ in range(1024):
                output.write(block)
        else:
            for index in range(1 if mode == 'idle' else 100_000):
                output.write(line('shutdown', 'shutdown-%06d' % index,
                                  int(time.time() * 1000), 'shutdown-fixture').encode())
        output.flush()
        os.fsync(output.fileno())
    return path.stat().st_size


def prepare(fixture, mode, size):
    if mode == 'idle':
        fixture.until(lambda: fixture.action('stats_workbuddy_status')['complete'], 'idle source completion')
        return {'facts': fixture.rows('SELECT COUNT(*) FROM usage_requests')[0][0]}
    if mode == 'import':
        def partly_imported():
            return 0 < fixture.rows('SELECT COUNT(*) FROM usage_requests')[0][0] < 100_000
        fixture.until(partly_imported, 'actual partially committed source import')
        return {'facts': fixture.rows('SELECT COUNT(*) FROM usage_requests')[0][0]}
    def draining():
        return fixture.rows('SELECT COUNT(*) FROM workbuddy_import_files WHERE discarding_oversized=1 '
                            'AND committed_offset>=4194304 AND committed_offset<observed_size') == [(1,)]
    fixture.until(draining, 'actual oversized source drain')
    state = fixture.rows('SELECT committed_offset,observed_size,discarding_oversized FROM workbuddy_import_files')[0]
    body = fixture.action('stats_workbuddy_status')['source_body_bytes_read']
    if not (4 * 1024**2 <= state[0] < state[1] == size and state[2] == 1 and state[0] <= body < size):
        raise AssertionError('oversized drain completed before shutdown measurement')
    return {'offset': state[0], 'observed_size': state[1], 'discarding': state[2], 'body_read': body}


def run_case(mode, exe, report):
    sandbox = Sandbox(prefix='workbuddy-shutdown-')
    fixture = Golden(sandbox)
    try:
        fixture.start(exe)
        size = source(fixture, mode)
        fixture.enable()
        before = prepare(fixture, mode, size)
        started = time.monotonic_ns()
        fixture.close()
        elapsed = (time.monotonic_ns() - started) / 1_000_000
        # close's fallback terminate/kill cannot pass this elapsed/exit-code check.
        report.check('stop: %s exits gracefully within 5s' % mode,
                     elapsed <= 5_000 and fixture.process.returncode == 0,
                     'elapsed_ms=%.3f exit=%s' % (elapsed, fixture.process.returncode))
        report.facts[mode] = {'elapsed_ms': elapsed, 'source_bytes': size, 'before_stop': before,
                              'core_binary_sha256': fixture.binary_sha256}
        sandbox.check_isolated(report)
    finally:
        try:
            fixture.close()
        finally:
            sandbox.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', default='target/release/workbuddy-proxy')
    parser.add_argument('--output', default='dist/workbuddy-import-validation/shutdown')
    args = parser.parse_args()
    report = Report('R09-shutdown', args.exe, script=Path(__file__).name)
    report.facts['script_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    try:
        for mode in ('idle', 'import', 'oversized'):
            run_case(mode, resolve_exe(args.exe), report)
    except Exception as failure:
        traceback.print_exc()
        report.check('stop: all actual shutdown states exercised', False, type(failure).__name__)
    report.result = 'FAIL' if report.failed else 'PASS'
    report.write(Path(args.output) / 'report.json')
    return 1 if report.failed else 0


if __name__ == '__main__':
    sys.exit(main())

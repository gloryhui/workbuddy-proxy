#!/usr/bin/env python3
"""Measure production status polling against a verified synthetic 1M snapshot.

This measures the current release, without importing an old phase's performance
result. The snapshot is copied into a disposable config; its source is read only.
"""
import argparse
from contextlib import closing
import hashlib
import json
from pathlib import Path
import shutil
import sqlite3
import statistics
import threading
import time

from workbuddy_import_bench import verify_snapshot
from workbuddy_import_golden import Golden
from workbuddy_import_performance import StreamingUpstream, one_stream, p95, streams
from workbuddy_import_resources import usage
from workbuddy_import_test import Report, Sandbox, resolve_exe


def file_digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def timed_status(fixture):
    start = time.monotonic_ns()
    status = fixture.action('stats_workbuddy_status')
    if status['imported'] != 1_010_000:
        raise AssertionError('status does not match the known synthetic ledger')
    return (time.monotonic_ns() - start) / 1_000_000


def stream_round(fixture, polling):
    stopped = threading.Event()
    latencies, failures = [], []

    def poll():
        while not stopped.is_set():
            try:
                latencies.append(timed_status(fixture))
            except Exception as failure:
                failures.append(type(failure).__name__)
                return
            stopped.wait(2)

    thread = threading.Thread(target=poll, name='qa-status-poller') if polling else None
    if thread:
        thread.start()
    try:
        first_content = streams(fixture)
    finally:
        stopped.set()
        if thread:
            thread.join(timeout=15)
    if thread and thread.is_alive():
        raise AssertionError('status poller did not exit')
    return {'first_content_ms': first_content, 'p95_ms': p95(first_content),
            'status_ms': latencies, 'status_failures': failures}


def run(args):
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    phase = Path(args.phase).resolve()
    # Reject arbitrary databases: this must be the complete known synthetic
    # scale corpus, including its exact unique ledger and token/credit totals.
    verified = verify_snapshot(phase / 'report.json', 1_000_000)
    snapshot = phase / 'usage-snapshot.sqlite3'
    report = Report('S10 status polling on current release', args.exe, Path(__file__).name)
    sandbox = Sandbox(prefix='workbuddy-status-budget-')
    fixture = Golden(sandbox, handler_class=StreamingUpstream)
    try:
        shutil.copy2(snapshot, fixture.database)
        fixture.start(resolve_exe(args.exe))
        report.facts.update(core_binary_sha256=fixture.binary_sha256,
                            snapshot_sha256=file_digest(snapshot),
                            snapshot_verification=verified,
                            script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest())
        # Fifty raw observations in each of three rounds; CPU is whole-core,
        # not a claim of per-thread CPU attribution.
        idle_rounds = []
        for _ in range(3):
            _, before_cpu = usage(fixture.process.pid)
            started = time.monotonic()
            samples = [timed_status(fixture) for _ in range(50)]
            _, after_cpu = usage(fixture.process.pid)
            idle_rounds.append({'samples_ms': samples, 'p95_ms': p95(samples),
                                'wall_seconds': time.monotonic() - started,
                                'whole_core_cpu_seconds': after_cpu - before_cpu})
        report.facts['status_rounds'] = idle_rounds
        rounds = []
        for number in range(3):
            for _ in range(8):
                one_stream(fixture)
            # Alternate ordering so every baseline does not get a cold cache.
            order = (False, True) if number % 2 == 0 else (True, False)
            measured = {enabled: stream_round(fixture, enabled) for enabled in order}
            baseline, polled = measured[False], measured[True]
            budget = max(10, baseline['p95_ms'] * 0.1)
            report.check('round %d: 2s status polling preserves stream latency' % number,
                         polled['p95_ms'] - baseline['p95_ms'] <= budget
                         and not polled['status_failures'] and bool(polled['status_ms']),
                         'baseline=%.3f polled=%.3f allowance=%.3fms' %
                         (baseline['p95_ms'], polled['p95_ms'], budget))
            rounds.append({'round': number, 'baseline': baseline, 'polled': polled,
                           'allowed_increase_ms': budget})
        report.facts['stream_rounds'] = rounds
        report.facts['median_status_p95_ms'] = statistics.median(r['p95_ms'] for r in idle_rounds)
        expected = 408 + 3 * (8 + 400)
        fixture.wait_rows("SELECT COUNT(*) FROM usage_requests WHERE record_origin='proxy'", [(expected,)])
        with closing(sqlite3.connect(fixture.database.as_uri() + '?mode=ro', uri=True)) as db:
            actual = db.execute("SELECT COUNT(*),SUM(outcome='completed' AND usage_status='complete' "
                                "AND input_tokens=10 AND output_tokens=2 AND total_tokens=12) "
                                "FROM usage_requests WHERE record_origin='proxy'").fetchone()
        report.check('all warm-up and measured proxy facts persisted exactly', actual == (expected, expected),
                     'expected=%d actual=%r' % (expected, actual))
        sandbox.check_isolated(report)
        report.result = 'PASS_STATUS_POLLING_OTHER_GATES_PENDING' if not report.failed else 'FAIL'
    except Exception as failure:
        report.result = 'FAIL'
        report.error = type(failure).__name__ + ': ' + str(failure)
        report.check('probe completed', False, report.error)
    finally:
        fixture.close()
        if (sandbox.root / 'golden-core.log').exists():
            shutil.copy2(sandbox.root / 'golden-core.log', output / 'core.log')
        report.write(output / 'report.json')
        sandbox.close()
    print('%s: %d passed / %d failed' % (report.result, report.passed, report.failed_count))
    return 1 if report.failed else 0


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', required=True)
    parser.add_argument('--phase', required=True)
    parser.add_argument('--output', required=True)
    raise SystemExit(run(parser.parse_args()))

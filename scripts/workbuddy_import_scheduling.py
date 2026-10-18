#!/usr/bin/env python3
"""Production scheduling probes; row identities, not unrelated totals, are timed."""
import argparse
import hashlib
import json
import os
import signal
import sys
import time
import traceback
from pathlib import Path

from workbuddy_import_golden import Golden
from workbuddy_import_performance import line
from workbuddy_import_test import Report, Sandbox, resolve_exe
from workbuddy_import_query_pressure import QueryPressure


def write_rows(path, session, prefix, first, count, model, minimum=0):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('a') as output:
        for index in range(first, first + count):
            output.write(line(session, '%s-%08d' % (prefix, index),
                              int(time.time() * 1000), model, minimum))
        output.flush()
        os.fsync(output.fileno())
    return time.monotonic_ns()


def count(fixture, model):
    # Model names here are fixed synthetic constants supplied only by this script.
    return fixture.rows("SELECT COUNT(*) FROM usage_requests WHERE upstream_model='%s'" % model)[0][0]


def visible(fixture, model, expected, flushed, budget_ms):
    deadline = flushed + budget_ms * 1_000_000
    while time.monotonic_ns() < deadline:
        actual = count(fixture, model)
        if actual == expected:
            return {'flushed_monotonic_ns': flushed, 'visible_monotonic_ns': time.monotonic_ns(),
                    'expected': expected, 'actual': actual}
        time.sleep(0.02)
    raise AssertionError('model %s missed %dms budget (expected %d, actual %d)' %
                         (model, budget_ms, expected, actual))


def manual_and_full(fixture, report):
    initial = fixture.project / 'steady.jsonl'
    write_rows(initial, 'steady', 'seed', 0, 1, 'steady-seed')
    fixture.enable()
    fixture.until(lambda: count(fixture, 'steady-seed') == 1, 'seed tracking')
    fixture.until(lambda: fixture.action('stats_workbuddy_status')['complete'], 'initial completion')
    flushed = write_rows(initial, 'steady', 'manual', 0, 500, 'manual-incremental')
    sent = time.monotonic_ns()
    response = fixture.action('stats_workbuddy_scan', mode='incremental')
    feedback_ms = (time.monotonic_ns() - sent) / 1_000_000
    observation = visible(fixture, 'manual-incremental', 500, flushed, 5_000)
    report.check('manual: scan feedback within 2s and exact 500 rows visible within 5s',
                 feedback_ms <= 2_000 and bool(response['scan_id']))
    fixture.until(lambda: fixture.action('stats_workbuddy_status')['complete'], 'manual completion')
    before = fixture.action('stats_workbuddy_status')['source_body_bytes_read']
    full = fixture.action('stats_workbuddy_scan', mode='full')
    fixture.until(lambda: fixture.action('stats_workbuddy_status')['scan_state'] == 'completed', 'full completion')
    after = fixture.action('stats_workbuddy_status')['source_body_bytes_read']
    report.check('full: measured body reread covers complete unchanged source',
                 after - before >= initial.stat().st_size,
                 'bytes=%d source=%d' % (after - before, initial.stat().st_size))
    report.check('full: no duplicate facts or ledger identities',
                 fixture.rows('SELECT COUNT(*),COUNT(DISTINCT entry_key) FROM workbuddy_import_ledger') == [(501, 501)]
                 and fixture.rows('SELECT COUNT(*) FROM usage_requests') == [(501,)])
    return {'feedback_ms': feedback_ms, 'manual': observation,
            'full_body_bytes': after - before, 'full_scan_id': full['scan_id']}


def commit_observation(fixture, path, flushed):
    relative = path.relative_to(fixture.sandbox.source_root).as_posix()
    key = fixture.rows("SELECT file_key FROM workbuddy_import_files WHERE relative_path='%s'" % relative)[0][0]
    sent = time.monotonic_ns()
    status = fixture.action('stats_workbuddy_status')
    received = time.monotonic_ns()
    receipt = next((row for row in status['source_commit_receipts'] if row['file_key'] == key), {})
    if receipt.get('offset') != path.stat().st_size or receipt.get('full') is not False:
        raise AssertionError('timed file has no matching successful commit receipt')
    commit = int(receipt['commit_ack_elapsed_ns'])
    now = int(status['monotonic_elapsed_ns'])
    interval = [sent + commit - now, received + commit - now]
    if not (0 <= commit <= now and flushed <= interval[1] <= flushed + 10_000_000_000):
        raise AssertionError('invalid commit receipt interval')
    return {'captured': True, 'clock_id': status['monotonic_clock_id'],
            'client_monotonic_interval_ns': interval}


def competing_sample(fixture, index, report):
    steady = fixture.project / 'steady.jsonl'
    fresh = fixture.sandbox.source_root / ('projects/new-%02d/new-%02d.jsonl' % (index, index))
    first_flush = write_rows(steady, 'steady', 'tracked', index * 250, 250, 'competing-tracked')
    new_flush = write_rows(fresh, fresh.stem, 'fresh', 0, 250, 'competing-new-%02d' % index)
    try:
        tracked = visible(fixture, 'competing-tracked', (index + 1) * 250, first_flush, 10_000)
        discovered = visible(fixture, 'competing-new-%02d' % index, 250, new_flush, 10_000)
    except AssertionError:
        failure = {'index': index, 'tracked_flushed_monotonic_ns': first_flush,
                   'new_flushed_monotonic_ns': new_flush,
                   'observed_monotonic_ns': time.monotonic_ns()}
        report.facts.setdefault('failed_incremental_windows', []).append(failure)
        try:
            failure['tracked_rows'] = count(fixture, 'competing-tracked')
            failure['new_rows'] = count(fixture, 'competing-new-%02d' % index)
            status = fixture.action('stats_workbuddy_status')
            failure['progress'] = status['progress']
            failure['runtime_metrics'] = status['runtime_metrics']
            failure['source_body_bytes_read'] = status['source_body_bytes_read']
        except Exception as diagnostic_error:
            failure['diagnostic_error'] = type(diagnostic_error).__name__
        raise
    report.check('X46: sample %02d exact tracked append and new project within 10s' % index, True)
    return {'tracked': tracked, 'new_project': discovered,
            'tracked_receipt': commit_observation(fixture, steady, first_flush),
            'receipt': commit_observation(fixture, fresh, new_flush)}


def competing(fixture, report, pressure=None):
    backlog = fixture.project / 'backlog-0000.jsonl'
    total = 200_000
    written = 0
    for number in range(979):
        rows = total // 979 + int(number < total % 979)
        session = 'backlog-%04d' % number
        write_rows(fixture.project / (session + '.jsonl'), session, 'backfill',
                   written, rows, 'competing-backfill', 1_024)
        written += rows
    fixture.until(lambda: 0 < count(fixture, 'competing-backfill') < total, 'active competing backfill')
    samples = []
    measured = {'backfill_records': total, 'samples': samples}
    report.facts['competing'] = measured
    try:
        if pressure:
            pressure.start()
        for index in range(20):
            before = count(fixture, 'competing-backfill')
            if total - before < 100_000:
                write_rows(backlog, 'backlog-0000', 'backfill', total, 100_000, 'competing-backfill', 1_024)
                total += 100_000
                measured['backfill_records'] = total
            imported = count(fixture, 'competing-backfill')
            if not (0 < imported < total):
                raise AssertionError('competing backfill already complete before timed sample')
            reads_before = pressure.snapshot() if pressure else None
            sample = competing_sample(fixture, index, report)
            sample['backfill_before'] = imported
            sample['backfill_written'] = total
            if pressure:
                sample['reports_before'] = reads_before
                sample['reports_after'] = pressure.snapshot()
            samples.append(sample)
            time.sleep((index % 5) * 0.23)
    finally:
        if pressure:
            report.facts['continuous_dashboard_pressure'] = pressure.stop(report)
    report.check('X46: twenty phases exercised with a real competing backlog', len(samples) == 20)
    if pressure:
        report.check('report pressure: both real clients span all20 increment windows',
                     len(samples) == 20 and all(
                         sample['reports_before']['live_clients'] == 2
                         and sample['reports_after']['live_clients'] == 2 for sample in samples))
        report.check('report pressure: historical backfill also progresses during continuous queries',
                     len(samples) == 20
                     and samples[-1]['backfill_before'] > samples[0]['backfill_before'])
    fixture.until(lambda: count(fixture, 'competing-backfill') == total, 'competing drain', timeout=600)
    fixture.until(lambda: fixture.action('stats_workbuddy_status')['complete'], 'competing checkpoints', timeout=60)
    expected = total + 501 + 20 * 500
    report.check('X46: every distinct written identity is durable exactly once',
                 fixture.rows('SELECT COUNT(*),COUNT(DISTINCT entry_key) FROM workbuddy_import_ledger') == [(expected, expected)]
                 and fixture.rows('SELECT COUNT(*) FROM usage_requests') == [(expected,)])
    return measured


def resume(fixture, report):
    if not hasattr(signal, 'SIGSTOP'):
        return {'result': 'BLOCKED_EXTERNAL', 'reason': 'POSIX process suspension unavailable'}
    os.kill(fixture.process.pid, signal.SIGSTOP)
    try:
        time.sleep(6)
        flushed = write_rows(fixture.project / 'steady.jsonl', 'steady', 'resume', 0, 1, 'resume-row')
    finally:
        os.kill(fixture.process.pid, signal.SIGCONT)
    resumed = time.monotonic_ns()
    observed = visible(fixture, 'resume-row', 1, resumed, 10_000)
    ids = [fixture.action('stats_workbuddy_scan', mode='full')['scan_id'] for _ in range(10)]
    report.check('X47: resume recovers durable rows and repeated pending work coalesces', len(set(ids)) == 1)
    return {'source_flush_during_suspend_ns': flushed, 'resume_ns': resumed, 'observed': observed}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', default='target/release/workbuddy-proxy')
    parser.add_argument('--output', required=True)
    parser.add_argument('--query-pressure', action='store_true',
                        help='Run two continuous real dashboards during all20 increment windows')
    args = parser.parse_args()
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    sandbox = Sandbox(prefix='workbuddy-scheduling-')
    fixture = Golden(sandbox)
    report = Report('R09-scheduling', args.exe, script=Path(__file__).name)
    report.facts['script_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    try:
        fixture.start(resolve_exe(args.exe))
        report.facts['core_binary_sha256'] = fixture.binary_sha256
        report.facts['manual_full'] = manual_and_full(fixture, report)
        report.facts['competing'] = competing(
            fixture, report, QueryPressure(fixture) if args.query_pressure else None)
        report.facts['resume'] = resume(fixture, report)
        stopped = time.monotonic_ns()
        fixture.close()
        report.check('stop: core exits within unchanged 5s budget',
                     (time.monotonic_ns() - stopped) <= 5_000_000_000 and fixture.process.returncode == 0)
    except Exception as failure:
        traceback.print_exc()
        report.check('scheduling: phase completes without exception', False, type(failure).__name__)
    finally:
        try:
            fixture.close()
        finally:
            sandbox.close()
    report.result = 'FAIL' if report.failed else 'PASS'
    report.write(output / 'report.json')
    return 1 if report.failed else 0


if __name__ == '__main__':
    sys.exit(main())

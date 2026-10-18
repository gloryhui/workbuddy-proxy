#!/usr/bin/env python3
"""Real OS write refusal and SIGKILL during a partly committed backfill.

Only disposable sandbox files are affected. No trigger or source-read error is
substituted for the filesystem write error. The write failure is EFBIG imposed
on the child process; this is not a claim that a physical disk was filled.
"""
import argparse
import hashlib
import os
import signal
import subprocess
import sys
import time
import traceback
from pathlib import Path

from workbuddy_import_golden import Golden
from workbuddy_import_test import Report, Sandbox, mixed_line, resolve_exe

FILE_LIMIT = 512 * 1024


def file_limit():
    import resource
    signal.signal(signal.SIGXFSZ, signal.SIG_IGN)
    resource.setrlimit(resource.RLIMIT_FSIZE, (FILE_LIMIT, FILE_LIMIT))


def write_corpus(fixture, session, records):
    fixture.project.mkdir(parents=True, exist_ok=True)
    pending = fixture.project / (session + '.pending')
    target = pending.with_suffix('.jsonl')
    with pending.open('w') as file:
        for index in range(records):
            file.write(mixed_line(session, '%s-%06d' % (session, index), 10, 2, 0.1) + '\n')
        file.flush()
        os.fsync(file.fileno())
    pending.replace(target)


def imported(fixture):
    return fixture.rows("SELECT COUNT(*) FROM usage_requests WHERE record_origin='workbuddy_builtin'")[0][0]


def restart(fixture, exe):
    if fixture.process.poll() is None:
        raise AssertionError('restart attempted before the old core exited')
    fixture.log.close()
    fixture.log = (fixture.sandbox.root / 'golden-core.log').open('ab')
    fixture.process = subprocess.Popen([str(fixture.exe), '--headless', '--config-dir',
        str(fixture.sandbox.config_dir)], cwd=fixture.sandbox.root,
        env=fixture.sandbox.environment(), stdout=fixture.log, stderr=fixture.log)
    fixture.until(fixture.ready, 'unlimited core restart')


def disk_write_refusal(fixture, exe, report):
    fixture.start(exe)
    report.facts['core_binary_sha256'] = fixture.binary_sha256
    fixture.enable()
    write_corpus(fixture, 'write-failure', 5_000)
    deadline = time.monotonic() + 30
    max_wal = 0
    error = None
    while time.monotonic() < deadline:
        wal = Path(str(fixture.database) + '-wal')
        if wal.exists():
            max_wal = max(max_wal, wal.stat().st_size)
        status = fixture.action('stats_workbuddy_status')
        error = status['last_error_code']
        if error and max_wal >= FILE_LIMIT - 4_200:
            break
        time.sleep(0.02)
    before = imported(fixture)
    report.check('faults: child OS file limit rejects real SQLite WAL growth',
                 FILE_LIMIT - 4_200 <= max_wal <= FILE_LIMIT and bool(error) and before < 5_000,
                 'limit=%d max_wal=%d committed=%d error=%s' % (FILE_LIMIT, max_wal, before, error))
    # A real upstream call must still return despite the database write error.
    response = fixture.post('/r/golden-route/v1/chat/completions', {'model': 'golden-model',
        'messages': [{'role': 'user', 'content': 'synthetic'}], 'stream': False}, 'fixture-local-key')
    report.check('faults: proxy forwarding survives the filesystem write refusal',
                 response.get('usage', {}).get('total_tokens') == 550)
    fixture.post('/_control/stop', {}, fixture.key)
    fixture.process.wait(timeout=10)
    restart(fixture, exe)
    fixture.until(lambda: imported(fixture) == 5_000, 'write-refusal backlog recovery', timeout=60)
    facts = fixture.rows("SELECT COUNT(*),SUM(total_tokens),SUM(workbuddy_credit_nanos) "
                         "FROM usage_requests WHERE record_origin='workbuddy_builtin'")
    report.check('faults: removing OS write limit recovers exact usage without duplicates',
                 facts == [(5_000, 60_000, 500_000_000_000)], repr(facts))


def partial_backfill_crash(fixture, exe, report):
    base = imported(fixture)
    records = 50_000
    write_corpus(fixture, 'crash-backfill', records)
    deadline = time.monotonic() + 30
    partial = base
    while time.monotonic() < deadline:
        partial = imported(fixture)
        if partial > base:
            break
        time.sleep(0.01)
    report.check('faults: SIGKILL targets a genuinely incomplete committed backfill',
                 base < partial < base + records,
                 'base=%d committed=%d target=%d' % (base, partial, base + records))
    if not base < partial < base + records:
        raise AssertionError('no partly committed backfill was observed')
    fixture.process.kill()
    fixture.process.wait(timeout=10)
    restart(fixture, exe)
    fixture.until(lambda: imported(fixture) == base + records, 'crashed backfill recovery', timeout=90)
    facts = fixture.rows("SELECT COUNT(*),SUM(total_tokens),SUM(workbuddy_credit_nanos) "
                         "FROM usage_requests WHERE record_origin='workbuddy_builtin'")
    target = base + records
    report.check('faults: crash restart resumes to exact token and credit totals',
                 facts == [(target, target * 12, target * 100_000_000)], repr(facts))
    ledger = fixture.rows('SELECT COUNT(*),COUNT(DISTINCT entry_key) FROM workbuddy_import_ledger')
    report.check('faults: crash recovery leaves one ledger identity per source message',
                 ledger == [(target, target)], repr(ledger))


def run(sandbox, exe, report):
    if os.name != 'posix':
        raise AssertionError('OS write-refusal probe requires POSIX RLIMIT_FSIZE')
    fixture = Golden(sandbox, process_options={'preexec_fn': file_limit})
    try:
        disk_write_refusal(fixture, exe, report)
        partial_backfill_crash(fixture, exe, report)
    finally:
        fixture.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', default='target/release/workbuddy-proxy')
    parser.add_argument('--output', default='dist/workbuddy-import-validation/os-faults')
    args = parser.parse_args()
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    report = Report('R09-os-faults', args.exe, script=Path(__file__).name)
    report.facts['script_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    if os.name != 'posix':
        report.result = 'BLOCKED_EXTERNAL'
        report.error = 'POSIX_RLIMIT_FSIZE_REQUIRED'
        report.write(output / 'report.json')
        return 2
    sandbox = Sandbox(prefix='workbuddy-os-faults-')
    try:
        run(sandbox, resolve_exe(args.exe), report)
        sandbox.check_isolated(report)
    except Exception as failure:
        traceback.print_exc()
        report.check('faults: real OS failure/restart chain completed', False, type(failure).__name__)
    finally:
        sandbox.close()
    report.result = 'FAIL' if report.failed else 'PASS'
    report.write(output / 'report.json')
    return 1 if report.failed else 0


if __name__ == '__main__':
    sys.exit(main())

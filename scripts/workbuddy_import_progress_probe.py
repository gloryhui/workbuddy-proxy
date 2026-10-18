#!/usr/bin/env python3
"""Checkpoint progress through real background import, restart and source changes.

Only synthetic source files create facts; SQL is read-only. This small fixture
tests progress semantics and restart, not the million-row performance gate.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import sqlite3
import subprocess
import time
import traceback
from contextlib import closing

from workbuddy_import_golden import Golden
from workbuddy_import_performance import line
from workbuddy_import_test import Report, Sandbox, resolve_exe


def write(path, first, count, at, append=False):
    with path.open('a' if append else 'w') as output:
        for index in range(first, first + count):
            output.write(line('progress-session', 'progress-%08d' % index, at,
                              model='progress-synthetic', minimum=1024))
        output.flush()
        os.fsync(output.fileno())


def status(fixture):
    return fixture.action('stats_workbuddy_status')


def count(fixture):
    return fixture.rows("SELECT COUNT(*) FROM usage_requests WHERE record_origin<>'proxy'")[0][0]


def run(args):
    assert os.environ['TMPDIR'].startswith('/Volumes/1TB/')
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    sandbox = Sandbox(prefix='workbuddy-progress-')
    fixture = Golden(sandbox)
    report = Report('R09 checkpoint progress production probe', args.exe, Path(__file__).name)
    report.facts['script_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    try:
        fixture.start(resolve_exe(args.exe))
        report.facts['core_binary_sha256'] = fixture.binary_sha256
        fixture.project.mkdir(parents=True, exist_ok=True)
        source = fixture.project / 'progress-session.jsonl'
        at = int(time.time() * 1000)
        write(source, 0, 50_000, at)
        size = source.stat().st_size
        fixture.enable()
        seen = []
        observations = []
        report.facts['progress_observations'] = observations

        def partial():
            current = status(fixture)
            seen.append(current)
            observations.append({key: current.get(key) for key in
                ('state', 'enabled', 'effective_enabled', 'root_found', 'imported',
                 'pending', 'last_error_code', 'progress', 'source_body_bytes_read')})
            return (current['progress']['total_known'] and 0 < current['imported'] < 50_000
                    and current['progress']['files_remaining'] > 0)

        fixture.until(partial, 'known backlog while import is partial')
        observed = seen[-1]
        report.check('progress: discovery finishes before backfill; remaining uses checkpoint bytes',
                     observed['progress']['files_total'] == 1
                     and observed['progress']['bytes_total'] == size
                     and observed['progress']['bytes_committed'] > 0
                     and observed['progress']['bytes_remaining'] > 0
                     and observed['progress']['bytes_committed'] + observed['progress']['bytes_remaining'] == size)
        # Keep a read-only observer open across shutdown so the system SQLite
        # can still inspect a WAL after its writer has closed its shared memory.
        with closing(sqlite3.connect(fixture.database.resolve().as_uri() + '?mode=ro', uri=True)) as observer:
            observer.execute('SELECT COUNT(*) FROM usage_requests').fetchone()
            stop_started = time.monotonic()
            fixture.post('/_control/stop', {}, fixture.key)
            fixture.process.wait(timeout=5)
            stop_seconds = time.monotonic() - stop_started
            saved_count = observer.execute("SELECT COUNT(*) FROM usage_requests WHERE record_origin<>'proxy'").fetchone()[0]
            saved_checkpoint = observer.execute('SELECT committed_offset,observed_size FROM workbuddy_import_files').fetchall()
        report.check('progress: graceful stop during backlog within five seconds',
                     fixture.process.returncode == 0 and stop_seconds <= 5)
        report.facts.update(saved_count=saved_count, saved_checkpoint=saved_checkpoint)
        restarted = time.monotonic()
        fixture.process = subprocess.Popen([str(fixture.exe), '--headless', '--config-dir',
            str(sandbox.config_dir)], cwd=sandbox.root, env=sandbox.environment(),
            stdout=fixture.log, stderr=fixture.log)
        fixture.until(fixture.ready, 'restart ready without waiting for backfill')
        startup_seconds = time.monotonic() - restarted
        restart_count = count(fixture)
        report.check('progress: restart exposes prior committed facts before full history finishes',
                     0 < saved_count <= restart_count < 50_000)
        fixture.until(lambda: count(fixture) == 50_000, 'eventual complete history', timeout=300)
        fixture.until(lambda: status(fixture)['complete'], 'complete checkpoints')
        completed = status(fixture)
        report.check('progress: resumed history is exact and fully caught up',
                     completed['progress']['bytes_total'] == size
                     and completed['progress']['bytes_committed'] == size
                     and completed['progress']['bytes_remaining'] == 0
                     and completed['progress']['files_remaining'] == 0
                     and fixture.rows('SELECT COUNT(*),COUNT(DISTINCT entry_key) FROM workbuddy_import_ledger') == [(50_000, 50_000)]
                     and fixture.rows('SELECT SUM(total_tokens),SUM(workbuddy_credit_nanos) FROM usage_requests')
                         == [(6_000_000, 57_000_000_000_000)])
        settings = fixture.action('stats_settings_get')
        fixture.action('stats_workbuddy_settings_update', enabled=False, expected_revision=settings['revision'])
        write(source, 50_000, 500, at, append=True)
        body_before = status(fixture)['source_body_bytes_read']
        paused = [status(fixture) for _ in range(10)]
        report.check('progress: paused status polling neither reads nor imports source bodies',
                     all(not value['effective_enabled'] for value in paused)
                     and paused[-1]['source_body_bytes_read'] == body_before and count(fixture) == 50_000)
        fixture.enable()
        fixture.until(lambda: count(fixture) == 50_500, 'append caught up')
        fixture.until(lambda: status(fixture)['complete'], 'append checkpoints')
        appended = status(fixture)
        report.check('progress: append expands discovered bytes and resumes from committed checkpoint',
                     appended['progress']['bytes_total'] == source.stat().st_size > size
                     and appended['progress']['bytes_remaining'] == 0)
        write(source, 50_500, 1, at)
        fixture.until(lambda: count(fixture) == 50_501, 'replacement new identity')
        fixture.until(lambda: status(fixture)['complete'], 'replacement checkpoints')
        replaced = status(fixture)
        report.check('progress: truncated replacement resets bytes without losing historical facts',
                     replaced['progress']['bytes_total'] == source.stat().st_size == 1024
                     and replaced['progress']['bytes_committed'] == 1024
                     and replaced['progress']['bytes_remaining'] == 0)
        pending = json.loads(line('pending-session', 'pending-row', at))
        pending['providerData']['requestModelId'] = 'custom-local:progress-unknown'
        (fixture.project / 'pending-session.jsonl').write_text(json.dumps(pending) + '\n')
        fixture.until(lambda: status(fixture)['pending'] == 1, 'unknown attribution')
        fixture.until(lambda: status(fixture)['progress']['files_remaining'] == 0, 'pending source read complete')
        pending_status = status(fixture)
        report.check('progress: pending attribution is separate from unread files and bytes',
                     pending_status['pending'] == 1 and not pending_status['complete']
                     and pending_status['progress']['bytes_remaining'] == 0
                     and pending_status['progress']['files_remaining'] == 0
                     and pending_status['imported'] == 50_501)
        report.facts.update(partial=observed, saved_count=saved_count,
                            saved_checkpoint=saved_checkpoint, restart_count=restart_count,
                            startup_seconds=startup_seconds, stop_seconds=stop_seconds,
                            completed=completed, appended=appended, replaced=replaced, pending=pending_status)
        sandbox.check_isolated(report)
    except (Exception, KeyboardInterrupt) as failure:
        traceback.print_exc()
        report.error = type(failure).__name__ + ': ' + str(failure)
        report.check('progress: full production probe completed', False, report.error)
    finally:
        try:
            fixture.close()
        finally:
            stopped = fixture.process is None or fixture.process.poll() is not None
            size = sum(p.stat().st_size for p in sandbox.root.rglob('*') if p.is_file())
            if (sandbox.root / 'golden-core.log').exists():
                shutil.copy2(sandbox.root / 'golden-core.log', output / 'core.log')
            if stopped:
                sandbox.close()
            report.facts['temporary_cleanup'] = {'owned_root': str(sandbox.root),
                'children_stopped': stopped, 'removed': not sandbox.root.exists(),
                'released_logical_bytes': size if stopped else 0,
                'retained_temp_bytes': 0 if stopped else size,
                'system_free_bytes': shutil.disk_usage('/').free}
            report.check('progress: stopped child before exact owned fixture cleanup', stopped and not sandbox.root.exists())
            report.result = 'FAIL' if report.failed else 'PASS_PROGRESS_PROBE_OTHER_GATES_PENDING'
            report.write(output / 'report.json')
    return 1 if report.failed else 0


if __name__ == '__main__':
    def interrupted(signum, frame):
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        raise KeyboardInterrupt('signal %d' % signum)
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', required=True)
    parser.add_argument('--output', required=True)
    raise SystemExit(run(parser.parse_args()))

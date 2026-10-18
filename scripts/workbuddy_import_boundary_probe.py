#!/usr/bin/env python3
"""Actual control API authentication and oversized-result boundary probe.

Seeds a disposable SQLite database with valid bounded labels, then deliberately
injects one check-invalid label to exercise the defensive result cap. Restores
the row and verifies integrity. This is a fault probe, not JSONL import evidence.
Run inside the source-protection launcher with an external TMPDIR.
"""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import sqlite3
import time
import traceback

from workbuddy_import_golden import Golden
from workbuddy_import_test import Report, Sandbox, resolve_exe


def run(fixture, exe, report):
    fixture.start(exe)
    fixture.enable()
    report.facts['core_binary_sha256'] = fixture.binary_sha256
    denied = fixture.post('/_control/action',
                          {'command': 'stats_workbuddy_status'}, 'fixture-local-key')
    report.check('local model key cannot call import management',
                 denied['_http_status'] == 401, repr(denied))
    at = int(time.time() * 1000)
    count, label = 20, 'SYNTHETIC_BOUNDARY_LABEL'
    with closing(sqlite3.connect(fixture.database)) as connection:
        run_id = connection.execute(
            'SELECT run_id FROM stats_runs WHERE stopped_at_ms IS NULL').fetchone()[0]
        with connection:
            connection.executemany(
                'INSERT INTO usage_requests(request_id,run_id,source,provider_id,route_id,'
                'provider_name,route_name,requested_model,upstream_model,started_at_ms,'
                'ended_at_ms,stream,upstream_started,http_status,outcome,usage_status,'
                'input_tokens,output_tokens,total_tokens,usage_source,created_at_ms,updated_at_ms) '
                "VALUES(?,?,'relay','boundary-provider','boundary-route',?,?,'model','model',"
                "?,?,0,1,200,'completed','complete',10,2,12,'upstream',?,?)",
                [('boundary-%03d' % index, run_id, label, label, at, at + 1, at, at + 1)
                 for index in range(count)])
        integrity = connection.execute('PRAGMA integrity_check').fetchall()
        foreign_keys = connection.execute('PRAGMA foreign_key_check').fetchall()
    report.check('synthetic boundary database remains integral with no orphan references',
                 integrity == [('ok',)] and foreign_keys == [])
    fields = dict(from_ms=at - 1, to_ms=at + 2, timezone='UTC',
                  include_tests=False, source_scope='all', capture_method='all')
    small = fixture.action('stats_requests', filter=fields, page_size=20)
    small_bytes = len(json.dumps(small).encode())
    report.check('normal query returns all valid bounded synthetic labels',
                 small['total_rows'] == count and len(small['items']) == count
                 and all(row['provider_name'] == row['route_name'] == label
                         for row in small['items']))
    oversized_label = 'SYNTHETIC_CORRUPT_BOUNDARY_' + 'Q' * (1024 * 1024 + 1)
    with closing(sqlite3.connect(fixture.database)) as connection:
        # Deliberate corruption of this disposable fixture only. Normal schema
        # bounds must not be weakened merely to manufacture a large response.
        connection.execute('PRAGMA ignore_check_constraints=ON')
        with connection:
            connection.execute("UPDATE usage_requests SET provider_name=? WHERE request_id='boundary-000'",
                               [oversized_label])
        connection.execute('PRAGMA ignore_check_constraints=OFF')
        corrupt_integrity = connection.execute('PRAGMA integrity_check').fetchall()
        injected_bytes = connection.execute("SELECT length(CAST(provider_name AS BLOB)) "
            "FROM usage_requests WHERE request_id='boundary-000'").fetchone()[0]
    report.check('negative fixture explicitly contains one check-invalid oversized field',
                 injected_bytes > 1024 * 1024
                 and any('CHECK constraint failed' in row[0] for row in corrupt_integrity))
    oversized = fixture.raw_action('stats_requests', filter=fields, page_size=count)
    message = oversized.get('message', '')
    report.check('result above 1MiB is refused with fixed code rather than truncated JSON',
                 not oversized['ok'] and message.startswith('stats_result_too_large:')
                 and len(message.encode()) < 512 and oversized_label not in message,
                 repr(oversized))
    with closing(sqlite3.connect(fixture.database)) as connection:
        with connection:
            connection.execute("UPDATE usage_requests SET provider_name=? WHERE request_id='boundary-000'",
                               [label])
        restored_integrity = connection.execute('PRAGMA integrity_check').fetchall()
        restored_foreign_keys = connection.execute('PRAGMA foreign_key_check').fetchall()
    after = fixture.action('stats_requests', filter=fields, page_size=20)
    report.check('oversized result does not corrupt the query or its durable facts',
                 after['total_rows'] == count and len(after['items']) == count
                 and all(row['total_tokens'] == 12 and row['provider_name'] == label
                         for row in after['items'])
                 and restored_integrity == [('ok',)] and restored_foreign_keys == [])
    report.facts['boundary'] = {
        'seed_scope': 'valid bounded SQLite seed plus one explicitly corrupt field; control defense only, not importer evidence',
        'rows': count, 'label_characters': len(label), 'small_response_bytes': small_bytes,
        'injected_field_bytes': injected_bytes,
        'response_limit_bytes': 1024 * 1024, 'oversized_response': oversized,
        'integrity_check_before': integrity, 'foreign_key_check_before': foreign_keys,
        'deliberate_corrupt_integrity_check': corrupt_integrity,
        'integrity_check_after': restored_integrity,
        'foreign_key_check_after': restored_foreign_keys}


def main():
    def interrupted(signum, frame):
        # A parent may also signal the process group. Repeated signals must not
        # interrupt the subsequent child wait or remove files still in use.
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        raise KeyboardInterrupt('signal %d' % signum)
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', required=True)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    assert Path(os.environ['TMPDIR']).is_dir(), 'explicit temporary root required'
    assert all(os.environ.get(key) == os.environ['TMPDIR'] for key in ('TMP', 'TEMP'))
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    sandbox = Sandbox(prefix='workbuddy-api-boundary-')
    fixture = Golden(sandbox)
    report = Report('R09-control-boundary', args.exe, script=Path(__file__).name)
    report.facts['script_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    try:
        run(fixture, resolve_exe(args.exe), report)
    except (Exception, KeyboardInterrupt) as failure:
        traceback.print_exc()
        report.check('boundary probe completes', False, type(failure).__name__)
    finally:
        try:
            fixture.close()
        except Exception as failure:
            report.check('fixture shutdown succeeds', False, type(failure).__name__)
        finally:
            try:
                sandbox.check_isolated(report)
                source_log = sandbox.root / 'golden-core.log'
                if source_log.exists():
                    shutil.copy2(source_log, output / source_log.name)
            finally:
                sandbox.close()
                report.facts['temporary_cleanup'] = {
                    'path': str(sandbox.root), 'removed': not sandbox.root.exists(),
                    'child_stopped': fixture.process is None or fixture.process.poll() is not None,
                    'system_free_bytes': shutil.disk_usage('/').free,
                    'external_free_bytes': shutil.disk_usage(output).free,
                    'retained_evidence_bytes': sum(p.stat().st_size for p in output.rglob('*')
                                                 if p.is_file())}
                report.check('fixture stopped before cleanup and temporary root removed',
                             report.facts['temporary_cleanup']['child_stopped']
                             and report.facts['temporary_cleanup']['removed'])
                report.result = 'FAIL' if report.failed else 'PASS'
                report.write(output / 'report.json')
    return int(report.failed)


if __name__ == '__main__':
    raise SystemExit(main())

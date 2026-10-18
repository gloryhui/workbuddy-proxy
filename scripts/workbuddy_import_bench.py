#!/usr/bin/env python3
"""Strict release performance matrix. Partial evidence never becomes an S10 pass.

Runs three independent trials for each of the two scales and two layouts.
Each phase performs its own warm-up and preserves raw samples. Existing old
COUNT-only benchmark output is not accepted by this aggregator.
"""
import argparse
import hashlib
import json
import math
import signal
import statistics
import sqlite3
import subprocess
import sys
from contextlib import closing
from pathlib import Path

from workbuddy_import_test import resolve_exe

SCALES = (100_000, 1_000_000)
LAYOUTS = ('many', 'single')
TRIALS = (1, 2, 3)


def verify_snapshot(path, records):
    database = path.parent / 'usage-snapshot.sqlite3'
    for suffix in ('-wal', '-journal'):
        sidecar = Path(str(database) + suffix)
        if sidecar.exists() and sidecar.stat().st_size:
            raise ValueError('snapshot has a nonempty transaction sidecar')
    with closing(sqlite3.connect(database.resolve().as_uri() + '?mode=ro&immutable=1', uri=True)) as conn:
        proxy = conn.execute("SELECT COUNT(*),SUM(outcome='completed' AND usage_status='complete' "
            "AND input_tokens=10 AND output_tokens=2 AND total_tokens=12) "
            "FROM usage_requests WHERE record_origin='proxy'").fetchone()
        if proxy != (408, 408):
            raise ValueError('snapshot lacks all 400 measured and 8 warm-up completed proxy facts')
        imported = conn.execute("SELECT upstream_model,COUNT(*),SUM(total_tokens),SUM(workbuddy_credit_nanos) "
            "FROM usage_requests WHERE record_origin='workbuddy_builtin' GROUP BY upstream_model "
            "ORDER BY upstream_model").fetchall()
        expected = [('scale-base', records, records * 120, records * 1_140_000_000),
                    ('scale-incremental', 10_000, 1_200_000, 11_400_000_000_000)]
        if imported != expected:
            raise ValueError('snapshot imported count/token/credit totals differ from known corpus')
        total = conn.execute('SELECT COUNT(*) FROM usage_requests').fetchone()[0]
        if total != records + 10_000 + 408:
            raise ValueError('snapshot contains unexpected or missing fact origins')
        ledger = conn.execute("SELECT COUNT(*),COUNT(DISTINCT entry_key),SUM(state='imported') "
                              "FROM workbuddy_import_ledger").fetchone()
        if ledger != (records + 10_000,) * 3:
            raise ValueError('snapshot lacks exact unique imported ledger identities')
        costs = conn.execute("SELECT COUNT(*) FROM usage_costs c JOIN usage_requests r "
                             "ON c.request_id=r.request_id WHERE r.record_origin<>'proxy'").fetchone()[0]
        if costs != 0:
            raise ValueError('snapshot contains fabricated imported money costs')
    return {'completed_proxy_facts': proxy[1], 'total_facts': total,
            'unique_imported_ledger': ledger[1], 'imported_money_rows': costs}


def load_phase(path, core_hash, scale, layout):
    document = json.loads(path.read_text())
    facts = document['isolation']
    if document['result'] != 'PHASE_PASS_GATE_INCOMPLETE' or document['failed'] != 0:
        raise ValueError('phase did not pass')
    if not document['checks'] or any(row['ok'] is not True for row in document['checks']):
        raise ValueError('phase contains missing or failed checks')
    if facts['core_binary_sha256'] != core_hash:
        raise ValueError('phase tested a different core binary')
    if sys.platform == 'darwin' and not facts.get('kernel_source_protection', {}).get('enforced'):
        raise ValueError('macOS phase lacks actual kernel source/config protection')
    if facts['scale'] != scale or facts['layout'] != layout:
        raise ValueError('phase scale or layout differs from matrix slot')
    if facts['corpus']['records'] != scale:
        raise ValueError('phase corpus record count differs')
    minimum = 128 * 1024**2 if scale == 100_000 else 1024**3
    if facts['corpus']['bytes'] < minimum:
        raise ValueError('phase corpus byte count is too small')
    if facts['corpus']['files'] != (1_000 if layout == 'many' else 1):
        raise ValueError('phase corpus file count differs')
    samples = facts['incremental']
    if len(samples) != 20 or any(row['elapsed_ms'] is None or not math.isfinite(row['elapsed_ms'])
                                 or not 0 <= row['elapsed_ms'] <= 10_000
                                 or row['records_visible'] != (index + 1) * 500
                                 for index, row in enumerate(samples)):
        raise ValueError('phase lacks twenty passing incremental observations')
    queries = facts['queries']
    expected = {(number, days, command) for number in range(3 if scale == 1_000_000 else 1)
                for days in (1, 30) for command in ('stats_dashboard', 'stats_requests')}
    keys = {(row['round'], row['days'], row['command']) for row in queries}
    if len(queries) != len(expected) or keys != expected or any(
            not math.isfinite(row['p95_ms']) or not 0 <= row['p95_ms'] <= 500 for row in queries):
        raise ValueError('phase query matrix is incomplete or over budget')
    if scale == 1_000_000 and any(len(row['samples_ms']) < 50 or 'warmup_ms' not in row for row in queries):
        raise ValueError('million-row phase lacks required warm-up or raw samples')
    memory = facts['memory']
    if memory['samples'] <= 0 or memory['error'] is not None or not 0 < memory['peak_bytes'] <= 64 * 1024**2:
        raise ValueError('phase memory evidence is missing or over budget')
    worker = facts['worker_memory_probe']
    if (worker['result'] != 'PASS_FULL_IMPORT_WORKER_MEMORY_BOUND'
            or worker['scale'] != scale or worker['layout'] != layout
            or worker['core_binary_sha256'] != core_hash
            or worker['report_queries_during_measurement'] != 0
            or worker['memory'] != memory
            or worker['totals'] != [[scale, scale * 120, scale * 1_140_000_000]]
            or worker['ledger'] != [[scale] * 3]
            or worker['corpus']['bytes'] < minimum
            or worker['offsets'] != [[worker['corpus']['files'], worker['corpus']['bytes'], worker['corpus']['bytes']]]
            or not worker['cleanup']['normal_exit'] or not worker['cleanup']['removed']):
        raise ValueError('worker RSS bound lacks full-scale import-only proof')
    concurrent = facts['background_query_memory']
    if concurrent['samples'] <= 0 or concurrent['error'] is not None or concurrent['peak_bytes'] <= 0:
        raise ValueError('concurrent report memory was not separately recorded')
    if facts.get('acceptance_contract') != '2026-10-18-background-progress-resource-v2':
        raise ValueError('phase uses an older acceptance contract')
    if not math.isfinite(facts['backfill_ms']) or facts['backfill_ms'] <= 0:
        raise ValueError('phase lacks measured eventual backfill completion')
    observations = facts['background_observations']
    if (not observations or not observations[-1]['complete']
            or not any(0 < row['query_visible'] < scale for row in observations)
            or not any(row['progress']['total_known'] and row['progress']['files_remaining'] > 0
                       for row in observations)):
        raise ValueError('phase lacks partial query availability or known backlog progress')
    for row in observations:
        progress = row['progress']
        if (not 0 <= row['count_before'] <= row['query_visible'] <= row['count_after'] <= scale
                or not math.isfinite(row['query_ms']) or row['query_ms'] < 0
                or not 0 <= progress['files_remaining'] <= progress['files_total']
                or not 0 <= progress['bytes_committed'] <= progress['bytes_total']
                or progress['bytes_remaining'] < 0
                or progress['bytes_committed'] + progress['bytes_remaining'] != progress['bytes_total']
                or (progress['total_known'] and
                    (progress['bytes_total'] != facts['corpus']['bytes']
                     or progress['files_total'] != facts['corpus']['files']))):
            raise ValueError('invalid background query or progress observation')
    final = observations[-1]
    if (final['query_visible'] != scale or final['progress']['files_remaining'] != 0
            or final['progress']['bytes_remaining'] != 0
            or sorted(row['query_ms'] for row in observations)[math.ceil(len(observations) * .95) - 1] > 500):
        raise ValueError('background query availability or final progress did not pass')
    runtime = facts['runtime_metrics']
    if not (1 <= runtime['queue_peak'] <= 2 and runtime['writer_batch_count'] > 0
            and runtime['source_io_thread_checks'] > 0
            and runtime['source_io_thread_violations'] == 0
            and runtime['writer_batch_failures'] == 0
            and 0 < runtime['writer_batch_max_us'] <= runtime['writer_batch_total_us']
            and 1 <= len(runtime['recent_batches']) <= 32):
        raise ValueError('actual production runtime observations missing or invalid')
    facts['snapshot_validation'] = verify_snapshot(path, scale)
    return facts


def aggregate(output, core_hash):
    phases, missing = [], []
    for scale in SCALES:
        for layout in LAYOUTS:
            for trial in TRIALS:
                path = output / ('%d-%s' % (scale, layout)) / ('trial-%d' % trial) / 'report.json'
                try:
                    facts = load_phase(path, core_hash, scale, layout)
                    phases.append({'scale': scale, 'layout': layout, 'trial': trial,
                                   'path': str(path), 'facts': facts})
                except (OSError, ValueError, KeyError, TypeError, sqlite3.Error) as failure:
                    missing.append({'path': str(path), 'reason': str(failure)})
    growth = []
    for layout in LAYOUTS:
        for trial in TRIALS:
            selected = {row['scale']: row['facts']['memory'] for row in phases
                        if row['layout'] == layout and row['trial'] == trial}
            if len(selected) == 2:
                # Whole-core peak is a conservative bound; do not call it thread RSS.
                delta = selected[1_000_000]['peak_bytes'] - selected[100_000]['peak_bytes']
                growth.append({'layout': layout, 'trial': trial, 'whole_core_peak_growth_bytes': delta})
                if delta > 16 * 1024**2:
                    missing.append({'reason': 'whole-core peak growth exceeds conservative 16MiB bound',
                                    'layout': layout, 'trial': trial})
    idle = [row for row in phases if row['facts'].get('idle', {}).get('seconds', 0) >= 300]
    if not idle:
        missing.append({'reason': 'five-minute unchanged-body/CPU measurement absent'})
    summaries = []
    for scale in SCALES:
        for layout in LAYOUTS:
            selected = [row['facts']['backfill_ms'] for row in phases
                        if row['scale'] == scale and row['layout'] == layout]
            if len(selected) == 3:
                summaries.append({'scale': scale, 'layout': layout, 'backfill_ms': selected,
                                  'median_backfill_ms': statistics.median(selected)})
    return {'result': 'INCOMPLETE' if missing else 'SCALE_MATRIX_PASS_OTHER_GATES_PENDING',
            'core_binary_sha256': core_hash, 'validated_phase_count': len(phases),
            'required_phase_count': 12, 'missing_or_failed': missing, 'growth': growth,
            'snapshots': [{'path': row['path'], **row['facts']['snapshot_validation']} for row in phases],
            'summary': summaries, 'remaining_gates': ['production scheduling/X46/X47',
                'full-scan body I/O', 'X45 real GUI refresh/hidden-window independence',
                'UI synchronous event budget', 'normal/import/oversized-drain shutdown']}


def execute(args, output):
    core_hash = hashlib.sha256(resolve_exe(args.exe).read_bytes()).hexdigest()
    for scale in SCALES:
        for layout in LAYOUTS:
            for trial in TRIALS:
                folder = output / ('%d-%s' % (scale, layout)) / ('trial-%d' % trial)
                folder.mkdir(parents=True, exist_ok=True)
                if args.resume:
                    try:
                        load_phase(folder / 'report.json', core_hash, scale, layout)
                    except (OSError, ValueError, KeyError, TypeError, sqlite3.Error):
                        pass
                    else:
                        print('verified existing %d/%s trial %d' % (scale, layout, trial), flush=True)
                        continue
                command = [sys.executable, '-u', str(Path(__file__).with_name('workbuddy_import_performance.py')),
                           '--exe', str(resolve_exe(args.exe)), '--records', str(scale),
                           '--layout', layout, '--output', str(folder)]
                if scale == 1_000_000 and layout == 'many' and trial == 1:
                    command += ['--idle-seconds', '300']
                print('running %d/%s trial %d' % (scale, layout, trial), flush=True)
                with (folder / 'run.log').open('w') as log:
                    child = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT)
                    try:
                        code = child.wait()
                    except BaseException:
                        child.terminate()
                        try:
                            child.wait(timeout=60)
                        except subprocess.TimeoutExpired:
                            child.kill()
                            child.wait()
                        raise
                if code != 0:
                    return
                phase = json.loads((folder / 'report.json').read_text())
                print('completed %d/%s trial %d cleanup=%s' % (
                    scale, layout, trial, json.dumps(phase['isolation']['temporary_cleanup'])), flush=True)


def main():
    def interrupted(signum, frame):
        raise KeyboardInterrupt('signal %d' % signum)
    signal.signal(signal.SIGTERM, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', default='target/release/workbuddy-proxy')
    parser.add_argument('--output', default='dist/workbuddy-import-validation/S10.03-strict')
    parser.add_argument('--aggregate-only', action='store_true', help='validate existing strict phase reports without running cores')
    parser.add_argument('--resume', action='store_true', help='reuse only fully validated same-binary phase reports and database snapshots')
    args = parser.parse_args()
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    core_hash = hashlib.sha256(resolve_exe(args.exe).read_bytes()).hexdigest()
    if not args.aggregate_only:
        execute(args, output)
    report = aggregate(output, core_hash)
    (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print('%s: %d/12 strict phases' % (report['result'], report['validated_phase_count']))
    return 1 if report['missing_or_failed'] else 0


if __name__ == '__main__':
    sys.exit(main())

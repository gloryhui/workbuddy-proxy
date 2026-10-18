#!/usr/bin/env python3
"""Full synthetic-corpus restart proof through the actual importer.

Uses the existing corpus generator, real HTTP settings and a real core process.
All database access by this probe is read-only. No facts/checkpoints are seeded.
Graceful-stop and SIGKILL cases are distinct; SIGKILL is never graceful-stop proof.
"""
import argparse
from contextlib import closing
import hashlib
import json
from pathlib import Path
import shutil
import signal
import sqlite3
import subprocess
import sys
import time
import traceback

from workbuddy_import_golden import Golden
from workbuddy_import_performance import corpus
from workbuddy_import_test import Report, Sandbox, resolve_exe
from workbuddy_import_ui_sandbox import kernel_protection


class IncompleteObservation(Exception):
    pass


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def rows(fixture, sql, parameters=()):
    with closing(sqlite3.connect(fixture.database.resolve().as_uri() + '?mode=ro', uri=True)) as db:
        return db.execute(sql, parameters).fetchall()


def count(fixture):
    return rows(fixture, 'SELECT COUNT(*) FROM usage_requests')[0][0]


def checkpoints(fixture):
    return {key: (namespace, generation, offset, size) for key, namespace, generation, offset, size
            in rows(fixture, 'SELECT file_key,namespace_id,generation,committed_offset,observed_size '
                             'FROM workbuddy_import_files')}


def source_stamps(fixture):
    result = {}
    for path in fixture.project.glob('scale-*.jsonl'):
        stat = path.stat()
        result[path.name] = [stat.st_size, stat.st_mtime_ns, stat.st_ino]
    return result


def active_run(fixture):
    active = rows(fixture, 'SELECT run_id FROM stats_runs WHERE stopped_at_ms IS NULL')
    if len(active) != 1:
        raise AssertionError('expected exactly one active synthetic core run')
    return active[0][0]


def attest(fixture, report, label):
    if sys.platform == 'darwin':
        proof = kernel_protection(fixture.sandbox.real_home, fixture.sandbox.root,
                                  fixture.process.pid)
        report.facts.setdefault('actual_core_kernel_protection', {})[label] = proof
        if not report.check('resume: %s actual core denies real source/config I/O' % label,
                            proof['enforced'] and proof['pid'] == fixture.process.pid):
            raise AssertionError('actual core source isolation missing')


def stop_core(fixture, mode, output, label, report):
    started = time.monotonic_ns()
    if mode == 'kill':
        fixture.process.kill()
    else:
        fixture.post('/_control/stop', {}, fixture.key)
    fixture.process.wait(timeout=5)
    elapsed = (time.monotonic_ns() - started) / 1e6
    expected = -signal.SIGKILL if mode == 'kill' else 0
    report.check('resume: %s %s process exit' % (label, mode),
                 fixture.process.returncode == expected and elapsed <= 5_000,
                 'elapsed_ms=%.3f exit=%s; kill is not graceful evidence' %
                 (elapsed, fixture.process.returncode))
    if fixture.log is not None:
        fixture.log.close()
        fixture.log = None
    log = fixture.sandbox.root / 'golden-core.log'
    if log.exists():
        shutil.copy2(log, output / (label + '-core.log'))
    return {'mode': mode, 'elapsed_ms': elapsed, 'exit_code': fixture.process.returncode}


def wait_for_partial(fixture, args):
    deadline = time.monotonic() + args.observation_seconds
    while time.monotonic() < deadline:
        value = count(fixture)
        if args.records // 10 <= value < args.records:
            return value
        if value == args.records:
            raise AssertionError('full corpus finished before an interrupted backfill was observed')
        time.sleep(.2)
    raise IncompleteObservation('partial-backfill operational observation ended')


def seek_witness(before, status):
    """A cold read from zero must read at least each published end offset.

    Receipts are published only after their real data read and SQL commit. If
    process-local read bytes are smaller than these absolute end offsets, some
    saved prefix was skipped. Prefetch increases the byte counter, so it cannot
    manufacture a positive witness. No source content or database is modified.
    """
    advanced = {}
    for receipt in status['source_commit_receipts']:
        saved = before.get(receipt['file_key'])
        if (saved and saved[2] > 0 and receipt['full'] is False
                and receipt['offset'] > saved[2]):
            advanced[receipt['file_key']] = {
                'saved_offset': saved[2], 'committed_offset': receipt['offset']}
    covered = sum(item['committed_offset'] for item in advanced.values())
    read = status['source_body_bytes_read']
    completed = {key for key, value in before.items() if value[2] == value[3] > 0}
    partial = {key for key, value in before.items() if 0 < value[2] < value[3]}
    completed_bytes = sum(before[key][2] for key in completed)
    # A many-small-files stop can fall exactly between files: there is then no
    # partial file to seek. Observe new committed data before the completed
    # prefix could have been reread, with receipts only for unfinished files.
    skipped_complete = (not partial and bool(completed)
                        and 0 < read < completed_bytes
                        and status['progress']['bytes_committed'] > completed_bytes
                        and bool(status['source_commit_receipts'])
                        and all(item['file_key'] not in completed
                                for item in status['source_commit_receipts']))
    return {'ok': (bool(advanced) and 0 < read < covered) or skipped_complete,
            'mode': 'partial-file-seek' if advanced else 'completed-file-skip',
            'advanced_saved_files': advanced, 'covered_absolute_offsets': covered,
            'completed_saved_files': len(completed), 'completed_saved_bytes': completed_bytes,
            'actual_data_read_bytes': read}


def observe_resume_seek(fixture, before, old_clock, report):
    deadline = time.monotonic() + 30
    observations = []
    while time.monotonic() < deadline:
        status = fixture.action('stats_workbuddy_status')
        witness = seek_witness(before, status)
        observations.append({'witness': witness,
                             'clock_id': status['monotonic_clock_id'],
                             'writer_batches': status['runtime_metrics']['writer_batch_count']})
        if witness['ok']:
            report.check('resume: early real receipts prove saved-prefix seeking, not cold replay',
                         status['monotonic_clock_id'] != old_clock)
            report.facts['early_seek_proof'] = observations[-16:]
            return
        time.sleep(.05)
    report.facts['early_seek_proof'] = observations[-16:]
    raise AssertionError('no early saved-prefix seek witness was captured')


def partial_dashboard(fixture, data, report):
    lower = count(fixture)
    fields = {'from_ms': data['at_ms'] - 86_400_000, 'to_ms': data['at_ms'] + 1,
              'timezone': 'UTC', 'include_tests': False,
              'source_scope': 'builtin', 'capture_method': 'all'}
    started = time.monotonic_ns()
    response = fixture.action('stats_dashboard', filter=fields, granularity='auto')
    elapsed = (time.monotonic_ns() - started) / 1e6
    visible = response['summary']['request_count']
    upper = count(fixture)
    report.check('resume: real dashboard exposes the committed partial corpus',
                 0 < lower <= visible <= upper < data['records'])
    report.facts['partial_dashboard'] = {'count_before': lower, 'visible': visible,
                                         'count_after': upper, 'elapsed_ms': elapsed}


def restart_existing(fixture, report):
    # Reuse the identical executable, config, source and management port.
    # Golden.start would rewrite config/source models and restart its Mock server.
    before_config = digest(fixture.sandbox.config_dir / 'config.json')
    before_exe = digest(fixture.exe)
    fixture.log = (fixture.sandbox.root / 'golden-core.log').open('wb')
    started = time.monotonic_ns()
    fixture.process = subprocess.Popen([str(fixture.exe), '--headless', '--config-dir',
                                       str(fixture.sandbox.config_dir)],
                                      cwd=fixture.sandbox.root, env=fixture.sandbox.environment(),
                                      stdout=fixture.log, stderr=fixture.log,
                                      **fixture.process_options)
    fixture.until(fixture.ready, 'same-config restarted core readiness')
    report.facts['restart_ready_ms'] = (time.monotonic_ns() - started) / 1e6
    report.check('resume: restart reused the same binary/config without fixture rewrites',
                 digest(fixture.exe) == before_exe
                 and digest(fixture.sandbox.config_dir / 'config.json') == before_config)
    attest(fixture, report, 'after_restart')


def finish_backfill(fixture, args, report):
    deadline = time.monotonic() + args.observation_seconds
    started = time.monotonic_ns()
    observations = []
    next_status = 0
    while time.monotonic() < deadline:
        value = count(fixture)
        if time.monotonic() >= next_status or value == args.records:
            status = fixture.action('stats_workbuddy_status')
            observations.append({'count': value, 'elapsed_ms': (time.monotonic_ns() - started) / 1e6,
                                 'progress': status['progress'], 'complete': status['complete'],
                                 'source_body_bytes_read': status['source_body_bytes_read']})
            report.facts['resumed_observations'] = observations[-32:]
            next_status = time.monotonic() + 5
            if value == args.records and status['complete']:
                return status
        time.sleep(1)
    raise IncompleteObservation('resumed full-corpus operational observation ended')


def verify_final(fixture, args, data, before, namespace, report):
    totals = rows(fixture, 'SELECT COUNT(*),SUM(input_tokens),SUM(output_tokens),SUM(total_tokens),'
                          'SUM(workbuddy_credit_nanos) FROM usage_requests')
    expected = [(args.records, args.records * 100, args.records * 20,
                 args.records * 120, args.records * 1_140_000_000)]
    report.check('resume: exact full count and integer Token/credit totals', totals == expected,
                 repr(totals))
    invalid = rows(fixture, "SELECT COUNT(*) FROM usage_requests WHERE "
                   "input_tokens IS NOT 100 OR output_tokens IS NOT 20 OR total_tokens IS NOT 120 "
                   "OR workbuddy_credit_nanos IS NOT 1140000000 OR usage_status<>'complete' "
                   "OR record_origin<>'workbuddy_builtin' OR outcome<>'recorded'")
    ledger = rows(fixture, "SELECT COUNT(*),COUNT(DISTINCT entry_key),SUM(state='imported') "
                          'FROM workbuddy_import_ledger')
    report.check('resume: every fact and unique ledger identity remains exact',
                 invalid == [(0,)] and ledger == [(args.records,) * 3])
    report.check('resume: credits never create currency cost rows',
                 rows(fixture, 'SELECT COUNT(*) FROM usage_costs') == [(0,)])
    after = checkpoints(fixture)
    regressed = [key for key, old in before.items() if key not in after
                 or after[key][0:2] != old[0:2] or after[key][2] < old[2]]
    report.check('resume: prior namespace/generations/offsets never regress', not regressed)
    report.check('resume: every discovered file reaches its exact EOF',
                 len(after) == data['files'] and all(x[2] == x[3] for x in after.values())
                 and sum(x[2] for x in after.values()) == data['bytes'])
    report.check('resume: namespace and enabled policy survive restart',
                 rows(fixture, 'SELECT workbuddy_namespace_id,workbuddy_import_enabled '
                               'FROM stats_settings') == [(namespace, 1)])
    report.check('resume: foreign key references remain valid',
                 rows(fixture, 'PRAGMA foreign_key_check') == [])
    report.facts.update(final_totals=totals, final_ledger=ledger,
                        checkpoint_count=len(after), checkpoint_regressions=regressed)


def run(fixture, args, output, report):
    data = corpus(fixture, args.records, args.layout)
    minimum = 1024**3 if args.records == 1_000_000 else 128 * 1024**2
    report.facts['corpus'] = data
    report.check('resume: full unique corpus meets declared count/byte/file budget',
                 data['records'] == args.records and data['bytes'] >= minimum
                 and data['files'] == (1 if args.layout == 'single' else 1000))
    stamps = source_stamps(fixture)
    fixture.start(resolve_exe(args.exe))
    report.facts['core_binary_sha256'] = fixture.binary_sha256
    if args.expected_sha256 and fixture.binary_sha256 != args.expected_sha256:
        raise AssertionError('frozen core identity differs from expected')
    attest(fixture, report, 'before_restart')
    fixture.enable()
    wait_for_partial(fixture, args)
    first_run = active_run(fixture)
    partial_dashboard(fixture, data, report)
    cold_status = fixture.action('stats_workbuddy_status')
    report.facts['first_stop'] = stop_core(fixture, args.stop_mode, output, 'before-restart', report)
    stopped_count = count(fixture)
    report.check('resume: interruption really occurred during partial committed backfill',
                 0 < stopped_count < args.records, 'committed=%d' % stopped_count)
    before = checkpoints(fixture)
    report.facts['stopped_checkpoints'] = before
    # A counterfactual checkpoint claim over a genuine first-run observation
    # must fail. This checks the witness against actual zero-start I/O without
    # changing the source, facts or durable checkpoints.
    width = data['bytes'] // data['records']
    cold_claim = {key: (value[0], value[1], min(width, value[2]), value[3])
                  for key, value in before.items() if value[2] > 0}
    cold_witness = seek_witness(cold_claim, cold_status)
    report.check('resume: real cold zero-start I/O rejects a saved-prefix claim',
                 cold_witness['covered_absolute_offsets'] > 0 and not cold_witness['ok'])
    report.facts['cold_start_negative_control'] = {
        'scope': 'Counterfactual in-memory checkpoint claim, genuine initial-run status; no SQL seeding',
        'witness': cold_witness, 'actual_status': cold_status}
    namespace = rows(fixture, 'SELECT workbuddy_namespace_id FROM stats_settings')[0][0]
    report.facts.update(committed_at_stop=stopped_count, checkpoint_count_at_stop=len(before))
    restart_existing(fixture, report)
    second_run = active_run(fixture)
    ready_count = count(fixture)
    report.check('resume: readiness returns before remaining history completes',
                 stopped_count <= ready_count < args.records)
    observe_resume_seek(fixture, before, cold_status['monotonic_clock_id'], report)
    final_status = finish_backfill(fixture, args, report)
    # Each fresh LineReader slice may discard up to one 64KiB prefetch buffer.
    # writer_batch_count counts actual Work::Batch transactions, not metadata.
    # Keep the real read counter intact and bound this known overlap separately.
    chunk = 64 * 1024
    assert 'pub const READ_CHUNK: usize = 64 * 1024;' in (
        Path(__file__).resolve().parents[1] / 'src/stats/workbuddy_import/reader.rs').read_text()
    suffix = data['bytes'] - sum(value[2] for value in before.values())
    batches = final_status['runtime_metrics']['writer_batch_count']
    unfinished = sum(value[2] < value[3] for value in before.values())
    # The final data slice for each unfinished file reaches its actual EOF and
    # cannot prefetch past it. One extra chunk permits an in-flight observation.
    budget = suffix + max(0, batches - unfinished + 1) * chunk
    actual = final_status['source_body_bytes_read']
    report.check('resume: actual reads obey remaining suffix plus bounded per-batch prefetch',
                 0 <= suffix <= actual <= budget and batches > 0
                 and final_status['runtime_metrics']['writer_batch_failures'] == 0,
                 'read=%d suffix=%d budget=%d batches=%d' % (actual, suffix, budget, batches))
    report.facts['resumed_read_budget'] = {
        'remaining_logical_bytes': suffix, 'read_chunk_bytes': chunk,
        'actual_data_batches': batches, 'unfinished_files_at_stop': unfinished,
        'maximum_with_prefetch_bytes': budget,
        'actual_data_read_bytes': actual,
        'counter_scope': 'LineReader read() bytes including prefetch; not physical disk traffic',
        'runtime_metrics': final_status['runtime_metrics']}
    report.facts['source_body_bytes_after_restart'] = final_status['source_body_bytes_read']
    verify_final(fixture, args, data, before, namespace, report)
    report.facts['final_stop'] = stop_core(fixture, 'graceful', output, 'after-restart', report)
    reasons = rows(fixture, 'SELECT run_id,stop_reason FROM stats_runs WHERE run_id IN (?,?)',
                   (first_run, second_run))
    reason_map = dict(reasons)
    report.check('resume: recorded run reasons distinguish graceful exit and actual crash',
                 first_run != second_run and reason_map.get(second_run) == 'normal'
                 and reason_map.get(first_run) == ('unclean' if args.stop_mode == 'kill' else 'normal'))
    report.check('resume: source file metadata remains unchanged', source_stamps(fixture) == stamps)
    report.facts['run_reasons'] = reasons


def cleanup_probe(fixture, sandbox, output, report):
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    signal.signal(signal.SIGINT, signal.SIG_IGN)
    try:
        fixture.close()
    except Exception as failure:
        report.check('resume: fixture close succeeds', False, type(failure).__name__)
    if fixture.process is not None and fixture.process.poll() is None:
        fixture.process.terminate()
        try:
            fixture.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            fixture.process.kill()
            fixture.process.wait()
    stopped = fixture.process is None or fixture.process.poll() is not None
    try:
        sandbox.check_isolated(report)
        if report.failed and stopped and fixture.database.exists():
            with closing(sqlite3.connect(fixture.database.resolve().as_uri() + '?mode=ro', uri=True)) as db:
                with closing(sqlite3.connect(output / 'failed-snapshot.sqlite3')) as saved:
                    db.backup(saved)
    except Exception as failure:
        report.check('resume: final isolation/snapshot checks succeed', False,
                     type(failure).__name__)
    size = sum(p.stat().st_size for p in sandbox.root.rglob('*')
               if p.is_file() and not p.is_symlink())
    handles = None
    if sys.platform == 'darwin':
        handles = subprocess.run(['lsof', '-nP', '+D', str(sandbox.root)],
                                 capture_output=True, text=True)
    safe = stopped and (handles is None or (handles.returncode == 1 and not handles.stdout.strip()))
    if safe:
        try:
            sandbox.close()
        except OSError as failure:
            report.check('resume: owned temporary root removal succeeds', False,
                         type(failure).__name__)
    if sandbox.root.exists():
        sandbox._temp._finalizer.detach()
    report.facts['cleanup'] = {'root': str(sandbox.root), 'core_stopped': stopped,
        'reference_check': {'lsof_exit': handles.returncode,
                            'remaining': handles.stdout} if handles else 'scoped process on non-macOS',
        'removed': not sandbox.root.exists(), 'released_logical_bytes': size if not sandbox.root.exists() else 0,
        'retained_temp_bytes': 0 if not sandbox.root.exists() else size,
        'system_free_bytes': shutil.disk_usage('/').free}
    report.check('resume: process/references exit before owned temporary files are removed',
                 safe and not sandbox.root.exists())


def main():
    def interrupted(signum, _frame):
        raise KeyboardInterrupt('signal %d' % signum)
    signal.signal(signal.SIGTERM, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', required=True)
    parser.add_argument('--expected-sha256')
    parser.add_argument('--records', type=int, choices=(100_000, 1_000_000), default=1_000_000)
    parser.add_argument('--layout', choices=('single', 'many'), required=True)
    parser.add_argument('--stop-mode', choices=('graceful', 'kill'), required=True)
    parser.add_argument('--observation-seconds', type=int, default=3600)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    if args.observation_seconds <= 0 or (args.stop_mode == 'kill' and not hasattr(signal, 'SIGKILL')):
        parser.error('positive observation window and POSIX SIGKILL support are required')
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=False)
    sandbox = Sandbox(prefix='workbuddy-full-resume-')
    fixture = Golden(sandbox)
    report = Report('R09-full-corpus-resume', args.exe, script=Path(__file__).name)
    report.facts.update(records=args.records, layout=args.layout, stop_mode=args.stop_mode,
                        script_sha256=digest(Path(__file__)), no_sql_seeding=True)
    incomplete = interrupted_run = False
    try:
        run(fixture, args, output, report)
    except IncompleteObservation as failure:
        incomplete = True
        report.error = str(failure)
    except KeyboardInterrupt as failure:
        interrupted_run = True
        report.error = str(failure)
    except Exception as failure:
        traceback.print_exc()
        report.check('resume: complete real-process proof without exception', False,
                     type(failure).__name__)
    finally:
        cleanup_probe(fixture, sandbox, output, report)
    report.result = ('FAIL' if report.failed else 'INTERRUPTED' if interrupted_run else
                     'INCOMPLETE_OPERATIONAL_WINDOW' if incomplete else
                     'PASS_FULL_CORPUS_RESUME_OTHER_GATES_PENDING')
    report.write(output / 'report.json')
    return 1 if report.failed else 130 if interrupted_run else 2 if incomplete else 0


if __name__ == '__main__':
    sys.exit(main())

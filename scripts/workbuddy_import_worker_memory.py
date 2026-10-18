"""Full-scale import-only RSS bound, separate from concurrent report readers.

Thread RSS is not independently measurable. A complete fresh core process below
64 MiB conservatively bounds the importer; dashboard caches are measured in the
separate concurrent-query workload, never attributed to the worker.
"""
import hashlib
import json
from pathlib import Path
import shutil
import time

from workbuddy_import_golden import Golden
from workbuddy_import_resources import MemoryMonitor
from workbuddy_import_test import Sandbox, resolve_exe


def measure(args, report, make_corpus, base_count, incomplete_error):
    sandbox = Sandbox(prefix='workbuddy-worker-memory-')
    fixture = Golden(sandbox)
    output = Path(args.output) / 'worker-memory'
    output.mkdir(exist_ok=False)
    observed = {'result': 'RUNNING', 'scale': args.records, 'layout': args.layout,
                'scope': 'complete import-only core; no dashboard/request report queries or query caches',
                'report_queries_during_measurement': 0}
    report.facts['worker_memory_probe'] = observed
    monitor = None
    try:
        data = make_corpus(fixture, args.records, args.layout)
        observed['corpus'] = data
        fixture.start(resolve_exe(args.exe))
        observed['core_binary_sha256'] = fixture.binary_sha256
        monitor = MemoryMonitor(fixture.process.pid)
        monitor.start()
        started = time.monotonic_ns()
        fixture.enable()
        deadline = time.monotonic() + args.backfill_observation_seconds
        while base_count(fixture) != args.records:
            if time.monotonic() >= deadline:
                observed['result'] = 'INCOMPLETE_OPERATIONAL_WINDOW'
                raise incomplete_error('import-only resource observation window ended before full backfill')
            time.sleep(.2)
        fixture.until(lambda: fixture.action('stats_workbuddy_status')['complete'],
                      'worker resource complete checkpoints')
        observed['backfill_ms'] = (time.monotonic_ns() - started) / 1_000_000
        observed['memory'] = monitor.stop()
        monitor = None
        totals = fixture.rows('SELECT COUNT(*),SUM(total_tokens),SUM(workbuddy_credit_nanos) FROM usage_requests')
        ledger = fixture.rows("SELECT COUNT(*),COUNT(DISTINCT entry_key),SUM(state='imported') FROM workbuddy_import_ledger")
        offsets = fixture.rows('SELECT COUNT(*),SUM(committed_offset),SUM(observed_size) FROM workbuddy_import_files')
        money = fixture.rows('SELECT COUNT(*) FROM usage_costs')
        observed.update(totals=totals, ledger=ledger, offsets=offsets, money_rows=money)
        report.check('worker resource: full corpus independently imported with exact tokens/credits/ledger',
                     totals == [(args.records, args.records * 120, args.records * 1_140_000_000)]
                     and ledger == [(args.records,) * 3]
                     and offsets == [(data['files'], data['bytes'], data['bytes'])]
                     and money == [(0,)])
        memory = observed['memory']
        report.check('worker resource: complete import-only core RSS conservatively bounds worker below 64MiB',
                     memory['samples'] > 0 and memory['error'] is None
                     and 0 < memory['peak_bytes'] <= 64 * 1024**2, repr(memory))
        sandbox.check_isolated(report)
        observed['result'] = 'PASS_FULL_IMPORT_WORKER_MEMORY_BOUND' if not report.failed else 'FAIL'
    except BaseException as failure:
        observed['error'] = type(failure).__name__ + ': ' + str(failure)
        if observed['result'] == 'RUNNING':
            observed['result'] = 'FAIL'
        raise
    finally:
        if monitor is not None:
            observed['memory'] = monitor.stop()
        try:
            fixture.close()
        finally:
            stopped = fixture.process is None or fixture.process.poll() is not None
            normal = fixture.process is not None and fixture.process.returncode == 0
            if (sandbox.root / 'golden-core.log').exists():
                shutil.copy2(sandbox.root / 'golden-core.log', output / 'core.log')
            size = sum(p.stat().st_size for p in sandbox.root.rglob('*') if p.is_file())
            if stopped:
                sandbox.close()
            observed['cleanup'] = {'owned_root': str(sandbox.root), 'child_stopped': stopped,
                'normal_exit': normal, 'removed': not sandbox.root.exists(),
                'released_logical_bytes': size if stopped else 0,
                'retained_temp_bytes': 0 if stopped else size,
                'system_free_bytes': shutil.disk_usage('/').free}
            report.check('worker resource: normal child exit before exact fixture cleanup',
                         stopped and normal and not sandbox.root.exists())
            if report.failed and observed['result'] != 'INCOMPLETE_OPERATIONAL_WINDOW':
                observed['result'] = 'FAIL'
            observed['script_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
            (output / 'report.json').write_text(json.dumps(observed, indent=2) + '\n')
    return observed

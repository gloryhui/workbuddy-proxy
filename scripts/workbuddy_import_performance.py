#!/usr/bin/env python3
"""Release-only, measured 100k/128MiB and 1M/1GiB corpus acceptance phases.

A phase report is not the complete S10 gate: the aggregate needs both scales,
both file layouts, growth comparison and every coverage item listed as missing.
"""
import argparse
import hashlib
import sqlite3
from contextlib import closing
import concurrent.futures
import json
import math
import os
import platform
import shutil
import signal
import subprocess
import statistics
import sys
import time
import traceback
from pathlib import Path

from workbuddy_import_golden import Golden, MockUpstream
from workbuddy_import_resources import MemoryMonitor, usage
from workbuddy_import_test import Sandbox, Report, resolve_exe

CANARY = 'SCALE_BODY_MUST_NOT_PERSIST'
CONTRACT = '2026-10-18-background-progress-resource-v2'


class BackfillIncomplete(Exception):
    """Observation window ended; this is neither final accuracy nor a product timeout."""


class StreamingUpstream(MockUpstream):
    def do_POST(self):
        self.rfile.read(int(self.headers.get('Content-Length', 0)))
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Connection', 'close')
        self.end_headers()
        time.sleep(0.04)
        first = {'id': 'scale-mock', 'object': 'chat.completion.chunk', 'model': 'golden-model',
                 'choices': [{'index': 0, 'delta': {'role': 'assistant', 'content': 'x'},
                              'finish_reason': None}]}
        last = {'id': 'scale-mock', 'object': 'chat.completion.chunk', 'model': 'golden-model',
                'choices': [{'index': 0, 'delta': {}, 'finish_reason': 'stop'}],
                'usage': {'prompt_tokens': 10, 'completion_tokens': 2, 'total_tokens': 12}}
        self.wfile.write(('data: ' + json.dumps(first) + '\n\n').encode())
        self.wfile.flush()
        time.sleep(0.01)
        self.wfile.write(('data: ' + json.dumps(last) + '\n\ndata: [DONE]\n\n').encode())
        self.wfile.flush()


def line(session, message, at, model='scale-base', minimum=0):
    value = {'type': 'function_call', 'sessionId': session, 'timestamp': at,
             'content': '', 'providerData': {'messageId': message, 'requestModelId': 'hy3',
                 'model': model, 'rawUsage': {'prompt_tokens': 100, 'completion_tokens': 20,
                                             'total_tokens': 120, 'credit': 1.14}}}
    value['content'] = CANARY
    raw = json.dumps(value, separators=(',', ':'))
    if len(raw) + 1 < minimum:
        value['content'] += 'x' * (minimum - len(raw) - 1)
        raw = json.dumps(value, separators=(',', ':'))
    return raw + '\n'


def corpus(fixture, records, layout):
    fixture.project.mkdir(parents=True, exist_ok=True)
    files = 1 if layout == 'single' else 1_000
    minimum = math.ceil((128 * 1024**2 if records == 100_000 else 1024**3) / records)
    at = int(time.time() * 1000)
    written = 0
    index = 0
    for number in range(files):
        session = 'scale-%04d' % number
        count = records // files + int(number < records % files)
        with (fixture.project / (session + '.jsonl')).open('w') as output:
            for _ in range(count):
                raw = line(session, 'scale-%08d' % index, at, minimum=minimum)
                output.write(raw)
                written += len(raw.encode())
                index += 1
            output.flush()
            os.fsync(output.fileno())
    return {'records': index, 'files': files, 'bytes': written, 'at_ms': at}


def p95(samples):
    return sorted(samples)[math.ceil(len(samples) * 0.95) - 1]


def one_stream(fixture):
    import urllib.request
    payload = {'model': 'golden-model', 'messages': [{'role': 'user', 'content': 'synthetic'}],
               'stream': True, 'stream_options': {'include_usage': True}}
    request = urllib.request.Request('http://127.0.0.1:%d/r/golden-route/v1/chat/completions' % fixture.port,
        data=json.dumps(payload).encode(), headers={'Authorization': 'Bearer fixture-local-key',
                                                   'Content-Type': 'application/json'})
    start = time.monotonic_ns()
    first = None
    complete = False
    with fixture.opener.open(request, timeout=20) as response:
        for raw in response:
            if not raw.startswith(b'data: '):
                continue
            data = raw[6:].strip()
            if data == b'[DONE]':
                complete = True
                continue
            chunk = json.loads(data)
            if any(choice.get('delta', {}).get('content') for choice in chunk.get('choices', [])):
                first = first or time.monotonic_ns()
    if first is None or not complete:
        raise AssertionError('stream did not deliver content and DONE')
    return (first - start) / 1_000_000


def streams(fixture):
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        return list(pool.map(lambda _: one_stream(fixture), range(200)))


def base_count(fixture):
    return fixture.rows("SELECT COUNT(*) FROM usage_requests WHERE upstream_model='scale-base'")[0][0]


def backfill(fixture, records, report, at, observation_seconds):
    for _ in range(8):
        one_stream(fixture)
    off = streams(fixture)
    fixture.wait_rows("SELECT COUNT(*) FROM usage_requests WHERE record_origin='proxy'", [(208,)])
    monitor = MemoryMonitor(fixture.process.pid)
    monitor.start()
    started = time.monotonic_ns()
    observations = []
    report.facts['background_observations'] = observations
    try:
        fixture.enable()
        on = streams(fixture)
        deadline = time.monotonic() + observation_seconds
        while True:
            lower = base_count(fixture)
            status_started = time.monotonic_ns()
            status = fixture.action('stats_workbuddy_status')
            status_ms = (time.monotonic_ns() - status_started) / 1_000_000
            fields = {'from_ms': at - 86_400_000, 'to_ms': at + 1,
                      'timezone': 'UTC', 'include_tests': False,
                      'source_scope': 'builtin', 'capture_method': 'all'}
            query_started = time.monotonic_ns()
            raw = fixture.raw_action('stats_dashboard', filter=fields, granularity='auto')
            query_ms = (time.monotonic_ns() - query_started) / 1_000_000
            if not raw.get('ok', False):
                code = str(raw.get('message', '')).split(':', 1)[0]
                safe_code = code if code in {
                    'stats_query_timeout', 'stats_query_too_large', 'stats_busy',
                    'stats_unavailable', 'stats_invalid_argument',
                } else 'unclassified_error'
                report.facts.setdefault('background_query_failures', []).append({
                    'elapsed_ms': (time.monotonic_ns() - started) / 1_000_000,
                    'count_before_request': lower, 'query_ms': query_ms,
                    'code': safe_code, 'progress_before_request': status['progress'],
                })
                raise AssertionError('background dashboard failed: ' + safe_code)
            query = json.loads(raw['message'])
            upper = base_count(fixture)
            visible = query['summary']['request_count']
            progress = status['progress']
            observation = {'elapsed_ms': (time.monotonic_ns() - started) / 1_000_000,
                           'count_before': lower, 'query_visible': visible, 'count_after': upper,
                           'status_ms': status_ms, 'query_ms': query_ms,
                           'progress': progress, 'complete': status['complete']}
            observations.append(observation)
            report.check('scale: background query shows committed portion within count bracket',
                         lower <= visible <= upper <= records, repr(observation))
            report.check('scale: background checkpoint bytes remain internally consistent',
                         progress['bytes_committed'] + progress['bytes_remaining'] == progress['bytes_total']
                         and 0 <= progress['files_remaining'] <= progress['files_total'])
            if upper == records and status['complete']:
                break
            if time.monotonic() >= deadline:
                raise BackfillIncomplete('operational observation window ended with %d/%d committed' % (upper, records))
            time.sleep(5)
        elapsed_ms = (time.monotonic_ns() - started) / 1_000_000
    finally:
        memory = monitor.stop()
        report.facts['background_query_memory'] = memory
    fixture.wait_rows("SELECT COUNT(*) FROM usage_requests WHERE record_origin='proxy'", [(408,)])
    report.check('scale: exact unique record and credit totals imported', fixture.rows(
        "SELECT COUNT(*),SUM(total_tokens),SUM(workbuddy_credit_nanos) FROM usage_requests "
        "WHERE upstream_model='scale-base'") == [(records, records * 120, records * 1_140_000_000)])
    report.check('scale: background backfill eventually completes with partial results observable',
                 any(0 < row['query_visible'] < records for row in observations)
                 and observations[-1]['complete'], 'elapsed_ms=%.1f; no product completion deadline' % elapsed_ms)
    report.check('scale: discovery publishes a known total while backlog remains',
                 any(row['progress']['total_known'] and row['progress']['files_remaining'] > 0
                     for row in observations))
    report.check('scale: queries remain responsive while history backfills',
                 p95([row['query_ms'] for row in observations]) <= 500,
                 'background query P95=%.3fms' % p95([row['query_ms'] for row in observations]))
    report.check('scale: known checkpoint size matches the full discovered corpus',
                 all(row['progress']['bytes_total'] == report.facts['corpus']['bytes']
                     and row['progress']['files_total'] == report.facts['corpus']['files']
                     for row in observations if row['progress']['total_known']))
    report.check('scale: concurrent report/import whole-core RSS is measured separately from worker bound',
                 memory['samples'] > 0 and memory['error'] is None and memory['peak_bytes'] > 0, repr(memory))
    increase = p95(on) - p95(off)
    report.check('scale: four concurrent streams keep first-content P95 within budget',
                 increase <= max(10, p95(off) * 0.1),
                 'off=%.3fms on=%.3fms increase=%.3fms' % (p95(off), p95(on), increase))
    report.check('scale: all 400 proxy streams persisted without new event loss',
                 fixture.action('stats_health')['dropped_requests_current_run'] == 0)
    return {'backfill_ms': elapsed_ms, 'background_query_memory': memory,
            'off_stream_ms': off, 'on_stream_ms': on}


def incremental(fixture, report):
    samples = []
    report.facts['incremental'] = samples
    target = fixture.project / 'scale-0000.jsonl'
    file_key = fixture.rows("SELECT file_key FROM workbuddy_import_files "
                            "WHERE relative_path='projects/golden/scale-0000.jsonl'")[0][0]
    clock_id = fixture.action('stats_workbuddy_status')['monotonic_clock_id']
    for index in range(20):
        with target.open('a') as output:
            for row in range(500):
                output.write(line('scale-0000', 'inc-%02d-%03d' % (index, row),
                                  int(time.time() * 1000), model='scale-incremental'))
            output.flush()
            os.fsync(output.fileno())
        flushed = time.monotonic_ns()
        deadline = flushed + 10_000_000_000
        first_visible = None
        count = 0
        while time.monotonic_ns() < deadline:
            count = fixture.rows("SELECT COUNT(*) FROM usage_requests WHERE upstream_model='scale-incremental'")[0][0]
            if count == (index + 1) * 500:
                first_visible = time.monotonic_ns()
                break
            time.sleep(0.02)
        elapsed = None if first_visible is None else (first_visible - flushed) / 1_000_000
        sent = time.monotonic_ns()
        status = fixture.action('stats_workbuddy_status')
        received = time.monotonic_ns()
        receipt = status.get('last_source_commit') or {}
        server_now = int(status['monotonic_elapsed_ns'])
        commit = int(receipt.get('commit_ack_elapsed_ns', '-1'))
        commit_interval = [sent + commit - server_now, received + commit - server_now]
        valid_receipt = (status['monotonic_clock_id'] == clock_id and receipt.get('file_key') == file_key
                         and receipt.get('offset') == target.stat().st_size and receipt.get('full') is False
                         and 0 <= commit <= server_now)
        report.check('scale: sample %02d has a matching post-commit monotonic receipt' % index,
                     valid_receipt and commit_interval[1] >= flushed
                     and commit_interval[1] - flushed <= 10_000_000_000)
        samples.append({'flushed_monotonic_ns': flushed, 'query_visible_monotonic_ns': first_visible,
                        'commit_ack_process_elapsed_ns': commit, 'clock_id': clock_id,
                        'commit_ack_client_monotonic_interval_ns': commit_interval,
                        'calibration_request_interval_ns': [sent, received],
                        'elapsed_ms': elapsed, 'records_visible': count})
        report.check('scale: 500-row incremental sample %02d visible within 10s' % index,
                     elapsed is not None and elapsed <= 10_000, 'elapsed_ms=%s' % elapsed)
        if elapsed is None:
            break
        # Cover different phases, including a source update immediately after a tick.
        time.sleep((index % 5) * 0.23)
    report.check('scale: all 20 incremental samples executed', len(samples) == 20)
    return samples


def query_warmup(fixture, command, args, count, report, observation):
    """Bounded preheat; preserve cold timeout observations and verify the guard.

    Only the documented timeout code is expected during cold I/O. All measured
    calls after preheat still require success and the unchanged 500ms P95.
    """
    report.facts.setdefault('query_warmups', []).append(observation)
    for _ in range(3):
        started = time.monotonic_ns()
        response = fixture.raw_action(command, **args)
        elapsed = (time.monotonic_ns() - started) / 1_000_000
        code = None if response['ok'] else response.get('message', '').split(':', 1)[0]
        observation['attempts'].append({'ok': response['ok'], 'elapsed_ms': elapsed, 'code': code})
        if response['ok']:
            data = json.loads(response['message'])
            actual = data['summary']['request_count'] if command == 'stats_dashboard' else data['total_rows']
            if actual != count:
                raise AssertionError('warm-up query count differs from known corpus')
            return elapsed
        if code != 'stats_query_timeout':
            raise AssertionError('unexpected query failure during warm-up')
        report.check('scale: cold query returns the existing timeout code within deadline plus transport margin',
                     elapsed <= 2_250, 'elapsed_ms=%.3f' % elapsed)
    raise AssertionError('query did not warm up within three attempts')


def query_rounds(fixture, at, count, report, million):
    # Query page-cache memory is separate from import-worker memory. Keep its
    # real whole-core peak visible rather than attributing it to the worker or
    # omitting the cost of report acceleration from the evidence.
    # Hundreds of query calls span minutes. Sampling once a second avoids
    # launching ten ps subprocesses per second during latency measurements;
    # the import-phase monitor retains its original 100ms sampling.
    monitor = MemoryMonitor(fixture.process.pid, interval=1.0 if million else 0.1)
    monitor.start()
    try:
        return measured_query_rounds(fixture, at, count, report, million)
    finally:
        report.facts['report_reader_memory'] = monitor.stop()


def measured_query_rounds(fixture, at, count, report, million):
    rounds = []
    report.facts['queries'] = rounds
    for number in range(3 if million else 1):
        for days in (1, 30):
            fields = {'from_ms': at - days * 86_400_000, 'to_ms': at + 1,
                      'timezone': 'UTC', 'include_tests': False, 'source_scope': 'all', 'capture_method': 'all'}
            for command in ('stats_dashboard', 'stats_requests'):
                samples = []
                measured = {'round': number, 'days': days, 'command': command, 'calls': []}
                report.facts.setdefault('query_measurements', []).append(measured)
                args = {'filter': fields, 'granularity': 'auto'} if command == 'stats_dashboard' else {
                    'filter': fields, 'page_size': 20}
                warmup = {'round': number, 'days': days, 'command': command, 'attempts': []}
                warmup_ms = query_warmup(fixture, command, args, count, report, warmup)
                for _ in range(50 if million else 3):
                    started = time.monotonic_ns()
                    raw = fixture.raw_action(command, **args)
                    elapsed = (time.monotonic_ns() - started) / 1_000_000
                    code = None if raw['ok'] else raw.get('message', '').split(':', 1)[0]
                    measured['calls'].append({'elapsed_ms': elapsed, 'ok': raw['ok'], 'code': code})
                    if not raw['ok']:
                        raise AssertionError('measured query failed: ' + command)
                    response = json.loads(raw['message'])
                    samples.append(elapsed)
                    actual = response['summary']['request_count'] if command == 'stats_dashboard' else response['total_rows']
                    if actual != count:
                        raise AssertionError('query count differs from independently known corpus')
                peak = p95(samples)
                report.check('scale: round%d %dd %s P95 <=500ms' % (number, days, command),
                             peak <= 500, 'p95_ms=%.3f' % peak)
                rounds.append({'round': number, 'days': days, 'command': command, 'p95_ms': peak,
                               'samples_ms': samples, 'warmup_ms': warmup_ms,
                               'warmup_attempts': warmup['attempts']})
    return rounds


def idle(fixture, seconds, report):
    before = fixture.action('stats_workbuddy_status')['source_body_bytes_read']
    _, previous = usage(fixture.process.pid)
    last = time.monotonic()
    samples = []
    for _ in range(seconds):
        time.sleep(1)
        _, current = usage(fixture.process.pid)
        now = time.monotonic()
        samples.append((current - previous) / (now - last) * 100)
        previous, last = current, now
    after = fixture.action('stats_workbuddy_status')['source_body_bytes_read']
    report.check('scale: unchanged file body reads remain zero through idle window',
                 before == after, 'seconds=%d bytes_before=%d bytes_after=%d' % (seconds, before, after))
    report.check('scale: idle core CPU median stays below 1 percent of one core',
                 statistics.median(samples) < 1, 'median=%.3f%%' % statistics.median(samples))
    return {'seconds': seconds, 'body_bytes_before': before, 'body_bytes_after': after,
            'cpu_percent_samples': samples}



def runtime_observations(fixture, report):
    observed = fixture.action('stats_workbuddy_status')['runtime_metrics']
    peak = observed['queue_peak']
    report.check('scale: actual import queue peak is nonzero and bounded',
                 1 <= peak <= 2 and 0 <= observed['queue_depth'] <= peak, repr(observed))
    report.check('scale: production source I/O was observed only on its OS worker',
                 observed['source_io_thread_checks'] > 0
                 and observed['source_io_thread_violations'] == 0)
    batches = observed['recent_batches']
    report.check('scale: actual writer transaction durations and bounded samples recorded',
                 observed['writer_batch_count'] > 0
                 and observed['writer_batch_failures'] == 0
                 and 0 < observed['writer_batch_max_us'] <= observed['writer_batch_total_us']
                 and 1 <= len(batches) <= 32
                 and all(row['committed'] and 0 <= row['candidates'] <= 128
                         and 0 <= row['elapsed_us'] <= observed['writer_batch_max_us']
                         for row in batches))
    return observed


def run(args, fixture, report):
    from workbuddy_import_worker_memory import measure
    worker = measure(args, report, corpus, base_count, BackfillIncomplete)
    if worker['result'] != 'PASS_FULL_IMPORT_WORKER_MEMORY_BOUND':
        raise AssertionError('complete import-only resource probe failed')
    report.facts['memory'] = worker['memory']
    data = corpus(fixture, args.records, args.layout)
    started = time.monotonic_ns()
    fixture.start(resolve_exe(args.exe))
    report.facts['startup_ms_with_source_present'] = (time.monotonic_ns() - started) / 1_000_000
    threshold = 128 * 1024**2 if args.records == 100_000 else 1024**3
    report.check('scale: corpus meets exact count and minimum byte budget',
                 data['records'] == args.records and data['bytes'] >= threshold, repr(data))
    results = report.facts
    results.update({'corpus': data, 'layout': args.layout, 'scale': args.records,
                    'core_binary_sha256': fixture.binary_sha256,
                    'acceptance_contract': CONTRACT,
                    'operational_observation_seconds': args.backfill_observation_seconds})
    results.update(backfill(fixture, args.records, report, data['at_ms'], args.backfill_observation_seconds))
    results['incremental'] = incremental(fixture, report)
    results['queries'] = query_rounds(fixture, data['at_ms'], args.records,
                                     report, args.records == 1_000_000)
    results['runtime_metrics'] = runtime_observations(fixture, report)
    if args.idle_seconds:
        results['idle'] = idle(fixture, args.idle_seconds, report)
    results['missing_coverage'] = ['new-project discovery during competing backfill', 'full-scan body I/O',
        'both scale/layout phase aggregation and 16MiB RSS-growth comparison']
    if args.idle_seconds < 300:
        results['missing_coverage'].append('five-minute idle measurement')
    if args.records < 1_000_000:
        results['missing_coverage'].append('1M rows and 3x50 query samples')
    return results


def main():
    # SIGTERM must take the same fixture shutdown/cleanup path as Ctrl+C.
    def interrupted(signum, frame):
        raise KeyboardInterrupt('signal %d' % signum)
    signal.signal(signal.SIGTERM, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', default='target/release/workbuddy-proxy')
    parser.add_argument('--records', type=int, choices=(100_000, 1_000_000), required=True)
    parser.add_argument('--layout', choices=('many', 'single'), required=True)
    parser.add_argument('--idle-seconds', type=int, choices=(0, 300), default=0)
    parser.add_argument('--output', required=True)
    parser.add_argument('--backfill-observation-seconds', type=int, default=3600,
                        help='operational observation window, not a product completion deadline')
    args = parser.parse_args()
    if args.backfill_observation_seconds <= 0:
        parser.error('--backfill-observation-seconds must be positive')
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    sandbox = Sandbox(prefix='workbuddy-scale-')
    fixture = Golden(sandbox, handler_class=StreamingUpstream)
    report = Report('R09-scale-phase', args.exe, script=Path(__file__).name)
    report.facts['script_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    machine = {'system': platform.system(), 'release': platform.release(),
               'architecture': platform.machine(), 'logical_cpus': os.cpu_count(),
               'processor': platform.processor(), 'python': platform.python_version()}
    if hasattr(os, 'getloadavg'):
        machine['load_average_at_start'] = list(os.getloadavg())
    if platform.system() == 'Darwin':
        machine['processor'] = subprocess.check_output(
            ['sysctl', '-n', 'machdep.cpu.brand_string'], text=True).strip()
        machine['macos_version'] = platform.mac_ver()[0]
    report.facts['machine'] = machine
    results = {}
    incomplete = None
    try:
        results = run(args, fixture, report)
    except BackfillIncomplete as failure:
        incomplete = str(failure)
        report.facts['incomplete_reason'] = incomplete
    except (Exception, KeyboardInterrupt) as failure:
        traceback.print_exc()
        report.check('scale: phase completed without exception', False, type(failure).__name__)
    finally:
        try:
            if fixture.database.exists() and fixture.process is not None and fixture.process.poll() is None:
                with closing(sqlite3.connect(fixture.database.resolve().as_uri() + '?mode=ro', uri=True)) as source:
                    with closing(sqlite3.connect(output / 'usage-snapshot.sqlite3')) as destination:
                        source.backup(destination)
                        destination.execute('PRAGMA journal_mode=DELETE')
        except (OSError, sqlite3.Error) as failure:
            report.check('scale: diagnostic database snapshot saved', False, type(failure).__name__)
        finally:
            try:
                fixture.close()
            except Exception as failure:
                report.check('scale: isolated fixture closed cleanly', False, type(failure).__name__)
            finally:
                try:
                    sandbox.check_isolated(report)
                except Exception as failure:
                    report.check('scale: isolation attestation completed', False, type(failure).__name__)
                temporary_bytes = sum(p.stat().st_size for p in sandbox.root.rglob('*')
                                      if p.is_file() and not p.is_symlink())
                try:
                    sandbox.close()
                except OSError as failure:
                    report.check('scale: isolated sandbox cleaned up', False, type(failure).__name__)
                report.facts['temporary_cleanup'] = {
                    'path': str(sandbox.root), 'children_stopped_before_cleanup':
                        fixture.process is None or fixture.process.poll() is not None,
                    'removed': not sandbox.root.exists(), 'removed_logical_bytes': temporary_bytes,
                    'system_disk_available_bytes': shutil.disk_usage('/').free,
                    'external_disk_available_bytes': shutil.disk_usage(output).free,
                    'retained_evidence_bytes': sum(p.stat().st_size for p in output.rglob('*')
                                                 if p.is_file() and not p.is_symlink())}
                report.check('scale: no fixture process or temporary files remain',
                             report.facts['temporary_cleanup']['children_stopped_before_cleanup']
                             and report.facts['temporary_cleanup']['removed'])
    report.result = ('FAIL' if report.failed else 'INCOMPLETE_OPERATIONAL_WINDOW' if incomplete
                     else 'PHASE_PASS_GATE_INCOMPLETE')
    report.facts.update(results)
    report.write(output / 'report.json')
    return 1 if report.failed else 2 if incomplete else 0


if __name__ == '__main__':
    sys.exit(main())

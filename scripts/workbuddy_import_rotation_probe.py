#!/usr/bin/env python3
"""R03: actual release rotation across 1137 distinct files, beyond the 1000-file scale corpus.

No new normal-increment latency claim is made above the contract's 1000-file
limit. The finite waits are operational probe windows. Every fact must enter
through source JSONL and the production reader/writer; no SQL seeds are used.
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
import subprocess
import sys
import time
import traceback

from workbuddy_import_golden import Golden
from workbuddy_import_test import CANARIES, Report, Sandbox, resolve_exe
from workbuddy_import_ui_sandbox import kernel_protection

FILES = 1137
RECORDS_PER_FILE = 2


def append(path, session, message, at):
    row = {'type': 'function_call', 'sessionId': session, 'timestamp': at,
           'content': CANARIES[0], 'providerData': {
               'messageId': message, 'requestModelId': 'hy3', 'model': 'rotation-probe',
               'rawUsage': {'prompt_tokens': 100, 'completion_tokens': 20,
                            'total_tokens': 120, 'credit': 1.14}}}
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('ab') as output:
        output.write(json.dumps(row, separators=(',', ':')).encode() + b'\n')
        output.flush()
        os.fsync(output.fileno())


def totals(fixture):
    return fixture.rows('SELECT COUNT(*),COUNT(DISTINCT request_id),SUM(input_tokens),'
                        'SUM(output_tokens),SUM(total_tokens),SUM(workbuddy_credit_nanos) '
                        'FROM usage_requests')


def expected_totals(count):
    return [(count, count, 100 * count, 20 * count, 120 * count, 1_140_000_000 * count)]


def id_hash(value):
    # DATA-API identity contract: u32 big-endian UTF-8 byte length + bytes.
    encoded = value.encode()
    return hashlib.sha256(len(encoded).to_bytes(4, 'big') + encoded).hexdigest()


def status(fixture):
    return fixture.action('stats_workbuddy_status')


def caught_up(fixture, count):
    value = status(fixture)
    return (value['complete'] and value['imported'] == count
            and value['progress']['total_known']
            and value['progress']['files_remaining'] == 0)


def run(args):
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    sandbox = Sandbox(prefix='workbuddy-rotation-1137-')
    owner = sandbox.root / 'session-owner.json'
    fixture = None
    report = Report('R03 production rotation above1000 files', args.exe, Path(__file__).name)
    report.facts.update(script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                        files=FILES, records_per_file=RECORDS_PER_FILE,
                        no_sql_seeding=True, normal_increment_latency_claim=False)
    try:
        sandbox._temp._finalizer.detach()
        owner.write_text(json.dumps({'thread': '01a148ae-2068-7800-a8d2-904539bc30d3',
                                     'pid': os.getpid(), 'purpose': 'R03 1137-file rotation'}))
        fixture = Golden(sandbox)
        at = int(time.time() * 1000)
        paths = []
        for index in range(FILES):
            session = 'rotation-%04d' % index
            path = sandbox.source_root / 'projects' / ('轮转 项目 %d' % (index % 3)) / (session + '.jsonl')
            for row in range(RECORDS_PER_FILE):
                append(path, session, '%s-message-%d' % (session, row), at + row)
            paths.append((path, session))
        startup = time.monotonic()
        fixture.start(resolve_exe(args.exe))
        report.facts.update(core_binary_sha256=fixture.binary_sha256,
                            startup_ms_with_sources=(time.monotonic() - startup) * 1000)
        if sys.platform == 'darwin':
            protection = kernel_protection(sandbox.real_home, sandbox.root, fixture.process.pid)
            report.facts['actual_core_kernel_protection'] = protection
            if not report.check('rotation: actual core denies real source/config I/O', protection['enforced']):
                raise AssertionError('actual core source guard missing')
        fixture.enable()
        fixture.until(lambda: caught_up(fixture, FILES * RECORDS_PER_FILE),
                      'all1137 files discovered and committed', timeout=90)

        def check_files(description):
            wanted = sorted((str(path.relative_to(sandbox.source_root)), path.stat().st_size,
                             path.stat().st_size) for path, _ in paths)
            actual = fixture.rows('SELECT relative_path,committed_offset,observed_size '
                                  'FROM workbuddy_import_files ORDER BY relative_path')
            report.check(description, actual == wanted,
                         'expected_files=%d actual_files=%d' % (FILES, len(actual)))
            progress = status(fixture)['progress']
            total = sum(path.stat().st_size for path, _ in paths)
            report.check(description + ': progress covers every file and byte',
                         progress['files_total'] == FILES and progress['files_remaining'] == 0
                         and progress['bytes_total'] == progress['bytes_committed'] == total
                         and progress['bytes_remaining'] == 0)

        check_files('rotation: each1137-file checkpoint equals its own EOF')
        report.check('rotation: all2274 distinct source facts have exact tokens and credit',
                     totals(fixture) == expected_totals(FILES * RECORDS_PER_FILE))
        before = status(fixture)['source_body_bytes_read']
        for _ in range(3):
            time.sleep(5.2)
            report.check('rotation: unchanged5s round does not reread source bodies',
                         status(fixture)['source_body_bytes_read'] == before)

        changed = [0, FILES // 2, FILES - 1]
        for index in changed:
            path, session = paths[index]
            append(path, session, '%s-late' % session, at + 1000)
        flushed = time.monotonic_ns()
        fixture.until(lambda: caught_up(fixture, FILES * RECORDS_PER_FILE + len(changed)),
                      'front middle and final files progress after initial discovery', timeout=90)
        report.facts['above1000_append_observation_ms'] = (time.monotonic_ns() - flushed) / 1e6
        for index in changed:
            session = paths[index][1]
            session_hash = id_hash(session)
            message_hash = id_hash(session + '-late')
            # A total-row increase cannot substitute for the target identity.
            report.check('rotation: appended target identity in file%d is durable' % index,
                         fixture.rows("SELECT COUNT(*) FROM workbuddy_import_ledger "
                                      "WHERE session_hash='%s' AND message_hash='%s' "
                                      "AND state='imported'" % (session_hash, message_hash)) == [(1,)])
        check_files('rotation: changed front middle and final checkpoints reach EOF')
        count = FILES * RECORDS_PER_FILE + len(changed)
        report.check('rotation: appended facts preserve exact counts/tokens/credit',
                     totals(fixture) == expected_totals(count))
        before = status(fixture)['source_body_bytes_read']
        full = fixture.action('stats_workbuddy_scan', mode='full')
        def full_completed():
            current = status(fixture)
            return current.get('scan_id') == full['scan_id'] and current['scan_state'] == 'completed'
        fixture.until(full_completed,
                      'full1137-file check completion', timeout=90)
        report.check('rotation: full scan actually rereads the complete source corpus',
                     status(fixture)['source_body_bytes_read'] - before >=
                     sum(path.stat().st_size for path, _ in paths))
        report.check('rotation: full replay keeps the exact facts and unique ledger',
                     totals(fixture) == expected_totals(count)
                     and fixture.rows('SELECT COUNT(*),COUNT(DISTINCT entry_key) '
                                      'FROM workbuddy_import_ledger') == [(count, count)])

        with closing(sqlite3.connect(fixture.database.as_uri() + '?mode=ro', uri=True)) as observer:
            observer.execute('SELECT COUNT(*) FROM stats_runs').fetchone()
            stopped = time.monotonic()
            fixture.post('/_control/stop', {}, fixture.key)
            fixture.process.wait(timeout=5)
            runs = observer.execute('SELECT stopped_at_ms,stop_reason FROM stats_runs').fetchall()
        report.check('rotation: normal stop persists the run within5s',
                     fixture.process.returncode == 0 and time.monotonic() - stopped <= 5
                     and all(end is not None and reason == 'normal' for end, reason in runs))
        fixture.process = subprocess.Popen([str(fixture.exe), '--headless', '--config-dir',
            str(sandbox.config_dir)], cwd=sandbox.root, env=sandbox.environment(),
            stdout=fixture.log, stderr=fixture.log)
        fixture.until(fixture.ready, 'rotation restart readiness')
        fixture.until(lambda: caught_up(fixture, count), 'rotation restart checkpoint recovery', timeout=90)
        check_files('rotation: restart restores every committed file checkpoint')
        report.check('rotation: restart never duplicates the source facts',
                     totals(fixture) == expected_totals(count))
        sandbox.check_isolated(report)
    except (Exception, KeyboardInterrupt) as failure:
        traceback.print_exc()
        report.error = type(failure).__name__ + ': ' + str(failure)
        report.check('rotation: complete production probe', False, report.error)
    finally:
        try:
            if fixture is not None:
                fixture.close()
        except Exception as failure:
            report.check('rotation: close fixture cleanly', False, type(failure).__name__)
        finally:
            stopped = fixture is None or fixture.process is None or fixture.process.poll() is not None
            handles = subprocess.run(['lsof', '-nP', '+D', str(sandbox.root)],
                                     capture_output=True, text=True)
            logical = sum(p.stat().st_size for p in sandbox.root.rglob('*') if p.is_file())
            if (sandbox.root / 'golden-core.log').exists():
                shutil.copy2(sandbox.root / 'golden-core.log', output / 'core.log')
            clean = stopped and handles.returncode == 1 and not handles.stdout.strip()
            if clean:
                if owner.exists():
                    assert json.loads(owner.read_text())['pid'] == os.getpid()
                shutil.rmtree(sandbox.root)
            report.facts['temporary_cleanup'] = {
                'owned_root': str(sandbox.root), 'children_stopped': stopped,
                'removed': not sandbox.root.exists(), 'lsof_exit': handles.returncode,
                'released_logical_bytes': logical if clean else 0,
                'retained_temp_bytes': 0 if clean else logical,
                'system_free_bytes': shutil.disk_usage('/').free}
            report.check('rotation: stopped and waited before exact owned-root cleanup', clean)
            report.result = 'FAIL' if report.failed else 'PASS_1137_FILE_ROTATION_OTHER_GATES_PENDING'
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

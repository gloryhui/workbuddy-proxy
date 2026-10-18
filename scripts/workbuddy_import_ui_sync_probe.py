#!/usr/bin/env python3
"""Data half of X45. Native refresh/page/minimize actions require CUA evidence.

Only the named isolated GUI fixture receives synthetic JSONL. This probe never
sets a UI option through a private API and never declares GUI acceptance alone.
"""
import argparse
import hashlib
import json
import os
import sqlite3
import subprocess
import time
import urllib.request
from contextlib import closing
from pathlib import Path

from workbuddy_import_performance import line

REVIEW = Path(__file__).resolve().parents[1] / 'dist/workbuddy-import-review'


def status(manifest, config):
    instance = json.loads((config / 'instance.json').read_text())
    if instance['pid'] != manifest['core_pid'] or instance['port'] != manifest['port']:
        raise ValueError('fixture core identity changed during GUI verification')
    key = instance['management_key']
    request = urllib.request.Request('http://127.0.0.1:%d/_control/action' % manifest['port'],
        data=json.dumps({'command': 'stats_workbuddy_status'}).encode(),
        headers={'Authorization': 'Bearer ' + key, 'Content-Type': 'application/json'})
    sent = time.monotonic_ns()
    with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(request, timeout=5) as response:
        value = json.loads(response.read())
    received = time.monotonic_ns()
    return sent, received, json.loads(value['message'])


def sample(manifest, name, owned_root=None):
    folder = Path(manifest['folder']).resolve()
    legacy = folder.parent == REVIEW.resolve() and folder.name.startswith('ui-release-')
    external = owned_root is not None and folder == Path(owned_root).resolve()
    if external:
        owner = json.loads((folder / 'session-owner.json').read_text())
        recorded = json.loads((folder / 'fixture.json').read_text())
        if not owner.get('thread') or recorded != manifest:
            raise ValueError('external fixture lacks matching session ownership and manifest')
        os.kill(owner['pid'], 0)
        ui = folder / manifest['runtime_ui_binary']
        if not ui.resolve().is_relative_to(folder):
            raise ValueError('external fixture UI path escapes its owned root')
        command = subprocess.check_output(
            ['ps', '-p', str(manifest['ui_pid']), '-o', 'command='], text=True).strip()
        if command != str(ui) and not command.startswith(str(ui) + ' '):
            raise ValueError('actual external fixture UI identity differs')
        from workbuddy_import_ui_sandbox import kernel_protection
        from workbuddy_import_test import real_user_home
        if not kernel_protection(real_user_home(), folder, manifest['core_pid'])['enforced']:
            raise ValueError('actual external fixture core lacks source protection')
    if not legacy and not external:
        raise ValueError('not the isolated review fixture')
    source = folder / 'home/.workbuddy/projects/demo/ui-refresh.jsonl'
    config = folder / 'config'
    database = config / 'usage.sqlite3'
    if not source.resolve().is_relative_to(folder) or not database.resolve().is_relative_to(folder):
        raise ValueError('fixture path escapes its isolated root')
    model = 'ui-refresh-' + name
    _, _, before = status(manifest, config)
    if before['effective_enabled'] is not True:
        raise ValueError('fixture importer is not effectively enabled')
    if source.exists():
        if source.stat().st_size > 65_536:
            raise ValueError('unexpected source fixture size')
        with source.open() as previous:
            if any(json.loads(raw)['providerData']['messageId'] == 'refresh-' + name for raw in previous):
                raise ValueError('sample was already written; use a fresh GUI fixture')
    row = json.loads(line('ui-refresh', 'refresh-' + name, int(time.time() * 1000), model))
    row['providerData']['rawUsage'].update(prompt_tokens=10, completion_tokens=2, total_tokens=12, credit=0.1)
    with source.open('a') as output:
        output.write(json.dumps(row, separators=(',', ':')) + '\n')
        output.flush()
        os.fsync(output.fileno())
    flushed = time.monotonic_ns()
    actual = None
    with closing(sqlite3.connect(database.resolve().as_uri() + '?mode=ro', uri=True)) as connection:
        while time.monotonic_ns() - flushed <= 10_000_000_000:
            actual = connection.execute('SELECT COUNT(*),SUM(total_tokens),SUM(workbuddy_credit_nanos) '
                'FROM usage_requests WHERE upstream_model=?', (model,)).fetchone()
            if actual == (1, 12, 100_000_000):
                break
            time.sleep(0.02)
        visible = time.monotonic_ns()
        key = connection.execute('SELECT file_key FROM workbuddy_import_files WHERE relative_path=?',
                                 ('projects/demo/ui-refresh.jsonl',)).fetchone()
    sent, received, value = status(manifest, config)
    receipt = next((row for row in value['source_commit_receipts'] if key and row['file_key'] == key[0]), {})
    commit = int(receipt.get('commit_ack_elapsed_ns', '-1'))
    now = int(value['monotonic_elapsed_ns'])
    interval = [sent + commit - now, received + commit - now]
    ok = (value['monotonic_clock_id'] == before['monotonic_clock_id'] and actual == (1, 12, 100_000_000) and visible - flushed <= 10_000_000_000
          and 0 <= commit <= now and receipt.get('offset') == source.stat().st_size
          and receipt.get('full') is False and flushed <= interval[1] <= flushed + 10_000_000_000)
    return {'sample': name, 'ok': ok, 'actual_count_tokens_credit': actual,
            'gui_kernel_guard': bool(manifest.get('gui_kernel_guard')),
            'isolation_scope': 'owned fixture and live scoped UI; actual core kernel protection checked; GUI guard only if explicitly enabled',
            'source_flush_ns': str(flushed), 'query_visible_ns': str(visible),
            'commit_ack_client_interval_ns': [str(item) for item in interval],
            'clock_id': value['monotonic_clock_id'], 'elapsed_ms': (visible - flushed) / 1_000_000}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture', default=str(REVIEW / 'ui-live-current.json'))
    parser.add_argument('--sample', required=True, choices=('off', '5', '10', '30', '60', 'page-hidden', 'window-minimized'))
    parser.add_argument('--output', required=True)
    parser.add_argument('--owned-root', help='explicit session-owned external fixture; requires owner marker, matching live UI manifest and protected core PID')
    args = parser.parse_args()
    manifest = json.loads(Path(args.fixture).read_text())
    try:
        result = sample(manifest, args.sample, args.owned_root)
    except Exception as failure:
        result = {'sample': args.sample, 'ok': False, 'error': type(failure).__name__}
    result.update({'result': 'DATA_SYNC_PASS_GUI_ATTESTATION_REQUIRED' if result['ok'] else 'FAIL',
                   'fixture': manifest['folder'],
                   'script_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()})
    path = Path(args.output)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(result, indent=2) + '\n')
    print('%s %s elapsed_ms=%s' % (result['sample'], result['result'], result.get('elapsed_ms')))
    return 0 if result['ok'] else 1


if __name__ == '__main__':
    raise SystemExit(main())

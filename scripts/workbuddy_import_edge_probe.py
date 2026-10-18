#!/usr/bin/env python3
"""Real reader/writer/API edge cases absent from the scale corpus.

All facts come from synthetic source files. Before dense pagination only the
surrogate IDs of four existing imported facts are raised above 2**53, explicitly
priming SQLite allocation for a query boundary. All measured high-ID facts are
then created by the production writer from new source messages. No desired fact
is inserted with SQL. Unix special files live only in the owned sandbox.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import sqlite3
from contextlib import closing
import sys
import time
import traceback

from workbuddy_import_golden import Golden, MockUpstream
from workbuddy_import_test import CANARIES, Report, Sandbox, resolve_exe, scan_canaries
from workbuddy_import_ui_sandbox import kernel_protection


class EdgeSandbox(Sandbox):
    def environment(self):
        env = super().environment()
        if hasattr(self, 'catalog_url'):
            env['WORKBUDDY_PROXY_CATALOG_TEST_URL'] = self.catalog_url
        return env


class EdgeUpstream(MockUpstream):
    catalog_requests = 0

    def do_GET(self):
        if self.path != '/catalog':
            return super().do_GET()
        type(self).catalog_requests += 1
        self.reply({'edge-lab': {
            'api': 'http://127.0.0.1:%d/v1' % self.server.server_port,
            'models': {
                'golden-model': {'cost': {'input': 2, 'output': 4}},
                'edge-synthetic': {'cost': {'input': 2, 'output': 4}},
            }}})


def record(session, message, at):
    return {'type': 'function_call', 'id': 'top-' + message, 'sessionId': session,
            'timestamp': at, 'content': CANARIES[0],
            'providerData': {'messageId': message, 'requestModelId': 'hy3',
                             'model': 'edge-synthetic',
                             'rawUsage': {'prompt_tokens': 100, 'completion_tokens': 20,
                                          'total_tokens': 120, 'credit': 1.14}}}


def encoded(row):
    return json.dumps(row, ensure_ascii=False, separators=(',', ':')).encode()


def write(path, data, append=False):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('ab' if append else 'wb') as output:
        output.write(data)
        output.flush()
        os.fsync(output.fileno())


def scan(fixture, mode):
    fixture.action('stats_workbuddy_scan', mode=mode)
    fixture.until(lambda: fixture.action('stats_workbuddy_status')['scan_state'] == 'completed',
                  'actual %s scan completion' % mode)


def wait_task(fixture, task_id, description):
    def completed():
        task = fixture.action('stats_workbuddy_status').get('mapping_task') or {}
        return task.get('task_id') == task_id and task.get('state') == 'completed'
    fixture.until(completed, description)


def run(args):
    assert os.environ.get('TMPDIR', '').startswith('/Volumes/1TB/')
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    report = Report('R09 production edges X01/X03/X12/X16/X34/X41/X42', args.exe, Path(__file__).name)
    sandbox = EdgeSandbox(prefix='workbuddy-import-edge-')
    # If shutdown fails, preserve this exact root rather than let the garbage
    # collector delete files that could still belong to a running child.
    sandbox._temp._finalizer.detach()
    fixture = Golden(sandbox, handler_class=EdgeUpstream)
    sandbox.catalog_url = 'http://127.0.0.1:%d/catalog' % fixture.server.server_port
    unix_socket = None
    try:
        fixture.start(resolve_exe(args.exe))
        if sys.platform == 'darwin':
            actual_core_guard = kernel_protection(sandbox.real_home, sandbox.root,
                                                 pid=fixture.process.pid)
            report.facts['actual_core_kernel_protection'] = actual_core_guard
            if not report.check('edge: actual production core PID denies real source/config I/O',
                                actual_core_guard['enforced']):
                raise AssertionError('actual core kernel protection is not enforced')
        fixture.enable()
        report.facts.update(core_binary_sha256=fixture.binary_sha256,
                            script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest())
        at = int(time.time() * 1000) - 60_000
        original = record('shared', 'same-message', at)
        first = fixture.project / 'shared.jsonl'
        write(first, encoded(original) + b'\n')
        fixture.wait_rows('SELECT COUNT(*) FROM usage_requests', [(1,)])
        before = fixture.rows('SELECT request_id,started_at_ms,total_tokens,workbuddy_credit_nanos FROM usage_requests')
        report.check('X03: initial source identity has exact timestamp and usage',
                     len(before) == 1 and before[0][1:] == (at, 120, 1_140_000_000), repr(before))
        changed = copy.deepcopy(original)
        changed['id'] = 'different-top-level-id'
        changed['timestamp'] = at + 1000
        write(first, encoded(changed) + b'\n', append=True)
        changed['timestamp'] = at + 2000
        second = sandbox.source_root / 'projects/second-project/shared.jsonl'
        write(second, encoded(changed) + b'\n')
        scan(fixture, 'full')
        checkpoint_sql = ('SELECT relative_path,committed_offset FROM workbuddy_import_files '
                          'WHERE relative_path LIKE \'%/shared.jsonl\' ORDER BY relative_path')
        expected = [('projects/golden/shared.jsonl', first.stat().st_size),
                    ('projects/second-project/shared.jsonl', second.stat().st_size)]
        fixture.wait_rows(checkpoint_sql, expected)
        report.check('X01: both real project-file checkpoints commit to their own complete EOF',
                     fixture.rows(checkpoint_sql) == expected, repr(fixture.rows(checkpoint_sql)))
        report.check('X01/X03: different file, top-level id and timestamp preserve exactly one first fact',
                     fixture.rows('SELECT request_id,started_at_ms,total_tokens,workbuddy_credit_nanos FROM usage_requests') == before
                     and fixture.rows('SELECT COUNT(*),COUNT(DISTINCT entry_key),SUM(state=\'imported\') '
                                      'FROM workbuddy_import_ledger') == [(1, 1, 1)])

        utf8 = fixture.project / 'utf8.jsonl'
        body = (b'\xef\xbb\xbf' + encoded(record('utf8', 'before-bad', at + 3000)) + b'\r\n'
                + b'\xff\xfe\n' + encoded(record('utf8', 'after-bad', at + 3001)) + b'\r\n')
        write(utf8, body)
        scan(fixture, 'incremental')
        fixture.wait_rows('SELECT COUNT(*) FROM usage_requests', [(3,)])
        utf8_state = fixture.rows("SELECT committed_offset,diagnostic_counts_json FROM workbuddy_import_files "
                                  "WHERE relative_path='projects/golden/utf8.jsonl'")
        report.check('X12: BOM/CRLF and malformed UTF-8 preserve both neighbouring complete facts',
                     fixture.rows('SELECT COUNT(*),SUM(total_tokens),SUM(workbuddy_credit_nanos) FROM usage_requests')
                     == [(3, 360, 3_420_000_000)]
                     and len(utf8_state) == 1 and utf8_state[0][0] == len(body)
                     and sum(json.loads(utf8_state[0][1]).values()) >= 1, repr(utf8_state))

        if os.name != 'posix' or not hasattr(os, 'mkfifo'):
            raise RuntimeError('Unix special-file scenario needs a POSIX host')
        outside = sandbox.root / 'outside-source'
        outside.mkdir()
        write(outside / 'outside.jsonl', encoded(record('outside', 'must-not-read', at)) + b'\n')
        (fixture.project / 'outside.jsonl').symlink_to(outside / 'outside.jsonl')
        (sandbox.source_root / 'projects/linked-project').symlink_to(outside, target_is_directory=True)
        (fixture.project / 'directory.jsonl').mkdir()
        os.mkfifo(fixture.project / 'pipe.jsonl', 0o600)
        unix_socket = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        # AF_UNIX has a short sun_path limit on macOS. A relative bind keeps
        # the socket inside this long external fixture root without /tmp.
        original_cwd = os.open('.', os.O_RDONLY)
        try:
            os.chdir(fixture.project)
            unix_socket.bind('socket.jsonl')
        finally:
            os.fchdir(original_cwd)
            os.close(original_cwd)
        write(fixture.project / 'nested/deeper.jsonl', encoded(record('deeper', 'must-not-read', at)) + b'\n')
        io_before = fixture.action('stats_workbuddy_status')['source_body_bytes_read']
        positive = fixture.project / 'regular.jsonl'
        positive_body = encoded(record('regular', 'positive-control', at + 4000)) + b'\n'
        write(positive, positive_body)
        started = time.monotonic()
        scan(fixture, 'incremental')
        fixture.wait_rows('SELECT COUNT(*) FROM usage_requests', [(4,)])
        io_after = fixture.action('stats_workbuddy_status')['source_body_bytes_read']
        files = fixture.rows('SELECT relative_path FROM workbuddy_import_files ORDER BY relative_path')
        expected_files = [('projects/golden/regular.jsonl',), ('projects/golden/shared.jsonl',),
                          ('projects/golden/utf8.jsonl',), ('projects/second-project/shared.jsonl',)]
        report.check('X16: FIFO/socket/directory/nested and outside symlinks never enter source checkpoints',
                     files == expected_files and time.monotonic() - started < 30, repr(files))
        report.check('X16: exact source body I/O delta equals the readable regular positive control only',
                     io_after - io_before == len(positive_body),
                     'body_delta=%d expected=%d' % (io_after - io_before, len(positive_body)))
        unix_socket.close()
        unix_socket = None

        blocked = fixture.project / 'blocked.jsonl'
        write(blocked, b'{malformed-utf8:\xff}\n')
        os.chmod(blocked, 0)
        try:
            # The healthy file advances its own complete checkpoint even while
            # the other real source file is unreadable. Its replay adds no fact.
            write(positive, positive_body, append=True)
            fixture.action('stats_workbuddy_scan', mode='incremental')
            fixture.wait_rows("SELECT committed_offset FROM workbuddy_import_files "
                              "WHERE relative_path='projects/golden/regular.jsonl'",
                              [(positive.stat().st_size,)])
            fixture.until(lambda: fixture.action('stats_workbuddy_status').get('last_error_code')
                          == 'stats_workbuddy_source_read_failed', 'actual unreadable source error')
            degraded = fixture.action('stats_workbuddy_status')
            with fixture.opener.open('http://127.0.0.1:%d/health' % fixture.port, timeout=3) as response:
                original_health = json.load(response)
            report.check('X43: readable neighbour continues while source status truthfully reports incomplete',
                         degraded['state'] == 'degraded' and degraded['complete'] is False
                         and original_health['ready'] is True
                         and fixture.rows('SELECT COUNT(*) FROM usage_requests') == [(4,)])
        finally:
            os.chmod(blocked, 0o600)
        scan(fixture, 'full')
        recovered = fixture.rows("SELECT committed_offset,last_error_code,diagnostic_counts_json "
                                 "FROM workbuddy_import_files WHERE relative_path='projects/golden/blocked.jsonl'")
        report.check('X43: permission recovery consumes malformed bytes with diagnostics, preserving exact facts',
                     len(recovered) == 1 and recovered[0][0] == blocked.stat().st_size
                     and recovered[0][1] is None
                     and sum(json.loads(recovered[0][2]).values()) >= 1
                     and fixture.rows('SELECT COUNT(*),SUM(total_tokens) FROM usage_requests') == [(4, 480)],
                     repr(recovered))
        report.facts['unreadable_source'] = {'degraded_state': degraded['state'],
            'complete_while_unreadable': degraded['complete'], 'original_proxy_health_ready': original_health['ready'],
            'recovered_checkpoint': recovered}

        saved = fixture.raw_action('save', provider_id='golden-provider', name='WorkBuddy')
        report.check('X42: actual configured provider can share the imported display name', saved['ok'])
        config_snapshot = fixture.action('snapshot')
        configured = json.loads((sandbox.config_dir / 'config.json').read_text())['providers']
        options = fixture.action('stats_filters', kind='provider', limit=100,
                                 source_scope='all', capture_method='all')['items']
        collisions = {(item['key'], item['label']) for item in options
                      if item['label'] == 'WorkBuddy'}
        report.check('X42: same provider label retains distinct real and imported filter IDs',
                     collisions == {('golden-provider', 'WorkBuddy'), ('wb:builtin', 'WorkBuddy')},
                     repr(sorted(collisions)))
        report.check('X42: imported provider never becomes a configured provider',
                     config_snapshot['providers'] == 1 and config_snapshot['models'] == 1
                     and len(configured) == 1 and configured[0]['id'] == 'golden-provider'
                     and configured[0]['name'] == 'WorkBuddy')
        targets = [{'provider_id': 'golden-provider',
                    'upstream': 'http://127.0.0.1:%d/v1' % fixture.server.server_port,
                    'model': 'golden-model'}]
        actual_targets = [{'provider_id': provider['id'], 'upstream': provider['upstream'], 'model': model['model']}
                          for provider in configured for model in provider['models']]
        report.check('X42: configured target list remains the fixed one-provider/one-model fixture',
                     actual_targets == targets, repr(actual_targets))
        target_key = hashlib.sha256(json.dumps(targets, separators=(',', ':')).encode()).hexdigest()
        price_sync = fixture.action('stats_prices_update')['catalog_sync']
        price_providers = fixture.rows('SELECT DISTINCT provider_id FROM stats_model_prices ORDER BY provider_id')
        report.check('X42: actual price sync target digest covers configured providers only',
                     price_sync['targets_key'] == target_key and EdgeUpstream.catalog_requests > 0
                     and price_providers == [('golden-provider',)], repr(price_providers))
        report.facts['provider_collision'] = {'filter_ids_and_labels': sorted(collisions),
            'configured_provider_ids': [p['id'] for p in configured],
            'price_provider_ids': price_providers, 'expected_target_digest': target_key,
            'actual_target_digest': price_sync['targets_key'], 'local_catalog_requests': EdgeUpstream.catalog_requests}

        # This is an explicit isolated storage-boundary setup, not importer
        # proof: change only surrogate IDs, preserving every existing fact field.
        # SQLite then allocates the following 205 real source imports above 2**53.
        with closing(sqlite3.connect(fixture.database)) as database:
            columns = [row[1] for row in database.execute('PRAGMA table_info(usage_requests)')
                       if row[1] != 'id']
            projection = ','.join('"' + name + '"' for name in columns)
            before_prime = database.execute('SELECT ' + projection +
                                            ' FROM usage_requests ORDER BY request_id').fetchall()
            with database:
                database.execute('UPDATE usage_requests SET id=id+?', (2 ** 53,))
            after_prime = database.execute('SELECT ' + projection +
                                           ' FROM usage_requests ORDER BY request_id').fetchall()
        report.check('X41 setup: isolated surrogate-ID priming changes no fact field or identity',
                     len(before_prime) == 4 and before_prime == after_prime)
        report.facts['high_id_priming_scope'] = {
            'sql': 'UPDATE usage_requests SET id=id+2**53',
            'existing_source_facts': 4, 'fact_fields_and_request_ids_unchanged': before_prime == after_prime,
            'measured_facts_inserted_by_sql': 0,
            'purpose': 'force actual production writer IDs beyond IEEE754 safe integer range'}
        tie_at = at + 10_000
        dense = fixture.project / 'dense.jsonl'
        write(dense, b''.join(encoded(record('dense', 'm-%03d' % i, tie_at)) + b'\n'
                              for i in range(205)))
        scan(fixture, 'incremental')
        fixture.wait_rows('SELECT COUNT(*) FROM usage_requests', [(209,)])
        numeric_ids = fixture.rows('SELECT MIN(id),MAX(id),COUNT(*) FROM usage_requests WHERE started_at_ms=%d' % tie_at)
        report.check('X41: all 205 newly imported production facts really have IDs above 2**53',
                     len(numeric_ids) == 1 and numeric_ids[0][0] > 2 ** 53
                     and numeric_ids[0][1] - numeric_ids[0][0] == 204 and numeric_ids[0][2] == 205, repr(numeric_ids))
        report.facts['production_dense_numeric_id_range'] = numeric_ids
        expected_ids = [row[0] for row in fixture.rows('SELECT request_id FROM usage_requests '
                       'WHERE started_at_ms=%d ORDER BY started_at_ms DESC,id DESC' % tie_at)]
        ids, page_sizes, seen_cursors = [], [], set()
        cursor = None
        first_cursor = None
        first_as_of = None
        while True:
            fields = {'from_ms': tie_at, 'to_ms': tie_at + 1, 'timezone': 'UTC',
                      'source_scope': 'builtin', 'capture_method': 'all', 'include_tests': False}
            payload = {'filter': fields, 'page_size': 20}
            if cursor:
                payload['cursor'] = cursor
            page = fixture.action('stats_requests', **payload)
            if first_as_of is None:
                first_as_of = page['as_of_id']
            # Existing request pagination counts once, then returns -1 on
            # continuation pages. The frozen set is proved by as_of and the
            # independently known 205 IDs, not a repeated count query.
            report.check('X41: page %02d retains snapshot and existing count protocol' % len(page_sizes),
                         page['total_rows'] == (205 if cursor is None else -1)
                         and page['as_of_id'] == first_as_of)
            ids.extend(item['request_id'] for item in page['items'])
            page_sizes.append(len(page['items']))
            cursor = page['next_cursor']
            if not cursor:
                break
            if first_cursor is None:
                first_cursor = copy.deepcopy(cursor)
            cursor_key = json.dumps(cursor, sort_keys=True, separators=(',', ':'))
            if cursor_key in seen_cursors or len(page_sizes) > 11:
                raise AssertionError('cursor repeated or more pages than the fixed corpus')
            seen_cursors.add(cursor_key)
        report.check('X41: dense identical timestamps paginate all 205 production IDs once in SQL order',
                     len(ids) == len(set(ids)) == 205 and ids == expected_ids
                     and page_sizes == [20] * 10 + [5], repr(page_sizes))
        final_totals = fixture.rows('SELECT COUNT(*),SUM(total_tokens),SUM(workbuddy_credit_nanos) FROM usage_requests')
        report.check('edge: all 209 source facts retain exact tokens, credits and unique imported ledger identities',
                     final_totals == [(209, 25_080, 238_260_000_000)]
                     and fixture.rows("SELECT COUNT(*),COUNT(DISTINCT entry_key),SUM(state='imported') FROM workbuddy_import_ledger") == [(209, 209, 209)]
                     and fixture.rows('SELECT COUNT(*) FROM usage_costs') == [(0,)], repr(final_totals))
        report.facts['before_cursor_mutations_source_fact_totals'] = final_totals
        # Reuse the actual first page cursor after a new, later-allocated source
        # fact arrives at the same timestamp. The frozen set must stay at 205.
        write(dense, encoded(record('dense', 'late-added', tie_at)) + b'\n', append=True)
        scan(fixture, 'incremental')
        fixture.wait_rows('SELECT COUNT(*) FROM usage_requests', [(210,)])
        frozen_ids, cursor, frozen_pages = [], copy.deepcopy(first_cursor), 0
        while cursor:
            page = fixture.action('stats_requests', filter=fields, page_size=20, cursor=cursor)
            report.check('X34: frozen continuation %02d retains pre-append watermark' % frozen_pages,
                         page['total_rows'] == -1 and page['as_of_id'] == first_as_of)
            frozen_ids.extend(item['request_id'] for item in page['items'])
            cursor = page['next_cursor']
            frozen_pages += 1
            if frozen_pages > 10:
                raise AssertionError('frozen pagination exceeded expected pages')
        report.check('X34: source append never duplicates, shifts or enters the frozen old pages',
                     frozen_ids == expected_ids[20:] and len(set(frozen_ids)) == 185)
        fresh = fixture.action('stats_requests', filter=fields, page_size=20)
        report.check('X34: a new first page observes the newly committed source identity',
                     fresh['total_rows'] == 206)
        other_scope = dict(fields, source_scope='all')
        rejected = fixture.raw_action('stats_requests', filter=other_scope, page_size=20,
                                      cursor=copy.deepcopy(first_cursor))
        report.check('X34: actual API rejects a cursor reused under a different source scope',
                     rejected['ok'] is False
                     and rejected.get('message', '').split(':', 1)[0] == 'stats_cursor_expired')
        conflicting = record('dense', 'm-000', tie_at)
        conflicting['providerData']['rawUsage'].update(prompt_tokens=101, total_tokens=121)
        write(dense, encoded(conflicting) + b'\n', append=True)
        scan(fixture, 'incremental')
        fixture.wait_rows("SELECT COUNT(*) FROM workbuddy_import_ledger WHERE state='conflict'", [(1,)])
        rejected = fixture.raw_action('stats_requests', filter=fields, page_size=20,
                                      cursor=copy.deepcopy(first_cursor))
        report.check('X34: structural withdrawal expires the actual old request cursor',
                     rejected['ok'] is False
                     and rejected.get('message', '').split(':', 1)[0] == 'stats_cursor_expired')
        final_totals = fixture.rows('SELECT COUNT(*),SUM(total_tokens),SUM(workbuddy_credit_nanos) FROM usage_requests')
        final_ledger = fixture.rows("SELECT state,COUNT(*) FROM workbuddy_import_ledger GROUP BY state ORDER BY state")
        report.check('X04/X34: conflicting source withdraws only its fact and preserves exact remaining data',
                     final_totals == [(209, 25_080, 238_260_000_000)]
                     and final_ledger == [('conflict', 1), ('imported', 209)]
                     and fixture.rows('SELECT COUNT(*) FROM usage_costs') == [(0,)],
                     repr((final_totals, final_ledger)))
        report.facts['post_append_and_conflict'] = {'facts': final_totals, 'ledger_states': final_ledger,
            'frozen_page_ids': frozen_ids, 'fresh_page_rows_after_append': fresh['total_rows']}
        # A real attribution withdrawal must invalidate request cursors too,
        # independently of the conflict path exercised above.
        manual_model = 'custom-local:edge/manual'
        manual = record('manual', 'mapping-cursor', tie_at)
        manual['providerData']['requestModelId'] = manual_model
        write(fixture.project / 'manual.jsonl', encoded(manual) + b'\n')
        scan(fixture, 'incremental')
        fixture.wait_rows("SELECT COUNT(*) FROM workbuddy_import_ledger WHERE state='pending_classification'", [(1,)])
        preview = fixture.action('stats_workbuddy_mapping_preview', model_id=manual_model,
                                 from_ms=tie_at, to_ms=tie_at + 1, classification='direct')
        report.check('X26/X34: real manual preview covers exactly the one pending source identity',
                     preview['affected_count'] == 1)
        task = fixture.action('stats_workbuddy_mapping_apply', token=preview['token'])
        fixture.wait_rows("SELECT COUNT(*) FROM usage_requests WHERE record_origin='workbuddy_custom'", [(1,)])
        wait_task(fixture, task['task_id'], 'direct task committed')
        all_fields = dict(fields, source_scope='all')
        mapped_page = fixture.action('stats_requests', filter=all_fields, page_size=20)
        report.check('X34: current all-source page includes the newly materialized direct fact',
                     mapped_page['total_rows'] == 206 and mapped_page['next_cursor'] is not None)
        revoke = fixture.action('stats_workbuddy_mapping_revoke_preview',
                                mapping_id=task['mapping_id'], expected_revision=1)
        report.check('X26/X34: revoke preview freezes precisely one materialized source fact',
                     revoke['affected_count'] == 1)
        revoke_task = fixture.action('stats_workbuddy_mapping_revoke', token=revoke['token'])
        fixture.wait_rows("SELECT COUNT(*) FROM usage_requests WHERE record_origin='workbuddy_custom'", [(0,)])
        wait_task(fixture, revoke_task['task_id'], 'revoke task committed')
        rejected = fixture.raw_action('stats_requests', filter=all_fields, page_size=20,
                                     cursor=mapped_page['next_cursor'])
        report.check('X34: actual attribution revoke expires the previously issued request cursor',
                     rejected['ok'] is False
                     and rejected.get('message', '').split(':', 1)[0] == 'stats_cursor_expired')
        final_ledger = fixture.rows("SELECT state,COUNT(*) FROM workbuddy_import_ledger GROUP BY state ORDER BY state")
        final_totals = fixture.rows('SELECT COUNT(*),SUM(total_tokens),SUM(workbuddy_credit_nanos) FROM usage_requests')
        report.check('X26/X34: revoke preserves unrelated facts and the conflict, restoring one pending identity',
                     final_totals == [(209, 25_080, 238_260_000_000)]
                     and final_ledger == [('conflict', 1), ('imported', 209), ('pending_classification', 1)]
                     and fixture.rows('SELECT COUNT(*) FROM usage_costs') == [(0,)],
                     repr((final_totals, final_ledger)))
        report.facts['post_mapping_revoke'] = {'facts': final_totals, 'ledger_states': final_ledger,
            'new_rule_id': task['mapping_id'], 'request_cursor_expired': rejected['ok'] is False}
        expired_preview = fixture.action('stats_workbuddy_mapping_preview', model_id=manual_model,
                                         from_ms=tie_at, to_ms=tie_at + 1, classification='direct')
        expiry_started = time.monotonic()
        body_before_expiry = fixture.action('stats_workbuddy_status')['source_body_bytes_read']
        while time.monotonic() - expiry_started < 61:
            time.sleep(0.25)
        expired = fixture.raw_action('stats_workbuddy_mapping_apply', token=expired_preview['token'])
        expiry_elapsed = time.monotonic() - expiry_started
        report.check('X26: real monotonic 60-second preview expiry is rejected by the actual API',
                     expiry_elapsed >= 61 and expired['ok'] is False
                     and expired.get('message', '').split(':', 1)[0] == 'stats_workbuddy_stale_batch',
                     'elapsed_seconds=%.3f' % expiry_elapsed)
        body_after_expiry = fixture.action('stats_workbuddy_status')['source_body_bytes_read']
        report.check('edge idle: unchanged ordinary and special-file source bodies stay unread',
                     body_after_expiry == body_before_expiry,
                     'before=%d after=%d' % (body_before_expiry, body_after_expiry))
        stale = fixture.action('stats_workbuddy_mapping_preview', model_id=manual_model,
                               from_ms=tie_at, to_ms=tie_at + 1, classification='direct')
        separate = fixture.action('stats_workbuddy_mapping_preview', model_id=manual_model,
                                  from_ms=tie_at + 2, to_ms=tie_at + 3, classification='direct')
        report.check('X26 CAS setup: disjoint intervening rule covers no source fact',
                     separate['affected_count'] == 0 and stale['affected_count'] == 1)
        intervening = fixture.action('stats_workbuddy_mapping_apply', token=separate['token'])
        wait_task(fixture, intervening['task_id'], 'intervening empty task committed')
        conflict = fixture.raw_action('stats_workbuddy_mapping_apply', token=stale['token'])
        report.check('X26: actual API rejects an old preview after an intervening mapping revision',
                     conflict['ok'] is False
                     and conflict.get('message', '').split(':', 1)[0] == 'stats_workbuddy_mapping_conflict')
        report.check('X26: expired and revision-conflicted previews leave every fact and ledger unchanged',
                     fixture.rows('SELECT COUNT(*),SUM(total_tokens),SUM(workbuddy_credit_nanos) FROM usage_requests')
                     == final_totals
                     and fixture.rows("SELECT state,COUNT(*) FROM workbuddy_import_ledger GROUP BY state ORDER BY state")
                     == final_ledger)
        report.facts['actual_preview_rejection'] = {'real_expiry_seconds': expiry_elapsed,
            'expired_error': expired.get('message', '').split(':', 1)[0],
            'revision_conflict_error': conflict.get('message', '').split(':', 1)[0],
            'unchanged_body_bytes_before': body_before_expiry, 'unchanged_body_bytes_after': body_after_expiry}
        report.facts.update(original_fact=before, shared_file_checkpoints=expected,
                            utf8_checkpoint=utf8_state, source_body_io_delta=io_after - io_before,
                            positive_regular_bytes=len(positive_body), dense_page_sizes=page_sizes,
                            dense_ordered_request_ids=ids)
        hits = scan_canaries(sandbox.config_dir, [('core-log', (sandbox.root / 'golden-core.log').read_text()),
                                                ('api-page-identities', json.dumps(ids))])
        report.check('X44: source canaries absent from real DB/config/log/API identity results', not hits, repr(hits))
        sandbox.check_isolated(report)
    except (Exception, KeyboardInterrupt) as failure:
        traceback.print_exc()
        report.error = type(failure).__name__ + ': ' + str(failure)
        report.check('edge chain completed', False, report.error)
    finally:
        if unix_socket is not None:
            unix_socket.close()
        try:
            fixture.close()
        finally:
            stopped = fixture.process is None or fixture.process.poll() is not None
            size = sum(p.stat().st_size for p in sandbox.root.rglob('*')
                       if p.is_file() and not p.is_symlink())
            if (sandbox.root / 'golden-core.log').exists():
                shutil.copy2(sandbox.root / 'golden-core.log', output / 'core.log')
            if stopped:
                # The finalizer was deliberately detached at creation. Its
                # cleanup() is a no-op on Python 3.9; remove this exact owned
                # root explicitly after the sole child and mock server stop.
                shutil.rmtree(sandbox.root)
            report.facts['temporary_cleanup'] = {'path': str(sandbox.root),
                'children_stopped_before_cleanup': stopped, 'removed': not sandbox.root.exists(),
                'removed_logical_bytes': size if stopped else 0,
                'retained_temp_bytes': 0 if stopped else size,
                'system_free_bytes': shutil.disk_usage('/').free,
                'retained_evidence_bytes': sum(p.stat().st_size for p in output.rglob('*') if p.is_file())}
            report.check('edge: stopped child before cleaning the exact owned root',
                         stopped and not sandbox.root.exists())
            report.result = 'PASS_PRODUCTION_EDGE_CASES_OTHER_GATES_PENDING' if not report.failed else 'FAIL'
            report.write(output / 'report.json')
    print('%s: %d passed / %d failed' % (report.result, report.passed, report.failed_count))
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

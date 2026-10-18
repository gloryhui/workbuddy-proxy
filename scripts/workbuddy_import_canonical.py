#!/usr/bin/env python3
"""Frozen AC fixture through production JSONL, HTTP and management commands.

The 838-token / 2.13-credit oracle includes every custom model automatically.
expected-two-model.json preserves an independently calculated current oracle. No facts, mappings, settings or prices are seeded with SQL.
"""
import hashlib
import argparse
import json
import shutil
import time
import traceback
from pathlib import Path

from workbuddy_import_golden import Golden, MockUpstream
from workbuddy_import_test import Sandbox, scan_canaries

FIXTURES = Path(__file__).resolve().parent / 'fixtures/workbuddy_import'


class CanonicalUpstream(MockUpstream):
    def do_POST(self):
        self.rfile.read(int(self.headers.get('Content-Length', 0)))
        self.reply({'id': 'canonical-response', 'object': 'chat.completion',
            'model': 'golden-model', 'choices': [{'index': 0, 'message': {
                'role': 'assistant', 'content': 'synthetic'}, 'finish_reason': 'stop'}],
            'usage': {'prompt_tokens': 300, 'completion_tokens': 40, 'total_tokens': 340,
                      'prompt_tokens_details': {'cached_tokens': 100}}})


def assert_reports(fixture, expected, at, report):
    for scope, capture, key in [('all', 'all', 'all'), ('builtin', 'all', 'builtin'),
                              ('supplier', 'all', 'supplier'),
                              ('all', 'workbuddy', 'workbuddy_only')]:
        fields = {'from_ms': at - 1, 'to_ms': int(time.time() * 1000) + 1,
                  'timezone': 'UTC', 'include_tests': False,
                  'source_scope': scope, 'capture_method': capture}
        document = fixture.action('stats_dashboard', filter=fields, granularity='auto')
        summary = document['summary']
        oracle = expected[key]
        pairs = [('request_count', 'record_count'), ('input_tokens', 'input_tokens'),
                 ('output_tokens', 'output_tokens'), ('total_tokens', 'total_tokens')]
        actual = {field: summary[field] for field, _ in pairs}
        target = {field: oracle[source] for field, source in pairs}
        report.check('canonical: %s overview matches frozen oracle' % key, actual == target, repr(actual))
        series = document['series']
        report.check('canonical: %s series matches frozen oracle' % key,
                     sum(row['request_count'] for row in series) == oracle['record_count']
                     and sum(row['total_tokens'] or 0 for row in series) == oracle['total_tokens'])
        page = fixture.action('stats_requests', filter=fields, page_size=20)
        report.check('canonical: %s real request page count' % key,
                     len(page['items']) == page['total_rows'] == oracle['record_count'])
        if key == 'workbuddy_only':
            report.check('canonical: mixed quality, precise credits and imported unknown costs',
                (summary['usage_complete_count'], summary['usage_partial_count'], summary['usage_invalid_count'],
                 summary['cache_read_tokens'], summary['workbuddy_credit_nanos'], summary['credit_known_count'],
                 summary['imported_unpriced_count'], summary['success_rate'], summary['cost_known_count'])
                == (5, 1, 1, 60, '2130000000', 3, 7, None, 0))
        if key == 'all':
            costs = [(row['currency'], row['total_cost_nanos']) for row in summary['costs']]
            report.check('canonical: only proxy has CNY0.123; credits remain separate',
                         costs == [('CNY', '123000000')]
                         and summary['workbuddy_credit_nanos'] == '2130000000'
                         and summary['imported_unpriced_count'] == 7)
            report.check('canonical: partial/invalid imports never corrupt proxy pricing or status denominators',
                (summary['usage_complete_count'], summary['usage_partial_count'], summary['usage_invalid_count'],
                 summary['cost_incomplete_request_count'], summary['cost_known_count'],
                 summary['status_known_count'], summary['pending_import_count'],
                 summary['proxy_request_count'], summary['workbuddy_record_count'], summary['cache_read_tokens'])
                == (6, 1, 1, 0, 1, 1, 0, 1, 7, 160))
        report.check('canonical: %s API never discloses fixture canaries' % key,
                     not scan_canaries(fixture.sandbox.config_dir,
                                       [('dashboard', json.dumps(document)), ('page', json.dumps(page))]))


def assert_imported_fields(fixture, report):
    rows = fixture.rows("SELECT request_id,usage_status,input_tokens,output_tokens,total_tokens,"
        "cache_read_tokens,workbuddy_credit_nanos FROM usage_requests WHERE record_origin<>'proxy' "
        "ORDER BY usage_status,input_tokens")
    oracle = [('complete', 0, 0, 0, None, 0), ('complete', 50, 10, 60, 20, 990_000_000),
              ('complete', 80, 8, 88, None, None),
              ('complete', 100, 20, 120, 40, 1_140_000_000),
              ('complete', 200, 30, 230, 0, None),
              ('invalid', None, None, None, None, None),
              ('partial', 7, None, None, None, None)]
    report.check('canonical: every imported SQL field, including explicit zero and invalid nulls',
                 [row[1:] for row in rows] == oracle, repr([row[1:] for row in rows]))
    for index, row in enumerate(rows):
        detail = fixture.action('stats_request_detail', request_id=row[0])
        data = detail['request']
        fields = ('usage_status', 'input_tokens', 'output_tokens', 'total_tokens', 'cache_read_tokens')
        wanted = oracle[index]
        credit = None if wanted[-1] is None else str(wanted[-1])
        report.check('canonical: imported detail %d preserves quality, zero and unknown transport' % index,
            tuple(data[field] for field in fields) == wanted[:-1]
            and data['workbuddy_credit_nanos'] == credit
            and data['outcome'] == 'recorded' and data['stream'] is None
            and data['http_status'] is None and data['duration_ms'] is None
            and data['upstream_protocol'] is None)


def usage_fields(sandbox, exe, report):
    fixture = Golden(sandbox)
    try:
        fixture.start(exe)
        fixture.enable()
        fixture.project.mkdir(parents=True, exist_ok=True)
        source = FIXTURES / 'session-fields.jsonl'
        shutil.copy2(source, fixture.project / source.name)
        fixture.wait_rows('SELECT COUNT(*) FROM usage_requests', [(1,)])
        report.check('fields: real importer preserves cache write, reasoning, derived total and one nanocredit',
            fixture.rows('SELECT input_tokens,output_tokens,total_tokens,cache_read_tokens,cache_write_tokens,'
                'reasoning_tokens,total_tokens_derived,workbuddy_credit_nanos FROM usage_requests')
            == [(100, 20, 120, 40, 10, 2, 1, 1)])
        fixture.at = 1792108800000
        fields = fixture.filter('all', 'workbuddy')
        page = fixture.action('stats_requests', filter=fields, page_size=20)
        row = page['items'][0]
        detail = fixture.action('stats_request_detail', request_id=row['request_id'])
        for name, value in [('list', row), ('detail', detail['request'])]:
            report.check('fields: %s API keeps all normalized usage fields' % name,
                tuple(value[key] for key in ('input_tokens', 'output_tokens', 'total_tokens',
                    'cache_read_tokens', 'cache_write_tokens', 'reasoning_tokens',
                    'total_tokens_derived', 'workbuddy_credit_nanos'))
                == (100, 20, 120, 40, 10, 2, True, '1'))
        document = fixture.action('stats_dashboard', filter=fields, granularity='auto')
        summary = document['summary']
        report.check('fields: real report keeps token subsets and exact nanocredit separate',
            tuple(summary[key] for key in ('request_count', 'input_tokens', 'output_tokens', 'total_tokens',
                'cache_read_tokens', 'cache_write_tokens', 'reasoning_tokens', 'workbuddy_credit_nanos'))
            == (1, 100, 20, 120, 40, 10, 2, '1')
            and summary['success_rate'] is None and summary['costs'] == []
            and summary['imported_unpriced_count'] == 1)
        report.check('fields: source canary never reaches DB, logs or API',
            not scan_canaries(sandbox.config_dir, [('dashboard', json.dumps(document)),
                ('list', json.dumps(page)), ('detail', json.dumps(detail))]))
        report.facts['usage_fields_fixture_sha256'] = hashlib.sha256(source.read_bytes()).hexdigest()
    finally:
        fixture.close()


def run(sandbox, exe, report):
    expected = json.loads((FIXTURES / 'expected-two-model.json').read_text())
    source = FIXTURES / 'session-test.jsonl'
    at = 1792108800000
    fixture = Golden(sandbox, handler_class=CanonicalUpstream)
    try:
        fixture.start(exe)
        fixture.enable()
        fixture.action('stats_price_save', provider_id='golden-provider', upstream_model='golden-model',
            currency='CNY', input_mode='flat', input_rate_nanos='410000000000',
            output_rate_nanos='0', label='synthetic canonical CNY0.123')
        response = fixture.post('/r/golden-route/v1/chat/completions', {
            'model': 'golden-model', 'messages': [{'role': 'user', 'content': 'synthetic'}],
            'stream': False}, 'fixture-local-key', headers={
                'X-Conversation-ID': 'session-test', 'X-Conversation-Message-ID': 'p1'})
        report.check('canonical: actual proxy response has frozen 340 tokens',
                     response['usage']['total_tokens'] == expected['proxy_seed']['total_tokens'])
        fixture.project.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, fixture.project / source.name)
        fixture.wait_rows('SELECT COUNT(*) FROM workbuddy_import_ledger', [(8,)])
        fixture.wait_rows('SELECT COUNT(*),SUM(total_tokens),SUM(workbuddy_credit_nanos) FROM usage_requests',
                          [(8, 838, 2_130_000_000)])
        report.check('canonical: seven automatic imports and one exactly correlated proxy exclusion',
            fixture.rows('SELECT state,COUNT(*) FROM workbuddy_import_ledger GROUP BY state ORDER BY state')
            == [('excluded_proxy', 1), ('imported', 7)])
        assert_imported_fields(fixture, report)
        assert_reports(fixture, expected, at, report)
        report.facts['canonical_fixture'] = {'source_sha256': hashlib.sha256(source.read_bytes()).hexdigest(),
            'oracle_sha256': hashlib.sha256((FIXTURES / 'expected-two-model.json').read_bytes()).hexdigest(),
            'core_binary_sha256': fixture.binary_sha256,
            'scope': 'unmodified frozen source; production HTTP/JSONL/management; SQLite assertions read-only'}
    finally:
        fixture.close()
    extra = Sandbox(prefix='workbuddy-usage-fields-')
    try:
        usage_fields(extra, exe, report)
        extra.check_isolated(report)
    finally:
        extra.close()


def main():
    from workbuddy_import_test import Report, Sandbox, resolve_exe
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', default='target/release/workbuddy-proxy')
    parser.add_argument('--output', default='dist/workbuddy-import-review/canonical-current')
    args = parser.parse_args()
    report = Report('R09-frozen-canonical', args.exe, script=Path(__file__).name)
    sandbox = Sandbox(prefix='workbuddy-canonical-')
    try:
        run(sandbox, resolve_exe(args.exe), report)
        sandbox.check_isolated(report)
    except Exception as failure:
        traceback.print_exc()
        report.check('canonical: production chain completed', False, type(failure).__name__)
    finally:
        sandbox.close()
    report.result = 'FAIL' if report.failed else 'PASS'
    report.write(Path(args.output) / 'report.json')
    return 1 if report.failed else 0


if __name__ == '__main__':
    raise SystemExit(main())

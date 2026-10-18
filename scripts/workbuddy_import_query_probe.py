#!/usr/bin/env python3
"""Measure real report APIs on a saved synthetic production-import snapshot.

This is query-only evidence. It cannot replace a new binary's ingestion,
resource, forwarding or scale/layout trials in the strict matrix.
"""
import argparse
import hashlib
import json
import shutil
import sqlite3
import sys
import traceback
from contextlib import closing
from pathlib import Path

from workbuddy_import_golden import Golden
from workbuddy_import_performance import query_rounds
from workbuddy_import_test import Report, Sandbox, resolve_exe


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', default='target/release/workbuddy-proxy')
    parser.add_argument('--phase', required=True, help='saved synthetic scale phase directory')
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    phase = Path(args.phase)
    original = json.loads((phase / 'report.json').read_text())
    corpus = original['isolation']['corpus']
    if corpus['records'] not in (100_000, 1_000_000):
        raise ValueError('not a supported synthetic scale phase')
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    report = Report('R09-query-only', args.exe, script=Path(__file__).name)
    report.facts.update({'source_phase_report_sha256': hashlib.sha256((phase / 'report.json').read_bytes()).hexdigest(),
        'source_import_binary_sha256': original['isolation']['core_binary_sha256'], 'corpus': corpus,
        'scope': 'saved production-import snapshot; no ingestion/resource/forwarding acceptance'})
    sandbox = Sandbox(prefix='workbuddy-query-')
    fixture = Golden(sandbox)
    try:
        fixture.database.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(phase / 'usage-snapshot.sqlite3', fixture.database)
        with closing(sqlite3.connect(fixture.database)) as connection:
            # A past diagnostic copy may contain this explicitly named test index.
            connection.execute('DROP INDEX IF EXISTS diagnostic_report_cover')
            connection.commit()
        fixture.start(resolve_exe(args.exe))
        report.facts['core_binary_sha256'] = fixture.binary_sha256
        actual = fixture.rows("SELECT COUNT(*),SUM(total_tokens),SUM(workbuddy_credit_nanos) "
                              "FROM usage_requests WHERE upstream_model='scale-base'")
        records = corpus['records']
        report.check('query: retained snapshot has exact independently known corpus totals',
                     actual == [(records, records * 120, records * 1_140_000_000)])
        report.facts['queries'] = query_rounds(fixture, corpus['at_ms'], records, report,
                                              records == 1_000_000)
    except Exception as failure:
        traceback.print_exc()
        report.check('query: real report API phase completed', False, type(failure).__name__)
    finally:
        try:
            fixture.close()
        finally:
            sandbox.close()
    report.result = 'FAIL' if report.failed else 'QUERY_PHASE_PASS_OTHER_GATES_PENDING'
    report.write(output / 'report.json')
    return 1 if report.failed else 0


if __name__ == '__main__':
    sys.exit(main())

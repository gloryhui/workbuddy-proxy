#!/usr/bin/env python3
"""R05 release-chain matrix: 0/7/30/180 days, changes and real retention sweep."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import time

from workbuddy_import_golden import Golden
from workbuddy_import_performance import line
from workbuddy_import_test import Report, Sandbox, resolve_exe


EXPECTED = {0: (4, 0), 7: (1, 3), 30: (2, 2), 180: (3, 1)}


def set_days(fixture, days):
    current = fixture.action('stats_settings_get')
    return fixture.action('stats_settings_update', enabled=True, retention_days=days,
                          expected_revision=current['revision'])


def append(fixture, message, age, at):
    path = fixture.project / 'retention.jsonl'
    with path.open('a') as output:
        output.write(line('retention', message, at - age * 86_400_000,
                          'retention-' + message))
        output.flush()
        os.fsync(output.fileno())


def full(fixture):
    fixture.action('stats_workbuddy_scan', mode='full')
    fixture.until(lambda: fixture.action('stats_workbuddy_status')['scan_state'] == 'completed',
                  'full source reconciliation', timeout=30)


def states(fixture):
    return fixture.rows('SELECT state,COUNT(*) FROM workbuddy_import_ledger GROUP BY state ORDER BY state')


def one_case(exe, days, report, output):
    sandbox = Sandbox(prefix='workbuddy-retention-matrix-')
    fixture = Golden(sandbox)
    try:
        fixture.start(exe)
        current = set_days(fixture, days)
        fixture.action('stats_workbuddy_settings_update', enabled=True,
                       expected_revision=current['revision'])
        fixture.project.mkdir(parents=True)
        at = int(time.time() * 1000)
        for age in (1, 10, 60, 400):
            append(fixture, str(age), age, at)
        fixture.wait_rows('SELECT COUNT(*) FROM workbuddy_import_ledger', [(4,)])
        imported, excluded = EXPECTED[days]
        actual = fixture.rows('SELECT COUNT(*),SUM(total_tokens),SUM(workbuddy_credit_nanos) FROM usage_requests')
        report.check('%d days: exact admitted tokens and credits' % days,
                     actual == [(imported, imported * 120, imported * 1_140_000_000)], repr(actual))
        expected_states = ([('excluded_retention', excluded)] if excluded else []) + [('imported', imported)]
        report.check('%d days: exact durable admission decisions' % days,
                     states(fixture) == expected_states, repr(states(fixture)))
        if days == 0:
            # Exercise the production one-minute housekeeping, not a private
            # test hook or a direct SQL deletion standing in for retention.
            set_days(fixture, 7)
            fixture.until(lambda: states(fixture) == [('deleted_retention', 3), ('imported', 1)],
                          'real retention sweep after 0 -> 7 days', timeout=90)
            report.check('0 -> 7: production sweep tombstones three old identities',
                         fixture.rows('SELECT COUNT(*) FROM usage_requests') == [(1,)], repr(states(fixture)))
            set_days(fixture, 0)
            full(fixture)
            report.check('7 -> 0: full reread does not revive retention tombstones',
                         states(fixture) == [('deleted_retention', 3), ('imported', 1)], repr(states(fixture)))
            append(fixture, 'fresh-400', 400, at)
            fixture.wait_rows('SELECT COUNT(*) FROM usage_requests', [(2,)])
            report.check('7 -> 0: a new 400-day identity uses current unlimited policy',
                         states(fixture) == [('deleted_retention', 3), ('imported', 2)], repr(states(fixture)))
        else:
            set_days(fixture, 0)
            full(fixture)
            report.check('%d -> 0: valid prior exclusions stay settled' % days,
                         states(fixture) == expected_states, repr(states(fixture)))
            append(fixture, 'fresh-400', 400, at)
            fixture.wait_rows('SELECT COUNT(*) FROM usage_requests', [(imported + 1,)])
            report.check('%d -> 0: new old identity is admitted exactly once' % days,
                         fixture.rows("SELECT COUNT(*),SUM(total_tokens) FROM usage_requests "
                                      "WHERE upstream_model='retention-fresh-400'") == [(1, 120)])
        sandbox.check_isolated(report)
    finally:
        fixture.close()
        if (sandbox.root / 'golden-core.log').exists():
            shutil.copy2(sandbox.root / 'golden-core.log', output / ('core-%d.log' % days))
        sandbox.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', required=True)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    exe = resolve_exe(args.exe)
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    report = Report('R05 current retention release matrix', str(exe), Path(__file__).name)
    report.facts.update(core_binary_sha256=hashlib.sha256(exe.read_bytes()).hexdigest(),
                        script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest())
    try:
        for days in (0, 7, 30, 180):
            one_case(exe, days, report, output)
        report.result = 'PASS_RETENTION_RELEASE_MATRIX' if not report.failed else 'FAIL'
    except Exception as failure:
        report.error = type(failure).__name__ + ': ' + str(failure)
        report.check('matrix completed', False, report.error)
        report.result = 'FAIL'
    report.write(output / 'report.json')
    print('%s: %d passed / %d failed' % (report.result, report.passed, report.failed_count))
    return 1 if report.failed else 0


if __name__ == '__main__':
    raise SystemExit(main())

#!/usr/bin/env python3
"""Validate this DESIGN package using disposable SQLite databases only.

This is not the production importer and cannot satisfy implementation acceptance.
Uses only the Python standard library. Never opens a user WorkBuddy/Proxy database.
"""
import argparse
import hashlib
import json
import re
import sqlite3
from pathlib import Path

SPEC = Path(__file__).resolve().parent
ROOT = SPEC.parent.parent
checks = []


def check(name, condition):
    if not condition:
        raise AssertionError(name)
    checks.append(name)


def rejects(conn, sql, values=()):
    conn.execute('SAVEPOINT negative_case')
    try:
        conn.execute(sql, values)
    except sqlite3.IntegrityError:
        result = True
    else:
        result = False
    finally:
        conn.execute('ROLLBACK TO negative_case')
        conn.execute('RELEASE negative_case')
    return result


def baseline_db():
    conn = sqlite3.connect(':memory:')
    for number in range(1, 7):
        conn.executescript((ROOT / f'src/stats/migrations/{number:04}.sql').read_text())
    conn.execute('PRAGMA user_version=6')
    conn.execute("INSERT INTO stats_runs(run_id,started_at_ms) VALUES('run-old',1)")
    conn.execute("""INSERT INTO usage_requests(
        id,request_id,run_id,source,provider_id,route_id,provider_name,route_name,
        requested_model,upstream_model,started_at_ms,ended_at_ms,duration_ms,
        stream,outcome,usage_status,usage_source,input_tokens,output_tokens,total_tokens,
        created_at_ms,updated_at_ms)
        VALUES(42,'old-request','run-old','relay','p','r','Provider','Route',
        'model','model',1,2,1,1,'completed','complete','upstream',100,20,120,1,2)""")
    conn.execute("""INSERT INTO usage_costs(request_id,price_source,currency,status,
        input_cost_nanos,output_cost_nanos,cache_read_cost_nanos,cache_write_cost_nanos,
        total_cost_nanos,calculated_at_ms)
        VALUES('old-request','legacy_unknown','CNY','legacy_unknown',5,7,0,0,12,2)""")
    conn.commit()
    return conn


def migration_checks():
    conn = baseline_db()
    old_cols = [row[1] for row in conn.execute('PRAGMA table_info(usage_requests)')]
    old = conn.execute('SELECT ' + ','.join(old_cols) + ' FROM usage_requests').fetchall()
    costs = conn.execute('SELECT * FROM usage_costs').fetchall()
    sql = (SPEC / 'schema-v7.sql').read_text()
    conn.execute('PRAGMA foreign_keys=OFF')
    conn.executescript('BEGIN IMMEDIATE;\n' + sql + '\nPRAGMA user_version=7; COMMIT;')
    conn.execute('PRAGMA foreign_keys=ON')
    check('schema v7', conn.execute('PRAGMA user_version').fetchone()[0] == 7)
    check('all legacy request fields unchanged',
          conn.execute('SELECT ' + ','.join(old_cols) + ' FROM usage_requests').fetchall() == old)
    check('all legacy costs unchanged', conn.execute('SELECT * FROM usage_costs').fetchall() == costs)
    check('foreign keys preserved', not conn.execute('PRAGMA foreign_key_check').fetchall())
    check('database integrity', conn.execute('PRAGMA integrity_check').fetchone()[0] == 'ok')
    check('legacy origin defaults proxy', conn.execute('SELECT record_origin FROM usage_requests').fetchone()[0] == 'proxy')
    check('import disabled by default', conn.execute('SELECT workbuddy_import_enabled FROM stats_settings').fetchone()[0] == 0)
    check('reject incomplete proxy row', rejects(conn, "UPDATE usage_requests SET stream=NULL WHERE id=42"))
    check('reject fake proxy recorded status', rejects(conn, "UPDATE usage_requests SET outcome='recorded' WHERE id=42"))
    key = 'a' * 64
    conn.execute("""INSERT INTO usage_requests(
      request_id,run_id,source,provider_id,route_id,provider_name,route_name,
      requested_model,upstream_model,started_at_ms,ended_at_ms,stream,upstream_protocol,
      outcome,usage_status,usage_source,input_tokens,output_tokens,total_tokens,
      created_at_ms,updated_at_ms,record_origin,time_basis,workbuddy_credit_nanos)
      VALUES(?,NULL,'workbuddy_import','wb:builtin','wb:route:1','WorkBuddy','hy3',
      'hy3','hy3',10,10,NULL,NULL,'recorded','complete','workbuddy_jsonl',100,20,120,
      11,11,'workbuddy_builtin','message_timestamp',1140000000)""", (key,))
    check('import complete fact accepted', conn.execute('SELECT counted_total_tokens FROM stats_request_facts WHERE request_id=?', (key,)).fetchone()[0] == 120)
    check('import fake HTTP forbidden', rejects(conn, 'UPDATE usage_requests SET http_status=200 WHERE request_id=?', (key,)))
    check('import fake latency forbidden', rejects(conn, 'UPDATE usage_requests SET duration_ms=0 WHERE request_id=?', (key,)))
    check('import current-price costs forbidden', rejects(conn, """INSERT INTO usage_costs(request_id,price_source,currency,status,
        input_cost_nanos,output_cost_nanos,cache_read_cost_nanos,cache_write_cost_nanos,total_cost_nanos,calculated_at_ms)
        VALUES(?,'legacy_unknown','CNY','legacy_unknown',0,0,0,0,0,12)""", (key,)))
    ns = 'b' * 48
    conn.execute("""INSERT INTO workbuddy_import_ledger(
        entry_key,namespace_id,session_hash,message_hash,event_at_ms,normalized_hash,state,
        request_id,mapping_revision,created_at_ms,updated_at_ms)
        VALUES(?,?,?,?,10,?,'imported',?,1,11,11)""", (key, ns, 'c'*64, 'd'*64, 'e'*64, key))
    check('deletion requires tombstone', rejects(conn, 'DELETE FROM usage_requests WHERE request_id=?', (key,)))
    conn.execute("UPDATE workbuddy_import_ledger SET state='deleted_manual',request_id=NULL WHERE entry_key=?", (key,))
    conn.execute('DELETE FROM usage_requests WHERE request_id=?', (key,))
    check('tombstone survives fact deletion', conn.execute('SELECT state FROM workbuddy_import_ledger WHERE entry_key=?', (key,)).fetchone()[0] == 'deleted_manual')
    mapping_sql = """INSERT INTO workbuddy_model_history(
        mapping_id,namespace_id,requested_model,classification,valid_from_ms,valid_to_ms,
        evidence,state,created_at_ms,updated_at_ms) VALUES(?,?,'custom-local:test','proxy',?,?,'manual','verified',1,1)"""
    conn.execute(mapping_sql, ('m1', ns, 100, 200))
    check('overlap rejected', rejects(conn, mapping_sql, ('m2', ns, 150, 250)))
    conn.execute(mapping_sql, ('m3', ns, 200, 300))
    check('adjacent mapping allowed', conn.execute('SELECT count(*) FROM workbuddy_model_history').fetchone()[0] == 2)
    check('updated mapping overlap rejected', rejects(conn, "UPDATE workbuddy_model_history SET valid_from_ms=199 WHERE mapping_id='m3'"))
    check('final foreign keys intact', not conn.execute('PRAGMA foreign_key_check').fetchall())
    conn.close()

    conn = baseline_db()
    conn.execute('PRAGMA foreign_keys=OFF')
    try:
        conn.executescript('BEGIN;\n' + sql + '\nTHIS IS AN INJECTED SQL FAILURE; COMMIT;')
    except sqlite3.DatabaseError:
        conn.rollback()
    else:
        raise AssertionError('failure injection did not fail')
    check('failed migration rolls back schema', conn.execute('PRAGMA user_version').fetchone()[0] == 6)
    check('failed migration preserves legacy row', conn.execute('SELECT request_id FROM usage_requests').fetchone()[0] == 'old-request')
    check('failed migration removes draft tables', not conn.execute("SELECT name FROM sqlite_master WHERE name='workbuddy_import_ledger'").fetchall())
    conn.close()


def document_checks(check_baseline):
    tasks = json.loads((SPEC / 'tasks.json').read_text())['tasks']
    expected_ids = [f'S{s:02}.{i:02}' for s in range(1, 11) for i in range(1, 9)]
    check('exactly 80 ordered subtasks', [t['id'] for t in tasks] == expected_ids)
    check('sequential dependencies', [t['depends_on'] for t in tasks] == [None] + expected_ids[:-1])
    check('80 unique acceptance ids', len({t['acceptance_id'] for t in tasks}) == 80)
    tasks_md = (SPEC / 'TASKS.md').read_text()
    acceptance = (SPEC / 'ACCEPTANCE.md').read_text()
    progress = (SPEC / 'PROGRESS.md').read_text()
    for t in tasks:
        check(t['id'] + ' task/AC/progress coverage',
              f"### {t['id']} " in tasks_md and
              f"| {t['acceptance_id']} | {t['id']} |" in acceptance and
              f"| {t['id']} " in progress and bool(t['allowed_paths']))
    baseline = json.loads((SPEC / 'baseline.json').read_text())
    for rel, digest in baseline['files'].items():
        if check_baseline or rel.startswith('src/stats/migrations/'):
            check('baseline hash ' + rel, hashlib.sha256((ROOT / rel).read_bytes()).hexdigest() == digest)
    for md in SPEC.rglob('*.md'):
        for target in re.findall(r'\]\(([^)]+)\)', md.read_text()):
            if not target.startswith(('https:', 'http:', '#')):
                check('local document link ' + md.name + ' → ' + target,
                      (md.parent / target.split('#')[0]).exists())
    ui = (SPEC / 'UI-SPEC.md').read_text()
    for text in ('全部来源', '内置模型', '供应商模型', '1012', '560', '默认仍为**当天**'):
        check('UI contract ' + text, text in ui)
    import xml.etree.ElementTree as ET
    svg = ET.parse(SPEC / 'UI-WIREFRAME.svg')
    check('three distinct SVG glyphs', {'all', 'builtin', 'supplier'} <= {e.get('id') for e in svg.iter()})


def fixture_checks():
    rows = [json.loads(line) for line in (SPEC / 'fixtures/session-test.jsonl').read_text().splitlines()]
    expected = json.loads((SPEC / 'fixtures/expected.json').read_text())
    counts = {}
    for row in rows:
        key = row['providerData']['messageId']
        counts[key] = counts.get(key, 0) + 1
    check('fixture duplicate rows', sum(n-1 for n in counts.values()) == expected['duplicate_rows'])
    check('fixture all distinct input identifiers covered', set(counts) == set(expected['imported_entry_ids'] + expected['excluded_proxy_ids'] + expected['pending_ids'] + expected['ignored_ids']))
    check('manually specified complete token arithmetic', 120 + 60 + 230 + 0 == expected['workbuddy_only']['total_tokens'])
    check('manual all-source sum', expected['all']['total_tokens'] == 410 + 340 == 180 + 570)
    check('manual all-source record count', expected['all']['record_count'] == 6 + 1 == 5 + 2)
    check('manual credit exact nanos', expected['workbuddy_only']['credit_nanos'] == str(1140000000 + 990000000))
    check('tail fixture intentionally incomplete', not (SPEC / 'fixtures/tail-prefix.txt').read_bytes().endswith(b'\n'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', action='store_true', help='also verify all inspected production files remain at the design baseline')
    parser.add_argument('--report', type=Path, help='write DESIGN-only validation evidence')
    args = parser.parse_args()
    document_checks(args.baseline)
    migration_checks()
    fixture_checks()
    report = {'status': 'PASS', 'scope': 'design_only_not_production_implementation',
              'check_count': len(checks), 'checks': checks,
              'not_run': ['production Rust importer', 'production migrations', 'GPUI UI acceptance',
                          'performance benchmark', 'Windows/macOS installation tests']}
    if args.report:
        args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps({'status': report['status'], 'scope': report['scope'], 'checks': len(checks)}, ensure_ascii=False))


if __name__ == '__main__':
    main()

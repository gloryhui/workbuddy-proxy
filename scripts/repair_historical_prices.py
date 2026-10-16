#!/usr/bin/env python3
"""One-time, backed-up repair of historical default-zero price snapshots.

Rehearses migration, catalogue matching and settlement in an isolated profile
using the real Rust core. --apply writes only the reviewed default-zero rows;
request facts and already priced history are never rewritten. Current catalogue
prices are estimates applied at repair time, not recovered historical tariffs.
"""
import argparse
from collections import Counter
from datetime import datetime
import json
import os
from pathlib import Path
import shutil
import sqlite3
import tempfile
import time
from urllib.parse import quote
import statistics_test as core


def readonly(path):
    connection = sqlite3.connect('file:' + quote(str(path)) + '?mode=ro', uri=True)
    connection.row_factory = sqlite3.Row
    return connection


def rows(connection, table, ids):
    placeholders = ','.join('?' for _ in ids)
    return {r['request_id']: dict(r) for r in connection.execute(
        f'SELECT * FROM {table} WHERE request_id IN ({placeholders})', ids)}


def private_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')
    path.chmod(0o600)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', type=Path, default=Path.home() / 'Library/Application Support/WorkBuddyProxy')
    parser.add_argument('--exe', default='target/debug/workbuddy-proxy')
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    profile = args.profile.resolve()
    db_path = profile / 'usage.sqlite3'
    with readonly(db_path) as source:
        ids = [r[0] for r in source.execute(
            "SELECT c.request_id FROM usage_costs c JOIN usage_requests r USING(request_id) "
            "WHERE c.price_source='default_zero' AND c.status='default_zero' "
            "AND r.outcome<>'in_flight' AND r.ended_at_ms IS NOT NULL ORDER BY r.id")]
    if not ids:
        print('No historical default-zero rows require repair.')
        return
    if len(ids) > 1000:
        raise RuntimeError('Repair is bounded to 1000 rows per run; narrow the scope first.')

    stamp = datetime.now().strftime('%Y%m%d-%H%M%S-%f')
    audit = profile / 'history-repairs' / stamp
    audit.mkdir(parents=True, mode=0o700)
    audit.parent.chmod(0o700)
    backup = audit / 'before.sqlite3'
    with readonly(db_path) as source, sqlite3.connect(backup) as destination:
        source.backup(destination)
    backup.chmod(0o600)
    with readonly(backup) as source:
        before_requests = rows(source, 'usage_requests', ids)
        before_costs = rows(source, 'usage_costs', ids)

    config = json.loads((profile / 'config.json').read_text())
    repair_time = int(time.time() * 1000)
    process = log = None
    with tempfile.TemporaryDirectory(prefix='workbuddy-history-rehearsal-') as folder:
        directory = Path(folder)
        port = core.free_port()
        isolated = {'local_key': core.LOCAL_KEY, 'language': 'en', 'port': port,
                    'providers': config['providers'], 'installed': []}
        for provider in isolated['providers']:
            provider['api_key'] = ''
            for model in provider.get('models', []):
                model['capabilities'] = {}
        private_json(directory / 'config.json', isolated)
        rehearsal_db = directory / 'usage.sqlite3'
        shutil.copy2(backup, rehearsal_db)
        try:
            process, log, _ = core.start_core(args.exe, directory, port)

            def catalogue_ready():
                page = core.stats(directory, port, 'stats_prices_list', limit=100)
                return page if page.get('catalog_sync', {}).get('last_success_day') else None

            page = core.wait_for(catalogue_ready, 'Catalogue did not initialize', timeout=60)
            prices = {(p['provider_id'], p['upstream_model']): p for p in page['items']}
            core.stop_core(process, port, directory, log)
            process = log = None
            selected = {rid: prices[(request['provider_id'], request['upstream_model'])]
                        for rid, request in before_requests.items()
                        if (request['provider_id'], request['upstream_model']) in prices}
            if not selected:
                raise RuntimeError('No historical model IDs matched catalogue prices; nothing changed.')

            # Reset only the clone's costs. Rust startup recovery performs the
            # production settlement, including NULL usage/cache split semantics.
            with sqlite3.connect(rehearsal_db) as clone:
                for rid, price in selected.items():
                    clone.execute(
                        "UPDATE usage_costs SET source_price_id=?,source_price_revision=?,price_source='manual',"
                        "price_snapshot_at_ms=?,currency=?,input_mode=?,input_rate_nanos=?,output_rate_nanos=?,"
                        "cache_read_rate_nanos=?,cache_write_rate_nanos=?,status='pending',input_cost_nanos=NULL,"
                        "output_cost_nanos=NULL,cache_read_cost_nanos=NULL,cache_write_cost_nanos=NULL,"
                        "total_cost_nanos=NULL,pricing_version=2,calculated_at_ms=? WHERE request_id=?",
                        (price['price_id'], price['revision'], repair_time, price['currency'], price['input_mode'],
                         price['input_rate_nanos'], price['output_rate_nanos'], price['cache_read_rate_nanos'],
                         price['cache_write_rate_nanos'], repair_time, rid))
            process, log, _ = core.start_core(args.exe, directory, port)

            def settled():
                with readonly(rehearsal_db) as clone:
                    costs = rows(clone, 'usage_costs', list(selected))
                return costs if all(c['status'] != 'pending' for c in costs.values()) else None

            after_costs = core.wait_for(settled, 'Rust did not settle the cloned historical costs')
            core.stop_core(process, port, directory, log)
            process = log = None
            with readonly(rehearsal_db) as clone:
                if rows(clone, 'usage_requests', ids) != before_requests:
                    raise RuntimeError('Rehearsal altered request facts; refusing to apply.')
                assert clone.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
                assert not clone.execute('PRAGMA foreign_key_check').fetchall()
        finally:
            if process is not None:
                core.stop_core(process, port, directory, log)

    summary = {'mode': 'apply' if args.apply else 'preview', 'backup': str(backup),
               'selected': len(selected), 'unmatched': len(ids) - len(selected),
               'statuses': dict(Counter(c['status'] for c in after_costs.values())),
               'total_nanos_by_currency': {}, 'pricing_basis': 'Current models.dev prices at repair time',
               'catalog_sync': page['catalog_sync']}
    for cost in after_costs.values():
        if cost['total_cost_nanos'] is not None:
            currency = cost['currency']
            summary['total_nanos_by_currency'][currency] = (
                summary['total_nanos_by_currency'].get(currency, 0) + cost['total_cost_nanos'])
    private_json(audit / 'preview.json', {'summary': summary, 'before': before_costs,
                                       'after': after_costs, 'prices': list(prices.values())})
    if not args.apply:
        print(json.dumps(summary, ensure_ascii=False))
        return

    # API writes publish the running core's current-price cache. Never mutate
    # that table from a second SQLite writer behind the core's back.
    with readonly(db_path) as live:
        if rows(live, 'usage_requests', ids) != before_requests or rows(live, 'usage_costs', ids) != before_costs:
            raise RuntimeError('Live historical rows changed during rehearsal; nothing applied.')
    instance = json.loads((profile / 'instance.json').read_text())
    port = instance['port']
    current_page = core.stats(profile, port, 'stats_prices_list', limit=100)
    if current_page.get('next_rule_id'):
        raise RuntimeError('Price listing exceeds one page; refusing an incomplete lookup.')
    current_prices = {(p['provider_id'], p['upstream_model']): p for p in current_page['items']}
    saved = {}
    for key in {(r['provider_id'], r['upstream_model']) for rid, r in before_requests.items() if rid in selected}:
        price, existing = prices[key], current_prices.get(key)
        payload = {field: price[field] for field in (
            'provider_id', 'upstream_model', 'currency', 'input_mode', 'input_rate_nanos',
            'output_rate_nanos', 'cache_read_rate_nanos', 'cache_write_rate_nanos', 'label')}
        payload.update(price_id=existing['price_id'] if existing else None,
                       expected_revision=existing['revision'] if existing else 0)
        saved[key] = core.stats(profile, port, 'stats_price_save', **payload)
    with sqlite3.connect(db_path, timeout=10) as live:
        live.row_factory = sqlite3.Row
        live.execute('PRAGMA foreign_keys=ON')
        live.execute('BEGIN IMMEDIATE')
        if rows(live, 'usage_requests', ids) != before_requests or rows(live, 'usage_costs', ids) != before_costs:
            raise RuntimeError('Historical rows changed before the repair transaction; rolled back.')
        for rid, cost in after_costs.items():
            request = before_requests[rid]
            price = saved[(request['provider_id'], request['upstream_model'])]
            cost['source_price_id'], cost['source_price_revision'] = price['price_id'], price['revision']
            fields = [name for name in cost if name != 'request_id']
            changed = live.execute('UPDATE usage_costs SET ' + ','.join(name + '=?' for name in fields)
                                   + " WHERE request_id=? AND price_source='default_zero' AND status='default_zero'",
                                   [cost[name] for name in fields] + [rid]).rowcount
            assert changed == 1, (rid, changed)
        assert rows(live, 'usage_requests', ids) == before_requests
        assert not live.execute('PRAGMA foreign_key_check').fetchall()
    with readonly(db_path) as live:
        assert rows(live, 'usage_costs', list(selected)) == after_costs
        assert live.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
    private_json(audit / 'applied.json', {'summary': summary, 'before': before_costs,
                                       'after': after_costs, 'current_prices_before': current_page['items'],
                                       'current_prices_after': list(saved.values())})
    # Confirm the active application's management API observes the repair.
    sample = next(rid for rid, cost in after_costs.items() if cost['status'] == 'priced')
    detail = core.stats(profile, port, 'stats_request_detail', request_id=sample)
    private_json(audit / 'verified-detail.json', detail)
    print(json.dumps(summary, ensure_ascii=False))
    print('Applied and verified through the running core; audit:', audit)


if __name__ == '__main__':
    main()

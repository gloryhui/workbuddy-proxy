#!/usr/bin/env python3
"""Verify real released Core data against a candidate, using an empty fixture.

Both executables must come from independently verified packages. This exercises
the real old schema/writer and candidate migration, not a fabricated legacy DB.
It does not install an OS package, send GUI input, or prove a live app upgrade.
The caller owns --directory and retains/cleans it after all children exit.
"""
import argparse
from contextlib import contextmanager
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import platform
import signal
import socket
import sqlite3
import subprocess
import sys
import threading
import time
import urllib.request


BUSINESS_TABLES = (
    'usage_requests', 'usage_costs', 'stats_model_prices',
    'stats_settings', 'stats_provider_settings',
)
OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def digest(blob):
    return hashlib.sha256(blob).hexdigest()


def wait_for(predicate, label, seconds=20):
    deadline = time.monotonic() + seconds
    while True:
        value = predicate()
        if value:
            return value
        if time.monotonic() >= deadline:
            raise AssertionError(label)
        time.sleep(.025)


def snapshot(directory):
    with sqlite3.connect(f'file:{directory / "usage.sqlite3"}?mode=ro', uri=True) as db:
        db.row_factory = sqlite3.Row
        tables = {}
        for table in BUSINESS_TABLES:
            columns = [row['name'] for row in db.execute(f'PRAGMA table_info({table})')]
            tables[table] = {'columns': columns,
                             'rows': [dict(row) for row in db.execute(f'SELECT * FROM {table}')]}
        return {'schema': db.execute('PRAGMA user_version').fetchone()[0], 'tables': tables}


def assert_preserved(before, after):
    """All old business fields survive; new schema fields are reported separately."""
    additions = {}
    for table, old in before['tables'].items():
        new = after['tables'][table]
        assert set(old['columns']) <= set(new['columns']), (table, 'old column removed')
        project = lambda rows: sorted(json.dumps({k: row[k] for k in old['columns']},
                                                 sort_keys=True) for row in rows)
        assert project(old['rows']) == project(new['rows']), (table, 'old data changed')
        additions[table] = sorted(set(new['columns']) - set(old['columns']))
    return additions


class Core:
    def __init__(self, exe, directory, environment, policy, label):
        self.directory, self.label = directory, label
        self.instance = None
        self.log = (directory / (label + '.stdout.log')).open('wb')
        command = [str(exe), '--headless', '--config-dir', str(directory)]
        if policy:
            command = ['/usr/bin/sandbox-exec', '-f', str(policy), *command]
        self.process = subprocess.Popen(command, env=environment, stdout=self.log,
                                        stderr=subprocess.STDOUT)
        self.terminal = None

    def ready(self):
        assert self.process.poll() is None, (self.label, 'Core exited before readiness')
        try:
            instance = json.loads((self.directory / 'instance.json').read_text())
            assert instance['pid'] == self.process.pid, 'Unexpected preexisting Core'
            self.instance = instance
            with OPENER.open(f'http://127.0.0.1:{instance["port"]}/health', timeout=.5) as response:
                health = json.load(response)
            if health.get('ready') and self.action('stats_health')['state'] == 'ready':
                return health
        except (OSError, ValueError):
            pass
        return None

    def action(self, command, **arguments):
        request = urllib.request.Request(
            f'http://127.0.0.1:{self.instance["port"]}/_control/action',
            data=json.dumps({'command': command, **arguments}).encode(),
            headers={'Authorization': 'Bearer ' + self.instance['management_key'],
                     'Content-Type': 'application/json'})
        with OPENER.open(request, timeout=5) as response:
            message = json.load(response)['message']
        try:
            return json.loads(message)
        except (ValueError, TypeError):
            return message

    def close(self):
        failure = None
        try:
            if self.process.poll() is None:
                if self.instance is None:
                    failure = 'Core never published its own instance'
                else:
                    request = urllib.request.Request(
                        f'http://127.0.0.1:{self.instance["port"]}/_control/stop', data=b'',
                        headers={'Authorization': 'Bearer ' + self.instance['management_key']})
                    with OPENER.open(request, timeout=3) as response:
                        response.read()
                    self.process.wait(timeout=8)
        except Exception as error:
            failure = type(error).__name__ + ': ' + str(error)
        finally:
            if self.process.poll() is None:
                self.process.terminate()
                try:
                    self.process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    self.process.wait(timeout=5)
            self.log.close()
            self.terminal = {'label': self.label, 'pid': self.process.pid,
                             'exit_code': self.process.returncode, 'waited': True,
                             'shutdown_error': failure}
        assert failure is None and self.process.returncode == 0, self.terminal
        assert not (self.directory / 'instance.json').exists(), 'Instance record remains'


class Upstream(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        payload = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        assert payload['model'] == 'release-upgrade-model'
        assert self.headers['Authorization'] == 'Bearer synthetic-upgrade-upstream-key'
        body = json.dumps({'id': 'synthetic-upgrade', 'model': payload['model'],
                           'choices': [{'message': {'role': 'assistant', 'content': 'fixture'},
                                        'finish_reason': 'stop'}],
                           'usage': {'prompt_tokens': 1000, 'completion_tokens': 100,
                                     'total_tokens': 1100}}).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def relay(core):
    request = urllib.request.Request(
        f'http://127.0.0.1:{core.instance["port"]}/v1/chat/completions',
        data=json.dumps({'model': 'wbp-release-upgrade',
                         'messages': [{'role': 'user', 'content': 'Synthetic upgrade receipt'}]}).encode(),
        headers={'Authorization': 'Bearer synthetic-upgrade-local-key',
                 'Content-Type': 'application/json'})
    with OPENER.open(request, timeout=5) as response:
        assert response.status == 200
        assert json.load(response)['usage']['total_tokens'] == 1100


@contextmanager
def lifecycle(exe, directory, environment, policy, label, report):
    core = Core(exe, directory, environment, policy, label)
    try:
        health = wait_for(core.ready, label + ' statistics not ready')
        report.setdefault('health', {})[label] = health
        if policy:
            from workbuddy_import_test import real_user_home
            from workbuddy_import_ui_sandbox import kernel_protection
            proof = kernel_protection(real_user_home(), directory, core.process.pid)
            assert proof['enforced'], proof
            report.setdefault('kernel_protection', {})[label] = proof
        yield core
    finally:
        try:
            core.close()
        finally:
            report.setdefault('children', []).append(core.terminal)


def run(args, report, replace_installation=None, inspect_installed=None):
    directory = args.directory.resolve()
    assert directory.is_dir() and not any(directory.iterdir()), 'Use an existing empty owned fixture'
    paths = {name: directory / name.lower() for name in (
        'HOME', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA', 'XDG_CONFIG_HOME',
        'XDG_DATA_HOME', 'XDG_CACHE_HOME', 'XDG_STATE_HOME', 'TMPDIR', 'TMP', 'TEMP')}
    for path in paths.values():
        path.mkdir()
    environment = dict(os.environ, **{name: str(path) for name, path in paths.items()},
                       WORKBUDDY_PROXY_CONFIG_DIR=str(directory),
                       WORKBUDDY_PROXY_CATALOG_TEST_URL='http://127.0.0.1:1/catalog',
                       NO_PROXY='127.0.0.1,localhost', no_proxy='127.0.0.1,localhost')
    for name in ('GH_TOKEN', 'GITHUB_TOKEN', 'ACTIONS_RUNTIME_TOKEN',
                 'ACTIONS_ID_TOKEN_REQUEST_TOKEN'):
        environment.pop(name, None)
    policy = None
    if sys.platform == 'darwin':
        from workbuddy_import_test import real_user_home
        from workbuddy_import_ui_sandbox import profile_text, protected_locations
        real_home = real_user_home()
        policy = directory / 'protection.sb'
        policy.write_text(profile_text([*protected_locations(real_home).values(),
                                        Path(real_home) / 'Library/LaunchAgents']))
    upstream = ThreadingHTTPServer(('127.0.0.1', 0), Upstream)
    thread = threading.Thread(target=upstream.serve_forever)
    thread.start()
    try:
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
        config = {'local_key': 'synthetic-upgrade-local-key', 'language': 'zh',
                  'port': port, 'autostart': False, 'installed': [],
                  'providers': [{'id': 'upgrade-provider', 'name': '旧版供应商🙂 e\u0301',
                                 'upstream': f'http://127.0.0.1:{upstream.server_port}/v1',
                                 'api_key': 'synthetic-upgrade-upstream-key',
                                 'models': [{'id': 'upgrade-model', 'alias': 'wbp-release-upgrade',
                                             'name': '旧版模型🙂 e\u0301',
                                             'model': 'release-upgrade-model',
                                             'reasoning_effort': 'default'}]}]}
        (directory / 'config.json').write_text(json.dumps(config))
        with lifecycle(args.old_exe, directory, environment, policy, 'released', report) as core:
            assert report['health']['released']['version'] == args.old_version
            # Actual released code performs the config serialization and DB writes.
            core.action('language', language='en')
            core.action('language', language='zh')
            settings = core.action('stats_settings_get')
            core.action('stats_settings_update', enabled=True, retention_days=731,
                        expected_revision=settings['revision'])
            core.action('stats_provider_settings_update', provider_id='upgrade-provider',
                        usage_mode='observe', expected_revision=0)
            rule = core.action('stats_price_save', provider_id='upgrade-provider',
                               upstream_model='release-upgrade-model', currency='USD',
                               input_mode='flat', input_rate_nanos='2000000000',
                               output_rate_nanos='8000000000', label='旧版价格🙂 e\u0301')
            assert rule['revision'] == 1
            for _ in range(12):
                relay(core)
            wait_for(lambda: len(snapshot(directory)['tables']['usage_costs']['rows']) == 12,
                     'Released requests/costs did not persist')
        before = snapshot(directory)
        assert before['schema'] == args.old_schema, before['schema']
        assert len(before['tables']['usage_requests']['rows']) == 12
        assert before['tables']['stats_settings']['rows'][0]['retention_days'] == 731
        assert len(before['tables']['stats_provider_settings']['rows']) == 1
        assert all(row['total_cost_nanos'] == 2800000 and row['status'] == 'priced'
                   for row in before['tables']['usage_costs']['rows'])
        original_config = (directory / 'config.json').read_bytes()
        before_blob = json.dumps(before, sort_keys=True).encode()
        # SQLite backup is produced from the real released DB, not a hand-built schema.
        with sqlite3.connect(directory / 'usage.sqlite3') as source:
            with sqlite3.connect(directory / 'released-backup.sqlite3') as backup:
                source.backup(backup)
        if replace_installation is not None:
            # The real old writer is stopped and its data already exists before
            # the caller runs an installer or replaces the installed app bundle.
            replace_installation(directory, report)
            unchanged = snapshot(directory)
            assert unchanged['schema'] == before['schema'], 'Installer ran a writer unexpectedly'
            assert_preserved(before, unchanged)
            assert (directory / 'config.json').read_bytes() == original_config
            report['installation_preserved_legacy_data_before_candidate_start'] = True
        report['candidate_exe_sha256'] = digest(args.candidate_exe.read_bytes())
        with lifecycle(args.candidate_exe, directory, environment, policy, 'candidate', report) as core:
            assert report['health']['candidate']['version'] == args.candidate_version
            after = snapshot(directory)
            assert after['schema'] == 11, after['schema']
            additions = assert_preserved(before, after)
            assert (directory / 'config.json').read_bytes() == original_config
            report.update(old_schema=before['schema'], candidate_schema=after['schema'],
                          preserved_tables={name: len(value['rows'])
                                            for name, value in before['tables'].items()},
                          candidate_new_columns=additions,
                          all_legacy_business_fields_equal=True,
                          config_byte_equal=True, config_sha256=digest(original_config),
                          legacy_snapshot_sha256=digest(before_blob),
                          legacy_cost_per_request_nanos=2800000)
            relay(core)
            wait_for(lambda: len(snapshot(directory)['tables']['usage_costs']['rows']) == 13,
                     'Candidate could not continue recording')
        final = snapshot(directory)
        assert len(final['tables']['usage_requests']['rows']) == 13
        assert len(final['tables']['usage_costs']['rows']) == 13
        assert all(row['total_cost_nanos'] == 2800000 and row['status'] == 'priced'
                   for row in final['tables']['usage_costs']['rows'])
        report['candidate_new_request_priced'] = True
        if inspect_installed is not None:
            inspect_installed(args.candidate_exe, directory, environment, policy, report)
            assert_preserved(final, snapshot(directory))
            assert (directory / 'config.json').read_bytes() == original_config
            report['all_13_rows_equal_after_installed_lifecycle'] = True
        for name in ('usage_requests', 'usage_costs'):
            keep = {row['request_id'] for row in before['tables'][name]['rows']}
            final['tables'][name]['rows'] = [row for row in final['tables'][name]['rows']
                                            if row['request_id'] in keep]
        assert_preserved(before, final)
        assert (directory / 'config.json').read_bytes() == original_config
        report.update(candidate_recorded_new_request=True,
                      legacy_rows_equal_after_candidate_shutdown=True,
                      scope='Real released Core schema/data → candidate Core, cold handover in same '
                            'isolated profile. No OS installer replacement, GUI, live-upgrade, '
                            'downgrade/rollback, or universal data compatibility claim.')
    finally:
        upstream.shutdown()
        upstream.server_close()
        thread.join(timeout=5)
        assert not thread.is_alive()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--old-exe', type=Path, required=True)
    parser.add_argument('--old-version', required=True)
    parser.add_argument('--old-schema', type=int, required=True)
    parser.add_argument('--candidate-exe', type=Path, required=True)
    parser.add_argument('--candidate-version', required=True)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    def interrupted(signum, frame):
        raise KeyboardInterrupt(signum)
    previous_term_handler = signal.signal(signal.SIGTERM, interrupted)
    report = {'result': 'FAIL', 'platform': platform.platform(), 'architecture': platform.machine(),
              'script_sha256': digest(Path(__file__).read_bytes()),
              'old_version': args.old_version, 'candidate_version': args.candidate_version,
              'old_exe_sha256': digest(args.old_exe.read_bytes()),
              'fixture': str(args.directory.resolve())}
    try:
        run(args, report)
        report['result'] = 'PASS'
    except BaseException as error:
        report['error'] = type(error).__name__ + ': ' + str(error)
        raise
    finally:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + '\n')
        signal.signal(signal.SIGTERM, previous_term_handler)
    print('PASS: real released Core data/config preserved and candidate continued recording')


if __name__ == '__main__':
    main()

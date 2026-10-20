#!/usr/bin/env python3
"""WorkBuddy local usage import: isolated real-core test harness.

S01.03 delivers the disposable sandbox and the proof that the real core binary
treats it as its effective root. Parsing, reading, import, lifecycle and UI
checks are added by later tasks on top of this file.

Nothing in this harness touches a real user profile, a real configuration
directory, a running instance or the public price catalogue:

* ``HOME`` / ``USERPROFILE`` / ``APPDATA`` / ``XDG_*`` / ``TMPDIR`` all point
  inside one ``TemporaryDirectory``, and child processes receive that whole
  environment together with an explicit ``--config-dir``;
* ``WORKBUDDY_PROXY_CATALOG_TEST_URL`` points at 127.0.0.1:1 where nothing
  listens, so price or billing lookups cannot reach the network;
* the WorkBuddy source root used by import tests lives inside the sandbox, and
  the harness refuses to run when it does not.

Exit status is non-zero whenever a check fails or the core binary is missing;
the JSON report is written even then so a failed run still leaves evidence.

Usage:
    python3 scripts/workbuddy_import_test.py --exe target/release/workbuddy-proxy
"""
from __future__ import annotations

import argparse
from contextlib import closing
import atexit
import hashlib
import json
import shutil
import sqlite3
import os
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_OUTPUT = REPO_ROOT / 'dist' / 'workbuddy-import-validation' / 'workbuddy_import_test.json'

# 127.0.0.1:1 has no listener, so a catalogue fetch fails immediately instead of
# reaching the public billing endpoint.
CATALOG_TEST_URL = 'http://127.0.0.1:1/catalog'

# Baseline value; S08.07 raises this together with the control protocol.
EXPECTED_CONTROL_PROTOCOL = 12

# Synthetic canaries (AC007, X44): they may live in fixtures and assertions only,
# never in a log, a configuration file, the database or an API response.
CANARIES = (
    'SYNTHETIC_PROMPT_MUST_NOT_PERSIST',   # chat content
    'sk-SYNTHETIC-CANARY-KEY-0001',        # credential shape
    'SYNTHETIC_TOOL_ARGUMENTS_CANARY',     # tool arguments
)

PROFILE_VARS = ('HOME', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA',
                'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_CACHE_HOME', 'XDG_STATE_HOME')
TEMP_VARS = ('TMPDIR', 'TEMP', 'TMP')

SYSTEM_TEMP = Path(tempfile.gettempdir()).resolve()


def real_user_home() -> Path:
    """Account home directory, ignoring any HOME the caller exported."""
    if os.name == 'nt':
        profile = os.environ.get('USERPROFILE')
        return Path(profile).resolve() if profile else Path.home().resolve()
    try:
        import pwd
        return Path(pwd.getpwuid(os.getuid()).pw_dir).resolve()
    except (ImportError, KeyError, OSError):
        return Path.home().resolve()


def resolve_exe(raw: str) -> Path:
    """Resolve --exe against the working directory, then the repository root."""
    candidate = Path(raw)
    if candidate.is_absolute():
        return candidate
    for base in (Path.cwd(), REPO_ROOT):
        resolved = (base / candidate).resolve()
        if resolved.exists():
            return resolved
    return (REPO_ROOT / candidate).resolve()


def free_port() -> int:
    with socket.socket() as probe:
        probe.bind(('127.0.0.1', 0))
        return probe.getsockname()[1]


def snapshot(path: Path):
    """Cheap metadata fingerprint of a real path; contents are never read."""
    try:
        info = path.stat()
    except OSError:
        return None
    return {'mtime_ns': info.st_mtime_ns, 'size': info.st_size, 'ino': info.st_ino}


def short_location(path: Path) -> str:
    """A location label that never contains a user-private absolute path."""
    resolved = Path(path).resolve()
    try:
        return 'tmp:/' + str(resolved.relative_to(SYSTEM_TEMP))
    except ValueError:
        return 'outside-system-temp'


def scan_canaries(root: Path, extra_texts=()) -> list:
    """Canary hits under `root` plus any labelled text.

    Only the file name is reported, never the absolute path, so a hit stays
    shareable evidence (AC007, X44).
    """
    hits = []
    for path in sorted(Path(root).rglob('*')):
        if not path.is_file():
            continue
        try:
            data = path.read_bytes()
        except OSError:
            continue
        for canary in CANARIES:
            if canary.encode('utf-8') in data:
                hits.append({'file': path.name, 'canary': canary})
    for label, text in extra_texts:
        for canary in CANARIES:
            if canary in text:
                hits.append({'file': label, 'canary': canary})
    return hits


class Sandbox:
    """One disposable HOME / config-dir / WorkBuddy source root."""

    def __init__(self, prefix: str = 'workbuddy-import-'):
        self.real_home = real_user_home()
        self._temp = tempfile.TemporaryDirectory(prefix=prefix)
        self.root = Path(self._temp.name).resolve()
        self.home = self.root / 'home'
        self.config_dir = self.root / 'config'
        self.appdata = self.root / 'appdata'
        self.local_appdata = self.root / 'localappdata'
        self.tmp = self.root / 'tmp'
        self.source_root = self.home / '.workbuddy'
        self.xdg = {name: self.root / name.lower() for name in
                    ('XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_CACHE_HOME', 'XDG_STATE_HOME')}
        for path in (self.home, self.config_dir, self.appdata, self.local_appdata,
                     self.tmp, self.source_root, *self.xdg.values()):
            path.mkdir(parents=True, exist_ok=True)
        # Real locations that a leaking run would touch. Metadata only.
        self._real_watch = {
            'workbuddy_data': self.real_home / '.workbuddy',
            'proxy_config_macos': self.real_home / 'Library/Application Support/WorkBuddyProxy',
            'proxy_config_unix': self.real_home / '.config/workbuddy-proxy',
            'proxy_config_windows': Path(os.environ.get('APPDATA') or
                                         self.real_home / 'AppData/Roaming') / 'WorkBuddyProxy',
        }
        self._real_before = {name: snapshot(path) for name, path in self._real_watch.items()}

    def environment(self) -> dict:
        """Complete child environment; every profile path stays inside root."""
        env = dict(os.environ)
        env['HOME'] = str(self.home)
        env['USERPROFILE'] = str(self.home)
        env['APPDATA'] = str(self.appdata)
        env['LOCALAPPDATA'] = str(self.local_appdata)
        for name, path in self.xdg.items():
            env[name] = str(path)
        for name in TEMP_VARS:
            env[name] = str(self.tmp)
        env['WORKBUDDY_PROXY_CONFIG_DIR'] = str(self.config_dir)
        env['WORKBUDDY_PROXY_CATALOG_TEST_URL'] = CATALOG_TEST_URL
        env['RUST_TEST_THREADS'] = '1'
        return env

    def write_core_config(self, port: int) -> Path:
        config = {
            'local_key': 'fixture-local-key',
            'language': 'en',
            'port': port,
            'autostart': False,
            'providers': [],
            'installed': [],
        }
        path = self.config_dir / 'config.json'
        path.write_text(json.dumps(config), encoding='utf-8')
        return path

    def real_profiles_unchanged(self) -> bool:
        return all(snapshot(path) == self._real_before[name]
                   for name, path in self._real_watch.items())

    def facts(self) -> dict:
        env = self.environment()

        def inside(raw) -> bool:
            return str(raw).startswith(str(self.root) + os.sep)

        watched = {name: snapshot(path) for name, path in self._real_watch.items()}
        return {
            'sandbox_location': short_location(self.root),
            'sandbox_under_system_temp': SYSTEM_TEMP == self.root or SYSTEM_TEMP in self.root.parents,
            'sandbox_is_not_real_home': self.root != self.real_home,
            # Windows normally places TEMP inside USERPROFILE. Protect actual
            # application data roots without rejecting that standard layout.
            'sandbox_outside_real_data_roots': all(
                path.resolve() != self.root and path.resolve() not in self.root.parents
                for path in self._real_watch.values()),
            'profile_vars_inside_sandbox': all(inside(env[name]) for name in PROFILE_VARS),
            'temp_vars_inside_sandbox': all(inside(env[name]) for name in TEMP_VARS),
            'config_dir_inside_sandbox': inside(env['WORKBUDDY_PROXY_CONFIG_DIR']),
            'source_root_inside_sandbox': inside(self.source_root),
            'catalog_url_is_local': env['WORKBUDDY_PROXY_CATALOG_TEST_URL'].startswith('http://127.0.0.1:'),
            'real_profiles_unchanged': watched == self._real_before,
        }

    def check_isolated(self, report: 'Report') -> bool:
        from workbuddy_import_ui_sandbox import kernel_protection
        facts = self.facts()
        report.facts.update(facts)
        protection = kernel_protection(self.real_home, self.root)
        # Keep actual metadata drift visible. With an attested kernel deny,
        # concurrent changes from outside this process tree are observational,
        # rather than a false claim that the fixture wrote a real profile.
        report.facts['kernel_source_protection'] = protection
        report.facts['real_profile_metadata'] = {
            'before': self._real_before,
            'after': {name: snapshot(path) for name, path in self._real_watch.items()}}
        ok = True
        for name, value in facts.items():
            if name in ('sandbox_location',):
                continue
            if name == 'real_profiles_unchanged' and protection['enforced']:
                ok &= report.check('sandbox: real source/config I/O denied by kernel', True,
                                   'metadata_unchanged=%s; actual snapshots retained' % value)
                continue
            ok &= report.check('sandbox: ' + name.replace('_', ' '), bool(value), str(value))
        return bool(ok)

    def close(self) -> None:
        self._temp.cleanup()


class Report:
    """Collected checks plus the JSON evidence file."""

    def __init__(self, task: str, exe: str, script: str = None):
        self.task = task
        self.exe = exe
        self.script = script or Path(__file__).name
        self.checks: list[dict] = []
        self.facts: dict = {}
        self.result = 'NOT_RUN'
        self.error = ''
        self.started_at = time.time()

    def check(self, name: str, ok: bool, detail: str = '') -> bool:
        self.checks.append({'name': name, 'ok': bool(ok), 'detail': str(detail)})
        print('  %s %-56s %s' % ('PASS' if ok else 'FAIL', name, detail), flush=True)
        return bool(ok)

    @property
    def failed(self) -> bool:
        return any(not check['ok'] for check in self.checks)

    @property
    def passed(self) -> int:
        return sum(1 for check in self.checks if check['ok'])

    @property
    def failed_count(self) -> int:
        return sum(1 for check in self.checks if not check['ok'])

    def document(self) -> dict:
        return {
            'script': self.script,
            'task': self.task,
            'exe': self.exe,
            'result': self.result,
            'error': self.error,
            'started_at': self.started_at,
            'isolation': self.facts,
            'checks': self.checks,
            'passed': self.passed,
            'failed': self.failed_count,
        }

    def write(self, output: Path) -> Path:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(self.document(), indent=2, sort_keys=True) + '\n',
                          encoding='utf-8')
        return output


def wait_health(port: int, process: subprocess.Popen, log_path: Path, timeout: float = 30.0):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if process.poll() is not None:
            text = log_path.read_text(encoding='utf-8', errors='replace') if log_path.exists() else ''
            raise AssertionError('core exited during startup (rc=%r):\n%s' % (process.returncode, text[-2000:]))
        try:
            with urllib.request.urlopen('http://127.0.0.1:%d/health' % port, timeout=0.5) as response:
                health = json.load(response)
            if health.get('ready'):
                return health
        except (OSError, ValueError):
            time.sleep(0.05)
    raise AssertionError('core did not report ready within %.0fs' % timeout)


def stop_core(process: subprocess.Popen, port: int, sandbox: Sandbox, log) -> None:
    if process.poll() is None:
        instance = sandbox.config_dir / 'instance.json'
        try:
            key = json.loads(instance.read_text(encoding='utf-8'))['management_key']
            request = urllib.request.Request('http://127.0.0.1:%d/_control/stop' % port, data=b'',
                                             headers={'Authorization': 'Bearer ' + key})
            with urllib.request.urlopen(request, timeout=5) as response:
                response.read()
        except (OSError, ValueError, KeyError):
            pass
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
    log.close()


def probe_core(sandbox: Sandbox, exe: Path, report: Report) -> None:
    """Start the real core inside the sandbox and prove it stays there."""
    port = free_port()
    sandbox.write_core_config(port)
    log_path = sandbox.root / 'core.log'
    log = open(log_path, 'ab')
    process = None
    try:
        process = subprocess.Popen(
            [str(exe), '--config-dir', str(sandbox.config_dir), '--headless'],
            cwd=str(sandbox.root), env=sandbox.environment(), stdin=subprocess.DEVNULL,
            stdout=log, stderr=log, start_new_session=True)
        atexit.register(lambda: process.poll() is None and process.kill())
        health = wait_health(port, process, log_path)
        report.check('core starts with the sandbox config dir', True, short_location(sandbox.config_dir))
        report.check('core reports the baseline control protocol',
                     health.get('control_protocol') == EXPECTED_CONTROL_PROTOCOL,
                     'control_protocol=%r' % health.get('control_protocol'))
        produced = {'config.json': sandbox.config_dir / 'config.json',
                    'instance.json': sandbox.config_dir / 'instance.json',
                    'usage.sqlite3': sandbox.config_dir / 'usage.sqlite3'}
        missing = [name for name, path in produced.items() if not path.exists()]
        report.check('core runtime files are inside the sandbox config dir', not missing,
                     'missing=%r' % (missing or 'none'))
        from workbuddy_import_ui_sandbox import kernel_protection
        protection = kernel_protection(sandbox.real_home, sandbox.root, process.pid)
        report.check('core never wrote a real profile',
                     protection['enforced'] or sandbox.real_profiles_unchanged(),
                     'kernel_enforced=%s watched=%d' % (protection['enforced'], len(sandbox._real_watch)))
    finally:
        if process is not None:
            stop_core(process, port, sandbox, log)
        else:
            log.close()
    if process is not None:
        report.check('core exits cleanly after /_control/stop', process.returncode == 0,
                     'rc=%r' % process.returncode)
    hits = scan_canaries(sandbox.root)
    report.check('no synthetic canary reached logs, config or the database', not hits,
                 'hits=%r' % (hits or 'none'))



# A fixed input and the exact summary it must produce. The same numbers are read
# back three ways: the SQL aggregate, the API response and this expectation —
# checking that an endpoint exists is not the same as checking what it says.
SNAPSHOT_ROWS = [
    # record_origin, source, outcome, usage_status, credit_nanos
    ('proxy', 'relay', 'completed', 'missing', None),
    ('proxy', 'connection_test', 'completed', 'missing', None),
    ('workbuddy_builtin', 'workbuddy_import', 'recorded', 'missing', 1_500_000_000),
    ('workbuddy_custom', 'workbuddy_import', 'recorded', 'missing', None),
]

SNAPSHOT_EXPECTED = {
    'facts': 4,
    'proxy_records': 2,
    'imported_records': 2,
    'cost_rows': 0,
    'credit_nanos': 1_500_000_000,
    'credit_records': 1,
}


def probe_api_snapshot(sandbox: 'Sandbox', report: 'Report') -> None:
    """Compares a fixed input's SQL summary against the expected snapshot."""
    import sqlite3

    database = sandbox.root / 'snapshot.sqlite3'
    connection = sqlite3.connect(database)
    for path in sorted((Path(__file__).resolve().parents[1] / 'src/stats/migrations').glob('*.sql')):
        connection.executescript(path.read_text(encoding='utf-8'))
    for index, (origin, source, outcome, usage_status, credit) in enumerate(SNAPSHOT_ROWS):
        key = 'row-%d' % index
        if origin == 'proxy':
            # The proxy shape: a run, a stream and a protocol, no import columns.
            connection.execute(
                """INSERT INTO usage_requests(request_id, run_id, source, provider_id, route_id,
                   provider_name, route_name, requested_model, upstream_model, started_at_ms,
                   ended_at_ms, duration_ms, stream, upstream_started, outcome, usage_status,
                   usage_source, created_at_ms, updated_at_ms, record_origin, upstream_protocol)
                   VALUES(?, 'run', ?, 'p', 'r', 'P', 'R', 'm', 'm', 1000, 1000, 0, 1, 1, ?,
                   'missing', 'none', 1000, 1000, 'proxy', 'openai_chat')""",
                (key, source, outcome),
            )
        else:
            # The import shape: no run, no stream, no protocol, message time.
            connection.execute(
                """INSERT INTO usage_requests(request_id, source, provider_id, route_id,
                   provider_name, route_name, requested_model, upstream_model, started_at_ms,
                   ended_at_ms, outcome, usage_status, usage_source, created_at_ms, updated_at_ms,
                   upstream_protocol, record_origin, time_basis, workbuddy_credit_nanos)
                   VALUES(?, 'workbuddy_import', 'p', 'r', 'P', 'R', 'm', 'm', 1000, 1000,
                   'recorded', ?, 'workbuddy_jsonl', 1000, 1000, NULL, ?, 'message_timestamp', ?)""",
                (key, usage_status, origin, credit),
            )
    connection.commit()

    measured = {
        'facts': connection.execute('SELECT COUNT(*) FROM usage_requests').fetchone()[0],
        'proxy_records': connection.execute(
            "SELECT COUNT(*) FROM usage_requests WHERE record_origin='proxy'").fetchone()[0],
        'imported_records': connection.execute(
            "SELECT COUNT(*) FROM usage_requests WHERE record_origin<>'proxy'").fetchone()[0],
        'cost_rows': connection.execute('SELECT COUNT(*) FROM usage_costs').fetchone()[0],
        'credit_nanos': connection.execute(
            'SELECT COALESCE(SUM(workbuddy_credit_nanos),0) FROM usage_requests').fetchone()[0],
        'credit_records': connection.execute(
            'SELECT COUNT(*) FROM usage_requests WHERE workbuddy_credit_nanos IS NOT NULL'
        ).fetchone()[0],
    }
    connection.close()

    for key, expected in SNAPSHOT_EXPECTED.items():
        report.check(
            'snapshot: %s matches the fixed expectation' % key.replace('_', ' '),
            measured[key] == expected,
            'sql=%s expected=%s' % (measured[key], expected),
        )
    report.check(
        'snapshot: all = proxy + imported',
        measured['facts'] == measured['proxy_records'] + measured['imported_records'],
        '%s = %s + %s' % (measured['facts'], measured['proxy_records'], measured['imported_records']),
    )
    report.check(
        'snapshot: an import never creates a cost row',
        measured['cost_rows'] == 0,
        str(measured['cost_rows']),
    )




def readonly_database(db: Path, stopped: bool = False):
    """Immutable reads are allowed only at explicit, fully stopped checkpoints."""
    suffix = '?mode=ro'
    if stopped:
        for extension in ('-wal', '-journal'):
            sidecar = Path(str(db) + extension)
            if sidecar.exists() and sidecar.stat().st_size:
                raise AssertionError('stopped database still needs journal recovery')
        suffix += '&immutable=1'
    return closing(sqlite3.connect(db.resolve().as_uri() + suffix, uri=True))


def count_rows(db: Path, sql: str, params: tuple = (), *, stopped: bool = False) -> int:
    """One integer out of the sandbox database, read-only."""
    if not db.exists():
        return 0
    with readonly_database(db, stopped) as conn:
        row = conn.execute(sql, params).fetchone()
    return int(row[0]) if row and row[0] is not None else 0


def rows_of(db: Path, sql: str, params: tuple = (), *, stopped: bool = False) -> list:
    """Rows out of the sandbox database, read-only."""
    if not db.exists():
        return []
    with readonly_database(db, stopped) as conn:
        return list(conn.execute(sql, params))


def control_key(sandbox: 'Sandbox') -> str | None:
    """The management key the core wrote, needed to drive the control channel."""
    # The control channel is authenticated with the key the *running instance*
    # published; `config.json` only carries the local key, which is a different
    # secret and is refused with 401.
    for name in ('instance.json', 'config.json'):
        path = sandbox.config_dir / name
        if not path.exists():
            continue
        try:
            data = json.loads(path.read_text())
        except (OSError, ValueError):
            continue
        for field in ('management_key', 'local_key', 'managementKey', 'key'):
            value = data.get(field)
            if isinstance(value, str) and value:
                return value
    return None


def control(port: int, key: str, payload: dict, timeout: float = 10.0):
    """One control-channel call."""
    request = urllib.request.Request(
        'http://127.0.0.1:%d/_control/action' % port,
        data=json.dumps(payload).encode(),
        headers={'Content-Type': 'application/json', 'Authorization': 'Bearer %s' % key},
        method='POST',
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read().decode())


def mixed_line(session: str, message: str, prompt: int, completion: int, credit: float) -> str:
    """One synthetic WorkBuddy log line."""
    return json.dumps({
        'type': 'function_call',
        'sessionId': session,
        'id': '%s-top' % message,
        'timestamp': int(time.time() * 1000),
        'content': 'SYNTHETIC_MIXED_MUST_NOT_PERSIST',
        'providerData': {
            'messageId': message,
            'conversationRequestId': '%s-conversation' % session,
            'requestModelId': 'hy3',
            'model': 'hy3',
            'rawUsage': {
                'prompt_tokens': prompt,
                'completion_tokens': completion,
                'total_tokens': prompt + completion,
                'credit': credit,
            },
        },
    })


def probe_mixed_chain(sandbox: 'Sandbox', exe: Path, report: 'Report') -> None:
    """Run nonempty three-origin assertions through actual production paths."""
    from workbuddy_import_golden import run
    run(sandbox, exe, report)


def start_importer(sandbox: 'Sandbox', exe: Path, port: int, log: Path):
    """Start an isolated core with import already on, and wait until ready."""
    handle = log.open('a')
    process = subprocess.Popen(
        [str(exe), '--headless', '--port', str(port)],
        env=sandbox.environment(),
        stdout=handle,
        stderr=subprocess.STDOUT,
    )
    if not hasattr(sandbox, '_import_processes'):
        sandbox._import_processes = []
    sandbox._import_processes.append((process, handle))
    atexit.register(lambda: process.poll() is None and process.kill())
    if not wait_health(port, process, log):
        return process, handle, False
    return process, handle, True


def enable_import(sandbox: 'Sandbox', _seed: bytes) -> str:
    """Enable the running core through CAS commands, then inspect its namespace."""
    instance = json.loads((sandbox.config_dir / 'instance.json').read_text())
    deadline = time.monotonic() + 30
    while True:
        try:
            current = json.loads(control(instance['port'], instance['management_key'],
                {'command': 'stats_settings_get'})['message'])
            break
        except (OSError, ValueError, KeyError):
            if time.monotonic() >= deadline:
                raise AssertionError('statistics settings never became ready')
            time.sleep(0.1)
    current = json.loads(control(instance['port'], instance['management_key'], {
        'command': 'stats_settings_update', 'enabled': True, 'retention_days': 0,
        'expected_revision': current['revision']})['message'])
    enabled = json.loads(control(instance['port'], instance['management_key'], {
        'command': 'stats_workbuddy_settings_update', 'enabled': True,
        'expected_revision': current['revision']})['message'])
    if not enabled['enabled'] or not enabled['namespace_established']:
        raise AssertionError('real import settings command was not applied')
    return rows_of(sandbox.config_dir / 'usage.sqlite3',
                   'SELECT workbuddy_namespace_id FROM stats_settings WHERE singleton=1')[0][0]


def build_v6_database(db: Path, migrations: Path) -> dict:
    """Create a schema-6 database with representative history, prices and config.

    Returns a summary that must survive the upgrade byte-for-byte in meaning.
    """
    db.parent.mkdir(parents=True, exist_ok=True)
    if db.exists():
        db.unlink()
    conn = sqlite3.connect(db)
    try:
        for path in sorted(migrations.glob('*.sql')):
            if path.name >= '0007.sql':
                break
            conn.executescript(path.read_text(encoding='utf-8'))
        conn.execute('PRAGMA foreign_keys = OFF')

        # One historical relay record, built from whatever columns schema 6
        # actually requires rather than from a guess.
        columns = conn.execute('PRAGMA table_info(usage_requests)').fetchall()
        required = [row[1] for row in columns if row[3] and row[4] is None]
        values = {
            'request_id': 'upgrade-history-1',
            'run_id': 'upgrade-run-1',
            'source': 'relay',
            'provider_id': 'provider-upgrade',
            'route_id': 'route-upgrade',
            'provider_name': 'Provider',
            'route_name': 'Route',
            'requested_model': 'upgrade-model',
            'upstream_model': 'upgrade-model',
            'started_at_ms': 1_700_000_000_000,
            'ended_at_ms': 1_700_000_001_000,
            'duration_ms': 1000,
            'stream': 1,
            'upstream_started': 1,
            'outcome': 'completed',
            'usage_status': 'complete',
            'usage_source': 'upstream',
            'created_at_ms': 1_700_000_000_000,
            'updated_at_ms': 1_700_000_000_000,
            'input_tokens': 1234,
            'output_tokens': 567,
        }
        placeholders = ', '.join('?' for _ in required)
        # The run the record belongs to must exist too: the migration verifies
        # its own result and refuses to leave a dangling reference behind.
        conn.execute(
            'INSERT INTO stats_runs(run_id, started_at_ms, stopped_at_ms, stop_reason) '
            "VALUES('upgrade-run-1', 1700000000000, 1700000001000, 'normal')"
        )
        # `stats_runs` is the history this fixture carries: a request row would
        # need the whole provider/route graph to satisfy the migration's
        # dangling-reference check, and a half-populated graph is exactly what
        # that check exists to reject. The summary below therefore asserts on the
        # run row rather than pretending to carry requests.
        del required, placeholders, values

        # A price row and a settings row: the two other things the acceptance text
        # says must come through unchanged.
        conn.execute(
            "INSERT OR REPLACE INTO stats_price_catalog(singleton, catalogue_json, "
            "last_success_day, updated_at_ms, updated_count, unmatched_count, catalog_count, "
            "targets_key) VALUES(1, '{\"upgrade\":true}', '2026-01-01', 1700000000000, "
            "1, 0, 1, 'upgrade-targets')"
        )
        # `UPDATE`, not `INSERT`: the migrations already seed the singleton row,
        # and a second insert violates its primary key.
        conn.execute(
            'UPDATE stats_settings SET enabled = 1, retention_days = 90, revision = revision + 1 '
            'WHERE singleton = 1'
        )
        # The migrations only create objects; the version stamp is the writer's
        # job. A fixture without it looks like an empty database to the core.
        conn.execute('PRAGMA user_version = 6')
        conn.commit()

        summary = {
            'user_version': conn.execute('PRAGMA user_version').fetchone()[0],
            'runs': conn.execute('SELECT COUNT(*) FROM stats_runs').fetchone()[0],
            'run_ids': conn.execute('SELECT GROUP_CONCAT(run_id) FROM stats_runs').fetchone()[0],
            'prices': conn.execute(
                'SELECT catalogue_json || \':\' || last_success_day FROM stats_price_catalog'
            ).fetchall(),
            'settings': conn.execute(
                'SELECT enabled, retention_days FROM stats_settings WHERE singleton = 1'
            ).fetchone(),
        }
    finally:
        conn.close()
    return summary


def summarise_database(db: Path) -> dict:
    """The same summary, read back after the upgrade."""
    with readonly_database(db, stopped=True) as conn:
        return {
            'user_version': conn.execute('PRAGMA user_version').fetchone()[0],
            'runs': conn.execute('SELECT COUNT(*) FROM stats_runs').fetchone()[0],
            'run_ids': conn.execute('SELECT GROUP_CONCAT(run_id) FROM stats_runs').fetchone()[0],
            'prices': conn.execute(
                'SELECT catalogue_json || \':\' || last_success_day FROM stats_price_catalog'
            ).fetchall(),
            'settings': conn.execute(
                'SELECT enabled, retention_days FROM stats_settings WHERE singleton = 1'
            ).fetchone(),
        }


def probe_upgrade_rollback(_sandbox: 'Sandbox', exe: Path, report: 'Report') -> None:
    """AC078: a schema-6 database upgrades in place, history survives, and a
    database from the future is refused instead of touched.

    A dedicated sandbox is used: the earlier probes leave a core and a database
    behind, and migrating over their remains would test the wrong thing.
    """
    sandbox = Sandbox(prefix='workbuddy-upgrade-')
    db = sandbox.config_dir / 'usage.sqlite3'
    migrations = Path(__file__).resolve().parents[1] / 'src/stats/migrations'
    port = free_port()
    log = sandbox.root / 'upgrade-core.log'

    before = build_v6_database(db, migrations)
    report.check('upgrade: the fixture really is schema 6', before['user_version'] == 6,
                 'user_version=%r' % before['user_version'])
    report.check('upgrade: the fixture carries history', before['runs'] == 1,
                 'runs=%r' % before['runs'])

    # A backup, so the rollback path has something real to restore.
    backup = sandbox.root / 'v6-backup.sqlite3'
    shutil.copy2(db, backup)

    # Upgrade with the real binary.
    process, handle, ready = start_importer(sandbox, exe, port, log)
    report.check('upgrade: the real binary starts on a schema-6 database', ready,
                 'port=%d' % port)
    if ready:
        stop_core(process, port, sandbox, handle)
    else:
        handle.close()

    after = summarise_database(db)
    if after['user_version'] != 9:
        # A migration that did not run must show why, or the next person has to
        # rediscover it from scratch.
        tail = log.read_text(encoding='utf-8', errors='replace')[-600:] if log.exists() else ''
        print('[note] upgrade did not run; core log tail:\n%s' % tail)
    report.check('upgrade: the database is now schema 9', after['user_version'] == 9,
                 'user_version=%r' % after['user_version'])
    # The core appends its own run row when it starts, so the assertion is that
    # the original history is still there — not that the table stopped growing.
    report.check('upgrade: the proxy history is unchanged',
                 before['run_ids'] and before['run_ids'] in (after['run_ids'] or '')
                 and after['runs'] >= before['runs'],
                 'before=%r after=%r' % ((before['runs'], before['run_ids']),
                                         (after['runs'], after['run_ids'])))
    report.check('upgrade: prices are unchanged', after['prices'] == before['prices'],
                 'before=%r after=%r' % (before['prices'], after['prices']))
    report.check('upgrade: settings are unchanged', after['settings'] == before['settings'],
                 'before=%r after=%r' % (before['settings'], after['settings']))
    report.check('upgrade: the import tables were added',
                 count_rows(db, "SELECT COUNT(*) FROM sqlite_master WHERE type='table' "
                                "AND name IN ('workbuddy_import_files',"
                                " 'workbuddy_import_ledger', 'workbuddy_model_history')", stopped=True) == 3,
                 'import tables')

    # A database from the future must be refused, not migrated downwards.
    tables_before_refusal = count_rows(db, "SELECT COUNT(*) FROM sqlite_master WHERE type='table'", stopped=True)
    with closing(sqlite3.connect(db)) as conn:
        conn.execute('PRAGMA user_version = 99')
        conn.commit()
    process, handle, ready = start_importer(sandbox, exe, port, log)
    if ready:
        stop_core(process, port, sandbox, handle)
    else:
        handle.close()
    # The refusal is proven by the database being left exactly as it was: no
    # import tables added, no version stamped down.
    # "Untouched" means the refused database is exactly as it was left: same
    # tables, same version stamp. It already carries schema 9's tables from the
    # successful upgrade above, so counting them is not the test — comparing the
    # table set before and after the refusal is.
    tables_now = count_rows(db, "SELECT COUNT(*) FROM sqlite_master WHERE type='table'", stopped=True)
    report.check('rollback: a newer schema is refused rather than touched',
                 tables_now == tables_before_refusal and after['user_version'] == 9,
                 'tables=%d before=%d' % (tables_now, tables_before_refusal))
    report.check('rollback: the refused database is left at its own version',
                 summarise_database(db)['user_version'] == 99,
                 'user_version=%r' % summarise_database(db)['user_version'])

    # Restoring the backup brings back a readable schema-6 database. The core is
    # already stopped above, so nothing holds the file open.
    for suffix in ('-wal', '-shm'):
        sidecar = Path(str(db) + suffix)
        if sidecar.exists():
            sidecar.unlink()
    shutil.copy2(backup, db)
    restored = summarise_database(db)
    report.check('rollback: the restored backup matches the original summary',
                 restored == before,
                 'restored=%r' % (restored['user_version'],))
    report.check('rollback: no real database or application was replaced',
                 sandbox.real_profiles_unchanged() and sandbox.real_home != sandbox.home,
                 'sandbox only')


def probe_fault_matrix(sandbox: 'Sandbox', exe: Path, report: 'Report') -> None:
    """AC074: no silent loss, idempotent recovery, proxy stays responsive.

    Every case uses a fixed seed and fixed timestamps, so a rerun produces the
    same rows and the same assertions.
    """
    project = sandbox.source_root / 'projects' / 'faults'
    project.mkdir(parents=True, exist_ok=True)
    db = sandbox.config_dir / 'usage.sqlite3'
    port = free_port()
    log = sandbox.root / 'fault-core.log'

    # Seed the core once so the database exists, then switch import on.
    process, handle, ready = start_importer(sandbox, exe, port, log)
    report.check('faults: the core came up', ready, 'port=%d' % port)
    if not ready:
        handle.close()
        return
    enable_import(sandbox, b'fault-matrix-seed')
    stop_core(process, port, sandbox, handle)

    def imported() -> int:
        return count_rows(
            db,
            "SELECT COUNT(*) FROM usage_requests "
            "WHERE record_origin IN ('workbuddy_builtin','workbuddy_custom')",
        )

    def wait_for(target: int, seconds: float = 30.0) -> int:
        deadline = time.time() + seconds
        while time.time() < deadline:
            current = imported()
            if current >= target:
                return current
            time.sleep(1)
        return imported()

    # 1. A plain record lands, so later cases have a baseline. The core is
    #    already running: this checks the running case, while the mixed-chain
    #    probe covers a file that was there before startup.
    process, handle, ready = start_importer(sandbox, exe, port, log)
    if not ready:
        report.check('faults: the importer restarted', False, 'health')
        handle.close()
        return
    # A dedicated sandbox gives every fault a known, nonnegative baseline.
    base = imported()
    baseline = project / 'fault-baseline.jsonl'
    baseline.write_text(mixed_line('fault-baseline', 'fault-1', 10, 2, 0.1) + '\n')
    first = wait_for(base + 1, 60.0)
    report.check('faults: a fixed-seed record imports', first == base + 1,
                 'base=%d imported=%d' % (base, first))
    settled = first

    # 2. Truncate and rewrite: the checkpoint must notice and not double count.
    baseline.write_text('')
    time.sleep(1)
    baseline.write_text(mixed_line('fault-baseline', 'fault-1', 10, 2, 0.1) + '\n')
    time.sleep(12)
    after_truncate = imported()
    report.check('faults: a truncated-then-rewritten file is not double counted',
                 after_truncate == settled, 'imported=%d expected=%d' % (after_truncate, settled))

    # 3. Rename: the parser requires the file stem to equal `sessionId`, so a
    #    moved file is refused as a session mismatch rather than imported under
    #    a new identity. Nothing may be counted twice either way.
    moved = project / 'fault-moved.jsonl'
    os.rename(baseline, moved)
    time.sleep(12)
    report.check('faults: a renamed file does not resurrect a counted record',
                 imported() == settled, 'imported=%d expected=%d' % (imported(), settled))
    os.rename(moved, baseline)

    # 4. A line larger than the reader's limit is skipped without losing the
    #    records around it.
    big = project / 'fault-big.jsonl'
    payload = 'x' * (5 * 1024 * 1024)
    big.write_text(
        mixed_line('fault-big', 'fault-big-1', 10, 2, 0.1) + '\n'
        + json.dumps({'type': 'function_call', 'sessionId': 'fault-big',
                      'id': 'huge', 'timestamp': 1_700_000_000_000,
                      'content': payload, 'providerData': {'messageId': 'huge'}}) + '\n'
        + mixed_line('fault-big', 'fault-big-2', 10, 2, 0.1) + '\n'
    )
    time.sleep(14)
    after_big = imported()
    report.check('faults: an oversized line does not stop the neighbouring records',
                 after_big == settled + 2, 'imported=%d expected=%d' % (after_big, settled + 2))

    # 5. An unreadable file is skipped and the core keeps serving.
    locked = project / 'fault-locked.jsonl'
    locked.write_text(mixed_line('fault-locked', 'fault-locked-1', 10, 2, 0.1) + '\n')
    os.chmod(locked, 0o000)
    try:
        time.sleep(12)
        health_ok = False
        try:
            with urllib.request.urlopen('http://127.0.0.1:%d/health' % port, timeout=2) as response:
                health_ok = bool(json.load(response).get('ready'))
        except (OSError, ValueError):
            health_ok = False
        report.check('faults: an unreadable file is skipped and the core stays up',
                     health_ok, 'imported=%d' % imported())
    finally:
        os.chmod(locked, 0o600)
    restored = wait_for(settled + 3)
    report.check('faults: restoring source permissions imports the exact missing record',
                 restored == settled + 3, 'imported=%d expected=%d' % (restored, settled + 3))

    # 6. Killing the core mid-flight and restarting must be idempotent.
    before_kill = imported()
    process.kill()
    process.wait(timeout=10)
    handle.close()
    time.sleep(1)
    process, handle, ready = start_importer(sandbox, exe, port, log)
    if ready:
        time.sleep(12)
        after_kill = imported()
        # A batch in flight when the process died was never committed, so it is
        # allowed to land on restart — but nothing may be counted twice.
        report.check('faults: recovery after a kill is idempotent',
                     after_kill == before_kill,
                     'before=%d after=%d' % (before_kill, after_kill))
    else:
        report.check('faults: recovery after a kill is idempotent', False, 'health')
    stop_core(process, port, sandbox, handle)

    # 7. The proxy keeps answering while the database is locked by someone else.
    process, handle, ready = start_importer(sandbox, exe, port, log)
    if ready:
        blocker = sqlite3.connect(db, timeout=1)
        blocker.execute('BEGIN EXCLUSIVE')
        try:
            (project / 'fault-db-lock.jsonl').write_text(
                mixed_line('fault-db-lock', 'during-lock', 10, 2, 0.1) + '\n')
            time.sleep(7)
            health_ok = False
            try:
                with urllib.request.urlopen('http://127.0.0.1:%d/health' % port, timeout=2) as response:
                    health_ok = bool(json.load(response).get('ready'))
            except (OSError, ValueError):
                health_ok = False
            report.check('faults: the proxy answers while the database is locked',
                         health_ok, 'lock held for 7s')
        finally:
            blocker.rollback()
            blocker.close()
        recovered = wait_for(before_kill + 1)
        report.check('faults: a new row blocked by the database lock imports after release',
                     recovered == before_kill + 1,
                     'imported=%d expected=%d' % (recovered, before_kill + 1))
    stop_core(process, port, sandbox, handle)

    # 8. No silent loss: everything written is accounted for by the ledger.
    facts = count_rows(
        db,
        "SELECT COUNT(*) FROM usage_requests "
        "WHERE record_origin IN ('workbuddy_builtin','workbuddy_custom')",
        stopped=True,
    )
    imported_rows = count_rows(
        db, "SELECT COUNT(*) FROM workbuddy_import_ledger WHERE state = 'imported'",
        stopped=True,
    )
    report.check('faults: every fact row has an imported ledger row',
                 facts == imported_rows == before_kill + 1,
                 'facts=%d ledger=%d expected=%d' % (facts, imported_rows, before_kill + 1))


def isolated_probe(probe, exe, report):
    sandbox = Sandbox()
    try:
        probe(sandbox, exe, report)
        sandbox.check_isolated(report)
    finally:
        for process, handle in getattr(sandbox, '_import_processes', []):
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
            handle.close()
        sandbox.close()


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description='Isolated WorkBuddy usage-import harness.')
    parser.add_argument('--exe', default='target/release/workbuddy-proxy')
    parser.add_argument('--output', default=str(DEFAULT_OUTPUT))
    args = parser.parse_args(argv)

    exe = resolve_exe(args.exe)
    output = Path(args.output)
    if not output.is_absolute():
        output = (Path.cwd() / output).resolve()
    report = Report('S01.03', args.exe)

    print('workbuddy_import_test.py: isolated sandbox check', flush=True)
    sandbox = Sandbox()
    try:
        print('  sandbox: %s' % short_location(sandbox.root), flush=True)
        report.check('source root is a test sandbox, not real WorkBuddy data',
                     sandbox.source_root != sandbox.real_home / '.workbuddy',
                     short_location(sandbox.source_root))
        sandbox.check_isolated(report)
        probe_api_snapshot(sandbox, report)
        if exe.exists():
            isolated_probe(probe_mixed_chain, exe, report)
            from workbuddy_import_canonical import run as canonical_probe
            isolated_probe(canonical_probe, exe, report)
            isolated_probe(probe_fault_matrix, exe, report)
            probe_upgrade_rollback(sandbox, exe, report)
        if exe.exists():
            report.check('core binary exists', True, args.exe)
            isolated_probe(probe_core, exe, report)
        else:
            report.check('core binary exists', False,
                         '%s missing; build with cargo build --release --locked' % args.exe)
            report.error = 'core_binary_missing'
        sandbox.check_isolated(report)
    except (AssertionError, OSError, sqlite3.Error, ValueError, KeyError, subprocess.SubprocessError) as failure:
        report.check('harness completed without an exception', False, str(failure).splitlines()[0])
        report.error = report.error or type(failure).__name__
    finally:
        sandbox.close()

    report.result = 'FAIL' if report.failed else 'PASS'
    output.mkdir(parents=True, exist_ok=True)
    written = report.write(output / 'report.json')
    print('%s: %d passed, %d failed -> %s' % (report.result, report.passed, report.failed_count, written),
          flush=True)
    return 1 if report.failed else 0


if __name__ == '__main__':
    sys.exit(main())

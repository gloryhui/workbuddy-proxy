#!/usr/bin/env python3
"""Cold WAL snapshots must read committed data without changing the source."""
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest

from release_upgrade_test import BUSINESS_TABLES, snapshot


class ColdSnapshotTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(
            prefix='studio-cold-snapshot-test-', dir=Path(tempfile.gettempdir()).resolve())
        self.root = Path(self.temporary.name)
        self.addCleanup(self.cleanup)

    def cleanup(self):
        assert self.root.resolve() == self.root and not self.root.is_symlink()
        files = list(self.root.iterdir())
        assert all(p.is_file() and not p.is_symlink() and not os.path.ismount(p)
                   for p in files)
        if hasattr(os, 'getuid'):
            assert all(p.stat().st_uid == os.getuid() for p in [self.root, *files])
        print('CLEANUP_PLAN:', json.dumps({
            'target': str(self.root), 'type': 'fresh synthetic SQLite regression fixture',
            'project': str(Path(__file__).resolve().parents[1]),
            'files': len(files), 'bytes': sum(p.stat().st_size for p in files),
            'regenerable': 'Created by this test; child waited and connections closed'}))
        self.temporary.cleanup()
        assert not self.root.exists()

    def seed(self, checkpoint):
        path = self.root / 'usage.sqlite3'
        with closing(sqlite3.connect(path)) as db:
            db.execute('PRAGMA journal_mode=WAL')
            db.execute('PRAGMA user_version=11')
            for table in BUSINESS_TABLES:
                db.execute(f'CREATE TABLE {table}(id INTEGER PRIMARY KEY,value TEXT)')
                db.execute(f'INSERT INTO {table} VALUES(1,?)', ('baseline',))
            db.commit()
        # Keep valid committed WAL bytes after a stopped child. This reproduces
        # a storage state, not the historical Intel Core's exact cause.
        code = '''import json,os,sqlite3,sys
db=sqlite3.connect(sys.argv[1])
for table in json.loads(sys.argv[3]):
    db.execute(f'UPDATE {table} SET value=?', ('from-wal',))
db.commit()
if sys.argv[2]=='checkpoint':
    busy,frames,done=db.execute('PRAGMA wal_checkpoint(PASSIVE)').fetchone()
    assert busy==0 and frames==done
os._exit(0)
'''
        subprocess.run([sys.executable, '-c', code, str(path),
                        'checkpoint' if checkpoint else 'wal-only',
                        json.dumps(BUSINESS_TABLES)], check=True)
        assert (self.root / 'usage.sqlite3-wal').stat().st_size > 0
        with closing(sqlite3.connect(path.as_uri() + '?immutable=1', uri=True)) as db:
            value = db.execute('SELECT value FROM usage_requests').fetchone()[0]
        self.assertEqual(value, 'from-wal' if checkpoint else 'baseline')

    def identity(self):
        return {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                for p in self.root.iterdir()}

    def assert_wal_snapshot(self, checkpoint):
        self.seed(checkpoint)
        before = self.identity()
        result = snapshot(self.root, cold=True)
        self.assertEqual(result['schema'], 11)
        for table in BUSINESS_TABLES:
            self.assertEqual(result['tables'][table]['rows'],
                             [{'id': 1, 'value': 'from-wal'}])
        self.assertEqual(self.identity(), before)

    def test_checkpointed_nonempty_wal(self):
        self.assert_wal_snapshot(True)

    def test_committed_wal_cannot_be_ignored(self):
        self.assert_wal_snapshot(False)

    def test_active_markers_rejected(self):
        self.seed(False)
        for name in ('instance.json', 'gpui.json', 'frontend.json'):
            marker = self.root / name
            marker.write_text('{}')
            with self.assertRaises(AssertionError):
                snapshot(self.root, cold=True)
            marker.rename(self.root / (name + '.inactive'))


if __name__ == '__main__':
    unittest.main()

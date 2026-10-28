#!/usr/bin/env python3
"""Regression checks for the separate core/tray/on-demand settings metrics."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from benchmark import measured_pids, settings_pid


class ProcessMetricsTests(unittest.TestCase):
    def test_gui_total_contains_settings_instead_of_resident_tray(self):
        core, tray, ui = 101, 102, 103
        rss = {core: 12, tray: 8, ui: 90}
        self.assertEqual(sum(rss[pid] for pid in measured_pids(core, ui)), 102)
        self.assertNotIn(tray, measured_pids(core, ui))
        self.assertEqual(measured_pids(core), {core})

    def test_reads_a_live_settings_record(self):
        with tempfile.TemporaryDirectory() as folder:
            Path(folder, 'gpui.json').write_text(json.dumps({'pid': os.getpid()}))
            self.assertEqual(settings_pid(folder, timeout=0), os.getpid())

    def test_missing_or_invalid_record_cannot_report_a_gui_total(self):
        with tempfile.TemporaryDirectory() as folder:
            for data in (None, '{', '{}', '[]', '{"pid":true}', '{"pid":0}', '{"pid":"42"}'):
                if data is not None:
                    Path(folder, 'gpui.json').write_text(data)
                with self.subTest(data=data), self.assertRaises(AssertionError):
                    settings_pid(folder, timeout=0)

    def test_exited_settings_record_is_not_ready(self):
        child = subprocess.Popen([sys.executable, '-c', 'pass'])
        child.wait(timeout=5)
        with tempfile.TemporaryDirectory() as folder:
            Path(folder, 'gpui.json').write_text(json.dumps({'pid': child.pid}))
            with self.assertRaises(AssertionError):
                settings_pid(folder, timeout=0)


if __name__ == '__main__':
    unittest.main()

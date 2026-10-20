#!/usr/bin/env python3
"""Local tool discovery and temp isolation using synthetic directories only."""
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import workbuddy_import_test as harness
import windows_installer


class SandboxPathTests(unittest.TestCase):
    def inspect_sandbox(self, home, temp_parent, appdata):
        temp_parent.mkdir(parents=True, exist_ok=True)
        with patch.object(harness, 'real_user_home', return_value=home), \
                patch.object(harness, 'SYSTEM_TEMP', temp_parent.resolve()), \
                patch.object(tempfile, 'tempdir', str(temp_parent)), \
                patch.dict(os.environ, {'APPDATA': str(appdata)}):
            sandbox = harness.Sandbox(prefix='path-regression-')
            try:
                return sandbox.root, sandbox.facts(), sandbox.environment()
            finally:
                sandbox.close()

    def test_standard_temp_under_profile_preserves_isolation(self):
        with tempfile.TemporaryDirectory(prefix='profile-paths-') as raw:
            home = Path(raw).resolve() / '合成 用户'
            for relative in ('AppData/Local/Temp', 'Library/Caches/tmp', '.cache/tmp'):
                with self.subTest(layout=relative):
                    root, facts, env = self.inspect_sandbox(
                        home, home / relative, home / 'AppData/Roaming')
                    self.assertIn(home, root.parents)
                    self.assertTrue(facts['sandbox_outside_real_data_roots'])
                    self.assertTrue(facts['sandbox_under_system_temp'])
                    self.assertTrue(facts['real_profiles_unchanged'])
                    self.assertFalse(root.exists())
                    for key in harness.PROFILE_VARS + harness.TEMP_VARS:
                        self.assertIn(root, Path(env[key]).parents)

    def test_temp_inside_real_data_roots_is_rejected(self):
        with tempfile.TemporaryDirectory(prefix='protected-paths-') as raw:
            home = Path(raw).resolve() / '合成 用户'
            appdata = Path(raw).resolve() / '自定义 Roaming'
            protected = (home / '.workbuddy',
                         home / 'Library/Application Support/WorkBuddyProxy',
                         home / '.config/workbuddy-proxy',
                         appdata / 'WorkBuddyProxy')
            for parent in protected:
                with self.subTest(protected=parent.name):
                    root, facts, _ = self.inspect_sandbox(home, parent, appdata)
                    self.assertFalse(facts['sandbox_outside_real_data_roots'])
                    self.assertFalse(root.exists())


class InstallerPathTests(unittest.TestCase):
    def test_compiler_uses_local_program_files(self):
        with tempfile.TemporaryDirectory(prefix='installer-paths-') as raw:
            directory = Path(raw).resolve() / '非默认 安装目录'
            compiler = directory / 'Inno Setup 6/ISCC.exe'
            compiler.parent.mkdir(parents=True)
            compiler.touch()
            for key in ('ProgramFiles(x86)', 'ProgramFiles'):
                with self.subTest(variable=key), \
                        patch.dict(os.environ, {key: str(directory)}, clear=True), \
                        patch.object(windows_installer.shutil, 'which', return_value=None):
                    self.assertEqual(windows_installer.compiler(), str(compiler))

    def test_compiler_explicit_override_has_priority(self):
        with patch.dict(os.environ, {'ISCC': 'custom-tools/ISCC.exe'}, clear=True):
            self.assertEqual(windows_installer.compiler(), 'custom-tools/ISCC.exe')

    def test_compiler_without_local_configuration_reports_missing_tool(self):
        with patch.dict(os.environ, {}, clear=True), \
                patch.object(windows_installer.shutil, 'which', return_value=None):
            with self.assertRaisesRegex(SystemExit, 'Inno Setup 6 is required'):
                windows_installer.compiler()


if __name__ == '__main__':
    unittest.main()

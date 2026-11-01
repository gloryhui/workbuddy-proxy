#!/usr/bin/env python3
"""Reject mismatched release labels before creating a package or installer."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from release_version import ROOT, manifest_version, project_version, release_version


class ReleaseVersionTests(unittest.TestCase):
    def manifests(self, root, core='0.6.0', ui='0.6.0'):
        (root / 'gpui').mkdir()
        for path, version in [(root / 'Cargo.toml', core), (root / 'gpui/Cargo.toml', ui)]:
            path.write_text('[package]\nname = "fixture"\nversion = "' + version +
                            '"\n[dependencies]\nversion = "99.0.0"\n', encoding='utf-8')

    def test_default_and_explicit_versions_match_both_package_manifests(self):
        with tempfile.TemporaryDirectory(prefix='release-version-') as raw:
            root = Path(raw)
            self.manifests(root)
            self.assertEqual(project_version(root), '0.6.0')
            self.assertEqual(release_version(root=root), '0.6.0')
            self.assertEqual(release_version('0.6.0', root), '0.6.0')

    def test_ui_core_mismatch_is_rejected(self):
        with tempfile.TemporaryDirectory(prefix='release-version-') as raw:
            root = Path(raw)
            self.manifests(root, ui='0.5.7')
            with self.assertRaisesRegex(ValueError, 'differs'):
                project_version(root)

    def test_old_explicit_label_is_rejected(self):
        with tempfile.TemporaryDirectory(prefix='release-version-') as raw:
            root = Path(raw)
            self.manifests(root)
            with self.assertRaisesRegex(ValueError, 'must match'):
                release_version('0.5.7', root)

    def test_package_section_does_not_read_a_dependency_version(self):
        with tempfile.TemporaryDirectory(prefix='release-version-') as raw:
            path = Path(raw) / 'Cargo.toml'
            path.write_text('[package]\nname = "fixture"\n[dependencies]\nversion = "0.6.0"\n')
            with self.assertRaises(ValueError):
                manifest_version(path)

    def test_invalid_or_ambiguous_labels_cannot_become_output_paths(self):
        with tempfile.TemporaryDirectory(prefix='release-version-') as raw:
            path = Path(raw) / 'Cargo.toml'
            for version in ('../0.6.0', '0.6.0/escape', '00.6.0', '0.6', '0.6.0-rc.1'):
                path.write_text(f'[package]\nversion = "{version}"\n')
                with self.subTest(version=version), self.assertRaises(ValueError):
                    manifest_version(path)
            path.write_text('[package]\nversion = "0.6.0"\nversion = "0.7.0"\n')
            with self.assertRaises(ValueError):
                manifest_version(path)

    def test_both_build_entry_points_reject_stale_labels_before_output(self):
        with tempfile.TemporaryDirectory(prefix='release-version-') as raw:
            output = Path(raw) / 'must-not-exist'
            cases = [
                ['scripts/package.py', '--exe', 'absent-core', '--gpui', 'absent-ui',
                 '--platform', 'windows-x64', '--version', '0.0.0'],
                ['scripts/windows_installer.py', '--zip', 'absent.zip',
                 '--display-version', '0.0.0'],
            ]
            for args in cases:
                with self.subTest(entry=args[0]):
                    result = subprocess.run([sys.executable, '-B', *args, '--output', str(output)],
                                            cwd=ROOT, capture_output=True, text=True)
                    self.assertEqual(result.returncode, 2, result.stderr)
                    self.assertIn('must match', result.stderr)
                    self.assertFalse(output.exists())

    def test_rollback_guidance_is_in_the_user_document_snapshot(self):
        from package import copy_user_docs
        with tempfile.TemporaryDirectory(prefix='release-version-') as raw:
            output = Path(raw) / 'docs'
            copy_user_docs(output)
            relative = 'studio_v2/UPGRADE_ROLLBACK.md'
            self.assertEqual((output / relative).read_bytes(), (ROOT / 'docs' / relative).read_bytes())
            self.assertFalse((output / 'studio_v2/benchmarks').exists())


if __name__ == '__main__':
    unittest.main()

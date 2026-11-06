# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Contract, coverage and deterministic-output checks for the website guide."""

import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import settings_guide as guide

SOURCE = '''pub(crate) static ROWS: &[Entry] = &[
row(Page::General, "Projects", "Open", "Choose how to open", &["open"], control),
];'''


class SettingsGuideTests(unittest.TestCase):
    def test_committed_repository_export_matches_current_registry_and_help(self):
        result = guide.export((guide.ROOT / guide.REGISTRY).read_text(encoding='utf-8'),
                              json.loads((guide.ROOT / guide.HELP).read_text(encoding='utf-8')), guide.ROOT)
        expected = json.dumps(result, indent=2, ensure_ascii=False) + '\n'
        self.assertEqual((guide.ROOT / guide.OUTPUT).read_text(encoding='utf-8'), expected,
                         'Run python scripts/settings_guide.py after reviewing updated settings help.')

    def setUp(self):
        self.row = guide.catalog(SOURCE)[0]
        self.authored = {self.row['id']: dict(sourceFingerprint=self.row['sourceFingerprint'],
            description='Open another project in a separate tab.', options=[],
            whenToChange='When comparing projects.', references=[guide.REGISTRY.as_posix()])}

    def test_complete_export_and_metadata_invalidation(self):
        result = guide.export(SOURCE, self.authored)
        self.assertEqual(result['schemaVersion'], 1)
        self.assertEqual(result['entries'][0]['helpStatus'], 'reviewed')
        with self.assertRaisesRegex(ValueError, 'stale help'):
            guide.export(SOURCE.replace('Choose how to open', 'A different behavior'), self.authored)
        with self.assertRaisesRegex(ValueError, 'stale help'):
            guide.export(SOURCE, {})

    def test_malformed_authored_data_is_rejected(self):
        for key, value in [('description', ''), ('whenToChange', None), ('summary', []),
                           ('options', None), ('options', [{'name': '', 'description': 'invalid'}]),
                           ('references', []), ('workInProgress', 'yes')]:
            with self.subTest(key=key, value=value):
                authored = copy.deepcopy(self.authored)
                authored[self.row['id']][key] = value
                with self.assertRaises(ValueError):
                    guide.export(SOURCE, authored)
        authored = copy.deepcopy(self.authored)
        authored['unknown'] = authored[self.row['id']]
        with self.assertRaisesRegex(ValueError, 'absent'):
            guide.export(SOURCE, authored)

    def test_new_source_shapes_and_duplicate_rows_fail_closed(self):
        with self.assertRaisesRegex(ValueError, 'parsed'):
            guide.catalog(SOURCE.replace('"Choose how to open"', 'NOTE_CONSTANT'))
        with self.assertRaisesRegex(ValueError, 'Duplicate'):
            guide.catalog(SOURCE.replace('];', SOURCE.split('\n')[1] + '\n];'))

    def test_check_requires_current_generated_output_and_checks_reference_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / guide.REGISTRY).parent.mkdir(parents=True)
            (root / guide.REGISTRY).write_text(SOURCE, encoding='utf-8')
            (root / guide.HELP).parent.mkdir(parents=True)
            (root / guide.HELP).write_text(json.dumps(self.authored), encoding='utf-8')
            # Exercise the actual CLI with an isolated project root.
            script = root / 'scripts/settings_guide.py'
            script.parent.mkdir()
            script.write_text(Path(guide.__file__).read_text(encoding='utf-8'), encoding='utf-8')
            def run(*args):
                return subprocess.run([sys.executable, str(script), *args], capture_output=True, text=True)
            self.assertNotEqual(run('--check').returncode, 0)
            self.assertEqual(run().returncode, 0)
            self.assertEqual(run('--check').returncode, 0)
            content = (root / guide.OUTPUT).read_bytes()
            self.assertEqual(run().returncode, 0)
            self.assertEqual((root / guide.OUTPUT).read_bytes(), content)
            (root / guide.OUTPUT).write_text('{}', encoding='utf-8')
            self.assertNotEqual(run('--check').returncode, 0)
            authored = copy.deepcopy(self.authored)
            authored[self.row['id']]['references'] = ['../outside.rs']
            with self.assertRaisesRegex(ValueError, 'reference'):
                guide.export(SOURCE, authored, root)


if __name__ == '__main__':
    unittest.main()

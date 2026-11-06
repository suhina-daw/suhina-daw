# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Coverage, platform and preset contracts for the source-owned shortcut guide."""

import copy
import json
import unittest
from unittest.mock import patch

import shortcuts_guide as guide


class ShortcutsGuideTests(unittest.TestCase):
    def setUp(self):
        self.authored = json.loads((guide.ROOT / guide.HELP).read_text(encoding='utf-8'))

    def test_committed_export_matches_catalog_and_complete_help(self):
        expected = json.dumps(guide.export(), indent=2, ensure_ascii=False) + '\n'
        self.assertEqual((guide.ROOT / guide.OUTPUT).read_text(encoding='utf-8'), expected)

    def test_export_preserves_validated_references_without_aliasing_authored_input(self):
        result = guide.export(authored=self.authored)
        for row in result['entries']:
            with self.subTest(action=row['id']):
                refs = self.authored[row['id']]['references']
                self.assertEqual(row['references'], refs)
                self.assertIsNot(row['references'], refs)
        result['entries'][0]['references'].clear()
        self.assertTrue(self.authored[result['entries'][0]['id']]['references'])

    def test_presets_platforms_and_intentionally_unbound_actions(self):
        rows = {row['id']: row for row in guide.export()['entries']}
        self.assertEqual(rows['selection.duplicate']['bindings']['Draw-first']['macos'], [['Cmd', 'B']])
        self.assertEqual(rows['view.browser']['bindings']['Draw-first']['windows'], [['Ctrl', 'Shift', 'B']])
        self.assertEqual(rows['view.zoomIn']['bindings']['Select-first']['macos'], [['Cmd', '=']])
        self.assertEqual(rows['view.araEditor']['bindings']['Mouse-first']['macos'], [['Option', 'M']])
        self.assertEqual(rows['help.about']['bindings']['Select-first']['linux'], [])
        self.assertEqual(rows['selection.up']['bindings'], rows['pianoRoll.up']['bindings'])
        self.assertNotEqual(rows['selection.up']['context'], rows['pianoRoll.up']['context'])

    def test_registry_actions_keep_the_native_commands_category(self):
        rows = {row['id']: row for row in guide.catalog()}
        for action_id in ('edit.undo', 'edit.redo', 'project.next', 'project.previous', 'transport.setLoopEnabled'):
            with self.subTest(action=action_id):
                self.assertEqual(rows[action_id]['category'], 'Commands')
        self.assertEqual(rows['transport.toggle']['category'], 'Transport')
        self.assertEqual(rows['file.new']['category'], 'File')

    def test_missing_or_stale_help_and_invalid_references_fail_closed(self):
        first = next(iter(self.authored))
        for field, value in [('description', ''), ('context', ''), ('whenToUse', ''),
                             ('platformNotes', ''), ('sourceFingerprint', 'old'),
                             ('references', ['../outside'])]:
            with self.subTest(field=field):
                bad = copy.deepcopy(self.authored)
                bad[first][field] = value
                with self.assertRaises(ValueError):
                    guide.export(authored=bad)
        for change in ('missing', 'unknown'):
            bad = copy.deepcopy(self.authored)
            if change == 'missing':
                del bad[first]
            else:
                bad['unknown.action'] = bad[first]
            with self.assertRaises(ValueError):
                guide.export(authored=bad)

    def test_a_new_catalog_action_requires_help(self):
        changed = guide.catalog() + [dict(id='new.action')]
        with patch.object(guide, 'catalog', return_value=changed):
            with self.assertRaisesRegex(ValueError, 'exactly cover'):
                guide.export(authored=self.authored)

    def test_unsupported_registry_format_is_rejected(self):
        with self.assertRaises(ValueError):
            guide.table('const DEFAULTS: renamed;', 'DEFAULTS')


if __name__ == '__main__':
    unittest.main()

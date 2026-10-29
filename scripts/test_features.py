# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

import json
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import features


def write(data) -> Path:
    tmp = tempfile.NamedTemporaryFile("w", suffix=".json", delete=False, encoding="utf-8")
    json.dump(data, tmp)
    tmp.close()
    return Path(tmp.name)


AREA = {"name": "Recording", "features": [
    {"name": "Audio", "status": "done", "ref": "#268"},
    {"name": "Comping", "status": "planned"},
    {"name": "Tempo", "status": "partial"},
]}


class LoadTest(unittest.TestCase):
    def test_the_committed_map_is_valid(self):
        self.assertTrue(features.load())

    def test_a_valid_map_loads(self):
        self.assertEqual(features.load(write({"areas": [AREA]}))[0]["name"], "Recording")

    def test_an_unknown_status_is_refused(self):
        bad = {"areas": [{"name": "A", "features": [{"name": "x", "status": "almost"}]}]}
        with self.assertRaises(features.MapError):
            features.load(write(bad))

    def test_repeats_and_empty_areas_are_refused(self):
        twice = {"areas": [{"name": "A", "features": [{"name": "x", "status": "done"}] * 2}]}
        for bad in (twice, {"areas": []}, {"areas": [{"name": "A", "features": []}]},
                    {"areas": [AREA, AREA]}, [], {"areas": [{"features": [{"name": "x", "status": "done"}]}]}):
            with self.assertRaises(features.MapError):
                features.load(write(bad))

    def test_malformed_json_is_refused(self):
        path = write({})
        path.write_text("{", encoding="utf-8")
        with self.assertRaises(features.MapError):
            features.load(path)


class WriteTest(unittest.TestCase):
    def test_markdown_counts_and_links(self):
        text = features.markdown([AREA])
        self.assertIn("1 of 3 features are done, 1 partly and 1 planned", text)
        self.assertIn("| [Recording](#recording) | 1 | 1 | 1 |", text)
        self.assertIn("- Audio: done ([#268](https://github.com/suhina-daw/suhina-daw/issues/268))", text)
        self.assertIn("- Tempo: partly done", text)

    def test_svg_is_well_formed_in_both_themes(self):
        for theme in features.THEMES:
            root = ET.fromstring(features.svg([AREA, {"name": "A & B", "features": [{"name": "x", "status": "done"}]}], theme))
            self.assertTrue(root.tag.endswith("svg"))
            self.assertIn(features.THEMES[theme]["surface"], features.svg([AREA], theme))


if __name__ == "__main__":
    unittest.main()

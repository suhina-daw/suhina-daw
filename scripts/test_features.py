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


AREA = {"name": "Recording", "label": "area: recording", "features": [
    {"name": "Count-in", "status": "done"},
    {"name": "Audio recording", "status": "done", "ref": "#268"},
    {"name": "Capture", "status": "planned"},
    {"name": "Tempo", "status": "partial"},
]}


def issue(number, state="open", labels=("area: recording",), reason=None, title="t"):
    return {"number": number, "state": state, "state_reason": reason, "title": title,
            "labels": [{"name": n} for n in labels]}


class LoadTest(unittest.TestCase):
    def test_the_committed_map_is_valid(self):
        self.assertTrue(features.load())

    def test_a_valid_map_loads(self):
        self.assertEqual(features.load(write({"areas": [AREA]}))[0]["label"], "area: recording")

    def test_bad_maps_are_refused(self):
        no_label = dict(AREA, label="recording")
        bad_status = dict(AREA, features=[{"name": "x", "status": "almost"}])
        twice = dict(AREA, features=[{"name": "x", "status": "done"}] * 2)
        for bad in ({"areas": [no_label]}, {"areas": [bad_status]}, {"areas": [twice]},
                    {"areas": [AREA, AREA]}, {"areas": []}, []):
            with self.assertRaises(features.MapError):
                features.load(write(bad))

    def test_malformed_json_is_refused(self):
        path = write({})
        path.write_text("{", encoding="utf-8")
        with self.assertRaises(features.MapError):
            features.load(path)


class TallyTest(unittest.TestCase):
    def tally(self, issues, pulls=()):
        return features.tally([AREA], issues, list(pulls))[0]

    def test_issues_and_features_without_an_issue_are_counted(self):
        area = self.tally([
            issue(268, "closed", reason="completed"),
            issue(300),
            issue(301),
            issue(302, "closed", reason="not_planned"),
            issue(303, labels=("area: recording", "bug")),
            issue(304, labels=("area: recording", "epic")),
            issue(305, labels=("area: mixing",)),
        ], pulls=[{"body": "This closes #301."}])
        # Count-in (done), Capture (planned), Tempo (partial), and #268 done; #300 planned; #301 in progress.
        self.assertEqual(features.counts(area), {"done": 2, "working": 2, "planned": 2})
        self.assertEqual([b["issue"] for b in area["bugs"]], [303])

    def test_a_feature_with_an_issue_ref_is_counted_only_through_the_issue(self):
        area = self.tally([])
        self.assertNotIn("Audio recording", [i["title"] for i in area["items"]])


class WriteTest(unittest.TestCase):
    def setUp(self):
        self.tallied = features.tally([AREA], [issue(268, "closed", reason="completed", title="Audio"),
                                               issue(303, labels=("area: recording", "bug"), title="Stuck")], [])

    def test_markdown_lists_counts_items_and_bugs(self):
        text = features.markdown(self.tallied)
        self.assertIn("| [Recording](#recording) | 2 | 1 | 1 | 1 |", text)
        self.assertIn("- [#268](https://github.com/suhina-daw/suhina-daw/issues/268) Audio", text)
        self.assertIn("**Open bugs**", text)

    def test_svg_is_well_formed_and_shows_the_breakdown(self):
        for theme in features.THEMES:
            text = features.svg(self.tallied, theme)
            self.assertTrue(ET.fromstring(text).tag.endswith("svg"))
            self.assertIn("All areas", text)
            self.assertIn("2 done · 1 in progress · 1 planned · 1 bug", text)

    def test_breakdown_leaves_out_zeros(self):
        self.assertEqual(features.breakdown({"done": 3, "working": 0, "planned": 2}, 0), "3 done · 2 planned")


if __name__ == "__main__":
    unittest.main()

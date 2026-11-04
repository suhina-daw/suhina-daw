# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Tests for scripts/status_figure.py: python -m unittest discover -s scripts -p 'test_*.py'

They need no access to the private feature count: the counts here are made up.
"""

import re
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import status_figure as sf  # noqa: E402


def status(**areas):
    return {"areas": {k: {"done": d, "started": s, "todo": t} for k, (d, s, t) in areas.items()}}


EVERY = status(**{key: (i, i % 3, 2 * i + 1) for i, key in enumerate(sf.NAMES)})


class LoadAreas(unittest.TestCase):
    def test_counts_and_bugs(self):
        areas = sf.load_areas(status(launcher=(8, 2, 4)), {"launcher": 1})
        self.assertEqual(areas, [{"key": "launcher", "name": "Launcher", "done": 8, "started": 2,
                                  "todo": 4, "total": 14, "bugs": 1}])

    def test_leaves_out_an_area_without_features(self):
        areas = sf.load_areas(status(launcher=(1, 0, 0), agents=(0, 0, 0)), {})
        self.assertEqual([a["key"] for a in areas], ["launcher"])

    def test_rejects_bad_input(self):
        for bad, bugs in ((status(nowhere=(1, 0, 0)), {}), ({"areas": {"launcher": {"done": 1}}}, {}),
                          (status(launcher=(-1, 0, 0)), {}), (status(launcher=(1, 0, 0)), {"launcher": "x"}),
                          ({}, {}), (status(launcher=(0, 0, 0)), {})):
            with self.subTest(bad=bad, bugs=bugs), self.assertRaises(ValueError):
                sf.load_areas(bad, bugs)


class Layout(unittest.TestCase):
    def test_boxes_fill_the_figure_without_overlapping(self):
        boxes = sf.layout(sf.load_areas(EVERY, {}))
        area = sum(b["w"] * b["h"] for b in boxes)
        self.assertAlmostEqual(area, sf.WIDTH * (sf.HEIGHT - sf.TOP), places=3)
        for a in boxes:
            self.assertGreaterEqual(a["x"], -1e-6)
            self.assertLessEqual(a["x"] + a["w"], sf.WIDTH + 1e-6)
            for b in boxes:
                if a is b:
                    continue
                overlap_w = min(a["x"] + a["w"], b["x"] + b["w"]) - max(a["x"], b["x"])
                overlap_h = min(a["y"] + a["h"], b["y"] + b["h"]) - max(a["y"], b["y"])
                self.assertFalse(overlap_w > 1e-6 and overlap_h > 1e-6, (a["key"], b["key"]))

    def test_every_drawn_label_fits_its_box(self):
        areas = sf.load_areas(EVERY, {key: 12 for key in sf.NAMES})
        for r in sf.layout(areas):
            inner_left = r["x"] + sf.GAP + sf.PAD - 0.05
            inner_right = r["x"] + r["w"] - sf.GAP - sf.PAD + 0.05
            for line in sf.box(r, sf.THEMES["light"], 0):
                m = re.match(r'<text x="([\d.]+)" y="([\d.]+)" font-size="(\d+)"( font-weight="600")?'
                             r'( text-anchor="(\w+)")? fill="[^"]+">(.*)</text>', line)
                if not m:
                    continue
                x, y, size = float(m.group(1)), float(m.group(2)), int(m.group(3))
                width = sf.measure(m.group(7), size, bool(m.group(4)))
                left = {"end": x - width, "middle": x - width / 2}.get(m.group(6), x)
                with self.subTest(area=r["key"], text=m.group(7)):
                    self.assertGreaterEqual(left, inner_left - 22)  # a badge's text has its own pad
                    self.assertLessEqual(left + width, inner_right + 0.1)
                    self.assertLessEqual(y, r["y"] + r["h"])

    def test_a_long_name_is_wrapped_or_shortened_to_fit(self):
        lines, size = sf.name_lines("Instruments and MIDI effects", 60)
        self.assertTrue(all(sf.measure(s, size, True) <= 60 for s in lines))
        lines, size = sf.name_lines("Supercalifragilistic", 30)
        self.assertTrue(lines[0].endswith("…"))


class Outputs(unittest.TestCase):
    def test_svg_names_every_area_and_both_themes_differ(self):
        areas = sf.load_areas(EVERY, {})
        light, dark = sf.svg(areas, "light"), sf.svg(areas, "dark")
        self.assertNotEqual(light, dark)
        for area in areas:
            self.assertIn(f"<title>{sf.esc(area['name'])}:", light)

    def test_readme_block_is_replaced_between_the_markers(self):
        readme = "intro\n<!-- status:start -->\nold\n<!-- status:end -->\nrest\n"
        new = sf.replace_block(readme, sf.block(sf.load_areas(status(launcher=(1, 1, 2)), {})))
        self.assertTrue(new.startswith("intro\n" + sf.START))
        self.assertTrue(new.endswith(sf.END + "\nrest\n"))
        self.assertNotIn("old", new)
        self.assertIsNone(sf.replace_block("no markers", "x"))

    def test_the_readme_has_the_markers(self):
        self.assertIsNotNone(sf.replace_block(sf.README.read_text(encoding="utf-8"), "x"))


if __name__ == "__main__":
    unittest.main()

# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Tests for works_now.py: python3 -m unittest discover -s scripts -p 'test_*.py'"""

import tempfile
import unittest
from pathlib import Path

import release_notes as rn
import works_now as wn

ROADMAP = """# Roadmap

## Later

- not a feature yet

## What works now

- a loose feature

### M1 Reading
- tempo

### M2 Playing
- `play` plays a set

## After
"""


class WorksNowTests(unittest.TestCase):
    def setUp(self) -> None:
        self.dir = tempfile.TemporaryDirectory()
        self.old = wn.FRAGMENTS, wn.ROOT
        wn.ROOT = Path(self.dir.name)
        wn.FRAGMENTS = wn.ROOT / "docs" / "works-now"
        (wn.FRAGMENTS).mkdir(parents=True)
        (wn.FRAGMENTS / "README.md").write_text("# not a fragment\n")

    def tearDown(self) -> None:
        wn.FRAGMENTS, wn.ROOT = self.old
        self.dir.cleanup()

    def add(self, rel: str, text: str) -> Path:
        path = wn.FRAGMENTS / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        return path

    def test_sections_are_top_and_each_milestone(self) -> None:
        self.assertEqual(list(wn.sections(ROADMAP)), ["top", "m1", "m2"])

    def test_fold_appends_each_file_to_its_section_in_name_order(self) -> None:
        self.add("m1/b.md", "- MIDI clips,\n  with loops\n")
        self.add("m1/a.md", "- audio clips\n")
        self.add("top/x.md", "- a new loose feature\n")
        self.add("m2/y.md", "- `stop` stops it\n")
        folded = wn.fold(ROADMAP, wn.fragments())
        self.assertIn("- a loose feature\n\n- a new loose feature\n\n### M1 Reading", folded)
        self.assertIn("- tempo\n\n- audio clips\n- MIDI clips,\n  with loops\n\n### M2", folded)
        self.assertIn("- `play` plays a set\n\n- `stop` stops it\n\n## After", folded)
        # The release notes read the folded bullets under their milestones.
        self.assertEqual(
            rn.new_bullets(ROADMAP, folded),
            [("", "a new loose feature"), ("M1 Reading", "audio clips"),
             ("M1 Reading", "MIDI clips, with loops"), ("M2 Playing", "`stop` stops it")],
        )

    def test_fold_with_no_files_changes_nothing(self) -> None:
        self.assertEqual(wn.fold(ROADMAP, []), ROADMAP)

    def test_check_names_each_wrong_file(self) -> None:
        self.add("m9/a.md", "- in no section\n")
        self.add("loose.md", "- not in a section folder\n")
        self.add("m1/heading.md", "## a heading\n")
        self.add("m1/blank.md", "- one\n\n- two\n")
        self.add("m1/fine.md", "- fine,\n  continued\n")
        found = "\n".join(wn.problems(wn.fragments(), wn.sections(ROADMAP)))
        self.assertIn("m9/a.md: no section 'm9'", found)
        self.assertIn("loose.md: put it at", found)
        self.assertIn("heading.md: starts with a bullet", found)
        self.assertIn("blank.md:2: only bullets", found)
        self.assertNotIn("fine.md", found)
        self.assertNotIn("README", found)


if __name__ == "__main__":
    unittest.main()

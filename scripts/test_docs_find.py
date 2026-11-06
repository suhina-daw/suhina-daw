# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Tests for `scripts/docs.py covers` and `find` (#768 › 2): asking the index instead of reading it.
python3 -m unittest discover -s scripts -p 'test_*.py'
"""

import contextlib
import io
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import docs  # noqa: E402


def doc(rel: str, title: str, read_when: str = "", description: str = "", covers=()) -> docs.Doc:
    d = docs.Doc(path=docs.ROOT / rel, title=title, has_front_matter=True)
    d.fields = {"read_when": read_when, "description": description}
    d.covers = list(covers)
    return d


LIBRARY = [
    doc("docs/spec/clip-launching.md", "Clip launching", "launch quantisation, scenes", "How clips launch",
        covers=["crates/engine/src/launcher.rs"]),
    doc("docs/spec/export.md", "Export", "render to a file, wav", "The mix to a file", covers=["crates/session/src/export.rs"]),
    doc("docs/spec/backups.md", "Backups", "autosave", "Copies of a project", covers=["crates/session/src"]),
]


def run(command, *args):
    out = io.StringIO()
    original = docs.documents
    docs.documents = lambda: LIBRARY
    try:
        with contextlib.redirect_stdout(out):
            command(list(args))
    finally:
        docs.documents = original
    return out.getvalue().splitlines()


class Find(unittest.TestCase):
    def test_a_title_word_counts_most_then_read_when_then_description(self):
        wanted = docs.words("launch")
        self.assertEqual(docs.score(LIBRARY[0], wanted), 3)
        self.assertEqual(docs.score(LIBRARY[0], docs.words("clip")), 3)
        self.assertEqual(docs.score(LIBRARY[1], docs.words("mix")), 1)

    def test_documents_come_best_first_and_unmatched_ones_not_at_all(self):
        lines = run(docs.find, "clip", "scenes")
        self.assertEqual(len(lines), 1)
        self.assertTrue(lines[0].startswith("docs/spec/clip-launching.md (5)"))
        self.assertEqual(run(docs.find, "nothing"), ["no document matches"])


class Covers(unittest.TestCase):
    def test_a_path_lists_every_document_that_covers_it(self):
        lines = run(docs.covers, "crates/session/src/export.rs")
        self.assertEqual([line.split(": ")[1].split(" ")[0] for line in lines], ["docs/spec/export.md", "docs/spec/backups.md"])

    def test_a_path_nothing_covers_says_so(self):
        self.assertEqual(run(docs.covers, "crates/ipc/src/lib.rs"), ["crates/ipc/src/lib.rs: no document covers it"])


if __name__ == "__main__":
    unittest.main()

# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Tests for release_notes.py: python3 -m unittest discover -s scripts -p 'test_*.py'"""

import unittest

import release_notes as rn

REPO = "o/r"

ROADMAP_BEFORE = """# Roadmap

## Later

- not a feature yet

## What works now

### M1 Reading
- tempo
- MIDI clips with notes,
  loop, deactivated clips

### M2 Playing
- `play` plays a set
"""

ROADMAP_AFTER = """# Roadmap

## Later

- not a feature yet, changed

## What works now

### M1 Reading
- tempo
- MIDI clips with notes,
  loop, deactivated clips, fades

### M2 Playing
- `play` plays a set
- `render` writes a WAV ([spec](docs/spec/render.md), [site](https://example.org))
"""


class Internal(unittest.TestCase):
    def test_docs_ci_scripts_and_tests_are_internal(self):
        for paths in (
            ["docs/releasing.md", "README.md"],
            [".github/workflows/release.yml"],
            ["scripts/docs.py", "LICENSE"],
            ["crates/plugin-host/tests/x11.rs"],
            [],
        ):
            self.assertTrue(rn.is_internal(paths), paths)

    def test_code_is_not_internal(self):
        for paths in (
            ["crates/engine/src/lib.rs", "docs/architecture.md"],
            ["Cargo.toml"],
            ["presets/fm-synth/bass.json"],
        ):
            self.assertFalse(rn.is_internal(paths), paths)


class Links(unittest.TestCase):
    def test_pull_requests_become_links(self):
        self.assertEqual(
            rn.link_pulls("Engine: fades (#215)", REPO),
            "Engine: fades ([#215](https://github.com/o/r/pull/215))",
        )

    def test_discord_links_have_no_preview(self):
        self.assertEqual(
            rn.link_pulls("A (#1)", REPO, angle=True), "A ([#1](<https://github.com/o/r/pull/1>))"
        )

    def test_relative_links_become_absolute(self):
        text = "[a](docs/a.md), [b](https://x.org), [c](#here)"
        self.assertEqual(
            rn.absolute_links(text, REPO, "abc"),
            "[a](https://github.com/o/r/blob/abc/docs/a.md), [b](https://x.org), [c](#here)",
        )


class Roadmap(unittest.TestCase):
    def test_bullets_only_from_what_works_now_with_continuations(self):
        self.assertEqual(
            rn.roadmap_bullets(ROADMAP_BEFORE),
            [
                ("M1 Reading", "tempo"),
                ("M1 Reading", "MIDI clips with notes, loop, deactivated clips"),
                ("M2 Playing", "`play` plays a set"),
            ],
        )

    def test_new_and_changed_bullets(self):
        self.assertEqual(
            rn.new_bullets(ROADMAP_BEFORE, ROADMAP_AFTER),
            [
                ("M1 Reading", "MIDI clips with notes, loop, deactivated clips, fades"),
                (
                    "M2 Playing",
                    "`render` writes a WAV ([spec](docs/spec/render.md), [site](https://example.org))",
                ),
            ],
        )

    def test_everything_is_new_without_a_base(self):
        self.assertEqual(len(rn.new_bullets("", ROADMAP_BEFORE)), 3)


class Discord(unittest.TestCase):
    def test_first_nightly(self):
        post = rn.discord_post([], 0, None, REPO, "0.0.1-nightly.1+a")
        self.assertIn("The first nightly", post)

    def test_only_internal_changes(self):
        post = rn.discord_post([], 3, "base", REPO, "v")
        self.assertIn("3 internal changes", post)
        self.assertNotIn("New since", post)

    def test_long_lists_are_cut_with_a_count(self):
        titles = [f"Area: change number {i} " + "x" * 150 + f" (#{i})" for i in range(40)]
        post = rn.discord_post(titles, 2, "base", REPO, "v")
        self.assertLessEqual(len(post), rn.DISCORD_LIMIT)
        shown = post.count("\n- ")
        self.assertLessEqual(shown, rn.DISCORD_ITEMS)
        self.assertIn(f"…and {40 - shown} more", post)
        self.assertTrue(post.endswith("2 internal changes (documentation, CI, scripts, tests) not listed."))

    def test_links_the_rolling_nightly_by_default(self):
        post = rn.discord_post([], 0, "base", REPO, "v")
        self.assertIn("<https://github.com/o/r/releases/tag/nightly>", post)

    def test_links_the_nights_own_release(self):
        post = rn.discord_post([], 0, "base", REPO, "v", "nightly-20261013")
        self.assertIn("<https://github.com/o/r/releases/tag/nightly-20261013>", post)

    def test_short_lists_are_whole(self):
        post = rn.discord_post(["A (#1)", "B (#2)"], 0, "base", REPO, "v")
        self.assertEqual(post.count("\n- "), 2)
        self.assertNotIn("more on the release page", post)


class Titles(unittest.TestCase):
    def test_squash_merge_keeps_its_subject(self):
        self.assertEqual(rn.title_of("Clip fades: shapes (#12)", ""), "Clip fades: shapes (#12)")

    def test_merge_commit_takes_the_pull_request_title(self):
        title = rn.title_of(
            "Merge pull request #1037 from o/feature/slice",
            "\nSampler: Slice workflow and panel\n\nMore text.\n",
        )
        self.assertEqual(title, "Sampler: Slice workflow and panel (#1037)")

    def test_merge_commit_without_a_body_keeps_its_subject(self):
        subject = "Merge pull request #5 from o/fix/x"
        self.assertEqual(rn.title_of(subject, "  \n"), subject)

    def test_other_merges_keep_their_subject(self):
        subject = "Merge branch 'main' into feature/x"
        self.assertEqual(rn.title_of(subject, "Body"), subject)


class Nightly(unittest.TestCase):
    def test_lists_changes_and_counts_internal_ones(self):
        notes = rn.nightly_notes(["A (#1)"], 1, "base", REPO)
        self.assertIn("- A ([#1](https://github.com/o/r/pull/1))", notes)
        self.assertIn("1 internal change ", notes)


if __name__ == "__main__":
    unittest.main()

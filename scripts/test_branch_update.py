# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Tests for branch_update.py: python3 -m unittest discover -s scripts -p 'test_*.py'"""

import os
import subprocess
import sys
import tempfile
import unittest

import branch_update as bu

LINES = [f"line {n}\n" for n in range(1, 41)]


class Repo:
    """A throwaway repository with main and a pull request branch `pr`."""

    def __init__(self, path):
        self.path = path
        self.run("init", "-q", "-b", "main")
        self.run("config", "user.name", "Test")
        self.run("config", "user.email", "test@example.com")
        self.run("config", "commit.gpgsign", "false")
        self.write("shared.txt", "".join(LINES))
        self.write("main.txt", "main\n")
        self.commit("start")
        self.run("checkout", "-q", "-b", "pr")
        # The pull request's own change: line 5 of shared.txt and a file of its own.
        self.edit("shared.txt", 4, "line 5 from the pull request\n")
        self.write("pr.txt", "pull request\n")
        self.commit("the pull request")

    def run(self, *args):
        return subprocess.run(
            ["git", *args], cwd=self.path, check=True, capture_output=True, text=True
        ).stdout.strip()

    def write(self, name, text):
        with open(os.path.join(self.path, name), "w", encoding="utf-8") as f:
            f.write(text)

    def edit(self, name, index, text):
        with open(os.path.join(self.path, name), encoding="utf-8") as f:
            lines = f.readlines()
        lines[index] = text
        self.write(name, "".join(lines))

    def commit(self, message):
        self.run("add", "-A")
        self.run("commit", "-q", "-m", message)
        return self.head()

    def head(self):
        return self.run("rev-parse", "HEAD")

    def on_main(self, change):
        self.run("checkout", "-q", "main")
        change()
        self.commit("main moves on")
        self.run("checkout", "-q", "pr")

    def update_branch(self):
        """What "Update branch" does: merge main into the pull request."""
        before = self.head()
        self.run("merge", "-q", "--no-edit", "main")
        return before, self.head()

    def check(self, before, head, main="main"):
        return bu.only_merges_main(before, head, main, cwd=self.path)


class OnlyMergesMain(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = Repo(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_update_branch_with_other_files(self):
        self.repo.on_main(lambda: self.repo.write("main.txt", "main, newer\n"))
        before, head = self.repo.update_branch()
        only, why = self.repo.check(before, head)
        self.assertTrue(only, why)

    def test_main_changes_the_same_file_far_away(self):
        self.repo.on_main(lambda: self.repo.edit("shared.txt", 35, "line 36 from main\n"))
        before, head = self.repo.update_branch()
        only, why = self.repo.check(before, head)
        self.assertTrue(only, why)

    def test_two_updates_in_a_row(self):
        self.repo.on_main(lambda: self.repo.write("main.txt", "one\n"))
        _, first = self.repo.update_branch()
        self.repo.on_main(lambda: self.repo.write("main.txt", "two\n"))
        before, head = self.repo.update_branch()
        self.assertEqual(before, first)
        only, why = self.repo.check(before, head)
        self.assertTrue(only, why)

    def test_new_commit_is_built(self):
        before = self.repo.head()
        self.repo.write("pr.txt", "more\n")
        head = self.repo.commit("more work")
        only, why = self.repo.check(before, head)
        self.assertFalse(only)
        self.assertIn("not a merge commit", why)

    def test_merge_of_another_branch_is_built(self):
        before = self.repo.head()
        self.repo.run("checkout", "-q", "-b", "other", "main")
        self.repo.write("other.txt", "other\n")
        self.repo.commit("other work")
        self.repo.run("checkout", "-q", "pr")
        self.repo.run("merge", "-q", "--no-edit", "other")
        only, why = self.repo.check(before, self.repo.head())
        self.assertFalse(only)
        self.assertIn("not on main", why)

    def test_merge_on_top_of_a_rewritten_head_is_built(self):
        self.repo.write("pr.txt", "rewritten\n")
        self.repo.run("add", "-A")
        self.repo.run("commit", "-q", "--amend", "--no-edit")
        rewritten = self.repo.head()
        self.repo.run("reset", "-q", "--hard", "HEAD@{1}")
        before = self.repo.head()
        self.repo.run("reset", "-q", "--hard", rewritten)
        self.repo.on_main(lambda: self.repo.write("main.txt", "newer\n"))
        self.repo.run("merge", "-q", "--no-edit", "main")
        only, why = self.repo.check(before, self.repo.head())
        self.assertFalse(only)
        self.assertIn("does not continue", why)

    def test_change_inside_the_merge_is_built(self):
        self.repo.on_main(lambda: self.repo.write("main.txt", "newer\n"))
        before = self.repo.head()
        self.repo.run("merge", "-q", "--no-commit", "main")
        self.repo.write("pr.txt", "slipped into the merge\n")
        self.repo.run("add", "-A")
        self.repo.run("commit", "-q", "--no-edit")
        only, why = self.repo.check(before, self.repo.head())
        self.assertFalse(only)
        self.assertIn("own diff", why)

    def test_whitespace_change_inside_merge_is_built(self):
        self.repo.on_main(lambda: self.repo.write("main.txt", "newer\n"))
        before = self.repo.head()
        self.repo.run("merge", "-q", "--no-commit", "main")
        # Whitespace can be significant in source strings and indentation. git patch-id
        # ignores it, so it cannot prove that the existing binary still represents the PR.
        self.repo.write("pr.txt", "pullrequest\n")
        head = self.repo.commit("changed inside merge")
        self.assertFalse(self.repo.check(before, head)[0])

    def test_resolved_conflict_is_built(self):
        self.repo.on_main(lambda: self.repo.edit("shared.txt", 4, "line 5 from main\n"))
        before = self.repo.head()
        merge = subprocess.run(
            ["git", "merge", "-q", "--no-edit", "main"], cwd=self.repo.path, capture_output=True
        )
        self.assertNotEqual(merge.returncode, 0)
        # The conflict markers took more lines; write the whole file again.
        lines = LINES.copy()
        lines[4] = "line 5, both\n"
        self.repo.write("shared.txt", "".join(lines))
        self.repo.run("add", "-A")
        self.repo.run("commit", "-q", "--no-edit")
        only, why = self.repo.check(before, self.repo.head())
        self.assertFalse(only)
        self.assertIn("own diff", why)

    def test_main_changes_a_nearby_line_is_built(self):
        # Line 8 is in the three lines of context around the pull request's line 5, so the
        # patch differs although the merge is clean.
        self.repo.on_main(lambda: self.repo.edit("shared.txt", 7, "line 8 from main\n"))
        before, head = self.repo.update_branch()
        only, _ = self.repo.check(before, head)
        self.assertFalse(only)

    def test_no_previous_head(self):
        head = self.repo.head()
        self.assertFalse(self.repo.check("", head)[0])
        self.assertFalse(self.repo.check("0" * 40, head)[0])

    def test_unknown_commit_is_an_error(self):
        with self.assertRaises(bu.GitError):
            self.repo.check("1" * 40, self.repo.head())


class CommandLine(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = Repo(self.tmp.name)
        self.script = os.path.join(os.path.dirname(os.path.abspath(__file__)), "branch_update.py")

    def tearDown(self):
        self.tmp.cleanup()

    def code(self, *args):
        return subprocess.run(
            [sys.executable, self.script, *args], cwd=self.repo.path, capture_output=True
        ).returncode

    def test_exit_codes(self):
        self.repo.on_main(lambda: self.repo.write("main.txt", "newer\n"))
        before, head = self.repo.update_branch()
        self.assertEqual(self.code(before, head, "main"), 0)
        self.assertEqual(self.code(before + "^", head, "main"), 1)
        self.assertEqual(self.code("1" * 40, head, "main"), 2)
        self.assertEqual(self.code(before, head), 2)


if __name__ == "__main__":
    unittest.main()

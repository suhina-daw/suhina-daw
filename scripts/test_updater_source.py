# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Tests for scripts/updater_source.py: python3 -m unittest discover -s scripts -p 'test_*.py'"""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

import updater_source

ROOT = Path(__file__).resolve().parent.parent


def source(*extra):
    """A github.rs with every pinned line, and `extra` lines after them."""
    return "\n".join(f"    {pin}" for pin in (*updater_source.PINNED, *extra)) + "\n"


class Pins(unittest.TestCase):
    def test_the_pinned_lines_pass(self):
        self.assertEqual(updater_source.problems({updater_source.SOURCE: source()}), [])

    def test_each_pinned_line_changed_fails(self):
        for pin in updater_source.PINNED:
            changed = source().replace(pin, pin.replace('"', '"other-', 1))
            found = updater_source.problems({updater_source.SOURCE: changed})
            self.assertTrue(any(pin in problem for problem in found), pin)

    def test_a_deleted_source_fails(self):
        found = updater_source.problems({"crates/update/src/lib.rs": "fn main() {}\n"})
        self.assertIn("is gone", found[0])


class Urls(unittest.TestCase):
    def test_urls_outside_this_repository_fail(self):
        for url in [
            "https://github.com/someone-else/suhina-daw/releases/tag/nightly",
            "https://api.github.com/repos/someone-else/suhina-daw/releases/tags/nightly",
            "https://objects.githubusercontent.com/w.zip",
            "http://github.com/suhina-daw/suhina-daw/releases",
            "https://github.com/suhina-daw/suhina-daw-fork/releases",
            "HTTPS://example.com/w.zip",
            "ftp://example.com/w.zip",
            # Starts like this repository, ends elsewhere.
            "https://github.com/suhina-daw/suhina-daw/../../someone-else/repo/w.zip",
            "https://github.com/suhina-daw/suhina-daw@example.com/w.zip",
            # The start of a URL put together at run time.
            "https://",
            "https://{host}/w.zip",
        ]:
            self.assertFalse(updater_source.url_allowed(url), url)

    def test_this_repository_and_test_hosts_pass(self):
        for url in [
            "https://github.com/suhina-daw/suhina-daw",
            "https://github.com/suhina-daw/suhina-daw/pull/663",
            "https://github.com/{REPOSITORY}/pull/{pr}",
            "https://api.github.com/repos/{REPOSITORY}/actions/artifacts/",
            "https://api.github.com/repos/suhina-daw/suhina-daw/actions/artifacts?per_page=100",
            "https://example.invalid/w.zip",
            "https://api.example.invalid/1/zip",
        ]:
            self.assertTrue(updater_source.url_allowed(url), url)

    def test_a_url_in_any_watched_file_is_found_with_its_line(self):
        files = {
            updater_source.SOURCE: source(),
            "crates/update/src/net.rs": 'fn a() {}\nconst B: &str = "https://example.com/w.zip";\n',
        }
        found = updater_source.problems(files)
        self.assertEqual(len(found), 1)
        self.assertIn("crates/update/src/net.rs:2", found[0])
        self.assertIn("https://example.com/w.zip", found[0])


class Annotation(unittest.TestCase):
    def test_a_message_stays_one_line(self):
        line = updater_source.annotation("a%b\nc\r::error::d")
        self.assertEqual(line, "::error title=Updater source::a%25b%0Ac%0D::error::d")


class Commits(unittest.TestCase):
    """The check reads a commit through Git, whatever is checked out."""

    def setUp(self):
        directory = tempfile.TemporaryDirectory(prefix="suhina-updater-source-")
        self.addCleanup(directory.cleanup)
        self.cwd = directory.name
        self.env = dict(
            os.environ,
            GIT_AUTHOR_NAME="Contributor",
            GIT_AUTHOR_EMAIL="contributor@example.invalid",
            GIT_COMMITTER_NAME="Contributor",
            GIT_COMMITTER_EMAIL="contributor@example.invalid",
            GIT_CONFIG_NOSYSTEM="1",
            GIT_CONFIG_GLOBAL=os.devnull,
        )
        self.git("init", "--quiet")

    def git(self, *args, input=None):
        return subprocess.run(
            ["git", *args], cwd=self.cwd, env=self.env, input=input,
            capture_output=True, check=True,
        ).stdout.decode().strip()

    def commit(self, text, mode="100644"):
        """A commit holding `text` as the source file, with nothing in the working tree."""
        blob = self.git("hash-object", "-w", "--stdin", input=text.encode())
        self.git("update-index", "--add", "--cacheinfo", f"{mode},{blob},{updater_source.SOURCE}")
        tree = self.git("write-tree")
        return self.git("commit-tree", tree, "-m", "Commit")

    def test_another_commit_is_read_and_not_the_working_tree(self):
        good = self.commit(source())
        changed = self.commit(source().replace("suhina-daw/suhina-daw", "someone-else/suhina-daw"))
        files, found = updater_source.read_commit(good, cwd=self.cwd)
        self.assertEqual(found, [])
        self.assertEqual(updater_source.problems(files), [])
        files, found = updater_source.read_commit(changed, cwd=self.cwd)
        self.assertEqual(found, [])
        self.assertTrue(updater_source.problems(files))

    def test_a_symbolic_link_fails(self):
        link = self.commit(source(), mode="120000")
        files, found = updater_source.read_commit(link, cwd=self.cwd)
        self.assertEqual(files, {})
        self.assertIn(updater_source.SOURCE, found[0])

    def test_a_commit_that_cannot_be_read_fails(self):
        with self.assertRaises(subprocess.CalledProcessError):
            updater_source.read_commit("no-such-commit", cwd=self.cwd)


class Wiring(unittest.TestCase):
    """The parts that hold the check in place, so that dropping one fails here too."""

    def test_the_project_itself_passes(self):
        files = {
            path.relative_to(ROOT).as_posix(): path.read_text(encoding="utf-8", errors="replace")
            for watched in updater_source.WATCHED
            for path in ([ROOT / watched] if (ROOT / watched).is_file() else (ROOT / watched).rglob("*"))
            if path.is_file()
        }
        self.assertEqual(updater_source.problems(files), [])

    def test_the_required_job_runs_the_check(self):
        ci = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        docs_job = ci.split("\n  docs:\n", 1)[1].split("\n  fmt:\n", 1)[0]
        self.assertIn("python3 scripts/updater_source.py\n", docs_job)
        self.assertIn("needs: [changes, docs,", ci)

    def test_the_workflow_runs_from_main_and_never_checks_out_the_pull_request(self):
        workflow = (ROOT / ".github/workflows/updater-source.yml").read_text(encoding="utf-8")
        self.assertIn("pull_request_target:", workflow)
        self.assertIn("ref: ${{ github.event.repository.default_branch }}", workflow)
        self.assertIn("persist-credentials: false", workflow)
        self.assertIn("--ref FETCH_HEAD", workflow)
        self.assertNotIn("secrets.", workflow)

    def test_a_code_owner_reviews_every_watched_path_and_the_check(self):
        owners = (ROOT / ".github/CODEOWNERS").read_text(encoding="utf-8")
        entries = dict(line.split(None, 1) for line in owners.splitlines() if " @" in line and not line.startswith("#"))
        for watched in updater_source.WATCHED:
            wanted = f"/{watched}/" if (ROOT / watched).is_dir() else f"/{watched}"
            self.assertIn(wanted, entries)
        # The workflow and this script are under these.
        self.assertIn("/.github/", entries)
        self.assertIn("/scripts/", entries)


if __name__ == "__main__":
    unittest.main()

# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Run the PR workflow scripts against local Git history and mocked GitHub answers.

Run with `python -m unittest discover -s scripts -p 'test_pr_workflows.py'`.
Requires Git, Bash (Git for Windows on Windows), and Node.js; no network is used.
"""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from claim_bot_test import job_script

ROOT = Path(__file__).resolve().parent.parent
WORKFLOWS = ROOT / ".github" / "workflows"


class DcoTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="suhina-dco-")
        self.addCleanup(self.directory.cleanup)
        self.cwd = Path(self.directory.name)
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
        self.tree = self.git("mktree", input="")
        self.base = self.commit("Base")
        self.git("update-ref", "refs/remotes/origin/main", self.base)
        workflow = (WORKFLOWS / "ci.yml").read_text(encoding="utf-8")
        dco_job = workflow.split("\n  dco:\n", 1)[1].split("\n  ci:\n", 1)[0]
        # Reuse the existing literal-block reader for the shell step as well.
        self.script = job_script(dco_job.replace("run: |", "script: |"))
        self.bash = shutil.which("bash")
        if os.name == "nt":
            git_bash = Path(os.environ.get("ProgramFiles", "C:/Program Files")) / "Git/bin/bash.exe"
            if git_bash.is_file():
                self.bash = str(git_bash)
        self.assertIsNotNone(self.bash, "Bash is required for the DCO workflow test")

    def git(self, *args, input=None):
        return subprocess.run(
            ["git", *args], cwd=self.cwd, env=self.env, input=input,
            capture_output=True, text=True, check=True,
        ).stdout.strip()

    def commit(self, message, *parents):
        args = ["commit-tree", self.tree, "-m", message]
        for parent in parents:
            args.extend(["-p", parent])
        return self.git(*args)

    def check_dco(self, head):
        merge = self.commit("Synthetic PR merge", self.base, head)
        # actions/checkout creates this remote ref in pull_request runs.
        self.git("update-ref", "refs/remotes/pull/999/merge", merge)
        return subprocess.run(
            [self.bash, "--noprofile", "--norc", "-e", "-o", "pipefail", "-c", self.script],
            cwd=self.cwd, env=dict(self.env, BASE=self.base, HEAD=head),
            capture_output=True, text=True,
        )

    def test_unsigned_fork_commit_is_rejected_despite_pull_merge_ref(self):
        head = self.commit("Unsigned fork change", self.base)
        result = self.check_dco(head)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn(head, result.stdout)
        self.assertIn("has no 'Signed-off-by:", result.stdout)

    def test_signed_fork_commit_on_unsigned_upstream_branch_passes(self):
        upstream = self.commit("Unsigned upstream change", self.base)
        self.git("update-ref", "refs/remotes/origin/feature/parent", upstream)
        head = self.commit(
            "Fork change\n\nSigned-off-by: Contributor <contributor@example.invalid>", upstream,
        )
        result = self.check_dco(head)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_unsigned_fork_commit_on_upstream_branch_is_still_rejected(self):
        upstream = self.commit("Unsigned upstream change", self.base)
        self.git("update-ref", "refs/remotes/origin/feature/parent", upstream)
        head = self.commit("Unsigned fork change", upstream)
        result = self.check_dco(head)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn(head, result.stdout)
        self.assertNotIn(upstream, result.stdout)


OVERLAP_HARNESS = r'''
const fs = require("node:fs");
const input = JSON.parse(fs.readFileSync(0, "utf8"));
const AsyncFunction = Object.getPrototypeOf(async function(){}).constructor;
const run = new AsyncFunction("github", "context", "core", input.script);
const state = { comments: [], warnings: [], fetched: [] };
const repo = "suhina-daw/suhina-daw";
const context = {
  repo: { owner: "suhina-daw", repo: "suhina-daw" },
  payload: { pull_request: {
    number: 999, body: "", base: { ref: "main" },
    head: { ref: "feature/current", repo: { full_name: repo } },
  } },
};
const github = {
  rest: {
    pulls: { listFiles: "files" },
    issues: {
      listForRepo: "issues", listComments: "comments",
      updateComment: async args => state.comments.push(args.body),
      createComment: async args => state.comments.push(args.body),
    },
  },
  graphql: async (query, variables) => {
    if (input.fail_page !== null && Number(variables.cursor || 0) === input.fail_page) throw new Error("fixture page failure");
    return { repository: {
    pullRequest: { closingIssuesReferences: { nodes: [] } },
    pullRequests: {
      pageInfo: {
        hasNextPage: Number(variables.cursor || 0) + 100 < input.pulls.length,
        endCursor: String(Number(variables.cursor || 0) + 100),
      },
      nodes: input.pulls.slice(Number(variables.cursor || 0), Number(variables.cursor || 0) + 100).map(p => ({
      number: p.number, title: "Work", headRefName: `feature/${p.number}`,
      baseRefName: "main", headRepository: { nameWithOwner: repo },
      files: { totalCount: p.totalCount || 1, nodes: [{
        path: p.path,
        ...(query.includes("changeType") ? { changeType: p.changeType || "MODIFIED" } : {}),
      }] },
    })) },
  } };
  },
  paginate: async (method, args) => {
    if (method === "files") {
      if (args.pull_number === 999) return input.mine || [{ filename: "a.rs" }];
      state.fetched.push(args.pull_number);
      const p = input.pulls.find(p => p.number === args.pull_number);
      if (p.fail) throw new Error("fixture API failure");
      return p.files;
    }
    if (method === "issues") return [];
    if (input.no_previous) return [];
    return [{ id: 1, user: { type: "Bot" }, body: "<!-- overlap -->\nEarlier notice" }];
  },
};
run(github, context, { warning: message => state.warnings.push(message) })
  .then(() => console.log(JSON.stringify(state)))
  .catch(error => { console.error(error); process.exitCode = 1; });
'''


class OverlapTests(unittest.TestCase):
    def run_notice(self, pulls, mine=None, fail_page=None, no_previous=False):
        script = job_script((WORKFLOWS / "overlap.yml").read_text(encoding="utf-8"))
        result = subprocess.run(
            ["node", "-e", OVERLAP_HARNESS],
            input=json.dumps(dict(script=script, pulls=pulls, mine=mine, fail_page=fail_page, no_previous=no_previous)),
            capture_output=True, text=True, check=True,
        )
        return json.loads(result.stdout)

    def test_later_page_failure_preserves_existing_notice(self):
        pulls = [dict(number=n, path="a.rs") for n in range(1, 102)]
        state = self.run_notice(pulls, fail_page=100)
        self.assertEqual(state["comments"], [])
        self.assertTrue(any("finish reading" in w for w in state["warnings"]))

    def test_later_page_failure_still_reports_known_overlap_as_incomplete(self):
        pulls = [dict(number=n, path="unrelated.rs") for n in range(1, 102)]
        pulls[0]["path"] = "a.rs"
        state = self.run_notice(pulls, fail_page=100, no_previous=True)
        self.assertIn("#1 Work", state["comments"][0])
        self.assertIn("scan is incomplete", state["comments"][0])
        self.assertNotIn("No other open", state["comments"][0])

    def test_other_pr_rename_compares_old_name(self):
        state = self.run_notice([dict(
            number=1, path="b.rs", changeType="RENAMED",
            files=[dict(filename="b.rs", previous_filename="a.rs")],
        )])
        self.assertIn("#1 Work", state["comments"][0])
        self.assertEqual(state["fetched"], [1])

    def test_overlap_on_later_open_pr_pages_is_reported(self):
        pulls = [dict(number=n, path="unrelated.rs") for n in range(1, 202)]
        pulls[100]["path"] = "a.rs"
        pulls[200]["path"] = "a.rs"
        state = self.run_notice(pulls)
        self.assertIn("#101 Work", state["comments"][0])
        self.assertIn("#201 Work", state["comments"][0])
        self.assertEqual(state["warnings"], [])

    def test_failed_pagination_keeps_known_files_and_checks_later_prs(self):
        state = self.run_notice([
            dict(number=1, path="a.rs", totalCount=101, fail=True),
            dict(number=2, path="a.rs"),
        ])
        self.assertIn("#1 Work", state["comments"][0])
        self.assertIn("#2 Work", state["comments"][0])
        self.assertTrue(any("#1" in warning for warning in state["warnings"]))

    def test_paginated_files_include_overlap_beyond_first_page(self):
        state = self.run_notice([dict(
            number=1, path="unrelated.rs", totalCount=101,
            files=[dict(filename="unrelated.rs"), dict(filename="a.rs")],
        )])
        self.assertIn("#1 Work", state["comments"][0])

    def test_ordinary_overlap_does_not_need_another_rest_fetch(self):
        state = self.run_notice([dict(number=1, path="a.rs")])
        self.assertIn("#1 Work", state["comments"][0])
        self.assertEqual(state["fetched"], [])

    def test_current_pr_rename_still_compares_old_name(self):
        state = self.run_notice(
            [dict(number=1, path="a.rs")],
            mine=[dict(filename="b.rs", previous_filename="a.rs")],
        )
        self.assertIn("#1 Work", state["comments"][0])


if __name__ == "__main__":
    unittest.main()

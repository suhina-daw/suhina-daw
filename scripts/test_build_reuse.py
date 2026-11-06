# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Build selection against API fixtures and real main-only merges in temporary git repos."""

from copy import deepcopy
from datetime import datetime, timezone
import tempfile
import unittest

import build_reuse as br
from test_branch_update import Repo

HEAD = "a" * 40
OLD = "b" * 40
NOW = datetime(2026, 10, 28, 12, tzinfo=timezone.utc)


class FakeApi:
    def __init__(self):
        self.pr = {"number": 960, "head": {"sha": HEAD, "ref": "feature/panel", "repo": {"full_name": "contributor/repo"}},
                   "labels": []}
        self.runs = [{"id": 42, "head_sha": HEAD, "head_branch": "feature/panel", "event": "pull_request",
                      "head_repository": {"full_name": "contributor/repo"}, "actor": {"login": "Maintainer"},
                      "created_at": "2026-10-28T11:00:00Z", "status": "completed", "conclusion": "success"}]
        self.jobs = [{"name": f"test build ({p})", "status": "completed", "conclusion": "success"} for p in br.PLATFORMS]
        self.artifacts = [{"name": f"suhina-pr960-{p}", "expired": False, "size_in_bytes": 100,
                           "expires_at": "2026-11-04T12:00:00Z"} for p in br.PLATFORMS]
        self.comments = []
        self.source = {"user": {"login": "Trusted", "type": "User"}, "body": "/test-build",
                       "updated_at": "2026-10-28T10:00:00Z",
                       "issue_url": "https://api.github.com/repos/owner/repo/issues/960"}

    def get(self, path):
        if path.startswith("actions/workflows/"):
            sha = path.split("head_sha=")[1].split("&")[0]
            return {"workflow_runs": [r for r in self.runs if r["head_sha"] == sha]}
        if path == "issues/comments/12":
            return self.source
        raise AssertionError(path)

    def pages(self, path, key=None):
        return {"issues/960/comments": self.comments, "actions/runs/42/jobs": self.jobs,
                "actions/runs/42/artifacts": self.artifacts,
                "actions/runs/43/jobs": [dict(j, status="in_progress", conclusion=None) for j in self.jobs]}[path]

    def proof(self, at="2026-10-28T10:01:00Z"):
        self.comments = [{"user": {"login": "suhina-bot[bot]", "type": "Bot"}, "created_at": at,
                          "body": f"<!-- trusted-build {HEAD} Trusted 12 -->"}]

    def find(self, candidates=None, trusted=None):
        return br.find_reuse(self, "owner/repo", self.pr, HEAD, candidates or [HEAD], {"maintainer"},
                             {"trusted"} if trusted is None else trusted, NOW)


class BuildReuse(unittest.TestCase):
    def test_ready_build_has_every_requested_artifact(self):
        api = FakeApi()
        self.assertEqual(api.find(), {"head": HEAD, "source": HEAD, "state": "ready",
                                     "url": "https://github.com/owner/repo/actions/runs/42"})

    def test_running_build_is_reused_without_cancelling_it(self):
        api = FakeApi()
        api.runs[0].update(status="in_progress", conclusion=None)
        api.jobs[0].update(status="in_progress", conclusion=None)
        self.assertEqual(api.find()["state"], "running")

    def test_failed_cancelled_skipped_or_partial_build_is_not_reused(self):
        for conclusion in ["failure", "cancelled", "skipped", "timed_out"]:
            api = FakeApi()
            api.runs[0]["conclusion"] = conclusion
            self.assertEqual(api.find(), {})
        for jobs in [[], FakeApi().jobs[:1]]:
            api = FakeApi()
            api.jobs = jobs
            self.assertEqual(api.find(), {})
        api = FakeApi()
        api.runs[0]["status"] = "in_progress"
        api.jobs[0]["conclusion"] = "failure"
        self.assertEqual(api.find(), {})

    def test_expired_missing_empty_or_deleted_artifact_requires_build(self):
        for change in [{"expired": True}, {"expires_at": "2026-10-28T11:59:59Z"},
                       {"size_in_bytes": 0}, {"expires_at": None}, {"name": "unrelated"}]:
            api = FakeApi()
            api.artifacts[0].update(change)
            self.assertEqual(api.find(), {})
        api = FakeApi()
        api.artifacts.pop()
        self.assertEqual(api.find(), {})

    def test_platform_request_is_respected(self):
        api = FakeApi()
        api.jobs = [j for j in api.jobs if "windows" in j["name"]]
        api.artifacts = [a for a in api.artifacts if "windows" in a["name"]]
        self.assertEqual(api.find(), {})
        api.pr["labels"] = [{"name": "test-build-windows"}]
        self.assertEqual(api.find()["state"], "ready")

    def test_unrelated_branch_repository_event_or_actor_does_not_count(self):
        for change in [{"head_branch": "other"}, {"head_repository": {"full_name": "attacker/repo"}},
                       {"event": "workflow_dispatch"}, {"actor": {"login": "outsider"}}]:
            api = FakeApi()
            api.runs[0].update(change)
            self.assertEqual(api.find(), {})

    def test_bot_run_needs_original_authorization_before_the_run(self):
        api = FakeApi()
        api.runs[0]["actor"] = {"login": "suhina-bot[bot]"}
        self.assertEqual(api.find(), {})
        api.proof()
        self.assertEqual(api.find()["state"], "ready")
        self.assertEqual(api.find(trusted=set()), {})
        for change in [{"body": "/close"}, {"updated_at": "2026-10-28T11:30:00Z"},
                       {"issue_url": "https://api.github.com/repos/owner/repo/issues/961"},
                       {"user": {"login": "attacker", "type": "User"}}]:
            other = deepcopy(api)
            other.source.update(change)
            self.assertEqual(other.find(), {})
        api.proof("2026-10-28T11:30:00Z")
        self.assertEqual(api.find(), {})

    def test_dispatch_window_coalesces_requests_before_run_is_visible(self):
        api = FakeApi()
        api.runs = []
        api.pr["labels"] = [{"name": "test-build"}]
        api.proof("2026-10-28T11:59:00Z")
        self.assertEqual(api.find()["state"], "requested")
        api.proof("2026-10-28T11:50:00Z")
        self.assertEqual(api.find(), {})
        api.proof("2026-10-28T12:01:00Z")
        self.assertEqual(api.find(), {})

    def test_a_retry_of_a_failed_run_also_has_a_dispatch_window(self):
        api = FakeApi()
        api.runs[0]["conclusion"] = "failure"
        api.pr["labels"] = [{"name": "test-build"}]
        api.proof("2026-10-28T11:59:00Z")
        self.assertEqual(api.find()["state"], "requested")
        api.runs[0]["created_at"] = "2026-10-28T11:59:30Z"
        self.assertEqual(api.find(), {})

    def test_queued_run_without_matrix_jobs_keeps_dispatch_window(self):
        api = FakeApi()
        api.pr["labels"] = [{"name": "test-build"}]
        api.proof("2026-10-28T11:59:00Z")
        api.runs[0].update(status="queued", conclusion=None, created_at="2026-10-28T11:59:30Z")
        api.jobs = []
        self.assertEqual(api.find()["state"], "requested")
        api.runs[0].update(status="completed", conclusion="success")
        self.assertEqual(api.find(), {})

    def test_successful_ancestor_only_reused_after_verified_main_update(self):
        api = FakeApi()
        api.runs[0]["head_sha"] = OLD
        self.assertEqual(api.find(), {})
        self.assertEqual(api.find([HEAD, OLD])["source"], OLD)
        api.runs[0]["status"] = "in_progress"
        self.assertEqual(api.find([HEAD, OLD]), {})

    def test_lookup_error_stops_instead_of_assuming_no_build(self):
        api = FakeApi()
        api.get = lambda _: (_ for _ in ()).throw(RuntimeError("API unavailable"))
        with self.assertRaises(RuntimeError):
            api.find()


class PreferReady(unittest.TestCase):
    def test_finished_ancestor_build_wins_over_running_current_build(self):
        api = FakeApi()
        api.runs[0]["head_sha"] = OLD
        api.runs.append(dict(api.runs[0], id=43, head_sha=HEAD, status="in_progress", conclusion=None))
        self.assertEqual(api.find([HEAD, OLD])["state"], "ready")
        self.assertEqual(api.find([HEAD, OLD])["source"], OLD)

    def test_running_current_build_is_kept_when_nothing_finished(self):
        api = FakeApi()
        api.runs = [dict(api.runs[0], id=43, status="in_progress", conclusion=None)]
        self.assertEqual(api.find([HEAD, OLD])["state"], "running")


class BinaryPaths(unittest.TestCase):
    def test_paths_come_from_test_build_workflow(self):
        from pathlib import Path
        text = (Path(__file__).resolve().parents[1] / ".github/workflows/test-build.yml").read_text(encoding="utf-8")
        paths = br.build_paths(text)
        self.assertIn("crates/**", paths)
        self.assertIn("Cargo.lock", paths)
        self.assertNotIn("docs/**", paths)

    def test_only_binary_paths_need_a_build(self):
        paths = ["crates/**", "Cargo.lock", ".github/workflows/test-build.yml"]
        self.assertFalse(br.builds_binary(["docs/a.md", ".github/workflows/ci.yml", "scripts/x.py"], paths))
        self.assertTrue(br.builds_binary(["docs/a.md", "crates/ui/src/a.rs"], paths))
        self.assertTrue(br.builds_binary(["Cargo.lock"], paths))
        self.assertTrue(br.builds_binary([".github/workflows/test-build.yml"], paths))
        self.assertFalse(br.builds_binary(["crates-old/a.rs", "Cargo.lock.bak"], paths))


class EligibleHeads(unittest.TestCase):
    def test_main_updates_can_reach_the_original_build_but_new_work_cannot(self):
        with tempfile.TemporaryDirectory() as folder:
            repo = Repo(folder)
            original = repo.head()
            repo.on_main(lambda: repo.write("main.txt", "one\n"))
            _, first = repo.update_branch()
            repo.on_main(lambda: repo.write("main.txt", "two\n"))
            _, head = repo.update_branch()
            self.assertEqual(br.eligible_heads(head, "main", folder), [head, first, original])
            repo.write("pr.txt", "changed\n")
            new = repo.commit("new work")
            self.assertEqual(br.eligible_heads(new, "main", folder), [new])

    def test_changed_merge_cannot_reuse_parent_build(self):
        with tempfile.TemporaryDirectory() as folder:
            repo = Repo(folder)
            repo.on_main(lambda: repo.edit("shared.txt", 7, "nearby change\n"))
            _, head = repo.update_branch()
            self.assertEqual(br.eligible_heads(head, "main", folder), [head])


if __name__ == "__main__":
    unittest.main()

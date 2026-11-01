# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Tests for agent_post.py: python3 -m unittest discover -s scripts -p 'test_*.py'"""

import json
import subprocess
import sys
import unittest
from unittest import mock

import agent_post as ap


def setUpModule():
    ap.say = lambda message: None


ISSUE = {"title": "Piano roll: snap notes", "body": "Goal.\n", "type": "enhancement", "labels": "ui, engine", "parent": "123"}


class FakeGh:
    """Answers `gh` calls from a table of argument prefixes; records each call and any body file."""

    def __init__(self, permissions, answers=()):
        self.answers = [(["api", f"repos/{ap.REPO}", "--jq", ".permissions"], ap.Result(0, json.dumps(permissions), ""))]
        self.answers += list(answers)
        self.answers.append((
            ["api", f"repos/{ap.REPO}/labels"],
            ap.Result(0, "enhancement\nui\nengine\nepic\n", ""),
        ))
        self.answers.append((
            ["api", f"repos/{ap.REPO}/issues/123"],
            ap.Result(0, json.dumps({"state": "open", "labels": [{"name": "epic"}]}), ""),
        ))
        self.calls = []
        self.bodies = []

    def __call__(self, args, stdin):
        self.calls.append((args, stdin))
        if "--body-file" in args:
            with open(args[args.index("--body-file") + 1], encoding="utf-8") as file:
                self.bodies.append(file.read())
        for prefix, result in self.answers:
            if args[: len(prefix)] == prefix:
                return result
        return ap.Result(0, "", "")

    def called(self, *prefix):
        return [args for args, _ in self.calls if args[: len(prefix)] == list(prefix)]


def created(number):
    return (["issue", "create"], ap.Result(0, f"https://github.com/{ap.REPO}/issues/{number}\n", ""))


class WithWriteAccess(unittest.TestCase):
    def test_an_issue_goes_through_the_workflow_and_prints_its_line(self):
        gh = FakeGh(
            {"push": True, "triage": True},
            [
                (["workflow", "run"], ap.Result(0, f"https://github.com/{ap.REPO}/actions/runs/42\n", "")),
                (["run", "view", "42"], ap.Result(0, f"open\tOPENED_ISSUE=#900 https://github.com/{ap.REPO}/issues/900\n", "")),
            ],
        )
        line = ap.post("issue", ISSUE, gh)
        self.assertEqual(line, f"OPENED_ISSUE=#900 https://github.com/{ap.REPO}/issues/900")
        (args, stdin), = [(a, s) for a, s in gh.calls if a[:2] == ["workflow", "run"]]
        self.assertEqual(args, ["workflow", "run", "open-issue.yml", "--repo", ap.REPO, "--json"])
        self.assertEqual(json.loads(stdin)["labels"], "ui, engine")
        self.assertEqual(gh.called("run", "watch", "42")[0][-1], "--exit-status")
        self.assertEqual(gh.called("issue", "create"), [])

    def test_a_comment_goes_through_its_workflow(self):
        gh = FakeGh(
            {"push": True},
            [
                (["workflow", "run"], ap.Result(0, f"https://github.com/{ap.REPO}/actions/runs/7\n", "")),
                (["run", "view", "7"], ap.Result(0, f"COMMENTED_PR=#600 https://github.com/{ap.REPO}/pull/600#c\n", "")),
            ],
        )
        self.assertEqual(ap.post("comment", {"pr": "#600", "body": "Review."}, gh), f"COMMENTED_PR=#600 https://github.com/{ap.REPO}/pull/600#c")
        self.assertEqual(gh.called("workflow", "run")[0][2], "comment-pr.yml")
        self.assertEqual(gh.called("pr", "comment"), [])

    def test_a_dispatch_answered_403_is_posted_as_the_user_once(self):
        gh = FakeGh(
            {"push": True},
            [
                (["workflow", "run"], ap.Result(1, "", "could not create workflow dispatch event: HTTP 403: Must have admin rights to Repository.")),
                created(901),
            ],
        )
        self.assertTrue(ap.post("issue", ISSUE, gh).startswith("OPENED_ISSUE=#901 "))
        self.assertEqual(len(gh.called("workflow", "run")), 1, "a refused dispatch is not retried")
        self.assertEqual(len(gh.called("issue", "create")), 1)

    def test_a_run_without_its_line_is_an_error(self):
        gh = FakeGh(
            {"push": True},
            [(["workflow", "run"], ap.Result(0, f"https://github.com/{ap.REPO}/actions/runs/9\n", ""))],
        )
        with self.assertRaises(ap.PostError):
            ap.post("issue", ISSUE, gh)
        self.assertEqual(gh.called("issue", "create"), [], "no second issue when the run's outcome is unknown")


class WithoutWriteAccess(unittest.TestCase):
    def test_triage_omits_missing_labels_and_names_them_in_the_body(self):
        for available, applied, omitted in (
            ("enhancement\nUI\n", ["enhancement", "ui"], "`engine`"),
            ("ui\nengine\n", ["ui", "engine"], "`enhancement`"),
            ("", [], "`enhancement`, `ui`, `engine`"),
        ):
            with self.subTest(available=available):
                gh = FakeGh({"triage": True}, [
                    (["api", f"repos/{ap.REPO}/labels"], ap.Result(0, available, "")),
                    created(902),
                ])
                self.assertTrue(ap.post("issue", ISSUE, gh).startswith("OPENED_ISSUE=#902 "))
                creates = gh.called("issue", "create")
                self.assertEqual(len(creates), 1)
                args = creates[0]
                self.assertEqual([args[i + 1] for i, arg in enumerate(args) if arg == "--label"], applied)
                self.assertIn(f"Labels for a maintainer to add: {omitted}.", gh.bodies[0])
                lookup = gh.called("api", f"repos/{ap.REPO}/labels")[0]
                self.assertIn("--paginate", lookup)
                self.assertLess([a for a, _ in gh.calls].index(lookup), [a for a, _ in gh.calls].index(args))

    def test_failed_label_lookup_stops_before_creating(self):
        for code in (1, 124):
            with self.subTest(code=code):
                gh = FakeGh({"triage": True}, [
                    (["api", f"repos/{ap.REPO}/labels"], ap.Result(code, "enhancement\n", "Lookup failed")),
                    created(902),
                ])
                with self.assertRaisesRegex(ap.PostError, "labels.*no issue was opened"):
                    ap.post("issue", ISSUE, gh)
                self.assertEqual(gh.called("issue", "create"), [])
                self.assertEqual(gh.called("api", "-X"), [])

    def test_create_failure_after_label_lookup_is_not_retried(self):
        gh = FakeGh({"triage": True}, [
            (["issue", "create"], ap.Result(1, "", "'engine' not found")),
        ])
        with self.assertRaises(ap.PostError):
            ap.post("issue", ISSUE, gh)
        self.assertEqual(len(gh.called("issue", "create")), 1)

    def test_issue_bodies_identify_the_agent_once_with_or_without_triage(self):
        for triage in (False, True):
            for body in ("Goal.", 'The guide mentions "AI-assisted".', f"Goal.\n\n{ap.AGENT_NOTE}"):
                with self.subTest(triage=triage, body=body):
                    gh = FakeGh({"push": False, "triage": triage}, [created(902)])
                    ap.post("issue", {**ISSUE, "body": body}, gh)
                    self.assertEqual(gh.bodies[0].count(ap.AGENT_NOTE), 1)

    def test_invalid_parents_are_rejected_before_creating_an_issue(self):
        for triage in (False, True):
            for parent in (
                {"state": "closed", "labels": [{"name": "epic"}]},
                {"state": "open", "labels": [{"name": "bug"}]},
                {"state": "open", "labels": [{"name": "epic"}], "pull_request": {"url": "pr"}},
            ):
                with self.subTest(triage=triage, parent=parent):
                    gh = FakeGh({"push": False, "triage": triage}, [
                        (["api", f"repos/{ap.REPO}/issues/123"], ap.Result(0, json.dumps(parent), "")),
                        created(902),
                    ])
                    with self.assertRaises(ap.RequestError):
                        ap.post("issue", ISSUE, gh)
                    self.assertEqual(gh.called("issue", "create"), [])

    def test_unreadable_parent_does_not_create_an_issue(self):
        for result in (ap.Result(1, "", "HTTP 404"), ap.Result(124, "", "Timed out"), ap.Result(0, "not JSON", "")):
            with self.subTest(result=result):
                gh = FakeGh({"push": False}, [
                    (["api", f"repos/{ap.REPO}/issues/123"], result), created(902),
                ])
                with self.assertRaises(ap.PostError):
                    ap.post("issue", ISSUE, gh)
                self.assertEqual(gh.called("issue", "create"), [])

    def test_no_parent_needs_no_parent_lookup(self):
        gh = FakeGh({"push": False}, [created(902)])
        ap.post("issue", {**ISSUE, "parent": ""}, gh)
        self.assertEqual(len(gh.called("api")), 1, "only the permission lookup is needed")

    def test_triage_sets_labels_type_and_parent(self):
        gh = FakeGh(
            {"push": False, "triage": True},
            [created(902), (["api", f"repos/{ap.REPO}/issues/902", "--jq", ".id"], ap.Result(0, "5550001\n", ""))],
        )
        self.assertEqual(ap.post("issue", ISSUE, gh), f"OPENED_ISSUE=#902 https://github.com/{ap.REPO}/issues/902")
        self.assertEqual(gh.called("workflow"), [], "no dispatch without write access")
        args = gh.called("issue", "create")[0]
        labels = [args[i + 1] for i, a in enumerate(args) if a == "--label"]
        self.assertEqual(labels, ["enhancement", "ui", "engine"])
        self.assertIn("type=Feature", gh.called("api", "-X", "PATCH")[0])
        self.assertIn("sub_issue_id=5550001", gh.called("api", "-X", "POST", f"repos/{ap.REPO}/issues/123/sub_issues")[0])
        self.assertIn("Part of #123", gh.bodies[0])
        self.assertNotIn("Labels for a maintainer", gh.bodies[0])

    def test_read_access_names_the_labels_and_parent_in_the_body(self):
        gh = FakeGh({"push": False, "triage": False}, [created(903)])
        ap.post("issue", ISSUE, gh)
        args = gh.called("issue", "create")[0]
        self.assertNotIn("--label", args)
        self.assertEqual(args[args.index("--title") + 1], "Piano roll: snap notes")
        self.assertIn("Part of #123", gh.bodies[0])
        self.assertIn("Labels for a maintainer to add: `enhancement`, `ui`, `engine`.", gh.bodies[0])
        self.assertEqual(gh.called("api", "-X"), [], "no type or parent calls without triage")

    def test_an_epic_has_no_issue_type(self):
        gh = FakeGh({"push": False, "triage": True}, [created(904)])
        ap.post("issue", {**ISSUE, "type": "epic", "parent": ""}, gh)
        self.assertEqual(gh.called("api", "-X", "PATCH"), [])

    def test_unknown_access_posts_as_the_user(self):
        gh = FakeGh({}, [(["api", f"repos/{ap.REPO}", "--jq", ".permissions"], ap.Result(1, "", "HTTP 401")), created(905)])
        gh.answers.pop(0)
        self.assertTrue(ap.post("issue", ISSUE, gh).startswith("OPENED_ISSUE=#905 "))
        self.assertEqual(gh.called("workflow"), [])

    def test_a_comment_is_marked_as_ai_assisted(self):
        gh = FakeGh(
            {"push": False, "triage": True},
            [(["pr", "comment"], ap.Result(0, f"https://github.com/{ap.REPO}/pull/600#issuecomment-1\n", ""))],
        )
        line = ap.post("comment", {"pr": "600", "body": "Two findings."}, gh)
        self.assertEqual(line, f"COMMENTED_PR=#600 https://github.com/{ap.REPO}/pull/600#issuecomment-1")
        self.assertEqual(gh.called("pr", "comment")[0][:4], ["pr", "comment", "600", "--repo"])
        self.assertTrue(gh.bodies[0].startswith("Two findings."))
        self.assertIn(ap.AGENT_NOTE, gh.bodies[0])

    def test_a_body_that_mentions_the_phrase_without_the_note_still_gets_the_note(self):
        for body in [
            'The guide says an "AI-assisted" note is required for agent feedback.',
            f'Reviewed. {ap.AGENT_NOTE}',
        ]:
            gh = FakeGh(
                {"push": False},
                [(["pr", "comment"], ap.Result(0, f"https://github.com/{ap.REPO}/pull/600#issuecomment-2\n", ""))],
            )
            ap.post("comment", {"pr": "600", "body": body}, gh)
            self.assertEqual(gh.bodies[0].count(ap.AGENT_NOTE), 1, gh.bodies[0])

    def test_a_refused_create_is_an_error(self):
        gh = FakeGh({"push": False}, [(["issue", "create"], ap.Result(1, "", "HTTP 410: Issues are disabled"))])
        with self.assertRaises(ap.PostError):
            ap.post("issue", ISSUE, gh)


class GhCalls(unittest.TestCase):
    def test_missing_marker_reports_uncertainty_without_posting_again(self):
        for kind, request in (("issue", ISSUE), ("comment", {"pr": "900", "body": "Review."})):
            for watch_code in (0, 1, 124):
                for log_code in (0, 1, 124):
                    with self.subTest(kind=kind, watch_code=watch_code, log_code=log_code):
                        gh = FakeGh({"push": True}, [
                            (["workflow", "run"], ap.Result(0, f"https://github.com/{ap.REPO}/actions/runs/42", "")),
                            (["run", "watch"], ap.Result(watch_code, "", "")),
                            (["run", "view"], ap.Result(log_code, "", "")),
                        ])
                        with self.assertRaises(ap.PostError) as caught:
                            ap.post(kind, request, gh)
                        message = str(caught.exception)
                        self.assertIn("posting outcome is unknown", message)
                        self.assertIn("Do not post again", message)
                        self.assertIn(f"gh run view 42 --repo {ap.REPO}", message)
                        self.assertEqual(len(gh.called("workflow", "run")), 1)
                        self.assertEqual(gh.called("issue", "create"), [])
                        self.assertEqual(gh.called("pr", "comment"), [])

    def test_utf8_pipes_ignore_the_windows_legacy_encoding(self):
        real_run = subprocess.run
        output = "Opened \u201dissue\u201d\nOPENED_ISSUE=#900 https://github.com/suhina-daw/suhina-daw/issues/900\n"
        diagnostic = "\u0101\u2014diagnostic\n"

        def utf8_child(args, **kwargs):
            # Model Python's legacy Windows default, but keep the real pipe I/O.
            kwargs.setdefault("encoding", "cp1252")
            script = (
                "import sys; "
                "data = sys.stdin.buffer.read(); "
                "sys.stdout.buffer.write(data + " + repr(output.encode("utf-8")) + "); "
                "sys.stderr.buffer.write(" + repr(diagnostic.encode("utf-8")) + ")"
            )
            return real_run([sys.executable, "-c", script], **kwargs)

        for request in ("", "\u0101\u201drequest\n"):
            with self.subTest(request=request), mock.patch.object(ap.subprocess, "run", utf8_child):
                result = ap.run_gh(["run", "view", "42", "--log"], request)
                self.assertEqual(result.returncode, 0)
                self.assertEqual(result.stdout, request + output)
                self.assertEqual(result.stderr, diagnostic)

    def test_unsuccessful_watch_preserves_the_post_without_posting_again(self):
        for kind, request, marker, url in (
            ("issue", ISSUE, "OPENED_ISSUE", f"https://github.com/{ap.REPO}/issues/900"),
            ("comment", {"pr": "900", "body": "Review."}, "COMMENTED_PR", f"https://github.com/{ap.REPO}/pull/900#issuecomment-1"),
        ):
            for code in (1, 124):
                with self.subTest(kind=kind, code=code):
                    line = f"{marker}=#900 {url}"
                    gh = FakeGh({"push": True}, [
                        (["workflow", "run"], ap.Result(0, f"https://github.com/{ap.REPO}/actions/runs/42", "")),
                        (["run", "watch"], ap.Result(code, "", "Could not confirm completion")),
                        (["run", "view"], ap.Result(0, line, "")),
                    ])
                    with self.assertRaises(ap.PostError) as caught:
                        ap.post(kind, request, gh)
                    self.assertIn(line, str(caught.exception))
                    self.assertIn("42", str(caught.exception))
                    self.assertEqual(len(gh.called("workflow", "run")), 1)
                    self.assertEqual(gh.called("issue", "create"), [])
                    self.assertEqual(gh.called("pr", "comment"), [])

    def test_a_hung_gh_comes_back_as_a_failed_result(self):
        def hung(args, input=None, stdin=None, capture_output=None, text=None, encoding=None, check=None, timeout=None):
            raise subprocess.TimeoutExpired(args, timeout)

        with mock.patch.object(ap.subprocess, "run", hung):
            done = ap.run_gh(["api", f"repos/{ap.REPO}"])
        self.assertEqual(done.returncode, 124)
        self.assertIn("gave no answer within 120 s", done.stderr)

    def test_the_watch_timeout_is_the_long_one(self):
        captured = {}

        def watching(args, input=None, stdin=None, capture_output=None, text=None, encoding=None, check=None, timeout=None):
            captured["timeout"] = timeout
            return ap.Result(0, "", "")

        with mock.patch.object(ap.subprocess, "run", watching):
            ap.run_gh(["run", "watch", "42", "--exit-status"])
        self.assertEqual(captured["timeout"], ap.RUN_WATCH_TIMEOUT)


class Requests(unittest.TestCase):
    def test_malformed_requests_are_refused_before_anything_is_asked(self):
        for kind, raw in [
            ("issue", {**ISSUE, "title": " "}),
            ("issue", {**ISSUE, "title": "x" * 257}),
            ("issue", {**ISSUE, "body": ""}),
            ("issue", {**ISSUE, "type": "feature"}),
            ("issue", {**ISSUE, "labels": "ui, bug"}),
            ("issue", {**ISSUE, "labels": "UI;rm"}),
            ("issue", {**ISSUE, "parent": "12a"}),
            ("comment", {"pr": "six", "body": "x"}),
            ("comment", {"pr": "6", "body": "x" * 60001}),
        ]:
            gh = FakeGh({"push": True})
            with self.subTest(raw=raw), self.assertRaises(ap.RequestError):
                ap.post(kind, raw, gh)
            self.assertEqual(gh.calls, [])

    def test_main_reports_a_bad_request_file(self):
        self.assertEqual(ap.main(["agent_post.py", "issue", "/nonexistent/request.json"]), 2)
        self.assertEqual(ap.main(["agent_post.py", "label", "request.json"]), 2)


if __name__ == "__main__":
    unittest.main()

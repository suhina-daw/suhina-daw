# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Tests for nightly_notes.py: python3 -m unittest discover -s scripts -p 'test_*.py'"""

import json
import unittest

import nightly_notes as nn

REPO = "suhina-daw/suhina-daw"
PULL = f"https://github.com/{REPO}/pull"
RANGE = "<!-- nightly-range: 565bc91..2e98c93 -->"
PREAMBLE = "**A development build, not a release.** Built from `main`.\n\nTalk about it on Discord."
SCRIPT_BODY = (
    f"{PREAMBLE}\n\n### New since the previous nightly\n\n- NAM: faster ([#1426]({PULL}/1426))\n\n"
    f"9 internal changes not listed.\n\n{RANGE}\n"
)
NOTES = (
    "### New since the previous nightly\n\n"
    f"#### NAM Amp Modeler uses far less CPU ([#1426]({PULL}/1426))\n\n"
    "- A new **Mono** button runs one model for both sides.\n\n"
    "#### Under the hood\n\n"
    f"Docs and CI ([#1462]({PULL}/1462))."
)


class Problems(unittest.TestCase):
    def test_good_notes_pass(self):
        self.assertEqual(nn.problems("nightly-20261115", NOTES), [])
        self.assertEqual(nn.problems("nightly-20261115-2", NOTES), [])

    def test_bad_tags_fail(self):
        for tag in ("nightly", "v0.1.0", "nightly-2026111", "nightly-20261115-x", "nightly-20261115\n"):
            with self.subTest(tag=tag):
                self.assertTrue(nn.problems(tag, NOTES))

    def test_notes_must_start_with_the_heading(self):
        self.assertTrue(nn.problems("nightly-20261115", "Some notes"))

    def test_html_mentions_and_images_fail(self):
        for bad in ("<script>x</script>", "<!-- notes: composed -->", "hi @everyone", f"![x]({PULL}/1.png)"):
            with self.subTest(bad=bad):
                self.assertTrue(nn.problems("nightly-20261115", NOTES + "\n\n" + bad))

    def test_links_outside_the_repository_fail(self):
        for bad in (
            "[x](https://example.com/a)",
            "https://github.com/someone/else/pull/1",
            "[x](javascript:alert(1))",
            "[x](/relative)",
            "see www.example.com",
            "ftp://example.com",
        ):
            with self.subTest(bad=bad):
                self.assertTrue(nn.problems("nightly-20261115", NOTES + "\n\n" + bad))

    def test_too_long_notes_fail(self):
        self.assertTrue(nn.problems("nightly-20261115", NOTES + "\n\n" + "a" * nn.NOTES_LIMIT))


class Compose(unittest.TestCase):
    def test_replaces_the_scripts_list_and_keeps_the_preamble_and_range(self):
        body = nn.compose(SCRIPT_BODY, NOTES)
        self.assertTrue(body.startswith(PREAMBLE + "\n\n### New since the previous nightly"))
        self.assertNotIn("9 internal changes", body)
        self.assertIn(NOTES, body)
        self.assertTrue(body.endswith(f"{RANGE}\n{nn.COMPOSED}\n"))

    def test_composing_again_replaces_the_notes_and_keeps_the_posted_marker(self):
        posted = nn.mark_posted(nn.compose(SCRIPT_BODY, NOTES))
        again = nn.compose(posted, NOTES.replace("Mono", "Stereo"))
        self.assertIn("Stereo", again)
        self.assertNotIn("**Mono**", again)
        self.assertEqual(again.count(nn.COMPOSED), 1)
        self.assertTrue(again.endswith(f"{RANGE}\n{nn.COMPOSED}\n{nn.POSTED}\n"))

    def test_a_body_without_the_range_line_is_refused(self):
        with self.assertRaises(ValueError):
            nn.compose(f"{PREAMBLE}\n\n### New since the previous nightly\n\n- x", NOTES)

    def test_mark_posted_adds_the_marker_once(self):
        body = nn.mark_posted(nn.compose(SCRIPT_BODY, NOTES))
        self.assertEqual(nn.mark_posted(body), body)
        self.assertEqual(body.count(nn.POSTED), 1)


class Description(unittest.TestCase):
    def test_headings_become_bold_lines(self):
        text = nn.description(NOTES)
        self.assertNotIn("###", text)
        self.assertTrue(text.startswith(f"**NAM Amp Modeler uses far less CPU ([#1426]({PULL}/1426))**"))
        self.assertIn("**Under the hood**", text)

    def test_long_notes_drop_under_the_hood_first(self):
        user = "### New since the previous nightly\n\n#### Big\n\n" + "\n".join("- " + "x" * 70 for _ in range(50))
        hood = "\n\n#### Under the hood\n\n" + "Internal work. " * 30
        self.assertLessEqual(len(nn.description(user)), nn.EMBED_LIMIT)
        # Without the cut, the heading-free text would be over the limit.
        self.assertGreater(len(user + hood) - len("### New since the previous nightly\n\n"), nn.EMBED_LIMIT)
        text = nn.description(user + hood)
        self.assertEqual(text, nn.description(user))
        self.assertNotIn("…", text)

    def test_longer_notes_lose_whole_lines_from_the_end(self):
        bullets = "\n".join(f"- line {i} [#{i}]({PULL}/{i}) " + "x" * 60 for i in range(80))
        text = nn.description("### New since the previous nightly\n\n#### Big\n\n" + bullets)
        self.assertLessEqual(len(text), nn.EMBED_LIMIT)
        self.assertTrue(text.endswith("\n…"))
        for line in text.splitlines()[1:-1]:
            if line:
                self.assertTrue(line.endswith("x"), line)


class FakeGitHub:
    def __init__(self, nightly_commit="2e98c93", posted=False):
        dated_body = SCRIPT_BODY if not posted else nn.mark_posted(nn.compose(SCRIPT_BODY, NOTES))
        self.releases = {
            "nightly-20261115": {"id": 1, "name": "Suhina nightly 0.0.1-nightly.20261115+2e98c93", "body": dated_body},
            "nightly": {"id": 2, "name": "Suhina nightly 0.0.1-nightly.20261115+2e98c93", "body": SCRIPT_BODY},
        }
        self.commits = {"nightly": nightly_commit, "nightly-20261115": "2e98c93"}
        self.patched = []

    def __call__(self, args, stdin=None):
        path = next(a for a in args if a.startswith("repos/"))
        if "-X" in args:
            release_id = int(path.rsplit("/", 1)[1])
            body = json.loads(stdin)["body"]
            self.patched.append((release_id, body))
            for item in self.releases.values():
                if item["id"] == release_id:
                    item["body"] = body
            return "{}"
        if "/releases/tags/" in path:
            return json.dumps(self.releases[path.rsplit("/", 1)[1]])
        if "/commits/" in path:
            return self.commits[path.rsplit("/", 1)[1]] + "\n"
        raise AssertionError(args)


class Publish(unittest.TestCase):
    def setUp(self):
        self.sent = []

    def send(self, method, url, label, body=None, headers=None):
        self.sent.append((method, url, body))
        return {}

    def test_publishes_on_both_releases_and_posts_once(self):
        gh = FakeGitHub()
        done = nn.publish("nightly-20261115", NOTES, REPO, "https://discord.test/hook", gh, self.send)
        self.assertEqual(len(self.sent), 1)
        payload = self.sent[0][2]
        self.assertEqual(payload["allowed_mentions"], {"parse": []})
        embed = payload["embeds"][0]
        self.assertEqual(embed["url"], f"https://github.com/{REPO}/releases/tag/nightly-20261115")
        self.assertEqual(embed["title"], "Suhina nightly 0.0.1-nightly.20261115+2e98c93")
        self.assertIn(nn.POSTED, gh.releases["nightly-20261115"]["body"])
        self.assertIn(nn.COMPOSED, gh.releases["nightly"]["body"])
        self.assertNotIn(nn.POSTED, gh.releases["nightly"]["body"])
        self.assertIn("posted to Discord", done)

    def test_a_rerun_publishes_again_but_never_posts_twice(self):
        gh = FakeGitHub(posted=True)
        done = nn.publish("nightly-20261115", NOTES, REPO, "https://discord.test/hook", gh, self.send)
        self.assertEqual(self.sent, [])
        self.assertIn("already posted to Discord; not posted again", done)

    def test_the_rolling_nightly_is_left_alone_when_it_is_another_build(self):
        gh = FakeGitHub(nightly_commit="ffffff0")
        nn.publish("nightly-20261115", NOTES, REPO, "", gh, self.send)
        self.assertNotIn(nn.COMPOSED, gh.releases["nightly"]["body"])
        self.assertEqual(self.sent, [])

    def test_a_failed_post_leaves_no_posted_marker(self):
        gh = FakeGitHub()

        def failing(*args, **kwargs):
            raise nn.discord_api.DiscordError("the releases webhook: 500", 500)

        with self.assertRaises(nn.discord_api.DiscordError):
            nn.publish("nightly-20261115", NOTES, REPO, "https://discord.test/hook", gh, failing)
        self.assertIn(nn.COMPOSED, gh.releases["nightly-20261115"]["body"])
        self.assertNotIn(nn.POSTED, gh.releases["nightly-20261115"]["body"])


if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Publishes the notes of a dated nightly and posts the nightly to Discord, for
.github/workflows/nightly-notes.yml (docs/releasing.md › Nightly notes).

The notes are written by a scheduled agent that may start workflows but not edit releases, so it
hands them over as this workflow's input. Anyone who can start the workflow can hand in text that
ends up on a public release page and in Discord, so the notes are checked first (`problems`):
they start with the nightly heading, carry no HTML, images or @, and link only to this
repository. Then:

  1. The dated release's notes section (from `### New since the previous nightly` to the hidden
     `nightly-range` line) is replaced and `<!-- notes: composed -->` added; the rolling `nightly`
     gets the same when it is the same build.
  2. Unless the dated release already says `<!-- discord: posted -->`, the notes go to the
     releases channel as one embed, and only after Discord took it is that marker added, so a
     failed post is retried by the next run and a successful one is never repeated.

Environment: TAG, NOTES, GITHUB_REPOSITORY, GH_TOKEN (for gh) and DISCORD_WEBHOOK (optional:
without it nothing is posted). Standard library and gh only; the webhook URL is never printed.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from typing import Callable

import discord_api

HEADING = "### New since the previous nightly"
COMPOSED = "<!-- notes: composed -->"
POSTED = "<!-- discord: posted -->"
TAG = re.compile(r"^nightly-[0-9]{8}(-[0-9]+)?$")
RANGE = re.compile(r"^<!-- nightly-range: [0-9a-f]*\.\.[0-9a-f]+ -->$", re.MULTILINE)
URL = re.compile(r"[a-zA-Z][a-zA-Z0-9+.-]*://[^\s)\]]+|www\.[^\s)\]]+")
# The target of a Markdown link, whatever its scheme.
LINK = re.compile(r"\]\(\s*([^)\s]*)")
ALLOWED_URL = "https://github.com/suhina-daw/suhina-daw/"
NOTES_LIMIT = 20000
# Discord allows 4096 characters in an embed's description; a little is kept free.
EMBED_LIMIT = 4000
EMBED_COLOUR = 0x5865F2

Gh = Callable[[list[str], str | None], str]
Send = Callable[..., dict]


def problems(tag: str, notes: str) -> list[str]:
    """What keeps the notes from being published; empty when they may be."""
    found = []
    # fullmatch: `$` alone would let a trailing newline through.
    if not TAG.fullmatch(tag):
        found.append(f"the tag {tag!r} is not nightly-YYYYMMDD or nightly-YYYYMMDD-N")
    if not notes.strip().startswith(HEADING):
        found.append(f"the notes do not start with {HEADING!r}")
    if len(notes) > NOTES_LIMIT:
        found.append(f"the notes are {len(notes)} characters, more than {NOTES_LIMIT}")
    if "<" in notes:
        found.append("the notes contain <, so possibly HTML")
    if "@" in notes:
        found.append("the notes contain @, which could mention someone")
    if "![" in notes:
        found.append("the notes contain an image")
    for url in dict.fromkeys(URL.findall(notes) + LINK.findall(notes)):
        if not url.startswith(ALLOWED_URL):
            found.append(f"the notes link outside this repository: {url}")
    return found


def compose(body: str, notes: str) -> str:
    """The release body with its notes section replaced: the text before the heading, the notes,
    the range line and the markers the body had, with COMPOSED added."""
    found = RANGE.search(body)
    if not found:
        raise ValueError("the release has no nightly-range line")
    head = body.split(HEADING, 1)[0] if HEADING in body else body[: found.start()]
    markers = [COMPOSED] + ([POSTED] if POSTED in body else [])
    return "\n\n".join([head.rstrip(), notes.strip(), "\n".join([found.group(0)] + markers)]) + "\n"


def mark_posted(body: str) -> str:
    return body if POSTED in body else body.rstrip("\n") + "\n" + POSTED + "\n"


def description(notes: str) -> str:
    """The notes as an embed's description: no top heading, each `#### ` heading a bold line, cut
    to EMBED_LIMIT by dropping Under the hood first and then whole lines from the end."""
    lines = []
    for line in notes.strip().splitlines():
        if line.strip() == HEADING:
            continue
        lines.append(f"**{line[5:].strip()}**" if line.startswith("#### ") else line)
    text = "\n".join(lines).strip()
    if len(text) > EMBED_LIMIT and "**Under the hood**" in text:
        text = text.split("**Under the hood**", 1)[0].rstrip()
    if len(text) <= EMBED_LIMIT:
        return text
    # Whole lines from the end, so no link is cut in half; "\n…" shows the cut.
    kept = text.splitlines()
    while kept and len("\n".join(kept).rstrip()) > EMBED_LIMIT - 2:
        kept.pop()
    if not kept:
        return text[: EMBED_LIMIT - 1] + "…"
    return "\n".join(kept).rstrip() + "\n…"


def run_gh(args: list[str], stdin: str | None = None) -> str:
    result = subprocess.run(["gh", *args], input=stdin, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        raise RuntimeError(f"gh {' '.join(args[:2])}: {result.stderr.strip()[:300]}")
    return result.stdout


def release(gh: Gh, repo: str, tag: str) -> dict:
    return json.loads(gh(["api", f"repos/{repo}/releases/tags/{tag}"], None))


def commit(gh: Gh, repo: str, ref: str) -> str:
    return gh(["api", f"repos/{repo}/commits/{ref}", "--jq", ".sha"], None).strip()


def set_body(gh: Gh, repo: str, release_id: int, body: str) -> None:
    gh(["api", "-X", "PATCH", f"repos/{repo}/releases/{release_id}", "--input", "-"], json.dumps({"body": body}))


def publish(tag: str, notes: str, repo: str, webhook: str, gh: Gh = run_gh, send: Send = discord_api.send) -> list[str]:
    """Publishes and posts as the module docstring says; returns what was done, one line each."""
    done = []
    dated = release(gh, repo, tag)
    body = compose(dated.get("body") or "", notes)
    set_body(gh, repo, dated["id"], body)
    done.append(f"notes published on {tag}")
    # The rolling nightly is a copy: a failure there is reported and does not hold up the post.
    try:
        if commit(gh, repo, "nightly") == commit(gh, repo, tag):
            rolling = release(gh, repo, "nightly")
            set_body(gh, repo, rolling["id"], compose(rolling.get("body") or "", notes))
            done.append("notes published on nightly, the same build")
    except (RuntimeError, ValueError, KeyError) as error:
        done.append(f"::warning::nightly not updated: {error}")
    if POSTED in body:
        done.append("already posted to Discord; not posted again")
    elif not webhook:
        done.append("no DISCORD_WEBHOOK; nothing posted")
    else:
        title = dated.get("name") or f"Suhina {tag}"
        embed = {
            "title": title[:256],
            "url": f"https://github.com/{repo}/releases/tag/{tag}",
            "description": description(notes),
            "color": EMBED_COLOUR,
        }
        send("POST", webhook, "the releases webhook", {"embeds": [embed], "allowed_mentions": {"parse": []}})
        set_body(gh, repo, dated["id"], mark_posted(body))
        done.append("posted to Discord")
    return done


def main() -> int:
    tag = os.environ.get("TAG", "")
    notes = os.environ.get("NOTES", "")
    repo = os.environ.get("GITHUB_REPOSITORY", "")
    found = problems(tag, notes)
    if found:
        for problem in found:
            print(f"::error::{problem}")
        return 1
    try:
        for line in publish(tag, notes, repo, os.environ.get("DISCORD_WEBHOOK", "")):
            print(line)
    except (RuntimeError, ValueError, KeyError, discord_api.DiscordError) as error:
        print(f"::error::{error}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

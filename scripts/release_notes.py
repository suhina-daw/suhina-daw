#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Release notes from git history, for the release workflow (docs/releasing.md).

    release_notes.py --repo OWNER/REPO --head REF [--base REF] --style STYLE [--version V] [--tag T]

`--base` is the previous release (or nightly); without it the history starts at the beginning.
Styles:

  release   the draft of a tagged release: the ROADMAP "What works now" bullets that are new
            since the base, by milestone, then every change
  nightly   the list of changes for the nightly's release page
  discord   the post announcing a nightly (at most 2000 characters, Discord's limit), linking
            the release under `--tag`: the night's own `nightly-YYYYMMDD`, so the link keeps
            pointing at the build it announced after the rolling `nightly` has moved on

A change is a commit on main's first-parent line, so it is one pull request, titled "Area: what
changed (#123)": a squash merge carries that title, and a merge commit ("Merge pull request #123
from owner/branch") carries it as the first line of its message body (MERGE). Changes that touch only documentation, CI,
scripts and tests (INTERNAL) are counted, not listed: they change nothing in the programs. Standard library
only; reads git, never the network.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys

# Paths that never change the programs: a change touching only these is internal.
INTERNAL = re.compile(r"^(docs/|\.github/|scripts/|corpus/)|(^|/)tests/|\.md$|^(LICENSE|NOTICE)$")
# The section of ROADMAP.md that lists what works, one bullet per feature.
WORKS_NOW = "## What works now"
# Discord rejects longer messages.
DISCORD_LIMIT = 2000
# Lists longer than this end with "and N more".
DISCORD_ITEMS = 15
RELEASE_ITEMS = 150
# The subject GitHub gives a merge commit; the pull request's title is the first line of its body.
MERGE = re.compile(r"^Merge pull request #(\d+) from \S+$")


def git(*args: str) -> str:
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def is_internal(paths: list[str]) -> bool:
    """True when every path is documentation, CI, a script or a test. No paths (an empty commit) counts
    as internal too."""
    return all(INTERNAL.search(path) for path in paths)


def title_of(subject: str, body: str) -> str:
    """The change's title: the subject of a squash merge, or for a merge commit the pull request's
    title (the first line of the body) with its number. A merge commit without a body keeps its
    subject."""
    m = MERGE.match(subject)
    if not m:
        return subject
    first = next((line.strip() for line in body.splitlines() if line.strip()), "")
    if not first:
        return subject
    return f"{first} (#{m.group(1)})"


def changes(base: str | None, head: str) -> list[tuple[str, list[str]]]:
    """(title, changed paths) of each change from `base` (excluded) to `head`, newest first."""
    span = f"{base}..{head}" if base else head
    out = []
    # Bodies span lines, so records end with \x1e and fields with \x00.
    log = git("log", "--first-parent", "--format=%H%x00%P%x00%s%x00%b%x1e", span)
    for record in log.split("\x1e"):
        record = record.strip("\n")
        if not record:
            continue
        sha, parents, subject, body = record.split("\x00", 3)
        first = parents.split(" ")[0] if parents else ""
        if first:
            paths = git("diff", "--name-only", first, sha).splitlines()
        else:
            paths = git("show", "--name-only", "--format=", sha).splitlines()
        out.append((title_of(subject, body), paths))
    return out


def link_pulls(title: str, repo: str, angle: bool = False) -> str:
    """'(#123)' as a link to the pull request. `angle` wraps the URL in <>, which keeps Discord
    from showing a preview of every link."""

    def link(m: re.Match[str]) -> str:
        url = f"https://github.com/{repo}/pull/{m.group(1)}"
        return f"[#{m.group(1)}]({f'<{url}>' if angle else url})"

    return re.sub(r"#(\d+)\b", link, title)


def roadmap_bullets(text: str) -> list[tuple[str, str]]:
    """(milestone heading, bullet) for each bullet under "What works now", a bullet's
    continuation lines joined to it."""
    out: list[tuple[str, str]] = []
    inside = False
    heading = ""
    current: list[str] = []

    def flush() -> None:
        if current:
            out.append((heading, " ".join(current)))
            current.clear()

    for line in text.splitlines():
        if line.startswith("## "):
            flush()
            inside = line.strip() == WORKS_NOW
            continue
        if not inside:
            continue
        if line.startswith("### "):
            flush()
            heading = line[4:].strip()
        elif line.startswith("- "):
            flush()
            current.append(line[2:].strip())
        elif line.startswith("  ") and current:
            current.append(line.strip())
        else:
            flush()
    flush()
    return out


def new_bullets(before: str, after: str) -> list[tuple[str, str]]:
    """The bullets in `after` that `before` does not have, word for word: new features, and
    features whose description changed."""
    old = {bullet for _, bullet in roadmap_bullets(before)}
    return [(h, b) for h, b in roadmap_bullets(after) if b not in old]


def absolute_links(text: str, repo: str, ref: str) -> str:
    """Links between repository files, made absolute so they work on a release page."""

    def fix(m: re.Match[str]) -> str:
        target = m.group(1)
        if re.match(r"^[a-z]+:|^#|^<", target):
            return m.group(0)
        return f"](https://github.com/{repo}/blob/{ref}/{target})"

    return re.sub(r"\]\(([^)\s]+)\)", fix, text)


def file_at(ref: str | None, path: str) -> str:
    if not ref:
        return ""
    try:
        return git("show", f"{ref}:{path}")
    except subprocess.CalledProcessError:
        return ""


def internal_line(count: int) -> str:
    if count == 0:
        return ""
    noun = "change" if count == 1 else "changes"
    return f"{count} internal {noun} (documentation, CI, scripts, tests) not listed."


def nightly_notes(listed: list[str], internal: int, base: str | None, repo: str) -> str:
    if not base:
        return "The first nightly."
    lines = ["### New since the previous nightly", ""]
    if listed:
        lines += [f"- {link_pulls(t, repo)}" for t in listed]
    else:
        lines.append("Nothing in the programs changed.")
    if internal:
        lines += ["", internal_line(internal)]
    return "\n".join(lines)


def discord_post(
    listed: list[str],
    internal: int,
    base: str | None,
    repo: str,
    version: str,
    tag: str = "nightly",
) -> str:
    page = f"https://github.com/{repo}/releases/tag/{tag}"
    head = f"**Suhina nightly `{version}`** is out: <{page}>"
    if not base:
        return f"{head}\nThe first nightly: try the current state and tell us how it goes."
    if not listed:
        tail = internal_line(internal) or "Nothing in the programs changed."
        return f"{head}\n{tail}"
    lines = [head, "", "New since the previous nightly:"]
    footer = internal_line(internal)
    items = [f"- {link_pulls(t, repo, angle=True)}" for t in listed[:DISCORD_ITEMS]]
    # Drop items from the end until the post, with its "more" and footer lines, fits.
    while True:
        rest = len(listed) - len(items)
        more = [f"…and {rest} more on the release page."] if rest else []
        post = "\n".join([*lines, *items, *more, *([footer] if footer else [])])
        if len(post) <= DISCORD_LIMIT or not items:
            break
        items.pop()
    lines = post.splitlines()
    return "\n".join(lines)[:DISCORD_LIMIT]


def release_notes(
    listed: list[str], internal: int, base: str | None, head: str, repo: str
) -> str:
    lines: list[str] = []
    new = new_bullets(file_at(base, "ROADMAP.md"), file_at(head, "ROADMAP.md"))
    if new:
        lines += ["## New and changed", ""]
        heading = None
        for h, bullet in new:
            if h != heading:
                if heading is not None:
                    lines.append("")
                lines += [f"### {h}", ""]
                heading = h
            lines.append(f"- {absolute_links(bullet, repo, head)}")
        lines.append("")
    lines += ["## All changes", ""]
    if listed:
        lines += [f"- {link_pulls(t, repo)}" for t in listed[:RELEASE_ITEMS]]
        if len(listed) > RELEASE_ITEMS:
            lines.append(f"- …and {len(listed) - RELEASE_ITEMS} more.")
    else:
        lines.append("Nothing in the programs changed.")
    if internal:
        lines += ["", internal_line(internal)]
    return "\n".join(lines)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--repo", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--base", default="")
    parser.add_argument("--style", required=True, choices=["release", "nightly", "discord"])
    parser.add_argument("--version", default="")
    parser.add_argument("--tag", default="nightly")
    args = parser.parse_args(argv)
    base = args.base or None

    listed: list[str] = []
    internal = 0
    for title, paths in changes(base, args.head):
        if is_internal(paths):
            internal += 1
        else:
            listed.append(title)

    if args.style == "nightly":
        print(nightly_notes(listed, internal, base, args.repo))
    elif args.style == "discord":
        print(discord_post(listed, internal, base, args.repo, args.version, args.tag))
    else:
        print(release_notes(listed, internal, base, args.head, args.repo))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

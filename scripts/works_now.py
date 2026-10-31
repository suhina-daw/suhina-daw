#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""ROADMAP.md › What works now, from one small file per pull request (issue #768 › 6).

    works_now.py check   what CI runs: every file under docs/works-now/ is a bullet list in a known section
    works_now.py fold    on main (docs-generated.yml): appends each file to its section, deletes the file

A pull request that changes what works for users adds its bullet as a file of its own,
`docs/works-now/<section>/<name>.md`, rather than editing ROADMAP.md, so it never reads the whole
list to add one line and two pull requests never conflict on it. `<section>` is `top` for the list
right under *What works now*, or a milestone, `m0` to `m8`, for the list under `### M<n> …`;
`<name>` is the branch's name or anything else unique. The file holds one or more bullets in the
ROADMAP's own style: a line starting `- `, continued on lines indented by two spaces.

`fold` adds the files to the end of their sections, in the order of their names, and removes
them; the result is what the pull request would have written into ROADMAP.md itself, so
release_notes.py reads the bullets as before. Standard library only.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ROADMAP = ROOT / "ROADMAP.md"
FRAGMENTS = ROOT / "docs" / "works-now"
# The section's heading, the same string as release_notes.py's WORKS_NOW.
WORKS_NOW = "## What works now"
README = "README.md"
MILESTONE = re.compile(r"^### M(\d+)\b")


def sections(roadmap: str) -> dict[str, int]:
    """Each section name and the index of the line after its last line: where a bullet appended
    to it goes. `top` ends at the first milestone heading."""
    lines = roadmap.splitlines()
    try:
        start = lines.index(WORKS_NOW)
    except ValueError:
        raise SystemExit(f"ROADMAP.md has no '{WORKS_NOW}' heading") from None
    out: dict[str, int] = {}
    name = "top"
    for i in range(start + 1, len(lines) + 1):
        line = lines[i] if i < len(lines) else "## "
        m = MILESTONE.match(line)
        if m or line.startswith("## ") or line.startswith("### "):
            out[name] = i
            if not m:
                if line.startswith("## "):
                    break
                raise SystemExit(f"ROADMAP.md:{i + 1}: a heading under '{WORKS_NOW}' that is not a milestone: {line}")
            name = f"m{m.group(1)}"
    return out


def fragments() -> list[Path]:
    if not FRAGMENTS.is_dir():
        return []
    return sorted(p for p in FRAGMENTS.rglob("*") if p.is_file() and p.relative_to(FRAGMENTS).as_posix() != README)


def problems(paths: list[Path], known: dict[str, int]) -> list[str]:
    out = []
    for path in paths:
        rel = path.relative_to(ROOT).as_posix()
        parts = path.relative_to(FRAGMENTS).parts
        if len(parts) != 2 or path.suffix != ".md":
            out.append(f"{rel}: put it at docs/works-now/<section>/<name>.md")
            continue
        if parts[0] not in known:
            out.append(f"{rel}: no section {parts[0]!r}; one of {', '.join(known)}")
        text = path.read_text(encoding="utf-8")
        lines = text.splitlines()
        if not lines or not lines[0].startswith("- "):
            out.append(f"{rel}: starts with a bullet, '- '")
        for n, line in enumerate(lines, 1):
            if not ((line.startswith("- ") and line[2:].strip()) or (line.startswith("  ") and line.strip())):
                out.append(f"{rel}:{n}: only bullets ('- ' and some text) and their continuation lines (two spaces), no blank lines")
                break
    return out


def fold(roadmap: str, paths: list[Path]) -> str:
    """`roadmap` with each fragment appended to its section, a blank line before it."""
    known = sections(roadmap)
    lines = roadmap.splitlines()
    inserts: dict[int, list[str]] = {}
    for path in paths:
        at = known[path.relative_to(FRAGMENTS).parts[0]]
        inserts.setdefault(at, []).extend(path.read_text(encoding="utf-8").rstrip("\n").splitlines())
    out: list[str] = []
    for i, line in enumerate([*lines, None]):
        if i in inserts:
            # Trailing blank lines of the section stay after the new bullets.
            blanks = 0
            while out and out[-1] == "":
                out.pop()
                blanks += 1
            out.extend(["", *inserts[i], *([""] * max(blanks, 1))])
        if line is not None:
            out.append(line)
    return "\n".join(out) + "\n"


def main(argv: list[str]) -> int:
    if argv not in (["check"], ["fold"]):
        print(__doc__, file=sys.stderr)
        return 2
    roadmap = ROADMAP.read_text(encoding="utf-8")
    paths = fragments()
    found = problems(paths, sections(roadmap))
    for problem in found:
        print(problem, file=sys.stderr)
    if found:
        return 1
    if argv == ["fold"] and paths:
        ROADMAP.write_text(fold(roadmap, paths), encoding="utf-8")
        for path in paths:
            path.unlink()
        for folder in sorted({p.parent for p in paths}, reverse=True):
            if not any(folder.iterdir()):
                folder.rmdir()
        print(f"folded {len(paths)} file(s) into ROADMAP.md")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

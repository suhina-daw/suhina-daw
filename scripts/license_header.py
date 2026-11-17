#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Puts the licence header at the top of every source file git tracks (issue #342).

    python scripts/license_header.py           # add the header where it is missing
    python scripts/license_header.py --check   # list the files without it, change nothing

Source files are the tracked `.rs`, `.py`, `.sh` and `.ps1` files. Each one starts with

    // Copyright (C) 2026 The Suhina contributors
    // SPDX-License-Identifier: GPL-3.0-or-later

(`#` instead of `//` in Python and shell scripts), then an empty line, then the file as before.
A `#!` line stays first, and in Python an encoding line (`# -*- coding: ... -*-`) after it; the
header follows them. A Rust `//!` module doc, a Python docstring and `from __future__` come
after the header. A file that has the header already is left alone, so running the script
again changes nothing; that is what lets a branch merge main and run it once more.

Exceptions, kept in the lists below:

- Ported code (`PORTED`) keeps its original notice and gets only the SPDX line of its own
  licence, `// SPDX-License-Identifier: <licence>`, then an empty line, then the file as
  before. Original copyright lines are never removed or changed. The ported files are the MSFA
  engine files of `devices/fm-synth` (Apache-2.0) and the Signalsmith Stretch port in
  `crates/stretch` (MIT); the other files of those crates are Suhina's own and get the header.
  The VST2 definitions ported from VeSTige into `crates/plugin-host` (ADR 0022) will be listed
  here with `GPL-2.0-or-later`. A newly ported file is added to `PORTED` in the same change.
- `third_party/` is vendored source, left as it is (`SKIP_PREFIXES`).
- Test fixtures (any path with a `fixtures`, `golden`, `testdata` or `tests/data` folder) and
  generated files (a `@generated` or `DO NOT EDIT` marker in their first lines) are left out.

A file that is not listed in `PORTED` but already carries a copyright line of someone else, or
only part of the header, is never rewritten: it is reported, to be sorted out by hand.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(
    subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, check=True, text=True).stdout.strip()
)

COPYRIGHT = "Copyright (C) 2026 The Suhina contributors"
SPDX = "SPDX-License-Identifier:"
OWN_LICENCE = "GPL-3.0-or-later"

EXTENSIONS = {".rs": "//", ".py": "#", ".sh": "#", ".ps1": "#"}

# Ported files and the licence of the code they were ported from.
PORTED = {
    "devices/fm-synth/src/core.rs": "Apache-2.0",
    "devices/fm-synth/src/env.rs": "Apache-2.0",
    "devices/fm-synth/src/lfo.rs": "Apache-2.0",
    "devices/fm-synth/src/tables.rs": "Apache-2.0",
    "devices/fm-synth/src/voice.rs": "Apache-2.0",
    "crates/stretch/src/lib.rs": "MIT",
    "crates/stretch/src/stft.rs": "MIT",
    # The VST2 interface definitions ported from VeSTige as Ardour distributes it, at the
    # commit ADR 0022 pins, with its notice kept (the one named clean-room exception).
    "crates/plugin-host/src/vst2/vestige.rs": "GPL-2.0-or-later",
}

SKIP_PREFIXES = ("third_party/",)
FIXTURE_DIRS = ("fixtures", "golden", "testdata")
GENERATED_MARKS = ("@generated", "DO NOT EDIT")
# How far into a file a generated marker or an existing notice is looked for.
HEAD_LINES = 12

ENCODING = re.compile(r"^#.*coding[:=]")


def skip_reason(path: str) -> str | None:
    """Why `path` gets no header, or None when it gets one."""
    if path.startswith(SKIP_PREFIXES):
        return "vendored (third_party/)"
    parts = path.split("/")[:-1]
    if any(p in FIXTURE_DIRS for p in parts) or "tests/data/" in path:
        return "test fixture"
    return None


def wanted(path: str) -> list[str] | None:
    """The header lines `path` should start with, or None when it is not a source file."""
    mark = EXTENSIONS.get(Path(path).suffix)
    if mark is None:
        return None
    licence = PORTED.get(path)
    if licence is not None:
        return [f"{mark} {SPDX} {licence}"]
    return [f"{mark} {COPYRIGHT}", f"{mark} {SPDX} {OWN_LICENCE}"]


def preamble(lines: list[str], suffix: str) -> int:
    """How many lines stay above the header: a `#!` line, and in Python an encoding line."""
    n = 0
    if lines and lines[0].startswith("#!") and not lines[0].startswith("#!["):
        n = 1
    if suffix == ".py" and len(lines) > n and ENCODING.match(lines[n]):
        n += 1
    return n


def apply(path: str, text: str) -> tuple[str, str]:
    """The file with its header, and what was done: "ok", "added", "generated" or a problem."""
    header = wanted(path)
    if header is None:
        return text, "ok"
    bom = "\ufeff" if text.startswith("\ufeff") else ""
    body = text[len(bom) :]
    newline = "\r\n" if "\r\n" in body else "\n"
    lines = body.split(newline)
    n = preamble(lines, Path(path).suffix)
    if lines[n : n + len(header)] == header:
        return text, "ok"
    head = lines[:HEAD_LINES]
    if any(m in line for line in head for m in GENERATED_MARKS):
        return text, "generated"
    if any(SPDX in line for line in head):
        return text, "has an SPDX line that is not the expected one"
    if path not in PORTED and any(
        "Copyright" in line and "Suhina contributors" not in line for line in head
    ):
        return text, "carries another copyright notice; list it in PORTED or add the header by hand"
    if any(COPYRIGHT in line for line in head):
        return text, "has part of the header"
    rest = lines[n:]
    if rest == [""] or not rest:
        new = lines[:n] + header + [""]
    else:
        new = lines[:n] + header + [""] + rest
    return bom + newline.join(new), "added"


def tracked() -> list[str]:
    patterns = [f"*{ext}" for ext in EXTENSIONS]
    out = subprocess.run(
        ["git", "ls-files", "-z", "--", *patterns], cwd=ROOT, capture_output=True, check=True, text=True
    ).stdout
    return sorted(p for p in out.split("\0") if p)


def main(argv: list[str]) -> int:
    check = "--check" in argv
    missing: list[str] = []
    problems: list[str] = []
    counts = {"added": 0, "ok": 0, "skipped": 0, "generated": 0}
    for path in tracked():
        if skip_reason(path) is not None:
            counts["skipped"] += 1
            continue
        file = ROOT / path
        if not file.is_file():
            continue
        text = file.read_text(encoding="utf-8")
        new, what = apply(path, text)
        if what in ("ok", "generated"):
            counts[what] += 1
        elif what == "added":
            missing.append(path)
            counts["added"] += 1
            if not check:
                file.write_text(new, encoding="utf-8", newline="")
        else:
            problems.append(f"{path}: {what}")
    for p in problems:
        print(p)
    if check:
        for path in missing:
            print(f"{path}: no licence header (python scripts/license_header.py adds it)")
        return 1 if missing or problems else 0
    print(
        f"{counts['added']} headers added, {counts['ok']} already there, "
        f"{counts['skipped']} skipped, {counts['generated']} generated"
    )
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

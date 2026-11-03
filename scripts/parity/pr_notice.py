# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Remind a pull request that changes code to keep the parity measurement current.

A pull request that adds, changes or removes something a user can do updates the coverage in
`scripts/parity/score.py` (and `devices.json` for built-in effects and instruments) in the same
change, or says `Parity: no change` with the reason in its description (docs/workflow.md ›
Parity). This script is the CI side of that rule, and only a notice for now: it prints a GitHub
Actions warning and always exits 0.

    python scripts/parity/pr_notice.py --base HEAD^1 --body-file body.txt
    git diff --name-only BASE HEAD | python scripts/parity/pr_notice.py --files - --body-file body.txt

The workflow passes the pull request's description through a file, never on the command line, so
that nothing in it is read as shell.
"""

import argparse
import subprocess
import sys
from pathlib import Path

# Where a change in what Suhina does lives.
CODE = ("crates/", "devices/", "apps/")
# The recorded measurement a feature change updates.
MEASUREMENT = ("scripts/parity/score.py", "scripts/parity/devices.json")
# The line a pull request that changes no feature writes, compared without regard to case.
NO_CHANGE = "parity: no change"


def notice(files: list[str], body: str) -> str | None:
    """The warning to print, or None when the pull request is fine."""
    code = sorted(f for f in files if f.startswith(CODE))
    if not code:
        return None
    if any(f in MEASUREMENT for f in files):
        return None
    if NO_CHANGE in body.lower():
        return None
    shown = ", ".join(code[:5]) + (f" and {len(code) - 5} more" if len(code) > 5 else "")
    return (
        f"This pull request changes code ({shown}) but not scripts/parity/score.py or "
        "scripts/parity/devices.json. If it adds, changes or removes something a user can do, "
        "update the coverage there (the report's numbers are regenerated on main after the "
        "merge); otherwise write `Parity: no change` and the reason in the description "
        "(docs/workflow.md › Parity)."
    )


def changed_files(base: str) -> list[str]:
    out = subprocess.run(
        ["git", "diff", "--name-only", base, "HEAD"],
        capture_output=True,
        text=True,
        check=False,
    )
    if out.returncode != 0:
        print(f"::notice::Could not list the changed files: {out.stderr.strip()}")
        return []
    return [line for line in out.stdout.splitlines() if line]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--base", help="the commit to diff HEAD against")
    source.add_argument("--files", help="a file listing the changed paths, one per line; - for stdin")
    parser.add_argument("--body-file", help="the pull request's description")
    args = parser.parse_args(argv)

    if args.files is not None:
        text = sys.stdin.read() if args.files == "-" else Path(args.files).read_text("utf-8")
        files = [line.strip() for line in text.splitlines() if line.strip()]
    else:
        files = changed_files(args.base)
    body = ""
    if args.body_file:
        try:
            body = Path(args.body_file).read_text("utf-8", errors="replace")
        except OSError:
            body = ""

    message = notice(files, body)
    if message is None:
        print("The parity measurement is updated, not needed, or marked `Parity: no change`.")
    else:
        print(f"::warning title=Parity measurement::{message}")
    # A notice, not a gate (issue #332): never fail the job.
    return 0


if __name__ == "__main__":
    sys.exit(main())

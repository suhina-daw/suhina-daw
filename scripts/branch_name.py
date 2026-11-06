# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Check a pull request's branch name against the rule in docs/workflow.md › Branch names.

A branch is named `<kind>/<short-description>`: `feature/` for something a user can do, `fix/` for
a bug, `docs/` for documentation and `chore/` for repository, build and release work; the
description is two to five lowercase words joined by hyphens (`feature/piano-roll-workflow`).

    python scripts/branch_name.py <branch>

Prints a GitHub warning annotation when the name does not follow the rule and always exits 0:
the check only reminds, so a branch opened before the rule can still merge
(`.github/workflows/branch-name.yml`).
"""

from __future__ import annotations

import re
import sys

KINDS = ("feature", "fix", "docs", "chore")
WORD = r"[a-z0-9]+"
PATTERN = re.compile(rf"^(?:{'|'.join(KINDS)})/{WORD}(?:-{WORD}){{1,4}}$")


def problem(branch: str) -> str | None:
    """What is wrong with `branch`, or `None` when it follows the rule."""
    kind, _, description = branch.partition("/")
    if kind not in KINDS:
        return f"starts with `{kind}/`; use one of " + ", ".join(f"`{k}/`" for k in KINDS)
    if not PATTERN.match(branch):
        return f"has `{description}` after `{kind}/`; use 2 to 5 lowercase words joined by hyphens"
    return None


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(__doc__)
        return 0
    branch = argv[1]
    found = problem(branch)
    if found is None:
        print(f"`{branch}` follows the branch name rule.")
    else:
        # A warning annotation, never a failure (see the docstring).
        print(f"::warning title=Branch name::`{branch}` {found} (docs/workflow.md › Branch names).")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

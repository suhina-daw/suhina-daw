# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Check that Suhina's updater still downloads its builds from this project's repository.

    python scripts/updater_source.py               # the current commit
    python scripts/updater_source.py --ref <rev>   # any commit; its files are only read

The updater installs and runs what it downloads, so a change to where it looks must never pass
unnoticed (`docs/spec/updates.md` › Where builds come from). This reads the files through Git and
fails when `crates/update/src/github.rs` no longer holds the pinned lines below, or when a watched
file holds a URL outside this repository. A checkout's own files are never run, so a pull request's
commit can be checked from `main` (`.github/workflows/updater-source.yml`).

Text is all it reads: a URL built from pieces at run time is not seen. That is what the code owner's
review of the same paths is for (`.github/CODEOWNERS`).

Moving the project to another repository changes the lines here in the same pull request; the check
that runs from `main` stays red on that pull request, and the code owner decides.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from urllib.parse import urlsplit

SOURCE = "crates/update/src/github.rs"

# Exact lines of SOURCE: the repository, the two addresses asked for builds, the release's page, and
# the names that pick a package out of what GitHub lists.
PINNED = (
    'pub const REPOSITORY: &str = "suhina-daw/suhina-daw";',
    'pub const NIGHTLY_URL: &str = "https://api.github.com/repos/suhina-daw/suhina-daw/releases/tags/nightly";',
    'pub const NIGHTLY_PAGE: &str = "https://github.com/suhina-daw/suhina-daw/releases/tag/nightly";',
    'pub const ARTIFACTS_URL: &str = "https://api.github.com/repos/suhina-daw/suhina-daw/actions/artifacts?per_page=100";',
    'let wanted = format!("suhina-nightly-{platform}.");',
    'let suffix = format!("-{platform}");',
    '.strip_prefix("suhina-pr")',
)

# Everything that decides what is downloaded and from where. Keep in step with .github/CODEOWNERS.
WATCHED = (
    "crates/update",
    "crates/ui/src/app/updates.rs",
    "crates/ui/src/settings/update_settings.rs",
)

# Where a URL in a watched file may point. `{REPOSITORY}` is the constant above, formatted in.
ALLOWED_PREFIXES = (
    "https://api.github.com/repos/suhina-daw/suhina-daw/",
    "https://github.com/suhina-daw/suhina-daw/",
    "https://api.github.com/repos/{REPOSITORY}/",
    "https://github.com/{REPOSITORY}/",
)
# The repository's own page.
ALLOWED_EXACT = ("https://github.com/suhina-daw/suhina-daw",)
# Reserved by RFC 2606, so it never resolves: the tests' example answers.
TEST_DOMAIN = ".invalid"

# Any scheme, with or without a rest: a bare `https://` is what joining pieces starts from.
URL = re.compile(r"""[a-zA-Z][a-zA-Z0-9+.-]*://[^\s"'`<>\\()\[\]]*""")

GIT_FILE = "100"


def url_allowed(url: str) -> bool:
    """Whether a URL found in a watched file stays inside this repository, or is a test's."""
    if ".." in url:
        return False
    try:
        host = urlsplit(url).hostname or ""
    except ValueError:
        return False
    return host.endswith(TEST_DOMAIN) or url in ALLOWED_EXACT or url.startswith(ALLOWED_PREFIXES)


def problems(files: dict[str, str]) -> list[str]:
    """What is wrong with the watched files, given as their paths and text."""
    found = []
    source = files.get(SOURCE)
    if source is None:
        found.append(f"{SOURCE} is gone: the updater's source cannot be checked")
    else:
        lines = {line.strip() for line in source.splitlines()}
        found += [f"{SOURCE} no longer has the line: {pin}" for pin in PINNED if pin not in lines]
    for path, text in sorted(files.items()):
        for number, line in enumerate(text.splitlines(), start=1):
            found += [
                f"{path}:{number} points outside this repository: {match.group()}"
                for match in URL.finditer(line)
                if not url_allowed(match.group())
            ]
    return found


def read_commit(ref: str, cwd: str | None = None) -> tuple[dict[str, str], list[str]]:
    """The watched files at `ref`, and a problem for each entry that is not a plain file."""
    listing = subprocess.run(
        ["git", "ls-tree", "-r", "-z", ref, "--", *WATCHED],
        cwd=cwd, capture_output=True, check=True,
    ).stdout.decode("utf-8", "replace")
    files, found = {}, []
    for entry in filter(None, listing.split("\0")):
        meta, path = entry.split("\t", 1)
        mode, _kind, blob = meta.split()
        # A symbolic link or a submodule would hide the text this check reads.
        if not mode.startswith(GIT_FILE):
            found.append(f"{path} is not a plain file (mode {mode})")
            continue
        text = subprocess.run(
            ["git", "cat-file", "blob", blob], cwd=cwd, capture_output=True, check=True,
        ).stdout
        files[path] = text.decode("utf-8", "replace")
    return files, found


def annotation(message: str) -> str:
    """A workflow's error line; the message comes from a pull request, so it stays one line of data."""
    escaped = message.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
    return f"::error title=Updater source::{escaped}"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--ref", default="HEAD", help="the commit to read (default: HEAD)")
    args = parser.parse_args(argv)
    try:
        files, found = read_commit(args.ref)
    except subprocess.CalledProcessError as error:
        print(annotation(f"could not read {args.ref}: {error.stderr.decode('utf-8', 'replace').strip()}"))
        return 1
    found += problems(files)
    if not found:
        print(f"The updater downloads from this repository ({len(files)} files read at {args.ref}).")
        return 0
    for problem in found:
        print(annotation(problem))
    print(annotation("This changes where Suhina's updater downloads builds from (docs/spec/updates.md › Where builds come from)."))
    return 1


if __name__ == "__main__":
    sys.exit(main())

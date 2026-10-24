#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Keeps long test modules out of Rust source files (issue #768, part 5).

    python scripts/inline_tests.py --check      # list inline test modules over the limit
    python scripts/inline_tests.py move FILE... # move each file's long test module out

An agent reading a source file reads its inline `#[cfg(test)] mod tests { ... }` too, so a
test module over `LIMIT` lines lives in a file of its own beside the source:
`foo.rs` keeps

    #[cfg(test)]
    #[path = "foo_tests.rs"]
    mod tests;

and `foo_tests.rs` holds the module's body under the licence header, formatted by `rustfmt`
(which re-indents the code and leaves string literals as they are). The tests themselves do
not change. `move` does exactly that for the first inline test module over
the limit in each file it is given (`lib.rs` and `main.rs` give `lib_tests.rs` and
`main_tests.rs`), and refuses a file whose test file exists already.

`--check` fails on any tracked `src/` file with an inline test module over the limit, except
those in `WAITING`: files that open pull requests are still changing, so that moving their tests
now would make every one of those pull requests conflict. Each is moved in a pull request of its
own once they have merged, and its entry is removed then; an entry for a file that no longer
has a long module fails the check too, so the list cannot go stale.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path
from typing import NamedTuple

ROOT = Path(
    subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, check=True, text=True).stdout.strip()
)

# Longest inline test module allowed, in lines, from `#[cfg(test)]` to its closing brace.
LIMIT = 150

HEADER = "// Copyright (C) 2026 The Suhina contributors\n// SPDX-License-Identifier: GPL-3.0-or-later\n"

# Files whose tests wait for the open pull requests that change them (see the docstring).
WAITING = {
    "crates/automation/src/confine.rs",
    "crates/device-sdk/src/lib.rs",
    "crates/model/src/lib.rs",
}

OPENING = re.compile(r"^(\s*)((?:pub(?:\([\w:]+\))? )?)mod (\w+) \{\s*$")
ATTRIBUTE = re.compile(r"^\s*#\[.*\]\s*$")


class Module(NamedTuple):
    """An inline test module: the indices of its `#[cfg(test)]` line, its `mod name {` line and
    its closing brace's line, its indent, its visibility (`""`, `"pub "`, `"pub(crate) "`) and
    its name."""

    start: int
    opening: int
    end: int
    indent: str
    visibility: str
    name: str

    def lines(self) -> int:
        return self.end - self.start + 1


def word_before(text: str, i: int) -> bool:
    """Whether the character before `i` is part of a word, so `text[i:]` is inside one."""
    return i > 0 and (text[i - 1].isalnum() or text[i - 1] == "_")


class Lexed(NamedTuple):
    """What a scan of Rust source finds outside comments and string, raw string, byte string and
    character literals: the lines that start in code, and each brace, as (line, `{` or `}`)."""

    code_lines: set[int]
    braces: list[tuple[int, str]]


def lex(text: str) -> Lexed:
    """Scans `text` once, skipping comments (nested block comments included) and literals."""
    code_lines, braces = {0}, []
    i, line = 0, 0
    while i < len(text):
        c = text[i]
        if c == "\n":
            line += 1
            code_lines.add(line)
        elif text.startswith("//", i):
            end = text.find("\n", i)
            i = len(text) if end < 0 else end
            continue
        elif text.startswith("/*", i):
            nested, i = 1, i + 2
            while i < len(text) and nested:
                if text.startswith("/*", i):
                    nested, i = nested + 1, i + 2
                elif text.startswith("*/", i):
                    nested, i = nested - 1, i + 2
                else:
                    line += text[i] == "\n"
                    i += 1
            continue
        elif not word_before(text, i) and (raw := re.match(r'b?r(#*)"', text[i:])):
            close = text.find('"' + raw.group(1), i + raw.end())
            close = len(text) if close < 0 else close
            line += text.count("\n", i, close)
            i = close + 1 + len(raw.group(1))
            continue
        elif c == '"':
            i += 1
            while i < len(text) and text[i] != '"':
                step = 2 if text[i] == "\\" else 1
                line += text.count("\n", i, i + step)
                i += step
        elif c == "'" and (char := re.match(r"'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]+\}|.)|[^\\'\n])'", text[i:])):
            i += char.end()
            continue
        elif c in "{}":
            braces.append((line, c))
        i += 1
    return Lexed(code_lines, braces)


def closing_line(lexed: Lexed, opening: int) -> int | None:
    """The index of the line where the block opened by the last `{` on line `opening` closes."""
    starts = [k for k, (line, brace) in enumerate(lexed.braces) if line == opening and brace == "{"]
    if not starts:
        return None
    depth = 0
    for line, brace in lexed.braces[starts[-1] :]:
        depth += 1 if brace == "{" else -1
        if depth == 0:
            return line
    return None


def cfg_test_end(lines: list[str], i: int) -> int | None:
    """The index of the line where the `#[cfg(test)]` attribute starting on line `i` ends, which
    is `i` unless its whitespace puts it over several lines; a trailing `//` comment is ignored; `None` when it is not that attribute."""
    if not lines[i].lstrip().startswith("#[cfg("):
        return None
    joined = ""
    for end in range(i, len(lines)):
        joined += lines[end].split("//", 1)[0].strip()
        if joined.endswith("]"):
            return end if "".join(joined.split()) == "#[cfg(test)]" else None
    return None


def test_modules(text: str) -> list[Module]:
    """Each inline test module in `text`: a `#[cfg(test)]` attribute (it may run over several lines), then any more attributes, each
    on a line of its own, then `mod name {`, which closes at its matching brace. Only lines that
    start in code count, so text in a block comment or a raw string that looks like a module is
    left alone."""
    lines = text.split("\n")
    lexed = lex(text)
    found = []
    for i, line in enumerate(lines):
        attribute_end = cfg_test_end(lines, i)
        if attribute_end is None or i not in lexed.code_lines:
            continue
        opening = attribute_end + 1
        while opening < len(lines) and ATTRIBUTE.match(lines[opening]) and not OPENING.match(lines[opening]):
            opening += 1
        matched = OPENING.match(lines[opening]) if opening < len(lines) else None
        if not matched or any(k not in lexed.code_lines for k in range(i, opening + 1)):
            continue
        end = closing_line(lexed, opening)
        if end is not None:
            indent, visibility, name = matched.groups()
            found.append(Module(i, opening, end, indent, visibility, name))
    return found


def long_module(text: str) -> Module | None:
    """The first inline test module of `text` over `LIMIT` lines, if any."""
    return next((m for m in test_modules(text) if m.lines() > LIMIT), None)


def moved(text: str, test_file: str) -> tuple[str, str] | None:
    """`text` with its long test module replaced by a `#[path]` module, which keeps the module's
    attributes and visibility, and the test file's text, or `None` when it has no long module."""
    module = long_module(text)
    if module is None:
        return None
    lines = text.split("\n")
    # The body keeps its indent: rustfmt takes it off the code, and leaves string literals that
    # run over several lines exactly as they are, as a hand-made dedent would not.
    body = lines[module.opening + 1 : module.end]
    while body and not body[0].strip():
        body.pop(0)
    while body and not body[-1].strip():
        body.pop()
    indent = module.indent
    stub = [
        *lines[module.start : module.opening],
        f'{indent}#[path = "{test_file}"]',
        f"{indent}{module.visibility}mod {module.name};",
    ]
    source = "\n".join(lines[: module.start] + stub + lines[module.end + 1 :])
    return source, HEADER + "\n" + "\n".join(body) + "\n"


def test_file_name(path: Path) -> str:
    return f"{path.stem}_tests.rs"


def tracked_sources() -> list[str]:
    out = subprocess.run(["git", "ls-files", "*.rs"], cwd=ROOT, capture_output=True, check=True, text=True).stdout
    return [p for p in out.split() if "/src/" in f"/{p}" and not p.startswith("third_party/")]


def check(paths: list[str]) -> list[str]:
    """What `--check` reports for `paths` (relative to the repository root)."""
    problems = []
    long = set()
    for path in paths:
        module = long_module((ROOT / path).read_text(encoding="utf-8"))
        if module is None:
            continue
        long.add(path)
        if path not in WAITING:
            problems.append(
                f"{path}:{module.start + 1}: an inline test module of {module.lines()} lines (over {LIMIT}); "
                f"move it out with `python scripts/inline_tests.py move {path}`"
            )
    for path in sorted(WAITING - long):
        problems.append(f"{path} is in WAITING but has no inline test module over {LIMIT} lines; remove its entry")
    return problems


def move(path: Path) -> str:
    """Moves `path`'s long test module out; returns what was done."""
    test_path = path.with_name(test_file_name(path))
    if test_path.exists():
        return f"{path}: {test_path.name} exists already, left as it is"
    result = moved(path.read_text(encoding="utf-8"), test_path.name)
    if result is None:
        return f"{path}: no inline test module over {LIMIT} lines"
    source, tests = result
    path.write_text(source, encoding="utf-8")
    test_path.write_text(tests, encoding="utf-8")
    subprocess.run(["rustfmt", "--edition", "2024", str(test_path)], check=True)
    return f"{path}: tests moved to {test_path.name}"


def main(argv: list[str]) -> int:
    if argv[:1] == ["--check"]:
        problems = check(tracked_sources())
        for problem in problems:
            print(problem)
        return 1 if problems else 0
    if argv[:1] == ["move"] and len(argv) > 1:
        for name in argv[1:]:
            print(move(Path(name)))
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

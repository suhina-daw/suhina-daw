# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Tests for the skill checks of `scripts/docs.py check` (#768 › 3), on the repository's own files.
python3 -m unittest discover -s scripts -p 'test_*.py'
"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import docs  # noqa: E402

FILES = docs.tracked_files()
CRATES = docs.crate_dirs()


def check(text: str) -> list[str]:
    return [f"{f.code} {f.message}" for f in docs.check_skill("x/SKILL.md", text, FILES, CRATES)]


class SkillChecks(unittest.TestCase):
    def test_paths_and_symbols_that_exist_pass(self):
        text = (
            "Edit `crates/session/src/commands.rs` and `devices/auto-pan/` with "
            "`suhina_session::commands::commands`, `suhina_test_support::ProjectBuilder`, "
            "`suhina_device_api::conformance::test_signal` and `suhina_engine::render_offline()`."
        )
        self.assertEqual(check(text), [])

    def test_a_path_that_is_gone_fails(self):
        self.assertEqual(
            check("See `crates/session/src/no_such_file.rs`."),
            ["SKILL_PATH crates/session/src/no_such_file.rs: no such file or folder in the repository"],
        )

    def test_a_symbol_that_is_gone_fails(self):
        found = check("Call `suhina_engine::no_such_function` from `suhina_session::no_such_module::commands`.")
        self.assertEqual([f.split(" ")[0] for f in found], ["SKILL_SYMBOL", "SKILL_SYMBOL"])

    def test_an_unknown_crate_fails(self):
        self.assertEqual(check("`suhina_nothing::Thing`"), ["SKILL_SYMBOL suhina_nothing::Thing: no crate suhina_nothing in the workspace"])

    def test_placeholders_commands_and_code_blocks_are_not_checked(self):
        text = "`devices/<name>/panel.json`, `cargo test -p suhina-ui`\n```sh\ncat crates/none.rs\n```\n"
        self.assertEqual(check(text), [])

    def test_every_skill_in_the_repository_passes(self):
        self.assertEqual([f"{f.rel}:{f.line} {f.message}" for f in docs.check_skills(FILES)], [])


if __name__ == "__main__":
    unittest.main()

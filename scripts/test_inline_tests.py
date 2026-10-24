# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Tests for scripts/inline_tests.py: finding long inline test modules and moving them out."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import inline_tests  # noqa: E402


def source(test_lines: int, name: str = "tests") -> str:
    body = "\n".join(f"    // line {n}" for n in range(test_lines - 3))
    return f'fn code() {{}}\n\n#[cfg(test)]\nmod {name} {{\n{body}\n}}\n\nfn after() {{\n    let s = "x\n  y";\n}}\n'


class InlineTests(unittest.TestCase):
    def test_a_module_over_the_limit_is_long_and_one_at_it_is_not(self):
        self.assertIsNone(inline_tests.long_module(source(inline_tests.LIMIT)))
        self.assertEqual(inline_tests.test_modules(source(inline_tests.LIMIT))[0].lines(), inline_tests.LIMIT)
        self.assertIsNotNone(inline_tests.long_module(source(inline_tests.LIMIT + 1)))

    def test_attributes_between_cfg_test_and_the_module_stay_on_it(self):
        text = source(400).replace("#[cfg(test)]\n", "#[cfg(test)]\n#[allow(dead_code)]\n")
        result = inline_tests.moved(text, "foo_tests.rs")
        assert result is not None
        self.assertIn('#[cfg(test)]\n#[allow(dead_code)]\n#[path = "foo_tests.rs"]\nmod tests;\n', result[0])
        self.assertEqual(inline_tests.test_modules(source(400).replace("#[cfg(test)]\n", "#[cfg(test)]\nfn x() {}\n")), [])

    def test_a_cfg_test_attribute_over_several_lines_still_starts_a_module(self):
        text = "#[cfg(\n    test\n)]\nmod tests {\n" + "    fn t() {}\n" * 5 + "}\n"
        modules = inline_tests.test_modules(text)
        self.assertEqual([(m.start, m.opening, m.name) for m in modules], [(0, 3, "tests")])
        self.assertEqual(inline_tests.test_modules("#[cfg(\n    not(test)\n)]\nmod tests {\n}\n"), [])

    def test_a_trailing_comment_after_cfg_test_is_ignored(self):
        text = "#[cfg(test)] // the tests\nmod tests {\n    fn t() {}\n}\n"
        self.assertEqual([m.name for m in inline_tests.test_modules(text)], ["tests"])
        text = "#[cfg(\n    test // only tests\n)]\nmod tests {\n}\n"
        self.assertEqual([m.name for m in inline_tests.test_modules(text)], ["tests"])

    def test_a_public_module_stays_public(self):
        result = inline_tests.moved(source(200).replace("mod tests {", "pub(crate) mod tests {"), "foo_tests.rs")
        assert result is not None
        self.assertIn('#[path = "foo_tests.rs"]\npub(crate) mod tests;\n', result[0])

    def test_a_module_inside_a_block_comment_or_a_raw_string_is_not_one(self):
        module = source(200).split("\n\nfn after()")[0]
        commented = f"fn code() {{}}\n/*\n{module}\n*/\n"
        quoted = f'const TEXT: &str = r#"\n{module}\n"#;\n'
        for text in [commented, quoted]:
            self.assertEqual(inline_tests.test_modules(text), [], text[:40])
            self.assertIsNone(inline_tests.moved(text, "foo_tests.rs"))

    def test_braces_in_strings_comments_and_chars_do_not_end_the_module(self):
        tricky = [
            '    const RAW: &str = r#"',
            "}",
            '"#;',
            '    const S: &str = "} \\" {";',
            "    // }",
            "    /* } /* } */ } */",
            "    const C: char = '}';",
            "    fn f<'a>(x: &'a str) -> &'a str { x }",
            '    const E: &str = "a \\',
            '        b";',
        ]
        text = source(200).replace("    // line 0\n", "\n".join(tricky) + "\n")
        (module,) = inline_tests.test_modules(text)
        lines = text.split("\n")
        self.assertEqual(lines[module.end], "}")
        self.assertEqual(lines[module.end - 1], f"    // line {200 - 4}")

    def test_moving_leaves_a_path_module_and_the_body_under_the_header(self):
        text = source(200, "checks")
        result = inline_tests.moved(text, "foo_tests.rs")
        assert result is not None
        new_source, tests = result
        self.assertIn('#[cfg(test)]\n#[path = "foo_tests.rs"]\nmod checks;\n', new_source)
        self.assertTrue(new_source.startswith("fn code() {}\n"))
        self.assertIn('fn after() {\n    let s = "x\n  y";\n}', new_source)
        self.assertNotIn("// line", new_source)
        self.assertTrue(tests.startswith(inline_tests.HEADER + "\n    // line 0\n"))
        self.assertTrue(tests.endswith(f"    // line {200 - 4}\n"))

    def test_a_file_without_a_long_module_is_not_moved(self):
        self.assertIsNone(inline_tests.moved(source(20), "foo_tests.rs"))

    def test_lib_and_main_get_a_tests_file_named_after_them(self):
        self.assertEqual(inline_tests.test_file_name(Path("crates/x/src/lib.rs")), "lib_tests.rs")
        self.assertEqual(inline_tests.test_file_name(Path("crates/x/src/view/menu.rs")), "menu_tests.rs")


if __name__ == "__main__":
    unittest.main()

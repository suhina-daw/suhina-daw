# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Tests for the structure checks of `scripts/docs.py check` (docs/README.md): one document that
passes and one that fails for each rule.
python3 -m unittest discover -s scripts -p 'test_*.py'
"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import docs  # noqa: E402


def codes(findings):
    return [f.code for f in findings]


def adr(number: str, status: str = "accepted", **fields: str) -> docs.Doc:
    d = docs.Doc(path=docs.ROOT / f"docs/decisions/{number}-x.md", title=number, has_front_matter=True)
    d.fields = {"type": "decision", "status": status, "built": "yes", **fields}
    return d


class AdrNumbers(unittest.TestCase):
    def test_unique_numbers_pass(self):
        files = ["docs/decisions/0001-a.md", "docs/decisions/0002-b.md", "docs/spec/0001-c.md"]
        self.assertEqual(docs.check_adr_numbers(files), [])

    def test_a_shared_number_fails_for_both_files(self):
        files = ["docs/decisions/0028-a.md", "docs/decisions/0028-b.md"]
        found = docs.check_adr_numbers(files)
        self.assertEqual(codes(found), ["ADR_NUMBER", "ADR_NUMBER"])
        self.assertEqual({f.rel for f in found}, set(files))


class Numbering(unittest.TestCase):
    def test_a_list_in_order_passes(self):
        text = "1. **One.** Text\n   more.\n\n2. **Two.**\n3. **Three.**\n"
        self.assertEqual(docs.check_numbering("a.md", text), [])

    def test_a_repeat_and_a_gap_fail(self):
        text = "1. **One.**\n2. **Two.**\n2. **Again.**\n4. **Gap.**\n"
        found = docs.check_numbering("a.md", text)
        self.assertEqual([f.line for f in found], [3, 4])

    def test_one_slip_does_not_flag_the_items_after_it(self):
        self.assertEqual(len(docs.check_numbering("a.md", "1. **A**\n3. **B**\n4. **C**\n")), 1)

    def test_a_list_must_start_at_one(self):
        self.assertEqual(codes(docs.check_numbering("a.md", "2. **A**\n3. **B**\n")), ["NUMBERING"])

    def test_a_heading_starts_a_new_list(self):
        text = "1. **A**\n2. **B**\n\n## Next\n\n1. **C**\n"
        self.assertEqual(docs.check_numbering("a.md", text), [])

    def test_a_list_that_does_not_start_with_bold_text_is_left_alone(self):
        self.assertEqual(docs.check_numbering("a.md", "1. Open it\n2. Close it\n4. **Done**\n"), [])

    def test_a_plain_item_inside_a_bold_list_counts(self):
        self.assertEqual(docs.check_numbering("a.md", "1. **A**\n2. Plain\n3. **C**\n"), [])

    def test_code_blocks_are_skipped(self):
        text = "1. **A**\n   ```\n1. **not an item**\n   ```\n2. **B**\n"
        self.assertEqual(docs.check_numbering("a.md", text), [])


class DecisionRefs(unittest.TestCase):
    ADRS = {
        "0004": [("docs/decisions/0004-built-in-devices.md", {1, 2, 3}, "accepted")],
        "0007": [("docs/decisions/0007-old.md", {1}, "superseded")],
        "0028": [
            ("docs/decisions/0028-ara-hosting.md", {1, 2, 3}, "accepted"),
            ("docs/decisions/0028-mcp-in-the-running-app.md", {1, 2, 3, 4, 5}, "accepted"),
        ],
    }

    def refs(self, text):
        return docs.check_decision_refs("a.md", text, self.ADRS)

    def test_decision_numbers_come_from_the_decision_section(self):
        text = "## Context\n\n1. **Not one.**\n\n## Decision (proposed)\n\n1. **A.**\n   text\n2. **B.**\n\n## Consequences\n\n3. **No.**\n"
        self.assertEqual(docs.decision_numbers(text), {1, 2})

    def test_references_that_resolve_pass(self):
        text = (
            "ADR 0004 › 2, ADR 0004 decision 3, [ADR 0004](../decisions/0004-built-in-devices.md) › 1, "
            "ADR 0004 §1, ADR 0004 decisions 1 and 3, ADR 0004 › Consequences.\n"
        )
        self.assertEqual(self.refs(text), [])

    def test_a_decision_that_does_not_exist_fails(self):
        self.assertEqual(codes(self.refs("See ADR 0004 › 9.\n")), ["DECISION_REF"])

    def test_each_number_of_a_list_is_checked(self):
        found = self.refs("ADR 0004 decisions 1 and 7.\n")
        self.assertEqual(codes(found), ["DECISION_REF"])
        self.assertIn("decision 7", found[0].message)

    def test_an_adr_that_does_not_exist_fails(self):
        self.assertEqual(codes(self.refs("ADR 0099 decision 1.\n")), ["DECISION_REF"])

    def test_a_link_picks_between_two_adrs_of_one_number(self):
        self.assertEqual(self.refs("[ADR 0028](0028-ara-hosting.md) decision 3\n"), [])
        self.assertEqual(codes(self.refs("[ADR 0028](0028-ara-hosting.md) decision 5\n")), ["DECISION_REF"])
        self.assertEqual(self.refs("ADR 0028 decision 5\n"), [])

    def test_a_superseded_adr_warns(self):
        self.assertEqual(codes(self.refs("ADR 0007 decision 1\n")), ["SUPERSEDED_REF"])

    def test_text_that_names_no_decision_is_left_alone(self):
        self.assertEqual(self.refs("ADR 0004 and ADR 0099 step 3, in 2026-10-18.\n"), [])


class Status(unittest.TestCase):
    def test_a_decision_with_the_new_vocabulary_passes(self):
        self.assertEqual(docs.check_status([adr("0001"), adr("0002", "proposed", built="no")]), [])

    def test_the_old_vocabulary_and_a_missing_built_fail(self):
        d = adr("0001", "current")
        del d.fields["built"]
        self.assertEqual(codes(docs.check_status([d])), ["DECISION_STATUS", "BUILT"])

    def test_a_bad_built_value_fails(self):
        self.assertEqual(codes(docs.check_status([adr("0001", built="mostly")])), ["BUILT"])

    def test_other_types_keep_their_status_and_need_no_built(self):
        spec = docs.Doc(path=docs.ROOT / "docs/spec/a.md", title="a", has_front_matter=True)
        spec.fields = {"type": "spec", "status": "draft"}
        self.assertEqual(docs.check_status([spec]), [])

    def test_superseded_by_and_supersedes_agree(self):
        old = adr("0003", "superseded", superseded_by="0009")
        new = adr("0009", supersedes="[0003]")
        self.assertEqual(docs.check_status([old, new]), [])

    def test_superseded_needs_superseded_by(self):
        self.assertEqual(codes(docs.check_status([adr("0003", "superseded")])), ["SUPERSEDED_BY"])

    def test_the_named_adr_must_list_it_in_supersedes(self):
        old = adr("0003", "superseded", superseded_by="0009")
        self.assertEqual(codes(docs.check_status([old, adr("0009")])), ["SUPERSEDED_BY"])
        self.assertEqual(codes(docs.check_status([old])), ["SUPERSEDED_BY"])

    def test_supersedes_needs_the_old_adr_to_be_superseded(self):
        self.assertEqual(codes(docs.check_status([adr("0003"), adr("0009", supersedes="[0003]")])), ["SUPERSEDES"])
        self.assertEqual(codes(docs.check_status([adr("0009", supersedes="[0003]")])), ["SUPERSEDES"])


class TextRules(unittest.TestCase):
    def test_a_status_line_in_an_adr_fails(self):
        self.assertEqual(codes(docs.check_status_lines("a.md", "# A\n\n- **Status:** accepted\n- **Date:** 2026-10-18\n")), ["STATUS_LINE"])
        self.assertEqual(docs.check_status_lines("a.md", "# A\n\n- **Date:** 2026-10-18\n"), [])

    def test_line_numbers_fail(self):
        for bad in ("`crates/model/src/lib.rs:285`", "`lib.rs:12-14`", "`lib.rs`:285", "see `lib.rs` `:285`"):
            with self.subTest(bad):
                self.assertEqual(codes(docs.check_line_refs("a.md", f"Text {bad} more.\n")), ["LINE_REF"])

    def test_symbols_ports_and_times_pass(self):
        text = "`media::decode_project_audio`, `lib.rs`, `localhost:8080`, 12:30, `Cargo.toml`.\n"
        self.assertEqual(docs.check_line_refs("a.md", text), [])

    def test_code_blocks_may_name_lines(self):
        self.assertEqual(docs.check_line_refs("a.md", "```\nerror at `lib.rs:12`\n```\n"), [])

    def test_sources_may_hold_a_list_of_sources(self):
        text = "## Sources\n\nSeen in:\n\n- [A](https://example.invalid/a):\n  what it says.\n- [B](https://example.invalid/b)\n\n[c]: https://example.invalid/c\n"
        self.assertEqual(docs.check_sources_last("a.md", text), [])

    def test_text_after_the_sources_fails(self):
        base = "## Sources\n\n- [A](https://example.invalid/a)\n\n"
        self.assertEqual(codes(docs.check_sources_last("a.md", base + "A late paragraph.\n")), ["SOURCES_LAST"])
        self.assertEqual(codes(docs.check_sources_last("a.md", base + "## More\n\nText.\n")), ["SOURCES_LAST"])

    def test_a_document_without_sources_passes(self):
        self.assertEqual(docs.check_sources_last("a.md", "# A\n\nText.\n\n## Other\n\nText.\n"), [])

    def test_sources_and_ip_is_another_section(self):
        self.assertEqual(docs.check_sources_last("a.md", "## Sources and IP\n\nText.\n\n- a\n\nMore.\n"), [])


if __name__ == "__main__":
    unittest.main()

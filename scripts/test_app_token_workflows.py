# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Keep GitHub App authentication off the deprecated app-id input."""

from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[1]


class AppTokenWorkflowTests(unittest.TestCase):
    def test_every_app_token_uses_client_id_and_existing_private_key(self):
        count = 0
        for path in (ROOT / '.github/workflows').glob('*.yml'):
            text = path.read_text(encoding='utf-8')
            blocks = re.findall(
                r'(?m)^( +)uses: actions/create-github-app-token@[^\n]+\n'
                r'((?:\1 +[^\n]*\n|\1with:\n)+)', text)
            expected = text.count('uses: actions/create-github-app-token@')
            self.assertEqual(len(blocks), expected, path.name)
            for _, block in blocks:
                count += 1
                with self.subTest(workflow=path.name, call=count):
                    prefix = 'SUHINA_BOT' if path.name in ('open-issue.yml', 'comment-pr.yml') else 'DOCS_BOT'
                    self.assertNotRegex(block, r'(?m)^\s*app-id:')
                    self.assertIn('client-id: ${{ vars.' + prefix + '_CLIENT_ID }}', block)
                    self.assertIn('private-key: ${{ secrets.' + prefix + '_PRIVATE_KEY }}', block)
                    # Owner alone would expand the token to all installed repositories.
                    self.assertNotRegex(block, r'(?m)^\s*owner:')
        self.assertGreaterEqual(count, 6)

    def test_missing_credentials_checks_use_the_same_client_id_variable(self):
        workflows = ROOT / '.github/workflows'
        docs = (workflows / 'docs-generated.yml').read_text(encoding='utf-8')
        self.assertIn("vars.DOCS_BOT_CLIENT_ID != '' && secrets.DOCS_BOT_PRIVATE_KEY != ''", docs)
        for name in ('open-issue.yml', 'comment-pr.yml'):
            text = (workflows / name).read_text(encoding='utf-8')
            self.assertIn("HAS_SUHINA_BOT_CLIENT_ID: ${{ vars.SUHINA_BOT_CLIENT_ID != '' }}", text)
            self.assertIn('["SUHINA_BOT_CLIENT_ID", "SUHINA_BOT_PRIVATE_KEY"]', text)


if __name__ == '__main__':
    unittest.main()

# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Exercise package build steps with a fake cargo, without compiling Rust."""

import os
from pathlib import Path
import shutil
import subprocess
import unittest

ROOT = Path(__file__).resolve().parent.parent


class AsioBuildTests(unittest.TestCase):
    def test_platform_features(self):
        bash = shutil.which("bash")
        if os.name == "nt":
            bash = str(Path(os.environ.get("ProgramFiles", "C:/Program Files")) / "Git/bin/bash.exe")
        self.assertIsNotNone(bash)
        for workflow in ("release.yml", "test-build.yml"):
            source = (ROOT / ".github/workflows" / workflow).read_text(encoding="utf-8")
            build = source.split("      - name: Build\n", 1)[1]
            lines = build.split("        run: |\n", 1)[1].splitlines()
            script = []
            for line in lines:
                if line and not line.startswith("          "):
                    break
                script.append(line[10:])
            for platform, targets in (
                ("Windows", "x86_64-pc-windows-msvc"),
                ("Linux", "x86_64-unknown-linux-gnu"),
                ("macOS", "aarch64-apple-darwin,x86_64-apple-darwin"),
            ):
                with self.subTest(workflow=workflow, platform=platform):
                    probe = 'cargo() { printf "<%s>" "$@"; printf "\\n"; }\n'
                    result = subprocess.run(
                        [bash, "--noprofile", "--norc", "-e", "-c", probe + "\n".join(script)],
                        env=dict(os.environ, RUNNER_OS=platform, TARGETS=targets),
                        capture_output=True, text=True, check=True,
                    )
                    calls = result.stdout.splitlines()
                    self.assertEqual(len(calls), len(targets.split(",")))
                    for call, target in zip(calls, targets.split(",")):
                        self.assertIn(f"<--target><{target}>", call)
                        self.assertIn("<-p><suhina-cli><-p><suhina>", call)
                        if platform == "Windows":
                            self.assertIn("<--features><suhina/asio,suhina-cli/asio>", call)
                        else:
                            self.assertNotIn("<--features>", call)


if __name__ == "__main__":
    unittest.main()

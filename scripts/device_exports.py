#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""Build/package the experimental native-device export and run actual sandbox-host tests."""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["build", "test"])
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    target = Path(os.environ.get("CARGO_TARGET_DIR", root / "target")).resolve()
    subprocess.run(["cargo", "build", "-p", "suhina-plugin-host", "--lib", "--bin", "suhina-plugin-host", "--features", "device-exports"], cwd=root, check=True)
    filename = {"win32": "suhina_device_exports.dll", "darwin": "libsuhina_device_exports.dylib"}.get(sys.platform, "libsuhina_device_exports.so")
    output = target / "device-exports"
    output.mkdir(parents=True, exist_ok=True)
    clap = output / "SuhinaDevices.clap"
    shutil.copy2(target / "debug" / filename, clap)
    print(f"CLAP: {clap}", flush=True)
    if args.action == "test":
        environment = dict(os.environ, SUHINA_TEST_EXPORT_CLAP=str(clap))
        subprocess.run(["cargo", "test", "-p", "suhina-plugin-host", "--features", "device-exports", "--test", "device_exports", "--", "--ignored"], cwd=root, env=environment, check=True)


if __name__ == "__main__":
    main()

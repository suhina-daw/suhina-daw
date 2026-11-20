#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Builds the 32-bit plug-in host, the bridge for 32-bit Windows plug-ins, and puts it beside
the 64-bit one (docs/spec/plugin-hosting.md › 32-bit plug-ins on Windows).

    bridge_host.py                       debug build, into target/debug
    bridge_host.py --release             release build, into target/release
    bridge_host.py --release --to DIR    release build, into DIR (a packaged Suhina's folder)

Suhina finds it as `suhina-plugin-host-32.exe` next to `suhina-plugin-host.exe`. Needs the Rust
target (`rustup target add i686-pc-windows-msvc`) and the MSVC x86 libraries, which the Visual
Studio C++ build tools install with the x64 ones.
"""

import argparse
import pathlib
import shutil
import subprocess
import sys

TARGET = "i686-pc-windows-msvc"
NAME = "suhina-plugin-host-32.exe"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--release", action="store_true", help="build with the release profile")
    parser.add_argument("--to", type=pathlib.Path, help="the folder to copy it into")
    args = parser.parse_args()

    root = pathlib.Path(__file__).resolve().parent.parent
    profile = "release" if args.release else "debug"
    command = ["cargo", "build", "-p", "suhina-plugin-host", "--target", TARGET]
    if args.release:
        command.append("--release")
    print(" ".join(command), flush=True)
    if subprocess.run(command, cwd=root).returncode != 0:
        return 1

    built = root / "target" / TARGET / profile / "suhina-plugin-host.exe"
    folder = args.to or root / "target" / profile
    folder.mkdir(parents=True, exist_ok=True)
    shutil.copy2(built, folder / NAME)
    print(f"{folder / NAME}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

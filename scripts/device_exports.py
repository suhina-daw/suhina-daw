#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""Build/package the experimental native-device export and run actual sandbox-host tests."""

import argparse
import os
import platform
import plistlib
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
    machine = platform.machine().lower()
    architecture = {"amd64": "x86_64", "x86_64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}.get(machine)
    if architecture is None:
        raise SystemExit(f"Unsupported plug-in architecture: {machine}")
    bundle = output / "SuhinaDevices.vst3"
    folder = "MacOS" if sys.platform == "darwin" else f"{architecture}-{'win' if sys.platform == 'win32' else 'linux'}"
    binary = "SuhinaDevices" if sys.platform == "darwin" else "SuhinaDevices.vst3" if sys.platform == "win32" else "SuhinaDevices.so"
    destination = bundle / "Contents" / folder
    destination.mkdir(parents=True, exist_ok=True)
    shutil.copy2(target / "debug" / filename, destination / binary)
    if sys.platform == "darwin":
        with (bundle / "Contents" / "Info.plist").open("wb") as output_file:
            plistlib.dump({"CFBundleExecutable": binary, "CFBundleIdentifier": "org.suhina.devices", "CFBundlePackageType": "BNDL"}, output_file)
    print(f"CLAP: {clap}", flush=True)
    print(f"VST3: {bundle}", flush=True)
    if args.action == "test":
        environment = dict(os.environ, SUHINA_TEST_EXPORT_CLAP=str(clap), SUHINA_TEST_EXPORT_VST3=str(bundle))
        subprocess.run(["cargo", "test", "-p", "suhina-plugin-host", "--features", "device-exports", "--test", "device_exports", "--", "--ignored"], cwd=root, env=environment, check=True)


if __name__ == "__main__":
    main()

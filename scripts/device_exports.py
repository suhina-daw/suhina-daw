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


def package_library(source: Path, output: Path, system: str, machine: str) -> tuple[Path, Path]:
    """Package a native library without running Cargo or loading foreign platform code."""
    architecture = {"amd64": "x86_64", "x86_64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}.get(machine.lower())
    if system not in ("win32", "linux", "darwin") or architecture is None:
        raise ValueError(f"Unsupported plug-in platform: {system}/{machine}")
    output.mkdir(parents=True, exist_ok=True)
    clap = output / "SuhinaDevices.clap"
    bundle = output / "SuhinaDevices.vst3"

    def apple_bundle(path: Path, identifier: str) -> None:
        destination = path / "Contents" / "MacOS"
        destination.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination / "SuhinaDevices")
        with (path / "Contents" / "Info.plist").open("wb") as output_file:
            plistlib.dump({"CFBundleExecutable": "SuhinaDevices", "CFBundleIdentifier": identifier,
                          "CFBundlePackageType": "BNDL", "CFBundleName": "Suhina Devices"}, output_file)

    if system == "darwin":
        apple_bundle(clap, "org.suhina.devices.clap")
        apple_bundle(bundle, "org.suhina.devices.vst3")
    else:
        shutil.copy2(source, clap)
        folder = f"{architecture}-{'win' if system == 'win32' else 'linux'}"
        binary = "SuhinaDevices.vst3" if system == "win32" else "SuhinaDevices.so"
        destination = bundle / "Contents" / folder
        destination.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination / binary)
    return clap, bundle


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["build", "test"])
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    target = Path(os.environ.get("CARGO_TARGET_DIR", root / "target")).resolve()
    subprocess.run(["cargo", "build", "-p", "suhina-plugin-host", "--lib", "--bin", "suhina-plugin-host", "--features", "device-exports"], cwd=root, check=True)
    filename = {"win32": "suhina_device_exports.dll", "darwin": "libsuhina_device_exports.dylib"}.get(sys.platform, "libsuhina_device_exports.so")
    output = target / "device-exports"
    clap, bundle = package_library(target / "debug" / filename, output, sys.platform, platform.machine())
    print(f"CLAP: {clap}", flush=True)
    print(f"VST3: {bundle}", flush=True)
    if args.action == "test":
        environment = dict(os.environ, SUHINA_TEST_EXPORT_CLAP=str(clap), SUHINA_TEST_EXPORT_VST3=str(bundle))
        subprocess.run(["cargo", "test", "-p", "suhina-plugin-host", "--features", "device-exports", "--test", "device_exports", "--", "--ignored"], cwd=root, env=environment, check=True)


if __name__ == "__main__":
    main()

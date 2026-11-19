# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Validate bundle layout on any OS without loading or pretending to run foreign binaries."""
import plistlib
from pathlib import Path
import tempfile
import unittest

from device_exports import package_library


class PackageTests(unittest.TestCase):
    def test_native_bundle_layouts_preserve_the_library_bytes(self):
        for system, machine, folder, binary in [
            ("win32", "AMD64", "x86_64-win", "SuhinaDevices.vst3"),
            ("win32", "ARM64", "aarch64-win", "SuhinaDevices.vst3"),
            ("linux", "x86_64", "x86_64-linux", "SuhinaDevices.so"),
            ("linux", "aarch64", "aarch64-linux", "SuhinaDevices.so"),
            ("darwin", "arm64", "MacOS", "SuhinaDevices"),
            ("darwin", "x86_64", "MacOS", "SuhinaDevices"),
        ]:
            with self.subTest(system=system, machine=machine), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                source = root / "dummy-library"
                source.write_bytes(b"not executable: packaging fixture")
                clap, vst3 = package_library(source, root / "output", system, machine)
                self.assertEqual((vst3 / "Contents" / folder / binary).read_bytes(), source.read_bytes())
                if system == "darwin":
                    for bundle in (clap, vst3):
                        with (bundle / "Contents" / "Info.plist").open("rb") as stream:
                            info = plistlib.load(stream)
                        self.assertEqual(info["CFBundlePackageType"], "BNDL")
                        self.assertEqual((bundle / "Contents" / "MacOS" / info["CFBundleExecutable"]).read_bytes(), source.read_bytes())
                else:
                    self.assertTrue(clap.is_file())
                    self.assertEqual(clap.read_bytes(), source.read_bytes())

    def test_unknown_platform_does_not_write_a_package(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with self.assertRaises(ValueError):
                package_library(root / "unused", root / "output", "linux", "unknown-cpu")
            self.assertFalse((root / "output").exists())


if __name__ == "__main__":
    unittest.main()

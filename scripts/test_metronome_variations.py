# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""The documented click generator runs on CI's Python and leaves its source untouched."""

import pathlib
import struct
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "assets/metronome/make_variations.py"
SOURCE = ROOT / "assets/metronome/soft.wav"


def generate(source, output):
    return subprocess.run(
        [sys.executable, "-I", str(SCRIPT), str(source), str(output)],
        capture_output=True, text=True, check=False,
    )


class MetronomeVariations(unittest.TestCase):
    def test_float_source_runs_repeatedly_without_changing_the_source(self):
        with tempfile.TemporaryDirectory() as temp:
            folder = pathlib.Path(temp)
            source = folder / "soft.wav"
            original = SOURCE.read_bytes()
            source.write_bytes(original)
            first = generate(source, folder)
            self.assertEqual(first.returncode, 0, first.stderr)
            files = {p.name: p.read_bytes() for p in folder.glob("*.wav")}
            self.assertEqual(set(files), {"soft.wav", "muted.wav", "deep.wav", "tick.wav", "knock.wav"})
            self.assertEqual(files["soft.wav"], original)
            for name, data in files.items():
                self.assertEqual(data[:4], b"RIFF", name)
                self.assertEqual(struct.unpack_from("<H", data, 20)[0], 3, name)
                self.assertGreater(len(data), 44, name)
            again = generate(source, folder)
            self.assertEqual(again.returncode, 0, again.stderr)
            self.assertEqual(files, {p.name: p.read_bytes() for p in folder.glob("*.wav")})

    def test_malformed_or_integer_input_is_refused_before_writing(self):
        original = SOURCE.read_bytes()
        integer = bytearray(original)
        struct.pack_into("<H", integer, 20, 1)
        for data in [b"", original[:20], original[:-1], integer]:
            with self.subTest(size=len(data)), tempfile.TemporaryDirectory() as temp:
                folder = pathlib.Path(temp)
                source = folder / "soft.wav"
                source.write_bytes(data)
                result = generate(source, folder)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(list(folder.iterdir()), [source])
                self.assertEqual(source.read_bytes(), data)


if __name__ == "__main__":
    unittest.main()

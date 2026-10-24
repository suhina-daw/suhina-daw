# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Analytical oracles and malformed input for the developer IR analyzer."""

from pathlib import Path
import tempfile
import unittest

import numpy as np
from scipy.io import wavfile

from analyze import analyze_ir, compare, decay_fit, load_wav, segments


class AnalysisTests(unittest.TestCase):
    def test_exponential_recovers_known_t60(self):
        for rate in (8000, 48000):
            for t60 in (0.5, 2, 6):
                t = np.arange(rate * 20) / rate
                samples = np.exp(-np.log(1000) * t / t60)
                result = decay_fit(samples, rate, -5, -35)
                self.assertAlmostEqual(result["seconds"], t60, delta=t60 * 0.01)

    def test_silent_and_truncated_response_are_invalid(self):
        self.assertIsNone(decay_fit(np.zeros(48000), 48000, -5, -25)["seconds"])
        t = np.arange(24000) / 48000
        short = np.exp(-np.log(1000) * t / 6)
        self.assertIn("truncated", decay_fit(short, 48000, -5, -25)["reason"])

    def test_stereo_energy_does_not_cancel(self):
        t = np.arange(48000 * 3) / 48000
        x = np.exp(-np.log(1000) * t / 0.5) * np.sin(2 * np.pi * 1000 * t)
        result = analyze_ir(np.column_stack((x, -x)), 48000)
        self.assertAlmostEqual(result["stereo_correlation"], -1)
        self.assertEqual(result["mono_to_channel_mean_energy_ratio"], 0)
        self.assertIsNotNone(result["broadband"]["t20"]["seconds"])

    def test_float_and_pcm_wav_scale_consistently(self):
        with tempfile.TemporaryDirectory() as folder:
            for dtype, scale in ((np.float32, 1), (np.int16, 32768)):
                path = Path(folder) / f"{dtype.__name__}.wav"
                x = np.zeros((100, 2), dtype=dtype)
                x[5] = 0.25 * scale
                wavfile.write(path, 48000, x)
                rate, read = load_wav(path)
                self.assertEqual(rate, 48000)
                self.assertEqual(read[5, 0], 0.25)
            path = Path(folder) / "bad.wav"
            wavfile.write(path, 48000, np.full(32, np.nan, dtype=np.float32))
            with self.assertRaisesRegex(ValueError, "non-finite"):
                load_wav(path)
            path.write_bytes(b"not a wave")
            with self.assertRaises(ValueError):
                load_wav(path)

    def test_periodic_capture_has_separate_complete_responses(self):
        x = np.zeros((6100, 2))
        x[[100, 2100, 4100]] = 0.25
        parts = segments(x, 100, 20)
        self.assertEqual(len(parts), 3)
        for _, part in parts:
            self.assertEqual(np.count_nonzero(part), 2)
        for bad in (0, -1, np.nan, 999):
            with self.assertRaises(ValueError):
                segments(x, 100, bad)
    def test_recording_ending_at_loop_boundary_retains_last_tail(self):
        x = np.zeros((6000, 2))
        x[[100, 2100, 4100]] = 0.25
        self.assertEqual(len(segments(x, 100, 20)), 3)

    def test_invalid_baseline_never_gives_numeric_comparison(self):
        empty = analyze_ir(np.zeros((1000, 2)), 48000)
        self.assertTrue(all(value is None for value in compare(empty, empty)["broadband_decay_ratios"].values()))


if __name__ == "__main__":
    unittest.main()

# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

import unittest

import numpy as np

from compare_ir import octave_shape, structure


class StructureTests(unittest.TestCase):
    def test_keeps_common_stimulus_clock_and_channel_energy(self):
        samples = np.zeros((16000, 2))
        samples[8040] = [.25, -.25]
        result = structure(samples, 8000, 1.)
        self.assertAlmostEqual(result["onset_ms"], 5.)
        self.assertEqual(result["energy_before_ms"]["10"], 1.)
        self.assertEqual(result["early_peak_energy_fraction"], 1.)
        self.assertEqual(result["energy_arrival_ms"], {"10": 5., "50": 5., "90": 5.})
        self.assertAlmostEqual(result["energy_centroid_ms"], 5.)

    def test_arrival_quantiles_use_integrated_energy_and_ignore_gain(self):
        samples = np.zeros((16000, 2))
        samples[8080] = [.5, -.5]
        samples[8160] = [1., -1.]
        for gain in [1., .1]:
            result = structure(samples * gain, 8000, 1.)
            self.assertEqual(result["energy_arrival_ms"], {"10": 10., "50": 20., "90": 20.})
            self.assertAlmostEqual(result["energy_centroid_ms"], 18.)

    def test_density_of_gaussian_noise_is_near_one(self):
        samples = np.random.default_rng(51).normal(size=(48000, 2))
        result = structure(samples, 48000, 0.)
        mean = np.mean([d["value"] for d in result["density"]])
        self.assertAlmostEqual(mean, 1., delta=.04)

    def test_energy_curve_of_exponential_has_expected_slope(self):
        rate = 8000
        t = np.arange(4 * rate) / rate
        response = np.exp(-np.log(1000) * t / 2)[:, None]
        result = structure(response, rate, 0.)
        second = next(p for p in result["remaining_energy"] if p["ms"] == 1000)
        self.assertAlmostEqual(second["db"], -30., delta=.02)
        with self.assertRaises(ValueError):
            structure(np.zeros((8000, 2)), rate, 0.)
        with self.assertRaises(ValueError):
            structure(response, rate, 5.)

    def test_octave_shape_removes_only_overall_level(self):
        report = {"tail_spectrum_octaves_hz": {"500": [{"energy_sample_squared_seconds": 1}],
                                              "1000": [{"energy_sample_squared_seconds": 2}]}}
        result = octave_shape(report)
        self.assertAlmostEqual(result["500"], -3.0103, places=4)
        self.assertEqual(result["1000"], 0.)


if __name__ == "__main__":
    unittest.main()

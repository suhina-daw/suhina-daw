// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A test signal and the two measures tests take of a render.

use std::f64::consts::TAU;

/// `frames` samples of a sine at `hz` and `level` (peak), starting at phase 0.
pub fn sine(hz: f64, sample_rate: f64, frames: usize, level: f64) -> Vec<f32> {
    (0..frames).map(|i| (level * (TAU * hz * i as f64 / sample_rate).sin()) as f32).collect()
}

/// The largest absolute sample, 0 for no samples.
pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0, |m, s| m.max(s.abs()))
}

/// The root mean square, 0 for no samples.
pub fn rms(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum::<f64>() / samples.len() as f64).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sine_has_its_level_as_peak_and_its_rms() {
        let s = sine(100.0, 48_000.0, 48_000, 0.5);
        assert!((peak(&s) - 0.5).abs() < 1e-4);
        assert!((rms(&s) - 0.5 / 2f64.sqrt()).abs() < 1e-4);
        assert_eq!((peak(&[]), rms(&[])), (0.0, 0.0));
    }
}

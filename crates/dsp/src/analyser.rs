// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A spectrum for a device's panel: `N` band-pass filters spaced evenly on a log axis, each with
//! a mean-square follower that rises fast and falls slowly, read as levels in decibels. It is a
//! display, not a measurement: the bands overlap as their width says.

use crate::biquad::{Biquad, Coefficients};
use crate::follower::AttackRelease;

/// Levels under this read as this, in decibels.
pub const FLOOR_DB: f64 = -100.0;

/// `N` bands from `low` to `high` hertz, each with its level.
#[derive(Debug, Clone)]
pub struct Analyser<const N: usize> {
    filters: [Biquad; N],
    power: [f64; N],
    follower: AttackRelease,
}

impl<const N: usize> Analyser<N> {
    /// Bands centred evenly in log frequency over `low`..`high` hertz at `sample_rate`, levels
    /// rising with a time constant of `rise` seconds and falling with `fall`. Bands over 45 % of
    /// the sample rate stay silent. Out-of-range inputs fall back to 20 Hz – 20 kHz.
    pub fn new(sample_rate: f64, low: f64, high: f64, (rise, fall): (f64, f64)) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 { sample_rate } else { 48_000.0 };
        let (low, high) =
            if low.is_finite() && high.is_finite() && low > 0.0 && high > low { (low, high) } else { (20.0, 20_000.0) };
        let octaves = (high / low).log2() / N.max(1) as f64;
        let q = Coefficients::q_for_octaves(octaves);
        let filters = std::array::from_fn(|i| {
            let centre = low * (high / low).powf((i as f64 + 0.5) / N.max(1) as f64);
            if centre < 0.45 * sample_rate {
                Biquad::new(Coefficients::bandpass(sample_rate, centre, q))
            } else {
                Biquad::new(Coefficients { b0: 0.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 })
            }
        });
        Self { filters, power: [0.0; N], follower: AttackRelease::new(rise, fall, sample_rate) }
    }

    /// The centre of band `index` of `N` over `low`..`high` hertz, as [`new`](Self::new) places it.
    pub fn centre(index: usize, low: f64, high: f64) -> f64 {
        low * (high / low).powf((index as f64 + 0.5) / N.max(1) as f64)
    }

    /// Takes one sample.
    #[inline]
    pub fn process(&mut self, x: f64) {
        let x = if x.is_finite() { x } else { 0.0 };
        for (filter, power) in self.filters.iter_mut().zip(&mut self.power) {
            let y = filter.process(x);
            *power = self.follower.rise(*power, y * y);
            if *power < 1e-30 {
                *power = 0.0;
            }
        }
    }

    /// Each band's level in decibels (RMS; a full-scale sine in a band reads about -3 dB).
    pub fn levels(&self) -> [f64; N] {
        self.power.map(|p| if p > 0.0 { (10.0 * p.log10()).max(FLOOR_DB) } else { FLOOR_DB })
    }

    pub fn reset(&mut self) {
        for filter in &mut self.filters {
            filter.reset();
        }
        self.power = [0.0; N];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48_000.0;

    #[test]
    fn bands_over_45_percent_of_the_rate_stay_silent() {
        let mut analyser = Analyser::<24>::new(8_000.0, 20.0, 20_000.0, (0.05, 0.05));
        let hz = Analyser::<24>::centre(12, 20.0, 20_000.0);
        for n in 0..8_000 {
            analyser.process((std::f64::consts::TAU * hz * n as f64 / 8_000.0).sin());
        }
        let levels = analyser.levels();
        for (band, level) in levels.iter().enumerate().skip(18) {
            assert!(*level < levels[12] - 60.0, "band {band}: {level} against {}", levels[12]);
        }
    }

    #[test]
    fn a_sine_lights_its_own_band_most() {
        let mut analyser = Analyser::<24>::new(SR, 20.0, 20_000.0, (0.05, 0.05));
        let hz = Analyser::<24>::centre(12, 20.0, 20_000.0);
        for n in 0..24_000 {
            analyser.process((std::f64::consts::TAU * hz * n as f64 / SR).sin());
        }
        let levels = analyser.levels();
        let loudest = (0..24).max_by(|&a, &b| levels[a].total_cmp(&levels[b]));
        assert_eq!(loudest, Some(12));
        assert!((levels[12] + 3.0).abs() < 1.0, "{}", levels[12]);
        assert!(levels[0] < -40.0, "{}", levels[0]);
    }

    #[test]
    fn silence_and_hostile_input_read_the_floor() {
        let mut analyser = Analyser::<8>::new(f64::NAN, -1.0, 0.0, (f64::INFINITY, -1.0));
        for x in [f64::NAN, f64::INFINITY, 0.0] {
            analyser.process(x);
        }
        assert!(analyser.levels().iter().all(|&l| l == FLOOR_DB));
        analyser.reset();
        assert!(analyser.levels().iter().all(|&l| l == FLOOR_DB));
    }
}

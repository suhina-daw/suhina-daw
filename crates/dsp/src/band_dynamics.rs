// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The dynamics of one band of an equaliser: a detector filter on the band's range, a peak
//! follower in decibels, and a gain computer with a soft knee that moves the band's gain by up to
//! its range once the band's level passes the threshold (docs/spec/eq.md › Dynamic and Spectral
//! bands). Negative range turns the band down as it gets louder, positive turns it up.
//!
//! Real-time safe: no allocation, no locks; every method runs on the audio thread except `new`.

use crate::follower::AttackRelease;
use crate::svf::{Svf, SvfCoefficients};

/// The knee's width in dB: the gain change starts 3 dB under the threshold and is fully on the
/// slope 3 dB over it.
pub const KNEE_DB: f64 = 6.0;
/// How far over the threshold, in dB, the band reaches its whole range.
pub const FULL_RANGE_OVER_DB: f64 = 12.0;
/// The level the follower rests at with silence in, in dB.
const FLOOR_DB: f64 = -120.0;

/// The dynamics of one band, on up to two channels whose levels are linked (the louder decides).
#[derive(Clone, Copy, Debug)]
pub struct BandDynamics {
    detectors: [Svf; 2],
    follower: AttackRelease,
    envelope_db: f64,
    gain_db: f64,
}

impl Default for BandDynamics {
    fn default() -> Self {
        Self::new()
    }
}

impl BandDynamics {
    /// At rest: no level, no gain change, a 10 ms attack and 120 ms release at 48 kHz until
    /// [`set_times`](Self::set_times) says otherwise.
    pub fn new() -> Self {
        Self {
            detectors: [Svf::default(); 2],
            follower: AttackRelease::new(0.01, 0.12, 48_000.0),
            envelope_db: FLOOR_DB,
            gain_db: 0.0,
        }
    }

    /// The detector's filter: the range of the sound whose level moves the band.
    pub fn set_detector(&mut self, coefficients: SvfCoefficients) {
        for detector in &mut self.detectors {
            detector.set(coefficients);
        }
    }

    /// The follower's attack and release, in seconds, at `sample_rate`.
    pub fn set_times(&mut self, attack: f64, release: f64, sample_rate: f64) {
        self.follower = AttackRelease::new(attack, release, sample_rate);
    }

    /// Back to rest, the filters' state cleared.
    pub fn reset(&mut self) {
        self.detectors.iter_mut().for_each(Svf::reset);
        self.envelope_db = FLOOR_DB;
        self.gain_db = 0.0;
    }

    /// The gain change in dB the band has now.
    pub fn gain_db(&self) -> f64 {
        self.gain_db
    }

    /// The level the band's detector follows now, in dB.
    pub fn level_db(&self) -> f64 {
        self.envelope_db
    }

    /// One frame: the detector runs on the channels `listen` picks out of `input`, the follower
    /// follows the louder, and the gain change is computed from `threshold` and `range` (dB).
    /// Returns the gain change in dB, between 0 and `range`.
    pub fn process(&mut self, input: [f64; 2], listen: [bool; 2], threshold: f64, range: f64) -> f64 {
        let mut level = 0.0f64;
        for ((detector, x), on) in self.detectors.iter_mut().zip(input).zip(listen) {
            if on {
                level = level.max(detector.process(x).abs());
            }
        }
        let level_db = 20.0 * level.max(1e-9).log10();
        self.envelope_db = self.follower.rise(self.envelope_db, level_db).max(FLOOR_DB);
        self.gain_db = gain_change(self.envelope_db, threshold, range);
        self.gain_db
    }
}

/// The gain change in dB for a level against a threshold: none below the knee, the whole `range`
/// [`FULL_RANGE_OVER_DB`] over the threshold, a soft knee of [`KNEE_DB`] around it.
pub fn gain_change(level_db: f64, threshold: f64, range: f64) -> f64 {
    let over = level_db - threshold;
    let half = KNEE_DB / 2.0;
    let into = if over <= -half {
        0.0
    } else if over >= half {
        over
    } else {
        (over + half).powi(2) / (2.0 * KNEE_DB)
    };
    range * (into / FULL_RANGE_OVER_DB).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gain_change_follows_the_level_over_the_threshold() {
        assert_eq!(gain_change(-40.0, -24.0, -6.0), 0.0, "below the knee, none");
        assert_eq!(gain_change(-24.0 + FULL_RANGE_OVER_DB, -24.0, -6.0), -6.0, "the whole range");
        assert_eq!(gain_change(0.0, -24.0, -6.0), -6.0, "never past the range");
        assert_eq!(gain_change(0.0, -24.0, 9.0), 9.0, "a positive range lifts");
        let at_threshold = gain_change(-24.0, -24.0, -12.0);
        assert!(at_threshold < 0.0 && at_threshold > -2.0, "the knee starts softly: {at_threshold}");
    }

    #[test]
    fn a_loud_tone_in_the_band_moves_it_and_a_quiet_one_does_not() {
        let rate = 48_000.0;
        let run = |amplitude: f64, hz: f64| {
            let mut dynamics = BandDynamics::new();
            dynamics.set_detector(SvfCoefficients::bandpass(rate, 1_000.0, 2.0));
            dynamics.set_times(0.005, 0.1, rate);
            let mut gain = 0.0;
            for n in 0..24_000 {
                let x = amplitude * (std::f64::consts::TAU * hz * n as f64 / rate).sin();
                gain = dynamics.process([x, x], [true, true], -30.0, -10.0);
            }
            gain
        };
        assert!((run(0.5, 1_000.0) + 10.0).abs() < 0.5, "loud in the band: the whole range");
        assert!(run(0.005, 1_000.0).abs() < 0.01, "quiet: nothing");
        assert!(run(0.5, 100.0) > -2.0, "loud but far outside the band: little");
    }

    #[test]
    fn silence_brings_it_back_to_rest() {
        let mut dynamics = BandDynamics::new();
        dynamics.set_detector(SvfCoefficients::bandpass(48_000.0, 1_000.0, 1.0));
        for n in 0..4_800 {
            let x = (std::f64::consts::TAU * 1_000.0 * n as f64 / 48_000.0).sin();
            dynamics.process([x, 0.0], [true, false], -20.0, -6.0);
        }
        assert!(dynamics.gain_db() < -5.0);
        for _ in 0..96_000 {
            dynamics.process([0.0, 0.0], [true, true], -20.0, -6.0);
        }
        assert!(dynamics.gain_db().abs() < 1e-6, "{}", dynamics.gain_db());
        dynamics.reset();
        assert_eq!(dynamics.level_db(), FLOOR_DB);
    }
}

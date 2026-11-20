// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The experimental bar profiles of revision 3 (`docs/spec/instrument.md` › Bar bodies): per-note
//! mode ratios, material decay laws, torsional modes, tubes, bar masses and the mallet, turned
//! into `suhina_dsp::bar` modes. The DSP itself lives in the shared crate; the values and their
//! sources are in the spec.

use suhina_dsp::bar::{BarMode, Mallet, Tube, mode_shape, uniform_ratio};

/// Impact speed at full velocity, in m/s.
pub const MAX_SPEED: f64 = 4.0;
/// Output scale of a bar's weighted mode velocities.
pub const OUTPUT_SCALE: f64 = 0.08;
/// Mallet noise per newton of contact force at the reference bar mass.
const NOISE_PER_NEWTON: f64 = 3e-5;
/// The stroke's offset from the centre line, relative to the cross-section's radius of gyration.
const TORSION_OFFSET: f64 = 0.2;
/// Torsional modes radiate weakly: their output weight relative to a bending mode's.
const TORSION_WEIGHT: f64 = 0.25;
/// Quality factor of every bar's tube.
const TUBE_QUALITY: f64 = 60.0;
/// The tube's steady velocity relative to the first mode's at full Resonator.
const TUBE_GAIN: f64 = 4.0;
const MALLET_MASS: f64 = 0.025;
const MALLET_EXPONENT: f64 = 2.5;
/// Rosewood's loss factor `eta`, in seconds.
const ROSEWOOD_LOSS: f64 = 9.4e-7;
/// The frequency-independent part of the wooden bar's amplitude decay rate, per second.
const WOOD_BASE_LOSS: f64 = 1.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Marimba,
    Vibraphone,
}

/// A tuned mode: its target ratio and the keys between which its register weight falls from 1 to 0.
struct Tuned {
    target: f64,
    full_until: f64,
    none_from: f64,
}

/// Piecewise-linear `(key, value)` points, held at both ends.
fn interpolate(points: &[(f64, f64)], key: f64) -> f64 {
    let Some(&(first_key, first)) = points.first() else {
        return 0.0;
    };
    if key <= first_key {
        return first;
    }
    for pair in points.windows(2) {
        if let [(a, va), (b, vb)] = pair
            && key <= *b
        {
            return va + (vb - va) * (key - a) / (b - a);
        }
    }
    points.last().map_or(first, |&(_, last)| last)
}

/// The first two torsional modes' ratios to the fundamental of a rosewood marimba (La Favre).
const MARIMBA_TORSION: [&[(f64, f64)]; 2] =
    [&[(36.0, 1.56), (43.0, 1.8), (50.0, 2.05), (59.0, 2.05), (60.0, 1.87)], &[(36.0, 9.1), (47.0, 7.6)]];

impl Profile {
    /// From the Body parameter's value; `None` for Struck.
    pub fn from_body(body: f64) -> Option<Self> {
        match body as u8 {
            1 => Some(Self::Marimba),
            2 => Some(Self::Vibraphone),
            _ => None,
        }
    }

    fn tuned(self) -> &'static [Tuned] {
        match self {
            Self::Marimba => &[
                Tuned { target: 4.0, full_until: 84.0, none_from: 96.0 },
                Tuned { target: 10.0, full_until: 60.0, none_from: 79.0 },
            ],
            Self::Vibraphone => &[Tuned { target: 4.0, full_until: f64::INFINITY, none_from: f64::INFINITY }],
        }
    }

    fn reference_mass(self) -> f64 {
        match self {
            Self::Marimba => 0.4,
            Self::Vibraphone => 0.5,
        }
    }

    /// The tube under the bar at `key` for Resonator `amount` (0 to 1); `None` at 0.
    pub fn tube(self, amount: f64, key: u8) -> Option<Tube> {
        let amount = if amount.is_finite() { amount.clamp(0.0, 1.0) } else { 0.0 };
        let fundamental = 440.0 * 2.0_f64.powf((f64::from(key) - 69.0) / 12.0);
        // The first mode's added amplitude decay rate at full amount, per second.
        let radiation = match self {
            Self::Marimba => 8.0 * (fundamental / 349.0).powf(0.75),
            Self::Vibraphone => 1000.0_f64.ln() / 12.0,
        };
        (amount > 0.0).then(|| Tube {
            amount,
            gain: TUBE_GAIN,
            quality: TUBE_QUALITY,
            radiation_seconds: 1000.0_f64.ln() / radiation,
        })
    }

    /// The mallet noise's output per newton at `key`, scaled like the bar's own level.
    pub fn noise_gain(self, key: u8) -> f64 {
        NOISE_PER_NEWTON * (self.bar_mass(f64::from(key)) / self.reference_mass()).sqrt()
    }

    /// Whether note-off damps the bar (unless the sustain pedal holds it).
    pub fn damps_on_release(self) -> bool {
        self == Self::Vibraphone
    }

    /// Whether mode `n` is tuned at `key`: the fundamental, or a target with register weight.
    pub fn is_tuned(self, n: usize, key: f64) -> bool {
        n == 1 || n.checked_sub(2).and_then(|index| self.tuned().get(index)).is_some_and(|mode| key < mode.none_from)
    }

    /// Mode `n`'s frequency ratio to the fundamental at `key`, `n` from one.
    pub fn ratio(self, n: usize, key: f64) -> f64 {
        let tuned = self.tuned();
        let tuned_ratio = |n: usize| {
            let Some(mode) = n.checked_sub(2).and_then(|index| tuned.get(index)) else {
                return uniform_ratio(n);
            };
            let weight = if key <= mode.full_until {
                1.0
            } else if key >= mode.none_from {
                0.0
            } else {
                (mode.none_from - key) / (mode.none_from - mode.full_until)
            };
            mode.target.powf(weight) * uniform_ratio(n).powf(1.0 - weight)
        };
        let last = tuned.len() + 1;
        if n <= last { tuned_ratio(n) } else { tuned_ratio(last) * uniform_ratio(n) / uniform_ratio(last) }
    }

    /// Time for a mode at `frequency_hz` to lose 60 dB, with Body decay `decay`.
    pub fn decay_seconds(self, frequency_hz: f64, decay: f64) -> f64 {
        let seconds = match self {
            // Chaigne & Doutaut's loss law: alpha = a0 + 2 pi^2 eta f^2.
            Self::Marimba => {
                let alpha =
                    WOOD_BASE_LOSS + 2.0 * std::f64::consts::PI.powi(2) * ROSEWOOD_LOSS * frequency_hz * frequency_hz;
                1000.0_f64.ln() / alpha
            }
            Self::Vibraphone => 8.0 * (1.0 + 261.63 / 1500.0) / (1.0 + frequency_hz / 1500.0),
        };
        seconds * decay / 2.0
    }

    /// The bar's mass at `key`.
    pub fn bar_mass(self, key: f64) -> f64 {
        self.reference_mass() * 2.0_f64.powf(-(key - 60.0) / 24.0)
    }

    /// The torsional modes' ratios at `key`; none for a profile without measurements.
    fn torsion(self, key: f64) -> [f64; 2] {
        match self {
            Self::Marimba => MARIMBA_TORSION.map(|points| interpolate(points, key)),
            Self::Vibraphone => [0.0; 2],
        }
    }

    /// The bar's modes at `key`: `count` modes for Brightness `brightness`, Strike position
    /// `strike` and Body decay `decay`.
    pub fn modes<const N: usize>(
        self,
        key: u8,
        count: usize,
        decay: f64,
        brightness: f64,
        strike: f64,
    ) -> [BarMode; N] {
        let key = f64::from(key);
        let fundamental = 440.0 * 2.0_f64.powf((key - 69.0) / 12.0);
        let contact = 0.5 * (0.98 - strike) / 0.96;
        let mass = self.bar_mass(key);
        let level = OUTPUT_SCALE * (mass / self.reference_mass()).sqrt();
        // A stroke slightly off the centre line also twists a bar with known torsional modes.
        let torsion = self.torsion(key);
        let available = torsion.iter().filter(|ratio| **ratio > 0.0).count();
        let torsional = if count >= 16 {
            available.min(2)
        } else if count >= 8 {
            available.min(1)
        } else {
            0
        };
        let bending = count.saturating_sub(torsional);
        let mut modes = [BarMode::default(); N];
        for (index, mode) in modes.iter_mut().take(count).enumerate() {
            let (ratio, shape, modal_mass, weight) = if index < bending {
                let n = index + 1;
                (self.ratio(n, key), mode_shape(n, contact), mass / 4.0, 1.0)
            } else {
                let n = index - bending + 1;
                let ratio = torsion.get(n - 1).copied().unwrap_or(0.0);
                let shape = TORSION_OFFSET * (n as f64 * std::f64::consts::PI * contact).cos();
                (ratio, shape, mass / 2.0, TORSION_WEIGHT)
            };
            let frequency_hz = fundamental * ratio;
            *mode = BarMode {
                frequency_hz,
                decay_seconds: self.decay_seconds(frequency_hz, decay),
                shape,
                modal_mass,
                output: weight * level * ratio.powf(brightness - 0.5),
            };
        }
        modes
    }
}

/// The hardness a graded mallet set gives `key` for Mallet hardness `hardness`: softer below
/// middle C, by up to 0.3 three octaves down.
pub fn graded_hardness(key: u8, hardness: f64) -> f64 {
    let low = ((60.0 - f64::from(key)) / 36.0).clamp(0.0, 1.0);
    (hardness - 0.3 * low).max(0.0)
}

/// The mallet for Mallet hardness `hardness` (0 soft to 1 hard): its contact against a rigid
/// surface at 1 m/s lasts 1.5 ms soft to 0.4 ms hard.
pub fn mallet(hardness: f64) -> Mallet {
    let hardness = hardness.clamp(0.0, 1.0);
    Mallet::from_contact_time(
        MALLET_MASS,
        MALLET_EXPONENT,
        0.0015 * (0.4_f64 / 1.5).powf(hardness),
        0.05 + 0.45 * (1.0 - hardness),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_values_follow_their_sources() {
        let with_tube = |key: u8| {
            let f1 = 440.0 * 2.0_f64.powf((f64::from(key) - 69.0) / 12.0);
            let bare = Profile::Marimba.decay_seconds(f1, 2.0);
            let tube = Profile::Marimba.tube(1.0, key).unwrap();
            (bare, 1.0 / (1.0 / bare + 1.0 / tube.radiation_seconds))
        };
        // A low D sharp rings 3 to 4 s over a detuned tube and 1 to 2 s over a tuned one (Stevens).
        let (bare, tuned) = with_tube(39);
        assert!((3.0..4.6).contains(&bare) && (1.0..2.0).contains(&tuned), "{bare} {tuned}");
        // A tuned resonator shortens an F4 bar to about 0.67 s (Rucz et al. 2021).
        let (_, tuned) = with_tube(65);
        assert!((tuned - 0.67).abs() < 0.12, "{tuned}");
        // The first torsional mode lies between the first two bending modes.
        for key in [36, 48, 60, 72, 84] {
            let ratio = Profile::Marimba.torsion(f64::from(key))[0];
            assert!(ratio > 1.0 && ratio < Profile::Marimba.ratio(2, f64::from(key)), "key {key}: {ratio}");
        }
        assert_eq!(Profile::Vibraphone.torsion(60.0), [0.0; 2]);
        assert_eq!(interpolate(&[(36.0, 1.0), (48.0, 2.0)], 42.0), 1.5);
        assert_eq!(interpolate(&[(36.0, 1.0), (48.0, 2.0)], 0.0), 1.0);
        assert_eq!(interpolate(&[(36.0, 1.0), (48.0, 2.0)], 99.0), 2.0);
        assert_eq!(interpolate(&[], 50.0), 0.0);
    }
}

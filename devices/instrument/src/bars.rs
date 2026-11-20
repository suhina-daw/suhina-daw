// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The experimental bar and tine bodies of revision 3 (`docs/spec/instrument.md` › Bar bodies):
//! per-note mode ratios, material decay laws, torsional modes, tubes and boxes, masses and the
//! mallet, shaped by the Tuning, Ring, Torsion and Mallet weight controls and turned into
//! `suhina_dsp::bar` modes. The DSP itself lives in the shared crate; the values and their sources
//! are in the spec.

use suhina_dsp::bar::{BarMode, Excitation, Mallet, Tube, mode_shape, tine_ratio, tine_shape, uniform_ratio};

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
    Xylophone,
    Glockenspiel,
    Kalimba,
}

/// The shaping controls of a bar body, neutral at their defaults.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shaping {
    /// 0 to 1: how far the tuned targets apply.
    pub tuning: f64,
    /// 0 to 1: the upper modes' decay relative to the fundamental's, 0.5 neutral.
    pub ring: f64,
    /// 0 to 1: the torsional modes' weight, 0.5 neutral.
    pub torsion: f64,
}

impl Default for Shaping {
    fn default() -> Self {
        Self { tuning: 1.0, ring: 0.5, torsion: 0.5 }
    }
}

/// A tuned mode: its target ratio and the keys between which its register weight falls from 1 to 0.
struct Tuned {
    target: f64,
    full_until: f64,
    none_from: f64,
}

const fn full(target: f64) -> Tuned {
    Tuned { target, full_until: f64::INFINITY, none_from: f64::INFINITY }
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

const VIBRAPHONE_TUNED: &[Tuned] = &[full(4.0)];
const XYLOPHONE_TUNED: &[Tuned] = &[full(3.0), full(6.0)];
const GLOCKENSPIEL_TUNED: &[Tuned] = &[full(2.71), full(5.15), full(8.43), full(12.21)];
const KALIMBA_TUNED: &[Tuned] = &[full(5.63), full(14.0)];

fn fundamental(key: f64) -> f64 {
    440.0 * 2.0_f64.powf((key - 69.0) / 12.0)
}

impl Profile {
    /// From the Body parameter's value; `None` for Struck.
    pub fn from_body(body: f64) -> Option<Self> {
        match body as u8 {
            1 => Some(Self::Marimba),
            2 => Some(Self::Vibraphone),
            3 => Some(Self::Xylophone),
            4 => Some(Self::Glockenspiel),
            5 => Some(Self::Kalimba),
            _ => None,
        }
    }

    fn tuned(self) -> &'static [Tuned] {
        match self {
            Self::Marimba => &[
                Tuned { target: 4.0, full_until: 84.0, none_from: 96.0 },
                Tuned { target: 10.0, full_until: 60.0, none_from: 79.0 },
            ],
            Self::Vibraphone => VIBRAPHONE_TUNED,
            Self::Xylophone => XYLOPHONE_TUNED,
            Self::Glockenspiel => GLOCKENSPIEL_TUNED,
            Self::Kalimba => KALIMBA_TUNED,
        }
    }

    /// The natural ratio of mode `n`: a free-free bar's, or a clamped-free tine's.
    fn natural(self, n: usize) -> f64 {
        if self == Self::Kalimba { tine_ratio(n) } else { uniform_ratio(n) }
    }

    /// The body's output level beside the bar mass law, measured so every body's loudest key
    /// peaks between a tenth and four times Struck's (`bar_levels_are_comparable_with_struck`).
    fn level(self) -> f64 {
        match self {
            Self::Marimba | Self::Vibraphone => 1.0,
            Self::Xylophone | Self::Kalimba => 0.6,
            Self::Glockenspiel => 3.0,
        }
    }

    fn reference_mass(self) -> f64 {
        match self {
            Self::Marimba => 0.4,
            Self::Vibraphone => 0.5,
            Self::Xylophone => 0.156,
            Self::Glockenspiel => 0.7,
            Self::Kalimba => 0.008,
        }
    }

    /// The hardness the body adds to Mallet hardness: harder mallets for the harder bars.
    pub fn added_hardness(self) -> f64 {
        match self {
            Self::Xylophone => 0.3,
            Self::Glockenspiel => 0.4,
            _ => 0.0,
        }
    }

    /// How a stroke reaches this body: a tine is plucked, a bar struck by `mallet_model`.
    pub fn excitation(self, mallet_model: f64) -> Excitation {
        match self {
            Self::Kalimba => Excitation::Pluck,
            _ if mallet_model < 0.5 => Excitation::Contact,
            _ => Excitation::Pulse,
        }
    }

    /// The tube or box under the body at `key` for Resonator `amount` (0 to 1); `None` at 0 or
    /// for a body without one.
    pub fn tube(self, amount: f64, key: u8) -> Option<Tube> {
        let amount = if amount.is_finite() { amount.clamp(0.0, 1.0) } else { 0.0 };
        if amount <= 0.0 {
            return None;
        }
        let f1 = fundamental(f64::from(key));
        // The first mode's added amplitude decay rate at full amount, per second.
        let tube = |radiation: f64| Tube {
            amount,
            gain: TUBE_GAIN,
            quality: TUBE_QUALITY,
            radiation_seconds: 1000.0_f64.ln() / radiation,
            frequency_hz: None,
        };
        match self {
            Self::Marimba | Self::Xylophone => Some(tube(8.0 * (f1 / 349.0).powf(0.75))),
            Self::Vibraphone => Some(tube(1000.0_f64.ln() / 12.0)),
            Self::Glockenspiel => None,
            Self::Kalimba => {
                Some(Tube { amount, gain: 1.0, quality: 10.0, radiation_seconds: 600.0, frequency_hz: Some(480.0) })
            }
        }
    }

    /// The mallet noise's output per newton at `key`, scaled like the bar's own level; a plucked
    /// tine has none.
    pub fn noise_gain(self, key: u8) -> f64 {
        if self == Self::Kalimba {
            return 0.0;
        }
        NOISE_PER_NEWTON * self.level() * (self.bar_mass(f64::from(key)) / self.reference_mass()).sqrt()
    }

    /// Whether note-off damps the bar (unless the sustain pedal holds it).
    pub fn damps_on_release(self) -> bool {
        self == Self::Vibraphone
    }

    /// Whether mode `n` is tuned at `key`: the fundamental, or a target with register weight.
    pub fn is_tuned(self, n: usize, key: f64) -> bool {
        n == 1 || n.checked_sub(2).and_then(|index| self.tuned().get(index)).is_some_and(|mode| key < mode.none_from)
    }

    /// Mode `n`'s frequency ratio to the fundamental at `key`, `n` from one, at full Tuning.
    pub fn ratio(self, n: usize, key: f64) -> f64 {
        self.tuned_ratio(n, key, 1.0)
    }

    /// Mode `n`'s ratio with every tuned target's register weight multiplied by `tuning`.
    fn tuned_ratio(self, n: usize, key: f64, tuning: f64) -> f64 {
        let tuned = self.tuned();
        let tuning = if tuning.is_finite() { tuning.clamp(0.0, 1.0) } else { 1.0 };
        let target_ratio = |n: usize| {
            let natural = self.natural(n);
            let Some(mode) = n.checked_sub(2).and_then(|index| tuned.get(index)) else {
                return natural;
            };
            let weight = if key <= mode.full_until {
                1.0
            } else if key >= mode.none_from {
                0.0
            } else {
                (mode.none_from - key) / (mode.none_from - mode.full_until)
            } * tuning;
            mode.target.powf(weight) * natural.powf(1.0 - weight)
        };
        let last = tuned.len() + 1;
        if n <= last { target_ratio(n) } else { target_ratio(last) * self.natural(n) / self.natural(last) }
    }

    /// Time for a mode at `frequency_hz` to lose 60 dB, with Body decay `decay`.
    pub fn decay_seconds(self, frequency_hz: f64, decay: f64) -> f64 {
        let seconds = match self {
            // Chaigne & Doutaut's loss law: alpha = a0 + 2 pi^2 eta f^2.
            Self::Marimba | Self::Xylophone => {
                let alpha =
                    WOOD_BASE_LOSS + 2.0 * std::f64::consts::PI.powi(2) * ROSEWOOD_LOSS * frequency_hz * frequency_hz;
                1000.0_f64.ln() / alpha
            }
            Self::Vibraphone => 8.0 * (1.0 + 261.63 / 1500.0) / (1.0 + frequency_hz / 1500.0),
            Self::Glockenspiel => 4.0 * (1047.0 / frequency_hz).powf(1.5),
            Self::Kalimba => 2.5 * 261.63 / frequency_hz,
        };
        seconds * decay / 2.0
    }

    /// The bar's mass at `key`.
    pub fn bar_mass(self, key: f64) -> f64 {
        self.reference_mass() * 2.0_f64.powf(-(key - 60.0) / 24.0)
    }

    /// The torsional modes' ratios at `key`; none for a body without measurements.
    fn torsion(self, key: f64) -> [f64; 2] {
        match self {
            Self::Marimba => MARIMBA_TORSION.map(|points| interpolate(points, key)),
            Self::Xylophone => [1.98, 4.07],
            Self::Glockenspiel => [3.57, 7.07],
            Self::Vibraphone | Self::Kalimba => [0.0; 2],
        }
    }

    /// The contact point, from Strike position `strike`: a bar's from its centre to its end, a
    /// tine's from its free end towards the clamp.
    fn contact(self, strike: f64) -> f64 {
        match self {
            Self::Kalimba => 1.0 - 0.5 * strike,
            _ => 0.5 * (0.98 - strike) / 0.96,
        }
    }

    /// The body's modes at `key`: `count` modes for Brightness `brightness`, Strike position
    /// `strike`, Body decay `decay` and the shaping controls.
    pub fn modes<const N: usize>(
        self,
        key: u8,
        count: usize,
        decay: f64,
        brightness: f64,
        strike: f64,
        shaping: Shaping,
    ) -> [BarMode; N] {
        let key = f64::from(key);
        let f1 = fundamental(key);
        let contact = self.contact(strike);
        let mass = self.bar_mass(key);
        let level = OUTPUT_SCALE * self.level() * (mass / self.reference_mass()).sqrt();
        let ring = if shaping.ring.is_finite() { shaping.ring.clamp(0.0, 1.0) } else { 0.5 } - 0.5;
        let torsion_weight =
            TORSION_WEIGHT * 2.0 * if shaping.torsion.is_finite() { shaping.torsion.clamp(0.0, 1.0) } else { 0.5 };
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
                let shape = if self == Self::Kalimba { tine_shape(n, contact) } else { mode_shape(n, contact) };
                (self.tuned_ratio(n, key, shaping.tuning), shape, mass / 4.0, 1.0)
            } else {
                let n = index - bending + 1;
                let ratio = torsion.get(n - 1).copied().unwrap_or(0.0);
                let shape = TORSION_OFFSET * (n as f64 * std::f64::consts::PI * contact).cos();
                (ratio, shape, mass / 2.0, torsion_weight)
            };
            let frequency_hz = f1 * ratio;
            *mode = BarMode {
                frequency_hz,
                decay_seconds: self.decay_seconds(frequency_hz, decay) * ratio.powf(ring),
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

/// The mallet for Mallet hardness `hardness` (0 soft to 1 hard) and Mallet weight `weight` (0 to
/// 1): its contact against a rigid surface at 1 m/s lasts 1.5 ms soft to 0.4 ms hard, and its head
/// weighs 12.5 to 50 g.
pub fn mallet(hardness: f64, weight: f64) -> Mallet {
    let hardness = if hardness.is_finite() { hardness.clamp(0.0, 1.0) } else { 0.5 };
    let weight = if weight.is_finite() { weight.clamp(0.0, 1.0) } else { 0.5 };
    Mallet::from_contact_time(
        MALLET_MASS * 2.0_f64.powf(2.0 * weight - 1.0),
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
            let f1 = fundamental(f64::from(key));
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
        assert_eq!(Profile::Xylophone.ratio(2, 70.0), 3.0);
        assert_eq!(Profile::Glockenspiel.ratio(3, 90.0), 5.15);
        assert!((Profile::Kalimba.ratio(2, 70.0) - 5.63).abs() < 1e-12);
        assert!((Profile::Kalimba.tuned_ratio(2, 70.0, 0.0) - tine_ratio(2)).abs() < 1e-12);
        assert!((Profile::Marimba.tuned_ratio(2, 60.0, 0.0) - uniform_ratio(2)).abs() < 1e-12);
        assert_eq!(Profile::Kalimba.excitation(0.0), Excitation::Pluck);
        assert!(Profile::Glockenspiel.tube(1.0, 80).is_none());
        assert_eq!(Profile::Kalimba.tube(1.0, 60).unwrap().frequency_hz, Some(480.0));
        assert_eq!(interpolate(&[(36.0, 1.0), (48.0, 2.0)], 42.0), 1.5);
        assert_eq!(interpolate(&[(36.0, 1.0), (48.0, 2.0)], 0.0), 1.0);
        assert_eq!(interpolate(&[(36.0, 1.0), (48.0, 2.0)], 99.0), 2.0);
        assert_eq!(interpolate(&[], 50.0), 0.0);
        for body in 1..=5 {
            assert!(Profile::from_body(f64::from(body)).is_some());
        }
        assert_eq!(Profile::from_body(0.0), None);
        assert_eq!(Profile::from_body(6.0), None);
    }

    #[test]
    fn shaping_controls_are_neutral_at_their_defaults_and_move_the_right_things() {
        let neutral: [BarMode; 16] = Profile::Marimba.modes(60, 16, 2.0, 0.5, 0.25, Shaping::default());
        let ringing: [BarMode; 16] =
            Profile::Marimba.modes(60, 16, 2.0, 0.5, 0.25, Shaping { ring: 1.0, ..Shaping::default() });
        assert_eq!(neutral[0].decay_seconds, ringing[0].decay_seconds, "the fundamental keeps its decay");
        assert!(ringing[2].decay_seconds > neutral[2].decay_seconds);
        let twisted: [BarMode; 16] =
            Profile::Marimba.modes(60, 16, 2.0, 0.5, 0.25, Shaping { torsion: 1.0, ..Shaping::default() });
        assert_eq!(twisted[15].output, 2.0 * neutral[15].output);
        assert_eq!(twisted[0].output, neutral[0].output);
        let light = mallet(0.5, 0.0);
        let heavy = mallet(0.5, 1.0);
        assert!((heavy.mass / light.mass - 4.0).abs() < 1e-12);
    }
}

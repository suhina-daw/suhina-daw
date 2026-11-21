// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fixed-capacity damped sinusoidal modes. A mode is a complex rotation with radius below
//! one, so a strike is bounded by its initial amplitude. No allocation, including excitation.

use std::f64::consts::TAU;

pub mod model;
pub mod profile;
pub mod sustain;

/// One exponentially decaying mode, expressed in physical units.
#[derive(Debug, Clone, Copy)]
pub struct Mode {
    pub frequency_hz: f64,
    /// Time to decay by 60 dB, in seconds.
    pub decay_seconds: f64,
    pub amplitude: f64,
}

#[derive(Debug, Clone, Copy)]
struct Resonance {
    real: f64,
    imaginary: f64,
    cosine: f64,
    sine: f64,
    frequency_hz: f64,
    radius: f64,
    /// The second output's weights on the sine (imaginary) and cosine (real) parts: `[1, 0]`
    /// plays the same sine, `[g cos d, g sin d]` plays it `g` times as loud and `d` later.
    other: [f32; 2],
}

impl Default for Resonance {
    fn default() -> Self {
        Self { real: 0.0, imaginary: 0.0, cosine: 0.0, sine: 0.0, frequency_hz: 0.0, radius: 0.0, other: [1.0, 0.0] }
    }
}

/// At most `N` modes. Invalid or inaudible modes are silent; amplitudes are normalized so
/// their absolute sum is at most one. The mode count is fixed by the caller's type.
#[derive(Debug, Clone, Copy)]
pub struct ModalBank<const N: usize> {
    modes: [Resonance; N],
    active: bool,
}

impl<const N: usize> Default for ModalBank<N> {
    fn default() -> Self {
        Self { modes: [Resonance::default(); N], active: false }
    }
}

impl<const N: usize> ModalBank<N> {
    /// Replace the previous strike. Frequencies must be positive and below 0.45 of the
    /// sample rate. Decay is capped at 60 seconds; amplitude at unity before normalization.
    pub fn strike(&mut self, sample_rate: f64, modes: &[Mode]) {
        self.reset();
        if !sample_rate.is_finite() || !(1000.0..=384000.0).contains(&sample_rate) {
            return;
        }
        let mut amplitude_sum = 0.0;
        for (target, mode) in self.modes.iter_mut().zip(modes) {
            if !mode.frequency_hz.is_finite()
                || mode.frequency_hz <= 0.0
                || mode.frequency_hz >= sample_rate * 0.45
                || !mode.decay_seconds.is_finite()
                || mode.decay_seconds <= 0.0
                || !mode.amplitude.is_finite()
            {
                continue;
            }
            let radius = (-1000.0_f64.ln() / (sample_rate * mode.decay_seconds.clamp(0.001, 60.0))).exp();
            let (sine, cosine) = (TAU * mode.frequency_hz / sample_rate).sin_cos();
            target.real = mode.amplitude.clamp(-1.0, 1.0);
            target.cosine = cosine * radius;
            target.sine = sine * radius;
            target.frequency_hz = mode.frequency_hz;
            target.radius = radius;
            amplitude_sum += target.real.abs();
        }
        let normalization = amplitude_sum.max(1.0);
        for mode in &mut self.modes {
            mode.real /= normalization;
        }
        self.active = amplitude_sum > 0.0;
    }

    /// Retune a ringing bank without resetting its phase or decay. The offset is absolute
    /// relative to the strike's frequencies, limited to eight octaves. A mode crossing the
    /// anti-alias margin is stopped; it is never aliased or resurrected with invented energy.
    pub fn transpose(&mut self, sample_rate: f64, semitones: f64) {
        if !sample_rate.is_finite() || !(1000.0..=384000.0).contains(&sample_rate) || !semitones.is_finite() {
            return;
        }
        let ratio = 2.0_f64.powf(semitones.clamp(-96.0, 96.0) / 12.0);
        for mode in &mut self.modes {
            let frequency = mode.frequency_hz * ratio;
            if frequency >= sample_rate * 0.45 {
                mode.real = 0.0;
                mode.imaginary = 0.0;
                continue;
            }
            let (sine, cosine) = (TAU * frequency / sample_rate).sin_cos();
            mode.cosine = cosine * mode.radius;
            mode.sine = sine * mode.radius;
        }
    }

    /// Advance one sample. Silent banks return immediately; tiny tails are cleared before
    /// they become subnormal. Output starts at zero to avoid an excitation discontinuity.
    pub fn next_sample(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }
        let mut output = 0.0;
        let mut active = false;
        for mode in &mut self.modes {
            if mode.real.abs() + mode.imaginary.abs() < 1e-12 {
                mode.real = 0.0;
                mode.imaginary = 0.0;
                continue;
            }
            output += mode.imaginary;
            let real = mode.real * mode.cosine - mode.imaginary * mode.sine;
            mode.imaginary = mode.real * mode.sine + mode.imaginary * mode.cosine;
            mode.real = real;
            active = true;
        }
        self.active = active;
        output as f32
    }

    /// Mode `n`'s amplitude: the size of its rotating state, which falls as it decays and does
    /// not move with its phase. Zero past the bank's modes.
    pub fn level(&self, n: usize) -> f64 {
        self.modes.get(n).map_or(0.0, |mode| mode.real.hypot(mode.imaginary))
    }

    /// Like [`next_sample`](Self::next_sample), with mode `i` weighted by `gains[i]`, which then
    /// moves by `steps[i]`: per-mode levels ramped by the caller without touching phase or decay.
    /// All gains 1 and steps 0 give exactly `next_sample`.
    pub fn next_sample_gained(&mut self, gains: &mut [f32; N], steps: &[f32; N]) -> f32 {
        if !self.active {
            return 0.0;
        }
        let mut output = 0.0;
        let mut active = false;
        for ((mode, gain), step) in self.modes.iter_mut().zip(gains.iter_mut()).zip(steps) {
            if mode.real.abs() + mode.imaginary.abs() < 1e-12 {
                mode.real = 0.0;
                mode.imaginary = 0.0;
                continue;
            }
            output += mode.imaginary * f64::from(*gain);
            *gain += *step;
            let real = mode.real * mode.cosine - mode.imaginary * mode.sine;
            mode.imaginary = mode.real * mode.sine + mode.imaginary * mode.cosine;
            mode.real = real;
            active = true;
        }
        self.active = active;
        output as f32
    }

    /// Sets mode `i`'s second output to `weights[i]` (see [`next_stereo`](Self::next_stereo)):
    /// `[g cos d, g sin d]` plays the mode `g` times as loud and `d` radians later. Modes without
    /// an entry keep `[1, 0]`, the same sine. Nonfinite weights are `[1, 0]`.
    pub fn set_other(&mut self, weights: &[[f32; 2]]) {
        for (index, mode) in self.modes.iter_mut().enumerate() {
            mode.other = weights.get(index).copied().filter(|w| w.iter().all(|v| v.is_finite())).unwrap_or([1.0, 0.0]);
        }
    }

    /// Like [`next_sample`](Self::next_sample), with a second output: `[first, second]`, where
    /// the second weighs each mode by its [`set_other`](Self::set_other) weights. Without them
    /// both outputs equal `next_sample`.
    pub fn next_stereo(&mut self) -> [f32; 2] {
        self.next_pair(None)
    }

    /// [`next_sample_gained`](Self::next_sample_gained) into two outputs, as
    /// [`next_stereo`](Self::next_stereo).
    pub fn next_stereo_gained(&mut self, gains: &mut [f32; N], steps: &[f32; N]) -> [f32; 2] {
        self.next_pair(Some((gains, steps)))
    }

    fn next_pair(&mut self, mut gains: Option<(&mut [f32; N], &[f32; N])>) -> [f32; 2] {
        if !self.active {
            return [0.0; 2];
        }
        let (mut first, mut second) = (0.0, 0.0);
        let mut active = false;
        for (index, mode) in self.modes.iter_mut().enumerate() {
            if mode.real.abs() + mode.imaginary.abs() < 1e-12 {
                mode.real = 0.0;
                mode.imaginary = 0.0;
                continue;
            }
            let gain = match gains.as_mut() {
                Some((gains, steps)) => match (gains.get_mut(index), steps.get(index)) {
                    (Some(gain), Some(step)) => {
                        let value = f64::from(*gain);
                        *gain += *step;
                        value
                    }
                    _ => 1.0,
                },
                None => 1.0,
            };
            first += mode.imaginary * gain;
            second += (mode.imaginary * f64::from(mode.other[0]) + mode.real * f64::from(mode.other[1])) * gain;
            let real = mode.real * mode.cosine - mode.imaginary * mode.sine;
            mode.imaginary = mode.real * mode.sine + mode.imaginary * mode.cosine;
            mode.real = real;
            active = true;
        }
        self.active = active;
        [first as f32, second as f32]
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn reset(&mut self) {
        self.modes.fill(Resonance::default());
        self.active = false;
    }

    /// Caps every mode's T60 at `seconds` (0.001..60), keeping its phase and current frequency,
    /// as a damper stops a ringing string. Modes already shorter are left alone. Allocation-free.
    pub fn cap_decay(&mut self, sample_rate: f64, seconds: f64) {
        if !sample_rate.is_finite() || !(1000.0..=384000.0).contains(&sample_rate) || !seconds.is_finite() {
            return;
        }
        let cap = (-1000.0_f64.ln() / (sample_rate * seconds.clamp(0.001, 60.0))).exp();
        for mode in &mut self.modes {
            if mode.radius > cap {
                let scale = cap / mode.radius;
                mode.cosine *= scale;
                mode.sine *= scale;
                mode.radius = cap;
            }
        }
    }

    /// Like [`strike`](Self::strike), with mode `i` starting at `phases[i]` radians: its output
    /// is `amplitude * radius^n * sin(omega * n + phase)`. A missing or nonfinite phase is zero,
    /// which is `strike`. The first output sample is therefore not zero unless every phase is;
    /// the caller ramps its gate.
    pub fn strike_phased(&mut self, sample_rate: f64, modes: &[Mode], phases: &[f64]) {
        self.strike(sample_rate, modes);
        for (target, phase) in self.modes.iter_mut().zip(phases) {
            let phase = if phase.is_finite() { *phase } else { 0.0 };
            let amplitude = target.real;
            target.real = amplitude * phase.cos();
            target.imaginary = amplitude * phase.sin();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_modes_level_is_its_decaying_amplitude() {
        let mut bank = ModalBank::<2>::default();
        assert_eq!(bank.level(0), 0.0);
        bank.strike(48000.0, &[Mode { frequency_hz: 440.0, decay_seconds: 0.5, amplitude: 0.5 }]);
        for n in 1..=24000 {
            bank.next_sample();
            if n % 6000 == 0 {
                let expected = 0.5 * (-1000.0_f64.ln() * n as f64 / 24000.0).exp();
                assert!((bank.level(0) - expected).abs() < 1e-6, "sample {n}: {} against {expected}", bank.level(0));
            }
        }
        assert_eq!(bank.level(1), 0.0, "an unused mode");
        assert_eq!(bank.level(9), 0.0, "past the bank");
    }

    #[test]
    fn one_mode_matches_the_analytical_decay_and_pitch() {
        let mut bank = ModalBank::<1>::default();
        bank.strike(48000.0, &[Mode { frequency_hz: 440.0, decay_seconds: 0.25, amplitude: 0.5 }]);
        for n in 0..24000 {
            let expected =
                0.5 * (-1000.0_f64.ln() * n as f64 / 12000.0).exp() * (TAU * 440.0 * n as f64 / 48000.0).sin();
            assert!((f64::from(bank.next_sample()) - expected).abs() < 2e-8);
        }
    }

    /// The second output plays each mode times its gain and shifted by its offset, from the same
    /// resonator; without weights both outputs are `next_sample`.
    #[test]
    fn the_second_output_places_each_mode_by_gain_and_offset() {
        let modes = [
            Mode { frequency_hz: 440.0, decay_seconds: 0.5, amplitude: 0.4 },
            Mode { frequency_hz: 1250.0, decay_seconds: 0.2, amplitude: 0.3 },
        ];
        let mut plain = ModalBank::<2>::default();
        plain.strike_phased(48000.0, &modes, &[0.3, -1.1]);
        let mut placed = plain;
        placed.set_other(&[[0.5 * 0.8_f32.cos(), 0.5 * 0.8_f32.sin()]]);
        let mut unplaced = plain;
        for n in 0..4800 {
            let t = n as f64 / 48000.0;
            let mono = f64::from(plain.next_sample());
            let [first, second] = placed.next_stereo();
            let decay = |t60: f64| (-1000.0_f64.ln() * t / t60).exp();
            let expected = 0.5 * 0.4 * decay(0.5) * (TAU * 440.0 * t + 0.3 + 0.8).sin()
                + 0.3 * decay(0.2) * (TAU * 1250.0 * t - 1.1).sin();
            assert!((f64::from(first) - mono).abs() < 1e-6, "{n}");
            assert!((f64::from(second) - expected).abs() < 1e-5, "{n}: {second} vs {expected}");
            assert_eq!(unplaced.next_stereo(), [mono as f32; 2]);
        }
    }

    #[test]
    fn hostile_modes_are_silent_and_strikes_are_bounded() {
        let mut bank = ModalBank::<16>::default();
        for rate in [f64::NAN, f64::INFINITY, -1.0, 0.0, 8000.0, 48000.0, 384000.0] {
            for frequency in [f64::NAN, f64::INFINITY, -1.0, 0.0, 440.0, 200000.0] {
                bank.strike(rate, &[Mode { frequency_hz: frequency, decay_seconds: 0.01, amplitude: 1.0 }; 32]);
                for _ in 0..8192 {
                    let output = bank.next_sample();
                    assert!(output.is_finite() && output.abs() <= 1.00001);
                }
                bank.reset();
                assert_eq!(bank.next_sample(), 0.0);
                assert!(!bank.is_active());
            }
        }
    }

    #[test]
    fn modes_above_the_anti_alias_margin_are_not_folded_down() {
        let mut bank = ModalBank::<1>::default();
        bank.strike(8000.0, &[Mode { frequency_hz: 3600.0, decay_seconds: 1.0, amplitude: 1.0 }]);
        assert!(!bank.is_active());
        assert_eq!(bank.next_sample(), 0.0);
    }

    #[test]
    fn a_capped_decay_keeps_phase_and_follows_the_new_t60() {
        let mut bank = ModalBank::<1>::default();
        bank.strike(48000.0, &[Mode { frequency_hz: 440.0, decay_seconds: 10.0, amplitude: 0.5 }]);
        for _ in 0..480 {
            bank.next_sample();
        }
        let at_cap = 0.5 * (-1000.0_f64.ln() * 480.0 / (48000.0 * 10.0)).exp();
        bank.cap_decay(48000.0, 0.1);
        for n in 0..4800 {
            let t = (480 + n) as f64;
            let expected =
                at_cap * (-1000.0_f64.ln() * n as f64 / (48000.0 * 0.1)).exp() * (TAU * 440.0 * t / 48000.0).sin();
            assert!((f64::from(bank.next_sample()) - expected).abs() < 1e-7, "{n}");
        }
        // A longer cap changes nothing.
        let mut long = ModalBank::<1>::default();
        let mut plain = ModalBank::<1>::default();
        let mode = [Mode { frequency_hz: 220.0, decay_seconds: 0.2, amplitude: 0.5 }];
        long.strike(48000.0, &mode);
        plain.strike(48000.0, &mode);
        long.cap_decay(48000.0, 5.0);
        for _ in 0..2000 {
            assert_eq!(long.next_sample(), plain.next_sample());
        }
    }

    #[test]
    fn phased_strikes_start_each_mode_at_its_phase() {
        let mut bank = ModalBank::<2>::default();
        let modes = [
            Mode { frequency_hz: 440.0, decay_seconds: 0.5, amplitude: 0.3 },
            Mode { frequency_hz: 1234.5, decay_seconds: 0.2, amplitude: 0.2 },
        ];
        bank.strike_phased(48000.0, &modes, &[0.7, -2.1]);
        for n in 0..4800 {
            let t = n as f64;
            let expected: f64 = modes
                .iter()
                .zip([0.7, -2.1])
                .map(|(m, phase)| {
                    m.amplitude
                        * (-1000.0_f64.ln() * t / (48000.0 * m.decay_seconds)).exp()
                        * (TAU * m.frequency_hz * t / 48000.0 + phase).sin()
                })
                .sum();
            assert!((f64::from(bank.next_sample()) - expected).abs() < 2e-7, "{n}");
        }
        let mut plain = ModalBank::<2>::default();
        let mut zero = ModalBank::<2>::default();
        plain.strike(48000.0, &modes);
        zero.strike_phased(48000.0, &modes, &[]);
        for _ in 0..256 {
            assert_eq!(plain.next_sample(), zero.next_sample());
        }
    }

    #[test]
    fn gained_samples_weight_each_mode_and_unit_gains_change_nothing() {
        let modes = [
            Mode { frequency_hz: 440.0, decay_seconds: 0.5, amplitude: 0.3 },
            Mode { frequency_hz: 1234.5, decay_seconds: 0.2, amplitude: 0.2 },
        ];
        let mut plain = ModalBank::<2>::default();
        let mut unit = ModalBank::<2>::default();
        plain.strike_phased(48000.0, &modes, &[0.3, 1.0]);
        unit.strike_phased(48000.0, &modes, &[0.3, 1.0]);
        let mut gains = [1.0; 2];
        for _ in 0..512 {
            assert_eq!(plain.next_sample(), unit.next_sample_gained(&mut gains, &[0.0; 2]));
        }
        let mut one = ModalBank::<2>::default();
        let mut weighted = ModalBank::<2>::default();
        one.strike_phased(48000.0, &modes[..1], &[0.3]);
        weighted.strike_phased(48000.0, &modes, &[0.3, 1.0]);
        let mut gains = [0.0, 0.0];
        let steps = [1.0 / 256.0, 0.0];
        for n in 0..256 {
            let expected = f64::from(one.next_sample()) * n as f64 / 256.0;
            assert!((f64::from(weighted.next_sample_gained(&mut gains, &steps)) - expected).abs() < 1e-6, "{n}");
        }
    }

    #[test]
    fn retuning_keeps_phase_and_applies_an_absolute_pitch_offset() {
        let mut bank = ModalBank::<1>::default();
        bank.strike(48000.0, &[Mode { frequency_hz: 440.0, decay_seconds: 1.0, amplitude: 0.5 }]);
        for _ in 0..100 {
            bank.next_sample();
        }
        bank.transpose(48000.0, 12.0);
        bank.transpose(48000.0, 12.0);
        for n in 0..100 {
            let expected = 0.5
                * (-1000.0_f64.ln() * (100 + n) as f64 / 48000.0).exp()
                * (TAU * (440.0 * 100.0 + 880.0 * n as f64) / 48000.0).sin();
            assert!((f64::from(bank.next_sample()) - expected).abs() < 2e-8);
        }
    }
}

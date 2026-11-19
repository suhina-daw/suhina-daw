// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fixed-capacity damped sinusoidal modes. A mode is a complex rotation with radius below
//! one, so a strike is bounded by its initial amplitude. No allocation, including excitation.

use std::f64::consts::TAU;

/// One exponentially decaying mode, expressed in physical units.
#[derive(Debug, Clone, Copy)]
pub struct Mode {
    pub frequency_hz: f64,
    /// Time to decay by 60 dB, in seconds.
    pub decay_seconds: f64,
    pub amplitude: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct Resonance {
    real: f64,
    imaginary: f64,
    cosine: f64,
    sine: f64,
    frequency_hz: f64,
    radius: f64,
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

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn reset(&mut self) {
        self.modes.fill(Resonance::default());
        self.active = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

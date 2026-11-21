// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fixed-capacity damped sinusoidal modes. A mode is a complex rotation with radius below
//! one, so a strike is bounded by its initial amplitude. No allocation, including excitation.

use std::f64::consts::TAU;

pub mod model;
pub mod profile;
pub mod sustain;
pub mod tail;

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
        self.strike_with(sample_rate, modes, true);
    }

    fn strike_with(&mut self, sample_rate: f64, modes: &[Mode], normalize: bool) {
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
        if normalize {
            let normalization = amplitude_sum.max(1.0);
            for mode in &mut self.modes {
                mode.real /= normalization;
            }
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
        self.set_phases(phases);
    }

    /// Like [`strike_phased`](Self::strike_phased) with each amplitude as given (still at most 1),
    /// not normalized to a sum of 1: a fitted voice's sines are its recording's own levels, and an
    /// aftersound in opposite phase makes their absolute sum exceed their sum.
    pub fn strike_phased_exact(&mut self, sample_rate: f64, modes: &[Mode], phases: &[f64]) {
        self.strike_with(sample_rate, modes, false);
        self.set_phases(phases);
    }

    fn set_phases(&mut self, phases: &[f64]) {
        for (target, phase) in self.modes.iter_mut().zip(phases) {
            let phase = if phase.is_finite() { *phase } else { 0.0 };
            let amplitude = target.real;
            target.real = amplitude * phase.cos();
            target.imaginary = amplitude * phase.sin();
        }
    }
}

#[cfg(test)]
#[path = "modal_tests.rs"]
mod tests;

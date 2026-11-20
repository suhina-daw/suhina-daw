// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Oversampling. A waveshaper creates harmonics above Nyquist that fold back as inharmonic
//! aliasing; running it at twice the sample rate, between a half-band interpolator and
//! decimator, keeps most of them out.

use crate::flush;

/// Taps of the half-band filter. A half-band filter's center tap is 1/2 and every other tap is
/// zero, so only the odd-offset taps are stored and computed.
const TAPS: usize = 63;
/// Center of the filter, in samples at the high rate.
const CENTER: usize = (TAPS - 1) / 2;
/// Non-zero taps on one side of the center.
const HALF: usize = TAPS.div_ceil(4);
/// Kaiser window β: about 80 dB of stopband attenuation.
const KAISER_BETA: f64 = 8.0;

/// The non-zero taps at offsets ±1, ±3, ±5, … from the center (the same on both sides).
fn side_taps() -> [f64; HALF] {
    let mut taps = [0.0; HALF];
    let i0_beta = bessel_i0(KAISER_BETA);
    for (j, tap) in taps.iter_mut().enumerate() {
        let offset = (2 * j + 1) as f64;
        let sinc = (std::f64::consts::FRAC_PI_2 * offset).sin() / (std::f64::consts::PI * offset);
        let ratio = offset / CENTER as f64;
        let window = bessel_i0(KAISER_BETA * (1.0 - ratio * ratio).max(0.0).sqrt()) / i0_beta;
        *tap = sinc * window;
    }
    taps
}

/// Modified Bessel function of the first kind, order zero (series; converges fast for the β
/// used here).
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term) = (1.0, 1.0);
    for k in 1..50 {
        let f = x / (2.0 * k as f64);
        term *= f * f;
        sum += term;
        if term < 1e-12 * sum {
            break;
        }
    }
    sum
}

/// Runs a per-sample process at twice the sample rate for one channel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oversampler2x {
    taps: [f64; HALF],
    /// Recent input samples, newest last, for the interpolator.
    input: [f64; 2 * HALF],
    /// Recent high-rate samples, newest last, for the decimator. One longer than the filter, so
    /// its center falls on a sample that lines up with an input sample and the delay is a whole
    /// number of base-rate frames.
    output: [f64; TAPS + 1],
}

impl Default for Oversampler2x {
    fn default() -> Self {
        Self::new()
    }
}

impl Oversampler2x {
    /// Delay the round trip adds, in frames at the base rate.
    pub const LATENCY: u32 = CENTER as u32;

    pub fn new() -> Self {
        Self { taps: side_taps(), input: [0.0; 2 * HALF], output: [0.0; TAPS + 1] }
    }

    /// Feeds one sample, runs `process` on the two samples at the high rate, and returns one
    /// sample at the base rate.
    #[inline]
    pub fn process(&mut self, x: f64, mut process: impl FnMut(f64) -> f64) -> f64 {
        let (a, b) = self.interpolate(x);
        let a = process(a);
        let b = process(b);
        self.decimate(a, b)
    }

    /// The two high-rate samples for input `x`.
    fn interpolate(&mut self, x: f64) -> (f64, f64) {
        self.input.copy_within(1.., 0);
        if let Some(last) = self.input.last_mut() {
            *last = flush(x);
        }
        // Zero-stuffed input filtered by the half-band, times two for the lost energy. The
        // sample between two inputs uses the odd taps; the one on an input is the center tap
        // alone, a delayed copy.
        let mut between = 0.0;
        for (j, tap) in self.taps.iter().enumerate() {
            let newer = self.input.get(HALF + j).copied().unwrap_or(0.0);
            let older = self.input.get(HALF - 1 - j).copied().unwrap_or(0.0);
            between += tap * (newer + older);
        }
        let on = self.input.get(HALF).copied().unwrap_or(0.0);
        (2.0 * between, on)
    }

    /// One base-rate sample from two new high-rate samples.
    fn decimate(&mut self, a: f64, b: f64) -> f64 {
        self.output.copy_within(2.., 0);
        let len = self.output.len();
        if let Some(slot) = self.output.get_mut(len - 2) {
            *slot = flush(a);
        }
        if let Some(slot) = self.output.get_mut(len - 1) {
            *slot = flush(b);
        }
        let mut sum = 0.5 * self.output.get(CENTER).copied().unwrap_or(0.0);
        for (j, tap) in self.taps.iter().enumerate() {
            let offset = 2 * j + 1;
            let newer = self.output.get(CENTER + offset).copied().unwrap_or(0.0);
            let older = self.output.get(CENTER - offset).copied().unwrap_or(0.0);
            sum += tap * (newer + older);
        }
        sum
    }

    pub fn reset(&mut self) {
        self.input = [0.0; 2 * HALF];
        self.output = [0.0; TAPS + 1];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48_000.0;

    fn sine(hz: f64, n: usize) -> f64 {
        (std::f64::consts::TAU * hz * n as f64 / SR).sin()
    }

    /// Amplitude of `hz` in `signal` (a single DFT bin).
    fn level(signal: &[f64], hz: f64) -> f64 {
        let (mut re, mut im) = (0.0, 0.0);
        for (n, x) in signal.iter().enumerate() {
            let w = std::f64::consts::TAU * hz * n as f64 / SR;
            re += x * w.cos();
            im += x * w.sin();
        }
        2.0 * re.hypot(im) / signal.len() as f64
    }

    #[test]
    fn the_filter_has_unity_gain() {
        let taps = side_taps();
        assert!((0.5 + 2.0 * taps.iter().sum::<f64>() - 1.0).abs() < 1e-3);
    }

    #[test]
    fn an_impulse_comes_out_after_the_latency() {
        let mut o = Oversampler2x::new();
        let out: Vec<f64> = (0..100).map(|n| o.process(if n == 0 { 1.0 } else { 0.0 }, |x| x)).collect();
        let peak = out
            .iter()
            .enumerate()
            .fold((0, 0.0f64), |best, (i, &v)| if v.abs() > best.1.abs() { (i, v) } else { best });
        assert_eq!(peak.0, Oversampler2x::LATENCY as usize, "{peak:?}");
    }

    #[test]
    fn passes_the_audio_band_unchanged() {
        for hz in [100.0, 1_000.0, 10_000.0, 18_000.0] {
            let mut o = Oversampler2x::new();
            let latency = Oversampler2x::LATENCY as usize;
            let out: Vec<f64> = (0..4_800).map(|n| o.process(sine(hz, n), |x| x)).collect();
            let worst = (1_000..4_800).map(|n| (out[n] - sine(hz, n - latency)).abs()).fold(0.0, f64::max);
            assert!(worst < 0.01, "{hz} Hz: {worst}");
        }
    }

    #[test]
    fn keeps_harmonics_above_nyquist_from_folding_back() {
        // Squaring a 15 kHz sine makes 30 kHz, which at 48 kHz without oversampling would fold
        // back to 18 kHz.
        let mut o = Oversampler2x::new();
        let out: Vec<f64> = (0..48_000).map(|n| o.process(sine(15_000.0, n), |x| x * x)).collect();
        let folded = level(&out[1_000..], 18_000.0);
        assert!(folded < 1e-3, "{folded}");
        let naive: Vec<f64> = (0..48_000).map(|n| sine(15_000.0, n).powi(2)).collect();
        assert!(level(&naive[1_000..], 18_000.0) > 0.4);
    }

    #[test]
    fn broken_input_does_not_stick() {
        let mut o = Oversampler2x::new();
        o.process(f64::NAN, |x| x);
        o.process(f64::INFINITY, |x| x);
        let out: Vec<f64> = (0..200).map(|n| o.process(sine(1_000.0, n), |x| x)).collect();
        assert!(out[150..].iter().all(|v| v.is_finite() && v.abs() < 1.1));
    }
}

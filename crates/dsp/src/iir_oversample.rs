// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Oversampling with a few frames of delay. [`Oversampler2x`](crate::Oversampler2x) is linear
//! phase but delays the signal by 31 frames and rings before an impulse; this one uses an IIR
//! half-band filter instead: two parallel chains of allpasses at the base rate (a polyphase
//! elliptic half-band, after Valenzuela and Constantinides, 1983). Its delay is about
//! [`LATENCY`](IirOversampler2x::LATENCY) frames through the audio band and grows near Nyquist,
//! and its magnitude is flat to within 0.02 dB up to 0.45 of the sample rate.

use crate::flush;

/// The allpass coefficients of the half-band filter, in pairs (the first of each pair in one
/// chain, the second in the other): an elliptic design of order 9 with its transition from 0.17
/// to 0.33 of the high rate. Images are 38 dB down at 0.28 of the high rate and at least 64 dB
/// from 0.30. A gentle design: few stages, a short ring, and a delay that grows only near
/// Nyquist (0.13 frames more at 5 kHz, 0.6 at 10 kHz, 1.8 at 15 kHz, 7.2 at 20 kHz, at 44.1 kHz).
const COEFFICIENTS: [[f64; 2]; PAIRS] =
    [[0.091_994_723_790_236_25, 0.317_108_117_416_400_9], [0.584_460_827_036_122_2, 0.854_180_068_705_942_8]];
/// Allpasses per chain.
const PAIRS: usize = 2;

/// First-order allpasses `(c + z^-1) / (1 + c z^-1)` at the base rate, one per coefficient;
/// the first of each pair is in one chain, the second in the other.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Chains {
    x: [[f64; 2]; PAIRS],
    y: [[f64; 2]; PAIRS],
}

impl Chains {
    /// Runs `a` through the first chain and `b` through the second.
    #[inline]
    fn process(&mut self, a: f64, b: f64) -> (f64, f64) {
        let mut v = [a, b];
        for ((c, x), y) in COEFFICIENTS.iter().zip(&mut self.x).zip(&mut self.y) {
            for (((v, c), x), y) in v.iter_mut().zip(c).zip(x).zip(y) {
                let out = flush(c * (*v - *y) + *x);
                *x = *v;
                *y = out;
                *v = out;
            }
        }
        (v[0], v[1])
    }
}

/// Runs a per-sample process at twice the sample rate for one channel, with an IIR half-band
/// filter on the way up and on the way down.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct IirOversampler2x {
    up: Chains,
    down: Chains,
    /// The second high-rate sample of the previous frame, which the odd chain takes on the way
    /// down.
    last: f64,
}

impl IirOversampler2x {
    /// Delay of the round trip through the audio band, in frames at the base rate (the exact
    /// group delay at low frequencies is 2.19 frames).
    pub const LATENCY: u32 = 2;

    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one sample, runs `process` on the two samples at the high rate, and returns one
    /// sample at the base rate.
    #[inline]
    pub fn process(&mut self, x: f64, mut process: impl FnMut(f64) -> f64) -> f64 {
        let x = flush(x);
        let (first, second) = self.up.process(x, x);
        let first = flush(process(first));
        let second = flush(process(second));
        let (a, b) = self.down.process(first, self.last);
        self.last = second;
        0.5 * (a + b)
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Runs a per-sample process at four times the sample rate for one channel: two
/// [`IirOversampler2x`] in series, the inner one at twice the rate. The inner stage's delay is
/// half the outer one's in base frames, and its phase turn starts an octave higher.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct IirOversampler4x {
    outer: IirOversampler2x,
    inner: IirOversampler2x,
}

impl IirOversampler4x {
    /// Delay of the round trip through the audio band, in frames at the base rate (the exact
    /// group delay at low frequencies is 3.29 frames).
    pub const LATENCY: u32 = 3;

    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one sample, runs `process` on the four samples at the high rate, and returns one
    /// sample at the base rate.
    #[inline]
    pub fn process(&mut self, x: f64, mut process: impl FnMut(f64) -> f64) -> f64 {
        let Self { outer, inner } = self;
        outer.process(x, |v| inner.process(v, &mut process))
    }

    pub fn reset(&mut self) {
        *self = Self::default();
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
    fn passes_the_audio_band_at_full_level() {
        for hz in [50.0, 1_000.0, 10_000.0, 18_000.0, 21_000.0] {
            let mut o = IirOversampler2x::new();
            let out: Vec<f64> = (0..48_000).map(|n| o.process(sine(hz, n), |x| x)).collect();
            let gain = 20.0 * level(&out[4_800..], hz).log10();
            assert!(gain.abs() < 0.02, "{hz} Hz: {gain} dB");
        }
    }

    #[test]
    fn delays_the_audio_band_by_its_latency() {
        // A 1 kHz sine comes out LATENCY frames later, to within a quarter of a frame.
        let mut o = IirOversampler2x::new();
        let out: Vec<f64> = (0..4_800).map(|n| o.process(sine(1_000.0, n), |x| x)).collect();
        let lag = IirOversampler2x::LATENCY as usize;
        let worst = (1_000..4_800).map(|n| (out[n] - sine(1_000.0, n - lag)).abs()).fold(0.0, f64::max);
        assert!(worst < 0.25 * std::f64::consts::TAU * 1_000.0 / SR, "{worst}");
    }

    #[test]
    fn keeps_harmonics_above_nyquist_from_folding_back() {
        // Squaring a 15 kHz sine makes 30 kHz, which at 48 kHz without oversampling would fold
        // back to 18 kHz.
        let mut o = IirOversampler2x::new();
        let out: Vec<f64> = (0..48_000).map(|n| o.process(sine(15_000.0, n), |x| x * x)).collect();
        assert!(level(&out[1_000..], 18_000.0) < 1e-3, "{}", level(&out[1_000..], 18_000.0));
        // Turning the high-rate signal upside down every other sample moves 1 kHz to 47 kHz,
        // all of which the way down removes.
        let mut sign = 1.0;
        let out: Vec<f64> = (0..48_000)
            .map(|n| {
                o.process(sine(1_000.0, n), |x| {
                    sign = -sign;
                    sign * x
                })
            })
            .collect();
        let rest = out[4_800..].iter().fold(0.0f64, |m, v| m.max(v.abs()));
        assert!(rest < 1e-3, "{rest}");
    }

    #[test]
    fn broken_input_does_not_stick() {
        let mut o = IirOversampler2x::new();
        o.process(f64::NAN, |x| x);
        o.process(f64::INFINITY, |x| x);
        o.process(1.0, |_| f64::NAN);
        let out: Vec<f64> = (0..200).map(|n| o.process(sine(1_000.0, n), |x| x)).collect();
        assert!(out[150..].iter().all(|v| v.is_finite() && v.abs() < 1.1));
    }

    #[test]
    fn four_times_passes_the_audio_band_at_full_level() {
        for hz in [50.0, 1_000.0, 10_000.0, 18_000.0, 21_000.0] {
            let mut o = IirOversampler4x::new();
            let out: Vec<f64> = (0..48_000).map(|n| o.process(sine(hz, n), |x| x)).collect();
            let gain = 20.0 * level(&out[4_800..], hz).log10();
            assert!(gain.abs() < 0.04, "{hz} Hz: {gain} dB");
        }
    }

    #[test]
    fn four_times_delays_the_audio_band_by_its_latency() {
        // As the 2x test: a 1 kHz sine comes out LATENCY frames later, to within a third of a
        // frame (the exact delay is 3.29 frames).
        let mut o = IirOversampler4x::new();
        let out: Vec<f64> = (0..4_800).map(|n| o.process(sine(1_000.0, n), |x| x)).collect();
        let lag = IirOversampler4x::LATENCY as usize;
        let worst = (1_000..4_800).map(|n| (out[n] - sine(1_000.0, n - lag)).abs()).fold(0.0, f64::max);
        assert!(worst < 0.34 * std::f64::consts::TAU * 1_000.0 / SR, "{worst}");
    }

    #[test]
    fn four_times_folds_back_far_less_of_a_hard_clip() {
        // A 10 kHz sine at -6 dBFS driven 24 dB into a hard clip: at 2x its 9th harmonic
        // (90 kHz) folds back from 96 kHz to 6 kHz, and the 11th to 18 kHz; at 4x the first fold
        // lands from 192 kHz, on the 19th harmonic, which is much weaker.
        let clip = |x: f64| (x * 15.85 * 0.5).clamp(-1.0, 1.0);
        let worst = |out: &[f64]| {
            let fundamental = level(out, 10_000.0);
            (1..48)
                .map(|k| f64::from(k) * 500.0)
                .filter(|hz| (hz - 10_000.0).abs() > 1.0)
                .map(|hz| 20.0 * (level(out, hz) / fundamental).log10())
                .fold(f64::NEG_INFINITY, f64::max)
        };
        let mut two = IirOversampler2x::new();
        let out: Vec<f64> = (0..48_000).map(|n| two.process(sine(10_000.0, n), clip)).collect();
        let at_two = worst(&out[4_800..]);
        let mut four = IirOversampler4x::new();
        let out: Vec<f64> = (0..48_000).map(|n| four.process(sine(10_000.0, n), clip)).collect();
        let at_four = worst(&out[4_800..]);
        assert!(at_four < -30.0 && at_four < at_two - 10.0, "2x {at_two} dB, 4x {at_four} dB");
    }
}

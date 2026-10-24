// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Experimental fixed eight-line reverb network, not a production device.
//! See `docs/spec/reverb-prototype.md` for the research basis and limitations.

use crate::{DelayLine, FirstOrder, FirstOrderCoefficients};

#[path = "fdn_scatter.rs"]
mod scatter;
use scatter::Scatter;

const DELAYS: [usize; 8] = [1429, 1777, 2081, 2383, 2719, 3083, 3469, 3911];
const NORM: f32 = 0.353_553_38;
const LEFT: [f32; 8] = [1., 1., 1., 1., -1., -1., -1., -1.];
const RIGHT: [f32; 8] = [1., -1., 1., -1., 1., -1., 1., -1.];

/// Fixed settings; construct a replacement off the audio thread to change them.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub sample_rate: u32,
    pub decay_seconds: f64,
    pub room_scale: f64,
    /// Asymptotic high-frequency decay divided by low-frequency decay.
    pub high_decay_ratio: f64,
    pub damping_hz: f64,
    /// Fixed optional scattering; zero retains the original network.
    pub loop_diffusion: f32,
    /// Fixed modal excitation/observation angles in radians; zero preserves the baseline.
    pub input_rotation: f64,
    pub output_rotation: f64,
    /// Optional orthogonal feedback rotation, prepared off-thread.
    pub feedback_rotation: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            decay_seconds: 2.,
            room_scale: 1.,
            high_decay_ratio: 1.,
            damping_hz: 4500.,
            loop_diffusion: 0.,
            input_rotation: 0.,
            output_rotation: 0.,
            feedback_rotation: 0.,
        }
    }
}

/// Invalid construction settings; processing has no fallible resource operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsError;

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FDN settings outside finite supported ranges")
    }
}

impl std::error::Error for SettingsError {}

impl Settings {
    fn validate(self) -> Result<Self, SettingsError> {
        if (8000..=384_000).contains(&self.sample_rate)
            && (0.2..=20.).contains(&self.decay_seconds)
            && (0.5..=2.).contains(&self.room_scale)
            && (0.1..=1.).contains(&self.high_decay_ratio)
            && (0. ..=0.7).contains(&self.loop_diffusion)
            && (-std::f64::consts::PI..=std::f64::consts::PI).contains(&self.input_rotation)
            && (-std::f64::consts::PI..=std::f64::consts::PI).contains(&self.output_rotation)
            && (-std::f64::consts::PI..=std::f64::consts::PI).contains(&self.feedback_rotation)
            && (20.0..=0.45 * f64::from(self.sample_rate)).contains(&self.damping_hz)
        {
            Ok(self)
        } else {
            Err(SettingsError)
        }
    }
}

/// State allocated once during construction. Processing does not allocate or lock.
#[derive(Debug)]
pub struct Fdn {
    lines: [DelayLine; 8],
    lengths: [usize; 8],
    gains: [f32; 8],
    damping: [FirstOrder; 8],
    scatter: [Scatter; 8],
    rotations: Option<[(f32, f32); 4]>,
    input_weights: [f32; 8],
    output_weights: [[f32; 8]; 2],
}

impl Fdn {
    /// Allocates bounded history off-thread. Rejects invalid settings before allocation.
    pub fn new(settings: Settings) -> Result<Self, SettingsError> {
        let s = settings.validate()?;
        let rate = f64::from(s.sample_rate);
        let lengths = DELAYS.map(|n| (n as f64 * rate / 48_000. * s.room_scale).round() as usize);
        let gains = lengths.map(|n| 10_f64.powf(-3. * n as f64 / rate / s.decay_seconds) as f32);
        let damping = lengths.map(|n| {
            let exponent = -3. * n as f64 / rate / s.decay_seconds * (1. / s.high_decay_ratio - 1.);
            FirstOrder::new(FirstOrderCoefficients::high_shelf(rate, s.damping_hz, 10_f64.powf(exponent)))
        });
        let scatter = [43., 59., 73., 89., 107., 127., 149., 173.].map(|n| {
            Scatter::new((n * rate / 48_000.).round().max(1.) as usize, rate, s.decay_seconds, s.loop_diffusion)
        });
        let (si, ci) = s.input_rotation.sin_cos();
        let input_weights = std::array::from_fn(|i| (ci + si * f64::from(RIGHT[i])) as f32);
        let (so, co) = s.output_rotation.sin_cos();
        let alternate_left = [1., 1., -1., -1., 1., 1., -1., -1.];
        let alternate_right = [1., -1., -1., 1., 1., -1., -1., 1.];
        let output_weights = [
            std::array::from_fn(|i| (co * f64::from(LEFT[i]) + so * alternate_left[i]) as f32),
            std::array::from_fn(|i| (co * f64::from(RIGHT[i]) + so * alternate_right[i]) as f32),
        ];
        let rotations = (s.feedback_rotation != 0.).then(|| {
            [1., 0.7, -0.9, -0.5].map(|factor| {
                let (sin, cos) = (factor * s.feedback_rotation).sin_cos();
                (sin as f32, cos as f32)
            })
        });
        Ok(Self {
            rotations,
            input_weights,
            output_weights,
            scatter,
            lines: std::array::from_fn(|i| DelayLine::new(lengths[i])),
            lengths,
            gains,
            damping,
        })
    }

    /// Allocated delay history only, excluding the small fixed state and allocator metadata.
    pub fn history_bytes(&self) -> usize {
        self.lines.iter().map(|line| (line.capacity() as usize + 3) * size_of::<f32>()).sum::<usize>()
            + self.scatter.iter().map(Scatter::history_bytes).sum::<usize>()
    }

    /// Clears history without allocation or resource destruction.
    pub fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        for section in &mut self.scatter {
            section.reset();
        }
        for filter in &mut self.damping {
            filter.reset();
        }
    }

    /// One wet-only stereo frame. Input is mono-summed and limited to +/-4 per side.
    pub fn process_frame(&mut self, input: [f32; 2]) -> [f32; 2] {
        let mono = (safe_input(input[0]) + safe_input(input[1])) * 0.5 * NORM;
        let mut values = [0.; 8];
        let mut output = [0.; 2];
        for (i, ((line, length), filter)) in self.lines.iter().zip(self.lengths).zip(&mut self.damping).enumerate() {
            let delayed = self.scatter[i].process(line.read(length));
            output[0] += delayed * self.output_weights[0][i] * NORM;
            output[1] += delayed * self.output_weights[1][i] * NORM;
            values[i] = (filter.process(f64::from(delayed)) * f64::from(self.gains[i])) as f32;
        }
        mix(&mut values);
        // Permutation prevents the same Hadamard coordinate from returning to itself every trip.
        values.rotate_left(1);
        if let Some(rotations) = self.rotations {
            rotate_pairs(&mut values, rotations);
        }
        for ((line, value), weight) in self.lines.iter_mut().zip(values).zip(self.input_weights) {
            line.push(mono * weight + value);
        }
        output
    }

    /// Interleaved stereo, in place. An incomplete final frame is left untouched.
    pub fn process(&mut self, samples: &mut [f32]) {
        for [left, right] in samples.as_chunks_mut::<2>().0 {
            let out = self.process_frame([*left, *right]);
            *left = out[0];
            *right = out[1];
        }
    }
}

fn safe_input(value: f32) -> f32 {
    if value.is_finite() { value.clamp(-4., 4.) } else { 0. }
}

fn rotate_pairs(values: &mut [f32; 8], rotations: [(f32, f32); 4]) {
    for ([a, b], (sin, cos)) in values.as_chunks_mut::<2>().0.iter_mut().zip(rotations) {
        let before = *a;
        *a = cos * before - sin * *b;
        *b = sin * before + cos * *b;
    }
}

fn mix(values: &mut [f32; 8]) {
    for step in [1, 2, 4] {
        for group in values.chunks_exact_mut(step * 2) {
            let (a, b) = group.split_at_mut(step);
            for (x, y) in a.iter_mut().zip(b) {
                let before = *x;
                *x = before + *y;
                *y = before - *y;
            }
        }
    }
    for value in values {
        *value *= NORM;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[path = "fdn_tests.rs"]
mod tests;

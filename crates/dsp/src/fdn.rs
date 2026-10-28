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
    /// Scale of the lossy allpass histories inside the feedback loop.
    pub loop_diffusion_scale: f64,
    /// Subtract nominal scattering history from main delays, prepared off-thread.
    pub loop_delay_compensation: bool,
    /// Fixed modal excitation/observation angles in radians; zero preserves the baseline.
    pub input_rotation: f64,
    /// Optional prepared eight-line injection; replaces input_rotation.
    pub input_weights: Option<[f32; 8]>,
    pub output_rotation: f64,
    /// Optional orthogonal feedback rotation, prepared off-thread.
    pub feedback_rotation: f64,
    /// Optional prepared orthogonal eight-line feedback matrix.
    pub feedback_matrix: Option<[[f32; 8]; 8]>,
    /// Output observation spread along existing lines; zero preserves the original path.
    pub output_tap_spread: f64,
    /// Optional independent finite output projections; replaces output_rotation.
    pub output_weights: Option<[[f32; 8]; 2]>,
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
            loop_diffusion_scale: 1.,
            loop_delay_compensation: false,
            input_rotation: 0.,
            input_weights: None,
            output_rotation: 0.,
            feedback_rotation: 0.,
            feedback_matrix: None,
            output_tap_spread: 0.,
            output_weights: None,
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
            && (0.25..=32.).contains(&self.loop_diffusion_scale)
            && (-std::f64::consts::PI..=std::f64::consts::PI).contains(&self.input_rotation)
            && (-std::f64::consts::PI..=std::f64::consts::PI).contains(&self.output_rotation)
            && (-std::f64::consts::PI..=std::f64::consts::PI).contains(&self.feedback_rotation)
            && (0. ..=1.).contains(&self.output_tap_spread)
            && self
                .output_weights
                .is_none_or(|channels| channels.into_iter().flatten().all(|x| (-8. ..=8.).contains(&x)))
            && self.input_weights.is_none_or(|weights| weights.into_iter().all(|x| (-8. ..=8.).contains(&x)))
            && self.feedback_matrix.is_none_or(valid_matrix)
            && (self.output_tap_spread == 0. || self.loop_diffusion == 0.)
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
pub struct Network<const N: usize> {
    lines: [DelayLine; N],
    lengths: [usize; N],
    gains: [f32; N],
    damping: [FirstOrder; N],
    scatter: [Scatter; N],
    rotations: Option<[(f32, f32); 4]>,
    feedback_matrix: Option<[[f32; 8]; 8]>,
    input_weights: [f32; N],
    output_weights: [[f32; N]; 2],
    output_taps: Option<[usize; N]>,
}

impl<const N: usize> Network<N> {
    /// Allocates bounded history off-thread. Rejects invalid settings before allocation.
    pub fn new(settings: Settings) -> Result<Self, SettingsError> {
        if ![8, 16].contains(&N)
            || (N != 8
                && (settings.output_weights.is_some()
                    || settings.feedback_matrix.is_some()
                    || settings.input_weights.is_some()))
        {
            return Err(SettingsError);
        }
        let s = settings.validate()?;
        let rate = f64::from(s.sample_rate);
        let mut lengths: [usize; N] = std::array::from_fn(|i| {
            let base = if N == 8 {
                DELAYS[i]
            } else {
                [1429, 1597, 1777, 1931, 2081, 2221, 2383, 2549, 2719, 2903, 3083, 3259, 3469, 3671, 3911, 4127][i]
            };
            (base as f64 * rate / 48_000. * s.room_scale).round() as usize
        });
        let scatter_lengths: [usize; N] = std::array::from_fn(|i| {
            let n = [43., 59., 73., 89., 107., 127., 149., 173., 191., 211., 233., 251., 271., 293., 313., 337.][i];
            (n * rate / 48_000. * s.loop_diffusion_scale).round().max(1.) as usize
        });
        if s.loop_delay_compensation && s.loop_diffusion != 0. {
            for (length, scatter_length) in lengths.iter_mut().zip(scatter_lengths) {
                *length = length.checked_sub(scatter_length).filter(|n| *n > 0).ok_or(SettingsError)?;
            }
        }
        let gains = lengths.map(|n| 10_f64.powf(-3. * n as f64 / rate / s.decay_seconds) as f32);
        let damping = lengths.map(|n| {
            let exponent = -3. * n as f64 / rate / s.decay_seconds * (1. / s.high_decay_ratio - 1.);
            FirstOrder::new(FirstOrderCoefficients::high_shelf(rate, s.damping_hz, 10_f64.powf(exponent)))
        });
        let scatter =
            std::array::from_fn(|i| Scatter::new(scatter_lengths[i], rate, s.decay_seconds, s.loop_diffusion));
        let (si, ci) = s.input_rotation.sin_cos();
        let input_weights = std::array::from_fn(|i| {
            s.input_weights.map_or_else(|| (ci + si * f64::from(RIGHT[i % 8])) as f32, |weights| weights[i])
        });
        let (so, co) = s.output_rotation.sin_cos();
        let alternate_left = [1., 1., -1., -1., 1., 1., -1., -1.];
        let alternate_right = [1., -1., -1., 1., 1., -1., -1., 1.];
        let output_weights = [
            std::array::from_fn(|i| {
                s.output_weights.map_or_else(
                    || {
                        (co * f64::from(if N == 8 {
                            LEFT[i]
                        } else if i < 8 {
                            1.
                        } else {
                            -1.
                        }) + so * alternate_left[i % 8]) as f32
                    },
                    |w| w[0][i],
                )
            }),
            std::array::from_fn(|i| {
                s.output_weights
                    .map_or_else(|| (co * f64::from(RIGHT[i % 8]) + so * alternate_right[i % 8]) as f32, |w| w[1][i])
            }),
        ];
        let rotations = (s.feedback_rotation != 0.).then(|| {
            [1., 0.7, -0.9, -0.5].map(|factor| {
                let (sin, cos) = (factor * s.feedback_rotation).sin_cos();
                (sin as f32, cos as f32)
            })
        });
        let output_taps = (s.output_tap_spread != 0.).then(|| {
            let fractions = [0., 0.13, 0.47, 0.71, 0.29, 0.89, 0.53, 1.];
            std::array::from_fn(|i| {
                (lengths[i] as f64 * (1. - s.output_tap_spread * fractions[i % 8])).round().max(1.) as usize
            })
        });
        Ok(Self {
            output_taps,
            rotations,
            feedback_matrix: s.feedback_matrix,
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
        let mono = (safe_input(input[0]) + safe_input(input[1])) * 0.5 * norm::<N>();
        let mut values = [0.; N];
        let mut output = [0.; 2];
        for (i, ((line, length), filter)) in self.lines.iter().zip(self.lengths).zip(&mut self.damping).enumerate() {
            let delayed = self.scatter[i].process(line.read(length));
            // Observation never changes the sample sent back through the feedback loop.
            let observed = self.output_taps.as_ref().map_or(delayed, |taps| line.read(taps[i]));
            output[0] += observed * self.output_weights[0][i] * norm::<N>();
            output[1] += observed * self.output_weights[1][i] * norm::<N>();
            values[i] = (filter.process(f64::from(delayed)) * f64::from(self.gains[i])) as f32;
        }
        if let Some(matrix) = &self.feedback_matrix {
            let before = values;
            for (output, row) in values.iter_mut().zip(matrix) {
                *output = row.iter().zip(before).map(|(a, b)| a * b).sum();
            }
        } else {
            mix(&mut values);
            // Permute and rotate the original fast path only.
            values.rotate_left(1);
            if let Some(rotations) = self.rotations {
                rotate_pairs(&mut values, rotations);
            }
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

fn valid_matrix(matrix: [[f32; 8]; 8]) -> bool {
    matrix.iter().flatten().all(|x| (-1. ..=1.).contains(x))
        && matrix.iter().enumerate().all(|(i, a)| {
            matrix.iter().enumerate().all(|(j, b)| {
                let dot = a.iter().zip(b).map(|(x, y)| f64::from(*x) * f64::from(*y)).sum::<f64>();
                (dot - if i == j { 1. } else { 0. }).abs() <= 1e-5
            })
        })
}

fn safe_input(value: f32) -> f32 {
    if value.is_finite() { value.clamp(-4., 4.) } else { 0. }
}

fn rotate_pairs<const N: usize>(values: &mut [f32; N], rotations: [(f32, f32); 4]) {
    for ([a, b], (sin, cos)) in values.as_chunks_mut::<2>().0.iter_mut().zip(rotations.into_iter().cycle()) {
        let before = *a;
        *a = cos * before - sin * *b;
        *b = sin * before + cos * *b;
    }
}

fn mix<const N: usize>(values: &mut [f32; N]) {
    for step in [1, 2, 4, 8].into_iter().filter(|step| *step < N) {
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
        *value *= norm::<N>();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[path = "fdn_tests.rs"]
mod tests;

/// Original eight-line research network.
pub type Fdn = Network<8>;
/// Increased-density research network using the same blocks.
pub type Fdn16 = Network<16>;
fn norm<const N: usize>() -> f32 {
    if N == 8 { NORM } else { 0.25 }
}

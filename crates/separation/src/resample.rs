// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Resampling between a source's rate and a model's (docs/spec/stem-separation.md › Resampling):
//! band-limited interpolation with a Blackman-windowed sinc, read from a table of the kernel at
//! `PHASES` points per input frame with linear interpolation between them.
//!
//! The output frame `j` sits at input position `j × from / to`, computed in integers, so the
//! positions never drift and a run is the same on every call. The cut-off is a little below the
//! lower of the two Nyquist frequencies, so nothing folds back when going down; going up, the
//! signal stays as it was below the old Nyquist frequency.

/// Zero crossings of the sinc on each side of the center, at the cut-off.
const ZEROS: usize = 16;
/// Table points per input frame.
const PHASES: usize = 512;
/// The cut-off as a share of the lower Nyquist frequency; above it is the transition band.
const CUTOFF: f64 = 0.94;

/// Resamples a signal from one rate to another.
pub struct Resampler {
    from: u64,
    to: u64,
    /// Input frames on each side of the output position that the kernel reaches.
    half: usize,
    /// The kernel at distances `-half ..= half` input frames, `PHASES` points per frame.
    table: Vec<f32>,
}

impl Resampler {
    /// A resampler from `from` Hz to `to` Hz. Both are checked by the caller to lie within
    /// [`crate::shape::SAMPLE_RATE_RANGE`], which bounds the table's size.
    pub fn new(from: u32, to: u32) -> Self {
        let (from, to) = (u64::from(from.max(1)), u64::from(to.max(1)));
        // The cut-off in cycles per input frame, relative to the input's Nyquist frequency.
        let cutoff = CUTOFF * (to as f64 / from as f64).min(1.0);
        let half = (ZEROS as f64 / cutoff).ceil() as usize;
        let points = 2 * half * PHASES + 1;
        let table = (0..points)
            .map(|i| {
                let distance = i as f64 / PHASES as f64 - half as f64;
                let x = cutoff * distance;
                let sinc = if x == 0.0 {
                    1.0
                } else {
                    let px = std::f64::consts::PI * x;
                    px.sin() / px
                };
                // Blackman, over the whole span of the table.
                let t = i as f64 / (points - 1) as f64;
                let tau = std::f64::consts::TAU;
                let window = 0.42 - 0.5 * (tau * t).cos() + 0.08 * (2.0 * tau * t).cos();
                (cutoff * sinc * window) as f32
            })
            .collect();
        Resampler { from, to, half, table }
    }

    /// The length of `frames` input frames at the output rate, rounded up.
    pub fn output_len(&self, frames: usize) -> usize {
        let scaled = (frames as u128 * u128::from(self.to)).div_ceil(u128::from(self.from));
        usize::try_from(scaled).unwrap_or(usize::MAX)
    }

    /// `input` at the output rate, `frames` frames long; past the input's ends it is silence.
    pub fn process(&self, input: &[f32], frames: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(frames);
        let half = self.half as i64;
        let len = input.len() as i64;
        for j in 0..frames as u64 {
            // The position j × from / to as a whole frame and a remainder over `to`.
            let scaled = u128::from(j) * u128::from(self.from);
            let whole = (scaled / u128::from(self.to)) as i64;
            let rest = (scaled % u128::from(self.to)) as u64;
            // The fraction in table points, as a whole point and a fraction for interpolation.
            let point_scaled = u128::from(rest) * PHASES as u128;
            let point = (point_scaled / u128::from(self.to)) as i64;
            let between = (point_scaled % u128::from(self.to)) as f64 / self.to as f64;
            let mut acc = 0.0f64;
            let mut gain = 0.0f64;
            // Taps k with distance (whole + fraction) - k inside the kernel's reach.
            for k in (whole - half + 1)..=(whole + half) {
                let distance_points = (whole - k + half) * PHASES as i64 + point;
                let Ok(at) = usize::try_from(distance_points) else { continue };
                let (Some(a), Some(b)) = (self.table.get(at), self.table.get(at + 1)) else { continue };
                let tap = f64::from(*a) + (f64::from(*b) - f64::from(*a)) * between;
                gain += tap;
                if (0..len).contains(&k) {
                    acc += tap * f64::from(input.get(k as usize).copied().unwrap_or(0.0));
                }
            }
            // The kernel's sum varies a little with the fraction; dividing it out keeps a constant
            // signal exactly constant.
            out.push(if gain.abs() > f64::EPSILON { (acc / gain) as f32 } else { 0.0 });
        }
        out
    }
}

// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Convolution with an impulse response (an IR), by uniform partitioned FFT convolution: the
//! IR's windows run through the frequency domain and the pieces add up, so a long IR costs a
//! small, even amount of CPU per sample. One channel; a device keeps one per side.
//!
//! The FFT work comes from `fft-convolver` (a port of the HiFi-LoFi FFTConvolver, MIT), whose
//! `process`, `set_response` and `reset` are allocation-free; everything that allocates happens
//! in [`Convolver::init`], which a device runs in its own `prepare`-time construction.

use std::sync::Arc;

use crate::Resampler;
use crate::resampler::HALF_TAPS;

pub use fft_convolver::FFTConvolverError;

/// How many frames one uniform partition spans. Smaller pairs with less CPU spent late; 256
/// keeps a cab IR's mix even at busy rates.
pub const PARTITION: usize = 256;
/// The longest IR a convolver holds, in frames at its own rate (about eight seconds at 48 kHz).
pub const MAX_FRAMES: usize = 400_000;

/// An impulse response, split into one sample list per channel. The session makes it out of a
/// decoded `.wav`; a device's two sides each take one channel's list.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ir {
    pub channels: u16,
    pub sample_rate: u32,
    /// One boxed list per channel, each `frames` long.
    pub sides: Arc<[Box<[f32]>]>,
}

impl Ir {
    /// An IR from interleaved samples; more channels than the data holds, or data that does
    /// not fill whole frames, is dropped, never read past what is there.
    pub fn from_interleaved(channels: u16, frames: usize, sample_rate: u32, data: &[f32]) -> Ir {
        let channels = channels.max(1);
        let frames = frames.clamp(0, MAX_FRAMES);
        let mut sides: Vec<Box<[f32]>> = vec![vec![0.0; frames].into_boxed_slice(); usize::from(channels)];
        let used = frames.min(data.len() / usize::from(channels)).saturating_mul(usize::from(channels));
        for (index, &value) in data.iter().take(used).enumerate() {
            let side = index % usize::from(channels);
            let frame = index / usize::from(channels);
            sides[side][frame] = if value.is_finite() { value } else { 0.0 };
        }
        Ir { channels, sample_rate, sides: sides.into() }
    }

    /// The frames each side holds.
    pub fn frames(&self) -> usize {
        self.sides.first().map_or(0, |side| side.len())
    }

    /// One side's samples.
    pub fn side(&self, side: usize) -> &[f32] {
        match self.sides.get(side) {
            Some(s) => s,
            None => &[],
        }
    }

    /// The IR with its samples moved to `project_rate`, when its own rate is another. Allocates
    /// and reads everything: run it off the audio thread, as an IR's load is. The same rate
    /// comes back unchanged.
    pub fn resampled_to(&self, project_rate: u32) -> Ir {
        if self.sample_rate == project_rate || self.sample_rate == 0 {
            return self.clone();
        }
        let mut sides: Vec<Box<[f32]>> = Vec::with_capacity(usize::from(self.channels.max(1)));
        for index in 0..usize::from(self.channels.max(1)) {
            let side = self.side(index);
            let mut leg = Resampler::new(f64::from(self.sample_rate), f64::from(project_rate.max(1)));
            // The room grows when a feed's production would outsurr it; 16 frames of margin
            // cover the kernels.
            let mut out: Vec<f64> = vec![0.0; side.len() * 2 + HALF_TAPS * 2 + 16];
            let mut staged = vec![0.0f64; 1024];
            let (mut at, mut from) = (0usize, 0usize);
            while from < side.len() {
                let take = 1024usize.min(side.len() - from);
                for (slot, value) in staged.iter_mut().zip(&side[from..from + take]) {
                    *slot = f64::from(*value);
                }
                at += leg.process(&staged[..take], &mut out[at..]);
                from += take;
                if at + staged.len() + HALF_TAPS + 8 > out.len() {
                    out.resize(out.len() + staged.len() * 2, 0.0);
                }
            }
            // The kernel's tail: what waited for input that now has all been fed. The IR keeps
            // the length its time says, and at least one frame when it had any.
            if out.len() < at + HALF_TAPS * 2 + 16 {
                out.resize(at + HALF_TAPS * 2 + 16, 0.0);
            }
            at += leg.flush(&mut out[at..]);
            let length =
                (side.len() as f64 * f64::from(project_rate.max(1)) / f64::from(self.sample_rate)).round() as usize;
            out.truncate(at.min(length.max(usize::from(!side.is_empty()))));
            sides.push(out.into_iter().map(|v| v as f32).collect::<Vec<f32>>().into_boxed_slice());
        }
        Ir { channels: self.channels.max(1), sample_rate: project_rate.max(1), sides: sides.into() }
    }
}

/// Convolves one channel's signal with an IR. Delay is the IR's own: the output sample at `n`
/// is the input around `n` through the IR's head, and what the IR holds back is its sound.
#[derive(Debug, Default)]
pub struct Convolver {
    inner: fft_convolver::FFTConvolver<f32>,
}

impl Convolver {
    /// Gets ready to convolve with `ir` at partitions of [`PARTITION`], allocating for the IR's
    /// length; run it off the audio thread. An empty IR passes the signal, scaled by nothing,
    /// silently.
    pub fn init(&mut self, ir: &[f32]) -> Result<(), FFTConvolverError> {
        self.inner.init(PARTITION, ir)
    }

    /// Convolves `input` into `output` in place, any buffer sizes, allocation-free.
    #[inline]
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        // The convolver errors only on a wrong `init`; host-fed buffers are fine, per its
        // contract, and a mismatch is treated as no output this time.
        let _ = self.inner.process(input, output);
        for value in output.iter_mut() {
            *value = if value.is_finite() { *value } else { 0.0 };
        }
    }

    /// Empties the convolution's memory (its recent inputs and pending tails).
    pub fn reset(&mut self) {
        self.inner.reset();
    }
}

#[cfg(test)]
#[path = "convolver_tests.rs"]
mod tests;

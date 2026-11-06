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
            // The kernel's tail: what waited for input that has now all been fed.
            loop {
                if at + HALF_TAPS + 4 > out.len() {
                    out.resize(out.len() + 256, 0.0);
                }
                let n = leg.process(&[], &mut out[at..]);
                if n == 0 {
                    break;
                }
                at += n;
            }
            out.truncate(at);
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
mod tests {
    use super::*;

    /// The plain linear convolution the convolver must reproduce: `y[n] = sum h[k] * x[n-k]`.
    /// `x` is the whole input stream handed over so far.
    fn naive(x: &[f32], h: &[f32]) -> Vec<f32> {
        let mut y = vec![0.0f32; x.len()];
        for (n, y_n) in y.iter_mut().enumerate() {
            let mut sum = 0.0;
            for (k, &h_k) in h.iter().enumerate() {
                let at = n as isize - k as isize;
                if at < 0 {
                    break;
                }
                sum += (x[at as usize] as f64) * (h_k as f64);
            }
            *y_n = sum as f32;
        }
        y
    }

    fn chunks_of(x: &[f32], n: usize) -> Vec<Vec<f32>> {
        x.chunks(n).map(|c| c.to_vec()).collect::<Vec<_>>()
    }

    #[test]
    fn a_convolved_stream_equals_the_naive_convolution() {
        let h: Vec<f32> =
            (0..2_000).map(|k| (((k as f64) * 0.013).sin() * 0.01) as f32).chain(std::iter::once(1.0)).collect();
        let x: Vec<f32> =
            (0..8_000).map(|n| (((n as f64) * 0.0021).sin() + ((n as f64) * 0.0107).sin() * 0.5) as f32).collect();
        let want = naive(&x, &h);
        let mut convolver = Convolver::default();
        convolver.init(&h).expect("init");
        let mut got = vec![0.0f32; 0];
        for chunk in chunks_of(&x, 97) {
            let mut out = vec![0.0f32; chunk.len()];
            convolver.process(&chunk, &mut out);
            got.extend(out);
        }
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(&want) {
            assert!((g - w).abs() < 1e-4, "{g} vs {w}");
        }
    }

    #[test]
    fn an_impulse_response_of_one_gives_the_input_back() {
        let h = [1.0f32];
        let x: Vec<f32> = (0..500).map(|n| ((n as f64) * 0.11).sin() as f32).collect();
        let mut convolver = Convolver::default();
        convolver.init(&h).expect("init");
        let mut got = Vec::new();
        for chunk in chunks_of(&x, 64) {
            let mut out = vec![0.0f32; chunk.len()];
            convolver.process(&chunk, &mut out);
            got.extend(out);
        }
        for (g, w) in got.iter().zip(&x) {
            assert!((g - w).abs() < 1e-5, "{g} vs {w}");
        }
    }

    #[test]
    fn an_ir_beyond_one_partition_lines_up_across_chunk_sizes() {
        // The same input in chunks of one size and in chunks of another gives the same stream.
        let h: Vec<f32> = (0..999).map(|k| (((k as f64) * 0.05).cos() * 0.03) as f32).collect();
        let x: Vec<f32> = (0..3_000).map(|n| (((n as f64) * 0.0023).sin()) as f32).collect();
        let want = naive(&x, &h);
        let mut sizes = Convolver::default();
        sizes.init(&h).expect("init");
        let mut got = Vec::new();
        for chunk in chunks_of(&x, 64) {
            let mut out = vec![0.0f32; chunk.len()];
            sizes.process(&chunk, &mut out);
            got.extend(out);
        }
        assert_eq!(got.len(), want.len());
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            assert!((g - w).abs() < 1e-4, "chunked vs naive at {i}: {g} vs {w}");
        }
        // And the odd-tail last chunk, as a project's end has.
        let mut but_first = Convolver::default();
        but_first.init(&h).expect("init");
        let first = x.len() - 7;
        let mut head = vec![0.0f32; first];
        but_first.process(&x[..first], &mut head);
        let mut tail = vec![0.0f32; 7];
        but_first.process(&x[first..], &mut tail);
        let combined: Vec<f32> = head.into_iter().chain(tail).collect();
        for (i, (g, w)) in combined.iter().zip(&want).enumerate() {
            assert!((g - w).abs() < 1e-4, "split end vs naive at {i}: {g} vs {w}");
        }
    }

    #[test]
    fn an_ir_tells_its_sides_apart() {
        let ir = Ir::from_interleaved(2, 5, 48_000, &[1.0, 10.0, 2.0, 20.0, 3.0, 30.0, 4.0, 40.0, 5.0, 50.0]);
        assert_eq!(ir.frames(), 5);
        assert_eq!(ir.side(0), &[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(ir.side(1), &[10.0, 20.0, 30.0, 40.0, 50.0]);
        assert!(ir.side(2).is_empty());
        // Hostile numbers and a short buffer do not panic.
        let ir = Ir::from_interleaved(2, 4, 48_000, &[f32::NAN, 1.0, 2.0]);
        // Only one whole frame pair fits; a NaN frame is silence.
        assert_eq!(ir.side(0), &[0.0, 0.0, 0.0, 0.0]);
        assert_eq!(ir.side(1), &[1.0, 0.0, 0.0, 0.0]);
        let ir = Ir::from_interleaved(0, 3, 48_000, &[]);
        assert_eq!(ir.frames(), 3);
    }

    #[test]
    fn a_zero_led_ir_keeps_only_what_it_has() {
        // An IR whose first samples are zero stays silent at first, as the IR says.
        let mut h = vec![0.0f32; 1_000];
        h[700] = 1.0;
        let x: Vec<f32> = (0..2_000).map(|n| ((n as f64) * 0.09).sin() as f32).collect();
        let mut convolver = Convolver::default();
        convolver.init(&h).expect("init");
        let mut got = Vec::new();
        for chunk in chunks_of(&x, 512) {
            let mut out = vec![0.0f32; chunk.len()];
            convolver.process(&chunk, &mut out);
            got.extend(out);
        }
        let want = naive(&x, &h);
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            assert!((g - w).abs() < 1e-4, "{i}: {g} vs {w}");
        }
    }
}

// SPDX-License-Identifier: MIT

//! A self-normalising short-time Fourier transform with Kaiser windows and a half-bin-shifted
//! ("modified") real FFT, ported from Signalsmith Linear's `DynamicSTFT` (MIT, see
//! `LICENSE-signalsmith.txt`).
//!
//! The spectrum has `fft_samples / 2` bins at frequencies `(b + 0.5) / fft_samples` cycles per
//! sample, so there is no DC or Nyquist bin to special-case. Analysis windows the input around
//! the window's peak and rotates that peak to time 0; synthesis does the reverse and overlap-adds.
//! Output is divided by the running sum of analysis × synthesis window products, so a round trip
//! is unity gain whatever the hop.

use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

/// Added to the window products so a division never meets zero.
const ALMOST_ZERO: f32 = 1e-30;

/// Cloning copies the buffers and shares the FFT plans, which run through `&self`: a clone
/// shares nothing mutable.
#[derive(Clone)]
pub(crate) struct Stft {
    channels: usize,
    block: usize,
    fft_size: usize,
    bins: usize,
    interval: usize,
    window: Vec<f32>,
    /// The window's peak, which analysis rotates to time 0.
    offset: usize,
    /// Input history per channel, a ring of `block + extra` samples.
    input: Vec<f32>,
    input_len: usize,
    input_pos: usize,
    /// Output accumulator per channel, a ring of `block` samples, and the window products.
    output: Vec<f32>,
    products: Vec<f32>,
    output_pos: usize,
    spectrum: Vec<Complex32>,
    time: Vec<f32>,
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
    /// Full-size complex scratch for the FFTs, the half-bin twist, and the FFT's own scratch.
    work: Vec<Complex32>,
    twist: Vec<Complex32>,
    scratch: Vec<Complex32>,
}

impl Stft {
    /// An STFT over `channels` channels with `block`-sample windows, `interval` samples apart, and
    /// `extra_history` more samples of input kept for re-analysing the previous position.
    pub(crate) fn new(channels: usize, block: usize, interval: usize, extra_history: usize) -> Self {
        let channels = channels.max(1);
        let block = block.max(4);
        let interval = interval.clamp(1, block);
        let fft_size = fast_size_above(block);
        let bins = fft_size / 2;
        let mut planner = FftPlanner::new();
        let forward = planner.plan_fft_forward(fft_size);
        let inverse = planner.plan_fft_inverse(fft_size);
        let scratch_len = forward.get_inplace_scratch_len().max(inverse.get_inplace_scratch_len());
        let twist = (0..fft_size)
            .map(|n| Complex32::from_polar(1.0, (-std::f64::consts::PI * n as f64 / fft_size as f64) as f32))
            .collect();
        let window = kaiser_window(block, interval);
        let offset = peak(&window);
        let input_len = block + extra_history;
        let mut stft = Self {
            channels,
            block,
            fft_size,
            bins,
            interval,
            window,
            offset,
            input: vec![0.0; input_len * channels],
            input_len,
            input_pos: 0,
            output: vec![0.0; block * channels],
            products: vec![0.0; block],
            output_pos: 0,
            spectrum: vec![Complex32::new(0.0, 0.0); bins * channels],
            time: vec![0.0; fft_size],
            forward,
            inverse,
            work: vec![Complex32::new(0.0, 0.0); fft_size],
            twist,
            scratch: vec![Complex32::new(0.0, 0.0); scratch_len],
        };
        stft.reset(1.0);
        stft
    }

    pub(crate) fn block(&self) -> usize {
        self.block
    }

    pub(crate) fn interval(&self) -> usize {
        self.interval
    }

    pub(crate) fn fft_size(&self) -> usize {
        self.fft_size
    }

    pub(crate) fn bins(&self) -> usize {
        self.bins
    }

    pub(crate) fn analysis_latency(&self) -> usize {
        self.block - self.offset
    }

    pub(crate) fn synthesis_latency(&self) -> usize {
        self.offset
    }

    /// Cycles per sample at (fractional) bin `b`.
    pub(crate) fn bin_to_freq(&self, b: f32) -> f32 {
        (b + 0.5) / self.fft_size as f32
    }

    pub(crate) fn freq_to_bin(&self, f: f32) -> f32 {
        f * self.fft_size as f32 - 0.5
    }

    /// Clears all state; the output starts with window products as if `product_weight` of a
    /// steady stream of blocks had already been synthesised (small, so the first output fades in).
    pub(crate) fn reset(&mut self, product_weight: f32) {
        self.input_pos = self.block;
        self.output_pos = 0;
        self.input.fill(0.0);
        self.output.fill(0.0);
        self.spectrum.fill(Complex32::new(0.0, 0.0));
        self.products.fill(0.0);
        self.add_window_product();
        for i in (0..self.block.saturating_sub(self.interval)).rev() {
            let later = self.products.get(i + self.interval).copied().unwrap_or(0.0);
            if let Some(p) = self.products.get_mut(i) {
                *p += later;
            }
        }
        for p in &mut self.products {
            *p = *p * product_weight + ALMOST_ZERO;
        }
        self.move_output(self.interval);
    }

    /// Writes `samples` into channel `channel`'s input, starting `offset` samples after the
    /// current input position.
    pub(crate) fn write_input(&mut self, channel: usize, offset: usize, samples: impl Iterator<Item = f32>) {
        let base = channel * self.input_len;
        for (i, s) in samples.enumerate() {
            let at = (self.input_pos + offset + i) % self.input_len;
            if let Some(slot) = self.input.get_mut(base + at) {
                *slot = s;
            }
        }
    }

    pub(crate) fn move_input(&mut self, samples: usize) {
        self.input_pos = (self.input_pos + samples) % self.input_len;
    }

    /// Analyses every channel's latest block, or the block `past` samples earlier.
    pub(crate) fn analyse(&mut self, past: usize) {
        for channel in 0..self.channels {
            let start = (self.input_len * 2 + self.input_pos - self.block - past) % self.input_len;
            let base = channel * self.input_len;
            let n = self.fft_size;
            self.time.fill(0.0);
            for i in 0..self.block {
                let s = self.input.get(base + (start + i) % self.input_len).copied().unwrap_or(0.0);
                let w = self.window.get(i).copied().unwrap_or(0.0);
                // The window's peak goes to time 0; what came before it wraps to the end of the
                // buffer, negated because the half-bin shift makes the transform anti-periodic.
                if i < self.offset {
                    if let Some(t) = self.time.get_mut(i + n - self.offset) {
                        *t = -s * w;
                    }
                } else if let Some(t) = self.time.get_mut(i - self.offset) {
                    *t = s * w;
                }
            }
            for ((w, t), twist) in self.work.iter_mut().zip(&self.time).zip(&self.twist) {
                *w = twist * *t;
            }
            self.forward.process_with_scratch(&mut self.work, &mut self.scratch);
            let bins = self.bins;
            if let Some(spectrum) = self.spectrum.get_mut(channel * bins..(channel + 1) * bins) {
                spectrum.copy_from_slice(&self.work[..bins]);
            }
        }
    }

    pub(crate) fn spectrum(&self, channel: usize) -> &[Complex32] {
        self.spectrum.get(channel * self.bins..(channel + 1) * self.bins).unwrap_or_default()
    }

    pub(crate) fn spectrum_mut(&mut self, channel: usize) -> &mut [Complex32] {
        let bins = self.bins;
        self.spectrum.get_mut(channel * bins..(channel + 1) * bins).unwrap_or_default()
    }

    /// Synthesises every channel's spectrum and overlap-adds it at the output position.
    pub(crate) fn synthesise(&mut self) {
        self.add_window_product();
        let n = self.fft_size;
        for channel in 0..self.channels {
            // The full spectrum of a real signal: the half-bins and their mirror images.
            for k in 0..self.bins {
                let x = self.spectrum.get(channel * self.bins + k).copied().unwrap_or_default();
                if let Some(w) = self.work.get_mut(k) {
                    *w = x;
                }
                if let Some(w) = self.work.get_mut(n - 1 - k) {
                    *w = x.conj();
                }
            }
            self.inverse.process_with_scratch(&mut self.work, &mut self.scratch);
            for ((t, w), twist) in self.time.iter_mut().zip(&self.work).zip(&self.twist) {
                *t = (w * twist.conj()).re;
            }
            let base = channel * self.block;
            for i in 0..self.block {
                let w = self.window.get(i).copied().unwrap_or(0.0);
                let value = if i < self.offset {
                    -self.time.get(i + n - self.offset).copied().unwrap_or(0.0) * w
                } else {
                    self.time.get(i - self.offset).copied().unwrap_or(0.0) * w
                };
                if let Some(o) = self.output.get_mut(base + (self.output_pos + i) % self.block) {
                    *o += value;
                }
            }
        }
    }

    /// Reads `out.len()` samples of channel `channel`, `offset` samples after the output position.
    pub(crate) fn read_output(&self, channel: usize, offset: usize, out: &mut [f32]) {
        let base = channel * self.block;
        for (i, o) in out.iter_mut().enumerate() {
            let at = (self.output_pos + offset + i) % self.block;
            let value = self.output.get(base + at).copied().unwrap_or(0.0);
            let product = self.products.get(at).copied().unwrap_or(1.0);
            *o = value / product;
        }
    }

    /// Adds `samples` (already normalised) to channel `channel`'s output.
    pub(crate) fn add_output(&mut self, channel: usize, samples: &[f32]) {
        let base = channel * self.block;
        for (i, s) in samples.iter().take(self.block).enumerate() {
            let at = (self.output_pos + i) % self.block;
            let product = self.products.get(at).copied().unwrap_or(1.0);
            if let Some(o) = self.output.get_mut(base + at) {
                *o += s * product;
            }
        }
    }

    pub(crate) fn move_output(&mut self, samples: usize) {
        for i in 0..samples.min(self.block) {
            let at = (self.output_pos + i) % self.block;
            for channel in 0..self.channels {
                if let Some(o) = self.output.get_mut(channel * self.block + at) {
                    *o = 0.0;
                }
            }
            if let Some(p) = self.products.get_mut(at) {
                *p = ALMOST_ZERO;
            }
        }
        self.output_pos = (self.output_pos + samples) % self.block;
    }

    /// No more synthesis is coming: lets the remaining output taper away with the windows instead
    /// of being scaled up as if the blocks were far apart.
    pub(crate) fn finish_output(&mut self) {
        let mut max_product = 0.0f32;
        for i in 0..self.block {
            let at = (self.output_pos + i) % self.block;
            if let Some(p) = self.products.get_mut(at) {
                max_product = max_product.max(*p);
                *p = max_product;
            }
        }
    }

    fn add_window_product(&mut self) {
        let scaling = self.fft_size as f32;
        for i in 0..self.block {
            let w = self.window.get(i).copied().unwrap_or(0.0);
            if let Some(p) = self.products.get_mut((self.output_pos + i) % self.block) {
                *p += w * w * scaling;
            }
        }
    }
}

/// The smallest even size at or above `n` whose half has only the factors 2, 3 and 5.
fn fast_size_above(n: usize) -> usize {
    let mut half = n.div_ceil(2).max(1);
    loop {
        let mut m = half;
        for f in [2, 3, 5] {
            while m.is_multiple_of(f) {
                m /= f;
            }
        }
        if m == 1 {
            return half * 2;
        }
        half += 1;
    }
}

/// A Kaiser window of `block` samples for blocks `interval` apart, scaled so the squared windows
/// overlap-add to 1 (perfect reconstruction).
fn kaiser_window(block: usize, interval: usize) -> Vec<f32> {
    let bandwidth = block as f64 / interval as f64;
    // The bandwidth heuristic of the original (found by numerical search).
    let bandwidth = bandwidth + 8.0 / ((bandwidth + 3.0) * (bandwidth + 3.0)) + 0.25 * (3.0 - bandwidth).max(0.0);
    let bandwidth = bandwidth.max(2.0);
    let beta = (bandwidth * bandwidth * 0.25 - 1.0).sqrt() * std::f64::consts::PI;
    let inv_b0 = 1.0 / bessel0(beta);
    let offset = usize::from(!block.is_multiple_of(2));
    let mut window: Vec<f64> = (0..block)
        .map(|i| {
            let r = (2 * i + offset) as f64 / block as f64 - 1.0;
            bessel0(beta * (1.0 - r * r).max(0.0).sqrt()) * inv_b0
        })
        .collect();
    for start in 0..interval {
        let sum2: f64 = window.iter().skip(start).step_by(interval).map(|w| w * w).sum();
        let factor = if sum2 > 0.0 { 1.0 / sum2.sqrt() } else { 1.0 };
        for w in window.iter_mut().skip(start).step_by(interval) {
            *w *= factor;
        }
    }
    window.into_iter().map(|w| w as f32).collect()
}

fn bessel0(x: f64) -> f64 {
    let (mut result, mut term, mut m) = (0.0, 1.0, 0.0);
    while term > 1e-4 {
        result += term;
        m += 1.0;
        term *= (x * x) / (4.0 * m * m);
    }
    result
}

/// Index of the first maximum.
fn peak(window: &[f32]) -> usize {
    let mut best = window.len() / 2;
    for (i, w) in window.iter().enumerate() {
        if *w > window.get(best).copied().unwrap_or(0.0) {
            best = i;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_even_and_fast() {
        assert_eq!(fast_size_above(5760), 5760);
        assert_eq!(fast_size_above(5761), 5832);
        assert_eq!(fast_size_above(7), 8);
    }

    #[test]
    fn analysis_then_synthesis_is_unity_gain() {
        let (block, interval) = (480, 120);
        let mut stft = Stft::new(1, block, interval, interval + 1);
        let signal: Vec<f32> = (0..4800).map(|i| ((i as f32) * 0.05).sin() * 0.5).collect();
        let mut out = Vec::new();
        let mut one = [0.0f32];
        for (i, s) in signal.iter().enumerate() {
            stft.write_input(0, 0, std::iter::once(*s));
            stft.move_input(1);
            if i % interval == interval - 1 {
                stft.analyse(0);
                stft.synthesise();
            }
            stft.read_output(0, 0, &mut one);
            out.push(one[0]);
            stft.move_output(1);
        }
        // After the start-up, the output is the input delayed by about the round-trip latency.
        let latency = stft.analysis_latency() + stft.synthesis_latency();
        let error = |lag: usize| (2000..4000).map(|i| (out[i] - signal[i - lag]).abs()).fold(0.0f32, f32::max);
        let best = (latency - 4..latency + 4).min_by(|a, b| error(*a).total_cmp(&error(*b))).unwrap();
        assert!(best.abs_diff(latency) <= 1, "best lag {best}, latency {latency}");
        assert!(error(best) < 1e-3, "round trip differs by {} at lag {best}", error(best));
    }
}

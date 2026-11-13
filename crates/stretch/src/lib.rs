// SPDX-License-Identifier: MIT

//! Time-stretching and pitch-shifting for polyphonic audio: a pure-Rust port of Signalsmith
//! Stretch 1.3.2 (MIT, Geraint Luff / Signalsmith Audio; see `LICENSE-signalsmith.txt`).
//!
//! A phase vocoder whose output phases are predicted both from the previous block (time) and from
//! the neighbouring bins (frequency), blended by energy. That keeps partials coherent across a
//! stretch without the "phasiness" of a plain phase vocoder. Pitch shifting maps spectral peaks to
//! new frequencies and moves the bins between them with the peaks. Stereo and other multichannel
//! audio stay phase-locked: the loudest channel of each bin leads and the others follow it.
//!
//! The stretch ratio is set by how much input each [`Stretch::process`] call is given for its
//! output, so a caller can change the speed from one call to the next.
//!
//! Left out from the original for now: formant shifting and custom frequency maps, and the option
//! to spread one block's work over several calls (the engine renders ahead on a worker instead).

mod stft;

use std::ops::{Deref, DerefMut};

use rustfft::num_complex::Complex32 as Complex;

use stft::Stft;

/// Energy below this counts as silence.
const NOISE_FLOOR: f32 = 1e-15;
/// Above this stretch ratio, phase steps are randomised (a slower clean stretch is not possible).
const MAX_CLEAN_STRETCH: f32 = 2.0;
/// Repeated down-and-up smoothing passes over the spectral energy, for finding peaks.
const SMOOTHING_PASSES: usize = 2;

#[derive(Debug, Clone, Copy, Default)]
struct Band {
    input: Complex,
    prev_input: Complex,
    output: Complex,
    input_energy: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct Prediction {
    energy: f32,
    input: Complex,
}

impl Prediction {
    /// The output for a predicted phase: the predicted energy, with that phase (or the input's,
    /// when the prediction is too weak to have one).
    fn make_output(&self, phase: Complex) -> Complex {
        let (mut phase, mut norm) = (phase, phase.norm_sqr());
        if norm <= NOISE_FLOOR {
            phase = self.input;
            norm = self.input.norm_sqr() + NOISE_FLOOR;
        }
        phase * (self.energy / norm).sqrt()
    }
}

#[derive(Debug, Clone, Copy)]
struct Peak {
    input: f32,
    output: f32,
}

/// The spectral peaks of one block, in a list whose capacity is fixed when the stretch is built:
/// [`Stretch::find_peaks`] never grows it, so it never allocates. A clone keeps that capacity,
/// which a [`Vec`]'s clone does not, or a cloned stretch would find no peaks and not transpose.
#[derive(Debug)]
struct Peaks(Vec<Peak>);

impl Clone for Peaks {
    fn clone(&self) -> Self {
        let mut peaks = Vec::with_capacity(self.0.capacity());
        peaks.extend_from_slice(&self.0);
        Self(peaks)
    }
}

impl Deref for Peaks {
    type Target = Vec<Peak>;
    fn deref(&self) -> &Vec<Peak> {
        &self.0
    }
}

impl DerefMut for Peaks {
    fn deref_mut(&mut self) -> &mut Vec<Peak> {
        &mut self.0
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct MapPoint {
    input_bin: f32,
    freq_grad: f32,
}

/// A streaming time-stretcher and pitch-shifter for interleaved audio.
///
/// Cloning is much cheaper than building one (no FFT planning) and allocates the same buffers. A
/// clone carries on exactly as the original would: [`Stretch::reset`] keeps the random phase
/// generator, so only a clone of a stretch that has never processed anything sounds like a new one.
#[derive(Clone)]
pub struct Stretch {
    channels: usize,
    bands: usize,
    stft: Stft,
    band_data: Vec<Band>,
    predictions: Vec<Prediction>,
    peaks: Peaks,
    energy: Vec<f32>,
    smoothed_energy: Vec<f32>,
    output_map: Vec<MapPoint>,
    freq_multiplier: f32,
    freq_tonality_limit: f32,
    samples_since_last: usize,
    prev_input_offset: i64,
    did_seek: bool,
    seek_time_factor: f32,
    silence_counter: usize,
    silence_first: bool,
    random: u32,
    /// Pre-roll output for [`Stretch::output_seek`], interleaved, and one channel of it.
    preroll: Vec<f32>,
    preroll_channel: Vec<f32>,
}

impl Stretch {
    /// The default preset: 120 ms blocks, 30 ms apart.
    pub fn new(channels: usize, sample_rate: f32) -> Self {
        Self::with_blocks(channels, (sample_rate * 0.12) as usize, (sample_rate * 0.03) as usize)
    }

    /// A cheaper preset: 100 ms blocks, 40 ms apart.
    pub fn cheaper(channels: usize, sample_rate: f32) -> Self {
        Self::with_blocks(channels, (sample_rate * 0.1) as usize, (sample_rate * 0.04) as usize)
    }

    /// Blocks of `block` samples, `interval` samples apart. Allocates; everything after this
    /// except [`Stretch::flush`] and [`Stretch::exact`] does not.
    pub fn with_blocks(channels: usize, block: usize, interval: usize) -> Self {
        let channels = channels.max(1);
        let block = block.max(16);
        let interval = interval.clamp(1, block / 2);
        let mut stft = Stft::new(channels, block, interval, interval + 1);
        stft.reset(0.1);
        let bands = stft.bins();
        let output_latency = stft.synthesis_latency();
        Self {
            channels,
            bands,
            band_data: vec![Band::default(); bands * channels],
            predictions: vec![Prediction::default(); bands * channels],
            peaks: Peaks(Vec::with_capacity(bands)),
            energy: vec![0.0; bands],
            smoothed_energy: vec![0.0; bands],
            output_map: vec![MapPoint::default(); bands],
            freq_multiplier: 1.0,
            freq_tonality_limit: 0.5,
            samples_since_last: usize::MAX,
            prev_input_offset: -1,
            did_seek: false,
            seek_time_factor: 1.0,
            silence_counter: 0,
            silence_first: true,
            random: 0x9E37_79B9,
            preroll: vec![0.0; output_latency * channels],
            preroll_channel: vec![0.0; output_latency.max(block)],
            stft,
        }
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Samples between the input position and the centre of the block analysed for it.
    pub fn input_latency(&self) -> usize {
        self.stft.analysis_latency()
    }

    /// Samples between a block's centre and when it is heard.
    pub fn output_latency(&self) -> usize {
        self.stft.synthesis_latency()
    }

    pub fn block_samples(&self) -> usize {
        self.stft.block()
    }

    pub fn interval_samples(&self) -> usize {
        self.stft.interval()
    }

    /// Clears all state.
    pub fn reset(&mut self) {
        self.stft.reset(0.1);
        self.prev_input_offset = -1;
        self.band_data.fill(Band::default());
        self.silence_counter = 0;
        self.did_seek = false;
        self.samples_since_last = usize::MAX;
    }

    /// Shifts every frequency by `multiplier`. Above `tonality_limit` (a fraction of the sample
    /// rate; 0 for none) frequencies are shifted by a fixed amount instead, which keeps noisy high
    /// content from sounding metallic.
    pub fn set_transpose_factor(&mut self, multiplier: f32, tonality_limit: f32) {
        let multiplier = if multiplier.is_finite() && multiplier > 0.0 { multiplier } else { 1.0 };
        self.freq_multiplier = multiplier;
        self.freq_tonality_limit = if tonality_limit > 0.0 { tonality_limit / multiplier.sqrt() } else { 1.0 };
    }

    pub fn set_transpose_semitones(&mut self, semitones: f32, tonality_limit: f32) {
        self.set_transpose_factor(2f32.powf(semitones / 12.0), tonality_limit);
    }

    /// Input frames [`Stretch::seek`] uses.
    pub fn seek_length(&self) -> usize {
        self.stft.block() + self.stft.interval()
    }

    /// Moves the input position without interrupting the output: `input` (interleaved) is the
    /// audio leading up to the new position, ideally [`Stretch::seek_length`] frames.
    pub fn seek(&mut self, input: &[f32], playback_rate: f32) {
        let channels = self.channels;
        let frames = input.len() / channels;
        let size = self.seek_length();
        let start = frames.saturating_sub(size);
        let pad = size + start - frames;
        for c in 0..channels {
            let samples =
                (0..size).map(|j| if j < pad { 0.0 } else { sample(input, (j - pad + start) * channels + c) });
            self.stft.write_input(c, 0, samples);
        }
        self.stft.move_input(size);
        let energy: f32 = input.iter().map(|s| clean(*s) * clean(*s)).sum();
        if energy >= NOISE_FLOOR {
            self.silence_counter = 0;
            self.silence_first = true;
        }
        self.did_seek = true;
        let interval = self.stft.interval() as f32;
        self.seek_time_factor = if playback_rate * interval > 1.0 { 1.0 / playback_rate } else { interval };
    }

    /// Input frames [`Stretch::output_seek`] wants at `playback_rate`.
    pub fn output_seek_length(&self, playback_rate: f32) -> usize {
        self.input_latency() + (playback_rate * self.output_latency() as f32) as usize
    }

    /// Starts afresh so that the next output frame is the start of `input` (interleaved,
    /// [`Stretch::output_seek_length`] frames): the stretch rate is inferred from its length.
    pub fn output_seek(&mut self, input: &[f32]) {
        self.reset();
        let channels = self.channels;
        let frames = input.len() / channels;
        let latency = self.output_latency();
        let surplus = frames.saturating_sub(self.input_latency());
        let rate = surplus as f32 / latency.max(1) as f32;
        let seek_frames = frames - surplus;
        self.seek(input.get(..seek_frames * channels).unwrap_or_default(), rate);
        let mut preroll = std::mem::take(&mut self.preroll);
        preroll.resize(latency * channels, 0.0);
        self.process(input.get(seek_frames * channels..).unwrap_or_default(), &mut preroll);
        // Turn the pre-roll around and subtract it, so the output so far cancels to the start.
        let mut channel = std::mem::take(&mut self.preroll_channel);
        for c in 0..channels {
            let reversed = channel.get_mut(..latency).unwrap_or_default();
            for (i, r) in reversed.iter_mut().enumerate() {
                *r = -sample(&preroll, (latency - 1 - i) * channels + c);
            }
            self.stft.add_output(c, reversed);
        }
        self.preroll = preroll;
        self.preroll_channel = channel;
    }

    /// Stretches `input` (interleaved) into `output` (interleaved, filled completely); the
    /// ratio of their lengths is the playback rate for this call. Never allocates.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let channels = self.channels;
        let in_frames = input.len() / channels;
        let out_frames = output.len() / channels;
        let interval = self.stft.interval();
        let mut copied = 0;

        let energy: f32 = input.iter().map(|s| clean(*s) * clean(*s)).sum();
        if energy < NOISE_FLOOR {
            if self.silence_counter >= 2 * self.stft.block() {
                if self.silence_first {
                    self.silence_first = false;
                    self.samples_since_last = usize::MAX;
                    for band in &mut self.band_data {
                        *band = Band::default();
                    }
                }
                for (o, frame) in output.chunks_exact_mut(channels).enumerate() {
                    for (c, s) in frame.iter_mut().enumerate() {
                        *s = if in_frames > 0 { sample(input, (o % in_frames) * channels + c) } else { 0.0 };
                    }
                }
                self.copy_input(input, &mut copied, in_frames);
                return;
            }
            self.silence_counter += in_frames;
        } else {
            self.silence_counter = 0;
            self.silence_first = true;
        }

        for o in 0..out_frames {
            if self.samples_since_last >= interval {
                self.samples_since_last = 0;
                // Where in the input this block comes from.
                let offset = (o as f64 * in_frames as f64 / out_frames as f64).round() as i64;
                let input_interval = offset - self.prev_input_offset;
                self.prev_input_offset = offset;
                self.copy_input(input, &mut copied, offset.max(0) as usize);

                let new_spectrum = self.did_seek || input_interval > 0;
                if new_spectrum {
                    // The previous block must be one interval back (give or take a sample).
                    if self.did_seek || (input_interval - interval as i64).abs() > 1 {
                        self.stft.analyse(interval);
                        self.load_spectrum(|band, x| band.prev_input = x);
                    }
                    self.stft.analyse(0);
                    self.load_spectrum(|band, x| band.input = x);
                }
                let time_factor =
                    if self.did_seek { self.seek_time_factor } else { interval as f32 / input_interval.max(1) as f32 };
                self.did_seek = false;
                self.process_spectrum(new_spectrum, time_factor);
                for c in 0..channels {
                    let bands = self.bands;
                    let outputs = self.band_data.get(c * bands..(c + 1) * bands).unwrap_or_default();
                    for (s, band) in self.stft.spectrum_mut(c).iter_mut().zip(outputs) {
                        *s = band.output;
                    }
                }
                self.stft.synthesise();
            }
            self.samples_since_last += 1;
            for c in 0..channels {
                let mut v = [0.0f32];
                self.stft.read_output(c, 0, &mut v);
                if let Some(s) = output.get_mut(o * channels + c) {
                    *s = v[0];
                }
            }
            self.stft.move_output(1);
        }
        self.copy_input(input, &mut copied, in_frames);
        self.prev_input_offset -= in_frames as i64;
    }

    /// Reads out the remaining output (`output`, interleaved) with no more input. Allocates if
    /// `output` is longer than one interval.
    pub fn flush(&mut self, output: &mut [f32], playback_rate: f32) {
        let channels = self.channels;
        let out_frames = output.len() / channels;
        let interval = self.stft.interval();
        let block_frames = out_frames.saturating_sub(interval);
        if block_frames > 0 {
            let zeros = vec![0.0; (block_frames as f32 * playback_rate) as usize * channels];
            self.process(&zeros, output.get_mut(..block_frames * channels).unwrap_or_default());
        }
        let tail = out_frames - block_frames;
        self.stft.finish_output();
        let mut channel = std::mem::take(&mut self.preroll_channel);
        channel.resize(channel.len().max(tail), 0.0);
        for c in 0..channels {
            let buffer = channel.get_mut(..tail).unwrap_or_default();
            self.stft.read_output(c, 0, buffer);
            for (i, v) in buffer.iter().enumerate() {
                if let Some(s) = output.get_mut((block_frames + i) * channels + c) {
                    *s = *v;
                }
            }
            // The window's far side, folded back so the end does not click.
            self.stft.read_output(c, tail, buffer);
            for (i, v) in buffer.iter().enumerate() {
                if let Some(s) = output.get_mut((block_frames + tail - 1 - i) * channels + c) {
                    *s -= *v;
                }
            }
        }
        self.preroll_channel = channel;
        self.stft.reset(0.1);
        for band in &mut self.band_data {
            band.prev_input = Complex::default();
            band.output = Complex::default();
        }
    }

    /// Stretches all of `input` into all of `output` (both interleaved), aligned at both ends.
    /// Returns `false` (and silence) if the input is too short for that. Allocates.
    pub fn exact(&mut self, input: &[f32], output: &mut [f32]) -> bool {
        let channels = self.channels;
        let (in_frames, out_frames) = (input.len() / channels, output.len() / channels);
        if out_frames == 0 {
            return false;
        }
        let rate = in_frames as f32 / out_frames as f32;
        let seek = self.output_seek_length(rate);
        if in_frames < seek || rate <= 0.0 {
            output.fill(0.0);
            return false;
        }
        self.output_seek(input.get(..seek * channels).unwrap_or_default());
        let index = (out_frames as f32 - seek as f32 / rate).max(0.0) as usize;
        let index = index.min(out_frames);
        let (body, tail) = output.split_at_mut(index * channels);
        self.process(input.get(seek * channels..).unwrap_or_default(), body);
        self.flush(tail, rate);
        true
    }

    /// Writes the input up to frame `to` (of this call's input) into the STFT's history.
    fn copy_input(&mut self, input: &[f32], copied: &mut usize, to: usize) {
        let channels = self.channels;
        let length = (self.stft.block() + self.stft.interval()).min(to.saturating_sub(*copied));
        let offset = to - length;
        for c in 0..channels {
            self.stft.write_input(c, 0, (0..length).map(|i| sample(input, (offset + i) * channels + c)));
        }
        self.stft.move_input(length);
        *copied = to;
    }

    fn load_spectrum(&mut self, set: impl Fn(&mut Band, Complex)) {
        let bands = self.bands;
        for c in 0..self.channels {
            let spectrum = self.stft.spectrum(c);
            for (band, x) in self.band_data.iter_mut().skip(c * bands).take(bands).zip(spectrum) {
                set(band, *x);
            }
        }
    }

    fn band(&self, channel: usize, index: i64) -> Option<&Band> {
        if index < 0 || index >= self.bands as i64 {
            return None;
        }
        self.band_data.get(channel * self.bands + index as usize)
    }

    /// A band field at a fractional bin, linearly interpolated; zero outside the spectrum.
    fn fractional<T>(&self, channel: usize, bin: f32, field: impl Fn(&Band) -> T) -> T
    where
        T: Default + std::ops::Add<Output = T> + std::ops::Sub<Output = T> + std::ops::Mul<f32, Output = T> + Copy,
    {
        let low = bin.floor();
        let fraction = bin - low;
        let low = low as i64;
        let a = self.band(channel, low).map(&field).unwrap_or_default();
        let b = self.band(channel, low + 1).map(&field).unwrap_or_default();
        a + (b - a) * fraction
    }

    fn next_random(&mut self) -> f32 {
        // xorshift32: the original's choice of generator does not matter, only that it is cheap.
        let mut x = self.random;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.random = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    fn process_spectrum(&mut self, new_spectrum: bool, time_factor: f32) {
        let (channels, bands) = (self.channels, self.bands);
        let interval = self.stft.interval() as f32;
        let smoothing_bins = self.stft.fft_size() as f32 / interval;
        let long_step = smoothing_bins.round() as usize;
        let time_factor = time_factor.max(1.0 / MAX_CLEAN_STRETCH);
        let random_time_factor = time_factor > MAX_CLEAN_STRETCH;
        let random_low = MAX_CLEAN_STRETCH * 2.0 - time_factor;

        if new_spectrum {
            // Rotate the previous block's phases on by one interval at each bin's frequency.
            let tau = std::f32::consts::TAU;
            let step =
                Complex::from_polar(1.0, (self.stft.bin_to_freq(1.0) - self.stft.bin_to_freq(0.0)) * interval * tau);
            for c in 0..channels {
                let mut rot = Complex::from_polar(1.0, self.stft.bin_to_freq(0.0) * interval * tau);
                for band in self.band_data.iter_mut().skip(c * bands).take(bands) {
                    band.output *= rot;
                    band.prev_input *= rot;
                    rot *= step;
                }
            }
        }

        if self.freq_multiplier != 1.0 {
            self.smooth_energy(smoothing_bins);
            self.find_peaks();
            self.update_output_map();
        } else {
            for band in &mut self.band_data {
                band.input_energy = band.input.norm_sqr();
            }
            for (b, point) in self.output_map.iter_mut().enumerate() {
                *point = MapPoint { input_bin: b as f32, freq_grad: 1.0 };
            }
        }

        // Preliminary output from the time-only (phase vocoder) prediction.
        for c in 0..channels {
            for b in 0..bands {
                let map = self.output_map.get(b).copied().unwrap_or_default();
                let energy = self.fractional(c, map.input_bin, |band| band.input_energy) * map.freq_grad.max(0.0);
                let input = self.fractional(c, map.input_bin, |band| band.input);
                let prev_input = self.fractional(c, map.input_bin, |band| band.prev_input);
                let index = c * bands + b;
                let Some(prediction) = self.predictions.get_mut(index) else { continue };
                let prev_energy = prediction.energy;
                prediction.energy = energy;
                prediction.input = input;
                let twist = input * prev_input.conj();
                if let Some(band) = self.band_data.get_mut(index) {
                    let phase = band.output * twist;
                    band.output = phase / (prev_energy.max(energy) + NOISE_FLOOR);
                }
            }
        }

        // Re-predict each bin from its neighbours' phase differences, led by its loudest channel.
        for b in 0..bands {
            let mut lead = 0;
            let mut lead_energy = self.predictions.get(b).map_or(0.0, |p| p.energy);
            for c in 1..channels {
                let e = self.predictions.get(c * bands + b).map_or(0.0, |p| p.energy);
                if e > lead_energy {
                    lead = c;
                    lead_energy = e;
                }
            }
            let base = lead * bands;
            let prediction = self.predictions.get(base + b).copied().unwrap_or_default();
            let map = self.output_map.get(b).copied().unwrap_or_default();
            let mut phase = Complex::default();

            if b > 0 {
                let factor =
                    if random_time_factor { self.random_between(random_low, time_factor) } else { time_factor };
                let down = self.fractional(lead, map.input_bin - factor, |band| band.input);
                let twist = prediction.input * down.conj();
                let below = self.band_data.get(base + b - 1).map_or(Complex::default(), |band| band.output);
                phase += below * twist;
                if b >= long_step {
                    let long_down = self.fractional(lead, map.input_bin - long_step as f32 * factor, |band| band.input);
                    let twist = prediction.input * long_down.conj();
                    let far = self.band_data.get(base + b - long_step).map_or(Complex::default(), |band| band.output);
                    phase += far * twist;
                }
            }
            if b + 1 < bands {
                let factor =
                    if random_time_factor { self.random_between(random_low, time_factor) } else { time_factor };
                let up_prediction = self.predictions.get(base + b + 1).copied().unwrap_or_default();
                let up_map = self.output_map.get(b + 1).copied().unwrap_or_default();
                let down = self.fractional(lead, up_map.input_bin - factor, |band| band.input);
                let twist = up_prediction.input * down.conj();
                let above = self.band_data.get(base + b + 1).map_or(Complex::default(), |band| band.output);
                phase += above * twist.conj();
                if b + long_step < bands {
                    let far_prediction = self.predictions.get(base + b + long_step).copied().unwrap_or_default();
                    let far_map = self.output_map.get(b + long_step).copied().unwrap_or_default();
                    let long_down =
                        self.fractional(lead, far_map.input_bin - long_step as f32 * factor, |band| band.input);
                    let twist = far_prediction.input * long_down.conj();
                    let far = self.band_data.get(base + b + long_step).map_or(Complex::default(), |band| band.output);
                    phase += far * twist.conj();
                }
            }

            let output = prediction.make_output(phase);
            if let Some(band) = self.band_data.get_mut(base + b) {
                band.output = output;
            }
            // Every other channel keeps its phase relation to the lead.
            for c in (0..channels).filter(|c| *c != lead) {
                let index = c * bands + b;
                let channel_prediction = self.predictions.get(index).copied().unwrap_or_default();
                let twist = channel_prediction.input * prediction.input.conj();
                if let Some(band) = self.band_data.get_mut(index) {
                    band.output = channel_prediction.make_output(output * twist);
                }
            }
        }

        if new_spectrum {
            for band in &mut self.band_data {
                band.prev_input = band.input;
            }
        }
    }

    fn random_between(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.next_random()
    }

    /// Total energy per bin over all channels, and a smoothed copy of it.
    fn smooth_energy(&mut self, smoothing_bins: f32) {
        let bands = self.bands;
        self.energy.fill(0.0);
        for c in 0..self.channels {
            for (band, total) in self.band_data.iter_mut().skip(c * bands).take(bands).zip(&mut self.energy) {
                let e = band.input.norm_sqr();
                band.input_energy = e;
                *total += e;
            }
        }
        self.smoothed_energy.copy_from_slice(&self.energy);
        let slew = 1.0 / (1.0 + smoothing_bins * 0.5);
        let mut e = 0.0;
        for _ in 0..SMOOTHING_PASSES {
            for s in self.smoothed_energy.iter_mut().rev() {
                e += (*s - e) * slew;
                *s = e;
            }
            for s in &mut self.smoothed_energy {
                e += (*s - e) * slew;
                *s = e;
            }
        }
    }

    fn map_freq(&self, freq: f32) -> f32 {
        if freq > self.freq_tonality_limit {
            freq + (self.freq_multiplier - 1.0) * self.freq_tonality_limit
        } else {
            freq * self.freq_multiplier
        }
    }

    /// Spectral peaks (runs of bins above the smoothed energy), with where each one maps to.
    fn find_peaks(&mut self) {
        self.peaks.clear();
        let bands = self.bands;
        let mut start = 0;
        while start < bands {
            if self.energy.get(start) > self.smoothed_energy.get(start) {
                let mut end = start;
                let (mut band_sum, mut energy_sum) = (0.0f32, 0.0f32);
                while end < bands && self.energy.get(end) > self.smoothed_energy.get(end) {
                    let e = self.energy.get(end).copied().unwrap_or(0.0);
                    band_sum += end as f32 * e;
                    energy_sum += e;
                    end += 1;
                }
                let average = band_sum / energy_sum;
                let output = self.stft.freq_to_bin(self.map_freq(self.stft.bin_to_freq(average)));
                if self.peaks.len() < self.peaks.capacity() {
                    self.peaks.push(Peak { input: average, output });
                }
                start = end;
            }
            start += 1;
        }
    }

    /// For each output bin, the input bin it comes from and how stretched that region is: peaks
    /// move to their mapped frequency and the bins between them follow smoothly.
    fn update_output_map(&mut self) {
        let bands = self.bands;
        let (Some(first), Some(last)) = (self.peaks.first().copied(), self.peaks.last().copied()) else {
            for (b, point) in self.output_map.iter_mut().enumerate() {
                *point = MapPoint { input_bin: b as f32, freq_grad: 1.0 };
            }
            return;
        };
        let bottom = first.input - first.output;
        for b in 0..bands.min(first.output.ceil().max(0.0) as usize) {
            if let Some(point) = self.output_map.get_mut(b) {
                *point = MapPoint { input_bin: b as f32 + bottom, freq_grad: 1.0 };
            }
        }
        for pair in self.peaks.windows(2) {
            let [prev, next] = pair else { continue };
            let range_scale = 1.0 / (next.output - prev.output);
            let out_offset = prev.input - prev.output;
            let out_scale = next.input - next.output - prev.input + prev.output;
            let grad_scale = out_scale * range_scale;
            let start = prev.output.ceil().max(0.0) as usize;
            let end = (next.output.ceil().max(0.0) as usize).min(bands);
            for b in start..end {
                let r = (b as f32 - prev.output) * range_scale;
                let h = r * r * (3.0 - 2.0 * r);
                let grad = 1.0 + 6.0 * r * (1.0 - r) * grad_scale;
                if let Some(point) = self.output_map.get_mut(b) {
                    *point = MapPoint { input_bin: b as f32 + out_offset + h * out_scale, freq_grad: grad };
                }
            }
        }
        let top = last.input - last.output;
        for b in (last.output.max(0.0) as usize)..bands {
            if let Some(point) = self.output_map.get_mut(b) {
                *point = MapPoint { input_bin: b as f32 + top, freq_grad: 1.0 };
            }
        }
    }
}

/// A sample of `buffer`, cleaned; silence outside it.
fn sample(buffer: &[f32], index: usize) -> f32 {
    clean(buffer.get(index).copied().unwrap_or(0.0))
}

/// Not-a-number and infinite samples are silence; absurdly loud ones are limited, so energy sums
/// stay finite.
fn clean(s: f32) -> f32 {
    if s.is_finite() { s.clamp(-1e4, 1e4) } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn sine(hz: f32, frames: usize, channels: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let s = (std::f32::consts::TAU * hz * i as f32 / SR).sin() * 0.5;
                std::iter::repeat_n(s, channels)
            })
            .collect()
    }

    /// Zero crossings per second of channel 0 over the middle half.
    fn frequency(out: &[f32], channels: usize) -> f32 {
        let left: Vec<f32> = out.iter().step_by(channels).copied().collect();
        let middle = &left[left.len() / 4..left.len() * 3 / 4];
        let ups = middle.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        ups as f32 / (middle.len() as f32 / SR)
    }

    fn rms(out: &[f32]) -> f32 {
        let middle = &out[out.len() / 4..out.len() * 3 / 4];
        (middle.iter().map(|s| s * s).sum::<f32>() / middle.len() as f32).sqrt()
    }

    #[test]
    fn stretching_keeps_pitch_and_level() {
        for ratio in [0.8f32, 1.25, 0.6, 1.7] {
            let input = sine(440.0, 96_000, 2);
            let out_frames = (96_000.0 * ratio) as usize;
            let mut output = vec![0.0; out_frames * 2];
            let mut stretch = Stretch::new(2, SR);
            assert!(stretch.exact(&input, &mut output));
            let hz = frequency(&output, 2);
            assert!((hz - 440.0).abs() < 5.0, "x{ratio}: {hz} Hz");
            let level = rms(&output) / rms(&input);
            assert!((level - 1.0).abs() < 0.1, "x{ratio}: level {level}");
        }
    }

    #[test]
    fn transposing_moves_pitch_but_not_time() {
        for (semitones, expected) in [(12.0f32, 880.0f32), (-5.0, 440.0 * 2f32.powf(-5.0 / 12.0))] {
            let input = sine(440.0, 96_000, 1);
            let mut output = vec![0.0; 96_000];
            let mut stretch = Stretch::new(1, SR);
            stretch.set_transpose_semitones(semitones, 0.0);
            assert!(stretch.exact(&input, &mut output));
            let hz = frequency(&output, 1);
            assert!((hz - expected).abs() < expected * 0.01, "{semitones} st: {hz} Hz, expected {expected}");
        }
    }

    #[test]
    fn streaming_in_chunks_at_a_changing_rate() {
        let input = sine(330.0, 200_000, 2);
        let mut stretch = Stretch::new(2, SR);
        let mut output = Vec::new();
        let mut at = 0;
        let mut chunk = vec![0.0; 512 * 2];
        // 512 output frames per call; the rate moves from 0.7 to 1.3.
        while at + 2_000 < 200_000 {
            let rate = 0.7 + 0.6 * at as f32 / 200_000.0;
            let frames = (512.0 * rate) as usize;
            stretch.process(&input[at * 2..(at + frames) * 2], &mut chunk);
            output.extend_from_slice(&chunk);
            at += frames;
        }
        let hz = frequency(&output, 2);
        assert!((hz - 330.0).abs() < 5.0, "{hz} Hz");
        assert!(output.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn silence_and_hostile_input_stay_finite() {
        let mut stretch = Stretch::new(2, SR);
        let mut output = vec![1.0; 4_096];
        stretch.process(&vec![0.0; 3_000], &mut output);
        assert!(output.iter().all(|s| *s == 0.0));
        let mut hostile = sine(440.0, 3_000, 2);
        hostile[100] = f32::NAN;
        hostile[200] = f32::INFINITY;
        hostile[300] = 1e30;
        stretch.process(&hostile, &mut output);
        stretch.process(&[], &mut output);
        assert!(output.iter().all(|s| s.is_finite()));
        // Too short for an exact stretch: silence, not a panic.
        let mut short = vec![1.0; 100];
        assert!(!stretch.exact(&[0.5; 20], &mut short));
        assert!(short.iter().all(|s| *s == 0.0));
    }

    /// A transpose (which looks for spectral peaks) at a stretch above 2× (which draws random
    /// phases), as the bits of the output.
    fn transposed_and_stretched(mut stretch: Stretch) -> Vec<u32> {
        let input = sine(440.0, 24_000, 2);
        stretch.set_transpose_semitones(7.0, 0.0);
        let mut output = vec![0.0; 60_000 * 2];
        assert!(stretch.exact(&input, &mut output));
        output.iter().map(|s| s.to_bits()).collect()
    }

    #[test]
    fn a_clone_of_a_new_stretch_sounds_bit_for_bit_like_a_new_one() {
        let fresh = transposed_and_stretched(Stretch::new(2, SR));
        let template = Stretch::new(2, SR);
        assert_eq!(transposed_and_stretched(template.clone()), fresh);
        assert_eq!(transposed_and_stretched(template), fresh);
        // A stretch that has played does not start like a new one, even after a reset: the
        // random phases go on. So the comparison above covers them.
        let mut played = Stretch::new(2, SR);
        let mut output = vec![0.0; 60_000 * 2];
        played.set_transpose_semitones(7.0, 0.0);
        assert!(played.exact(&sine(440.0, 24_000, 2), &mut output));
        played.reset();
        assert_ne!(transposed_and_stretched(played), fresh);
    }
}

#[cfg(test)]
mod timing {
    use super::*;

    const SR: f32 = 48_000.0;

    /// Mono clicks every 0.5 s: 2 ms bursts.
    fn clicks(seconds: f32) -> Vec<f32> {
        let period = (0.5 * SR) as usize;
        (0..(seconds * SR) as usize).map(|i| if i % period < 96 { 0.9 } else { 0.0 }).collect()
    }

    fn onsets(out: &[f32]) -> Vec<f32> {
        let quiet_needed = (0.05 * SR) as usize;
        let mut quiet = quiet_needed;
        let mut found = Vec::new();
        for (i, s) in out.iter().enumerate() {
            if s.abs() > 0.2 {
                if quiet >= quiet_needed {
                    found.push(i as f32 / SR);
                }
                quiet = 0;
            } else {
                quiet += 1;
            }
        }
        found
    }

    #[test]
    fn transients_stay_on_the_grid() {
        // Clicks every 0.5 s land where the stretch puts them, without drifting over 30 s.
        for (rate, worst) in [(0.95f32, 3.0f32), (1.05, 3.0), (0.7, 4.0), (1.5, 4.0), (0.5, 5.0), (2.0, 3.0)] {
            let ratio = 1.0 / rate;
            let input = clicks(30.0);
            let mut output = vec![0.0; (input.len() as f32 * ratio) as usize];
            let mut stretch = Stretch::new(1, SR);
            assert!(stretch.exact(&input, &mut output));
            let errors: Vec<f32> =
                onsets(&output).iter().map(|t| (t - (t / ratio / 0.5).round() * 0.5 * ratio) * 1000.0).collect();
            assert!(errors.len() >= 58, "x{rate}: {} clicks", errors.len());
            let off = errors.iter().fold(0.0f32, |w, e| w.max(e.abs()));
            assert!(off < worst, "x{rate}: {errors:?}");
        }
    }
}

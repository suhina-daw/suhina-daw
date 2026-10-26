// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The mix space analysis: where one stereo signal sits in the mix, band by band.
//!
//! Every band of a logarithmic grid from 20 Hz to 20 kHz gets four readings: its level in dBFS
//! (the louder channel, so a full-scale sine reads 0 dB), a falling peak of that level, its
//! position in the stereo field (-1 fully left, 0 centre, 1 fully right, the exact inverse of
//! [`crate::pan::gains`]) and the correlation of the two channels in it (1 the same, 0
//! unrelated, -1 opposite). The Mix Space device draws these as one line per track in a box.
//!
//! [`MixSpaceAnalyzer::new`] allocates everything; [`MixSpaceAnalyzer::push`] never allocates,
//! locks or panics, so it may run on any thread. It is meant for an analysis thread fed from the
//! audio thread through a queue, not for the audio callback itself: one spectrum costs a few
//! hundred microseconds.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

/// The lowest level a reading can have, for silence.
pub const FLOOR_DB: f32 = -120.0;

/// The lowest band edge.
pub const LOWEST_HZ: f32 = 20.0;

/// The highest band edge.
pub const HIGHEST_HZ: f32 = 20_000.0;

/// Bands per octave when none is given: 1/24 octave, fine enough to show single notes.
pub const DEFAULT_BANDS_PER_OCTAVE: u32 = 24;

/// How fast a peak falls, in dB per second.
const PEAK_FALL_DB_PER_SECOND: f32 = 12.0;

/// Below this a band counts as silent: its stereo position and correlation keep their last value.
const AUDIBLE_DB: f32 = -96.0;

/// One band's readings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandReading {
    /// The band's centre, in Hz.
    pub frequency: f32,
    /// The level of the louder channel, smoothed, in dBFS ([`FLOOR_DB`] for silence).
    pub level_db: f32,
    /// The highest recent level, falling by 12 dB a second.
    pub peak_db: f32,
    /// Where the band sits: -1 fully left, 0 centre, 1 fully right.
    pub pan: f32,
    /// How alike the two channels are in the band: 1 the same, 0 unrelated, -1 opposite.
    pub correlation: f32,
}

/// The FFT bins one band sums.
#[derive(Clone, Copy, Debug)]
struct Bins {
    first: usize,
    end: usize,
}

/// The analysis of one stereo signal. See the module documentation.
pub struct MixSpaceAnalyzer {
    fft: Arc<dyn RealToComplex<f32>>,
    sample_rate: f32,
    size: usize,
    hop: usize,
    window: Vec<f32>,
    /// `2 / (size · Σw²)`: turns a band's summed bin power into a mean square.
    scale: f32,
    left: Vec<f32>,
    right: Vec<f32>,
    write: usize,
    since_frame: usize,
    input: Vec<f32>,
    spectrum_left: Vec<Complex<f32>>,
    spectrum_right: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    bins: Vec<Bins>,
    readings: Vec<BandReading>,
    attack: f32,
    release: f32,
    position: f32,
    peak_fall: f32,
    frames: u64,
}

impl MixSpaceAnalyzer {
    /// An analyzer for `sample_rate` with `bands_per_octave` bands (1 to 48) from 20 Hz to
    /// 20 kHz, or up to just below half the sample rate when that is lower. A sample rate that
    /// is not a positive finite number counts as 48 kHz.
    pub fn new(sample_rate: f64, bands_per_octave: u32) -> Self {
        let sample_rate = crate::usable_sample_rate(sample_rate) as f32;
        // About 6 Hz per bin, so the bands under 100 Hz still tell notes apart.
        let size = ((sample_rate / 6.0) as usize).next_power_of_two().clamp(1024, 32_768);
        let hop = size / 8;
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(size);
        let window: Vec<f32> = (0..size)
            .map(|n| {
                let phase = std::f32::consts::TAU * n as f32 / size as f32;
                0.5 - 0.5 * phase.cos()
            })
            .collect();
        let energy: f32 = window.iter().map(|w| w * w).sum();
        let bins = band_bins(sample_rate, size, bands_per_octave.clamp(1, 48));
        let readings = bins
            .iter()
            .map(|(_, frequency)| BandReading {
                frequency: *frequency,
                level_db: FLOOR_DB,
                peak_db: FLOOR_DB,
                pan: 0.0,
                correlation: 1.0,
            })
            .collect();
        let frame_seconds = hop as f32 / sample_rate;
        let mut analyzer = Self {
            input: fft.make_input_vec(),
            spectrum_left: fft.make_output_vec(),
            spectrum_right: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            sample_rate,
            size,
            hop,
            window,
            scale: 2.0 / (size as f32 * energy.max(f32::MIN_POSITIVE)),
            left: vec![0.0; size],
            right: vec![0.0; size],
            write: 0,
            since_frame: 0,
            bins: bins.into_iter().map(|(bins, _)| bins).collect(),
            readings,
            attack: 0.0,
            release: 0.0,
            position: coefficient(frame_seconds, 0.08),
            peak_fall: PEAK_FALL_DB_PER_SECOND * frame_seconds,
            frames: 0,
        };
        analyzer.set_times(0.01, 0.3);
        analyzer
    }

    /// Sets how fast a level rises (`attack`) and falls (`release`), in seconds. Short attacks
    /// keep drum hits visible.
    pub fn set_times(&mut self, attack: f32, release: f32) {
        let frame_seconds = self.frame_seconds();
        self.attack = coefficient(frame_seconds, attack);
        self.release = coefficient(frame_seconds, release);
    }

    /// The seconds between two spectra.
    pub fn frame_seconds(&self) -> f32 {
        self.hop as f32 / self.sample_rate
    }

    /// The readings, lowest band first.
    pub fn readings(&self) -> &[BandReading] {
        &self.readings
    }

    /// How many spectra have been taken since the start or the last [`reset`](Self::reset).
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Clears the history and every reading back to silence.
    pub fn reset(&mut self) {
        self.left.fill(0.0);
        self.right.fill(0.0);
        self.write = 0;
        self.since_frame = 0;
        self.frames = 0;
        for reading in &mut self.readings {
            reading.level_db = FLOOR_DB;
            reading.peak_db = FLOOR_DB;
            reading.pan = 0.0;
            reading.correlation = 1.0;
        }
    }

    /// Adds samples of the two channels (the shorter slice sets how many) and takes a spectrum
    /// every eighth of the FFT size. Returns whether the readings changed.
    pub fn push(&mut self, left: &[f32], right: &[f32]) -> bool {
        let mut changed = false;
        for (&l, &r) in left.iter().zip(right) {
            let Some(slot) = self.left.get_mut(self.write) else {
                self.write = 0;
                continue;
            };
            *slot = sanitize(l);
            if let Some(slot) = self.right.get_mut(self.write) {
                *slot = sanitize(r);
            }
            self.write = (self.write + 1) % self.size;
            self.since_frame += 1;
            if self.since_frame >= self.hop {
                self.since_frame = 0;
                self.frame();
                changed = true;
            }
        }
        changed
    }

    fn frame(&mut self) {
        if !self.transform(true) || !self.transform(false) {
            return;
        }
        self.frames = self.frames.saturating_add(1);
        for (bins, reading) in self.bins.iter().zip(&mut self.readings) {
            let (mut power_left, mut power_right, mut cross) = (0.0f32, 0.0f32, 0.0f32);
            let band = bins.first..bins.end;
            let (Some(left), Some(right)) = (self.spectrum_left.get(band.clone()), self.spectrum_right.get(band))
            else {
                continue;
            };
            for (l, r) in left.iter().zip(right) {
                power_left += l.norm_sqr();
                power_right += r.norm_sqr();
                cross += l.re * r.re + l.im * r.im;
            }
            let louder = power_left.max(power_right) * self.scale;
            let level = if louder > 0.0 { (10.0 * (2.0 * louder).log10()).max(FLOOR_DB) } else { FLOOR_DB };
            let step = if level > reading.level_db { self.attack } else { self.release };
            reading.level_db += (level - reading.level_db) * step;
            reading.peak_db = (reading.peak_db - self.peak_fall).max(level).max(FLOOR_DB);
            if level > AUDIBLE_DB {
                let angle = power_right.sqrt().atan2(power_left.sqrt());
                let pan = angle / std::f32::consts::FRAC_PI_4 - 1.0;
                let both = (power_left * power_right).sqrt();
                let correlation = if both > 0.0 { (cross / both).clamp(-1.0, 1.0) } else { 0.0 };
                reading.pan += (pan - reading.pan) * self.position;
                reading.correlation += (correlation - reading.correlation) * self.position;
            }
        }
    }

    /// Windows the last `size` samples of one channel into the FFT's input and transforms it.
    fn transform(&mut self, left: bool) -> bool {
        let history = if left { &self.left } else { &self.right };
        let (newer, older) = history.split_at(self.write.min(history.len()));
        for ((slot, sample), weight) in self.input.iter_mut().zip(older.iter().chain(newer)).zip(&self.window) {
            *slot = sample * weight;
        }
        let output = if left { &mut self.spectrum_left } else { &mut self.spectrum_right };
        self.fft.process_with_scratch(&mut self.input, output, &mut self.scratch).is_ok()
    }
}

/// The one-pole step that covers `1 - 1/e` of the way in `seconds`.
fn coefficient(frame_seconds: f32, seconds: f32) -> f32 {
    if seconds.is_finite() && seconds > 0.0 { 1.0 - (-frame_seconds / seconds).exp() } else { 1.0 }
}

fn sanitize(sample: f32) -> f32 {
    if sample.is_finite() { sample.clamp(-16.0, 16.0) } else { 0.0 }
}

/// Each band's bins and centre frequency. A band narrower than a bin takes the bin nearest its
/// centre, so the lowest bands still read.
fn band_bins(sample_rate: f32, size: usize, bands_per_octave: u32) -> Vec<(Bins, f32)> {
    let bin_hz = sample_rate / size as f32;
    let top = HIGHEST_HZ.min(sample_rate * 0.49);
    let step = 2f32.powf(1.0 / bands_per_octave as f32);
    let last_bin = size / 2;
    let mut bands = Vec::new();
    let mut low = LOWEST_HZ;
    while low < top {
        let high = (low * step).min(top);
        let centre = (low * high).sqrt();
        let first = (low / bin_hz).ceil() as usize;
        let end = ((high / bin_hz).ceil() as usize).min(last_bin + 1);
        let bins = if first < end {
            Bins { first, end }
        } else {
            let nearest = ((centre / bin_hz).round() as usize).min(last_bin);
            Bins { first: nearest, end: nearest + 1 }
        };
        bands.push((bins, centre));
        low = high;
    }
    bands
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;

    fn sine(frequency: f32, amplitude: f32, frames: usize, rate: f64) -> Vec<f32> {
        (0..frames)
            .map(|n| amplitude * (std::f64::consts::TAU * f64::from(frequency) * n as f64 / rate).sin() as f32)
            .collect()
    }

    fn noise(seed: u32, frames: usize) -> Vec<f32> {
        let mut state = seed;
        (0..frames)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as f32 / u32::MAX as f32 * 0.5 - 0.25
            })
            .collect()
    }

    fn settled(left: &[f32], right: &[f32], rate: f64) -> MixSpaceAnalyzer {
        let mut analyzer = MixSpaceAnalyzer::new(rate, DEFAULT_BANDS_PER_OCTAVE);
        analyzer.push(left, right);
        analyzer
    }

    fn band_at(analyzer: &MixSpaceAnalyzer, frequency: f32) -> BandReading {
        *analyzer
            .readings()
            .iter()
            .min_by(|a, b| (a.frequency / frequency).ln().abs().total_cmp(&(b.frequency / frequency).ln().abs()))
            .expect("bands")
    }

    fn loudest(analyzer: &MixSpaceAnalyzer) -> BandReading {
        *analyzer.readings().iter().max_by(|a, b| a.level_db.total_cmp(&b.level_db)).expect("bands")
    }

    const SECONDS: usize = 48_000 * 2;

    #[test]
    fn a_sine_on_the_left_reads_left_at_its_level() {
        let tone = sine(1_000.0, 0.5, SECONDS, RATE);
        let silent = vec![0.0; SECONDS];
        let analyzer = settled(&tone, &silent, RATE);
        let band = loudest(&analyzer);
        assert!((band.frequency / 1_000.0).log2().abs() < 1.0 / 24.0, "{band:?}");
        assert!((band.pan + 1.0).abs() < 0.02, "{band:?}");
        assert!((band.level_db + 6.02).abs() < 0.5, "{band:?}");
        let right = settled(&silent, &tone, RATE);
        assert!((loudest(&right).pan - 1.0).abs() < 0.02);
    }

    #[test]
    fn the_same_signal_in_both_channels_is_centred_and_correlated() {
        let tone = sine(440.0, 0.25, SECONDS, RATE);
        let band = loudest(&settled(&tone, &tone, RATE));
        assert!(band.pan.abs() < 0.02, "{band:?}");
        assert!((band.correlation - 1.0).abs() < 0.02, "{band:?}");
        let inverted: Vec<f32> = tone.iter().map(|s| -s).collect();
        let band = loudest(&settled(&tone, &inverted, RATE));
        assert!((band.correlation + 1.0).abs() < 0.02, "{band:?}");
    }

    #[test]
    fn unrelated_noise_is_centred_and_uncorrelated() {
        let analyzer = settled(&noise(1, SECONDS), &noise(99, SECONDS), RATE);
        let band = band_at(&analyzer, 2_000.0);
        assert!(band.pan.abs() < 0.2, "{band:?}");
        assert!(band.correlation.abs() < 0.3, "{band:?}");
    }

    #[test]
    fn the_pan_law_reads_back_as_its_position() {
        let tone = sine(2_000.0, 0.5, SECONDS, RATE);
        for pan in [-0.6f64, 0.3, 0.9] {
            let (gl, gr) = crate::pan::gains(pan);
            let left: Vec<f32> = tone.iter().map(|s| s * gl as f32).collect();
            let right: Vec<f32> = tone.iter().map(|s| s * gr as f32).collect();
            let band = loudest(&settled(&left, &right, RATE));
            assert!((band.pan - pan as f32).abs() < 0.02, "{pan}: {band:?}");
        }
    }

    #[test]
    fn tones_land_in_their_bands_across_the_range() {
        for frequency in [20.0f32, 100.0, 10_000.0, 19_000.0] {
            let tone = sine(frequency, 0.5, SECONDS, RATE);
            let analyzer = settled(&tone, &tone, RATE);
            let band = loudest(&analyzer);
            assert!((band.frequency / frequency).log2().abs() < 0.1, "{frequency}: {band:?}");
        }
    }

    #[test]
    fn nothing_above_the_bands_is_shown() {
        let rate = 96_000.0;
        let tone = sine(30_000.0, 0.5, 96_000 * 2, rate);
        let analyzer = settled(&tone, &tone, rate);
        assert!(analyzer.readings().iter().all(|band| band.frequency < 20_500.0));
        assert!(loudest(&analyzer).level_db < -60.0, "{:?}", loudest(&analyzer));
    }

    #[test]
    fn full_scale_reads_zero_and_silence_the_floor() {
        let tone = sine(1_000.0, 1.0, SECONDS, RATE);
        let band = loudest(&settled(&tone, &tone, RATE));
        assert!(band.level_db.abs() < 0.5 && band.level_db < 0.3, "{band:?}");
        let silent = vec![0.0; SECONDS];
        let analyzer = settled(&silent, &silent, RATE);
        assert!(analyzer.readings().iter().all(|band| band.level_db == FLOOR_DB));
    }

    #[test]
    fn levels_rise_fast_fall_slowly_and_peaks_hold() {
        let mut analyzer = MixSpaceAnalyzer::new(RATE, DEFAULT_BANDS_PER_OCTAVE);
        let tone = sine(1_000.0, 0.5, SECONDS, RATE);
        analyzer.push(&tone, &tone);
        let loud = band_at(&analyzer, 1_000.0);
        // The window is 170 ms long, so silence shows once it has filled it.
        let silent = vec![0.0; 48_000 * 3 / 10];
        analyzer.push(&silent, &silent);
        let after = band_at(&analyzer, 1_000.0);
        assert!(after.level_db < loud.level_db - 10.0, "{after:?}");
        assert!(after.level_db > FLOOR_DB + 10.0, "a 0.3 s release is still falling: {after:?}");
        assert!(after.peak_db > after.level_db, "{after:?}");
    }

    #[test]
    fn hostile_input_and_sample_rates_are_safe() {
        let mut analyzer = MixSpaceAnalyzer::new(f64::NAN, 0);
        let junk = [f32::NAN, f32::INFINITY, -1e30, 0.5];
        for _ in 0..20_000 {
            analyzer.push(&junk, &junk[..2]);
        }
        assert!(analyzer.readings().iter().all(|band| band.level_db.is_finite()));
        assert!(analyzer.frames() > 0);
        analyzer.reset();
        assert_eq!(analyzer.frames(), 0);
    }
}

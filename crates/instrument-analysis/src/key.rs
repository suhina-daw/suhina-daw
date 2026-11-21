// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The tonal center of material with several notes: a spectral pitch-class profile correlated
//! with the published Krumhansl-Kessler key profiles. Preparation only; see
//! instrument-analysis.md › Loops and melodic material.

use rustfft::{Fft, FftPlanner, num_complex::Complex64};
use std::{f64::consts::TAU, sync::Arc};

/// Window length of the pitch-class profile at the analysis rate (2.7 Hz bins).
pub const KEY_FRAME: usize = 8192;
/// Windows per channel within the eight-second bound.
pub const MAX_KEY_FRAMES: usize = crate::MAX_SECONDS * crate::RATE as usize / KEY_FRAME;
const MIN_KEY_FRAMES: usize = 2;
const LOW_HZ: f64 = 55.0;
const HIGH_HZ: f64 = 4000.0;
/// Peaks quieter than this fraction of a window's strongest (-40 dB) are ignored.
const PEAK_FLOOR: f64 = 0.01;
const PEAKS_PER_FRAME: usize = 64;
/// A peak must stand this many times above the median of its neighbours 3..=32 bins away.
const PROMINENCE: f64 = 10.0;
/// A peak within this many semitones of 2..=8 times a lower kept peak is that peak's harmonic.
const HARMONIC_TOLERANCE: f64 = 0.25;
/// A window's lowest note is its lowest kept peak within this fraction (-20 dB) of the strongest.
const LOWEST_FLOOR: f64 = 0.1;
/// A loop's root is its tonic nearest this key (middle C, shown as C3), so the keys around the
/// middle of the keyboard transpose it by at most six semitones.
pub const ROOT_CENTER: f64 = 60.0;
/// Below this Pearson correlation the material has no clear tonal center.
pub const MIN_KEY_CORRELATION: f32 = 0.6;

/// Krumhansl and Kessler (1982), major and minor key profiles from the tonic upwards.
const MAJOR: [f64; 12] = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const MINOR: [f64; 12] = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyMode {
    Major,
    Minor,
}

/// One key and how well the profile fits it; a correlation, not a probability.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyCandidate {
    /// Pitch class of the tonic, 0 is C.
    pub tonic: u8,
    pub mode: KeyMode,
    pub correlation: f32,
}

/// A proposed root for melodic or chordal material: its key's tonic nearest middle C.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyProposal {
    pub root: u8,
    pub key: KeyCandidate,
    pub runner_up: KeyCandidate,
    /// Magnitude-weighted deviation of the material from equal temperament, in cents.
    pub tuning_cents: f32,
    /// Median of each window's lowest prominent pitch, fractional MIDI.
    pub median_midi: f32,
    /// Profile windows used across channels.
    pub windows: usize,
}

/// Collects spectral peaks of one or two channels; bounded by `MAX_KEY_FRAMES` per channel.
pub(crate) struct PitchClasses {
    fft: Arc<dyn Fft<f64>>,
    window: Vec<f64>,
    buffer: Vec<Complex64>,
    scratch: Vec<Complex64>,
    /// (fractional MIDI, magnitude) of every kept peak.
    peaks: Vec<(f64, f64)>,
    /// Each window's lowest prominent pitch, fractional MIDI: the register of the material.
    lowest: Vec<f64>,
    frames: usize,
    shortest_channel: usize,
}

impl PitchClasses {
    pub(crate) fn new(channels: usize) -> Self {
        let fft = FftPlanner::<f64>::new().plan_fft_forward(KEY_FRAME);
        let scratch = vec![Complex64::default(); fft.get_inplace_scratch_len()];
        Self {
            fft,
            window: (0..KEY_FRAME).map(|i| 0.5 - 0.5 * (TAU * i as f64 / KEY_FRAME as f64).cos()).collect(),
            buffer: vec![Complex64::default(); KEY_FRAME],
            scratch,
            peaks: Vec::with_capacity(MAX_KEY_FRAMES * PEAKS_PER_FRAME * channels),
            lowest: Vec::with_capacity(MAX_KEY_FRAMES * channels),
            frames: 0,
            shortest_channel: usize::MAX,
        }
    }

    /// Windows of one channel's prepared signal; `step` is called per window and false cancels
    /// (None). Returns how many windows were inspected.
    pub(crate) fn add_channel(&mut self, mono: &[f32], mut step: impl FnMut() -> bool) -> Option<usize> {
        let count = (mono.len() / KEY_FRAME).min(MAX_KEY_FRAMES);
        self.shortest_channel = self.shortest_channel.min(count);
        let bin_hz = f64::from(crate::RATE) / KEY_FRAME as f64;
        let low = (LOW_HZ / bin_hz).ceil() as usize;
        let high = ((HIGH_HZ / bin_hz).floor() as usize).min(KEY_FRAME / 2 - 2);
        let mut frame_peaks: Vec<(f64, f64)> = Vec::with_capacity(KEY_FRAME / 2);
        let mut kept: Vec<(f64, f64)> = Vec::with_capacity(PEAKS_PER_FRAME);
        let mut neighbours: Vec<f64> = Vec::with_capacity(60);
        for index in 0..count {
            if !step() {
                return None;
            }
            let Some(input) = mono.get(index * KEY_FRAME..(index + 1) * KEY_FRAME) else { break };
            let mean = input.iter().map(|x| f64::from(*x)).sum::<f64>() / KEY_FRAME as f64;
            for ((slot, x), w) in self.buffer.iter_mut().zip(input).zip(&self.window) {
                *slot = Complex64::new((f64::from(*x) - mean) * w, 0.0);
            }
            self.fft.process_with_scratch(&mut self.buffer, &mut self.scratch);
            let magnitude = |bin: usize| self.buffer.get(bin).map_or(0.0, |c| c.norm());
            let strongest = (low..=high).map(magnitude).fold(0.0_f64, f64::max);
            if strongest <= 1e-9 {
                continue;
            }
            frame_peaks.clear();
            for bin in low..=high {
                let (before, value, after) = (magnitude(bin - 1), magnitude(bin), magnitude(bin + 1));
                if value < strongest * PEAK_FLOOR || value <= before || value < after {
                    continue;
                }
                // Noise has local maxima too; a partial stands well above its surroundings.
                neighbours.clear();
                neighbours
                    .extend((3..=32).flat_map(|d| [bin.checked_sub(d), bin.checked_add(d)]).flatten().map(magnitude));
                neighbours.sort_by(f64::total_cmp);
                if neighbours.get(neighbours.len() / 2).is_some_and(|median| value < PROMINENCE * median) {
                    continue;
                }
                // Parabolic interpolation on log magnitude.
                let (a, b, c) = (before.max(1e-12).ln(), value.ln(), after.max(1e-12).ln());
                let curvature = a - 2.0 * b + c;
                let offset = if curvature < 0.0 { (0.5 * (a - c) / curvature).clamp(-0.5, 0.5) } else { 0.0 };
                let hz = (bin as f64 + offset) * bin_hz;
                frame_peaks.push((69.0 + 12.0 * (hz / 440.0).log2(), value));
            }
            frame_peaks.sort_by(|x, y| y.1.total_cmp(&x.1));
            frame_peaks.truncate(PEAKS_PER_FRAME);
            // From the lowest up: a harmonic of a kept peak is not a note of its own, and would
            // otherwise lend every note's fifth and third to the profile.
            frame_peaks.sort_by(|x, y| x.0.total_cmp(&y.0));
            kept.clear();
            for &(midi, level) in &frame_peaks {
                let harmonic = kept.iter().any(|(lower, _)| {
                    (2..=8).any(|k| (midi - (lower + 12.0 * f64::from(k).log2())).abs() < HARMONIC_TOLERANCE)
                });
                if !harmonic {
                    kept.push((midi, level));
                }
            }
            let loudest = kept.iter().map(|p| p.1).fold(0.0_f64, f64::max);
            if let Some(&(midi, _)) = kept.iter().find(|p| p.1 >= loudest * LOWEST_FLOOR) {
                self.lowest.push(midi);
            }
            self.peaks.extend_from_slice(&kept);
            self.frames += 1;
        }
        Some(count)
    }

    /// The key estimate, or None for too little material or no clear tonal center.
    pub(crate) fn key(&self) -> Option<KeyProposal> {
        if self.shortest_channel < MIN_KEY_FRAMES || self.shortest_channel == usize::MAX || self.peaks.is_empty() {
            return None;
        }
        // Circular mean of each peak's deviation from the nearest equal-tempered pitch.
        let (sine, cosine) = self.peaks.iter().fold((0.0, 0.0), |(s, c), (midi, weight)| {
            let angle = TAU * (midi - midi.round());
            (s + weight * angle.sin(), c + weight * angle.cos())
        });
        let tuning = if sine == 0.0 && cosine == 0.0 { 0.0 } else { sine.atan2(cosine) / TAU };
        let mut profile = [0.0_f64; 12];
        for (midi, weight) in &self.peaks {
            let class = ((midi - tuning).round() as i64).rem_euclid(12) as usize;
            if let Some(slot) = profile.get_mut(class) {
                *slot += weight;
            }
        }
        let mut candidates = Vec::with_capacity(24);
        for tonic in 0..12u8 {
            for (mode, reference) in [(KeyMode::Major, &MAJOR), (KeyMode::Minor, &MINOR)] {
                let rotated: [f64; 12] = std::array::from_fn(|class| {
                    reference.get((class + 12 - usize::from(tonic)) % 12).copied().unwrap_or(0.0)
                });
                candidates.push(KeyCandidate { tonic, mode, correlation: pearson(&profile, &rotated) as f32 });
            }
        }
        candidates.sort_by(|a, b| {
            b.correlation
                .total_cmp(&a.correlation)
                .then(a.tonic.cmp(&b.tonic))
                .then((a.mode == KeyMode::Minor).cmp(&(b.mode == KeyMode::Minor)))
        });
        let (&key, &runner_up) = (candidates.first()?, candidates.get(1)?);
        if !key.correlation.is_finite() || key.correlation < MIN_KEY_CORRELATION {
            return None;
        }
        let mut lowest = self.lowest.clone();
        lowest.sort_by(f64::total_cmp);
        let median = *lowest.get((lowest.len().checked_sub(1)?) / 2)?;
        Some(KeyProposal {
            root: nearest_root(key.tonic, ROOT_CENTER)?,
            key,
            runner_up,
            tuning_cents: (tuning * 100.0) as f32,
            median_midi: median as f32,
            windows: self.frames,
        })
    }
}

fn pearson(x: &[f64; 12], y: &[f64; 12]) -> f64 {
    let mean_x = x.iter().sum::<f64>() / 12.0;
    let mean_y = y.iter().sum::<f64>() / 12.0;
    let (mut covariance, mut var_x, mut var_y) = (0.0, 0.0, 0.0);
    for (a, b) in x.iter().zip(y) {
        covariance += (a - mean_x) * (b - mean_y);
        var_x += (a - mean_x).powi(2);
        var_y += (b - mean_y).powi(2);
    }
    if var_x <= 0.0 || var_y <= 0.0 { 0.0 } else { covariance / (var_x * var_y).sqrt() }
}

/// The MIDI note of pitch class `tonic` nearest to `median` (the lower on a tie), in 0..=127.
pub(crate) fn nearest_root(tonic: u8, median: f64) -> Option<u8> {
    if !median.is_finite() || tonic > 11 {
        return None;
    }
    let tonic = f64::from(tonic);
    let lower = tonic + 12.0 * ((median - tonic) / 12.0).floor();
    let mut root = if median - lower <= lower + 12.0 - median { lower } else { lower + 12.0 };
    while root > 127.0 {
        root -= 12.0;
    }
    while root < 0.0 {
        root += 12.0;
    }
    Some(root as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_is_the_tonic_nearest_the_median_pitch() {
        assert_eq!(nearest_root(9, 60.0), Some(57));
        assert_eq!(nearest_root(9, 64.0), Some(69));
        assert_eq!(nearest_root(0, 66.0), Some(60), "a tie takes the lower octave");
        assert_eq!(nearest_root(7, 140.0), Some(127));
        assert_eq!(nearest_root(0, -20.0), Some(0));
        assert_eq!(nearest_root(12, 60.0), None);
        assert_eq!(nearest_root(0, f64::NAN), None);
    }

    #[test]
    fn profiles_correlate_perfectly_with_their_own_key_only() {
        let mut classes = PitchClasses::new(1);
        // A profile equal to A minor's: fake peaks weighted by the rotated profile.
        for class in 0..12 {
            classes.peaks.push((60.0 + class as f64, MINOR[(class + 12 - 9) % 12]));
        }
        classes.lowest.push(60.0);
        classes.shortest_channel = MIN_KEY_FRAMES;
        let key = classes.key().unwrap();
        assert_eq!((key.key.tonic, key.key.mode), (9, KeyMode::Minor));
        assert!((key.key.correlation - 1.0).abs() < 1e-6);
        assert!(key.runner_up.correlation < 1.0);
        assert!(key.tuning_cents.abs() < 1e-3);
        assert_eq!(key.root, 57, "the A nearest middle C");
    }
}

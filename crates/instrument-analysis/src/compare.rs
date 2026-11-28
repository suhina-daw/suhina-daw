// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Level-matched comparison of a fitted render with its source: level difference and octave-band
//! spectral distance. Preparation only; see instrument-modal-fitting.md › Audition and comparison.

use rustfft::{FftPlanner, num_complex::Complex64};
use std::f64::consts::TAU;

const FRAME: usize = 2048;
const HOP: usize = 1024;
const MAX_SECONDS: usize = 8;
const BAND_CENTERS: [f64; 9] = [63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];

/// One octave band's difference after level matching, in dB (fitted minus original).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandDistance {
    pub center_hz: f64,
    pub difference_db: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    /// Fitted RMS level relative to the original before matching, in dB.
    pub level_difference_db: f64,
    /// Mean absolute band difference after matching, in dB.
    pub mean_distance_db: f64,
    pub bands: Vec<BandDistance>,
}

/// Compare two mono signals at `sample_rate`; None when either is silent or too short for one
/// window. Bounded to eight seconds.
pub fn compare(original: &[f32], fitted: &[f32], sample_rate: u32) -> Option<Comparison> {
    let length = original.len().min(fitted.len()).min(sample_rate as usize * MAX_SECONDS);
    if length < FRAME || sample_rate == 0 {
        return None;
    }
    let (original, fitted) = (original.get(..length)?, fitted.get(..length)?);
    let rms = |x: &[f32]| (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64).sqrt();
    let (a, b) = (rms(original), rms(fitted));
    if !(a > 1e-12 && b > 1e-12) {
        return None;
    }
    let level_difference_db = 20.0 * (b / a).log10();
    let spectrum_a = average_power(original, 1.0);
    let spectrum_b = average_power(fitted, a / b);
    let bin_hz = f64::from(sample_rate) / FRAME as f64;
    let nyquist = f64::from(sample_rate) / 2.0;
    let band_power = |spectrum: &[f64], center: f64| -> f64 {
        let (low, high) = (center / 2.0_f64.sqrt(), center * 2.0_f64.sqrt());
        spectrum
            .iter()
            .enumerate()
            .filter(|(bin, _)| (low..high).contains(&(*bin as f64 * bin_hz)))
            .map(|(_, p)| p)
            .sum()
    };
    let powers: Vec<(f64, f64, f64)> = BAND_CENTERS
        .iter()
        .copied()
        .filter(|c| c * 2.0_f64.sqrt() <= nyquist)
        .map(|c| (c, band_power(&spectrum_a, c), band_power(&spectrum_b, c)))
        .collect();
    let loudest = powers.iter().map(|p| p.1).fold(0.0_f64, f64::max);
    let bands: Vec<BandDistance> = powers
        .iter()
        .filter(|(_, original, _)| *original > loudest * 1e-6 && *original > 0.0)
        .map(|(center, original, fitted)| BandDistance {
            center_hz: *center,
            difference_db: 10.0 * (fitted.max(1e-30) / original).log10(),
        })
        .collect();
    if bands.is_empty() {
        return None;
    }
    let mean_distance_db = bands.iter().map(|b| b.difference_db.abs()).sum::<f64>() / bands.len() as f64;
    Some(Comparison { level_difference_db, mean_distance_db, bands })
}

/// Average power spectrum of Hann windows, the signal multiplied by `gain`.
fn average_power(signal: &[f32], gain: f64) -> Vec<f64> {
    let fft = FftPlanner::<f64>::new().plan_fft_forward(FRAME);
    let window: Vec<f64> = (0..FRAME).map(|i| 0.5 - 0.5 * (TAU * i as f64 / FRAME as f64).cos()).collect();
    let mut buffer = vec![Complex64::default(); FRAME];
    let mut scratch = vec![Complex64::default(); fft.get_inplace_scratch_len()];
    let mut power = vec![0.0; FRAME / 2];
    let mut frames = 0u32;
    let mut start = 0;
    while let Some(frame) = signal.get(start..start + FRAME) {
        for ((slot, x), w) in buffer.iter_mut().zip(frame).zip(&window) {
            *slot = Complex64::new(f64::from(*x) * gain * w, 0.0);
        }
        fft.process_with_scratch(&mut buffer, &mut scratch);
        for (p, c) in power.iter_mut().zip(&buffer) {
            *p += c.norm_sqr();
        }
        frames += 1;
        start += HOP;
    }
    if frames > 0 {
        for p in &mut power {
            *p /= f64::from(frames);
        }
    }
    power
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(partials: &[(f64, f64)], seconds: f64) -> Vec<f32> {
        (0..(48000.0 * seconds) as usize)
            .map(|n| partials.iter().map(|(f, a)| a * (TAU * f * n as f64 / 48000.0).sin()).sum::<f64>() as f32)
            .collect()
    }

    #[test]
    fn the_same_sound_is_close_and_level_is_reported_apart_from_timbre() {
        let original = tone(&[(220.0, 0.5), (660.0, 0.2), (3300.0, 0.05)], 1.0);
        let same = compare(&original, &original, 48000).unwrap();
        assert!(same.level_difference_db.abs() < 1e-9 && same.mean_distance_db < 1e-9);
        let quieter: Vec<f32> = original.iter().map(|x| x * 0.5).collect();
        let half = compare(&original, &quieter, 48000).unwrap();
        assert!((half.level_difference_db + 6.0206).abs() < 1e-3, "{half:?}");
        assert!(half.mean_distance_db < 1e-6, "level matching removes the level: {half:?}");
        let darker = tone(&[(220.0, 0.5), (660.0, 0.2)], 1.0);
        let dull = compare(&original, &darker, 48000).unwrap();
        let high = dull.bands.iter().find(|b| b.center_hz == 4000.0).unwrap();
        assert!(high.difference_db < -20.0, "{dull:?}");
        assert!(dull.mean_distance_db > half.mean_distance_db);
    }

    #[test]
    fn silence_and_short_signals_have_no_comparison() {
        let original = tone(&[(440.0, 0.5)], 0.5);
        assert!(compare(&original, &vec![0.0; original.len()], 48000).is_none());
        assert!(compare(&original[..1000], &original[..1000], 48000).is_none());
        assert!(compare(&original, &original, 0).is_none());
    }
}

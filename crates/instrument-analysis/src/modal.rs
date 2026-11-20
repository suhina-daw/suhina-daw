// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded Fourier approximation of separated exponentially decaying resonances.
//! Preparation only. Assumptions and primary sources: instrument-modal-fitting.md.

use crate::{Error, MAX_SECONDS, RATE};
use rustfft::{FftPlanner, num_complex::Complex64};
use std::{f64::consts::TAU, ops::Range};
use suhina_dsp::{
    Resampler,
    modal::profile::{MAX_PROFILE_MODES, ModalProfile, ProfileMode},
};

pub const VERSION: u32 = 2;
const MIN_FRAME: usize = 1024;
const MAX_FRAME: usize = 8192;
const MAX_FRAMES: usize = 12;
const MAX_CANDIDATES: usize = 32;
const CHUNK: usize = 256;
const REFINEMENTS: usize = 20;
const DB: f64 = 20.0 / std::f64::consts::LN_10;
/// A mode's span ends this far below its own peak, where leakage and noise dominate.
const SPAN_FLOOR_DB: f64 = 60.0;
/// A mode's span ends where its level rises this much above its lowest so far.
const RERISE_DB: f64 = 6.0;
const MAX_ESTIMATION_DB: f64 = 2.5;
const MAX_VALIDATION_DB: f64 = 3.0;
const MIN_DROP_DB: f64 = 1.0;
/// The fitted fall must exceed the larger envelope error this many times.
const DROP_TO_ERROR: f64 = 3.0;
/// A failing span this long is fitted once more on its last two thirds.
const LATE_RETRY_WINDOWS: usize = 9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Fitted,
    Silent,
    TooShort,
    NoDecayingModes,
}

/// Errors measure one mode's amplitude trajectory, not waveform fidelity or realism.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModeEvidence {
    pub frequency_hz: f64,
    pub decay_seconds: f64,
    pub estimation_rms_db: f64,
    pub validation_rms_db: f64,
    pub estimation_windows: usize,
    pub validation_windows: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub version: u32,
    pub range: Range<usize>,
    /// Zero-based channel with greatest AC energy. Stereo width is not reconstructed.
    pub channel: u16,
    pub frame_size: usize,
    pub analyzed_seconds: f64,
    pub outcome: Outcome,
    pub profile: Option<ModalProfile>,
    pub modes: Vec<ModeEvidence>,
}

struct Work<F> {
    done: usize,
    total: usize,
    progress: F,
}
impl<F: FnMut(usize, usize) -> bool> Work<F> {
    fn step(&mut self) -> Result<(), Error> {
        if !(self.progress)(self.done, self.total) {
            return Err(Error::Cancelled);
        }
        self.done = self.done.saturating_add(1).min(self.total);
        Ok(())
    }
    fn finish(&mut self) -> Result<(), Error> {
        if (self.progress)(self.total, self.total) { Ok(()) } else { Err(Error::Cancelled) }
    }
}

/// Fit one isolated struck source, retaining the caller's chosen reference key.
/// All preparation is bounded, deterministic, offline and cooperatively cancellable.
pub fn fit(
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    range: Range<usize>,
    reference_key: u8,
    progress: impl FnMut(usize, usize) -> bool,
) -> Result<Report, Error> {
    if reference_key > 127 {
        return Err(Error::InvalidSettings);
    }
    if !(1..=2).contains(&channels)
        || !(1000..=384000).contains(&sample_rate)
        || !samples.len().is_multiple_of(usize::from(channels))
    {
        return Err(Error::InvalidFormat);
    }
    let channels = usize::from(channels);
    if range.start >= range.end || range.end > samples.len() / channels {
        return Err(Error::InvalidRange);
    }
    let length = (range.end - range.start).min(sample_rate as usize * MAX_SECONDS);
    let range = range.start..range.start + length;
    let first = range.start.checked_mul(channels).ok_or(Error::InvalidRange)?;
    let last = range.end.checked_mul(channels).ok_or(Error::InvalidRange)?;
    let input = samples.get(first..last).ok_or(Error::InvalidRange)?;
    let total = 2 * length.div_ceil(CHUNK) + MAX_CANDIDATES * (REFINEMENTS + MAX_FRAMES + 4) + 4;
    let mut work = Work { done: 0, total, progress };
    let mut report = Report {
        version: VERSION,
        range,
        channel: 0,
        frame_size: 0,
        analyzed_seconds: 0.0,
        outcome: Outcome::NoDecayingModes,
        profile: None,
        modes: Vec::new(),
    };
    let mut sums = [0.0_f64; 2];
    let mut squares = [0.0_f64; 2];
    let mut peaks = [0.0_f64; 2];
    let mut minima = [f64::INFINITY; 2];
    let mut maxima = [f64::NEG_INFINITY; 2];
    for chunk in input.chunks(CHUNK * channels) {
        work.step()?;
        for frame in chunk.chunks_exact(channels) {
            for (channel, &sample) in frame.iter().enumerate() {
                if !sample.is_finite() {
                    return Err(Error::NonFinite);
                }
                if let (Some(sum), Some(square), Some(peak)) =
                    (sums.get_mut(channel), squares.get_mut(channel), peaks.get_mut(channel))
                {
                    let value = f64::from(sample);
                    *sum += value;
                    *square += value * value;
                    *peak = peak.max(value.abs());
                    if let (Some(low), Some(high)) = (minima.get_mut(channel), maxima.get_mut(channel)) {
                        *low = low.min(value);
                        *high = high.max(value);
                    }
                }
            }
        }
    }
    let energy = |c: usize| squares[c] - sums[c] * sums[c] / length as f64;
    let channel = usize::from(channels == 2 && energy(1) > energy(0));
    report.channel = channel as u16;
    // A constant channel is silent even when the one-pass energy keeps rounding residue: summing
    // tens of thousands of equal f32 values leaves more than the energy threshold in f64.
    let span = maxima[channel] - minima[channel];
    if peaks[channel] <= 1e-12
        || span <= peaks[channel] * 1e-9
        || energy(channel) <= peaks[channel].powi(2) * length as f64 * 1e-14
    {
        report.outcome = Outcome::Silent;
        work.finish()?;
        return Ok(report);
    }
    let mean = sums[channel] / length as f64;
    let scale = (maxima[channel] - mean).abs().max((minima[channel] - mean).abs()).max(1e-12);
    let mut resampler = Resampler::new(f64::from(sample_rate), f64::from(RATE));
    let mut output = vec![0.0; (CHUNK as f64 * f64::from(RATE) / f64::from(sample_rate)).ceil() as usize + 1024];
    let capacity = (length as f64 * f64::from(RATE) / f64::from(sample_rate)).ceil() as usize + 1024;
    let mut mono = Vec::with_capacity(capacity);
    let mut block = [0.0; CHUNK];
    for chunk in input.chunks(CHUNK * channels) {
        work.step()?;
        let frames = chunk.len() / channels;
        for (frame, value) in chunk.chunks_exact(channels).zip(block.iter_mut()) {
            *value = (f64::from(frame.get(channel).copied().unwrap_or(0.0)) - mean) / scale;
        }
        let count = resampler.process(block.get(..frames).ok_or(Error::InvalidRange)?, &mut output);
        mono.extend(output.iter().take(count).copied());
    }
    let count = resampler.flush(&mut output);
    mono.extend(output.iter().take(count).copied());
    mono.truncate((length as f64 * f64::from(RATE) / f64::from(sample_rate)).floor() as usize);
    // Long silent tails must not turn a quick strike into widely separated empty windows.
    // Use the residual's AC span, rather than a DC offset introduced by mean subtraction.
    let tail_mean = mono.iter().rev().take(MIN_FRAME).sum::<f64>() / mono.len().clamp(1, MIN_FRAME) as f64;
    let mono_peak = mono.iter().map(|x| (x - tail_mean).abs()).fold(0.0_f64, f64::max);
    let useful = mono.iter().rposition(|x| (x - tail_mean).abs() > mono_peak * 1e-5).map_or(0, |i| i + 1);
    mono.truncate(useful);
    let mut frame_size = MIN_FRAME;
    while frame_size < MAX_FRAME && frame_size * 2 * MAX_FRAMES <= mono.len() {
        frame_size *= 2;
    }
    let frames = (mono.len() / frame_size).min(MAX_FRAMES);
    if frames < 6 {
        report.outcome = Outcome::TooShort;
        work.finish()?;
        return Ok(report);
    }
    report.frame_size = frame_size;
    report.analyzed_seconds = (frames * frame_size) as f64 / f64::from(RATE);
    let window = hann(frame_size);
    // Detection resolves the partials of a low note that a short envelope window would merge.
    let mut detection_size = frame_size;
    while detection_size < MAX_FRAME && detection_size * 2 <= mono.len() {
        detection_size *= 2;
    }
    let detection = mono.get(..detection_size).ok_or(Error::InvalidRange)?;
    let detection_window = hann(detection_size);
    let mut bins: Vec<Complex64> =
        detection.iter().zip(&detection_window).map(|(x, w)| Complex64::new(x * w, 0.0)).collect();
    let fft = FftPlanner::<f64>::new().plan_fft_forward(detection_size);
    let mut scratch = vec![Complex64::default(); fft.get_inplace_scratch_len()];
    work.step()?;
    fft.process_with_scratch(&mut bins, &mut scratch);
    work.step()?;
    let reference_hz = 440.0 * 2.0_f64.powf((f64::from(reference_key) - 69.0) / 12.0);
    let low_hz = 40.0_f64.max(reference_hz / 256.0);
    let high_hz = 10000.0_f64.min(f64::from(sample_rate) * 0.4).min(reference_hz * 256.0);
    let bin_hz = f64::from(RATE) / detection_size as f64;
    // Two candidates inside one envelope window's main lobe would measure the same energy.
    let separation_bins = 2.0 * detection_size as f64 / frame_size as f64;
    let spectrum: Vec<f64> = bins.iter().take(detection_size / 2).map(|x| x.norm()).collect();
    let highest = spectrum.iter().copied().fold(0.0_f64, f64::max);
    let mut peaks = Vec::with_capacity(detection_size / 2);
    for bin in 2..spectrum.len().saturating_sub(9) {
        let hz = bin as f64 * bin_hz;
        if hz < low_hz || hz > high_hz {
            continue;
        }
        let Some(&value) = spectrum.get(bin) else { continue };
        let (Some(&before), Some(&after)) = (spectrum.get(bin - 1), spectrum.get(bin + 1)) else { continue };
        if value < highest * 1e-3 || value <= before || value <= after {
            continue;
        }
        let mut nearby = [0.0_f64; 12];
        for (slot, delta) in nearby.iter_mut().zip([-8_isize, -7, -6, -5, -4, -3, 3, 4, 5, 6, 7, 8]) {
            *slot = bin.checked_add_signed(delta).and_then(|b| spectrum.get(b)).copied().unwrap_or(value);
        }
        nearby.sort_by(f64::total_cmp);
        if value >= nearby[6] * 10.0 {
            peaks.push((bin, value));
        }
    }
    peaks.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut candidates = Vec::with_capacity(MAX_CANDIDATES);
    for (bin, _) in peaks {
        if candidates.iter().all(|other: &usize| other.abs_diff(bin) as f64 >= separation_bins.max(3.0)) {
            candidates.push(bin);
        }
        if candidates.len() == MAX_CANDIDATES {
            break;
        }
    }
    let mut accepted = Vec::with_capacity(MAX_CANDIDATES);
    for bin in candidates {
        let hz = refine(
            detection,
            &detection_window,
            (bin as f64 - 0.75) * bin_hz,
            (bin as f64 + 0.75) * bin_hz,
            &mut work,
        )?;
        if hz < low_hz || hz > high_hz {
            continue;
        }
        let mut measurements = Vec::with_capacity(frames);
        for index in 0..frames {
            work.step()?;
            let start = index * frame_size;
            let frame = mono.get(start..start + frame_size).ok_or(Error::InvalidRange)?;
            let amplitude = projection(frame, &window, hz);
            if amplitude > 1e-7 {
                measurements.push((index, (start + frame_size / 2) as f64 / f64::from(RATE), amplitude.ln()));
            }
        }
        let Some((intercept, slope, evidence)) = estimate(hz, &measurements) else { continue };
        let window_bias = window
            .iter()
            .enumerate()
            .map(|(i, w)| w * (slope * (i as f64 - frame_size as f64 / 2.0) / f64::from(RATE)).exp())
            .sum::<f64>()
            / window.iter().sum::<f64>();
        let magnitude = intercept.exp() / window_bias;
        if magnitude.is_finite() && magnitude > 0.0 {
            accepted.push((
                ProfileMode { ratio: hz / reference_hz, magnitude, decay_seconds: evidence.decay_seconds },
                evidence,
            ));
        }
    }
    accepted.sort_by(|a, b| b.0.magnitude.total_cmp(&a.0.magnitude).then_with(|| a.0.ratio.total_cmp(&b.0.ratio)));
    accepted.truncate(MAX_PROFILE_MODES);
    accepted.sort_by(|a, b| a.0.ratio.total_cmp(&b.0.ratio));
    let sum = accepted.iter().map(|v| v.0.magnitude).sum::<f64>().max(1.0);
    let modes: Vec<ProfileMode> =
        accepted.iter().map(|(m, _)| ProfileMode { magnitude: m.magnitude / sum, ..*m }).collect();
    if !modes.is_empty() {
        report.profile =
            Some(ModalProfile::new(reference_key, reference_hz, &modes).map_err(|_| Error::InvalidSettings)?);
        report.modes = accepted.into_iter().map(|(_, e)| e).collect();
        report.outcome = Outcome::Fitted;
    }
    work.finish()?;
    Ok(report)
}

fn projection(frame: &[f64], window: &[f64], hz: f64) -> f64 {
    let (sin, cos) = (TAU * hz / f64::from(RATE)).sin_cos();
    let (mut real, mut imaginary, mut re, mut im, mut sum) = (1.0, 0.0, 0.0, 0.0, 0.0);
    for (&x, &w) in frame.iter().zip(window) {
        re += x * w * real;
        im += x * w * imaginary;
        sum += w;
        (real, imaginary) = (real * cos - imaginary * sin, real * sin + imaginary * cos);
    }
    2.0 * re.hypot(im) / sum.max(1e-12)
}

fn refine<F: FnMut(usize, usize) -> bool>(
    frame: &[f64],
    window: &[f64],
    mut left: f64,
    mut right: f64,
    work: &mut Work<F>,
) -> Result<f64, Error> {
    const RATIO: f64 = 0.6180339887498949;
    let mut a = right - RATIO * (right - left);
    let mut b = left + RATIO * (right - left);
    let mut va = projection(frame, window, a);
    let mut vb = projection(frame, window, b);
    for _ in 0..REFINEMENTS {
        work.step()?;
        if va > vb {
            right = b;
            b = a;
            vb = va;
            a = right - RATIO * (right - left);
            va = projection(frame, window, a);
        } else {
            left = a;
            a = b;
            va = vb;
            b = left + RATIO * (right - left);
            vb = projection(frame, window, b);
        }
    }
    Ok((left + right) * 0.5)
}

fn hann(size: usize) -> Vec<f64> {
    (0..size).map(|i| 0.5 - 0.5 * (TAU * i as f64 / size as f64).cos()).collect()
}

/// Decay of one mode from its (window, time, log amplitude) measurements: fitted on its own
/// span, and once more on the span's last two thirds when a long span fails.
fn estimate(hz: f64, values: &[(usize, f64, f64)]) -> Option<(f64, f64, ModeEvidence)> {
    let span = envelope_span(values);
    fit_span(hz, span).or_else(|| {
        if span.len() < LATE_RETRY_WINDOWS {
            return None;
        }
        fit_span(hz, span.get(span.len() / 3..)?)
    })
}

/// From the loudest of the first three windows (an attack is not a decay) until the level is
/// more than `SPAN_FLOOR_DB` below that peak or `RERISE_DB` above its lowest so far.
fn envelope_span(values: &[(usize, f64, f64)]) -> &[(usize, f64, f64)] {
    let head = values.get(..values.len().min(3)).unwrap_or_default();
    let mut start = None;
    for (index, value) in head.iter().enumerate() {
        if start.is_none_or(|(_, peak)| value.2 > peak) {
            start = Some((index, value.2));
        }
    }
    let Some((start, peak)) = start else { return &[] };
    let mut low = peak;
    let mut end = start;
    for value in values.iter().skip(start) {
        if (peak - value.2) * DB > SPAN_FLOOR_DB || (value.2 - low) * DB > RERISE_DB {
            break;
        }
        low = low.min(value.2);
        end += 1;
    }
    values.get(start..end).unwrap_or_default()
}

fn fit_span(hz: f64, span: &[(usize, f64, f64)]) -> Option<(f64, f64, ModeEvidence)> {
    let training: Vec<_> = span.iter().step_by(2).collect();
    let validation: Vec<_> = span.iter().skip(1).step_by(2).collect();
    if training.len() < 3 || validation.len() < 3 {
        return None;
    }
    let count = training.len() as f64;
    let mean_t = training.iter().map(|v| v.1).sum::<f64>() / count;
    let mean_y = training.iter().map(|v| v.2).sum::<f64>() / count;
    let variance = training.iter().map(|v| (v.1 - mean_t).powi(2)).sum::<f64>();
    if variance <= 0.0 {
        return None;
    }
    let slope = training.iter().map(|v| (v.1 - mean_t) * (v.2 - mean_y)).sum::<f64>() / variance;
    if !slope.is_finite() || slope >= 0.0 {
        return None;
    }
    let decay = -1000.0_f64.ln() / slope;
    if !(0.001..=60.0).contains(&decay) {
        return None;
    }
    let intercept = mean_y - slope * mean_t;
    let rms = |v: &[&(usize, f64, f64)]| {
        (v.iter().map(|p| ((p.2 - intercept - slope * p.1) * DB).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
    };
    let estimation_rms_db = rms(&training);
    let validation_rms_db = rms(&validation);
    let drop_db = -slope * (training.last()?.1 - training.first()?.1) * DB;
    if drop_db < MIN_DROP_DB
        || drop_db < DROP_TO_ERROR * estimation_rms_db.max(validation_rms_db)
        || estimation_rms_db > MAX_ESTIMATION_DB
        || validation_rms_db > MAX_VALIDATION_DB
    {
        return None;
    }
    Some((
        intercept,
        slope,
        ModeEvidence {
            frequency_hz: hz,
            decay_seconds: decay,
            estimation_rms_db,
            validation_rms_db,
            estimation_windows: training.len(),
            validation_windows: validation.len(),
        },
    ))
}

#[cfg(test)]
#[path = "modal_tests.rs"]
mod tests;

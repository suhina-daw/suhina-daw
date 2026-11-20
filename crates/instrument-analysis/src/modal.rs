// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded Fourier approximation of separated exponentially decaying resonances.
//! Preparation only. Assumptions and primary sources: instrument-modal-fitting.md.

use crate::{Error, MAX_SECONDS, RATE, sustain};
use rustfft::{FftPlanner, num_complex::Complex64};
use std::{f64::consts::TAU, ops::Range};
use suhina_dsp::{
    Resampler,
    modal::{
        model::{MAX_MODEL_MODES, ModelMode, ModelRecording},
        profile::{MAX_PROFILE_MODES, ModalProfile, ProfileMode},
        sustain::Sustain,
    },
};

/// Version 3 measures and removes vibrato first and adds the held layer.
pub const VERSION: u32 = 3;
const MIN_FRAME: usize = 1024;
const MAX_FRAME: usize = 8192;
const MAX_FRAMES: usize = 12;
const MAX_CANDIDATES: usize = 96;
/// Partials along the string (instrument-modal-fitting.md): the fundamental's distance from the
/// reference key, the inharmonicity bound, the search width and the weakest partial kept.
const FUNDAMENTAL_RATIO: f64 = 1.041_299; // 70 cents
const MAX_INHARMONICITY: f64 = 0.01;
const PARTIAL_SEARCH: f64 = 0.01;
const PARTIAL_FLOOR: f64 = 1e-4;
const PARTIAL_PROMINENCE: f64 = 2.0;
const MAX_PARTIALS: usize = 128;
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
/// A mode whose fitted level changes by at most this across its span, without clearly
/// decaying, is sustained.
const SUSTAIN_DB: f64 = 3.0;
/// T60 stored for a sustained mode: the profile's maximum; note-off releases it.
const SUSTAINED_SECONDS: f64 = 60.0;
/// A failing span this long is fitted once more on its last two thirds.
const LATE_RETRY_WINDOWS: usize = 9;
/// A partial that fails every fit is kept loose with at least this many windows, this fall over
/// them and at most this RMS envelope error (see instrument-modal-fitting.md › Loose partials).
const LOOSE_MIN_WINDOWS: usize = 6;
const LOOSE_DROP_DB: f64 = 6.0;
const LOOSE_MAX_DB: f64 = 15.0;
/// Each third of a loose span's windows is on average at least this much below the one before:
/// a burst followed by a flat floor is not a decay.
const LOOSE_THIRD_DB: f64 = 1.0;
/// A fast part is kept when it lowers the span's RMS envelope error by this much, or leaves at
/// most this fraction of a single exponential's error of at least this much.
const TWO_STAGE_GAIN_DB: f64 = 1.0;
const TWO_STAGE_RATIO: f64 = 1.0 / 3.0;
const TWO_STAGE_MIN_SINGLE_DB: f64 = 0.25;
/// Fine envelope for beating: 1024-sample windows every 256 samples.
const BEAT_FRAME: usize = 1024;
const BEAT_HOP: usize = 256;
const MIN_BEAT_SECONDS: f64 = 0.1;
const MAX_BEAT_SECONDS: f64 = 5.0;
const MIN_BEAT_CORRELATION: f64 = 0.5;
const MIN_BEAT_RIPPLE_DB: f64 = 1.0;
/// Attack: the first 0.25 s, kept when it holds at least 0.1 % of the signal's energy there.
const ATTACK_SECONDS: f64 = 0.25;
const MIN_ATTACK_ENERGY: f64 = 1e-3;
/// Velocity a fitted recording stands for until the person says otherwise.
pub const DEFAULT_VELOCITY: f32 = 0.8;

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
    /// The level did not fall: a bowed, blown or held partial stored with the longest T60.
    pub sustained: bool,
    /// T60 of a faster first part, and its share of the magnitude; 0 for none.
    pub fast_decay_seconds: f64,
    pub fast_share: f64,
    /// Rate of a beat in the envelope and the beating partner's share; 0 for none.
    pub beat_hz: f64,
    pub beat_share: f64,
    /// Radians at the strike.
    pub phase: f64,
    /// Kept although its envelope failed the error limits: in the recording, not the profile.
    pub loose: bool,
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
    /// Up to 32 modes with phases, two-part decays, partners and the attack, for a revision-4
    /// set. `modes` lists the profile's evidence; `recording_modes` this recording's.
    pub recording: Option<ModelRecording>,
    pub recording_modes: Vec<ModeEvidence>,
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
    let total = 2 * length.div_ceil(CHUNK) + MAX_CANDIDATES * (REFINEMENTS + MAX_FRAMES + 4) + MAX_MODEL_MODES + 8;
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
        recording: None,
        recording_modes: Vec::new(),
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
    // Output frame j is the source at time j / RATE (the resampler's latency is only in when
    // frames come out), so time zero, every measured phase and the attack start at the
    // source's first analyzed frame.
    mono.truncate((length as f64 * f64::from(RATE) / f64::from(sample_rate)).floor() as usize);
    // Long silent tails must not turn a quick strike into widely separated empty windows.
    // Use the residual's AC span, rather than a DC offset introduced by mean subtraction.
    let tail_mean = mono.iter().rev().take(MIN_FRAME).sum::<f64>() / mono.len().clamp(1, MIN_FRAME) as f64;
    let mono_peak = mono.iter().map(|x| (x - tail_mean).abs()).fold(0.0_f64, f64::max);
    let useful = mono.iter().rposition(|x| (x - tail_mean).abs() > mono_peak * 1e-5).map_or(0, |i| i + 1);
    mono.truncate(useful);
    // Vibrato smears every partial: measure the pitch and resample it flat before fitting.
    work.step()?;
    let pitch = sustain::pitch(&mono);
    let vibrato = pitch.as_ref().and_then(sustain::vibrato);
    if let (Some(pitch), Some(vibrato)) = (&pitch, &vibrato) {
        mono = sustain::flatten(&mono, pitch, vibrato);
    }
    work.step()?;
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
    let bias = |slope: f64| {
        window
            .iter()
            .enumerate()
            .map(|(i, w)| w * (slope * (i as f64 - frame_size as f64 / 2.0) / f64::from(RATE)).exp())
            .sum::<f64>()
            / window.iter().sum::<f64>()
    };
    // The prominent peaks first: the string's other partials are predicted from them.
    let mut frequencies = Vec::with_capacity(MAX_CANDIDATES);
    for bin in candidates {
        let hz = refine(
            detection,
            &detection_window,
            (bin as f64 - 0.75) * bin_hz,
            (bin as f64 + 0.75) * bin_hz,
            &mut work,
        )?;
        if (low_hz..=high_hz).contains(&hz) {
            frequencies.push(hz);
        }
    }
    let string = Spectrum { values: &spectrum, bin_hz, highest, separation_hz: separation_bins.max(3.0) * bin_hz };
    for bin in string_partials(&frequencies, reference_hz, high_hz, &string, MAX_CANDIDATES) {
        let hz = refine(
            detection,
            &detection_window,
            (bin as f64 - 0.75) * bin_hz,
            (bin as f64 + 0.75) * bin_hz,
            &mut work,
        )?;
        if (low_hz..=high_hz).contains(&hz) {
            frequencies.push(hz);
        }
    }
    let mut accepted: Vec<Fitted> = Vec::with_capacity(MAX_CANDIDATES);
    for hz in frequencies {
        let mut measurements = Vec::with_capacity(frames);
        let mut thetas = Vec::with_capacity(frames);
        for index in 0..frames {
            work.step()?;
            let start = index * frame_size;
            let frame = mono.get(start..start + frame_size).ok_or(Error::InvalidRange)?;
            let (amplitude, theta) = projection_complex(frame, &window, hz);
            thetas.push(theta);
            if amplitude > 1e-7 {
                measurements.push((index, (start + frame_size / 2) as f64 / f64::from(RATE), amplitude.ln()));
            }
        }
        let Some(estimate) = estimate(hz, &measurements) else { continue };
        let slow = estimate.intercept.exp() / bias(estimate.slope);
        let fast = estimate.fast.map_or(0.0, |(intercept, slope)| intercept.exp() / bias(slope));
        // Phase at the strike, from the first window of the mode's span.
        let theta = thetas.get(estimate.first_window).copied().unwrap_or(0.0);
        let phase = wrap(theta - TAU * hz * (estimate.first_window * frame_size) as f64 / f64::from(RATE));
        if slow.is_finite() && slow > 0.0 && fast.is_finite() {
            let mut evidence = estimate.evidence;
            evidence.phase = phase;
            if let Some((_, fast_slope)) = estimate.fast {
                evidence.fast_decay_seconds = -1000.0_f64.ln() / fast_slope;
                evidence.fast_share = fast / (slow + fast);
            }
            accepted.push(Fitted { hz, slow, fast, evidence });
        }
    }
    // Schema-1 profile: the 16 largest slow magnitudes of validated modes, as before.
    let mut largest: Vec<&Fitted> = accepted.iter().filter(|f| !f.evidence.loose).collect();
    largest.sort_by(|a, b| b.slow.total_cmp(&a.slow).then_with(|| a.hz.total_cmp(&b.hz)));
    largest.truncate(MAX_PROFILE_MODES);
    largest.sort_by(|a, b| a.hz.total_cmp(&b.hz));
    let sum = largest.iter().map(|f| f.slow).sum::<f64>().max(1.0);
    let modes: Vec<ProfileMode> = largest
        .iter()
        .map(|f| ProfileMode {
            ratio: f.hz / reference_hz,
            magnitude: f.slow / sum,
            decay_seconds: f.evidence.decay_seconds,
        })
        .collect();
    if !modes.is_empty() {
        report.profile =
            Some(ModalProfile::new(reference_key, reference_hz, &modes).map_err(|_| Error::InvalidSettings)?);
        report.modes = largest.iter().map(|f| f.evidence).collect();
        report.outcome = Outcome::Fitted;
        // Revision-4 recording: the 32 most energetic modes with beats and the attack.
        accepted.sort_by(|a, b| b.energy().total_cmp(&a.energy()).then_with(|| a.hz.total_cmp(&b.hz)));
        accepted.truncate(MAX_MODEL_MODES);
        accepted.sort_by(|a, b| a.hz.total_cmp(&b.hz));
        let beat_window = hann(BEAT_FRAME);
        for fitted in &mut accepted {
            work.step()?;
            if let Some((rate, share)) = beat(&mono, &beat_window, fitted) {
                fitted.evidence.beat_hz = rate;
                fitted.evidence.beat_share = share;
            }
        }
        work.step()?;
        let sum: f64 = accepted.iter().map(Fitted::magnitude).sum();
        let held_sum: f64 = accepted.iter().filter(|f| f.evidence.sustained).map(Fitted::magnitude).sum();
        let held = sum > 0.0 && held_sum >= 0.5 * sum;
        let layer = if held || vibrato.is_some() {
            let inputs: Vec<sustain::ModeInput> = accepted
                .iter()
                .map(|f| sustain::ModeInput {
                    hz: f.hz,
                    partner_hz: (f.evidence.beat_hz > 0.0).then_some(f.hz + f.evidence.beat_hz),
                    magnitude: f.magnitude(),
                    sustained: f.evidence.sustained,
                })
                .collect();
            sustain::layer(&mono, &inputs, pitch.as_ref(), vibrato.as_ref(), held, sum)
        } else {
            None
        };
        work.step()?;
        report.recording = recording(&accepted, &mono, reference_key, reference_hz, scale, layer);
        report.recording_modes = accepted.iter().map(|f| f.evidence).collect();
    }
    work.finish()?;
    Ok(report)
}

/// The detection spectrum the string's partials are looked for in.
struct Spectrum<'a> {
    values: &'a [f64],
    bin_hz: f64,
    highest: f64,
    /// Closest two candidates may be, in Hz.
    separation_hz: f64,
}

/// Detection bins of the string's partials that prominence missed: from the fundamental nearest
/// `reference_hz` and an inharmonicity fitted to the `found` frequencies, each predicted partial
/// below `high_hz` with a peak near it and no candidate there yet, until `total` candidates.
fn string_partials(
    found: &[f64],
    reference_hz: f64,
    high_hz: f64,
    spectrum: &Spectrum<'_>,
    total: usize,
) -> Vec<usize> {
    let Some(f0) = found
        .iter()
        .copied()
        .filter(|f| (1.0 / FUNDAMENTAL_RATIO..=FUNDAMENTAL_RATIO).contains(&(f / reference_hz)))
        .min_by(|a, b| (a / reference_hz).ln().abs().total_cmp(&(b / reference_hz).ln().abs()))
    else {
        return Vec::new();
    };
    // B through the origin: (f / (k f0))^2 - 1 = B k^2.
    let (mut sum_xy, mut sum_xx, mut points) = (0.0, 0.0, 0);
    for f in found {
        let ratio = f / f0;
        let k = ratio.round();
        if (2.0..=32.0).contains(&k) && (ratio / k - 1.0).abs() < 0.05 {
            let k2 = k * k;
            sum_xy += ((ratio / k).powi(2) - 1.0) * k2;
            sum_xx += k2 * k2;
            points += 1;
        }
    }
    let b = if points >= 3 && sum_xx > 0.0 { (sum_xy / sum_xx).clamp(0.0, MAX_INHARMONICITY) } else { 0.0 };
    let values = spectrum.values;
    let mut taken: Vec<f64> = found.to_vec();
    let mut bins = Vec::new();
    for k in 1..=MAX_PARTIALS {
        if taken.len() >= total {
            break;
        }
        let k = k as f64;
        let hz = k * f0 * (1.0 + b * k * k).sqrt();
        if !hz.is_finite() || hz > high_hz {
            break;
        }
        let near = spectrum.separation_hz.max(PARTIAL_SEARCH * hz);
        if taken.iter().any(|f| (f - hz).abs() < near) {
            continue;
        }
        let low = (hz * (1.0 - PARTIAL_SEARCH) / spectrum.bin_hz).floor().max(2.0) as usize;
        let high =
            ((hz * (1.0 + PARTIAL_SEARCH) / spectrum.bin_hz).ceil() as usize).min(values.len().saturating_sub(10));
        let Some((bin, value)) =
            (low..=high).filter_map(|b| values.get(b).map(|v| (b, *v))).max_by(|a, b| a.1.total_cmp(&b.1))
        else {
            continue;
        };
        let (Some(&before), Some(&after)) = (values.get(bin - 1), values.get(bin + 1)) else { continue };
        if value < spectrum.highest * PARTIAL_FLOOR || value <= before || value <= after {
            continue;
        }
        let mut nearby = [0.0_f64; 12];
        for (slot, delta) in nearby.iter_mut().zip([-8_isize, -7, -6, -5, -4, -3, 3, 4, 5, 6, 7, 8]) {
            *slot = bin.checked_add_signed(delta).and_then(|b| values.get(b)).copied().unwrap_or(value);
        }
        nearby.sort_by(f64::total_cmp);
        if value < nearby[6] * PARTIAL_PROMINENCE {
            continue;
        }
        taken.push(bin as f64 * spectrum.bin_hz);
        bins.push(bin);
    }
    bins
}

/// One accepted mode: frequency, slow and fast magnitudes at the strike (prepared units).
struct Fitted {
    hz: f64,
    slow: f64,
    fast: f64,
    evidence: ModeEvidence,
}

impl Fitted {
    fn magnitude(&self) -> f64 {
        self.slow + self.fast
    }

    fn energy(&self) -> f64 {
        self.magnitude().powi(2) * self.evidence.decay_seconds
    }

    /// The parts this mode renders: (frequency, T60, amplitude) at the strike.
    fn parts(&self) -> [(f64, f64, f64); 3] {
        let e = &self.evidence;
        let partner = if e.beat_hz > 0.0 { e.beat_share } else { 0.0 };
        [
            (self.hz, e.decay_seconds, self.slow * (1.0 - partner)),
            (self.hz, e.fast_decay_seconds, self.fast),
            (self.hz + e.beat_hz, e.decay_seconds, self.slow * partner),
        ]
    }
}

fn wrap(phase: f64) -> f64 {
    let wrapped = (phase + std::f64::consts::PI).rem_euclid(TAU) - std::f64::consts::PI;
    wrapped.clamp(-std::f64::consts::PI, std::f64::consts::PI)
}

/// A beat in the mode's fine envelope after its fitted decay: (rate Hz, partner share).
fn beat(mono: &[f64], window: &[f64], fitted: &Fitted) -> Option<(f64, f64)> {
    let e = &fitted.evidence;
    let ln1000 = 1000.0_f64.ln();
    let model = |t: f64| {
        let slow = fitted.slow * (-ln1000 * t / e.decay_seconds).exp();
        let fast =
            if e.fast_decay_seconds > 0.0 { fitted.fast * (-ln1000 * t / e.fast_decay_seconds).exp() } else { 0.0 };
        slow + fast
    };
    let mut residual = Vec::with_capacity(mono.len() / BEAT_HOP + 1);
    let mut start = 0;
    while let Some(frame) = mono.get(start..start + BEAT_FRAME) {
        let t = (start + BEAT_FRAME / 2) as f64 / f64::from(RATE);
        let level = model(t);
        // Only while the model is within 40 dB of the strike: later, noise and leakage rule.
        if level < fitted.magnitude() * 0.01 {
            break;
        }
        let measured = projection(frame, window, fitted.hz);
        if measured > 1e-9 {
            residual.push((measured / level).ln() * DB);
        }
        start += BEAT_HOP;
    }
    let n = residual.len();
    if n < 32 {
        return None;
    }
    // Remove the best straight line too: a slightly wrong T60 leaves a drift, not a beat.
    let points: Vec<(f64, f64)> = residual.iter().enumerate().map(|(i, r)| (i as f64, *r)).collect();
    let (intercept, slope) = line(&points)?;
    for (i, r) in residual.iter_mut().enumerate() {
        *r -= intercept + slope * i as f64;
    }
    let power = residual.iter().map(|r| r * r).sum::<f64>() / n as f64;
    let ripple = power.sqrt();
    if ripple < MIN_BEAT_RIPPLE_DB {
        return None;
    }
    let hop_seconds = BEAT_HOP as f64 / f64::from(RATE);
    let first = (MIN_BEAT_SECONDS / hop_seconds).ceil() as usize;
    let last = ((MAX_BEAT_SECONDS / hop_seconds) as usize).min(n / 2);
    if first >= last {
        return None;
    }
    let correlation = |lag: usize| {
        let sum: f64 = residual.iter().zip(residual.iter().skip(lag)).map(|(a, b)| a * b).sum();
        sum / (n - lag) as f64 / power
    };
    // The strongest peak that rises at least 0.3 above the valley before it: a smooth misfit
    // decorrelates monotonically and has none.
    let mut valley = f64::INFINITY;
    let mut best: Option<(usize, f64)> = None;
    let mut previous = correlation(first);
    for lag in first + 1..last {
        let value = correlation(lag);
        let next = correlation(lag + 1);
        valley = valley.min(previous);
        if value >= previous
            && value >= next
            && value >= MIN_BEAT_CORRELATION
            && value - valley >= 0.3
            && best.is_none_or(|(_, b)| value > b)
        {
            best = Some((lag, value));
        }
        previous = value;
    }
    let (lag, _) = best?;
    let rate = 1.0 / (lag as f64 * hop_seconds);
    if rate > 0.03 * fitted.hz {
        return None;
    }
    let q = 10.0_f64.powf(2.0 * std::f64::consts::SQRT_2 * ripple / 20.0);
    Some((rate, ((q - 1.0) / (2.0 * q)).clamp(0.0, 0.5)))
}

/// The revision-4 recording: normalized modes, level in source units and the attack residual.
fn recording(
    accepted: &[Fitted],
    mono: &[f64],
    key: u8,
    reference_hz: f64,
    scale: f64,
    layer: Option<Sustain>,
) -> Option<ModelRecording> {
    let sum: f64 = accepted.iter().map(Fitted::magnitude).sum();
    if !(sum.is_finite() && sum > 0.0) {
        return None;
    }
    let length = ((ATTACK_SECONDS * f64::from(RATE)) as usize).min(mono.len());
    let ln1000 = 1000.0_f64.ln();
    let mut attack = Vec::with_capacity(length);
    let (mut residual_energy, mut signal_energy) = (0.0, 0.0);
    let half = length / 2;
    // A held mode swells in by its onset and rise: the attack holds what the modes miss, not the
    // opposite of their swell.
    let onset = |index: usize, t: f64| {
        let Some(layer) = &layer else { return 1.0 };
        let at = |a: &[f32; MAX_MODEL_MODES]| f64::from(a.get(index).copied().unwrap_or(0.0));
        let (start, rise) = (at(&layer.onset), at(&layer.rise));
        if t < start {
            0.0
        } else if rise > 0.0 {
            1.0 - (-(t - start) * std::f64::consts::LN_10 / rise).exp()
        } else {
            1.0
        }
    };
    for (n, x) in mono.iter().take(length).enumerate() {
        let t = n as f64 / f64::from(RATE);
        let mut model = 0.0;
        for (index, fitted) in accepted.iter().enumerate() {
            let envelope = onset(index, t);
            for (hz, t60, amplitude) in fitted.parts() {
                if amplitude > 0.0 && t60 > 0.0 {
                    model +=
                        envelope * amplitude * (-ln1000 * t / t60).exp() * (TAU * hz * t + fitted.evidence.phase).sin();
                }
            }
        }
        let fade = if n < half {
            1.0
        } else {
            0.5 * (1.0 + (std::f64::consts::PI * (n - half) as f64 / (length - half).max(1) as f64).cos())
        };
        let residual = (x - model) * fade;
        residual_energy += residual * residual;
        signal_energy += x * x;
        attack.push((residual / sum) as f32);
    }
    if residual_energy < MIN_ATTACK_ENERGY * signal_energy {
        attack.clear();
    }
    let modes: Vec<ModelMode> = accepted
        .iter()
        .map(|f| {
            let e = &f.evidence;
            let fast = e.fast_decay_seconds > 0.0 && e.fast_share > 0.0;
            let partner = e.beat_hz > 0.0 && e.beat_share > 0.0;
            ModelMode {
                ratio: f.hz / reference_hz,
                magnitude: f.magnitude() / sum,
                decay_seconds: e.decay_seconds,
                fast_decay_seconds: if fast { e.fast_decay_seconds } else { 0.0 },
                fast_share: if fast { e.fast_share } else { 0.0 },
                beat_ratio: if partner { 1.0 + e.beat_hz / f.hz } else { 1.0 },
                beat_share: if partner { e.beat_share } else { 0.0 },
                phase: e.phase,
            }
        })
        .collect();
    let sustain = layer.map(|mut layer| {
        layer.attack_seconds = (attack.len() as f64 / f64::from(RATE)).min(ATTACK_SECONDS) as f32;
        layer
    });
    let recording = ModelRecording {
        key,
        source: 0,
        velocity: DEFAULT_VELOCITY,
        level: sum * scale,
        modes,
        attack_rate: if attack.is_empty() { 0 } else { RATE },
        attack,
        sustain: sustain.filter(|s| !s.is_empty() && s.validate(accepted.len()).is_ok()),
    };
    recording.validate().ok().map(|()| recording)
}

fn projection(frame: &[f64], window: &[f64], hz: f64) -> f64 {
    projection_complex(frame, window, hz).0
}

/// Amplitude of a sine at `hz` in the windowed frame, and its phase at the frame's first sample:
/// for `A sin(omega n + theta)` the sums are `A sin(theta) / 2` and `A cos(theta) / 2` times the
/// window's sum.
fn projection_complex(frame: &[f64], window: &[f64], hz: f64) -> (f64, f64) {
    let (sin, cos) = (TAU * hz / f64::from(RATE)).sin_cos();
    let (mut real, mut imaginary, mut re, mut im, mut sum) = (1.0, 0.0, 0.0, 0.0, 0.0);
    for (&x, &w) in frame.iter().zip(window) {
        re += x * w * real;
        im += x * w * imaginary;
        sum += w;
        (real, imaginary) = (real * cos - imaginary * sin, real * sin + imaginary * cos);
    }
    (2.0 * re.hypot(im) / sum.max(1e-12), re.atan2(im))
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

/// The decay of one mode: its slow part, an optional faster first part and where its span starts.
struct Estimate {
    intercept: f64,
    slope: f64,
    evidence: ModeEvidence,
    /// Log-amplitude intercept and slope of a faster first part.
    fast: Option<(f64, f64)>,
    /// Window index where the mode's span starts.
    first_window: usize,
}

/// Decay of one mode from its (window, time, log amplitude) measurements: fitted on its own
/// span, once more on the span's last two thirds when a long span fails, and with a faster
/// first part when that describes the span clearly better.
fn estimate(hz: f64, values: &[(usize, f64, f64)]) -> Option<Estimate> {
    let span = envelope_span(values);
    let first_window = span.first()?.0;
    let single = fit_span(hz, span);
    // The late retry describes what rings on after a fast early decay; it never makes a mode
    // sustained (beating can flatten a decaying partial's late windows).
    let late = if span.len() >= LATE_RETRY_WINDOWS {
        span.get(span.len() / 3..).and_then(|l| fit_span(hz, l)).filter(|(_, _, e)| !e.sustained)
    } else {
        None
    };
    let plain = |(intercept, slope, evidence): (f64, f64, ModeEvidence)| Estimate {
        intercept,
        slope,
        evidence,
        fast: None,
        first_window,
    };
    let two_stage = late.and_then(|(intercept, slope, evidence)| {
        let early = span.get(..span.len() / 3)?;
        let excess: Vec<(f64, f64)> = early
            .iter()
            .filter_map(|&(_, t, y)| {
                let remainder = y.exp() - (intercept + slope * t).exp();
                (remainder > 0.0).then(|| (t, remainder.ln()))
            })
            .collect();
        if excess.len() < 2 {
            return None;
        }
        let (fast_intercept, fast_slope) = line(&excess)?;
        if fast_slope >= 0.0 {
            return None;
        }
        let fast_decay = -1000.0_f64.ln() / fast_slope;
        if !(0.001..evidence.decay_seconds * 0.5).contains(&fast_decay) {
            return None;
        }
        let rms = span_rms(span, |t| ((intercept + slope * t).exp() + (fast_intercept + fast_slope * t).exp()).ln());
        Some((Estimate { intercept, slope, evidence, fast: Some((fast_intercept, fast_slope)), first_window }, rms))
    });
    match (single, two_stage) {
        (Some(one), Some((two, rms))) => {
            let single_rms = span_rms(span, |t| one.0 + one.1 * t);
            let better = rms + TWO_STAGE_GAIN_DB <= single_rms
                || (single_rms >= TWO_STAGE_MIN_SINGLE_DB && rms <= single_rms * TWO_STAGE_RATIO);
            Some(if better { two } else { plain(one) })
        }
        (Some(one), None) => Some(plain(one)),
        (None, Some((two, _))) => Some(two),
        (None, None) => late.or_else(|| loose_span(hz, values)).map(plain),
    }
}

/// A partial no fit describes, kept loose: the least-squares decay from its loudest of the first
/// three windows until the floor, ignoring rises, when it clearly falls.
fn loose_span(hz: f64, values: &[(usize, f64, f64)]) -> Option<(f64, f64, ModeEvidence)> {
    let head = values.get(..values.len().min(3))?;
    let start = head.iter().enumerate().max_by(|a, b| a.1.2.total_cmp(&b.1.2))?.0;
    let peak = values.get(start)?.2;
    let span: Vec<(f64, f64)> =
        values.get(start..)?.iter().take_while(|v| (peak - v.2) * DB <= SPAN_FLOOR_DB).map(|v| (v.1, v.2)).collect();
    if span.len() < LOOSE_MIN_WINDOWS {
        return None;
    }
    let (intercept, slope) = line(&span)?;
    let decay = -1000.0_f64.ln() / slope;
    let drop_db = -slope * (span.last()?.0 - span.first()?.0) * DB;
    let rms_db =
        (span.iter().map(|(t, y)| ((y - intercept - slope * t) * DB).powi(2)).sum::<f64>() / span.len() as f64).sqrt();
    let third = span.len() / 3;
    let mean = |part: &[(f64, f64)]| part.iter().map(|p| p.1).sum::<f64>() / part.len().max(1) as f64;
    let (first, middle, last) =
        (span.get(..third)?, span.get(third..span.len() - third)?, span.get(span.len() - third..)?);
    let falling =
        (mean(first) - mean(middle)) * DB >= LOOSE_THIRD_DB && (mean(middle) - mean(last)) * DB >= LOOSE_THIRD_DB;
    let clear = slope < 0.0
        && falling
        && (0.001..=SUSTAINED_SECONDS).contains(&decay)
        && drop_db >= LOOSE_DROP_DB
        && rms_db <= LOOSE_MAX_DB;
    clear.then_some((
        intercept,
        slope,
        ModeEvidence {
            frequency_hz: hz,
            decay_seconds: decay,
            estimation_rms_db: rms_db,
            validation_rms_db: rms_db,
            estimation_windows: span.len(),
            validation_windows: 0,
            sustained: false,
            fast_decay_seconds: 0.0,
            fast_share: 0.0,
            beat_hz: 0.0,
            beat_share: 0.0,
            phase: 0.0,
            loose: true,
        },
    ))
}

/// Least-squares intercept and slope through (time, value) points.
fn line(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    let n = points.len() as f64;
    let mean_t = points.iter().map(|p| p.0).sum::<f64>() / n;
    let mean_y = points.iter().map(|p| p.1).sum::<f64>() / n;
    let variance = points.iter().map(|p| (p.0 - mean_t).powi(2)).sum::<f64>();
    if variance <= 0.0 {
        return None;
    }
    let slope = points.iter().map(|p| (p.0 - mean_t) * (p.1 - mean_y)).sum::<f64>() / variance;
    slope.is_finite().then_some((mean_y - slope * mean_t, slope))
}

/// RMS difference in dB between a span's log amplitudes and a log-amplitude model.
fn span_rms(span: &[(usize, f64, f64)], model: impl Fn(f64) -> f64) -> f64 {
    (span.iter().map(|p| ((p.2 - model(p.1)) * DB).powi(2)).sum::<f64>() / span.len().max(1) as f64).sqrt()
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
    if !slope.is_finite() {
        return None;
    }
    let intercept = mean_y - slope * mean_t;
    let rms = |v: &[&(usize, f64, f64)]| {
        (v.iter().map(|p| ((p.2 - intercept - slope * p.1) * DB).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
    };
    let estimation_rms_db = rms(&training);
    let validation_rms_db = rms(&validation);
    if estimation_rms_db > MAX_ESTIMATION_DB || validation_rms_db > MAX_VALIDATION_DB {
        return None;
    }
    let drop_db = -slope * (training.last()?.1 - training.first()?.1) * DB;
    let decay = -1000.0_f64.ln() / slope;
    let decaying = slope < 0.0
        && (0.001..=SUSTAINED_SECONDS).contains(&decay)
        && drop_db >= MIN_DROP_DB
        && drop_db >= DROP_TO_ERROR * estimation_rms_db.max(validation_rms_db);
    // A level that stays put within the error limits is a sustained partial, not a failure.
    let (intercept, slope, decay, sustained) = if decaying {
        (intercept, slope, decay, false)
    } else if drop_db.abs() <= SUSTAIN_DB {
        let slope = -1000.0_f64.ln() / SUSTAINED_SECONDS;
        (mean_y - slope * mean_t, slope, SUSTAINED_SECONDS, true)
    } else {
        return None;
    };
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
            sustained,
            fast_decay_seconds: 0.0,
            fast_share: 0.0,
            beat_hz: 0.0,
            beat_share: 0.0,
            phase: 0.0,
            loose: false,
        },
    ))
}

#[cfg(test)]
#[path = "modal_tests.rs"]
mod tests;

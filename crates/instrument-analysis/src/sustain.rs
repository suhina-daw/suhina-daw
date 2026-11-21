// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The held layer of a bowed, blown or sung recording: its vibrato (measured and removed before
//! the modes are fitted), per-mode onsets and vibrato-coupled levels, tremolo, release, drift and
//! band noise. Preparation only, on the fitter's 22050 Hz mono signal. Assumptions and sources:
//! `docs/spec/instrument-sustained-models.md`.

use crate::RATE;
use rustfft::{FftPlanner, num_complex::Complex64};
use std::f64::consts::{PI, TAU};
use suhina_dsp::modal::{
    model::MAX_MODEL_MODES,
    sustain::{NOISE_BANDS, Sustain, noise_center_hz},
};

const TRACK_FRAME: usize = 1024;
const TRACK_HOP: usize = 256;
const SPECTRUM_FRAME: usize = 2048;
const SPECTRUM_HOP: usize = 1024;
const MAX_SPECTRA: usize = 64;
const HELD_START: f64 = 0.15;
const MIN_SECONDS: f64 = 1.0;
const MIN_TRACK_HZ: f64 = 60.0;
const MAX_TRACK_HZ: f64 = 2000.0;
const MIN_RATE: f64 = 3.0;
const MAX_RATE: f64 = 10.0;
const MIN_PERIODICITY: f64 = 0.4;
const MIN_AGREEMENT: f64 = 0.7;
const MIN_CENTS: f64 = 5.0;
/// A lone tracked partial whose level ripples more than this (RMS dB) is beating.
const MAX_SINGLE_RIPPLE_DB: f64 = 1.5;
const MAX_CENTS: f64 = 200.0;
const NOISE_FRAME: usize = 4096;
const NOISE_HOP: usize = 2048;
const NOISE_START: f64 = 0.25;
const MASK_BINS: f64 = 4.5;
const DB: f64 = 20.0 / std::f64::consts::LN_10;

/// The pitch curve of a tone: cents against the tracked partials' center, per frame.
pub(crate) struct Pitch {
    /// (time, cents) per frame, gaps filled, over the held region.
    pub curve: Vec<(f64, f64)>,
}

impl Pitch {
    /// Cents at time `t`, interpolated and held at the ends.
    pub fn at(&self, t: f64) -> f64 {
        interpolate(&self.curve, t)
    }
}

/// A measured vibrato.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Vibrato {
    pub rate: f64,
    /// Corrected peak deviation in cents.
    pub cents: f64,
    pub delay: f64,
    pub fade: f64,
    /// (time, local depth relative to the full depth) per frame.
    pub depth: Vec<(f64, f64)>,
    /// How much of a deviation at this rate the tracking frames measure.
    pub response: f64,
}

impl Vibrato {
    fn depth_at(&self, t: f64) -> f64 {
        interpolate(&self.depth, t)
    }
}

pub(crate) fn interpolate(points: &[(f64, f64)], t: f64) -> f64 {
    let Some(&(first_t, first_v)) = points.first() else { return 0.0 };
    if t <= first_t {
        return first_v;
    }
    let after = points.partition_point(|p| p.0 < t);
    match (points.get(after.wrapping_sub(1)), points.get(after)) {
        (Some(a), Some(b)) if b.0 > a.0 => a.1 + (b.1 - a.1) * (t - a.0) / (b.0 - a.0),
        (Some(a), _) => a.1,
        _ => first_v,
    }
}

fn hann(size: usize) -> Vec<f64> {
    (0..size).map(|i| 0.5 - 0.5 * (TAU * i as f64 / size as f64).cos()).collect()
}

/// Four-term Blackman-Harris (Harris 1978): sidelobes 92 dB down.
fn blackman_harris(size: usize) -> Vec<f64> {
    (0..size)
        .map(|i| {
            let x = TAU * i as f64 / size as f64;
            0.35875 - 0.48829 * x.cos() + 0.14128 * (2.0 * x).cos() - 0.01168 * (3.0 * x).cos()
        })
        .collect()
}

/// Amplitude and phase (at the frame's first sample) of a sine at `hz`.
pub(crate) fn projection(frame: &[f64], window: &[f64], hz: f64) -> (f64, f64) {
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

fn wrap(phase: f64) -> f64 {
    (phase + PI).rem_euclid(TAU) - PI
}

fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values.get(middle - 1).copied().unwrap_or(0.0) + values.get(middle).copied().unwrap_or(0.0)) / 2.0
    } else {
        values.get(middle).copied().unwrap_or(0.0)
    }
}

/// Least-squares line (intercept, slope) through points.
fn line(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    let n = points.len() as f64;
    if points.len() < 2 {
        return None;
    }
    let mean_t = points.iter().map(|p| p.0).sum::<f64>() / n;
    let mean_y = points.iter().map(|p| p.1).sum::<f64>() / n;
    let variance = points.iter().map(|p| (p.0 - mean_t).powi(2)).sum::<f64>();
    if variance <= 0.0 {
        return None;
    }
    let slope = points.iter().map(|p| (p.0 - mean_t) * (p.1 - mean_y)).sum::<f64>() / variance;
    slope.is_finite().then_some((mean_y - slope * mean_t, slope))
}

/// The values with their least-squares line removed.
fn detrend(points: &[(f64, f64)]) -> Vec<f64> {
    match line(points) {
        Some((intercept, slope)) => points.iter().map(|p| p.1 - intercept - slope * p.0).collect(),
        None => points.iter().map(|p| p.1).collect(),
    }
}

/// Normalized autocorrelation at `lag`.
fn correlation(values: &[f64], power: f64, lag: usize) -> f64 {
    let n = values.len();
    if lag >= n || power <= 0.0 {
        return 0.0;
    }
    let sum: f64 = values.iter().zip(values.iter().skip(lag)).map(|(a, b)| a * b).sum();
    sum / (n - lag) as f64 / power
}

/// The period (in frames, refined) of the strongest autocorrelation peak for rates 3..10 Hz,
/// at least `MIN_PERIODICITY`.
fn period(values: &[f64], frame_rate: f64) -> Option<f64> {
    let n = values.len();
    let power = values.iter().map(|v| v * v).sum::<f64>() / n.max(1) as f64;
    let first = (frame_rate / MAX_RATE).floor().max(2.0) as usize;
    let last = ((frame_rate / MIN_RATE).ceil() as usize).min(n / 2);
    if power <= 0.0 || first + 2 > last {
        return None;
    }
    let mut best: Option<(usize, f64)> = None;
    for lag in first..last {
        let (before, value, after) =
            (correlation(values, power, lag - 1), correlation(values, power, lag), correlation(values, power, lag + 1));
        if value >= before && value >= after && value >= MIN_PERIODICITY && best.is_none_or(|(_, b)| value > b) {
            best = Some((lag, value));
        }
    }
    let (lag, value) = best?;
    let before = correlation(values, power, lag - 1);
    let after = correlation(values, power, lag + 1);
    let curvature = before - 2.0 * value + after;
    let offset = if curvature < 0.0 { (0.5 * (before - after) / curvature).clamp(-0.5, 0.5) } else { 0.0 };
    let refined = lag as f64 + offset;
    let rate = frame_rate / refined;
    (MIN_RATE..=MAX_RATE).contains(&rate).then_some(refined)
}

/// Mean power spectrum peaks of the held region: up to two partials to track (Hz, power).
fn partials(mono: &[f64]) -> Vec<(f64, f64)> {
    let start = (HELD_START * f64::from(RATE)) as usize;
    let window = hann(SPECTRUM_FRAME);
    let fft = FftPlanner::<f64>::new().plan_fft_forward(SPECTRUM_FRAME);
    let mut scratch = vec![Complex64::default(); fft.get_inplace_scratch_len()];
    let mut power = vec![0.0; SPECTRUM_FRAME / 2];
    let mut frames = 0;
    let mut at = start;
    while let Some(frame) = mono.get(at..at + SPECTRUM_FRAME) {
        if frames == MAX_SPECTRA {
            break;
        }
        let mut bins: Vec<Complex64> = frame.iter().zip(&window).map(|(x, w)| Complex64::new(x * w, 0.0)).collect();
        fft.process_with_scratch(&mut bins, &mut scratch);
        for (slot, bin) in power.iter_mut().zip(&bins) {
            *slot += bin.norm_sqr();
        }
        frames += 1;
        at += SPECTRUM_HOP;
    }
    if frames == 0 {
        return Vec::new();
    }
    let bin_hz = f64::from(RATE) / SPECTRUM_FRAME as f64;
    let mut peaks = Vec::new();
    for bin in 1..power.len().saturating_sub(1) {
        let hz = bin as f64 * bin_hz;
        let (Some(&a), Some(&b), Some(&c)) = (power.get(bin - 1), power.get(bin), power.get(bin + 1)) else {
            continue;
        };
        if !(MIN_TRACK_HZ..=MAX_TRACK_HZ).contains(&hz) || b <= a || b < c || b <= 0.0 {
            continue;
        }
        let (la, lb, lc) = (a.max(1e-300).ln(), b.ln(), c.max(1e-300).ln());
        let curvature = la - 2.0 * lb + lc;
        let offset = if curvature < 0.0 { (0.5 * (la - lc) / curvature).clamp(-0.5, 0.5) } else { 0.0 };
        peaks.push(((bin as f64 + offset) * bin_hz, b));
    }
    peaks.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.total_cmp(&y.0)));
    let Some(&first) = peaks.first() else { return Vec::new() };
    let mut chosen = vec![first];
    if let Some(&second) = peaks.iter().find(|p| (p.0 / first.0 - 1.0).abs() > 0.15 && p.1 >= first.1 * 0.01) {
        chosen.push(second);
    }
    chosen
}

/// (time, amplitude, cents) of a partial near `hz`, from the phase advance between frames.
fn track(mono: &[f64], window: &[f64], hz: f64) -> Vec<(f64, f64, f64)> {
    let advance = TAU * hz * TRACK_HOP as f64 / f64::from(RATE);
    let mut out = Vec::with_capacity(mono.len() / TRACK_HOP + 1);
    let mut previous: Option<(f64, f64)> = None;
    let mut at = 0;
    while let Some(frame) = mono.get(at..at + TRACK_FRAME) {
        let (amplitude, phase) = projection(frame, window, hz);
        if let Some((last_amplitude, last_phase)) = previous {
            let delta = wrap(phase - last_phase - advance);
            let frequency = hz + delta * f64::from(RATE) / (TAU * TRACK_HOP as f64);
            let t = (at as f64 - TRACK_HOP as f64 / 2.0 + TRACK_FRAME as f64 / 2.0) / f64::from(RATE);
            let cents = if frequency > 0.0 { 1200.0 * (frequency / hz).log2() } else { f64::NAN };
            out.push((t, (amplitude + last_amplitude) / 2.0, cents));
        }
        previous = Some((amplitude, phase));
        at += TRACK_HOP;
    }
    out
}

/// One partial's cent curve over the held region, with gaps (amplitude under 10 % of its median)
/// filled linearly; None with more than 10 % gaps.
/// A partial's cent curve over the held region, its median amplitude and its level's RMS ripple
/// in dB.
type HeldCurve = (Vec<(f64, f64)>, f64, f64);

fn held_curve(track: &[(f64, f64, f64)]) -> Option<HeldCurve> {
    let held: Vec<&(f64, f64, f64)> = track.iter().filter(|p| p.0 >= HELD_START).collect();
    let mut amplitudes: Vec<f64> = held.iter().map(|p| p.1).collect();
    let level = median(&mut amplitudes);
    if level <= 0.0 {
        return None;
    }
    // The held region ends at the last frame above the floor (a release or the trim's end).
    let last = held.iter().rposition(|p| p.1 >= 0.1 * level)?;
    let held = held.get(..=last)?;
    let good = |p: &(f64, f64, f64)| p.1 >= 0.1 * level && p.2.is_finite();
    let gaps = held.iter().filter(|p| !good(p)).count();
    if held.len() < 8 || gaps * 10 > held.len() {
        return None;
    }
    let known: Vec<(f64, f64)> = held.iter().filter(|p| good(p)).map(|p| (p.0, p.2)).collect();
    let curve = held.iter().map(|p| (p.0, if good(p) { p.2 } else { interpolate(&known, p.0) })).collect();
    // RMS ripple of the level in dB: beating moves the level as much as the pitch.
    let levels: Vec<(f64, f64)> = held.iter().filter(|p| good(p)).map(|p| (p.0, p.1.ln() * DB)).collect();
    Some((curve, level, rms(&detrend(&levels))))
}

fn pearson(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    if n < 2 {
        return 0.0;
    }
    let (ma, mb) = (a.iter().take(n).sum::<f64>() / n as f64, b.iter().take(n).sum::<f64>() / n as f64);
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b).take(n) {
        ab += (x - ma) * (y - mb);
        aa += (x - ma).powi(2);
        bb += (y - mb).powi(2);
    }
    if aa <= 0.0 || bb <= 0.0 { 0.0 } else { ab / (aa * bb).sqrt() }
}

fn rms(values: &[f64]) -> f64 {
    (values.iter().map(|v| v * v).sum::<f64>() / values.len().max(1) as f64).sqrt()
}

/// The tone's pitch curve (steps 1-4): None for a short recording, no partial, too many gaps,
/// or two partials that do not move together (beating, not vibrato).
pub(crate) fn pitch(mono: &[f64]) -> Option<Pitch> {
    if (mono.len() as f64) < MIN_SECONDS * f64::from(RATE) {
        return None;
    }
    let window = hann(TRACK_FRAME);
    let mut curves = Vec::with_capacity(2);
    for (hz, _) in partials(mono) {
        if let Some(curve) = held_curve(&track(mono, &window, hz)) {
            curves.push(curve);
        }
    }
    let ((first, first_level, first_ripple), second) = match curves.as_slice() {
        [one] => (one.clone(), None),
        [one, two, ..] => (one.clone(), Some(two.clone())),
        [] => return None,
    };
    let Some((second, second_level, _)) = second else {
        // One partial cannot be compared with another: a beating pair (its level dipping as
        // its frequency wobbles) is told from vibrato by the level's ripple.
        return (first_ripple <= MAX_SINGLE_RIPPLE_DB).then_some(Pitch { curve: first });
    };
    // Compare over the frames both cover.
    let n = first.len().min(second.len());
    let (a, b) = (detrend(first.get(..n)?), detrend(second.get(..n)?));
    let (ra, rb) = (rms(&a), rms(&b));
    let agree = pearson(&a, &b) >= MIN_AGREEMENT && ra <= 1.5 * rb && rb <= 1.5 * ra;
    // Two steady partials have no common motion to compare: a negligible wander is agreement.
    let steady = ra.max(rb) < 1.0;
    if !(agree || steady) {
        return None;
    }
    let total = first_level + second_level;
    let curve =
        first.iter().zip(&second).map(|(p, q)| (p.0, (p.1 * first_level + q.1 * second_level) / total)).collect();
    Some(Pitch { curve })
}

/// Response of the tracking to a vibrato at `rate`: the 1024-sample Hann frame,
/// `sinc(x) / (1 - x^2)` with `x = rate 1024 / 22050`, times the 256-sample phase difference,
/// `sinc(rate 256 / 22050)`.
fn response(rate: f64) -> f64 {
    let sinc = |x: f64| if x.abs() < 1e-12 { 1.0 } else { (PI * x).sin() / (PI * x) };
    let x = rate * TRACK_FRAME as f64 / f64::from(RATE);
    (sinc(x) / (1.0 - x * x) * sinc(rate * TRACK_HOP as f64 / f64::from(RATE))).clamp(0.1, 1.0)
}

fn frame_rate() -> f64 {
    f64::from(RATE) / TRACK_HOP as f64
}

/// Vibrato of a pitch curve (steps 5-6), or None.
pub(crate) fn vibrato(pitch: &Pitch) -> Option<Vibrato> {
    let v = detrend(&pitch.curve);
    let lag = period(&v, frame_rate())?;
    let rate = frame_rate() / lag;
    let span = pitch.curve.last()?.0 - pitch.curve.first()?.0;
    if span * rate < 2.5 {
        return None;
    }
    let p = lag.round().max(2.0) as usize;
    let mut depths = Vec::with_capacity(v.len());
    for (center, &(t, _)) in pitch.curve.iter().enumerate() {
        let Some(begin) = center.checked_sub(p / 2) else { continue };
        let (Some(values), Some(times)) = (v.get(begin..begin + p), pitch.curve.get(begin..begin + p)) else {
            continue;
        };
        let (mut s, mut c) = (0.0, 0.0);
        for (value, (time, _)) in values.iter().zip(times) {
            let (sin, cos) = (TAU * rate * time).sin_cos();
            s += value * sin;
            c += value * cos;
        }
        depths.push((t, 2.0 * s.hypot(c) / p as f64));
    }
    let tail = depths.len() * 2 / 5;
    let mut late: Vec<f64> = depths.get(tail..)?.iter().map(|d| d.1).collect();
    let raw = median(&mut late);
    let response = response(rate);
    let cents = raw / response;
    if !(MIN_CENTS..=MAX_CENTS).contains(&cents) || raw <= 0.0 {
        return None;
    }
    let crossing = |share: f64| depths.iter().position(|d| d.1 >= share * raw).and_then(|i| depths.get(i));
    let first = depths.first()?;
    let (t10, t90) = (crossing(0.1)?, crossing(0.9)?);
    // Reached already at the first measured frame: no delay (or fade) can be seen before it.
    // The one-period window smooths the ramp by about a period: removed in quadrature.
    let (delay, fade) = if t90.0 <= first.0 {
        (0.0, 0.0)
    } else {
        let width = ((t90.0 - t10.0) / 0.8).max(0.0);
        let fade = (width * width - 1.0 / (rate * rate)).max(0.0).sqrt();
        let delay = if t10.0 <= first.0 { 0.0 } else { ((t10.0 + t90.0) / 2.0 - fade / 2.0).max(0.0) };
        (delay.min(4.0), fade.min(4.0))
    };
    let depth = depths.iter().map(|&(t, d)| (t, d / raw)).collect();
    Some(Vibrato { rate, cents, delay, fade, depth, response })
}

/// Drift (step 7): RMS cents and bandwidth Hz of the pitch with the vibrato averaged out, or
/// None under 1 cent.
pub(crate) fn drift(pitch: &Pitch, vibrato: Option<&Vibrato>) -> Option<(f64, f64)> {
    let frames = match vibrato {
        Some(v) => (frame_rate() / v.rate).round(),
        None => (0.1 * frame_rate()).round(),
    }
    .max(1.0) as usize;
    let smoothed: Vec<(f64, f64)> = pitch
        .curve
        .windows(frames)
        .map(|w| {
            let t = w.iter().map(|p| p.0).sum::<f64>() / frames as f64;
            (t, w.iter().map(|p| p.1).sum::<f64>() / frames as f64)
        })
        .collect();
    let s = detrend(&smoothed);
    if s.len() < 8 {
        return None;
    }
    let cents = rms(&s);
    if cents < 1.0 {
        return None;
    }
    let power = cents * cents;
    let half = (1..s.len() / 2).find(|&lag| correlation(&s, power, lag) <= 0.5).unwrap_or(s.len() / 2);
    let bandwidth = 2.0_f64.ln() / (TAU * half as f64 / frame_rate());
    Some((cents.min(20.0), bandwidth.clamp(0.5, 10.0)))
}

/// Step 8: the signal resampled along the pitch curve, so every partial keeps one frequency.
pub(crate) fn flatten(mono: &[f64], pitch: &Pitch, vibrato: &Vibrato) -> Vec<f64> {
    // The measured curve is the true one seen through the tracking frames.
    flatten_by(mono, |t| pitch.at(t) / vibrato.response)
}

/// The signal resampled so that a pitch `cents(t)` away from its note at input time `t` (seconds)
/// is moved onto the note: every partial keeps one frequency.
pub(crate) fn flatten_by(mono: &[f64], cents: impl Fn(f64) -> f64) -> Vec<f64> {
    const HALF: isize = 8;
    let rate = f64::from(RATE);
    let mut out = Vec::with_capacity(mono.len() + mono.len() / 8);
    // phi(n): warped time in samples at input sample n; output m is the input where phi = m.
    let mut phi = 0.0;
    let mut m = 0.0;
    for n in 0..mono.len() {
        let ratio = 2.0_f64.powf(cents(n as f64 / rate) / 1200.0);
        let next = phi + ratio;
        while m < next {
            let t = n as f64 + (m - phi) / ratio;
            let base = t.floor();
            let fraction = t - base;
            let mut sum = 0.0;
            for j in (1 - HALF)..=HALF {
                let x = fraction - j as f64;
                let index = base as isize + j;
                let Some(&sample) = usize::try_from(index).ok().and_then(|i| mono.get(i)) else { continue };
                let sinc = if x.abs() < 1e-12 { 1.0 } else { (PI * x).sin() / (PI * x) };
                let taper = 0.5 + 0.5 * (PI * x / HALF as f64).cos();
                sum += sample * sinc * taper;
            }
            out.push(sum);
            m += 1.0;
        }
        phi = next;
    }
    out
}

/// One fitted mode, as the held layer needs it.
pub(crate) struct ModeInput {
    pub hz: f64,
    pub partner_hz: Option<f64>,
    pub magnitude: f64,
    pub sustained: bool,
}

/// Fine envelope (time, amplitude) of a partial at `hz` on 1024-sample frames every 256 samples.
fn envelope(mono: &[f64], window: &[f64], hz: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::with_capacity(mono.len() / TRACK_HOP + 1);
    let mut at = 0;
    while let Some(frame) = mono.get(at..at + TRACK_FRAME) {
        out.push(((at + TRACK_FRAME / 2) as f64 / f64::from(RATE), projection(frame, window, hz).0));
        at += TRACK_HOP;
    }
    out
}

/// The held layer of a recording (section 2 of the spec), without its attack length, which the
/// caller sets. `held` says whether sustained modes hold at least half the magnitude; `sum` is
/// the modes' magnitude sum in the signal's units (the normalization of the stored layer).
pub(crate) fn layer(
    mono: &[f64],
    modes: &[ModeInput],
    pitch: Option<&Pitch>,
    vibrato: Option<&Vibrato>,
    held: bool,
    sum: f64,
) -> Option<Sustain> {
    if !(sum.is_finite() && sum > 0.0) || modes.len() > MAX_MODEL_MODES {
        return None;
    }
    let mut layer = Sustain::NONE;
    if let Some(v) = vibrato {
        layer.vibrato_hz = v.rate as f32;
        layer.vibrato_cents = v.cents as f32;
        layer.vibrato_delay = v.delay as f32;
        layer.vibrato_fade = v.fade as f32;
    }
    if !held {
        return (!layer.is_empty()).then_some(layer);
    }
    layer.body_fixed = 1.0;
    if let Some((cents, bandwidth)) = pitch.and_then(|p| drift(p, vibrato)) {
        layer.drift_cents = cents as f32;
        layer.drift_hz = bandwidth as f32;
    }
    let window = hann(TRACK_FRAME);
    let envelopes: Vec<Vec<(f64, f64)>> =
        modes.iter().map(|m| if m.sustained { envelope(mono, &window, m.hz) } else { Vec::new() }).collect();
    let end = mono.len() as f64 / f64::from(RATE);
    // Release from the three strongest sustained modes.
    let mut strongest: Vec<usize> = (0..modes.len()).filter(|&i| modes.get(i).is_some_and(|m| m.sustained)).collect();
    strongest.sort_by(|a, b| {
        let magnitude = |i: &usize| modes.get(*i).map_or(0.0, |m| m.magnitude);
        magnitude(b).total_cmp(&magnitude(a))
    });
    strongest.truncate(3);
    let summed: Vec<(f64, f64)> = envelopes
        .iter()
        .find(|e| !e.is_empty())
        .map(|first| {
            first
                .iter()
                .enumerate()
                .map(|(k, &(t, _))| {
                    let total = strongest
                        .iter()
                        .filter_map(|&i| envelopes.get(i).and_then(|e| e.get(k)).map(|p| p.1))
                        .sum::<f64>();
                    (t, total)
                })
                .collect()
        })
        .unwrap_or_default();
    let held_from = 0.3_f64.min(end * 0.5);
    let (release, release_start) = release(&summed, held_from).unwrap_or((0.0, end));
    layer.release_seconds = release as f32;
    // Onsets, rises and the vibrato-coupled levels of the sustained modes.
    let mut onsets = Vec::new();
    let mut rises = Vec::new();
    for (index, (mode, envelope)) in modes.iter().zip(&envelopes).enumerate() {
        if !mode.sustained || envelope.is_empty() {
            continue;
        }
        let mut held_values: Vec<f64> =
            envelope.iter().filter(|p| p.0 >= held_from && p.0 <= release_start).map(|p| p.1).collect();
        let level = median(&mut held_values);
        if level <= 0.0 {
            continue;
        }
        let reach = |share: f64| envelope.iter().find(|p| p.1 >= share * level).map(|p| p.0);
        if let (Some(t10), Some(t90)) = (reach(0.1), reach(0.9))
            && t90 >= 0.04
        {
            // The curve 1 - exp(-(t - onset) / tau) through 10 % at t10 and 90 % at t90:
            // tau = (t90 - t10) / ln 9, and it starts tau ln(10/9) before t10.
            let tau = (t90 - t10).max(0.0) / 9.0_f64.ln();
            let onset = (t10 - tau * (10.0_f64 / 9.0).ln()).clamp(0.0, 1.0);
            let rise = (t90 - onset).clamp(0.0, 4.0);
            if let (Some(o), Some(r)) = (layer.onset.get_mut(index), layer.rise.get_mut(index)) {
                *o = onset as f32;
                *r = rise as f32;
            }
            onsets.push(onset);
            rises.push(rise);
        } else {
            onsets.push(0.0);
            rises.push(0.0);
        }
        if let (Some(pitch), Some(vibrato)) = (pitch, vibrato)
            && let Some((slope, curve)) = coupling(envelope, pitch, vibrato, held_from, release_start)
            && let (Some(s), Some(c)) = (layer.am_slope.get_mut(index), layer.am_curve.get_mut(index))
        {
            *s = slope as f32;
            *c = curve as f32;
        }
    }
    layer.noise_onset = median(&mut onsets).clamp(0.0, 1.0) as f32;
    layer.noise_rise = median(&mut rises).clamp(0.0, 4.0) as f32;
    if vibrato.is_none()
        && let Some((rate, depth)) = tremolo(mono, held_from, release_start)
    {
        layer.tremolo_hz = rate as f32;
        layer.tremolo_depth = depth as f32;
    }
    layer.noise = noise(mono, modes, release_start, sum);
    Some(layer)
}

/// T60 of a release at the end of `summed`, and when it starts.
fn release(summed: &[(f64, f64)], held_from: f64) -> Option<(f64, f64)> {
    // The held part ends at the last frame at least half the loudest, so a long tail does not
    // pull its median into the decay.
    let loudest = summed.iter().filter(|p| p.0 >= held_from).map(|p| p.1).fold(0.0_f64, f64::max);
    let held_end = summed.iter().rposition(|p| p.1 >= 0.5 * loudest).unwrap_or(0);
    let mut held: Vec<f64> = summed.iter().take(held_end + 1).filter(|p| p.0 >= held_from).map(|p| p.1).collect();
    let level = median(&mut held);
    if level <= 0.0 {
        return None;
    }
    let start = summed.iter().rposition(|p| p.1 >= 0.5 * level && p.0 >= held_from)?;
    let after = summed.get(start..)?;
    if after.len() < 5 || after.last()?.1 > 0.1 * level {
        return None;
    }
    let stop = after.iter().position(|p| p.1 < 0.01 * level).unwrap_or(after.len());
    let points: Vec<(f64, f64)> =
        after.get(..stop.max(2))?.iter().filter(|p| p.1 > 0.0).map(|p| (p.0, p.1.ln() * DB)).collect();
    if points.len() < 4 {
        return None;
    }
    let (_, slope) = line(&points)?;
    let t60 = -60.0 / slope;
    (slope < 0.0 && (0.01..=20.0).contains(&t60)).then_some((t60, summed.get(start)?.0))
}

/// dB per cent and per cent squared of a mode's level against the vibrato's pitch deviation.
fn coupling(envelope: &[(f64, f64)], pitch: &Pitch, vibrato: &Vibrato, from: f64, until: f64) -> Option<(f64, f64)> {
    let frames: Vec<(f64, f64)> = envelope
        .iter()
        .filter(|p| p.0 >= from && p.0 <= until && p.1 > 0.0 && vibrato.depth_at(p.0) >= 0.5)
        .map(|p| (p.0, p.1.ln() * DB))
        .collect();
    if (frames.len() as f64) < 2.0 * frame_rate() / vibrato.rate {
        return None;
    }
    let levels = detrend(&frames);
    let deviations = detrend(&frames.iter().map(|p| (p.0, pitch.at(p.0))).collect::<Vec<_>>());
    let squares: Vec<f64> = deviations.iter().map(|v| v * v).collect();
    let mean_square = squares.iter().sum::<f64>() / squares.len() as f64;
    let mean_level = levels.iter().sum::<f64>() / levels.len() as f64;
    let (mut vv, mut vw, mut ww, mut vy, mut wy) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for ((v, s), y) in deviations.iter().zip(&squares).zip(&levels) {
        let w = s - mean_square;
        let y = y - mean_level;
        vv += v * v;
        vw += v * w;
        ww += w * w;
        vy += v * y;
        wy += w * y;
    }
    let determinant = vv * ww - vw * vw;
    if determinant.abs() <= 1e-12 * (vv * ww).max(1e-300) {
        return None;
    }
    let slope = (vy * ww - wy * vw) / determinant;
    let curve = (wy * vv - vy * vw) / determinant;
    (slope.is_finite() && curve.is_finite()).then_some((slope.clamp(-0.5, 0.5), curve.clamp(-0.01, 0.01)))
}

/// Tremolo without vibrato: (rate Hz, peak depth fraction) of the total level, or None.
fn tremolo(mono: &[f64], from: f64, until: f64) -> Option<(f64, f64)> {
    let window = hann(TRACK_FRAME);
    let weight = window.iter().map(|w| w * w).sum::<f64>();
    let mut points = Vec::new();
    let mut at = 0;
    while let Some(frame) = mono.get(at..at + TRACK_FRAME) {
        let t = (at + TRACK_FRAME / 2) as f64 / f64::from(RATE);
        if t >= from && t <= until {
            let energy = frame.iter().zip(&window).map(|(x, w)| (x * w).powi(2)).sum::<f64>() / weight;
            if energy > 0.0 {
                points.push((t, 10.0 * energy.log10()));
            }
        }
        at += TRACK_HOP;
    }
    let levels = detrend(&points);
    let lag = period(&levels, frame_rate())?;
    let rate = frame_rate() / lag;
    let (mut s, mut c) = (0.0, 0.0);
    for (level, (t, _)) in levels.iter().zip(&points) {
        let (sin, cos) = (TAU * rate * t).sin_cos();
        s += level * sin;
        c += level * cos;
    }
    let amplitude = 2.0 * s.hypot(c) / levels.len().max(1) as f64;
    (amplitude >= 0.5).then(|| (rate, (10.0_f64.powf(amplitude / 20.0) - 1.0).clamp(0.0, 0.9)))
}

/// Amplitude density per 2/3-octave band of what the modes do not explain, normalized by `sum`.
fn noise(mono: &[f64], modes: &[ModeInput], until: f64, sum: f64) -> [f32; NOISE_BANDS] {
    let mut out = [0.0_f32; NOISE_BANDS];
    let rate = f64::from(RATE);
    let start = (NOISE_START * rate) as usize;
    let stop = ((until * rate) as usize).min(mono.len());
    let window = blackman_harris(NOISE_FRAME);
    let fft = FftPlanner::<f64>::new().plan_fft_forward(NOISE_FRAME);
    let mut scratch = vec![Complex64::default(); fft.get_inplace_scratch_len()];
    let mut power = vec![0.0; NOISE_FRAME / 2];
    let mut frames = 0_usize;
    let mut at = start;
    while at + NOISE_FRAME <= stop && frames < MAX_SPECTRA {
        let Some(frame) = mono.get(at..at + NOISE_FRAME) else { break };
        let mut bins: Vec<Complex64> = frame.iter().zip(&window).map(|(x, w)| Complex64::new(x * w, 0.0)).collect();
        fft.process_with_scratch(&mut bins, &mut scratch);
        for (slot, bin) in power.iter_mut().zip(&bins) {
            *slot += bin.norm_sqr();
        }
        frames += 1;
        at += NOISE_HOP;
    }
    if frames == 0 {
        return out;
    }
    for slot in &mut power {
        *slot /= frames as f64;
    }
    let bin_hz = rate / NOISE_FRAME as f64;
    let mut masked = vec![false; power.len()];
    let mut mask = |hz: f64| {
        let center = hz / bin_hz;
        let low = (center - MASK_BINS).ceil().max(0.0) as usize;
        let high = ((center + MASK_BINS).floor().max(0.0) as usize).min(masked.len().saturating_sub(1));
        for flag in masked.iter_mut().take(high + 1).skip(low) {
            *flag = true;
        }
    };
    for mode in modes {
        mask(mode.hz);
        if let Some(partner) = mode.partner_hz {
            mask(partner);
        }
    }
    // Partials the fit did not keep: local maxima 12 dB above the median of their neighbours.
    for bin in 1..power.len().saturating_sub(1) {
        let (Some(&a), Some(&b), Some(&c)) = (power.get(bin - 1), power.get(bin), power.get(bin + 1)) else {
            continue;
        };
        if b <= a || b < c {
            continue;
        }
        let mut neighbours: Vec<f64> = (bin.saturating_sub(16)..(bin + 17).min(power.len()))
            .filter(|&k| k != bin)
            .filter_map(|k| power.get(k).copied())
            .collect();
        if b >= 10.0_f64.powf(1.2) * median(&mut neighbours) {
            mask(bin as f64 * bin_hz);
        }
    }
    let window_power = window.iter().map(|w| w * w).sum::<f64>();
    let correction = 1.0 - 1.0 / (3.0 * frames as f64);
    let edge = 2.0_f64.powf(1.0 / 3.0);
    for (band, slot) in out.iter_mut().enumerate() {
        let center = noise_center_hz(band);
        let (low, high) = ((center / edge / bin_hz).ceil() as usize, (center * edge / bin_hz).floor() as usize);
        let mut values: Vec<f64> = (low..=high.min(power.len().saturating_sub(1)))
            .filter(|&k| !masked.get(k).copied().unwrap_or(true))
            .filter_map(|k| power.get(k).copied())
            .collect();
        if values.len() < 3 {
            continue;
        }
        let band_power = median(&mut values) / correction.max(0.1);
        let density = 2.0 * band_power / (rate * window_power);
        let amplitude = density.sqrt() / sum;
        // Under -80 dB of the modes over its band, a band is numerical floor, not noise.
        let width = center * (edge - 1.0 / edge);
        if amplitude.is_finite() && amplitude * width.sqrt() >= 1e-4 {
            *slot = amplitude.min(1.0) as f32;
        }
    }
    out
}

#[cfg(test)]
#[path = "sustain_tests.rs"]
mod tests;

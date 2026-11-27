// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fits a held recording's continuation (`suhina_dsp::continuation::Continuation`): the pitch
//! track by phase-locked demodulation, vibrato and jitter, every harmonic's vibrato
//! participation, carrier and diffuse part, the residual noise, the side, the join, and the
//! gains measured on trial renders of the model itself. Off the audio thread; allocates.
//! See `docs/spec/instrument-held-continuation.md` › What is fitted.

use std::f64::consts::{PI, TAU};
use std::ops::Range;

use rustfft::{FftPlanner, num_complex::Complex64};

use crate::Error;
use suhina_dsp::continuation::math::{Butter3, Butter3State, C};
use suhina_dsp::continuation::voice::{Generator, Prepared};
use suhina_dsp::continuation::{
    AR_ORDER, CONTROL_HZ, Continuation, DEMOD_HZ, Diffuse, GROUP_FROM, HISTORY, JITTER_ORDER, MAX_PARTIALS,
    MIN_CUTOFF_HZ, NOISE_POINTS, NoteModel, Partial, noise_point_hz,
};

/// Seconds analyzed on each side of the held region, then dropped.
const PAD_SECONDS: f64 = 0.15;
/// The shortest held region, and the longest analyzed (its end).
const MIN_HELD_SECONDS: f64 = 0.4;
const MAX_HELD_SECONDS: f64 = 4.0;
/// Join candidates: every 0.05 s over the held region's last 40 %.
const JOIN_STEP: f64 = 0.05;
const JOIN_SHARE: f64 = 0.4;
/// Pitch track: 50 ms windows every 4 ms.
const TRACK_WINDOW: f64 = 0.05;
const TRACK_HOP: f64 = 0.004;
/// Welch frames of the diffuse parts.
const WELCH: usize = 128;
const WELCH_HOP: usize = 32;
/// The residual's spectrum frames.
const NOISE_FFT: usize = 4096;
/// Trial renders: seeds and lengths.
const LEVEL_RENDERS: u64 = 6;
const COUPLING_RENDERS: u64 = 10;

/// Work units the progress callback counts to.
pub const FIT_STEPS: usize = 8;

/// Fits the continuation of the held note in `held` (frames of the interleaved `samples`),
/// whose pitch is near `root_hz`. `progress(done, of)` returning false cancels.
pub fn fit(
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    held: Range<usize>,
    root_hz: f64,
    mut progress: impl FnMut(usize, usize) -> bool,
) -> Result<Continuation, Error> {
    let mut step = |done: usize| if progress(done, FIT_STEPS) { Ok(()) } else { Err(Error::Cancelled) };
    if !(1..=2).contains(&channels) || !(8000..=384_000).contains(&sample_rate) {
        return Err(Error::InvalidFormat);
    }
    let channels = usize::from(channels);
    if !samples.len().is_multiple_of(channels) {
        return Err(Error::InvalidFormat);
    }
    if !root_hz.is_finite() || !(20.0..=5000.0).contains(&root_hz) {
        return Err(Error::NoPitch);
    }
    let sr = f64::from(sample_rate);
    let frames = samples.len() / channels;
    let min_held = (MIN_HELD_SECONDS * sr) as usize;
    if held.start >= held.end || held.end > frames || held.end - held.start < min_held {
        return Err(Error::InvalidRange);
    }
    if !samples.iter().all(|v| v.is_finite()) {
        return Err(Error::NonFinite);
    }
    // The analyzed held part: at most its last four seconds.
    let t1 = held.end;
    let t0 = held.start.max(t1.saturating_sub((MAX_HELD_SECONDS * sr) as usize));
    let pad = ((PAD_SECONDS * sr) as usize).min(t0).min(frames - t1);
    let (a, b) = (t0 - pad, t1 + pad);
    let channel = |c: usize| -> Vec<f64> {
        (a..b).map(|i| f64::from(samples.get(i * channels + c.min(channels - 1)).copied().unwrap_or(0.0))).collect()
    };
    let left = channel(0);
    let right = channel(1);
    let mono: Vec<f64> = left.iter().zip(&right).map(|(l, r)| (l + r) / 2.0).collect();
    let side_signal: Vec<f64> = left.iter().zip(&right).map(|(l, r)| (l - r) / 2.0).collect();
    step(1)?;

    // 1. Pitch track, refined on the harmonics' phases.
    let (fmin, fmax) = (root_hz / 1.3, root_hz * 1.3);
    let mut f0 = pitch_track(&mono, sr, fmin, fmax).ok_or(Error::NoPitch)?;
    for _ in 0..3 {
        refine(&mono, sr, &mut f0);
    }
    step(2)?;
    let phi = phase_of(&f0, sr);
    let held_f0 = f0.get(pad..pad + (t1 - t0)).unwrap_or(&[]);
    let median_f0 = median(held_f0);
    if median_f0.is_nan() || median_f0 <= 0.0 {
        return Err(Error::NoPitch);
    }
    let harmonics = ((0.45 * sr / median_f0).floor() as usize).clamp(1, MAX_PARTIALS);

    // 2. Every harmonic demodulated, decimated to the control rate, and summed back for the
    //    residual; the side on the mid's phase.
    let ds = ((sr / CONTROL_HZ).floor() as usize).max(1);
    let mut recon = vec![0.0; mono.len()];
    let mut side_recon = vec![0.0; mono.len()];
    let mut demodulated = Vec::with_capacity(harmonics);
    let mut side_demodulated = Vec::with_capacity(harmonics);
    let stereo_power: f64 = side_signal.iter().map(|v| v * v).sum::<f64>();
    let mono_power: f64 = mono.iter().map(|v| v * v).sum::<f64>();
    let stereo = channels == 2 && stereo_power > 1e-10 * mono_power.max(1e-30);
    for h in 1..=harmonics {
        demodulated.push(demodulate(&mono, &phi, h, sr, ds, &mut recon));
        if stereo {
            side_demodulated.push(demodulate(&side_signal, &phi, h, sr, ds, &mut side_recon));
        }
    }
    step(3)?;
    let residual: Vec<f64> = mono.iter().zip(&recon).map(|(x, r)| x - r).collect();
    let side_residual: Vec<f64> = side_signal.iter().zip(&side_recon).map(|(x, r)| x - r).collect();

    // 3. The join: where the diffuse parts are smallest relative to the carriers.
    let frame_of = |sample: usize| (sample - a) / ds;
    let start_frame = frame_of(t0);
    let join = choose_join(&demodulated, start_frame, held.start.max(t0), t1, a, ds, sr);
    let end_frame = frame_of(join).max(start_frame + 2 * HISTORY);
    let segment = start_frame..end_frame;
    let audio_segment = (t0 - a)..(join - a);
    step(4)?;

    // 4. Mean pitch, vibrato, jitter.
    let f0_held = f0.get(audio_segment.clone()).unwrap_or(&[]);
    let f0m = geometric_mean(f0_held);
    if !(20.0..=5000.0).contains(&f0m) {
        return Err(Error::NoPitch);
    }
    let cents: Vec<f64> = f0_held.iter().map(|f| 1200.0 * (f / f0m).log2()).collect();
    let smooth_cents = zero_phase(&cents, 12.0, sr);
    let decimated_vibrato: Vec<f64> = smooth_cents.iter().step_by(ds).copied().collect();
    let (vibrato_hz, vibrato_cents, vibrato_offset) = fit_sine(&decimated_vibrato, sr / ds as f64);
    let jitter: Vec<f64> = cents.iter().zip(&smooth_cents).map(|(c, s)| c - s).collect();
    let jitter_cents = standard_deviation(&jitter);
    let jitter_frames: Vec<f64> = jitter.iter().step_by(ds).copied().collect();
    let (jitter_ar, jitter_innovation) = real_ar(&jitter_frames, JITTER_ORDER);
    let jitter_filter = Butter3::new(25.0, CONTROL_HZ);
    let jitter_power = suhina_dsp::continuation::math::filtered_real_ar_power(
        &jitter_ar,
        jitter_innovation,
        &jitter_filter,
        CONTROL_HZ,
    );
    let jitter_gain = if jitter_power > 0.0 { jitter_cents / jitter_power.sqrt() } else { 0.0 };
    let held_seconds = (join - t0) as f64 / sr;
    let vibrato_phase = TAU * vibrato_hz * held_seconds + vibrato_offset;
    // Phases one sample after the held part's end, where the model's first sample is.
    let last = join - a - 1;
    let f_last = f0.get(last).copied().unwrap_or(f0m);
    let phase_at = phi.get(last).copied().unwrap_or(0.0) + TAU * f_last / sr;
    let deviation_series: Vec<f64> = phi.iter().enumerate().map(|(n, p)| p - TAU * f0m * n as f64 / sr).collect();
    let deviation_at = deviation_series.get(last).copied().unwrap_or(0.0) + TAU * (f_last - f0m) / sr;
    let deviation_frames: Vec<f64> = deviation_series.iter().step_by(ds).copied().collect();
    let f0_frames: Vec<f64> = f0.iter().step_by(ds).copied().collect();

    // 5. Per harmonic: participation, carrier, diffuse part; the side.
    let frame_rate = sr / ds as f64;
    let vib_at = |n: usize| {
        let t = (n - start_frame) as f64 / frame_rate;
        let angle = TAU * vibrato_hz * t + vibrato_offset;
        (vibrato_cents * angle.sin(), vibrato_cents * angle.cos())
    };
    let gammas = participation(&demodulated, &f0_frames, f0m, segment.clone(), frame_rate);
    let mut corrected: Vec<Vec<C>> = Vec::with_capacity(harmonics);
    let mut side_corrected: Vec<Vec<C>> = Vec::with_capacity(harmonics);
    for (h, series) in demodulated.iter().enumerate() {
        let gamma = gammas.get(h).copied().unwrap_or(1.0);
        let correct = |s: &[C]| -> Vec<C> {
            segment
                .clone()
                .map(|n| {
                    let psi = deviation_frames.get(n).copied().unwrap_or(0.0);
                    s.get(n).copied().unwrap_or(C::ZERO) * C::cis((1.0 - gamma) * (h + 1) as f64 * psi)
                })
                .collect()
        };
        corrected.push(correct(series));
        if let Some(side) = side_demodulated.get(h) {
            side_corrected.push(correct(side));
        }
    }
    let vibrato_terms: Vec<(f64, f64)> = segment.clone().map(vib_at).collect();
    let carriers = carriers(&corrected, &vibrato_terms);
    step(5)?;
    let diffuse: Vec<Vec<C>> = corrected
        .iter()
        .zip(&carriers)
        .map(|(series, (carrier, ru, rw))| {
            series
                .iter()
                .zip(&vibrato_terms)
                .map(|(a, (v, vd))| *a - *carrier * (C::new(1.0, 0.0) + ru.scale(*v) + rw.scale(*vd)))
                .collect()
        })
        .collect();
    let mid_models = diffuse_models(&diffuse);
    let (rhos, side_parts): (Vec<C>, Vec<Vec<C>>) = side_corrected
        .iter()
        .zip(&corrected)
        .map(|(side, mid)| {
            let num = side.iter().zip(mid).fold(C::ZERO, |acc, (s, m)| acc + *s * m.conj());
            let den: f64 = mid.iter().map(|m| m.norm_sqr()).sum();
            let rho = if den > 0.0 { num.scale(1.0 / den) } else { C::ZERO };
            let rho = if rho.abs() > 64.0 { rho.scale(64.0 / rho.abs()) } else { rho };
            let independent = side.iter().zip(mid).map(|(s, m)| *s - rho * *m).collect();
            (rho, independent)
        })
        .unzip();
    let side_models = if stereo { diffuse_models(&side_parts) } else { Vec::new() };
    let mut partials = Vec::with_capacity(harmonics);
    for (h, series) in corrected.iter().enumerate() {
        let (carrier, _, _) = carriers.get(h).copied().unwrap_or((C::ZERO, C::ZERO, C::ZERO));
        let logs: Vec<f64> = series.iter().map(|a| (a.abs() + 1e-9).ln()).collect();
        let mean_log = mean(&logs);
        let sigma_db = 8.686 * standard_deviation(&logs);
        let tail = diffuse.get(h).map_or(&[][..], Vec::as_slice);
        let start_level = tail.iter().rev().take(6).map(|c| (carrier + *c).abs()).sum::<f64>() / 6.0;
        let mut partial = Partial::NONE;
        partial.carrier = carrier.to_pair();
        partial.gamma = gammas.get(h).copied().unwrap_or(1.0) as f32;
        partial.mean_log = mean_log.clamp(-80.0, 10.0) as f32;
        partial.sigma_db = sigma_db.clamp(0.0, 200.0) as f32;
        partial.spread = 1.0;
        partial.start_level = start_level.clamp(0.0, 16.0) as f32;
        partial.diffuse = mid_models.get(h).copied().unwrap_or(Diffuse::NONE);
        if stereo {
            partial.side_rho = rhos.get(h).copied().unwrap_or(C::ZERO).to_pair();
            partial.side = side_models.get(h).copied().unwrap_or(Diffuse::NONE);
        }
        partials.push(partial);
    }

    // 6. Residual noise, the side's as the mid's shape times their measured ratio.
    let noise_part = residual.get(audio_segment.clone()).unwrap_or(&[]);
    let noise_spectrum = power_spectrum(noise_part);
    let noise_rms = rms(noise_part);
    let noise_db = to_grid(&noise_spectrum, sr);
    let (side_noise_db, side_noise_rms) = if stereo {
        let side_spectrum = power_spectrum(side_residual.get(audio_segment.clone()).unwrap_or(&[]));
        let shaped = side_shape(&noise_spectrum, &side_spectrum);
        let total: f64 = noise_spectrum.iter().sum();
        let side_total: f64 = shaped.iter().sum();
        let ratio = if total > 0.0 { (side_total / total).sqrt() } else { 0.0 };
        (to_grid(&shaped, sr), noise_rms * ratio)
    } else {
        ([0.0; NOISE_POINTS], 0.0)
    };
    step(6)?;

    // 7. Where the recording's own release starts.
    let release_start = release_start(samples, channels, sample_rate, join);

    let mut continuation = Continuation {
        recording_rate: sample_rate as f32,
        f0: f0m as f32,
        vibrato_hz: if vibrato_cents > 0.0 { vibrato_hz.clamp(3.0, 10.0) as f32 } else { 0.0 },
        vibrato_cents: vibrato_cents.clamp(0.0, 200.0) as f32,
        vibrato_phase: vibrato_phase.rem_euclid(TAU) as f32,
        phase: phase_at.rem_euclid(TAU) as f32,
        deviation_phase: deviation_at.clamp(-1e6, 1e6) as f32,
        join: (join as f64 / sr) as f32,
        release_start: release_start.map_or(0.0, |s| (s as f64 / sr) as f32),
        jitter_ar: std::array::from_fn(|k| jitter_ar.get(k).copied().unwrap_or(0.0).clamp(-16.0, 16.0) as f32),
        jitter_innovation: jitter_innovation.max(0.0) as f32,
        jitter_cents: jitter_cents.clamp(0.0, 200.0) as f32,
        jitter_gain: jitter_gain.clamp(0.0, 1e6) as f32,
        level_db: 0.0,
        coef_target: [0.0; 3],
        coef_model: [0.0; 3],
        noise_db,
        noise_rms: noise_rms.clamp(0.0, 16.0) as f32,
        side_noise_db,
        side_noise_rms: side_noise_rms.clamp(0.0, 16.0) as f32,
        stereo,
        partials,
    };
    if continuation.vibrato_cents > 0.0 && continuation.vibrato_hz == 0.0 {
        continuation.vibrato_cents = 0.0;
    }

    // 8. Trial renders: the spread factors, the level gain and the model's vibrato-locked level.
    let target = coupling_target(&mono, audio_segment.clone(), &cents, sr, vibrato_hz);
    trial_renders(&mut continuation, sr, target, &mono, audio_segment)?;
    step(7)?;
    continuation.validate().map_err(|_| Error::NoPitch)?;
    step(8)?;
    Ok(continuation)
}

/// A normalized cross-correlation pitch track at `TRACK_HOP`, interpolated linearly to every
/// sample and clipped to the range; none when the median window correlates below 0.5 with
/// itself a period later.
fn pitch_track(signal: &[f64], sr: f64, fmin: f64, fmax: f64) -> Option<Vec<f64>> {
    let window = (TRACK_WINDOW * sr) as usize;
    let hop = ((TRACK_HOP * sr) as usize).max(1);
    let lag_min = ((sr / fmax).floor() as usize).max(2);
    let lag_max = (sr / fmin).ceil() as usize + 1;
    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut peaks: Vec<f64> = Vec::new();
    let mut start = 0;
    while start + window + lag_max + 2 <= signal.len() {
        let a = signal.get(start..start + window)?;
        let energy: f64 = a.iter().map(|v| v * v).sum::<f64>() + 1e-12;
        let correlation = |lag: usize| -> f64 {
            let Some(b) = signal.get(start + lag..start + lag + window) else { return -1.0 };
            let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
            let eb: f64 = b.iter().map(|v| v * v).sum::<f64>() + 1e-12;
            dot / (energy * eb).sqrt()
        };
        let mut best = (lag_min, f64::MIN);
        for lag in lag_min..=lag_max {
            let c = correlation(lag);
            if c > best.1 {
                best = (lag, c);
            }
        }
        let (lag, peak) = best;
        let (y0, y2) = (correlation(lag.saturating_sub(1)), correlation(lag + 1));
        let denominator = y0 - 2.0 * peak + y2;
        let shift = if denominator.abs() > 1e-12 { (0.5 * (y0 - y2) / denominator).clamp(-0.5, 0.5) } else { 0.0 };
        let center = start as f64 + window as f64 / 2.0;
        points.push((center, sr / (lag as f64 + shift)));
        peaks.push(peak);
        start += hop;
    }
    // A pitched tone correlates with itself a period later; silence or noise does not.
    if points.len() < 4 || median_mut(&mut peaks) < 0.5 {
        return None;
    }
    let mut out = Vec::with_capacity(signal.len());
    let mut at = 0;
    for n in 0..signal.len() {
        let t = n as f64;
        while at + 2 < points.len() && points.get(at + 1).is_some_and(|p| p.0 < t) {
            at += 1;
        }
        let (Some(&(t0, f0)), Some(&(t1, f1))) = (points.get(at), points.get(at + 1)) else { break };
        let f = if t1 > t0 { f0 + (f1 - f0) * (t - t0) / (t1 - t0) } else { f0 };
        out.push(f.clamp(fmin, fmax));
    }
    (out.len() == signal.len()).then_some(out)
}

/// The phase `2 pi integral f0`.
fn phase_of(f0: &[f64], sr: f64) -> Vec<f64> {
    let mut phase = 0.0;
    f0.iter()
        .map(|f| {
            phase += TAU * f / sr;
            phase
        })
        .collect()
}

/// A third-order Butterworth low-pass at `cutoff`, run forward and backward.
fn zero_phase_complex(values: &mut [C], cutoff: f64, rate: f64) {
    let filter = Butter3::new(cutoff, rate);
    let mut state = Butter3State::default();
    for value in values.iter_mut() {
        *value = state.process(&filter, *value);
    }
    let mut state = Butter3State::default();
    for value in values.iter_mut().rev() {
        *value = state.process(&filter, *value);
    }
}

fn zero_phase(values: &[f64], cutoff: f64, rate: f64) -> Vec<f64> {
    let mut complex: Vec<C> = values.iter().map(|v| C::new(*v, 0.0)).collect();
    zero_phase_complex(&mut complex, cutoff, rate);
    complex.into_iter().map(|c| c.re).collect()
}

/// One iteration of the pitch refinement: the first eight harmonics' phase slopes, weighted by
/// their power and low-passed at 12 Hz, added to `f0`.
fn refine(signal: &[f64], sr: f64, f0: &mut [f64]) {
    let phi = phase_of(f0, sr);
    let mut numerator = vec![0.0; signal.len()];
    let mut denominator = vec![0.0; signal.len()];
    for h in 1..=8_usize {
        let mut z: Vec<C> = signal.iter().zip(&phi).map(|(x, p)| C::cis(-(h as f64) * p).scale(*x)).collect();
        zero_phase_complex(&mut z, DEMOD_HZ, sr);
        let mut previous = z.first().map_or(0.0, |c| c.arg());
        let mut unwrapped = Vec::with_capacity(z.len());
        let mut offset = 0.0;
        for c in &z {
            let angle = c.arg();
            let mut delta = angle - previous;
            if delta > PI {
                delta -= TAU;
            } else if delta < -PI {
                delta += TAU;
            }
            offset += delta;
            previous = angle;
            unwrapped.push(offset);
        }
        for (n, (num, den)) in numerator.iter_mut().zip(denominator.iter_mut()).enumerate() {
            let before = unwrapped.get(n.saturating_sub(1)).copied().unwrap_or(0.0);
            let after = unwrapped.get((n + 1).min(unwrapped.len().saturating_sub(1))).copied().unwrap_or(0.0);
            let span = ((n + 1).min(unwrapped.len().saturating_sub(1)) - n.saturating_sub(1)).max(1) as f64;
            let slope = (after - before) / span * sr / TAU / h as f64;
            let weight = z.get(n).map_or(0.0, |c| 4.0 * c.norm_sqr());
            *num += weight * slope;
            *den += weight;
        }
    }
    let correction: Vec<f64> = numerator.iter().zip(&denominator).map(|(n, d)| n / (d + 1e-18)).collect();
    let correction = zero_phase(&correction, 12.0, sr);
    for (f, c) in f0.iter_mut().zip(correction) {
        if c.is_finite() {
            *f += c;
        }
    }
}

/// Harmonic `h` of `signal` on the phase `phi`: `2 LP(x exp(-j h phi))`, added back into
/// `recon`, and decimated by `ds`.
fn demodulate(signal: &[f64], phi: &[f64], h: usize, sr: f64, ds: usize, recon: &mut [f64]) -> Vec<C> {
    let hf = h as f64;
    let mut z: Vec<C> = signal.iter().zip(phi).map(|(x, p)| C::cis(-hf * p).scale(2.0 * x)).collect();
    zero_phase_complex(&mut z, DEMOD_HZ, sr);
    for ((r, a), p) in recon.iter_mut().zip(&z).zip(phi) {
        *r += (*a * C::cis(hf * p)).re;
    }
    z.into_iter().step_by(ds).collect()
}

/// The join among the candidates: the frame where the carriers' share of the diffuse parts at
/// the end is smallest.
fn choose_join(
    demodulated: &[Vec<C>],
    start_frame: usize,
    held_start: usize,
    held_end: usize,
    a: usize,
    ds: usize,
    sr: f64,
) -> usize {
    let span = held_end - held_start;
    let first = held_end - (span as f64 * JOIN_SHARE) as usize;
    let step = ((JOIN_STEP * sr) as usize).max(1);
    let min_end = held_start + (MIN_HELD_SECONDS * sr * 0.75) as usize;
    let mut best = (held_end, f64::INFINITY);
    let mut candidate = held_end;
    loop {
        if candidate >= min_end {
            let end = (candidate - a) / ds;
            if end > start_frame + 6 {
                let mut diffuse = 0.0;
                let mut carrier = 0.0;
                for series in demodulated {
                    let Some(part) = series.get(start_frame..end) else { continue };
                    let n = part.len() as f64;
                    let mean = part.iter().fold(C::ZERO, |acc, c| acc + *c).scale(1.0 / n);
                    let tail = part.iter().rev().take(6).fold(C::ZERO, |acc, c| acc + (*c - mean)).scale(1.0 / 6.0);
                    diffuse += tail.norm_sqr();
                    carrier += mean.norm_sqr();
                }
                let d = diffuse / carrier.max(1e-30);
                if d < best.1 {
                    best = (candidate, d);
                }
            }
        }
        if candidate < first + step {
            break;
        }
        candidate -= step;
    }
    best.0
}

/// Per harmonic: the share of the pitch deviation it follows, from its frequency regressed on
/// `h (f0 - f0m)`, clipped and the median of itself and its two same-parity neighbours.
fn participation(
    demodulated: &[Vec<C>],
    f0_frames: &[f64],
    f0m: f64,
    segment: Range<usize>,
    frame_rate: f64,
) -> Vec<f64> {
    let raw: Vec<f64> = demodulated
        .iter()
        .enumerate()
        .map(|(h, series)| {
            let mut points = Vec::new();
            for n in segment.clone().skip(1) {
                let (Some(z1), Some(z0)) = (series.get(n), series.get(n - 1)) else { continue };
                let d = (*z1 * z0.conj()).arg() * frame_rate / TAU;
                let x = (h + 1) as f64 * (f0_frames.get(n).copied().unwrap_or(f0m) - f0m);
                points.push((x, d, z1.norm_sqr()));
            }
            let w: f64 = points.iter().map(|p| p.2).sum::<f64>() + 1e-30;
            let xm = points.iter().map(|p| p.2 * p.0).sum::<f64>() / w;
            let dm = points.iter().map(|p| p.2 * p.1).sum::<f64>() / w;
            let sxx: f64 = points.iter().map(|p| p.2 * (p.0 - xm).powi(2)).sum();
            let sxd: f64 = points.iter().map(|p| p.2 * (p.0 - xm) * (p.1 - dm)).sum();
            (1.0 + sxd / (sxx + 1e-30)).clamp(0.0, 1.3)
        })
        .collect();
    (0..raw.len())
        .map(|h| {
            let mut near: Vec<f64> = [h.checked_sub(2), Some(h), Some(h + 2)]
                .into_iter()
                .flatten()
                .filter_map(|k| raw.get(k).copied())
                .collect();
            median_mut(&mut near).clamp(0.0, 1.3)
        })
        .collect()
}

/// Per harmonic, the complex least-squares fit on `[1, v, v' / omega]`: the carrier and its
/// vibrato coupling (relative), the coupling smoothed over three neighbours on each side.
fn carriers(corrected: &[Vec<C>], terms: &[(f64, f64)]) -> Vec<(C, C, C)> {
    // Normal equations of the real regressors, shared by every harmonic.
    let mut gram = [[0.0; 3]; 3];
    for (v, vd) in terms {
        let r = [1.0, *v, *vd];
        for (row, a) in gram.iter_mut().zip(r) {
            for (cell, b) in row.iter_mut().zip(r) {
                *cell += a * b;
            }
        }
    }
    let raw: Vec<(C, C, C)> = corrected
        .iter()
        .map(|series| {
            let mut rhs = [C::ZERO; 3];
            for (a, (v, vd)) in series.iter().zip(terms) {
                for (slot, r) in rhs.iter_mut().zip([1.0, *v, *vd]) {
                    *slot += a.scale(r);
                }
            }
            let re = solve3(gram, rhs.map(|c| c.re));
            let im = solve3(gram, rhs.map(|c| c.im));
            let c = [C::new(re[0], im[0]), C::new(re[1], im[1]), C::new(re[2], im[2])];
            let carrier = c[0];
            if carrier.abs() > 1e-12 {
                let inverse = carrier.conj().scale(1.0 / carrier.norm_sqr());
                (carrier, c[1] * inverse, c[2] * inverse)
            } else {
                (carrier, C::ZERO, C::ZERO)
            }
        })
        .collect();
    (0..raw.len())
        .map(|h| {
            let lo = h.saturating_sub(3);
            let hi = (h + 4).min(raw.len());
            let (mut u, mut w, mut total) = (C::ZERO, C::ZERO, 0.0);
            for (carrier, ru, rw) in raw.get(lo..hi).unwrap_or(&[]) {
                let weight = carrier.norm_sqr() + 1e-30;
                u += ru.scale(weight);
                w += rw.scale(weight);
                total += weight;
            }
            let carrier = raw.get(h).map_or(C::ZERO, |r| r.0);
            (carrier, u.scale(1.0 / total), w.scale(1.0 / total))
        })
        .collect()
}

fn solve3(m: [[f64; 3]; 3], b: [f64; 3]) -> [f64; 3] {
    let mut rows: Vec<Vec<f64>> = m.iter().zip(b).map(|(row, v)| vec![row[0], row[1], row[2], v]).collect();
    gauss(&mut rows).map_or([0.0; 3], |x| [x[0], x[1], x[2]])
}

/// Gaussian elimination with partial pivoting on an augmented real matrix.
fn gauss(rows: &mut [Vec<f64>]) -> Option<Vec<f64>> {
    let n = rows.len();
    for col in 0..n {
        let pivot = (col..n).max_by(|&i, &j| {
            let a = rows.get(i).and_then(|r| r.get(col)).map_or(0.0, |v| v.abs());
            let b = rows.get(j).and_then(|r| r.get(col)).map_or(0.0, |v| v.abs());
            a.total_cmp(&b)
        })?;
        rows.swap(col, pivot);
        let head = rows.get(col)?.clone();
        let p = *head.get(col)?;
        if p.abs() < 1e-300 {
            return None;
        }
        for row in rows.iter_mut().skip(col + 1) {
            let factor = row.get(col).copied().unwrap_or(0.0) / p;
            for (cell, h) in row.iter_mut().zip(&head).skip(col) {
                *cell -= factor * h;
            }
        }
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let row = rows.get(i)?;
        let mut v = *row.get(n)?;
        for j in i + 1..n {
            v -= row.get(j)? * x.get(j)?;
        }
        *x.get_mut(i)? = v / row.get(i)?;
    }
    Some(x)
}

/// Gaussian elimination on an augmented complex matrix.
fn gauss_complex(rows: &mut [Vec<C>]) -> Option<Vec<C>> {
    let n = rows.len();
    for col in 0..n {
        let pivot = (col..n).max_by(|&i, &j| {
            let a = rows.get(i).and_then(|r| r.get(col)).map_or(0.0, |v| v.abs());
            let b = rows.get(j).and_then(|r| r.get(col)).map_or(0.0, |v| v.abs());
            a.total_cmp(&b)
        })?;
        rows.swap(col, pivot);
        let head = rows.get(col)?.clone();
        let p = *head.get(col)?;
        if p.abs() < 1e-300 {
            return None;
        }
        let inverse = p.conj().scale(1.0 / p.norm_sqr());
        for row in rows.iter_mut().skip(col + 1) {
            let factor = row.get(col).copied().unwrap_or(C::ZERO) * inverse;
            for (cell, h) in row.iter_mut().zip(&head).skip(col) {
                *cell = *cell - factor * *h;
            }
        }
    }
    let mut x = vec![C::ZERO; n];
    for i in (0..n).rev() {
        let row = rows.get(i)?;
        let mut v = *row.get(n)?;
        for j in i + 1..n {
            v = v - *row.get(j)? * *x.get(j)?;
        }
        let d = *row.get(i)?;
        *x.get_mut(i)? = v * d.conj().scale(1.0 / d.norm_sqr().max(1e-300));
    }
    Some(x)
}

/// Every harmonic's diffuse model: its Welch spectrum (averaged from harmonic 12 on with its
/// same-parity neighbours two on each side at half weight), an AR(6) with its power, the
/// bandwidth's low-pass, the power fix and the last frames.
fn diffuse_models(parts: &[Vec<C>]) -> Vec<Diffuse> {
    let spectra: Vec<Vec<f64>> = parts.iter().map(|p| welch(p)).collect();
    let powers: Vec<f64> =
        parts.iter().map(|p| p.iter().map(|c| c.norm_sqr()).sum::<f64>() / p.len().max(1) as f64).collect();
    (0..parts.len())
        .map(|h| {
            let group = if h < GROUP_FROM { 0 } else { 2 };
            let mut sum = vec![0.0; WELCH];
            let mut weights = 0.0;
            for k in (h as i64 - 2 * group..=h as i64 + 2 * group).step_by(2) {
                let Some(spectrum) = usize::try_from(k).ok().and_then(|k| spectra.get(k)) else { continue };
                let w = if k == h as i64 { 1.0 } else { 0.5 };
                for (s, v) in sum.iter_mut().zip(spectrum) {
                    *s += w * v;
                }
                weights += w;
            }
            for s in &mut sum {
                *s /= weights.max(1e-30);
            }
            let power = powers.get(h).copied().unwrap_or(0.0).max(1e-24);
            let (ar, innovation) = ar_from_spectrum(&sum, power);
            let half_width = bandwidth(&sum);
            let cutoff = if h < GROUP_FROM { (2.0 * half_width).clamp(MIN_CUTOFF_HZ, DEMOD_HZ) } else { DEMOD_HZ };
            let coefficients: [C; AR_ORDER] = std::array::from_fn(|k| ar.get(k).copied().unwrap_or(C::ZERO));
            let fix = Diffuse::fix_for(&coefficients, innovation, power, cutoff);
            let part = parts.get(h).map_or(&[][..], Vec::as_slice);
            let mut history = [[0.0_f32; 2]; HISTORY];
            let tail = part.len().saturating_sub(HISTORY);
            for (slot, value) in history.iter_mut().zip(part.iter().skip(tail)) {
                *slot = value.to_pair();
            }
            Diffuse {
                ar: coefficients.map(C::to_pair),
                innovation: innovation.min(power) as f32,
                power: power as f32,
                cutoff_hz: cutoff as f32,
                power_fix: fix as f32,
                history,
            }
        })
        .collect()
}

/// The unit-sum Welch spectrum (FFT order) of a complex series: 128-point Hann frames every 32.
fn welch(series: &[C]) -> Vec<f64> {
    let length = WELCH.min(series.len()).max(1);
    let hop = WELCH_HOP.min((length / 2).max(1));
    let window: Vec<f64> = (0..length).map(|i| 0.5 - 0.5 * (TAU * i as f64 / length as f64).cos()).collect();
    let fft = FftPlanner::<f64>::new().plan_fft_forward(WELCH);
    let mut sum = vec![0.0; WELCH];
    let mut frames = 0;
    let mut start = 0;
    while start + length <= series.len() {
        let mut buffer = vec![Complex64::new(0.0, 0.0); WELCH];
        for ((slot, value), w) in buffer.iter_mut().zip(series.iter().skip(start)).zip(&window) {
            *slot = Complex64::new(value.re * w, value.im * w);
        }
        fft.process(&mut buffer);
        for (s, b) in sum.iter_mut().zip(&buffer) {
            *s += b.norm_sqr();
        }
        frames += 1;
        start += hop;
    }
    let total: f64 = sum.iter().sum::<f64>() + 1e-30;
    if frames == 0 {
        // Too short for a frame: white.
        return vec![1.0 / WELCH as f64; WELCH];
    }
    sum.into_iter().map(|v| v / total).collect()
}

/// A complex AR(6) whose spectrum follows the unit-sum `spectrum` with power `power`
/// (Yule-Walker on its autocorrelation), poles pulled inside radius 0.985; and its innovation
/// variance.
fn ar_from_spectrum(spectrum: &[f64], power: f64) -> (Vec<C>, f64) {
    let length = spectrum.len().max(1);
    // r[k] = sum_f S[f] exp(+j 2 pi f k / L), scaled to the power.
    let r: Vec<C> = (0..=AR_ORDER)
        .map(|k| {
            spectrum
                .iter()
                .enumerate()
                .fold(C::ZERO, |acc, (f, s)| acc + C::cis(TAU * f as f64 * k as f64 / length as f64).scale(*s))
        })
        .collect();
    let r0 = r.first().map_or(1.0, |c| c.re).max(1e-30);
    let r: Vec<C> = r.iter().map(|c| c.scale(power / r0)).collect();
    let at = |k: i64| -> C {
        if k >= 0 {
            r.get(k as usize).copied().unwrap_or(C::ZERO)
        } else {
            r.get((-k) as usize).copied().unwrap_or(C::ZERO).conj()
        }
    };
    let mut rows: Vec<Vec<C>> = (0..AR_ORDER)
        .map(|i| {
            let mut row: Vec<C> = (0..AR_ORDER).map(|j| at(i as i64 - j as i64)).collect();
            if let Some(d) = row.get_mut(i) {
                *d += C::new(1e-9 * power, 0.0);
            }
            row.push(at(i as i64 + 1));
            row
        })
        .collect();
    let mut a = gauss_complex(&mut rows).unwrap_or_else(|| vec![C::ZERO; AR_ORDER]);
    let largest = roots(&a).iter().map(|z| z.abs()).fold(0.0, f64::max);
    if largest >= 0.985 {
        let s = 0.985 / largest;
        let mut factor = 1.0;
        for coefficient in &mut a {
            factor *= s;
            *coefficient = coefficient.scale(factor);
        }
    }
    let rhs: Vec<C> = (1..=AR_ORDER).map(|k| at(k as i64)).collect();
    let explained = a.iter().zip(&rhs).fold(C::ZERO, |acc, (c, r)| acc + *c * r.conj());
    let innovation = (at(0) - explained).re.max(1e-18 * power.max(1e-30));
    (a, innovation)
}

/// The roots of `z^p - a1 z^(p-1) - ... - ap` (Durand-Kerner iteration).
fn roots(a: &[C]) -> Vec<C> {
    let p = a.len();
    if p == 0 {
        return Vec::new();
    }
    // Monic polynomial coefficients, highest first: 1, -a1, ..., -ap.
    let poly: Vec<C> = std::iter::once(C::new(1.0, 0.0)).chain(a.iter().map(|c| c.scale(-1.0))).collect();
    let evaluate = |z: C| poly.iter().fold(C::ZERO, |acc, c| acc * z + *c);
    let mut z: Vec<C> = (0..p).map(|k| C::cis(TAU * k as f64 / p as f64 + 0.4).scale(0.9)).collect();
    for _ in 0..200 {
        let previous = z.clone();
        for i in 0..p {
            let Some(&zi) = z.get(i) else { continue };
            let mut denominator = C::new(1.0, 0.0);
            for (j, zj) in previous.iter().enumerate() {
                if j != i {
                    denominator = denominator * (zi - *zj);
                }
            }
            let d = denominator.norm_sqr();
            if d < 1e-300 {
                continue;
            }
            let delta = evaluate(zi) * denominator.conj().scale(1.0 / d);
            if let Some(slot) = z.get_mut(i) {
                *slot = (zi - delta).finite();
            }
        }
    }
    z
}

/// Half the width (Hz) where the unit-sum spectrum stays within 10 dB of its peak; at least 1.
fn bandwidth(spectrum: &[f64]) -> f64 {
    let length = spectrum.len();
    if length == 0 {
        return 1.0;
    }
    // FFT order to centered: index i of the shifted spectrum is FFT bin (i + L/2) mod L.
    let shifted: Vec<f64> =
        (0..length).map(|i| spectrum.get((i + length / 2) % length).copied().unwrap_or(0.0)).collect();
    let (peak, value) =
        shifted.iter().enumerate().fold((0, f64::MIN), |best, (i, v)| if *v > best.1 { (i, *v) } else { best });
    let threshold = value / 10.0;
    let mut lo = peak;
    while lo > 0 && shifted.get(lo - 1).is_some_and(|v| *v >= threshold) {
        lo -= 1;
    }
    let mut hi = peak;
    while hi + 1 < length && shifted.get(hi + 1).is_some_and(|v| *v >= threshold) {
        hi += 1;
    }
    let resolution = CONTROL_HZ / length as f64;
    ((hi - lo) as f64 * resolution).max(1.0) / 2.0
}

/// A real AR model by Yule-Walker, poles pulled inside 0.985 (by shrinking the coefficients),
/// and its innovation variance.
fn real_ar(values: &[f64], order: usize) -> (Vec<f64>, f64) {
    let mean = mean(values);
    let centered: Vec<f64> = values.iter().map(|v| v - mean).collect();
    let n = centered.len().max(1) as f64;
    let r: Vec<f64> = (0..=order)
        .map(|k| centered.iter().zip(centered.iter().skip(k)).map(|(a, b)| a * b).sum::<f64>() / n)
        .collect();
    let at = |k: usize| r.get(k).copied().unwrap_or(0.0);
    let mut rows: Vec<Vec<f64>> = (0..order)
        .map(|i| {
            let mut row: Vec<f64> = (0..order).map(|j| at(i.abs_diff(j))).collect();
            if let Some(d) = row.get_mut(i) {
                *d += 1e-12 * at(0).max(1e-30);
            }
            row.push(at(i + 1));
            row
        })
        .collect();
    let mut a = gauss(&mut rows).unwrap_or_else(|| vec![0.0; order]);
    let complex: Vec<C> = a.iter().map(|v| C::new(*v, 0.0)).collect();
    let largest = roots(&complex).iter().map(|z| z.abs()).fold(0.0, f64::max);
    if largest >= 0.985 {
        let s = 0.985 / largest;
        let mut factor = 1.0;
        for coefficient in &mut a {
            factor *= s;
            *coefficient *= factor;
        }
    }
    let explained: f64 = a.iter().enumerate().map(|(k, c)| c * at(k + 1)).sum();
    (a, (at(0) - explained).max(1e-18))
}

/// The least-squares sine of 3.5..9 Hz (0.02 Hz steps) through `values` at `rate`: its
/// frequency, peak and phase (`values ~ peak sin(2 pi f t + phase)`).
fn fit_sine(values: &[f64], rate: f64) -> (f64, f64, f64) {
    let mut best = (0.0, 0.0, 0.0, f64::INFINITY);
    let mut f = 3.5;
    while f < 9.0 {
        let (mut ss, mut cc, mut sc, mut vs, mut vc) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for (n, v) in values.iter().enumerate() {
            let (s, c) = (TAU * f * n as f64 / rate).sin_cos();
            ss += s * s;
            cc += c * c;
            sc += s * c;
            vs += v * s;
            vc += v * c;
        }
        let det = ss * cc - sc * sc;
        if det.abs() > 1e-12 {
            let a = (vs * cc - vc * sc) / det;
            let b = (vc * ss - vs * sc) / det;
            let error: f64 = values
                .iter()
                .enumerate()
                .map(|(n, v)| {
                    let (s, c) = (TAU * f * n as f64 / rate).sin_cos();
                    (v - a * s - b * c).powi(2)
                })
                .sum();
            if error < best.3 {
                best = (f, a, b, error);
            }
        }
        f += 0.02;
    }
    let (f, a, b, _) = best;
    (f, a.hypot(b), b.atan2(a))
}

/// The recording's vibrato-locked level coefficients over `segment`, from its cents curve.
fn coupling_target(mono: &[f64], segment: Range<usize>, cents: &[f64], sr: f64, rate: f64) -> [f64; 3] {
    let part = mono.get(segment).unwrap_or(&[]);
    let smooth = zero_phase(cents, 12.0, sr);
    let velocity: Vec<f64> = (0..smooth.len())
        .map(|n| {
            let before = smooth.get(n.saturating_sub(1)).copied().unwrap_or(0.0);
            let after = smooth.get((n + 1).min(smooth.len().saturating_sub(1))).copied().unwrap_or(0.0);
            let span = ((n + 1).min(smooth.len().saturating_sub(1)) - n.saturating_sub(1)).max(1) as f64;
            (after - before) / span * sr / (TAU * rate.max(1e-9))
        })
        .collect();
    vibrato_level(part, sr, |n| (smooth.get(n).copied().unwrap_or(0.0), velocity.get(n).copied().unwrap_or(0.0)))
}

/// The total level (dB, 20 ms windows every 5 ms) regressed on the vibrato's position, velocity
/// and squared position, with a quadratic trend: the first three coefficients.
fn vibrato_level(signal: &[f64], sr: f64, pitch: impl Fn(usize) -> (f64, f64)) -> [f64; 3] {
    let window = (0.02 * sr) as usize;
    let hop = ((0.005 * sr) as usize).max(1);
    if signal.len() < window + 4 * hop {
        return [0.0; 3];
    }
    let frames = (signal.len() - window) / hop;
    let mut rows = Vec::with_capacity(frames);
    for i in 0..frames {
        let part = signal.get(i * hop..i * hop + window).unwrap_or(&[]);
        let level = 10.0 * (part.iter().map(|v| v * v).sum::<f64>() / window as f64 + 1e-14).log10();
        let (c, d) = pitch(i * hop + window / 2);
        rows.push((level, c, d));
    }
    let mean_square = rows.iter().map(|r| r.1 * r.1).sum::<f64>() / rows.len().max(1) as f64;
    let mut gram = vec![vec![0.0; 7]; 6];
    for (i, (level, c, d)) in rows.iter().enumerate() {
        let t = if frames > 1 { -1.0 + 2.0 * i as f64 / (frames - 1) as f64 } else { 0.0 };
        let x = [*c, *d, c * c - mean_square, 1.0, t, t * t];
        for (row, a) in gram.iter_mut().zip(x) {
            for (cell, b) in row.iter_mut().zip(x) {
                *cell += a * b;
            }
            if let Some(last) = row.get_mut(6) {
                *last += a * level;
            }
        }
    }
    gauss(&mut gram).map_or([0.0; 3], |x| {
        let get = |k: usize| x.get(k).copied().filter(|v| v.is_finite()).unwrap_or(0.0).clamp(-100.0, 100.0);
        [get(0), get(1), get(2)]
    })
}

/// The mean power spectrum (`NOISE_FFT / 2 + 1` bins) of 4096-point Hann frames with half
/// overlap.
fn power_spectrum(signal: &[f64]) -> Vec<f64> {
    let bins = NOISE_FFT / 2 + 1;
    let mut sum = vec![0.0; bins];
    let window: Vec<f64> = (0..NOISE_FFT).map(|i| 0.5 - 0.5 * (TAU * i as f64 / NOISE_FFT as f64).cos()).collect();
    let fft = FftPlanner::<f64>::new().plan_fft_forward(NOISE_FFT);
    let mut frames = 0;
    let mut start = 0;
    while start + NOISE_FFT <= signal.len() {
        let mut buffer: Vec<Complex64> =
            signal.iter().skip(start).zip(&window).map(|(v, w)| Complex64::new(v * w, 0.0)).collect();
        buffer.resize(NOISE_FFT, Complex64::new(0.0, 0.0));
        fft.process(&mut buffer);
        for (s, b) in sum.iter_mut().zip(&buffer) {
            *s += b.norm_sqr();
        }
        frames += 1;
        start += NOISE_FFT / 2;
    }
    if frames > 0 {
        for s in &mut sum {
            *s /= f64::from(frames);
        }
    }
    sum
}

/// The side's noise: the mid's spectrum times the side-to-mid ratio smoothed in log frequency
/// (each bin the mean of the log ratio over 12 % of its index on each side, at least two bins).
fn side_shape(mid: &[f64], side: &[f64]) -> Vec<f64> {
    let peak = mid.iter().chain(side).fold(0.0_f64, |m, v| m.max(*v));
    let eps = 1e-30 + 1e-12 * peak;
    let ratio: Vec<f64> = mid.iter().zip(side).map(|(m, s)| ((s + eps) / (m + eps)).ln()).collect();
    let n = ratio.len();
    (0..n)
        .map(|i| {
            let w = ((0.12 * i as f64) as usize).max(2);
            let lo = i.saturating_sub(w);
            let hi = (i + w + 1).min(n);
            let part = ratio.get(lo..hi).unwrap_or(&[]);
            let smoothed = part.iter().sum::<f64>() / part.len().max(1) as f64;
            mid.get(i).copied().unwrap_or(0.0) * smoothed.exp()
        })
        .collect()
}

/// A power spectrum on the noise grid, in dB: the mean power of the bins within half a step of
/// each point (the nearest bin where none falls inside).
fn to_grid(spectrum: &[f64], sr: f64) -> [f32; NOISE_POINTS] {
    let bin_hz = sr / NOISE_FFT as f64;
    let peak = spectrum.iter().fold(0.0_f64, |m, v| m.max(*v)).max(1e-300);
    std::array::from_fn(|i| {
        let center = noise_point_hz(i);
        let (lo, hi) = (center * 2f64.powf(-1.0 / 24.0), center * 2f64.powf(1.0 / 24.0));
        let (first, last) = ((lo / bin_hz).ceil() as usize, (hi / bin_hz).floor() as usize);
        let values: Vec<f64> = (first..=last).filter_map(|k| spectrum.get(k).copied()).collect();
        let power = if values.is_empty() {
            spectrum.get((center / bin_hz).round() as usize).copied().unwrap_or(0.0)
        } else {
            values.iter().sum::<f64>() / values.len() as f64
        };
        // Relative to the peak, floored at -250 dB.
        (10.0 * (power / peak).max(1e-25).log10()) as f32
    })
}

/// The start of the recording's own release after the held part: the first 20 ms window (every
/// 10 ms) 6 dB below the mean of the 15 before it, taken 30 ms earlier, when the level then
/// falls below a tenth of the held level before the recording ends.
fn release_start(samples: &[f32], channels: usize, sample_rate: u32, join: usize) -> Option<usize> {
    let sr = f64::from(sample_rate);
    let frames = samples.len() / channels.max(1);
    let window = (0.02 * sr) as usize;
    let hop = ((0.01 * sr) as usize).max(1);
    let level = |start: usize| -> f64 {
        let mut sum = 0.0;
        for i in start..(start + window).min(frames) {
            let mut v = 0.0;
            for c in 0..channels {
                v += f64::from(samples.get(i * channels + c).copied().unwrap_or(0.0));
            }
            v /= channels as f64;
            sum += v * v;
        }
        10.0 * (sum / window.max(1) as f64 + 1e-14).log10()
    };
    let levels: Vec<f64> = (0..frames.saturating_sub(window) / hop).map(|k| level(k * hop)).collect();
    let first = join / hop;
    for k in first.max(15)..levels.len() {
        let before = levels.get(k - 15..k)?;
        let mean = before.iter().sum::<f64>() / 15.0;
        if levels.get(k).is_some_and(|l| *l < mean - 6.0) {
            let held = mean;
            let falls = levels.iter().skip(k).any(|l| *l < held - 20.0);
            if !falls {
                return None;
            }
            return Some(k.saturating_sub(3) * hop).filter(|s| *s >= join);
        }
    }
    None
}

/// The trial renders of the fitted model: the spread factors, the level gain and the model's
/// vibrato-locked level coefficients.
fn trial_renders(
    continuation: &mut Continuation,
    sr: f64,
    target: [f64; 3],
    mono: &[f64],
    segment: Range<usize>,
) -> Result<(), Error> {
    let prepared = Prepared::new(continuation, sr);
    let mut generator = Generator::new(&prepared, None);
    generator.set_raw(true);
    let mut model = NoteModel::default();
    model.transposed(continuation, 1.0, sr);
    // Spread: one render, each partial's level spread against the recording's.
    generator.start(&model, 0.0, 900);
    let length = (3.4 * sr) as usize;
    for _ in 0..length {
        generator.next();
    }
    for (k, partial) in continuation.partials.iter_mut().enumerate() {
        let rendered = generator.level_spread_db(k);
        let ratio = if rendered > 0.0 { f64::from(partial.sigma_db) / rendered } else { 1.0 };
        partial.spread = ratio.clamp(0.25, 1.0) as f32;
    }
    model.transposed(continuation, 1.0, sr);
    // Level: the recording's RMS just before the join against six renders'.
    let measure = |from: f64, until: f64, generator: &mut Generator, seed: u64| -> (f64, Vec<f64>, Vec<(f64, f64)>) {
        generator.start(&model, 0.0, seed);
        let total = (until * sr) as usize;
        let mut out = Vec::with_capacity(total);
        let mut pitch = Vec::with_capacity(total);
        for m in 0..total {
            out.push(f64::from(generator.next()[0]));
            pitch.push(generator.pitch_of(m as i64));
        }
        let start = (from * sr) as usize;
        let part = out.get(start..).unwrap_or(&[]);
        (20.0 * rms(part).max(1e-12).log10(), out, pitch)
    };
    let mut levels = 0.0;
    for k in 0..LEVEL_RENDERS {
        let (level, _, _) = measure(0.3, 2.5, &mut generator, 900 + k);
        levels += level;
    }
    let model_level = levels / LEVEL_RENDERS as f64;
    let before = (0.25 * sr) as usize;
    let end = segment.end.min(mono.len());
    let recording_level = 20.0 * rms(mono.get(end.saturating_sub(before)..end).unwrap_or(&[])).max(1e-12).log10();
    continuation.level_db = (recording_level - model_level).clamp(-60.0, 60.0) as f32;
    // The model's own vibrato-locked level, against which the recording's is set.
    let mut coefficients = [0.0; 3];
    for k in 0..COUPLING_RENDERS {
        let (_, out, pitch) = measure(0.0, 3.2, &mut generator, 21 + k);
        let from = (0.1 * sr) as usize;
        let until = ((2.9 * sr) as usize).min(out.len());
        let part = out.get(from..until).unwrap_or(&[]);
        let c = vibrato_level(part, sr, |n| pitch.get(n + from).copied().unwrap_or((0.0, 0.0)));
        for (sum, v) in coefficients.iter_mut().zip(c) {
            *sum += v / COUPLING_RENDERS as f64;
        }
    }
    continuation.coef_target = target.map(|v| v.clamp(-100.0, 100.0) as f32);
    continuation.coef_model = coefficients.map(|v| v.clamp(-100.0, 100.0) as f32);
    Ok(())
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len().max(1) as f64
}

fn standard_deviation(values: &[f64]) -> f64 {
    let m = mean(values);
    (values.iter().map(|v| (v - m).powi(2)).sum::<f64>() / values.len().max(1) as f64).sqrt()
}

fn rms(values: &[f64]) -> f64 {
    (values.iter().map(|v| v * v).sum::<f64>() / values.len().max(1) as f64).sqrt()
}

fn geometric_mean(values: &[f64]) -> f64 {
    let logs: Vec<f64> = values.iter().filter(|v| **v > 0.0).map(|v| v.ln()).collect();
    if logs.is_empty() { 0.0 } else { mean(&logs).exp() }
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    median_mut(&mut sorted)
}

fn median_mut(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    if n == 0 {
        return 0.0;
    }
    if n % 2 == 1 {
        values.get(n / 2).copied().unwrap_or(0.0)
    } else {
        (values.get(n / 2 - 1).copied().unwrap_or(0.0) + values.get(n / 2).copied().unwrap_or(0.0)) / 2.0
    }
}

#[cfg(test)]
#[path = "continuation_tests.rs"]
mod tests;

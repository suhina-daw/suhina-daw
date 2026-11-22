// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tempo detection for loops (`docs/spec/browser.md` › Tempo and key of a file): an onset
//! envelope of the sound, its autocorrelation, and the tempos at which the file is a whole
//! number of bars long. A tempo is given only when one of those stands out, so one-shots and
//! long recordings get none.

use crate::AudioData;

/// Seconds of a file analysed at most.
const MAX_SECONDS: f64 = 30.0;
/// Frames per step of the onset envelope at 44.1 kHz, about 5.8 ms.
const HOP_SECONDS: f64 = 256.0 / 44_100.0;
/// Bar counts a loop may have (in 4/4).
const BARS: [f64; 5] = [1.0, 2.0, 4.0, 8.0, 16.0];
/// The range of tempos taken.
const LOWEST: f64 = 60.0;
const HIGHEST: f64 = 200.0;
/// The share of the onsets' variance the beat's autocorrelation must reach.
const CONFIDENCE: f64 = 0.3;
/// The tempo preferred among equally likely ones, and how fast the preference falls (octaves).
const PREFERRED: f64 = 120.0;
const SPREAD: f64 = 0.6;

/// The tempo of a loop, or `None` when the file is not one (not a whole number of bars at a
/// tempo between 60 and 200 BPM) or its beat does not stand out.
pub fn detect(data: &AudioData) -> Option<f64> {
    let channels = usize::from(data.channels.max(1));
    let rate = f64::from(data.sample_rate.max(1));
    let duration = data.frames() as f64 / rate;
    if !(0.5..=MAX_SECONDS * 4.0).contains(&duration) {
        return None;
    }
    let candidates: Vec<f64> =
        BARS.iter().map(|bars| bars * 4.0 * 60.0 / duration).filter(|bpm| (LOWEST..=HIGHEST).contains(bpm)).collect();
    if candidates.is_empty() {
        return None;
    }

    // The onset envelope: the rise in energy from one hop to the next.
    let hop = ((HOP_SECONDS * rate).round() as usize).max(1);
    let frames = data.frames().min((MAX_SECONDS * rate) as usize);
    let mut energies = Vec::with_capacity(frames / hop + 1);
    let mut start = 0;
    while start + hop <= frames {
        let mut sum = 0.0f64;
        for frame in start..start + hop {
            let base = frame * channels;
            let mut mono = 0.0f64;
            for c in 0..channels {
                mono += f64::from(data.samples.get(base + c).copied().unwrap_or(0.0));
            }
            sum += mono * mono;
        }
        energies.push((sum / hop as f64 + 1e-10).ln());
        start += hop;
    }
    let rises: Vec<f64> = energies.windows(2).map(|w| (w[1] - w[0]).max(0.0)).collect();
    // Smoothed over five hops, so onsets that fall between hops still line up across beats.
    const KERNEL: [f64; 5] = [1.0, 2.0, 3.0, 2.0, 1.0];
    let onsets: Vec<f64> = (0..rises.len())
        .map(|i| {
            KERNEL
                .iter()
                .enumerate()
                .filter_map(|(k, w)| (i + k).checked_sub(2).and_then(|j| rises.get(j)).map(|r| r * w))
                .sum::<f64>()
                / 9.0
        })
        .collect();
    if onsets.len() < 16 || onsets.iter().all(|o| *o == 0.0) {
        return None;
    }
    let mean = onsets.iter().sum::<f64>() / onsets.len() as f64;
    let centered: Vec<f64> = onsets.iter().map(|o| o - mean).collect();

    let hop_seconds = hop as f64 / rate;
    let lag_of = |bpm: f64| 60.0 / bpm / hop_seconds;
    let autocorrelation = |lag: f64| -> f64 {
        // Linear interpolation between whole lags.
        let whole = lag.floor() as usize;
        let fraction = lag - lag.floor();
        let at = |l: usize| -> f64 {
            if l >= centered.len() {
                return 0.0;
            }
            let count = centered.len() - l;
            centered.iter().zip(centered.iter().skip(l)).map(|(a, b)| a * b).sum::<f64>() / count as f64
        };
        at(whole) * (1.0 - fraction) + at(whole + 1) * fraction
    };
    // The onsets' own variance: a beat must correlate with a good part of it.
    let variance = autocorrelation(0.0);
    if variance.is_nan() || variance <= 0.0 || (lag_of(LOWEST) as usize) >= centered.len() {
        return None;
    }
    // Onsets fall between hops, so a beat's peak spreads over neighbouring lags: the strongest
    // lag within a hop and a half counts.
    let peak = |lag: f64| (-3..=3).map(|step| autocorrelation(lag + f64::from(step) * 0.5)).fold(f64::MIN, f64::max);
    // Every multiple of the beat correlates as well as the beat on an even loop, so among the
    // candidates the one nearest a usual tempo is preferred: a weight falling off by octaves
    // from 120 BPM.
    let preference = |bpm: f64| {
        let octaves = (bpm / PREFERRED).log2() / SPREAD;
        (-0.5 * octaves * octaves).exp()
    };
    let (best, score) = candidates
        .iter()
        .map(|bpm| (*bpm, peak(lag_of(*bpm))))
        .max_by(|a, b| (a.1 * preference(a.0)).total_cmp(&(b.1 * preference(b.0))))?;
    (score / variance >= CONFIDENCE && best > 0.0).then_some(round_tempo(best))
}

/// Tempos are shown to two decimals; a loop's tempo is nearly always whole, so a value within
/// 0.05 of a whole number is taken as that.
fn round_tempo(bpm: f64) -> f64 {
    if (bpm - bpm.round()).abs() < 0.05 { bpm.round() } else { (bpm * 100.0).round() / 100.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    /// A loop of `bars` bars at `bpm`: a decaying click on every beat, louder on the bar.
    fn clicks(bpm: f64, bars: usize) -> AudioData {
        let beat = (60.0 / bpm * f64::from(RATE)) as usize;
        let frames = (bars as f64 * 4.0 * 60.0 / bpm * f64::from(RATE)) as usize;
        let mut samples = vec![0.0f32; frames * 2];
        let mut seed = 1u32;
        for b in 0..bars * 4 {
            let level = if b % 4 == 0 { 0.9 } else { 0.5 };
            for i in 0..2000 {
                let Some(frame) = (b * beat).checked_add(i).filter(|f| *f < frames) else { break };
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 9) as f32 / (1u32 << 23) as f32 * 2.0 - 1.0;
                let value = noise * level * (-(i as f32) / 300.0).exp();
                samples[frame * 2] = value;
                samples[frame * 2 + 1] = value;
            }
        }
        AudioData { sample_rate: RATE, channels: 2, samples }
    }

    #[test]
    fn a_click_loop_gives_its_tempo() {
        assert_eq!(detect(&clicks(120.0, 2)), Some(120.0));
        assert_eq!(detect(&clicks(125.0, 4)), Some(125.0));
        assert_eq!(detect(&clicks(87.0, 2)), Some(87.0));
        assert_eq!(detect(&clicks(160.0, 4)), Some(160.0));
    }

    #[test]
    fn one_shots_silence_and_noise_give_none() {
        // One hit, 0.3 s: no whole number of bars between 60 and 200 BPM.
        let mut hit = clicks(120.0, 1);
        hit.samples.truncate((0.3 * f64::from(RATE)) as usize * 2);
        assert_eq!(detect(&hit), None);
        let silence = AudioData { sample_rate: RATE, channels: 1, samples: vec![0.0; RATE as usize * 4] };
        assert_eq!(detect(&silence), None);
        // Steady noise four seconds long has no beat to stand out.
        let mut seed = 7u32;
        let noise: Vec<f32> = (0..RATE as usize * 4)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 9) as f32 / (1u32 << 23) as f32 - 1.0
            })
            .collect();
        assert_eq!(detect(&AudioData { sample_rate: RATE, channels: 1, samples: noise }), None);
        // Hostile values never panic.
        assert_eq!(detect(&AudioData { sample_rate: 0, channels: 0, samples: vec![f32::NAN; 1000] }), None);
        assert_eq!(detect(&AudioData { sample_rate: RATE, channels: 2, samples: Vec::new() }), None);
    }
}

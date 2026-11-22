// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where a sound's transients are: the frames at which its level jumps, for the slicing stretch
//! (`docs/decisions/0003-time-stretching.md` › Amendment: a sound of each mode). The level is
//! measured in short hops; a transient is a hop whose level rises well above how the level has
//! been rising around it, at least a gap after the one before, and is placed at the first frame
//! of the hop that reaches a quarter of the hop's peak, so a slice starts on the attack.

/// Seconds a hop of the level measurement lasts.
const HOP_SECONDS: f32 = 0.005;
/// Seconds over which the rises around a hop are averaged on each side, for the threshold.
const AROUND_SECONDS: f32 = 0.1;
/// A rise counts when it is this many times the average rise around it ...
const ABOVE_AVERAGE: f32 = 2.0;
/// ... and at least this much (natural log of the energy ratio: about 3.5 dB).
const LEAST_RISE: f32 = 0.8;
/// Hops quieter than this energy (about -60 dBFS) start no transient.
const QUIET: f32 = 1e-6;
/// Two transients are at least this far apart, in seconds.
const GAP_SECONDS: f32 = 0.04;
/// Most transients found in one file.
pub const MAX_TRANSIENTS: usize = 200_000;

/// The frames of `samples` (interleaved, `channels` of them, at `rate` frames a second) where a
/// transient starts, in order. Allocates; for a worker thread, not the audio thread.
pub fn transients(samples: &[f32], channels: usize, rate: f32) -> Vec<usize> {
    let channels = channels.max(1);
    let frames = samples.len() / channels;
    let hop = ((HOP_SECONDS * rate) as usize).max(16);
    let hops = frames / hop;
    if hops < 3 {
        return Vec::new();
    }
    // The energy of each hop: each channel's squared samples, averaged over the channels. The
    // channels are squared before they are combined, so channels in opposite phase do not cancel
    // into silence and hide their attacks (#1540). A sample that is not finite, or whose square is
    // not, counts as silence: one infinite hop made the running sum below infinite, and every
    // attack after it was measured against a NaN average and lost.
    let energy: Vec<f32> = (0..hops)
        .map(|h| {
            let from = h * hop * channels;
            let part = samples.get(from..from + hop * channels).unwrap_or_default();
            let sum: f32 = part.iter().map(|v| v * v).filter(|e| e.is_finite()).sum::<f32>() / channels as f32;
            let energy = sum / hop as f32;
            if energy.is_finite() { energy } else { 0.0 }
        })
        .collect();
    let rises: Vec<f32> = std::iter::once(0.0)
        .chain(energy.windows(2).map(|w| ((w[1] + 1e-12).ln() - (w[0] + 1e-12).ln()).max(0.0)))
        .collect();
    let around = ((AROUND_SECONDS * rate) as usize / hop).max(1);
    let gap = ((GAP_SECONDS * rate) as usize / hop).max(1);
    // A running sum of the rises, for each hop's average around it.
    let mut sums = Vec::with_capacity(rises.len() + 1);
    sums.push(0.0f64);
    for r in &rises {
        let last = sums.last().copied().unwrap_or(0.0);
        sums.push(last + f64::from(*r));
    }
    let mut found = Vec::new();
    let mut last: Option<usize> = None;
    for h in 1..hops {
        let rise = rises.get(h).copied().unwrap_or(0.0);
        let (from, to) = (h.saturating_sub(around), (h + around + 1).min(rises.len()));
        let mean =
            (sums.get(to).copied().unwrap_or(0.0) - sums.get(from).copied().unwrap_or(0.0)) / (to - from).max(1) as f64;
        let peak = rises.get(h + 1).is_none_or(|next| rise >= *next) && rise > rises.get(h - 1).copied().unwrap_or(0.0);
        let loud = energy.get(h).is_some_and(|e| *e > QUIET);
        if peak
            && loud
            && rise >= LEAST_RISE
            && f64::from(rise) >= mean * f64::from(ABOVE_AVERAGE)
            && last.is_none_or(|l| h >= l + gap)
        {
            last = Some(h);
            found.push(attack(samples, channels, h * hop, hop));
            if found.len() >= MAX_TRANSIENTS {
                break;
            }
        }
    }
    found
}

/// The first frame of the hop from `start` (`hop` frames) whose level reaches a quarter of the
/// hop's peak: where the attack begins.
fn attack(samples: &[f32], channels: usize, start: usize, hop: usize) -> usize {
    let level = |f: usize| {
        samples
            .get(f * channels..(f + 1) * channels)
            .map_or(0.0, |s| s.iter().map(|v| v.abs()).filter(|v| v.is_finite()).fold(0.0, f32::max))
    };
    let peak = (start..start + hop).map(level).fold(0.0, f32::max);
    (start..start + hop).find(|f| level(*f) >= peak * 0.25).unwrap_or(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    /// A decaying click every `every` seconds over `seconds`, with a little noise under it.
    fn clicks(every: f32, seconds: f32) -> Vec<f32> {
        let frames = (seconds * RATE) as usize;
        let period = (every * RATE) as usize;
        let mut noise = 12_345u32;
        (0..frames)
            .map(|i| {
                noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let hiss = (noise >> 9) as f32 / (1u32 << 23) as f32 - 0.5;
                let t = (i % period) as f32 / RATE;
                (t * 2_000.0).sin() * (-t * 40.0).exp() * 0.8 + hiss * 0.001
            })
            .collect()
    }

    #[test]
    fn clicks_are_found_where_they_start() {
        let found = transients(&clicks(0.25, 2.0), 1, RATE);
        let want: Vec<usize> = (1..8).map(|k| k * 12_000).collect();
        assert_eq!(found.len(), want.len(), "{found:?}");
        for (f, w) in found.iter().zip(&want) {
            assert!(f.abs_diff(*w) <= 48, "{f} for {w}");
        }
    }

    /// One sample that is not finite does not hide the attacks after it.
    #[test]
    fn an_invalid_sample_leaves_the_later_clicks_found() {
        for invalid in [f32::INFINITY, f32::NAN, f32::MAX] {
            let mut samples = clicks(0.25, 2.0);
            samples[1_000] = invalid;
            let found = transients(&samples, 1, RATE);
            let want: Vec<usize> = (1..8).map(|k| k * 12_000).collect();
            assert_eq!(found.len(), want.len(), "{invalid}: {found:?}");
            for (f, w) in found.iter().zip(&want) {
                assert!(f.abs_diff(*w) <= 48, "{invalid}: {f} for {w}");
            }
        }
    }

    #[test]
    fn a_steady_tone_and_silence_have_none() {
        let tone: Vec<f32> = (0..96_000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        assert!(transients(&tone, 1, RATE).is_empty());
        assert!(transients(&[0.0; 96_000], 1, RATE).is_empty());
        assert!(transients(&[], 2, RATE).is_empty());
    }

    #[test]
    fn stereo_is_mixed_and_odd_input_does_not_break_it() {
        let mono = clicks(0.5, 2.0);
        let stereo: Vec<f32> = mono.iter().flat_map(|s| [*s, *s]).collect();
        assert_eq!(transients(&stereo, 2, RATE), transients(&mono, 1, RATE));
        // Channels in opposite phase are as loud as in phase: their attacks are found the same.
        let opposite: Vec<f32> = mono.iter().flat_map(|s| [*s, -*s]).collect();
        assert_eq!(transients(&opposite, 2, RATE), transients(&mono, 1, RATE));
        let odd = [f32::NAN, 1.0, f32::INFINITY, -1.0];
        let _ = transients(&odd.repeat(10_000), 3, RATE);
    }
}

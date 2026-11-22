// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where a sound's notes start, for warp markers (`docs/spec/ui-audio-editor.md` › Transients):
//! the spectral flux of short windows (how much each frequency's level rose over the loudest of it
//! and its two neighbours two windows before, the rises summed: SuperFlux, so the wobble of a held
//! note's partials, as a piano's or a voice's vibrato, makes little), picked where it peaks well
//! above its median around and makes up a share of what sounds there. A candidate is a note when
//! the pitches sounding change across it (the powers of semitone bands, octaves told apart, as
//! pitch-based note detection reads them) or its level rises clearly; the wobble of a held chord
//! does neither. Unlike
//! [`transients`](crate::transients), which follows the level alone, it finds a note that starts
//! with no jump in level, as one note of a legato arpeggio or a sustained synth line does when its
//! pitch changes. Spectral flux onset detection is the textbook method (Bello et al., "A Tutorial
//! on Onset Detection in Music Signals", IEEE TASLP 2005; Dixon, "Onset Detection Revisited",
//! DAFx 2006); the maximum filter and the lag are SuperFlux's (Böck and Widmer, "Maximum Filter
//! Vibrato Suppression for Onset Detection", DAFx 2013).

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

/// Seconds a window lasts (to the nearest power of two of frames) and seconds between windows.
const WINDOW_SECONDS: f32 = 0.023;
const HOP_SECONDS: f32 = 0.005;
/// Windows back the flux compares with (SuperFlux's lag: 10 ms).
const LAG: usize = 2;
/// How much a frequency's level is compressed before rises are taken: `ln(1 + LOG_GAIN * level)`,
/// a level in sample units, so a quiet hiss under a sound makes almost no flux.
const LOG_GAIN: f32 = 100.0;
/// Windows on each side whose flux's median is the threshold's base (the median, not the mean, so
/// the notes either side of one in a fast run do not raise it).
const AROUND: usize = 20;
/// An onset's flux is at least this many times the median around it ...
const ABOVE_MEDIAN: f32 = 1.5;
/// ... and at least this share of the levels sounding in its window, so a held sound that only
/// swells starts none, however loud or quiet the file is. Low, so every note of a soft legato line
/// is a candidate; the pitch and level checks below sort them.
const LEAST_SHARE: f32 = 0.1;
/// A candidate is a note when the pitches sounding change: the cosine distance between the
/// semitone bands' powers (octaves told apart) 45 ms before it and 45 ms after it is at least
/// this ...
const PITCH_CHANGE: f32 = 0.3;
/// ... or when its level rises at least this many dB from the quietest 5 ms in the 60 ms before
/// it to the loudest 5 ms in the 30 ms after it (a drum, a chord struck again). A held note's
/// wobble moves neither: on a piano chord part measured with two legato arpeggios, 57 candidates
/// became the part's 4 chords and 1 more, while the arpeggios kept 50 of 57 and 28 of 32 notes.
const LEAST_RISE_DB: f32 = 11.0;
/// Seconds the pitch is measured over on each side of a candidate (to the nearest power of two of
/// frames), and how far from it each side's window is centered.
const PITCH_WINDOW_SECONDS: f32 = 0.046;
const PITCH_OFFSET_SECONDS: f32 = 0.045;
/// The frequencies the semitone bands cover, in Hz.
const PITCH_LOW: f32 = 80.0;
const PITCH_HIGH: f32 = 5_000.0;
/// Semitone bands from four octaves below A440 up: 96 reach past 5 kHz.
const BANDS: usize = 96;
/// Two notes are at least this far apart, in seconds; of two closer, the clearer stays.
const NOTE_GAP_SECONDS: f32 = 0.1;
/// Windows quieter than this mean square (about -60 dBFS) start no onset.
const QUIET: f32 = 1e-6;
/// Two onsets are at least this far apart, in seconds; of two closer, the stronger stays.
const GAP_SECONDS: f32 = 0.07;
/// Most onsets found in one file.
pub const MAX_ONSETS: usize = 200_000;

/// The frames of `samples` (interleaved, `channels` of them, at `rate` frames a second) where a
/// note starts, in order. Each channel's spectrum is taken on its own and their levels added, so
/// channels in opposite phase do not cancel. A sharp attack is placed where its level first
/// reaches a quarter of its peak in the window that found it; a note that starts without one, at
/// the middle of that window (within a window's length: [`transients`](crate::transients) places
/// an attack to the millisecond). Allocates; for a worker thread, not the audio thread.
pub fn note_onsets(samples: &[f32], channels: usize, rate: f32) -> Vec<usize> {
    let channels = channels.max(1);
    let frames = samples.len() / channels;
    if !(rate.is_finite() && rate > 0.0) {
        return Vec::new();
    }
    let wanted = ((WINDOW_SECONDS * rate) as usize).clamp(64, 1 << 15);
    let above = wanted.next_power_of_two();
    let size = if above - wanted > wanted - above / 2 { above / 2 } else { above };
    let hop = ((HOP_SECONDS * rate) as usize).max(16);
    if frames < size + 2 * hop {
        return Vec::new();
    }
    let bins = size / 2 - 1;
    let window: Vec<f32> =
        (0..size).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / size as f32).cos()).collect();
    let fft = FftPlanner::<f32>::new().plan_fft_forward(size);
    let mut buffer = vec![Complex::new(0.0f32, 0.0); size];
    let mut levels = vec![0.0f32; bins];
    // The last `LAG` windows' levels, the oldest first, and the oldest's maximum over each bin
    // and its two neighbours.
    let mut history: std::collections::VecDeque<Vec<f32>> = std::collections::VecDeque::with_capacity(LAG + 1);
    let mut widened = vec![0.0f32; bins];
    let sample = |frame: usize, channel: usize| {
        samples.get(frame * channels + channel).copied().filter(|v| v.is_finite()).unwrap_or(0.0)
    };
    let count = (frames - size) / hop + 1;
    let mut flux = Vec::with_capacity(count);
    let mut sounding = Vec::with_capacity(count);
    let mut loud = Vec::with_capacity(count);
    for index in 0..count {
        let start = index * hop;
        levels.iter_mut().for_each(|l| *l = 0.0);
        let mut energy = 0.0f32;
        for channel in 0..channels {
            for (i, slot) in buffer.iter_mut().enumerate() {
                let v = sample(start + i, channel);
                energy += v * v;
                *slot = Complex::new(v * window.get(i).copied().unwrap_or(0.0), 0.0);
            }
            fft.process(&mut buffer);
            for (level, bin) in levels.iter_mut().zip(buffer.iter().skip(1)) {
                *level += bin.norm() * 2.0 / size as f32;
            }
        }
        for level in levels.iter_mut() {
            *level = (LOG_GAIN * *level).ln_1p();
        }
        let rise: f32 = match history.front().filter(|_| history.len() >= LAG) {
            Some(then) => {
                for (k, slot) in widened.iter_mut().enumerate() {
                    let near = then.get(k.saturating_sub(1)..(k + 2).min(bins)).unwrap_or_default();
                    *slot = near.iter().copied().fold(0.0, f32::max);
                }
                levels.iter().zip(&widened).map(|(now, then)| (now - then).max(0.0)).sum()
            }
            None => 0.0,
        };
        flux.push(rise);
        sounding.push(levels.iter().sum::<f32>());
        loud.push(energy / (size * channels) as f32 > QUIET);
        // The oldest window's buffer is reused for the next one's levels.
        let reused = if history.len() >= LAG { history.pop_front() } else { None };
        history.push_back(std::mem::replace(&mut levels, reused.unwrap_or_else(|| vec![0.0f32; bins])));
    }
    let gap = ((GAP_SECONDS * rate) as usize / hop).max(1);
    // (window index, flux) of the onsets kept so far.
    let mut picked: Vec<(usize, f32)> = Vec::new();
    let mut around: Vec<f32> = Vec::with_capacity(2 * AROUND + 1);
    for i in 1..count.saturating_sub(1) {
        let (Some(&now), Some(&last), Some(&next)) = (flux.get(i), flux.get(i - 1), flux.get(i + 1)) else { continue };
        let peak = now >= last && now > next;
        let least = LEAST_SHARE * sounding.get(i).copied().unwrap_or(0.0);
        if !(peak && now > least && loud.get(i).copied().unwrap_or(false)) {
            continue;
        }
        let (from, to) = (i.saturating_sub(AROUND), (i + AROUND + 1).min(count));
        around.clear();
        around.extend(flux.get(from..to).unwrap_or_default());
        around.sort_unstable_by(f32::total_cmp);
        let median = around.get(around.len() / 2).copied().unwrap_or(0.0);
        if now <= median * ABOVE_MEDIAN {
            continue;
        }
        match picked.last_mut() {
            Some(kept) if i < kept.0 + gap => {
                if now > kept.1 {
                    *kept = (i, now);
                }
            }
            _ => {
                if picked.len() >= MAX_ONSETS {
                    break;
                }
                picked.push((i, now));
            }
        }
    }
    // The candidates that are notes: a change of pitch, or a clear rise of level.
    let pitch = Pitch::new(rate);
    let note_gap = (NOTE_GAP_SECONDS * rate) as usize;
    let mut notes: Vec<(usize, f32)> = Vec::new();
    for (i, _) in picked {
        let at = i * hop;
        let change = pitch.change(samples, channels, frames, at, rate);
        let rise = rise_db(samples, channels, frames, at, rate);
        if change < PITCH_CHANGE && rise < LEAST_RISE_DB {
            continue;
        }
        let clear = change + rise / 20.0;
        match notes.last_mut() {
            Some(kept) if at < kept.0 + note_gap => {
                if clear > kept.1 {
                    *kept = (at, clear);
                }
            }
            _ => notes.push((at, clear)),
        }
    }
    notes.into_iter().map(|(at, _)| placed(samples, channels, frames, at, size)).collect()
}

/// The powers of `samples`' semitone bands around a frame, for telling whether the pitches
/// sounding change there.
struct Pitch {
    size: usize,
    window: Vec<f32>,
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    /// The semitone band of each FFT bin, if it is in the bands' range.
    band_of: Vec<Option<usize>>,
}

impl Pitch {
    fn new(rate: f32) -> Pitch {
        let wanted = ((PITCH_WINDOW_SECONDS * rate) as usize).clamp(256, 1 << 15);
        let above = wanted.next_power_of_two();
        let size = if above - wanted > wanted - above / 2 { above / 2 } else { above };
        let window =
            (0..size).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / size as f32).cos()).collect();
        let band_of = (0..size / 2)
            .map(|k| {
                let hz = k as f32 * rate / size as f32;
                if !(PITCH_LOW..=PITCH_HIGH).contains(&hz) {
                    return None;
                }
                let band = (12.0 * (hz / 440.0).log2()).round() as i64 + 48;
                usize::try_from(band).ok().filter(|b| *b < BANDS)
            })
            .collect();
        Pitch { size, window, fft: FftPlanner::<f32>::new().plan_fft_forward(size), band_of }
    }

    /// The bands' powers in the window centered on frame `center`, every channel's added, scaled
    /// to length 1; `None` where the window does not fit in the file.
    fn bands(&self, samples: &[f32], channels: usize, frames: usize, center: usize) -> Option<Vec<f32>> {
        let start = center.checked_sub(self.size / 2)?;
        if start + self.size > frames {
            return None;
        }
        let mut powers = vec![0.0f32; BANDS];
        let mut buffer = vec![Complex::new(0.0f32, 0.0); self.size];
        for channel in 0..channels {
            for (i, slot) in buffer.iter_mut().enumerate() {
                let v = samples.get((start + i) * channels + channel).copied().filter(|v| v.is_finite()).unwrap_or(0.0);
                *slot = Complex::new(v * self.window.get(i).copied().unwrap_or(0.0), 0.0);
            }
            self.fft.process(&mut buffer);
            for (bin, band) in buffer.iter().zip(&self.band_of) {
                if let Some(power) = band.and_then(|b| powers.get_mut(b)) {
                    *power += bin.norm_sqr();
                }
            }
        }
        let length = powers.iter().map(|p| p * p).sum::<f32>().sqrt();
        if length > 0.0 {
            powers.iter_mut().for_each(|p| *p /= length);
        }
        Some(powers)
    }

    /// How much the pitches sounding change at frame `at`: the cosine distance between the bands
    /// before and after it, 0 for the same, 1 for none in common (or where a side is outside the
    /// file).
    fn change(&self, samples: &[f32], channels: usize, frames: usize, at: usize, rate: f32) -> f32 {
        let offset = (PITCH_OFFSET_SECONDS * rate) as usize;
        let before = at.checked_sub(offset).and_then(|c| self.bands(samples, channels, frames, c));
        let after = self.bands(samples, channels, frames, at + offset);
        match (before, after) {
            (Some(a), Some(b)) => 1.0 - a.iter().zip(&b).map(|(p, q)| p * q).sum::<f32>(),
            _ => 1.0,
        }
    }
}

/// How many dB the level rises at frame `at`: the loudest 5 ms in the 30 ms after it over the
/// quietest 5 ms in the 60 ms before it.
fn rise_db(samples: &[f32], channels: usize, frames: usize, at: usize, rate: f32) -> f32 {
    let step = ((0.005 * rate) as usize).max(1);
    let power = |from: usize| {
        let (from, to) = (from.min(frames), (from + step).min(frames));
        let part = samples.get(from * channels..to * channels).unwrap_or_default();
        part.iter().filter(|v| v.is_finite()).map(|v| v * v).sum::<f32>() / part.len().max(1) as f32 + 1e-12
    };
    let before = (0..12).map(|k| power((at + k * step).saturating_sub(12 * step))).fold(f32::INFINITY, f32::min);
    let after = (0..6).map(|k| power(at + k * step)).fold(0.0, f32::max);
    10.0 * (after / before).log10()
}

/// Where an onset found in the window of `size` frames from `start` begins: the first frame whose
/// level reaches a quarter of the window's peak when its first quarter is quieter than that (a
/// sharp attack), else the window's middle.
fn placed(samples: &[f32], channels: usize, frames: usize, start: usize, size: usize) -> usize {
    let level = |f: usize| {
        samples
            .get(f * channels..(f + 1) * channels)
            .map_or(0.0, |s| s.iter().filter(|v| v.is_finite()).map(|v| v.abs()).fold(0.0, f32::max))
    };
    let (from, to) = (start.min(frames), (start + size).min(frames));
    let middle = (start + size / 2).min(frames);
    let peak = (from..to).map(level).fold(0.0, f32::max);
    let quiet_start = (from..(from + size / 4).min(to)).map(level).fold(0.0, f32::max) < peak * 0.25;
    if quiet_start { (from..to).find(|f| level(*f) >= peak * 0.25).unwrap_or(middle) } else { middle }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    /// A decaying click every `every` seconds over `seconds`, with a little noise under it.
    fn clicks(every: f32, seconds: f32) -> Vec<f32> {
        let period = (every * RATE) as usize;
        let mut noise = 12_345u32;
        (0..(seconds * RATE) as usize)
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
        let found = note_onsets(&clicks(0.25, 2.0), 1, RATE);
        let want: Vec<usize> = (1..8).map(|k| k * 12_000).collect();
        assert_eq!(found.len(), want.len(), "{found:?}");
        for (f, w) in found.iter().zip(&want) {
            // Within a window's length: `transients` places an attack to the millisecond.
            assert!(f.abs_diff(*w) <= 1_024, "{f} for {w}");
        }
    }

    /// A tone at one level that changes pitch every 160 ms, its phase running on: no jump in
    /// level, yet each new pitch is a new note.
    #[test]
    fn a_new_pitch_at_the_same_level_is_a_note() {
        let pitches = [440.0f32, 554.4, 659.3, 880.0, 659.3, 554.4, 440.0, 330.0, 440.0, 554.4, 659.3, 880.0];
        let note = (0.16 * RATE) as usize;
        let mut phase = 0.0f32;
        let tone: Vec<f32> = (0..note * pitches.len())
            .map(|i| {
                let pitch = pitches.get(i / note).copied().unwrap_or(440.0);
                phase += 2.0 * std::f32::consts::PI * pitch / RATE;
                phase.sin() * 0.3
            })
            .collect();
        let found = note_onsets(&tone, 1, RATE);
        let missed: Vec<usize> =
            (1..pitches.len()).map(|k| k * note).filter(|at| !found.iter().any(|f| f.abs_diff(*at) <= 960)).collect();
        assert!(missed.len() <= 1, "missed {missed:?} of {found:?}");
        assert!(found.len() <= pitches.len(), "no more than one a note: {found:?}");
    }

    /// A held chord whose partials wobble in level by up to 15 % every 120 ms (as a piano's do
    /// while it rings), then another chord: one note, where the chord changes.
    #[test]
    fn a_held_chords_wobble_is_no_note_and_a_new_chord_is() {
        let chords = [[220.0f32, 277.2, 329.6], [246.9, 311.1, 370.0]];
        let mut seed = 7u32;
        let mut wobble = [1.0f32; 3];
        let step = (0.12 * RATE) as usize;
        let audio: Vec<f32> = (0..(2.0 * RATE) as usize)
            .map(|i| {
                if i % step == 0 {
                    for w in wobble.iter_mut() {
                        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        *w = 0.85 + 0.3 * (seed >> 8) as f32 / (1u32 << 24) as f32;
                    }
                }
                let chord = chords.get(i / RATE as usize).copied().unwrap_or(chords[0]);
                let t = i as f32 / RATE;
                chord.iter().zip(&wobble).map(|(hz, w)| (t * 2.0 * std::f32::consts::PI * hz).sin() * 0.15 * w).sum()
            })
            .collect();
        let found = note_onsets(&audio, 1, RATE);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found.iter().all(|f| f.abs_diff(RATE as usize) <= 2_400), "{found:?}");
    }

    #[test]
    fn a_steady_or_swelling_tone_and_silence_have_none() {
        let tone: Vec<f32> = (0..96_000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        assert!(note_onsets(&tone, 1, RATE).is_empty());
        // A tenth louder half way (under 1 dB): a swell, not a note.
        let swell: Vec<f32> =
            (0..96_000).map(|i| (i as f32 * 0.05).sin() * if i < 48_000 { 0.4 } else { 0.44 }).collect();
        assert!(note_onsets(&swell, 1, RATE).is_empty(), "{:?}", note_onsets(&swell, 1, RATE));
        assert!(note_onsets(&[0.0; 96_000], 1, RATE).is_empty());
        assert!(note_onsets(&[], 2, RATE).is_empty());
    }

    #[test]
    fn stereo_and_opposite_phase_find_the_same_and_odd_input_does_not_break_it() {
        let mono = clicks(0.5, 2.0);
        let stereo: Vec<f32> = mono.iter().flat_map(|s| [*s, *s]).collect();
        assert_eq!(note_onsets(&stereo, 2, RATE), note_onsets(&mono, 1, RATE));
        let opposite: Vec<f32> = mono.iter().flat_map(|s| [*s, -*s]).collect();
        assert_eq!(note_onsets(&opposite, 2, RATE), note_onsets(&mono, 1, RATE));
        let odd = [f32::NAN, 1.0, f32::INFINITY, -1.0];
        assert!(note_onsets(&odd, 2, RATE).is_empty());
        assert!(note_onsets(&mono, 0, RATE).len() == note_onsets(&mono, 1, RATE).len());
        assert!(note_onsets(&mono, 1, 0.0).is_empty() && note_onsets(&mono, 1, f32::NAN).is_empty());
    }
}

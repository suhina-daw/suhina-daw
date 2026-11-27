// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure, bounded PCM analysis. Hosts own files, jobs, permissions and explicit adoption.
//! None of this preparation is called from audio rendering.

use std::ops::Range;

use suhina_dsp::{Resampler, pitch::Yin};

pub mod compare;
pub mod continuation;
mod key;
pub mod modal;
mod notes;
mod sustain;

pub use key::{KeyCandidate, KeyMode, KeyProposal, MIN_KEY_CORRELATION};
pub use notes::{Glide, HeldNote};

pub const VERSION: u32 = 3;
/// A held note with at least this share of the voiced energy is the sample's note when the
/// windows find no stable pitch (a glide into it, an accent before it, a reverberant tail).
const DOMINANT_SHARE: f32 = 0.6;
/// A dominant note agrees with a channel whose median pitch lies within this many semitones of
/// it or of its octave.
const NOTE_AGREEMENT: f32 = 0.5;
pub const RATE: u32 = 22_050;
pub const MAX_SECONDS: usize = 8;
pub const MAX_WINDOWS: usize = 32;
pub const MIN_HZ: f32 = 40.0;
pub const MAX_HZ: f32 = 4_000.0;
const FRAME: usize = 2048;
const HOP: usize = 1024;
const CHUNK: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidFormat,
    InvalidRange,
    NonFinite,
    Cancelled,
    InvalidSettings,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidFormat => "analysis needs mono/stereo PCM at 1000..384000 Hz with complete frames",
            Self::InvalidRange => "analysis needs a nonempty frame range inside the source",
            Self::NonFinite => "the analyzed audio contains a nonfinite sample",
            Self::Cancelled => "analysis was cancelled",
            Self::InvalidSettings => "analysis settings are outside the supported bounds",
        })
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Stable,
    Silent,
    TooShort,
    NoStablePitch,
    /// Several notes without one stable pitch, with a clear key: `Report::key` proposes its tonic.
    TonalCenter,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Proposal {
    pub root: u8,
    pub midi: f32,
    pub hz: f32,
    pub cents: f32,
    pub periodicity: f32,
    /// Interdecile pitch spread, in cents; not a correctness probability.
    pub spread_cents: f32,
}

/// Periodicity measurements under a single isolated pitched-source assumption. This report
/// does not establish that the recording is monophonic: a harmonic mixture can have exactly
/// the same waveform as a tone with a missing fundamental. Hosts must present that limitation.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub version: u32,
    pub range: Range<usize>,
    pub outcome: Outcome,
    pub proposal: Option<Proposal>,
    /// Only when no single-note proposal exists: the material's tonal center.
    pub key: Option<KeyProposal>,
    pub active_windows: usize,
    pub voiced_windows: usize,
    /// The held notes of the analyzed material, in time order (a phrase has several).
    pub notes: Vec<HeldNote>,
    /// The voiced material moves into the proposed note from another pitch.
    pub glide: Option<Glide>,
}

/// Analyze a bounded prefix of `range`. The progress callback receives completed work and
/// its fixed upper bound; returning false cooperatively cancels even during estimator lags.
/// Scratch storage is bounded independently of source duration. PCM is only borrowed.
pub fn analyze(
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    range: Range<usize>,
    mut progress: impl FnMut(usize, usize) -> bool,
) -> Result<Report, Error> {
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
    let mut report = Report {
        version: VERSION,
        range: range.clone(),
        outcome: Outcome::NoStablePitch,
        proposal: None,
        key: None,
        active_windows: 0,
        voiced_windows: 0,
        notes: Vec::new(),
        glide: None,
    };
    // The complete borrowed source can be large. Only the admitted range is inspected.
    let first = range.start.checked_mul(channels).ok_or(Error::InvalidRange)?;
    let last = range.end.checked_mul(channels).ok_or(Error::InvalidRange)?;
    let input = samples.get(first..last).ok_or(Error::InvalidRange)?;
    let chunks = length.div_ceil(CHUNK);
    let total = chunks * (1 + channels) + channels * (MAX_WINDOWS + key::MAX_KEY_FRAMES) + notes::MAX_NOTE_FRAMES;
    let mut done = 0;
    let mut peak = 0.0f64;
    let mut channel_min = [f32::INFINITY; 2];
    let mut channel_max = [f32::NEG_INFINITY; 2];
    for chunk in input.chunks(CHUNK * channels) {
        if !progress(done, total) {
            return Err(Error::Cancelled);
        }
        for (index, &x) in chunk.iter().enumerate() {
            if !x.is_finite() {
                return Err(Error::NonFinite);
            }
            peak = peak.max(f64::from(x).abs());
            let channel = index % channels;
            if let (Some(low), Some(high)) = (channel_min.get_mut(channel), channel_max.get_mut(channel)) {
                *low = low.min(x);
                *high = high.max(x);
            }
        }
        done += 1;
    }
    if peak <= 1e-12 || (0..channels).all(|c| channel_min.get(c) == channel_max.get(c)) {
        report.outcome = Outcome::Silent;
        return Ok(report);
    }
    let mut classes = key::PitchClasses::new(channels);
    // (fractional MIDI note, periodicity) of each voiced window.
    let mut windows: Vec<(f32, f32)> = Vec::with_capacity(MAX_WINDOWS * channels);
    let (mut voiced_energy, mut active_energy) = (0.0f64, 0.0f64);
    // The louder channel, kept for the note track.
    let mut louder: (f64, Vec<f32>) = (0.0, Vec::new());
    let mut channel_medians = Vec::with_capacity(channels);
    let mut any_active = false;
    for channel in 0..channels {
        let mut resampler = Resampler::new(f64::from(sample_rate), f64::from(RATE));
        let output_capacity = (CHUNK as f64 * f64::from(RATE) / f64::from(sample_rate)).ceil() as usize + 1024;
        let mut output = vec![0.0; output_capacity];
        let mut block = [0.0; CHUNK];
        let mono_capacity = (length as f64 * f64::from(RATE) / f64::from(sample_rate)).ceil() as usize + 1024;
        let mut mono = Vec::with_capacity(mono_capacity);
        for chunk in input.chunks(CHUNK * channels) {
            if !progress(done, total) {
                return Err(Error::Cancelled);
            }
            let frames = chunk.len() / channels;
            for (frame, x) in chunk.chunks_exact(channels).zip(block.iter_mut()) {
                *x = f64::from(frame.get(channel).copied().unwrap_or(0.0)) / peak;
            }
            let count = resampler.process(block.get(..frames).ok_or(Error::InvalidRange)?, &mut output);
            mono.extend(output.iter().take(count).map(|x| *x as f32));
            done += 1;
        }
        let count = resampler.flush(&mut output);
        mono.extend(output.iter().take(count).map(|x| *x as f32));
        // Flush may cover the rounded end; no extra duration is treated as recorded audio.
        let actual = (length as f64 * f64::from(RATE) / f64::from(sample_rate)).floor() as usize;
        mono.truncate(actual);
        let available = mono.len().saturating_sub(FRAME) / HOP + 1;
        if mono.len() < FRAME || available < 3 {
            report.outcome = Outcome::TooShort;
            return Ok(report);
        }
        let used = classes
            .add_channel(&mono, || {
                let keep = progress(done, total);
                done += 1;
                keep
            })
            .ok_or(Error::Cancelled)?;
        done += key::MAX_KEY_FRAMES - used;
        let count = available.min(MAX_WINDOWS);
        let starts: Vec<usize> = (0..count).map(|i| i * (mono.len() - FRAME) / (count - 1)).collect();
        let mut energies = Vec::with_capacity(count);
        for &start in &starts {
            let frame = mono.get(start..start + FRAME).ok_or(Error::InvalidRange)?;
            let mean = frame.iter().map(|x| f64::from(*x)).sum::<f64>() / FRAME as f64;
            energies.push(frame.iter().map(|x| (f64::from(*x) - mean).powi(2)).sum::<f64>() / FRAME as f64);
        }
        let loudest = energies.iter().copied().fold(0.0f64, f64::max);
        if loudest < 1e-12 {
            done += MAX_WINDOWS;
            continue;
        }
        any_active = true;
        let top = MAX_HZ.min(sample_rate as f32 * 0.45);
        let Some(mut yin) = Yin::new(RATE as f32, FRAME, MIN_HZ, top, 0.15) else {
            return Err(Error::InvalidFormat);
        };
        let mut frame = [0.0f32; FRAME];
        let mut channel_notes = Vec::with_capacity(count);
        for (&start, energy) in starts.iter().zip(energies) {
            if !progress(done, total) {
                return Err(Error::Cancelled);
            }
            if energy >= loudest * 1e-4 && energy >= 1e-12 {
                report.active_windows += 1;
                active_energy += energy;
                let input = mono.get(start..start + FRAME).ok_or(Error::InvalidRange)?;
                let mean = input.iter().map(|x| f64::from(*x)).sum::<f64>() / FRAME as f64;
                let scale = input.iter().map(|x| (f64::from(*x) - mean).abs()).fold(0.0f64, f64::max);
                for (to, from) in frame.iter_mut().zip(input) {
                    *to = ((f64::from(*from) - mean) / scale.max(1e-12)) as f32;
                }
                // A restricted period search can mistake an out-of-band tone's repeated
                // cycles for a lower fundamental. Treat a very high crossing rate as
                // uncertain instead; this deliberately also rejects some bright mixtures.
                let crossings =
                    frame.windows(2).filter(|pair| matches!(pair, [a, b] if (*a < 0.0) != (*b < 0.0))).count();
                let crossing_hz = crossings as f32 * RATE as f32 / (2.0 * (FRAME - 1) as f32);
                if crossing_hz > top * 1.01 {
                    done += 1;
                    continue;
                }
                let mut cancelled = false;
                let estimate = yin.estimate_while(&frame, || {
                    let keep = progress(done, total);
                    cancelled |= !keep;
                    keep
                });
                if cancelled {
                    return Err(Error::Cancelled);
                }
                if let Some(pitch) = estimate.filter(|p| (MIN_HZ..=top).contains(&p.hz)) {
                    windows.push((pitch.midi_note(), pitch.periodicity));
                    channel_notes.push(pitch.midi_note());
                    report.voiced_windows += 1;
                    voiced_energy += energy;
                }
            }
            done += 1;
        }
        done += MAX_WINDOWS - count;
        if !channel_notes.is_empty() {
            channel_notes.sort_by(f32::total_cmp);
            channel_medians.push(channel_notes.get(channel_notes.len() / 2).copied().unwrap_or(0.0));
        } else {
            // A loud, unpitched channel cannot be hidden behind the other channel's tone.
            channel_medians.push(f32::NAN);
        }
        let channel_energy: f64 = mono.iter().map(|x| f64::from(*x).powi(2)).sum();
        if channel_energy > louder.0 {
            louder = (channel_energy, mono);
        }
    }
    // The note track of the louder channel: held notes, glides and phrases.
    let mut frames = notes::track(&louder.1, sample_rate, |advance| {
        if advance {
            done += 1;
        }
        progress(done, total)
    })?;
    notes::fold_octaves(&mut frames);
    report.notes = notes::held_notes(&frames);
    if !progress(total, total) {
        return Err(Error::Cancelled);
    }
    if !any_active {
        report.outcome = Outcome::Silent;
        return Ok(report);
    }
    // A dominant note must agree with every channel's median pitch (or its octave), as a stable
    // proposal must: two channels at different pitches propose nothing.
    let agrees = |note: &HeldNote| {
        channel_medians.iter().all(|m| {
            let distance = (m - note.midi).abs();
            m.is_finite() && distance.min((distance - 12.0).abs()) <= NOTE_AGREEMENT
        })
    };
    let dominant = report
        .notes
        .iter()
        .copied()
        .filter(|n| n.share >= DOMINANT_SHARE && agrees(n))
        .max_by(|a, b| a.share.total_cmp(&b.share));
    let voicing = Voicing {
        active_windows: report.active_windows,
        voiced_windows: report.voiced_windows,
        active_energy,
        voiced_energy,
    };
    if let Some(proposal) = stable(&mut windows, voicing, &channel_medians) {
        report.outcome = Outcome::Stable;
        report.proposal = Some(proposal);
        // The held note that is this pitch, for a glide into it.
        let held = report.notes.iter().filter(|n| n.root == proposal.root).max_by(|a, b| a.share.total_cmp(&b.share));
        report.glide = held.and_then(|n| notes::glide(&frames, n));
    } else if let Some(note) = dominant {
        // A glide into one note, an accent before it or a reverberant tail after it: the note
        // that holds most of the voiced energy.
        report.outcome = Outcome::Stable;
        report.proposal = Some(Proposal {
            root: note.root,
            midi: note.midi,
            hz: 440.0 * 2.0f32.powf((note.midi - 69.0) / 12.0),
            cents: (note.midi - f32::from(note.root)) * 100.0,
            periodicity: note.periodicity,
            spread_cents: note.spread_cents,
        });
        report.glide = notes::glide(&frames, &note);
    } else if let Some(key) = classes.key() {
        report.outcome = Outcome::TonalCenter;
        report.key = Some(key);
    }
    Ok(report)
}

/// How much of the analyzed windows is pitched.
#[derive(Clone, Copy, Debug)]
struct Voicing {
    active_windows: usize,
    voiced_windows: usize,
    active_energy: f64,
    voiced_energy: f64,
}

/// One stable pitch across the voiced windows and channels, or None. `windows` holds each voiced
/// window's (fractional MIDI note, periodicity).
fn stable(windows: &mut [(f32, f32)], voicing: Voicing, channel_medians: &[f32]) -> Option<Proposal> {
    // Most of the sound must be pitched: three quarters of the active windows, or of their
    // energy, so neither a quiet reverberant tail nor a loud unpitched knock outvotes the note
    // (version 3; version 2 counted windows only).
    let by_count = voicing.voiced_windows * 4 >= voicing.active_windows * 3;
    let by_energy = voicing.voiced_energy * 4.0 >= voicing.active_energy * 3.0;
    if windows.len() < 3 || !(by_count || by_energy) {
        return None;
    }
    windows.sort_by(|a, b| a.0.total_cmp(&b.0));
    let notes: Vec<f32> = windows.iter().map(|w| w.0).collect();
    let periodicity = windows.iter().map(|w| w.1).sum::<f32>() / windows.len() as f32;
    let middle = notes.get(notes.len() / 2).copied()?;
    let lower = notes.get((notes.len() - 1) / 10).copied().unwrap_or(middle);
    let upper = notes.get((notes.len() - 1) * 9 / 10).copied().unwrap_or(middle);
    let spread_cents = (upper - lower) * 100.0;
    // Vibrato swings around one note; a melody, trill or glide leaves it. Three quarters of the
    // voiced windows must lie within 50 cents of the median (version 1 demanded a 35-cent spread,
    // which a cello's or a voice's vibrato exceeds).
    let near = |center: f32| windows.iter().filter(|w| (w.0 - center).abs() <= 0.5).copied().collect::<Vec<_>>();
    let class = near(middle);
    let class_median = |class: &[(f32, f32)]| {
        let mut values: Vec<f32> = class.iter().map(|w| w.0).collect();
        values.sort_by(f32::total_cmp);
        values.get(values.len() / 2).copied()
    };
    let mean_periodicity = |class: &[(f32, f32)]| class.iter().map(|w| w.1).sum::<f32>() / class.len().max(1) as f32;
    let (midi, folded) = if class.len() * 4 >= windows.len() * 3 {
        (middle, false)
    } else {
        // YIN's octave errors: the windows an octave from the median and the median's are one
        // note together, at the octave YIN found clearly more periodic (version 3).
        let octave = [12.0f32, -12.0].into_iter().find_map(|offset| {
            let other = near(middle + offset);
            let together = (class.len() + other.len()) * 4 >= windows.len() * 3;
            let difference = mean_periodicity(&other) - mean_periodicity(&class);
            if together && !other.is_empty() && difference.abs() >= notes::OCTAVE_PERIODICITY {
                if difference > 0.0 { class_median(&other) } else { class_median(&class) }
            } else {
                None
            }
        })?;
        (octave, true)
    };
    let off = |m: f32| {
        let distance = (m - midi).abs();
        if folded { distance.min((distance - 12.0).abs()) } else { distance }
    };
    if channel_medians.iter().any(|m| !m.is_finite() || off(*m) > 0.35) {
        return None;
    }
    let rounded = midi.round();
    if !(0.0..=127.0).contains(&rounded) {
        return None;
    }
    Some(Proposal {
        root: rounded as u8,
        midi,
        hz: 440.0 * 2.0f32.powf((midi - 69.0) / 12.0),
        cents: (midi - rounded) * 100.0,
        periodicity,
        spread_cents,
    })
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

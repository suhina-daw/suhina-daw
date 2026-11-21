// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The note track of a sample: a dense YIN pitch track, octave errors folded, cut into held
//! notes and the glides between them. Preparation only, on the analysis's 22050 Hz channel.
//! Assumptions and limits: `docs/spec/instrument-analysis.md` › Notes, glides and phrases.

use crate::{Error, MAX_HZ, MAX_SECONDS, MIN_HZ, RATE};
use suhina_dsp::pitch::Yin;

pub(crate) const NOTE_FRAME: usize = 2048;
pub(crate) const NOTE_HOP: usize = 512;
/// Frames of the note track within the analysis's eight seconds.
pub(crate) const MAX_NOTE_FRAMES: usize = MAX_SECONDS * RATE as usize / NOTE_HOP + 1;
/// A frame more than 30 dB below the loudest is unvoiced.
const VOICED_FLOOR: f64 = 1e-3;
/// A run of frames an octave from the frames around it, shorter than this, is an octave error.
const OCTAVE_RUN: usize = 4;
/// Unvoiced frames bridged inside a note.
const MAX_GAP_FRAMES: usize = 2;
/// A held note: at least this many frames (0.14 s) within half a semitone of their median.
const MIN_HELD_FRAMES: usize = 6;
const NOTE_SEMITONES: f32 = 0.5;
/// Two held notes an octave apart are one note when the upper one's mean periodicity is this
/// much lower (YIN's octave-up error) or higher (a subharmonic) than the lower one's.
pub(crate) const OCTAVE_PERIODICITY: f32 = 0.05;
/// Held notes this close (seconds between one's last frame's end and the next one's first
/// frame's start; frames overlap, so touching notes are negative) can merge.
const MERGE_GAP_SECONDS: f32 = 0.1;
/// A note is reached by a glide when the voiced material before it starts at least this far
/// (semitones) from it.
const GLIDE_SEMITONES: f32 = 1.0;

/// One frame of the note track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Frame {
    pub start: usize,
    pub midi: Option<f32>,
    pub periodicity: f32,
    pub energy: f64,
}

/// A held note of the analyzed material, in seconds from the analyzed range's start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeldNote {
    pub start_seconds: f32,
    pub end_seconds: f32,
    /// Median fractional MIDI pitch and its nearest key.
    pub midi: f32,
    pub root: u8,
    pub periodicity: f32,
    /// Interdecile pitch spread, in cents.
    pub spread_cents: f32,
    /// Share of the voiced frames' energy.
    pub share: f32,
}

/// The voiced material starts away from the note it settles on and moves into it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glide {
    /// Fractional MIDI pitch the material starts at.
    pub from_midi: f32,
    /// Seconds from the first voiced frame until the held note starts.
    pub seconds: f32,
}

/// The dense pitch track of `mono` (22050 Hz) over at most `MAX_NOTE_FRAMES` frames. `progress`
/// is called with `true` as each frame starts and with `false` between the estimator's lags; it
/// cancels when it returns false.
pub(crate) fn track(
    mono: &[f32],
    sample_rate: u32,
    mut progress: impl FnMut(bool) -> bool,
) -> Result<Vec<Frame>, Error> {
    let top = MAX_HZ.min(sample_rate as f32 * 0.45);
    let mut yin = Yin::new(RATE as f32, NOTE_FRAME, MIN_HZ, top, 0.15).ok_or(Error::InvalidFormat)?;
    let count = (mono.len().saturating_sub(NOTE_FRAME) / NOTE_HOP + 1).min(MAX_NOTE_FRAMES);
    if mono.len() < NOTE_FRAME {
        return Ok(Vec::new());
    }
    let mut frames = Vec::with_capacity(count);
    let mut scratch = [0.0f32; NOTE_FRAME];
    for index in 0..count {
        if !progress(true) {
            return Err(Error::Cancelled);
        }
        let start = index * NOTE_HOP;
        let input = mono.get(start..start + NOTE_FRAME).ok_or(Error::InvalidRange)?;
        let mean = input.iter().map(|x| f64::from(*x)).sum::<f64>() / NOTE_FRAME as f64;
        let energy = input.iter().map(|x| (f64::from(*x) - mean).powi(2)).sum::<f64>() / NOTE_FRAME as f64;
        frames.push(Frame { start, midi: None, periodicity: 0.0, energy });
        let scale = input.iter().map(|x| (f64::from(*x) - mean).abs()).fold(0.0f64, f64::max);
        if scale <= 1e-12 {
            continue;
        }
        for (to, from) in scratch.iter_mut().zip(input) {
            *to = ((f64::from(*from) - mean) / scale) as f32;
        }
        // As the window analysis: a crossing rate above the band is uncertain, not a low note.
        let crossings = scratch.windows(2).filter(|pair| matches!(pair, [a, b] if (*a < 0.0) != (*b < 0.0))).count();
        if crossings as f32 * RATE as f32 / (2.0 * (NOTE_FRAME - 1) as f32) > top * 1.01 {
            continue;
        }
        let mut cancelled = false;
        let estimate = yin.estimate_while(&scratch, || {
            let keep = progress(false);
            cancelled |= !keep;
            keep
        });
        if cancelled {
            return Err(Error::Cancelled);
        }
        if let (Some(pitch), Some(frame)) = (estimate.filter(|p| (MIN_HZ..=top).contains(&p.hz)), frames.last_mut()) {
            frame.midi = Some(pitch.midi_note());
            frame.periodicity = pitch.periodicity;
        }
    }
    let loudest = frames.iter().map(|f| f.energy).fold(0.0f64, f64::max);
    for frame in &mut frames {
        if frame.energy < loudest * VOICED_FLOOR {
            frame.midi = None;
        }
    }
    Ok(frames)
}

/// Folds short runs of frames an octave away from the frames before or after them (YIN's
/// octave errors) onto those frames' octave.
pub(crate) fn fold_octaves(frames: &mut [Frame]) {
    // Runs of voiced frames (gaps of up to MAX_GAP_FRAMES bridged) whose neighbours differ by
    // less than half an octave.
    let voiced: Vec<usize> = frames.iter().enumerate().filter(|(_, f)| f.midi.is_some()).map(|(i, _)| i).collect();
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut previous: Option<(usize, f32)> = None;
    for (position, &index) in voiced.iter().enumerate() {
        let Some(midi) = frames.get(index).and_then(|f| f.midi) else { continue };
        let joins =
            previous.is_some_and(|(last, value)| index - last <= MAX_GAP_FRAMES + 1 && (midi - value).abs() < 6.0);
        match runs.last_mut() {
            Some(run) if joins => run.1 = position + 1,
            _ => runs.push((position, position + 1)),
        }
        previous = Some((index, midi));
    }
    let run_median = |frames: &[Frame], run: (usize, usize)| -> Option<f32> {
        let mut values: Vec<f32> =
            voiced.get(run.0..run.1)?.iter().filter_map(|&i| frames.get(i).and_then(|f| f.midi)).collect();
        values.sort_by(f32::total_cmp);
        values.get(values.len() / 2).copied()
    };
    for (position, &run) in runs.iter().enumerate() {
        if run.1 - run.0 >= OCTAVE_RUN {
            continue;
        }
        let Some(own) = run_median(frames, run) else { continue };
        let neighbour = |other: Option<&(usize, usize)>| other.and_then(|&r| run_median(frames, r));
        let before = position.checked_sub(1).and_then(|p| neighbour(runs.get(p)));
        let after = neighbour(runs.get(position + 1));
        let Some(shift) = [before, after].into_iter().flatten().find_map(|target| {
            let difference = target - own;
            ((difference.abs() - 12.0).abs() <= 0.7).then(|| 12.0 * difference.signum())
        }) else {
            continue;
        };
        for &index in voiced.get(run.0..run.1).unwrap_or_default() {
            if let Some(midi) = frames.get_mut(index).and_then(|f| f.midi.as_mut()) {
                *midi += shift;
            }
        }
    }
}

/// The held notes of a folded track, in time order: maximal runs of voiced frames (gaps of up
/// to `MAX_GAP_FRAMES` bridged) within half a semitone of their running median, of at least
/// `MIN_HELD_FRAMES` frames. Touching notes of one pitch are one note, and so are touching notes
/// an octave apart whose periodicities differ by at least `OCTAVE_PERIODICITY`, at the more
/// periodic octave.
pub(crate) fn held_notes(frames: &[Frame]) -> Vec<HeldNote> {
    let total: f64 = frames.iter().filter(|f| f.midi.is_some()).map(|f| f.energy).sum();
    if total <= 0.0 {
        return Vec::new();
    }
    let mut segments: Vec<Vec<Frame>> = Vec::new();
    let mut last_index: Option<usize> = None;
    for (index, frame) in frames.iter().enumerate() {
        let Some(midi) = frame.midi else { continue };
        let close = last_index.is_none_or(|last| index - last > MAX_GAP_FRAMES + 1);
        let fits = segments.last().is_some_and(|segment| (midi - median(segment)).abs() <= NOTE_SEMITONES);
        match segments.last_mut() {
            Some(segment) if !close && fits => segment.push(*frame),
            _ => segments.push(vec![*frame]),
        }
        last_index = Some(index);
    }
    let mut notes: Vec<HeldNote> =
        segments.iter().filter(|s| s.len() >= MIN_HELD_FRAMES).filter_map(|s| note(s, total)).collect();
    // An octave-ambiguous note (each octave held for a while): one note at the octave YIN found
    // more periodic.
    let mut merged: Vec<HeldNote> = Vec::with_capacity(notes.len());
    for next in notes.drain(..) {
        if let Some(last) = merged.last_mut() {
            // Only touching notes: a note played again after a pause stays a note of its own.
            let touching = next.start_seconds - last.end_seconds < MERGE_GAP_SECONDS;
            let octave = ((next.midi - last.midi).abs() - 12.0).abs() <= NOTE_SEMITONES;
            let same = (next.midi - last.midi).abs() <= NOTE_SEMITONES;
            if touching && (same || (octave && (next.periodicity - last.periodicity).abs() >= OCTAVE_PERIODICITY)) {
                let keep_next = !same && next.periodicity > last.periodicity;
                let (midi, root, periodicity, spread) = if keep_next {
                    (next.midi, next.root, next.periodicity, next.spread_cents)
                } else {
                    (last.midi, last.root, last.periodicity, last.spread_cents)
                };
                *last = HeldNote {
                    start_seconds: last.start_seconds,
                    end_seconds: next.end_seconds,
                    midi,
                    root,
                    periodicity,
                    spread_cents: spread,
                    share: last.share + next.share,
                };
                continue;
            }
        }
        merged.push(next);
    }
    merged
}

/// The glide into `note`: the median of the first three voiced frames, when it lies at least
/// `GLIDE_SEMITONES` from the note and before it.
pub(crate) fn glide(frames: &[Frame], note: &HeldNote) -> Option<Glide> {
    let mut first: Vec<(f32, f32)> = frames
        .iter()
        .filter_map(|f| f.midi.map(|m| (f.start as f32 / RATE as f32, m)))
        .take_while(|(t, _)| *t < note.start_seconds)
        .take(3)
        .collect();
    let (start, _) = *first.first()?;
    first.sort_by(|a, b| a.1.total_cmp(&b.1));
    let (_, from) = *first.get(first.len() / 2)?;
    ((from - note.midi).abs() >= GLIDE_SEMITONES)
        .then_some(Glide { from_midi: from, seconds: note.start_seconds - start })
}

fn median(segment: &[Frame]) -> f32 {
    let mut values: Vec<f32> = segment.iter().filter_map(|f| f.midi).collect();
    values.sort_by(f32::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(f32::NAN)
}

fn note(segment: &[Frame], total: f64) -> Option<HeldNote> {
    let mut values: Vec<f32> = segment.iter().filter_map(|f| f.midi).collect();
    values.sort_by(f32::total_cmp);
    let midi = values.get(values.len() / 2).copied()?;
    let lower = values.get((values.len() - 1) / 10).copied().unwrap_or(midi);
    let upper = values.get((values.len() - 1) * 9 / 10).copied().unwrap_or(midi);
    let rounded = midi.round();
    if !(0.0..=127.0).contains(&rounded) {
        return None;
    }
    let first = segment.first()?;
    let last = segment.last()?;
    Some(HeldNote {
        start_seconds: first.start as f32 / RATE as f32,
        end_seconds: (last.start + NOTE_FRAME) as f32 / RATE as f32,
        midi,
        root: rounded as u8,
        periodicity: segment.iter().map(|f| f.periodicity).sum::<f32>() / segment.len() as f32,
        spread_cents: (upper - lower) * 100.0,
        share: (segment.iter().map(|f| f.energy).sum::<f64>() / total) as f32,
    })
}

#[cfg(test)]
#[path = "notes_tests.rs"]
mod tests;

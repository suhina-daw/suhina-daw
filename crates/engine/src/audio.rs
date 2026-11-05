// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Audio clip playback. Each clip placement becomes an [`AudioEvent`] when the renderer is built;
//! `process` only reads decoded samples that are already in memory.
//!
//! This is the direct reader: it reads the file faster or slower, so speed and pitch change
//! together ("re-pitch"). It plays unwarped clips, warped clips in Repitch mode, and warped
//! clips the stretcher refuses (a segment outside 0.25x-4x, or a file it cannot take); the
//! time-stretched path is `stretch.rs` (ADR 0003).
//!
//! Warped clips follow their warp markers: a beat in the clip maps to a time in the file, piece
//! by piece. Transpose applies to unwarped clips only: a warped clip's speed is set by the tempo,
//! and without a stretcher its pitch is set with it, so there is nothing left for transpose to
//! change (ADR 0003 › Transpose needs the stretcher). A warped clip in a stretching mode that
//! falls back to this reader loses its transpose for the same reason.

use std::sync::Arc;

pub use suhina_model::{AudioData, AudioPool};
use suhina_model::{AudioRegion, Clip, FadeShape, TempoMap};

use crate::tempo::TempoClock;

/// File sample rates the direct reader plays; a file outside them is not read (a rate of 0 would
/// freeze on one frame). Wider than [`STRETCH_SAMPLE_RATES`], since the direct reader is where a
/// clip the stretcher refuses goes.
pub(crate) const FILE_SAMPLE_RATES: std::ops::RangeInclusive<u32> = 1_000..=384_000;
/// File sample rates the stretcher takes.
pub(crate) const STRETCH_SAMPLE_RATES: std::ops::RangeInclusive<u32> = 8_000..=192_000;

/// Most warp markers the engine reads from one region, so a malformed project cannot make every
/// clip placement copy an unbounded list; markers past it are ignored (the last segment extends).
pub(crate) const MAX_WARP_MARKERS: usize = 100_000;

/// A region's usable warp markers as (beat, seconds): finite, sorted by beat, one per beat, at
/// most [`MAX_WARP_MARKERS`].
pub(crate) fn warp_markers(region: &AudioRegion) -> Vec<(f64, f64)> {
    let mut markers: Vec<(f64, f64)> = region
        .warp_markers
        .iter()
        .filter(|m| m.time.is_finite() && m.audio_time.is_finite())
        .map(|m| (m.time, m.audio_time))
        .collect();
    markers.sort_by(|a, b| a.0.total_cmp(&b.0));
    markers.dedup_by(|a, b| a.0 == b.0);
    markers.truncate(MAX_WARP_MARKERS);
    markers
}

/// File seconds at clip beat `beat` through sorted `markers`, extending the first and last
/// segments. A binary search: this runs per frame on the audio thread.
pub(crate) fn seconds_at(markers: &[(f64, f64)], beat: f64) -> f64 {
    // The segment that contains `beat`, or the first or last one to extend.
    let i = markers.partition_point(|(b, _)| *b <= beat).clamp(1, markers.len().max(2) - 1);
    let (Some(&(b0, s0)), Some(&(b1, s1))) = (markers.get(i - 1), markers.get(i)) else { return 0.0 };
    if b1 <= b0 {
        return s0;
    }
    s0 + (beat - b0) * (s1 - s0) / (b1 - b0)
}

/// How a beat in the clip maps to seconds in the file.
#[derive(Debug, Clone, PartialEq)]
enum TimeMap {
    /// Unwarped, or warped without usable markers: a fixed rate.
    Linear { seconds_per_beat: f64 },
    /// Warp markers as (beat, seconds), sorted by beat, at least two.
    Warp(Vec<(f64, f64)>),
}

impl TimeMap {
    fn seconds(&self, beat: f64) -> f64 {
        match self {
            Self::Linear { seconds_per_beat } => beat * seconds_per_beat,
            Self::Warp(markers) => seconds_at(markers, beat),
        }
    }
}

/// How loud a clip is near its edges: a short click guard at both ends, times the clip's own
/// fade-in and fade-out (`docs/spec/arrangement-editing.md` › Fades). All lengths are in frames;
/// the fades follow their shape and curve, the guard is linear, and they multiply. The curves'
/// exponents are worked out here, off the audio thread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Fades {
    click: f64,
    fade_in: f64,
    fade_out: f64,
    fade_in_shape: FadeShape,
    fade_out_shape: FadeShape,
    fade_in_exponent: f64,
    fade_out_exponent: f64,
}

/// The click guard at both ends of every audio clip, in seconds.
pub(crate) const CLICK_GUARD_SECONDS: f64 = 0.002;

impl Fades {
    /// The fades of `clip` at `frames_per_beat`, fitted to the clip (a malformed project cannot
    /// make them negative or longer than the clip).
    pub(crate) fn new(clip: &Clip, frames_per_beat: f64, sample_rate: u32) -> Self {
        let (fade_in, fade_out) = clip.fitted_fades();
        let frames = |beats: f64| {
            let f = beats * frames_per_beat;
            if f.is_finite() { f.max(0.0) } else { 0.0 }
        };
        Self {
            click: (CLICK_GUARD_SECONDS * f64::from(sample_rate)).max(1.0),
            fade_in: frames(fade_in),
            fade_out: frames(fade_out),
            fade_in_shape: clip.fade_in_curve.shape,
            fade_out_shape: clip.fade_out_curve.shape,
            fade_in_exponent: clip.fade_in_curve.exponent(),
            fade_out_exponent: clip.fade_out_curve.exponent(),
        }
    }

    /// The fades of `clip` on `clock`: their length follows the tempo where they are. At a
    /// constant tempo exactly [`Fades::new`].
    pub(crate) fn on(clip: &Clip, clock: &TempoClock, sample_rate: u32) -> Self {
        if clock.map().is_none() {
            return Self::new(clip, clock.per_beat(), sample_rate);
        }
        let (fade_in, fade_out) = clip.fitted_fades();
        let end = clip.start + clip.length;
        let frames = |f: f64| if f.is_finite() { f.max(0.0) } else { 0.0 };
        Self {
            fade_in: frames(clock.span(clip.start, fade_in)),
            fade_out: frames(clock.span(end - fade_out, fade_out)),
            ..Self::new(clip, clock.per_beat(), sample_rate)
        }
    }

    /// The gain `since_start` frames after the clip starts and `until_end` frames before it
    /// ends (the end is the frame after the last), 0 … 1. The ramps count the frames before and
    /// after this one, so the first and the last frame are both 0. Never allocates.
    pub(crate) fn gain(&self, since_start: f64, until_end: f64) -> f32 {
        let until_end = until_end - 1.0;
        let guard = (since_start / self.click).min(until_end / self.click);
        // A shape is worked out only inside its fade; past it the gain is 1.
        let bent = |length: f64, frames: f64, shape: FadeShape, exponent: f64| {
            let t = if length > 0.0 { frames / length } else { 1.0 };
            if t < 1.0 { shape.gain_bent(t, exponent) } else { 1.0 }
        };
        let fade_in = bent(self.fade_in, since_start, self.fade_in_shape, self.fade_in_exponent);
        let fade_out = bent(self.fade_out, until_end, self.fade_out_shape, self.fade_out_exponent);
        (guard.clamp(0.0, 1.0) * fade_in.clamp(0.0, 1.0) * fade_out.clamp(0.0, 1.0)) as f32
    }
}

/// The clip beat a clip starts playing from: its offset, or the loop start when the offset is at
/// or past the loop end (`docs/spec/arrangement-editing.md` › Start marker and loop). Every
/// reader (notes, direct and stretched audio, launched clips) starts there.
pub(crate) fn start_beat(offset: f64, loop_range: Option<(f64, f64)>) -> f64 {
    match loop_range {
        Some((start, end)) if offset >= end => start,
        _ => offset,
    }
}

/// The project timing an audio event is resolved against: its [`TempoClock`] (a constant one for
/// a clip placed outside the arrangement's time, which follows the launcher's clock) and the
/// sample rate.
#[derive(Debug, Clone)]
pub(crate) struct Timing {
    pub clock: TempoClock,
    pub sample_rate: u32,
}

/// One audio clip placement, resolved to frames.
#[derive(Debug, Clone)]
pub(crate) struct AudioEvent {
    pub start: u64,
    pub end: u64,
    frames_per_beat: f64,
    /// Where playback starts in the clip, in clip beats.
    offset: f64,
    loop_range: Option<(f64, f64)>,
    /// Clip beats advanced per project beat (transpose on unwarped clips).
    rate: f64,
    map: TimeMap,
    data: Arc<AudioData>,
    gain_left: f32,
    gain_right: f32,
    fades: Fades,
    /// Whether the clip follows the beat (warped); an unwarped clip plays at its own speed.
    warped: bool,
    /// Seconds added to the file position: an unwarped clip that was playing when the tempo
    /// changed carries on from where it was (ADR 0005).
    shift: f64,
    /// A warped clip on an arrangement whose tempo changes: the map in frames, and the clip's
    /// start beat. Its clip beat at a frame comes from the map, so it stays on the grid.
    beats: Option<(Arc<TempoMap>, f64)>,
}

impl AudioEvent {
    pub(crate) fn new(
        clip: &Clip,
        region: &AudioRegion,
        data: Arc<AudioData>,
        timing: Timing,
        gains: (f32, f32),
    ) -> Option<Self> {
        let Timing { clock, sample_rate } = timing;
        let (frames_per_beat, tempo_bpm) = (clock.per_beat(), clock.bpm());
        let finite = [clip.start, clip.length, clip.offset, frames_per_beat, tempo_bpm].iter().all(|v| v.is_finite());
        if !finite
            || clip.length <= 0.0
            || frames_per_beat <= 0.0
            || tempo_bpm <= 0.0
            || data.frames() == 0
            || !FILE_SAMPLE_RATES.contains(&data.sample_rate)
        {
            return None;
        }
        let linear = TimeMap::Linear { seconds_per_beat: crate::tempo::seconds_per_beat(tempo_bpm) };
        let markers = warp_markers(region);
        let (map, rate) = if region.warped {
            (if markers.len() >= 2 { TimeMap::Warp(markers) } else { linear }, 1.0)
        } else {
            let rate = 2f64.powf(region.transpose / 12.0);
            (linear, if rate.is_finite() && rate > 0.0 { rate } else { 1.0 })
        };
        let gain = if region.gain.is_finite() { region.gain.max(0.0) as f32 } else { 0.0 };
        let fades = Fades::on(clip, &clock, sample_rate);
        let (start, end) = (clock.frame_of(clip.start), clock.frame_of(clip.start + clip.length));
        // An unwarped clip plays at its file's speed whatever the tempo: only its start and end
        // follow the map.
        let beats = clock.map().filter(|_| region.warped).map(|m| (Arc::clone(m), clip.start));
        let loop_range = clip
            .loop_range
            .filter(|r| r.start.is_finite() && r.end.is_finite() && r.end > r.start)
            .map(|r| (r.start, r.end));
        Some(Self {
            start,
            end,
            frames_per_beat,
            offset: start_beat(clip.offset, loop_range),
            loop_range,
            rate,
            map,
            data,
            gain_left: gains.0 * gain,
            gain_right: gains.1 * gain,
            fades,
            warped: region.warped,
            shift: 0.0,
            beats,
        })
    }

    /// The gain envelope at the clip's edges.
    pub(crate) fn fades(&self) -> Fades {
        self.fades
    }

    pub(crate) fn warped(&self) -> bool {
        self.warped
    }

    /// Project beats from the clip's start to frame `position`: through the tempo map for a
    /// warped clip on a changing tempo, otherwise at the start tempo (an unwarped clip's beats
    /// are its file's seconds at that tempo).
    fn elapsed(&self, position: u64) -> f64 {
        match &self.beats {
            Some((map, start_beat)) => crate::tempo::beats_since(map, self.start, *start_beat, position),
            None => position.saturating_sub(self.start) as f64 / self.frames_per_beat,
        }
    }

    /// The file position, in seconds, played at frame `position` (inside `start..end`).
    pub(crate) fn file_seconds(&self, position: u64) -> f64 {
        self.map.seconds(self.clip_beat(self.elapsed(position))) + self.shift
    }

    /// Makes the clip play file position `seconds` at frame `position`, and on from there.
    pub(crate) fn continue_from(&mut self, position: u64, seconds: f64) {
        self.shift = 0.0;
        let shift = seconds - self.file_seconds(position);
        self.shift = if shift.is_finite() { shift } else { 0.0 };
    }

    /// The clip beat played `elapsed` project beats after the clip starts: from the offset, then
    /// around the loop if there is one.
    fn clip_beat(&self, elapsed: f64) -> f64 {
        let beat = self.offset + elapsed * self.rate;
        match self.loop_range {
            Some((start, end)) if beat >= end => start + (beat - end).rem_euclid(end - start),
            _ => beat,
        }
    }

    /// The stereo sample at frame `position`, which must be inside `start..end`.
    pub(crate) fn sample(&self, position: u64) -> (f32, f32) {
        let since_start = position.saturating_sub(self.start) as f64;
        let until_end = self.end.saturating_sub(position) as f64;
        let fade = self.fades.gain(since_start, until_end);
        let seconds = self.map.seconds(self.clip_beat(self.elapsed(position))) + self.shift;
        let (left, right) = self.data.stereo_at(seconds);
        (left * self.gain_left * fade, right * self.gain_right * fade)
    }

    /// Like [`AudioEvent::sample`] for an event placed at frame 0, at a fractional number of
    /// frames since it started; silence outside `0..end`. Launched clips use this, since their
    /// position follows the launcher clock rather than a fixed frame.
    pub(crate) fn sample_elapsed(&self, elapsed_frames: f64) -> (f32, f32) {
        let until_end = self.end as f64 - elapsed_frames;
        if !(elapsed_frames >= 0.0 && until_end > 0.0) {
            return (0.0, 0.0);
        }
        let fade = self.fades.gain(elapsed_frames, until_end);
        let seconds = self.map.seconds(self.clip_beat(elapsed_frames / self.frames_per_beat));
        let (left, right) = self.data.stereo_at(seconds);
        (left * self.gain_left * fade, right * self.gain_right * fade)
    }
}

#[cfg(test)]
#[path = "audio_tests.rs"]
mod tests;

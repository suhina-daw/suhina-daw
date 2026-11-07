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
use suhina_model::{Clip, ClipAudio, FadeShape, GainPoint, TempoMap, db_to_gain, envelope_db_at};

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

/// A clip's usable warp markers as (beat, seconds): finite, sorted by beat, one per beat, at
/// most [`MAX_WARP_MARKERS`].
pub(crate) fn warp_markers(audio: &ClipAudio) -> Vec<(f64, f64)> {
    let mut markers: Vec<(f64, f64)> = audio
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

/// How loud a clip is over its length: a short click guard at both ends, times the clip's own
/// fade-in and fade-out (`docs/spec/arrangement-editing.md` › Fades), times its gain envelope
/// (ADR 0035). All lengths are in frames; the fades follow their shape and curve, the guard is
/// linear, and they multiply. The curves' exponents and the envelope's points are worked out here,
/// off the audio thread.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Fades {
    click: f64,
    fade_in: f64,
    fade_out: f64,
    fade_in_shape: FadeShape,
    fade_out_shape: FadeShape,
    fade_in_exponent: f64,
    fade_out_exponent: f64,
    /// The clip's gain envelope; `None` when it has none (flat at its gain).
    envelope: Option<Arc<Envelope>>,
    /// An envelope edited while the clip plays: the gain it had (as a factor of the new one's),
    /// the frame since the clip's start where the edit came and the frames it glides over.
    glide: Option<Glide>,
}

/// A step in a playing clip's envelope (an edit swapped in under it), made a short ramp: the old
/// gain over the new one, `ratio`, goes to 1 from frame `at` since the clip's start over
/// `frames`. Cut from one to the other, a dragged point crackled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Glide {
    ratio: f64,
    at: f64,
    frames: f64,
}

/// How long the gain glides from an envelope to the edited one, in seconds: about as long as the
/// blocks the edits of a drag come in, so a drag is heard as a smooth move.
pub(crate) const ENVELOPE_GLIDE_SECONDS: f64 = 0.03;

/// A clip's gain envelope as the audio thread reads it: its points, and how a frame since the
/// clip's start maps to a beat of its content, as the readers play it (offset, loop, the repitch
/// rate of an unwarped clip, the tempo map of a warped one). Built off the audio thread.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Envelope {
    points: Vec<GainPoint>,
    /// The content beat the clip starts playing at.
    offset: f64,
    loop_range: Option<(f64, f64)>,
    /// Content beats per project beat.
    rate: f64,
    /// Frames per project beat, at a constant tempo.
    per_beat: f64,
    /// A warped clip on a changing tempo: the map, the clip's start frame and start beat.
    map: Option<(Arc<TempoMap>, f64, f64)>,
}

impl Envelope {
    /// The envelope of `clip` on `clock`, or `None` when it has no points.
    pub(crate) fn of(clip: &Clip, clock: &TempoClock) -> Option<Arc<Self>> {
        let points = clip.audio.envelope_points();
        if points.is_empty() {
            return None;
        }
        let audio = &clip.audio;
        let rate = if audio.warped { 1.0 } else { 2f64.powf(audio.transpose / 12.0) };
        let loop_range = clip
            .loop_range
            .filter(|r| r.start.is_finite() && r.end.is_finite() && r.end > r.start)
            .map(|r| (r.start, r.end));
        let map = clock
            .map()
            .filter(|_| audio.warped)
            .map(|m| (Arc::clone(m), clock.frame_of(clip.start) as f64, clip.start));
        Some(Arc::new(Self {
            points,
            offset: start_beat(clip.offset, loop_range),
            loop_range,
            rate: if rate.is_finite() && rate > 0.0 { rate } else { 1.0 },
            per_beat: clock.per_beat(),
            map,
        }))
    }

    /// The envelope's gain `since_start` frames after the clip starts, `extra` content beats on
    /// from where the clip's beats put it (an unwarped clip carried on through a tempo edit plays
    /// that far from them). Never allocates.
    fn gain(&self, since_start: f64, extra: f64) -> f64 {
        let elapsed = match &self.map {
            Some((map, start, start_beat)) => (map.beat_at(start + since_start) - start_beat).max(0.0),
            None if self.per_beat > 0.0 => since_start / self.per_beat,
            None => 0.0,
        };
        let mut beat = self.offset + elapsed * self.rate;
        if let Some((start, end)) = self.loop_range
            && beat >= end
        {
            beat = start + (beat - end).rem_euclid(end - start);
        }
        db_to_gain(envelope_db_at(&self.points, beat + extra))
    }
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
            envelope: Envelope::of(clip, &TempoClock::Constant { bpm: 0.0, per_beat: frames_per_beat }),
            glide: None,
        }
    }

    /// The envelope's gain (a factor, with any glide) `since_start` frames after the clip
    /// starts. Never allocates.
    pub(crate) fn envelope_gain(&self, since_start: f64, extra: f64) -> f64 {
        let gain = self.envelope.as_ref().map_or(1.0, |e| e.gain(since_start, extra));
        match self.glide {
            Some(Glide { ratio, at, frames }) if since_start >= at && since_start < at + frames => {
                let t = (since_start - at) / frames;
                gain * (ratio + (1.0 - ratio) * t)
            }
            _ => gain,
        }
    }

    /// The clip's envelope, shared (no copy): what a graph swapped in after an envelope message
    /// takes from the graph it replaces.
    pub(crate) fn envelope(&self) -> Option<Arc<Envelope>> {
        self.envelope.clone()
    }

    /// Puts `envelope` in place of the clip's envelope (an edit sent as a message, without a new
    /// graph) and returns the one it replaces, for the controller to free: nothing is freed on
    /// the audio thread. A clip playing `since_start` frames after its start glides from the gain
    /// it had there to the new one over `frames`. Never allocates.
    pub(crate) fn replace_envelope(
        &mut self,
        envelope: Option<Arc<Envelope>>,
        since_start: Option<f64>,
        frames: f64,
    ) -> Option<Arc<Envelope>> {
        let was = since_start.map(|at| (at, self.envelope_gain(at, 0.0)));
        let old = std::mem::replace(&mut self.envelope, envelope);
        if let Some((at, gain)) = was {
            self.glide_from(gain, at, frames);
        }
        old
    }

    /// Glides from `before`, the gain the clip played `since_start` frames after its start with
    /// its old envelope, to this envelope over `frames`, when they differ. Never allocates.
    pub(crate) fn glide_from(&mut self, before: f64, since_start: f64, frames: f64) {
        self.glide = None;
        let now = self.envelope_gain(since_start, 0.0);
        let ratio = before / now;
        if ratio.is_finite() && (ratio - 1.0).abs() > 1e-6 && frames >= 1.0 {
            self.glide = Some(Glide { ratio, at: since_start, frames });
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
            envelope: Envelope::of(clip, clock),
            ..Self::new(clip, clock.per_beat(), sample_rate)
        }
    }

    /// The gain `since_start` frames after the clip starts and `until_end` frames before it
    /// ends (the end is the frame after the last), 0 … 1. The ramps count the frames before and
    /// after this one, so the first and the last frame are both 0. Never allocates.
    pub(crate) fn gain(&self, since_start: f64, until_end: f64) -> f32 {
        self.gain_at(since_start, until_end, 0.0)
    }

    /// [`Fades::gain`] with the envelope read `extra` content beats on, where the reader plays.
    pub(crate) fn gain_at(&self, since_start: f64, until_end: f64, extra: f64) -> f32 {
        let until_end = until_end - 1.0;
        let guard = (since_start / self.click).min(until_end / self.click);
        // A shape is worked out only inside its fade; past it the gain is 1.
        let bent = |length: f64, frames: f64, shape: FadeShape, exponent: f64| {
            let t = if length > 0.0 { frames / length } else { 1.0 };
            if t < 1.0 { shape.gain_bent(t, exponent) } else { 1.0 }
        };
        let fade_in = bent(self.fade_in, since_start, self.fade_in_shape, self.fade_in_exponent);
        let fade_out = bent(self.fade_out, until_end, self.fade_out_shape, self.fade_out_exponent);
        let envelope = self.envelope_gain(since_start, extra);
        (guard.clamp(0.0, 1.0) * fade_in.clamp(0.0, 1.0) * fade_out.clamp(0.0, 1.0) * envelope) as f32
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
    pub(crate) fn new(clip: &Clip, data: Arc<AudioData>, timing: Timing, gains: (f32, f32)) -> Option<Self> {
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
        let markers = warp_markers(&clip.audio);
        let (map, rate) = if clip.audio.warped {
            (if markers.len() >= 2 { TimeMap::Warp(markers) } else { linear }, 1.0)
        } else {
            let rate = 2f64.powf(clip.audio.transpose / 12.0);
            (linear, if rate.is_finite() && rate > 0.0 { rate } else { 1.0 })
        };
        let gain = if clip.audio.gain.is_finite() { clip.audio.gain.max(0.0) as f32 } else { 0.0 };
        let fades = Fades::on(clip, &clock, sample_rate);
        let (start, end) = (clock.frame_of(clip.start), clock.frame_of(clip.start + clip.length));
        // An unwarped clip plays at its file's speed whatever the tempo: only its start and end
        // follow the map.
        let beats = clock.map().filter(|_| clip.audio.warped).map(|m| (Arc::clone(m), clip.start));
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
            warped: clip.audio.warped,
            shift: 0.0,
            beats,
        })
    }

    /// The clip's fades and gain envelope.
    pub(crate) fn fades(&self) -> &Fades {
        &self.fades
    }

    /// The clip's fades and gain envelope, to glide an edit in at a swap.
    pub(crate) fn fades_mut(&mut self) -> &mut Fades {
        &mut self.fades
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

    /// Drops a carry-over from [`AudioEvent::continue_from`]: the clip plays the file position
    /// of its beats again.
    pub(crate) fn play_from_beats(&mut self) {
        self.shift = 0.0;
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
        // The envelope where the reader is: an unwarped clip carried on through a tempo edit plays
        // `shift` seconds from its beats, so its envelope is read as far on.
        let extra = match &self.map {
            TimeMap::Linear { seconds_per_beat } if *seconds_per_beat > 0.0 => self.shift / seconds_per_beat,
            _ => 0.0,
        };
        let fade = self.fades.gain_at(since_start, until_end, extra);
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

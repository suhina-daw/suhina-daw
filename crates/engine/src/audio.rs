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

use std::collections::HashMap;
use std::sync::Arc;

use suhina_model::{AudioRegion, Clip, FadeShape, TempoMap};

/// File sample rates the direct reader plays; a file outside them is not read (a rate of 0 would
/// freeze on one frame). Wider than [`STRETCH_SAMPLE_RATES`], since the direct reader is where a
/// clip the stretcher refuses goes.
pub(crate) const FILE_SAMPLE_RATES: std::ops::RangeInclusive<u32> = 1_000..=384_000;
/// File sample rates the stretcher takes.
pub(crate) const STRETCH_SAMPLE_RATES: std::ops::RangeInclusive<u32> = 8_000..=192_000;

/// Decoded audio, interleaved f32.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioData {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl AsRef<[f32]> for AudioData {
    fn as_ref(&self) -> &[f32] {
        &self.samples
    }
}

impl AudioData {
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }

    /// The stereo sample at `seconds` into the file, linearly interpolated; silence outside it.
    fn stereo_at(&self, seconds: f64) -> (f32, f32) {
        self.frame_at(seconds * f64::from(self.sample_rate))
    }

    /// The stereo sample at frame `position` of the file (fractional frames interpolate
    /// linearly); silence outside it. Mono plays on both sides.
    pub(crate) fn frame_at(&self, position: f64) -> (f32, f32) {
        let channels = usize::from(self.channels.max(1));
        // Past the end (a huge or infinite position from hostile warp markers or offsets
        // included) is silence, before the cast could saturate; NaN fails this too.
        if !(position >= 0.0 && position < self.frames() as f64) {
            return (0.0, 0.0);
        }
        let index = position.floor() as usize;
        let fraction = (position - position.floor()) as f32;
        let frame = |i: usize| -> (f32, f32) {
            let base = i.saturating_mul(channels);
            let left = self.samples.get(base).copied().unwrap_or(0.0);
            let right =
                if channels > 1 { self.samples.get(base.saturating_add(1)).copied().unwrap_or(0.0) } else { left };
            (left, right)
        };
        let (l0, r0) = frame(index);
        let (l1, r1) = frame(index.saturating_add(1));
        (l0 + (l1 - l0) * fraction, r0 + (r1 - r0) * fraction)
    }
}

/// The audio a project needs, prepared before playback.
#[derive(Debug, Clone, Default)]
pub struct AudioPool {
    /// Decoded files, keyed by the `file` string of the project's audio regions.
    pub files: HashMap<String, Arc<AudioData>>,
}

impl AudioPool {
    pub fn new() -> Self {
        Self::default()
    }
}

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

    /// The fades of `clip` through the tempo map in frames: their length follows the tempo where
    /// they are. At a constant tempo exactly [`Fades::new`].
    pub(crate) fn on_map(clip: &Clip, map: &TempoMap, sample_rate: u32) -> Self {
        if map.is_constant() {
            return Self::new(clip, map.start_per_beat(), sample_rate);
        }
        let (fade_in, fade_out) = clip.fitted_fades();
        let end = clip.start + clip.length;
        let frames = |f: f64| if f.is_finite() { f.max(0.0) } else { 0.0 };
        Self {
            fade_in: frames(map.span(clip.start, fade_in)),
            fade_out: frames(map.span(end - fade_out, fade_out)),
            ..Self::new(clip, map.start_per_beat(), sample_rate)
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

/// The project timing an audio event is resolved against: the start tempo, and the tempo map in
/// frames when the tempo changes (`None` for a constant tempo, or a clip placed outside the
/// arrangement's time).
#[derive(Debug, Clone)]
pub(crate) struct Timing {
    pub frames_per_beat: f64,
    pub tempo_bpm: f64,
    pub sample_rate: u32,
    pub map: Option<Arc<TempoMap>>,
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
        let Timing { frames_per_beat, tempo_bpm, sample_rate, map: tempo } = timing;
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
        let linear = TimeMap::Linear { seconds_per_beat: 60.0 / tempo_bpm };
        let markers = warp_markers(region);
        let (map, rate) = if region.warped {
            (if markers.len() >= 2 { TimeMap::Warp(markers) } else { linear }, 1.0)
        } else {
            let rate = 2f64.powf(region.transpose / 12.0);
            (linear, if rate.is_finite() && rate > 0.0 { rate } else { 1.0 })
        };
        let gain = if region.gain.is_finite() { region.gain.max(0.0) as f32 } else { 0.0 };
        let to_frame = |beats: f64| match &tempo {
            Some(map) => crate::tempo::frame_of(map, beats),
            None => (beats.max(0.0) * frames_per_beat).round() as u64,
        };
        let fades = match &tempo {
            Some(map) => Fades::on_map(clip, map, sample_rate),
            None => Fades::new(clip, frames_per_beat, sample_rate),
        };
        let (start, end) = (to_frame(clip.start), to_frame(clip.start + clip.length));
        // An unwarped clip plays at its file's speed whatever the tempo: only its start and end
        // follow the map.
        let beats = tempo.filter(|m| region.warped && !m.is_constant()).map(|m| (m, clip.start));
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
mod tests {
    use super::*;
    use suhina_model::*;

    const SR: u32 = 48_000;

    /// One second of a ramp, so the sample value tells the file position: value = seconds.
    fn ramp() -> Arc<AudioData> {
        Arc::new(AudioData { sample_rate: 8000, channels: 1, samples: (0..8000).map(|i| i as f32 / 8000.0).collect() })
    }

    fn region(warped: bool, markers: &[(f64, f64)], transpose: f64) -> AudioRegion {
        AudioRegion {
            sample_rate: Some(8000),
            channels: Some(1),
            duration_seconds: Some(1.0),
            warped,
            warp_markers: markers.iter().map(|&(time, audio_time)| WarpMarker { time, audio_time }).collect(),
            transpose,
            ..AudioRegion::new("ramp.wav")
        }
    }

    fn clip(length: f64, offset: f64, loop_range: Option<(f64, f64)>) -> Clip {
        Clip {
            offset,
            loop_range: loop_range.map(|(start, end)| LoopRange { start, end }),
            ..Clip::new(ContentId(1), 0.0, length)
        }
    }

    /// File position (seconds) played at `beat` in the clip, at 120 bpm (0.5 s per beat).
    fn file_seconds_at(event: &AudioEvent, beat: f64) -> f32 {
        event.sample((beat * event.frames_per_beat) as u64).0
    }

    fn event(clip: &Clip, region: &AudioRegion) -> AudioEvent {
        AudioEvent::new(
            clip,
            region,
            ramp(),
            Timing { frames_per_beat: 60.0 / 120.0 * f64::from(SR), tempo_bpm: 120.0, sample_rate: SR, map: None },
            (1.0, 1.0),
        )
        .unwrap()
    }

    #[test]
    fn fades_are_fitted_to_the_clip_and_meet_the_click_guard() {
        // 4 beats of fades on a 2-beat clip are fitted to 1.5 + 0.5 beats, at 100 frames a beat.
        let c = Clip { fade_in: 3.0, fade_out: 1.0, ..clip(2.0, 0.0, None) };
        let fades = Fades::new(&c, 100.0, 1000);
        assert_eq!((fades.fade_in, fades.fade_out), (150.0, 50.0));
        assert_eq!(fades.click, 2.0);
        // Half-way into the fade-in; the guard is over after two frames.
        assert!((fades.gain(75.0, 125.0) - 0.5).abs() < 1e-6);
        assert!((fades.gain(1.0, 199.0) - 0.5 / 150.0).abs() < 1e-6, "guard and fade multiply");
        assert!((fades.gain(160.0, 40.0) - 0.78).abs() < 1e-6, "39 frames after this one, of 50");
        assert_eq!(fades.gain(100.0, 1.0), 0.0, "the last frame");
        assert_eq!(fades.gain(0.0, 200.0), 0.0);
        let c = Clip { fade_in: f64::NAN, fade_out: -1.0, ..clip(2.0, 0.0, None) };
        assert_eq!(Fades::new(&c, 100.0, 1000).gain(100.0, 100.0), 1.0, "bad fades are no fades");
    }

    /// The direct reader refuses a tempo of 0 and a file sample rate outside 1 to 384 kHz, as the
    /// stretcher refuses them (#164).
    #[test]
    fn a_zero_tempo_or_an_unplayable_sample_rate_is_no_event() {
        let timing = |tempo_bpm: f64| Timing { frames_per_beat: 24_000.0, tempo_bpm, sample_rate: SR, map: None };
        let c = clip(2.0, 0.0, None);
        assert!(AudioEvent::new(&c, &region(false, &[], 0.0), ramp(), timing(0.0), (1.0, 1.0)).is_none());
        assert!(AudioEvent::new(&c, &region(false, &[], 0.0), ramp(), timing(-120.0), (1.0, 1.0)).is_none());
        for rate in [0, 999, 384_001, 1_000_000] {
            let data = Arc::new(AudioData { sample_rate: rate, ..(*ramp()).clone() });
            assert!(AudioEvent::new(&c, &region(false, &[], 0.0), data, timing(120.0), (1.0, 1.0)).is_none(), "{rate}");
        }
        assert!(AudioEvent::new(&c, &region(false, &[], 0.0), ramp(), timing(120.0), (1.0, 1.0)).is_some());
    }

    /// Hostile warp markers and offsets put the read position far past the file (or at
    /// infinity): silence, never a panic or a NaN (issue #152). Stereo, where the right
    /// channel's index used to overflow.
    #[test]
    fn a_huge_position_in_a_stereo_file_is_silence() {
        let stereo = Arc::new(AudioData { sample_rate: 1000, channels: 2, samples: vec![0.5; 2000] });
        for position in [1e300, f64::INFINITY, usize::MAX as f64, 1000.0, f64::NAN, -1.0] {
            assert_eq!(stereo.frame_at(position), (0.0, 0.0), "{position}");
        }
        assert_eq!(stereo.frame_at(999.0), (0.5, 0.5));
        let warped = AudioRegion { channels: Some(2), ..region(true, &[(0.0, 0.0), (1.0, 1e300)], 0.0) };
        let e = AudioEvent::new(
            &clip(2.0, 0.0, None),
            &warped,
            stereo,
            Timing { frames_per_beat: 60.0 / 120.0 * f64::from(SR), tempo_bpm: 120.0, sample_rate: SR, map: None },
            (1.0, 1.0),
        )
        .unwrap();
        for frame in [0u64, 1, 24_000, 47_999] {
            let (l, r) = e.sample(frame);
            assert!(l.is_finite() && r.is_finite(), "frame {frame}: {l} {r}");
        }
    }

    #[test]
    fn unwarped_plays_at_file_speed() {
        let e = event(&clip(2.0, 0.0, None), &region(false, &[], 0.0));
        assert!((file_seconds_at(&e, 1.0) - 0.5).abs() < 1e-3);
        assert!((file_seconds_at(&e, 1.5) - 0.75).abs() < 1e-3);
    }

    /// Without a stretcher a warped clip's pitch follows the tempo, so transpose changes nothing
    /// (Repitch mode, and clips the stretcher refuses; #168).
    #[test]
    fn transpose_leaves_a_warped_clip_read_directly_unchanged() {
        let markers = [(0.0, 0.0), (2.0, 0.75)];
        let plain = event(&clip(2.0, 0.0, None), &region(true, &markers, 0.0));
        let transposed = event(&clip(2.0, 0.0, None), &region(true, &markers, 7.0));
        for beat in [0.0, 0.3, 1.0, 1.7] {
            assert_eq!(file_seconds_at(&plain, beat), file_seconds_at(&transposed, beat), "beat {beat}");
        }
    }

    /// The binary search finds the same segment as a scan from the start (#162), at, between,
    /// before and after markers.
    #[test]
    fn warp_lookup_matches_a_linear_scan() {
        let linear = |markers: &[(f64, f64)], beat: f64| {
            let i = markers.iter().position(|(b, _)| *b > beat).unwrap_or(markers.len()).clamp(1, markers.len() - 1);
            let ((b0, s0), (b1, s1)) = (markers[i - 1], markers[i]);
            s0 + (beat - b0) * (s1 - s0) / (b1 - b0)
        };
        let markers: Vec<(f64, f64)> = (0..500).map(|i| (f64::from(i) * 0.37, f64::from(i * i) * 0.001)).collect();
        for k in -40..2000 {
            let beat = f64::from(k) * 0.0925;
            assert_eq!(seconds_at(&markers, beat), linear(&markers, beat), "beat {beat}");
        }
        for &(beat, _) in &markers {
            assert_eq!(seconds_at(&markers, beat), linear(&markers, beat), "on the marker at {beat}");
        }
    }

    /// Markers past the cap are ignored, so no clip placement copies an unbounded list.
    #[test]
    fn warp_markers_are_capped() {
        let many: Vec<(f64, f64)> = (0..MAX_WARP_MARKERS + 10).map(|i| (i as f64, i as f64 * 0.01)).collect();
        assert_eq!(warp_markers(&region(true, &many, 0.0)).len(), MAX_WARP_MARKERS);
    }

    #[test]
    fn transpose_speeds_up_unwarped_clips() {
        let e = event(&clip(2.0, 0.0, None), &region(false, &[], 12.0));
        assert!((file_seconds_at(&e, 0.5) - 0.5).abs() < 1e-3, "an octave up plays twice as fast");
    }

    #[test]
    fn warp_markers_map_beats_to_file_time() {
        // Beat 0 at 0 s, beat 2 at 0.5 s: the file plays at half speed against 120 bpm.
        let e = event(&clip(2.0, 0.0, None), &region(true, &[(0.0, 0.0), (2.0, 0.5)], 12.0));
        assert!((file_seconds_at(&e, 1.0) - 0.25).abs() < 1e-3);
        // Past the last marker the last segment continues.
        let e = event(&clip(4.0, 0.0, None), &region(true, &[(0.0, 0.0), (2.0, 0.5)], 0.0));
        assert!((file_seconds_at(&e, 3.0) - 0.75).abs() < 1e-3);
    }

    #[test]
    fn offset_and_loop() {
        // Start at clip beat 1 (0.5 s), loop beats 0..1.5 (0..0.75 s).
        let e = event(&clip(4.0, 1.0, Some((0.0, 1.5))), &region(false, &[], 0.0));
        assert!((file_seconds_at(&e, 0.25) - 0.625).abs() < 1e-3);
        // After 0.5 beats the loop end is reached and playback wraps to its start.
        assert!((file_seconds_at(&e, 0.75) - 0.125).abs() < 1e-3);
    }

    /// An offset past the loop end starts at the loop start, as the notes do (#166).
    #[test]
    fn an_offset_past_the_loop_end_starts_at_the_loop_start() {
        let e = event(&clip(4.0, 5.0, Some((0.0, 1.5))), &region(false, &[], 0.0));
        assert!((file_seconds_at(&e, 0.25) - 0.125).abs() < 1e-3);
        assert!((file_seconds_at(&e, 1.75) - 0.125).abs() < 1e-3, "and loops from there");
        let at_end = event(&clip(4.0, 1.5, Some((0.0, 1.5))), &region(false, &[], 0.0));
        assert!((file_seconds_at(&at_end, 0.25) - 0.125).abs() < 1e-3);
    }

    #[test]
    fn outside_the_file_is_silent_and_bad_clips_are_rejected() {
        let e = event(&clip(8.0, 0.0, None), &region(false, &[], 0.0));
        assert_eq!(e.sample(e.start + (3.0 * e.frames_per_beat) as u64), (0.0, 0.0));
        let fpb = 24_000.0;
        assert!(
            AudioEvent::new(
                &clip(0.0, 0.0, None),
                &region(false, &[], 0.0),
                ramp(),
                Timing { frames_per_beat: fpb, tempo_bpm: 120.0, sample_rate: SR, map: None },
                (1.0, 1.0)
            )
            .is_none()
        );
        assert!(
            AudioEvent::new(
                &clip(f64::NAN, 0.0, None),
                &region(false, &[], 0.0),
                ramp(),
                Timing { frames_per_beat: fpb, tempo_bpm: 120.0, sample_rate: SR, map: None },
                (1.0, 1.0)
            )
            .is_none()
        );
        let empty = Arc::new(AudioData { sample_rate: 1000, channels: 2, samples: Vec::new() });
        assert!(
            AudioEvent::new(
                &clip(1.0, 0.0, None),
                &region(false, &[], 0.0),
                empty,
                Timing { frames_per_beat: fpb, tempo_bpm: 120.0, sample_rate: SR, map: None },
                (1.0, 1.0)
            )
            .is_none()
        );
    }
}

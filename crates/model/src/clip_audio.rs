// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What an audio clip does with the audio it plays: its gain and gain envelope, its transpose, its
//! warp and the tempo it was warped from. Every audio clip has its own; two clips that play the same
//! file never share them ([ADR 0001](../../docs/decisions/0001-daw-neutral-model.md) › Amendment:
//! per-clip audio, [ADR 0035](../../docs/decisions/0035-clip-gain-envelope.md)). The content keeps
//! only the source: the file, its stream, its rate, channels and length.

use crate::{StretchMode, WarpMarker};

/// Most points one clip's gain envelope keeps; a malformed project cannot make the engine copy an
/// unbounded list.
pub const MAX_GAIN_POINTS: usize = 10_000;

/// The lowest and highest offset a gain envelope point takes, in dB from the clip's gain.
pub const GAIN_POINT_RANGE: std::ops::RangeInclusive<f64> = -70.0..=24.0;

/// The lowest and highest clip gain the clip panel's fader and `clip.setGain` take, in dB; below the
/// lowest is silence.
pub const CLIP_GAIN_MAX_DB: f64 = 24.0;

/// The lowest and highest transpose of an audio clip, in semitones.
pub const TRANSPOSE_RANGE: std::ops::RangeInclusive<f64> = -48.0..=48.0;

/// The lowest and highest tempo a clip's audio is set to, in beats per minute.
pub const CLIP_TEMPO_RANGE: std::ops::RangeInclusive<f64> = 20.0..=999.0;

/// The audio state of one clip. A clip whose content has no audio keeps the starting values and
/// ignores them.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipAudio {
    /// Linear gain; 1.0 is unity.
    pub gain: f64,
    /// The gain envelope: offsets in dB from `gain`, at beats of the content. Empty is flat at
    /// `gain`.
    pub gain_envelope: Vec<GainPoint>,
    /// Pitch shift in semitones (fractions are cents / 100).
    pub transpose: f64,
    /// When false the audio plays at its original speed and `warp_markers` are kept but unused.
    pub warped: bool,
    /// How a warped clip keeps its pitch when it plays faster or slower (ADR 0003).
    pub stretch: StretchMode,
    /// Pins positions in the audio file to beats of the content. Empty when the audio plays at its
    /// original speed from the content start.
    pub warp_markers: Vec<WarpMarker>,
    /// Where the tempo the clip was warped from came from (#824); `None` when it was never known.
    pub tempo_source: Option<TempoSource>,
}

impl Default for ClipAudio {
    /// Unity gain, no envelope, no transpose, unwarped, no tempo.
    fn default() -> Self {
        Self {
            gain: 1.0,
            gain_envelope: Vec::new(),
            transpose: 0.0,
            warped: false,
            stretch: StretchMode::default(),
            warp_markers: Vec::new(),
            tempo_source: None,
        }
    }
}

/// One point of a clip's gain envelope.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GainPoint {
    /// Beats from the start of the content, where the warp markers are too: a loop plays the same
    /// stretch of the envelope on every pass, and a split clip keeps its part of it.
    pub beat: f64,
    /// Offset from the clip's gain in dB, within [`GAIN_POINT_RANGE`]; 0 follows the gain.
    pub db: f64,
    /// The shape of the segment from this point to the next, as an automation segment's
    /// ([`crate::automation::Curve`]): straight in dB by default.
    pub curve: crate::automation::Curve,
}

impl GainPoint {
    /// A point at content beat `beat`, `db` from the clip's gain, straight to the next.
    pub fn new(beat: f64, db: f64) -> Self {
        Self { beat, db, curve: crate::automation::Curve::STRAIGHT }
    }
}

/// Where a clip's tempo came from (#824, `docs/spec/clip-panel.md` › Tempo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TempoSource {
    /// The file's own metadata.
    Metadata,
    /// The file's name ("loop_128bpm").
    Name,
    /// Detected from the sound.
    Detected,
    /// Typed, halved or doubled by hand.
    Manual,
}

impl TempoSource {
    pub const ALL: [TempoSource; 4] = [Self::Metadata, Self::Name, Self::Detected, Self::Manual];

    /// The name used in project files and commands.
    pub fn name(self) -> &'static str {
        match self {
            Self::Metadata => "metadata",
            Self::Name => "name",
            Self::Detected => "detected",
            Self::Manual => "manual",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }
}

impl ClipAudio {
    /// The envelope's usable points: finite, within range, sorted by beat, at most
    /// [`MAX_GAIN_POINTS`]. Two points on one beat are both kept, in the order they were, so a
    /// step can be drawn.
    pub fn envelope_points(&self) -> Vec<GainPoint> {
        let mut points: Vec<GainPoint> = self
            .gain_envelope
            .iter()
            .filter(|p| p.beat.is_finite() && p.db.is_finite())
            .map(|p| GainPoint {
                db: p.db.clamp(*GAIN_POINT_RANGE.start(), *GAIN_POINT_RANGE.end()),
                curve: p.curve.held(),
                ..*p
            })
            .take(MAX_GAIN_POINTS)
            .collect();
        points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        points
    }

    /// Whether the envelope, fields and markers hold values a clip can have.
    pub fn is_valid(&self) -> bool {
        self.gain.is_finite()
            && self.gain >= 0.0
            && self.gain <= max_clip_gain()
            && self.transpose.is_finite()
            && self.gain_envelope.len() <= MAX_GAIN_POINTS
            && self.gain_envelope.iter().all(|p| p.beat.is_finite() && p.db.is_finite())
    }

    /// The clip's tempo from its warp markers: the beats between the first and the last marker over
    /// the seconds of audio between them. `None` when there are fewer than two usable markers.
    /// Markers on one beat count as the engine plays them: the first of them.
    pub fn tempo(&self) -> Option<f64> {
        // One pass, no copy: the earliest beat and the latest, each the first marker on its beat.
        let mut ends: Option<(&WarpMarker, &WarpMarker)> = None;
        for marker in self.warp_markers.iter().filter(|m| m.time.is_finite() && m.audio_time.is_finite()) {
            ends = Some(match ends {
                None => (marker, marker),
                Some((first, last)) => (
                    if marker.time < first.time { marker } else { first },
                    if marker.time > last.time { marker } else { last },
                ),
            });
        }
        let (first, last) = ends?;
        let (beats, seconds) = (last.time - first.time, last.audio_time - first.audio_time);
        let bpm = beats / seconds * 60.0;
        (beats > 0.0 && seconds > 0.0 && bpm.is_finite() && bpm > 0.0).then_some(bpm)
    }
}

/// The envelope offset in dB at content beat `beat` through sorted `points`: straight lines in dB
/// between points, the first point's value before it and the last one's after it, 0 with no points.
/// Two points on one beat make a step: the later one holds from there. A binary search, never
/// allocates.
pub fn envelope_db_at(points: &[GainPoint], beat: f64) -> f64 {
    let i = points.partition_point(|p| p.beat <= beat);
    match (i.checked_sub(1).and_then(|k| points.get(k)), points.get(i)) {
        (None, None) => 0.0,
        (None, Some(next)) => next.db,
        (Some(prev), None) => prev.db,
        (Some(prev), Some(next)) => {
            // Halved, so points far apart cannot overflow the span to infinity.
            let span = next.beat / 2.0 - prev.beat / 2.0;
            if span <= 0.0 {
                next.db
            } else {
                // Along the first point's curve, in dB.
                let t = (beat / 2.0 - prev.beat / 2.0) / span;
                prev.db + prev.curve.shape(t) * (next.db - prev.db)
            }
        }
    }
}

/// The highest linear gain a clip takes: [`CLIP_GAIN_MAX_DB`].
pub fn max_clip_gain() -> f64 {
    db_to_gain(CLIP_GAIN_MAX_DB)
}

/// `db` as linear gain; below −150 dB is silence.
pub fn db_to_gain(db: f64) -> f64 {
    if db.is_finite() && db >= -150.0 {
        10f64.powf(db / 20.0)
    } else if db == f64::INFINITY {
        f64::MAX
    } else {
        0.0
    }
}

/// `gain` in dB; silence is −∞.
pub fn gain_to_db(gain: f64) -> f64 {
    if gain > 0.0 && gain.is_finite() { 20.0 * gain.log10() } else { f64::NEG_INFINITY }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(beat: f64, db: f64) -> GainPoint {
        GainPoint::new(beat, db)
    }

    #[test]
    fn an_empty_envelope_follows_the_gain() {
        assert_eq!(envelope_db_at(&[], 3.0), 0.0);
    }

    #[test]
    fn the_envelope_is_straight_in_db_between_points_and_held_past_the_ends() {
        let points = [point(1.0, -6.0), point(3.0, -12.0)];
        assert_eq!(envelope_db_at(&points, 0.0), -6.0);
        assert_eq!(envelope_db_at(&points, 2.0), -9.0);
        assert_eq!(envelope_db_at(&points, 5.0), -12.0);
    }

    #[test]
    fn two_points_on_one_beat_make_a_step() {
        let points = [point(0.0, 0.0), point(2.0, 0.0), point(2.0, -20.0), point(4.0, -20.0)];
        assert_eq!(envelope_db_at(&points, 1.999), 0.0);
        assert_eq!(envelope_db_at(&points, 2.0), -20.0);
    }

    #[test]
    fn envelope_points_drop_what_is_not_a_number_and_clamp_the_rest() {
        let audio = ClipAudio {
            gain_envelope: vec![point(2.0, 40.0), point(f64::NAN, 0.0), point(1.0, -100.0)],
            ..ClipAudio::default()
        };
        assert_eq!(audio.envelope_points(), vec![point(1.0, -70.0), point(2.0, 24.0)]);
    }

    #[test]
    fn a_clips_tempo_comes_from_its_first_and_last_marker() {
        let audio = ClipAudio {
            warp_markers: vec![WarpMarker { time: 0.0, audio_time: 0.0 }, WarpMarker { time: 8.0, audio_time: 4.0 }],
            ..ClipAudio::default()
        };
        assert_eq!(audio.tempo(), Some(120.0));
        assert_eq!(ClipAudio::default().tempo(), None);
    }

    #[test]
    fn markers_on_one_beat_count_as_the_first_of_them() {
        let marker = |time, audio_time| WarpMarker { time, audio_time };
        let audio = ClipAudio {
            warp_markers: vec![marker(0.0, 0.0), marker(4.0, 2.0), marker(4.0, 3.0)],
            ..ClipAudio::default()
        };
        assert_eq!(audio.tempo(), Some(120.0), "as the engine plays it");
    }

    #[test]
    fn points_far_apart_interpolate_without_overflow() {
        let points = [point(-f64::MAX, 0.0), point(f64::MAX, -20.0)];
        assert!((envelope_db_at(&points, 0.0) + 10.0).abs() < 1e-9);
    }

    #[test]
    fn a_gain_past_the_most_is_not_valid() {
        assert!(!ClipAudio { gain: 1e300, ..ClipAudio::default() }.is_valid());
        assert!(ClipAudio { gain: max_clip_gain(), ..ClipAudio::default() }.is_valid());
    }

    #[test]
    fn decibels_and_gain_convert_both_ways() {
        assert_eq!(db_to_gain(0.0), 1.0);
        assert_eq!(db_to_gain(-200.0), 0.0);
        assert!((gain_to_db(db_to_gain(-6.0)) + 6.0).abs() < 1e-12);
        assert_eq!(gain_to_db(0.0), f64::NEG_INFINITY);
    }

    #[test]
    fn tempo_sources_round_trip_their_names() {
        for source in TempoSource::ALL {
            assert_eq!(TempoSource::from_name(source.name()), Some(source));
        }
    }

    #[test]
    fn a_bent_segment_follows_its_curve_in_db() {
        let mut first = point(0.0, 0.0);
        first.curve = crate::automation::Curve::bend(1.0);
        let points = [first, point(4.0, -12.0)];
        let half = envelope_db_at(&points, 2.0);
        // Bent up toward the second point's level: past half of the way at half of the time.
        assert!(half < -6.0 && half > -12.0, "{half}");
        assert_eq!(envelope_db_at(&points, 0.0), 0.0);
        assert_eq!(envelope_db_at(&points, 4.0), -12.0);
    }
}

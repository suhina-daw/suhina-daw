// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How an audio clip's content beats map to its file's seconds, as the engine plays it, and
//! keeping each clip on the same audio when that mapping changes (#830): warp turned on or off,
//! a transpose that repitches unwarped audio, or the tempo unwarped audio is counted in. The
//! clip's start on the arrangement stays; its offset, length and loop move so they still cover
//! the same seconds of the file, so the clip is drawn as long as what it plays.

use suhina_model::{AudioRegion, Clip, ClipAudio, ContentId, LaneData, LoopRange, Project};

/// Shortest a clip is made by a remap, in beats.
const SHORTEST: f64 = 1.0 / 64.0;
/// Most warp markers read from one clip, as the engine reads (`MAX_WARP_MARKERS` in
/// `crates/engine/src/audio.rs`): a project with more cannot make an edit copy them all.
const MAX_WARP_MARKERS: usize = 100_000;

/// A clip's mapping from its content's beats to file seconds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Mapping {
    /// Unwarped (or warped without two usable markers): seconds per content beat at the
    /// project's tempo, and the content beats a project beat plays (a transpose's repitch on an
    /// unwarped clip). The engine plays content beat `offset + elapsed × rate`, so the rate
    /// applies to the time played, never to where the clip starts in its file.
    Linear { per_beat: f64, rate: f64 },
    /// Warp markers as (beat, seconds), sorted by beat, rising in both.
    Warp(Vec<(f64, f64)>),
}

impl Mapping {
    /// The mapping of a clip with `audio` at `tempo_bpm`, as the engine plays it
    /// (`crates/engine/src/audio.rs`).
    pub(crate) fn of(audio: &ClipAudio, tempo_bpm: f64) -> Option<Self> {
        if !(tempo_bpm.is_finite() && tempo_bpm > 0.0) {
            return None;
        }
        let mut markers: Vec<(f64, f64)> = audio
            .warp_markers
            .iter()
            .take(MAX_WARP_MARKERS.saturating_mul(2))
            .filter(|m| m.time.is_finite() && m.audio_time.is_finite())
            .map(|m| (m.time, m.audio_time))
            .collect();
        markers.sort_by(|a, b| a.0.total_cmp(&b.0));
        markers.dedup_by(|a, b| a.0 == b.0);
        markers.truncate(MAX_WARP_MARKERS);
        let per_beat = 60.0 / tempo_bpm;
        if audio.warped && markers.len() >= 2 {
            // The engine plays any two markers; only markers that rise in both can be read back
            // from seconds, so a clip on markers that fall is left as it is (no mapping).
            let rising = markers.windows(2).all(|w| w[1].0 > w[0].0 && w[1].1 > w[0].1);
            return rising.then_some(Self::Warp(markers));
        }
        let rate = if audio.warped { 1.0 } else { 2f64.powf(audio.transpose / 12.0) };
        let rate = if rate.is_finite() && rate > 0.0 { rate } else { 1.0 };
        Some(Self::Linear { per_beat, rate })
    }

    /// File seconds at content beat `beat`, the end segments extended.
    fn seconds(&self, beat: f64) -> f64 {
        match self {
            Self::Linear { per_beat, .. } => beat * per_beat,
            Self::Warp(markers) => along(markers, beat, |m| m.0, |m| m.1),
        }
    }

    /// Content beats one project beat plays.
    fn rate(&self) -> f64 {
        match self {
            Self::Linear { rate, .. } => *rate,
            Self::Warp(_) => 1.0,
        }
    }

    /// Content beat at file second `seconds`: the inverse of [`Mapping::seconds`].
    fn beat(&self, seconds: f64) -> f64 {
        match self {
            Self::Linear { per_beat, .. } => seconds / per_beat,
            Self::Warp(markers) => along(markers, seconds, |m| m.1, |m| m.0),
        }
    }
}

/// `y` at `x` along the piecewise-linear `markers` (rising in `x`), the first and last segments
/// extended.
fn along(markers: &[(f64, f64)], x: f64, key: impl Fn(&(f64, f64)) -> f64, value: impl Fn(&(f64, f64)) -> f64) -> f64 {
    let i = markers.partition_point(|m| key(m) <= x).clamp(1, markers.len().max(2) - 1);
    let (Some(a), Some(b)) = (markers.get(i - 1), markers.get(i)) else { return x };
    let (x0, x1) = (key(a), key(b));
    if x1 <= x0 {
        return value(a);
    }
    value(a) + (x - x0) * (value(b) - value(a)) / (x1 - x0)
}

/// The audio region `content` plays, if it has one.
pub(crate) fn region(p: &Project, content: ContentId) -> Option<&AudioRegion> {
    p.content(content)?.lanes.iter().find_map(|l| match &l.data {
        LaneData::Audio(region) => Some(region),
        LaneData::Notes(_) => None,
    })
}

fn lane_audio(lane: &suhina_model::ContentLane) -> Option<&AudioRegion> {
    match &lane.data {
        LaneData::Audio(region) => Some(region),
        LaneData::Notes(_) => None,
    }
}

/// The audio region `content` plays on `track`: its lane for that track, or one that plays on
/// whichever track the clip is on, as the engine and ARA pick it; else the first audio lane.
pub(crate) fn region_on(p: &Project, content: ContentId, track: suhina_model::TrackId) -> Option<&AudioRegion> {
    let lanes = &p.content(content)?.lanes;
    lanes
        .iter()
        .filter(|l| l.target.is_none_or(|t| t == track))
        .find_map(lane_audio)
        .or_else(|| lanes.iter().find_map(lane_audio))
}

/// The mapping of `clip`, at the project's tempo; `None` for a clip without audio.
pub(crate) fn mapping(p: &Project, clip: &Clip) -> Option<Mapping> {
    region(p, clip.content)?;
    Mapping::of(&clip.audio, p.transport.tempo_bpm)
}

/// Every clip of `p` (on the arrangement, in take lanes, in slots, in that order) with its
/// mapping, for [`follow_all`] after a tempo change.
pub(crate) fn mappings(p: &Project) -> Vec<Option<Mapping>> {
    let lanes = p.arrangement.lanes.iter().flat_map(|l| l.clips.iter());
    let takes = p.arrangement.takes.iter().flat_map(|t| t.clips.iter());
    let slots = p.launcher.columns.iter().flat_map(|c| c.slots.iter().filter_map(|s| s.clip.as_ref()));
    lanes.chain(takes).chain(slots).map(|c| mapping(p, c)).collect()
}

/// Moves `clip`'s offset, length, loop and gain envelope points from `old` to `new`, so it plays
/// the same seconds of its file and each point stays on the sound it was put on; its start stays.
fn remap(clip: &mut Clip, old: &Mapping, new: &Mapping) {
    let convert = |beat: f64| new.beat(old.seconds(beat));
    // The clip's length is in project beats; the content beats it plays are that times the rate.
    let end = clip.offset + clip.length * old.rate();
    let (offset, end) = (convert(clip.offset), convert(end));
    if !(offset.is_finite() && end.is_finite()) {
        return;
    }
    clip.offset = offset.max(0.0);
    clip.length = ((end - clip.offset) / new.rate()).max(SHORTEST);
    if let Some(range) = clip.loop_range {
        let (start, end) = (convert(range.start), convert(range.end));
        if start.is_finite() && end.is_finite() && end > start {
            clip.loop_range = Some(LoopRange { start: start.max(0.0), end });
        }
    }
    // The envelope's points are content beats, as the offset is: each goes to the beat its
    // second of the file is at now (warp turned on or off, or the tempo of unwarped audio). Left
    // on their beats, they slid off the sound they were put on as the waveform stretched under
    // them. The mapping rises, so the points stay in order.
    for point in &mut clip.audio.gain_envelope {
        let beat = convert(point.beat);
        if beat.is_finite() {
            point.beat = beat.max(0.0);
        }
    }
    clip.fit_fades();
}

/// Keeps `clip` on the same audio after its mapping went from `old` to what its audio state and
/// `tempo_bpm` make it now.
pub(crate) fn follow(clip: &mut Clip, old: &Mapping, tempo_bpm: f64) {
    let Some(new) = Mapping::of(&clip.audio, tempo_bpm) else { return };
    if &new != old {
        remap(clip, old, &new);
    }
}

/// [`follow`] for every clip, from the mappings [`mappings`] took before the project's tempo
/// changed.
pub(crate) fn follow_all(p: &mut Project, before: &[Option<Mapping>]) {
    let tempo = p.transport.tempo_bpm;
    let lanes = p.arrangement.lanes.iter_mut().flat_map(|l| l.clips.iter_mut());
    let takes = p.arrangement.takes.iter_mut().flat_map(|t| t.clips.iter_mut());
    let slots = p.launcher.columns.iter_mut().flat_map(|c| c.slots.iter_mut().filter_map(|s| s.clip.as_mut()));
    for (clip, old) in lanes.chain(takes).chain(slots).zip(before) {
        if let Some(old) = old {
            follow(clip, old, tempo);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use suhina_model::{GainPoint, StretchMode, WarpMarker};

    fn region(warped: bool, markers: &[(f64, f64)]) -> ClipAudio {
        ClipAudio {
            warped,
            stretch: StretchMode::Smooth,
            warp_markers: markers.iter().map(|&(time, audio_time)| WarpMarker { time, audio_time }).collect(),
            ..ClipAudio::default()
        }
    }

    #[test]
    fn a_four_beat_loop_at_124_bpm_spans_what_its_seconds_take_at_90_with_warp_off() {
        // 4 beats at 124 BPM: 1.935484 s.
        let seconds = 4.0 * 60.0 / 124.0;
        let warped = Mapping::of(&region(true, &[(0.0, 0.0), (4.0, seconds)]), 90.0)
            .unwrap_or(Mapping::Linear { per_beat: 0.0, rate: 1.0 });
        let unwarped = Mapping::of(&region(false, &[(0.0, 0.0), (4.0, seconds)]), 90.0)
            .unwrap_or(Mapping::Linear { per_beat: 0.0, rate: 1.0 });
        let mut clip = Clip::new(ContentId(1), 8.0, 4.0);
        remap(&mut clip, &warped, &unwarped);
        assert_eq!(clip.start, 8.0, "the start stays");
        assert!((clip.length - 2.903226).abs() < 1e-5, "{}", clip.length);
        // And back: 4 beats again.
        remap(&mut clip, &unwarped, &warped);
        assert!((clip.length - 4.0).abs() < 1e-9, "{}", clip.length);
        // A trimmed, looped clip keeps the same part of the file.
        let mut trimmed = Clip {
            offset: 1.0,
            loop_range: Some(LoopRange { start: 1.0, end: 3.0 }),
            ..Clip::new(ContentId(1), 0.0, 2.0)
        };
        remap(&mut trimmed, &warped, &unwarped);
        let scale = 90.0 / 124.0;
        assert!((trimmed.offset - scale).abs() < 1e-9 && (trimmed.length - 2.0 * scale).abs() < 1e-9);
        let range = trimmed.loop_range.unwrap_or(LoopRange { start: 0.0, end: 0.0 });
        assert!((range.start - scale).abs() < 1e-9 && (range.end - 3.0 * scale).abs() < 1e-9);
    }

    #[test]
    fn the_envelope_points_stay_on_their_sound_when_warp_turns_off_and_on() {
        let seconds = 4.0 * 60.0 / 124.0;
        let warped = Mapping::of(&region(true, &[(0.0, 0.0), (4.0, seconds)]), 90.0)
            .unwrap_or(Mapping::Linear { per_beat: 0.0, rate: 1.0 });
        let unwarped = Mapping::of(&region(false, &[(0.0, 0.0), (4.0, seconds)]), 90.0)
            .unwrap_or(Mapping::Linear { per_beat: 0.0, rate: 1.0 });
        let mut clip = Clip::new(ContentId(1), 0.0, 4.0);
        clip.audio.gain_envelope = vec![GainPoint::new(1.0, 0.0), GainPoint::new(2.0, -6.0)];
        // The second of the file each point is on, before and after: the same.
        let second = |m: &Mapping, beat: f64| m.seconds(beat);
        let before: Vec<f64> = clip.audio.gain_envelope.iter().map(|p| second(&warped, p.beat)).collect();
        remap(&mut clip, &warped, &unwarped);
        let after: Vec<f64> = clip.audio.gain_envelope.iter().map(|p| second(&unwarped, p.beat)).collect();
        for (b, a) in before.iter().zip(&after) {
            assert!((b - a).abs() < 1e-9, "{before:?} -> {after:?}");
        }
        assert!((clip.audio.gain_envelope[0].beat - 90.0 / 124.0).abs() < 1e-9);
        assert_eq!(clip.audio.gain_envelope[1].db, -6.0, "levels stay");
        // And back to the beats they had.
        remap(&mut clip, &unwarped, &warped);
        assert!((clip.audio.gain_envelope[1].beat - 2.0).abs() < 1e-9);
    }

    #[test]
    fn markers_that_do_not_rise_have_no_mapping_to_remap_by() {
        let backwards = region(true, &[(0.0, 2.0), (4.0, 1.0)]);
        assert_eq!(Mapping::of(&backwards, 120.0), None);
        assert_eq!(Mapping::of(&backwards, 0.0), None);
    }

    /// A transpose repitches the time played, not where the clip starts in its file: at 120 BPM
    /// an offset of 2 is second 1 with or without +12, and the clip plays half as long (#916).
    #[test]
    fn a_transpose_keeps_the_start_and_halves_the_length() {
        let plain = Mapping::of(&region(false, &[]), 120.0).unwrap_or(Mapping::Linear { per_beat: 0.0, rate: 1.0 });
        let octave = Mapping::of(&ClipAudio { transpose: 12.0, ..region(false, &[]) }, 120.0)
            .unwrap_or(Mapping::Linear { per_beat: 0.0, rate: 1.0 });
        let mut clip = Clip { offset: 2.0, ..Clip::new(ContentId(1), 0.0, 4.0) };
        remap(&mut clip, &plain, &octave);
        assert!((clip.offset - 2.0).abs() < 1e-9, "{}", clip.offset);
        assert!((clip.length - 2.0).abs() < 1e-9, "{}", clip.length);
    }
}

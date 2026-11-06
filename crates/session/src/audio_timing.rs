// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How an audio clip's content beats map to its file's seconds, as the engine plays it, and
//! keeping each clip on the same audio when that mapping changes (#830): warp turned on or off,
//! a transpose that repitches unwarped audio, or the tempo unwarped audio is counted in. The
//! clip's start on the arrangement stays; its offset, length and loop move so they still cover
//! the same seconds of the file, so the clip is drawn as long as what it plays.

use suhina_model::{AudioRegion, Clip, ContentId, LaneData, LoopRange, Project};

/// Shortest a clip is made by a remap, in beats.
const SHORTEST: f64 = 1.0 / 64.0;
/// Most warp markers read from one region, as the engine reads (`MAX_WARP_MARKERS` in
/// `crates/engine/src/audio.rs`): a project with more cannot make an edit copy them all.
const MAX_WARP_MARKERS: usize = 100_000;

/// A content's mapping from beats to file seconds.
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
    /// `region`'s mapping at `tempo_bpm`, as the engine plays it (`crates/engine/src/audio.rs`).
    pub(crate) fn of(region: &AudioRegion, tempo_bpm: f64) -> Option<Self> {
        if !(tempo_bpm.is_finite() && tempo_bpm > 0.0) {
            return None;
        }
        let mut markers: Vec<(f64, f64)> = region
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
        if region.warped && markers.len() >= 2 {
            // The engine plays any two markers; only markers that rise in both can be read back
            // from seconds, so a clip on markers that fall is left as it is (no mapping).
            let rising = markers.windows(2).all(|w| w[1].0 > w[0].0 && w[1].1 > w[0].1);
            return rising.then_some(Self::Warp(markers));
        }
        let rate = if region.warped { 1.0 } else { 2f64.powf(region.transpose / 12.0) };
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

/// The mapping of the audio `content` holds, at the project's tempo; `None` for no audio.
pub(crate) fn mapping(p: &Project, content: ContentId) -> Option<Mapping> {
    let region = p.content(content)?.lanes.iter().find_map(|l| match &l.data {
        LaneData::Audio(region) => Some(region),
        LaneData::Notes(_) => None,
    })?;
    Mapping::of(region, p.transport.tempo_bpm)
}

/// Every audio content of `p` with its mapping, for [`follow_all`] after a tempo change.
pub(crate) fn mappings(p: &Project) -> Vec<(ContentId, Mapping)> {
    p.contents.iter().filter_map(|c| Some((c.id, mapping(p, c.id)?))).collect()
}

/// Moves `clip`'s offset, length and loop from `old` to `new`, so it plays the same seconds of
/// its file; its start stays.
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
    clip.fit_fades();
}

/// Keeps every clip that plays `content` (on the arrangement, in slots and in take lanes) on the
/// same audio after its mapping went from `old` to what it is now.
pub(crate) fn follow(p: &mut Project, content: ContentId, old: &Mapping) {
    let Some(new) = mapping(p, content) else { return };
    if &new == old {
        return;
    }
    let lanes = p.arrangement.lanes.iter_mut().flat_map(|l| l.clips.iter_mut());
    let takes = p.arrangement.takes.iter_mut().flat_map(|t| t.clips.iter_mut());
    let slots = p.launcher.columns.iter_mut().flat_map(|c| c.slots.iter_mut().filter_map(|s| s.clip.as_mut()));
    for clip in lanes.chain(takes).chain(slots).filter(|c| c.content == content) {
        remap(clip, old, &new);
    }
}

/// [`follow`] for every content in `before`, after the project's tempo changed.
pub(crate) fn follow_all(p: &mut Project, before: &[(ContentId, Mapping)]) {
    for (content, old) in before {
        follow(p, *content, old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use suhina_model::{StretchMode, WarpMarker};

    fn region(warped: bool, markers: &[(f64, f64)]) -> AudioRegion {
        AudioRegion {
            warped,
            stretch: StretchMode::Smooth,
            warp_markers: markers.iter().map(|&(time, audio_time)| WarpMarker { time, audio_time }).collect(),
            ..AudioRegion::new("a.wav")
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
        let octave = Mapping::of(&AudioRegion { transpose: 12.0, ..region(false, &[]) }, 120.0)
            .unwrap_or(Mapping::Linear { per_beat: 0.0, rate: 1.0 });
        let mut clip = Clip { offset: 2.0, ..Clip::new(ContentId(1), 0.0, 4.0) };
        remap(&mut clip, &plain, &octave);
        assert!((clip.offset - 2.0).abs() < 1e-9, "{}", clip.offset);
        assert!((clip.length - 2.0).abs() < 1e-9, "{}", clip.length);
    }
}

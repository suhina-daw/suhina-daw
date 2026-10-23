// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The clip the piano roll edits, and where it plays: on an arrangement lane or in a slot.

use super::*;

/// A MIDI clip as the piano roll shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct PianoClip {
    /// The params that name the clip in a `note.*` command: `track` and `index` or `scene`.
    pub address: Value,
    pub notes: Vec<Note>,
    /// Where the part of the content the clip plays ends, in beats (its loop's end when it
    /// loops): the grid past it is shaded.
    pub length: f64,
    pub color: Color32,
    /// Beats in a bar.
    pub bar_beats: f64,
    /// The fixed grid step in beats (the piano roll's own value or the control bar's); 0 is none
    /// or an adaptive grid, whose step [`grid_menu`](Self::grid_menu) gives at each zoom.
    pub grid: f64,
    /// Where the clip is on the arrangement, to find the playhead in its content.
    pub placement: Placement,
    /// The grid value as the toolbar's grid menu shows it and ticks it, and whether things snap.
    pub grid_menu: PianoGrid,
    /// The grid is the control bar's (Settings › Piano roll grid): the menu offers only what the
    /// control bar can hold.
    pub grid_follows: bool,
    /// The clip's start marker, length and loop, as the ruler shows and changes them.
    pub shape: ClipShape,
    /// The notes of other clips drawn faintly behind this clip's own, and the clips the ghost
    /// menu offers (› Ghost notes); the window fills them in.
    pub ghosts: Ghosts,
    /// The scale of the clip's content, whose keys the rows mark (› Scale and key).
    pub scale: Option<Scale>,
}

/// Where a clip plays its content on the arrangement.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Placement {
    /// The arrangement beat it starts at, and how long it is.
    pub start: f64,
    pub length: f64,
    /// Where in the content it starts playing, and the part it loops (start, end), if any.
    pub offset: f64,
    pub loop_range: Option<(f64, f64)>,
}

impl Placement {
    /// The content beat played at arrangement beat `at`; `None` outside the clip.
    pub fn content_beat(&self, at: f64) -> Option<f64> {
        let elapsed = at - self.start;
        if !(elapsed >= 0.0 && elapsed < self.length) {
            return None;
        }
        let mut beat = self.offset + elapsed;
        if let Some((start, end)) = self.loop_range
            && end > start
            && beat >= end
        {
            beat = start + (beat - start) % (end - start);
        }
        beat.is_finite().then_some(beat.max(0.0))
    }

    /// The arrangement beat that plays content beat `beat`; `None` when the clip never plays it.
    /// The inverse of [`Placement::content_beat`], used to play from the point under the pointer.
    pub fn arrangement_beat(&self, beat: f64) -> Option<f64> {
        if !(beat.is_finite() && self.start.is_finite() && self.length.is_finite() && self.offset.is_finite()) {
            return None;
        }
        let mut content = beat.max(0.0);
        if let Some((start, end)) = self.loop_range
            && end > start
            && content >= end
        {
            content = start + (content - start) % (end - start);
        }
        let elapsed = content - self.offset;
        // A beat before the clip's offset is not played by it; one past its end lands on the end.
        if elapsed < 0.0 {
            return None;
        }
        let at = self.start + elapsed;
        (at < self.start + self.length).then_some(at.max(self.start))
    }
}

/// The piano roll for the arrangement clip `id` on `track`, with the grid `quantization`;
/// `None` when the clip is missing or plays audio.
pub fn arrangement_clip(
    project: &Project,
    track: TrackId,
    id: ClipId,
    quantization: Quantization,
) -> Option<PianoClip> {
    let lane = project.arrangement.lanes.iter().find(|l| l.track == Some(track))?;
    let clip = lane.clips.iter().find(|c| c.id == id)?;
    let mut piano = piano_clip_from(project, track, clip, quantization)?;
    // The note commands name the clip by its id; the track is there for what reads it.
    piano.address = json!({ "track": track.0, "clip": id.0 });
    // Up to the next clip on the lane, as `clip.setLength` allows.
    let next =
        lane.clips.iter().filter(|c| c.id != id && c.start > clip.start).map(|c| c.start).fold(f64::INFINITY, f64::min);
    piano.shape.room = next.is_finite().then_some(next - clip.start);
    Some(piano)
}

/// The piano roll for a launcher slot's clip: `scene` is 0-based, as the session's selection
/// carries it; the `note.*` commands name the clip by `scene` (1-based). A slot's clip plays
/// from its launch, not from an arrangement position: its placement starts at the launch and
/// never ends, so the beats since the launch map through it — the playhead and paste at the
/// playhead take those beats.
pub fn launcher_clip(project: &Project, track: TrackId, scene: usize, quantization: Quantization) -> Option<PianoClip> {
    let clip = project.launcher.columns.iter().find(|c| c.track == track)?.slots.get(scene)?.clip.as_ref()?;
    let mut piano = piano_clip_from(project, track, clip, quantization)?;
    piano.placement = Placement {
        start: 0.0,
        length: f64::INFINITY,
        offset: clip.offset,
        loop_range: clip.loop_range.map(|l| (l.start, l.end)),
    };
    piano.address = json!({ "track": track.0, "scene": scene + 1 });
    Some(piano)
}

/// What `arrangement_clip` and `launcher_clip` share: the content's notes and shape as the
/// piano roll shows them.
pub(super) fn piano_clip_from(
    project: &Project,
    track: TrackId,
    clip: &Clip,
    quantization: Quantization,
) -> Option<PianoClip> {
    let content = project.content(clip.content)?;
    let mut notes = None;
    for lane in content.lanes.iter().filter(|l| l.target.is_none_or(|t| t == track)) {
        match &lane.data {
            LaneData::Audio(_) => return None,
            LaneData::Notes(n) if notes.is_none() => notes = Some(n.clone()),
            LaneData::Notes(_) => {}
        }
    }
    if content.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))) {
        return None;
    }
    let position = project.tracks.iter().position(|t| t.id == track)?;
    let color = project.tracks.get(position).and_then(|t| t.color).unwrap_or_else(|| default_track_colour(position));
    let ts = project.transport.time_signature;
    let bar_beats =
        if ts.denominator == 0 { 4.0 } else { f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator) };
    Some(PianoClip {
        address: json!({ "track": track.0 }),
        notes: notes.unwrap_or_default(),
        length: content_end(clip.loop_range.map(|l| l.end), clip.offset, clip.length, content.length),
        color: Color32::from_rgb(color.0, color.1, color.2),
        bar_beats,
        grid: grid_step(quantization, bar_beats),
        placement: Placement {
            start: clip.start,
            length: clip.length,
            offset: clip.offset,
            loop_range: clip.loop_range.map(|l| (l.start, l.end)),
        },
        grid_menu: PianoGrid::default(),
        grid_follows: false,
        shape: ClipShape {
            offset: clip.offset,
            length: clip.length,
            loop_range: clip.loop_range.map(|l| (l.start, l.end)),
            room: None,
        },
        ghosts: Ghosts::default(),
        scale: content.scale,
    })
}

/// Where the part of the content a clip plays ends, in content beats: its loop's end when it
/// loops, else its offset and length; at least a beat.
pub(super) fn content_end(loop_end: Option<f64>, offset: f64, length: f64, content_length: f64) -> f64 {
    let end = loop_end.unwrap_or(offset + length);
    let end = if end.is_finite() && end > 0.0 { end } else { content_length };
    if end.is_finite() { end.clamp(1.0, f64::from(BEAT_LIMIT)) } else { 4.0 }
}

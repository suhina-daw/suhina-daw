// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The notes a track's arrangement clips play, as plug-in events, for listening to one
//! instrument device of a project (`plugin run-device`). The engine plays plug-ins in a track's
//! chain itself once they are part of it; until then this reads the clips the way the engine's
//! renderer does: each clip's window of its content, a loop range repeating until the clip ends,
//! through the project's tempo map.

use suhina_engine::model::*;
use suhina_plugins::Event;

/// Most notes read, so a malformed project cannot grow the list without end.
const MAX_NOTES: usize = 100_000;

/// Note-on and note-off events of the track whose channel is `channel`, from `from_beat` for
/// `frames` frames at `sample_rate`, sorted by frame. Frames count from `from_beat`.
pub fn events(project: &Project, channel: ChannelId, from_beat: f64, frames: u64, sample_rate: u32) -> Vec<Event> {
    let Some(track) = project.tracks.iter().find(|t| t.channel == Some(channel)) else { return Vec::new() };
    let map = project.transport.tempo_map(f64::from(sample_rate));
    let from_frame = map.time_at(from_beat);
    let mut notes: Vec<(f64, f64, &Note)> = Vec::new();
    for lane in project.arrangement.lanes.iter().filter(|l| l.track == Some(track.id)) {
        for clip in lane.clips.iter().filter(|c| c.enabled) {
            let Some(content) = project.contents.iter().find(|c| c.id == clip.content) else { continue };
            for content_lane in content.lanes.iter().filter(|l| l.target.is_none() || l.target == Some(track.id)) {
                if let LaneData::Notes(list) = &content_lane.data {
                    place(clip, list, &mut notes);
                }
            }
        }
    }
    let end_beat = map.beat_at(from_frame + frames as f64);
    let frame = |beat: f64| (map.time_at(beat.max(from_beat)) - from_frame).max(0.0).round() as u32;
    let mut events = Vec::new();
    for (start, end, note) in notes {
        if end <= from_beat || start >= end_beat {
            continue;
        }
        let velocity = note.velocity.clamp(0.0, 1.0);
        events.push(Event::note_on(frame(start), note.channel.min(15), note.key.min(127), velocity));
        if end < end_beat {
            events.push(Event::note_off(frame(end), note.channel.min(15), note.key.min(127), note.release_velocity));
        }
    }
    // Note-offs before note-ons at the same frame, so a repeated key starts again.
    events.sort_by_key(|e| (e.frame, matches!(e.kind, suhina_plugins::EventKind::NoteOn { .. })));
    events
}

/// Adds the notes `clip` plays from `notes` as (start, end) in arrangement beats.
fn place<'a>(clip: &Clip, notes: &'a [Note], out: &mut Vec<(f64, f64, &'a Note)>) {
    if !(clip.length.is_finite() && clip.length > 0.0 && clip.start.is_finite() && clip.offset.is_finite()) {
        return;
    }
    let clip_end = clip.start + clip.length;
    // Each pass maps the content range [from, to) to the clip position `at`.
    let mut pass = |from: f64, to: f64, at: f64| {
        for note in notes {
            if out.len() >= MAX_NOTES {
                return;
            }
            if !(note.time.is_finite() && note.duration.is_finite()) || note.time < from || note.time >= to {
                continue;
            }
            let start = clip.start + at + (note.time - from);
            if start >= clip_end {
                continue;
            }
            out.push((start, (start + note.duration.max(0.0)).min(clip_end), note));
        }
    };
    match clip.loop_range {
        Some(range) if range.start.is_finite() && range.end.is_finite() && range.end > range.start => {
            let first = clip.offset.clamp(range.start, range.end);
            pass(first, range.end, 0.0);
            let mut at = range.end - first;
            let mut passes = 0;
            while at < clip.length && passes < 100_000 {
                pass(range.start, range.end, at);
                at += range.end - range.start;
                passes += 1;
            }
        }
        _ => pass(clip.offset, f64::INFINITY, 0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use suhina_plugins::EventKind;

    fn project(clips: Vec<Clip>, notes: Vec<Note>) -> Project {
        Project {
            tracks: vec![Track {
                content: ContentKinds { notes: true, audio: false },
                channel: Some(ChannelId(1)),
                ..Track::new(TrackId(1), "Lead")
            }],
            contents: vec![ClipContent {
                id: ContentId(1),
                name: "Riff".into(),
                length: 4.0,
                lanes: vec![ContentLane { target: None, data: LaneData::Notes(notes) }],
                extensions: Extensions::default(),
                scale: None,
            }],
            arrangement: Arrangement {
                lanes: vec![ArrangementLane {
                    id: Default::default(),
                    name: "Lead".into(),
                    track: Some(TrackId(1)),
                    clips,
                }],
                markers: Vec::new(),
                takes: Vec::new(),
            },
            ..Project::default()
        }
    }

    fn clip(start: f64, length: f64, loop_range: Option<LoopRange>) -> Clip {
        Clip { loop_range, ..Clip::new(ContentId(1), start, length) }
    }

    fn note(time: f64, key: u8) -> Note {
        Note { release_velocity: 0.25, ..Note::new(time, 0.5, key, 0.75) }
    }

    fn summary(events: &[Event]) -> Vec<(u32, bool, u8)> {
        events
            .iter()
            .map(|e| match e.kind {
                EventKind::NoteOn { key, .. } => (e.frame, true, key),
                EventKind::NoteOff { key, .. } => (e.frame, false, key),
                _ => (e.frame, false, 0),
            })
            .collect()
    }

    #[test]
    fn a_clips_notes_become_events_at_the_projects_tempo() {
        // 120 bpm at 1000 frames a second: 500 frames a beat.
        let p = project(vec![clip(2.0, 4.0, None)], vec![note(0.0, 60), note(1.0, 64)]);
        let events = events(&p, ChannelId(1), 0.0, 10_000, 1_000);
        assert_eq!(summary(&events), [(1_000, true, 60), (1_250, false, 60), (1_500, true, 64), (1_750, false, 64)]);
        // From beat 2.5 for 1.2 beats: the first note started before and is left out.
        let later = events_from(&p, 2.5, 600);
        assert_eq!(summary(&later), [(250, true, 64), (500, false, 64)]);
        // A note still sounding when the window ends gets no note-off.
        assert_eq!(summary(&events_from(&p, 2.5, 400)), [(250, true, 64)]);
        // Another channel has no track, so nothing.
        assert!(super::events(&p, ChannelId(9), 0.0, 10_000, 1_000).is_empty());
    }

    fn events_from(p: &Project, beat: f64, frames: u64) -> Vec<Event> {
        events(p, ChannelId(1), beat, frames, 1_000)
    }

    #[test]
    fn a_loop_repeats_until_the_clip_ends() {
        // A 5-beat clip looping beats 0–2: the note at 0.5 plays at 0.5, 2.5 and 4.5.
        let p = project(vec![clip(0.0, 5.0, Some(LoopRange { start: 0.0, end: 2.0 }))], vec![note(0.5, 60)]);
        let ons: Vec<u32> = events_from(&p, 0.0, 100_000)
            .iter()
            .filter(|e| matches!(e.kind, EventKind::NoteOn { .. }))
            .map(|e| e.frame)
            .collect();
        assert_eq!(ons, [250, 1_250, 2_250]);
    }
}

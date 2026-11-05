// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A clip's edges dragged (`docs/spec/arrangement-editing.md` › Clip edges): trimmed within its
//! audio, made longer when it loops or plays notes, and stretched.
#![allow(clippy::unwrap_used, clippy::panic)] // test helpers outside #[test] functions

use serde_json::json;
use suhina_model::*;
use suhina_session::{Session, blank_project};
use suhina_test_support::ProjectBuilder;

use crate::support::run;

/// The blank project with an 8-beat audio clip (4 s at 120 BPM) at beat 4 on track 3, and a
/// 4-beat MIDI clip at beat 0 on track 1.
fn project() -> Project {
    let region = AudioRegion {
        file: "loop.wav".into(),
        absolute_file: None,
        sample_rate: Some(48_000),
        channels: Some(1),
        duration_seconds: Some(4.0),
        warped: false,
        stretch: StretchMode::default(),
        warp_markers: Vec::new(),
        gain: 1.0,
        transpose: 0.0,
        stream: None,
    };
    ProjectBuilder::from(blank_project())
        .audio(1, "Loop", 8.0, region)
        .notes(2, "Riff", 4.0, vec![Note::new(0.0, 1.0, 60, 0.8)])
        .clip(3, Clip::new(ContentId(1), 4.0, 8.0))
        .clip(1, Clip::new(ContentId(2), 0.0, 4.0))
        .build_without_ids()
}

fn first(session: &Session, track: u32) -> Clip {
    let p = session.project();
    p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(track))).unwrap().clips[0].clone()
}

#[test]
fn an_unlooped_audio_clip_shortens_but_grows_no_further_than_its_audio() {
    let mut session = Session::new(project());
    run(&mut session, "clip.trim", json!({ "track": 3, "index": 0, "end": 10.0 })).unwrap();
    assert_eq!(first(&session, 3).length, 6.0, "shorter");
    let out = run(&mut session, "clip.trim", json!({ "track": 3, "index": 0, "end": 30.0 })).unwrap();
    assert_eq!(out["end"], 12.0, "the right edge stops where the audio ends");
    assert_eq!(first(&session, 3).length, 8.0);
}

#[test]
fn the_left_edge_moves_the_content_with_it_and_stops_at_its_start() {
    let mut session = Session::new(project());
    run(&mut session, "clip.trim", json!({ "track": 3, "index": 0, "start": 6.0 })).unwrap();
    let clip = first(&session, 3);
    assert_eq!((clip.start, clip.length, clip.offset), (6.0, 6.0, 2.0), "what plays stays where it was");
    run(&mut session, "clip.trim", json!({ "track": 3, "index": 0, "start": 0.0 })).unwrap();
    let clip = first(&session, 3);
    assert_eq!((clip.start, clip.offset), (4.0, 0.0), "no earlier than the audio's start");
}

#[test]
fn a_looping_audio_clip_goes_on_as_far_as_it_is_dragged() {
    let mut session = Session::new(project());
    // The loop handle: the loop turns on over what plays, then the edge goes on.
    let out = run(&mut session, "clip.trim", json!({ "track": 3, "index": 0, "end": 36.0, "loop": true })).unwrap();
    assert_eq!(out["loop"], true);
    let clip = first(&session, 3);
    assert_eq!(clip.length, 32.0, "four times its audio");
    assert_eq!(clip.loop_range, Some(LoopRange { start: 0.0, end: 8.0 }));
    // Looping already, a plain drag of the edge does the same.
    run(&mut session, "clip.trim", json!({ "track": 3, "index": 0, "end": 44.0 })).unwrap();
    assert_eq!(first(&session, 3).length, 40.0);
}

#[test]
fn an_edge_dragged_past_the_other_leaves_the_least_length() {
    let mut session = Session::new(project());
    run(&mut session, "clip.trim", json!({ "track": 1, "index": 0, "start": 10.0 })).unwrap();
    let clip = first(&session, 1);
    assert!(clip.length > 0.0 && clip.length < 0.1, "never negative: {}", clip.length);
    assert!((clip.start + clip.length - 4.0).abs() < 1e-9, "the right edge stays");
    run(&mut session, "clip.trim", json!({ "track": 3, "index": 0, "end": 0.0 })).unwrap();
    assert!(first(&session, 3).length > 0.0);
}

#[test]
fn a_midi_clip_widens_and_its_content_with_it() {
    let mut session = Session::new(project());
    run(&mut session, "clip.trim", json!({ "track": 1, "index": 0, "end": 16.0 })).unwrap();
    assert_eq!(first(&session, 1).length, 16.0);
    assert_eq!(session.project().content(ContentId(2)).unwrap().length, 16.0, "the content grew to the clip");
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(first(&session, 1).length, 4.0);
}

#[test]
fn stretching_an_audio_clip_scales_its_audio_to_the_new_length() {
    let mut session = Session::new(project());
    let out = run(&mut session, "clip.stretch", json!({ "track": 3, "index": 0, "end": 20.0 })).unwrap();
    assert_eq!(out["factor"], 2.0);
    let clip = first(&session, 3);
    assert_eq!(clip.length, 16.0);
    let content = session.project().content(clip.content).unwrap();
    assert_eq!(content.length, 16.0);
    let LaneData::Audio(region) = &content.lanes[0].data else { panic!("audio") };
    assert!(region.warped, "stretching warps it");
    assert_eq!(region.warp_markers.last().map(|m| (m.time, m.audio_time)), Some((16.0, 4.0)), "4 s now over 16 beats");
    assert!(
        run(&mut session, "clip.stretch", json!({ "track": 1, "index": 0, "end": 8.0 })).is_err(),
        "MIDI does not stretch"
    );
}

#[test]
fn a_stretched_clip_whose_content_another_plays_gets_its_own() {
    let mut p = project();
    p.arrangement.lanes.iter_mut().find(|l| l.track == Some(TrackId(3))).unwrap().clips.push(Clip::new(
        ContentId(1),
        20.0,
        8.0,
    ));
    let mut session = Session::new(p);
    run(&mut session, "clip.stretch", json!({ "track": 3, "index": 0, "end": 8.0 })).unwrap();
    let lane = session.project().arrangement.lanes.iter().find(|l| l.track == Some(TrackId(3))).unwrap();
    assert_ne!(lane.clips[0].content, lane.clips[1].content, "the stretched one plays a copy");
    assert_eq!(session.project().content(ContentId(1)).unwrap().length, 8.0, "the other keeps its speed");
}

/// Warp turned on after a tempo change: the four-second loop, eight beats at the 120 bpm it was
/// imported at, keeps its own speed at the song's 90 bpm, so its end marker is at six beats.
#[test]
fn warp_turned_on_marks_the_audio_s_end_at_the_song_s_tempo_now() {
    let mut p = project();
    p.transport.tempo_bpm = 90.0;
    let mut session = Session::new(p);
    let clip = first(&session, 3).id.0;
    run(&mut session, "clip.setAudio", json!({ "clip": clip, "warp": true })).unwrap();
    let markers = match &session.project().content(ContentId(1)).unwrap().lanes[0].data {
        LaneData::Audio(region) => region.warp_markers.clone(),
        LaneData::Notes(_) => panic!("not audio"),
    };
    let end = markers.last().unwrap();
    assert!((end.time - 6.0).abs() < 1e-9, "{markers:?}");
    assert_eq!(end.audio_time, 4.0);
}

/// Two clips play one content where the tempo differs (120 BPM, then 90 from beat 20): warp
/// turned on for one gives it its own copy, so the other's speed does not change (#874).
#[test]
fn warp_turned_on_for_a_clip_whose_content_plays_at_another_tempo_elsewhere_gets_its_own_copy() {
    let mut p = project();
    p.transport.tempo_bpm = 120.0;
    p.transport.tempo_changes = vec![TempoPoint { beat: 20.0, bpm: 90.0, ramp: false }];
    p.arrangement.lanes.iter_mut().find(|l| l.track == Some(TrackId(3))).unwrap().clips.push(Clip::new(
        ContentId(1),
        24.0,
        8.0,
    ));
    let before = p.clone();
    let mut session = Session::new(p);
    let region =
        |session: &Session, content: ContentId| match &session.project().content(content).unwrap().lanes[0].data {
            LaneData::Audio(region) => region.clone(),
            LaneData::Notes(_) => panic!("not audio"),
        };
    let clips = |session: &Session| {
        session.project().arrangement.lanes.iter().find(|l| l.track == Some(TrackId(3))).unwrap().clips.clone()
    };
    let clip = first(&session, 3).id.0;
    run(&mut session, "clip.setAudio", json!({ "clip": clip, "warp": true })).unwrap();
    let [warped, other] = clips(&session).try_into().unwrap();
    assert_ne!(warped.content, other.content, "the warped clip plays a copy");
    assert_eq!(other.content, ContentId(1));
    let shared = region(&session, ContentId(1));
    assert!(!shared.warped && shared.warp_markers.is_empty(), "the other clip plays as it did");
    let end = *region(&session, warped.content).warp_markers.last().unwrap();
    assert!((end.time - 8.0).abs() < 1e-9 && end.audio_time == 4.0, "{end:?}: 4 s at 120 BPM");
    // The other now plays its content alone: warp on marks its end at 90 BPM, without a copy.
    let count = session.project().contents.len();
    run(&mut session, "clip.setAudio", json!({ "clip": other.id.0, "warp": true })).unwrap();
    assert_eq!(session.project().contents.len(), count);
    let end = *region(&session, ContentId(1)).warp_markers.last().unwrap();
    assert!((end.time - 6.0).abs() < 1e-9, "{end:?}: 4 s at 90 BPM");
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().contents, before.contents, "undone, the shared content is as it was");
    assert!(clips(&session).iter().all(|c| c.content == ContentId(1)), "both play the shared content again");
}

/// Warp turned off keeps the markers; when the two clips of that content then play where the
/// tempo differs, warp turned on again for one still gives it its own copy, so the other keeps
/// playing unwarped; one undo step puts both back on the shared content (#874).
#[test]
fn warp_turned_on_again_for_marked_audio_another_clip_plays_at_another_tempo_gets_its_own_copy() {
    let mut p = project();
    p.transport.tempo_bpm = 120.0;
    p.transport.tempo_changes = vec![TempoPoint { beat: 20.0, bpm: 90.0, ramp: false }];
    let markers = vec![WarpMarker { time: 0.0, audio_time: 0.0 }, WarpMarker { time: 8.0, audio_time: 4.0 }];
    if let LaneData::Audio(region) = &mut p.contents[0].lanes[0].data {
        region.warp_markers = markers.clone();
    }
    p.arrangement.lanes.iter_mut().find(|l| l.track == Some(TrackId(3))).unwrap().clips.push(Clip::new(
        ContentId(1),
        24.0,
        8.0,
    ));
    let before = p.clone();
    let mut session = Session::new(p);
    let region =
        |session: &Session, content: ContentId| match &session.project().content(content).unwrap().lanes[0].data {
            LaneData::Audio(region) => region.clone(),
            LaneData::Notes(_) => panic!("not audio"),
        };
    let clips = |session: &Session| {
        session.project().arrangement.lanes.iter().find(|l| l.track == Some(TrackId(3))).unwrap().clips.clone()
    };
    let clip = first(&session, 3).id.0;
    run(&mut session, "clip.setAudio", json!({ "clip": clip, "warp": true })).unwrap();
    let [warped, other] = clips(&session).try_into().unwrap();
    assert_ne!(warped.content, other.content, "the warped clip plays a copy");
    assert_eq!(other.content, ContentId(1));
    let shared = region(&session, ContentId(1));
    assert!(!shared.warped, "the other clip plays as it did");
    let copy = region(&session, warped.content);
    assert!(copy.warped && copy.warp_markers == markers, "the copy warps with the markers kept");
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().contents, before.contents, "undone, the shared content is as it was");
    assert!(clips(&session).iter().all(|c| c.content == ContentId(1)), "both play the shared content again");
}

#[test]
fn warp_turned_on_for_duplicates_at_the_same_tempo_keeps_their_content_shared() {
    let mut p = project();
    p.arrangement.lanes.iter_mut().find(|l| l.track == Some(TrackId(3))).unwrap().clips.push(Clip::new(
        ContentId(1),
        20.0,
        8.0,
    ));
    let mut session = Session::new(p);
    let clip = first(&session, 3).id.0;
    run(&mut session, "clip.setAudio", json!({ "clip": clip, "warp": true })).unwrap();
    let lane = session.project().arrangement.lanes.iter().find(|l| l.track == Some(TrackId(3))).unwrap();
    assert_eq!(lane.clips[0].content, lane.clips[1].content, "both follow, as with the gain");
}

#[test]
fn warp_turned_on_in_a_slot_marks_the_audio_s_end_at_the_song_s_tempo_not_the_arrangement_s() {
    // A slot's clip has no place in the arrangement: a tempo change there (120, then 60 from
    // beat 2) does not reach it, and its 4 s are 8 beats at the song's 120.
    let mut p = project();
    p.transport.tempo_bpm = 120.0;
    p.transport.tempo_changes = vec![TempoPoint { beat: 2.0, bpm: 60.0, ramp: false }];
    p.launcher.scenes = vec![Scene::default()];
    p.launcher.columns = vec![LauncherColumn {
        track: TrackId(3),
        slots: vec![Slot { clip: Some(Clip::new(ContentId(1), 0.0, 8.0)), has_stop: true }],
    }];
    let mut session = Session::new(p);
    run(&mut session, "clip.setAudio", json!({ "track": 3, "scene": 1, "warp": true })).unwrap();
    // The arrangement's clip of the same content plays at 120 then 60, so the slot's clip got its
    // own copy (#874).
    let slot = session.project().launcher.columns[0].slots[0].clip.as_ref().unwrap().content;
    assert_ne!(slot, ContentId(1));
    let markers = match &session.project().content(slot).unwrap().lanes[0].data {
        LaneData::Audio(region) => region.warp_markers.clone(),
        LaneData::Notes(_) => panic!("not audio"),
    };
    let end = markers.last().unwrap();
    assert!((end.time - 8.0).abs() < 1e-9, "{markers:?}");
    assert_eq!(end.audio_time, 4.0);
}

/// The clip properties' audio controls (#699): warp on gives the audio two warp markers and
/// keeps them when turned off again; the mode and transpose are set; one undo step each.
#[test]
fn an_audio_clips_warp_mode_and_transpose_are_set() {
    let mut session = Session::new(project());
    let clip = first(&session, 3).id.0;
    let region = |session: &Session| {
        let p = session.project();
        match &p.content(ContentId(1)).unwrap().lanes[0].data {
            LaneData::Audio(region) => region.clone(),
            LaneData::Notes(_) => panic!("not audio"),
        }
    };
    let out = run(&mut session, "clip.setAudio", json!({ "clip": clip, "warp": true })).unwrap();
    assert_eq!((out["warp"].as_bool(), out["mode"].as_str()), (Some(true), Some("smooth")));
    let warped = region(&session);
    assert!(warped.warped);
    assert_eq!(warped.warp_markers.len(), 2, "its start and its end");
    run(&mut session, "clip.setAudio", json!({ "clip": clip, "mode": "rhythmic", "transpose": -3.5 })).unwrap();
    let set = region(&session);
    assert_eq!((set.stretch, set.transpose), (StretchMode::Rhythmic, -3.5));
    run(&mut session, "clip.setAudio", json!({ "clip": clip, "warp": false })).unwrap();
    assert!(!region(&session).warped);
    assert_eq!(region(&session).warp_markers.len(), 2, "the markers are kept for when it warps again");
    // Refused: nothing to set, an unknown mode, too far, a MIDI clip.
    assert!(run(&mut session, "clip.setAudio", json!({ "clip": clip })).is_err());
    assert!(run(&mut session, "clip.setAudio", json!({ "clip": clip, "mode": "fast" })).is_err());
    assert!(run(&mut session, "clip.setAudio", json!({ "clip": clip, "transpose": 60 })).is_err());
    let midi = first(&session, 1).id.0;
    assert!(run(&mut session, "clip.setAudio", json!({ "clip": midi, "warp": true })).is_err());
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(region(&session).warped, "undone, warp is back on");
}

/// Warp off plays the file at its own speed, so the clip spans what its seconds take at the
/// song's tempo, and a tempo change resizes it; warp on follows the tempo, and the clip keeps its
/// start throughout (#830, #831).
#[test]
fn an_unwarped_clip_spans_its_seconds_at_the_tempo_and_keeps_its_start() {
    let mut session = Session::new(project());
    let clip = first(&session, 3);
    assert_eq!((clip.start, clip.length), (4.0, 8.0), "4 s at 120 BPM");
    // Unwarped at 60 BPM: the same 4 s are 4 beats.
    run(&mut session, "transport.setTempo", json!({ "bpm": 60 })).unwrap();
    let clip = first(&session, 3);
    assert_eq!(clip.start, 4.0);
    assert!((clip.length - 4.0).abs() < 1e-9, "{}", clip.length);
    // Warped from here it follows the tempo: back at 120 it stays 4 beats.
    run(&mut session, "clip.setAudio", json!({ "clip": clip.id.0, "warp": true })).unwrap();
    run(&mut session, "transport.setTempo", json!({ "bpm": 120 })).unwrap();
    assert!((first(&session, 3).length - 4.0).abs() < 1e-9, "{}", first(&session, 3).length);
    // Warp off again: the file's 4 s at 120 BPM are 8 beats, from the same start.
    run(&mut session, "clip.setAudio", json!({ "clip": clip.id.0, "warp": false })).unwrap();
    let unwarped = first(&session, 3);
    assert_eq!(unwarped.start, 4.0);
    assert!((unwarped.length - 8.0).abs() < 1e-9, "{}", unwarped.length);
    // Undo puts the length back with the warp switch.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!((first(&session, 3).length - 4.0).abs() < 1e-9);
    // Loop and gain edits never move it.
    run(&mut session, "clip.setLoop", json!({ "clip": clip.id.0, "loop": true })).unwrap();
    run(&mut session, "clip.setGain", json!({ "clip": clip.id.0, "db": -3.0 })).unwrap();
    assert_eq!(first(&session, 3).start, 4.0);
}

/// The selected clips dragged by one edge move it as far, each stopping at its own limits, in
/// one undo step (#956): the MIDI clip grows by eight beats, the unlooped audio clip only as far
/// as its audio goes.
#[test]
fn several_clips_move_the_same_edge_by_as_much_in_one_step() {
    let mut session = Session::new(suhina_test_support::with_ids(project()));
    let ids: Vec<u32> = [1, 3].iter().map(|t| first(&session, *t).id.0).collect();
    let clips: Vec<serde_json::Value> = ids.iter().map(|id| json!({ "clip": id })).collect();
    run(&mut session, "clip.trimMany", json!({ "clips": clips, "edge": "end", "by": 8.0 })).unwrap();
    assert_eq!(first(&session, 1).length, 12.0);
    assert!(first(&session, 3).length <= 8.0 + 1e-9, "the audio clip stops at its audio: {:?}", first(&session, 3));
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!((first(&session, 1).length, first(&session, 3).length), (4.0, 8.0));
    assert!(run(&mut session, "clip.trimMany", json!({ "clips": clips, "edge": "middle", "by": 1.0 })).is_err());
}

/// Two MIDI clips side by side, both selected, the later one's end dragged a bar on: the earlier
/// one stops where the later begins, so it does not run over it and cut it short.
#[test]
fn a_selected_clip_does_not_grow_over_the_next_one_of_the_selection() {
    let mut session = Session::new(suhina_test_support::with_ids(project()));
    let a = first(&session, 1);
    run(&mut session, "clip.create", json!({ "track": 1, "start": a.start + a.length })).unwrap();
    let lane = |s: &Session| {
        let mut clips: Vec<(f64, f64)> = s
            .project()
            .arrangement
            .lanes
            .iter()
            .find(|l| l.track == Some(suhina_engine::model::TrackId(1)))
            .map(|l| l.clips.iter().map(|c| (c.start, c.length)).collect())
            .unwrap_or_default();
        clips.sort_by(|x, y| x.0.total_cmp(&y.0));
        clips
    };
    let before = lane(&session);
    assert_eq!(before.len(), 2);
    let ids: Vec<serde_json::Value> = session
        .project()
        .arrangement
        .lanes
        .iter()
        .find(|l| l.track == Some(suhina_engine::model::TrackId(1)))
        .map(|l| l.clips.iter().map(|c| json!({ "clip": c.id.0 })).collect())
        .unwrap_or_default();
    run(&mut session, "clip.trimMany", json!({ "clips": ids, "edge": "end", "by": 4.0 })).unwrap();
    let after = lane(&session);
    assert_eq!(after.len(), 2, "both clips are still there: {after:?}");
    assert_eq!(after[0], before[0], "the earlier clip stops at the later one");
    assert_eq!((after[1].0, after[1].1), (before[1].0, before[1].1 + 4.0), "the later one grows a bar");
}

/// A MIDI clip's left edge goes on before its notes (#832): the clip starts earlier with empty
/// time, its note stays where it plays, and a content another clip plays is copied first.
#[test]
fn a_midi_clips_left_edge_goes_before_its_notes() {
    let mut project = project();
    // The riff on beat 4 too, playing the same content.
    if let Some(lane) = project.arrangement.lanes.iter_mut().find(|l| l.track == Some(TrackId(1))) {
        lane.clips.push(Clip::new(ContentId(2), 8.0, 4.0));
    }
    let mut session = Session::new(suhina_test_support::with_ids(project));
    run(&mut session, "clip.trim", json!({ "track": 1, "index": 1, "start": 6.0 })).unwrap();
    let p = session.project();
    let clips = &p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(1))).unwrap().clips;
    let moved = clips.iter().find(|c| (c.start - 6.0).abs() < 1e-9).unwrap();
    assert_eq!((moved.length, moved.offset), (6.0, 0.0));
    assert_ne!(moved.content, ContentId(2), "the shared content was copied");
    let content = p.content(moved.content).unwrap();
    assert_eq!(content.length, 6.0);
    let LaneData::Notes(notes) = &content.lanes[0].data else { panic!("notes") };
    assert_eq!(notes[0].time, 2.0, "the note still plays on beat 8");
    // The other clip keeps its content as it was.
    let LaneData::Notes(theirs) = &p.content(ContentId(2)).unwrap().lanes[0].data else { panic!("notes") };
    assert_eq!(theirs[0].time, 0.0);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().contents.len(), 2);
}

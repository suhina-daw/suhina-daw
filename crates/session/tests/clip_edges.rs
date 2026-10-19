// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A clip's edges dragged (`docs/spec/arrangement-editing.md` › Clip edges): trimmed within its
//! audio, made longer when it loops or plays notes, and stretched.
#![allow(clippy::unwrap_used, clippy::panic)] // test helpers outside #[test] functions

use serde_json::{Value, json};
use suhina_model::*;
use suhina_session::{Session, blank_project};

fn run(session: &mut Session, id: &str, params: Value) -> Result<Value, String> {
    session.execute(id, &params).map_err(|e| e.0)
}

/// The blank project with an 8-beat audio clip (4 s at 120 BPM) at beat 4 on track 3, and a
/// 4-beat MIDI clip at beat 0 on track 1.
fn project() -> Project {
    let mut p = blank_project();
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
    p.contents.push(ClipContent {
        id: ContentId(1),
        name: "Loop".into(),
        length: 8.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Audio(region) }],
        extensions: Extensions::default(),
        scale: None,
    });
    p.contents.push(ClipContent {
        id: ContentId(2),
        name: "Riff".into(),
        length: 4.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![Note::new(0.0, 1.0, 60, 0.8)]) }],
        extensions: Extensions::default(),
        scale: None,
    });
    p.arrangement.lanes.iter_mut().find(|l| l.track == Some(TrackId(3))).unwrap().clips =
        vec![Clip::new(ContentId(1), 4.0, 8.0)];
    p.arrangement.lanes.iter_mut().find(|l| l.track == Some(TrackId(1))).unwrap().clips =
        vec![Clip::new(ContentId(2), 0.0, 4.0)];
    p
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

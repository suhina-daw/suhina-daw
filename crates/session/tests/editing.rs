// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Splitting and consolidating arrangement clips (`docs/spec/arrangement-editing.md` ›
//! Splitting, Consolidating). The offline render is the oracle: a split or a consolidation
//! sounds as the clips did before it.
#![allow(clippy::unwrap_used, clippy::panic)] // test helpers outside #[test] functions

use std::path::{Path, PathBuf};
use suhina_engine::{RenderInputs, render_offline_from};

use serde_json::{Value, json};
use suhina_model::*;
use suhina_session::{Session, blank_project};

use crate::support::run;
use suhina_test_support::{WavFormat, write_wav};

const SR: u32 = 48_000;
/// Frames in one beat at 120 BPM.
const BEAT: usize = SR as usize / 2;

fn render(session: &Session) -> Vec<f32> {
    render_offline_from(session.project(), RenderInputs::from_audio(session.audio()), SR, 512, None).unwrap()
}

/// The largest difference between two renders over the frames both have, outside `skip`.
fn max_diff(a: &[f32], b: &[f32], skip: Option<std::ops::Range<usize>>) -> f32 {
    let frames = a.len().min(b.len()) / 2;
    (0..frames)
        .filter(|f| !skip.as_ref().is_some_and(|r| r.contains(f)))
        .map(|f| (a[f * 2] - b[f * 2]).abs().max((a[f * 2 + 1] - b[f * 2 + 1]).abs()))
        .fold(0.0, f32::max)
}

fn note(time: f64, duration: f64, key: u8) -> Note {
    Note::new(time, duration, key, 0.8)
}

fn clip(content: u32, start: f64, length: f64) -> Clip {
    Clip::new(ContentId(content), start, length)
}

/// The blank project with a notes content on track 1 ("1 MIDI") placed by `clips`.
fn midi_project(notes: Vec<Note>, clips: Vec<Clip>) -> Project {
    // Ids on its lanes, clips and scenes, as a session gives them.
    let mut project = midi_project_built(notes, clips);
    assert_eq!(project.assign_ids(), Ok(()));
    project
}

fn midi_project_built(notes: Vec<Note>, clips: Vec<Clip>) -> Project {
    let mut p = blank_project();
    p.contents.push(ClipContent {
        id: ContentId(1),
        name: "Riff".into(),
        length: 4.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Notes(notes) }],
        extensions: Extensions::default(),
        scale: None,
    });
    let lane = p.arrangement.lanes.iter_mut().find(|l| l.track == Some(TrackId(1))).unwrap();
    lane.clips = clips;
    p
}

fn lane_clips(session: &Session, track: u32) -> Vec<Clip> {
    let p = session.project();
    p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(track))).unwrap().clips.clone()
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-editing-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A mono WAV file of `seconds` of a sine at `hz`.
fn sine(dir: &Path, name: &str, seconds: f64, hz: f64) -> PathBuf {
    let path = dir.join(name);
    let frames = (seconds * f64::from(SR)) as usize;
    write_wav(&path, SR, 1, WavFormat::Float32, &suhina_test_support::sine(hz, f64::from(SR), frames, 0.5));
    path
}

/// A saved blank project in `dir` with the given files placed one after another on track 3
/// ("3 Audio") from `start`.
fn audio_session(dir: &Path, files: &[&Path], start: f64) -> Session {
    let mut session = Session::new(blank_project());
    let paths: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    run(&mut session, "clip.addFiles", json!({ "paths": paths, "track": 3, "start": start })).unwrap();
    session.save(&dir.join("song.suhina"), false).unwrap();
    session
}

#[test]
fn a_split_midi_clip_plays_as_it_did() {
    // A looped clip with its start marker inside the loop; no note crosses the cut.
    let notes = vec![note(0.0, 0.5, 60), note(1.0, 0.5, 64), note(2.0, 0.5, 67), note(3.0, 0.5, 72)];
    let mut looped = clip(1, 2.0, 10.0);
    looped.loop_range = Some(LoopRange { start: 0.0, end: 4.0 });
    looped.offset = 1.0;
    let mut session = Session::new(midi_project(notes, vec![looped]));
    let before = render(&session);
    let out = run(&mut session, "clip.split", json!({ "track": 1, "index": 0, "at": 6.75 })).unwrap();
    assert_eq!((out["first"].as_u64(), out["second"].as_u64()), (Some(0), Some(1)));
    let clips = lane_clips(&session, 1);
    assert_eq!(clips.len(), 2);
    assert_eq!((clips[0].start, clips[0].length, clips[0].offset), (2.0, 4.75, 1.0));
    // 4.75 beats from content beat 1 is beat 5.75, which the loop wraps to 1.75.
    assert_eq!((clips[1].start, clips[1].length, clips[1].offset), (6.75, 5.25, 1.75));
    assert_eq!(clips[1].loop_range, clips[0].loop_range);
    assert_eq!(clips[0].content, clips[1].content, "both parts play the same content");
    let after = render(&session);
    assert_eq!(before.len(), after.len());
    assert!(max_diff(&before, &after, None) < 1e-6, "{}", max_diff(&before, &after, None));

    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(lane_clips(&session, 1).len(), 1, "one undo step");
}

#[test]
fn a_note_across_the_cut_ends_there() {
    let mut session = Session::new(midi_project(vec![note(0.0, 3.0, 60)], vec![clip(1, 0.0, 4.0)]));
    let before = render(&session);
    run(&mut session, "clip.split", json!({ "track": 1, "index": 0, "at": 1.0 })).unwrap();
    let after = render(&session);
    let rms = |out: &[f32], from: usize, to: usize| {
        // Past its end a render is silent.
        let part = out.get(from * 2..(to * 2).min(out.len())).unwrap_or_default();
        (part.iter().map(|s| s * s).sum::<f32>() / ((to - from) * 2) as f32).sqrt()
    };
    // Until the cut the note sounds the same; a release later it is over, and the second part
    // does not start it again.
    assert!(max_diff(&before, &after, Some(BEAT..usize::MAX)) < 1e-6);
    assert!(rms(&before, BEAT * 3 / 2, BEAT * 2) > 0.05);
    assert!(rms(&after, BEAT * 3 / 2, BEAT * 2) < 1e-6);
}

#[test]
fn a_split_audio_clip_plays_as_it_did_apart_from_the_click_guard_at_the_cut() {
    let dir = temp_dir("split-audio");
    let tone = sine(&dir, "tone.wav", 2.0, 441.0);
    let mut session = audio_session(&dir, &[&tone], 1.0);
    let before = render(&session);
    run(&mut session, "clip.split", json!({ "track": 3, "index": 0, "at": 2.5 })).unwrap();
    let clips = lane_clips(&session, 3);
    assert_eq!((clips[1].start, clips[1].offset), (2.5, 1.5));
    let after = render(&session);
    // The engine's 2 ms ramp at both ends of every audio clip, around the cut.
    let cut = BEAT * 5 / 2;
    let guard = cut - 100..cut + 100;
    let diff = max_diff(&before, &after, Some(guard));
    assert!(diff < 1e-6, "{diff}");

    // The file is warped to the project (its length fits whole beats); transposed and not
    // warped, the clip runs faster through its file; the second part
    // carries on from where it was.
    run(&mut session, "edit.undo", json!({})).unwrap();
    let mut transposed = session.project().clone();
    for content in &mut transposed.contents {
        for lane in &mut content.lanes {
            if let LaneData::Audio(region) = &mut lane.data {
                region.warped = false;
                region.transpose = 7.0;
            }
        }
    }
    let audio = session.audio().clone();
    let mut session = Session::new(transposed);
    session.set_audio(audio);
    let before = render(&session);
    run(&mut session, "clip.split", json!({ "track": 3, "index": 0, "at": 2.5 })).unwrap();
    let rate = 2f64.powf(7.0 / 12.0);
    let offset = lane_clips(&session, 3)[1].offset;
    assert!((offset - 1.5 * rate).abs() < 1e-9, "{offset}");
    let after = render(&session);
    assert!(max_diff(&before, &after, Some(cut - 100..cut + 100)) < 1e-5);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_split_keeps_the_fade_in_before_the_cut_and_the_fade_out_after() {
    let mut faded = clip(1, 0.0, 4.0);
    faded.fade_in = 1.0;
    faded.fade_out = 1.5;
    faded.fade_in_curve = FadeCurve { shape: FadeShape::EqualPower, curve: 0.5 };
    faded.fade_out_curve = FadeCurve { shape: FadeShape::Slow, curve: -0.5 };
    let mut session = Session::new(midi_project(vec![], vec![faded]));
    run(&mut session, "clip.split", json!({ "track": 1, "index": 0, "at": 2.0 })).unwrap();
    let clips = lane_clips(&session, 1);
    assert_eq!((clips[0].fade_in, clips[0].fade_out), (1.0, 0.0));
    assert_eq!(clips[0].fade_in_curve.shape, FadeShape::EqualPower);
    assert_eq!((clips[1].fade_in, clips[1].fade_out), (0.0, 1.5));
    assert_eq!(clips[1].fade_out_curve.shape, FadeShape::Slow);

    // A fade-in longer than the first part shrinks to fit it.
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "clip.split", json!({ "track": 1, "index": 0, "at": 0.5 })).unwrap();
    let clips = lane_clips(&session, 1);
    assert_eq!((clips[0].length, clips[0].fade_in), (0.5, 0.5));
}

#[test]
fn malformed_splits_are_refused_and_change_nothing() {
    let mut session = Session::new(midi_project(vec![note(0.0, 1.0, 60)], vec![clip(1, 4.0, 4.0)]));
    let before = session.project().clone();
    for params in [
        json!({ "track": 1, "index": 0 }),
        json!({ "track": 1, "index": 0, "at": "middle" }),
        json!({ "track": 1, "index": 0, "at": 4.0 }),
        json!({ "track": 1, "index": 0, "at": 8.0 }),
        json!({ "track": 1, "index": 0, "at": 2.0 }),
        json!({ "track": 1, "index": 0, "at": 1e300 }),
        json!({ "track": 1, "index": 0, "at": -6.0 }),
        json!({ "track": 1, "index": 1, "at": 6.0 }),
        json!({ "track": 1, "index": -1, "at": 6.0 }),
        json!({ "track": 99, "index": 0, "at": 6.0 }),
        json!({ "track": 1, "index": 0, "at": 6.0, "extra": true }),
    ] {
        assert!(run(&mut session, "clip.split", params.clone()).is_err(), "{params}");
    }
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo());
}

#[test]
fn consolidated_midi_clips_join_their_notes_and_sound_the_same() {
    let notes = vec![note(0.0, 0.5, 60), note(1.0, 0.5, 64), note(3.5, 2.0, 67)];
    let mut looped = clip(1, 8.0, 3.0);
    looped.loop_range = Some(LoopRange { start: 0.0, end: 2.0 });
    let mut session = Session::new(midi_project(notes, vec![clip(1, 0.0, 4.0), looped, clip(1, 16.0, 4.0)]));
    let before = render(&session);
    let out = run(&mut session, "clip.consolidate", json!({ "track": "1 MIDI", "start": 0.0, "end": 12.0 })).unwrap();
    assert_eq!(out["index"], 0);
    assert_eq!(out["file"], Value::Null);
    let clips = lane_clips(&session, 1);
    assert_eq!(clips.len(), 2, "the consolidated clip and the one after the range");
    assert_eq!((clips[0].start, clips[0].length, clips[0].offset, clips[0].loop_range), (0.0, 12.0, 0.0, None));
    let content = session.project().content(clips[0].content).unwrap();
    assert_eq!(content.length, 12.0);
    let LaneData::Notes(joined) = &content.lanes[0].data else { panic!("not notes") };
    let placed: Vec<(f64, f64, u8)> = joined.iter().map(|n| (n.time, n.duration, n.key)).collect();
    // The first clip's last note is cut where the clip ends; the loop plays its two notes,
    // then the first again.
    assert_eq!(
        placed,
        [(0.0, 0.5, 60), (1.0, 0.5, 64), (3.5, 0.5, 67), (8.0, 0.5, 60), (9.0, 0.5, 64), (10.0, 0.5, 60)]
    );
    let after = render(&session);
    assert!(max_diff(&before, &after, None) < 1e-6, "{}", max_diff(&before, &after, None));

    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(lane_clips(&session, 1).len(), 3);
}

#[test]
fn a_range_cuts_the_clips_at_its_edges() {
    let notes = vec![note(0.0, 0.5, 60), note(2.0, 0.5, 64)];
    let mut session = Session::new(midi_project(notes, vec![clip(1, 0.0, 4.0), clip(1, 4.0, 4.0)]));
    run(&mut session, "clip.consolidate", json!({ "track": 1, "start": 1.0, "end": 5.0 })).unwrap();
    let clips = lane_clips(&session, 1);
    let shape: Vec<(f64, f64, f64)> = clips.iter().map(|c| (c.start, c.length, c.offset)).collect();
    assert_eq!(shape, [(0.0, 1.0, 0.0), (1.0, 4.0, 0.0), (5.0, 3.0, 1.0)]);
    let content = session.project().content(clips[1].content).unwrap();
    let LaneData::Notes(joined) = &content.lanes[0].data else { panic!("not notes") };
    let placed: Vec<(f64, u8)> = joined.iter().map(|n| (n.time, n.key)).collect();
    assert_eq!(placed, [(1.0, 64), (3.0, 60)]);
}

#[test]
fn consolidated_audio_is_rendered_to_a_file_next_to_the_project() {
    let dir = temp_dir("consolidate-audio");
    let low = sine(&dir, "low.wav", 1.0, 220.0);
    let high = sine(&dir, "high.wav", 0.5, 660.0);
    let mut session = audio_session(&dir, &[&low], 1.0);
    run(&mut session, "clip.addFiles", json!({ "paths": [high.display().to_string()], "track": 3, "start": 5.0 }))
        .unwrap();
    // A fade on the first clip and a gain on the second are rendered in.
    run(&mut session, "clip.setFades", json!({ "track": 3, "index": 0, "in": 0.5, "out": 0.25 })).unwrap();
    run(&mut session, "clip.setGain", json!({ "track": 3, "index": 1, "db": -6 })).unwrap();
    let before = render(&session);
    let out = run(&mut session, "clip.consolidate", json!({ "track": 3, "start": 0.0, "end": 8.0 })).unwrap();
    let file = PathBuf::from(out["file"].as_str().unwrap());
    assert_eq!(file.parent(), Some(dir.join("Consolidated").as_path()));
    assert_eq!(file.file_name().and_then(|n| n.to_str()), Some("3 Audio bar 1.wav"));
    let decoded = suhina_media::decode_file(&file).unwrap();
    assert_eq!((decoded.sample_rate, decoded.channels, decoded.frames()), (SR, 2, 8 * BEAT));

    let clips = lane_clips(&session, 3);
    assert_eq!(clips.len(), 1);
    assert_eq!((clips[0].start, clips[0].length, clips[0].fade_in), (0.0, 8.0, 0.0));
    let content = session.project().content(clips[0].content).unwrap();
    let LaneData::Audio(region) = &content.lanes[0].data else { panic!("not audio") };
    assert!(!region.warped);
    assert_eq!(region.gain, 1.0);
    assert!(session.audio().files.contains_key(&region.file), "the rendered audio joins the pool");
    let after = render(&session);
    // The range starts and ends in silence, so even the click guard of the new clip is not heard.
    assert!(max_diff(&before, &after, None) < 1e-5, "{}", max_diff(&before, &after, None));

    // A second consolidation of the same place does not overwrite the first file.
    run(&mut session, "edit.undo", json!({})).unwrap();
    let again = run(&mut session, "clip.consolidate", json!({ "track": 3, "start": 0.0, "end": 8.0 })).unwrap();
    assert!(again["file"].as_str().unwrap().ends_with("3 Audio bar 1 2.wav"));
    assert!(file.is_file(), "undo leaves the file on disk");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn audio_is_not_consolidated_before_the_project_is_saved() {
    let dir = temp_dir("unsaved");
    let tone = sine(&dir, "tone.wav", 1.0, 441.0);
    let mut session = Session::new(blank_project());
    run(&mut session, "clip.addFiles", json!({ "paths": [tone.display().to_string()], "track": 3, "start": 0.0 }))
        .unwrap();
    let before = session.project().clone();
    let error = run(&mut session, "clip.consolidate", json!({ "track": 3, "start": 0.0, "end": 2.0 })).unwrap_err();
    assert!(error.contains("save the project first"), "{error}");
    assert_eq!(session.project(), &before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn malformed_consolidations_are_refused_and_change_nothing() {
    let mut session = Session::new(midi_project(vec![note(0.0, 1.0, 60)], vec![clip(1, 4.0, 4.0)]));
    let before = session.project().clone();
    for params in [
        json!({ "track": 1, "start": 0.0 }),
        json!({ "track": 1, "start": 4.0, "end": 4.0 }),
        json!({ "track": 1, "start": 6.0, "end": 5.0 }),
        json!({ "track": 1, "start": -1.0, "end": 5.0 }),
        json!({ "track": 1, "start": 0.0, "end": 1e12 }),
        json!({ "track": 1, "start": "0", "end": 5.0 }),
        json!({ "track": 1, "start": 9.0, "end": 12.0 }),
        json!({ "track": 2, "start": 0.0, "end": 12.0 }),
        json!({ "track": 99, "start": 0.0, "end": 12.0 }),
    ] {
        assert!(run(&mut session, "clip.consolidate", params.clone()).is_err(), "{params}");
    }
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo());
}

#[test]
fn a_shared_pattern_is_not_consolidated() {
    let mut p = midi_project(vec![note(0.0, 1.0, 60)], vec![clip(1, 0.0, 4.0)]);
    p.contents[0].lanes[0].target = Some(TrackId(2));
    let mut session = Session::new(p);
    let error = run(&mut session, "clip.consolidate", json!({ "track": 1, "start": 0.0, "end": 4.0 })).unwrap_err();
    assert!(error.contains("shared pattern"), "{error}");
}

#[test]
fn stacked_midi_clips_consolidate_into_one_that_plays_them_all() {
    let notes = vec![note(0.0, 0.5, 60), note(2.0, 0.5, 64)];
    // Two clips over each other (stacked), the second from beat 1.
    let mut session = Session::new(midi_project(notes, vec![clip(1, 0.0, 4.0), clip(1, 1.0, 4.0)]));
    run(&mut session, "clip.consolidate", json!({ "track": 1, "start": 0.0, "end": 5.0 })).unwrap();
    let clips = lane_clips(&session, 1);
    assert_eq!(clips.len(), 1, "{clips:?}");
    let content = session.project().content(clips[0].content).unwrap();
    let LaneData::Notes(joined) = &content.lanes[0].data else { panic!("not notes") };
    let mut placed: Vec<(f64, u8)> = joined.iter().map(|n| (n.time, n.key)).collect();
    placed.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(placed, [(0.0, 60), (1.0, 60), (2.0, 64), (3.0, 64)]);
}

/// One row of a stack consolidated (`clips`): only its clips are joined and cut away, the clip in
/// the other row stays as it was.
#[test]
fn one_row_of_a_stack_consolidates_alone() {
    let notes = vec![note(0.0, 0.5, 60), note(2.0, 0.5, 64)];
    let mut session = Session::new(midi_project(notes, vec![clip(1, 0.0, 4.0), clip(1, 1.0, 4.0)]));
    let before = lane_clips(&session, 1);
    let (first, second) = (before[0].clone(), before[1].clone());
    run(&mut session, "clip.consolidate", json!({ "track": 1, "start": 0.0, "end": 4.0, "clips": [first.id.0] }))
        .unwrap();
    let clips = lane_clips(&session, 1);
    assert_eq!(clips.len(), 2, "the joined clip and the other row's: {clips:?}");
    assert!(clips.contains(&second), "the other row's clip is untouched");
    assert!(!clips.iter().any(|c| c.id == first.id), "the joined clip took the first one's place");
    // A malformed list is refused.
    assert!(
        run(&mut session, "clip.consolidate", json!({ "track": 1, "start": 0.0, "end": 4.0, "clips": "x" })).is_err()
    );
}

// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Comping from take lanes (#989): parts named and coloured after their takes, swapping a part's
//! take, moving the cut between parts, recolouring a take, and splits reaching the takes.
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use serde_json::json;
use suhina_model::*;
use suhina_session::Session;
use suhina_test_support::ProjectBuilder;

use crate::support::run;

/// An audio track "Vox" with three takes of eight beats, passes of one 24-beat file.
fn session() -> Session {
    let mut project =
        ProjectBuilder::new().audio_track(1, "Vox").audio(1, "Recording", 24.0, AudioRegion::new("take.wav")).build();
    let pass = |n: f64| Clip { offset: n * 8.0, ..Clip::new(ContentId(1), 0.0, 8.0) };
    project.arrangement.takes = (0..3)
        .map(|n| TakeLane {
            track: TrackId(1),
            name: format!("Take {}", n + 1),
            clips: vec![pass(f64::from(n))],
            color: None,
        })
        .collect();
    Session::new(project)
}

fn lane(session: &Session) -> Vec<Clip> {
    session.project().arrangement.lanes.iter().find(|l| l.track == Some(TrackId(1))).unwrap().clips.clone()
}

fn source(session: &Session, clip: &Clip) -> Option<usize> {
    session.project().comp_source(TrackId(1), clip)
}

#[test]
fn a_parts_take_is_swapped_in_turn_and_the_cut_between_parts_moves() {
    let mut s = session();
    run(&mut s, "take.comp", json!({ "track": 1, "take": 1, "start": 0.0, "end": 4.0 })).unwrap();
    run(&mut s, "take.comp", json!({ "track": 1, "take": 2, "start": 4.0, "end": 8.0 })).unwrap();
    let parts = lane(&s);
    assert_eq!(parts.iter().map(|c| source(&s, c)).collect::<Vec<_>>(), [Some(0), Some(1)]);
    assert_eq!(s.project().comp_used(1), [(4.0, 8.0)]);
    // Down from Take 1 is Take 2, then Take 3, then round to Take 1.
    let first = parts[0].id.0;
    run(&mut s, "take.swap", json!({ "clip": first, "direction": "down" })).unwrap();
    let swapped = lane(&s).into_iter().find(|c| c.start == 0.0).unwrap();
    assert_eq!(source(&s, &swapped), Some(1));
    assert_eq!((swapped.length, swapped.offset), (4.0, 8.0), "the same span of Take 2");
    run(&mut s, "take.swap", json!({ "clip": swapped.id.0, "direction": "up" })).unwrap();
    let back = lane(&s).into_iter().find(|c| c.start == 0.0).unwrap();
    assert_eq!(source(&s, &back), Some(0));
    // The cut at beat 4 moves to beat 6: Take 1 plays to 6, Take 2 from 6.
    run(&mut s, "take.moveCut", json!({ "clip": back.id.0, "to": 6.0 })).unwrap();
    let parts = lane(&s);
    assert_eq!(parts.len(), 2);
    assert_eq!((parts[0].start, parts[0].length, source(&s, &parts[0])), (0.0, 6.0, Some(0)));
    assert_eq!((parts[1].start, parts[1].length, parts[1].offset, source(&s, &parts[1])), (6.0, 2.0, 14.0, Some(1)));
    // And back to 3.
    run(&mut s, "take.moveCut", json!({ "clip": parts[0].id.0, "to": 3.0 })).unwrap();
    let parts = lane(&s);
    assert_eq!((parts[0].length, parts[1].start, parts[1].offset), (3.0, 3.0, 11.0));
    // Each step is one undo step.
    run(&mut s, "edit.undo", json!({})).unwrap();
    assert_eq!(lane(&s)[0].length, 6.0);
}

#[test]
fn recolouring_a_take_colours_its_parts_and_a_split_cuts_the_takes() {
    let mut s = session();
    run(&mut s, "take.comp", json!({ "track": 1, "take": 2, "start": 0.0, "end": 8.0 })).unwrap();
    run(&mut s, "take.setColor", json!({ "track": 1, "take": 2, "color": "#ff00aa" })).unwrap();
    assert_eq!(s.project().arrangement.takes[1].color, Some(Rgb(0xff, 0x00, 0xaa)));
    assert_eq!(lane(&s)[0].color, Some(Rgb(0xff, 0x00, 0xaa)));
    assert_eq!(s.project().arrangement.takes[0].color, None, "other takes keep theirs");
    // One piece of a split take, and its part on the track, alone (Recolor).
    run(&mut s, "clip.split", json!({ "track": 1, "index": 0, "at": 4.0 })).unwrap();
    run(&mut s, "take.setColor", json!({ "track": 1, "take": 2, "color": "#00ff00", "piece": 2 })).unwrap();
    let take = &s.project().arrangement.takes[1];
    assert_eq!((take.clips[0].color, take.clips[1].color), (None, Some(Rgb(0, 0xff, 0))));
    let colours: Vec<Option<Rgb>> = lane(&s).iter().map(|c| c.color).collect();
    assert_eq!(colours, [Some(Rgb(0xff, 0x00, 0xaa)), Some(Rgb(0, 0xff, 0))]);
    // Recolor lane gives every piece the lane's colour again.
    run(&mut s, "take.setColor", json!({ "track": 1, "take": 2, "color": "#123456" })).unwrap();
    let take = &s.project().arrangement.takes[1];
    assert!(take.clips.iter().all(|c| c.color.is_none()));
    assert!(lane(&s).iter().all(|c| c.color == Some(Rgb(0x12, 0x34, 0x56))));
    run(&mut s, "edit.undo", json!({})).unwrap();
    run(&mut s, "edit.undo", json!({})).unwrap();
    run(&mut s, "edit.undo", json!({})).unwrap();
    // A split of the track's clip at beat 2 cuts every take there too.
    run(&mut s, "clip.split", json!({ "track": 1, "index": 0, "at": 2.0 })).unwrap();
    for take in &s.project().arrangement.takes {
        let cuts: Vec<(f64, f64)> = take.clips.iter().map(|c| (c.start, c.length)).collect();
        assert_eq!(cuts, [(0.0, 2.0), (2.0, 6.0)], "{}", take.name);
    }
    // The parts still know their take.
    assert!(lane(&s).iter().all(|c| source(&s, c) == Some(1)));
}

/// The up arrow puts a whole take on the track; deleting the unused takes keeps those the comp
/// uses; a soloed take is heard in place of the track (#989).
#[test]
fn a_take_is_promoted_soloed_and_the_unused_ones_deleted() {
    let mut s = session();
    run(&mut s, "take.promote", json!({ "track": 1, "take": 3 })).unwrap();
    let parts = lane(&s);
    assert_eq!(parts.len(), 1);
    assert_eq!((parts[0].start, parts[0].length, source(&s, &parts[0])), (0.0, 8.0, Some(2)));
    run(&mut s, "take.solo", json!({ "track": 1, "take": 2 })).unwrap();
    assert_eq!(s.take_solo(), Some((TrackId(1), 2)));
    run(&mut s, "take.solo", json!({ "track": 1, "take": 0 })).unwrap();
    assert_eq!(s.take_solo(), None);
    let done = run(&mut s, "take.deleteUnused", json!({ "track": 1 })).unwrap();
    assert_eq!(done["deleted"], json!(2));
    let names: Vec<&str> = s.project().arrangement.takes.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Take 3"]);
    assert_eq!(source(&s, &lane(&s)[0]), Some(0), "the part keeps its take");
}

/// Pieces picked with Ctrl and deleted go from their take, and a take with none left goes too
/// (#989).
#[test]
fn deleting_pieces_of_a_take_removes_them_and_an_emptied_take() {
    let mut s = session();
    run(&mut s, "take.comp", json!({ "track": 1, "take": 1, "start": 0.0, "end": 8.0 })).unwrap();
    run(&mut s, "clip.split", json!({ "track": 1, "index": 0, "at": 4.0 })).unwrap();
    let done = run(&mut s, "take.deletePieces", json!({ "track": 1, "take": 2, "pieces": [2] })).unwrap();
    assert_eq!(done["take_deleted"], json!(false));
    let take = &s.project().arrangement.takes[1];
    assert_eq!(take.clips.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>(), [(0.0, 4.0)]);
    let done = run(&mut s, "take.deletePieces", json!({ "track": 1, "take": 2, "pieces": [1] })).unwrap();
    assert_eq!(done["take_deleted"], json!(true));
    let names: Vec<&str> = s.project().arrangement.takes.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Take 1", "Take 3"]);
    assert!(run(&mut s, "take.deletePieces", json!({ "track": 1, "take": 1, "pieces": [3] })).is_err());
    run(&mut s, "edit.undo", json!({})).unwrap();
    assert_eq!(s.project().arrangement.takes.len(), 3);
}

/// Where the track has takes, a clip moved with stacking on still replaces what it covers, so
/// the comp stays one line of clips; outside the takes it stacks (#989).
#[test]
fn clips_do_not_stack_where_the_track_has_takes() {
    let mut s = session();
    run(&mut s, "take.comp", json!({ "track": 1, "take": 1, "start": 0.0, "end": 4.0 })).unwrap();
    run(&mut s, "take.comp", json!({ "track": 1, "take": 2, "start": 4.0, "end": 8.0 })).unwrap();
    let second = lane(&s)[1].id.0;
    run(&mut s, "clip.move", json!({ "track": 1, "index": 1, "start": 2.0, "stack": true })).unwrap();
    let spans: Vec<(f64, f64)> = lane(&s).iter().map(|c| (c.start, c.length)).collect();
    assert_eq!(spans, [(0.0, 2.0), (2.0, 4.0)], "the moved part cut the one under it");
    let moved = lane(&s).into_iter().position(|c| c.id.0 == second).unwrap();
    run(&mut s, "clip.move", json!({ "track": 1, "index": moved, "start": 20.0, "stack": true })).unwrap();
    // Past the takes (they end at beat 8) two clips may share time.
    let index = lane(&s).iter().position(|c| c.start == 0.0).unwrap();
    run(&mut s, "clip.move", json!({ "track": 1, "index": index, "start": 21.0, "stack": true })).unwrap();
    let after: Vec<(f64, f64)> = lane(&s).iter().filter(|c| c.start >= 20.0).map(|c| (c.start, c.length)).collect();
    assert!(after.contains(&(20.0, 4.0)) && after.contains(&(21.0, 2.0)), "{after:?}");
}

/// A take keeps the number its lane shows after another is deleted: with Take 1 gone, `take: 3`
/// is still the lane named "Take 3" (#916).
#[test]
fn a_take_is_named_by_the_number_its_lane_shows() {
    let mut s = session();
    run(&mut s, "take.delete", json!({ "track": 1, "take": 1 })).unwrap();
    let done = run(&mut s, "take.delete", json!({ "track": 1, "take": 3 })).unwrap();
    assert_eq!(done["deleted"], json!("Take 3"));
    let names: Vec<&str> = s.project().arrangement.takes.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Take 2"]);
}

fn take_clips(session: &Session, take: usize) -> Vec<(f64, f64, f64)> {
    session.project().arrangement.takes[take].clips.iter().map(|c| (c.start, c.length, c.offset)).collect()
}

/// Each pass is a window on one recording: pulled out to the right, every take goes on up to the
/// end of the file, so the earliest is the longest, a staircase (#1078).
#[test]
fn a_takes_edge_brings_back_the_recording_up_to_its_end() {
    let mut s = session();
    for take in 1..=3 {
        run(&mut s, "take.trimPiece", json!({ "track": 1, "take": take, "piece": 1, "end": 1000.0 })).unwrap();
    }
    assert_eq!(take_clips(&s, 0), [(0.0, 24.0, 0.0)]);
    assert_eq!(take_clips(&s, 1), [(0.0, 16.0, 8.0)]);
    assert_eq!(take_clips(&s, 2), [(0.0, 8.0, 16.0)]);
    // The left edge moves the audio with it, and stops at the start of the recording.
    run(&mut s, "take.trimPiece", json!({ "track": 1, "take": 2, "piece": 1, "start": 2.0 })).unwrap();
    assert_eq!(take_clips(&s, 1), [(2.0, 14.0, 10.0)]);
    run(&mut s, "take.trimPiece", json!({ "track": 1, "take": 2, "piece": 1, "start": 0.0 })).unwrap();
    assert_eq!(take_clips(&s, 1), [(0.0, 16.0, 8.0)]);
}

/// What a recording's take trimmed by an area comes back by its edge, up to the take's next piece
/// when there is one; a part on the track stays that take's part (#1078).
#[test]
fn an_area_deletes_across_takes_and_an_edge_brings_it_back() {
    let mut s = session();
    run(&mut s, "take.comp", json!({ "track": 1, "take": 1, "start": 0.0, "end": 2.0 })).unwrap();
    let area = json!({ "track": 1, "takes": [1, 2], "start": 2.0, "end": 4.0 });
    run(&mut s, "take.deleteRange", area).unwrap();
    assert_eq!(take_clips(&s, 0), [(0.0, 2.0, 0.0), (4.0, 4.0, 4.0)]);
    assert_eq!(take_clips(&s, 1), [(0.0, 2.0, 8.0), (4.0, 4.0, 12.0)]);
    assert_eq!(take_clips(&s, 2), [(0.0, 8.0, 16.0)], "a take not listed stays");
    // The first piece stops at the second.
    run(&mut s, "take.trimPiece", json!({ "track": 1, "take": 1, "piece": 1, "end": 7.0 })).unwrap();
    assert_eq!(take_clips(&s, 0), [(0.0, 4.0, 0.0), (4.0, 4.0, 4.0)]);
    assert_eq!(source(&s, &lane(&s)[0]), Some(0), "the part keeps its take");
}

#[test]
fn an_area_that_empties_a_take_deletes_it_and_the_numbers_of_the_others_hold() {
    let mut s = session();
    run(&mut s, "take.deleteRange", json!({ "track": 1, "takes": [1], "start": 0.0, "end": 8.0 })).unwrap();
    let names: Vec<&str> = s.project().arrangement.takes.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Take 2", "Take 3"]);
    assert_eq!(s.project().take_number(0), Some(2));
    // Take 2 is still take 2 to the commands, not the first one left.
    run(&mut s, "take.deleteRange", json!({ "track": 1, "takes": [2], "start": 0.0, "end": 4.0 })).unwrap();
    assert_eq!(take_clips(&s, 0), [(4.0, 4.0, 12.0)]);
    assert_eq!(take_clips(&s, 1), [(0.0, 8.0, 16.0)]);
}

#[test]
fn a_cut_area_pastes_back_on_its_track() {
    let mut s = session();
    let area = json!({ "track": 1, "takes": [1], "start": 2.0, "end": 4.0 });
    run(&mut s, "take.cutRange", area).unwrap();
    assert_eq!(take_clips(&s, 0), [(0.0, 2.0, 0.0), (4.0, 4.0, 4.0)]);
    run(&mut s, "clip.paste", json!({ "track": 1, "start": 16.0 })).unwrap();
    let pasted = lane(&s).into_iter().find(|c| c.start == 16.0).unwrap();
    assert_eq!((pasted.length, pasted.offset), (2.0, 2.0));
}

/// A moved piece keeps its audio, covers what is under it on its own lane and stops at the start
/// of the timeline; a part comped from it after the move is its part (#1078).
#[test]
fn pieces_of_takes_move_along_the_timeline() {
    let mut s = session();
    run(&mut s, "take.movePieces", json!({ "track": 1, "takes": [2], "pieces": [1], "by": 16.0 })).unwrap();
    assert_eq!(take_clips(&s, 1), [(16.0, 8.0, 8.0)]);
    assert_eq!(take_clips(&s, 0), [(0.0, 8.0, 0.0)], "other lanes stay");
    run(&mut s, "take.comp", json!({ "track": 1, "take": 2, "start": 16.0, "end": 24.0 })).unwrap();
    let part = lane(&s).into_iter().find(|c| c.start == 16.0).unwrap();
    assert_eq!(part.offset, 8.0, "the part plays what the piece plays");
    assert_eq!(source(&s, &part), Some(1));
    // Back past the start: it stops there.
    run(&mut s, "take.movePieces", json!({ "track": 1, "takes": [2], "pieces": [1], "by": -100.0 })).unwrap();
    assert_eq!(take_clips(&s, 1), [(0.0, 8.0, 8.0)]);
    // Onto a neighbour on its own lane: it replaces what it covers.
    run(&mut s, "take.deleteRange", json!({ "track": 1, "takes": [3], "start": 2.0, "end": 4.0 })).unwrap();
    run(&mut s, "take.movePieces", json!({ "track": 1, "takes": [3], "pieces": [1], "by": 3.0 })).unwrap();
    assert_eq!(take_clips(&s, 2), [(3.0, 2.0, 16.0), (5.0, 3.0, 21.0)]);
}

/// Several pieces move together and keep their places to each other at the timeline's start.
#[test]
fn pieces_moved_together_keep_their_distance() {
    let mut s = session();
    run(&mut s, "take.movePieces", json!({ "track": 1, "takes": [1], "pieces": [1], "by": 4.0 })).unwrap();
    run(&mut s, "take.movePieces", json!({ "track": 1, "takes": [1, 2], "pieces": [1, 1], "by": -100.0 })).unwrap();
    assert_eq!(take_clips(&s, 0)[0].0, 4.0);
    assert_eq!(take_clips(&s, 1)[0].0, 0.0);
}

#[test]
fn take_area_and_piece_commands_refuse_what_is_not_there() {
    let mut s = session();
    let before = s.project().arrangement.takes.clone();
    for (id, params) in [
        ("take.deleteRange", json!({ "track": 1, "takes": [9], "start": 0.0, "end": 1.0 })),
        ("take.deleteRange", json!({ "track": 1, "takes": [1], "start": 4.0, "end": 2.0 })),
        ("take.deleteRange", json!({ "track": 1, "takes": [1], "start": 100.0, "end": 200.0 })),
        ("take.cutRange", json!({ "track": 1, "takes": [1], "start": 100.0, "end": 200.0 })),
        ("take.movePieces", json!({ "track": 1, "takes": [1], "pieces": [2], "by": 1.0 })),
        ("take.movePieces", json!({ "track": 1, "takes": [1], "pieces": [0], "by": 1.0 })),
        ("take.movePieces", json!({ "track": 1, "takes": [1, 2], "pieces": [1], "by": 1.0 })),
        ("take.trimPiece", json!({ "track": 1, "take": 1, "piece": 2, "end": 1.0 })),
        ("take.trimPiece", json!({ "track": 1, "take": 1, "piece": 1 })),
        ("take.trimPiece", json!({ "track": 1, "take": 9, "piece": 1, "end": 1.0 })),
    ] {
        assert!(run(&mut s, id, params.clone()).is_err(), "{id} {params}");
    }
    assert_eq!(s.project().arrangement.takes, before);
}

/// A MIDI pass has a content of its own: its edge stops where the pass does.
#[test]
fn a_midi_takes_edge_stops_at_the_end_of_its_pass() {
    let mut project = ProjectBuilder::new().midi_track(1, "Keys").notes(1, "Pass", 8.0, Vec::new()).build();
    project.arrangement.takes = vec![TakeLane {
        track: TrackId(1),
        name: "Take 1".into(),
        clips: vec![Clip::new(ContentId(1), 0.0, 4.0)],
        color: None,
    }];
    let mut s = Session::new(project);
    run(&mut s, "take.trimPiece", json!({ "track": 1, "take": 1, "piece": 1, "end": 100.0 })).unwrap();
    assert_eq!(take_clips(&s, 0), [(0.0, 8.0, 0.0)]);
}

/// A part moved takes what the takes hold under it along, each on its own lane (#1078).
#[test]
fn a_moved_part_takes_its_takes_along() {
    let mut s = session();
    run(&mut s, "take.comp", json!({ "track": 1, "take": 3 })).unwrap();
    run(&mut s, "clip.move", json!({ "track": 1, "index": 0, "start": 16.0 })).unwrap();
    assert_eq!(take_clips(&s, 0), [(16.0, 8.0, 0.0)]);
    assert_eq!(take_clips(&s, 1), [(16.0, 8.0, 8.0)]);
    assert_eq!(take_clips(&s, 2), [(16.0, 8.0, 16.0)]);
    assert_eq!(source(&s, &lane(&s)[0]), Some(2), "it is still that take's part");
}

/// Where the takes already hold something, what is carried replaces nothing: it gets as many new
/// lanes as the part has takes.
#[test]
fn a_moved_part_over_other_takes_gets_new_lanes() {
    let mut s = session();
    run(&mut s, "take.comp", json!({ "track": 1, "take": 1, "start": 0.0, "end": 4.0 })).unwrap();
    run(&mut s, "take.comp", json!({ "track": 1, "take": 2, "start": 4.0, "end": 8.0 })).unwrap();
    run(&mut s, "clip.move", json!({ "track": 1, "index": 0, "start": 2.0 })).unwrap();
    let names: Vec<&str> = s.project().arrangement.takes.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Take 1", "Take 2", "Take 3", "Take 4", "Take 5", "Take 6"]);
    assert_eq!(take_clips(&s, 0), [(4.0, 4.0, 4.0)]);
    assert_eq!(take_clips(&s, 3), [(2.0, 4.0, 0.0)]);
    assert_eq!(take_clips(&s, 4), [(2.0, 4.0, 8.0)]);
    assert_eq!(take_clips(&s, 5), [(2.0, 4.0, 16.0)]);
}

/// Moved to another track, the takes go too; lanes left with nothing go.
#[test]
fn a_part_moved_to_another_track_takes_its_takes_there() {
    let mut project = ProjectBuilder::new()
        .audio_track(1, "Vox")
        .audio_track(2, "Other")
        .audio(1, "Recording", 24.0, AudioRegion::new("take.wav"))
        .build();
    let pass = |n: f64| Clip { offset: n * 8.0, ..Clip::new(ContentId(1), 0.0, 8.0) };
    project.arrangement.takes = (0..2)
        .map(|n| TakeLane {
            track: TrackId(1),
            name: format!("Take {}", n + 1),
            clips: vec![pass(f64::from(n))],
            color: None,
        })
        .collect();
    let mut s = Session::new(project);
    run(&mut s, "take.comp", json!({ "track": 1, "take": 1 })).unwrap();
    run(&mut s, "clip.move", json!({ "track": 1, "index": 0, "start": 8.0, "toTrack": 2 })).unwrap();
    let takes = &s.project().arrangement.takes;
    assert_eq!(takes.len(), 2);
    assert!(takes.iter().all(|t| t.track == TrackId(2)));
    assert_eq!(take_clips(&s, 0), [(8.0, 8.0, 0.0)]);
    assert_eq!(take_clips(&s, 1), [(8.0, 8.0, 8.0)]);
}

/// A clip that is no part of a take moves alone.
#[test]
fn a_clip_of_no_take_leaves_the_takes_alone() {
    let mut project =
        ProjectBuilder::new().audio_track(1, "Vox").audio(1, "Recording", 24.0, AudioRegion::new("take.wav")).build();
    project.arrangement.takes = vec![TakeLane {
        track: TrackId(1),
        name: "Take 1".into(),
        clips: vec![Clip::new(ContentId(1), 0.0, 8.0)],
        color: None,
    }];
    project.arrangement.lanes = vec![ArrangementLane {
        id: LaneId(1),
        name: String::new(),
        track: Some(TrackId(1)),
        // Three beats off the take: not a part of it.
        clips: vec![Clip { offset: 3.0, ..Clip::new(ContentId(1), 0.0, 4.0) }],
    }];
    project.assign_ids().unwrap();
    let before = project.arrangement.takes.clone();
    let mut s = Session::new(project);
    run(&mut s, "clip.move", json!({ "track": 1, "index": 0, "start": 16.0 })).unwrap();
    assert_eq!(s.project().arrangement.takes, before);
}

/// A copied part leaves its takes where they are and takes them to the copy as well; copied to the
/// very same spot again, nothing is added (#1078).
#[test]
fn a_copied_part_copies_its_takes_once_for_a_spot() {
    let mut s = session();
    run(&mut s, "take.comp", json!({ "track": 1, "take": 3 })).unwrap();
    let id = lane(&s)[0].id.0;
    for _ in 0..2 {
        run(&mut s, "clip.moveMany", json!({ "clips": [{ "clip": id }], "beats": 16, "copy": true })).unwrap();
        assert_eq!(take_clips(&s, 0), [(0.0, 8.0, 0.0), (16.0, 8.0, 0.0)]);
        assert_eq!(take_clips(&s, 1), [(0.0, 8.0, 8.0), (16.0, 8.0, 8.0)]);
        assert_eq!(take_clips(&s, 2), [(0.0, 8.0, 16.0), (16.0, 8.0, 16.0)]);
    }
    assert_eq!(s.project().arrangement.takes.len(), 3, "no new lanes for what is there already");
    let copy = lane(&s).into_iter().find(|c| c.start == 16.0).unwrap();
    assert_eq!(source(&s, &copy), Some(2), "the copy is a part of that take");
}

/// A copy over something else on the lanes replaces nothing: it gets new lanes.
#[test]
fn a_part_copied_over_its_own_takes_gets_new_lanes() {
    let mut s = session();
    run(&mut s, "take.comp", json!({ "track": 1, "take": 1 })).unwrap();
    let id = lane(&s)[0].id.0;
    run(&mut s, "clip.moveMany", json!({ "clips": [{ "clip": id }], "beats": 2, "copy": true })).unwrap();
    assert_eq!(s.project().arrangement.takes.len(), 6);
    assert_eq!(take_clips(&s, 0), [(0.0, 8.0, 0.0)]);
    assert_eq!(take_clips(&s, 3), [(2.0, 8.0, 0.0)]);
}

/// Copy and paste, and duplicate, take the takes along too (#1078).
#[test]
fn pasting_and_duplicating_a_part_take_its_takes_along() {
    let mut s = session();
    run(&mut s, "take.comp", json!({ "track": 1, "take": 2 })).unwrap();
    let id = lane(&s)[0].id.0;
    run(&mut s, "clip.copy", json!({ "clips": [{ "clip": id }] })).unwrap();
    for _ in 0..2 {
        run(&mut s, "clip.paste", json!({ "track": 1, "start": 16.0 })).unwrap();
        assert_eq!(take_clips(&s, 1), [(0.0, 8.0, 8.0), (16.0, 8.0, 8.0)]);
    }
    assert_eq!(s.project().arrangement.takes.len(), 3);
    let pasted = lane(&s).into_iter().find(|c| c.start == 16.0).unwrap();
    assert_eq!(source(&s, &pasted), Some(1));
    run(&mut s, "clip.duplicate", json!({ "clips": [{ "clip": id }] })).unwrap();
    assert_eq!(take_clips(&s, 0), [(0.0, 8.0, 0.0), (8.0, 8.0, 0.0), (16.0, 8.0, 0.0)]);
    assert_eq!(s.project().arrangement.takes.len(), 3);
}

/// A comped audio part fades as long as Settings says, and the handle's command sets the length
/// at a cut and puts it back (#1078).
#[test]
fn comped_parts_fade_and_their_crossfade_is_set() {
    let mut s = session();
    s.set_take_crossfade_ms(40.0);
    run(&mut s, "take.comp", json!({ "track": 1, "take": 1, "start": 0.0, "end": 4.0 })).unwrap();
    run(&mut s, "take.comp", json!({ "track": 1, "take": 2, "start": 4.0, "end": 8.0 })).unwrap();
    let parts = lane(&s);
    let beats = 40.0 * s.project().transport.tempo_bpm / 60_000.0;
    assert!((parts[0].fade_out - beats).abs() < 1e-9, "{}", parts[0].fade_out);
    assert!((parts[1].fade_in - beats).abs() < 1e-9);
    let left = parts[0].id.0;
    run(&mut s, "take.setCrossfade", json!({ "clip": left, "length": 1.0 })).unwrap();
    let parts = lane(&s);
    assert_eq!((parts[0].fade_out, parts[1].fade_in), (1.0, 1.0));
    run(&mut s, "take.setCrossfade", json!({ "clip": left, "length": 100.0 })).unwrap();
    assert_eq!(lane(&s)[0].fade_out, 4.0, "no longer than the shorter part");
    run(&mut s, "take.setCrossfade", json!({ "clip": left })).unwrap();
    assert!((lane(&s)[0].fade_out - beats).abs() < 1e-9);
    assert!(run(&mut s, "take.setCrossfade", json!({ "clip": lane(&s)[1].id.0 })).is_err(), "no part after it");
    assert!(run(&mut s, "take.setCrossfade", json!({ "clip": left, "length": -1.0 })).is_err());
}

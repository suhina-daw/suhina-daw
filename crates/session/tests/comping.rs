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

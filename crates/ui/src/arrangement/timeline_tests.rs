// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::demo::demo_project;

#[test]
fn the_ruler_marks_tempo_and_time_signature_changes() {
    use suhina_engine::model::{TempoPoint, TimeSignature, TimeSignatureChange};
    let mut project = demo_project();
    project.transport.tempo_changes =
        vec![TempoPoint { beat: 16.0, bpm: 140.0, ramp: false }, TempoPoint { beat: 32.0, bpm: 97.5, ramp: true }];
    project.transport.time_signature_changes =
        vec![TimeSignatureChange { bar: 4, time_signature: TimeSignature { numerator: 7, denominator: 8 } }];
    let timeline = Timeline::build(&project, None);
    let marks: Vec<(f64, &str)> = timeline.marks.iter().map(|m| (m.beat, m.text.as_str())).collect();
    // Bar 5 (counted from 1) starts on beat 16, where the tempo changes too.
    assert_eq!(marks, [(16.0, "140 bpm  7/8"), (32.0, "→ 97.5 bpm")]);
    assert_eq!(timeline.meter.bar_start(5), 19.5);
    assert!(Timeline::build(&demo_project(), None).marks.is_empty());
}

#[test]
fn lanes_follow_the_tracks_with_returns_last() {
    let timeline = Timeline::build(&demo_project(), None);
    let names: Vec<&str> = timeline.lanes.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Drums", "Bass", "Keys", "Vox", "A Reverb", "B Delay"]);
    assert_eq!(timeline.end, 192.0);
    assert_eq!(timeline.main_volume_db, Some(0.0));
    assert_eq!(timeline.lanes[0].clips[1].index, 1);
    assert_eq!(timeline.lanes[0].clips[1].name, "Beat B");
}

#[test]
fn audio_clips_map_beats_to_file_seconds_like_the_engine() {
    let linear = AudioView {
        file: "a.wav".into(),
        offset: 1.0,
        loop_range: Some((0.0, 4.0)),
        markers: Vec::new(),
        seconds_per_beat: 0.5,
        rate: 1.0,
        gain_db: 0.0,
    };
    assert_eq!(linear.seconds(0.0), 0.5);
    assert_eq!(linear.seconds(2.0), 1.5);
    // Past the loop end it starts the loop again.
    assert_eq!(linear.seconds(3.5), 0.25);
    let warped = AudioView { markers: vec![(0.0, 0.0), (4.0, 1.0)], loop_range: None, offset: 0.0, ..linear };
    assert_eq!(warped.seconds(2.0), 0.5);
    // Beyond the last marker the last segment carries on.
    assert_eq!(warped.seconds(8.0), 2.0);
}

#[test]
fn overlapping_clips_get_sub_rows() {
    let mut project = demo_project();
    let lane = &mut project.arrangement.lanes[0];
    lane.clips[1].start = 32.0; // over the first clip (0..64)
    let timeline = Timeline::build(&project, None);
    let rows: Vec<(usize, usize)> = timeline.lanes[0].clips.iter().map(|c| (c.row, c.rows)).collect();
    assert_eq!(rows[0], (0, 2));
    assert_eq!(rows[1], (1, 2));
    assert!(Timeline::build(&demo_project(), None).lanes.iter().flat_map(|l| &l.clips).all(|c| c.rows == 1));
}

#[test]
fn a_split_clip_in_a_stack_keeps_both_parts_on_its_row() {
    // Three clips stacked: the first from 0, the third from a quarter of its length, the
    // middle one from half of it, in row 2. Its end part, split off, still overlaps the third
    // one but lies where row 0 is free again: it stays in row 2 all the same (#703).
    let mut project = demo_project();
    let lane = &mut project.arrangement.lanes[0];
    let first = lane.clips[0].clone();
    let length = first.length;
    lane.clips = vec![first.clone(), first.clone(), first.clone()];
    for (clip, at) in lane.clips.iter_mut().zip([0.0, 0.5, 0.25]) {
        clip.start = first.start + at * length;
    }
    let before = Timeline::build(&project, None);
    let row_at =
        |t: &Timeline, start: f64| t.lanes[0].clips.iter().find(|c| (c.start - start).abs() < 1e-9).map(|c| c.row);
    let middle = project.arrangement.lanes[0].clips[1].clone();
    assert_eq!(row_at(&before, middle.start), Some(2));
    let cut = 0.6 * length;
    let mut tail = middle.clone();
    tail.start = middle.start + cut;
    tail.length = middle.length - cut;
    tail.content = suhina_engine::model::ContentId(9_000);
    let content = project.content(middle.content).cloned().unwrap();
    project.contents.push(ClipContent { id: suhina_engine::model::ContentId(9_000), ..content });
    let lane = &mut project.arrangement.lanes[0];
    lane.clips[1].length = cut;
    lane.clips.push(tail);
    let after = Timeline::build_after(&project, None, &before.stack_rows);
    assert_eq!(row_at(&after, middle.start), Some(2));
    assert_eq!(row_at(&after, middle.start + cut), Some(2), "the end part stays on the row");
}

#[test]
fn headers_show_what_automation_plays_while_it_plays() {
    use suhina_engine::model::{AutomationLane, AutomationPoint};
    let mut project = demo_project();
    let track = project.tracks[0].id;
    let channel = project.tracks[0].channel.unwrap();
    let c = project.channel_mut(channel).unwrap();
    c.volume = 1.0;
    c.automation.push(AutomationLane {
        param: "volume".into(),
        points: vec![AutomationPoint::new(0.0, 1.0), AutomationPoint::new(8.0, 0.5)],
        enabled: true,
    });
    let mut timeline = Timeline::build(&project, None);
    let volume = |t: &Timeline| t.lanes.iter().find(|l| l.track == track).unwrap().volume_db;
    timeline.running = true;
    timeline.beat = 8.0;
    timeline.follow_automation(&project);
    assert!((volume(&timeline) + 6.02).abs() < 0.05, "{}", volume(&timeline));
    timeline.running = false;
    timeline.follow_automation(&project);
    assert!(volume(&timeline).abs() < 1e-9, "stopped: the project's own value");
}

#[test]
fn stacked_clips_take_rows_only_where_they_overlap() {
    // Two clips over each other at the start, a third on its own later (#697).
    let mut project = demo_project();
    let lane = &mut project.arrangement.lanes[0];
    let first = lane.clips[0].clone();
    let length = first.length;
    lane.clips = vec![first.clone(), first.clone(), first.clone()];
    for (clip, at) in lane.clips.iter_mut().zip([0.0, 0.5, 3.0]) {
        clip.start = first.start + at * length;
    }
    let timeline = Timeline::build(&project, None);
    let rows: Vec<(usize, usize)> = timeline.lanes[0].clips.iter().map(|c| (c.row, c.rows)).collect();
    assert_eq!(rows, [(0, 2), (1, 2), (0, 1)], "the lone clip fills the lane");
}

#[test]
fn a_stacked_clip_deleted_leaves_the_others_in_their_rows() {
    // Three clips of the first lane stacked over the first one's span.
    let mut project = demo_project();
    let lane = &mut project.arrangement.lanes[0];
    let first = lane.clips[0].clone();
    lane.clips = vec![first.clone(), first.clone(), first.clone()];
    for (i, clip) in lane.clips.iter_mut().enumerate() {
        clip.start = first.start + i as f64 * 1e-3;
    }
    let before = Timeline::build(&project, None);
    let rows = |t: &Timeline| t.lanes[0].clips.iter().map(|c| (c.row, c.rows)).collect::<Vec<_>>();
    assert_eq!(rows(&before), [(0, 3), (1, 3), (2, 3)]);
    // The middle one goes: the top keeps row 0, the bottom stays in row 2, the gap stays.
    project.arrangement.lanes[0].clips.remove(1);
    let after = Timeline::build_after(&project, None, &before.stack_rows);
    assert_eq!(rows(&after), [(0, 3), (2, 3)]);
    // The bottom one goes too: the empty rows under the top one go with it.
    project.arrangement.lanes[0].clips.remove(1);
    let last = Timeline::build_after(&project, None, &after.stack_rows);
    assert_eq!(rows(&last), [(0, 1)]);
}

#[test]
fn a_looping_clip_repeats_its_notes() {
    let project = demo_project();
    let clip = &project.arrangement.lanes[0].clips[0];
    let content = project.content(clip.content).unwrap();
    let marks = note_marks(content, clip);
    // Four notes a bar, sixteen bars in the clip; past its end the marks go on, for the passes
    // a drag of its right edge shows.
    assert_eq!(marks.iter().filter(|m| m.start < clip.length).count(), 64);
    assert_eq!(marks[4].start, 4.0);
    assert!(marks.iter().any(|m| m.start >= clip.length));
}

#[test]
fn a_looping_clip_marks_where_each_pass_starts() {
    let project = demo_project();
    let mut clip = project.arrangement.lanes[0].clips[0].clone();
    clip.loop_range = Some(suhina_engine::model::LoopRange { start: 1.0, end: 3.0 });
    clip.length = 7.0;
    // From the offset to the loop's end, then every two beats, none at the clip's end.
    clip.offset = 0.0;
    assert_eq!(loop_seams(&clip), [3.0, 5.0]);
    clip.length = 5.0;
    assert_eq!(loop_seams(&clip), [3.0]);
    // An offset past the loop starts at the loop's start, a whole pass first.
    clip.offset = 4.0;
    assert_eq!(loop_seams(&clip), [2.0, 4.0]);
    clip.loop_range = None;
    assert!(loop_seams(&clip).is_empty());
}

#[test]
fn a_clip_without_a_loop_shows_its_window_of_the_content() {
    let project = demo_project();
    let mut clip = project.arrangement.lanes[0].clips[0].clone();
    clip.loop_range = None;
    clip.offset = 1.0;
    clip.length = 2.0;
    let content = project.content(clip.content).unwrap();
    let starts: Vec<f64> = note_marks(content, &clip).iter().map(|m| m.start).collect();
    assert_eq!(starts, [0.0, 1.0]);
}

/// The demo project with Drums and Bass inside a group, "Group", in front of them, as
/// `track.group` leaves them.
fn grouped_project() -> Project {
    crate::demo::grouped_project()
}

#[test]
fn a_folded_group_shows_its_first_track_and_follows_the_one_being_worked_on() {
    let project = grouped_project();
    let group = TrackId(50);
    let (drums, bass) = (TrackId(1), TrackId(2));
    let mut folds = Folds::default();
    assert!(!folds.is_folded(group));
    assert_eq!(folds.shown(group), None);

    folds.toggle(group);
    folds.reconcile(&project, None);
    assert!(folds.is_folded(group));
    assert_eq!(folds.shown(group), Some(drums), "the first track inside the group");

    // Working on a track of the group's own: the row follows it.
    folds.reconcile(&project, Some(bass));
    assert_eq!(folds.shown(group), Some(bass));
    // A track outside it leaves the row on the first one, and a click opens the group again.
    folds.reconcile(&project, Some(TrackId(3)));
    assert_eq!(folds.shown(group), Some(drums));
    folds.toggle(group);
    folds.reconcile(&project, None);
    assert!(!folds.is_folded(group));
    assert_eq!(folds.shown(group), None);
}

#[test]
fn a_fold_forgets_groups_the_project_no_longer_has() {
    let mut project = grouped_project();
    let group = TrackId(50);
    let mut folds = Folds::default();
    folds.fold(group);
    folds.reconcile(&project, None);
    assert!(folds.is_folded(group));
    // The group's tracks are gone: the fold goes with them.
    project.tracks.retain(|t| t.parent != Some(group));
    folds.reconcile(&project, None);
    assert!(!folds.is_folded(group));
    assert_eq!(folds.shown(group), None);
}

#[test]
fn inside_lanes_counts_the_tracks_a_group_holds() {
    let timeline = Timeline::build(&grouped_project(), None);
    let lanes: Vec<&Lane> = timeline.lanes.iter().collect();
    let group = lanes.iter().position(|l| l.track == TrackId(50)).unwrap();
    assert_eq!(inside_lanes(&lanes, group), 2);
    // A track holds nothing, and the last lane has nothing after it.
    assert_eq!(inside_lanes(&lanes, group + 1), 0);
    assert_eq!(inside_lanes(&lanes, lanes.len() + 5), 0);
}

// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn view(markers: &[(f64, f64)]) -> AudioView {
    AudioView {
        file: "a.wav".into(),
        offset: 0.0,
        loop_range: None,
        markers: markers.to_vec(),
        seconds_per_beat: 0.5,
        rate: 1.0,
        gain_db: 0.0,
        envelope: Vec::new(),
    }
}

#[test]
fn picked_markers_move_together_between_the_ones_not_moved() {
    let markers = [(0.0, 0.0), (1.0, 0.5), (2.0, 1.0), (3.0, 1.5), (4.0, 2.0)];
    // 1 and 2 go back to just after 0 and on to just before 3.
    let (low, high) = group_bounds(&markers, &[1.0, 2.0]);
    assert!((low - (MARKER_GAP - 1.0)).abs() < 1e-9 && (high - (1.0 - MARKER_GAP)).abs() < 1e-9, "{low} {high}");
    // 1 and 3 together: 2 between them holds both.
    let (low, high) = group_bounds(&markers, &[1.0, 3.0]);
    assert!((low - (MARKER_GAP - 1.0)).abs() < 1e-9 && (high - (1.0 - MARKER_GAP)).abs() < 1e-9, "{low} {high}");
    // The last one has no neighbour after it.
    assert_eq!(group_bounds(&markers, &[4.0]).1, f64::INFINITY);
    assert_eq!(markers_in(&markers, Some((0.5, 3.0))), [1.0, 2.0, 3.0]);
    assert!(markers_in(&markers, None).is_empty());
    let drag = MarkerDrag { from: 2.0, grab: 0.0, to: 2.5, others: vec![1.0] };
    assert_eq!((drag.moves(1.0), drag.moves(3.0), drag.moved()), (Some(1.5), None, vec![1.0, 2.0]));
    assert!(drag.lands_on(1.5) && drag.lands_on(2.5) && !drag.lands_on(2.0));
}

fn clip_with(id: u32, length: f64, markers: &[(f64, f64)]) -> AudioClip {
    AudioClip {
        address: json!({ "track": 1, "clip": id }),
        id: ClipId(id),
        view: view(markers),
        length,
        start: 0.0,
        endless: false,
        color: Color32::WHITE,
        bar_beats: 4.0,
        name: format!("clip {id}"),
        others: Vec::new(),
    }
}

#[test]
fn a_marker_edit_goes_to_every_picked_clip_as_long_in_one_gesture() {
    let mut shown = clip_with(1, 8.0, &[(0.0, 0.0), (2.0, 1.0), (8.0, 4.0)]);
    shown.others = vec![
        // As long, without a marker at beat 2: it gets one first.
        clip_with(2, 8.0, &[(0.0, 0.0), (8.0, 4.0)]),
        // Another length: left alone.
        clip_with(3, 6.0, &[(0.0, 0.0), (6.0, 3.0)]),
    ];
    let moved = Action::new("clip.moveWarpMarker", json!({ "track": 1, "clip": 1, "from": 2.0, "beat": 2.5 }));
    let out = for_joined(&shown, moved);
    let ids: Vec<(&str, Value)> = out.iter().map(|a| (a.id, a.params["clip"].clone())).collect();
    assert_eq!(
        ids,
        [("clip.moveWarpMarker", json!(1)), ("clip.addWarpMarker", json!(2)), ("clip.moveWarpMarker", json!(2))]
    );
    let gesture = &out[0].params["gesture"];
    assert!(gesture.is_string() && out.iter().all(|a| &a.params["gesture"] == gesture), "one gesture");
    assert_eq!(out[1].params["beat"], json!(2.0), "pinned where the moved one was");
    // A removal only where the clip has the marker and keeps two.
    let removed = Action::new("clip.removeWarpMarker", json!({ "track": 1, "clip": 1, "beat": 2.0 }));
    assert_eq!(for_joined(&shown, removed).len(), 1);
    // Another action, or no clip joined, is the action alone.
    let other = Action::new("clip.setLoop", json!({ "track": 1, "clip": 1, "loop": true }));
    assert_eq!(for_joined(&shown, other).len(), 1);
}

#[test]
fn delete_removes_the_picked_markers_in_one_command_and_keeps_two() {
    let clip = AudioClip {
        address: json!({ "track": 1, "clip": 2 }),
        id: ClipId(2),
        view: view(&[(0.0, 0.0), (1.0, 0.5), (2.0, 1.0), (4.0, 2.0)]),
        length: 8.0,
        start: 0.0,
        endless: false,
        color: Color32::WHITE,
        bar_beats: 4.0,
        name: String::new(),
        others: Vec::new(),
    };
    let state = |picked: Vec<f64>| AudioEditorState { shown: Some(clip.address.clone()), picked, ..Default::default() };
    let action = state(vec![1.0, 2.0]).delete_key(&clip).map(|a| (a.id, a.params["beats"].clone()));
    assert_eq!(action, Some(("clip.removeWarpMarkers", json!([1.0, 2.0]))));
    let one = state(vec![2.0]).delete_key(&clip).map(|a| (a.id, a.params["beat"].clone()));
    assert_eq!(one, Some(("clip.removeWarpMarker", json!(2.0))));
    assert!(state(vec![0.0, 1.0, 2.0]).delete_key(&clip).is_none(), "two markers stay");
}

#[test]
fn the_loop_key_loops_a_selection_from_before_the_clips_first_beat_from_beat_0() {
    let clip = AudioClip {
        address: json!({ "track": 1, "clip": 2 }),
        id: ClipId(2),
        view: view(&[(-0.375, 0.0), (4.0, 2.0)]),
        length: 8.0,
        start: 0.0,
        endless: false,
        color: Color32::WHITE,
        bar_beats: 4.0,
        name: String::new(),
        others: Vec::new(),
    };
    let state = AudioEditorState { shown: Some(clip.address.clone()), span: Some((-0.375, 2.0)), ..Default::default() };
    let actions = state.loop_key(&clip);
    let params: Vec<(&str, Value)> = actions.iter().map(|a| (a.id, a.params.clone())).collect();
    assert_eq!(params.len(), 2, "{params:?}");
    assert_eq!((params[0].0, &params[0].1["start"], &params[0].1["end"]), ("clip.setLoop", &json!(0.0), &json!(2.0)));
    assert_eq!((params[1].0, &params[1].1["offset"]), ("clip.setStart", &json!(0.0)));
}

#[test]
fn an_unwarped_clip_plays_a_steady_rate() {
    let v = view(&[]);
    assert_eq!(seconds_at(&v, 4.0), 2.0);
    assert_eq!(beat_at(&v, 2.0), 4.0);
}

#[test]
fn markers_join_by_straight_lines_and_the_ends_carry_on() {
    // 2 beats take 1 s, then 2 beats take 2 s.
    let v = view(&[(0.0, 0.0), (2.0, 1.0), (4.0, 3.0)]);
    assert_eq!(seconds_at(&v, 1.0), 0.5);
    assert_eq!(seconds_at(&v, 3.0), 2.0);
    assert_eq!(seconds_at(&v, 6.0), 5.0, "the last segment carries on");
    assert_eq!(seconds_at(&v, -2.0), -1.0, "the first segment carries on");
    for beat in [-2.0, 0.5, 2.0, 3.5, 6.0] {
        assert!((beat_at(&v, seconds_at(&v, beat)) - beat).abs() < 1e-9, "{beat}");
    }
}

#[test]
fn markers_that_stand_still_in_the_file_do_not_divide_by_zero() {
    let v = view(&[(0.0, 1.0), (2.0, 1.0)]);
    assert!(seconds_at(&v, 1.0).is_finite());
    assert!(beat_at(&v, 1.0).is_finite());
}

#[test]
fn the_played_part_runs_to_the_loops_end_or_the_clips_length() {
    let mut v = view(&[]);
    v.offset = 2.0;
    assert_eq!(played(&v, 4.0), (2.0, 6.0));
    v.rate = 2.0;
    assert_eq!(played(&v, 4.0), (2.0, 10.0));
    v.loop_range = Some((1.0, 5.0));
    assert_eq!(played(&v, 4.0), (1.0, 5.0));
}

#[test]
fn fitting_fills_the_area_and_zooming_keeps_the_beat_under_the_pointer() {
    let mut state = AudioEditorState::default();
    state.fit(16.0, 816.0);
    assert!((state.beat_width() - 50.0).abs() < 1e-3);
    let before = (state.scroll + 100.0) / state.beat_width;
    state.zoom(2.0, 100.0);
    let after = (state.scroll + 100.0) / state.beat_width;
    assert!((before - after).abs() < 1e-4);
    assert!((state.beat_width() - 100.0).abs() < 1e-3);
}

#[test]
fn a_zoom_that_is_not_a_number_changes_nothing() {
    let mut state = AudioEditorState::default();
    state.fit(8.0, 100.0);
    let kept = state.clone();
    state.zoom(f32::NAN, 10.0);
    state.zoom(-1.0, 10.0);
    assert_eq!(state, kept);
}

#[test]
fn the_grid_starts_adaptive_and_the_grid_keys_change_it() {
    let grid = AudioEditorState::default().grid();
    assert!(grid.is_adaptive() && grid.snap);
    let finer = grid_key("pianoRoll.gridNarrow", grid);
    assert!(finer.is_some_and(|g| g != grid), "Ctrl+1 makes it finer");
    assert_eq!(finer.and_then(|g| grid_key("pianoRoll.gridWiden", g)), Some(grid), "Ctrl+2 back");
    assert_eq!(grid_key("pianoRoll.gridSnap", grid).map(|g| g.snap), Some(false), "Ctrl+4 stops snapping");
    assert_eq!(grid_key("pianoRoll.gridTriplet", grid).map(|g| g.triplet), Some(true));
    assert_eq!(grid_key("pianoRoll.zoomClip", grid), None);
    // A fixed grid as fine as 1/16 at a zoom where its lines would crowd is drawn coarser.
    let sixteenth = PianoGrid::from_value("1/16");
    let step = TimelineGrid::new(sixteenth.step(4.0, 4.0), 4.0, 4.0, GRID_LEAST).step;
    assert!(f64::from(4.0_f32) * step >= f64::from(GRID_LEAST), "{step}");
}

#[test]
fn the_adaptive_grid_goes_finer_than_a_32nd_and_says_its_value() {
    let grid = default_grid();
    // Far in, the lines go down to a 4096th note.
    assert_eq!(editor_step(grid, 1.0e7, 4.0), EDITOR_FINEST);
    // Narrow lines are 18 points apart here: at 400 points a beat a 128th (12.5 points) is too
    // close, a 64th (25 points) fits.
    assert_eq!(editor_step(grid, 400.0, 4.0), 1.0 / 16.0);
    // Two bars fitted to about 1,600 points (200 a beat): 32nds, not 64ths (12.5 points).
    assert_eq!(editor_step(grid, 200.0, 4.0), 1.0 / 8.0);
    assert_eq!(step_label(1.0 / 32.0, 4.0), "1/128");
    assert_eq!(step_label(0.25, 4.0), "1/16");
    assert_eq!(step_label(4.0, 4.0), "1 Bar");
    assert_eq!(step_label(8.0, 4.0), "2 Bars");
    assert_eq!(step_label(EDITOR_FINEST, 4.0), "1/4096");
}

#[test]
fn a_dragged_loop_end_stays_clear_of_the_other_and_the_bar_keeps_its_length() {
    let from = (1.0, 3.0);
    assert_eq!(dragged_loop(from, LoopPart::End, 0.6, 0.25, false), (1.0, 3.5), "on the grid");
    assert_eq!(dragged_loop(from, LoopPart::End, -5.0, 0.25, false), (1.0, 1.25), "a step after the start");
    assert_eq!(dragged_loop(from, LoopPart::Start, 5.0, 0.25, false), (2.75, 3.0), "a step before the end");
    assert_eq!(dragged_loop(from, LoopPart::Start, -3.0, 0.25, false), (0.0, 3.0), "never before 0");
    assert_eq!(dragged_loop(from, LoopPart::Whole, 0.4, 0.25, false), (1.5, 3.5), "the same length");
    let (start, end) = dragged_loop(from, LoopPart::Whole, 0.4, 0.25, true);
    assert!((start - 1.4).abs() < 1e-9 && (end - 3.4).abs() < 1e-9, "Alt: free: {start} {end}");
}

#[test]
fn marks_snap_to_the_grid_unless_free() {
    assert_eq!(snap(2.3, 0.25, false), 2.25);
    assert_eq!(snap(2.4, 0.25, false), 2.5);
    assert_eq!(snap(2.3, 0.25, true), 2.3);
    assert_eq!(snap(2.3, 0.0, false), 2.3);
    assert!(snap(f64::NAN, 0.25, false).is_nan());
}

#[test]
fn the_ruler_counts_bars_beats_and_sixteenths_from_one() {
    assert_eq!(position_label(0.0, 4.0), "1.1.1");
    assert_eq!(position_label(5.5, 4.0), "2.2.3");
    assert_eq!(position_label(3.99, 4.0), "1.4.4");
    assert_eq!(position_label(9.0, 3.0), "4.1.1");
    assert_eq!(position_label(-0.25, 4.0), "0.4.4", "before the first bar it counts back");
}

#[test]
fn a_bar_number_is_written_every_few_bars_when_bars_crowd() {
    assert_eq!(label_stride(100.0), 1);
    assert_eq!(label_stride(20.0), 2);
    assert_eq!(label_stride(3.0), 16);
    assert_eq!(label_stride(0.0), 1 << 30);
}

#[test]
fn the_waveform_colour_setting_says_what_ink_the_waveform_is_drawn_in() {
    let clip = Color32::from_rgb(10, 120, 200);
    let panel = Color32::from_rgb(30, 30, 30);
    assert_eq!(waveform_ink(false, clip, panel), clip, "the clip's colour");
    // On Theme, with no theme rule for `clip-waveform`, a neutral ink, not the clip's colour.
    assert_ne!(waveform_ink(true, clip, panel), clip);
}

#[test]
fn a_marker_command_names_the_clip_and_adds_its_own_params() {
    let params = with_address(&json!({ "track": 3, "clip": 7 }), json!({ "beat": 2.5 }));
    assert_eq!(params, json!({ "track": 3, "clip": 7, "beat": 2.5 }));
}

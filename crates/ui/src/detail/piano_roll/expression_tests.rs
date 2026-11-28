// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::detail::piano_roll::{PianoClip, PianoRollState};
use suhina_engine::model::{NoteExpression, Shared};

fn layout(clip: &PianoClip, state: &PianoRollState) -> Layout {
    crate::detail::piano_roll::tests::layout_for(clip, state)
}

fn bent(time: f64, duration: f64, key: u8, points: &[(f64, f64)]) -> Note {
    let mut note = Note::new(time, duration, key, 0.8);
    note.expression = Some(Shared::new(NoteExpression {
        pitch: points.iter().map(|&(o, v)| ExpressionPoint::new(o, v)).collect(),
        ..NoteExpression::default()
    }));
    note
}

fn points(curve: &[(f64, f64)]) -> Vec<ExpressionPoint> {
    curve.iter().map(|&(o, v)| ExpressionPoint::new(o, v)).collect()
}

#[test]
fn pitch_y_and_pitch_at_are_inverse_on_whole_rows() {
    let clip = crate::detail::piano_roll::tests::clip_with(vec![Note::new(0.0, 1.0, 60, 0.8)]);
    let state = PianoRollState::default();
    let layout = layout(&clip, &state);
    for key in [48.0, 60.0, 64.5, 72.0] {
        let y = pitch_y(&layout, key).unwrap();
        assert!((pitch_at(&layout, y).unwrap() - key).abs() < 1e-3, "{key}");
    }
    assert!(pitch_y(&layout, 64.0).unwrap() < pitch_y(&layout, 60.0).unwrap());
}

#[test]
fn the_pitch_line_starts_at_the_note_and_holds_to_its_end() {
    let clip = crate::detail::piano_roll::tests::clip_with(vec![bent(1.0, 2.0, 60, &[(0.5, 0.0), (1.0, 4.0)])]);
    let state = PianoRollState::default();
    let layout = layout(&clip, &state);
    let note = &clip.notes[0];
    let line = pitch_line(&layout, note, note.expression().unwrap().curve(ExpressionKind::Pitch));
    assert_eq!(line.len(), 4);
    assert_eq!(line[0].x, layout.x_of(1.0));
    assert_eq!(line[3].x, layout.x_of(3.0));
    assert_eq!(line[3].y, pitch_y(&layout, 64.0).unwrap());
}

#[test]
fn a_curved_segment_is_drawn_in_steps() {
    let mut note = Note::new(0.0, 2.0, 60, 0.8);
    note.expression_mut().pitch = vec![ExpressionPoint::curved(0.0, 0.0, 0.8), ExpressionPoint::new(1.0, 4.0)];
    let clip = crate::detail::piano_roll::tests::clip_with(vec![note]);
    let state = PianoRollState::default();
    let layout = layout(&clip, &state);
    let note = &clip.notes[0];
    let line = pitch_line(&layout, note, &note.expression().unwrap().pitch);
    assert!(line.len() > 10, "{}", line.len());
    // Halfway along the segment it is already above the straight line's middle.
    let middle = line.iter().find(|p| (p.x - layout.x_of(0.5)).abs() < 0.01).unwrap();
    assert!(middle.y < pitch_y(&layout, 62.0).unwrap());
}

#[test]
fn a_first_point_also_starts_the_curve_at_the_note() {
    let clip = crate::detail::piano_roll::tests::clip_with(vec![Note::new(0.0, 4.0, 60, 0.8)]);
    let state = PianoRollState::default();
    let layout = layout(&clip, &state);
    let grid = TimelineGrid::new(0.25, layout.beat_width, 4.0, 6.0);
    let pos = Pos2::new(layout.x_of(1.0), pitch_y(&layout, 67.0).unwrap());
    let action = add_point(&layout, &clip, grid, true, (0, ExpressionKind::Pitch, layout.grid), pos).unwrap();
    assert_eq!(action.id, "note.setExpression");
    assert_eq!(action.params["points"], json!([[0.0, 0.0], [1.0, 7.0]]));
}

#[test]
fn a_lane_point_starts_from_the_neutral_value() {
    let clip = crate::detail::piano_roll::tests::clip_with(vec![Note::new(0.0, 4.0, 60, 0.8)]);
    let state = PianoRollState::default();
    let layout = layout(&clip, &state);
    let grid = TimelineGrid::new(0.25, layout.beat_width, 4.0, 6.0);
    let lane = Rect::from_min_size(Pos2::new(36.0, 300.0), Vec2::new(400.0, 64.0));
    let pos = Pos2::new(layout.x_of(2.0), lane_y(lane, 100.0 / 127.0));
    let action = add_point(&layout, &clip, grid, true, (0, ExpressionKind::Timbre, lane), pos).unwrap();
    assert_eq!(action.params["kind"], "timbre");
    let points = action.params["points"].as_array().unwrap();
    assert_eq!(points[0], json!([0.0, 0.5]));
    assert!((points[1][1].as_f64().unwrap() - 100.0 / 127.0).abs() < 1e-6);
}

#[test]
fn lane_heights_and_values_are_inverse() {
    let lane = Rect::from_min_size(Pos2::new(0.0, 100.0), Vec2::new(200.0, 60.0));
    for value in [0.0, 0.25, 0.5, 1.0] {
        assert!((lane_value(lane, lane_y(lane, value)) - value).abs() < 1e-6);
    }
    assert!(lane_y(lane, 1.0) < lane_y(lane, 0.0));
}

#[test]
fn a_double_click_point_goes_to_the_nearest_selected_note_in_time() {
    let clip = crate::detail::piano_roll::tests::clip_with(vec![
        Note::new(0.0, 2.0, 48, 0.8),
        Note::new(0.0, 2.0, 60, 0.8),
        Note::new(4.0, 2.0, 62, 0.8),
    ]);
    let state = PianoRollState { selected: [0, 1, 2].into(), ..PianoRollState::default() };
    let layout = layout(&clip, &state);
    let near_upper = Pos2::new(layout.x_of(1.0), pitch_y(&layout, 58.0).unwrap());
    assert_eq!(note_for_point(&layout, &clip, &state, near_upper), Some(1));
    let near_lower = Pos2::new(layout.x_of(1.0), pitch_y(&layout, 50.0).unwrap());
    assert_eq!(note_for_point(&layout, &clip, &state, near_lower), Some(0));
    let outside = Pos2::new(layout.x_of(3.0), pitch_y(&layout, 60.0).unwrap());
    assert_eq!(note_for_point(&layout, &clip, &state, outside), None);
}

#[test]
fn a_dragged_point_removes_the_neighbours_it_passes() {
    let from = points(&[(0.0, 0.0), (1.0, 2.0), (2.0, 4.0), (3.0, 1.0)]);
    let moved =
        moved_points(&from, &BTreeSet::new(), 1, Move { beats: 1.5, value: 1.0 }, 4.0, ExpressionKind::Pitch, false);
    assert_eq!(moved, points(&[(0.0, 0.0), (2.5, 3.0), (3.0, 1.0)]));
    // Kept inside the note and on whole semitones with snap.
    let moved =
        moved_points(&from, &BTreeSet::new(), 3, Move { beats: 5.0, value: 0.4 }, 4.0, ExpressionKind::Pitch, true);
    assert_eq!(moved.last(), Some(&ExpressionPoint::new(4.0, 1.0)));
}

#[test]
fn selected_points_move_together() {
    let from = points(&[(0.0, 0.0), (1.0, 2.0), (2.0, 4.0), (3.0, 1.0)]);
    let group = BTreeSet::from([1, 2]);
    let moved = moved_points(&from, &group, 2, Move { beats: 0.25, value: -1.0 }, 4.0, ExpressionKind::Pitch, false);
    assert_eq!(moved, points(&[(0.0, 0.0), (1.25, 1.0), (2.25, 3.0), (3.0, 1.0)]));
}

#[test]
fn shift_keeps_a_drag_to_one_axis_and_makes_vertical_moves_fine() {
    let moved = Move { beats: 1.0, value: 2.0 };
    assert_eq!(constrain(moved, Vec2::new(30.0, 5.0), true), Move { beats: 1.0, value: 0.0 });
    assert_eq!(constrain(moved, Vec2::new(3.0, 50.0), true), Move { beats: 0.0, value: 0.2 });
    assert_eq!(constrain(moved, Vec2::new(3.0, 50.0), false), moved);
}

#[test]
fn removing_the_last_non_neutral_point_clears_the_curve() {
    let curve = points(&[(0.0, 0.0), (1.0, 3.0)]);
    assert!(removed(&curve, 1, ExpressionKind::Pitch).is_empty());
    let slide = points(&[(0.0, 0.5), (1.0, 0.9), (2.0, 0.2)]);
    assert_eq!(removed(&slide, 1, ExpressionKind::Timbre), points(&[(0.0, 0.5), (2.0, 0.2)]));
}

#[test]
fn a_freehand_stroke_replaces_what_it_passes_over() {
    let mut curve = points(&[(0.0, 0.0), (0.5, 1.0), (1.0, 2.0), (2.0, 3.0)]);
    let last = draw_point(&mut curve, f64::NAN, 0.25, 5.0);
    let last = draw_point(&mut curve, last, 1.25, 6.0);
    assert_eq!(curve, points(&[(0.0, 0.0), (0.25, 5.0), (1.25, 6.0), (2.0, 3.0)]));
    // Closer than a step to the last point: it takes its place.
    draw_point(&mut curve, last, 1.26, 7.0);
    assert_eq!(curve, points(&[(0.0, 0.0), (0.25, 5.0), (1.26, 7.0), (2.0, 3.0)]));
}

#[test]
fn typed_values_are_semitones_or_steps_of_127() {
    assert_eq!(parse_value(ExpressionKind::Pitch, "+4.50 st"), Some(4.5));
    assert_eq!(parse_value(ExpressionKind::Pitch, "-3,5"), Some(-3.5));
    assert_eq!(parse_value(ExpressionKind::Pressure, "127"), Some(1.0));
    assert_eq!(parse_value(ExpressionKind::Pressure, "500"), Some(1.0));
    assert_eq!(parse_value(ExpressionKind::Timbre, "soft"), None);
    assert_eq!(value_text(ExpressionKind::Pitch, 4.0), "+4.00 st");
    assert_eq!(value_text(ExpressionKind::Pressure, 1.0), "127");
}

#[test]
fn ctrl_a_selects_the_points_of_the_one_selected_notes_curve() {
    let clip = crate::detail::piano_roll::tests::clip_with(vec![bent(0.0, 2.0, 60, &[(0.0, 0.0), (1.0, 2.0)])]);
    let mut state = PianoRollState { selected: [0].into(), ..PianoRollState::default() };
    assert!(!select_all_points(&clip, &mut state), "MPE off: the key selects notes");
    state.expression = Some(ExpressionKind::Pitch);
    assert!(select_all_points(&clip, &mut state));
    assert_eq!(state.mpe.selection.points, BTreeSet::from([0, 1]));
}

#[test]
fn the_lanes_share_the_room_below_the_header() {
    let mut state = PianoRollState { expression: Some(ExpressionKind::Pitch), ..PianoRollState::default() };
    let room = Rect::from_min_size(Pos2::new(0.0, 100.0), Vec2::new(300.0, 132.0));
    let velocity = split_lanes(&mut state, room);
    let lanes: Vec<Lane> = state.mpe.rects.iter().map(|(l, _)| *l).collect();
    assert_eq!(lanes, vec![Lane::Slide, Lane::Pressure, Lane::Velocity]);
    assert_eq!(velocity.height(), 40.0);
    assert_eq!(state.mpe.rects[0].1.top(), 100.0 + HEADER);
    // MPE off: the room is the velocity lane's.
    state.expression = None;
    assert_eq!(split_lanes(&mut state, room), room);
    assert!(state.mpe.rects.is_empty());
}

#[test]
fn a_point_taken_off_a_flat_stretch_bends_from_a_grid_step_before_it() {
    // Flat from the start to beat 4; the point at beat 4 dragged down two semitones.
    let from = points(&[(0.0, 0.0), (4.0, 0.0)]);
    let delta = Move { beats: 0.0, value: -2.0 };
    let moved = moved_points(&from, &BTreeSet::new(), 1, delta, 4.0, ExpressionKind::Pitch, true);
    let (held, at) = with_hold(&from, 1, &moved, (delta, 4.0, 0.25)).unwrap();
    assert_eq!(held, points(&[(0.0, 0.0), (3.75, 0.0), (4.0, -2.0)]));
    assert_eq!(at, 2);
}

#[test]
fn no_hold_point_after_a_slope_without_a_change_or_without_room() {
    let delta = Move { beats: 0.0, value: 2.0 };
    // A slope before: the bend already starts at the point before.
    let sloped = points(&[(0.0, 0.0), (1.0, 1.0)]);
    let moved = moved_points(&sloped, &BTreeSet::new(), 1, delta, 1.0, ExpressionKind::Pitch, true);
    assert!(with_hold(&sloped, 1, &moved, (delta, 1.0, 0.25)).is_none());
    // A sideways drag changes no value.
    let flat = points(&[(0.0, 0.0), (1.0, 0.0)]);
    let sideways = Move { beats: -0.25, value: 0.0 };
    let moved = moved_points(&flat, &BTreeSet::new(), 1, sideways, 1.0, ExpressionKind::Pitch, true);
    assert!(with_hold(&flat, 1, &moved, (sideways, 1.0, 0.25)).is_none());
    // The point before is within a step: no room for a hold.
    let close = points(&[(0.0, 0.0), (0.2, 0.0)]);
    let moved = moved_points(&close, &BTreeSet::new(), 1, delta, 1.0, ExpressionKind::Pitch, true);
    assert!(with_hold(&close, 1, &moved, (delta, 1.0, 0.25)).is_none());
}

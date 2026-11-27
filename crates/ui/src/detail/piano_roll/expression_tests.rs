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

#[test]
fn pitch_y_and_pitch_at_are_inverse_on_whole_rows() {
    let clip = crate::detail::piano_roll::tests::clip_with(vec![Note::new(0.0, 1.0, 60, 0.8)]);
    let state = PianoRollState::default();
    let layout = layout(&clip, &state);
    for key in [48.0, 60.0, 64.5, 72.0] {
        let y = pitch_y(&layout, key).unwrap();
        assert!((pitch_at(&layout, y).unwrap() - key).abs() < 1e-3, "{key}");
    }
    // A bend up is higher on the screen.
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
    assert_eq!(line[0].y, pitch_y(&layout, 60.0).unwrap());
    assert_eq!(line[3].y, pitch_y(&layout, 64.0).unwrap());
}

#[test]
fn a_first_point_also_starts_the_curve_at_the_note() {
    let clip = crate::detail::piano_roll::tests::clip_with(vec![Note::new(0.0, 4.0, 60, 0.8)]);
    let state = PianoRollState::default();
    let layout = layout(&clip, &state);
    let grid = TimelineGrid::new(0.25, layout.beat_width, 4.0, 6.0);
    let pos = Pos2::new(layout.x_of(1.0), pitch_y(&layout, 67.0).unwrap());
    let action = add_point(&layout, &clip, grid, false, (0, ExpressionKind::Pitch), pos).unwrap();
    assert_eq!(action.id, "note.setExpression");
    assert_eq!(action.params["kind"], "pitch");
    assert_eq!(action.params["note"], 0);
    assert_eq!(action.params["points"], json!([[0.0, 0.0], [1.0, 7.0]]));
}

#[test]
fn a_dragged_point_stays_between_its_neighbours_on_whole_semitones() {
    let clip =
        crate::detail::piano_roll::tests::clip_with(vec![bent(0.0, 4.0, 60, &[(0.0, 0.0), (1.0, 2.0), (2.0, 4.0)])]);
    let state = PianoRollState::default();
    let layout = layout(&clip, &state);
    let grid = TimelineGrid::new(0.25, layout.beat_width, 4.0, 6.0);
    let note = &clip.notes[0];
    let mut drag =
        PointDrag { note: 0, kind: ExpressionKind::Pitch, index: 1, curve: note.expression().unwrap().pitch.clone() };
    // Past the next point and between rows: held before it, on a whole semitone.
    let pos = Pos2::new(layout.x_of(3.0), pitch_y(&layout, 63.4).unwrap());
    drag_point(&layout, note, grid, false, &mut drag, pos);
    let moved = drag.curve[1];
    assert!(moved.offset < 2.0 && moved.offset > 1.9, "{moved:?}");
    assert_eq!(moved.value, 3.0);
    // Free: the fraction stays.
    drag_point(&layout, note, grid, true, &mut drag, Pos2::new(layout.x_of(1.5), pitch_y(&layout, 63.5).unwrap()));
    assert!((drag.curve[1].value - 3.5).abs() < 1e-3);
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
fn a_lane_point_starts_from_the_neutral_value_and_snaps_to_hundredths() {
    let clip = crate::detail::piano_roll::tests::clip_with(vec![Note::new(0.0, 4.0, 60, 0.8)]);
    let state = PianoRollState::default();
    let layout = layout(&clip, &state);
    let grid = TimelineGrid::new(0.25, layout.beat_width, 4.0, 6.0);
    let pos = Pos2::new(layout.x_of(2.0), lane_y(layout.lane, 0.733));
    let action = add_point(&layout, &clip, grid, false, (0, ExpressionKind::Timbre), pos).unwrap();
    assert_eq!(action.params["kind"], "timbre");
    assert_eq!(action.params["points"], json!([[0.0, 0.5], [2.0, 0.73]]));
}

#[test]
fn lane_heights_and_values_are_inverse() {
    let lane = Rect::from_min_size(Pos2::new(0.0, 100.0), Vec2::new(200.0, 60.0));
    for value in [0.0, 0.25, 0.5, 1.0] {
        assert!((lane_value(lane, lane_y(lane, value)) - value).abs() < 1e-6);
    }
    assert!(lane_y(lane, 1.0) < lane_y(lane, 0.0));
}

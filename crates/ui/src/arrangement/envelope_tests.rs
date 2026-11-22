// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn zero_db_is_the_middle_and_the_first_db_of_a_cut_are_wide() {
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 100.0));
    assert_eq!(y_of(rect, 0.0, 6.0), 50.0);
    assert_eq!(y_of(rect, 6.0, 6.0), 0.0);
    assert_eq!(y_of(rect, 3.0, 6.0), 25.0, "even in dB above the middle");
    assert_eq!(y_of(rect, -70.0, 6.0), 100.0);
    // Below it by gain: a 3 dB cut is near a third of the way down, 6 dB half of it.
    assert!((y_of(rect, -3.0, 6.0) - 64.6).abs() < 0.1, "{}", y_of(rect, -3.0, 6.0));
    assert!((y_of(rect, -6.0, 6.0) - 75.0).abs() < 0.1, "{}", y_of(rect, -6.0, 6.0));
    assert!(y_of(rect, -1.0, 6.0) - 50.0 > 5.0, "a dB is over five points of a 100-point clip");
    for db in [-0.5, -1.0, -6.0, -24.0, -60.0, 2.5] {
        assert!((db_at(rect, y_of(rect, db, 6.0), 6.0) - db).abs() < 1e-3, "{db}");
    }
    assert_eq!(db_at(rect, -10.0, 6.0), 6.0, "kept below the top");
    assert_eq!(db_at(rect, 120.0, 6.0), -70.0, "and above the floor");
}

#[test]
fn the_scale_reaches_a_point_above_the_ceiling() {
    let points = [GainPoint::new(0.0, 12.0)];
    assert_eq!(top(6.0, &points), 12.0);
    assert_eq!(top(6.0, &[]), 6.0);
    assert_eq!(top(24.0, &points), 24.0);
}

#[test]
fn points_snap_to_the_grid_unless_free() {
    assert_eq!(snapped(-3.4, 1.0, false), -3.0);
    assert_eq!(snapped(-3.6, 0.5, false), -3.5);
    assert_eq!(snapped(-3.44, 1.0, true), -3.4);
    assert_eq!(db_text(-3.0), "\u{2212}3 dB");
    assert_eq!(db_text(1.5), "+1.5 dB");
    assert_eq!(db_text(0.04), "0 dB");
}

#[test]
fn a_point_added_on_a_bent_segment_keeps_its_shape() {
    let first = GainPoint { curve: Curve::bend(0.8), ..GainPoint::new(0.0, 0.0) };
    let points = [first, GainPoint::new(4.0, -12.0)];
    let (more, at) = with_point(&points, 1.0, envelope_db_at(&points, 1.0));
    assert_eq!(at, 1);
    for beat in [0.5, 1.5, 3.0] {
        assert!((envelope_db_at(&more, beat) - envelope_db_at(&points, beat)).abs() < 1e-6, "{beat}");
    }
    let action = set(ClipId(1), &more);
    assert_eq!(action.params["curves"].as_array().map(Vec::len), Some(3));
    assert!(set(ClipId(1), &[GainPoint::new(0.0, 0.0)]).params.get("curves").is_none(), "straight: none sent");
}

#[test]
fn a_segment_moves_up_and_down_as_one_and_the_whole_line_with_it() {
    let points = [GainPoint::new(0.0, 0.0), GainPoint::new(2.0, -3.0), GainPoint::new(4.0, 2.0)];
    assert_eq!(segment_at(&points, 1.0), vec![0, 1]);
    assert_eq!(segment_at(&points, 5.0), vec![2], "after the last point: that point alone");
    assert_eq!(segment_at(&[], 1.0), Vec::<usize>::new());
    let db = |p: Vec<GainPoint>| p.iter().map(|p| p.db).collect::<Vec<_>>();
    // Ctrl+Shift: the segment's two points; Shift: all of them.
    assert_eq!(db(lifted(&points, Some(&[0, 1]), -2.0, 6.0)), vec![-2.0, -5.0, 2.0]);
    assert_eq!(db(lifted(&points, None, 3.0, 6.0)), vec![3.0, 0.0, 5.0]);
    // Kept below the ceiling and above −70 dB.
    assert_eq!(db(lifted(&points, None, 5.0, 6.0)), vec![5.0, 2.0, 6.0]);
    assert_eq!(db(lifted(&points, None, -80.0, 6.0)), vec![-70.0, -70.0, -70.0]);
}

#[test]
fn a_dragged_point_passes_the_points_it_is_dragged_over() {
    let at = |beats: &[f64]| beats.iter().map(|&b| GainPoint::new(b, 0.0)).collect::<Vec<_>>();
    let beats = |points: &[GainPoint]| points.iter().map(|p| p.beat).collect::<Vec<_>>();
    // The last point dragged left past the one before it.
    let mut points = at(&[0.0, 2.0, 4.0]);
    let index = passed(&mut points, 2, 1.0, -3.0);
    assert_eq!((index, beats(&points)), (1, vec![0.0, 1.0, 2.0]));
    assert_eq!(points[1].db, -3.0);
    // The first dragged right past both.
    let mut points = at(&[0.0, 2.0, 4.0]);
    let index = passed(&mut points, 0, 5.0, 0.0);
    assert_eq!((index, beats(&points)), (2, vec![2.0, 4.0, 5.0]));
    // Onto another's beat it stays on the side it came from.
    let mut points = at(&[0.0, 2.0, 4.0]);
    assert_eq!(passed(&mut points, 2, 2.0, 0.0), 2, "from the right");
    let mut points = at(&[0.0, 2.0, 4.0]);
    assert_eq!(passed(&mut points, 0, 2.0, 0.0), 0, "from the left");
}

#[test]
fn the_left_point_of_a_step_at_the_loops_start_is_not_drawn_where_a_pass_wraps() {
    let step = GainPoint::new;
    // 0 dB up to the loop's start at beat 1, then silence, a step: passes of the loop start
    // at the silent point, so the 0 dB one is drawn only where the clip comes to beat 1 from
    // before it.
    let points = [step(0.0, 0.0), step(1.0, 0.0), step(1.0, -70.0), step(2.0, -70.0)];
    let lead_in = Mapping { offset: 0.0, loop_range: Some((1.0, 2.0)), rate: 1.0 };
    assert_eq!(lead_in.point_places(&points, 1, 4.0), vec![1.0], "reached from before the loop");
    assert_eq!(lead_in.point_places(&points, 2, 4.0), vec![1.0, 2.0, 3.0, 4.0], "the step's right point on every pass");
    let from_start = Mapping { offset: 1.0, loop_range: Some((1.0, 2.0)), rate: 1.0 };
    assert!(from_start.point_places(&points, 1, 3.0).is_empty(), "never reached");
    assert_eq!(from_start.point_places(&points, 2, 3.0), vec![0.0, 1.0, 2.0, 3.0]);
}

#[test]
fn a_point_let_go_on_a_hidden_dot_is_not_merged_with_it() {
    let step = GainPoint::new;
    // A step at the loop's start, beat 1 (0 dB, then silence), and a loud point just after
    // it: from the loop's start its passes come round at 1, 2 and so on, where the step's left
    // point is never drawn (`point_places`), so the loud point is not one with it there.
    let points = [step(1.0, 0.0), step(1.0, -70.0), step(1.001, 0.0), step(2.0, -70.0)];
    let mapping = Mapping { offset: 1.0, loop_range: Some((1.0, 2.0)), rate: 1.0 };
    let drawn = |points: &[GainPoint], i: usize| -> Vec<Pos2> {
        let y = points[i].db as f32;
        mapping.point_places(points, i, 3.0).into_iter().map(|at| Pos2::new(at as f32 * 100.0, y)).collect()
    };
    assert!(!lands_on_another(&points, 2, drawn), "the hidden left point takes nothing");
    // Every place, hidden ones included, would have merged it.
    let every = |points: &[GainPoint], i: usize| -> Vec<Pos2> {
        let y = points[i].db as f32;
        mapping.places(points[i].beat, 3.0).into_iter().map(|at| Pos2::new(at as f32 * 100.0, y)).collect()
    };
    assert!(lands_on_another(&points, 2, every));
    // A dot that is drawn still takes it.
    let lead_in = Mapping { offset: 0.0, loop_range: Some((1.0, 2.0)), rate: 1.0 };
    let reached = |points: &[GainPoint], i: usize| -> Vec<Pos2> {
        let y = points[i].db as f32;
        lead_in.point_places(points, i, 3.0).into_iter().map(|at| Pos2::new(at as f32 * 100.0, y)).collect()
    };
    assert!(lands_on_another(&points, 2, reached), "reached from before the loop, the left point is drawn");
    assert!(!lands_on_another(&points, 9, reached));
}

/// A drag started in the audio editor ends when the editor stops drawing the clip before the
/// button comes up: a pass later no drawing shows its points, a lift it carried goes, and the
/// editor drawing the clip again does not bring it back (#1657).
#[test]
fn a_drag_ends_when_the_drawing_it_started_in_stops() {
    let ctx = egui::Context::default();
    let clip = ClipId(3);
    let committed = vec![GainPoint::new(0.0, 0.0)];
    let moved = vec![GainPoint::new(0.0, -6.0)];
    let mut shown = Vec::new();
    let mut left = Vec::new();
    // The editor starts a drag of the whole line.
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let seen = ui.ctx().cumulative_pass_nr();
        let draft = Draft { clip, site: "audio-editor", index: usize::MAX, points: moved.clone(), seen };
        let lift = Lift { clip, from: committed.clone(), moved: None, start: 0.0 };
        ui.data_mut(|d| {
            d.insert_temp(draft_key(), draft);
            d.insert_temp(lift_key(), lift);
        });
    });
    output.textures_delta.clear();
    // The editor draws it again: the drag stands.
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        keep_draft(ui.ctx(), clip, "audio-editor");
        shown.push(points_now(ui.ctx(), clip, &committed));
    });
    output.textures_delta.clear();
    // The editor is gone; the Arrangement, drawing first in a pass, still reads it once, and
    // its own drawing does not keep a drag that started elsewhere.
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        end_stale_draft(ui.ctx(), clip);
        shown.push(points_now(ui.ctx(), clip, &committed));
        keep_draft(ui.ctx(), clip, "arrangement");
    });
    output.textures_delta.clear();
    // A pass on, the drag is over for every drawing.
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        end_stale_draft(ui.ctx(), clip);
        shown.push(points_now(ui.ctx(), clip, &committed));
        left.push(ui.data(|d| (d.get_temp::<Draft>(draft_key()).is_some(), d.get_temp::<Lift>(lift_key()).is_some())));
    });
    output.textures_delta.clear();
    // The editor drawing the clip again starts from the envelope as set.
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        keep_draft(ui.ctx(), clip, "audio-editor");
        shown.push(points_now(ui.ctx(), clip, &committed));
    });
    output.textures_delta.clear();
    assert_eq!(shown, vec![moved.clone(), moved, committed.clone(), committed.clone(), committed]);
    assert_eq!(left, vec![(false, false)], "the draft and its lift went");
}

#[test]
fn a_point_is_placed_once_or_on_every_pass_of_the_loop() {
    let once = Mapping { offset: 1.0, loop_range: None, rate: 1.0 };
    assert_eq!(once.places(3.0, 4.0), vec![2.0]);
    assert!(once.places(0.5, 4.0).is_empty(), "before the start marker");
    let looped = Mapping { offset: 0.0, loop_range: Some((0.0, 2.0)), rate: 1.0 };
    assert_eq!(looped.places(0.5, 6.0), vec![0.5, 2.5, 4.5]);
    assert_eq!(looped.content(4.5), 0.5);
    // An unwarped clip an octave up plays its content twice as fast.
    let up = Mapping { offset: 0.0, loop_range: None, rate: 2.0 };
    assert_eq!(up.places(4.0, 4.0), vec![2.0]);
}

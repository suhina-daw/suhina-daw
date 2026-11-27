// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn p(offset: f64, value: f64) -> ExpressionPoint {
    ExpressionPoint::new(offset, value)
}

fn bend(points: &[(f64, f64)]) -> NoteExpression {
    NoteExpression { pitch: points.iter().map(|&(o, v)| p(o, v)).collect(), ..NoteExpression::default() }
}

#[test]
fn a_missing_curve_is_neutral() {
    let expression = NoteExpression::default();
    assert_eq!(expression.value_at(ExpressionKind::Pitch, 0.5), 0.0);
    assert_eq!(expression.value_at(ExpressionKind::Pressure, 0.5), 0.0);
    assert_eq!(expression.value_at(ExpressionKind::Timbre, 0.5), 0.5);
}

#[test]
fn values_interpolate_linearly_and_hold_the_ends() {
    let expression = bend(&[(0.5, 0.0), (1.5, 4.0)]);
    assert_eq!(expression.value_at(ExpressionKind::Pitch, 0.0), 0.0);
    assert_eq!(expression.value_at(ExpressionKind::Pitch, 1.0), 2.0);
    assert_eq!(expression.value_at(ExpressionKind::Pitch, 3.0), 4.0);
}

#[test]
fn set_point_replaces_an_equal_time_point_and_keeps_order() {
    let mut expression = NoteExpression::default();
    expression.set_point(ExpressionKind::Pitch, 2.0, 1.0, 3.0).unwrap();
    expression.set_point(ExpressionKind::Pitch, 2.0, 0.0, 0.0).unwrap();
    expression.set_point(ExpressionKind::Pitch, 2.0, 1.0, 5.0).unwrap();
    assert_eq!(expression.pitch, vec![p(0.0, 0.0), p(1.0, 5.0)]);
}

#[test]
fn set_point_clamps_into_the_note_and_the_range() {
    let mut expression = NoteExpression::default();
    expression.set_point(ExpressionKind::Pressure, 1.0, 4.0, 2.0).unwrap();
    assert_eq!(expression.pressure, vec![p(1.0, 1.0)]);
    expression.set_point(ExpressionKind::Pitch, 1.0, 0.0, -1000.0).unwrap();
    assert_eq!(expression.pitch, vec![p(0.0, -MAX_PITCH_SEMITONES)]);
    assert_eq!(expression.set_point(ExpressionKind::Pitch, 1.0, f64::NAN, 0.0), Err(ExpressionError::NotFinite));
    assert_eq!(expression.set_point(ExpressionKind::Pitch, 1.0, 0.0, f64::INFINITY), Err(ExpressionError::NotFinite));
}

#[test]
fn set_point_stops_at_the_curve_limit() {
    let mut expression = NoteExpression::default();
    for i in 0..MAX_CURVE_POINTS {
        expression.set_point(ExpressionKind::Timbre, MAX_CURVE_POINTS as f64, i as f64, 0.5).unwrap();
    }
    assert_eq!(
        expression.set_point(ExpressionKind::Timbre, MAX_CURVE_POINTS as f64, MAX_CURVE_POINTS as f64, 0.5),
        Err(ExpressionError::TooManyPoints(MAX_CURVE_POINTS + 1))
    );
    // Replacing an existing point still works at the limit.
    expression.set_point(ExpressionKind::Timbre, MAX_CURVE_POINTS as f64, 0.0, 0.1).unwrap();
}

#[test]
fn validate_rejects_malformed_curves() {
    assert_eq!(bend(&[(0.0, 0.0), (0.0, 1.0)]).validate(1.0), Err(ExpressionError::NotIncreasing));
    assert_eq!(bend(&[(0.5, 0.0), (0.25, 1.0)]).validate(1.0), Err(ExpressionError::NotIncreasing));
    assert_eq!(bend(&[(2.0, 0.0)]).validate(1.0), Err(ExpressionError::OffsetOutside { offset: 2.0, duration: 1.0 }));
    assert_eq!(bend(&[(-0.1, 0.0)]).validate(1.0), Err(ExpressionError::OffsetOutside { offset: -0.1, duration: 1.0 }));
    assert_eq!(
        bend(&[(0.0, 300.0)]).validate(1.0),
        Err(ExpressionError::ValueOutside { kind: ExpressionKind::Pitch, value: 300.0 })
    );
    assert_eq!(bend(&[(f64::NAN, 0.0)]).validate(1.0), Err(ExpressionError::NotFinite));
    let pressure = NoteExpression { pressure: vec![p(0.0, 1.5)], ..NoteExpression::default() };
    assert!(matches!(pressure.validate(1.0), Err(ExpressionError::ValueOutside { .. })));
    assert_eq!(bend(&[(0.0, 0.0), (1.0, 12.0)]).validate(1.0), Ok(()));
}

#[test]
fn crop_end_keeps_the_value_at_the_new_end() {
    let mut expression = bend(&[(0.0, 0.0), (2.0, 4.0)]);
    expression.crop_end(1.0);
    assert_eq!(expression.pitch, vec![p(0.0, 0.0), p(1.0, 2.0)]);
    // Lengthening holds the last value: nothing changes.
    expression.crop_end(8.0);
    assert_eq!(expression.pitch, vec![p(0.0, 0.0), p(1.0, 2.0)]);
}

#[test]
fn move_start_later_crops_and_rebases() {
    let mut expression = bend(&[(0.0, 0.0), (2.0, 4.0)]);
    expression.move_start(1.0);
    assert_eq!(expression.pitch, vec![p(0.0, 2.0), p(1.0, 4.0)]);
}

#[test]
fn move_start_earlier_holds_the_old_first_value() {
    let mut expression = bend(&[(0.5, 1.0), (1.0, 2.0)]);
    expression.move_start(-0.5);
    assert_eq!(expression.pitch, vec![p(1.0, 1.0), p(1.5, 2.0)]);
    assert_eq!(expression.value_at(ExpressionKind::Pitch, 0.0), 1.0);
}

#[test]
fn stretch_scales_offsets_and_ignores_bad_factors() {
    let mut expression = bend(&[(0.0, 0.0), (1.0, 4.0)]);
    expression.stretch(2.0);
    assert_eq!(expression.pitch, vec![p(0.0, 0.0), p(2.0, 4.0)]);
    expression.stretch(0.0);
    expression.stretch(f64::NAN);
    expression.stretch(-1.0);
    assert_eq!(expression.pitch, vec![p(0.0, 0.0), p(2.0, 4.0)]);
}

#[test]
fn split_gives_both_parts_the_boundary_value() {
    let expression = bend(&[(0.0, 0.0), (2.0, 4.0)]);
    let (left, right) = expression.split(1.0);
    assert_eq!(left.pitch, vec![p(0.0, 0.0), p(1.0, 2.0)]);
    assert_eq!(right.pitch, vec![p(0.0, 2.0), p(1.0, 4.0)]);
    assert!(left.pressure.is_empty() && right.pressure.is_empty());
}

#[test]
fn join_rebuilds_a_split_and_refuses_a_jump() {
    let expression = bend(&[(0.0, 0.0), (2.0, 4.0)]);
    let (left, right) = expression.split(1.0);
    let joined = left.joined(1.0, &right).unwrap();
    assert_eq!(joined.pitch, vec![p(0.0, 0.0), p(1.0, 2.0), p(2.0, 4.0)]);
    assert_eq!(joined.value_at(ExpressionKind::Pitch, 1.5), 3.0);

    let jump = bend(&[(0.0, 7.0)]);
    assert_eq!(left.joined(1.0, &jump), None);
}

#[test]
fn kinds_round_trip_their_names() {
    for kind in ExpressionKind::ALL {
        assert_eq!(ExpressionKind::from_name(kind.name()), Some(kind));
    }
    assert_eq!(ExpressionKind::from_name("slide"), None);
}

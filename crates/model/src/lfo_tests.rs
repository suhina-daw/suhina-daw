// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::expression::NoteExpression;

fn sine(amount: f64) -> Lfo {
    Lfo { wave: Wave::Sine, shape: 0.0, period: 1.0, shift: 0.0, amount, base: 0.0, attack: 0.0, decay: 0.0, seed: 1 }
}

fn kept(from: f64, to: f64, lfo: Lfo) -> NoteExpression {
    NoteExpression {
        pitch: vec![ExpressionPoint::new(0.0, 2.0)],
        lfos: vec![NoteLfo::new(ExpressionKind::Pitch, (from, to), lfo)],
        ..NoteExpression::default()
    }
}

#[test]
fn a_kept_lfo_rides_on_the_curve_over_its_stretch_only() {
    let e = kept(1.0, 3.0, sine(1.0));
    assert_eq!(e.value_at(ExpressionKind::Pitch, 0.5), 2.0, "before the stretch: the curve");
    assert!((e.value_at(ExpressionKind::Pitch, 1.25) - 3.0).abs() < 1e-9, "a quarter cycle in: the curve plus 1");
    assert!((e.value_at(ExpressionKind::Pitch, 1.75) - 1.0).abs() < 1e-9);
    assert_eq!(e.value_at(ExpressionKind::Pitch, 3.5), 2.0, "after it: the curve");
    assert_eq!(e.value_at(ExpressionKind::Pressure, 1.25), 0.0, "another kind is not moved");
}

#[test]
fn baking_writes_what_it_plays() {
    let e = kept(1.0, 3.0, Lfo { attack: 0.25, decay: 0.25, ..sine(1.5) });
    let baked = e.baked(4.0);
    assert!(baked.lfos.is_empty());
    for step in 0..=64 {
        let offset = f64::from(step) / 16.0;
        let (played, written) =
            (e.value_at(ExpressionKind::Pitch, offset), baked.value_at(ExpressionKind::Pitch, offset));
        assert!((played - written).abs() < 0.05, "at {offset}: kept {played}, baked {written}");
    }
}

#[test]
fn cuts_keep_the_wave_where_it_was() {
    let lfo = NoteLfo::new(ExpressionKind::Pitch, (0.5, 3.5), Lfo { attack: 0.3, decay: 0.3, ..sine(1.0) });
    let (left, right) = lfo.split(2.0);
    let (left, right) = (left.unwrap(), right.unwrap());
    for offset in [0.6, 1.0, 1.9] {
        assert!((left.swing_at(offset) - lfo.swing_at(offset)).abs() < 1e-9);
    }
    for offset in [2.1, 2.7, 3.4] {
        assert!((right.swing_at(offset - 2.0) - lfo.swing_at(offset)).abs() < 1e-9, "at {offset}");
    }
    let cropped = lfo.cropped_to(2.5).unwrap();
    assert!((cropped.swing_at(2.2) - lfo.swing_at(2.2)).abs() < 1e-9);
    assert!(lfo.cropped_to(0.5).is_none(), "nothing of it is left");
    let moved = lfo.moved_earlier(1.0).unwrap();
    assert_eq!(moved.from, 0.0);
    assert!((moved.swing_at(0.7) - lfo.swing_at(1.7)).abs() < 1e-9);
    let stretched = lfo.stretched(2.0);
    assert!((stretched.swing_at(3.4) - lfo.swing_at(1.7)).abs() < 1e-9);
}

#[test]
fn a_note_split_and_joined_keeps_its_lfo() {
    let e = kept(1.0, 3.0, sine(1.0));
    let (left, right) = e.split(2.0);
    assert_eq!(left.lfos.len(), 1);
    assert_eq!(right.lfos.len(), 1);
    let joined = left.joined(2.0, &right).unwrap();
    assert_eq!(joined.lfos.len(), 2);
    for offset in [1.3, 1.9, 2.4, 2.9] {
        let (a, b) = (e.value_at(ExpressionKind::Pitch, offset), joined.value_at(ExpressionKind::Pitch, offset));
        assert!((a - b).abs() < 1e-9, "at {offset}: {a} and {b}");
    }
}

#[test]
fn a_kept_lfo_is_checked() {
    let fine = kept(1.0, 3.0, sine(1.0));
    assert!(fine.validate(4.0).is_ok());
    assert!(fine.validate(2.0).is_err(), "past the note's end");
    let mut bad = fine.clone();
    bad.lfos[0].lfo.period = 0.0;
    assert!(bad.validate(4.0).is_err());
    let mut nan = fine.clone();
    nan.lfos[0].from = f64::NAN;
    assert!(nan.validate(4.0).is_err());
    let mut many = fine;
    many.lfos = vec![many.lfos[0]; MAX_NOTE_LFOS + 1];
    assert!(matches!(many.validate(4.0), Err(crate::expression::ExpressionError::TooManyLfos(_))));
}

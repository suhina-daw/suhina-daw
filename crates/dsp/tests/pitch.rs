// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::unwrap_used)]

use suhina_dsp::pitch::Yin;

#[test]
fn measured_period_matches_a_self_authored_sine() {
    let frame: Vec<_> = (0..1024).map(|i| (std::f32::consts::TAU * 220.0 * i as f32 / 22050.0).sin()).collect();
    let mut yin = Yin::new(22050.0, 1024, 40.0, 1000.0, 0.15).unwrap();
    let pitch = yin.estimate(&frame).unwrap();
    assert!((pitch.midi_note() - 57.0).abs() < 0.01);
    assert!(pitch.periodicity > 0.99);
    assert_eq!(yin.estimate(&[0.0; 1024]), None);
    assert_eq!(yin.estimate(&[f32::NAN; 1024]), None);
    assert_eq!(yin.estimate(&frame[..50]), None);
    assert_eq!(yin.estimate_while(&frame, || false), None);
}

#[test]
fn hostile_configuration_is_refused_before_allocation() {
    assert!(Yin::new(f32::NAN, 1024, 40.0, 1000.0, 0.15).is_none());
    assert!(Yin::new(22050.0, usize::MAX, 40.0, 1000.0, 0.15).is_none());
    assert!(Yin::new(22050.0, 1024, 0.01, 1000.0, 0.15).is_none());
    assert!(Yin::new(22050.0, 1024, 40.0, 20000.0, 0.15).is_none());
}

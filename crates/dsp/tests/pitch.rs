// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::unwrap_used)]

use suhina_dsp::pitch::Yin;

#[test]
fn extracted_estimator_preserves_sample_map_pitch_fixtures() {
    // Captured from the original private Soundmap YIN at e5b2a04b9, before extraction.
    // Integer-period triangular fixtures avoid platform-dependent sine generation.
    let mut yin = Yin::new(22050.0, 1024, 40.0, 1000.0, 0.15).unwrap();
    for (period, midi) in [(27, 79.71826), (57, 66.77355), (100, 57.04017), (341, 35.802116)] {
        let frame: Vec<_> = (0..1024)
            .map(|i| {
                let x = (i % period) as f32 / period as f32;
                1.0 - 4.0 * (x - 0.5).abs()
            })
            .collect();
        assert!((yin.estimate(&frame).unwrap().midi_note() - midi).abs() < 0.00002);
    }
}

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

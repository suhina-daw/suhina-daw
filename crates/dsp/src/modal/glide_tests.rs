// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// A slide up a minor third that settles at 0.3 s.
fn slide() -> Glide {
    Glide::from_points(&[(0.0, -300.0), (0.1, -200.0), (0.3, 0.0)])
}

#[test]
fn a_glide_interpolates_its_points_and_holds_the_note_after_it() {
    let glide = slide();
    assert!(glide.validate().is_ok() && !glide.is_empty());
    assert_eq!(glide.end(), f64::from(0.3_f32));
    assert_eq!(glide.cents_at(0.0), -300.0);
    assert!((glide.cents_at(0.05) + 250.0).abs() < 1e-3);
    assert!((glide.cents_at(0.2) + 100.0).abs() < 1e-3);
    assert!(glide.cents_at(0.3).abs() < 1e-3);
    assert_eq!(glide.cents_at(5.0), 0.0);
    assert_eq!(Glide::NONE.cents_at(0.0), 0.0);
    assert!(Glide::NONE.is_empty());
}

#[test]
fn glides_round_trip_their_fields_and_validate_bounds() {
    let glide = slide();
    assert_eq!(Glide::from_fields(glide.count, &glide.fields()), glide);
    for broken in [
        Glide::from_points(&[(0.0, -300.0)]),
        Glide::from_points(&[(0.1, -300.0), (0.3, 0.0)]),
        Glide::from_points(&[(0.0, -300.0), (0.0, 0.0)]),
        Glide::from_points(&[(0.0, -300.0), (0.3, 10.0)]),
        Glide::from_points(&[(0.0, -3000.0), (0.3, 0.0)]),
        Glide::from_points(&[(0.0, -300.0), (5.0, 0.0)]),
        Glide::from_points(&[(0.0, f32::NAN), (0.3, 0.0)]),
    ] {
        assert!(broken.validate().is_err(), "{broken:?}");
    }
    let mut stray = slide();
    stray.times[10] = 1.0;
    assert!(stray.validate().is_err());
}

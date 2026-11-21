// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn example() -> ModalProfile {
    ModalProfile::new(
        60,
        263.1,
        &[
            ProfileMode { ratio: 1.0, magnitude: 0.7, decay_seconds: 1.3 },
            ProfileMode { ratio: 2.756, magnitude: 0.2, decay_seconds: 0.42 },
        ],
    )
    .unwrap()
}

#[test]
fn canonical_codec_preserves_exact_modes_and_reference_tuning() {
    let profile = example();
    let bytes = profile.encode();
    assert_eq!(bytes.len(), PROFILE_BYTES);
    assert_eq!(ModalProfile::decode(&bytes).unwrap(), profile);
    assert_eq!(profile.reference_key(), 60);
    assert_eq!(profile.reference_hz(), 263.1);
    assert_eq!(profile.modes().len(), 2);
    assert!(bytes[72..].iter().all(|byte| *byte == 0));
}

#[test]
fn malformed_lengths_headers_revisions_padding_and_numbers_are_rejected() {
    let bytes = example().encode();
    for len in 0..PROFILE_BYTES {
        assert!(ModalProfile::decode(&bytes[..len]).is_err());
    }
    assert!(ModalProfile::decode(&[0; PROFILE_BYTES + 1]).is_err());
    for offset in [0, 8, 10, 14, 72, PROFILE_BYTES - 1] {
        let mut changed = bytes;
        changed[offset] ^= 1;
        assert!(ModalProfile::decode(&changed).is_err(), "offset {offset}");
    }
    for offset in [16, 24, 32, 40] {
        let mut changed = bytes;
        changed[offset..offset + 8].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(ModalProfile::decode(&changed).is_err());
    }
    for count in [0, 17, 255] {
        let mut changed = bytes;
        changed[13] = count;
        assert!(ModalProfile::decode(&changed).is_err());
    }
}

#[test]
fn constructor_bounds_mode_order_and_silent_profiles() {
    let mode = ProfileMode { ratio: 1.0, magnitude: 1.0, decay_seconds: 1.0 };
    for reference in [f64::NAN, f64::INFINITY, 0.0, 7.9, 20001.0] {
        assert!(ModalProfile::new(60, reference, &[mode]).is_err());
    }
    assert!(ModalProfile::new(128, 440.0, &[mode]).is_err());
    assert!(ModalProfile::new(60, 440.0, &[]).is_err());
    assert!(ModalProfile::new(60, 440.0, &[mode; 17]).is_err());
    assert!(ModalProfile::new(60, 440.0, &[mode; 2]).is_err());
    for value in [f64::NAN, f64::INFINITY, -1.0] {
        assert!(ModalProfile::new(60, 440.0, &[ProfileMode { ratio: value, ..mode }]).is_err());
        assert!(ModalProfile::new(60, 440.0, &[ProfileMode { magnitude: value, ..mode }]).is_err());
        assert!(ModalProfile::new(60, 440.0, &[ProfileMode { decay_seconds: value, ..mode }]).is_err());
    }
    assert!(ModalProfile::new(60, 440.0, &[ProfileMode { magnitude: 0.0, ..mode }]).is_err());
    let boundary = [
        ProfileMode { ratio: 1.0 / 256.0, magnitude: 0.0, decay_seconds: 0.001 },
        ProfileMode { ratio: 256.0, magnitude: 1.0, decay_seconds: 60.0 },
    ];
    for (key, hz) in [(0, 8.0), (127, 20000.0)] {
        let profile = ModalProfile::new(key, hz, &boundary).unwrap();
        assert_eq!(ModalProfile::decode(&profile.encode()).unwrap(), profile);
    }
}

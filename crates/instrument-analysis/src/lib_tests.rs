// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::unwrap_used)]
use super::*;

const VOICED: Voicing = Voicing { active_windows: 20, voiced_windows: 20, active_energy: 1.0, voiced_energy: 1.0 };

/// Windows split between a note and its upper octave are one note at the octave YIN found
/// clearly more periodic; equally periodic octaves give no proposal.
#[test]
fn an_octave_split_note_is_proposed_at_its_more_periodic_octave() {
    let mut windows: Vec<(f32, f32)> = (0..20).map(|i| if i < 12 { (48.02, 0.97) } else { (60.02, 0.87) }).collect();
    let proposal = stable(&mut windows, VOICED, &[48.0]).unwrap();
    assert_eq!(proposal.root, 48);
    // A channel whose median is the other octave agrees once the octaves are folded.
    assert_eq!(stable(&mut windows, VOICED, &[60.0]).unwrap().root, 48);
    // The upper octave more periodic: a subharmonic error below it.
    let mut windows: Vec<(f32, f32)> = (0..20).map(|i| if i < 12 { (48.02, 0.80) } else { (60.02, 0.97) }).collect();
    assert_eq!(stable(&mut windows, VOICED, &[48.0]).unwrap().root, 60);
    let mut windows: Vec<(f32, f32)> = (0..20).map(|i| if i < 12 { (48.02, 0.97) } else { (60.02, 0.95) }).collect();
    assert_eq!(stable(&mut windows, VOICED, &[48.0]), None);
}

/// Three quarters of the windows or of their energy must be pitched.
#[test]
fn voicing_counts_windows_or_energy() {
    let mut windows: Vec<(f32, f32)> = vec![(69.0, 0.99); 10];
    let tail = Voicing { active_windows: 20, voiced_windows: 10, active_energy: 1.0, voiced_energy: 0.9 };
    assert_eq!(stable(&mut windows, tail, &[69.0]).unwrap().root, 69);
    let knock = Voicing { active_windows: 20, voiced_windows: 16, active_energy: 1.0, voiced_energy: 0.3 };
    assert_eq!(stable(&mut windows, knock, &[69.0]).unwrap().root, 69);
    let neither = Voicing { active_windows: 20, voiced_windows: 10, active_energy: 1.0, voiced_energy: 0.5 };
    assert_eq!(stable(&mut windows, neither, &[69.0]), None);
}

// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The roll of a note's rolled values ([ADR 0037](../../docs/decisions/0037-rolled-note-values.md)):
//! a pure function of what the note is and where it plays, so an offline render repeats and
//! matches playback, with nothing that allocates, locks or keeps state (it runs on the audio
//! thread for a launched clip).

use crate::Note;

/// What a roll is for, so the rolls of one start differ: the velocity range (#1282), and later a
/// note's chance (#1152).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollFor {
    Velocity,
    Chance,
}

impl RollFor {
    fn salt(self) -> u64 {
        match self {
            Self::Velocity => 0x7665_6c6f_6369_7479,
            Self::Chance => 0x6368_616e_6365_0000,
        }
    }
}

/// Where a note plays, for its roll: its track, where its clip plays it (an arrangement clip's
/// start, as bits of its beat; a slot's scene) and the pass through the clip's loop, from 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RollPlace {
    pub track: u32,
    pub clip: u64,
    pub pass: u64,
}

/// One step of splitmix64.
const fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A number in `[0, 1)` for `note` starting at `place`, the same on every machine and every run.
pub fn note_roll(note: &Note, place: RollPlace, what: RollFor) -> f64 {
    let mut h = mix(what.salt());
    for part in [u64::from(place.track), place.clip, place.pass, note.time.to_bits(), u64::from(note.key)] {
        h = mix(h ^ part);
    }
    (h >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
}

/// The velocity `note` plays at when it starts at `place`, 0 to 1: its own, plus a whole number of
/// MIDI steps from 0 to its range (towards the range's sign), kept within 1 to 127. A note with a
/// range of 0 plays its own velocity exactly, unrounded.
pub fn played_velocity(note: &Note, place: RollPlace) -> f32 {
    if note.velocity_range == 0 {
        return note.velocity;
    }
    let range = i32::from(note.velocity_range);
    // Uniform over 0 ..= |range| steps.
    let steps = (note_roll(note, place, RollFor::Velocity) * f64::from(range.unsigned_abs() + 1)).floor() as i32;
    let steps = steps.min(range.abs()) * range.signum();
    let own = if note.velocity.is_finite() { (note.velocity.clamp(0.0, 1.0) * 127.0).round() as i32 } else { 0 };
    (own + steps).clamp(1, 127) as f32 / 127.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(range: i8) -> Note {
        Note { velocity_range: range, ..Note::new(1.0, 1.0, 60, 100.0 / 127.0) }
    }

    #[test]
    fn a_roll_repeats_for_the_same_note_and_place_and_differs_by_pass() {
        let place = RollPlace { track: 1, clip: 0, pass: 0 };
        let first = note_roll(&note(20), place, RollFor::Velocity);
        assert_eq!(first, note_roll(&note(20), place, RollFor::Velocity));
        assert!((0.0..1.0).contains(&first));
        let passes: Vec<f64> =
            (0..8).map(|pass| note_roll(&note(20), RollPlace { pass, ..place }, RollFor::Velocity)).collect();
        assert!(passes.windows(2).any(|w| w[0] != w[1]), "passes roll differently");
        assert_ne!(first, note_roll(&note(20), place, RollFor::Chance), "a chance rolls apart from velocity");
    }

    #[test]
    fn the_played_velocity_stays_within_the_range_and_1_to_127() {
        for pass in 0..200 {
            let place = RollPlace { track: 2, clip: 7, pass };
            let up = (played_velocity(&note(20), place) * 127.0).round() as i32;
            assert!((100..=120).contains(&up), "{up}");
            let down = (played_velocity(&note(-30), place) * 127.0).round() as i32;
            assert!((70..=100).contains(&down), "{down}");
            let capped = (played_velocity(&note(127), place) * 127.0).round() as i32;
            assert!((100..=127).contains(&capped), "{capped}");
        }
        let place = RollPlace::default();
        assert_eq!(played_velocity(&note(0), place), 100.0 / 127.0, "no range: the note's own velocity");
        let quiet = Note { velocity: 1.0 / 127.0, velocity_range: -127, ..Note::new(0.0, 1.0, 60, 0.0) };
        assert!(played_velocity(&quiet, place) >= 1.0 / 127.0 - 1e-6, "never below 1");
    }

    #[test]
    fn the_rolls_reach_both_ends_of_the_range() {
        let rolled: Vec<i32> = (0..400)
            .map(|pass| (played_velocity(&note(4), RollPlace { track: 1, clip: 0, pass }) * 127.0).round() as i32)
            .collect();
        assert!(rolled.contains(&100) && rolled.contains(&104), "{rolled:?}");
    }
}

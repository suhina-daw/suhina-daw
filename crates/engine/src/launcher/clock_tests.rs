// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_model::{Meter, TimeSignature, TimeSignatureChange};

use super::LaunchClock;
use crate::launcher::Quantization;

const FOUR: TimeSignature = TimeSignature { numerator: 4, denominator: 4 };
const THREE: TimeSignature = TimeSignature { numerator: 3, denominator: 4 };

fn four_four() -> LaunchClock {
    LaunchClock::new(FOUR, Meter::new(FOUR, &[]))
}

#[test]
fn a_launch_waits_for_the_next_bar_or_note_value() {
    let clock = four_four();
    assert_eq!(clock.launch_beat(0.0, Quantization::Bars(1)), 0.0);
    assert_eq!(clock.launch_beat(0.5, Quantization::Bars(1)), 4.0);
    assert_eq!(clock.launch_beat(5.0, Quantization::Bars(2)), 8.0);
    assert_eq!(clock.launch_beat(1.1, Quantization::Beats(0.5)), 1.5);
    assert_eq!(clock.launch_beat(1.1, Quantization::None), 1.1);
}

#[test]
fn a_scene_signature_counts_the_bars_before_it_in_the_old_one() {
    let mut clock = four_four();
    clock.leave_map(5.0);
    assert!(!clock.follows_map);
    clock.change_signature(8.0, THREE);
    assert_eq!(clock.bar(8.0), (2, 8.0, THREE));
    assert_eq!(clock.bar(11.5), (3, 11.0, THREE));
    // Bars are counted from where the signature changed.
    assert_eq!(clock.launch_beat(9.0, Quantization::Bars(1)), 11.0);
    assert!(clock.starts_bar(14) && !clock.starts_bar(12));
    clock.follow_map();
    assert_eq!(clock.bar(11.5), (2, 8.0, FOUR));
}

#[test]
fn on_a_changing_meter_bars_are_the_meters() {
    let meter = Meter::new(FOUR, &[TimeSignatureChange { bar: 1, time_signature: THREE }]);
    let clock = LaunchClock::new(FOUR, meter);
    // Bar 1 starts at beat 4 and lasts three beats.
    assert_eq!(clock.launch_beat(4.5, Quantization::Bars(1)), 7.0);
    assert_eq!(clock.bar(5.0), (1, 4.0, THREE));
}

#[test]
fn adopting_keeps_the_bars_only_on_the_same_map_and_meter() {
    let mut old = four_four();
    old.quantization = Quantization::Beats(1.0);
    old.leave_map(0.0);
    old.pending_signature = Some((4.0, THREE));
    let mut new = four_four();
    assert!(new.adopt(&old, true));
    assert!(!new.follows_map);
    assert_eq!(new.quantization, Quantization::Beats(1.0));
    assert_eq!(new.pending_signature, Some((4.0, THREE)));
    // An edited map also drops the signature still waiting for its launch beat.
    let mut edited = four_four();
    assert!(!edited.adopt(&old, false));
    assert!(edited.follows_map);
    assert_eq!(edited.quantization, Quantization::Beats(1.0));
    assert_eq!(edited.pending_signature, None);
}

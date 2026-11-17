// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn note(time: f64, key: u8) -> Note {
    Note { time, duration: 0.5, key, velocity: 0.8, release_velocity: 0.5, velocity_range: 0, channel: 0, muted: false }
}

fn set(indices: &[usize]) -> BTreeSet<usize> {
    indices.iter().copied().collect()
}

fn every(_: u8) -> bool {
    true
}

/// Two lines: key 60 at beats 0, 1, 2 (indices 0, 1, 2) and key 64 at beats 0, 1 (3, 4).
fn lines() -> Vec<Note> {
    vec![note(0.0, 60), note(1.0, 60), note(2.0, 60), note(0.0, 64), note(1.0, 64)]
}

#[test]
fn a_selected_chord_walks_along_its_keys_and_an_ended_line_stays() {
    let notes = lines();
    let chord = set(&[0, 3]);
    let once = stepped(Step::NextOnKey, &notes, &chord, &every);
    assert_eq!(once, set(&[1, 4]));
    // Key 64 has no further note: its note stays selected, key 60 goes on.
    assert_eq!(stepped(Step::NextOnKey, &notes, &once, &every), set(&[2, 4]));
    assert_eq!(stepped(Step::PreviousOnKey, &notes, &once, &every), set(&[0, 3]));
    assert_eq!(stepped(Step::PreviousOnKey, &notes, &chord, &every), chord);
}

#[test]
fn up_and_down_step_through_every_note_by_time_then_key() {
    // Given out of order: the order is by start, then key from the lowest.
    let notes = vec![note(1.0, 62), note(0.0, 67), note(0.0, 60), note(2.0, 50)];
    let mut at = set(&[2]);
    let mut seen = vec![2];
    for _ in 0..3 {
        at = stepped(Step::Next, &notes, &at, &every);
        seen.extend(at.iter().copied());
    }
    assert_eq!(seen, vec![2, 1, 0, 3]);
    // At the end the note stays; Down goes back the same way.
    assert_eq!(stepped(Step::Next, &notes, &set(&[3]), &every), set(&[3]));
    assert_eq!(stepped(Step::Previous, &notes, &set(&[0]), &every), set(&[1]));
    assert_eq!(stepped(Step::Previous, &notes, &set(&[2]), &every), set(&[2]));
}

#[test]
fn from_a_chord_up_takes_its_lowest_note_and_down_its_highest() {
    let notes = vec![note(0.0, 64), note(0.0, 60), note(0.0, 67), note(1.0, 60)];
    let chord = set(&[0, 1, 2]);
    assert_eq!(stepped(Step::Next, &notes, &chord, &every), set(&[1]));
    assert_eq!(stepped(Step::Previous, &notes, &chord, &every), set(&[2]));
}

#[test]
fn with_nothing_selected_right_and_up_take_the_first_note_left_and_down_the_last() {
    let notes = lines();
    let none = BTreeSet::new();
    for step in [Step::NextOnKey, Step::Next, Step::GrowOnKey, Step::Grow] {
        assert_eq!(stepped(step, &notes, &none, &every), set(&[0]), "{step:?}");
    }
    for step in [Step::PreviousOnKey, Step::Previous, Step::ShrinkOnKey, Step::Shrink] {
        assert_eq!(stepped(step, &notes, &none, &every), set(&[2]), "{step:?}");
    }
    assert!(stepped(Step::Next, &[], &none, &every).is_empty());
}

#[test]
fn shift_grows_and_shrinks_along_each_key() {
    let notes = lines();
    let grown = stepped(Step::GrowOnKey, &notes, &set(&[0, 3]), &every);
    assert_eq!(grown, set(&[0, 1, 3, 4]));
    // Key 64's line has ended: it takes in nothing more.
    assert_eq!(stepped(Step::GrowOnKey, &notes, &grown, &every), set(&[0, 1, 2, 3, 4]));
    assert_eq!(stepped(Step::ShrinkOnKey, &notes, &grown, &every), set(&[0, 3]));
    // A key whose one selected note is its first keeps it: nothing comes before it.
    assert_eq!(stepped(Step::ShrinkOnKey, &notes, &set(&[0, 3]), &every), set(&[0, 3]));
}

#[test]
fn shift_left_from_a_keys_last_note_grows_back_and_right_then_shrinks_it() {
    let notes = lines();
    // Key 60's last note alone: Left grows the selection back along the key.
    let (once, anchor) = stepped_from(Step::ShrinkOnKey, &notes, &set(&[2]), &every, &Anchor::default());
    assert_eq!(once, set(&[1, 2]));
    let (twice, anchor) = stepped_from(Step::ShrinkOnKey, &notes, &once, &every, &anchor);
    assert_eq!(twice, set(&[0, 1, 2]));
    // At the line's start it stays; Right shrinks it back towards the anchor.
    assert_eq!(stepped_from(Step::ShrinkOnKey, &notes, &twice, &every, &anchor).0, twice);
    let (back, anchor) = stepped_from(Step::GrowOnKey, &notes, &twice, &every, &anchor);
    assert_eq!(back, set(&[1, 2]));
    let (home, anchor) = stepped_from(Step::GrowOnKey, &notes, &back, &every, &anchor);
    assert_eq!(home, set(&[2]));
    // At the anchor, with the line ended after it, Right stays.
    assert_eq!(stepped_from(Step::GrowOnKey, &notes, &home, &every, &anchor).0, set(&[2]));
}

#[test]
fn shift_up_and_down_move_the_end_away_from_the_anchor() {
    let notes = lines();
    // The order: 0, 3, 1, 4, 2. From 1 (the third), Down grows back and Up shrinks again.
    let (down, anchor) = stepped_from(Step::Shrink, &notes, &set(&[1]), &every, &Anchor::default());
    assert_eq!(down, set(&[3, 1]));
    let (up, anchor) = stepped_from(Step::Grow, &notes, &down, &every, &anchor);
    assert_eq!(up, set(&[1]));
    assert_eq!(stepped_from(Step::Grow, &notes, &up, &every, &anchor).0, set(&[1, 4]));
}

#[test]
fn shift_grows_and_shrinks_in_the_clip_order() {
    let notes = lines();
    // The order: 0 (0, 60), 3 (0, 64), 1 (1, 60), 4 (1, 64), 2 (2, 60).
    let grown = stepped(Step::Grow, &notes, &set(&[0]), &every);
    assert_eq!(grown, set(&[0, 3]));
    assert_eq!(stepped(Step::Grow, &notes, &grown, &every), set(&[0, 1, 3]));
    assert_eq!(stepped(Step::Shrink, &notes, &grown, &every), set(&[0]));
    assert_eq!(stepped(Step::Shrink, &notes, &set(&[0]), &every), set(&[0]));
}

#[test]
fn only_notes_the_rows_show_are_stepped_to() {
    let notes = vec![note(0.0, 60), note(1.0, 61), note(2.0, 62)];
    let no_black = |key: u8| key != 61;
    assert_eq!(stepped(Step::Next, &notes, &set(&[0]), &no_black), set(&[2]));
}

#[test]
fn every_action_names_its_step() {
    for action in [
        "pianoRoll.selectNextOnKey",
        "pianoRoll.selectPreviousOnKey",
        "pianoRoll.selectNextNote",
        "pianoRoll.selectPreviousNote",
        "pianoRoll.growSelectionOnKey",
        "pianoRoll.shrinkSelectionOnKey",
        "pianoRoll.growSelection",
        "pianoRoll.shrinkSelection",
    ] {
        assert!(Step::of(action).is_some(), "{action}");
    }
    assert!(Step::of("pianoRoll.later").is_none());
}

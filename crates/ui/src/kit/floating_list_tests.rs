// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::step;

#[test]
fn step_wraps_both_ways() {
    assert_eq!(step(0, 3, 1, 0), 1);
    assert_eq!(step(2, 3, 1, 0), 0, "Down from the last row goes to the first");
    assert_eq!(step(0, 3, 0, 1), 2, "Up from the first row goes to the last");
    assert_eq!(step(1, 3, 7, 0), 2, "many presses wrap");
    assert_eq!(step(1, 3, 2, 2), 1, "Down and Up cancel out");
}

#[test]
fn step_on_an_empty_list_or_a_stale_pick() {
    assert_eq!(step(0, 0, 1, 1), 0);
    assert_eq!(step(5, 3, 0, 0), 2, "a pick past the end comes back into the list");
}

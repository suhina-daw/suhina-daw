// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The name a MIDI key is shown by, in the piano roll, the automation lanes, the detail area and
//! the device panels.

const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// The name of MIDI key `key`, with 60 as C3.
pub fn key_name(key: u8) -> String {
    let name = NAMES.get(usize::from(key % 12)).copied().unwrap_or("?");
    format!("{name}{}", i32::from(key / 12) - 2)
}

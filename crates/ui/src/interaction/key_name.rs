// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The name a MIDI key is shown by, in the piano roll, the automation lanes, the detail area and
//! the device panels.

const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const FLAT_NAMES: [&str; 12] = ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"];

thread_local! {
    /// How black keys are named on this thread (the window's): the setting, given each frame.
    static SPELLING: std::cell::Cell<crate::settings::NoteSpelling> =
        const { std::cell::Cell::new(crate::settings::NoteSpelling::Sharps) };
}

/// Names black keys as sharps or flats from now on, wherever a note or a scale's root is named:
/// the Settings' *Note names*, given each frame.
pub fn set_note_spelling(spelling: crate::settings::NoteSpelling) {
    SPELLING.with(|s| s.set(spelling));
}

/// The name of pitch class `class` (0 is C), as a sharp or a flat by the setting.
pub fn pitch_class_name(class: u8) -> &'static str {
    let names = match SPELLING.with(std::cell::Cell::get) {
        crate::settings::NoteSpelling::Sharps => &NAMES,
        crate::settings::NoteSpelling::Flats => &FLAT_NAMES,
    };
    names.get(usize::from(class % 12)).copied().unwrap_or("?")
}

/// The name of MIDI key `key`, with 60 as C3.
pub fn key_name(key: u8) -> String {
    format!("{}{}", pitch_class_name(key % 12), i32::from(key / 12) - 2)
}

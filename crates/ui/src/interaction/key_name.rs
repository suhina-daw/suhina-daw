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

/// The MIDI key a typed name or number names: `A2`, `c#3`, `Db-1`, `Bb2` or `57` (60 is C3).
/// None for anything else or a key outside 0..=127.
pub fn parse_key_name(text: &str) -> Option<u8> {
    let text = text.trim();
    if let Ok(number) = text.parse::<i32>() {
        return u8::try_from(number).ok().filter(|key| *key <= 127);
    }
    let mut chars = text.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let class = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest = chars.as_str();
    let (shift, octave) = match rest.chars().next() {
        Some('#') => (1, rest.get(1..)?),
        Some('b') => (-1, rest.get(1..)?),
        _ => (0, rest),
    };
    let octave: i32 = octave.parse().ok()?;
    let key = (octave.checked_add(2)?).checked_mul(12)?.checked_add(class + shift)?;
    u8::try_from(key).ok().filter(|key| *key <= 127)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_note_names_and_numbers_name_the_key_that_is_shown() {
        for key in 0..=127u8 {
            assert_eq!(parse_key_name(&key_name(key)), Some(key), "{}", key_name(key));
        }
        assert_eq!(parse_key_name("A2"), Some(57));
        assert_eq!(parse_key_name(" c#3 "), Some(61));
        assert_eq!(parse_key_name("Db3"), Some(61));
        assert_eq!(parse_key_name("Cb3"), Some(59));
        assert_eq!(parse_key_name("57"), Some(57));
        for bad in ["", "H2", "A", "C#", "128", "-1", "G9", "C-3", "A2x", "#2"] {
            assert_eq!(parse_key_name(bad), None, "{bad:?}");
        }
    }
}

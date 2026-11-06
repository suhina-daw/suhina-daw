// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Notes from the computer keyboard (#944): with the mode on, two rows of letters play the armed
//! tracks as a MIDI controller's keys do (ADR 0017), through the same live input, so they sound,
//! monitor and record alike. A W S E D F T G Y H U J K play C to the C above, O L P ; go on up;
//! Z and X move the octave, C and V the velocity. While the mode is on these keys play notes and
//! run no shortcut; a text field with the keyboard takes them as typing.

use egui::Key;
use suhina_engine::LiveEvent;

use super::*;

/// The keys that play, and the semitone each is above the octave's C.
const NOTE_KEYS: [(Key, u8); 17] = [
    (Key::A, 0),
    (Key::W, 1),
    (Key::S, 2),
    (Key::E, 3),
    (Key::D, 4),
    (Key::F, 5),
    (Key::T, 6),
    (Key::G, 7),
    (Key::Y, 8),
    (Key::H, 9),
    (Key::U, 10),
    (Key::J, 11),
    (Key::K, 12),
    (Key::O, 13),
    (Key::L, 14),
    (Key::P, 15),
    (Key::Semicolon, 16),
];
/// The lowest and highest octave of A (C-2 is MIDI key 0, C8 key 120).
const OCTAVES: (i32, i32) = (-2, 8);
/// How far C and V move the velocity, and its range.
const VELOCITY_STEP: f32 = 0.1;
const VELOCITY_RANGE: (f32, f32) = (0.1, 1.0);

/// The computer keyboard's state.
#[derive(Debug, Clone)]
pub(crate) struct TypingKeys {
    pub on: bool,
    /// The octave of A, as the piano roll names keys (60 is C3).
    pub octave: i32,
    pub velocity: f32,
    /// The keys held, with the MIDI key each started, so a release ends that note even after
    /// the octave moved.
    held: Vec<(Key, u8)>,
}

impl Default for TypingKeys {
    fn default() -> Self {
        Self { on: false, octave: 3, velocity: 0.8, held: Vec::new() }
    }
}

impl TypingKeys {
    /// The MIDI key `semitone` above the octave's C, if it is one.
    fn key(&self, semitone: u8) -> Option<u8> {
        let key = (self.octave + 2) * 12 + i32::from(semitone);
        u8::try_from(key).ok().filter(|k| *k <= 127)
    }

    /// The note of A, as the control bar shows it ("C3").
    pub(crate) fn label(&self) -> String {
        format!("C{}", self.octave)
    }
}

impl App {
    /// Turns the computer keyboard's notes on or off; off, its held notes end.
    pub(super) fn toggle_typing_keys(&mut self) {
        self.typing.on = !self.typing.on;
        // The plug-in hosts hand these keys back from their editor windows while it is on.
        suhina_plugins::set_keys_to_engine(self.typing.on);
        if !self.typing.on {
            self.release_typing_keys();
        }
        self.status = Status::Hint(if self.typing.on {
            format!("Computer keyboard plays notes ({}): A to K, Z X octave, C V velocity", self.typing.label())
        } else {
            "Computer keyboard: shortcuts again".into()
        });
    }

    /// Ends every note the computer keyboard holds.
    pub(super) fn release_typing_keys(&mut self) {
        let input = self.projects.live_input();
        for (_, key) in std::mem::take(&mut self.typing.held) {
            input.send(LiveEvent::NoteOff { key });
        }
    }

    /// Plays the notes of this frame's key presses and releases, taking those keys from the
    /// shortcuts, while the mode is on and no text field has the keyboard. A held key's repeats
    /// play nothing more.
    pub(super) fn typing_keys(&mut self, ui: &Ui) {
        // Keys pressed in a plug-in's editor window, which had the keyboard (#944).
        let from_editors = suhina_plugins::take_typed_keys();
        if !self.typing.on {
            return;
        }
        if ui.ctx().egui_wants_keyboard_input() {
            self.release_typing_keys();
            return;
        }
        let mut events: Vec<(Key, bool)> =
            from_editors.into_iter().filter_map(|(code, pressed)| Some((key_of(code)?, pressed))).collect();
        ui.input_mut(|i| {
            i.events.retain(|event| match event {
                egui::Event::Key { key, pressed, repeat, modifiers, .. }
                    if !modifiers.command && !modifiers.alt && is_typing_key(*key) =>
                {
                    if !repeat {
                        events.push((*key, *pressed));
                    }
                    false
                }
                _ => true,
            });
        });
        let input = self.projects.live_input();
        for (key, pressed) in events {
            match (key, pressed) {
                (Key::Z, true) => self.typing.octave = (self.typing.octave - 1).max(OCTAVES.0),
                (Key::X, true) => self.typing.octave = (self.typing.octave + 1).min(OCTAVES.1),
                (Key::C, true) => {
                    self.typing.velocity = (self.typing.velocity - VELOCITY_STEP).max(VELOCITY_RANGE.0);
                }
                (Key::V, true) => {
                    self.typing.velocity = (self.typing.velocity + VELOCITY_STEP).min(VELOCITY_RANGE.1);
                }
                (key, true) => {
                    let semitone = NOTE_KEYS.iter().find(|(k, _)| *k == key).map(|(_, s)| *s);
                    if let Some(note) = semitone.and_then(|s| self.typing.key(s))
                        && !self.typing.held.iter().any(|(k, _)| *k == key)
                    {
                        input.send(LiveEvent::NoteOn { key: note, velocity: self.typing.velocity });
                        self.typing.held.push((key, note));
                    }
                }
                (key, false) => {
                    if let Some(index) = self.typing.held.iter().position(|(k, _)| *k == key) {
                        let (_, note) = self.typing.held.swap_remove(index);
                        input.send(LiveEvent::NoteOff { key: note });
                    }
                }
            }
        }
    }
}

/// Whether `key` is one the computer keyboard takes: a note, or octave and velocity.
fn is_typing_key(key: Key) -> bool {
    NOTE_KEYS.iter().any(|(k, _)| *k == key) || matches!(key, Key::Z | Key::X | Key::C | Key::V)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_play_from_the_octaves_c() {
        let keys = TypingKeys::default();
        assert_eq!(keys.label(), "C3");
        assert_eq!(keys.key(0), Some(60), "A is C3, MIDI key 60");
        assert_eq!(keys.key(16), Some(76));
        let top = TypingKeys { octave: 8, ..TypingKeys::default() };
        assert_eq!(top.key(7), Some(127));
        assert_eq!(top.key(8), None, "past the last MIDI key");
    }
}

/// The key a Windows virtual-key code from a plug-in host names: a letter's code is its capital,
/// and ; is VK_OEM_1.
fn key_of(code: u8) -> Option<Key> {
    if code == 0xBA {
        return Some(Key::Semicolon);
    }
    let letter = char::from(code);
    letter.is_ascii_uppercase().then(|| Key::from_name(&letter.to_string())).flatten()
}

#[cfg(test)]
mod key_tests {
    use super::*;

    #[test]
    fn a_hosts_key_codes_name_the_typing_keys() {
        assert_eq!(key_of(b'A'), Some(Key::A));
        assert_eq!(key_of(b'Z'), Some(Key::Z));
        assert_eq!(key_of(0xBA), Some(Key::Semicolon));
        assert_eq!(key_of(b'1'), None);
        for code in b"AWSEDFTGYHUJKOLPZXCV" {
            assert!(key_of(*code).is_some_and(is_typing_key), "{}", char::from(*code));
        }
    }
}

// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Suhina's own track and clip colors (`docs/spec/arrangement-editing.md` › Track colors). A
//! track's or clip's color is its own data: themes never choose it, they only draw it from the
//! element's `color` attribute. The suggestions in [`PALETTE`] belong to Suhina, not to a
//! theme; [`LEGACY`] stays as it is, since it says what a saved file means.

use suhina_model::Rgb;

/// Number of suggestions in each of the palette's three rows.
pub const PALETTE_COLUMNS: usize = 12;

/// The colors the color picker offers, in its order: twelve columns, each a hue (the last
/// gray) with a full color above a pale and a deep one. Every color is a swatch of the color
/// window's Steps grid, so picking one there finds it again.
pub const PALETTE: [Rgb; 36] = [
    Rgb(0xFF, 0x99, 0x39),
    Rgb(0xFF, 0xEE, 0x42),
    Rgb(0x57, 0xC0, 0xFF),
    Rgb(0xAC, 0x91, 0xFF),
    Rgb(0x3B, 0xD0, 0x80),
    Rgb(0xFF, 0x73, 0xA7),
    Rgb(0x1E, 0xD0, 0xC2),
    Rgb(0xFF, 0x53, 0x33),
    Rgb(0x70, 0x9E, 0xFF),
    Rgb(0xA9, 0xE1, 0x6D),
    Rgb(0xF1, 0x84, 0xE6),
    Rgb(0xA2, 0xA2, 0xA2),
    Rgb(0xFF, 0xDC, 0xC2),
    Rgb(0xFE, 0xF7, 0xA9),
    Rgb(0xC3, 0xE6, 0xFF),
    Rgb(0xE6, 0xE1, 0xFF),
    Rgb(0xAD, 0xE1, 0xBD),
    Rgb(0xFF, 0xC7, 0xD7),
    Rgb(0x8C, 0xF0, 0xE5),
    Rgb(0xFF, 0xCA, 0xBE),
    Rgb(0xD6, 0xE4, 0xFF),
    Rgb(0xE5, 0xFF, 0xCF),
    Rgb(0xFF, 0xCA, 0xF7),
    Rgb(0xD0, 0xD0, 0xD0),
    Rgb(0xA3, 0x5D, 0x19),
    Rgb(0xA2, 0x97, 0x21),
    Rgb(0x2D, 0x77, 0xA3),
    Rgb(0x77, 0x5D, 0xBD),
    Rgb(0x24, 0x8C, 0x54),
    Rgb(0xA8, 0x48, 0x6C),
    Rgb(0x25, 0x8C, 0x83),
    Rgb(0xBD, 0x49, 0x33),
    Rgb(0x42, 0x69, 0xBD),
    Rgb(0x65, 0x8D, 0x38),
    Rgb(0xA6, 0x4B, 0x9E),
    Rgb(0x4E, 0x4E, 0x4E),
];

/// An exact, opaque sRGB value; malformed and incomplete text has no color.
pub fn parse_hex(text: &str) -> Option<Rgb> {
    let hex = text.strip_prefix('#').filter(|h| h.len() == 6)?;
    let part = |at: usize| hex.get(at..at + 2).and_then(|s| u8::from_str_radix(s, 16).ok());
    Some(Rgb(part(0)?, part(2)?, part(4)?))
}

/// What a track saved without a color of its own shows, by its place among the tracks.
pub use suhina_model::LEGACY_TRACK_COLORS as LEGACY;

/// The color a new MIDI track or clip gets (and every new track, with *One color*).
pub const MIDI: Rgb = suhina_model::NEW_MIDI_COLOR;

/// The color a new audio track or clip gets.
pub const AUDIO: Rgb = suhina_model::NEW_AUDIO_COLOR;

/// The color a track at `place` among the tracks shows: its own, or else [`LEGACY`]'s.
pub fn track_color(own: Option<Rgb>, place: usize) -> Rgb {
    own.unwrap_or(LEGACY[place % LEGACY.len()])
}

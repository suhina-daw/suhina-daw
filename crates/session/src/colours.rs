// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Suhina's own track and clip colours (`docs/spec/arrangement-editing.md` › Track colours). A
//! track's or clip's colour is its own data: themes never choose it, they only draw it from the
//! element's `color` attribute. The suggestions in [`PALETTE`] belong to Suhina, not to a
//! theme; [`LEGACY`] stays as it is, since it says what a saved file means.

use suhina_model::Rgb;

/// The colours the colour picker offers, in its order.
pub const PALETTE: [Rgb; 24] = [
    Rgb(0xFF, 0x9A, 0x3C),
    Rgb(0xF2, 0xD5, 0x41),
    Rgb(0x4F, 0xC3, 0xF7),
    Rgb(0xB3, 0x8B, 0xFF),
    Rgb(0x7F, 0xDB, 0x5A),
    Rgb(0xFF, 0x6F, 0xA8),
    Rgb(0x5C, 0x8D, 0xFF),
    Rgb(0x2F, 0xD9, 0xC4),
    Rgb(0xE8, 0x54, 0x54),
    Rgb(0xD4, 0x78, 0x3B),
    Rgb(0xB8, 0xA4, 0x32),
    Rgb(0x59, 0x9B, 0x51),
    Rgb(0x36, 0x9B, 0x91),
    Rgb(0x42, 0x7C, 0xAA),
    Rgb(0x7C, 0x62, 0xAA),
    Rgb(0xB5, 0x54, 0x83),
    Rgb(0xED, 0xB4, 0xA0),
    Rgb(0xD3, 0xCF, 0x96),
    Rgb(0xA8, 0xCE, 0xC2),
    Rgb(0xB8, 0xBC, 0xDE),
    Rgb(0xED, 0xED, 0xED),
    Rgb(0xB5, 0xB5, 0xB5),
    Rgb(0x7B, 0x7B, 0x7B),
    Rgb(0x43, 0x43, 0x43),
];

/// An exact, opaque sRGB value; malformed and incomplete text has no colour.
pub fn parse_hex(text: &str) -> Option<Rgb> {
    let hex = text.strip_prefix('#').filter(|h| h.len() == 6)?;
    let part = |at: usize| hex.get(at..at + 2).and_then(|s| u8::from_str_radix(s, 16).ok());
    Some(Rgb(part(0)?, part(2)?, part(4)?))
}

/// What a track saved without a colour of its own shows, by its place among the tracks.
pub use suhina_model::LEGACY_TRACK_COLOURS as LEGACY;

/// The colour a new MIDI track or clip gets (and every new track, with *One colour*).
pub const MIDI: Rgb = suhina_model::NEW_MIDI_COLOUR;

/// The colour a new audio track or clip gets.
pub const AUDIO: Rgb = suhina_model::NEW_AUDIO_COLOUR;

/// The colour a track at `place` among the tracks shows: its own, or else [`LEGACY`]'s.
pub fn track_colour(own: Option<Rgb>, place: usize) -> Rgb {
    own.unwrap_or(LEGACY[place % LEGACY.len()])
}

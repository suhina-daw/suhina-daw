// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Suhina's own track and clip colours (`docs/spec/arrangement-editing.md` › Track colours). A
//! track's or clip's colour is its own data: themes never choose it, they only draw it from the
//! element's `color` attribute (#995 › 2). The track colour palette the user can choose (#1025)
//! later takes [`PALETTE`]'s place; [`LEGACY`] stays as it is, since it says what a saved file
//! means.

use suhina_model::Rgb;

/// The colours the colour picker offers, in its order.
pub const PALETTE: [Rgb; 8] = [
    Rgb(0xFF, 0x9A, 0x3C),
    Rgb(0xF2, 0xD5, 0x41),
    Rgb(0x4F, 0xC3, 0xF7),
    Rgb(0xB3, 0x8B, 0xFF),
    Rgb(0x7F, 0xDB, 0x5A),
    Rgb(0xFF, 0x6F, 0xA8),
    Rgb(0x5C, 0x8D, 0xFF),
    Rgb(0x2F, 0xD9, 0xC4),
];

/// What a track saved without a colour of its own shows, by its place among the tracks.
pub use suhina_model::LEGACY_TRACK_COLOURS as LEGACY;

/// The colour a new MIDI track or clip gets (and every new track, with *One colour*).
pub const MIDI: Rgb = Rgb(0x4F, 0xC3, 0xF7);

/// The colour a new audio track or clip gets.
pub const AUDIO: Rgb = Rgb(0xFF, 0x9A, 0x3C);

/// The colour a track at `place` among the tracks shows: its own, or else [`LEGACY`]'s.
pub fn track_colour(own: Option<Rgb>, place: usize) -> Rgb {
    own.unwrap_or(LEGACY[place % LEGACY.len()])
}

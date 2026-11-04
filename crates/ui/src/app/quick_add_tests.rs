// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn a_track_that_plays_no_notes_is_offered_no_instrument_or_midi_effect() {
    for kind in ["instrument", "midi_effect"] {
        assert!(offered(kind, None), "{kind} on a new track");
        assert!(offered(kind, Some(true)), "{kind} on a MIDI track");
        assert!(!offered(kind, Some(false)), "{kind} on an audio track");
    }
    for kind in ["effect", "sample", "midi"] {
        assert!(offered(kind, Some(false)) && offered(kind, Some(true)) && offered(kind, None), "{kind}");
    }
    for kind in ["preset", "project", "audio_effect", "stems", ""] {
        assert!(!offered(kind, None), "{kind} is added from the browser");
    }
}

#[test]
fn the_header_names_the_bar_and_the_beat_within_it() {
    assert_eq!(place_label(0.0, 4.0), "Bar 1");
    assert_eq!(place_label(64.0, 4.0), "Bar 17");
    assert_eq!(place_label(66.0, 4.0), "Bar 17.3");
    assert_eq!(place_label(6.5, 3.0), "Bar 3", "half a beat into the bar is still its start");
    assert_eq!(place_label(7.5, 3.0), "Bar 3.2");
    assert_eq!(place_label(8.0, 0.0), "Bar 3", "a bar of no length counts as four beats");
}

#[test]
fn refusing_says_why() {
    let found = Found {
        item: Item::Device("sampler".into()),
        name: "Sampler".into(),
        kind: "instrument",
        detail: None,
        plugin: false,
        folder: None,
    };
    assert!(refused(&found, Some(false)).is_some_and(|w| w.contains("MIDI track")));
    assert_eq!(refused(&found, Some(true)), None);
    assert_eq!(kind_label(&found), "Instrument");
}

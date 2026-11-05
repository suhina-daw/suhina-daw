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
    for kind in ["project", "audio_effect", "stems", ""] {
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
        format: None,
        folder: None,
    };
    assert!(refused(&found, Some(false)).is_some_and(|w| w.contains("MIDI track")));
    assert_eq!(refused(&found, Some(true)), None);
    assert_eq!(kind_label(&found), "Instrument");
}

fn plugin(name: &str, kind: &'static str, format: PluginFormat) -> Found {
    Found {
        item: Item::Plugin(format!("{}:/{name}#{name}", format.name())),
        name: name.into(),
        kind,
        detail: None,
        plugin: true,
        format: Some(format),
        folder: None,
    }
}

#[test]
fn a_plugin_row_shows_its_format_at_the_right_and_in_its_name() {
    let diva = plugin("Diva", "instrument", PluginFormat::Vst3);
    let shown = row(&diva);
    assert_eq!(shown.tag.as_deref(), Some("VST3"));
    assert_eq!(shown.label, "Add Diva (VST3)");
    assert_eq!(shown.text, "Diva\nPlug-in instrument");
}

#[test]
fn the_format_buttons_keep_plugins_of_the_chosen_formats_while_they_show() {
    let vst2 = plugin("Diva", "instrument", PluginFormat::Vst2);
    let clap = plugin("Diva", "instrument", PluginFormat::Clap);
    let builtin = Found { format: None, plugin: false, ..plugin("EQ", "effect", PluginFormat::Vst3) };
    let mut filter = Filter { kinds: Vec::new(), formats: vec![PluginFormat::Clap] };
    assert!(filter.keeps(&vst2), "no instruments or effects chosen: the format buttons are hidden and keep all");
    filter.kinds = vec![Kind::Instruments];
    assert!(filter.shows_formats());
    assert!(!filter.keeps(&vst2) && filter.keeps(&clap), "only CLAP");
    assert!(filter.keeps(&builtin), "a built-in device has no format and stays");
    filter.formats.clear();
    assert!(filter.keeps(&vst2), "no format chosen: every format");
    filter.kinds = vec![Kind::Samples];
    assert!(!filter.shows_formats());
}

#[test]
fn the_kind_buttons_choose_what_is_searched() {
    assert_eq!(Filter::default().categories(), [Category::All], "nothing chosen: everything");
    let filter = Filter { kinds: vec![Kind::Presets, Kind::Samples], formats: Vec::new() };
    assert_eq!(filter.categories(), [Category::Samples, Category::Midi, Category::Presets], "in the buttons' order");
    let mut kinds = vec![Kind::Effects];
    toggle(&mut kinds, Kind::Effects);
    toggle(&mut kinds, Kind::Instruments);
    assert_eq!(kinds, [Kind::Instruments]);
    assert!(offered("preset", Some(false)), "a preset says itself whether the track can take it");
}

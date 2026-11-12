// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn a_track_that_plays_no_notes_is_offered_no_instrument_or_midi_effect() {
    assert!(offered("instrument", None), "an instrument makes a new track");
    for kind in ["instrument", "midi_effect"] {
        assert!(offered(kind, Some(true)), "{kind} on a MIDI track");
        assert!(!offered(kind, Some(false)), "{kind} on an audio track");
    }
    for kind in ["effect", "midi_effect"] {
        assert!(!offered(kind, None), "{kind} goes on the selected track only");
    }
    assert!(offered("effect", Some(false)) && offered("effect", Some(true)));
    for kind in ["sample", "midi"] {
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

/// The tracks' names and devices, to compare two ways of adding.
fn tracks_and_devices(app: &App) -> Vec<(String, Vec<String>)> {
    let p = app.session().map(|s| s.project().clone()).unwrap_or_default();
    p.tracks
        .iter()
        .map(|t| {
            let devices = p.channel_of_track(t.id).map(|c| c.devices.iter().map(|d| d.name.clone()).collect());
            (p.shown_track_name(t.id), devices.unwrap_or_default())
        })
        .collect()
}

fn blank_app() -> App {
    let mut app = App::new();
    app.set_session(suhina_session::Session::new(suhina_session::blank_project()), &[]);
    app
}

/// A device from the palette lands as a drag from the browser does: the same commands, so the
/// same names (a track named after its new instrument, #946) and the same devices.
#[test]
fn a_device_from_the_palette_lands_as_a_drag_from_the_browser_does() {
    let device = |id: &str, name: &str, kind: &'static str| Found {
        item: Item::Device(id.into()),
        name: name.into(),
        kind,
        detail: None,
        plugin: false,
        format: None,
        folder: None,
    };
    let dragged =
        |found: &Found, kind| BrowserItem { key: found.item.key(), name: found.name.clone(), kind, plugin: false };
    let fm = device("fm-synth", "FM Synth", "instrument");
    let sampler = device("sampler", "Sampler", "instrument");
    let reverb = device("reverb", "Reverb", "effect");
    // An effect with no track selected is not added from the palette (see below).
    let cases: [(&Found, drop::ItemKind, Option<TrackId>); 3] = [
        (&fm, drop::ItemKind::Instrument, Some(TrackId(2))),
        (&reverb, drop::ItemKind::Effect, Some(TrackId(3))),
        (&fm, drop::ItemKind::Instrument, None),
    ];
    for (found, kind, track) in cases {
        let (mut palette, mut drag) = (blank_app(), blank_app());
        // An instrument already there is replaced, and the track's name follows.
        for app in [&mut palette, &mut drag] {
            app.quick_add_choose(&sampler, Some(TrackId(2)), 0.0);
        }
        palette.quick_add_choose(found, track, 0.0);
        let target = match track {
            Some(track) => DropTarget::Track(track),
            None => DropTarget::NewTracks { beat: Some(0.0), scene: None },
        };
        drag.drop_items(&[dragged(found, kind)], target, false);
        assert_eq!(tracks_and_devices(&palette), tracks_and_devices(&drag), "{} on {track:?}", found.name);
    }
}

/// A Cab Loader from the palette and one dragged start on the file its folder lists first
/// ([docs/spec/nam-amp-and-cab.md]): the same commands, so the same state.
#[test]
fn the_palette_and_the_drag_start_the_cab_loader_on_its_folders_first_file() {
    use suhina_engine::model::*;
    let dir = std::env::temp_dir().join(format!("suhina-quick-add-nam-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let irs = dir.join("IRs");
    std::fs::create_dir_all(irs.join("Suhina")).unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(irs.join("Suhina").join("green.wav"), spec).unwrap();
    writer.write_sample(16_384_i16).unwrap();
    drop(writer);

    let found = Found {
        item: Item::Device("nam-cab-loader".into()),
        name: "Cab Loader".into(),
        kind: "effect",
        detail: None,
        plugin: false,
        format: None,
        folder: None,
    };
    let dragged =
        |found: &Found, kind| BrowserItem { key: found.item.key(), name: found.name.clone(), kind, plugin: false };
    let (mut palette, mut drag) = (blank_app(), blank_app());
    for app in [&mut palette, &mut drag] {
        app.set_nam_default_folders(dir.join("models"), irs.clone());
    }
    palette.quick_add_choose(&found, Some(TrackId(3)), 0.0);
    drag.drop_items(&[dragged(&found, drop::ItemKind::Effect)], DropTarget::Track(TrackId(3)), false);
    assert_eq!(tracks_and_devices(&palette), tracks_and_devices(&drag), "the palette matches the drag");
    let file_of = |app: &App| {
        let session = app.session().unwrap();
        let channel = session.project().tracks.iter().find(|t| t.id.0 == 3).and_then(|t| t.channel).unwrap();
        let device = session.project().channel(channel).unwrap().devices.last().unwrap();
        String::from_utf8(device.state.as_ref().map(|b| b.as_ref().to_vec()).unwrap_or_default()).unwrap_or_default()
    };
    let wanted = irs.join("Suhina").join("green.wav").display().to_string();
    assert_eq!(file_of(&palette), wanted, "the palette's cab runs the folder's first file");
    assert_eq!(file_of(&drag), wanted, "the drag's cab runs the folder's first file");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_effect_with_no_track_selected_says_to_select_one_and_adds_nothing() {
    let mut app = blank_app();
    let before = tracks_and_devices(&app);
    let reverb = Found {
        item: Item::Device("reverb".into()),
        name: "Reverb".into(),
        kind: "effect",
        detail: None,
        plugin: false,
        format: None,
        folder: None,
    };
    app.quick_add_choose(&reverb, None, 0.0);
    assert_eq!(tracks_and_devices(&app), before);
    assert!(matches!(&app.status, Status::Hint(why) if why.contains("Select a track")), "{:?}", app.status);
}

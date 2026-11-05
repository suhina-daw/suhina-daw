// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The quick-add palette (`docs/spec/browser.md` › Quick add), driven like a user would: the
//! shortcut, typing, the arrow keys, Enter, a click and Escape, and what each choice adds.
#![allow(clippy::unwrap_used, clippy::panic)] // test helpers outside #[test] functions

use std::path::{Path, PathBuf};

use egui::{Key, Modifiers, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::model::{Project, TrackId};
use suhina_library::Library;
use suhina_session::{Session, blank_project};
use suhina_ui::kit::widgets::Action;
use suhina_ui::{App, Status, kit::theme};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-quick-add-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn wav(path: &Path, seconds: f32) {
    let spec =
        hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for _ in 0..(seconds * 8000.0) as usize {
        writer.write_sample(8000i16).unwrap();
    }
    writer.finalize().unwrap();
}

/// The window in the Arrangement with a blank project (two MIDI tracks, two audio tracks) and a
/// browser folder holding one sample, `Kick Punchy.wav`.
fn window(name: &str) -> (Harness<'static, App>, PathBuf) {
    let (settings, folder) = (temp(&format!("{name}-settings")), temp(name));
    wav(&folder.join("Kick Punchy.wav"), 0.25);
    let mut app = App::new();
    let mut library = Library::new(Some(settings.clone()));
    library.add_folder(&folder).unwrap();
    app.set_file_library(library);
    app.set_session(Session::new(blank_project()), &[]);
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .with_size((1280.0, 800.0))
        .with_step_dt(0.05)
        .with_max_steps(20)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.state_mut().do_action("view.arrangement", &mut Vec::new());
    harness.run();
    (harness, settings)
}

/// The same window with a plug-in catalog as a scan leaves one: a synth as VST2 and as VST3, an
/// effect as CLAP.
fn window_with_plugins(name: &str) -> (Harness<'static, App>, PathBuf) {
    let (settings, folder) = (temp(&format!("{name}-settings")), temp(name));
    wav(&folder.join("Kick Punchy.wav"), 0.25);
    // A VST2 plug-in is a `.vst` bundle on macOS, a `.dll` on Windows and a `.so` elsewhere.
    let vst2 = if cfg!(target_os = "macos") {
        "/VST2/Nova Synth.vst"
    } else if cfg!(windows) {
        "/VST2/Nova Synth.dll"
    } else {
        "/VST2/Nova Synth.so"
    };
    let catalog = serde_json::json!({
        "version": 2,
        "entries": [
            { "path": "/VST3/Nova Synth.vst3", "modified": 1_760_000_000, "size": 10, "plugins": [
                { "id": "ABCDEF019182FAEB49746D78426E6401", "name": "Nova Synth", "vendor": "Nova", "version": "1.0", "features": ["Instrument", "Synth"], "replaces": [] }] },
            { "path": vst2, "modified": 1_760_000_000, "size": 10, "plugins": [
                { "id": "NvSy", "name": "Nova Synth", "vendor": "Nova", "version": "1.0", "features": ["instrument"], "replaces": [] }] },
            { "path": "/CLAP/Nova Verb.clap", "modified": 1_760_100_000, "size": 10, "plugins": [
                { "id": "com.nova.verb", "name": "Nova Verb", "vendor": "Nova", "version": "1.0", "features": ["audio-effect", "reverb"], "replaces": [] }] },
        ],
    });
    std::fs::create_dir_all(settings.join("plugins")).unwrap();
    std::fs::write(settings.join("plugins").join("plugin-catalog.json"), catalog.to_string()).unwrap();
    let mut app = App::with_library(suhina_plugins::library::Library::new(
        Some(settings.join("plugins")),
        PathBuf::from("no-host"),
    ));
    let mut library = Library::new(Some(settings.clone()));
    library.add_folder(&folder).unwrap();
    app.set_file_library(library);
    app.set_session(Session::new(blank_project()), &[]);
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .with_size((1280.0, 800.0))
        .with_step_dt(0.05)
        .with_max_steps(20)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.state_mut().do_action("view.arrangement", &mut Vec::new());
    harness.run();
    (harness, settings)
}

fn project(harness: &Harness<'static, App>) -> Project {
    harness.state().session().unwrap().project().clone()
}

/// The names of a track's devices.
fn devices(harness: &Harness<'static, App>, track: TrackId) -> Vec<String> {
    let p = project(harness);
    let Some(t) = p.track(track) else { return Vec::new() };
    p.channels
        .iter()
        .find(|c| Some(c.id) == t.channel)
        .map_or(Vec::new(), |c| c.devices.iter().map(|d| d.name.clone()).collect())
}

fn open(harness: &mut Harness<'static, App>) {
    harness.key_press_modifiers(Modifiers::COMMAND, Key::P);
    harness.run();
}

/// Types `text` over what the field says (it keeps a search the palette was closed with).
fn type_text(harness: &mut Harness<'static, App>, text: &str) {
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    harness.input_mut().events.push(egui::Event::Text(text.into()));
    harness.run();
}

fn press(harness: &mut Harness<'static, App>, key: Key) {
    harness.key_press(key);
    harness.run();
}

fn select(harness: &mut Harness<'static, App>, track: &str) {
    harness.get_by_label(&format!("Track {track}")).click();
    harness.run();
}

#[test]
fn the_shortcut_opens_the_palette_naming_the_target_and_escape_closes_it() {
    let (mut harness, settings) = window("open");
    open(&mut harness);
    assert!(harness.state().quick_add_open());
    assert!(
        harness.query_by_label("New track · Bar 1 · select a track for effects").is_some(),
        "no track selected: a new track"
    );
    let before = project(&harness);
    type_text(&mut harness, "fm synth");
    assert!(harness.query_by_label("Add FM Synth").is_some(), "typing finds the instrument");
    press(&mut harness, Key::Escape);
    assert!(!harness.state().quick_add_open(), "Escape closes it");
    assert_eq!(project(&harness), before, "and changes nothing");

    select(&mut harness, "3 Audio");
    open(&mut harness);
    assert!(harness.query_by_label("Track: 3 Audio · Bar 1").is_some(), "the selected track is the target");
    // A click outside closes it too.
    let at = Pos2::new(5.0, 790.0);
    harness.hover_at(at);
    harness.run();
    harness.drag_at(at);
    harness.step();
    harness.drop_at(at);
    harness.run();
    assert!(!harness.state().quick_add_open());
    let _ = std::fs::remove_dir_all(&settings);
}

#[test]
fn typing_filters_and_enter_adds_the_lit_device_to_the_selected_track() {
    let (mut harness, settings) = window("device");
    select(&mut harness, "1 MIDI");
    open(&mut harness);
    type_text(&mut harness, "fm synth");
    assert!(harness.query_by_label("Add FM Synth").is_some());
    assert!(harness.query_by_label("Add Overdrive").is_none(), "what does not match is left out");
    press(&mut harness, Key::Enter);
    assert!(!harness.state().quick_add_open(), "Enter adds and closes");
    assert_eq!(devices(&harness, TrackId(1)), ["FM Synth"]);
    // One undo step takes it away again.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(devices(&harness, TrackId(1)).is_empty());
    let _ = std::fs::remove_dir_all(&settings);
}

#[test]
fn a_click_adds_and_down_moves_the_lit_row() {
    let (mut harness, settings) = window("click");
    select(&mut harness, "3 Audio");
    open(&mut harness);
    type_text(&mut harness, "e");
    // Down moves the lit row; the list still offers rows to click.
    press(&mut harness, Key::ArrowDown);
    harness.get_by_label("Add Reverb").click();
    harness.run();
    assert!(!harness.state().quick_add_open());
    assert_eq!(devices(&harness, TrackId(3)), ["Reverb"]);
    let _ = std::fs::remove_dir_all(&settings);
}

#[test]
fn a_device_with_no_track_selected_goes_on_a_new_track_that_becomes_selected() {
    let (mut harness, settings) = window("new-track");
    let before = project(&harness).tracks.len();
    open(&mut harness);
    type_text(&mut harness, "fm synth");
    press(&mut harness, Key::Enter);
    let p = project(&harness);
    assert_eq!(p.tracks.len(), before + 1, "a new track");
    let new = p.tracks.iter().find(|t| t.name == "FM Synth").expect("named after the device");
    assert!(new.content.notes, "an instrument makes a MIDI track");
    assert_eq!(harness.state().selected_tracks(), [new.id], "the new track is selected");
    // The next add goes there.
    open(&mut harness);
    type_text(&mut harness, "overdrive");
    press(&mut harness, Key::Enter);
    assert_eq!(devices(&harness, new.id), ["FM Synth", "Overdrive"]);
    let _ = std::fs::remove_dir_all(&settings);
}

#[test]
fn an_audio_track_is_offered_no_instrument_or_midi_effect() {
    let (mut harness, settings) = window("audio-filter");
    select(&mut harness, "3 Audio");
    open(&mut harness);
    type_text(&mut harness, "fm synth");
    assert!(harness.query_by_label("Add FM Synth").is_none(), "no instrument on an audio track");
    press(&mut harness, Key::Escape);
    open(&mut harness);
    type_text(&mut harness, "pitch");
    assert!(harness.query_by_label("Add Pitch").is_none(), "no MIDI effect either");
    press(&mut harness, Key::Escape);
    // On a MIDI track both are offered.
    select(&mut harness, "1 MIDI");
    open(&mut harness);
    type_text(&mut harness, "pitch");
    assert!(harness.query_by_label("Add Pitch").is_some());
    let _ = std::fs::remove_dir_all(&settings);
}

/// Opens the palette with the pointer over `track`'s lane, right of the headers, and adds the
/// sample; the beat it was opened for.
fn add_sample_over(harness: &mut Harness<'static, App>, track: &str) -> f64 {
    let lane = harness.get_by_label(&format!("Track {track}")).rect().center().y;
    harness.hover_at(Pos2::new(900.0, lane));
    harness.run();
    open(harness);
    let (_, beat) = harness.state().quick_add_target().unwrap();
    assert!(beat > 0.0 && beat.fract() == 0.0, "the beat under the pointer, on the grid: {beat}");
    type_text(harness, "kick punchy");
    assert!(harness.query_by_label("Add Kick Punchy.wav").is_some());
    press(harness, Key::Enter);
    // The sample is read on another thread; wait for that, then the clip is placed.
    harness.state_mut().finish_drop();
    harness.run();
    assert!(!matches!(harness.state().status(), Status::Error(_)), "{:?}", harness.state().status());
    beat
}

/// The clips on `track`'s arrangement lane: their starts.
fn clip_starts(harness: &Harness<'static, App>, track: TrackId) -> Vec<f64> {
    let p = project(harness);
    p.arrangement
        .lanes
        .iter()
        .filter(|l| l.track == Some(track))
        .flat_map(|l| l.clips.iter().map(|c| c.start))
        .collect()
}

#[test]
fn a_sample_goes_on_the_selected_audio_track_at_the_beat_under_the_pointer() {
    let (mut harness, settings) = window("sample");
    let tracks = project(&harness).tracks.len();
    select(&mut harness, "3 Audio");
    let beat = add_sample_over(&mut harness, "3 Audio");
    assert_eq!(project(&harness).tracks.len(), tracks, "no new track: as a drop on the lane");
    assert_eq!(clip_starts(&harness, TrackId(3)), [beat], "on the selected track, at the palette's place");
    let _ = std::fs::remove_dir_all(&settings);
}

#[test]
fn a_sample_a_track_cannot_hold_goes_on_a_new_track_right_after_it() {
    let (mut harness, settings) = window("sample-midi");
    // A MIDI track with a clip holds no audio. (An empty one would become an audio track for the
    // sample, as a drop on its lane makes it, #945.)
    harness.state_mut().run(&Action::new("clip.create", serde_json::json!({ "track": 1, "start": 0.0 })));
    harness.run();
    select(&mut harness, "1 MIDI");
    let beat = add_sample_over(&mut harness, "1 MIDI");
    let p = project(&harness);
    let names: Vec<String> = p.tracks.iter().map(|t| p.shown_track_name(t.id)).collect();
    assert_eq!(names[..2], ["1 MIDI".to_string(), "Kick Punchy".to_string()], "right after the selected track");
    assert_eq!(clip_starts(&harness, p.tracks[1].id), [beat], "at the palette's place");
    let _ = std::fs::remove_dir_all(&settings);
}

#[test]
fn the_kind_buttons_under_the_list_narrow_it_and_stay_for_the_next_palette() {
    let (mut harness, settings) = window("kinds");
    open(&mut harness);
    type_text(&mut harness, "k");
    assert!(harness.query_by_label("Add Kick Punchy.wav").is_some());
    harness.get_by_label("Quick add: Effects").click();
    harness.run();
    assert!(harness.query_by_label("Add Kick Punchy.wav").is_none(), "Effects alone: no samples");
    harness.get_by_label("Quick add: Samples").click();
    harness.run();
    assert!(harness.query_by_label("Add Kick Punchy.wav").is_some(), "Effects and Samples: both");
    assert!(harness.state().quick_add_open(), "a click on a button keeps the palette open");
    press(&mut harness, Key::Escape);
    // The next palette starts with the same buttons on.
    open(&mut harness);
    harness.get_by_label("Quick add: Effects").click();
    harness.run();
    type_text(&mut harness, "overdrive");
    assert!(harness.query_by_label("Add Overdrive").is_none(), "Samples alone now");
    let _ = std::fs::remove_dir_all(&settings);
}

#[test]
fn a_plugin_shows_its_format_and_the_format_buttons_choose_one() {
    let (mut harness, settings) = window_with_plugins("formats");
    // A MIDI track selected, so effects are listed as well as instruments.
    select(&mut harness, "1 MIDI");
    open(&mut harness);
    type_text(&mut harness, "nova");
    // The synth is there twice, told apart by its format.
    assert!(harness.query_by_label("Add Nova Synth (VST3)").is_some());
    assert!(harness.query_by_label("Add Nova Synth (VST2)").is_some());
    assert!(harness.query_by_label("Add Nova Verb (CLAP)").is_some());
    assert!(harness.query_by_label("Quick add: VST3").is_none(), "no format buttons until instruments or effects");
    harness.get_by_label("Quick add: Instruments").click();
    harness.run();
    assert!(harness.query_by_label("Add Nova Verb (CLAP)").is_none(), "instruments only");
    harness.get_by_label("Quick add: VST3").click();
    harness.run();
    assert!(harness.query_by_label("Add Nova Synth (VST3)").is_some());
    assert!(harness.query_by_label("Add Nova Synth (VST2)").is_none(), "VST3 only");
    press(&mut harness, Key::Enter);
    let p = project(&harness);
    let on_track = p.channel_of_track(TrackId(1)).and_then(|c| c.devices.first()).map(|d| d.kind.clone());
    assert!(matches!(on_track, Some(suhina_engine::model::DeviceKind::Vst3 { .. })), "{on_track:?}");
    assert_eq!(p.shown_track_name(TrackId(1)), "1 Nova Synth", "the track takes the instrument's name");

    let _ = std::fs::remove_dir_all(&settings);
}

#[test]
fn effects_wait_for_a_selected_track_and_the_search_is_kept_until_something_is_added() {
    let (mut harness, settings) = window("kept");
    open(&mut harness);
    type_text(&mut harness, "reverb");
    assert!(harness.query_by_label("Add Reverb").is_none(), "no track selected: no effects");
    // Closed to select a track: the search is still there when it opens again.
    press(&mut harness, Key::Escape);
    select(&mut harness, "3 Audio");
    open(&mut harness);
    assert_eq!(harness.state().quick_add_text(), "reverb");
    assert!(harness.query_by_label("Add Reverb").is_some(), "with a track selected the effect is listed");
    press(&mut harness, Key::Enter);
    assert_eq!(devices(&harness, TrackId(3)), ["Reverb"]);
    assert_eq!(project(&harness).tracks[2].name, "# Audio", "an effect does not name the track");
    // Adding cleared it.
    open(&mut harness);
    assert_eq!(harness.state().quick_add_text(), "");
    let _ = std::fs::remove_dir_all(&settings);
}

#[test]
fn a_plugin_instrument_with_no_track_selected_makes_a_track_named_after_it() {
    let (mut harness, settings) = window_with_plugins("plugin-new-track");
    open(&mut harness);
    type_text(&mut harness, "nova synth");
    press(&mut harness, Key::Enter);
    let p = project(&harness);
    let plugin_on = |t: &suhina_engine::model::Track| {
        p.channel_of_track(t.id).and_then(|c| c.devices.first()).map(|d| d.kind.clone())
    };
    let new = p.tracks.iter().find(|t| plugin_on(t).is_some()).expect("a new track with the synth");
    assert!(new.content.notes, "an instrument makes a MIDI track");
    assert_eq!(new.name, "Nova Synth", "named after the plug-in, as a drag of it onto the empty area names it");
    let _ = std::fs::remove_dir_all(&settings);
}

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

fn type_text(harness: &mut Harness<'static, App>, text: &str) {
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
    assert!(harness.query_by_label("New track · Bar 1").is_some(), "no track selected: a new track");
    let before = project(&harness);
    type_text(&mut harness, "overdrive");
    assert!(harness.query_by_label("Add Overdrive").is_some(), "typing finds the effect");
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

#[test]
fn a_sample_goes_on_a_new_track_after_the_selected_one_at_the_beat_under_the_pointer() {
    let (mut harness, settings) = window("sample");
    select(&mut harness, "1 MIDI");
    // The pointer over the timeline, on the first track's lane, when the shortcut is pressed.
    let lane = harness.get_by_label("Track 1 MIDI").rect().center().y;
    harness.hover_at(Pos2::new(900.0, lane));
    harness.run();
    open(&mut harness);
    let (track, beat) = harness.state().quick_add_target().unwrap();
    assert_eq!(track, Some(TrackId(1)));
    assert!(beat > 0.0 && beat.fract() == 0.0, "the beat under the pointer, on the grid: {beat}");
    type_text(&mut harness, "kick punchy");
    assert!(harness.query_by_label("Add Kick Punchy.wav").is_some());
    press(&mut harness, Key::Enter);
    // The sample is read on another thread; wait for that, then the clip is placed.
    harness.state_mut().finish_drop();
    harness.run();
    assert!(!matches!(harness.state().status(), Status::Error(_)), "{:?}", harness.state().status());
    let p = project(&harness);
    let names: Vec<String> = p.tracks.iter().map(|t| p.shown_track_name(t.id)).collect();
    assert_eq!(names[..2], ["1 MIDI".to_string(), "Kick Punchy".to_string()], "right after the selected track");
    let added = p.tracks[1].id;
    let lane = p.arrangement.lanes.iter().find(|l| l.track == Some(added)).unwrap();
    assert_eq!(lane.clips.len(), 1);
    assert_eq!(lane.clips[0].start, beat, "at the palette's place");
    let _ = std::fs::remove_dir_all(&settings);
}

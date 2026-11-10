// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The browser panel in the window (`docs/spec/browser.md`), driven like a user would: search,
//! filters, places, the keyboard, and dragging samples, MIDI files and devices into the
//! project, which runs the same commands agents use.
#![allow(clippy::unwrap_used, clippy::panic)] // test helpers outside #[test] functions

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Key, Modifiers, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;
use suhina_engine::model::ChannelId;
use suhina_library::{Library, SectionKind};
use suhina_session::Session;
use suhina_ui::browser::Place;
use suhina_ui::demo::demo_project;
use suhina_ui::{App, Status, kit::theme};

const SIZE: (f32, f32) = (1280.0, 800.0);

/// A file dropped on the window from the system in a test.
#[derive(Debug)]
struct TestFile(PathBuf);

impl egui::DroppedFile for TestFile {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        Ok(Vec::new())
    }
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-browser-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn wav(path: &Path, seconds: f32) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let spec =
        hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for _ in 0..(seconds * 8000.0) as usize {
        writer.write_sample(8000i16).unwrap();
    }
    writer.finalize().unwrap();
}

/// A MIDI file with one note a beat long.
fn midi(path: &Path) {
    let mut file = b"MThd\0\0\0\x06\0\0\0\x01\0\x60".to_vec();
    let track = [0x00, 0x90, 60, 100, 0x60, 0x80, 60, 0, 0x00, 0xFF, 0x2F, 0x00];
    file.extend_from_slice(b"MTrk");
    file.extend_from_slice(&(track.len() as u32).to_be_bytes());
    file.extend_from_slice(&track);
    std::fs::write(path, file).unwrap();
}

/// A sample pack: drums, a loop, a MIDI file and a project.
fn pack(root: &Path) {
    wav(&root.join("Drums").join("Kick Hard.wav"), 0.25);
    wav(&root.join("Drums").join("Snare.wav"), 0.25);
    wav(&root.join("Loop 120bpm.wav"), 2.0);
    midi(&root.join("Groove.mid"));
    std::fs::write(root.join("Song.suhina"), b"{}").unwrap();
}

/// The window with the demo project and a browser whose library is kept in `settings` and
/// holds `folder`.
fn window(settings: &Path, folder: &Path) -> Harness<'static, App> {
    window_at(settings, folder, true)
}

/// A window on a library that may already hold `folder` (a browser.json left by a first
/// window), so the tests can reopen one on the same settings.
fn window_reopened(settings: &Path, folder: &Path) -> Harness<'static, App> {
    window_at(settings, folder, false)
}

fn window_at(settings: &Path, folder: &Path, adding: bool) -> Harness<'static, App> {
    window_at_size(settings, folder, SIZE, adding)
}

fn window_at_size(settings: &Path, folder: &Path, size: (f32, f32), adding: bool) -> Harness<'static, App> {
    let mut app = App::new();
    let mut library = Library::new(Some(settings.to_path_buf()));
    if adding {
        library.add_folder(folder).unwrap();
    }
    app.set_file_library(library);
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .allow_missing_glyphs()
        .with_size(size)
        .with_step_dt(0.05)
        .with_max_steps(20)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness
}

/// A plug-in catalog as a scan leaves one, in the settings' plug-in library: two effects of
/// one vendor and an instrument of another.
fn catalog(settings: &Path) {
    let text = serde_json::json!({
        "version": 2,
        "entries": [
            { "path": "/VST3/Alpha Verb.vst3", "modified": 1_760_000_000, "size": 10, "plugins": [
                { "id": "ABCDEF019182FAEB49746D78426E6477", "name": "Alpha Verb", "vendor": "Alpha", "version": "1.0", "features": ["Fx", "Reverb"], "replaces": [] }] },
            { "path": "/VST3/Alpha Delay.vst3", "modified": 1_760_100_000, "size": 10, "plugins": [
                { "id": "ABCDEF019182FAEB49746D78426E6476", "name": "Alpha Delay", "vendor": "Alpha", "version": "1.0", "features": ["Fx", "Delay"], "replaces": [] }] },
            { "path": "/CLAP/Zeta Synth.clap", "modified": 1_760_200_000, "size": 10, "plugins": [
                { "id": "ABCDEF019182FAEB49746D78426E6478", "name": "Zeta Synth", "vendor": "Zeta", "version": "2.0", "features": ["instrument", "synthesizer"], "replaces": [] }] },
        ],
    });
    std::fs::create_dir_all(settings.join("plugins")).unwrap();
    std::fs::write(settings.join("plugins").join("plugin-catalog.json"), text.to_string()).unwrap();
}

/// The window with the demo project, a browser whose library is in `settings`, and the plug-in
/// catalog [`catalog`] put in the same settings.
fn window_with_plugins(settings: &Path, folder: &Path) -> Harness<'static, App> {
    catalog(settings);
    let mut app = App::with_library(suhina_plugins::library::Library::new(
        Some(settings.join("plugins")),
        PathBuf::from("no-host"),
    ));
    let mut library = Library::new(Some(settings.to_path_buf()));
    library.add_folder(folder).unwrap();
    app.set_file_library(library);
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .allow_missing_glyphs()
        .with_size(SIZE)
        .with_step_dt(0.05)
        .with_max_steps(20)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness
}

fn row(name: &str) -> String {
    format!("Browser {name}")
}

/// The screen height of a row's centre, to compare two rows' order.
fn row_y(harness: &Harness<'static, App>, name: &str) -> f32 {
    harness.get_by_label(&row(name)).rect().center().y
}

/// Drags from `from` to `to` and lets go, a frame at a time.
fn drag(harness: &mut Harness<'static, App>, from: Pos2, to: Pos2) {
    // A drag repaints every frame, so the window is stepped rather than run to rest.
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    for share in [0.1, 0.3, 0.6, 1.0] {
        harness.hover_at(from + (to - from) * share);
        harness.step();
    }
    harness.drop_at(to);
    harness.step();
    harness.step();
}

/// Presses and lets go twice, a frame apart: a double-click.
fn double_click(harness: &mut Harness<'static, App>, label: &str) {
    let at = harness.get_by_label(label).rect().center();
    harness.hover_at(at);
    harness.run();
    for _ in 0..2 {
        harness.hover_at(at);
        harness.drag_at(at);
        harness.drop_at(at);
        harness.step();
    }
    harness.run();
}

/// Presses and lets go once, at `pos`, a frame apart, without waiting: two in a row are a
/// double-click.
fn quick_click_at(harness: &mut Harness<'static, App>, pos: Pos2) {
    harness.hover_at(pos);
    harness.step();
    harness.drag_at(pos);
    harness.step();
    harness.drop_at(pos);
    harness.step();
}

/// Presses at `at` and lets go a frame later, like a mouse click.
fn click_at(harness: &mut Harness<'static, App>, at: Pos2) {
    harness.hover_at(at);
    harness.run();
    harness.drag_at(at);
    harness.step();
    harness.drop_at(at);
    harness.step();
    harness.run();
}

/// Where the chevron of a top-level row is drawn.
fn chevron_of(harness: &Harness<'static, App>, label: &str) -> Pos2 {
    let rect = harness.get_by_label(label).rect();
    Pos2::new(rect.left() + 9.0, rect.center().y)
}

/// Where the name of a top-level row is drawn.
fn title_of(harness: &Harness<'static, App>, label: &str) -> Pos2 {
    let rect = harness.get_by_label(label).rect();
    Pos2::new(rect.left() + 40.0, rect.center().y)
}

/// Drags `path` from the system over the window to `at` and drops it there, as the windowing
/// layer reports it: hovered files for a frame, then the dropped file.
fn system_drop(harness: &mut Harness<'static, App>, path: &Path, at: Pos2) {
    harness.hover_at(at);
    harness.run();
    harness.input_mut().hovered_files.push(egui::HoveredFile { path: Some(path.into()), ..Default::default() });
    harness.step();
    // The windowing layer stops reporting them as hovered when they are dropped.
    harness.input_mut().hovered_files.clear();
    harness.input_mut().dropped_files.push(std::sync::Arc::new(TestFile(path.into())));
    harness.step();
}

/// Steps the window until `done` holds (a drop decodes its samples on another thread); only a
/// hang gets to the end of its minute.
fn until(harness: &mut Harness<'static, App>, done: impl Fn(&App) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(60);
    while Instant::now() < end {
        harness.step();
        if done(harness.state()) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn the_tree_lists_devices_and_folders_and_opens_them() {
    let (settings, folder) = (temp("tree-settings"), temp("tree"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    for heading in ["Instruments", "Effects", "MIDI Effects", "Plug-ins"] {
        assert!(harness.query_by_label(&row(heading)).is_some(), "{heading}");
    }
    harness.get_by_label(&row("Instruments")).click();
    harness.run();
    assert!(harness.query_by_label(&row("FM Synth")).is_some(), "a heading opens with a click");
    let name = folder.file_name().unwrap().to_string_lossy().to_string();
    double_click(&mut harness, &row(&name));
    harness.run();
    assert!(harness.query_by_label(&row("Groove.mid")).is_some(), "a folder opens with a double-click");
    double_click(&mut harness, &row("Drums"));
    harness.run();
    assert!(harness.query_by_label(&row("Kick Hard.wav")).is_some());
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_click_on_a_headings_chevron_opens_it_once_and_a_second_closes_it() {
    let (settings, folder) = (temp("heading-chevron-settings"), temp("heading-chevron"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let at = chevron_of(&harness, &row("Effects"));
    click_at(&mut harness, at);
    assert!(harness.query_by_label(&row("Overdrive")).is_some(), "the section stays open");
    click_at(&mut harness, at);
    assert!(harness.query_by_label(&row("Overdrive")).is_none(), "a second click closes it");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_click_on_a_headings_title_opens_it_once_and_a_second_closes_it() {
    let (settings, folder) = (temp("heading-title-settings"), temp("heading-title"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let at = title_of(&harness, &row("Effects"));
    click_at(&mut harness, at);
    assert!(harness.query_by_label(&row("Overdrive")).is_some(), "the section stays open");
    click_at(&mut harness, at);
    assert!(harness.query_by_label(&row("Overdrive")).is_none(), "a second click closes it");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_click_on_a_folders_chevron_or_its_title_opens_it_and_a_second_closes_it() {
    let (settings, folder) = (temp("folder-chevron-settings"), temp("folder-chevron"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let name = row(&folder.file_name().unwrap().to_string_lossy());
    let title = title_of(&harness, &name);
    click_at(&mut harness, title);
    assert!(harness.query_by_label(&row("Groove.mid")).is_some(), "a click on the title opens it (#869)");
    assert_eq!(harness.state().browser.selected().len(), 1, "and selects it");
    click_at(&mut harness, title);
    assert!(harness.query_by_label(&row("Groove.mid")).is_none(), "a second click on the title closes it");
    let at = chevron_of(&harness, &name);
    click_at(&mut harness, at);
    assert!(harness.query_by_label(&row("Groove.mid")).is_some(), "the folder stays open");
    click_at(&mut harness, at);
    assert!(harness.query_by_label(&row("Groove.mid")).is_none(), "a second click closes it");
    // A double-click on the title leaves it open: its first click opened it.
    double_click(&mut harness, &name);
    assert!(harness.query_by_label(&row("Groove.mid")).is_some(), "a double-click leaves it open");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// A library folder inside another library folder is listed twice, at the top and inside the
/// other one, and each opens and closes on its own (#869).
#[test]
fn a_library_folder_inside_another_opens_on_its_own_in_both_places() {
    let (settings, outer) = (temp("nested-settings"), temp("nested"));
    let inner = outer.join("Inner");
    wav(&outer.join("Outer Hit.wav"), 0.01);
    wav(&inner.join("Inner Hit.wav"), 0.01);
    let mut harness = window(&settings, &outer);
    harness.state_mut().projects_mut().file_library_mut().add_folder(&inner).unwrap();
    harness.state_mut().browser.relist();
    harness.run();
    let outer_name = row(&outer.file_name().unwrap().to_string_lossy());
    let inner_name = row("Inner");
    assert_eq!(harness.get_all_by_label(&inner_name).count(), 1, "the inner folder at the top only");
    // The inner folder at the top opens without opening the outer one.
    let at = title_of(&harness, &inner_name);
    click_at(&mut harness, at);
    assert_eq!(harness.get_all_by_label(&row("Inner Hit.wav")).count(), 1, "the inner folder opens");
    assert!(harness.query_by_label(&row("Outer Hit.wav")).is_none(), "the outer one stays closed");
    // The outer one opens: the inner folder in it is listed closed, its own row.
    let at = title_of(&harness, &outer_name);
    click_at(&mut harness, at);
    assert!(harness.query_by_label(&row("Outer Hit.wav")).is_some(), "the outer folder opens");
    let copies: Vec<_> = harness.get_all_by_label(&inner_name).map(|n| n.rect()).collect();
    assert_eq!(copies.len(), 2, "the inner folder at the top and inside the outer one");
    assert_eq!(harness.get_all_by_label(&row("Inner Hit.wav")).count(), 1, "the copy inside is closed");
    // The copy inside the outer folder opens on its own, and closes on its own. It is the copy
    // right under the outer folder: the one at the top is above the outer folder or after all
    // that is in it.
    let outer_top = harness.get_by_label(&outer_name).rect().top();
    let nested = copies
        .iter()
        .copied()
        .filter(|rect| rect.top() > outer_top)
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap();
    let nested_title = egui::pos2(nested.left() + 60.0, nested.center().y);
    click_at(&mut harness, nested_title);
    assert_eq!(harness.get_all_by_label(&row("Inner Hit.wav")).count(), 2, "both copies are open now");
    click_at(&mut harness, nested_title);
    assert_eq!(harness.get_all_by_label(&row("Inner Hit.wav")).count(), 1, "the copy inside closes alone");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&outer);
}

/// The list's scroll bar is a scroll area's, so it is dragged like a device panel's (#867).
#[test]
fn the_lists_scroll_bar_can_be_dragged() {
    let (settings, folder) = (temp("scroll-bar-settings"), temp("scroll-bar"));
    for i in 0..80 {
        wav(&folder.join(format!("Hit {i:02}.wav")), 0.01);
    }
    let mut harness = window(&settings, &folder);
    let name = row(&folder.file_name().unwrap().to_string_lossy());
    let at = chevron_of(&harness, &name);
    click_at(&mut harness, at);
    assert!(harness.query_by_label(&row("Hit 00.wav")).is_some(), "the folder is open");
    let list = harness.get_by_label(&row("Hit 00.wav")).rect();
    let bar = harness
        .get_all_by_role(egui::accesskit::Role::ScrollBar)
        .map(|node| node.rect())
        .find(|rect| rect.right() <= list.right() + 1.0 && rect.left() >= list.right() - 20.0)
        .expect("the list has a scroll bar on its right edge");
    assert_eq!(harness.state().browser.scroll, 0.0);
    let picked: Vec<String> = harness.state().browser.selected().iter().map(|s| s.key.clone()).collect();
    let from = Pos2::new(bar.center().x, bar.top() + 4.0);
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    for y in [10.0, 30.0, 60.0, 100.0] {
        harness.hover_at(from + egui::vec2(0.0, y));
        harness.step();
    }
    harness.drop_at(from + egui::vec2(0.0, 100.0));
    harness.run();
    assert!(harness.state().browser.scroll > 50.0, "the list follows the bar: {}", harness.state().browser.scroll);
    assert!(harness.query_by_label(&row("Hit 00.wav")).is_none(), "the first row has scrolled out of view");
    let after: Vec<String> = harness.state().browser.selected().iter().map(|s| s.key.clone()).collect();
    assert_eq!(after, picked, "the rows under the bar took nothing");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_double_click_on_a_folders_chevron_opens_it_and_stays_open() {
    let (settings, folder) = (temp("chevron-settings"), temp("chevron"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    // Clicking the chevron of the library folder twice in quick succession — the second
    // click inside egui's double-click window (0.3 s, 6 pt) — opens it once and leaves it
    // open, instead of opening and closing it again (#293).
    let name = folder.file_name().unwrap().to_string_lossy().to_string();
    let at = harness.get_by_label(&row(&name)).rect();
    let chevron = Pos2::new(at.left() + 8.0, at.center().y);
    quick_click_at(&mut harness, chevron);
    quick_click_at(&mut harness, chevron);
    harness.run();
    assert!(harness.query_by_label(&row("Groove.mid")).is_some(), "a double-click on the chevron opens the folder");
    // Past the double-click window, a click on the chevron closes it again: every distinct
    // click toggles.
    for _ in 0..8 {
        harness.step();
    }
    quick_click_at(&mut harness, chevron);
    harness.run();
    assert!(harness.query_by_label(&row("Groove.mid")).is_none(), "a click after the window closes it");
    // A double-click on the row itself still opens it.
    double_click(&mut harness, &row(&name));
    harness.run();
    assert!(harness.query_by_label(&row("Groove.mid")).is_some(), "a double-click on the folder opens it");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_click_on_a_headings_chevron_opens_it_and_stays_open() {
    let (settings, folder) = (temp("heading-chevron-stays-settings"), temp("heading-chevron-stays"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let at = harness.get_by_label(&row("Instruments")).rect();
    let chevron = Pos2::new(at.left() + 8.0, at.center().y);
    // A single click on a heading's chevron opens it and it stays open: the press and the
    // release of the click must not each toggle it (#293).
    quick_click_at(&mut harness, chevron);
    harness.run();
    assert!(harness.query_by_label(&row("FM Synth")).is_some(), "a click on the heading's chevron opens it");
    // Past the double-click window (eight frames is 0.4 s, the window is 0.3 s), the next
    // click closes it again.
    for _ in 0..8 {
        harness.step();
    }
    quick_click_at(&mut harness, chevron);
    harness.run();
    assert!(harness.query_by_label(&row("FM Synth")).is_none(), "a click after the window closes it");
    // A rapid double-click on the chevron opens it once: the second click of the pair is a
    // double and does not toggle again.
    for _ in 0..8 {
        harness.step();
    }
    quick_click_at(&mut harness, chevron);
    quick_click_at(&mut harness, chevron);
    harness.run();
    assert!(harness.query_by_label(&row("FM Synth")).is_some(), "a double-click on the chevron opens it once");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn search_and_the_category_filters_narrow_the_results() {
    let (settings, folder) = (temp("search-settings"), temp("search"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().browser.search = "kick".into();
    harness.run();
    assert!(harness.query_by_label(&row("Kick Hard.wav")).is_some());
    assert!(harness.query_by_label(&row("Snare.wav")).is_none());

    harness.state_mut().browser.search.clear();
    harness.get_by_label("Show MIDI").click();
    harness.run();
    harness.state_mut().browser.search = "o".into();
    harness.run();
    assert!(harness.query_by_label(&row("Groove.mid")).is_some());
    assert!(harness.query_by_label(&row("Loop 120bpm.wav")).is_none(), "MIDI only");
    harness.get_by_label("Show Instruments").click();
    harness.run();
    harness.state_mut().browser.search = "synth".into();
    harness.run();
    assert!(harness.query_by_label(&row("FM Synth")).is_some());
    assert!(harness.query_by_label(&row("Overdrive")).is_none());
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn ctrl_f_searches_ctrl_b_hides_and_rows_never_take_the_keyboard() {
    let (settings, folder) = (temp("keys-settings"), temp("keys"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.get_by_label(&row("Instruments")).click();
    harness.run();
    let focused = harness.ctx.memory(|m| m.focused());
    assert_eq!(focused, None, "a click on a row leaves the keyboard to the shortcuts");

    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    harness.run();
    harness.run();
    assert!(
        harness.ctx.memory(|m| m.has_focus(egui::Id::unique("browser-search"))),
        "Ctrl+F puts the cursor in the search"
    );
    // Typing goes to the field, and Down selects the first result.
    harness.input_mut().events.push(egui::Event::Text("snare".into()));
    harness.run();
    assert_eq!(harness.state().browser.search, "snare");
    harness.key_press(Key::ArrowDown);
    harness.run();
    assert_eq!(harness.state().browser.selected().first().map(|s| s.name.as_str()), Some("Snare.wav"));
    // Escape clears the search, a second Escape gives the keyboard back.
    harness.key_press(Key::Escape);
    harness.run();
    assert!(harness.state().browser.search.is_empty());
    harness.key_press(Key::Escape);
    harness.run();
    assert!(!harness.ctx.memory(|m| m.has_focus(egui::Id::unique("browser-search"))));

    harness.key_press_modifiers(Modifiers::COMMAND, Key::B);
    harness.run();
    assert!(!harness.state().settings().browser_open);
    assert!(harness.query_by_label(&row("Instruments")).is_none());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::B);
    harness.run();
    assert!(harness.state().settings().browser_open);
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// Ctrl+F always searches everything: All is chosen and the search kept, selected, so typing
/// replaces it; a kind chosen afterwards keeps the search and narrows it to that kind.
#[test]
fn ctrl_f_searches_all_and_a_kind_chosen_after_keeps_the_search() {
    let (settings, folder) = (temp("all-settings"), temp("all"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.get_by_label("Show Samples").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    harness.run();
    harness.run();
    assert_eq!(harness.state().browser.category, suhina_session::Category::All, "Ctrl+F chooses All");
    assert_eq!(harness.state().browser.place, None);
    harness.input_mut().events.push(egui::Event::Text("snare".into()));
    harness.run();
    assert_eq!(harness.state().browser.search, "snare");

    // A kind chosen keeps the search, and the search looks in that kind only.
    harness.get_by_label("Show Samples").click();
    harness.run();
    assert_eq!(harness.state().browser.category, suhina_session::Category::Samples);
    assert_eq!(harness.state().browser.search, "snare");

    // Ctrl+F again: All, the search selected, so what is typed replaces it.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    harness.run();
    harness.run();
    assert_eq!(harness.state().browser.category, suhina_session::Category::All);
    harness.input_mut().events.push(egui::Event::Text("kick".into()));
    harness.run();
    assert_eq!(harness.state().browser.search, "kick", "typing replaced the selected search");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_midi_file_dragged_onto_a_slot_becomes_a_clip_there() {
    let (settings, folder) = (temp("slot-settings"), temp("slot"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().browser.search = "groove".into();
    harness.run();
    let from = harness.get_by_label(&row("Groove.mid")).rect().center();
    let to = harness.get_by_label("Stop Keys (scene 1)").rect().center();
    drag(&mut harness, from, to);
    let keys_clip = |app: &App| {
        app.session().is_some_and(|s| {
            let p = s.project();
            let keys = p.tracks.iter().find(|t| t.name == "Keys").map(|t| t.id);
            p.launcher.columns.iter().find(|c| Some(c.track) == keys).and_then(|c| c.slots[0].clip.as_ref()).is_some()
        })
    };
    assert!(until(&mut harness, keys_clip), "the clip is in Keys' first slot");
    assert!(harness.state().browser.drag.is_none());
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_midi_file_dropped_from_the_system_onto_a_slot_becomes_a_clip_there() {
    let (settings, folder) = (temp("system-slot-settings"), temp("system-slot"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let to = harness.get_by_label("Stop Keys (scene 1)").rect().center();
    system_drop(&mut harness, &folder.join("Groove.mid"), to);
    let keys_clip = |app: &App| {
        app.session().is_some_and(|s| {
            let p = s.project();
            let keys = p.tracks.iter().find(|t| t.name == "Keys").map(|t| t.id);
            p.launcher.columns.iter().find(|c| Some(c.track) == keys).and_then(|c| c.slots[0].clip.as_ref()).is_some()
        })
    };
    assert!(until(&mut harness, keys_clip), "the clip is in Keys' first slot");
    assert!(!matches!(harness.state().status(), suhina_ui::Status::Error(_)), "no error");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_sample_dropped_from_the_system_onto_the_empty_space_makes_an_audio_track() {
    let (settings, folder) = (temp("system-empty-settings"), temp("system-empty"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let tracks = |app: &App| app.session().map_or(0, |s| s.project().tracks.len());
    let before = tracks(harness.state());
    let to = harness.get_by_label("+ Audio track").rect().center() + egui::vec2(0.0, 40.0);
    system_drop(&mut harness, &folder.join("Loop 120bpm.wav"), to);
    assert!(until(&mut harness, |app| tracks(app) == before + 1), "a new track");
    let session = harness.state().session().unwrap();
    assert!(session.project().tracks.iter().any(|t| t.name == "Loop 120bpm" && t.content.audio));
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// The repository's Stems test file (a master and four stems: Drums, Bass, Other, Vox in one
/// MP4, `crates/media/tests/data`), copied into `folder` under a name of its own.
fn stems(folder: &Path) -> PathBuf {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../media/tests/data/tones-aac.stem.mp4");
    let to = folder.join("tones.stem.mp4");
    std::fs::copy(from, &to).unwrap();
    to
}

#[test]
fn a_stems_file_dropped_on_a_lane_becomes_one_folded_group() {
    let (settings, folder) = (temp("stems-settings"), temp("stems"));
    pack(&folder);
    let file = stems(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().do_action("view.toggle", &mut Vec::new());
    harness.run();
    let header = harness.get_by_label("Track Keys").rect();
    let x = if header.center().x < SIZE.0 / 2.0 { header.right() + 60.0 } else { header.left() - 60.0 };
    system_drop(&mut harness, &file, Pos2::new(x, header.center().y));
    let grouped = |app: &App| {
        app.session().is_some_and(|s| {
            let p = s.project();
            p.tracks.iter().filter(|t| t.parent.is_some()).count() == 5 && p.tracks.iter().any(|t| t.name == "Master")
        })
    };
    assert!(until(&mut harness, grouped), "a group of the master and its four stems");
    // One more frame: the drop runs after the view has drawn, so the lanes follow it next frame.
    harness.run();
    // The group arrives folded: one lane named after the file, its tracks away. The folded lane
    // draws every stem's clips as a thin row each, marks rather than clips to pick: the stems are
    // reached by unfolding.
    assert!(harness.query_by_label("Track tones").is_some(), "the group's own lane");
    assert!(harness.query_by_label("Unfold tones").is_some(), "it arrives folded");
    assert!(harness.query_by_label("Track Master").is_none(), "the stems' lanes are folded away");
    assert!(harness.query_by_label_contains("Clip tones – Master").is_none(), "the fold draws marks, not clips");

    harness.get_by_label("Unfold tones").click();
    harness.run();
    assert!(harness.query_by_label("Track Master").is_some(), "unfolding brings the stems back");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_midi_file_dropped_from_the_system_onto_an_arrangement_lane_lands_on_that_track() {
    let (settings, folder) = (temp("system-lane-settings"), temp("system-lane"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().do_action("view.toggle", &mut Vec::new());
    harness.run();
    let header = harness.get_by_label("Track Keys").rect();
    // The lane is beside the header, on the side away from the window's edge.
    let x = if header.center().x < SIZE.0 / 2.0 { header.right() + 60.0 } else { header.left() - 60.0 };
    system_drop(&mut harness, &folder.join("Groove.mid"), Pos2::new(x, header.center().y));
    let landed = |app: &App| {
        app.session().is_some_and(|s| {
            let p = s.project();
            let keys = p.tracks.iter().find(|t| t.name == "Keys").map(|t| t.id);
            let groove = |c: &suhina_engine::model::Clip| {
                p.contents.iter().any(|x| x.id == c.content && x.name.starts_with("Groove"))
            };
            p.tracks.len() == demo_project().tracks.len()
                && p.arrangement.lanes.iter().any(|l| l.track == keys && l.clips.iter().any(groove))
        })
    };
    assert!(until(&mut harness, landed), "a Groove clip on Keys, no new track");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn system_files_without_a_pointer_position_go_where_enter_puts_them_and_projects_still_open() {
    let (settings, folder) = (temp("system-nopos-settings"), temp("system-nopos"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let tracks = |app: &App| app.session().map_or(0, |s| s.project().tracks.len());
    let before = tracks(harness.state());
    // The pointer is outside the window, as during an OS drag that reports no position.
    harness.remove_cursor();
    harness.run();
    let wav = folder.join("Loop 120bpm.wav");
    harness.input_mut().dropped_files.push(std::sync::Arc::new(TestFile(wav)));
    harness.step();
    assert!(until(&mut harness, |app| tracks(app) == before + 1), "no track selected: a new track");

    let song = folder.join("Song.suhina");
    harness.input_mut().dropped_files.push(std::sync::Arc::new(TestFile(song)));
    harness.step();
    harness.run();
    let asked = harness.state_mut().take_open_request().is_some()
        || harness.query_by_label_contains("Open Song.suhina").is_some();
    assert!(asked, "a project dropped from the system still opens");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_sample_dragged_onto_the_empty_space_makes_an_audio_track() {
    let (settings, folder) = (temp("empty-settings"), temp("empty"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().browser.search = "loop".into();
    harness.run();
    let tracks = |app: &App| app.session().map_or(0, |s| s.project().tracks.len());
    let before = tracks(harness.state());
    let from = harness.get_by_label(&row("Loop 120bpm.wav")).rect().center();
    let to = harness.get_by_label("+ Audio track").rect().center() + egui::vec2(0.0, 40.0);
    drag(&mut harness, from, to);
    assert!(until(&mut harness, |app| tracks(app) == before + 1), "a new track");
    let session = harness.state().session().unwrap();
    let track = session.project().tracks.iter().find(|t| t.name == "Loop 120bpm").unwrap();
    assert!(track.content.audio);
    // The loop's tempo is in its name, so it is warped.
    let project = session.project();
    let slots = project.launcher.columns.iter().flat_map(|c| &c.slots).filter_map(|s| s.clip.as_ref());
    let warped = project.arrangement.lanes.iter().flat_map(|l| &l.clips).chain(slots).any(|c| c.audio.warped);
    assert!(warped);
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// Drops Groove.mid from the browser onto the Drums lane in the Arrangement view with the
/// control bar's grid at 1/8, zoomed in to 64 points a beat, 1.3 beats after Beat B's start
/// (beat 64), holding `modifiers`. Returns the starts of the Drums lane's clips once it is in.
fn drop_groove_on_the_drums_lane(name: &str, modifiers: Modifiers) -> Vec<f64> {
    let (settings, folder) = (temp(&format!("{name}-settings")), temp(name));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().browser.search = "groove".into();
    harness.run();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.state_mut().set_arrangement_grid(suhina_ui::arrangement::arrangement_view::PianoGrid::from_value("1/8"));
    harness.run();
    // Zoom in eight times around a point half a beat into Beat B: its start stays in view.
    let clip = harness.get_by_label("Clip Beat B on Drums at bar 17").rect();
    harness.hover_at(egui::pos2(clip.left() + 4.0, clip.center().y));
    harness.run();
    harness.event(egui::Event::Zoom(8.0));
    harness.run();
    let clip = harness.get_by_label("Clip Beat B on Drums at bar 17").rect();
    let to = egui::pos2(clip.left() + 1.3 * 64.0, clip.center().y);

    let starts = |app: &App| {
        app.session()
            .map_or_else(Vec::new, |s| s.project().arrangement.lanes[0].clips.iter().map(|c| c.start).collect())
    };
    let before = starts(harness.state()).len();
    let from = harness.get_by_label(&row("Groove.mid")).rect().center();
    harness.event(egui::Event::ModifiersChanged(modifiers));
    drag(&mut harness, from, to);
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    assert!(until(&mut harness, |app| starts(app).len() > before), "the clip is on the Drums lane");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
    starts(harness.state())
}

#[test]
fn a_midi_file_dropped_on_a_lane_starts_on_the_chosen_grid_unless_alt_is_held() {
    // 1/8: on the eighth nearest the pointer, beat 65.5 (not the sixteenth the zoom alone
    // would allow).
    let starts = drop_groove_on_the_drums_lane("lane-grid", Modifiers::NONE);
    assert!(starts.contains(&65.5), "{starts:?}");
    // Alt: where the pointer is, off the grid.
    let starts = drop_groove_on_the_drums_lane("lane-free", Modifiers::ALT);
    assert!(starts.iter().any(|s| (s - 65.3).abs() < 0.02 && (s * 8.0).fract().abs() > 0.01), "{starts:?}");
}

#[test]
fn a_device_dragged_onto_a_track_header_joins_its_chain_and_becomes_recent() {
    let (settings, folder) = (temp("device-settings"), temp("device"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.get_by_label(&row("Effects")).click();
    harness.run();
    let from = harness.get_by_label(&row("Overdrive")).rect().center();
    let to = harness.get_by_label("Track Bass").rect().center();
    drag(&mut harness, from, to);
    let bass_devices = |app: &App| {
        app.session().map_or(0, |s| {
            let p = s.project();
            let channel = p.tracks.iter().find(|t| t.name == "Bass").and_then(|t| t.channel).unwrap_or(ChannelId(0));
            p.channel(channel).map_or(0, |c| c.devices.len())
        })
    };
    assert!(until(&mut harness, |app| bass_devices(app) == 1));
    harness.get_by_label("Place Recent Effects").click();
    harness.run();
    assert!(harness.query_by_label(&row("Overdrive")).is_some(), "Recent › Effects lists it");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// A device with stock presets opens on them: its own row adds it at its defaults, a preset's row
/// with that preset. The same presets are under their device's heading in Stock presets.
#[test]
fn a_device_opens_on_its_presets_and_each_row_adds_what_it_names() {
    let (settings, folder) = (temp("device-presets-settings"), temp("device-presets"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.get_by_label(&row("Stock presets")).click();
    harness.run();
    assert!(harness.query_by_label(&row("Warm FM Bass")).is_none(), "the presets lie under their device");
    harness.get_by_label(&row("FM Synth")).click();
    harness.run();
    assert!(harness.query_by_label(&row("Warm FM Bass")).is_some());
    harness.get_by_label(&row("FM Synth")).click();
    harness.run();
    harness.get_by_label(&row("Stock presets")).click();
    harness.run();

    let value_on_bass = |app: &App, id: &str| {
        app.session().and_then(|s| {
            let p = s.project();
            let channel = p.tracks.iter().find(|t| t.name == "Bass").and_then(|t| t.channel)?;
            let device = p.channels.iter().find(|c| c.id == channel)?.devices.first()?;
            device.params.iter().find(|param| param.id == id).map(|param| param.value)
        })
    };
    harness.get_by_label(&row("Instruments")).click();
    harness.run();
    let to = harness.get_by_label("Track Bass").rect().center();
    let device = harness.get_by_label(&row("FM Synth")).rect();
    assert!(harness.query_by_label(&row("Warm FM Bass")).is_none(), "closed at first");
    let from = device.center();
    drag(&mut harness, from, to);
    assert!(
        until(&mut harness, |app| value_on_bass(app, "op2.on") == Some(0.0)),
        "the device itself starts at its defaults"
    );
    quick_click_at(&mut harness, Pos2::new(device.left() + 8.0, device.center().y));
    harness.run();
    let from = harness.get_by_label(&row("Warm FM Bass")).rect().center();
    drag(&mut harness, from, to);
    assert!(
        until(&mut harness, |app| value_on_bass(app, "op2.level") == Some(86.0)),
        "a preset's row brings its values"
    );
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// A return's and a group's rows in the Arrangement take devices as a track's do, on the header
/// and on the lane.
#[test]
fn a_device_dragged_onto_a_return_in_the_arrangement_joins_its_chain() {
    let (settings, folder) = (temp("return-device-settings"), temp("return-device"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().do_action("view.toggle", &mut Vec::new());
    harness.run();
    harness.get_by_label(&row("Effects")).click();
    harness.run();
    let devices = |app: &App, name: &str| {
        app.session().map_or(0, |s| {
            let p = s.project();
            let channel = p.tracks.iter().find(|t| t.name == name).and_then(|t| t.channel).unwrap_or(ChannelId(0));
            p.channel(channel).map_or(0, |c| c.devices.len())
        })
    };
    // Two tracks in a group, so there is a group's row as well.
    let tracks: Vec<u32> =
        harness.state().session().map_or(Vec::new(), |s| s.project().tracks.iter().take(2).map(|t| t.id.0).collect());
    harness.state_mut().run(&suhina_ui::kit::widgets::Action::new("track.group", json!({ "tracks": tracks })));
    harness.run();
    let group = harness.state().session().and_then(|s| {
        let p = s.project();
        let submix = |t: &&suhina_engine::model::Track| {
            t.channel.and_then(|c| p.channel(c)).is_some_and(|c| c.role == suhina_engine::model::ChannelRole::Submix)
        };
        p.tracks.iter().find(submix).map(|t| t.name.clone())
    });
    let group = group.unwrap();
    for name in ["A Reverb", group.as_str()] {
        let before = devices(harness.state(), name);
        let header = harness.get_by_label(&format!("Turn {name} off")).rect();
        // On the header, then on the lane beside it, on the side away from the window's edge.
        let lane = if header.center().x < SIZE.0 / 2.0 { header.right() + 300.0 } else { header.left() - 300.0 };
        for (added, at) in [(1, header.center()), (2, Pos2::new(lane, header.center().y))] {
            let from = harness.get_by_label(&row("Overdrive")).rect().center();
            drag(&mut harness, from, at);
            assert!(until(&mut harness, |app| devices(app, name) == before + added), "{name}: {added} at {at:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// The browser lists MIDI effects under a heading of their own, and an instrument dragged onto a
/// track lands after the MIDI effect dragged there first.
#[test]
fn midi_effects_have_their_own_heading_and_go_before_the_instrument() {
    let (settings, folder) = (temp("midi-effect-settings"), temp("midi-effect"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    assert!(harness.query_by_label(&row("Pitch")).is_none(), "closed headings hide their devices");
    harness.get_by_label(&row("MIDI Effects")).click();
    harness.run();
    assert!(harness.query_by_label(&row("Pitch")).is_some(), "MIDI Effects lists the pitch effect");
    harness.get_by_label(&row("Effects")).click();
    harness.run();
    assert!(harness.query_by_label(&row("Pitch")).is_some(), "and Effects does not take it over");
    let bass_devices = |app: &App| {
        app.session().map_or(Vec::new(), |s| {
            let p = s.project();
            let channel = p.tracks.iter().find(|t| t.name == "Bass").and_then(|t| t.channel).unwrap_or(ChannelId(0));
            p.channels
                .iter()
                .find(|c| c.id == channel)
                .map_or(Vec::new(), |c| c.devices.iter().map(|d| d.name.clone()).collect::<Vec<_>>())
        })
    };
    let to = harness.get_by_label("Track Bass").rect().center();
    let from = harness.get_by_label(&row("Pitch")).rect().center();
    drag(&mut harness, from, to);
    assert!(until(&mut harness, |app| bass_devices(app) == ["Pitch"]), "{:?}", bass_devices(harness.state()));
    harness.get_by_label(&row("Instruments")).click();
    harness.run();
    let from = harness.get_by_label(&row("FM Synth")).rect().center();
    drag(&mut harness, from, to);
    assert!(
        until(&mut harness, |app| bass_devices(app) == ["Pitch", "FM Synth"]),
        "the instrument goes after the MIDI effect: {:?}",
        bass_devices(harness.state())
    );
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn escape_cancels_a_drag_and_a_project_opens_with_a_double_click() {
    let (settings, folder) = (temp("cancel-settings"), temp("cancel"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().browser.search = "song".into();
    harness.run();
    double_click(&mut harness, &row("Song.suhina"));
    harness.run();
    let asked = harness.state_mut().take_open_request().is_some()
        || harness.query_by_label_contains("Open Song.suhina").is_some();
    assert!(asked, "the project is asked to open");

    harness.state_mut().browser.search = "kick".into();
    harness.run();
    let from = harness.get_by_label(&row("Kick Hard.wav")).rect().center();
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(from + egui::vec2(300.0, 0.0));
    harness.step();
    assert!(harness.state().browser.drag.is_some());
    harness.key_press(Key::Escape);
    harness.step();
    assert!(harness.state().browser.drag.is_none());
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_folder_is_added_from_the_sidebar_and_the_panel_moves_to_the_right() {
    let (settings, folder) = (temp("add-settings"), temp("add"));
    let other = temp("add-other");
    pack(&folder);
    wav(&other.join("Clap.wav"), 0.1);
    let mut harness = window(&settings, &folder);
    harness.get_by_label("Add folder").click();
    harness.run();
    harness.input_mut().events.push(egui::Event::Text(other.display().to_string()));
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    let name = other.file_name().unwrap().to_string_lossy().to_string();
    assert!(harness.query_by_label(&format!("Place {name}")).is_some(), "the folder is in the sidebar");

    let left = harness.get_by_label("Place Pinned").rect().center().x;
    assert!(left < SIZE.0 / 2.0);
    let mut settings_now = harness.state().settings().clone();
    settings_now.browser_side = suhina_ui::settings::Side::Right;
    let ctx = harness.ctx.clone();
    harness.state_mut().set_settings(&ctx, settings_now);
    harness.run();
    assert!(harness.get_by_label("Place Pinned").rect().center().x > SIZE.0 / 2.0);
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
    let _ = std::fs::remove_dir_all(&other);
}

/// Renders the window with the browser open to `target/ui-snapshots/browser.png`, for looking
/// at: `cargo test -p suhina-ui --test browser -- --ignored`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_browser() {
    let (settings, folder) = (temp("render-settings"), temp("render"));
    pack(&folder);
    let mut app = App::new();
    let mut library = Library::new(Some(settings.clone()));
    library.add_folder(&folder).unwrap();
    library
        .tag(
            &folder.join("Drums").join("Kick Hard.wav").display().to_string(),
            suhina_library::store::named_colours()
                .iter()
                .position(|(name, _)| *name == "Red")
                .map(|at| suhina_library::store::FIRST_TAG_ID + at as u32)
                .unwrap_or(1),
            true,
        )
        .unwrap();
    app.set_file_library(library);
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .allow_missing_glyphs()
        .with_size(SIZE)
        .with_pixels_per_point(2.0)
        .with_step_dt(0.05)
        .with_max_steps(40)
        .wgpu()
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.get_by_label(&row("Instruments")).click();
    harness.run();
    let name = folder.file_name().unwrap().to_string_lossy().to_string();
    double_click(&mut harness, &row(&name));
    double_click(&mut harness, &row("Drums"));
    harness.get_by_label(&row("Kick Hard.wav")).click();
    for _ in 0..20 {
        harness.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&out).unwrap_or_else(|e| panic!("{e}"));
    image.save(out.join("browser.png")).unwrap_or_else(|e| panic!("{e}"));
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_start_panel_hides_the_browser_until_a_project_is_open() {
    let (settings, folder) = (temp("nothing-open-settings"), temp("nothing-open"));
    pack(&folder);
    let mut app = App::new();
    let mut library = Library::new(Some(settings.to_path_buf()));
    library.add_folder(&folder).unwrap();
    app.set_file_library(library);
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .allow_missing_glyphs()
        .with_size(SIZE)
        .with_step_dt(0.05)
        .with_max_steps(20)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    assert!(harness.state().projects().is_empty());
    // With nothing open the start panel stands alone (#632): no browser beside it.
    let name = folder.file_name().unwrap().to_string_lossy().to_string();
    assert!(harness.query_by_label(&row(&name)).is_none(), "no browser on the start panel");
    // A project brings it back.
    harness.state_mut().set_session(Session::new(demo_project()), &[]);
    harness.run();
    assert!(harness.query_by_label(&row(&name)).is_some(), "the browser shows with a project open");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_plug_ins_category_lists_the_instruments_or_the_effects_by_name_and_format() {
    let (settings, folder) = (temp("listed-settings"), temp("listed"));
    let mut harness = window_with_plugins(&settings, &folder);
    harness.get_by_label("Show Plug-ins").click();
    harness.run();
    // The instruments first: no headings, the built-in effects and instruments not among them,
    // and only the name and format columns.
    assert!(harness.query_by_label(&row("Zeta Synth")).is_some());
    assert!(harness.query_by_label(&row("Alpha Verb")).is_none(), "an effect");
    assert!(harness.query_by_label(&row("Instruments")).is_none(), "no built-in heading");
    assert!(harness.query_by_label(&row("Effects")).is_none(), "no built-in heading");
    assert!(harness.query_by_label("Sort by Format").is_some());
    assert!(harness.query_by_label("Sort by Vendor").is_none() && harness.query_by_label("Sort by Modified").is_none());
    // The effects, in name order; the name heading again reverses them.
    harness.get_by_label("Show plug-in effects").click();
    harness.run();
    assert!(harness.query_by_label(&row("Zeta Synth")).is_none(), "an instrument");
    assert!(row_y(&harness, "Alpha Delay") < row_y(&harness, "Alpha Verb"));
    harness.get_by_label("Sort by Name").click();
    harness.run();
    assert!(row_y(&harness, "Alpha Verb") < row_y(&harness, "Alpha Delay"), "the same column again reverses");
    // The choice is kept for the next start: it is a library command, not the window's.
    let mut view = Library::new(Some(settings.to_path_buf()));
    assert_eq!(view.plugins_view(), suhina_library::store::PluginsView::Effects);
    // Other categories keep the plug-ins in the tree under their vendors.
    harness.get_by_label("Show All").click();
    harness.run();
    assert!(harness.query_by_label(&row("Plug-ins")).is_some(), "the Plug-ins heading in the tree");
    assert!(harness.query_by_label(&row("Alpha Verb")).is_none(), "under a closed heading");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_place_stays_chosen_on_a_second_click_and_its_cross_shows_everything() {
    let (settings, folder) = (temp("place-settings"), temp("place"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.get_by_label("Place Pinned").click();
    harness.run();
    harness.get_by_label("Place Pinned").click();
    harness.run();
    assert!(harness.state().browser.place.is_some(), "a second click keeps the place");
    harness.get_by_label("Show everything").click();
    harness.run();
    assert!(harness.state().browser.place.is_none(), "the cross lets go of it");
    // A recent group brings its category; another category lets go of the group.
    harness.get_by_label("Place Recent MIDI").click();
    harness.run();
    assert_eq!(harness.state().browser.category, suhina_session::Category::Midi);
    harness.get_by_label("Show Instruments").click();
    harness.run();
    assert!(harness.state().browser.place.is_none(), "MIDI's recent items are not instruments");
    // The chosen category clicked again shows its whole tree.
    harness.get_by_label("Place Pinned").click();
    harness.run();
    harness.get_by_label("Show Instruments").click();
    harness.run();
    assert!(harness.state().browser.place.is_none());
    // The sample map is the samples' other view: List and Sample map over the list while
    // Samples is chosen, and nowhere else (#711).
    assert!(harness.query_by_label("Show the samples laid out by how they sound").is_none());
    harness.get_by_label("Show Samples").click();
    harness.run();
    harness.get_by_label("Show the samples laid out by how they sound").click();
    harness.run();
    assert_eq!(harness.state().browser.mode, suhina_ui::browser::ResultsMode::Other);
    harness.get_by_label("Show the samples as a list").click();
    harness.run();
    assert_eq!(harness.state().browser.mode, suhina_ui::browser::ResultsMode::List);
    // A place that holds samples has the switch too, and the map shows it: Pinned (#862).
    let map = "Show the samples laid out by how they sound";
    harness.get_by_label("Place Pinned").click();
    harness.run();
    harness.get_by_label(map).click();
    harness.run();
    assert_eq!(harness.state().browser.mode, suhina_ui::browser::ResultsMode::Other, "Pinned's samples on the map");
    // The views without samples have none, and the list shows again there.
    harness.get_by_label("Show Plug-ins").click();
    harness.run();
    assert!(harness.query_by_label(map).is_none(), "no switch for Plug-ins");
    assert_eq!(harness.state().browser.mode, suhina_ui::browser::ResultsMode::List);
    // A tag's place holds the samples its items carry: the switch shows there too, and the
    // place of one maps with it ([#1254] review: the map follows the tags).
    harness.get_by_label("Show Samples").click();
    harness.run();
    harness
        .state_mut()
        .projects_mut()
        .execute(
            "library.tag",
            &json!({ "item": folder.join("Drums").join("Kick Hard.wav").display().to_string(), "tag": "Red", "on": true }),
        )
        .unwrap();
    harness.run();
    harness.get_by_label("Place Tag Red").click();
    harness.run();
    harness.get_by_label(map).click();
    harness.run();
    assert_eq!(harness.state().browser.mode, suhina_ui::browser::ResultsMode::Other, "a tag's own place maps");
    harness.get_by_label("Show the samples as a list").click();
    harness.run();
    assert_eq!(harness.state().browser.mode, suhina_ui::browser::ResultsMode::List);
    // With Recent's kinds as chips, the switch has its own row under them (#862).
    let ctx = harness.ctx.clone();
    let mut chips = harness.state().settings().clone();
    chips.view.recent_chips = true;
    harness.state_mut().set_settings(&ctx, chips);
    harness.get_by_label("Place Recent").click();
    harness.run();
    let kinds = harness.get_by_label("Recent Samples").rect();
    let list = harness.get_by_label("Show the samples as a list").rect();
    assert!(list.top() >= kinds.bottom(), "the switch is below Recent's chips: {list:?} under {kinds:?}");
    // Recent is an item of its own, every kind's recent items together.
    harness.get_by_label("Place Recent").click();
    harness.run();
    assert_eq!(harness.state().browser.place, Some(suhina_ui::browser::Place::Recent(None)));
    assert_eq!(harness.state().browser.category, suhina_session::Category::All);
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_plug_in_manager_switches_a_plug_in_off_and_the_browser_no_longer_offers_it() {
    let (settings, folder) = (temp("manager-settings"), temp("manager"));
    let mut harness = window_with_plugins(&settings, &folder);
    harness.get_by_label("Show Plug-ins").click();
    harness.run();
    assert!(harness.query_by_label(&row("Zeta Synth")).is_some());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Plug-ins").click();
    harness.run();
    harness.get_by_label("Open the plug-in manager").click();
    harness.run();
    // Every plug-in in one table, sortable by its columns.
    assert!(harness.query_by_label("Sort the plug-ins by Vendor").is_some());
    assert!(harness.query_by_label("Use Alpha Verb").is_some());
    harness.get_by_label("Use Zeta Synth").click();
    harness.run();
    let mut plugins = suhina_plugins::library::Library::new(Some(settings.join("plugins")), PathBuf::from("no-host"));
    let spec = plugins.catalog().find("Zeta Synth").map(|p| p.spec()).unwrap();
    assert!(!plugins.is_enabled(&spec), "switched off, and kept so");
    harness.get_by_label("Close the plug-in manager").click();
    harness.run();
    assert!(harness.query_by_label("Settings page: Plug-ins").is_some(), "Settings shows again");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    assert!(harness.query_by_label(&row("Zeta Synth")).is_none(), "the browser no longer offers it");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_folder_for_the_browser_is_chosen_in_the_systems_dialog() {
    use suhina_ui::interaction::pick::PickField;
    let (settings, folder) = (temp("pick-settings"), temp("pick"));
    let other = temp("pick-other");
    pack(&folder);
    wav(&other.join("Clap.wav"), 0.1);
    let mut harness = window(&settings, &folder);
    harness.get_by_label("Add folder").click();
    harness.run();
    harness.get_by_label("Browse for a folder").click();
    harness.run();
    let request = harness.state_mut().take_pick_request();
    assert_eq!(request.map(|r| r.field), Some(PickField::BrowserFolder));
    // The app shows the dialog and hands back the folder chosen.
    harness.state_mut().set_picked(PickField::BrowserFolder, other.clone());
    harness.run();
    let name = other.file_name().unwrap().to_string_lossy().to_string();
    assert!(harness.query_by_label(&format!("Place {name}")).is_some(), "the folder is in the sidebar");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
    let _ = std::fs::remove_dir_all(&other);
}

#[test]
fn a_plugin_drags_to_a_track_from_every_view() {
    let (settings, folder) = (temp("drag-settings"), temp("drag"));
    let mut harness = window_with_plugins(&settings, &folder);
    harness.get_by_label("Show Plug-ins").click();
    harness.run();
    let devices = |app: &App| {
        app.session().map_or(0, |s| {
            let p = s.project();
            let channel = p.tracks.iter().find(|t| t.name == "Bass").and_then(|t| t.channel).unwrap_or(ChannelId(0));
            p.channel(channel).map_or(0, |c| c.devices.len())
        })
    };
    let to = harness.get_by_label("Track Bass").rect().center();
    // From the Plug-ins category's list.
    assert!(harness.query_by_label(&row("Zeta Synth")).is_some(), "the instruments' list lists it");
    let before = devices(harness.state());
    let from = harness.get_by_label(&row("Zeta Synth")).rect().center();
    drag(&mut harness, from, to);
    assert!(until(&mut harness, |app| devices(app) == before + 1), "from the plug-ins' list");
    // From the tree, under the vendor.
    harness.get_by_label("Show All").click();
    harness.run();
    harness.get_by_label(&row("Plug-ins")).click();
    harness.run();
    harness.get_by_label(&row("Alpha")).click();
    harness.run();
    let before = devices(harness.state());
    let from = harness.get_by_label(&row("Alpha Verb")).rect().center();
    drag(&mut harness, from, to);
    assert!(until(&mut harness, |app| devices(app) == before + 1), "from the tree");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn hovering_the_footers_tag_dot_says_the_tags_name_and_count_in_the_status_bar() {
    let (settings, folder) = (temp("dot-footer-settings"), temp("dot-footer"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    // Tag one of the samples: the footer's dot's hover says the tag's name and the count.
    let item = folder.join("Drums").join("Kick Hard.wav").display().to_string();
    harness
        .state_mut()
        .projects_mut()
        .execute("library.tag", &json!({ "item": item, "tag": "Red", "on": true }))
        .unwrap();
    harness.run();
    let at = harness.get_by_label("Tag Red").rect().center();
    harness.hover_at(at);
    harness.run();
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.contains("Red") && text.contains("1 item")),
        "{:?}",
        harness.state().status()
    );
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn hovering_an_items_tag_dot_says_the_tag_in_the_status_bar() {
    let (settings, folder) = (temp("dot-row-settings"), temp("dot-row"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    // The library's folder opens to its files, the tag dots beside their names.
    let name = folder.file_name().unwrap().to_string_lossy().to_string();
    double_click(&mut harness, &row(&name));
    harness.run();
    // Tag two of the root's files: each row carries the tag's dot, whose hover says the tag's
    // name in the status bar.
    for (file, tag) in [("Loop 120bpm.wav", "Green"), ("Groove.mid", "Blue")] {
        harness
            .state_mut()
            .projects_mut()
            .execute("library.tag", &json!({ "item": folder.join(file).display().to_string(), "tag": tag, "on": true }))
            .unwrap();
    }
    harness.run();
    let at = harness.get_by_label("Tagged Green").rect().center();
    harness.hover_at(at);
    harness.run();
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.contains("Green") && !text.contains("Blue")),
        "{:?}",
        harness.state().status()
    );
    // The other row's dot says its own tag, not the first's.
    let at = harness.get_by_label("Tagged Blue").rect().center();
    harness.hover_at(at);
    harness.run();
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.contains("Blue") && !text.contains("Green")),
        "{:?}",
        harness.state().status()
    );
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn an_items_tag_menu_ticks_its_tags_with_painted_boxes_and_toggles_them() {
    let (settings, folder) = (temp("tag-menu-settings"), temp("tag-menu"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let name = folder.file_name().unwrap().to_string_lossy().to_string();
    double_click(&mut harness, &row(&name));
    harness.run();
    let item = folder.join("Loop 120bpm.wav").display().to_string();
    harness
        .state_mut()
        .projects_mut()
        .execute("library.tag", &json!({ "item": item, "tag": "Red", "on": true }))
        .unwrap();
    harness.run();
    harness.get_by_label(&row("Loop 120bpm.wav")).click_secondary();
    harness.run();
    let at = harness.get_by_label("Tag").rect().center();
    harness.hover_at(at);
    harness.run();
    // Each entry is a checkbox named by the tag alone: no check mark or padding in its text,
    // which a font without the glyph draws as an empty box (#853).
    assert_eq!(harness.get_by_label("Red").accesskit_node().toggled(), Some(egui::accesskit::Toggled::True));
    assert_eq!(harness.get_by_label("Orange").accesskit_node().toggled(), Some(egui::accesskit::Toggled::False));
    assert!(harness.query_by_label_contains("✓").is_none());
    harness.get_by_label("Orange").click();
    harness.run();
    let library = harness.state_mut().projects_mut().file_library_mut();
    let flat = library.tag_flat();
    let tags: Vec<&str> = library
        .item_tags(&item)
        .iter()
        .filter_map(|id| flat.iter().find(|t| t.id == *id).map(|t| t.name.as_str()))
        .collect();
    assert!(tags.contains(&"Orange") && tags.contains(&"Red"), "{tags:?}");
    // A click on a ticked entry takes its tag off.
    harness.get_by_label(&row("Loop 120bpm.wav")).click_secondary();
    harness.run();
    let at = harness.get_by_label("Tag").rect().center();
    harness.hover_at(at);
    harness.run();
    harness.get_by_label("Red").click();
    harness.run();
    let library = harness.state_mut().projects_mut().file_library_mut();
    let flat = library.tag_flat();
    let tags: Vec<&str> = library
        .item_tags(&item)
        .iter()
        .filter_map(|id| flat.iter().find(|t| t.id == *id).map(|t| t.name.as_str()))
        .collect();
    assert!(!tags.contains(&"Red") && tags.contains(&"Orange"), "{tags:?}");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// A tagged item is found again through the sidebar's Tags section (#1196): the tag listed
/// with its colour and count, clicked as a place lists every kind's items carrying it, and
/// the × lets it go again.
#[test]
fn a_tagged_item_is_found_again_through_the_tags_section() {
    let (settings, folder) = (temp("tags-side-settings"), temp("tags-side"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let name = folder.file_name().unwrap().to_string_lossy().to_string();
    double_click(&mut harness, &row(&name));
    harness.run();
    // The sample tagged from its row's Tag menu.
    harness.get_by_label(&row("Loop 120bpm.wav")).click_secondary();
    harness.run();
    let at = harness.get_by_label("Tag").rect().center();
    harness.hover_at(at);
    harness.run();
    harness.get_by_label("Red").click();
    harness.run();
    // The Tags section lists the tag; its count is said to the status bar.
    harness.hover_at(harness.get_by_label("Place Tag Red").rect().center());
    harness.run();
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.contains("Red") && text.contains("1 item")),
        "{:?}",
        harness.state().status()
    );
    // Clicking the tag shows the items carrying it, of any kind: a flat list, not the tree.
    harness.get_by_label("Place Tag Red").click();
    harness.run();
    assert!(harness.query_by_label(&row("Loop 120bpm.wav")).is_some(), "the tagged sample is listed");
    assert!(harness.query_by_label(&row("Instruments")).is_none(), "a tag's place is a flat list");
    let Some(suhina_ui::browser::Place::Tag(tags)) = harness.state().browser.place.clone() else {
        panic!("the tag is the place, got {:?}", harness.state().browser.place);
    };
    assert_eq!(
        tags.iter()
            .map(|id| harness.state_mut().projects_mut().file_library_mut().tag_name_of(*id))
            .collect::<Vec<_>>(),
        [Some("Red".to_string())],
        "the earlier click left it"
    );
    // A row of the list is selected as any row is.
    harness.get_by_label(&row("Loop 120bpm.wav")).click();
    harness.run();
    let selected = harness.state().browser.selected().first().map(|s| s.name.clone());
    assert_eq!(selected.as_deref(), Some("Loop 120bpm.wav"));
    // The × at the right of the chosen tag lets the place go: the tree shows again.
    harness.get_by_label("Show everything").click();
    harness.run();
    assert!(harness.query_by_label(&row("Instruments")).is_some());
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// A tag renamed through its row's menu carries the new name in the section; removing asks
/// first while items carry it, and a second click does it (#1196).
#[test]
fn the_tags_section_renames_and_removes_a_tag_asking_first() {
    let (settings, folder) = (temp("tags-menu-settings"), temp("tags-menu"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let item = folder.join("Loop 120bpm.wav").display().to_string();
    harness
        .state_mut()
        .projects_mut()
        .execute("library.tag", &json!({ "item": item, "tag": "Red", "on": true }))
        .unwrap();
    harness.run();
    // Typed into the menu's field, Enter renames the tag.
    harness.get_by_label("Place Tag Red").click_secondary();
    harness.run();
    harness.get_by_label("Rename tag Red").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    harness.get_by_label("Rename tag Red").type_text("Hot");
    harness.run();
    // The field holds its typing until Enter commits it: nothing has renamed yet, so the
    // section still reads the old name ([#1250] review — the rename under every key made
    // the field a widget of another id each frame and lost the typing).
    assert!(harness.query_by_label("Place Tag Red").is_some(), "typing alone renames nothing");
    harness.key_press(Key::Enter);
    harness.run();
    assert!(harness.query_by_label("Place Tag Hot").is_some(), "Enter renames the tag");
    assert!(harness.query_by_label("Place Tag Red").is_none(), "the old name is gone");
    // The menu is still open; a click off closes it.
    let at = Pos2::new(SIZE.0 - 20.0, SIZE.1 - 10.0);
    click_at(&mut harness, at);
    harness.run();
    assert!(harness.query_by_label("Place Tag Hot").is_some(), "the section lists the new name");
    // The menu went with the click off.
    assert!(harness.query_by_label("New tag name").is_none(), "the menu went with the rename");
    // The place still holds: the renamed tag's items.
    harness.get_by_label("Place Tag Hot").click();
    harness.run();
    assert!(harness.query_by_label(&row("Loop 120bpm.wav")).is_some());
    // Remove: while an item carries it the first click arms the asking, the second removes.
    harness.get_by_label("Place Tag Hot").click_secondary();
    harness.run();
    harness.get_by_label("Remove tag Hot").click();
    harness.run();
    assert!(harness.query_by_label_contains("Remove Hot?").is_some(), "the menu asks first");
    harness.get_by_label("Keep").click();
    harness.run();
    assert!(harness.query_by_label("Place Tag Hot").is_some(), "Keep stands the remove down");
    harness.get_by_label("Remove tag Hot").click();
    harness.run();
    harness.get_by_label_contains("Remove Hot?").click();
    harness.run();
    assert!(harness.query_by_label("Place Tag Hot").is_none(), "the tag is gone from the section");
    assert_eq!(harness.state().browser.place, None, "the place went with the tag");
    assert!(harness.state_mut().projects_mut().file_library_mut().item_tags(&item).is_empty());
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// A name another tag has can be typed to its end and Enter refuses it: the field holds its
/// typing until it commits, so the refusal comes once, with the tag as it was ([#1250]
/// review — the rename under every key was refused before the name could be typed).
#[test]
fn a_taken_name_is_typed_to_its_end_and_enter_refuses_it_once() {
    let (settings, folder) = (temp("tags-taken-settings"), temp("tags-taken"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.get_by_label("Place Tag Red").click_secondary();
    harness.run();
    harness.get_by_label("Rename tag Red").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Rename tag Red").type_text("Orange");
    harness.run();
    // Typed to its end, nothing has renamed yet.
    assert!(harness.query_by_label("Place Tag Red").is_some(), "typing alone renames nothing");
    harness.key_press(Key::Enter);
    harness.run();
    assert!(harness.query_by_label("Place Tag Red").is_some(), "the taken name refused");
    assert!(
        matches!(harness.state().status(), Status::Error(text) if text.contains("Orange")),
        "{:?}",
        harness.state().status()
    );
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// The remove's asking goes with the menu that asked: armed and let go with, the next
/// opening asks again — a click that never asked deletes nothing ([#1250] review).
#[test]
fn the_remove_asks_again_after_the_menu_was_let_go() {
    let (settings, folder) = (temp("tags-arm-settings"), temp("tags-arm"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let item = folder.join("Loop 120bpm.wav").display().to_string();
    harness
        .state_mut()
        .projects_mut()
        .execute("library.tag", &json!({ "item": item, "tag": "Red", "on": true }))
        .unwrap();
    harness.run();
    harness.get_by_label("Place Tag Red").click_secondary();
    harness.run();
    harness.get_by_label("Remove tag Red").click();
    harness.run();
    assert!(harness.query_by_label_contains("Remove Red?").is_some(), "the first click arms");
    // A click away: the menu closes, and the asking with it.
    click_at(&mut harness, Pos2::new(SIZE.0 - 20.0, SIZE.1 - 10.0));
    harness.run();
    harness.get_by_label("Place Tag Red").click_secondary();
    harness.run();
    assert!(
        harness.query_by_label("Remove tag Red").is_some() && harness.query_by_label_contains("Remove Red?").is_none(),
        "the next opening asks from the start",
    );
    harness.get_by_label("Remove tag Red").click();
    harness.run();
    assert!(harness.query_by_label_contains("Remove Red?").is_some(), "it asks again");
    harness.get_by_label("Keep").click();
    harness.run();
    assert!(harness.query_by_label("Place Tag Red").is_some(), "the tag stands");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// The footer's remove asks like the section's row menu does, the two menus in step: the
/// first click says what will go, and only the next removes ([#1250] review).
#[test]
fn the_footers_remove_asks_like_the_sections() {
    let (settings, folder) = (temp("tags-footer-arm-settings"), temp("tags-footer-arm"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let item = folder.join("Loop 120bpm.wav").display().to_string();
    harness
        .state_mut()
        .projects_mut()
        .execute("library.tag", &json!({ "item": item, "tag": "Red", "on": true }))
        .unwrap();
    harness.run();
    harness.get_by_label("Tag Red").click_secondary();
    harness.run();
    harness.get_by_label("Remove tag Red").click();
    harness.run();
    assert!(harness.query_by_label_contains("Remove Red?").is_some(), "the footer asks first");
    harness.get_by_label_contains("Remove Red?").click();
    harness.run();
    assert!(harness.query_by_label("Tag Red").is_none(), "the second click removes");
    assert!(harness.state_mut().projects_mut().file_library_mut().item_tags(&item).is_empty());
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// The footer's rename commits nothing with the field erased: a click off with an empty
/// field is no rename, and no error ([#1250] review).
#[test]
fn the_footers_erased_rename_commits_nothing() {
    let (settings, folder) = (temp("tags-footer-erase-settings"), temp("tags-footer-erase"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.get_by_label("Tag Red").click_secondary();
    harness.run();
    harness.get_by_label("Rename tag Red").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.key_press(Key::Delete);
    harness.run();
    click_at(&mut harness, Pos2::new(SIZE.0 - 20.0, SIZE.1 - 10.0));
    harness.run();
    assert!(!matches!(harness.state().status(), Status::Error(_)), "{:?}", harness.state().status());
    assert!(harness.query_by_label("Tag Red").is_some(), "the tag stays as it was");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// A place of several tags keeps every carrier the one-tag places list, and the footer's
/// dots narrow it — a filtered tag that a place's item does not carry brings nothing in
/// ([#1250] review: the dots' tags went into one any-of list and widened the place).
#[test]
fn a_place_of_several_tags_keeps_every_carrier_and_the_dots_narrow_it() {
    let (settings, folder) = (temp("tags-place-settings"), temp("tags-place"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let loop_file = folder.join("Loop 120bpm.wav").display().to_string();
    let groove = folder.join("Groove.mid").display().to_string();
    let hit = folder.join("Hit 00.wav").display().to_string();
    for (item, tag) in [(&loop_file, "Red"), (&groove, "Blue"), (&hit, "Green")] {
        harness
            .state_mut()
            .projects_mut()
            .execute("library.tag", &json!({ "item": item, "tag": tag, "on": true }))
            .unwrap();
    }
    harness.run();
    harness.get_by_label("Place Tag Red").click();
    harness.run();
    harness.get_by_label("Place Tag Blue").click_modifiers(Modifiers::COMMAND);
    harness.run();
    assert!(harness.query_by_label(&row("Loop 120bpm.wav")).is_some());
    assert!(harness.query_by_label(&row("Groove.mid")).is_some());
    assert!(harness.query_by_label(&row("Hit 00.wav")).is_none(), "the place is Red and Blue, not Green");
    // A dot the place's items do not carry: the place narrows to nothing, and nothing
    // comes in.
    harness.get_by_label("Tag Green").click();
    harness.run();
    assert!(harness.query_by_label(&row("Hit 00.wav")).is_none(), "the dot never widens the place");
    assert!(harness.query_by_label(&row("Loop 120bpm.wav")).is_none(), "the loop carries no Green");
    assert!(harness.query_by_label(&row("Groove.mid")).is_none(), "the groove carries no Green");
    // A dot the place's items do carry narrows it to those.
    harness.get_by_label("Tag Green").click();
    harness.run();
    harness.get_by_label("Tag Red").click();
    harness.run();
    assert!(harness.query_by_label(&row("Loop 120bpm.wav")).is_some(), "the loop carries Red");
    assert!(harness.query_by_label(&row("Groove.mid")).is_none(), "the groove does not");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// A drag that carries headings only — as the results' own drag start never lets one begin,
/// but a drag source that grows one could — says on the release what the pill said mid-drag:
/// a done "tagged 0 items" is neither ([#1250] review). A folder is tagged as a whole
/// ([#1254] review), so only a heading is left out.
#[test]
fn a_headings_drop_on_a_tag_says_so_instead_of_tagging_zero() {
    let (settings, folder) = (temp("tags-folder-drop-settings"), temp("tags-folder-drop"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let name = folder.file_name().unwrap().to_string_lossy().to_string();
    harness.state_mut().browser.drag = Some(vec![suhina_ui::interaction::drop::BrowserItem {
        key: folder.display().to_string(),
        name,
        kind: suhina_ui::interaction::drop::ItemKind::Heading,
        plugin: false,
    }]);
    // The pointer goes down on the empty corner — no row's drag competes with the folder's —
    // and flies to the tag's row while it is down, as a drag of its own would.
    harness.drag_at(Pos2::new(SIZE.0 - 20.0, SIZE.1 - 10.0));
    harness.step();
    let to = harness.get_by_label("Place Tag Red").rect().center();
    harness.hover_at(to);
    harness.step();
    // Mid-drag the status holds back: nothing is tagged yet (the pill beside the pointer
    // says what a heading's drop does not do).
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.contains("tags items")),
        "the flight promises no tagging: {:?}",
        harness.state().status(),
    );
    harness.drop_at(to);
    harness.run();
    // Nothing came of the drop: no done tagging (the release's own word, where "tagged 0
    // items" was neither), and the tag carries nothing.
    assert!(
        !matches!(harness.state().status(), Status::Done(_)),
        "no done tagging came of it: {:?}",
        harness.state().status(),
    );
    assert_eq!(
        harness.state_mut().projects_mut().file_library_mut().tagged_count(1),
        0,
        "the heading was never tagged (Red is the first tag)",
    );
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// A tag's right-click menu also adds one: a typed name with the next default colour shows
/// the section, dimmed while nothing carries it; a name one already has is an error, and
/// nothing is added twice (#1196).
#[test]
fn the_tags_right_menu_adds_a_tag() {
    let (settings, folder) = (temp("tags-add-settings"), temp("tags-add"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    // From a row of the section's right-click: a field that adds one, with a button beside it
    // or with Enter.
    harness.get_by_label("Place Tag Red").click_secondary();
    harness.run();
    assert!(harness.query_by_label("New tag name").is_some(), "the menu offers adding a tag");
    assert!(harness.query_by_label("Add tag").is_some(), "the button beside the field");
    harness.get_by_label("New tag name").click();
    harness.run();
    harness.get_by_label("New tag name").type_text("Punchy");
    harness.run();
    harness.get_by_label("Add tag").click();
    harness.run();
    assert!(harness.query_by_label("Place Tag Punchy").is_some(), "the section lists the new tag");
    // The add closes the menu.
    assert!(harness.query_by_label("New tag name").is_none(), "the menu went with the add");
    let tags = harness.state_mut().projects_mut().file_library_mut().tag_flat();
    let added = tags.iter().find(|t| t.name == "Punchy");
    assert_eq!(
        added.map(|t| t.color.as_str()),
        Some(suhina_library::store::named_colours()[0].1),
        "the colour the menu's own tag shows"
    );
    // Nothing carries it: the row is dimmed and the hover says so.
    harness.hover_at(harness.get_by_label("Place Tag Punchy").rect().center());
    harness.run();
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.contains("Punchy") && text.contains("0 item")),
        "{:?}",
        harness.state().status()
    );
    // A name one already has is an error, and adds nothing a second time.
    harness.get_by_label("Place Tag Punchy").click_secondary();
    harness.run();
    harness.get_by_label("New tag name").click();
    harness.run();
    harness.get_by_label("New tag name").type_text("Punchy");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert!(
        matches!(harness.state().status(), Status::Error(text) if text.contains("Punchy")),
        "{:?}",
        harness.state().status()
    );
    let count = harness.state_mut().projects_mut().file_library_mut().tags().len();
    assert_eq!(count, 8, "seven defaults and the one added");
    // From another tag's menu the colour is that one's own: Punchy's menu is not the next
    // one in the default cycle's order.
    harness.get_by_label("Place Tag Punchy").click_secondary();
    harness.run();
    harness.get_by_label("New tag name").click();
    harness.run();
    // The failed duplicate left "Punchy" in the field; Ctrl+A selects it, typing replaces it.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    harness.get_by_label("New tag name").type_text("Warm");
    harness.run();
    harness.get_by_label("Add tag").click();
    harness.run();
    let tags = harness.state_mut().projects_mut().file_library_mut().tag_flat();
    let punchy_color = tags.iter().find(|t| t.name == "Punchy").map(|t| t.color.clone()).unwrap_or_default();
    assert_ne!(Some(punchy_color.as_str()), Some(suhina_library::store::named_colours()[2].1));
    assert_eq!(
        tags.iter().find(|t| t.name == "Warm").map(|t| t.color.as_str()),
        Some(punchy_color.as_str()),
        "the menu's own tag's colour"
    );
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// The tag's right-click menu recolours it like the tag editor does: a palette swatch, or the
/// theme editor's picker opened in the menu by Select colour, where a picker of its own would
/// end the menu as a click outside it does (#1196).
#[test]
fn the_tags_right_menu_selects_a_colour() {
    let (settings, folder) = (temp("tags-colour-settings"), temp("tags-colour"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let item = folder.join("Loop 120bpm.wav").display().to_string();
    harness
        .state_mut()
        .projects_mut()
        .execute("library.tag", &json!({ "item": item.clone(), "tag": "Red", "on": true }))
        .unwrap();
    harness.run();
    // The menu lists the palette's swatches and Select colour; the picker comes when pressed.
    harness.get_by_label("Place Tag Red").click_secondary();
    harness.run();
    assert!(harness.query_by_label("Select colour").is_some(), "the menu offers it");
    harness.get_by_label("Select colour").click();
    harness.run();
    assert!(harness.query_by_label("Select colour…").is_some(), "the picker opened in the menu");
    // A palette swatch recolours the tag, in one command, and the menu stays open.
    let colour = suhina_ui::interaction::colour_picker::swatches()[3];
    harness.get_by_label(&format!("Colour {}", suhina_ui::interaction::colour_picker::hex(colour))).click();
    harness.run();
    let first = harness.state_mut().projects_mut().file_library_mut().tag_flat().swap_remove(0);
    assert_eq!(
        (first.name, first.color),
        ("Red".to_string(), theme::hex(suhina_ui::interaction::colour_picker::color32(colour)))
    );
    assert!(harness.query_by_label("Select colour…").is_some(), "the picker stays for a second look");
    // A second press closes it again.
    harness.get_by_label("Select colour…").click();
    harness.run();
    assert!(harness.query_by_label("Select colour…").is_none());
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// Ctrl-click on a tag adds it to the place's tags and takes it off again: the list shows
/// the items carrying any of them (#1196).
#[test]
fn ctrl_click_combines_tags_in_the_tags_section() {
    let (settings, folder) = (temp("tags-ctrl-settings"), temp("tags-ctrl"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let loop_file = folder.join("Loop 120bpm.wav").display().to_string();
    let groove = folder.join("Groove.mid").display().to_string();
    harness
        .state_mut()
        .projects_mut()
        .execute("library.tag", &json!({ "item": loop_file, "tag": "Red", "on": true }))
        .unwrap();
    harness
        .state_mut()
        .projects_mut()
        .execute("library.tag", &json!({ "item": groove, "tag": "Blue", "on": true }))
        .unwrap();
    harness.run();
    harness.get_by_label("Place Tag Red").click();
    harness.run();
    assert!(harness.query_by_label(&row("Loop 120bpm.wav")).is_some());
    assert!(harness.query_by_label(&row("Groove.mid")).is_none(), "only the red one yet");
    // Ctrl-click adds the second tag: the items with either of them.
    harness.get_by_label("Place Tag Blue").click_modifiers(Modifiers::COMMAND);
    harness.run();
    let Some(suhina_ui::browser::Place::Tag(tags)) = harness.state().browser.place.clone() else {
        panic!("the place is two tags, got {:?}", harness.state().browser.place);
    };
    let tag_names = harness.state_mut().projects_mut().file_library_mut().tag_flat();
    let names: Vec<String> =
        tags.iter().filter_map(|id| tag_names.iter().find(|t| t.id == *id).map(|t| t.name.clone())).collect();
    assert!(names == ["Red", "Blue"]);
    assert!(harness.query_by_label(&row("Loop 120bpm.wav")).is_some());
    assert!(harness.query_by_label(&row("Groove.mid")).is_some());
    assert!(harness.query_by_label(&row("Instruments")).is_none());
    // Ctrl-click takes the tag off again, and the place with the last one.
    harness.get_by_label("Place Tag Blue").click_modifiers(Modifiers::COMMAND);
    harness.run();
    harness.get_by_label("Place Tag Red").click_modifiers(Modifiers::COMMAND);
    harness.run();
    assert_eq!(harness.state().browser.place, None, "no tags left, no place");
    assert!(harness.query_by_label(&row("Instruments")).is_some());
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// Items dragged from the results onto a tag row carry the tag when dropped, the row lit
/// while they hover, as a drop on a folder row moves them (#1196).
#[test]
fn items_dropped_on_a_tag_carry_it() {
    let (settings, folder) = (temp("tags-drop-settings"), temp("tags-drop"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let name = folder.file_name().unwrap().to_string_lossy().to_string();
    double_click(&mut harness, &row(&name));
    harness.run();
    let item = folder.join("Loop 120bpm.wav").display().to_string();
    // The loop swept onto the sidebar's tag row is tagged there. Mid-drag the pointer says
    // what a drop does, status bar included.
    let from = harness.get_by_label(&row("Loop 120bpm.wav")).rect().center();
    let to = harness.get_by_label("Place Tag Red").rect().center();
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    for share in [0.1, 0.3, 0.6, 1.0] {
        harness.hover_at(from + (to - from) * share);
        harness.step();
    }
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.contains("Red") && text.contains("tag")),
        "{:?}",
        harness.state().status()
    );
    harness.drop_at(to);
    harness.step();
    harness.run();
    let tags = harness.state_mut().projects_mut().file_library_mut().item_tags(&item);
    assert_eq!(
        tags.iter()
            .map(|id| harness.state_mut().projects_mut().file_library_mut().tag_name_of(*id))
            .collect::<Vec<_>>(),
        [Some("Red".to_string())],
        "the drop tagged it"
    );
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// A row's Pin, Unpin and Tag act on the selection when the row is one of it, as Copy to folder
/// does ([#1254] review: unpinning six selected samples unpinned the one right-clicked).
#[test]
fn a_rows_pin_and_tag_act_on_the_selection() {
    let (settings, folder) = (temp("menu-selection-settings"), temp("menu-selection"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let name = folder.file_name().unwrap().to_string_lossy().to_string();
    double_click(&mut harness, &row(&name));
    harness.run();
    let looped = folder.join("Loop 120bpm.wav").display().to_string();
    let groove = folder.join("Groove.mid").display().to_string();
    for item in [&looped, &groove] {
        harness.state_mut().projects_mut().execute("library.pin", &json!({ "item": item })).unwrap();
    }
    harness.state_mut().browser.relist();
    harness.run();
    harness.get_by_label(&row("Loop 120bpm.wav")).click();
    harness.run();
    harness.get_by_label(&row("Groove.mid")).click_modifiers(Modifiers::COMMAND);
    harness.run();
    assert_eq!(harness.state().browser.selected().len(), 2, "both are selected");
    // Unpin from either row's menu takes both off.
    harness.get_by_label(&row("Loop 120bpm.wav")).click_secondary();
    harness.run();
    harness.get_by_label("Unpin 2 items").click();
    harness.run();
    assert!(harness.state_mut().projects_mut().file_library_mut().pinned().is_empty(), "both unpinned");
    // Tag sets the tag on both.
    harness.get_by_label(&row("Groove.mid")).click_secondary();
    harness.run();
    let at = harness.get_by_label("Tag").rect().center();
    harness.hover_at(at);
    harness.run();
    harness.get_by_label("Red").click();
    harness.run();
    for item in [&looped, &groove] {
        let tags = harness.state_mut().projects_mut().file_library_mut().item_tags(item);
        assert_eq!(tags, [1], "{item} carries Red");
    }
    // A row outside the selection keeps its menu to itself.
    harness.get_by_label(&row("Drums")).click_secondary();
    harness.run();
    assert!(harness.query_by_label("Pin").is_some(), "the row's own Pin");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// A folder of the results dragged onto a tag row carries the tag as a whole, a sub-folder and
/// the library's own folder alike ([#1254] review: a folder was tagged only from its menu).
#[test]
fn a_folder_dropped_on_a_tag_carries_it() {
    let (settings, folder) = (temp("folder-drop-settings"), temp("folder-drop"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let name = folder.file_name().unwrap().to_string_lossy().to_string();
    double_click(&mut harness, &row(&name));
    harness.run();
    let drums = folder.join("Drums").display().to_string();
    let root = folder.display().to_string();
    for (label, tag, item) in [(row("Drums"), "Red", &drums), (row(&name), "Orange", &root)] {
        let from = harness.get_by_label(&label).rect().center();
        let to = harness.get_by_label(&format!("Place Tag {tag}")).rect().center();
        drag(&mut harness, from, to);
        harness.run();
        let tags = harness.state_mut().projects_mut().file_library_mut().item_tags(item);
        let names: Vec<Option<String>> =
            tags.iter().map(|id| harness.state_mut().projects_mut().file_library_mut().tag_name_of(*id)).collect();
        assert_eq!(names, [Some(tag.to_string())], "{label} carries {tag}");
        assert!(harness.state().browser.drag.is_none(), "the drop ended the drag");
    }
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// A tag row dragged onto a folder of the results tags the folder as a whole, as a folder
/// dragged onto the tag does ([#1254] review: a folder row took no tag's drop).
#[test]
fn a_tag_dragged_onto_a_folder_tags_it() {
    let (settings, folder) = (temp("tag-on-folder-settings"), temp("tag-on-folder"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let name = folder.file_name().unwrap().to_string_lossy().to_string();
    double_click(&mut harness, &row(&name));
    harness.run();
    let drums = folder.join("Drums").display().to_string();
    let from = harness.get_by_label("Place Tag Red").rect().center();
    let to = harness.get_by_label(&row("Drums")).rect().center();
    drag(&mut harness, from, to);
    harness.run();
    let tags = harness.state_mut().projects_mut().file_library_mut().item_tags(&drums);
    assert_eq!(tags, [1], "the folder carries Red");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// The tagging the other way round: a tag row dragged onto an item of the results carries the
/// tag there, the row lit under it, and a drop on one of a selection tags them all (#1196).
#[test]
fn a_tag_dragged_onto_an_item_tags_it() {
    let (settings, folder) = (temp("tag-drag-settings"), temp("tag-drag"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let name = folder.file_name().unwrap().to_string_lossy().to_string();
    double_click(&mut harness, &row(&name));
    harness.run();
    let loop_item = folder.join("Loop 120bpm.wav").display().to_string();
    let kick = folder.join("Drums").join("Kick Hard.wav").display().to_string();
    let groove = folder.join("Groove.mid").display().to_string();
    // The tag swept from its sidebar row onto the loop carries Red there. Mid-drag the
    // pointer says what a drop does, status bar included.
    let from = harness.get_by_label("Place Tag Red").rect().center();
    let to = harness.get_by_label(&row("Loop 120bpm.wav")).rect().center();
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    for share in [0.1, 0.3, 0.6, 1.0] {
        harness.hover_at(from + (to - from) * share);
        harness.step();
    }
    assert!(
        matches!(
            harness.state().status(),
            Status::Hint(text) if text.contains("Loop 120bpm.wav") && text.contains("Red")
        ),
        "{:?}",
        harness.state().status()
    );
    harness.drop_at(to);
    harness.step();
    harness.run();
    let tags = harness.state_mut().projects_mut().file_library_mut().item_tags(&loop_item);
    assert_eq!(
        tags.iter()
            .map(|id| harness.state_mut().projects_mut().file_library_mut().tag_name_of(*id))
            .collect::<Vec<_>>(),
        [Some("Red".to_string())],
        "the drop tagged the loop"
    );
    // Both of the two rows selected: the drop on one of them tags the whole selection.
    harness.get_by_label(&row("Groove.mid")).click();
    harness.run();
    harness.get_by_label(&row("Loop 120bpm.wav")).click_modifiers(Modifiers::COMMAND);
    harness.run();
    let from = harness.get_by_label("Place Tag Blue").rect().center();
    let to = harness.get_by_label(&row("Loop 120bpm.wav")).rect().center();
    drag(&mut harness, from, to);
    harness.run();
    let library = harness.state_mut().projects_mut().file_library_mut();
    let flat = library.tag_flat();
    let has_blue = |library: &mut suhina_library::Library, key: &str| {
        library.item_tags(key).iter().any(|id| flat.iter().any(|t| t.id == *id))
    };
    let has_loop_blue = has_blue(library, &loop_item);
    let kick_carries = library.item_tags(kick.as_str());
    let has_groove_blue = has_blue(library, &groove);
    assert!(has_loop_blue && kick_carries.is_empty() && has_groove_blue, "the selection the row belongs to carries it");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// After a click on a row the arrow keys move through the list: down and up step, left goes up
/// to the folder or heading and closes it, right opens it again (#916).
#[test]
fn the_arrow_keys_move_through_the_list_after_a_click_on_a_row() {
    let (settings, folder) = (temp("arrows-settings"), temp("arrows"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.get_by_label(&row("Instruments")).click();
    harness.run();
    harness.get_by_label(&row("FM Synth")).click();
    harness.run();
    let selected = |h: &Harness<'static, App>| h.state().browser.selected().first().map(|s| s.name.clone());
    assert_eq!(selected(&harness).as_deref(), Some("FM Synth"));
    harness.key_press(Key::ArrowDown);
    harness.run();
    let next = selected(&harness);
    assert_ne!(next.as_deref(), Some("FM Synth"), "down steps to the next row");
    harness.key_press(Key::ArrowUp);
    harness.run();
    assert_eq!(selected(&harness).as_deref(), Some("FM Synth"));
    // Left goes up to the heading, a second left closes it, right opens it again.
    harness.key_press(Key::ArrowLeft);
    harness.run();
    assert_eq!(selected(&harness).as_deref(), Some("Instruments"));
    harness.key_press(Key::ArrowLeft);
    harness.run();
    assert!(harness.query_by_label(&row("FM Synth")).is_none(), "the heading closed");
    harness.key_press(Key::ArrowRight);
    harness.run();
    assert!(harness.query_by_label(&row("FM Synth")).is_some(), "the heading opened");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_sidebar_titles_collapse_reorder_and_keep_their_order() {
    let (settings, folder) = (temp("sections-settings"), temp("sections"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    // The section kinds in the store, in their order ([#1254] › Sidebar sections).
    fn order(harness: &mut Harness<'static, App>) -> Vec<SectionKind> {
        harness.state_mut().projects_mut().file_library_mut().sections().iter().map(|s| s.kind).collect()
    }
    assert_eq!(order(&mut harness), [SectionKind::Pinned, SectionKind::Tags, SectionKind::Library]);
    assert!(harness.query_by_label("Place Tag Red").is_some(), "the open section shows its tags");
    // Clicking the title closes the section; its rows go with the click.
    harness.get_by_label("Tags section").click();
    harness.run();
    assert!(harness.query_by_label("Place Tag Red").is_none(), "closed hides the tags");
    assert!(harness.query_by_label("Place Tag Orange").is_none(), "every tag row goes with the section");
    // The title's menu opens the section again.
    harness.get_by_label("Tags section").click_secondary();
    harness.run();
    harness.get_by_label("Open Tags").click();
    harness.run();
    assert!(harness.query_by_label("Place Tag Red").is_some(), "open again, the tags show");
    // Move up from the menu takes Tags above Pinned.
    harness.get_by_label("Tags section").click_secondary();
    harness.run();
    harness.get_by_label("Move up").click();
    harness.run();
    assert_eq!(
        order(&mut harness),
        [SectionKind::Tags, SectionKind::Pinned, SectionKind::Library],
        "the menu's move reorders the sections"
    );
    // The order and the open state are the library's own: a reopened window keeps them.
    drop(harness);
    let mut harness = window_reopened(&settings, &folder);
    harness.run();
    assert_eq!(
        order(&mut harness),
        [SectionKind::Tags, SectionKind::Pinned, SectionKind::Library],
        "the order survives the restart"
    );
    assert!(harness.query_by_label("Place Tag Red").is_some(), "the reopen kept Tags open where it was left");
    // Library last: its Move down is refused — the dimmed move does nothing — and the
    // first section's (Tags, from the move above) Move up is dimmed too.
    harness.get_by_label("Tags section").click_secondary();
    harness.run();
    harness.get_by_label("Move up").click();
    harness.run();
    assert_eq!(
        order(&mut harness),
        [SectionKind::Tags, SectionKind::Pinned, SectionKind::Library],
        "the first section's move up does nothing"
    );
    harness.get_by_label("Library section").click_secondary();
    harness.run();
    harness.get_by_label("Move down").click();
    harness.run();
    assert_eq!(
        order(&mut harness),
        [SectionKind::Tags, SectionKind::Pinned, SectionKind::Library],
        "the last section's move down does nothing"
    );
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_tags_titles_menu_adds_a_folder_and_the_drag_files_the_tag() {
    let (settings, folder) = (temp("section-menu-settings"), temp("section-menu"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    // The title's menu makes tags too ([#1254] review): palette, field, button. The folder
    // making it already had, and it keeps its place beside them.
    harness.get_by_label("Tags section").click_secondary();
    harness.run();
    assert!(harness.query_by_label("New folder").is_some(), "the title's menu offers the first folder");
    assert!(
        harness.query_by_label("New tag name").is_some() && harness.query_by_label("Select colour").is_some(),
        "the title's menu makes tags too ([#1254] review)"
    );
    // New folder puts a folder at the top level, named by its kind when left out.
    harness.get_by_label("New folder").click();
    harness.run();
    assert!(harness.query_by_label_contains("Folder Folder").is_some(), "the menu's folder lands in the section");
    let forest = harness.state_mut().projects_mut().file_library_mut().tags();
    let folders = forest
        .iter()
        .filter_map(|e| match e {
            suhina_library::TagEntry::Folder { folder, .. } => Some((folder.name.clone(), folder.id)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].0, "Folder", "the folder is named by its own kind when left out");
    // A default tag carried onto the folder's row files it inside ([#1254] › Tag folders).
    let at = harness.get_by_label("Place Tag Purple").rect().center();
    let to = harness.get_by_label_contains("Folder Folder").rect().center();
    harness.hover_at(at);
    harness.step();
    harness.drag_at(at);
    harness.step();
    for share in [0.3, 0.7, 1.0] {
        harness.hover_at(at + (to - at) * share);
        harness.step();
    }
    harness.drop_at(to);
    harness.run();
    let tree = harness.state_mut().projects_mut().file_library_mut().tags();
    let punched = tree.iter().any(|e| match e {
        suhina_library::TagEntry::Folder { tags, .. } => {
            suhina_library::store::tags_iter(tags).any(|t| t.name == "Purple")
        }
        _ => false,
    });
    assert!(punched, "the drop put the tag into the folder");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// The Tags title's menu makes a tag where the rows' menus do: typed beside the palette, the
/// store's next colour when no pick was made, a pick's colour when one was, the menu closed
/// by the add and a name one already has refused ([#1254] review).
#[test]
fn the_tags_titles_menu_adds_a_tag_in_a_colour() {
    let (settings, folder) = (temp("title-tag-settings"), temp("title-tag"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.get_by_label("Tags section").click_secondary();
    harness.run();
    // No pick: the named colours' next, the store's own dealing ([#1254]).
    harness.get_by_label("New tag name").click();
    harness.run();
    harness.get_by_label("New tag name").type_text("Punchy");
    harness.run();
    harness.get_by_label("Add tag").click();
    harness.run();
    assert!(harness.query_by_label("Place Tag Punchy").is_some(), "the section lists the title's tag");
    assert!(harness.query_by_label("New tag name").is_none(), "the add closes the title's menu");
    let tags = harness.state_mut().projects_mut().file_library_mut().tag_flat();
    // The store's dealing without a colour: the first named colour whose name no tag has
    // ([#1254]; "Punchy" names its own, so the dealing names nothing it makes here).
    let wanted = suhina_library::store::named_colours()
        .iter()
        .find(|(name, _)| tags.iter().all(|t| t.name != *name))
        .map(|(_, color)| *color)
        .expect("a named colour deals itself into every library");
    assert_eq!(
        tags.iter().find(|t| t.name == "Punchy").map(|t| t.color.as_str()),
        Some(wanted),
        "no pick takes the named colours' next"
    );
    // A pick from the palette is the colour the next tag comes out with, the way a folder's
    // menu colours its own.
    harness.get_by_label("Tags section").click_secondary();
    harness.run();
    harness.get_by_label("Select colour").click();
    harness.run();
    let colour = suhina_ui::interaction::colour_picker::swatches()[5];
    harness.get_by_label(&format!("Colour {}", suhina_ui::interaction::colour_picker::hex(colour))).click();
    harness.run();
    harness.get_by_label("New tag name").click();
    harness.run();
    harness.get_by_label("New tag name").type_text("Everything");
    harness.run();
    harness.get_by_label("Add tag").click();
    harness.run();
    let tags = harness.state_mut().projects_mut().file_library_mut().tag_flat();
    assert_eq!(
        tags.iter().find(|t| t.name == "Everything").map(|t| t.color.as_str()),
        Some(theme::hex(suhina_ui::interaction::colour_picker::color32(colour)).as_str()),
        "the pick colours the tag the title's menu made",
    );
    // The name one already has refuses, the add leaving the menu open for the correction.
    harness.get_by_label("Tags section").click_secondary();
    harness.run();
    harness.get_by_label("New tag name").click();
    harness.run();
    harness.get_by_label("New tag name").type_text("Punchy");
    harness.run();
    harness.get_by_label("Add tag").click();
    harness.run();
    assert!(
        matches!(harness.state().status(), Status::Error(text) if text.contains("Punchy")),
        "{:?}",
        harness.state().status()
    );
    // The add shows itself: over a closed section the menu's add opens it with its tag
    // ([#1254] review), as a folder's add opens the branch it lands in.
    harness.get_by_label("Close Tags").click();
    harness.run();
    assert!(harness.query_by_label("Place Tag Punchy").is_none(), "the section closed, its rows hidden");
    harness.get_by_label("Tags section").click_secondary();
    harness.run();
    harness.get_by_label("New tag name").click();
    harness.run();
    harness.get_by_label("New tag name").type_text("Warm");
    harness.run();
    harness.get_by_label("Add tag").click();
    harness.run();
    assert!(harness.query_by_label("Place Tag Warm").is_some(), "the add opened the section to show its tag");
    assert!(harness.query_by_label("New tag name").is_none(), "the menu closed with the add");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_plain_click_of_a_place_tag_shows_it_alone() {
    let (settings, folder) = (temp("click-alone-settings"), temp("click-alone"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    // Ctrl-click builds the place of two tags ([#1254]: items with any of them). The
    // modifiers are in state only between the events of these steps: a run in between
    // resets the report (as the skill's drops note), so press, release and reset go
    // without one.
    let control = |harness: &mut Harness<'static, App>, pos: Pos2| {
        harness.event(egui::Event::ModifiersChanged(Modifiers::COMMAND));
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::COMMAND,
            });
            harness.step();
        }
        harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
        harness.run();
    };
    let red = harness.get_by_label("Place Tag Red").rect().center();
    control(&mut harness, red);
    let orange = harness.get_by_label("Place Tag Orange").rect().center();
    control(&mut harness, orange);
    let place = |harness: &Harness<'static, App>| harness.state().browser.place.clone();
    assert_eq!(place(&harness), Some(Place::Tag(vec![1, 2])), "the two tags in one place");
    // A plain click of a tag the place holds shows its tag alone ([#1254] review).
    harness.get_by_label("Place Tag Red").click();
    harness.run();
    assert_eq!(place(&harness), Some(Place::Tag(vec![1])), "the clicked tag alone");
    // Its ctrl-click lets it go: the place was its own ([#1254] review).
    let red = harness.get_by_label("Place Tag Red").rect().center();
    control(&mut harness, red);
    assert_eq!(place(&harness), None, "the single tag's place lets go");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// Dragging a folder row onto another folder's lower half moves it inside ([#1254]); the
/// top half orders it before the folder, as the spec's *before another row* says ([#1254]
/// review).
#[test]
fn a_folder_drops_before_another_folder_at_its_top_half() {
    let (settings, folder) = (temp("folder-before-settings"), temp("folder-before"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().projects_mut().execute("library.addTagFolder", &json!({ "name": "Disco" })).unwrap();
    harness.run();
    harness.state_mut().projects_mut().execute("library.addTagFolder", &json!({ "name": "Boogie" })).unwrap();
    harness.run();
    let names = |harness: &mut Harness<'static, App>| {
        let tree = harness.state_mut().projects_mut().file_library_mut().tags();
        tree.iter()
            .filter_map(|e| match e {
                suhina_library::TagEntry::Folder { folder, .. } => Some(folder.name.clone()),
                _ => None,
            })
            .collect::<Vec<String>>()
    };
    assert_eq!(names(&mut harness), ["Disco", "Boogie"], "the folders lead in the forest's order");
    // The top half of Disco's row: the drag carries Boogie before it.
    let from = harness.get_by_label("Folder Boogie (0 tags)").rect().center();
    let to = harness.get_by_label("Folder Disco (0 tags)").rect();
    let top = egui::pos2(to.left() + 30.0, to.top() + 3.0);
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    for share in [0.3, 0.6, 1.0] {
        harness.hover_at(from + (top - from) * share);
        harness.step();
    }
    harness.drop_at(top);
    harness.run();
    assert_eq!(names(&mut harness), ["Boogie", "Disco"], "the top half put Boogie before Disco");
    // The lower half: the same drag lands Boogie inside Disco after a new folder row set.
    let boogie = harness.get_by_label_contains("Folder Boogie").rect().center();
    let to = harness.get_by_label("Folder Disco (0 tags)").rect();
    let bottom = egui::pos2(to.left() + 30.0, to.bottom() - 3.0);
    harness.hover_at(boogie);
    harness.step();
    harness.drag_at(boogie);
    harness.step();
    for share in [0.3, 0.6, 1.0] {
        harness.hover_at(boogie + (bottom - boogie) * share);
        harness.step();
    }
    harness.drop_at(bottom);
    harness.run();
    let tree = harness.state_mut().projects_mut().file_library_mut().tags();
    assert!(suhina_library::store::folders_iter(&tree).any(|f| f.name == "Disco"), "Disco stands at the top",);
    assert_eq!(names(&mut harness), ["Disco"], "Boogie moved into Disco, the top holds Disco alone");
    let tree = harness.state_mut().projects_mut().file_library_mut().tags();
    assert!(
        suhina_library::store::folder_children(
            &tree,
            suhina_library::store::folders_iter(&tree).find(|f| f.name == "Disco").map(|f| f.id).unwrap_or(0)
        )
        .unwrap_or(&[])
        .iter()
        .any(|e| e.name() == "Boogie"),
        "{tree:?}: Boogie is Disco's child after its lower-half drop",
    );
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_tag_drags_into_a_closed_folder() {
    let (settings, folder) = (temp("closed-folder-settings"), temp("closed-folder"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().projects_mut().execute("library.addTagFolder", &json!({ "name": "Drums" })).unwrap();
    harness.run();
    // The chevron row closes on its click ([#1254]).
    harness.get_by_label("Folder Drums (0 tags)").click();
    harness.run();
    let closed = harness.state_mut().projects_mut().file_library_mut().tags();
    assert!(
        closed.iter().any(|e| matches!(
            e,
            suhina_library::TagEntry::Folder { folder, .. } if folder.name == "Drums" && !folder.open
        )),
        "the row's click closed the folder"
    );
    // Red drags in though the folder is closed: the hint says it, and the drop lands
    // inside ([#1254] review › dragging into a closed folder).
    let at = harness.get_by_label("Place Tag Red").rect().center();
    let to = harness.get_by_label("Folder Drums (0 tags)").rect().center();
    harness.hover_at(at);
    harness.step();
    harness.drag_at(at);
    harness.step();
    harness.hover_at(to);
    harness.step();
    // The hint says where the drop goes — into the closed folder ([#1254] review).
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.contains("Move Red into Drums")),
        "{:?}",
        harness.state().status()
    );
    harness.drop_at(to);
    harness.run();
    let inside = harness.state_mut().projects_mut().file_library_mut().tags();
    let holds = inside.iter().any(|e| match e {
        suhina_library::TagEntry::Folder { folder: f, tags } => {
            f.name == "Drums" && suhina_library::store::tags_iter(tags).any(|t| t.name == "Red")
        }
        _ => false,
    });
    assert!(holds, "the closed folder took the tag in");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_picked_rows_menu_removes_every_picked_tag_and_folder() {
    let (settings, folder) = (temp("pick-remove-settings"), temp("pick-remove"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().projects_mut().execute("library.addTagFolder", &json!({ "name": "Drums" })).unwrap();
    harness.run();
    for label in ["Place Tag Red", "Place Tag Purple", "Folder Drums (0 tags)"] {
        harness.get_by_label(label).click_modifiers(Modifiers::SHIFT);
        harness.run();
    }
    harness.get_by_label("Place Tag Red").click_secondary();
    harness.run();
    assert!(harness.query_by_label_contains("2 tags and 1 folder picked").is_some(), "the menu is the pick's");
    harness.get_by_label("Remove 2 tags and 1 folder").click();
    harness.run();
    // Removing asks first only when a picked tag carries items; the asking button goes on.
    if let Some(confirm) = harness.query_by_label_contains("Remove 2 tags and 1 folder?") {
        confirm.click();
        harness.run();
    }
    let forest = harness.state_mut().projects_mut().file_library_mut().tags();
    let tags: Vec<String> = suhina_library::store::tags_iter(&forest).map(|t| t.name.clone()).collect();
    assert!(!tags.iter().any(|t| t == "Red" || t == "Purple"), "both picked tags went: {tags:?}");
    assert!(!tags.is_empty(), "the tags not picked stay");
    assert_eq!(suhina_library::store::folders_iter(&forest).count(), 0, "the picked folder went");
    assert!(harness.state().browser.tag_selection.is_empty(), "the pick went with them");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn shift_picked_tags_move_with_the_carry() {
    let (settings, folder) = (temp("shift-pick-settings"), temp("shift-pick"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().projects_mut().execute("library.addTagFolder", &json!({ "name": "Drums" })).unwrap();
    harness.run();
    // Shift-click picks Red and Purple beside each other ([#1254] review › multi-select).
    harness.get_by_label("Place Tag Red").click_modifiers(Modifiers::SHIFT);
    harness.run();
    harness.get_by_label("Place Tag Purple").click_modifiers(Modifiers::SHIFT);
    harness.run();
    // A drag of one of them lands both in the folder, in the pick's order.
    let at = harness.get_by_label("Place Tag Red").rect().center();
    let to = harness.get_by_label("Folder Drums (0 tags)").rect().center();
    harness.hover_at(at);
    harness.step();
    harness.drag_at(at);
    harness.step();
    for share in [0.4, 0.8, 1.0] {
        harness.hover_at(at + (to - at) * share);
        harness.step();
    }
    harness.drop_at(to);
    harness.run();
    let tree = harness.state_mut().projects_mut().file_library_mut().tags();
    let names = tree
        .iter()
        .filter_map(|e| match e {
            suhina_library::TagEntry::Folder { folder, tags } if folder.name == "Drums" => {
                Some(suhina_library::store::tags_iter(tags).map(|t| t.name.clone()).collect::<Vec<_>>())
            }
            _ => None,
        })
        .next()
        .unwrap_or_default();
    assert_eq!(names, vec!["Red", "Purple"], "both picked tags moved in the pick's order");
    // The pick lifted itself with the move.
    assert!(harness.state().browser.tag_selection.is_empty());
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_tags_menu_adds_a_folder_at_the_tags_own_level() {
    let (settings, folder) = (temp("tags-new-folder-settings"), temp("tags-new-folder"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().projects_mut().execute("library.addTagFolder", &json!({ "name": "Drums" })).unwrap();
    harness.run();
    // Red files inside Drums by drag ([#1254] › Tag folders).
    let at = harness.get_by_label("Place Tag Red").rect().center();
    let to = harness.get_by_label("Folder Drums (0 tags)").rect().center();
    harness.hover_at(at);
    harness.step();
    harness.drag_at(at);
    harness.step();
    for share in [0.4, 0.8, 1.0] {
        harness.hover_at(at + (to - at) * share);
        harness.step();
    }
    harness.drop_at(to);
    harness.run();
    // A right-click on the filed Red's menu makes a folder beside it: inside Drums, a
    // second level ([#1254] review › New folder on any tag). Then the menu needs the
    // section rendered around Red's row: Drums must be open for its row to show.
    let open_now =
        harness.state_mut().projects_mut().file_library_mut().set_section_open(suhina_library::SectionKind::Tags, true);
    let _ = open_now;
    harness.get_by_label("Place Tag Red").click_secondary();
    harness.run();
    harness.get_by_label("New folder").click();
    harness.run();
    let tree = harness.state_mut().projects_mut().file_library_mut().tags();
    let any_folder =
        suhina_library::store::folders_iter(&tree).any(|f| suhina_library::store::entry_depth(&tree, f.id) == 2);
    assert!(any_folder, "a folder made from Red's menu sits at Red's own level (under Drums)");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_section_title_drags_to_another_slot() {
    let (settings, folder) = (temp("section-drag-settings"), temp("section-drag"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let order = |harness: &mut Harness<'static, App>| -> Vec<SectionKind> {
        harness.state_mut().projects_mut().file_library_mut().sections().iter().map(|s| s.kind).collect()
    };
    assert_eq!(order(&mut harness), [SectionKind::Pinned, SectionKind::Tags, SectionKind::Library]);
    // Tags's title drags under Library's: the pointer's lower half of Library's title is the
    // slot ([#1254] review: a thin line shows the slot while it drags).
    let from = harness.get_by_label("Tags section").rect().center();
    let to = harness.get_by_label("Library section").rect().center();
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    for share in [0.4, 0.8, 1.0] {
        harness.hover_at(from + (to - from) * share);
        harness.step();
    }
    // Below the Library title's middle: the slot is under it.
    harness.drop_at(to + egui::vec2(0.0, 4.0));
    harness.run();
    assert_eq!(
        order(&mut harness),
        [SectionKind::Pinned, SectionKind::Library, SectionKind::Tags],
        "the dragged title landed under Library"
    );
    // The tree's own search reads the new order; no reset happens by hand.
    assert_eq!(order(&mut harness), [SectionKind::Pinned, SectionKind::Library, SectionKind::Tags]);
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_tag_drags_with_hints_into_a_subfolder() {
    let (settings, folder) = (temp("subfolder-hint-settings"), temp("subfolder-hint"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().projects_mut().execute("library.addTagFolder", &json!({ "name": "Drums" })).unwrap();
    harness.run();
    // A folder under Drums comes by command, the drag hints are what this test follows
    // ([#1254] › Tag folders).
    let drums_id = {
        let forest = harness.state_mut().projects_mut().file_library_mut().tags();
        forest
            .iter()
            .find_map(|e| match e {
                suhina_library::TagEntry::Folder { folder, .. } if folder.name == "Drums" => Some(folder.id),
                _ => None,
            })
            .unwrap_or(0)
    };
    harness
        .state_mut()
        .projects_mut()
        .execute("library.addTagFolder", &json!({ "name": "Folder", "parent": format!("#{drums_id}") }))
        .unwrap();
    harness.run();
    // Red into the subfolder: the hint names both the carried and its landing.
    let at = harness.get_by_label("Place Tag Red").rect().center();
    let to = harness.get_by_label_contains("Folder Folder").rect().center();
    harness.hover_at(at);
    harness.step();
    harness.drag_at(at);
    harness.step();
    harness.hover_at(to);
    harness.step();
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.contains("Move Red into Folder")),
        "{:?}",
        harness.state().status()
    );
    harness.drop_at(to);
    harness.run();
    let two = harness.state_mut().projects_mut().file_library_mut().tags();
    assert!(two.iter().any(|e| matches!(
        e,
        suhina_library::TagEntry::Folder { folder, .. } if folder.name == "Drums"
    ) && {
        // Its children hold the subfolder with Red inside it.
        if let Some(suhina_library::TagEntry::Folder { tags, .. }) = two.iter().find_map(|e| match e {
            suhina_library::TagEntry::Folder { folder, tags } if folder.name == "Drums" => {
                Some(suhina_library::TagEntry::Folder { folder: folder.clone(), tags: tags.clone() })
            }
            _ => None,
        }) {
            tags.iter().any(|e| {
                matches!(
                    e,
                    suhina_library::TagEntry::Folder { folder, tags } if folder.name == "Folder"
                        && suhina_library::store::tags_iter(tags).any(|t| t.name == "Red")
                )
            })
        } else {
            false
        }
    }),);
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_tag_drops_into_the_deepest_child_folder() {
    let (settings, folder) = (temp("deepest-drop-settings"), temp("deepest-drop"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    // The tree's folders by name: the chain's ids come from the store, not guesses.
    let id_of = |harness: &mut Harness<'static, App>, name: &str| -> u32 {
        let forest = harness.state_mut().projects_mut().file_library_mut().tags();
        suhina_library::store::folders_iter(&forest).find(|f| f.name == name).map(|f| f.id).unwrap_or(0)
    };
    // Four levels of folders, each under the one above ([#1254] › Tag folders).
    harness.state_mut().projects_mut().execute("library.addTagFolder", &json!({ "name": "One" })).unwrap();
    harness.run();
    let one = id_of(&mut harness, "One");
    harness
        .state_mut()
        .projects_mut()
        .execute("library.addTagFolder", &json!({ "name": "Two", "parent": format!("#{one}") }))
        .unwrap();
    harness.run();
    let two = id_of(&mut harness, "Two");
    harness
        .state_mut()
        .projects_mut()
        .execute("library.addTagFolder", &json!({ "name": "Three", "parent": format!("#{two}") }))
        .unwrap();
    harness.run();
    let three = id_of(&mut harness, "Three");
    harness
        .state_mut()
        .projects_mut()
        .execute("library.addTagFolder", &json!({ "name": "Four", "parent": format!("#{three}") }))
        .unwrap();
    harness.run();
    // Red carried onto the deepest folder's row lands in it: the cap counts folder levels,
    // a tag adds none ([#1254] review: the lowest folder took no tag before).
    let at = harness.get_by_label("Place Tag Red").rect().center();
    let to = harness.get_by_label_contains("Folder Four").rect().center();
    harness.hover_at(at);
    harness.step();
    harness.drag_at(at);
    harness.step();
    for share in [0.4, 0.8, 1.0] {
        harness.hover_at(at + (to - at) * share);
        harness.step();
    }
    harness.drop_at(to);
    harness.run();
    let forest = harness.state_mut().projects_mut().file_library_mut().tags();
    assert_eq!(
        suhina_library::store::entry_depth(&forest, 1),
        5,
        "Red put in the fourth level's folder, its row under it"
    );
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_tags_section_leads_with_its_folders() {
    let (settings, folder) = (temp("folder-first-settings"), temp("folder-first"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    // A folder made last still draws over the tags: each level's folders lead
    // ([#1254] review).
    harness.state_mut().projects_mut().execute("library.addTagFolder", &json!({ "name": "Present" })).unwrap();
    harness.run();
    let folders = harness.get_by_label("Folder Present (0 tags)").rect();
    let red = harness.get_by_label("Place Tag Red").rect();
    let grey = harness.get_by_label("Place Tag Grey").rect();
    assert!(folders.top() < red.top(), "the section's own folder rows come first ({:?} vs {:?})", folders, red);
    assert!(red.bottom() < grey.top(), "the tags keep their order below the folders");
    // A folder's own children: folders of the level lead, then the level's tags. The
    // folder's own row says what it holds ([#1254] review: the open row keeps its count).
    harness.state_mut().projects_mut().execute("library.moveTag", &json!({ "id": "#1", "folder": "#8" })).unwrap();
    harness.run();
    let child = harness.get_by_label("Place Tag Red").rect();
    let folder_now = harness.get_by_label("Folder Present (1 tag)").rect();
    assert!(folder_now.bottom() <= child.top(), "a level's tags sit under its folders");
    // Where the rows change level a few points of air sit between them, a sibling's rows
    // tight as ever ([#1254] review: the nesting reads like the Pinned section's groups).
    let gap = child.top() - folder_now.bottom();
    assert!(
        (suhina_ui::browser::browser_view::LEVEL_GAP - gap).abs() < 0.5,
        "a child's level takes its air after its folder ({gap:?})"
    );
    let orange = harness.get_by_label("Place Tag Orange").rect();
    let yellow = harness.get_by_label("Place Tag Yellow").rect();
    let back = orange.top() - child.bottom();
    assert!(
        (suhina_ui::browser::browser_view::LEVEL_GAP - back).abs() < 0.5,
        "the row the child's level returns to keeps its air too ({back:?})"
    );
    assert!((yellow.top() - orange.bottom()).abs() < 0.5, "a level's own siblings sit tight");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn adding_a_subfolder_opens_the_targets_branch() {
    let (settings, folder) = (temp("branch-open-settings"), temp("branch-open"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().projects_mut().execute("library.addTagFolder", &json!({ "name": "Cowboy" })).unwrap();
    harness.run();
    // Cowboy hides inside a closed folder ([#1254]: closed folders show no children).
    harness
        .state_mut()
        .projects_mut()
        .execute("library.setFolderOpen", &json!({ "folder": "#8", "open": false }))
        .unwrap();
    harness.run();
    assert!(harness.query_by_label_contains("Folder Belt").is_none(), "the closed target hides its children");
    // A folder added to the closed Cowboy's menu lands under it, and the branch opens.
    harness.get_by_label("Folder Cowboy (0 tags)").click_secondary();
    harness.run();
    // The menu's own "Add folder", not the section's "+ Add folder": whichever node sits
    // beside the clicked row (the higher one).
    let adds: Vec<_> = harness.get_all_by_label("Add folder").collect();
    let menu = adds.iter().map(|n| n.rect()).min_by_key(|rect| rect.top() as u32).map(|rect| rect.center()).unwrap();
    click_at(&mut harness, menu);
    harness.run();
    let tree = harness.state_mut().projects_mut().file_library_mut().tags();
    assert!(
        suhina_library::store::folders_iter(&tree).any(|f| f.name == "Cowboy" && f.open)
            && suhina_library::store::folders_iter(&tree).any(|f| suhina_library::store::entry_depth(&tree, f.id) == 2),
        "the new subfolder sits under Cowboy with the branch open"
    );
    assert!(
        harness.query_by_label_contains("Folder Folder").is_some(),
        "the subfolder shows without opening anything by hand"
    );
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_folders_menu_colours_a_new_tag_from_the_palette() {
    let (settings, folder) = (temp("folder-color-settings"), temp("folder-color"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().projects_mut().execute("library.addTagFolder", &json!({ "name": "Cowboy" })).unwrap();
    harness.run();
    // The folder's menu carries the palette, beside the field for the tag it makes.
    harness.get_by_label("Folder Cowboy (0 tags)").click_secondary();
    harness.run();
    assert!(harness.query_by_label("Select colour").is_some(), "the folder's menu offers the palette");
    // A pick is the new tag's colour, the same way a tag's own menu recolours.
    harness.get_by_label("Select colour").click();
    harness.run();
    let colour = suhina_ui::interaction::colour_picker::swatches()[5];
    harness.get_by_label(&format!("Colour {}", suhina_ui::interaction::colour_picker::hex(colour))).click();
    harness.run();
    harness.get_by_label("New tag name").click();
    harness.run();
    harness.get_by_label("New tag name").type_text("Everything");
    harness.run();
    harness.get_by_label("Add tag").click();
    harness.run();
    let tree = harness.state_mut().projects_mut().file_library_mut().tag_flat();
    let made = tree.iter().find(|t| t.name == "Everything");
    assert_eq!(
        made.map(|t| t.color.as_str()),
        Some(theme::hex(suhina_ui::interaction::colour_picker::color32(colour)).as_str()),
        "the pick colours the tag the folder's menu made",
    );
    // The add shows itself ([#1254] review): a tag made at a closed folder opens the
    // folder, the way a subfolder's making opens the branch it lands in.
    harness.get_by_label("Folder Cowboy (1 tag)").click();
    harness.run();
    assert!(harness.query_by_label("Place Tag Everything").is_none(), "the closed folder hides its tags");
    harness.get_by_label("Folder Cowboy (1 tag)").click_secondary();
    harness.run();
    harness.get_by_label("New tag name").click();
    harness.run();
    harness.get_by_label("New tag name").type_text("Second");
    harness.run();
    harness.get_by_label("Add tag").click();
    harness.run();
    assert!(harness.query_by_label("Place Tag Second").is_some(), "the add opened the folder to show its tag");
    let tree = harness.state_mut().projects_mut().file_library_mut().tags();
    assert!(
        tree.iter().any(|e| matches!(
            e,
            suhina_library::TagEntry::Folder { folder, .. } if folder.name == "Cowboy" && folder.open
        )),
        "the folder stands open after its add",
    );
    // A pick the menu closes without the add: the next add takes the named colours' next,
    // not the colour that was held ([#1254] review: the close drops the drafts).
    harness.get_by_label_contains("Folder Cowboy").click_secondary();
    harness.run();
    harness.get_by_label("Select colour").click();
    harness.run();
    let abandoned = suhina_ui::interaction::colour_picker::swatches()[3];
    harness.get_by_label(&format!("Colour {}", suhina_ui::interaction::colour_picker::hex(abandoned))).click();
    harness.run();
    harness.key_press(Key::Escape);
    harness.run();
    assert!(harness.query_by_label("Select colour").is_none(), "the closed menu went away");
    harness.get_by_label_contains("Folder Cowboy").click_secondary();
    harness.run();
    harness.get_by_label("New tag name").click();
    harness.run();
    harness.get_by_label("New tag name").type_text("Third");
    harness.run();
    harness.get_by_label("Add tag").click();
    harness.run();
    let tags = harness.state_mut().projects_mut().file_library_mut().tag_flat();
    let wanted = suhina_library::store::named_colours()
        .iter()
        .find(|(name, _)| tags.iter().all(|t| t.name != *name))
        .map(|(_, color)| *color)
        .expect("a named colour deals itself into every library");
    let third = tags.iter().find(|t| t.name == "Third").map(|t| t.color.clone()).unwrap_or_default();
    assert_eq!(third.as_str(), wanted, "the menu's close kept no pick for the tag it makes next");
    // The pick went with the tag: the next one under this menu takes the named colours' next.
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

// An open folder's own row keeps saying what it holds, and its chevron still takes its
// cortège in on Ctrl-click ([#1254] review: the padded row dropped them).
#[test]
fn an_open_folder_counts_its_descendants_and_ctrl_click_takes_them_in() {
    let (settings, folder) = (temp("open-folder-count-settings"), temp("open-folder-count"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let loop_item = folder.join("Loop 120bpm.wav").display().to_string();
    harness.state_mut().projects_mut().execute("library.addTagFolder", &json!({ "name": "Drums" })).unwrap();
    harness.state_mut().projects_mut().execute("library.moveTag", &json!({ "id": "#1", "folder": "#8" })).unwrap();
    harness
        .state_mut()
        .projects_mut()
        .execute("library.tag", &json!({ "item": loop_item, "tag": "Red", "on": true }))
        .unwrap();
    harness.run();
    assert!(harness.query_by_label("Folder Drums (1 tag)").is_some(), "the open row says what it holds");
    // Closed: the children hide, the count stays the whole cortège's.
    harness.get_by_label("Folder Drums (1 tag)").click();
    harness.run();
    assert!(harness.query_by_label("Place Tag Red").is_none(), "the closed folder hides its tag");
    assert!(harness.query_by_label("Folder Drums (1 tag)").is_some(), "the count kept what it holds");
    // Open again: Ctrl-click takes the descendant tags into the place, as a closed one's
    // whole click does.
    harness.get_by_label("Folder Drums (1 tag)").click();
    harness.run();
    harness.get_by_label("Folder Drums (1 tag)").click_modifiers(Modifiers::COMMAND);
    harness.run();
    let Some(suhina_ui::browser::Place::Tag(tags)) = harness.state().browser.place.clone() else {
        panic!("the place is Drums' tags, got {:?}", harness.state().browser.place);
    };
    assert_eq!(tags, [1], "Red came into the place");
    assert!(harness.query_by_label(&row("Loop 120bpm.wav")).is_some(), "the place lists what Red tags");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

// A drag of a tag that is not one of the picked files only the carried one: the pick
// follows a picked carry ([#1254] review).
#[test]
fn a_drag_of_an_unpicked_tag_files_only_the_carried() {
    let (settings, folder) = (temp("unpicked-settings"), temp("unpicked"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().projects_mut().execute("library.addTagFolder", &json!({ "name": "Drums" })).unwrap();
    harness.run();
    // Red and Purple are picked; Grey is carried into the folder.
    harness.get_by_label("Place Tag Red").click_modifiers(Modifiers::SHIFT);
    harness.run();
    harness.get_by_label("Place Tag Purple").click_modifiers(Modifiers::SHIFT);
    harness.run();
    let at = harness.get_by_label("Place Tag Grey").rect().center();
    let to = harness.get_by_label("Folder Drums (0 tags)").rect().center();
    drag(&mut harness, at, to);
    harness.run();
    let tree = harness.state_mut().projects_mut().file_library_mut().tags();
    let inside = tree.iter().any(|e| match e {
        suhina_library::TagEntry::Folder { folder, tags } if folder.name == "Drums" => {
            suhina_library::store::tags_iter(tags).map(|t| t.name.clone()).collect::<Vec<_>>() == ["Grey"]
        }
        _ => false,
    });
    assert!(inside, "only Grey filed in");
    // Red and Purple stayed where they were, and the pick with them.
    assert!(
        suhina_library::store::tag_by_id(&tree, 1).is_some() && suhina_library::store::tag_by_id(&tree, 6).is_some()
    );
    assert_eq!(
        harness.state().browser.tag_selection,
        [1, 6].to_vec(),
        "the pick is still there for the picked ones' own carry"
    );
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

// A move that goes halfway says both: what moved and the refusal that stopped the rest,
// and the pick stays for its next carry ([#1254] review).
#[test]
fn a_group_move_refused_partway_keeps_the_error_and_the_pick() {
    let (settings, folder) = (temp("partial-move-settings"), temp("partial-move"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let add = |harness: &mut Harness<'static, App>, name: &str, parent: Option<u32>| {
        let mut params = json!({ "name": name });
        if let Some(p) = parent {
            params["parent"] = json!(format!("#{p}"));
        }
        harness.state_mut().projects_mut().execute("library.addTagFolder", &params).unwrap();
        harness.run();
    };
    add(&mut harness, "One", None);
    let id_of = |harness: &mut Harness<'static, App>, name: &str| -> u32 {
        let forest = harness.state_mut().projects_mut().file_library_mut().tags();
        suhina_library::store::folders_iter(&forest).find(|f| f.name == name).map_or(0, |f| f.id)
    };
    let one = id_of(&mut harness, "One");
    add(&mut harness, "Two", Some(one));
    let two = id_of(&mut harness, "Two");
    add(&mut harness, "Three", Some(two));
    let three = id_of(&mut harness, "Three");
    add(&mut harness, "Four", Some(three));
    // Red is carried onto the fourth level's folder: Red fits (a tag reaches any folder),
    // Two would nest a fifth level: refused there.
    harness.get_by_label("Place Tag Red").click_modifiers(Modifiers::SHIFT);
    harness.run();
    harness.get_by_label("Folder Two (0 tags)").click_modifiers(Modifiers::SHIFT);
    harness.run();
    let at = harness.get_by_label("Place Tag Red").rect().center();
    let to = harness.get_by_label("Folder Four (0 tags)").rect().center();
    harness.hover_at(at);
    harness.step();
    harness.drag_at(at);
    harness.step();
    for share in [0.3, 0.7, 1.0] {
        harness.hover_at(at + (to - at) * share);
        harness.step();
    }
    // The release frame holds the landing's own reading: what moved, and the refusal
    // ([#1254] review). One event, one frame — drop_at's cursor removal would paint
    // another, and the folder's hover hint would cover the status by then.
    harness.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    let forest = harness.state_mut().projects_mut().file_library_mut().tags();
    let four = id_of(&mut harness, "Four");
    let red_in = suhina_library::store::folder_children(&forest, four)
        .is_some_and(|row| row.iter().any(|e| matches!(e, suhina_library::TagEntry::Tag(tag) if tag.name == "Red")));
    assert!(red_in, "Red moved into the deepest folder");
    let two = id_of(&mut harness, "Two");
    let one = id_of(&mut harness, "One");
    assert_eq!(
        suhina_library::store::find_entry_parent(&forest, two).map(|(p, _)| p),
        Some(Some(one)),
        "Two was refused, it stayed under One"
    );
    assert_eq!(harness.state().browser.tag_selection, [1, two].to_vec(), "the pick kept itself for the refused part");
    assert!(
        matches!(harness.state().status(), Status::Error(text) if text.contains("Moved 1 tag") && text.contains("refused")),
        "{:?}",
        harness.state().status()
    );
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

// A drop on a later tag of the level lands the carried one BEFORE that tag: taking the
// carried one out of its level with the old index put it behind it ([#1254] review).
#[test]
fn a_tag_dropped_on_a_later_tag_lands_before_it() {
    let (settings, folder) = (temp("before-settings"), temp("before"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    // Red onto Yellow: theforest order would be untouched if the drop moved nothing
    // (Red already sits before Orange), so the landed order has to say the move.
    let from = harness.get_by_label("Place Tag Red").rect().center();
    let to = harness.get_by_label("Place Tag Yellow").rect().center();
    drag(&mut harness, from, to);
    harness.run();
    let forest = harness.state_mut().projects_mut().file_library_mut().tags();
    let names: Vec<&str> = forest.iter().map(|e| e.name()).collect();
    assert_eq!(names[..3], ["Orange", "Red", "Yellow"], "Red before Yellow, past Orange: {names:?}");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

// The carry lands where the RELEASE is: a pointer that spent a frame over an item and let
// go over the sidebar moves the tag into its folder still ([#1254] review).
#[test]
fn a_tag_landed_back_on_the_sidebar_moves_even_after_an_item_dropped_it() {
    let (settings, folder) = (temp("release-sidebar-settings"), temp("release-sidebar"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let name = folder.file_name().unwrap().to_string_lossy().to_string();
    double_click(&mut harness, &row(&name));
    harness.run();
    harness.state_mut().projects_mut().execute("library.addTagFolder", &json!({ "name": "Drums" })).unwrap();
    harness.run();
    // The carry goes Red, sweeps over the loop (the item row the drop would tag), then
    // back to Drums' row, let go there the frame it arrives ([#1254] review).
    let from = harness.get_by_label("Place Tag Red").rect().center();
    let over_item = harness.get_by_label(&row("Loop 120bpm.wav")).rect().center();
    let back = harness.get_by_label("Folder Drums (0 tags)").rect().center();
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(over_item);
    harness.step();
    // The move back and the release are the one frame the pointer lands back there.
    harness.hover_at(back);
    harness.drop_at(back);
    harness.step();
    harness.run();
    let forest = harness.state_mut().projects_mut().file_library_mut().tags();
    let holds = forest.iter().any(|e| match e {
        suhina_library::TagEntry::Folder { folder, tags } => {
            folder.name == "Drums" && suhina_library::store::tags_iter(tags).any(|t| t.name == "Red")
        }
        _ => false,
    });
    assert!(holds, "the release over the folder moved Red, whatever the frame before carried");
    let loop_item = folder.join("Loop 120bpm.wav").display().to_string();
    let library = harness.state_mut().projects_mut().file_library_mut();
    assert!(library.item_tags(&loop_item).is_empty(), "no item got tagged on a sidebar release");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

// The carry lands where the RELEASE is: a drop onto an item tags that item alone, over
// any sidebar target it crossed leaving ([#1254] review).
#[test]
fn a_tag_dropped_on_an_item_the_frame_it_crosses_tags_the_item_only() {
    let (settings, folder) = (temp("cross-item-settings"), temp("cross-item"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let name = folder.file_name().unwrap().to_string_lossy().to_string();
    double_click(&mut harness, &row(&name));
    harness.run();
    // The carry is armed at Red, swept onto the loop, and let go there.
    let from = harness.get_by_label("Place Tag Red").rect().center();
    let on_item = harness.get_by_label(&row("Loop 120bpm.wav")).rect().center();
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(on_item);
    harness.step();
    // The release: the exact frame the pointer crossed onto the item and let go.
    harness.event(egui::Event::PointerButton {
        pos: on_item,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    harness.run();
    let loop_item = folder.join("Loop 120bpm.wav").display().to_string();
    let library = harness.state_mut().projects_mut().file_library_mut();
    assert!(!library.item_tags(&loop_item).is_empty(), "the drop tagged the item the release crossed onto");
    // The tag did not half-move too: the tags' own list is untouched.
    let forest = library.tags();
    assert_eq!(forest.first().map(suhina_library::TagEntry::name), Some("Red"), "{forest:?}");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

// A typed rename commits when the user closes the menu with the same dot's own click,
// before the draft can be dropped with the menu ([#1254] review).
#[test]
fn the_footer_renames_when_its_own_dot_is_clicked_again() {
    let (settings, folder) = (temp("footer-rename-click-settings"), temp("footer-rename-click"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.get_by_label("Tag Red").click_secondary();
    harness.run();
    harness.get_by_label("Rename tag Red").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Rename tag Red").type_text("Rose");
    harness.run();
    // The dot's own click: the menu closes for it, and the typed name runs with it.
    harness.get_by_label("Tag Red").click();
    harness.run();
    let library = harness.state_mut().projects_mut().file_library_mut();
    let names: Vec<String> = library.tag_flat().into_iter().map(|t| t.name).collect();
    assert!(names.contains(&"Rose".to_string()) && !names.contains(&"Red".to_string()), "{names:?}");
    assert!(harness.query_by_label("Tag Rose").is_some(), "the dot itself says the new name");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

// The footer's rename survives another menu opening over it: the right-click that opened
// that menu closed the footer's before its body could run, and commits the typed name as
// the outside click it is ([#1254] review).
#[test]
fn the_footers_rename_survives_another_menu_opening_over_it() {
    let (settings, folder) = (temp("footer-rename-row-settings"), temp("footer-rename-row"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.get_by_label("Tag Red").click_secondary();
    harness.run();
    harness.get_by_label("Rename tag Red").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Rename tag Red").type_text("Rose");
    harness.run();
    // A right-click on another row: its menu takes the footer's place, and the rename
    // goes with the click all the same.
    harness.get_by_label("Place Tag Orange").click_secondary();
    harness.run();
    let library = harness.state_mut().projects_mut().file_library_mut();
    let names: Vec<String> = library.tag_flat().into_iter().map(|t| t.name).collect();
    assert!(names.contains(&"Rose".to_string()) && !names.contains(&"Red".to_string()), "{names:?}");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

// The display leads a level with its folders, tags under them: a tag dropped on an open
// folder's top half cannot show before the folder, so it lands above the tags instead, and
// the status says where it landed ([#1254] review).
#[test]
fn a_tag_dropped_on_a_folders_top_half_lands_above_the_tags() {
    let (settings, folder) = (temp("tag-above-tags-settings"), temp("tag-above-tags"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().projects_mut().execute("library.addTagFolder", &json!({ "name": "Box" })).unwrap();
    harness.run();
    harness.state_mut().projects_mut().execute("library.setTag", &json!({ "name": "Alpha" })).unwrap();
    harness.run();
    harness.state_mut().projects_mut().execute("library.setTag", &json!({ "name": "Beta" })).unwrap();
    harness.run();
    let top = |harness: &mut Harness<'static, App>| {
        let library = harness.state_mut().projects_mut().file_library_mut();
        library.tags().iter().map(|e| e.name().to_string()).collect::<Vec<String>>()
    };
    // The seven named colours lead the top level; the folder comes after them, the tags
    // after it.
    assert_eq!(&top(&mut harness)[7..], ["Box", "Alpha", "Beta"], "the folder leads, the tags follow");
    let from = harness.get_by_label("Place Tag Beta").rect().center();
    let to = harness.get_by_label("Folder Box (0 tags)").rect();
    let above = egui::pos2(to.left() + 30.0, to.top() + 3.0);
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    for share in [0.3, 0.6, 1.0] {
        harness.hover_at(from + (above - from) * share);
        harness.step();
    }
    harness.drop_at(above);
    harness.run();
    // "Above the tags" is the tags block's head: Red's slot at the level's first tag,
    // with the seven named colours before nothing — Beta goes before Red, and the
    // folder stays first of the level.
    assert_eq!(
        top(&mut harness),
        ["Beta", "Red", "Orange", "Yellow", "Green", "Blue", "Purple", "Grey", "Box", "Alpha"],
        "Beta above the tags, the folder still first",
    );
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

/// The footer's dot menu adds a tag of its own, and the add shows itself: the tags section
/// closed stays reachable from the footer, which opens with the tag it made ([#1254] review).
#[test]
fn the_footers_add_opens_the_closed_section() {
    let (settings, folder) = (temp("footer-add-settings"), temp("footer-add"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    // The section closed, its rows hidden, the footer's dots still at hand.
    harness.get_by_label("Tags section").click();
    harness.run();
    assert!(harness.query_by_label("Place Tag Red").is_none(), "the closed section hides its rows");
    harness.get_by_label("Tag Red").click_secondary();
    harness.run();
    harness.get_by_label("New tag name").click();
    harness.run();
    harness.get_by_label("New tag name").type_text("Warm");
    harness.run();
    harness.get_by_label("Add tag").click();
    harness.run();
    assert!(harness.query_by_label("Place Tag Warm").is_some(), "the add opened the section to show its tag");
    assert!(harness.query_by_label("New tag name").is_none(), "the menu closed with the add");
    let tags = harness.state_mut().projects_mut().file_library_mut().tag_flat();
    assert_eq!(tags.iter().filter(|t| t.name == "Warm").count(), 1, "added once");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

// The tags past the footer's room are no dots, but their filters stay: the control where
// the next dot would have been lists them ([#1254] review).
#[test]
fn the_footer_reaches_the_tags_its_dots_do_not_draw() {
    let (settings, folder) = (temp("footer-overflow-settings"), temp("footer-overflow"));
    pack(&folder);
    // The full-size window fits the seven first tags: no control.
    let mut harness = window(&settings, &folder);
    assert!(harness.query_by_label("More tags").is_none(), "the seven first tags draw as dots");
    // Forty-five more, saved with the library.
    for n in 8..53 {
        harness.state_mut().projects_mut().execute("library.setTag", &json!({ "name": format!("Extra {n}") })).unwrap();
    }
    drop(harness);
    let mut harness = window_at_size(&settings, &folder, (700.0, 800.0), false);
    // The narrow footer draws the first dots and the control for the rest.
    assert!(harness.query_by_label("More tags").is_some(), "the narrow footer announced the rest");
    harness.get_by_label("More tags").click();
    harness.run();
    let last = {
        let library = harness.state_mut().projects_mut().file_library_mut();
        library.tag_flat().into_iter().last().map(|t| (t.id, t.name)).unwrap_or_default()
    };
    // The hidden tags list, each under its own name as the dot would say it. Past ten
    // rows the menu scrolls its own room: the last tag is brought into its view
    // ([#1254] review), the way a user reads them by wheel.
    assert!(harness.query_by_label(&format!("Tag {}", last.1)).is_some(), "the last tag reached");
    harness.get_by_label(&format!("Tag {}", last.1)).scroll_to_me();
    harness.run();
    harness.get_by_label(&format!("Tag {}", last.1)).click();
    harness.run();
    let on = harness.state().browser.tag_filter.clone();
    assert_eq!(on, [last.0].to_vec(), "the hidden tag's filter runs from the menu");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

// A tagged folder lists in its tag's place as a folder, and a double-click opens it as the
// place ([#1254] review: the tag's row counted it, the place said no item carried the tag).
#[test]
fn a_tagged_folder_lists_in_its_tags_place_and_opens_there() {
    let (settings, folder) = (temp("tagged-folder-settings"), temp("tagged-folder"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let drums = folder.join("Drums").display().to_string();
    let tagging = json!({ "item": drums, "tag": "Red", "on": true });
    harness.state_mut().projects_mut().execute("library.tag", &tagging).unwrap();
    harness.state_mut().browser.relist();
    harness.run();
    harness.get_by_label("Place Tag Red").click();
    harness.run();
    assert!(harness.query_by_label(&row("Drums")).is_some(), "the tagged folder lists");
    double_click(&mut harness, &row("Drums"));
    harness.run();
    let Some(Place::Folder(opened)) = harness.state().browser.place.clone() else {
        panic!("the double-click opened no folder: {:?}", harness.state().browser.place);
    };
    assert!(opened.ends_with("Drums"), "{opened:?}");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

// A heading is no item: a right-click on it opens no menu, where it opened an empty one, and
// an item's right-click still opens its own ([#1254] review).
#[test]
fn a_headings_right_click_opens_no_menu() {
    let (settings, folder) = (temp("heading-menu-settings"), temp("heading-menu"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.get_by_label(&row("Instruments")).click_secondary();
    harness.run();
    assert!(harness.query_by_label("Pin").is_none() && harness.query_by_label("Tag").is_none(), "no menu");
    harness.key_press(Key::Escape);
    harness.run();
    harness.get_by_label(&row("Instruments")).click();
    harness.run();
    harness.get_by_label(&row("FM Synth")).click_secondary();
    harness.run();
    assert!(harness.query_by_label("Pin").is_some(), "an instrument's own menu opens");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

// The pick and the carry hold the sidebar's rows, folders among them: an agent's change keeps
// a folder that still is, and a command closing a folder keeps the tags it hides picked
// ([#1254] review: the reconcile kept tags alone, and the rows' prune dropped what a closed
// folder hides).
#[test]
fn the_pick_and_the_carry_keep_live_folders_and_hidden_tags() {
    let (settings, folder) = (temp("pick-live-settings"), temp("pick-live"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    let made = harness.state_mut().projects_mut().execute("library.addTagFolder", &json!({ "name": "Box" })).unwrap();
    let box_id = made["folder"].as_u64().and_then(|id| u32::try_from(id).ok()).unwrap();
    let boxed = json!({ "id": "#1", "folder": format!("#{box_id}") });
    harness.state_mut().projects_mut().execute("library.moveTag", &boxed).unwrap();
    harness.run();
    assert!(harness.query_by_label("Place Tag Red").is_some(), "Red shows inside the open Box");
    // Red and Box picked, Box carried; an agent's change elsewhere keeps them.
    harness.state_mut().browser.tag_selection = vec![1, box_id];
    harness.state_mut().browser.drag_tag_id = Some(box_id);
    harness.state_mut().browser.drag_tag = Some("Box".into());
    harness.state_mut().projects_mut().execute("library.setTag", &json!({ "name": "Elsewhere" })).unwrap();
    harness.state_mut().note_outside_change();
    assert_eq!(harness.state().browser.tag_selection, [1, box_id], "the folder stays picked");
    assert_eq!(harness.state().browser.drag_tag_id, Some(box_id), "the folder's carry goes on");
    harness.state_mut().browser.drag_tag_id = None;
    harness.state_mut().browser.drag_tag = None;
    // A command closes Box: Red's row hides, and Red stays picked.
    let closed = json!({ "folder": format!("#{box_id}"), "open": false });
    harness.state_mut().projects_mut().execute("library.setFolderOpen", &closed).unwrap();
    harness.state_mut().note_outside_change();
    harness.run();
    assert!(harness.query_by_label("Place Tag Red").is_none(), "the closed folder hides Red");
    assert_eq!(harness.state().browser.tag_selection, [1, box_id], "the hidden tag stays picked");
    // A folder the library dropped leaves the pick; its tag moved up and stays.
    let removed = json!({ "folder": format!("#{box_id}") });
    harness.state_mut().projects_mut().execute("library.removeTagFolder", &removed).unwrap();
    harness.state_mut().note_outside_change();
    harness.run();
    assert_eq!(harness.state().browser.tag_selection, [1]);
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

// A replaced library's tag ids are its own: the place of tags, the filter and the pick go
// with the old one, not onto whatever tag the new one gives those numbers ([#1254] review).
#[test]
fn a_replaced_library_takes_the_tag_place_filter_and_pick_with_it() {
    let (settings, folder, other) = (temp("replaced-settings"), temp("replaced"), temp("replaced-other-settings"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().browser.place = Some(Place::Tag(vec![1]));
    harness.state_mut().browser.tag_filter = vec![2];
    harness.state_mut().browser.tag_selection = vec![3];
    // A section drag mid-flight ends with its library too: its order was the old one's
    // ([#1254] review).
    harness.state_mut().browser.drag_section = Some(SectionKind::Tags);
    harness.state_mut().browser.drop_section = Some(SectionKind::Library);
    harness.state_mut().set_file_library(Library::new(Some(other.clone())));
    harness.run();
    let browser = &harness.state().browser;
    assert_eq!(browser.place, None, "the tag place went with its library");
    assert!(browser.tag_filter.is_empty() && browser.tag_selection.is_empty(), "the filter and the pick too");
    assert!(
        browser.drag_section.is_none() && browser.drop_section.is_none(),
        "the section carry went with its library",
    );
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
    let _ = std::fs::remove_dir_all(&other);
}

// A tag menu open when the library is replaced goes with the old library: its typed rename
// does not run on the replacement's tag of the same id ([#1254] review).
#[test]
fn a_replaced_library_leaves_an_open_tag_menu_behind() {
    let (settings, folder, other) =
        (temp("menu-replaced-settings"), temp("menu-replaced"), temp("menu-replaced-other"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.get_by_label("Place Tag Red").click_secondary();
    harness.run();
    harness.get_by_label("Rename tag Red").click();
    harness.run();
    harness.get_by_label("Rename tag Red").type_text("dish");
    harness.run();
    // The typed name waits for Enter or a click ([#1250] review): the replacement comes first.
    let old = harness.state_mut().projects_mut().file_library_mut().tag_flat();
    assert!(old.iter().any(|t| t.name == "Red"), "typing alone renames nothing: {old:?}");
    harness.state_mut().set_file_library(Library::new(Some(other.clone())));
    harness.run();
    harness.run();
    let new = harness.state_mut().projects_mut().file_library_mut().tag_flat();
    assert_eq!(new.first().map(|t| t.name.as_str()), Some("Red"), "the replacement's tag keeps its name: {new:?}");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
    let _ = std::fs::remove_dir_all(&other);
}

// The Tags title's menu goes with a replaced library too: its typed tag name is not made in
// the replacement ([#1254] review).
#[test]
fn a_replaced_library_leaves_the_tags_titles_menu_behind() {
    let (settings, folder, other) =
        (temp("title-replaced-settings"), temp("title-replaced"), temp("title-replaced-other"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.get_by_label("Tags section").click_secondary();
    harness.run();
    harness.get_by_label("New tag name").click();
    harness.run();
    harness.get_by_label("New tag name").type_text("Stale");
    harness.run();
    harness.state_mut().set_file_library(Library::new(Some(other.clone())));
    harness.run();
    assert!(harness.query_by_label("New tag name").is_none(), "the old library's menu closed");
    harness.get_by_label("Tags section").click_secondary();
    harness.run();
    harness.get_by_label("Add tag").click();
    harness.run();
    let tags = harness.state_mut().projects_mut().file_library_mut().tag_flat();
    assert!(!tags.iter().any(|t| t.name == "Stale"), "no draft of the old menu made a tag: {tags:?}");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
    let _ = std::fs::remove_dir_all(&other);
}

// A carried tag renamed meanwhile (by an agent) carries its new name, and its drop names it by
// its id ([#1254] review: the old name could reach another tag or none).
#[test]
fn a_carried_tag_renamed_meanwhile_carries_its_new_name() {
    let (settings, folder) = (temp("carry-rename-settings"), temp("carry-rename"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.state_mut().browser.drag_tag_id = Some(1);
    harness.state_mut().browser.drag_tag = Some("Red".into());
    let rename = json!({ "id": "#1", "rename": "Crimson" });
    harness.state_mut().projects_mut().execute("library.setTag", &rename).unwrap();
    harness.state_mut().note_outside_change();
    assert_eq!(harness.state().browser.drag_tag.as_deref(), Some("Crimson"));
    assert_eq!(harness.state().browser.drag_tag_id, Some(1), "the carry goes on");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

// The Tags title's New folder shows its folder as Add tag shows its tag: a closed section
// opens for it ([#1254] review).
#[test]
fn the_tags_titles_new_folder_opens_a_closed_section() {
    let (settings, folder) = (temp("title-folder-settings"), temp("title-folder"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    harness.get_by_label("Tags section").click();
    harness.run();
    assert!(harness.query_by_label("Place Tag Red").is_none(), "the section closed");
    harness.get_by_label("Tags section").click_secondary();
    harness.run();
    harness.get_by_label("New folder").click();
    harness.run();
    let sections = harness.state_mut().projects_mut().file_library_mut().sections();
    assert!(sections.iter().any(|s| s.kind == SectionKind::Tags && s.open), "the add opened the section");
    let forest = harness.state_mut().projects_mut().file_library_mut().tags();
    let folders: Vec<String> = suhina_library::store::folders_iter(&forest).map(|f| f.name.clone()).collect();
    assert_eq!(folders.len(), 1, "one folder made: {folders:?}");
    assert!(harness.query_by_label("Place Tag Red").is_some(), "the section's rows show again");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

// A tag added below many others shows itself: the sidebar scrolls its row into view
// ([#1254] review: the status said it was added, and the row stayed under the bottom edge).
#[test]
fn an_added_tag_scrolls_into_view() {
    let (settings, folder) = (temp("reveal-settings"), temp("reveal"));
    pack(&folder);
    let mut harness = window_at_size(&settings, &folder, (1280.0, 600.0), true);
    for n in 8..53 {
        let params = json!({ "name": format!("Extra {n}") });
        harness.state_mut().projects_mut().execute("library.setTag", &params).unwrap();
    }
    harness.state_mut().browser.relist();
    harness.run();
    assert_eq!(harness.state().browser.sidebar_scroll, 0.0, "the sidebar starts at its top");
    harness.get_by_label("Tags section").click_secondary();
    harness.run();
    harness.get_by_label("New tag name").click();
    harness.run();
    harness.get_by_label("New tag name").type_text("Newest");
    harness.run();
    harness.get_by_label("Add tag").click();
    harness.run();
    assert!(harness.state().browser.sidebar_scroll > 0.0, "the sidebar scrolled to the new row");
    // The whole row shows, not a sliver of it at the edge ([#1254] review).
    let row = harness.get_by_label("Place Tag Newest").rect();
    assert!(row.height() >= theme::BROWSER_ROW - 0.5 && row.bottom() <= 600.0, "the new row shows: {row:?}");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

// A tag the footer has no room for keeps its sidebar menu: the footer's menus keep their
// state under keys of their own ([#1254] review: the overflow cleared the sidebar menu's
// picker and new-tag draft every frame).
#[test]
fn a_tag_in_the_footers_overflow_keeps_its_sidebar_menu() {
    let (settings, folder) = (temp("overflow-menu-settings"), temp("overflow-menu"));
    pack(&folder);
    let mut harness = window(&settings, &folder);
    for n in 8..53 {
        let params = json!({ "name": format!("Extra {n}") });
        harness.state_mut().projects_mut().execute("library.setTag", &params).unwrap();
    }
    drop(harness);
    let mut harness = window_at_size(&settings, &folder, (700.0, 800.0), false);
    assert!(harness.query_by_label("More tags").is_some(), "the last tags are in the footer's overflow");
    let last = harness.state_mut().projects_mut().file_library_mut().tag_flat().into_iter().last().map(|t| t.id);
    let last = last.unwrap_or(0);
    // What the sidebar row's menu holds while it is open for the last tag: a rename and a
    // new tag typed in, and its Select colour picker open. The overflow's cleanup runs every
    // frame the tag has no dot; it reaches the footer's own keys only, so these stay.
    let epoch = harness.state().browser.tag_epoch;
    let rename = egui::Id::unique(("browser-rename-tag", epoch, last));
    let draft = egui::Id::unique(("browser-section-new-tag", epoch, last));
    let picker = egui::Id::unique(("browser-tag-colour", epoch, last));
    harness.ctx.data_mut(|d| {
        d.insert_temp(rename, "Renamed".to_string());
        d.insert_temp(draft, "Draft".to_string());
        d.insert_temp(picker, true);
    });
    harness.run();
    harness.run();
    let (renamed, drafted, open) =
        harness.ctx.data(|d| (d.get_temp::<String>(rename), d.get_temp::<String>(draft), d.get_temp::<bool>(picker)));
    assert_eq!(renamed.as_deref(), Some("Renamed"), "the sidebar menu's rename stays");
    assert_eq!(drafted.as_deref(), Some("Draft"), "the sidebar menu's new tag stays");
    assert_eq!(open, Some(true), "the sidebar menu's picker stays open");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&folder);
}

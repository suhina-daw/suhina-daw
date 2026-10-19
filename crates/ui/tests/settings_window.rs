// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Settings window: each page scrolls to its last setting in a short window
//! (`docs/spec/ui-settings.md`).

use egui::{Key, Modifiers, Pos2, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::settings::settings_view::Page;
use suhina_ui::{App, kit::theme};

/// A window too short for most pages.
const SHORT: (f32, f32) = (1000.0, 420.0);

fn short_window() -> Harness<'static, App> {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = Harness::builder()
        .with_size(SHORT)
        .with_step_dt(0.25)
        .with_max_steps(4)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness
}

/// Runs frames until the window rests; the Audio page repaints all the time, so it never does.
fn settle(harness: &mut Harness<'static, App>) {
    let _ = harness.run_ok();
}

/// Whether the control named `label` is inside the window, under the Settings title.
fn shown(harness: &Harness<'static, App>, label: &str) -> bool {
    let title = harness.get_by_label("Close settings").rect();
    let rect = harness.get_by_label(label).rect();
    rect.top() > title.bottom() && rect.bottom() < SHORT.1
}

/// Turns the wheel over the page until `label` shows, at most `turns` times; returns how often.
fn wheel_to(harness: &mut Harness<'static, App>, label: &str, turns: usize) -> usize {
    let title = harness.get_by_label("Close settings").rect();
    // The middle of the page, right of the list of pages.
    let over = Pos2::new(title.right() - 200.0, (title.bottom() + SHORT.1) / 2.0);
    for turn in 0..turns {
        if shown(harness, label) {
            return turn;
        }
        harness.hover_at(over);
        harness.step();
        harness.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, -80.0),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        settle(harness);
    }
    turns
}

#[test]
fn every_page_scrolls_to_its_last_setting_in_a_short_window() {
    let mut harness = short_window();
    // Each page, and the control at its end (MIDI has none with no controller connected).
    let pages = [
        ("General", Some("Warp mode: Re-pitch")),
        ("Audio", Some("List the audio devices again")),
        ("MIDI", None),
        ("Plug-ins", Some("Open the plug-in manager")),
        ("Arrangement", Some("Track headers: Right")),
        ("Session and mixer", Some("Mixer controls: Knob")),
        ("Clip editor", Some("Double-click on a note: Nothing")),
        ("Transport", Some("Stop: Stay")),
        ("Detail area", Some("Detail area layout: Full width")),
        ("Browser", Some("Tag limit: 1024")),
        ("Appearance", Some("Renderer: OpenGL")),
        ("Theme", Some("Export the theme template")),
        ("Shortcuts", Some("Change keys of file.new")),
    ];
    let names: Vec<&str> = Page::ALL.iter().map(|(_, name)| *name).collect();
    assert_eq!(pages.map(|(page, _)| page).to_vec(), names, "every page is checked");
    for (page, last) in pages {
        harness.get_by_label(&format!("Settings page: {page}")).click();
        settle(&mut harness);
        let Some(last) = last else { continue };
        let turns = wheel_to(&mut harness, last, 40);
        assert!(shown(&harness, last), "{page}: {last} stays out of view");
        assert!(turns < 40, "{page}: the wheel did not reach {last}");
    }
}

#[test]
fn the_general_page_needs_the_wheel_and_another_page_opens_at_its_top() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: General").click();
    harness.run();
    let last = "Warp mode: Re-pitch";
    assert!(!shown(&harness, last), "the short window shows the whole General page");
    assert!(wheel_to(&mut harness, last, 40) > 0);
    // Scrolled down, the page's first setting is out of view; another page starts at its top.
    assert!(!shown(&harness, "When opening a project: Ask"));
    harness.get_by_label("Settings page: Browser").click();
    harness.run();
    assert!(shown(&harness, "Browser side: Left"));
    harness.get_by_label("Settings page: General").click();
    harness.run();
    assert!(shown(&harness, "When opening a project: Ask"));
}

#[test]
fn a_control_given_the_focus_is_scrolled_into_view() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: General").click();
    harness.run();
    let last = "Warp mode: Re-pitch";
    assert!(!shown(&harness, last));
    harness.get_by_label(last).focus();
    harness.run();
    assert!(shown(&harness, last), "the focused control stays out of view");
}

#[test]
fn a_search_shows_the_settings_that_match_from_every_page() {
    let mut harness = short_window();
    harness.get_by_label("Search settings").focus();
    settle(&mut harness);
    harness.get_by_label("Search settings").type_text("preview");
    settle(&mut harness);
    // Previews in the browser and on the map, from one page; settings that do not match are gone.
    assert!(harness.query_by_label("Auto-play: On").is_some());
    assert!(harness.query_by_label("Map preview on hover: On").is_some());
    assert!(harness.query_by_label("Browser side: Left").is_none());
    assert!(harness.query_by_label("When opening a project: Ask").is_none());
    // A change made in the results is made as on its page.
    harness.get_by_label("Auto-play: Off").click();
    settle(&mut harness);
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.preview_autoplay), Some(false));

    // Words match anywhere: the page and the section count too.
    harness.get_by_label("Search settings").focus();
    settle(&mut harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Search settings").type_text("transport stay");
    settle(&mut harness);
    assert!(harness.query_by_label("Stop: Stay").is_some());
    assert!(harness.query_by_label("Play from: Marker").is_none(), "only Stop has both words (Stay is its note)");

    // Nothing matches.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Search settings").type_text("nothing like this");
    settle(&mut harness);
    assert!(harness.query_by_label("Stop: Stay").is_none());

    // A page clicked empties the search and shows the page.
    harness.get_by_label("Settings page: Transport").click();
    settle(&mut harness);
    assert!(harness.query_by_label("Play from: Marker").is_some());
    assert_eq!(harness.get_by_label("Search settings").value().as_deref(), Some(""));
    assert!(harness.state().settings_open());
}

#[test]
fn escape_in_the_search_empties_it_and_keeps_settings_open() {
    let mut harness = short_window();
    harness.get_by_label("Search settings").focus();
    settle(&mut harness);
    harness.get_by_label("Search settings").type_text("zoom");
    settle(&mut harness);
    assert!(harness.query_by_label("Zoom: +").is_some());
    harness.key_press(Key::Escape);
    settle(&mut harness);
    assert!(harness.state().settings_open(), "Escape in the search closed Settings");
    assert!(harness.query_by_label("Zoom: +").is_none(), "the General page shows again");
    harness.key_press(Key::Escape);
    settle(&mut harness);
    assert!(!harness.state().settings_open());
}

/// The search finds single shortcuts, and the Shortcuts page filters its own list by action,
/// group or key.
#[test]
fn a_search_finds_shortcuts_and_the_shortcuts_page_filters_its_list() {
    let mut harness = short_window();
    harness.get_by_label("Search settings").focus();
    settle(&mut harness);
    harness.get_by_label("Search settings").type_text("waveform");
    settle(&mut harness);
    assert!(harness.query_by_label("Change keys of view.waveformZoomIn").is_some(), "the action is found");
    assert!(harness.query_by_label("Change keys of file.save").is_none(), "the rest are not listed");

    // On the page, its own filter: by the keys too.
    harness.get_by_label("Settings page: Shortcuts").click();
    settle(&mut harness);
    assert!(harness.query_by_label("Change keys of file.save").is_some(), "the whole list without a filter");
    harness.get_by_label("Filter shortcuts").focus();
    settle(&mut harness);
    harness.get_by_label("Filter shortcuts").type_text("ctrl+alt+up");
    settle(&mut harness);
    assert!(harness.query_by_label("Change keys of view.waveformZoomIn").is_some());
    assert!(harness.query_by_label("Change keys of file.save").is_none());
}

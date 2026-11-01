// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Launch keys in the window (`docs/spec/ui-session-view.md` › Launch keys): the mapping mode
//! maps a key to the picked slot, scene or stop button, and the key then launches it while the
//! Launcher view is shown, ahead of a shortcut on the same key.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::model::{MappingSource, MappingTarget, TrackId};
use suhina_engine::{AudioPool, Renderer};
use suhina_session::{Player, Session};
use suhina_ui::demo::demo_project;
use suhina_ui::{App, Status, View, kit::theme};

/// The demo project in the window with an engine that is attached but not playing, so a
/// launch is taken and says so. The renderer is kept so the engine's queue stays open.
fn window() -> (Harness<'static, App>, Renderer) {
    let project = demo_project();
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), 48_000).unwrap();
    let mut session = Session::new(project);
    session.attach_player(Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut harness = Harness::builder()
        .with_size((1280.0, 800.0))
        .with_step_dt(0.25)
        .with_max_steps(4)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    (harness, renderer)
}

fn mappings(harness: &Harness<'static, App>) -> Vec<(MappingSource, MappingTarget)> {
    let session = harness.state().session().unwrap();
    session.project().launcher.mappings.iter().map(|m| (m.source.clone(), m.target)).collect()
}

fn status(harness: &Harness<'static, App>) -> String {
    match harness.state().status() {
        Status::Hint(text) | Status::Done(text) | Status::Error(text) => text.clone(),
    }
}

fn press(harness: &mut Harness<'static, App>, key: Key) {
    harness.key_press(key);
    harness.run();
}

#[test]
fn the_mapping_mode_maps_a_key_that_then_launches_the_slot() {
    let (mut harness, _renderer) = window();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::M);
    harness.run();
    assert!(status(&harness).contains("Mapping keys"), "{}", status(&harness));

    // In the mapping mode a click picks the slot and launches nothing.
    harness.get_by_label("Map a key to Drums scene 1").click();
    harness.run();
    press(&mut harness, Key::Q);
    let slot = MappingTarget::Slot { track: TrackId(1), scene: 0 };
    assert_eq!(mappings(&harness), [(MappingSource::key("Q").unwrap(), slot)]);

    // Escape leaves the mode; Q now launches the slot.
    press(&mut harness, Key::Escape);
    assert!(harness.query_by_label("Map a key to Drums scene 1").is_none());
    press(&mut harness, Key::Q);
    assert_eq!(status(&harness), "Launched scene 1 on track 1");

    // Undo takes the mapping back: Q launches nothing.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(mappings(&harness).is_empty());
}

#[test]
fn a_mapped_key_wins_over_a_shortcut_only_in_the_session_view() {
    let (mut harness, _renderer) = window();
    // Tab switches the views; mapped to scene 2, it launches the scene in the Launcher view.
    harness.get_by_label("Map keys to slots and scenes").click();
    harness.run();
    harness.get_by_label("Launch scene 2 Groove").click();
    harness.run();
    press(&mut harness, Key::Tab);
    assert_eq!(mappings(&harness), [(MappingSource::key("Tab").unwrap(), MappingTarget::Scene { scene: 1 })]);
    assert_eq!(harness.state().view(), View::Session, "the mapping mode took Tab");
    harness.get_by_label("Map keys to slots and scenes").click();
    harness.run();

    press(&mut harness, Key::Tab);
    assert_eq!(status(&harness), "Launched scene 2");
    assert_eq!(harness.state().view(), View::Session);

    // In the Arrangement view Tab is the shortcut again.
    harness.get_by_label("Arrangement view").click();
    harness.run();
    press(&mut harness, Key::Tab);
    assert_eq!(harness.state().view(), View::Session);
}

#[test]
fn delete_in_the_mapping_mode_removes_the_keys_of_the_picked_control() {
    let (mut harness, _renderer) = window();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::M);
    harness.run();
    harness.get_by_label("Stop Drums").click();
    harness.run();
    press(&mut harness, Key::S);
    assert_eq!(mappings(&harness), [(MappingSource::key("S").unwrap(), MappingTarget::Stop { track: TrackId(1) })]);
    press(&mut harness, Key::Delete);
    assert!(mappings(&harness).is_empty());
}

#[test]
fn a_text_field_keeps_a_mapped_key() {
    let (mut harness, _renderer) = window();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::M);
    harness.run();
    harness.get_by_label("Map a key to Drums scene 1").click();
    harness.run();
    press(&mut harness, Key::Q);
    press(&mut harness, Key::Escape);
    // Ctrl+F shows the browser and puts the keys in its search field: Q is typed, not launched.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    harness.run();
    assert!(harness.ctx.text_edit_focused());
    let before = status(&harness);
    press(&mut harness, Key::Q);
    assert_eq!(status(&harness), before);
}

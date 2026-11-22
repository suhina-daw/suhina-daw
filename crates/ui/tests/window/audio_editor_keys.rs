// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The audio editor's hold on the keys: a press in the detail area gives it the keyboard and one
//! anywhere else takes it away, whether or not its waveform can be drawn, and its reset dialog
//! holds the keys as the window's other dialogs do (`docs/spec/ui-audio-editor.md`).

use super::*;

/// The `Loop` clip of [`project_with_audio`](super::project_with_audio) opened in the audio editor
/// in the Arrangement view; without its decoded audio when `loaded` is false.
fn opened_audio_editor(loaded: bool) -> Harness<'static, App> {
    let (project, waveforms) = super::project_with_audio();
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(project), &[]);
    if loaded {
        app.set_waveforms(waveforms);
    }
    let mut harness = window_with_step(app, 1.0 / 60.0);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    let at = harness.get_by_label("Clip Loop on Loop at bar 1").rect().center();
    double_click_at(&mut harness, at);
    harness.run();
    harness
}

#[allow(clippy::unwrap_used)]
fn loop_lane(harness: &Harness<'static, App>) -> Vec<suhina_engine::model::Clip> {
    let project = harness.state().session().unwrap().project();
    project.arrangement.lanes.iter().find(|l| l.name == "Loop").unwrap().clips.clone()
}

/// How many warp markers the `Loop` clip has.
fn loop_markers(harness: &Harness<'static, App>) -> usize {
    loop_lane(harness).first().map_or(0, |clip| clip.audio.warp_markers.len())
}

/// Presses and lets go once at `at`, a second after anything before it, so it is a click and not
/// part of a double-click.
fn click_once_at(harness: &mut Harness<'static, App>, at: egui::Pos2) {
    harness.hover_at(at);
    harness.run();
    let ready = harness.ctx.input(|i| i.time) + 1.0;
    while harness.ctx.input(|i| i.time) < ready {
        harness.step();
    }
    harness.hover_at(at);
    harness.drag_at(at);
    harness.drop_at(at);
    harness.step();
    harness.run();
}

/// An editor whose audio is not loaded draws no waveform, and still gives up the keyboard on a
/// press outside the detail area, as it takes it on a press inside.
#[test]
fn an_editor_without_its_audio_gives_up_the_keyboard_on_a_press_elsewhere() {
    let mut harness = opened_audio_editor(false);
    let editor = harness.get_by_label("Audio editor").rect();
    click_once_at(&mut harness, editor.center());
    assert!(harness.state().audio_editor().focused(), "a press in the editor gives it the keyboard");
    let elsewhere = harness.get_by_label("Arrangement view").rect().center();
    click_once_at(&mut harness, elsewhere);
    assert!(!harness.state().audio_editor().focused(), "a press outside the detail area takes it away");
}

/// While the reset dialog is open, Delete removes no warp marker behind it, though the editor had
/// the keyboard and a marker picked when the dialog opened.
#[test]
fn delete_behind_the_reset_dialog_removes_no_warp_marker() {
    let mut harness = opened_audio_editor(true);
    let id = loop_lane(&harness).first().map_or(0, |clip| clip.id.0);
    harness.state_mut().run(&Action::new("clip.addWarpMarker", json!({ "clip": id, "beat": 2.0 })));
    harness.run();
    assert_eq!(loop_markers(&harness), 3);
    let on_it = harness.get_by_label("Warp marker 2").rect().center();
    click_once_at(&mut harness, on_it);
    assert_eq!(harness.state().audio_editor().picked(), [2.0], "the marker clicked is picked");
    harness.get_by_label("Reset warp…").click();
    harness.run();
    assert!(harness.state().audio_editor().reset_open(), "the dialog asks");
    assert!(harness.state().audio_editor().focused(), "the editor still has the keyboard");
    assert_eq!(harness.state().audio_editor().picked(), [2.0], "and the marker is still picked");
    harness.key_press(Key::Delete);
    harness.run();
    assert_eq!(loop_markers(&harness), 3, "the dialog holds the key");
    assert_eq!(loop_lane(&harness).len(), 1, "and the clip is still there");
    harness.get_by_label("Keep warp markers").click();
    harness.run();
    assert!(!harness.state().audio_editor().reset_open());
    assert_eq!(loop_markers(&harness), 3, "keeping changes nothing");
}

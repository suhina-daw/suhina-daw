// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The selection's window side (`docs/spec/selection.md` › The window follows): a command that
//! moves the session's selection during a frame is not undone by the window's older pick, and a
//! click that moves the window's is published.

use super::*;

/// A window on a track with two devices, pointing at the first, with the session agreeing.
fn window_on_two_devices() -> (App, TrackId) {
    let mut app = App::new();
    let mut session = Session::new(crate::demo::demo_project());
    let track = session.project().tracks[0].id;
    session.execute("device.add", &json!({ "track": track.0, "id": "utility" })).unwrap();
    session.execute("device.add", &json!({ "track": track.0, "id": "fm-synth" })).unwrap();
    session.execute("track.select", &json!({ "track": track.0 })).unwrap();
    app.set_session(session, &[]);
    app.arrangement.selected_tracks = vec![track];
    app.view.selected = Some(track);
    app.view.selected_tracks = vec![track];
    app.selected_device = Some((track, detail_view::DeviceAt::at(0)));
    (app, track)
}

/// A controller's `device.selectNext` runs between the frame's start and its end: the window
/// still points at the first device, and its own publish must not put the selection back.
#[test]
fn a_script_selection_is_not_undone_by_the_windows_older_pick() {
    let (mut app, _) = window_on_two_devices();
    let before = app.window_selection();
    app.run(&Action::new("device.selectNext", json!({})));
    assert_eq!(app.session().and_then(Session::selected_device_path), Some(vec![1]), "the command moved it");
    app.publish_session_selection(before);
    assert_eq!(
        app.session().and_then(Session::selected_device_path),
        Some(vec![1]),
        "the window had not changed, so it published nothing"
    );
}

/// A click that picks another device changes the window during the frame; the publish runs the
/// same `device.select` the CLI and scripts run.
#[test]
fn a_click_on_another_device_is_published() {
    let (mut app, track) = window_on_two_devices();
    let before = app.window_selection();
    app.selected_device = Some((track, detail_view::DeviceAt::at(1)));
    app.publish_session_selection(before);
    assert_eq!(app.session().and_then(Session::selected_device_path), Some(vec![1]));
}

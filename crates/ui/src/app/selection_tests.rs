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
    let project = app.projects.active();
    app.run(&Action::new("device.selectNext", json!({})));
    assert_eq!(app.session().and_then(Session::selected_device_path), Some(vec![1]), "the command moved it");
    app.publish_session_selection(before, project);
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
    let project = app.projects.active();
    app.selected_device = Some((track, detail_view::DeviceAt::at(1)));
    app.publish_session_selection(before, project);
    assert_eq!(app.session().and_then(Session::selected_device_path), Some(vec![1]));
}

/// A command that selects another track leaves no slot of the old one behind: the detail area
/// shows the new track's chain, not a clip on the track the command left.
#[test]
fn a_command_selecting_another_track_clears_the_old_slot() {
    let mut app = App::new();
    let mut session = Session::new(crate::demo::demo_project());
    let first = session.project().tracks[0].id;
    let second = session.project().tracks[1].id;
    session.execute("track.select", &json!({ "track": first.0 })).unwrap();
    app.set_session(session, &[]);
    app.arrangement.selected_tracks = vec![first];
    app.view.selected = Some(first);
    app.view.selected_slot = Some((first, 0));
    app.projects_mut().execute("track.select", &json!({ "track": second.0 })).unwrap();
    app.follow_session_selection();
    assert_eq!(app.view.selected, Some(second));
    assert_eq!(app.view.selected_slot, None, "the old track's slot is gone");
}

/// A pick made in the frame that switches projects is not published to the project that
/// became active: it belongs to the one the frame began on.
#[test]
fn a_project_switch_in_the_frame_does_not_publish_the_outgoing_pick() {
    let mut app = App::new();
    let mut first = Session::new(crate::demo::demo_project());
    let first_track = first.project().tracks[0].id;
    first.execute("track.select", &json!({ "track": first_track.0 })).unwrap();
    let first_id = app.open_project(first, &[], OpenAs::NewTab).unwrap();
    let second = Session::new(crate::demo::demo_project());
    let second_id = app.open_project(second, &[], OpenAs::NewTab).unwrap();
    app.projects_mut().execute("project.switch", &json!({ "project": second_id.0 })).unwrap();
    // The outgoing project's window pick, published after the switch landed.
    app.arrangement.selected_tracks = vec![first_track];
    app.view.selected = Some(first_track);
    app.publish_session_selection((Some(first_track), None), Some(first_id));
    assert!(app.session().and_then(Session::selected_track).is_none(), "the new project's own empty selection stays");
}

/// A deleted track's highlight stays gone even when an undo brings the track back before the
/// next frame: undo does not restore the selection, and the window follows the empty one.
#[test]
fn undo_before_the_follow_does_not_restore_a_removed_tracks_highlight() {
    let mut app = App::new();
    let mut session = Session::new(crate::demo::demo_project());
    let first = session.project().tracks[0].id;
    session.execute("track.select", &json!({ "track": first.0 })).unwrap();
    app.set_session(session, &[]);
    app.arrangement.selected_tracks = vec![first];
    app.view.selected = Some(first);
    app.follow_session_selection();
    app.projects_mut().execute("track.delete", &json!({ "track": first.0 })).unwrap();
    app.projects_mut().execute("edit.undo", &json!({})).unwrap();
    assert!(app.session().and_then(Session::selected_track).is_none(), "the session stays empty");
    app.follow_session_selection();
    assert_eq!(app.view.selected, None, "the window follows the empty selection after undo");
    assert!(app.arrangement.selected_tracks.is_empty());
}

/// A device clicked in another track's mixer list publishes its track and its device in the
/// same frame; the track's first device must not replace the click next frame.
#[test]
fn a_device_clicked_with_its_track_is_published_together() {
    let mut app = App::new();
    let mut session = Session::new(crate::demo::demo_project());
    let first = session.project().tracks[0].id;
    let second = session.project().tracks[1].id;
    session.execute("device.add", &json!({ "track": first.0, "id": "utility" })).unwrap();
    session.execute("device.add", &json!({ "track": second.0, "id": "utility" })).unwrap();
    session.execute("track.select", &json!({ "track": first.0 })).unwrap();
    app.set_session(session, &[]);
    app.arrangement.selected_tracks = vec![first];
    app.view.selected = Some(first);
    let before = app.window_selection();
    let project = app.projects.active();
    app.arrangement.selected_tracks = vec![second];
    app.view.selected = Some(second);
    app.view.selected_tracks = vec![second];
    app.selected_device = Some((second, detail_view::DeviceAt::at(0)));
    app.publish_session_selection(before, project);
    let session = app.session().unwrap();
    assert_eq!(session.selected_track(), Some(second));
    assert_eq!(session.selected_device_path(), Some(vec![0]));
}

/// A presentation folder is a track even without a mixer channel: a window click on it is
/// published, and selecting it again keeps it.
#[test]
fn a_presentation_folder_is_published() {
    let mut app = App::new();
    let mut session = Session::new(crate::demo::demo_project());
    let first = session.project().tracks[0].id;
    let made = session.execute("track.folder", &json!({ "tracks": [first.0], "name": "Folder" })).unwrap();
    let folder = TrackId(made["track"].as_u64().unwrap_or_default() as u32);
    session.execute("track.select", &json!({ "track": first.0 })).unwrap();
    app.set_session(session, &[]);
    app.arrangement.selected_tracks = vec![first];
    app.view.selected = Some(first);
    let before = app.window_selection();
    let project = app.projects.active();
    app.arrangement.selected_tracks = vec![folder];
    app.view.selected = Some(folder);
    app.view.selected_tracks = vec![folder];
    app.publish_session_selection(before, project);
    assert_eq!(app.session().and_then(Session::selected_track), Some(folder));
}

/// A device a command set on the project becoming active is applied after that project's own
/// view is restored, not overwritten by the pick the view had when it was left.
#[test]
fn a_command_on_the_project_being_shown_wins_over_its_stored_pick() {
    let mut app = App::new();
    let mut first = Session::new(crate::demo::demo_project());
    let track = first.project().tracks[0].id;
    first.execute("device.add", &json!({ "track": track.0, "id": "utility" })).unwrap();
    first.execute("device.add", &json!({ "track": track.0, "id": "fm-synth" })).unwrap();
    first.execute("track.select", &json!({ "track": track.0 })).unwrap();
    let first_id = app.open_project(first, &[], OpenAs::NewTab).unwrap();
    app.arrangement.selected_tracks = vec![track];
    app.view.selected = Some(track);
    app.selected_device = Some((track, detail_view::DeviceAt::at(0)));
    app.open_project(Session::new(crate::demo::demo_project()), &[], OpenAs::NewTab).unwrap();

    // On the inactive first project, a command picks its second device; then the tab switches.
    app.projects_mut().execute_in(Some(first_id), "device.select", &json!({ "track": track.0, "device": 1 })).unwrap();
    app.projects_mut().execute("project.switch", &json!({ "project": first_id.0 })).unwrap();
    app.follow_active(&egui::Context::default());
    app.follow_session_selection();
    assert_eq!(app.session().and_then(Session::selected_device_path), Some(vec![1]));
    assert_eq!(app.selected_device, Some((track, detail_view::DeviceAt::at(1))), "the stored pick did not win");
}

/// A track removed from the session clears the window highlight its empty selection left
/// behind, so an undo that brings the track back does not leave a stale highlight.
#[test]
fn an_empty_session_clears_a_removed_tracks_highlight() {
    let mut app = App::new();
    let mut session = Session::new(crate::demo::demo_project());
    let track = session.project().tracks[0].id;
    session.execute("track.select", &json!({ "track": track.0 })).unwrap();
    app.set_session(session, &[]);
    app.arrangement.selected_tracks = vec![track];
    app.view.selected = Some(track);
    app.projects_mut().execute("track.delete", &json!({ "track": track.0 })).unwrap();
    app.follow_session_selection();
    assert_eq!(app.view.selected, None);
    assert!(app.arrangement.selected_tracks.is_empty());
    assert_eq!(app.selected_device, None);
}

// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// A control surface's window counts Launcher columns, not the visible UI columns: a
/// presentation folder between them, a reordered track, or a folded child must not move the
/// outline onto another track.
#[test]
fn a_surface_outline_follows_launcher_columns_not_visible_column_numbers() {
    let surface = suhina_session::ControlSurfaceWindow {
        id: "apc40-mkii".into(),
        name: "APC40 MkII".into(),
        track: 1,
        scene: 0,
        tracks: 2,
        scenes: 5,
    };
    let span = |min, max| egui::Rangef::new(min, max);
    let spans = [
        (None, span(0.0, 10.0)),
        (Some(2), span(10.0, 20.0)),
        (Some(0), span(20.0, 30.0)),
        (Some(1), span(30.0, 40.0)),
        (None, span(40.0, 50.0)),
    ];
    assert_eq!(
        surface_track_runs(&surface, spans),
        [span(10.0, 20.0), span(30.0, 40.0)],
        "a reordered launcher 0 between 2 and 1 is not enclosed"
    );
    let spans = [(None, span(0.0, 10.0)), (Some(2), span(10.0, 20.0)), (Some(0), span(20.0, 30.0))];
    assert_eq!(surface_track_runs(&surface, spans), [span(10.0, 20.0)], "only launcher 2 is visible");
    // A presentation folder has no Launcher index and breaks a run too.
    let folder = [(Some(1), span(0.0, 10.0)), (None, span(10.0, 20.0)), (Some(2), span(20.0, 30.0))];
    assert_eq!(
        surface_track_runs(&surface, folder),
        [span(0.0, 10.0), span(20.0, 30.0)],
        "the folder between launcher 1 and 2 is not enclosed"
    );
    let folded = [(Some(0), span(10.0, 20.0)), (Some(2), span(20.0, 30.0))];
    let surface = suhina_session::ControlSurfaceWindow { track: 0, tracks: 3, ..surface };
    assert_eq!(surface_track_runs(&surface, folded), [span(10.0, 30.0)], "folded launcher 1 leaves no gap");
}

/// The most the mixer grows leaves the body the slot rows and the room a track column's border and
/// padding add around them, so the stop row under the last scene shows whole (#1336).
#[test]
fn the_mixer_grows_no_higher_than_leaves_the_slot_rows_and_their_insets() {
    let state = SessionViewState { mixer_dragged: Some(10_000.0), ..Default::default() };
    let (view, scenes) = (600.0, 3);
    let plain = mixer_height(&state, view, scenes, 0.0);
    let padded = mixer_height(&state, view, scenes, 14.0);
    assert_eq!(plain - padded, 14.0, "{plain} without insets, {padded} with");
    let body = view - header_height() - 1.0 - padded - 1.0;
    assert_eq!(body, (scenes as f32 + 1.0) * slot_pitch() + 14.0, "the body holds every row and the insets");
}

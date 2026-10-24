// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The right-click menu of an empty spot on a lane.

use super::*;

/// The menu of an empty spot of a lane: a new empty MIDI clip where it was right-clicked (on a
/// notes track), and, with a time selection, looping it and consolidating it on this track.
pub(super) fn empty_lane_menu(
    ui: &mut Ui,
    track: TrackId,
    notes: bool,
    state: &ArrangementViewState,
    actions: &mut Vec<Action>,
) {
    ui.set_min_width(180.0);
    let beat = state.menu_beat.unwrap_or(0.0);
    if ui.add_enabled(notes, egui::Button::new("Insert empty MIDI clip")).clicked() {
        let stack = state.stack_midi;
        actions.push(Action::new("clip.create", json!({ "track": track.0, "start": beat, "stack": stack })));
        ui.close();
    }
    let selection = state.time_selection.as_ref();
    ui.separator();
    if ui
        .add_enabled(
            selection.is_some(),
            egui::Button::new("Loop selection").shortcut_text(crate::settings::keymap::shown("Ctrl+L")),
        )
        .clicked()
        && let Some(s) = selection
    {
        actions.push(Action::new("transport.setLoop", json!({ "start": s.start, "end": s.end, "enabled": true })));
        ui.close();
    }
    let here = selection.filter(|s| s.tracks.contains(&track));
    if ui
        .add_enabled(
            here.is_some(),
            egui::Button::new("Consolidate selection").shortcut_text(crate::settings::keymap::shown("Ctrl+J")),
        )
        .clicked()
        && let Some(s) = here
    {
        actions.push(Action::new("clip.consolidate", json!({ "track": track.0, "start": s.start, "end": s.end })));
        ui.close();
    }
}

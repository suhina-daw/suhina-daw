// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The right-click menu of an empty spot on a lane.

use super::*;
use crate::kit::menu;

/// The Arrangement's grid, in a menu itself, as its toolbar offers it.
pub(super) fn grid_section(ui: &mut Ui, state: &mut ArrangementViewState) {
    let keys = state.grid_keys.clone();
    if let Some(picked) = crate::interaction::grid_menu::grid_entries(ui, state.grid, &keys, &|_| true) {
        state.grid = picked;
    }
}

/// The menu of an empty spot of a lane: a new empty MIDI clip where it was right-clicked (on a
/// notes track), and, with a time selection, looping it and consolidating it on this track.
pub(super) fn empty_lane_menu(
    ui: &mut Ui,
    track: TrackId,
    notes: bool,
    has_takes: bool,
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
) {
    ui.set_min_width(180.0);
    // The take lanes under the track fold away and back (`docs/spec/recording.md` › Take lanes).
    if has_takes {
        let hidden = state.takes_hidden.contains(&track);
        if menu::entry(ui, if hidden { "Show take lanes" } else { "Hide take lanes" }, None, true).clicked() {
            if !state.takes_hidden.remove(&track) {
                state.takes_hidden.insert(track);
            }
            ui.close();
        }
    }
    if menu::entry(ui, "Add take", None, true).clicked() {
        state.takes_hidden.remove(&track);
        actions.push(Action::new("take.add", json!({ "track": track.0 })));
        ui.close();
    }
    if has_takes && menu::entry(ui, "Delete unused takes", None, true).clicked() {
        actions.push(Action::new("take.deleteUnused", json!({ "track": track.0 })));
        ui.close();
    }
    menu::separator(ui);
    grid_section(ui, state);
    menu::separator(ui);
    let beat = state.menu_beat.unwrap_or(0.0);
    if menu::entry(ui, "Insert empty MIDI clip", None, notes).clicked() {
        let stack = state.stack_midi;
        actions.push(Action::new("clip.create", json!({ "track": track.0, "start": beat, "stack": stack })));
        ui.close();
    }
    let selection = state.time_selection.as_ref();
    menu::separator(ui);
    if menu::entry(ui, "Loop selection", Some(&crate::settings::keymap::shown("Ctrl+L")), selection.is_some()).clicked()
        && let Some(s) = selection
    {
        actions.push(Action::new("transport.setLoop", json!({ "start": s.start, "end": s.end, "enabled": true })));
        ui.close();
    }
    let here = selection.filter(|s| s.tracks.contains(&track));
    let key = crate::settings::keymap::shown("Ctrl+J");
    if menu::entry(ui, "Consolidate selection", Some(&key), here.is_some()).clicked()
        && let Some(s) = here
    {
        actions.push(Action::new("clip.consolidate", json!({ "track": track.0, "start": s.start, "end": s.end })));
        ui.close();
    }
}

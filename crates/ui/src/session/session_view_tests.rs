// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

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

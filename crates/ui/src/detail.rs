// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The detail area under or beside a view: the selected track's device chain, the selected
//! clip's properties and follow action, and the clip editor (the piano roll and its tools).

pub mod audio_editor;
pub mod detail_view;
pub(crate) mod device_canvas;
pub(crate) mod device_widgets;
pub(crate) mod eq_curve;
pub mod follow_view;
pub mod midi_tools;
pub mod piano_roll;
pub mod piano_roll_ghosts;
pub mod piano_roll_grid;
pub mod piano_roll_ruler;
pub mod piano_roll_scale;

/// The fill the detail panels and the piano roll paint for themselves, of `kind` (`panel` or
/// `window`): the region's own background where it gives one, else the `background-color` of
/// `detail-fill[kind]`.
pub(crate) fn detail_fill(kind: &str) -> egui::Color32 {
    crate::kit::style_class::surface_or(fill_of(kind))
}

/// The `background-color` of `detail-fill[kind]` alone, for a part that paints it whatever the
/// region behind it is.
pub(crate) fn fill_of(kind: &str) -> egui::Color32 {
    crate::kit::element::DETAIL_FILL.attribute("kind", kind).look().background()
}

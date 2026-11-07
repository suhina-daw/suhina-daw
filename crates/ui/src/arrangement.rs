// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Arrangement view: the tracks along time, their clips and automation lanes.

pub mod arrangement_view;
pub mod automation_lanes;
pub mod envelope;
pub mod timeline;

/// A clip's tempo being dragged in the clip panel: the tempo it is at so far. The panel sends
/// the tempo a few times a second (a new stretch voice each time), and the Arrangement draws the
/// clip at this tempo on every frame in between, as it draws an edge stretch, so the clip follows
/// the drag smoothly. Kept until the project has the tempo, or `frames` run out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoPreview {
    pub clip: suhina_engine::model::ClipId,
    pub bpm: f64,
    pub frames: u8,
}

/// Where the clip panel leaves a [`TempoPreview`] for the Arrangement.
pub fn tempo_preview_key() -> egui::Id {
    egui::Id::unique("clip-tempo-preview")
}

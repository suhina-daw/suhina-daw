// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Arrangement view: the tracks along time, their clips and automation lanes.

pub mod arrangement_view;
pub mod automation_lanes;
pub mod envelope;
pub mod timeline;

/// A clip's tempo, or an unwarped clip's pitch, being dragged in the clip panel: the value it is
/// at so far. Both change how long the clip is. The panel sends the value a few times a second
/// (a new stretch voice each time), and the Arrangement draws the clip at this value on every
/// frame in between, as it draws an edge stretch, so the clip follows the drag smoothly. Kept
/// until the project has the value, or `frames` run out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoPreview {
    pub clip: suhina_engine::model::ClipId,
    pub value: Dragged,
    pub frames: u8,
}

/// What a [`TempoPreview`] drags.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dragged {
    /// The clip's tempo in BPM.
    Tempo(f64),
    /// An unwarped clip's transpose in semitones, which repitches it and so changes its length.
    Transpose(f64),
}

impl TempoPreview {
    /// How much longer the clip is drawn than `view` plays it now; `None` when it is not
    /// (nearly) different.
    pub fn factor(&self, view: &timeline::AudioView) -> Option<f64> {
        let factor = match self.value {
            Dragged::Tempo(bpm) => bpm / view.tempo()?,
            Dragged::Transpose(semitones) => view.rate / 2f64.powf(semitones / 12.0),
        };
        (factor.is_finite() && factor > 0.0 && (factor - 1.0).abs() > 1e-6).then_some(factor)
    }
}

/// Where the clip panel leaves a [`TempoPreview`] for the Arrangement.
pub fn tempo_preview_key() -> egui::Id {
    egui::Id::unique("clip-tempo-preview")
}

// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Decoded audio and the pool of it a project plays ([`AudioData`], [`AudioPool`]). They live in
//! the model, which depends on nothing, so a crate that decodes files (`media`) needs neither the
//! engine nor every device the engine builds (#764 › 11). The engine re-exports both.

use std::collections::HashMap;
use std::sync::Arc;

/// Decoded audio, interleaved f32.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioData {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl AsRef<[f32]> for AudioData {
    fn as_ref(&self) -> &[f32] {
        &self.samples
    }
}

impl AudioData {
    /// How many frames it holds (samples per channel).
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }

    /// The stereo sample at `seconds` into the file, linearly interpolated; silence outside it.
    pub fn stereo_at(&self, seconds: f64) -> (f32, f32) {
        self.frame_at(seconds * f64::from(self.sample_rate))
    }

    /// The stereo sample at frame `position` of the file (fractional frames interpolate
    /// linearly); silence outside it. Mono plays on both sides.
    pub fn frame_at(&self, position: f64) -> (f32, f32) {
        let channels = usize::from(self.channels.max(1));
        // Past the end (a huge or infinite position from hostile warp markers or offsets
        // included) is silence, before the cast could saturate; NaN fails this too.
        if !(position >= 0.0 && position < self.frames() as f64) {
            return (0.0, 0.0);
        }
        let index = position.floor() as usize;
        let fraction = (position - position.floor()) as f32;
        let frame = |i: usize| -> (f32, f32) {
            let base = i.saturating_mul(channels);
            let left = self.samples.get(base).copied().unwrap_or(0.0);
            let right =
                if channels > 1 { self.samples.get(base.saturating_add(1)).copied().unwrap_or(0.0) } else { left };
            (left, right)
        };
        let (l0, r0) = frame(index);
        let (l1, r1) = frame(index.saturating_add(1));
        (l0 + (l1 - l0) * fraction, r0 + (r1 - r0) * fraction)
    }
}

/// The audio a project needs, prepared before playback.
#[derive(Debug, Clone, Default)]
pub struct AudioPool {
    /// Decoded files, keyed by the `file` string of the project's audio regions.
    pub files: HashMap<String, Arc<AudioData>>,
}

impl AudioPool {
    pub fn new() -> Self {
        Self::default()
    }
}

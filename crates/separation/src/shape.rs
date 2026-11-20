// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a model takes and gives (docs/spec/stem-separation.md › Chunking): its sample rate,
//! channels, window, hop, weighting and stem count. A model's manifest supplies these later; every
//! value is checked here, since a manifest is input.

use crate::Error;

/// Sample rates a model or a source may have, in hertz.
pub const SAMPLE_RATE_RANGE: (u32, u32) = (8_000, 384_000);
/// The most channels a model takes in one pass.
pub const MAX_MODEL_CHANNELS: usize = 2;
/// The most stems a model gives.
pub const MAX_STEMS: usize = 16;
/// The shortest window, in frames at the model's rate.
pub const MIN_WINDOW: usize = 16;
/// The longest window, in frames at the model's rate (about 87 s at 192 kHz; htdemucs uses
/// 343 980 frames, 7.8 s at 44.1 kHz).
pub const MAX_WINDOW: usize = 1 << 24;
/// The most windows that may cover one frame: the hop is at least the window divided by this.
pub const MAX_OVERLAP: usize = 16;

/// How a window's output is weighted before the windows are added up; the sum of the weights
/// at each frame is divided out afterwards, so any weighting gives unity gain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weighting {
    /// Every frame of the window counts the same: neighbouring windows meet with a step if
    /// their outputs differ.
    Flat,
    /// A triangle peaking at the center, as htdemucs weights its windows.
    Triangle,
    /// A Hann window sampled between its zeros, as MDX-Net weights its windows.
    Hann,
}

impl Weighting {
    /// The weight of each frame of a window `len` frames long. Every weight is above zero, so
    /// the frames at a source's two ends, which one window alone covers, divide out exactly.
    pub fn weights(self, len: usize) -> Vec<f32> {
        match self {
            Weighting::Flat => vec![1.0; len],
            Weighting::Triangle => {
                // 1, 2, …, len/2, then len - len/2, …, 1; scaled so the peak is 1.
                let half = len / 2;
                let peak = half.max(len - half).max(1) as f64;
                (0..len)
                    .map(|i| {
                        let rise = if i < half { i + 1 } else { len - i };
                        (rise as f64 / peak) as f32
                    })
                    .collect()
            }
            Weighting::Hann => {
                let n = len.max(1) as f64;
                (0..len)
                    .map(|i| {
                        let phase = std::f64::consts::TAU * (i as f64 + 0.5) / n;
                        (0.5 - 0.5 * phase.cos()) as f32
                    })
                    .collect()
            }
        }
    }
}

/// What a model takes and gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelShape {
    /// The sample rate the model runs at; a source at another rate is resampled to it and back.
    pub sample_rate: u32,
    /// Channels the model takes in one pass (1 or 2).
    pub channels: usize,
    /// Frames the model takes at once.
    pub window: usize,
    /// Frames from one window's start to the next one's; `window - hop` frames overlap.
    pub hop: usize,
    pub weighting: Weighting,
    /// Stems the model gives, each with `channels` channels.
    pub stems: usize,
}

impl ModelShape {
    /// Checks every value against the limits above.
    pub fn validate(&self) -> Result<(), Error> {
        let bad = |reason: String| Err(Error::InvalidShape(reason));
        let (low, high) = SAMPLE_RATE_RANGE;
        if !(low..=high).contains(&self.sample_rate) {
            return bad(format!("sample rate {} Hz is outside {low}..={high} Hz", self.sample_rate));
        }
        if !(1..=MAX_MODEL_CHANNELS).contains(&self.channels) {
            return bad(format!("{} channels; a model takes 1 or {MAX_MODEL_CHANNELS}", self.channels));
        }
        if !(1..=MAX_STEMS).contains(&self.stems) {
            return bad(format!("{} stems; a model gives 1 to {MAX_STEMS}", self.stems));
        }
        if !(MIN_WINDOW..=MAX_WINDOW).contains(&self.window) {
            return bad(format!("a window of {} frames is outside {MIN_WINDOW}..={MAX_WINDOW}", self.window));
        }
        if self.hop == 0 || self.hop > self.window {
            return bad(format!("a hop of {} frames must be 1..={} (the window)", self.hop, self.window));
        }
        if self.hop.saturating_mul(MAX_OVERLAP) < self.window {
            return bad(format!(
                "a hop of {} frames overlaps a window of {} more than {MAX_OVERLAP} times",
                self.hop, self.window
            ));
        }
        Ok(())
    }

    /// Samples the model takes per window: `channels × window`.
    pub fn input_len(&self) -> usize {
        self.channels.saturating_mul(self.window)
    }

    /// Samples the model gives per window: `stems × channels × window`.
    pub fn output_len(&self) -> usize {
        self.stems.saturating_mul(self.input_len())
    }
}

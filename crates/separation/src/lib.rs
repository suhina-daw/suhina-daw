// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stem separation (docs/spec/stem-separation.md): an audio signal into its stems, offline.
//!
//! The crate owns everything around the model: the source is brought to the model's sample rate
//! and channel count, cut into the model's windows (the last one padded), each window handed to
//! a [`Backend`], the windows' outputs added back with their weighting divided out, and the stems
//! brought back to the source's rate and channels. [`BandSplitter`] is a backend with no weights
//! that runs the whole pipeline in tests. Nothing here touches the audio thread or the disk.
//!
//! In detail: Stem separation (spec (`docs/spec/stem-separation.md`), planned in steps): a source
//! brought to a model's sample rate and channels (`resample`, a windowed-sinc resampler), cut into
//! the model's windows with the last one padded, each handed to a `Backend`, and the windows'
//! outputs added back with their weighting divided out (`chunk`); limits on every shape and source
//! (`shape`); progress per window and cancel between windows. `BandSplitter` is a backend with no
//! weights that runs the pipeline in tests. No model, runtime, command or session wiring yet.

pub mod backend;
pub mod band_splitter;
pub mod chunk;
pub mod resample;
pub mod shape;

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

pub use backend::{Backend, BackendError};
pub use band_splitter::BandSplitter;
pub use shape::{ModelShape, Weighting};

use chunk::{OverlapAdd, Plan, fill_window};
use resample::Resampler;
use shape::SAMPLE_RATE_RANGE;

/// The most channels a source may have.
pub const MAX_SOURCE_CHANNELS: usize = 2;
/// The longest source, in seconds (spec › Limits).
pub const MAX_SOURCE_SECONDS: usize = 30 * 60;
/// The most samples of stems (stems × channels × frames) a run may hold, at the source's rate or
/// at the model's: 4 GiB of 32-bit floats.
pub const MAX_STEM_SAMPLES: usize = 1 << 30;

/// Why a separation did not finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The model's shape is out of bounds.
    InvalidShape(String),
    /// The source cannot be separated: no frames, too many channels, a sample rate out of range,
    /// channels of different lengths, or a sample that is not a finite number.
    InvalidSource(String),
    /// The source is too long, or its stems would not fit [`MAX_STEM_SAMPLES`].
    TooLong(String),
    /// The backend failed on a window (counted from 0 over all passes).
    Backend { window: usize, error: BackendError },
    /// The backend gave a sample that is not a finite number.
    NotFinite { window: usize },
    /// The run was cancelled between two windows.
    Cancelled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidShape(reason) => write!(f, "the model's shape is not usable: {reason}"),
            Error::InvalidSource(reason) => write!(f, "the audio cannot be separated: {reason}"),
            Error::TooLong(reason) => write!(f, "the audio is too long to separate: {reason}"),
            Error::Backend { window, error } => write!(f, "the model failed on window {window}: {error}"),
            Error::NotFinite { window } => write!(f, "the model gave a value that is not a number in window {window}"),
            Error::Cancelled => f.write_str("the separation was cancelled"),
        }
    }
}

impl std::error::Error for Error {}

/// Audio as one buffer per channel, all of the same length.
#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    sample_rate: u32,
    channels: Vec<Vec<f32>>,
}

impl Audio {
    /// Audio from one buffer per channel. Checks the sample rate, the channel count (1 or 2),
    /// that every channel has the same, non-zero length, and that every sample is finite.
    pub fn new(sample_rate: u32, channels: Vec<Vec<f32>>) -> Result<Self, Error> {
        let bad = |reason: String| Err(Error::InvalidSource(reason));
        let (low, high) = SAMPLE_RATE_RANGE;
        if !(low..=high).contains(&sample_rate) {
            return bad(format!("sample rate {sample_rate} Hz is outside {low}..={high} Hz"));
        }
        if !(1..=MAX_SOURCE_CHANNELS).contains(&channels.len()) {
            return bad(format!("{} channels; 1 or {MAX_SOURCE_CHANNELS} can be separated", channels.len()));
        }
        let frames = channels.first().map_or(0, Vec::len);
        if frames == 0 {
            return bad("it has no frames".into());
        }
        if channels.iter().any(|c| c.len() != frames) {
            return bad("its channels differ in length".into());
        }
        if channels.iter().flatten().any(|s| !s.is_finite()) {
            return bad("a sample is not a finite number".into());
        }
        Ok(Audio { sample_rate, channels })
    }

    /// Audio from interleaved samples; a partial frame at the end is dropped.
    pub fn from_interleaved(sample_rate: u32, channels: usize, samples: &[f32]) -> Result<Self, Error> {
        if !(1..=MAX_SOURCE_CHANNELS).contains(&channels) {
            return Err(Error::InvalidSource(format!(
                "{channels} channels; 1 or {MAX_SOURCE_CHANNELS} can be separated"
            )));
        }
        let planar = (0..channels)
            .map(|c| samples.chunks_exact(channels).filter_map(|frame| frame.get(c).copied()).collect())
            .collect();
        Audio::new(sample_rate, planar)
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }

    /// The buffers, one per channel.
    pub fn channels(&self) -> &[Vec<f32>] {
        &self.channels
    }

    /// The samples interleaved, frame by frame.
    pub fn interleaved(&self) -> Vec<f32> {
        (0..self.frames()).flat_map(|i| self.channels.iter().map(move |c| c.get(i).copied().unwrap_or(0.0))).collect()
    }
}

/// A finished separation: one [`Audio`] per stem, in the model's stem order, at the source's
/// sample rate, channel count and length.
#[derive(Debug, Clone, PartialEq)]
pub struct Separated {
    pub stems: Vec<Audio>,
}

/// How far a run is: windows done of the total, over every pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
}

/// Separates `source` with `backend`. `progress` hears `0` of the total first and then after
/// every window; `cancel` is checked before every window, and a cancelled run returns
/// [`Error::Cancelled`] and keeps nothing.
///
/// Channels (spec › Channels): a model with the source's channel count takes it in one pass; a
/// mono model takes each channel of a stereo source in a pass of its own; a stereo model takes a
/// mono source on both channels and its two outputs are averaged back to one.
pub fn separate(
    source: &Audio,
    backend: &mut dyn Backend,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Progress),
) -> Result<Separated, Error> {
    let shape = backend.shape().clone();
    shape.validate()?;
    let frames = source.frames();
    let source_channels = source.channels.len();
    let rate = usize::try_from(source.sample_rate).unwrap_or(usize::MAX);
    if frames > MAX_SOURCE_SECONDS.saturating_mul(rate) {
        return Err(Error::TooLong(format!("it is longer than {MAX_SOURCE_SECONDS} seconds")));
    }

    let passes = passes(source_channels, shape.channels)?;
    let to_model =
        (source.sample_rate != shape.sample_rate).then(|| Resampler::new(source.sample_rate, shape.sample_rate));
    let model_frames = to_model.as_ref().map_or(frames, |r| r.output_len(frames));
    let widest = source_channels.max(shape.channels);
    for (what, len) in [("at the model's rate", model_frames), ("at its own rate", frames)] {
        let samples = shape.stems.checked_mul(widest).and_then(|n| n.checked_mul(len));
        if samples.is_none_or(|n| n > MAX_STEM_SAMPLES) {
            return Err(Error::TooLong(format!(
                "{} stems of {widest} channels and {len} frames {what} exceed {MAX_STEM_SAMPLES} samples",
                shape.stems
            )));
        }
    }

    let resampled: Vec<Vec<f32>> = match &to_model {
        Some(r) => source.channels.iter().map(|c| r.process(c, model_frames)).collect(),
        None => source.channels.clone(),
    };
    let plan = Plan::new(model_frames, &shape);
    let total = plan.count.saturating_mul(passes.len());
    let mut done = 0;
    progress(Progress { done, total });

    // `[stem][source channel]` at the model's rate.
    let mut stems = vec![vec![vec![0.0f32; model_frames]; source_channels]; shape.stems];
    let mut input = vec![0.0f32; shape.input_len()];
    let mut output = vec![0.0f32; shape.output_len()];
    for pass in &passes {
        let channels: Vec<&[f32]> = pass.iter().filter_map(|&c| resampled.get(c).map(Vec::as_slice)).collect();
        let mut sum = OverlapAdd::new(plan, &shape, shape.stems * shape.channels);
        for window in 0..plan.count {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            fill_window(&plan, window, &channels, &mut input);
            output.fill(0.0);
            backend.run(&input, &mut output).map_err(|error| Error::Backend { window: done, error })?;
            if output.iter().any(|s| !s.is_finite()) {
                return Err(Error::NotFinite { window: done });
            }
            sum.add(window, &output);
            done += 1;
            progress(Progress { done, total });
        }
        // Each model channel goes back to the source channel it came from; a source channel that
        // fed several model channels gets their average.
        let lanes = sum.finish();
        let mut lanes = lanes.chunks_exact(model_frames.max(1));
        for stem in &mut stems {
            for &target in pass {
                let Some(lane) = lanes.next() else { break };
                let share = 1.0 / pass.iter().filter(|&&c| c == target).count().max(1) as f32;
                if let Some(dest) = stem.get_mut(target) {
                    for (d, s) in dest.iter_mut().zip(lane) {
                        *d += s * share;
                    }
                }
            }
        }
    }

    let from_model = to_model.is_some().then(|| Resampler::new(shape.sample_rate, source.sample_rate));
    let stems = stems
        .into_iter()
        .map(|channels| {
            let channels = match &from_model {
                Some(r) => channels.iter().map(|c| r.process(c, frames)).collect(),
                None => channels,
            };
            Audio { sample_rate: source.sample_rate, channels }
        })
        .collect();
    Ok(Separated { stems })
}

/// The source channels each pass hands the model, one per model channel.
fn passes(source: usize, model: usize) -> Result<Vec<Vec<usize>>, Error> {
    if source == model {
        Ok(vec![(0..source).collect()])
    } else if model == 1 {
        Ok((0..source).map(|c| vec![c]).collect())
    } else if source == 1 {
        Ok(vec![vec![0; model]])
    } else {
        Err(Error::InvalidSource(format!("{source} channels cannot be fed to a model of {model}")))
    }
}

#[cfg(test)]
mod tests;

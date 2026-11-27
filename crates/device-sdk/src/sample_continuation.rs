// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A held note continued past its recording, in the shared sample runtime: the Instrument's
//! Continue playback (`docs/spec/instrument-held-continuation.md` › How it plays). A note plays
//! the recording up to the continuation's join, crossfades into the model over 40 ms, and at
//! note-off crossfades back into the recording's own release where it has one.
//!
//! [`ContinuationPlayer::new`] allocates its generators (off the audio thread); claiming,
//! playing and splicing do not.

use std::sync::Arc;

use suhina_dsp::continuation::voice::{Generator, Prepared};
use suhina_dsp::continuation::{Continuation, NoteModel};

/// Notes that can continue at once; a further note holds in segments as before.
pub const POOL: usize = 8;
/// The join's and the release splice's crossfade.
const CROSSFADE_SECONDS: f64 = 0.04;
/// How far after note-off a release splice may start.
const SPLICE_SEARCH_SECONDS: f64 = 0.25;
/// The coarse step of the splice search, in samples.
const SPLICE_STRIDE: usize = 8;

/// The controls a continuing note follows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContinuationControls {
    pub enabled: bool,
    /// Side scale of the model and the recording, 0..2.
    pub width: f64,
    /// Vibrato depth as a factor of the recorded, 0..4; rate in Hz, 0 for the recorded.
    pub depth: f64,
    pub rate: f64,
    /// Morph toward the second continuation, 0..1.
    pub morph: f64,
}

impl Default for ContinuationControls {
    fn default() -> Self {
        Self { enabled: false, width: 1.0, depth: 1.0, rate: 0.0, morph: 0.0 }
    }
}

/// The continuation, its morph target and a pool of generators for one sample rate.
pub struct ContinuationPlayer {
    pub(crate) own: Arc<Continuation>,
    pub(crate) other: Option<Arc<Continuation>>,
    pub(crate) rate: f64,
    pub(crate) generators: Vec<Generator>,
    /// The voice each generator plays for.
    pub(crate) owners: [Option<usize>; POOL],
    scratch: Box<NoteModel>,
    other_scratch: Box<NoteModel>,
    /// The upcoming model and the release's opening, for a splice.
    upcoming: Vec<f32>,
    opening: Vec<f32>,
}

impl ContinuationPlayer {
    pub fn new(own: Arc<Continuation>, other: Option<Arc<Continuation>>, rate: f64) -> Self {
        let prepared = Prepared::new(&own, rate);
        let prepared_other = other.as_ref().map(|o| Prepared::new(o, rate));
        let generators = (0..POOL).map(|_| Generator::new(&prepared, prepared_other.as_ref())).collect();
        let crossfade = (CROSSFADE_SECONDS * rate) as usize + 1;
        let search = (SPLICE_SEARCH_SECONDS * rate) as usize + 1;
        Self {
            own,
            other,
            rate,
            generators,
            owners: [None; POOL],
            scratch: Box::default(),
            other_scratch: Box::default(),
            upcoming: vec![0.0; search + crossfade + SPLICE_STRIDE],
            opening: vec![0.0; crossfade],
        }
    }

    /// Samples of the join's and the splice's crossfade.
    pub(crate) fn crossfade(&self) -> usize {
        ((CROSSFADE_SECONDS * self.rate) as usize).max(1)
    }

    /// Where the model takes over and where the recording's release starts, in source frames.
    pub(crate) fn join_frame(&self, source_rate: f64) -> f64 {
        f64::from(self.own.join) * source_rate
    }

    fn release_frame(&self, source_rate: f64) -> Option<f64> {
        (self.own.release_start > 0.0).then(|| f64::from(self.own.release_start) * source_rate)
    }

    /// Gives `voice` a generator, when one is free, for a note `ratio` times the recording's
    /// pitch. Allocation-free.
    pub(crate) fn claim(
        &mut self,
        voice: usize,
        ratio: f64,
        controls: &ContinuationControls,
        seed: u64,
    ) -> Option<usize> {
        let slot = self.owners.iter().position(Option::is_none)?;
        self.scratch.transposed(&self.own, ratio, self.rate);
        let mut other_weight = 0.0;
        if let Some(other) = &self.other
            && controls.morph > 0.0
        {
            self.other_scratch.transposed(other, ratio, self.rate);
            self.scratch.morph(
                &self.other_scratch,
                controls.morph,
                f64::from(self.own.noise_rms),
                f64::from(other.noise_rms),
            );
            other_weight = controls.morph.clamp(0.0, 1.0);
        }
        let generator = self.generators.get_mut(slot)?;
        generator.start(&self.scratch, other_weight, seed);
        generator.set_vibrato(controls.depth, controls.rate);
        if let Some(owner) = self.owners.get_mut(slot) {
            *owner = Some(voice);
        }
        Some(slot)
    }

    pub(crate) fn free(&mut self, slot: usize) {
        if let Some(owner) = self.owners.get_mut(slot) {
            *owner = None;
        }
    }

    /// Where and how loud the recording's release takes over from the model of `slot`: the
    /// lag (samples from now, within 0.25 s) at which the model's next 40 ms best match the
    /// release's opening read at `step`, and the gain that matches their levels (0.7..1.4).
    /// None without a release of the recording's own.
    pub(crate) fn splice(
        &mut self,
        slot: usize,
        source: &[f32],
        channels: usize,
        source_rate: f64,
        step: f64,
    ) -> Option<(usize, f64, f64)> {
        let start = self.release_frame(source_rate)?;
        let length = self.crossfade();
        let search = (SPLICE_SEARCH_SECONDS * self.rate) as usize;
        for (i, value) in self.opening.iter_mut().enumerate().take(length) {
            let [l, r] = super::sample_instrument::read_frame(source, channels, start + i as f64 * step);
            *value = ((l + r) / 2.0) as f32;
        }
        let generator = self.generators.get_mut(slot)?;
        let wanted = (search + length + SPLICE_STRIDE).min(self.upcoming.len());
        generator.upcoming(self.upcoming.get_mut(..wanted)?);
        let opening = self.opening.get(..length)?;
        let upcoming = self.upcoming.get(..wanted)?;
        let opening_energy: f64 = opening.iter().map(|v| f64::from(*v).powi(2)).sum();
        let score = |lag: usize, stride: usize| -> f64 {
            let (mut dot, mut energy, mut reference) = (0.0, 0.0, 0.0);
            for i in (0..length).step_by(stride) {
                let (Some(a), Some(b)) = (upcoming.get(lag + i), opening.get(i)) else { break };
                let (a, b) = (f64::from(*a), f64::from(*b));
                dot += a * b;
                energy += a * a;
                reference += b * b;
            }
            dot / (energy * reference).sqrt().max(1e-30)
        };
        let mut best = (0, f64::MIN);
        for lag in (0..=search).step_by(SPLICE_STRIDE) {
            let s = score(lag, SPLICE_STRIDE);
            if s > best.1 {
                best = (lag, s);
            }
        }
        let coarse = best.0;
        best = (coarse, f64::MIN);
        for lag in coarse.saturating_sub(SPLICE_STRIDE)..=(coarse + SPLICE_STRIDE).min(search) {
            let s = score(lag, 1);
            if s > best.1 {
                best = (lag, s);
            }
        }
        let lag = best.0;
        let model_energy: f64 = upcoming.iter().skip(lag).take(length).map(|v| f64::from(*v).powi(2)).sum();
        let gain = if opening_energy > 0.0 { (model_energy / opening_energy).sqrt().clamp(0.7, 1.4) } else { 1.0 };
        Some((lag, gain, start))
    }
}

/// A voice's continuation: its generator, where the model takes over, the join's crossfade,
/// and a release splice back into the recording.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ModelHold {
    /// The voice continues: its recording is widened too, and the model takes over at `join`.
    pub(crate) on: bool,
    pub(crate) generator: Option<usize>,
    pub(crate) join: f64,
    pub(crate) joined: bool,
    pub(crate) fade: usize,
    /// Samples since the note-on, for spreading the model's generation before the join.
    pub(crate) tick: usize,
    /// The read step the model was transposed for.
    pub(crate) step: f64,
    /// A splice planned: samples until it starts, its gain and the release's start frame.
    pub(crate) splice: Option<(usize, f64, f64)>,
    pub(crate) splicing: Option<usize>,
    /// What the recording is multiplied by after a splice.
    pub(crate) gain: f64,
}

impl Default for ModelHold {
    fn default() -> Self {
        Self {
            on: false,
            generator: None,
            join: 0.0,
            joined: false,
            fade: 0,
            tick: 0,
            step: 1.0,
            splice: None,
            splicing: None,
            gain: 1.0,
        }
    }
}

/// `[left, right]` with its side scaled by `width`.
pub(crate) fn widen(signal: [f64; 2], width: f64) -> [f64; 2] {
    let mid = (signal[0] + signal[1]) / 2.0;
    let side = (signal[0] - signal[1]) / 2.0 * width;
    [mid + side, mid - side]
}

/// The `sin^2` crossfade weight of sample `k` of `length`.
pub(crate) fn fade_in(k: usize, length: usize) -> f64 {
    let x = (k as f64 / length.max(1) as f64).clamp(0.0, 1.0);
    (x * std::f64::consts::FRAC_PI_2).sin().powi(2)
}

#[cfg(test)]
#[path = "sample_continuation_tests.rs"]
mod tests;

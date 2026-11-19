// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-sample f64 linear stages, originally shared by SDK graph nodes and the sampler.

/// Stage durations in seconds and sustain in 0..1. Finite times are limited to 0..60;
/// non-finite settings become zero. Each moving stage takes at least one sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearEnvelopeSettings {
    pub attack: f64,
    pub decay: f64,
    pub sustain: f64,
    pub release: f64,
}

impl Default for LinearEnvelopeSettings {
    fn default() -> Self {
        Self { attack: 0.005, decay: 0.2, sustain: 0.8, release: 0.2 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Stage {
    Attack,
    Decay,
    Sustain,
    Release,
    #[default]
    Done,
}

/// A linear ADSR, idle by default. The caller supplies gate policy: `start` on a rising gate,
/// `release` on a falling gate; AD callers stop at `is_sustaining` instead of releasing.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LinearEnvelope {
    level: f64,
    stage: Stage,
    released: f64,
}

fn setting(value: f64, max: f64) -> f64 {
    if value.is_finite() { value.clamp(0.0, max) } else { 0.0 }
}

impl LinearEnvelope {
    /// Starts a new attack at zero, including when retriggering an active envelope.
    pub fn start(&mut self) {
        *self = Self { stage: Stage::Attack, ..Self::default() };
    }

    /// Captures the current level on every call, including repeated releases.
    pub fn release(&mut self) {
        self.released = self.level;
        self.stage = Stage::Release;
    }

    /// Advances exactly one sample, at most one stage. Positive finite sample rates are kept;
    /// others use 48 kHz. Sustain retains its reached level when the setting changes.
    #[inline]
    pub fn next(&mut self, settings: &LinearEnvelopeSettings, sample_rate: f64) -> f64 {
        let rate = if sample_rate.is_finite() && sample_rate > 0.0 { sample_rate } else { 48_000.0 };
        match self.stage {
            Stage::Attack => {
                self.level = (self.level + 1.0 / (setting(settings.attack, 60.0) * rate).max(1.0)).min(1.0);
                if self.level >= 1.0 {
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                let sustain = setting(settings.sustain, 1.0);
                self.level =
                    (self.level - (1.0 - sustain) / (setting(settings.decay, 60.0) * rate).max(1.0)).max(sustain);
                if self.level <= sustain {
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Release => {
                self.level = (self.level
                    - self.released.max(1e-12) / (setting(settings.release, 60.0) * rate).max(1.0))
                .max(0.0);
                if self.level <= 0.0 {
                    self.stage = Stage::Done;
                }
            }
            Stage::Sustain | Stage::Done => {}
        }
        self.level
    }

    pub fn value(&self) -> f64 {
        self.level
    }
    pub fn is_sustaining(&self) -> bool {
        self.stage == Stage::Sustain
    }
    pub fn is_releasing(&self) -> bool {
        self.stage == Stage::Release
    }
    pub fn is_done(&self) -> bool {
        self.stage == Stage::Done
    }
}

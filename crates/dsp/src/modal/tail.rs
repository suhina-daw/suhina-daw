// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The decaying noise tail of a fitted recording (what its modes miss after the attack: hammer
//! and soundboard noise, partials too weak or crowded to fit) and the allocation-free per-voice
//! player. See `docs/spec/instrument-modal-profile.md` › Tail playback.

use super::sustain::{NOISE_BANDS, noise_center_hz};

/// Number of f32 values of an encoded tail.
pub const TAIL_FIELDS: usize = 3 * NOISE_BANDS;
/// Shortest and longest band T60.
pub const MIN_TAIL_DECAY: f32 = 0.05;
pub const MAX_TAIL_DECAY: f32 = 60.0;
/// A band below this gain no longer sounds.
const SILENT: f32 = 1e-7;
/// Each band is three identical unity-peak band-pass sections: 18 dB per octave on each side,
/// so the low bands' noise does not leak into the treble, where a band spans far more hertz. A
/// section's Q makes the cascade's noise bandwidth exactly the band's 2/3 octave:
/// `f0 sqrt(pi) Gamma(5/2) / (2 Q Gamma(3)) = f0 (2^(1/3) - 2^(-1/3))`.
const SECTIONS: usize = 3;
const SECTION_Q: f64 = 1.263_713;

/// A recording's tail: per noise band, the fitted channel's and the other channel's amplitude
/// density (normalized units per square-root hertz) at the strike, and one T60. A band with a
/// zero T60 is off.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tail {
    pub level: [f32; NOISE_BANDS],
    pub other: [f32; NOISE_BANDS],
    pub decay: [f32; NOISE_BANDS],
}

impl Default for Tail {
    fn default() -> Self {
        Self::NONE
    }
}

impl Tail {
    pub const NONE: Self = Self { level: [0.0; NOISE_BANDS], other: [0.0; NOISE_BANDS], decay: [0.0; NOISE_BANDS] };

    /// Whether no band sounds, which is the same as no tail.
    pub fn is_empty(&self) -> bool {
        self.decay
            .iter()
            .zip(self.level.iter().zip(&self.other))
            .all(|(d, (a, b))| *d == 0.0 || (*a == 0.0 && *b == 0.0))
    }

    /// The values in their encoded order: levels, other levels, decays.
    pub fn fields(&self) -> [f32; TAIL_FIELDS] {
        let mut out = [0.0; TAIL_FIELDS];
        for (slot, value) in out.iter_mut().zip(self.level.iter().chain(&self.other).chain(&self.decay)) {
            *slot = *value;
        }
        out
    }

    /// The tail whose encoded values are `fields`, unvalidated.
    pub fn from_fields(fields: &[f32; TAIL_FIELDS]) -> Self {
        let mut tail = Self::NONE;
        let targets = tail.level.iter_mut().chain(tail.other.iter_mut()).chain(tail.decay.iter_mut());
        for (target, value) in targets.zip(fields) {
            *target = *value;
        }
        tail
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        let level_ok = |v: &f32| v.is_finite() && (0.0..=1.0).contains(v);
        let decay_ok = |v: &f32| *v == 0.0 || (v.is_finite() && (MIN_TAIL_DECAY..=MAX_TAIL_DECAY).contains(v));
        if self.level.iter().chain(&self.other).all(level_ok) && self.decay.iter().all(decay_ok) {
            Ok(())
        } else {
            Err("a tail needs densities of 0..1 and T60s of 0 or 0.05..60 seconds")
        }
    }
}

/// One band of a playing tail: per channel, the cascade of unity-peak band-pass sections
/// (topology-preserving state variable filters, `f32`), each channel's gain and the decay factor
/// per sample.
#[derive(Debug, Clone, Copy, Default)]
struct Band {
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
    state: [[[f32; 2]; SECTIONS]; 2],
    gain: [f32; 2],
    factor: f32,
}

impl Band {
    fn filter(&mut self, channel: usize, x: f32) -> f32 {
        let Some(sections) = self.state.get_mut(channel) else { return 0.0 };
        let flush = |v: f32| if v.abs() < 1e-20 { 0.0 } else { v };
        let mut signal = x;
        for state in sections.iter_mut() {
            let [ic1, ic2] = *state;
            let v3 = signal - ic2;
            let v1 = self.a1 * ic1 + self.a2 * v3;
            let v2 = ic2 + self.a2 * ic1 + self.a3 * v3;
            *state = [flush(2.0 * v1 - ic1), flush(2.0 * v2 - ic2)];
            signal = self.k * v1;
        }
        signal
    }
}

/// Plays a [`Tail`] for one voice: per band and channel, seeded white noise through the band's
/// filter, times a gain that falls with the band's T60, faded in as the attack fades out.
#[derive(Debug, Clone, Copy)]
pub struct TailPlayer {
    bands: [Band; NOISE_BANDS],
    noise: [u32; 2],
    active: bool,
    /// Samples since the note-on and the fade-in window (starts at `fade.0`, full at `fade.1`).
    at: u32,
    fade: (u32, u32),
    /// A stereo recording's tail: each channel its own noise; a mono one plays the same in both.
    independent: bool,
}

impl Default for TailPlayer {
    fn default() -> Self {
        Self {
            bands: [Band::default(); NOISE_BANDS],
            noise: [1, 2],
            active: false,
            at: 0,
            fade: (0, 0),
            independent: true,
        }
    }
}

impl TailPlayer {
    /// Starts `tail` with its T60s times `decay_scale`; it fades in over the samples `fade`
    /// (the attack's second half; `(0, 0)` sounds at once). `independent` gives each channel its
    /// own noise (a stereo recording); otherwise both play the fitted channel's. `seed` makes the
    /// noise repeatable.
    pub fn start(
        &mut self,
        tail: &Tail,
        decay_scale: f64,
        sample_rate: f64,
        fade: (usize, usize),
        independent: bool,
        seed: u32,
    ) {
        *self = Self::default();
        self.independent = independent;
        if tail.is_empty() || !sample_rate.is_finite() || !(1000.0..=384000.0).contains(&sample_rate) {
            return;
        }
        let scale = if decay_scale.is_finite() && decay_scale > 0.0 { decay_scale } else { 1.0 };
        // Unit-variance white noise has a one-sided density of 2 / fs per hertz; through a
        // unity-peak cascade whose noise bandwidth is the band's, `density * sqrt(fs / 2)` gives
        // the band its measured power.
        let gain = (sample_rate / 2.0).sqrt() as f32;
        for (index, band) in self.bands.iter_mut().enumerate() {
            let center = noise_center_hz(index);
            let (Some(&level), Some(&other), Some(&decay)) =
                (tail.level.get(index), tail.other.get(index), tail.decay.get(index))
            else {
                continue;
            };
            if decay <= 0.0 || center >= sample_rate * 0.45 || (level <= 0.0 && other <= 0.0) {
                continue;
            }
            let g = (std::f64::consts::PI * center / sample_rate).tan();
            let k = 1.0 / SECTION_Q;
            let a1 = 1.0 / (1.0 + g * (g + k));
            let t60 = (f64::from(decay) * scale).clamp(0.001, f64::from(MAX_TAIL_DECAY));
            *band = Band {
                a1: a1 as f32,
                a2: (g * a1) as f32,
                a3: (g * g * a1) as f32,
                k: k as f32,
                state: [[[0.0; 2]; SECTIONS]; 2],
                gain: [level * gain, other * gain],
                factor: (-1000.0_f64.ln() / (t60 * sample_rate)).exp() as f32,
            };
            self.active = true;
        }
        self.noise =
            [seed.wrapping_mul(2_654_435_761).max(1), seed.wrapping_mul(40_503).wrapping_add(0x9E37_79B9).max(1)];
        let clamp = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        self.fade = (clamp(fade.0.min(fade.1)), clamp(fade.1));
    }

    /// The next sample of the fitted channel and of the other channel.
    pub fn next(&mut self) -> [f32; 2] {
        if !self.active {
            return [0.0; 2];
        }
        let (start, end) = self.fade;
        let fade = if self.at >= end {
            1.0
        } else if self.at < start {
            0.0
        } else {
            let x = f64::from(self.at - start) / f64::from((end - start).max(1));
            (0.5 * (1.0 - (std::f64::consts::PI * x).cos())) as f32
        };
        self.at = self.at.saturating_add(1);
        let mut out = [0.0_f32; 2];
        let mut sounding = false;
        let channels = if self.independent { 2 } else { 1 };
        for band in &mut self.bands {
            if band.factor == 0.0 {
                continue;
            }
            for (channel, slot) in out.iter_mut().enumerate().take(channels) {
                let white = white(self.noise.get_mut(channel));
                let filtered = band.filter(channel, white);
                *slot += band.gain.get(channel).copied().unwrap_or(0.0) * filtered;
            }
            for gain in &mut band.gain {
                *gain *= band.factor;
            }
            if band.gain.iter().any(|g| g.abs() >= SILENT) {
                sounding = true;
            } else {
                band.factor = 0.0;
            }
        }
        self.active = sounding;
        if !self.independent {
            out[1] = out[0];
        }
        [out[0] * fade, out[1] * fade]
    }

    /// Caps every band's T60 at `seconds`, as the damper caps the modes'.
    pub fn cap_decay(&mut self, sample_rate: f64, seconds: f64) {
        if !sample_rate.is_finite() || !(1000.0..=384000.0).contains(&sample_rate) || !seconds.is_finite() {
            return;
        }
        let cap = (-1000.0_f64.ln() / (sample_rate * seconds.clamp(0.001, 60.0))).exp() as f32;
        for band in &mut self.bands {
            if band.factor > cap {
                band.factor = cap;
            }
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Uniform white noise of unit variance from a xorshift state.
fn white(state: Option<&mut u32>) -> f32 {
    let Some(state) = state else { return 0.0 };
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (f64::from(x) / 2_147_483_648.0 - 1.0) as f32 * 3.0_f32.sqrt()
}

#[cfg(test)]
#[path = "tail_tests.rs"]
mod tests;

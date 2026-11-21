// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The held layer of a fitted recording (vibrato, per-mode onsets and vibrato-coupled levels,
//! a band noise layer, a measured release and a spectral envelope that stays in Hz) and the
//! allocation-free per-voice performer that plays it. See
//! `docs/spec/instrument-sustained-models.md`.

use super::{ModalBank, model::MAX_MODEL_MODES, model::MAX_VOICE_SINES};
use crate::{Svf, SvfCoefficients};
use std::f64::consts::{LN_10, PI, TAU};

/// 2/3-octave noise bands centred at `80 * 2^(2b/3)` Hz.
pub const NOISE_BANDS: usize = 11;
/// Half-power width of a 2/3-octave band: `1 / (2^(1/3) - 2^(-1/3))`.
pub const NOISE_Q: f64 = 2.145;
/// Number of f32 values of an encoded layer.
pub const SUSTAIN_FIELDS: usize = 14 + NOISE_BANDS + 4 * MAX_MODEL_MODES;
/// Samples between control updates of a held voice.
pub const CONTROL: usize = 32;
/// Envelope grid: 1/8 octave from 2^-8 to 2^8.
pub const ENVELOPE_POINTS: usize = 129;
const ENVELOPE_STEPS_PER_OCTAVE: f64 = 8.0;
const ENVELOPE_LOW_OCTAVE: f64 = -8.0;
/// Largest body-envelope correction and vibrato-coupled swing, in dB.
pub const MAX_CORRECTION_DB: f64 = 24.0;
pub const MAX_SWING_DB: f64 = 12.0;
/// A falling voice ends this far down.
const FALL_END: f64 = 1e-3;
/// Largest shimmer (RMS of a partial's level fluctuation, in dB) and its bandwidth's bounds.
pub const MAX_SHIMMER_DB: f32 = 9.0;
pub const MIN_SHIMMER_HZ: f32 = 0.5;
pub const MAX_SHIMMER_HZ: f32 = 20.0;

/// Centre frequency of noise band `band`.
pub fn noise_center_hz(band: usize) -> f64 {
    80.0 * 2.0_f64.powf(2.0 * band as f64 / 3.0)
}

/// The held layer of one recording. Every field is finite; zero means "none".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sustain {
    pub vibrato_hz: f32,
    /// Peak deviation in cents.
    pub vibrato_cents: f32,
    pub vibrato_delay: f32,
    pub vibrato_fade: f32,
    pub tremolo_hz: f32,
    /// Peak amplitude deviation as a fraction.
    pub tremolo_depth: f32,
    /// Standard deviation of the slow pitch wander, in cents, and its bandwidth.
    pub drift_cents: f32,
    pub drift_hz: f32,
    /// T60 after note-off.
    pub release_seconds: f32,
    /// How much of the spectral envelope stays in Hz when the key moves.
    pub body_fixed: f32,
    /// Length of the recording's attack, for the noise hand-over.
    pub attack_seconds: f32,
    pub noise_onset: f32,
    pub noise_rise: f32,
    /// Reserved, zero: keeps the field count even for a later addition.
    pub reserved: f32,
    /// Amplitude density per band, in the recording's normalized units per square-root hertz.
    pub noise: [f32; NOISE_BANDS],
    /// Per mode, by index: onset (s), rise from 10 % to 90 % (s), vibrato-coupled level in dB
    /// per cent and per cent squared.
    pub onset: [f32; MAX_MODEL_MODES],
    pub rise: [f32; MAX_MODEL_MODES],
    pub am_slope: [f32; MAX_MODEL_MODES],
    pub am_curve: [f32; MAX_MODEL_MODES],
    /// Liveliness (schema 7, outside the encoded fields above): per mode, the RMS in dB of its
    /// level's fast fluctuation, and the fluctuation's bandwidth shared by the modes.
    pub shimmer: [f32; MAX_MODEL_MODES],
    pub shimmer_hz: f32,
}

impl Default for Sustain {
    fn default() -> Self {
        Self::NONE
    }
}

impl Sustain {
    pub const NONE: Self = Self {
        vibrato_hz: 0.0,
        vibrato_cents: 0.0,
        vibrato_delay: 0.0,
        vibrato_fade: 0.0,
        tremolo_hz: 0.0,
        tremolo_depth: 0.0,
        drift_cents: 0.0,
        drift_hz: 0.0,
        release_seconds: 0.0,
        body_fixed: 0.0,
        attack_seconds: 0.0,
        noise_onset: 0.0,
        noise_rise: 0.0,
        reserved: 0.0,
        noise: [0.0; NOISE_BANDS],
        onset: [0.0; MAX_MODEL_MODES],
        rise: [0.0; MAX_MODEL_MODES],
        am_slope: [0.0; MAX_MODEL_MODES],
        am_curve: [0.0; MAX_MODEL_MODES],
        shimmer: [0.0; MAX_MODEL_MODES],
        shimmer_hz: 0.0,
    };

    /// Whether every value is zero, which is the same as no layer.
    pub fn is_empty(&self) -> bool {
        self.fields().iter().all(|v| *v == 0.0) && !self.lively()
    }

    /// Whether the layer carries liveliness (schema 7).
    pub fn lively(&self) -> bool {
        self.shimmer.iter().any(|v| *v != 0.0) || self.shimmer_hz != 0.0
    }

    /// The values in their encoded order.
    pub fn fields(&self) -> [f32; SUSTAIN_FIELDS] {
        let mut out = [0.0; SUSTAIN_FIELDS];
        let scalars = [
            self.vibrato_hz,
            self.vibrato_cents,
            self.vibrato_delay,
            self.vibrato_fade,
            self.tremolo_hz,
            self.tremolo_depth,
            self.drift_cents,
            self.drift_hz,
            self.release_seconds,
            self.body_fixed,
            self.attack_seconds,
            self.noise_onset,
            self.noise_rise,
            self.reserved,
        ];
        let values = scalars
            .iter()
            .chain(&self.noise)
            .chain(&self.onset)
            .chain(&self.rise)
            .chain(&self.am_slope)
            .chain(&self.am_curve);
        for (slot, value) in out.iter_mut().zip(values) {
            *slot = *value;
        }
        out
    }

    /// The layer whose encoded values are `fields`, unvalidated.
    pub fn from_fields(fields: &[f32; SUSTAIN_FIELDS]) -> Self {
        let mut values = fields.iter().copied();
        let mut next = || values.next().unwrap_or(0.0);
        let mut layer = Self::NONE;
        layer.vibrato_hz = next();
        layer.vibrato_cents = next();
        layer.vibrato_delay = next();
        layer.vibrato_fade = next();
        layer.tremolo_hz = next();
        layer.tremolo_depth = next();
        layer.drift_cents = next();
        layer.drift_hz = next();
        layer.release_seconds = next();
        layer.body_fixed = next();
        layer.attack_seconds = next();
        layer.noise_onset = next();
        layer.noise_rise = next();
        layer.reserved = next();
        let arrays: [&mut [f32]; 5] =
            [&mut layer.noise, &mut layer.onset, &mut layer.rise, &mut layer.am_slope, &mut layer.am_curve];
        for array in arrays {
            for slot in array {
                *slot = next();
            }
        }
        layer
    }

    /// Checks every bound for a recording with `modes` modes.
    pub fn validate(&self, modes: usize) -> Result<(), &'static str> {
        let fields = self.fields();
        if !fields.iter().all(|v| v.is_finite()) {
            return Err("a held layer needs finite values");
        }
        let optional = |value: f32, low: f32, high: f32| value == 0.0 || (low..=high).contains(&value);
        let within = |value: f32, low: f32, high: f32| (low..=high).contains(&value);
        let vibrato = if self.vibrato_hz == 0.0 {
            self.vibrato_cents == 0.0 && self.vibrato_delay == 0.0 && self.vibrato_fade == 0.0
        } else {
            within(self.vibrato_hz, 3.0, 10.0)
                && within(self.vibrato_cents, 5.0, 200.0)
                && within(self.vibrato_delay, 0.0, 4.0)
                && within(self.vibrato_fade, 0.0, 4.0)
        };
        let tremolo = if self.tremolo_hz == 0.0 {
            self.tremolo_depth == 0.0
        } else {
            within(self.tremolo_hz, 3.0, 10.0) && within(self.tremolo_depth, 0.0, 0.9)
        };
        let drift = if self.drift_cents == 0.0 {
            self.drift_hz == 0.0
        } else {
            within(self.drift_cents, 1.0, 20.0) && within(self.drift_hz, 0.5, 10.0)
        };
        let scalars = vibrato
            && tremolo
            && drift
            && optional(self.release_seconds, 0.01, 20.0)
            && within(self.body_fixed, 0.0, 1.0)
            && within(self.attack_seconds, 0.0, 0.25)
            && within(self.noise_onset, 0.0, 1.0)
            && within(self.noise_rise, 0.0, 4.0)
            && self.reserved == 0.0
            && self.noise.iter().all(|v| within(*v, 0.0, 1.0));
        let shimmer_on = self.shimmer.iter().any(|v| *v != 0.0);
        let shimmer = self.shimmer.iter().all(|v| v.is_finite() && within(*v, 0.0, MAX_SHIMMER_DB))
            && self.shimmer.iter().skip(modes).all(|v| *v == 0.0)
            && if shimmer_on {
                self.shimmer_hz.is_finite() && within(self.shimmer_hz, MIN_SHIMMER_HZ, MAX_SHIMMER_HZ)
            } else {
                self.shimmer_hz == 0.0
            };
        let per_mode = (0..MAX_MODEL_MODES).all(|i| {
            let get = |a: &[f32; MAX_MODEL_MODES]| a.get(i).copied().unwrap_or(0.0);
            let (onset, rise, slope, curve) =
                (get(&self.onset), get(&self.rise), get(&self.am_slope), get(&self.am_curve));
            if i >= modes {
                onset == 0.0 && rise == 0.0 && slope == 0.0 && curve == 0.0
            } else {
                within(onset, 0.0, 1.0)
                    && within(rise, 0.0, 4.0)
                    && within(slope, -0.5, 0.5)
                    && within(curve, -0.01, 0.01)
            }
        });
        if scalars && per_mode && shimmer { Ok(()) } else { Err("a held layer is outside its bounds") }
    }
}

/// A recording's spectral envelope in dB on the 1/8-octave grid of ratios, from its modes'
/// (ratio, magnitude): an upper envelope through the partial peaks. A mode more than
/// `DIP_DB` below the straight line (dB over log frequency) between its neighbours is a dip
/// that belongs to the source (a clarinet's weak even partials) and is left out, the deepest
/// first, until none is; straight lines join the rest. Above the highest mode it continues with
/// the top octave's tilt (-24..0 dB per octave), below the lowest it is flat. Off the audio
/// thread.
pub fn envelope(modes: &[(f64, f64)]) -> [f32; ENVELOPE_POINTS] {
    const DIP_DB: f64 = 6.0;
    let mut table = [0.0_f32; ENVELOPE_POINTS];
    let mut points: Vec<(f64, f64)> = modes
        .iter()
        .filter(|(r, m)| *r > 0.0 && *m > 0.0 && r.is_finite() && m.is_finite())
        .map(|(r, m)| (r.log2(), 20.0 * m.log10()))
        .collect();
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    points.dedup_by(|a, b| a.0 == b.0);
    // At most one removal per point: bounded by the mode count.
    while points.len() > 2 {
        let mut deepest: Option<(usize, f64)> = None;
        for (index, window) in points.windows(3).enumerate() {
            let &[(x0, y0), (x, y), (x1, y1)] = window else { continue };
            let dip = y0 + (y1 - y0) * (x - x0) / (x1 - x0) - y;
            if dip > DIP_DB && deepest.is_none_or(|(_, d)| dip > d) {
                deepest = Some((index + 1, dip));
            }
        }
        let Some((index, _)) = deepest else { break };
        points.remove(index);
    }
    let (Some(&(low, at_low)), Some(&(high, at_high))) = (points.first(), points.last()) else {
        return table;
    };
    let top: Vec<(f64, f64)> = points.iter().copied().filter(|p| p.0 >= high - 1.0).collect();
    let tilt = if top.len() >= 2 {
        let n = top.len() as f64;
        let mean_x = top.iter().map(|p| p.0).sum::<f64>() / n;
        let mean_y = top.iter().map(|p| p.1).sum::<f64>() / n;
        let variance = top.iter().map(|p| (p.0 - mean_x).powi(2)).sum::<f64>();
        if variance > 0.0 {
            (top.iter().map(|p| (p.0 - mean_x) * (p.1 - mean_y)).sum::<f64>() / variance).clamp(-24.0, 0.0)
        } else {
            0.0
        }
    } else {
        0.0
    };
    for (index, slot) in table.iter_mut().enumerate() {
        let octave = ENVELOPE_LOW_OCTAVE + index as f64 / ENVELOPE_STEPS_PER_OCTAVE;
        let value = if octave >= high {
            at_high + tilt * (octave - high)
        } else if octave <= low {
            at_low
        } else {
            let after = points.partition_point(|p| p.0 < octave);
            match (points.get(after.wrapping_sub(1)), points.get(after)) {
                (Some(&(x0, y0)), Some(&(x1, y1))) if x1 > x0 => y0 + (y1 - y0) * (octave - x0) / (x1 - x0),
                _ => at_low,
            }
        };
        *slot = value as f32;
    }
    table
}

/// The envelope's dB value at `ratio`, interpolated on the grid; held at its ends.
pub fn envelope_at(table: &[f32; ENVELOPE_POINTS], ratio: f64) -> f64 {
    if !(ratio.is_finite() && ratio > 0.0) {
        return 0.0;
    }
    let position =
        ((ratio.log2() - ENVELOPE_LOW_OCTAVE) * ENVELOPE_STEPS_PER_OCTAVE).clamp(0.0, (ENVELOPE_POINTS - 1) as f64);
    let index = position.floor() as usize;
    let fraction = position - index as f64;
    let a = f64::from(table.get(index).copied().unwrap_or(0.0));
    let b = f64::from(table.get(index + 1).copied().unwrap_or(a as f32));
    a + (b - a) * fraction
}

/// The held layer of one played note: what `Performer` needs, per sine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Held {
    pub vibrato_hz: f64,
    pub vibrato_cents: f64,
    pub vibrato_delay: f64,
    pub vibrato_fade: f64,
    pub tremolo_hz: f64,
    pub tremolo_depth: f64,
    pub drift_cents: f64,
    pub drift_hz: f64,
    pub release_seconds: f64,
    pub attack_seconds: f64,
    pub noise_onset: f64,
    pub noise_rise: f64,
    pub noise: [f64; NOISE_BANDS],
    pub onset: [f32; MAX_VOICE_SINES],
    pub rise: [f32; MAX_VOICE_SINES],
    pub am_slope: [f32; MAX_VOICE_SINES],
    pub am_curve: [f32; MAX_VOICE_SINES],
    /// Per sine, its level's fluctuation (RMS dB), and the fluctuation's bandwidth.
    pub shimmer: [f32; MAX_VOICE_SINES],
    pub shimmer_hz: f64,
}

impl Default for Held {
    fn default() -> Self {
        Self::NONE
    }
}

impl Held {
    pub const NONE: Self = Self {
        vibrato_hz: 0.0,
        vibrato_cents: 0.0,
        vibrato_delay: 0.0,
        vibrato_fade: 0.0,
        tremolo_hz: 0.0,
        tremolo_depth: 0.0,
        drift_cents: 0.0,
        drift_hz: 0.0,
        release_seconds: 0.0,
        attack_seconds: 0.0,
        noise_onset: 0.0,
        noise_rise: 0.0,
        noise: [0.0; NOISE_BANDS],
        onset: [0.0; MAX_VOICE_SINES],
        rise: [0.0; MAX_VOICE_SINES],
        am_slope: [0.0; MAX_VOICE_SINES],
        am_curve: [0.0; MAX_VOICE_SINES],
        shimmer: [0.0; MAX_VOICE_SINES],
        shimmer_hz: 0.0,
    };

    /// Whether anything here changes a voice: otherwise it plays as plain revision 4.
    pub fn is_active(&self) -> bool {
        let any = |a: &[f32]| a.iter().any(|v| *v != 0.0);
        self.vibrato_hz > 0.0
            || self.tremolo_hz > 0.0
            || self.drift_cents > 0.0
            || self.release_seconds > 0.0
            || self.noise.iter().any(|v| *v > 0.0)
            || any(&self.onset[..])
            || any(&self.rise[..])
            || any(&self.am_slope[..])
            || any(&self.am_curve[..])
            || (self.shimmer_hz > 0.0 && any(&self.shimmer[..]))
    }
}

/// Seeded xorshift32: uniform in [-1, 1).
#[derive(Debug, Clone, Copy)]
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        f64::from(x) / 2_147_483_648.0 - 1.0
    }
}

/// Plays a `Held` layer on a voice's fitted bank: control every `CONTROL` samples (pitch,
/// per-sine levels, noise level, release), linear level ramps in between. Fixed storage; no
/// allocation, locking or I/O.
#[derive(Debug, Clone, Copy)]
pub struct Performer {
    held: Held,
    active: bool,
    sample_rate: f64,
    gains: [f32; MAX_VOICE_SINES],
    steps: [f32; MAX_VOICE_SINES],
    bands: [Svf; NOISE_BANDS],
    band_gains: [f64; NOISE_BANDS],
    noise_on: bool,
    generators: Noise,
    noise_level: f64,
    noise_step: f64,
    counter: usize,
    /// Control updates since the note-on.
    ticks: u64,
    bend: f64,
    drift: f64,
    drift_noise: Noise,
    /// Level of the noise layer after note-off and its factor per update (the damper fades the
    /// sines).
    fall: f64,
    fall_factor: f64,
    retune: bool,
    /// Per sine, its level fluctuation: unit-variance low-passed noise, and its generator.
    shimmer: [f32; MAX_VOICE_SINES],
    shimmer_noise: Noise,
}

impl Default for Performer {
    fn default() -> Self {
        Self {
            held: Held::NONE,
            active: false,
            sample_rate: 48000.0,
            gains: [1.0; MAX_VOICE_SINES],
            steps: [0.0; MAX_VOICE_SINES],
            bands: [Svf::default(); NOISE_BANDS],
            band_gains: [0.0; NOISE_BANDS],
            noise_on: false,
            generators: Noise(1),
            noise_level: 0.0,
            noise_step: 0.0,
            counter: 0,
            ticks: 0,
            bend: 0.0,
            drift: 0.0,
            drift_noise: Noise(1),
            fall: 1.0,
            fall_factor: 1.0,
            retune: false,
            shimmer: [0.0; MAX_VOICE_SINES],
            shimmer_noise: Noise(1),
        }
    }
}

impl Performer {
    /// Start a note. An inactive `held` leaves the performer inactive, and the caller plays
    /// the bank as before. `seed` makes the noise and drift repeatable.
    pub fn start(&mut self, held: &Held, sample_rate: f64, bend_semitones: f64, seed: u32) {
        *self = Self::default();
        if !held.is_active() || !sample_rate.is_finite() || !(1000.0..=384000.0).contains(&sample_rate) {
            return;
        }
        self.held = *held;
        self.active = true;
        self.sample_rate = sample_rate;
        self.bend = if bend_semitones.is_finite() { bend_semitones } else { 0.0 };
        self.retune = held.vibrato_hz > 0.0 || held.drift_cents > 0.0;
        // Nonzero, distinct seeds for the noise and the drift.
        self.generators = Noise(seed.wrapping_mul(2_654_435_761).max(1));
        self.drift_noise = Noise(seed.wrapping_mul(40_503).wrapping_add(0x9E37_79B9).max(1));
        self.shimmer_noise = Noise(seed.wrapping_mul(0x85EB_CA6B).wrapping_add(0x27D4_EB2F).max(1));
        let scale = noise_gain(sample_rate);
        for (band, ((filter, gain), density)) in
            self.bands.iter_mut().zip(self.band_gains.iter_mut()).zip(held.noise).enumerate()
        {
            let center = noise_center_hz(band);
            if density > 0.0 && center < sample_rate * 0.45 {
                *filter = Svf::new(SvfCoefficients::bandpass(sample_rate, center, NOISE_Q));
                *gain = density * scale;
                self.noise_on = true;
            }
        }
        let (gains, noise) = self.levels(0.0, 0.0);
        self.gains = gains;
        self.noise_level = noise;
        self.counter = 0;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Whether the noise layer still sounds (the bank reports its own sines).
    pub fn sounding(&self) -> bool {
        self.active && self.noise_on && self.fall > FALL_END
    }

    /// Whether the noise layer's release has reached its end.
    pub fn finished(&self) -> bool {
        self.active && self.fall <= FALL_END
    }

    /// Let go of the note with the Release control at `release` seconds: the T60 the caller's
    /// damper caps the sines at, which is the measured release scaled by `release / 0.25` (the
    /// control's default) when there is one, else `release`. The noise layer falls with it.
    pub fn release(&mut self, release: f64) -> f64 {
        let release = if release.is_finite() { release.max(0.001) } else { 0.25 };
        if !self.active {
            return release;
        }
        let t60 = if self.held.release_seconds > 0.0 {
            (self.held.release_seconds * release / 0.25).clamp(0.005, 60.0)
        } else {
            release
        };
        self.fall_factor = (-1000.0_f64.ln() * CONTROL as f64 / (t60 * self.sample_rate)).exp();
        t60
    }

    /// Set the pitch-wheel offset; the bank is retuned with the current deviation.
    pub fn bend(&mut self, bank: &mut ModalBank<MAX_VOICE_SINES>, semitones: f64) {
        if semitones.is_finite() {
            self.bend = semitones;
        }
        bank.transpose(self.sample_rate, self.bend + self.drift_and_vibrato(self.time()) / 100.0);
    }

    fn time(&self) -> f64 {
        (self.ticks as f64 * CONTROL as f64) / self.sample_rate
    }

    /// 0 before the vibrato's delay, rising linearly to 1 over its fade.
    fn depth(&self, t: f64) -> f64 {
        let held = &self.held;
        if held.vibrato_hz <= 0.0 || t < held.vibrato_delay {
            0.0
        } else if held.vibrato_fade > 0.0 {
            ((t - held.vibrato_delay) / held.vibrato_fade).min(1.0)
        } else {
            1.0
        }
    }

    fn vibrato(&self, t: f64) -> f64 {
        self.depth(t) * self.held.vibrato_cents * (TAU * self.held.vibrato_hz * t).sin()
    }

    fn drift_and_vibrato(&self, t: f64) -> f64 {
        self.vibrato(t) + self.drift
    }

    /// Per-sine levels and the noise level at time `t`, with vibrato deviation `u` (cents).
    fn levels(&self, t: f64, u: f64) -> ([f32; MAX_VOICE_SINES], f64) {
        let held = &self.held;
        let e = self.depth(t);
        let mean_square = e * e * held.vibrato_cents * held.vibrato_cents / 2.0;
        let tremolo =
            if held.tremolo_hz > 0.0 { 1.0 + held.tremolo_depth * (TAU * held.tremolo_hz * t).sin() } else { 1.0 };
        let common = tremolo;
        let mut gains = [0.0_f32; MAX_VOICE_SINES];
        for (i, gain) in gains.iter_mut().enumerate() {
            let at = |a: &[f32; MAX_VOICE_SINES]| f64::from(a.get(i).copied().unwrap_or(0.0));
            let onset = rise_level(t, at(&held.onset), at(&held.rise));
            // A sine's own fluctuation adds to the vibrato-coupled swing.
            let shimmer = at(&held.shimmer) * f64::from(self.shimmer.get(i).copied().unwrap_or(0.0));
            let swing = (at(&held.am_slope) * u + at(&held.am_curve) * (u * u - mean_square) + shimmer)
                .clamp(-MAX_SWING_DB, MAX_SWING_DB);
            *gain = (onset * (swing * LN_10 / 20.0).exp() * common) as f32;
        }
        let handover = if held.attack_seconds > 0.0 {
            let half = held.attack_seconds / 2.0;
            if t <= half {
                0.0
            } else if t >= held.attack_seconds {
                1.0
            } else {
                0.5 - 0.5 * (PI * (t - half) / half).cos()
            }
        } else {
            1.0
        };
        let noise = rise_level(t, held.noise_onset, held.noise_rise) * handover * self.fall;
        (gains, noise)
    }

    /// One control update: the levels and pitch at the end of the next `CONTROL` samples.
    fn control(&mut self, bank: &mut ModalBank<MAX_VOICE_SINES>) {
        self.ticks = self.ticks.saturating_add(1);
        let t = self.time();
        // Each sine's level fluctuation: one-pole low-passed noise of unit variance at the
        // shimmer's bandwidth.
        if self.held.shimmer_hz > 0.0 {
            let a = (-TAU * self.held.shimmer_hz * CONTROL as f64 / self.sample_rate).exp();
            let b = (1.0 - a * a).sqrt() * 3.0_f64.sqrt();
            for (state, depth) in self.shimmer.iter_mut().zip(&self.held.shimmer) {
                if *depth > 0.0 {
                    *state = (a * f64::from(*state) + b * self.shimmer_noise.next()) as f32;
                }
            }
        }
        if self.held.drift_cents > 0.0 {
            let a = (-TAU * self.held.drift_hz * CONTROL as f64 / self.sample_rate).exp();
            let b = self.held.drift_cents * (1.0 - a * a).sqrt();
            // Uniform noise of unit variance.
            self.drift = a * self.drift + b * 3.0_f64.sqrt() * self.drift_noise.next();
        }
        self.fall *= self.fall_factor;
        let u = self.vibrato(t);
        let (targets, noise) = self.levels(t, u);
        for ((step, gain), target) in self.steps.iter_mut().zip(&self.gains).zip(targets) {
            *step = (target - gain) / CONTROL as f32;
        }
        self.noise_step = (noise - self.noise_level) / CONTROL as f64;
        if self.retune {
            bank.transpose(self.sample_rate, self.bend + (u + self.drift) / 100.0);
        }
    }

    /// The next sample of the bank with this layer, plus the noise.
    pub fn next(&mut self, bank: &mut ModalBank<MAX_VOICE_SINES>) -> f32 {
        if !self.active {
            return bank.next_sample();
        }
        if self.counter == 0 {
            self.control(bank);
        }
        self.counter = (self.counter + 1) % CONTROL;
        let output = f64::from(bank.next_sample_gained(&mut self.gains, &self.steps));
        (output + self.next_noise()) as f32
    }

    /// [`next`](Self::next) into the bank's two outputs ([`ModalBank::next_stereo`]); the noise
    /// is the same in both.
    pub fn next_stereo(&mut self, bank: &mut ModalBank<MAX_VOICE_SINES>) -> [f32; 2] {
        if !self.active {
            return bank.next_stereo();
        }
        if self.counter == 0 {
            self.control(bank);
        }
        self.counter = (self.counter + 1) % CONTROL;
        let [first, second] = bank.next_stereo_gained(&mut self.gains, &self.steps);
        let noise = self.next_noise();
        [(f64::from(first) + noise) as f32, (f64::from(second) + noise) as f32]
    }

    fn next_noise(&mut self) -> f64 {
        if !self.noise_on {
            return 0.0;
        }
        let mut sum = 0.0;
        for (filter, gain) in self.bands.iter_mut().zip(&self.band_gains) {
            // Every band draws, so each has its own independent sequence.
            let white = self.generators.next() * 3.0_f64.sqrt();
            if *gain > 0.0 {
                sum += gain * filter.process(white);
            }
        }
        let output = self.noise_level * sum;
        self.noise_level += self.noise_step;
        output
    }
}

/// Gain that turns a band's amplitude density into the level of its unit-variance noise through
/// a unity-peak band-pass: the bands tile log frequency at 2/3 octave, where a band's squared
/// response integrates to `I = 2a acos(a) / sqrt(1 - a^2)` with `a = 1 / (2Q)`, so
/// `gain = sqrt(sample_rate ln 2 / (3 I))` (about `0.6007 sqrt(sample_rate)`).
pub fn noise_gain(sample_rate: f64) -> f64 {
    let a = 1.0 / (2.0 * NOISE_Q);
    let integral = 2.0 * a * a.acos() / (1.0 - a * a).sqrt();
    (sample_rate * std::f64::consts::LN_2 / (3.0 * integral)).sqrt()
}

/// `1 - exp(-(t - onset) ln 10 / rise)` after the onset (90 % at `onset + rise`), 0 before;
/// 1 with no rise or onset.
fn rise_level(t: f64, onset: f64, rise: f64) -> f64 {
    if t < onset {
        0.0
    } else if rise > 0.0 {
        1.0 - (-(t - onset) * LN_10 / rise).exp()
    } else {
        1.0
    }
}

#[cfg(test)]
#[path = "sustain_tests.rs"]
mod tests;

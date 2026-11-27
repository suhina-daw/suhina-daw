// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A held note continued past the end of its recording: the stochastic harmonic model fitted to
//! the recording's held part (`Continuation`), its encoding, the transposed and morphed models a
//! note plays, and the allocation-free per-voice generator ([`voice::Generator`]). See
//! `docs/spec/instrument-held-continuation.md`.
//!
//! Decoding, preparing and building generators allocate (off the audio thread); starting a note,
//! transposing, morphing and rendering do not.

pub mod math;
pub mod voice;

use math::{Butter3, C, filtered_ar_power};
use std::f64::consts::{LN_10, TAU};

/// Most harmonics a continuation carries (a 130 Hz note at 44.1 kHz has 152).
pub const MAX_PARTIALS: usize = 160;
/// Order of each diffuse part's AR model.
pub const AR_ORDER: usize = 6;
/// Frames of each diffuse part kept to continue from.
pub const HISTORY: usize = 30;
/// Rate of the control frames the diffuse parts and the pitch run at.
pub const CONTROL_HZ: f64 = 300.0;
/// Order of the jitter's AR model.
pub const JITTER_ORDER: usize = 4;
/// Noise spectra: 1/12 octave from 20 Hz, 121 points to 20480 Hz, in dB.
pub const NOISE_POINTS: usize = 121;
pub const NOISE_LOW_HZ: f64 = 20.0;
pub const NOISE_STEPS_PER_OCTAVE: f64 = 12.0;
/// Partials above this share of the sample rate are left out.
pub const PARTIAL_LIMIT: f64 = 0.46;
/// Diffuse parts are band-limited at twice their bandwidth, within these bounds (Hz); from
/// harmonic `GROUP_FROM` on at the demodulation's own band.
pub const MIN_CUTOFF_HZ: f64 = 5.0;
pub const DEMOD_HZ: f64 = 60.0;
pub const GROUP_FROM: usize = 12;
/// First bytes of an encoded continuation and its version.
pub const MAGIC: &[u8; 8] = b"SHMCONT1";
pub const VERSION: u16 = 1;
/// f32 values per partial and of the scalars, in their encoded order.
pub const DIFFUSE_FIELDS: usize = 2 * AR_ORDER + 4 + 2 * HISTORY;
pub const PARTIAL_FIELDS: usize = 7 + DIFFUSE_FIELDS + 2 + DIFFUSE_FIELDS;
pub const SCALAR_FIELDS: usize = 26;

/// One narrowband random process: a complex AR model, its innovation variance and power, the
/// low-pass it runs through, the factor that brings the filtered process back to its power, and
/// the recording's last frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Diffuse {
    pub ar: [[f32; 2]; AR_ORDER],
    pub innovation: f32,
    pub power: f32,
    pub cutoff_hz: f32,
    pub power_fix: f32,
    pub history: [[f32; 2]; HISTORY],
}

impl Diffuse {
    pub const NONE: Self = Self {
        ar: [[0.0; 2]; AR_ORDER],
        innovation: 0.0,
        power: 0.0,
        cutoff_hz: 0.0,
        power_fix: 0.0,
        history: [[0.0; 2]; HISTORY],
    };

    fn fields(&self) -> impl Iterator<Item = f32> + '_ {
        self.ar
            .iter()
            .flatten()
            .copied()
            .chain([self.innovation, self.power, self.cutoff_hz, self.power_fix])
            .chain(self.history.iter().flatten().copied())
    }

    fn read(values: &mut impl Iterator<Item = f32>) -> Self {
        let mut next = || values.next().unwrap_or(0.0);
        let mut diffuse = Self::NONE;
        for pair in &mut diffuse.ar {
            *pair = [next(), next()];
        }
        diffuse.innovation = next();
        diffuse.power = next();
        diffuse.cutoff_hz = next();
        diffuse.power_fix = next();
        for pair in &mut diffuse.history {
            *pair = [next(), next()];
        }
        diffuse
    }

    fn validate(&self) -> bool {
        let finite = self.fields().all(f32::is_finite);
        finite
            && self.ar.iter().flatten().all(|a| a.abs() <= 64.0)
            && self.innovation >= 0.0
            && self.power >= 0.0
            && self.innovation <= self.power.max(0.0) * 1.0001 + 1e-30
            && (self.power == 0.0 || (MIN_CUTOFF_HZ as f32..=DEMOD_HZ as f32).contains(&self.cutoff_hz))
            && (0.0..=64.0).contains(&self.power_fix)
    }

    /// The AR coefficients as complex values.
    pub fn coefficients(&self) -> [C; AR_ORDER] {
        std::array::from_fn(|k| self.ar.get(k).map_or(C::ZERO, |p| C::from_pair(*p)))
    }

    /// The factor that brings this process, through its low-pass, back to its own power.
    pub fn fix_for(ar: &[C; AR_ORDER], innovation: f64, power: f64, cutoff: f64) -> f64 {
        let filter = Butter3::new(cutoff, CONTROL_HZ);
        let filtered = filtered_ar_power(ar, innovation, &filter, CONTROL_HZ);
        if filtered > 0.0 && power > 0.0 { (power / filtered).sqrt().clamp(0.0, 64.0) } else { 0.0 }
    }
}

/// One harmonic: its carrier, how much of the vibrato it follows, its level statistics, the
/// factors fitted on trial renders, its diffuse part, and for a stereo recording the side's
/// panning and independent part.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Partial {
    pub carrier: [f32; 2],
    /// Share of the pitch deviation the partial follows (0..1.3).
    pub gamma: f32,
    /// Mean of `ln |a|` and the standard deviation of its dB level over the held part.
    pub mean_log: f32,
    pub sigma_db: f32,
    /// Spread factor (0.25..1): how far the rendered level swings are kept.
    pub spread: f32,
    /// The partial's level just before the join, for the first 20 ms.
    pub start_level: f32,
    pub diffuse: Diffuse,
    pub side_rho: [f32; 2],
    pub side: Diffuse,
}

impl Partial {
    pub const NONE: Self = Self {
        carrier: [0.0; 2],
        gamma: 1.0,
        mean_log: 0.0,
        sigma_db: 0.0,
        spread: 1.0,
        start_level: 0.0,
        diffuse: Diffuse::NONE,
        side_rho: [0.0; 2],
        side: Diffuse::NONE,
    };

    fn fields(&self) -> impl Iterator<Item = f32> + '_ {
        self.carrier
            .iter()
            .copied()
            .chain([self.gamma, self.mean_log, self.sigma_db, self.spread, self.start_level])
            .chain(self.diffuse.fields())
            .chain(self.side_rho.iter().copied())
            .chain(self.side.fields())
    }

    fn read(values: &mut impl Iterator<Item = f32>) -> Self {
        let mut partial = Self::NONE;
        let mut take = || values.next().unwrap_or(0.0);
        partial.carrier = [take(), take()];
        partial.gamma = take();
        partial.mean_log = take();
        partial.sigma_db = take();
        partial.spread = take();
        partial.start_level = take();
        partial.diffuse = Diffuse::read(values);
        let mut take = || values.next().unwrap_or(0.0);
        partial.side_rho = [take(), take()];
        partial.side = Diffuse::read(values);
        partial
    }

    /// The expected power the partial adds: carrier plus diffuse part.
    pub fn power(&self) -> f64 {
        C::from_pair(self.carrier).norm_sqr() + f64::from(self.diffuse.power)
    }
}

/// A recording's continuation. Frequencies are in Hz, levels in the recording's own sample
/// units, times in seconds from the recording's start.
#[derive(Debug, Clone, PartialEq)]
pub struct Continuation {
    /// Sample rate of the recording it was fitted on.
    pub recording_rate: f32,
    /// Mean pitch (the geometric mean of the held part's pitch track).
    pub f0: f32,
    pub vibrato_hz: f32,
    /// Peak deviation in cents.
    pub vibrato_cents: f32,
    /// The vibrato's phase at the join.
    pub vibrato_phase: f32,
    /// The harmonic phase `phi` at the join (mod 2 pi) and the phase the pitch deviation added,
    /// `Psi`, there.
    pub phase: f32,
    pub deviation_phase: f32,
    /// Where the model takes over, and where the recording's own release starts (0: none).
    pub join: f32,
    pub release_start: f32,
    pub jitter_ar: [f32; JITTER_ORDER],
    pub jitter_innovation: f32,
    /// The jitter's standard deviation in cents and the gain that gives it through the causal
    /// low-pass.
    pub jitter_cents: f32,
    pub jitter_gain: f32,
    /// The level gain (dB) that brings the model to the recording's level before the join.
    pub level_db: f32,
    /// Vibrato-locked level coefficients (dB per cent, per cent of velocity, per cent squared) of
    /// the recording and of the model's trial renders.
    pub coef_target: [f32; 3],
    pub coef_model: [f32; 3],
    /// Residual noise: spectrum shape (dB, `NOISE_POINTS`) and RMS; the side's the same.
    pub noise_db: [f32; NOISE_POINTS],
    pub noise_rms: f32,
    pub side_noise_db: [f32; NOISE_POINTS],
    pub side_noise_rms: f32,
    /// Whether the recording had a side; without one every side field is zero.
    pub stereo: bool,
    pub partials: Vec<Partial>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContinuationError(pub &'static str);

impl std::fmt::Display for ContinuationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ContinuationError {}

/// The noise grid's frequency at point `index`.
pub fn noise_point_hz(index: usize) -> f64 {
    NOISE_LOW_HZ * 2.0_f64.powf(index as f64 / NOISE_STEPS_PER_OCTAVE)
}

/// A noise spectrum's dB value at `hz`, interpolated in log frequency, held at its ends.
pub fn noise_at(table: &[f32; NOISE_POINTS], hz: f64) -> f64 {
    if !(hz.is_finite() && hz > 0.0) {
        return f64::from(table.first().copied().unwrap_or(0.0));
    }
    let position = ((hz / NOISE_LOW_HZ).log2() * NOISE_STEPS_PER_OCTAVE).clamp(0.0, (NOISE_POINTS - 1) as f64);
    let index = position.floor() as usize;
    let fraction = position - index as f64;
    let a = f64::from(table.get(index).copied().unwrap_or(0.0));
    let b = f64::from(table.get(index + 1).copied().unwrap_or(a as f32));
    a + (b - a) * fraction
}

impl Continuation {
    /// The values before the noise spectra and partials, in their encoded order.
    fn scalars(&self) -> [f32; SCALAR_FIELDS] {
        let [j0, j1, j2, j3] = self.jitter_ar;
        let [t0, t1, t2] = self.coef_target;
        let [m0, m1, m2] = self.coef_model;
        [
            self.recording_rate,
            self.f0,
            self.vibrato_hz,
            self.vibrato_cents,
            self.vibrato_phase,
            self.phase,
            self.deviation_phase,
            self.join,
            self.release_start,
            j0,
            j1,
            j2,
            j3,
            self.jitter_innovation,
            self.jitter_cents,
            self.jitter_gain,
            self.level_db,
            t0,
            t1,
            t2,
            m0,
            m1,
            m2,
            self.noise_rms,
            self.side_noise_rms,
            0.0,
        ]
    }

    pub fn validate(&self) -> Result<(), ContinuationError> {
        let fail = |message| Err(ContinuationError(message));
        if self.partials.is_empty() || self.partials.len() > MAX_PARTIALS {
            return fail("a continuation needs 1..160 partials");
        }
        let scalars = self.scalars();
        let spectra = self.noise_db.iter().chain(&self.side_noise_db);
        if !scalars.iter().chain(spectra).all(|v| v.is_finite()) {
            return fail("a continuation needs finite values");
        }
        let within = |value: f32, low: f32, high: f32| (low..=high).contains(&value);
        let vibrato = if self.vibrato_cents == 0.0 {
            self.vibrato_hz == 0.0 || within(self.vibrato_hz, 3.0, 10.0)
        } else {
            within(self.vibrato_hz, 3.0, 10.0) && within(self.vibrato_cents, 0.0, 200.0)
        };
        let scalars_ok = within(self.recording_rate, 8000.0, 384_000.0)
            && within(self.f0, 20.0, 5000.0)
            && vibrato
            && within(self.vibrato_phase.abs(), 0.0, 1e6)
            && within(self.phase, 0.0, TAU as f32 + 1e-3)
            && within(self.deviation_phase.abs(), 0.0, 1e6)
            && within(self.join, 0.0, 3600.0)
            && (self.release_start == 0.0 || within(self.release_start, self.join, 3600.0))
            && self.jitter_ar.iter().all(|a| a.abs() <= 16.0)
            && self.jitter_innovation >= 0.0
            && within(self.jitter_cents, 0.0, 200.0)
            && within(self.jitter_gain, 0.0, 1e6)
            && within(self.level_db, -60.0, 60.0)
            && self.coef_target.iter().chain(&self.coef_model).all(|c| c.abs() <= 100.0)
            && self.noise_db.iter().chain(&self.side_noise_db).all(|v| within(*v, -300.0, 300.0))
            && within(self.noise_rms, 0.0, 16.0)
            && within(self.side_noise_rms, 0.0, 16.0);
        if !scalars_ok {
            return fail("a continuation is outside its bounds");
        }
        let partials_ok = self.partials.iter().all(|p| {
            p.fields().all(f32::is_finite)
                && C::from_pair(p.carrier).abs() <= 16.0
                && within(p.gamma, 0.0, 1.3)
                && within(p.mean_log, -80.0, 10.0)
                && within(p.sigma_db, 0.0, 200.0)
                && within(p.spread, 0.25, 1.0)
                && within(p.start_level, 0.0, 16.0)
                && p.diffuse.validate()
                && C::from_pair(p.side_rho).abs() <= 64.0
                && p.side.validate()
        });
        if !partials_ok {
            return fail("a continuation's partial is outside its bounds");
        }
        if !self.stereo {
            let silent_side = self.side_noise_rms == 0.0
                && self.side_noise_db.iter().all(|v| *v == 0.0)
                && self.partials.iter().all(|p| p.side_rho == [0.0; 2] && p.side == Diffuse::NONE);
            if !silent_side {
                return fail("a mono continuation has no side");
            }
        }
        Ok(())
    }

    pub fn encode(&self) -> Vec<u8> {
        let count = self.partials.len();
        let mut bytes = Vec::with_capacity(16 + 4 * (SCALAR_FIELDS + 2 * NOISE_POINTS + count * PARTIAL_FIELDS));
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&(count as u16).to_le_bytes());
        bytes.extend_from_slice(&[u8::from(self.stereo), 0, 0, 0]);
        let values = self
            .scalars()
            .into_iter()
            .chain(self.noise_db.iter().copied())
            .chain(self.side_noise_db.iter().copied())
            .chain(self.partials.iter().flat_map(Partial::fields));
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    /// Decodes and validates; only canonical bytes (what [`encode`](Self::encode) writes) are
    /// accepted.
    pub fn decode(bytes: &[u8]) -> Result<Self, ContinuationError> {
        let malformed = ContinuationError("unsupported or malformed continuation");
        let header = bytes.get(..16).ok_or(malformed)?;
        if header.get(..8) != Some(MAGIC.as_slice()) || header.get(8..10) != Some(VERSION.to_le_bytes().as_slice()) {
            return Err(malformed);
        }
        let count = usize::from(u16::from_le_bytes([
            header.get(10).copied().unwrap_or(0),
            header.get(11).copied().unwrap_or(0),
        ]));
        let stereo = match header.get(12..16) {
            Some([0, 0, 0, 0]) => false,
            Some([1, 0, 0, 0]) => true,
            _ => return Err(malformed),
        };
        if !(1..=MAX_PARTIALS).contains(&count) {
            return Err(malformed);
        }
        let expected = 16 + 4 * (SCALAR_FIELDS + 2 * NOISE_POINTS + count * PARTIAL_FIELDS);
        if bytes.len() != expected {
            return Err(malformed);
        }
        let mut values =
            bytes.get(16..).unwrap_or(&[]).as_chunks::<4>().0.iter().map(|chunk| f32::from_le_bytes(*chunk));
        let mut scalar = [0.0_f32; SCALAR_FIELDS];
        for slot in &mut scalar {
            *slot = values.next().unwrap_or(0.0);
        }
        let mut noise_db = [0.0_f32; NOISE_POINTS];
        for slot in &mut noise_db {
            *slot = values.next().unwrap_or(0.0);
        }
        let mut side_noise_db = [0.0_f32; NOISE_POINTS];
        for slot in &mut side_noise_db {
            *slot = values.next().unwrap_or(0.0);
        }
        let mut partials = Vec::with_capacity(count);
        for _ in 0..count {
            partials.push(Partial::read(&mut values));
        }
        let [
            rate,
            f0,
            vhz,
            vcents,
            vphase,
            phase,
            dphase,
            join,
            release,
            j0,
            j1,
            j2,
            j3,
            jinn,
            jcents,
            jgain,
            level,
            t0,
            t1,
            t2,
            m0,
            m1,
            m2,
            nrms,
            srms,
            reserved,
        ] = scalar;
        if reserved != 0.0 {
            return Err(malformed);
        }
        let continuation = Self {
            recording_rate: rate,
            f0,
            vibrato_hz: vhz,
            vibrato_cents: vcents,
            vibrato_phase: vphase,
            phase,
            deviation_phase: dphase,
            join,
            release_start: release,
            jitter_ar: [j0, j1, j2, j3],
            jitter_innovation: jinn,
            jitter_cents: jcents,
            jitter_gain: jgain,
            level_db: level,
            coef_target: [t0, t1, t2],
            coef_model: [m0, m1, m2],
            noise_db,
            noise_rms: nrms,
            side_noise_db,
            side_noise_rms: srms,
            stereo,
            partials,
        };
        continuation.validate()?;
        if continuation.encode() != bytes {
            return Err(malformed);
        }
        Ok(continuation)
    }

    /// The expected RMS of the model's mid before its gains: half the partials' powers plus the
    /// noise power.
    pub fn expected_rms(&self) -> f64 {
        let partials: f64 = self.partials.iter().map(|p| p.power() / 2.0).sum();
        (partials + f64::from(self.noise_rms).powi(2)).sqrt()
    }
}

/// What a note plays: the model, transposed and morphed, in fixed storage. Built at note start
/// without allocating.
#[derive(Debug, Clone)]
pub struct NoteModel {
    pub f0: f64,
    pub vibrato_hz: f64,
    pub vibrato_cents: f64,
    pub vibrato_phase: f64,
    pub phase: f64,
    pub deviation_phase: f64,
    pub jitter_ar: [f64; JITTER_ORDER],
    pub jitter_innovation: f64,
    pub jitter_gain: f64,
    pub level_db: f64,
    pub coef_target: [f64; 3],
    pub coef_model: [f64; 3],
    pub stereo: bool,
    /// Whether the diffuse parts continue from the recording's history (an untransposed note of
    /// one model) or start empty, generated a while first.
    pub from_history: bool,
    pub partials: [Partial; MAX_PARTIALS],
    pub count: usize,
}

impl Default for NoteModel {
    fn default() -> Self {
        Self {
            f0: 0.0,
            vibrato_hz: 0.0,
            vibrato_cents: 0.0,
            vibrato_phase: 0.0,
            phase: 0.0,
            deviation_phase: 0.0,
            jitter_ar: [0.0; JITTER_ORDER],
            jitter_innovation: 0.0,
            jitter_gain: 0.0,
            level_db: 0.0,
            coef_target: [0.0; 3],
            coef_model: [0.0; 3],
            stereo: false,
            from_history: false,
            partials: [Partial::NONE; MAX_PARTIALS],
            count: 0,
        }
    }
}

/// Fractional index `u` (1-based harmonic number) between `0..count`: the two neighbours and the
/// weight of the second.
fn neighbours(u: f64, count: usize) -> (usize, usize, f64) {
    let last = count.saturating_sub(1);
    let u = if u.is_finite() { u.clamp(1.0, count.max(1) as f64) } else { 1.0 };
    let low = ((u.floor() as usize).saturating_sub(1)).min(last);
    let high = (low + 1).min(last);
    (low, high, u - u.floor())
}

impl NoteModel {
    /// The note `ratio` times the recording's pitch: harmonic `k` takes the recording's values
    /// at its frequency, interpolated between the two neighbours (log carrier magnitude, mean
    /// log level, level spread, diffuse power in log); its phase, line shapes, participation and
    /// side panning come from the nearest. At ratio 1 it is the recording's own model and
    /// continues from its history.
    pub fn transposed(&mut self, source: &Continuation, ratio: f64, sample_rate: f64) {
        let ratio = if ratio.is_finite() && ratio > 0.0 { ratio.clamp(1.0 / 16.0, 16.0) } else { 1.0 };
        let same = (ratio - 1.0).abs() < 1e-9;
        let count = source.partials.len().min(MAX_PARTIALS);
        let f0 = f64::from(source.f0) * ratio;
        let by_rate = (PARTIAL_LIMIT * sample_rate / f0.max(1.0)).floor() as usize;
        let by_source = if same { count } else { (count as f64 / ratio).floor() as usize };
        let new_count = by_rate.min(by_source).clamp(1, MAX_PARTIALS);
        self.f0 = f0;
        self.vibrato_hz = f64::from(source.vibrato_hz);
        self.vibrato_cents = f64::from(source.vibrato_cents);
        self.vibrato_phase = f64::from(source.vibrato_phase);
        self.phase = f64::from(source.phase);
        self.deviation_phase = f64::from(source.deviation_phase);
        self.jitter_ar = source.jitter_ar.map(f64::from);
        self.jitter_innovation = f64::from(source.jitter_innovation);
        self.jitter_gain = f64::from(source.jitter_gain);
        self.coef_target = source.coef_target.map(f64::from);
        self.coef_model = source.coef_model.map(f64::from);
        self.stereo = source.stereo;
        self.from_history = same;
        self.count = new_count;
        for (k, slot) in self.partials.iter_mut().enumerate() {
            if k >= new_count {
                *slot = Partial::NONE;
                continue;
            }
            if same {
                *slot = source.partials.get(k).copied().unwrap_or(Partial::NONE);
                continue;
            }
            let (low, high, f) = neighbours((k + 1) as f64 * ratio, count);
            let (Some(a), Some(b)) = (source.partials.get(low), source.partials.get(high)) else {
                *slot = Partial::NONE;
                continue;
            };
            let near = if f < 0.5 { a } else { b };
            let lin = |x: f64, y: f64| (1.0 - f) * x + f * y;
            let log_mag =
                lin(C::from_pair(a.carrier).abs().max(1e-12).ln(), C::from_pair(b.carrier).abs().max(1e-12).ln());
            let mut out = *near;
            out.carrier = C::cis(C::from_pair(near.carrier).arg()).scale(log_mag.exp()).to_pair();
            out.mean_log = lin(f64::from(a.mean_log), f64::from(b.mean_log)) as f32;
            out.sigma_db = lin(f64::from(a.sigma_db), f64::from(b.sigma_db)) as f32;
            out.spread = lin(f64::from(a.spread), f64::from(b.spread)) as f32;
            out.start_level = 0.0;
            out.diffuse = rescaled(&near.diffuse, lin_log(f64::from(a.diffuse.power), f64::from(b.diffuse.power), f));
            out.side = rescaled(&near.side, lin_log(f64::from(a.side.power), f64::from(b.side.power), f));
            out.diffuse.history = [[0.0; 2]; HISTORY];
            out.side.history = [[0.0; 2]; HISTORY];
            *slot = out;
        }
        // The level gain follows the change of the model's expected level, so a transposed note
        // sounds at the recording's level.
        let before = source.expected_rms();
        let after = self.expected_rms(f64::from(source.noise_rms));
        self.level_db = f64::from(source.level_db) + db_ratio(before, after);
    }

    /// This note's model mixed with `other` (the same note of a second model) at `amount`
    /// (0: this, 1: other). Levels and powers mix in log, participation, panning and the
    /// vibrato linearly; a partial's phase and line shapes come from the model that carries
    /// more of its power; the side's independent power follows the mixed partial.
    pub fn morph(&mut self, other: &NoteModel, amount: f64, noise_rms: f64, other_noise_rms: f64) {
        let w = if amount.is_finite() { amount.clamp(0.0, 1.0) } else { 0.0 };
        if w == 0.0 {
            return;
        }
        let before_a = self.expected_rms(noise_rms);
        let before_b = other.expected_rms(other_noise_rms);
        let count = self.count.min(other.count).max(1);
        let lin = |x: f64, y: f64| (1.0 - w) * x + w * y;
        for (a, b) in self.partials.iter_mut().zip(other.partials.iter()).take(count) {
            let ma = C::from_pair(a.carrier).abs();
            let mb = C::from_pair(b.carrier).abs();
            let pa = f64::from(a.diffuse.power);
            let pb = f64::from(b.diffuse.power);
            let use_a = (1.0 - w) * ma >= w * mb;
            let use_a_diffuse = (1.0 - w) * pa >= w * pb;
            let psa = f64::from(a.side.power);
            let psb = f64::from(b.side.power);
            let use_a_side = (1.0 - w) * psa >= w * psb;
            let magnitude = lin(ma.max(1e-12).ln(), mb.max(1e-12).ln()).exp();
            let phase = if use_a { C::from_pair(a.carrier).arg() } else { C::from_pair(b.carrier).arg() };
            let mixed_power = lin_log(pa, pb, w);
            let ratio_a = psa / (ma * ma + pa + 1e-30);
            let ratio_b = psb / (mb * mb + pb + 1e-30);
            let side_power = lin(ratio_a, ratio_b) * (magnitude * magnitude + mixed_power);
            let diffuse = if use_a_diffuse { a.diffuse } else { b.diffuse };
            let side = if use_a_side { a.side } else { b.side };
            let rho = C::from_pair(a.side_rho).scale(1.0 - w) + C::from_pair(b.side_rho).scale(w);
            a.carrier = C::cis(phase).scale(magnitude).to_pair();
            a.gamma = lin(f64::from(a.gamma), f64::from(b.gamma)) as f32;
            a.mean_log = lin(f64::from(a.mean_log), f64::from(b.mean_log)) as f32;
            a.sigma_db = lin(f64::from(a.sigma_db), f64::from(b.sigma_db)) as f32;
            a.spread = lin(f64::from(a.spread), f64::from(b.spread)) as f32;
            a.start_level = 0.0;
            a.diffuse = rescaled(&diffuse, mixed_power);
            a.side = rescaled(&side, side_power);
            a.side_rho = rho.to_pair();
            a.diffuse.history = [[0.0; 2]; HISTORY];
            a.side.history = [[0.0; 2]; HISTORY];
        }
        for slot in self.partials.iter_mut().skip(count) {
            *slot = Partial::NONE;
        }
        self.count = count;
        self.vibrato_cents = lin(self.vibrato_cents, other.vibrato_cents);
        if self.vibrato_hz > 0.0 && other.vibrato_hz > 0.0 {
            self.vibrato_hz = lin(self.vibrato_hz, other.vibrato_hz);
        } else if self.vibrato_hz == 0.0 {
            self.vibrato_hz = other.vibrato_hz;
        }
        self.stereo = self.stereo || other.stereo;
        self.from_history = false;
        // The level is the mean of the two notes' levels (in dB).
        let target = lin(
            self.level_db + 20.0 * before_a.max(1e-12).log10(),
            other.level_db + 20.0 * before_b.max(1e-12).log10(),
        );
        let noise = lin(noise_rms, other_noise_rms);
        let after = self.expected_rms(noise);
        self.level_db = target - 20.0 * after.max(1e-12).log10();
    }

    /// The expected RMS of this note's mid before its gains, with a noise of `noise_rms`.
    pub fn expected_rms(&self, noise_rms: f64) -> f64 {
        let partials: f64 = self.partials.iter().take(self.count).map(|p| p.power() / 2.0).sum();
        (partials + noise_rms * noise_rms).sqrt()
    }
}

/// `diffuse` with its power (and innovation in proportion) set to `power`; the line shape, the
/// low-pass and the power factor stay.
fn rescaled(diffuse: &Diffuse, power: f64) -> Diffuse {
    let mut out = *diffuse;
    let old = f64::from(diffuse.power);
    let power = if power.is_finite() { power.max(0.0) } else { 0.0 };
    out.power = power as f32;
    out.innovation = if old > 0.0 { (f64::from(diffuse.innovation) * power / old) as f32 } else { 0.0 };
    if power > 0.0 && diffuse.cutoff_hz == 0.0 {
        out.cutoff_hz = DEMOD_HZ as f32;
    }
    out
}

/// Interpolation of two powers in log.
fn lin_log(a: f64, b: f64, f: f64) -> f64 {
    ((1.0 - f) * (a + 1e-30).ln() + f * (b + 1e-30).ln()).exp()
}

/// `20 log10(a / b)`, 0 for an empty side.
fn db_ratio(a: f64, b: f64) -> f64 {
    if a > 0.0 && b > 0.0 { 20.0 * (a / b).log10() } else { 0.0 }
}

/// dB to a factor.
pub fn db(value: f64) -> f64 {
    (value * LN_10 / 20.0).exp()
}

#[cfg(test)]
#[path = "continuation_tests.rs"]
mod tests;

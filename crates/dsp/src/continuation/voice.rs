// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The per-voice generator of a continuation: control frames at about 300 Hz (pitch, vibrato,
//! jitter, every partial's diffuse part, the slow level), partials summed at the sample rate
//! with their levels interpolated between frames, the residual noise through its filter, and
//! the gains (level shaping, level gain, event limit), which look ahead in the model's own
//! output: the model depends on nothing that comes later but its controls, so it is generated
//! about 0.6 s ahead of what is heard and the gains see the whole window an offline render sees.
//!
//! [`Generator::new`] allocates (off the audio thread); [`Generator::start`],
//! [`Generator::produce`] and [`Generator::next`] do not, and never panic.

use super::math::{Butter3, Butter3State, C, Rng, filtered_real_ar_power, ou_step};
use super::{AR_ORDER, CONTROL_HZ, Continuation, JITTER_ORDER, MAX_PARTIALS, NOISE_POINTS, NoteModel, db, noise_at};
use crate::Convolver;
use std::f64::consts::{LN_10, PI, TAU};

/// Taps of the noise filters.
pub const NOISE_TAPS: usize = 1025;
/// Frames the partial levels are generated ahead of the frame being played (the first 20 ms
/// are matched to the recording's level, which needs them).
const LAG: i64 = 6;
/// Frames kept per partial; at least `LAG + 2`.
const FRAMES: usize = 8;
/// Pitch frames kept, for the level shaping's regressors.
const PITCH_FRAMES: usize = 512;
/// Envelope frames kept (5 ms each).
const ENV_FRAMES: usize = 512;
/// One second of warm-up for a note without history (transposed or morphed).
const WARM_UP_FRAMES: i64 = 300;
/// The pitch's random depth and rate wander and the slow level wander.
const DEPTH_TAU: f64 = 1.2;
const DEPTH_SD: f64 = 0.15;
const RATE_TAU: f64 = 1.5;
const RATE_SD: f64 = 0.04;
const LEVEL_TAU: f64 = 2.0;
const LEVEL_DB: f64 = 0.6;
/// The jitter's low-pass (Hz).
const JITTER_HZ: f64 = 25.0;
/// Time constants of the power fix and spread fade-ins, the start match and the controls.
const FIX_TAU: f64 = 0.25;
const MATCH_TAU: f64 = 0.12;
const CONTROL_TAU: f64 = 0.6;
/// Level shaping: slow swells band-passed at 2.5..4.5 Hz removed by this factor.
const SHAPE_LOW_HZ: f64 = 2.5;
const SHAPE_HIGH_HZ: f64 = 4.5;
const SHAPE_SLOW: f64 = 1.0;
/// Event limit: departures beyond `EVENT_THRESHOLD_DB` from the slow level pulled back by
/// `EVENT_RATIO` of their excess, at most `EVENT_MAX_DB`.
const EVENT_THRESHOLD_DB: f64 = 2.0;
const EVENT_RATIO: f64 = 0.85;
const EVENT_MAX_DB: f64 = 6.0;

/// A continuation's parts that depend on the sample rate: the control frame length, the noise
/// filters and the level shaping's band-pass. Built off the audio thread.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub sample_rate: f64,
    /// Samples per control frame (about `sample_rate / 300`).
    pub control: usize,
    pub noise_ir: Vec<f32>,
    pub side_ir: Vec<f32>,
    pub noise_rms: f64,
}

impl Prepared {
    pub fn new(continuation: &Continuation, sample_rate: f64) -> Self {
        let sample_rate = if sample_rate.is_finite() { sample_rate.clamp(8000.0, 384_000.0) } else { 48000.0 };
        let control = (sample_rate / CONTROL_HZ).round().max(1.0) as usize;
        let noise_ir = noise_filter(&continuation.noise_db, f64::from(continuation.noise_rms), sample_rate);
        let side_ir = if continuation.stereo {
            noise_filter(&continuation.side_noise_db, f64::from(continuation.side_noise_rms), sample_rate)
        } else {
            vec![0.0; NOISE_TAPS]
        };
        Self { sample_rate, control, noise_ir, side_ir, noise_rms: f64::from(continuation.noise_rms) }
    }
}

/// A linear-phase filter whose response follows the dB spectrum `table` (on the noise grid),
/// scaled so that unit-variance white noise comes out at `rms`.
fn noise_filter(table: &[f32; NOISE_POINTS], rms: f64, sample_rate: f64) -> Vec<f32> {
    const BANDS: usize = 512;
    let center = (NOISE_TAPS / 2) as f64;
    let magnitudes: Vec<(f64, f64)> = (0..BANDS)
        .map(|j| {
            let f = (j as f64 + 0.5) / BANDS as f64 * sample_rate / 2.0;
            (f, db(noise_at(table, f)))
        })
        .collect();
    let peak = magnitudes.iter().map(|(_, m)| *m).fold(0.0, f64::max);
    let mut taps: Vec<f64> = (0..NOISE_TAPS)
        .map(|n| {
            let k = n as f64 - center;
            let sum: f64 =
                magnitudes.iter().map(|(f, m)| m / peak.max(1e-300) * (TAU * f * k / sample_rate).cos()).sum();
            // Hann window over the taps.
            let window = 0.5 - 0.5 * (TAU * n as f64 / (NOISE_TAPS - 1) as f64).cos();
            sum * window
        })
        .collect();
    let energy: f64 = taps.iter().map(|t| t * t).sum();
    let scale = if energy > 0.0 && rms.is_finite() { rms.max(0.0) / energy.sqrt() } else { 0.0 };
    for tap in &mut taps {
        *tap *= scale;
    }
    taps.into_iter().map(|t| t as f32).collect()
}

/// A zero-phase band-pass over envelope frames at `frame_rate`: the squared magnitude of a
/// second-order Butterworth band-pass run forward and backward, as `2 half + 1` windowed taps.
fn shape_taps(frame_rate: f64, half: usize) -> Vec<f64> {
    const POINTS: usize = 1024;
    let warp = |f: f64| (PI * f / frame_rate).tan();
    let (low, high) = (warp(SHAPE_LOW_HZ), warp(SHAPE_HIGH_HZ));
    let width = high - low;
    let center = low * high;
    let response = |f: f64| {
        let omega = warp(f);
        if omega <= 0.0 || width <= 0.0 {
            return 0.0;
        }
        let w = (omega * omega - center) / (width * omega);
        let once = 1.0 / (1.0 + w.powi(4));
        once * once
    };
    let step = frame_rate / 2.0 / POINTS as f64;
    (0..=2 * half)
        .map(|n| {
            let k = n as f64 - half as f64;
            let sum: f64 = (0..POINTS)
                .map(|j| {
                    let f = (j as f64 + 0.5) * step;
                    response(f) * (TAU * f * k / frame_rate).cos()
                })
                .sum();
            let window = 0.5 + 0.5 * (PI * k / (half as f64 + 1.0)).cos();
            sum * 2.0 * step / frame_rate * window
        })
        .collect()
}

/// A power-of-two ring of samples indexed by absolute position.
#[derive(Debug, Clone)]
struct Ring {
    data: Vec<f32>,
    mask: usize,
}

impl Ring {
    fn new(at_least: usize) -> Self {
        let size = at_least.max(2).next_power_of_two();
        Self { data: vec![0.0; size], mask: size - 1 }
    }

    fn get(&self, index: i64) -> f32 {
        let index = index.max(0) as usize;
        self.data.get(index & self.mask).copied().unwrap_or(0.0)
    }

    fn set(&mut self, index: i64, value: f32) {
        let index = index.max(0) as usize;
        if let Some(slot) = self.data.get_mut(index & self.mask) {
            *slot = value;
        }
    }
}

/// One diffuse part's running state: its AR memory (newest first), its low-pass and the
/// filter's coefficients.
#[derive(Debug, Clone, Copy, Default)]
struct Lane {
    memory: [C; AR_ORDER],
    filter: Butter3,
    state: Butter3State,
    coefficients: [C; AR_ORDER],
    innovation: f64,
    fix: f64,
    on: bool,
}

impl Lane {
    fn start(&mut self, diffuse: &super::Diffuse, cutoff: f64, history: bool) {
        *self = Self::default();
        self.on = diffuse.power > 0.0;
        if !self.on {
            return;
        }
        self.coefficients = diffuse.coefficients();
        self.innovation = f64::from(diffuse.innovation);
        self.fix = f64::from(diffuse.power_fix);
        self.filter = Butter3::new(cutoff, CONTROL_HZ);
        if history {
            // The recording's last frames run through the low-pass first, and the newest of them
            // seed the AR memory: the output starts right after them.
            for frame in &diffuse.history {
                let value = C::from_pair(*frame);
                self.state.process(&self.filter, value);
                self.push(value);
            }
        }
    }

    fn push(&mut self, value: C) {
        self.memory.rotate_right(1);
        if let Some(first) = self.memory.first_mut() {
            *first = value;
        }
    }

    /// The next frame: one AR step, the low-pass, and the power fix fading in over `t` seconds.
    fn next(&mut self, rng: &mut Rng, t: f64) -> C {
        if !self.on {
            return C::ZERO;
        }
        let mut value = rng.complex(self.innovation);
        for (a, past) in self.coefficients.iter().zip(&self.memory) {
            value += *a * *past;
        }
        let value = value.finite();
        self.push(value);
        let filtered = self.state.process(&self.filter, value);
        filtered.scale(1.0 + (self.fix - 1.0) * (1.0 - (-t / FIX_TAU).exp()))
    }
}

/// One partial's state: its two diffuse parts, its recent level frames (mid, and the side's
/// independent part), its start match and the oscillator that turns it.
#[derive(Debug, Clone, Copy, Default)]
struct Voice {
    mid: Lane,
    side: Lane,
    levels: [C; FRAMES],
    sides: [C; FRAMES],
    /// The side's panning of this partial.
    rho: C,
    matched: f64,
    /// This interval's four level points (the start match applied) and the side's.
    points: [C; 4],
    side_points: [C; 4],
    rotor: C,
    step: C,
    turn: C,
}

/// The pitch of one control frame.
#[derive(Debug, Clone, Copy, Default)]
struct Pitch {
    /// Deviation in cents (vibrato plus jitter) and the vibrato's normalized velocity.
    cents: f64,
    velocity: f64,
    /// Frequency of the harmonic grid and of the part every partial follows fully.
    f0: f64,
    f0_main: f64,
    /// Expected mean square of the deviation, for the level shaping.
    mean_square: f64,
    level: f64,
}

/// Plays one note's continuation. See the module documentation.
pub struct Generator {
    sample_rate: f64,
    control: usize,
    model: Box<NoteModel>,
    voices: Box<[Voice]>,
    pitches: Box<[Pitch]>,
    rng: Rng,
    side_rng: Rng,
    noise_rng: Rng,
    side_noise_rng: Rng,
    other_noise_rng: Rng,
    other_side_noise_rng: Rng,
    noise: Convolver,
    side_noise: Convolver,
    other_noise: Convolver,
    other_side_noise: Convolver,
    scratch_in: Vec<f32>,
    scratch_out: Vec<f32>,
    noise_mid: Vec<f32>,
    noise_side: Vec<f32>,
    /// Noise weights: the model's own and the morph target's (power mix).
    noise_weight: f64,
    other_weight: f64,
    stereo: bool,
    /// Raw, then shaped, mid and side, by model sample.
    mid: Ring,
    side: Ring,
    /// The event limit's raw gain (dB) by sample.
    event: Ring,
    envelope: Vec<f32>,
    frame_gains: Vec<f32>,
    frame_gain_ready: i64,
    shape: Vec<f64>,
    shape_half: i64,
    hop: i64,
    window: i64,
    fast: i64,
    slow: i64,
    smooth: i64,
    /// The next control frame to generate, and how far raw output, envelope frames, the
    /// shaping, the event gain and the output have got.
    frame: i64,
    warm_up: i64,
    produced: i64,
    envelopes: i64,
    shaped: i64,
    evented: i64,
    out: i64,
    sum_fast: f64,
    sum_slow: f64,
    sum_event: f64,
    event_started: bool,
    output_started: bool,
    /// Pitch state.
    depth_wander: f64,
    rate_wander: f64,
    level_wander: f64,
    depth: f64,
    rate: f64,
    depth_target: f64,
    rate_target: f64,
    vibrato_phase: f64,
    jitter_memory: [f64; JITTER_ORDER],
    jitter_filter: Butter3,
    jitter_state: Butter3State,
    jitter_variance: f64,
    phase: f64,
    deviation: f64,
    active: bool,
}

impl Generator {
    /// A generator for notes of `own` (and, when morphing, `other`) at `sample_rate`; allocates
    /// its rings and noise filters.
    pub fn new(own: &Prepared, other: Option<&Prepared>) -> Self {
        let sample_rate = own.sample_rate;
        let control = own.control;
        let hop = (0.005 * sample_rate).round().max(1.0) as i64;
        let window = (0.02 * sample_rate).round().max(1.0) as i64;
        let frame_rate = sample_rate / hop as f64;
        let shape_half = (0.3 * frame_rate).round().max(1.0) as usize;
        let lookahead = Self::lookahead_for(sample_rate, hop, window, shape_half as i64);
        let span = lookahead as usize + (0.4 * sample_rate) as usize + 4 * control;
        let mut noise = Convolver::default();
        let mut side_noise = Convolver::default();
        let mut other_noise = Convolver::default();
        let mut other_side_noise = Convolver::default();
        // A filter that fails to build stays silent rather than stopping the note.
        let _ = noise.init(&own.noise_ir);
        let _ = side_noise.init(&own.side_ir);
        if let Some(other) = other {
            let _ = other_noise.init(&other.noise_ir);
            let _ = other_side_noise.init(&other.side_ir);
        }
        Self {
            sample_rate,
            control,
            model: Box::default(),
            voices: vec![Voice::default(); MAX_PARTIALS].into_boxed_slice(),
            pitches: vec![Pitch::default(); PITCH_FRAMES].into_boxed_slice(),
            rng: Rng::default(),
            side_rng: Rng::default(),
            noise_rng: Rng::default(),
            side_noise_rng: Rng::default(),
            other_noise_rng: Rng::default(),
            other_side_noise_rng: Rng::default(),
            noise,
            side_noise,
            other_noise,
            other_side_noise,
            scratch_in: vec![0.0; control],
            scratch_out: vec![0.0; control],
            noise_mid: vec![0.0; control],
            noise_side: vec![0.0; control],
            noise_weight: 1.0,
            other_weight: 0.0,
            stereo: false,
            mid: Ring::new(span),
            side: Ring::new(span),
            event: Ring::new(span),
            envelope: vec![0.0; ENV_FRAMES],
            frame_gains: vec![0.0; ENV_FRAMES],
            frame_gain_ready: 0,
            shape: shape_taps(frame_rate, shape_half),
            shape_half: shape_half as i64,
            hop,
            window,
            fast: (0.02 * sample_rate).round().max(1.0) as i64,
            slow: (0.5 * sample_rate).round().max(1.0) as i64,
            smooth: (0.03 * sample_rate).round().max(1.0) as i64,
            frame: 0,
            warm_up: 0,
            produced: 0,
            envelopes: 0,
            shaped: 0,
            evented: 0,
            out: 0,
            sum_fast: 0.0,
            sum_slow: 0.0,
            sum_event: 0.0,
            event_started: false,
            output_started: false,
            depth_wander: 0.0,
            rate_wander: 0.0,
            level_wander: 0.0,
            depth: 1.0,
            rate: 1.0,
            depth_target: 1.0,
            rate_target: 1.0,
            vibrato_phase: 0.0,
            jitter_memory: [0.0; JITTER_ORDER],
            jitter_filter: Butter3::new(JITTER_HZ, CONTROL_HZ),
            jitter_state: Butter3State::default(),
            jitter_variance: 0.0,
            phase: 0.0,
            deviation: 0.0,
            active: false,
        }
    }

    /// How far (in samples) the raw model must run ahead of what is heard.
    fn lookahead_for(sample_rate: f64, hop: i64, window: i64, shape_half: i64) -> i64 {
        let shaping = (shape_half + 2) * hop + window;
        let event = (0.25 * sample_rate) as i64 + (0.015 * sample_rate) as i64 + (0.01 * sample_rate) as i64 + 4;
        shaping + event
    }

    /// The lookahead of this generator, in samples.
    pub fn lookahead(&self) -> i64 {
        Self::lookahead_for(self.sample_rate, self.hop, self.window, self.shape_half)
    }

    /// Starts a note of `model`. `other` is the morph target's noise weight share (0: none);
    /// the model is already morphed. `seed` makes the note repeatable. Allocation-free.
    pub fn start(&mut self, model: &NoteModel, other: f64, seed: u64) {
        self.model.as_mut().clone_from(model);
        let count = model.count.min(MAX_PARTIALS);
        self.rng = Rng::new(seed);
        self.side_rng = Rng::new(seed ^ 0x5151_5151);
        self.noise_rng = Rng::new(seed ^ 0xA5A5_0001);
        self.side_noise_rng = Rng::new(seed ^ 0xA5A5_0002);
        self.other_noise_rng = Rng::new(seed ^ 0xA5A5_0003);
        self.other_side_noise_rng = Rng::new(seed ^ 0xA5A5_0004);
        self.noise.reset();
        self.side_noise.reset();
        self.other_noise.reset();
        self.other_side_noise.reset();
        let other = if other.is_finite() { other.clamp(0.0, 1.0) } else { 0.0 };
        self.noise_weight = (1.0 - other).sqrt();
        self.other_weight = other.sqrt();
        self.stereo = model.stereo;
        for (k, voice) in self.voices.iter_mut().enumerate() {
            *voice = Voice::default();
            let Some(partial) = model.partials.get(k).filter(|_| k < count) else { continue };
            let cutoff = |d: &super::Diffuse| f64::from(d.cutoff_hz);
            voice.mid.start(&partial.diffuse, cutoff(&partial.diffuse), model.from_history);
            voice.side.start(&partial.side, cutoff(&partial.side), model.from_history && model.stereo);
            voice.rho = if model.stereo { C::from_pair(partial.side_rho) } else { C::ZERO };
            voice.matched = 1.0;
        }
        self.jitter_variance = jitter_variance(model);
        for pitch in self.pitches.iter_mut() {
            *pitch = Pitch::default();
        }
        for slot in self.envelope.iter_mut().chain(self.frame_gains.iter_mut()) {
            *slot = 0.0;
        }
        self.warm_up = if model.from_history { 0 } else { WARM_UP_FRAMES };
        self.frame = -self.warm_up;
        self.produced = 0;
        self.envelopes = 0;
        self.frame_gain_ready = 0;
        self.shaped = 0;
        self.evented = 0;
        self.out = 0;
        self.sum_fast = 0.0;
        self.sum_slow = 0.0;
        self.sum_event = 0.0;
        self.event_started = false;
        self.output_started = false;
        self.depth_wander = 0.0;
        self.rate_wander = 0.0;
        self.level_wander = 0.0;
        self.depth = 1.0;
        self.rate = 1.0;
        // The vibrato reaches its stored phase at frame 0, after the warm-up.
        self.vibrato_phase = model.vibrato_phase - TAU * model.vibrato_hz * self.warm_up as f64 / CONTROL_HZ;
        self.jitter_memory = [0.0; JITTER_ORDER];
        self.jitter_state = Butter3State::default();
        self.phase = model.phase;
        self.deviation = model.deviation_phase;
        self.active = count > 0 && model.f0 > 0.0;
    }

    /// Sets the vibrato depth (a factor of the recorded depth, 0..4) and rate (Hz; 0 keeps the
    /// recorded rate). They take effect smoothly.
    pub fn set_vibrato(&mut self, depth: f64, rate_hz: f64) {
        self.depth_target = if depth.is_finite() { depth.clamp(0.0, 4.0) } else { 1.0 };
        let recorded = self.model.vibrato_hz;
        self.rate_target = if rate_hz.is_finite() && rate_hz > 0.0 && recorded > 0.0 {
            (rate_hz.clamp(1.0, 20.0) / recorded).clamp(0.05, 20.0)
        } else {
            1.0
        };
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// How many samples of raw model are ready beyond what has been heard.
    pub fn ahead(&self) -> i64 {
        self.produced - self.out
    }

    /// Generates up to `frames` control frames ahead (warm-up included), stopping once the
    /// lookahead is full. Lets a voice spread the work before the join over several blocks.
    pub fn produce(&mut self, frames: usize) {
        if !self.active {
            return;
        }
        let target = self.out + self.lookahead() + 2 * self.control as i64;
        for _ in 0..frames {
            if self.produced >= target {
                break;
            }
            self.control_frame();
        }
    }

    /// The next output sample (mid, side) in the recording's units. Generates what it needs.
    pub fn next(&mut self) -> [f32; 2] {
        if !self.active {
            return [0.0; 2];
        }
        let m = self.out;
        // The event gain at m needs shaped samples up to m + half its slow window and smoothing.
        let needed = m + self.slow / 2 + self.smooth / 2 + self.fast / 2 + 2;
        while self.shaped <= needed {
            self.shape_one();
        }
        while self.evented <= m + self.smooth / 2 + 1 {
            self.event_one();
        }
        if !self.output_started {
            // The smoothing window starts with its first value repeated (nearest padding).
            let first = f64::from(self.event.get(0));
            let half = self.smooth / 2;
            self.sum_event = first * half as f64;
            for i in 0..(self.smooth - half) {
                self.sum_event += f64::from(self.event.get(i));
            }
            self.output_started = true;
        } else {
            let add = m + (self.smooth - self.smooth / 2) - 1;
            let drop = m - self.smooth / 2 - 1;
            self.sum_event += f64::from(self.event.get(add)) - f64::from(self.event.get(drop.max(0)));
        }
        let ramp = (m as f64 / (0.1 * self.sample_rate)).min(1.0);
        let gain_db = self.sum_event / self.smooth as f64 * ramp;
        let gain = db(gain_db);
        let mid = f64::from(self.mid.get(m)) * gain;
        let side = if self.stereo { f64::from(self.side.get(m)) * gain } else { 0.0 };
        self.out += 1;
        [finite(mid), finite(side)]
    }

    /// The shaped mid (before the event limit) from the current output on, into `out`: what a
    /// release splice lines up with.
    pub fn upcoming(&mut self, out: &mut [f32]) {
        let wanted = self.out + out.len() as i64;
        while self.active && self.shaped < wanted {
            self.shape_one();
        }
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = self.mid.get(self.out + i as i64);
        }
    }

    /// One control frame, and the samples of the frame `LAG` behind it.
    fn control_frame(&mut self) {
        let n = self.frame;
        let t = (n + self.warm_up) as f64 / CONTROL_HZ;
        let pitch = self.pitch_frame(n);
        if n >= 0
            && let Some(slot) = self.pitches.get_mut(n as usize % PITCH_FRAMES)
        {
            *slot = pitch;
        }
        let count = self.model.count.min(MAX_PARTIALS);
        let slot = (n.rem_euclid(FRAMES as i64)) as usize;
        for (k, voice) in self.voices.iter_mut().enumerate().take(count) {
            let Some(partial) = self.model.partials.get(k) else { break };
            let diffuse = voice.mid.next(&mut self.rng, t);
            let mut level = C::from_pair(partial.carrier) + diffuse;
            let mean = f64::from(partial.mean_log);
            let spread = f64::from(partial.spread);
            if spread < 1.0 {
                let log = level.abs().max(1e-9).ln();
                let weight = 1.0 - (1.0 - spread) * (1.0 - (-t / FIX_TAU).exp());
                let target = mean + weight * (log - mean);
                level = level.scale((target - log).exp());
            }
            let bound = (2.0 * f64::from(partial.sigma_db) / 8.686).max(0.25);
            let log = level.abs().max(1e-9).ln();
            let d = log - mean;
            level = level.scale((bound * (d / bound).tanh() - d).exp());
            let side = voice.side.next(&mut self.side_rng, t);
            if let (Some(l), Some(s)) = (voice.levels.get_mut(slot), voice.sides.get_mut(slot)) {
                *l = level.finite();
                *s = side.finite();
            }
        }
        // The first 20 ms meet the recording's level just before the join.
        if n == LAG - 1 && self.model.from_history {
            for (k, voice) in self.voices.iter_mut().enumerate().take(count) {
                let Some(partial) = self.model.partials.get(k) else { break };
                let generated: f64 =
                    (0..LAG).filter_map(|i| voice.levels.get(i as usize % FRAMES)).map(|c| c.abs()).sum::<f64>()
                        / LAG as f64;
                let ratio = f64::from(partial.start_level) / generated.max(1e-12);
                voice.matched = if (0.4..2.5).contains(&ratio) { ratio } else { 1.0 };
            }
        }
        if n >= LAG {
            self.synthesize(n - LAG);
        }
        self.frame += 1;
    }

    /// The pitch of frame `n` (negative during the warm-up).
    fn pitch_frame(&mut self, n: i64) -> Pitch {
        let model = &self.model;
        let a = 1.0 - (-1.0 / (CONTROL_HZ * CONTROL_TAU)).exp();
        if n > -self.warm_up {
            self.depth += (self.depth_target - self.depth) * a;
            self.rate += (self.rate_target - self.rate) * a;
        }
        self.depth_wander = ou_step(self.depth_wander, DEPTH_TAU, DEPTH_SD, CONTROL_HZ, &mut self.rng);
        self.rate_wander = ou_step(self.rate_wander, RATE_TAU, RATE_SD, CONTROL_HZ, &mut self.rng);
        self.level_wander = ou_step(self.level_wander, LEVEL_TAU, LEVEL_DB / 8.686, CONTROL_HZ, &mut self.rng);
        let depth = model.vibrato_cents * self.depth_wander.exp() * self.depth;
        let rate = model.vibrato_hz * self.rate_wander.exp() * self.rate;
        if n > -self.warm_up {
            self.vibrato_phase += TAU * rate / CONTROL_HZ;
        }
        let (sin, cos) = self.vibrato_phase.sin_cos();
        let vibrato = depth * sin;
        let velocity_rate = if model.vibrato_hz > 0.0 { rate / (model.vibrato_hz * self.rate).max(1e-9) } else { 0.0 };
        let velocity = depth * cos * velocity_rate;
        let mut jitter = self.rng.normal() * model.jitter_innovation.max(0.0).sqrt();
        for (a, past) in model.jitter_ar.iter().zip(&self.jitter_memory) {
            jitter += a * past;
        }
        let jitter = if jitter.is_finite() { jitter } else { 0.0 };
        self.jitter_memory.rotate_right(1);
        if let Some(first) = self.jitter_memory.first_mut() {
            *first = jitter;
        }
        let jitter = self.jitter_state.process(&self.jitter_filter, C::new(jitter, 0.0)).re * model.jitter_gain;
        let cents = vibrato + jitter;
        // Vibrato beyond the recorded depth moves every partial fully.
        let extra = if self.depth > 1.0 { vibrato * (1.0 - 1.0 / self.depth) } else { 0.0 };
        let f0 = model.f0 * 2.0_f64.powf(cents / 1200.0);
        let f0_main = model.f0 * 2.0_f64.powf((cents - extra) / 1200.0);
        let nominal = model.vibrato_cents * self.depth;
        let mean_square = nominal * nominal * (2.0 * DEPTH_SD * DEPTH_SD).exp() / 2.0 + self.jitter_variance;
        Pitch { cents, velocity, f0, f0_main, mean_square, level: self.level_wander.exp() }
    }

    /// The samples of frame interval `j` (`j * control` to `(j + 1) * control`).
    fn synthesize(&mut self, j: i64) {
        let n = self.control as i64;
        let base = j * n;
        self.produced = base + n;
        let start = self.pitches.get(j as usize % PITCH_FRAMES).copied().unwrap_or_default();
        let end = self.pitches.get((j + 1) as usize % PITCH_FRAMES).copied().unwrap_or_default();
        let dt = self.control as f64 / self.sample_rate;
        let f0 = self.model.f0;
        // Phases at the interval's start and end; the frequency is linear across it.
        let phase_end = self.phase + PI * dt * (start.f0 + end.f0);
        let deviation_end = self.deviation + PI * dt * ((start.f0_main - f0) + (end.f0_main - f0));
        let count = self.model.count.min(MAX_PARTIALS);
        let limit = super::PARTIAL_LIMIT * self.sample_rate;
        let match_frames = MATCH_TAU * CONTROL_HZ;
        for (k, voice) in self.voices.iter_mut().enumerate().take(count) {
            let Some(partial) = self.model.partials.get(k) else { break };
            let h = (k + 1) as f64;
            let gamma = f64::from(partial.gamma);
            let theta = h * (self.phase - (1.0 - gamma) * self.deviation);
            let omega0 = TAU * h * (start.f0 - (1.0 - gamma) * (start.f0_main - f0)) / self.sample_rate;
            let omega1 = TAU * h * (end.f0 - (1.0 - gamma) * (end.f0_main - f0)) / self.sample_rate;
            voice.rotor = if h * f0.max(start.f0).max(end.f0) > limit { C::ZERO } else { C::cis(theta) };
            voice.step = C::cis(omega0 + (omega1 - omega0) / (2.0 * n as f64));
            voice.turn = C::cis((omega1 - omega0) / n as f64);
            for (offset, (point, side_point)) in (-1..=2).zip(voice.points.iter_mut().zip(voice.side_points.iter_mut()))
            {
                // Before the first frame, the first (the curve starts flat).
                let frame = (j + offset).max(0);
                let index = frame.rem_euclid(FRAMES as i64) as usize;
                let gain = 1.0 + (voice.matched - 1.0) * (-(frame as f64) / match_frames).exp();
                *point = voice.levels.get(index).map_or(C::ZERO, |l| l.scale(gain));
                *side_point = voice.sides.get(index).copied().unwrap_or(C::ZERO);
            }
        }
        self.phase = phase_end.rem_euclid(TAU);
        self.deviation = deviation_end;
        self.fill_noise();
        let stereo = self.stereo;
        for i in 0..n {
            let u = i as f64 / n as f64;
            let weights = catmull(u);
            let level = start.level + (end.level - start.level) * u;
            let mut mid = 0.0;
            let mut side = 0.0;
            for voice in self.voices.iter_mut().take(count) {
                if voice.rotor == C::ZERO {
                    continue;
                }
                let mut a = C::ZERO;
                for (point, weight) in voice.points.iter().zip(weights) {
                    a += point.scale(weight);
                }
                mid += (a * voice.rotor).re;
                if stereo {
                    let mut e = C::ZERO;
                    for (point, weight) in voice.side_points.iter().zip(weights) {
                        e += point.scale(weight);
                    }
                    side += ((voice.rho * a + e) * voice.rotor).re;
                }
                voice.rotor = voice.rotor * voice.step;
                voice.step = voice.step * voice.turn;
            }
            let at = i as usize;
            let noise = self.noise_mid.get(at).copied().map_or(0.0, f64::from);
            let side_noise = self.noise_side.get(at).copied().map_or(0.0, f64::from);
            self.mid.set(base + i, ((mid + noise) * level) as f32);
            self.side.set(base + i, if stereo { ((side + side_noise) * level) as f32 } else { 0.0 });
        }
        self.update_envelopes();
    }

    /// The interval's noise: the model's own and, when morphing, the target's, each with its
    /// weight (their powers add).
    fn fill_noise(&mut self) {
        let (own, other) = (self.noise_weight as f32, self.other_weight as f32);
        for value in self.scratch_in.iter_mut() {
            *value = self.noise_rng.white() as f32;
        }
        self.noise.process(&self.scratch_in, &mut self.noise_mid);
        for value in self.noise_mid.iter_mut() {
            *value *= own;
        }
        if other > 0.0 {
            for value in self.scratch_in.iter_mut() {
                *value = self.other_noise_rng.white() as f32;
            }
            self.other_noise.process(&self.scratch_in, &mut self.scratch_out);
            for (target, value) in self.noise_mid.iter_mut().zip(&self.scratch_out) {
                *target += value * other;
            }
        }
        if !self.stereo {
            return;
        }
        for value in self.scratch_in.iter_mut() {
            *value = self.side_noise_rng.white() as f32;
        }
        self.side_noise.process(&self.scratch_in, &mut self.noise_side);
        for value in self.noise_side.iter_mut() {
            *value *= own;
        }
        if other > 0.0 {
            for value in self.scratch_in.iter_mut() {
                *value = self.other_side_noise_rng.white() as f32;
            }
            self.other_side_noise.process(&self.scratch_in, &mut self.scratch_out);
            for (target, value) in self.noise_side.iter_mut().zip(&self.scratch_out) {
                *target += value * other;
            }
        }
    }

    /// Envelope frames whose window the raw output now covers: 10 log10 of the mean square over
    /// `window` samples every `hop`.
    fn update_envelopes(&mut self) {
        while self.envelopes * self.hop + self.window <= self.produced {
            let start = self.envelopes * self.hop;
            let mut sum = 0.0;
            for i in 0..self.window {
                let v = f64::from(self.mid.get(start + i));
                sum += v * v;
            }
            let value = 10.0 * (sum / self.window as f64 + 1e-14).log10();
            if let Some(slot) = self.envelope.get_mut(self.envelopes as usize % ENV_FRAMES) {
                *slot = value as f32;
            }
            self.envelopes += 1;
        }
    }

    fn envelope_at(&self, frame: i64) -> f64 {
        // Before the first frame, the first (nearest); beyond the last ready, the last.
        let frame = frame.clamp(0, (self.envelopes - 1).max(0));
        self.envelope.get(frame as usize % ENV_FRAMES).copied().map_or(0.0, f64::from)
    }

    /// The level shaping's gain (dB) of envelope frame `i`.
    fn frame_gain(&mut self, i: i64) -> f64 {
        while self.envelopes <= i + self.shape_half + 1 {
            self.control_frame();
        }
        let mut band = 0.0;
        for (k, tap) in self.shape.iter().enumerate() {
            band += tap * self.envelope_at(i + k as i64 - self.shape_half);
        }
        let center = i * self.hop + self.window / 2;
        let frame = (center / self.control as i64).max(0);
        let pitch = self.pitches.get(frame as usize % PITCH_FRAMES).copied().unwrap_or_default();
        let model = &self.model;
        let regressors = [pitch.cents, pitch.velocity, pitch.cents * pitch.cents - pitch.mean_square];
        let locked: f64 =
            regressors.iter().zip(model.coef_target.iter().zip(&model.coef_model)).map(|(r, (t, m))| r * (t - m)).sum();
        let gain = -SHAPE_SLOW * band + locked;
        if gain.is_finite() { gain.clamp(-24.0, 24.0) } else { 0.0 }
    }

    /// Shapes the next raw sample in place: the level shaping, then the level gain.
    fn shape_one(&mut self) {
        let m = self.shaped;
        // Frame centers at i * hop + window / 2; interpolate between the two around m.
        let offset = m - self.window / 2;
        let i0 = if offset < 0 { 0 } else { offset / self.hop };
        while self.frame_gain_ready <= i0 + 1 {
            let i = self.frame_gain_ready;
            let gain = self.frame_gain(i);
            if let Some(slot) = self.frame_gains.get_mut(i as usize % ENV_FRAMES) {
                *slot = gain as f32;
            }
            self.frame_gain_ready += 1;
        }
        let g0 = self.frame_gains.get(i0 as usize % ENV_FRAMES).copied().map_or(0.0, f64::from);
        let g1 = self.frame_gains.get((i0 + 1) as usize % ENV_FRAMES).copied().map_or(0.0, f64::from);
        let fraction =
            if offset < 0 { 0.0 } else { ((offset - i0 * self.hop) as f64 / self.hop as f64).clamp(0.0, 1.0) };
        let shaping = (g0 + (g1 - g0) * fraction) * (m as f64 / (0.1 * self.sample_rate)).min(1.0);
        let level = self.model.level_db * (m as f64 / (0.3 * self.sample_rate)).min(1.0);
        let gain = ((shaping + level) * LN_10 / 20.0).exp();
        self.mid.set(m, (f64::from(self.mid.get(m)) * gain) as f32);
        self.side.set(m, (f64::from(self.side.get(m)) * gain) as f32);
        self.shaped += 1;
    }

    /// The event limit's raw gain (dB) at the next sample, from the shaped mid's fast and slow
    /// mean power around it.
    fn event_one(&mut self) {
        let r = self.evented;
        let power = |ring: &Ring, index: i64| {
            let v = f64::from(ring.get(index.max(0)));
            v * v
        };
        let (half_fast, half_slow) = (self.fast / 2, self.slow / 2);
        if !self.event_started {
            self.sum_fast = (-half_fast..(self.fast - half_fast)).map(|i| power(&self.mid, r + i)).sum();
            self.sum_slow = (-half_slow..(self.slow - half_slow)).map(|i| power(&self.mid, r + i)).sum();
            self.event_started = true;
        } else {
            self.sum_fast += power(&self.mid, r + self.fast - half_fast - 1) - power(&self.mid, r - half_fast - 1);
            self.sum_slow += power(&self.mid, r + self.slow - half_slow - 1) - power(&self.mid, r - half_slow - 1);
        }
        let fast = 10.0 * (self.sum_fast.max(0.0) / self.fast as f64 + 1e-18).log10();
        let slow = 10.0 * (self.sum_slow.max(0.0) / self.slow as f64 + 1e-18).log10();
        let d = fast - slow;
        let excess = d.signum() * (d.abs() - EVENT_THRESHOLD_DB).max(0.0);
        let gain = (-EVENT_RATIO * excess).clamp(-EVENT_MAX_DB, EVENT_MAX_DB);
        self.event.set(r, if gain.is_finite() { gain as f32 } else { 0.0 });
        self.evented += 1;
    }
}

/// The jitter's variance in cents squared, as the level shaping's expected mean square counts
/// it.
fn jitter_variance(model: &NoteModel) -> f64 {
    let filter = Butter3::new(JITTER_HZ, CONTROL_HZ);
    let power = filtered_real_ar_power(&model.jitter_ar, model.jitter_innovation, &filter, CONTROL_HZ);
    power * model.jitter_gain * model.jitter_gain
}

/// Catmull-Rom weights of the points before, at, after and two after the interval at `u`.
fn catmull(u: f64) -> [f64; 4] {
    let u2 = u * u;
    let u3 = u2 * u;
    [0.5 * (-u3 + 2.0 * u2 - u), 0.5 * (3.0 * u3 - 5.0 * u2 + 2.0), 0.5 * (-3.0 * u3 + 4.0 * u2 + u), 0.5 * (u3 - u2)]
}

fn finite(value: f64) -> f32 {
    if value.is_finite() { value.clamp(-64.0, 64.0) as f32 } else { 0.0 }
}

#[cfg(test)]
#[path = "voice_tests.rs"]
mod tests;

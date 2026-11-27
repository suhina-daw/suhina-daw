// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The runtime every sample instrument shares: the SDK's `sample` node and the built-in Sampler
//! (`devices/sampler`) both run this [`Sampler`], so it lives here rather than in the device
//! (#763 › 5). Bounded: voices, slices and buffers have fixed limits.
use std::sync::Arc;
use suhina_device_api::{
    Device, DeviceError, DeviceInfo, NoteEvent, NoteExpressionKind, NoteExpressionState, NoteInstanceDevice,
    NoteInstanceDispatchError, NoteInstanceEvent, NoteInstanceId, NoteLifetimes, ParamInfo, ProcessContext, Role,
    SampleSource, Scale, Unit,
};
use suhina_dsp::{
    Biquad, Coefficients,
    envelope::{LinearEnvelope, LinearEnvelopeSettings},
};

use crate::nodes::stretch as warp;
use crate::sample_continuation::{ContinuationControls, ContinuationPlayer, ModelHold, fade_in, widen};
use suhina_dsp::continuation::Continuation;

#[cfg(test)]
#[path = "sample_envelope_tests.rs"]
mod envelope_tests;
#[cfg(test)]
#[path = "sample_expression_tests.rs"]
mod expression_tests;

/// The most notes a sampler plays at once; each has a voice and a warp voice.
const VOICES: usize = 32;

const fn continuous(id: &'static str, name: &'static str, min: f64, max: f64, default: f64, unit: Unit) -> ParamInfo {
    ParamInfo::continuous(id, name, (min, max), default, unit, Scale::Linear)
}
pub const PARAMS: &[ParamInfo] = &[
    // Keep the historical Sample/Grain values; Slice is appended.
    ParamInfo::choice("mode", "Mode", &["Sample", "Grain", "Slice"], 0),
    continuous("start", "Start", 0., 1., 0., Unit::Percent),
    continuous("end", "End", 0., 1., 1., Unit::Percent),
    continuous("loop_start", "Loop start", 0., 1., 0., Unit::Percent),
    continuous("loop_end", "Loop end", 0., 1., 1., Unit::Percent),
    continuous("pitch", "Semitones", -48., 48., 0., Unit::None),
    continuous("cents", "Cents", -100., 100., 0., Unit::None),
    ParamInfo::integer("poly", "Poly", (1, 32), 8, Unit::None),
    ParamInfo::choice("playback", "Playback", &["Gate", "One shot", "Loop", "Slices", "Sustain"], 0),
    ParamInfo::toggle("ping_pong", "Ping pong", false),
    ParamInfo::toggle("reverse", "Reverse", false),
    continuous("attack", "Attack", 0., 10., 0.005, Unit::Seconds),
    continuous("decay", "Decay", 0., 10., 0.22, Unit::Seconds),
    continuous("sustain", "Sustain", 0., 1., 0.87, Unit::Percent),
    continuous("release", "Release", 0., 10., 0.48, Unit::Seconds),
    ParamInfo::choice("filter", "Filter", &["Off", "Low pass", "High pass", "Band pass", "Notch"], 0),
    ParamInfo::continuous("cutoff", "Cutoff", (20., 20000.), 3100., Unit::Hertz, Scale::Logarithmic),
    continuous("resonance", "Resonance", 0.1, 10., 0.707, Unit::None),
    continuous("gain", "Gain", -60., 12., 0., Unit::Decibels),
    continuous("pan", "Pan", -1., 1., 0., Unit::Balance),
    continuous("position", "Position", 0., 1., 0.5, Unit::Percent),
    continuous("scan", "Scan", -4., 4., 0., Unit::None),
    continuous("size", "Size (ms)", 5., 500., 120., Unit::None),
    continuous("density", "Density (grains/s)", 1., 100., 8., Unit::None),
    continuous("jitter", "Jitter", 0., 1., 0.25, Unit::Percent),
    continuous("spread", "Spread", 0., 1., 0., Unit::Percent),
    ParamInfo::toggle("freeze", "Freeze", false),
    // Retained at its historical index for saved definitions; loop edges use fade_in/out.
    continuous("crossfade", "Loop fade (ms)", 0., 100., 5., Unit::None),
    continuous("velocity", "Velocity to volume", 0., 1., 1., Unit::Percent),
    continuous("glide", "Glide", 0., 2., 0., Unit::Seconds),
    ParamInfo::integer("root", "Root key", (0, 127), 60, Unit::None),
    ParamInfo::integer("key_low", "Lowest key", (0, 127), 0, Unit::None),
    ParamInfo::integer("key_high", "Highest key", (0, 127), 127, Unit::None),
    continuous("velocity_low", "Lowest velocity", 0., 1., 0., Unit::Percent),
    continuous("velocity_high", "Highest velocity", 0., 1., 1., Unit::Percent),
    continuous("filter_env", "Filter envelope (oct)", -8., 8., 0., Unit::Octaves),
    continuous("velocity_filter", "Velocity to filter (oct)", 0., 8., 0., Unit::Octaves),
    continuous("lfo_rate", "LFO rate", 0.01, 20., 1., Unit::Hertz),
    continuous("lfo_pitch", "LFO pitch (st)", 0., 12., 0., Unit::None),
    ParamInfo::toggle("grain_env", "Show grain amplitude envelope", false),
    ParamInfo::choice("voice_mode", "Voices", &["Poly", "Mono"], 0),
    continuous("pitch_env", "Pitch envelope (st)", -48., 48., 0., Unit::None),
    continuous("pitch_attack", "Pitch attack", 0., 10., 0., Unit::Seconds),
    continuous("pitch_decay", "Pitch decay", 0., 10., 0.2, Unit::Seconds),
    ParamInfo::choice("envelope_tab", "Envelope view", &["AMP", "PITCH"], 0),
    ParamInfo::choice("filter_slope", "Filter slope", &["12 dB/oct", "24 dB/oct"], 0),
    continuous("key_filter", "Cutoff < Key", -2., 2., 0., Unit::None),
    ParamInfo::toggle("retrigger", "Retrigger", true),
    continuous("filter_attack", "Filter attack", 0., 10., 0.005, Unit::Seconds),
    continuous("filter_decay", "Filter decay", 0., 10., 0.22, Unit::Seconds),
    continuous("filter_sustain", "Filter sustain", 0., 1., 0.87, Unit::Percent),
    continuous("filter_release", "Filter release", 0., 10., 0.48, Unit::Seconds),
    ParamInfo::choice("panel_view", "Panel view", &["Sample", "Controls", "Controls (LFO)", "More"], 0),
    continuous("lfo_filter", "LFO filter (oct)", 0., 8., 0., Unit::Octaves),
    continuous("lfo_volume", "LFO volume", 0., 1., 0., Unit::Percent),
    continuous("lfo_pan", "LFO pan", 0., 1., 0., Unit::Percent),
    ParamInfo::choice("lfo_shape", "LFO shape", &["Sine", "Triangle", "Saw up", "Saw down", "Square", "Random"], 0),
    continuous("fade_in", "Fade in (ms)", 0., 1000., 0., Unit::None),
    continuous("fade_out", "Fade out (ms)", 0., 1000., 0., Unit::None),
    continuous("sensitivity", "Sensitivity", 0., 1., 0.5, Unit::Percent),
    ParamInfo::choice("slice_by", "Slice by", &["Transient", "Beat", "Region", "Manual"], 0),
    continuous("slice_1", "Slice 1", 0., 1., 1., Unit::Percent),
    continuous("slice_2", "Slice 2", 0., 1., 1., Unit::Percent),
    continuous("slice_3", "Slice 3", 0., 1., 1., Unit::Percent),
    continuous("slice_4", "Slice 4", 0., 1., 1., Unit::Percent),
    continuous("slice_5", "Slice 5", 0., 1., 1., Unit::Percent),
    continuous("slice_6", "Slice 6", 0., 1., 1., Unit::Percent),
    continuous("slice_7", "Slice 7", 0., 1., 1., Unit::Percent),
    continuous("slice_8", "Slice 8", 0., 1., 1., Unit::Percent),
    continuous("slice_9", "Slice 9", 0., 1., 1., Unit::Percent),
    continuous("slice_10", "Slice 10", 0., 1., 1., Unit::Percent),
    continuous("slice_11", "Slice 11", 0., 1., 1., Unit::Percent),
    continuous("slice_12", "Slice 12", 0., 1., 1., Unit::Percent),
    continuous("slice_13", "Slice 13", 0., 1., 1., Unit::Percent),
    continuous("slice_14", "Slice 14", 0., 1., 1., Unit::Percent),
    continuous("slice_15", "Slice 15", 0., 1., 1., Unit::Percent),
    continuous("slice_16", "Slice 16", 0., 1., 1., Unit::Percent),
    ParamInfo::toggle("warp", "Warp", false),
    ParamInfo::choice(
        "warp_mode",
        "Warp mode",
        &["Repitch", "Rhythmic", "Tonal", "Texture", "Smooth", "HighQuality"],
        1,
    ),
    continuous("warp_beats", "Source length (beats)", 0.25, 1024., 4., Unit::None),
    ParamInfo::choice("lfo_rate_mode", "LFO rate mode", &["Hz", "Time", "Beat", "Triplet", "Dotted", "16th"], 0),
    continuous("lfo_time", "LFO time (ms)", 1., 20000., 1000., Unit::None),
    ParamInfo::integer("lfo_division", "LFO division", (1, 64), 4, Unit::None),
    continuous("source_bpm", "Source BPM", 0., 999., 0., Unit::None),
    ParamInfo::choice("slice_release", "Slice release", &["Play to end", "Release on key up"], 0),
    continuous("first_beat", "First beat", 0., 1., 0., Unit::Percent),
    ParamInfo::choice(
        "grid_division",
        "Grid",
        &["Off", "1/4", "1/8", "1/16", "1/32", "1/8 triplet", "1/16 triplet"],
        3,
    ),
    ParamInfo::choice("grid_snap", "Snap", &["Off", "On"], 1),
    ParamInfo::choice(
        "slice_division",
        "Division",
        &["1 bar", "1/2", "1/4", "1/8", "1/16", "1/32", "1/8 triplet", "1/16 triplet"],
        3,
    ),
    ParamInfo::integer("slice_regions", "Regions", (1, 128), 16, Unit::None),
    ParamInfo::choice("slice_overlap", "Slice voices", &["Mono", "Poly", "Through"], 1),
    ParamInfo::integer("source_numerator", "Beats per bar", (1, 16), 4, Unit::None),
    ParamInfo::choice("source_denominator", "Beat unit", &["2", "4", "8", "16"], 1),
    continuous("warp_tempo", "Warp tempo", 0., 999., 0., Unit::None),
];
use crate::modulation::lfo_hz;

/// The gain as a factor: the lowest setting is silence.
fn gain_factor(decibels: f64) -> f64 {
    if decibels <= -60. { 0. } else { 10f64.powf(decibels / 20.) }
}

pub static INFO: DeviceInfo = DeviceInfo::new("sampler", "Sampler", Role::Instrument, PARAMS);

#[derive(Clone, Copy, Default)]
struct Grain {
    window_step: f64,
    position: f64,
    age: usize,
    length: usize,
    step: f64,
    pan: f64,
}
/// Sustain playback's hold: the loop region played on in long segments of the recording, each
/// joined to the next with a short crossfade (a seam) where the next one's waveform continues it
/// (`docs/spec/sampler.md` › Sustain).
#[derive(Clone, Copy, Default)]
struct Hold {
    active: bool,
    /// The segment playing and, during a seam, the one fading in: read position (source frames)
    /// and the gain that keeps the seam's level.
    current: (f64, f64),
    incoming: (f64, f64),
    /// Where the current segment's seam starts (source frames).
    seam_at: f64,
    /// During a seam: output samples into it, its length, and the two segments' correlation
    /// there (0..1), which sets the crossfade's power law.
    in_seam: bool,
    seam: usize,
    seam_length: usize,
    correlation: f64,
    /// The level the hold keeps: the mean square where it took over, the output's running mean
    /// square, and the follower's coefficient per sample.
    target: f64,
    running: f64,
    follow: f64,
}

/// Random points a seam's next segment is chosen from.
const HOLD_CANDIDATES: usize = 8;
/// Points a candidate is compared over: an eighth of a period apart, spanning at least
/// `HOLD_REFERENCE` seconds, so a candidate matches the vibrato's phase and the pitch too.
const HOLD_POINTS: usize = 128;
const HOLD_REFERENCE: f64 = 0.12;
/// A seam's crossfade (seconds); at least four periods, at most a sixth of the loop region.
const HOLD_SEAM: f64 = 0.05;
/// The shortest segment, as a share of the loop region.
const HOLD_SHORTEST: f64 = 1. / 3.;
/// The level follower's time (seconds): slow, so a tremolo or a bow's swell stays and only a
/// drift (a crescendo looped over and over) is taken out.
const HOLD_FOLLOW: f64 = 0.5;
/// The gains a seam or the follower may apply: at most 12 dB either way.
const HOLD_GAIN: (f64, f64) = (0.25, 4.0);
/// The sound's period in source frames, for lining up a seam's segments.
fn hold_period(source_rate: f64, root: f64) -> f64 {
    (source_rate / (440. * 2f64.powf((root - 69.) / 12.))).clamp(16., 2048.)
}

/// What a hold reads: the source, the loop region (`low..end`), the last start a segment may
/// take (`high`), the sound's period, the read step, and a seam's and the shortest segment's
/// length in output samples.
struct HoldSource<'a> {
    source: &'a [f32],
    channels: usize,
    low: f64,
    high: f64,
    end: f64,
    period: f64,
    step: f64,
    seam: usize,
    shortest: usize,
}

impl HoldSource<'_> {
    /// RMS of `length` output samples from `position` (every fourth sample).
    fn level(&self, position: f64, length: usize) -> f64 {
        let points = (length / 4).max(1);
        let sum: f64 = (0..points).map(|n| self.mono(position + (4 * n) as f64 * self.step).powi(2)).sum();
        (sum / points as f64).sqrt().max(1e-9)
    }

    /// Mono level at `position`, for lining up segments.
    fn mono(&self, position: f64) -> f64 {
        let [left, right] = read(self.source, self.channels, position);
        left + right
    }

    /// Where a segment starting at `start` begins its seam: a random point from the shortest
    /// segment on to the last that still ends its seam inside the loop region.
    fn seam_after(&self, start: f64, seed: &mut u32) -> f64 {
        let latest = self.end - self.seam as f64 * self.step - self.period - 2.;
        let earliest = (start + self.shortest as f64 * self.step).min(latest);
        earliest + (random(seed) + 1.) * 0.5 * (latest - earliest).max(0.)
    }

    /// Where the next segment starts and its correlation with the one now at `follow`: of
    /// `HOLD_CANDIDATES` random points of the region, each moved within one period to where its
    /// waveform best continues the one at `follow`, the one scoring highest. The score is the
    /// normalized correlation over `HOLD_POINTS` points, less a quarter of the absolute log of
    /// their energy ratio. The shift is searched every 1/32 period, then sample by sample around
    /// the best.
    fn start(&self, follow: f64, seed: &mut u32, source_rate: f64) -> (f64, f64) {
        let spacing = (self.period / 8.).max(HOLD_REFERENCE * source_rate / HOLD_POINTS as f64).max(1.) * self.step;
        let reference: [f64; HOLD_POINTS] = std::array::from_fn(|n| self.mono(follow + n as f64 * spacing));
        let reference_energy = reference.iter().map(|r| r * r).sum::<f64>() + 1e-12;
        let score = |start: f64| {
            let (mut dot, mut energy) = (0., 1e-12);
            for (n, r) in reference.iter().enumerate() {
                let x = self.mono(start + n as f64 * spacing);
                dot += x * r;
                energy += x * x;
            }
            let correlation = dot / (energy * reference_energy).sqrt();
            (correlation - 0.25 * (energy / reference_energy).ln().abs(), correlation)
        };
        let coarse = (self.period / 32.).max(1.) as usize;
        let mut best = (f64::NEG_INFINITY, self.low, 0.);
        for _ in 0..HOLD_CANDIDATES {
            let candidate = self.low + (random(seed) + 1.) * 0.5 * (self.high - self.low);
            let mut local = (f64::NEG_INFINITY, candidate, 0.);
            for shift in (0..(self.period as usize).clamp(1, 2048)).step_by(coarse) {
                let (value, correlation) = score(candidate + shift as f64);
                if value > local.0 {
                    local = (value, candidate + shift as f64, correlation);
                }
            }
            let around = local.1;
            for offset in 1..coarse {
                for start in [around - offset as f64, around + offset as f64] {
                    let (value, correlation) = score(start);
                    if value > local.0 {
                        local = (value, start, correlation);
                    }
                }
            }
            if local.0 > best.0 {
                best = local;
            }
        }
        (best.1.clamp(self.low, self.high + self.period), best.2.clamp(0., 1.))
    }
}

impl Hold {
    /// Takes over from direct playback at `position`: the recording plays on from there as the
    /// first segment, at the level it has there.
    fn enter(&mut self, from: &HoldSource<'_>, position: f64, rate: f64, seed: &mut u32) {
        let target = from.level(position, from.shortest).powi(2);
        *self = Self {
            active: true,
            current: (position, 1.),
            incoming: (position, 1.),
            seam_at: from.seam_after(position, seed),
            in_seam: false,
            seam: 0,
            seam_length: from.seam.max(4),
            correlation: 1.,
            target,
            running: target,
            follow: 1. / (HOLD_FOLLOW * rate).max(1.),
        };
    }

    /// The next output frame. The current segment plays until its seam, where the next one fades
    /// in over a raised cosine whose sum is kept at unit power for the two segments' correlation;
    /// a slow follower keeps the level the hold took over at.
    fn next(&mut self, from: &HoldSource<'_>, seed: &mut u32, source_rate: f64) -> [f64; 2] {
        let mut out = [0.; 2];
        let (fade_out, fade_in) = if self.in_seam {
            let angle = self.seam as f64 / self.seam_length as f64 * std::f64::consts::FRAC_PI_2;
            let (fade_out, fade_in) = (angle.cos().powi(2), angle.sin().powi(2));
            let power = fade_out * fade_out + fade_in * fade_in + 2. * self.correlation * fade_out * fade_in;
            let norm = 1. / power.max(1e-6).sqrt();
            (fade_out * norm, fade_in * norm)
        } else {
            (1., 0.)
        };
        let data = read(from.source, from.channels, self.current.0);
        out[0] += data[0] * self.current.1 * fade_out;
        out[1] += data[1] * self.current.1 * fade_out;
        self.current.0 += from.step;
        if self.in_seam {
            let data = read(from.source, from.channels, self.incoming.0);
            out[0] += data[0] * self.incoming.1 * fade_in;
            out[1] += data[1] * self.incoming.1 * fade_in;
            self.incoming.0 += from.step;
            self.seam += 1;
            if self.seam >= self.seam_length {
                self.current = self.incoming;
                self.in_seam = false;
                self.seam_at = from.seam_after(self.current.0, seed);
            }
        } else if self.current.0 >= self.seam_at {
            let (start, correlation) = from.start(self.current.0, seed, source_rate);
            let gain = (self.current.1 * from.level(self.current.0, self.seam_length)
                / from.level(start, self.seam_length))
            .clamp(HOLD_GAIN.0, HOLD_GAIN.1);
            self.incoming = (start, gain);
            self.correlation = correlation;
            self.in_seam = true;
            self.seam = 0;
        }
        let mono = out[0] + out[1];
        self.running += (mono * mono - self.running) * self.follow;
        let gain = (self.target / self.running.max(1e-18)).sqrt().clamp(HOLD_GAIN.0, HOLD_GAIN.1);
        [out[0] * gain, out[1] * gain]
    }
}

#[derive(Clone, Copy)]
struct Voice {
    instance: Option<NoteInstanceId>,
    retiring_instance: Option<NoteInstanceId>,
    expression: NoteExpressionState,
    active: bool,
    /// Sustain playback: the segments holding the note once it reaches the loop region.
    hold: Hold,
    /// Continue playback: the model taking over from the recording.
    model: ModelHold,
    deferred_release: bool,
    age: f64,
    key: u8,
    velocity: f64,
    position: f64,
    slice_start: f64,
    slice_end: f64,
    direction: f64,
    envelope: LinearEnvelope,
    step: f64,
    scan: f64,
    next_grain: f64,
    grains: [Grain; 32],
    filters: [Biquad; 2],
    filters_24: [Biquad; 2],
    filter_env: f64,
    filter_released: f64,
    last: [f64; 2],
    fade_from: [f64; 2],
    fade: f64,
}
impl Default for Voice {
    fn default() -> Self {
        Self {
            instance: None,
            retiring_instance: None,
            expression: NoteExpressionState::NEUTRAL,
            active: false,
            hold: Hold::default(),
            model: ModelHold::default(),
            deferred_release: false,
            age: 0.,
            key: 60,
            velocity: 1.,
            position: 0.,
            slice_start: 0.,
            slice_end: 1.,
            direction: 1.,
            envelope: LinearEnvelope::default(),
            step: 1.,
            scan: 0.,
            next_grain: 0.,
            grains: [Grain::default(); 32],
            filters: [Biquad::default(); 2],
            filters_24: [Biquad::default(); 2],
            filter_env: 0.,
            filter_released: 0.,
            last: [0.; 2],
            fade_from: [0.; 2],
            fade: 0.,
        }
    }
}
impl Voice {
    fn release(&mut self) {
        self.deferred_release = false;
        self.envelope.release();
        self.filter_released = self.filter_env;
    }
}
pub struct Sampler {
    lifetimes: NoteLifetimes<64>,
    window: [f64; 2049],
    values: [f64; PARAMS.len()],
    analysis: Arc<SliceAnalysis>,
    manual_slices: Option<Arc<[f64]>>,
    sample_edits: Option<Arc<suhina_device_api::sample_edits::SampleEdits>>,
    tempo: f64,
    rate: f64,
    source_rate: f64,
    channels: usize,
    source: Arc<dyn AsRef<[f32]> + Send + Sync>,
    voices: Vec<Voice>,
    warp: Vec<warp::Voice>,
    next_voice: usize,
    seed: u32,
    phase: f64,
    lfo_cycle: u32,
    bend: f64,
    pedal: bool,
    gain: f64,
    grain_gain: f64,
    block_grain_limit: usize,
    block_grain_gain: f64,
    block_gain: f64,
    pan: f64,
    /// Continue playback (the Instrument's): the continuation and its controls.
    continuation: Option<Box<ContinuationPlayer>>,
    continuation_controls: ContinuationControls,
}
impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}
impl Sampler {
    /// Starts an ordinary note and returns the voice it plays on, or `None` when the key,
    /// velocity or slice map rejects it.
    fn start_note(&mut self, key: u8, velocity: f32) -> Option<usize> {
        // Of a crossed pair of markers the one further right is the end (`docs/spec/sampler.md`).
        let p = ordered(self.values);
        if f64::from(key) < p[31]
            || f64::from(key) > p[32]
            || f64::from(velocity) < p[33]
            || f64::from(velocity) > p[34]
        {
            return None;
        }
        let count = if (p[0] == 2. && p[90] != 1.) || (p[0] != 2. && p[40] > 0.) { 1 } else { p[7] as usize };
        let slices = p[0] == 2. || p[0] == 0. && p[8] == 3.;
        let map = self.analysis.points(&p, self.frames(), self.source_rate, self.tempo);
        let index = usize::from(key).checked_sub(p[30] as usize);
        let slice = if let Some(edits) = self.sample_edits.as_ref().filter(|_| p[0] == 2.) {
            index.and_then(|index| {
                let mut points = std::iter::once(p[1])
                    .chain(self.analysis.edited_points(&p, self.frames(), self.source_rate, self.tempo, edits))
                    .chain(std::iter::once(p[2]));
                let start = points.nth(index)?;
                Some((start, points.next()?))
            })
        } else if p[60] == 3. && self.manual_slices.is_some() {
            index.and_then(|i| self.manual_slices.as_ref().and_then(|points| manual_slice(points, p[1], p[2], i)))
        } else {
            index.and_then(|i| map.get(i..=i + 1).map(|s| (s[0], s[1])))
        };
        if slices && slice.is_none_or(|s| s.1 <= s.0) {
            return None;
        }
        let (slice_start, slice_end) = if slices { slice.unwrap_or((p[1], p[2])) } else { (p[1], p[2]) };
        let (slice_start, slice_end) = if p[0] == 2. && p[90] == 2. {
            if p[10] > 0. { (p[1], slice_end) } else { (slice_start, p[2]) }
        } else {
            (slice_start, slice_end)
        };
        let target_step = 2f64.powf(((if slices { 0. } else { f64::from(key) - p[30] }) + p[5] + p[6] / 100.) / 12.)
            * self.source_rate
            / self.rate;
        let step = if p[29] > 0. {
            self.voices.get((self.next_voice + count - 1) % count).filter(|v| v.active).map_or(target_step, |v| v.step)
        } else {
            target_step
        };
        let slot = self.voices.iter().take(count).position(|v| !v.active).unwrap_or(self.next_voice % count);
        self.next_voice = (slot + 1) % count;
        let frames = self.frames() as f64;
        // Continue playback: forward Sustain with a continuation set.
        let controls = self.continuation_controls;
        let continuing = controls.enabled && p[0] == 0. && p[8] == 4. && p[10] == 0.;
        if let Some(v) = self.voices.get_mut(slot) {
            // A voice keeps one retiring tail. Taking it again before that tail retired ends
            // the older tail now, so its completion is never lost.
            if v.instance.is_some()
                && let Some(older) = v.retiring_instance
            {
                self.lifetimes.finish(older);
            }
            let retiring_instance = v.instance.take().or(v.retiring_instance);
            if v.active && p[47] == 0. && !slices {
                v.key = key;
                v.velocity = f64::from(velocity.clamp(0., 1.));
                v.deferred_release = false;
                v.retiring_instance = retiring_instance;
                v.expression = NoteExpressionState::NEUTRAL;
                return Some(slot);
            }
            if let (Some(generator), Some(player)) = (v.model.generator, self.continuation.as_deref_mut()) {
                player.free(generator);
            }
            let fade_from = v.last;
            if let Some(warp) = self.warp.get_mut(slot) {
                warp.restart();
            }
            let fade = if v.active { 1. } else { 0. };
            let direction = if p[10] > 0. { -1. } else { 1. };
            *v = Voice {
                active: true,
                retiring_instance,
                fade_from,
                fade,
                key,
                velocity: f64::from(velocity.clamp(0., 1.)),
                position: if direction > 0. { slice_start * frames } else { (slice_end * frames - 1.).max(0.) },
                slice_start,
                slice_end,
                direction,
                step,
                ..Voice::default()
            };
            v.envelope.start();
            if continuing && let Some(player) = self.continuation.as_deref_mut() {
                let ratio = target_step * self.rate / self.source_rate;
                let seed = (u64::from(self.seed) << 8) | u64::from(key);
                self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                v.model = ModelHold {
                    on: true,
                    generator: player.claim(slot, ratio, &controls, seed),
                    join: player.join_frame(self.source_rate),
                    step: target_step,
                    ..ModelHold::default()
                };
            }
        }
        Some(slot)
    }
    /// Ordinary device event entry point; identified callers use `NoteInstanceDevice`.
    pub fn event(&mut self, event: NoteEvent) {
        Device::event(self, event);
    }
    /// A sampler with no sample, made for the default sample rate.
    pub fn new() -> Self {
        Self::at(suhina_device_api::DEFAULT_SAMPLE_RATE)
    }

    /// A sampler with no sample, made for `rate` (ADR 0004 › Amendment: a device is made at its
    /// rate), so a sample loaded next gets its stretch voices for that rate; fails for a rate a
    /// device cannot run at.
    pub fn with_rate(rate: f64) -> Result<Self, DeviceError> {
        suhina_device_api::check_sample_rate(rate).map(Self::at)
    }

    fn at(rate: f64) -> Self {
        Self {
            window: std::array::from_fn(|i| crate::nodes::granular::hann(i as f64 / 2048.)),
            values: std::array::from_fn(|i| PARAMS[i].default),
            lifetimes: NoteLifetimes::default(),
            analysis: Arc::new(SliceAnalysis::default()),
            manual_slices: None,
            sample_edits: None,
            tempo: 120.,
            rate,
            source_rate: 48000.,
            channels: 1,
            source: Arc::new(Vec::<f32>::new()),
            voices: vec![Voice::default(); VOICES],
            warp: Vec::new(),
            next_voice: 0,
            seed: 1,
            phase: 0.,
            lfo_cycle: 0,
            bend: 0.,
            pedal: false,
            gain: 1.,
            grain_gain: 1.,
            block_grain_limit: 32,
            block_grain_gain: 1.,
            block_gain: 1.,
            pan: 0.,
            continuation: None,
            continuation_controls: ContinuationControls::default(),
        }
    }
    fn frames(&self) -> usize {
        self.source.as_ref().as_ref().len() / self.channels.max(1)
    }
    /// Gives every voice a new stretch for the current rate: one voice, cloned, which costs about
    /// one voice's build rather than 32 (#1396). Allocates.
    fn build_warp(&mut self) {
        self.warp = vec![warp::Voice::new(self.rate); VOICES];
    }

    /// Sets the held-note continuation and the one it morphs toward (Continue playback), or
    /// none; builds its generators for the current rate. Off the audio thread: allocates.
    pub fn set_continuation(&mut self, own: Option<Arc<Continuation>>, other: Option<Arc<Continuation>>) {
        for voice in &mut self.voices {
            voice.model.generator = None;
        }
        self.continuation = own.map(|own| Box::new(ContinuationPlayer::new(own, other, self.rate)));
    }

    /// Whether a continuation is set.
    pub fn has_continuation(&self) -> bool {
        self.continuation.is_some()
    }

    /// Continue playback on or off, and the controls a continuing note follows. Real-time safe;
    /// the morph applies to the next note.
    pub fn set_continuation_controls(&mut self, controls: ContinuationControls) {
        self.continuation_controls = controls;
    }

    /// Frees generators whose voice no longer plays them, and passes the vibrato controls on.
    fn tend_continuation(&mut self) {
        let Some(player) = self.continuation.as_deref_mut() else { return };
        let controls = self.continuation_controls;
        for slot in 0..crate::sample_continuation::POOL {
            let owner = player.owners.get(slot).copied().flatten();
            let playing = owner
                .and_then(|voice| self.voices.get(voice))
                .is_some_and(|voice| voice.active && voice.model.generator == Some(slot));
            if owner.is_some() && !playing {
                player.free(slot);
            } else if playing && let Some(generator) = player.generators.get_mut(slot) {
                generator.set_vibrato(controls.depth, controls.rate);
            }
        }
    }

    /// Plans the release splice of voice `index`, when it plays its model and the recording has
    /// a release of its own.
    fn plan_splice(&mut self, index: usize) {
        let Some(player) = self.continuation.as_deref_mut() else { return };
        let Some(v) = self.voices.get_mut(index) else { return };
        let Some(slot) = v.model.generator.filter(|_| v.model.joined && v.model.splice.is_none()) else { return };
        let source = self.source.as_ref().as_ref();
        if let Some(splice) = player.splice(slot, source, self.channels, self.source_rate, v.step) {
            v.model.splice = Some(splice);
        }
    }
}
/// The frame at `position` (source frames) of an interleaved source, linearly interpolated.
pub(crate) fn read_frame(source: &[f32], channels: usize, position: f64) -> [f64; 2] {
    read(source, channels, position)
}
fn read(source: &[f32], channels: usize, position: f64) -> [f64; 2] {
    let frames = source.len() / channels.max(1);
    if !(position >= 0. && position < frames as f64) {
        return [0.; 2];
    }
    let i = position as usize;
    let fraction = position.fract();
    std::array::from_fn(|ch| {
        let ch = ch.min(channels.saturating_sub(1));
        let a = source.get(i.saturating_mul(channels).saturating_add(ch)).copied().unwrap_or(0.) as f64;
        let b =
            source.get(i.saturating_add(1).saturating_mul(channels).saturating_add(ch)).copied().unwrap_or(0.) as f64;
        let value = a + (b - a) * fraction;
        if value.is_finite() { value } else { 0. }
    })
}
/// One sample of a continuing voice once its model has taken over: the model, crossfaded in
/// from the recording over the join and out into the recording's release at a splice. None
/// before the join, after a splice and without a generator: the recording plays as before.
/// Before the join, generates the model ahead.
fn continue_voice(
    v: &mut Voice,
    player: Option<&mut ContinuationPlayer>,
    source: &[f32],
    channels: usize,
    width: f64,
) -> Option<[f64; 2]> {
    let slot = v.model.generator?;
    let player = player?;
    let crossfade = player.crossfade();
    let generator = player.generators.get_mut(slot)?;
    generator.set_pitch_ratio(v.step / v.model.step.max(1e-12));
    if !v.model.joined && v.position >= v.model.join {
        v.model.joined = true;
        v.model.fade = 0;
    }
    if !v.model.joined {
        // Two control frames per control frame's time: the lookahead fills before the join.
        if v.model.tick.is_multiple_of((generator.control_samples() / 2).max(1)) {
            generator.produce(1);
        }
        v.model.tick = v.model.tick.wrapping_add(1);
        return None;
    }
    let [mid, side] = generator.next_sample();
    let (mid, side) = (f64::from(mid), f64::from(side) * width);
    let mut out = [mid + side, mid - side];
    if v.model.fade < crossfade {
        let recording = widen(read(source, channels, v.position), width);
        v.position += v.step;
        let w = fade_in(v.model.fade, crossfade);
        out = [recording[0] * (1.0 - w) + out[0] * w, recording[1] * (1.0 - w) + out[1] * w];
        v.model.fade += 1;
    }
    if let Some((countdown, gain, start)) = v.model.splice {
        if countdown == 0 {
            v.model.splice = None;
            v.model.splicing = Some(0);
            v.model.gain = gain;
            v.position = start;
        } else {
            v.model.splice = Some((countdown - 1, gain, start));
        }
    }
    if let Some(k) = v.model.splicing {
        let recording = widen(read(source, channels, v.position), width).map(|s| s * v.model.gain);
        v.position += v.step;
        let w = fade_in(k, crossfade);
        out = [out[0] * (1.0 - w) + recording[0] * w, out[1] * (1.0 - w) + recording[1] * w];
        if k + 1 >= crossfade {
            // The recording carries on from here; the generator is free for another note.
            v.model.splicing = None;
            v.model.generator = None;
            v.model.joined = false;
            player.free(slot);
        } else {
            v.model.splicing = Some(k + 1);
        }
    }
    Some(out)
}
fn source_fade(position: f64, start: f64, end: f64, fade_in: f64, fade_out: f64, rate: f64) -> f64 {
    let fade = |distance: f64, ms: f64| if ms > 0. { (distance / (ms * rate / 1000.)).clamp(0., 1.) } else { 1. };
    fade(position - start, fade_in) * fade(end - 1. - position, fade_out)
}
fn random(seed: &mut u32) -> f64 {
    crate::nodes::random::bipolar(seed)
}
/// The values with each crossed pair in order: of Start and End, and of Loop start and Loop end,
/// the marker further right is always the end (`docs/spec/sampler.md`). Playback reads these, the
/// stored values stay as they were set.
fn ordered(mut p: [f64; PARAMS.len()]) -> [f64; PARAMS.len()] {
    for (a, b) in [(1, 2), (3, 4)] {
        if let (Some(&low), Some(&high)) = (p.get(a), p.get(b))
            && low > high
            && let Ok([first, second]) = p.get_disjoint_mut([a, b])
        {
            std::mem::swap(first, second);
        }
    }
    p
}

impl SampleSource for Sampler {
    fn sample_position(&self) -> Option<f32> {
        self.sample_positions().into_iter().flatten().next()
    }
    fn sample_positions(&self) -> [Option<f32>; 32] {
        let frames = self.frames();
        std::array::from_fn(|i| {
            let v = self.voices.get(i).filter(|v| v.active && frames > 0)?;
            let p = ordered(self.values);
            let position = if p[0] != 1. {
                v.position
            } else {
                let start = p[1] * frames as f64;
                let span = (p[2] - p[1]) * frames as f64;
                start + (p[20] * span + v.scan).rem_euclid(span.max(1.))
            };
            Some((position / frames as f64) as f32)
        })
    }
    fn load_shared_sample(&mut self, rate: u32, channels: u16, samples: Arc<dyn AsRef<[f32]> + Send + Sync>) {
        let pcm = samples.as_ref().as_ref();
        if !(1000..=384000).contains(&rate)
            || !(1..=32).contains(&channels)
            || !pcm.len().is_multiple_of(usize::from(channels))
        {
            return;
        }
        self.analysis = shared_analysis(&samples, channels, rate);
        self.source = samples;
        self.channels = usize::from(channels);
        self.source_rate = f64::from(rate);
        self.build_warp();
        self.reset();
    }
    fn load_sample(&mut self, rate: u32, channels: u16, samples: Arc<[f32]>) {
        self.load_shared_sample(rate, channels, Arc::new(samples));
    }
    fn load_sample_slices(&mut self, slices: Option<Arc<[f64]>>) {
        self.manual_slices = slices.map(|points| {
            let mut points: Vec<_> = points.iter().copied().filter(|v| v.is_finite() && *v > 0. && *v < 1.).collect();
            points.sort_unstable_by(f64::total_cmp);
            points.dedup();
            Arc::from(points)
        });
    }
    fn swap_sample_slices(&mut self, slices: &mut Option<Arc<[f64]>>) {
        std::mem::swap(&mut self.manual_slices, slices);
    }
    fn load_sample_edits(&mut self, edits: Option<Arc<suhina_device_api::sample_edits::SampleEdits>>) {
        self.sample_edits = edits;
    }
    fn swap_sample_edits(&mut self, edits: &mut Option<Arc<suhina_device_api::sample_edits::SampleEdits>>) {
        std::mem::swap(&mut self.sample_edits, edits);
    }
}
impl Device for Sampler {
    fn samples(&mut self) -> Option<&mut dyn SampleSource> {
        Some(self)
    }
    fn samples_ref(&self) -> Option<&dyn SampleSource> {
        Some(self)
    }
    fn info(&self) -> &DeviceInfo {
        &INFO
    }
    /// Moves the sampler to `rate`: a loaded sample's stretch voices are built again only when
    /// the rate is another than the one they were built for.
    fn prepare(&mut self, rate: f64) -> Result<(), DeviceError> {
        suhina_device_api::check_sample_rate(rate)?;
        let moved = rate != self.rate;
        self.rate = rate;
        if moved && self.frames() > 0 {
            self.build_warp();
        }
        if let Some(player) = self.continuation.take() {
            // Generators are built for one rate.
            self.continuation = Some(if player.rate == rate {
                player
            } else {
                Box::new(ContinuationPlayer::new(player.own.clone(), player.other.clone(), rate))
            });
        }
        self.reset();
        Ok(())
    }
    fn reset(&mut self) {
        self.lifetimes.finish_all();
        self.voices.fill(Voice::default());
        for voice in &mut self.warp {
            voice.restart();
        }
        if let Some(player) = self.continuation.as_deref_mut() {
            for slot in 0..crate::sample_continuation::POOL {
                player.free(slot);
            }
        }
        self.next_voice = 0;
        self.seed = 1;
        self.phase = 0.;
        self.lfo_cycle = 0;
        self.bend = 0.;
        self.pedal = false;
        self.gain = gain_factor(self.values[18]);
        self.grain_gain = 1. / (self.values[23] * self.values[22] / 1000. * 0.5).clamp(1., 16.);
        self.pan = self.values[19];
    }
    fn set_param(&mut self, index: usize, value: f64) {
        if PARAMS.get(index).is_some_and(|p| p.id == "warp_beats")
            && self.values.get(index).is_some_and(|old| *old != value)
        {
            self.values[83] = 0.;
            self.values[93] = 0.;
        }
        let warp_enabled = self.values[77] > 0.;
        if let Some((p, v)) = PARAMS.get(index).zip(self.values.get_mut(index)) {
            let next = p.clamp(value);
            if p.id == "reverse" && next != *v {
                for voice in &mut self.voices {
                    voice.direction = -voice.direction;
                    for grain in &mut voice.grains {
                        grain.step = -grain.step;
                    }
                }
            }
            if matches!(p.id.as_ref(), "reverse" | "mode") && next != *v {
                for voice in &mut self.warp {
                    voice.restart();
                }
            }
            if matches!(p.id.as_ref(), "playback" | "mode") && next != *v {
                self.lifetimes.finish_all();
                self.voices.fill(Voice::default());
                for voice in &mut self.warp {
                    voice.restart();
                }
            }
            let warp_transition =
                p.id == "warp" && next != *v || p.id == "warp_mode" && (next == 0.) != (*v == 0.) && warp_enabled;
            if warp_transition {
                for voice in &mut self.voices {
                    if voice.active {
                        voice.fade_from = voice.last;
                        voice.fade = 1.;
                    }
                }
                for voice in &mut self.warp {
                    voice.restart();
                }
            }
            *v = next;
        }
    }
    fn param(&self, index: usize) -> Option<f64> {
        self.values.get(index).copied()
    }
    fn event(&mut self, event: NoteEvent) {
        match event {
            NoteEvent::Instance { id, event } => {
                let _ = NoteInstanceDevice::event(self, id, event);
            }
            NoteEvent::On { key, velocity } if key <= 127 && velocity.is_finite() && velocity > 0. => {
                self.start_note(key, velocity);
            }
            NoteEvent::Off { key, .. } | NoteEvent::On { key, velocity: 0. } => {
                let slicing = self.values[0] == 2. || self.values[0] == 0. && self.values[8] == 3.;
                if slicing && self.values[84] > 0. || !slicing && self.values[8] != 1. && self.values[8] != 3. {
                    for index in 0..self.voices.len() {
                        let Some(v) = self.voices.get_mut(index) else { continue };
                        if v.key == key && v.active && v.instance.is_none() {
                            if self.pedal {
                                // A released tail keeps advancing; pedal-up must not recapture it.
                                if !v.envelope.is_releasing() && !v.envelope.is_done() {
                                    v.deferred_release = true;
                                }
                            } else {
                                v.release();
                                self.plan_splice(index);
                            }
                        }
                    }
                }
            }
            NoteEvent::AllNotesOff => {
                self.pedal = false;
                for index in 0..self.voices.len() {
                    if let Some(v) = self.voices.get_mut(index) {
                        v.release();
                        self.plan_splice(index);
                    }
                }
            }
            NoteEvent::Controller { number: 64, value } if value.is_finite() => {
                let was_down = self.pedal;
                self.pedal = value >= 0.5;
                if was_down && !self.pedal {
                    for index in 0..self.voices.len() {
                        let Some(voice) = self.voices.get_mut(index) else { continue };
                        if voice.deferred_release {
                            voice.release();
                            self.plan_splice(index);
                        }
                    }
                }
            }
            NoteEvent::PitchBend(value) if value.is_finite() => {
                self.bend = f64::from(value.clamp(-1., 1.)) * 2.;
            }
            _ => {}
        }
    }
    fn set_context(&mut self, context: &ProcessContext) {
        if context.tempo.is_finite() && context.tempo > 0. {
            self.tempo = context.tempo;
        }
    }
    fn note_instances(&mut self) -> Option<&mut dyn NoteInstanceDevice> {
        Some(self)
    }
    fn process(&mut self, context: &ProcessContext, buffer: &mut [f32]) {
        self.prepare_block(context);
        self.process_prepared(buffer);
    }
}

impl NoteInstanceDevice for Sampler {
    fn event(&mut self, id: NoteInstanceId, event: NoteInstanceEvent) -> Result<(), NoteInstanceDispatchError> {
        let event = event.validated().ok_or(NoteInstanceDispatchError::InvalidEvent)?;
        if let NoteInstanceEvent::On { key, velocity, expression } = event {
            if self.lifetimes.contains(id) {
                return Err(NoteInstanceDispatchError::AlreadyActive);
            }
            self.lifetimes.admit(id)?;
            // The voice is chosen as for an ordinary note: an idle one, else the next in turn,
            // whose occurrence moves to its retiring tail. The legacy handler retains its MIDI
            // zero-velocity rule; this native endpoint starts an occurrence at zero velocity
            // and restores the exact native value.
            let slot = self.start_note(key, velocity.max(f32::MIN_POSITIVE));
            if let Some(voice) = slot.and_then(|slot| self.voices.get_mut(slot)).filter(|v| v.active) {
                voice.instance = Some(id);
                voice.expression = expression;
                voice.velocity = f64::from(velocity);
            } else {
                self.lifetimes.finish(id);
            }
            return Ok(());
        }
        if let NoteInstanceEvent::Choke { .. } = event
            && let Some(voice) = self.voices.iter_mut().find(|v| v.retiring_instance == Some(id))
        {
            // A newer note took this occurrence's voice and it still sounds as the crossfade:
            // a choke ends that crossfade at once, without touching the newer note.
            voice.fade = 0.;
            voice.fade_from = [0.; 2];
            voice.retiring_instance = None;
            self.lifetimes.finish(id);
            return Ok(());
        }
        let index = self
            .voices
            .iter()
            .position(|v| v.instance == Some(id) && v.active)
            .ok_or(NoteInstanceDispatchError::UnknownInstance)?;
        let voice = self.voices.get_mut(index).ok_or(NoteInstanceDispatchError::UnknownInstance)?;
        match event {
            NoteInstanceEvent::Expression { kind, value } => {
                voice.expression.set(kind, value);
            }
            NoteInstanceEvent::Off { .. } => {
                let slicing = self.values[0] == 2. || self.values[0] == 0. && self.values[8] == 3.;
                if slicing && self.values[84] > 0. || !slicing && self.values[8] != 1. && self.values[8] != 3. {
                    voice.release();
                    self.plan_splice(index);
                }
            }
            NoteInstanceEvent::Choke { .. } => {
                voice.active = false;
                voice.instance = None;
                self.lifetimes.finish(id);
            }
            NoteInstanceEvent::On { .. } => {}
        }
        Ok(())
    }

    fn drain_finished(&mut self, output: &mut [Option<NoteInstanceId>]) -> usize {
        for voice in &mut self.voices {
            if !voice.active
                && let Some(id) = voice.instance.take()
            {
                self.lifetimes.finish(id);
            }
            if (!voice.active || voice.fade == 0.)
                && let Some(id) = voice.retiring_instance.take()
            {
                self.lifetimes.finish(id);
            }
        }
        self.lifetimes.drain(output)
    }
}

type SharedPcm = Arc<dyn AsRef<[f32]> + Send + Sync>;
type CachedAnalysis = (std::sync::Weak<dyn AsRef<[f32]> + Send + Sync>, u16, u32, std::sync::Weak<SliceAnalysis>);

// Loading is off the audio thread; weak entries do not retain unused PCM or analyses.
fn shared_analysis(source: &SharedPcm, channels: u16, rate: u32) -> Arc<SliceAnalysis> {
    static CACHE: std::sync::Mutex<Vec<CachedAnalysis>> = std::sync::Mutex::new(Vec::new());
    let build = || Arc::new(SliceAnalysis::new(source.as_ref().as_ref(), usize::from(channels), f64::from(rate)));
    let Ok(mut cache) = CACHE.lock() else { return build() };
    cache.retain(|(pcm, _, _, analysis)| pcm.strong_count() > 0 && analysis.strong_count() > 0);
    if let Some(analysis) = cache.iter().find_map(|(pcm, ch, hz, analysis)| {
        (*ch == channels && *hz == rate && pcm.ptr_eq(&Arc::downgrade(source))).then(|| analysis.upgrade()).flatten()
    }) {
        return analysis;
    }
    let analysis = build();
    cache.push((Arc::downgrade(source), channels, rate, Arc::downgrade(&analysis)));
    analysis
}

pub use crate::nodes::slicing::SliceAnalysis;

fn manual_slice(points: &[f64], start: f64, end: f64, index: usize) -> Option<(f64, f64)> {
    if end <= start {
        return None;
    }
    let first = points.partition_point(|p| *p <= start);
    let last = points.partition_point(|p| *p < end);
    let count = last.saturating_sub(first);
    if index > count {
        return None;
    }
    Some((
        if index == 0 { start } else { *points.get(first.checked_add(index)?.checked_sub(1)?)? },
        if index == count { end } else { *points.get(first.checked_add(index)?)? },
    ))
}

#[cfg(test)]
mod shared_source_tests {
    use super::*;
    #[test]
    fn preparations_share_pcm_and_analysis_but_distinguish_format_and_source() {
        let source: SharedPcm = Arc::new(vec![0.25; 96000]);
        let mut a = Sampler::new();
        let mut b = Sampler::new();
        a.load_shared_sample(48000, 1, source.clone());
        b.load_shared_sample(48000, 1, source.clone());
        assert!(Arc::ptr_eq(&a.source, &source));
        assert!(Arc::ptr_eq(&a.analysis, &b.analysis));
        b.load_shared_sample(24000, 1, source.clone());
        assert!(!Arc::ptr_eq(&a.analysis, &b.analysis));
        b.load_shared_sample(48000, 2, source.clone());
        assert!(!Arc::ptr_eq(&a.analysis, &b.analysis));
        b.load_shared_sample(48000, 1, Arc::new(vec![0.25; 96000]));
        assert!(!Arc::ptr_eq(&a.analysis, &b.analysis));
        drop(a);
        drop(b);
        assert_eq!(Arc::strong_count(&source), 1, "cache must not retain PCM");
    }

    #[test]
    fn a_sampler_builds_one_warp_voice_and_clones_it_for_its_rate() {
        // The rates of the voices built since `before`; each build is cloned into the 32.
        let built = |before: usize| warp::BUILT.with(|built| built.borrow().get(before..).unwrap_or_default().to_vec());
        let count = || warp::BUILT.with(|built| built.borrow().len());
        let source: SharedPcm = Arc::new(vec![0.25; 4800]);
        for rate in [44_100., 48_000., 96_000.] {
            // Made at its rate, prepared at it (as a device kept across a rate change is), then
            // given its sample.
            let before = count();
            let mut sampler = Sampler::with_rate(rate).unwrap();
            sampler.prepare(rate).unwrap();
            assert!(sampler.warp.is_empty(), "no sample, no voices");
            sampler.load_shared_sample(48000, 1, source.clone());
            assert_eq!((built(before), sampler.warp.len()), (vec![rate], VOICES), "prepared, then loaded");
            // A new sample gets new voices.
            sampler.load_shared_sample(48000, 1, Arc::new(vec![0.5; 4800]));
            assert_eq!(built(before), vec![rate, rate], "a new sample");
            // Given its sample, then prepared at the rate it was made for.
            let before = count();
            let mut sampler = Sampler::with_rate(rate).unwrap();
            sampler.load_shared_sample(48000, 1, source.clone());
            sampler.prepare(rate).unwrap();
            assert_eq!((built(before), sampler.warp.len()), (vec![rate], VOICES), "loaded, then prepared");
        }
        // Moved to another rate, a loaded Sampler builds its voices again, once, for that rate.
        let before = count();
        let mut sampler = Sampler::with_rate(48_000.).unwrap();
        sampler.load_shared_sample(48000, 1, source);
        sampler.prepare(44_100.).unwrap();
        assert_eq!((built(before), sampler.warp.len()), (vec![48_000., 44_100.], VOICES), "moved to 44.1 kHz");
        assert!(Sampler::with_rate(0.).is_err());
    }
}

#[cfg(test)]
mod rate_tests {
    #[test]
    fn rate_modes_match_delay_and_follow_tempo() {
        for (mode, expected) in [(0., 5.), (1., 2.), (2., 2.), (3., 3.), (4., 4. / 3.), (5., 2.)] {
            let division = 4.;
            assert!((super::lfo_hz(5., 500., division, mode, 120.) - expected).abs() < 1e-12);
            if mode >= 2. {
                assert!((super::lfo_hz(5., 500., division, mode, 60.) - expected / 2.).abs() < 1e-12);
            }
        }
    }
}

fn grain_window(table: &[f64; 2049], phase: f64) -> f64 {
    let index = (phase as usize).min(2047);
    let a = table.get(index).copied().unwrap_or(0.);
    let b = table.get(index + 1).copied().unwrap_or(0.);
    a + (b - a) * (phase - index as f64).clamp(0., 1.)
}

#[cfg(test)]
mod grain_window_tests {
    use suhina_device_api::SampleSource as _;

    #[test]
    fn grain_normalization_smooths_voice_count_changes_independently_of_master_gain() {
        use suhina_device_api::{Device, NoteEvent, ProcessContext};
        let mut sampler = super::Sampler::new();
        sampler.load_sample(48000, 1, vec![0.25; 48000].into());
        sampler.prepare(48000.).unwrap();
        for (id, value) in [("mode", 1.), ("poly", 32.), ("size", 500.), ("density", 100.)] {
            sampler.set_param(super::INFO.param_index(id).unwrap(), value);
        }
        for key in 48..56 {
            sampler.event(NoteEvent::On { key, velocity: 1. });
        }
        let context = ProcessContext::default();
        sampler.process(&context, &mut [0.; 4800]);
        let before = sampler.grain_gain;
        let master = sampler.gain;
        sampler.event(NoteEvent::On { key: 56, velocity: 1. });
        sampler.process(&context, &mut [0.; 2]);
        let target = 1. / 14.;
        assert!(sampler.grain_gain > before && sampler.grain_gain < target);
        assert!((sampler.grain_gain - before) < (target - before) * 0.01);
        assert_eq!(sampler.gain, master);
        sampler.process(&context, &mut [0.; 4800]);
        assert!((sampler.grain_gain - target).abs() < 1e-6);
    }

    #[test]
    fn many_grain_voices_share_one_bounded_budget() {
        use suhina_device_api::{Device, NoteEvent, ProcessContext};
        let mut sampler = super::Sampler::new();
        sampler.load_sample(48000, 1, vec![0.25; 48000].into());
        sampler.prepare(48000.).unwrap();
        for (id, value) in [("mode", 1.), ("poly", 32.), ("size", 500.), ("density", 100.)] {
            sampler.set_param(super::INFO.param_index(id).unwrap(), value);
        }
        for key in 48..80 {
            sampler.event(NoteEvent::On { key, velocity: 1. });
        }
        let mut block = [0.; 512];
        for _ in 0..100 {
            sampler.process(&ProcessContext::default(), &mut block);
            assert!(sampler.voices.iter().flat_map(|v| v.grains).filter(|g| g.age < g.length).count() <= 256);
        }
        assert_eq!(sampler.voices.iter().filter(|v| v.active).count(), 32);
        assert!(block.iter().all(|v| v.is_finite()));
    }

    #[test]
    #[ignore = "local performance measurement, not a portable timing gate"]
    fn measure_maximum_grain_workload() {
        use suhina_device_api::{Device, NoteEvent, ProcessContext};
        let mut sampler = super::Sampler::new();
        sampler.load_sample(48000, 1, vec![0.25; 48000].into());
        sampler.prepare(48000.).unwrap();
        for (id, value) in [("mode", 1.), ("poly", 32.), ("size", 500.), ("density", 100.)] {
            sampler.set_param(super::INFO.param_index(id).unwrap(), value);
        }
        for key in 48..80 {
            sampler.event(NoteEvent::On { key, velocity: 1. });
        }
        let mut block = [0.; 512];
        let context = ProcessContext::default();
        for _ in 0..100 {
            sampler.process(&context, &mut block);
        }
        assert_eq!(sampler.voices.iter().filter(|v| v.active).count(), 32);
        assert!(sampler.voices.iter().flat_map(|v| v.grains).filter(|g| g.age < g.length).count() >= 32 * 7);
        let start = std::time::Instant::now();
        for _ in 0..200 {
            sampler.process(&context, &mut block);
            std::hint::black_box(&block);
        }
        let elapsed = start.elapsed().as_secs_f64();
        println!(
            "32 voices, bounded to 256 grains total, 48 kHz: {:.3} s for {:.3} s audio ({:.1}% real-time budget)",
            elapsed,
            51200. / 48000.,
            elapsed / (51200. / 48000.) * 100.
        );
        assert!(block.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn cached_hann_matches_the_formula_including_endpoints() {
        let sampler = super::Sampler::new();
        for length in [2, 17, 480, 24000] {
            for age in 0..length {
                let phase = age as f64 / (length - 1) as f64;
                let expected = 0.5 - 0.5 * (std::f64::consts::TAU * phase).cos();
                assert!((super::grain_window(&sampler.window, phase * 2048.) - expected).abs() < 1e-6);
            }
        }
    }
}

impl Sampler {
    pub(crate) fn prepare_block(&mut self, context: &ProcessContext) {
        self.set_context(context);
        let p = self.values;
        let grain_limit = (256 / self.voices.iter().filter(|v| v.active).count().max(1)).min(32);
        for voice in &mut self.voices {
            for grain in voice.grains.iter_mut().skip(grain_limit) {
                grain.length = 0;
            }
        }
        let target_grain_gain = 1. / (p[23] * p[22] / 1000. * 0.5).min(grain_limit as f64 * 0.5).max(1.);
        let target_gain = gain_factor(p[18]);
        self.block_grain_limit = grain_limit;
        self.block_grain_gain = target_grain_gain;
        self.block_gain = target_gain;
        self.tend_continuation();
    }

    pub(crate) fn process_prepared(&mut self, buffer: &mut [f32]) {
        let p = ordered(self.values);
        let total = self.frames() as f64;
        let start = p[1] * total;
        let end = p[2] * total;
        let lo = (p[3] * total).max(start);
        let hi = (p[4] * total).min(end);
        let looping = p[0] != 2. && p[8] == 2. && hi > lo + 1.;
        // Sustain: the note plays from its start, then holds in segments of the loop region.
        let sustaining = p[0] == 0. && p[8] == 4. && p[10] == 0.;
        let period = hold_period(self.source_rate, p[30]);
        let width = self.continuation_controls.width;
        let grain_limit = self.block_grain_limit;
        let target_grain_gain = self.block_grain_gain;
        let target_gain = self.block_gain;
        for frame in buffer.as_chunks_mut::<2>().0 {
            let mut out = [0.; 2];
            let phase = self.phase + lfo_hz(p[37], p[81], p[82], p[80], self.tempo) / self.rate;
            if phase >= 1. {
                self.lfo_cycle = self.lfo_cycle.wrapping_add(1);
            }
            self.phase = phase.fract();
            let shape =
                suhina_dsp::lfo::Shape::ALL.get(p[56] as usize).copied().unwrap_or(suhina_dsp::lfo::Shape::Sine);
            let modulation =
                suhina_dsp::lfo::wave(shape, if p[56] == 5. { f64::from(self.lfo_cycle) } else { self.phase });
            let lfo = modulation * p[38];
            let volume = 1. - p[54] * (modulation + 1.) * 0.5;
            self.gain += (target_gain - self.gain) * (1. / (0.005 * self.rate));
            // A fade to silence ends there, never in ever smaller numbers.
            if target_gain == 0. && self.gain < 1e-9 {
                self.gain = 0.;
            }
            self.grain_gain += (target_grain_gain - self.grain_gain) * (1. / (0.005 * self.rate));
            self.pan += (p[19] - self.pan) * (1. / (0.005 * self.rate));
            for (index, v) in self.voices.iter_mut().enumerate() {
                if !v.active {
                    continue;
                }
                if index >= (if (p[0] == 2. && p[90] != 1.) || (p[0] != 2. && p[40] > 0.) { 1 } else { p[7] as usize })
                    || end <= start + 1.
                    || total < 2.
                {
                    v.active = false;
                    continue;
                }
                let slices = p[0] == 2. || p[0] == 0. && p[8] == 3.;
                let (start, end) = if slices { (v.slice_start * total, v.slice_end * total) } else { (start, end) };
                v.envelope.next(
                    &LinearEnvelopeSettings { attack: p[11], decay: p[12], sustain: p[13], release: p[14] },
                    self.rate,
                );
                if v.envelope.is_done() {
                    v.active = false;
                    continue;
                }
                let pitch_env = if v.age < p[42] {
                    v.age / p[42]
                } else {
                    (1. - (v.age - p[42]) / p[43].max(1. / self.rate)).max(0.)
                };
                v.age += 1. / self.rate;
                let target_step = 2f64.powf(
                    ((if slices { 0. } else { f64::from(v.key) - p[30] })
                        + p[5]
                        + p[6] / 100.
                        + self.bend
                        + v.expression.value(NoteExpressionKind::Pitch)
                        + lfo
                        + pitch_env * p[41])
                        / 12.,
                ) * self.source_rate
                    / self.rate;
                v.step += (target_step - v.step) / (p[29] * self.rate).max(1.);
                let mut signal = [0.; 2];
                // The hold's segments: a seam of 50 ms (at least four periods, at most a sixth of
                // the loop region) and segments at least a third of the region long.
                let region = (hi - lo) / v.step.max(1e-6);
                let seam = (HOLD_SEAM * self.rate).max(4. * period / v.step.max(1e-6)).min(region / 6.) as usize;
                let shortest = (region * HOLD_SHORTEST) as usize;
                // The last start a segment may take and still play its shortest and its seam.
                let hold_high = hi - (shortest + seam) as f64 * v.step - period - 2.;
                let from = HoldSource {
                    source: self.source.as_ref().as_ref(),
                    channels: self.channels,
                    low: lo,
                    high: hold_high.max(lo),
                    end: hi,
                    period,
                    step: v.step,
                    seam,
                    shortest,
                };
                let continued = continue_voice(
                    v,
                    self.continuation.as_deref_mut(),
                    self.source.as_ref().as_ref(),
                    self.channels,
                    width,
                );
                if sustaining && !v.hold.active && v.model.generator.is_none() && hold_high > lo && v.position >= lo {
                    v.hold.enter(&from, v.position, self.rate, &mut self.seed);
                }
                if let Some(model) = continued {
                    signal = model;
                } else if v.hold.active {
                    signal = v.hold.next(&from, &mut self.seed, self.source_rate);
                    if v.model.on {
                        signal = widen(signal, width);
                    }
                } else if p[0] != 1. {
                    if v.position < start || v.position >= end {
                        v.active = false;
                        continue;
                    }
                    let speed = if p[83] > 0. {
                        self.tempo / p[83]
                    } else if p[93] > 0. {
                        self.tempo / p[93]
                    } else {
                        (p[2] - p[1]) * total / self.source_rate * self.tempo / (60. * p[79])
                    }
                    .clamp(1e-6, 1e6);
                    let warped = p[77] > 0.;
                    let stretched = warped && p[78] > 0. && (0.25..=4.).contains(&speed);
                    signal = if stretched {
                        self.warp.get_mut(index).map_or([0.; 2], |warp| {
                            warp.next(speed, v.step * self.rate / self.source_rate, |ahead| {
                                let mut position = v.position + ahead * self.source_rate / self.rate * v.direction;
                                if looping
                                    && ((v.direction > 0. && position >= hi) || (v.direction < 0. && position < lo))
                                {
                                    let span = hi - lo;
                                    if p[9] > 0. {
                                        let phase = (position - lo).rem_euclid(2. * (span - 1.));
                                        position =
                                            lo + if phase < span - 1. { phase } else { 2. * (span - 1.) - phase };
                                    } else if position >= hi || position < lo {
                                        position = lo + (position - lo).rem_euclid(span);
                                    }
                                }
                                if position < start || position >= end {
                                    [0.; 2]
                                } else {
                                    read(self.source.as_ref().as_ref(), self.channels, position)
                                }
                            })
                        })
                    } else {
                        read(self.source.as_ref().as_ref(), self.channels, v.position)
                    };
                    let (fade_start, fade_end) =
                        if looping && v.position >= lo && v.position < hi { (lo, hi) } else { (start, end) };
                    let fade = source_fade(v.position, fade_start, fade_end, p[57], p[58], self.source_rate);
                    signal.iter_mut().for_each(|sample| *sample *= fade);
                    if v.model.on {
                        // A continuing note's recording, widened as its model is, and after a
                        // release splice at the splice's gain.
                        signal = widen(signal, width).map(|sample| sample * v.model.gain);
                    }
                    v.position +=
                        if warped { speed * self.source_rate / self.rate * v.direction } else { v.step * v.direction };
                    let boundary = if p[9] > 0. { hi - 1. } else { hi };
                    if looping
                        && ((v.direction > 0. && v.position >= boundary) || (v.direction < 0. && v.position < lo))
                    {
                        let length = boundary - lo;
                        if p[9] > 0. {
                            let phase = (v.position - lo).rem_euclid(2. * length);
                            v.position = lo + if phase < length { phase } else { 2. * length - phase };
                            v.position = v.position.min(boundary);
                            v.direction *= if phase < length { 1. } else { -1. };
                        } else {
                            v.position = lo + (v.position - lo).rem_euclid(length);
                        }
                    }
                } else {
                    if p[26] == 0. {
                        v.scan += p[21] * self.source_rate / self.rate;
                    }
                    if crate::nodes::granular::tick(&mut v.next_grain, self.rate / p[23])
                        && let Some(g) = v.grains.iter_mut().take(grain_limit).find(|g| g.age >= g.length)
                    {
                        let length = (p[22] * self.rate / 1000.).max(2.) as usize;
                        *g = Grain {
                            window_step: 2048. / (length - 1) as f64,
                            position: start
                                + ((p[20] * (end - start)
                                    + v.scan
                                    + random(&mut self.seed) * p[24] * (end - start) * 0.5)
                                    .rem_euclid(end - start)),
                            age: 0,
                            length,
                            step: v.step * v.direction,
                            pan: random(&mut self.seed) * p[25],
                        };
                    }
                    for g in v.grains.iter_mut().take(grain_limit) {
                        if g.age >= g.length {
                            continue;
                        }
                        let window = grain_window(&self.window, g.age as f64 * g.window_step);
                        let position = start + (g.position - start).rem_euclid(end - start);
                        let data = read(self.source.as_ref().as_ref(), self.channels, position);
                        let window = window * source_fade(position, start, end, p[57], p[58], self.source_rate);
                        signal[0] += data[0] * window * (1. - g.pan.max(0.)) * self.grain_gain;
                        signal[1] += data[1] * window * (1. + g.pan.min(0.)) * self.grain_gain;
                        g.age += 1;
                        g.position += g.step;
                    }
                }
                v.filter_env = if v.envelope.is_releasing() {
                    (v.filter_env - v.filter_released / (p[51] * self.rate).max(1.)).max(0.)
                } else if v.age < p[48] {
                    v.age / p[48].max(1. / self.rate)
                } else {
                    (1. - (v.age - p[48]) / p[49].max(1. / self.rate) * (1. - p[50])).max(p[50])
                };
                if p[15] > 0. {
                    let freq = (p[16]
                        * 2f64.powf(
                            v.filter_env * p[35]
                                + v.velocity * p[36]
                                + (f64::from(v.key) - p[30]) / 12. * p[46]
                                + modulation * p[53]
                                + (v.expression.value(NoteExpressionKind::Timbre) - 0.5) * 4.,
                        ))
                    .clamp(20., (self.rate * 0.49).min(20000.));
                    let c = match p[15] as u8 {
                        1 => Coefficients::lowpass(self.rate, freq, p[17]),
                        2 => Coefficients::highpass(self.rate, freq, p[17]),
                        3 => Coefficients::bandpass(self.rate, freq, p[17]),
                        _ => {
                            let bp = Coefficients::bandpass(self.rate, freq, p[17]);
                            Coefficients { b0: 1. - bp.b0, b1: bp.a1 - bp.b1, b2: bp.a2 - bp.b2, ..bp }
                        }
                    };
                    for (ch, sample) in signal.iter_mut().enumerate() {
                        if let (Some(filter), Some(second)) = (v.filters.get_mut(ch), v.filters_24.get_mut(ch)) {
                            filter.set(c);
                            second.set(c);
                            *sample = filter.process(*sample);
                            if p[45] > 0. {
                                *sample = second.process(*sample);
                            }
                        }
                    }
                }
                for ch in 0..2 {
                    // Neutral pressure preserves old patches; pressure adds up to 6 dB and
                    // timbre is a bipolar brightness source when the voice filter is enabled.
                    let next = signal[ch]
                        * v.envelope.value()
                        * (1. - p[28] + p[28] * v.velocity)
                        * (1. + v.expression.value(NoteExpressionKind::Pressure));
                    v.last[ch] = next * (1. - v.fade) + v.fade_from[ch] * v.fade;
                    out[ch] += v.last[ch];
                }
                v.fade = (v.fade - 1. / (0.005 * self.rate)).max(0.);
            }
            let pan = (self.pan + modulation * p[55]).clamp(-1., 1.);
            let limit = f64::from(f32::MAX);
            frame[0] = (out[0] * self.gain * volume * (1. - pan.max(0.))).clamp(-limit, limit) as f32;
            frame[1] = (out[1] * self.gain * volume * (1. + pan.min(0.))).clamp(-limit, limit) as f32;
        }
    }
}

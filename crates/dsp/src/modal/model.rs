// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fitted recordings and their fusion in parameter space, rendered as Instrument revision 4.
//! Decoding allocates (off the audio thread); `ModelSet::voice` does not. See
//! `docs/spec/instrument-modal-profile.md` › Fitted model sets.

use super::Mode;
use std::f64::consts::PI;

pub const MODEL_SCHEMA_VERSION: u16 = 2;
pub const MODEL_REVISION: u16 = 4;
pub const MAX_RECORDINGS: usize = 32;
/// Sources a set can tell apart for morphing (ten pianos are ten sources).
pub const MAX_SOURCES: usize = 16;
pub const MAX_MODEL_MODES: usize = 64;
/// Attack samples per recording: 0.25 s at 48 kHz.
pub const MAX_ATTACK: usize = 12000;
pub const MAX_VOICE_SINES: usize = 128;
const CANDIDATE_SINES: usize = 4 * MAX_MODEL_MODES;
/// First bytes of an encoded set, so a loader can tell it from a schema-1 profile.
pub const MODEL_MAGIC: &[u8; 8] = b"SHMSET2\0";
const MATCH_TOLERANCE: f64 = 0.03;
const NO_MATCH: u8 = u8::MAX;
const RECORDING_HEADER: usize = 28;

/// One fitted resonance. Ratios refer to the recording key's equal-tempered frequency.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelMode {
    pub ratio: f64,
    pub magnitude: f64,
    /// T60 of the slow part.
    pub decay_seconds: f64,
    /// T60 of the fast part; 0 for none.
    pub fast_decay_seconds: f64,
    pub fast_share: f64,
    /// Frequency factor of a beating partner; 1 for none.
    pub beat_ratio: f64,
    pub beat_share: f64,
    /// Radians at the strike.
    pub phase: f64,
}

impl ModelMode {
    /// A plain decaying sine: no fast part, no partner, zero phase.
    pub const fn plain(ratio: f64, magnitude: f64, decay_seconds: f64) -> Self {
        Self {
            ratio,
            magnitude,
            decay_seconds,
            fast_decay_seconds: 0.0,
            fast_share: 0.0,
            beat_ratio: 1.0,
            beat_share: 0.0,
            phase: 0.0,
        }
    }
}

/// One fitted recording: modes, level and the attack the modes do not explain.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelRecording {
    pub key: u8,
    /// Which instrument it was recorded from, 0..15, for Source morph.
    pub source: u8,
    pub velocity: f32,
    /// Source full-scale amplitude that the normalized magnitudes and attack multiply.
    pub level: f64,
    pub modes: Vec<ModelMode>,
    /// Attack sample rate; 0 with an empty attack.
    pub attack_rate: u32,
    pub attack: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelError(pub &'static str);

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ModelError {}

impl ModelRecording {
    pub fn validate(&self) -> Result<(), ModelError> {
        let fail = |message| Err(ModelError(message));
        if self.key > 127
            || usize::from(self.source) >= MAX_SOURCES
            || !self.velocity.is_finite()
            || !(self.velocity > 0.0 && self.velocity <= 1.0)
        {
            return fail("a fitted recording needs key 0..127, source 0..15 and velocity in (0, 1]");
        }
        if !self.level.is_finite() || !(self.level > 0.0 && self.level <= 16.0) {
            return fail("a fitted recording needs a level in (0, 16]");
        }
        if self.modes.is_empty() || self.modes.len() > MAX_MODEL_MODES {
            return fail("a fitted recording needs 1..64 modes");
        }
        let mut previous = 0.0;
        let mut sum = 0.0;
        for mode in &self.modes {
            let finite = [
                mode.ratio,
                mode.magnitude,
                mode.decay_seconds,
                mode.fast_decay_seconds,
                mode.fast_share,
                mode.beat_ratio,
                mode.beat_share,
                mode.phase,
            ]
            .iter()
            .all(|v| v.is_finite());
            let fast_ok = if mode.fast_decay_seconds == 0.0 {
                mode.fast_share == 0.0
            } else {
                (0.001..=mode.decay_seconds).contains(&mode.fast_decay_seconds)
                    && (0.0..=1.0).contains(&mode.fast_share)
            };
            let beat_ok = if mode.beat_ratio == 1.0 {
                mode.beat_share == 0.0
            } else {
                (0.97..=1.03).contains(&mode.beat_ratio) && (0.0..=0.5).contains(&mode.beat_share)
            };
            if !finite
                || !(1.0 / 256.0..=256.0).contains(&mode.ratio)
                || mode.ratio <= previous
                || !(0.0..=1.0).contains(&mode.magnitude)
                || !(0.001..=60.0).contains(&mode.decay_seconds)
                || !fast_ok
                || !beat_ok
                || !(-PI..=PI).contains(&mode.phase)
            {
                return fail(
                    "fitted modes need increasing ratios, magnitudes 0..1, T60 0.001..60, a faster fast part, a partner within 3 % and phases within pi",
                );
            }
            previous = mode.ratio;
            sum += mode.magnitude;
        }
        if sum <= 0.0 || sum > 1.0 + 1e-9 {
            return fail("fitted magnitudes need a positive sum of at most 1");
        }
        let attack_ok = if self.attack.is_empty() {
            self.attack_rate == 0
        } else {
            (1000..=48000).contains(&self.attack_rate)
                && self.attack.len() <= MAX_ATTACK
                && self.attack.len() <= self.attack_rate as usize / 4
                && self.attack.iter().all(|x| x.is_finite())
        };
        if !attack_ok {
            return fail("an attack needs a rate of 1000..48000 Hz, at most 0.25 s and finite samples");
        }
        Ok(())
    }
}

/// The model controls of a note, as revision 2 applies them to a profile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Controls {
    /// Body decay; 2 is neutral.
    pub decay: f64,
    /// 0.5 is neutral.
    pub brightness: f64,
    /// 0.002 is neutral.
    pub structure: f64,
    /// 0.25 is neutral.
    pub strike: f64,
    /// Sines per voice, at most `MAX_VOICE_SINES`.
    pub sines: usize,
    /// Source morph, 0..1; 0.5 weighs every source equally.
    pub morph: f64,
    /// Velocity to brightness, 0..1: how much a strike harder or softer than the recording
    /// brightens or darkens it and changes its attack.
    pub velocity_brightness: f64,
    /// Pedal resonance while the sustain pedal is down, else 0.
    pub pedal_resonance: f64,
    /// The soft pedal is down.
    pub soft: bool,
}

impl Controls {
    pub const NEUTRAL: Self = Self {
        decay: 2.0,
        brightness: 0.5,
        structure: 0.002,
        strike: 0.25,
        sines: MAX_VOICE_SINES,
        morph: 0.5,
        velocity_brightness: 0.0,
        pedal_resonance: 0.0,
        soft: false,
    };
}

/// What a note plays: sines with phases, a level and the anchor recording's attack.
#[derive(Debug, Clone, Copy)]
pub struct Voice {
    pub modes: [Mode; MAX_VOICE_SINES],
    pub phases: [f64; MAX_VOICE_SINES],
    pub count: usize,
    pub level: f64,
    /// Recording whose attack plays, and its read step in attack samples per output sample.
    pub attack: Option<(usize, f64)>,
    /// What the attack is multiplied by: harder strikes than the recording knock more.
    pub attack_gain: f64,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            modes: [SILENT; MAX_VOICE_SINES],
            phases: [0.0; MAX_VOICE_SINES],
            count: 0,
            level: 0.0,
            attack: None,
            attack_gain: 1.0,
        }
    }
}

const SILENT: Mode = Mode { frequency_hz: 0.0, decay_seconds: 0.0, amplitude: 0.0 };

/// A validated set of fitted recordings with each mode's match in every other recording.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelSet {
    recordings: Vec<ModelRecording>,
    /// `matches[a][i][r]`: the mode of recording `r` matching mode `i` of recording `a`.
    matches: Vec<[[u8; MAX_RECORDINGS]; MAX_MODEL_MODES]>,
    /// Each recording's decay law: d ln T60 / d ln frequency over its decaying modes.
    decay_laws: Vec<f64>,
    /// Morph rank of each source present, or `None`; and how many sources are present.
    source_ranks: [Option<usize>; MAX_SOURCES],
    sources: usize,
}

impl ModelSet {
    pub fn new(mut recordings: Vec<ModelRecording>) -> Result<Self, ModelError> {
        if recordings.is_empty() || recordings.len() > MAX_RECORDINGS {
            return Err(ModelError("a model set needs 1..32 recordings"));
        }
        for recording in &recordings {
            recording.validate()?;
        }
        recordings
            .sort_by(|a, b| a.key.cmp(&b.key).then(a.velocity.total_cmp(&b.velocity)).then(a.source.cmp(&b.source)));
        if recordings
            .windows(2)
            .any(|pair| matches!(pair, [a, b] if a.key == b.key && a.velocity == b.velocity && a.source == b.source))
        {
            return Err(ModelError("two recordings of a model set share a key, velocity and source"));
        }
        let matches = recordings.iter().map(|anchor| match_modes(anchor, &recordings)).collect();
        let decay_laws = recordings.iter().map(decay_law).collect();
        let mut present = [false; MAX_SOURCES];
        for recording in &recordings {
            if let Some(flag) = present.get_mut(usize::from(recording.source)) {
                *flag = true;
            }
        }
        let mut source_ranks = [None; MAX_SOURCES];
        let mut sources = 0;
        for (rank, flag) in source_ranks.iter_mut().zip(present) {
            if flag {
                *rank = Some(sources);
                sources += 1;
            }
        }
        Ok(Self { recordings, matches, decay_laws, source_ranks, sources })
    }

    /// This set with one more recording, validated as a whole.
    pub fn with(&self, recording: ModelRecording) -> Result<Self, ModelError> {
        let mut recordings = self.recordings.clone();
        recordings.push(recording);
        Self::new(recordings)
    }

    pub fn recordings(&self) -> &[ModelRecording] {
        &self.recordings
    }

    /// The recording count an encoded set's header declares, without decoding the set; None for
    /// anything that is not a schema-2 set header.
    pub fn recording_count(bytes: &[u8]) -> Option<usize> {
        let header = bytes.get(..16)?;
        (header.get(..8) == Some(MODEL_MAGIC.as_slice())
            && header.get(8..10) == Some(MODEL_SCHEMA_VERSION.to_le_bytes().as_slice())
            && header.get(10..12) == Some(MODEL_REVISION.to_le_bytes().as_slice()))
        .then(|| usize::from(header.get(12).copied().unwrap_or(0)))
        .filter(|count| (1..=MAX_RECORDINGS).contains(count))
    }

    pub fn attack(&self, recording: usize) -> &[f32] {
        self.recordings.get(recording).map_or(&[], |r| r.attack.as_slice())
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(16 + self.recordings.len() * (RECORDING_HEADER + 2048));
        bytes.extend_from_slice(MODEL_MAGIC);
        bytes.extend_from_slice(&MODEL_SCHEMA_VERSION.to_le_bytes());
        bytes.extend_from_slice(&MODEL_REVISION.to_le_bytes());
        bytes.extend_from_slice(&[self.recordings.len() as u8, 0, 0, 0]);
        for recording in &self.recordings {
            bytes.extend_from_slice(&[recording.key, recording.source, 0, 0]);
            bytes.extend_from_slice(&recording.velocity.to_le_bytes());
            bytes.extend_from_slice(&recording.level.to_le_bytes());
            bytes.extend_from_slice(&[recording.modes.len() as u8, 0, 0, 0]);
            bytes.extend_from_slice(&recording.attack_rate.to_le_bytes());
            bytes.extend_from_slice(&(recording.attack.len() as u32).to_le_bytes());
            for mode in &recording.modes {
                for value in [
                    mode.ratio,
                    mode.magnitude,
                    mode.decay_seconds,
                    mode.fast_decay_seconds,
                    mode.fast_share,
                    mode.beat_ratio,
                    mode.beat_share,
                    mode.phase,
                ] {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
            for sample in &recording.attack {
                bytes.extend_from_slice(&sample.to_le_bytes());
            }
        }
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ModelError> {
        let header = ModelError("unsupported or malformed fitted model set");
        let mut reader = Reader { bytes, at: 0 };
        if reader.take(8)? != MODEL_MAGIC.as_slice()
            || reader.u16()? != MODEL_SCHEMA_VERSION
            || reader.u16()? != MODEL_REVISION
        {
            return Err(header);
        }
        let count = usize::from(reader.u8()?);
        if reader.take(3)? != [0, 0, 0] || !(1..=MAX_RECORDINGS).contains(&count) {
            return Err(header);
        }
        let mut recordings = Vec::with_capacity(count);
        for _ in 0..count {
            let key = reader.u8()?;
            let source = reader.u8()?;
            if reader.take(2)? != [0, 0] {
                return Err(header);
            }
            let velocity = f32::from_le_bytes(reader.array()?);
            let level = reader.f64()?;
            let modes = usize::from(reader.u8()?);
            if reader.take(3)? != [0, 0, 0] || !(1..=MAX_MODEL_MODES).contains(&modes) {
                return Err(header);
            }
            let attack_rate = u32::from_le_bytes(reader.array()?);
            let attack_len = u32::from_le_bytes(reader.array()?) as usize;
            if attack_len > MAX_ATTACK {
                return Err(header);
            }
            let mut parsed = Vec::with_capacity(modes);
            for _ in 0..modes {
                parsed.push(ModelMode {
                    ratio: reader.f64()?,
                    magnitude: reader.f64()?,
                    decay_seconds: reader.f64()?,
                    fast_decay_seconds: reader.f64()?,
                    fast_share: reader.f64()?,
                    beat_ratio: reader.f64()?,
                    beat_share: reader.f64()?,
                    phase: reader.f64()?,
                });
            }
            let mut attack = Vec::with_capacity(attack_len);
            for _ in 0..attack_len {
                attack.push(f32::from_le_bytes(reader.array()?));
            }
            recordings.push(ModelRecording { key, source, velocity, level, modes: parsed, attack_rate, attack });
        }
        if reader.at != bytes.len() {
            return Err(header);
        }
        let set = Self::new(recordings)?;
        // Canonical order only: a set that sorting would change is not this encoding.
        if set.encode() != bytes {
            return Err(header);
        }
        Ok(set)
    }

    /// The sines, level and attack of `key` at `velocity` with `controls`. Allocation-free.
    pub fn voice(&self, key: u8, velocity: f32, controls: &Controls, sample_rate: f64, out: &mut Voice) {
        *out = Voice::default();
        let morph = if controls.morph.is_finite() { controls.morph.clamp(0.0, 1.0) } else { 0.5 };
        let mut chosen = [(usize::MAX, 0.0_f64); MAX_RECORDINGS];
        for (index, recording) in self.recordings.iter().enumerate() {
            let distance = (f64::from(key) - f64::from(recording.key)).abs() / 12.0
                + 2.0 * (f64::from(velocity) - f64::from(recording.velocity)).abs();
            // Source morph tilts the weights across the sources present, in source order.
            let factor = match self.source_ranks.get(usize::from(recording.source)).copied().flatten() {
                Some(rank) if self.sources >= 2 => {
                    let x = rank as f64 / (self.sources - 1) as f64;
                    (1.0 - x) * (1.0 - morph) + x * morph
                }
                _ => 1.0,
            };
            let weight = factor / (distance * distance + 1e-6);
            if let Some(slot) = chosen.iter().position(|(_, w)| weight > *w) {
                chosen.copy_within(slot..MAX_RECORDINGS - 1, slot + 1);
                if let Some(target) = chosen.get_mut(slot) {
                    *target = (index, weight);
                }
            }
        }
        let total: f64 = chosen.iter().filter(|(i, _)| *i != usize::MAX).map(|(_, w)| w).sum();
        let Some(&(anchor, _)) = chosen.first().filter(|(i, _)| *i != usize::MAX) else { return };
        let (Some(anchor_recording), Some(anchor_matches)) = (self.recordings.get(anchor), self.matches.get(anchor))
        else {
            return;
        };
        let participants = chosen.iter().filter(|(i, _)| *i != usize::MAX);
        out.level = participants
            .clone()
            .filter_map(|(i, w)| self.recordings.get(*i).map(|r| w / total * r.level.ln()))
            .sum::<f64>()
            .exp();
        let fundamental = 440.0 * 2.0_f64.powf((f64::from(key) - 69.0) / 12.0);
        let anchor_velocity = f64::from(anchor_recording.velocity);
        let harder = f64::from(velocity) - anchor_velocity;
        let velocity_brightness =
            if controls.velocity_brightness.is_finite() { controls.velocity_brightness.clamp(0.0, 1.0) } else { 0.0 };
        let tilt = 1.5 * (controls.brightness - 0.5) + 1.5 * velocity_brightness * harder
            - if controls.soft { 0.45 } else { 0.0 };
        let resonance =
            if controls.pedal_resonance.is_finite() { controls.pedal_resonance.clamp(0.0, 1.0) } else { 0.0 };
        if controls.soft {
            out.level *= 0.7;
        }
        if velocity > 0.0 && anchor_velocity > 0.0 {
            out.attack_gain = (f64::from(velocity) / anchor_velocity).powf(velocity_brightness);
        }
        let dispersion = |s: f64, r: f64| ((1.0 + s * r * r) / (1.0 + s)).sqrt();
        let excitation = |n: usize, strike: f64| 0.25 + 0.75 * (PI * n as f64 * strike).sin().abs();
        let mut candidates = [(SILENT, 0.0_f64, 0.0_f64); CANDIDATE_SINES];
        let mut count = 0;
        for (index, anchor_mode) in anchor_recording.modes.iter().enumerate() {
            let mut fused = Fused::default();
            for &(recording, weight) in participants.clone() {
                let matched = if recording == anchor {
                    Some(index)
                } else {
                    anchor_matches
                        .get(index)
                        .and_then(|row| row.get(recording))
                        .filter(|j| **j != NO_MATCH)
                        .map(|j| usize::from(*j))
                };
                let mode = matched.and_then(|j| self.recordings.get(recording).and_then(|r| r.modes.get(j)));
                // Each recording's decay law carries its T60s to the played key.
                let decay_scale = self
                    .recordings
                    .get(recording)
                    .zip(self.decay_laws.get(recording))
                    .map_or(1.0, |(r, law)| 2.0_f64.powf((f64::from(key) - f64::from(r.key)) / 12.0 * law));
                fused.add(mode, weight / total, decay_scale);
            }
            let Some(mode) = fused.mode(anchor_mode.phase) else { continue };
            let ratio = mode.ratio * dispersion(controls.structure, mode.ratio) / dispersion(0.002, mode.ratio);
            let decay_scale = controls.decay / 2.0;
            let magnitude = mode.magnitude * mode.ratio.powf(tilt) * excitation(index + 1, controls.strike)
                / excitation(index + 1, 0.25);
            let frequency = fundamental * ratio;
            let slow = magnitude * (1.0 - mode.fast_share);
            // The soft pedal strikes fewer strings: no beating partner.
            let beat_share = if controls.soft { 0.0 } else { mode.beat_share };
            let slow_t60 = mode.decay_seconds * decay_scale;
            let parts = [
                (frequency, slow_t60, slow * (1.0 - beat_share), mode.phase),
                (frequency, mode.fast_decay_seconds * decay_scale, magnitude * mode.fast_share, mode.phase),
                (frequency * mode.beat_ratio, slow_t60, slow * beat_share, mode.phase),
                // Undamped strings under the sustain pedal ring along at this partial.
                (frequency, (2.0 * slow_t60).min(60.0), slow * resonance * 0.15, mode.phase + PI / 2.0),
            ];
            for (frequency_hz, decay_seconds, amplitude, phase) in parts {
                if amplitude > 0.0
                    && decay_seconds > 0.0
                    && frequency_hz < sample_rate * 0.45
                    && let Some(slot) = candidates.get_mut(count)
                {
                    *slot = (Mode { frequency_hz, decay_seconds, amplitude }, phase, amplitude * decay_seconds.sqrt());
                    count += 1;
                }
            }
        }
        // Keep the strongest sines, by amplitude times the square root of T60 (in place).
        let used = candidates.get_mut(..count).unwrap_or_default();
        used.sort_unstable_by(|a, b| b.2.total_cmp(&a.2));
        out.count = count.min(controls.sines.min(MAX_VOICE_SINES));
        for ((mode, phase), (candidate, candidate_phase, _)) in
            out.modes.iter_mut().zip(out.phases.iter_mut()).zip(used.iter()).take(out.count)
        {
            *mode = *candidate;
            *phase = *candidate_phase;
        }
        if !anchor_recording.attack.is_empty() && sample_rate > 0.0 {
            let transpose = 2.0_f64.powf((f64::from(key) - f64::from(anchor_recording.key)) / 12.0);
            out.attack = Some((anchor, f64::from(anchor_recording.attack_rate) / sample_rate * transpose));
        }
    }
}

/// Weighted accumulation of one anchor mode's matches.
#[derive(Default)]
struct Fused {
    weight: f64,
    matched: f64,
    ratio: f64,
    decay: f64,
    magnitude: f64,
    fast: f64,
    fast_weight: f64,
    fast_share: f64,
    beat: f64,
    beat_share: f64,
}

impl Fused {
    fn add(&mut self, mode: Option<&ModelMode>, weight: f64, decay_scale: f64) {
        self.weight += weight;
        let Some(mode) = mode else { return };
        self.matched += weight;
        self.ratio += weight * mode.ratio.ln();
        // A sustained (60 s) mode stays sustained at every key.
        let decay =
            if mode.decay_seconds >= 60.0 { 60.0 } else { (mode.decay_seconds * decay_scale).clamp(0.001, 60.0) };
        self.decay += weight * decay.ln();
        self.magnitude += weight * mode.magnitude;
        if mode.fast_decay_seconds > 0.0 {
            self.fast += weight * (mode.fast_decay_seconds * decay_scale).clamp(0.001, 60.0).ln();
            self.fast_weight += weight;
        }
        self.fast_share += weight * mode.fast_share;
        self.beat += weight * mode.beat_ratio.ln();
        self.beat_share += weight * mode.beat_share;
    }

    fn mode(&self, phase: f64) -> Option<ModelMode> {
        if self.matched <= 0.0 || self.weight <= 0.0 {
            return None;
        }
        let decay_seconds = (self.decay / self.matched).exp();
        let fast_decay_seconds =
            if self.fast_weight > 0.0 { (self.fast / self.fast_weight).exp().min(decay_seconds) } else { 0.0 };
        Some(ModelMode {
            ratio: (self.ratio / self.matched).exp(),
            magnitude: self.magnitude / self.weight,
            decay_seconds,
            fast_decay_seconds,
            fast_share: if fast_decay_seconds > 0.0 { self.fast_share / self.matched } else { 0.0 },
            beat_ratio: (self.beat / self.matched).exp(),
            beat_share: self.beat_share / self.matched,
            phase,
        })
    }
}

/// The slope of ln T60 against ln frequency over a recording's decaying modes, weighted by
/// magnitude, limited to -2..0; 0 when fewer than two modes decay.
fn decay_law(recording: &ModelRecording) -> f64 {
    let points: Vec<(f64, f64, f64)> = recording
        .modes
        .iter()
        .filter(|m| m.decay_seconds < 60.0 && m.magnitude > 0.0)
        .map(|m| (m.ratio.ln(), m.decay_seconds.ln(), m.magnitude))
        .collect();
    if points.len() < 2 {
        return 0.0;
    }
    let weight: f64 = points.iter().map(|p| p.2).sum();
    let mean_x = points.iter().map(|p| p.2 * p.0).sum::<f64>() / weight;
    let mean_y = points.iter().map(|p| p.2 * p.1).sum::<f64>() / weight;
    let variance: f64 = points.iter().map(|p| p.2 * (p.0 - mean_x).powi(2)).sum();
    if variance <= 1e-12 {
        return 0.0;
    }
    let slope = points.iter().map(|p| p.2 * (p.0 - mean_x) * (p.1 - mean_y)).sum::<f64>() / variance;
    if slope.is_finite() { slope.clamp(-2.0, 0.0) } else { 0.0 }
}

/// For each mode of `anchor`, strongest first, the nearest unused mode of every recording
/// within the tolerance.
fn match_modes(anchor: &ModelRecording, recordings: &[ModelRecording]) -> [[u8; MAX_RECORDINGS]; MAX_MODEL_MODES] {
    let mut table = [[NO_MATCH; MAX_RECORDINGS]; MAX_MODEL_MODES];
    let mut order: Vec<usize> = (0..anchor.modes.len()).collect();
    order.sort_by(|a, b| {
        let magnitude = |i: &usize| anchor.modes.get(*i).map_or(0.0, |m| m.magnitude);
        magnitude(b).total_cmp(&magnitude(a)).then(a.cmp(b))
    });
    for (column, other) in recordings.iter().enumerate() {
        let mut used = [false; MAX_MODEL_MODES];
        for &index in &order {
            let Some(mode) = anchor.modes.get(index) else { continue };
            let best = other
                .modes
                .iter()
                .enumerate()
                .filter(|(j, _)| !used.get(*j).copied().unwrap_or(true))
                .map(|(j, m)| (j, (m.ratio / mode.ratio).ln().abs()))
                .filter(|(_, distance)| *distance <= MATCH_TOLERANCE)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((j, _)) = best
                && let (Some(cell), Some(flag)) =
                    (table.get_mut(index).and_then(|row| row.get_mut(column)), used.get_mut(j))
            {
                *cell = j as u8;
                *flag = true;
            }
        }
    }
    table
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], ModelError> {
        let end = self.at.checked_add(count).ok_or(ModelError("truncated fitted model set"))?;
        let slice = self.bytes.get(self.at..end).ok_or(ModelError("truncated fitted model set"))?;
        self.at = end;
        Ok(slice)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], ModelError> {
        self.take(N)?.try_into().map_err(|_| ModelError("truncated fitted model set"))
    }
    fn u8(&mut self) -> Result<u8, ModelError> {
        Ok(self.array::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16, ModelError> {
        Ok(u16::from_le_bytes(self.array()?))
    }
    fn f64(&mut self) -> Result<f64, ModelError> {
        Ok(f64::from_le_bytes(self.array()?))
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;

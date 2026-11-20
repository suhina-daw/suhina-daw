// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Public native Instrument runtime, shared by stock and export adapters. The struck-modal
//! model is experimental revision 1; accepted modal profiles use revision 2; fitted model sets
//! (recordings fused in parameter space, with attacks) use revision 4. Sample playback uses the
//! existing SDK runtime.

use std::sync::Arc;
use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, NoteEvent, ParamBank, ParamInfo, ProcessContext, Role,
    SampleSource, Scale, Unit, check_sample_rate,
};
use suhina_device_sdk::sample_instrument::{self, Sampler};
use suhina_dsp::{
    Smoother,
    envelope::{LinearEnvelope, LinearEnvelopeSettings},
    modal::{
        ModalBank, Mode,
        model::{
            Controls, MAX_VOICE_SINES, MODEL_MAGIC, MODEL_REVISION as SET_MODEL_REVISION, ModelSet, Voice as SetVoice,
        },
        profile::{ModalProfile, PROFILE_MODEL_REVISION},
        sustain::Performer,
    },
};

pub mod panel;

/// Model revision retained in project parameter state. Unknown revisions never run revision 1.
pub const MODEL_REVISION: u16 = 1;
pub const MAX_VOICES: usize = 16;
pub const MAX_MODES: usize = 16;

pub mod param {
    pub const SOURCE: usize = 0;
    pub const REVISION: usize = 1;
    pub const QUALITY: usize = 2;
    pub const DECAY: usize = 3;
    pub const BRIGHTNESS: usize = 4;
    pub const STRUCTURE: usize = 5;
    pub const STRIKE: usize = 6;
    pub const RELEASE: usize = 7;
    pub const BLEND: usize = 8;
    pub const GAIN: usize = 9;
    pub const ROOT: usize = 10;
    pub const KEY_LOW: usize = 11;
    pub const KEY_HIGH: usize = 12;
    pub const START: usize = 13;
    pub const END: usize = 14;
    pub const LOOP_START: usize = 15;
    pub const LOOP_END: usize = 16;
    pub const PLAYBACK: usize = 17;
    pub const ATTACK: usize = 18;
    pub const SUSTAIN: usize = 19;
    pub const VELOCITY_LOW: usize = 20;
    pub const VELOCITY_HIGH: usize = 21;
    /// Amplitude envelope decay, shared by the Sample layer and fitted models.
    pub const ENVELOPE_DECAY: usize = 22;
    pub const MORPH: usize = 23;
    pub const VELOCITY_BRIGHTNESS: usize = 24;
    pub const DAMPER_NOISE: usize = 25;
    pub const PEDAL_RESONANCE: usize = 26;
    pub const SOFT_PEDAL: usize = 27;
    pub const COUNT: usize = 28;
}

const fn control(id: &'static str, name: &'static str, range: (f64, f64), default: f64, unit: Unit) -> ParamInfo {
    ParamInfo::continuous(id, name, range, default, unit, Scale::Linear)
}

pub static PARAMS: [ParamInfo; param::COUNT] = [
    ParamInfo::choice("source", "Source", &["Modal", "Sample", "Hybrid"], 0),
    ParamInfo::integer("model_revision", "Experimental model revision", (1, 65535), 1, Unit::None),
    ParamInfo::choice("quality", "Modes per voice", &["8", "16"], 1),
    ParamInfo::continuous("body_decay", "Body decay", (0.05, 20.0), 2.0, Unit::Seconds, Scale::Logarithmic),
    control("brightness", "Brightness", (0.0, 1.0), 0.5, Unit::Percent),
    control("structure", "Structure", (0.0, 0.05), 0.002, Unit::None),
    control("strike", "Strike position", (0.02, 0.98), 0.25, Unit::Percent),
    control("release", "Release", (0.005, 10.0), 0.25, Unit::Seconds),
    control("blend", "Hybrid sample blend", (0.0, 1.0), 0.5, Unit::Percent),
    control("gain", "Gain", (-60.0, 12.0), -6.0, Unit::Decibels),
    ParamInfo::integer("root", "Sample root key", (0, 127), 60, Unit::Key),
    ParamInfo::integer("key_low", "Lowest key", (0, 127), 0, Unit::Key),
    ParamInfo::integer("key_high", "Highest key", (0, 127), 127, Unit::Key),
    control("start", "Sample start", (0.0, 1.0), 0.0, Unit::Percent),
    control("end", "Sample end", (0.0, 1.0), 1.0, Unit::Percent),
    control("loop_start", "Loop start", (0.0, 1.0), 0.0, Unit::Percent),
    control("loop_end", "Loop end", (0.0, 1.0), 1.0, Unit::Percent),
    ParamInfo::choice("playback", "Sample playback", &["Gate", "One shot", "Loop"], 0),
    control("attack", "Attack", (0.0, 10.0), 0.005, Unit::Seconds),
    control("sustain", "Sustain", (0.0, 1.0), 1.0, Unit::Percent),
    control("velocity_low", "Lowest velocity", (0.0, 1.0), 0.0, Unit::Percent),
    control("velocity_high", "Highest velocity", (0.0, 1.0), 1.0, Unit::Percent),
    control("decay", "Decay", (0.0, 10.0), 0.2, Unit::Seconds),
    control("morph", "Source morph", (0.0, 1.0), 0.5, Unit::Percent),
    control("velocity_brightness", "Velocity to brightness", (0.0, 1.0), 0.35, Unit::Percent),
    control("damper_noise", "Damper noise", (0.0, 1.0), 0.2, Unit::Percent),
    control("pedal_resonance", "Pedal resonance", (0.0, 1.0), 0.3, Unit::Percent),
    ParamInfo::toggle("soft_pedal", "Soft pedal", false),
];

pub static INFO: DeviceInfo = DeviceInfo::new("instrument", "Instrument (experimental)", Role::Instrument, &PARAMS);

#[derive(Clone, Copy)]
struct Voice {
    bank: ModalBank<MAX_MODES>,
    /// Revision 4: fused fitted sines with their phases.
    fitted: ModalBank<MAX_VOICE_SINES>,
    /// Revision 4: (recording, read position, step) of the attack still playing.
    attack: Option<(usize, f64, f64)>,
    attack_gain: f32,
    /// Revision 4: the held layer (vibrato, per-partial onsets, noise) playing on `fitted`.
    layer: Performer,
    /// Revision 4: the amplitude envelope's attack, decay and sustain; its release is the damper.
    envelope: LinearEnvelope,
    fitted_voice: bool,
    damped: bool,
    damper: Option<Damper>,
    key: u8,
    held: bool,
    serial: u64,
    amplitude: f32,
    gate: Smoother,
    last_output: f32,
    stolen_tail: f32,
    stolen_fade: Smoother,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            bank: ModalBank::default(),
            fitted: ModalBank::default(),
            attack: None,
            attack_gain: 1.0,
            layer: Performer::default(),
            envelope: LinearEnvelope::default(),
            fitted_voice: false,
            damped: false,
            damper: None,
            key: 0,
            held: false,
            serial: 0,
            amplitude: 0.0,
            gate: Smoother::new(0.0, 1),
            last_output: 0.0,
            stolen_tail: 0.0,
            stolen_fade: Smoother::new(0.0, 1),
        }
    }
}

/// The noise of a damper landing: 40 ms of low-passed noise fading out.
#[derive(Clone, Copy)]
struct Damper {
    remaining: u32,
    total: u32,
    level: f32,
    lowpass: f32,
    seed: u32,
}

impl Voice {
    /// Whether any modal layer of this voice still sounds.
    fn sounding(&self) -> bool {
        self.bank.is_active()
            || self.fitted.is_active()
            || self.attack.is_some()
            || self.damper.is_some()
            || self.layer.sounding()
    }

    /// A revision-4 damper lands once: every sine's T60 is capped at `release`, and its noise
    /// starts at `noise * 0.05` of the voice's level now.
    fn damp(&mut self, sample_rate: f64, release: f64, noise: f64) {
        if self.damped {
            return;
        }
        self.damped = true;
        // A held recording's measured release, scaled by Release against its default, caps the
        // sines instead; its noise layer fades with the same T60.
        let cap = self.layer.release(release);
        self.fitted.cap_decay(sample_rate, cap);
        let level = (f64::from(self.amplitude) * self.envelope.value() * noise * 0.05).abs() as f32;
        let total = (0.04 * sample_rate) as u32;
        if level > 0.0 && total > 0 {
            self.damper =
                Some(Damper { remaining: total, total, level, lowpass: 0.0, seed: 0x9e37_79b9 ^ (self.serial as u32) });
        }
    }
}

/// The complete public core. Construction, preparation and sample loading run off the audio
/// thread. Device rendering/events/reset use fixed storage and delegate sample voices to the SDK.
pub struct Instrument {
    sample_rate: f64,
    params: ParamBank<{ param::COUNT }>,
    sampler: Sampler,
    sample_silent: bool,
    voices: [Voice; MAX_VOICES],
    serial: u64,
    pedal: bool,
    bend_semitones: f64,
    gain: Smoother,
    sample_mix: Smoother,
    modal_mix: Smoother,
    profile: Option<ModalProfile>,
    /// The soft pedal (controller 67) is down.
    soft_pedal: bool,
    /// Revision 4. Prepared off the audio thread; the callback only reads it.
    set: Option<Arc<ModelSet>>,
    /// Scratch for a revision-4 note-on: no allocation in the callback.
    set_voice: SetVoice,
    invalid_profile: bool,
}

impl Default for Instrument {
    fn default() -> Self {
        Self::new()
    }
}

impl Instrument {
    pub fn new() -> Self {
        Self::at_rate(DEFAULT_SAMPLE_RATE, Sampler::new())
    }

    pub fn with_rate(sample_rate: f64) -> Result<Self, DeviceError> {
        let sample_rate = check_sample_rate(sample_rate)?;
        Ok(Self::at_rate(sample_rate, Sampler::with_rate(sample_rate)?))
    }

    fn at_rate(sample_rate: f64, sampler: Sampler) -> Self {
        let ramp = Smoother::ramp_frames(0.01, sample_rate);
        let mut core = Self {
            sample_rate,
            params: ParamBank::new(&PARAMS),
            sampler,
            sample_silent: true,
            voices: [Voice::default(); MAX_VOICES],
            serial: 0,
            pedal: false,
            bend_semitones: 0.0,
            gain: Smoother::new(0.0, ramp),
            sample_mix: Smoother::new(0.0, ramp),
            modal_mix: Smoother::new(1.0, ramp),
            profile: None,
            soft_pedal: false,
            set: None,
            set_voice: SetVoice::default(),
            invalid_profile: false,
        };
        core.set_sample_parameter("poly", MAX_VOICES as f64);
        for (index, info) in PARAMS.iter().enumerate() {
            core.set_param(index, info.default);
        }
        core.reset();
        core
    }

    fn set_sample_parameter(&mut self, id: &str, value: f64) {
        if let Some(index) = sample_instrument::PARAMS.iter().position(|info| info.id == id) {
            self.sampler.set_param(index, value);
        }
    }

    fn mix_targets(&self) -> (f32, f32) {
        match self.params.get(param::SOURCE) as u8 {
            1 => (1.0, 0.0),
            2 => {
                let sample = self.params.get(param::BLEND) as f32;
                (sample, 1.0 - sample)
            }
            _ => (0.0, 1.0),
        }
    }

    /// Whether the persisted model revision is supported. Unsupported modal bodies remain
    /// silent instead of silently changing their sound; sample playback remains available.
    pub fn model_supported(&self) -> bool {
        if self.invalid_profile {
            return false;
        }
        match self.params.get(param::REVISION) as u16 {
            MODEL_REVISION => self.profile.is_none() && self.set.is_none(),
            PROFILE_MODEL_REVISION => self.profile.is_some(),
            SET_MODEL_REVISION => self.set.is_some(),
            _ => false,
        }
    }

    /// Adopt validated modes off the audio thread. Preparation and reset retain this profile.
    pub fn set_modal_profile(&mut self, profile: ModalProfile) {
        self.profile = Some(profile);
        self.set = None;
        self.invalid_profile = false;
        let _ = self.params.set(param::REVISION, f64::from(PROFILE_MODEL_REVISION));
        self.reset();
    }

    pub fn modal_profile(&self) -> Option<&ModalProfile> {
        self.profile.as_ref()
    }

    /// Adopt a fitted model set off the audio thread (revision 4); any profile is cleared.
    pub fn set_model_set(&mut self, set: ModelSet) {
        self.set = Some(Arc::new(set));
        self.profile = None;
        self.invalid_profile = false;
        let _ = self.params.set(param::REVISION, f64::from(SET_MODEL_REVISION));
        self.reset();
    }

    pub fn model_set(&self) -> Option<&ModelSet> {
        self.set.as_deref()
    }

    fn release(&mut self, key: Option<u8>) {
        let frames = Smoother::ramp_frames(self.params.get(param::RELEASE), self.sample_rate);
        let (release, noise) = (self.params.get(param::RELEASE), self.params.get(param::DAMPER_NOISE));
        for voice in &mut self.voices {
            if key.is_none_or(|key| key == voice.key) {
                voice.held = false;
                if !self.pedal {
                    if voice.fitted_voice {
                        voice.damp(self.sample_rate, release, noise);
                    } else {
                        voice.gate.set_ramp(frames);
                        voice.gate.set_target(0.0);
                    }
                }
            }
        }
    }

    fn strike(&mut self, key: u8, velocity: f32) {
        if !self.model_supported() {
            return;
        }
        // Revision 4: the set fuses its recordings for this key and velocity.
        let fitted = if let Some(set) = self.set.as_deref() {
            let controls = Controls {
                decay: self.params.get(param::DECAY),
                brightness: self.params.get(param::BRIGHTNESS),
                structure: self.params.get(param::STRUCTURE),
                strike: self.params.get(param::STRIKE),
                sines: if self.params.get(param::QUALITY) < 0.5 { MAX_VOICE_SINES / 2 } else { MAX_VOICE_SINES },
                morph: self.params.get(param::MORPH),
                velocity_brightness: self.params.get(param::VELOCITY_BRIGHTNESS),
                pedal_resonance: if self.pedal { self.params.get(param::PEDAL_RESONANCE) } else { 0.0 },
                // Controller 67 or the switch: either holds the soft pedal down.
                soft: self.soft_pedal || self.params.get(param::SOFT_PEDAL) >= 0.5,
            };
            set.voice(key, velocity, &controls, self.sample_rate, &mut self.set_voice);
            true
        } else {
            false
        };
        let mut modes = [Mode { frequency_hz: 0.0, decay_seconds: 0.0, amplitude: 0.0 }; MAX_MODES];
        let fundamental = 440.0 * 2.0_f64.powf((f64::from(key) - 69.0) / 12.0);
        let structure = self.params.get(param::STRUCTURE);
        let count = if self.params.get(param::QUALITY) < 0.5 { 8 } else { MAX_MODES };
        if self.profile.is_none() {
            for (index, mode) in modes.iter_mut().take(count).enumerate() {
                let harmonic = (index + 1) as f64;
                mode.frequency_hz =
                    fundamental * harmonic * ((1.0 + structure * harmonic * harmonic) / (1.0 + structure)).sqrt();
                mode.decay_seconds = self.params.get(param::DECAY) / (1.0 + 0.12 * (harmonic - 1.0));
                mode.amplitude = (std::f64::consts::PI * harmonic * self.params.get(param::STRIKE)).sin()
                    / harmonic.powf(2.0 - 1.5 * self.params.get(param::BRIGHTNESS));
            }
        }
        if let Some(profile) = self.profile.as_ref() {
            modes.fill(Mode { frequency_hz: 0.0, decay_seconds: 0.0, amplitude: 0.0 });
            let reference =
                profile.reference_hz() * 2.0_f64.powf((f64::from(key) - f64::from(profile.reference_key())) / 12.0);
            let mut magnitude_sum = 0.0;
            for (index, (mode, fitted)) in modes.iter_mut().take(count).zip(profile.modes()).enumerate() {
                let ratio = fitted.ratio;
                let dispersion = |s: f64| ((1.0 + s * ratio * ratio) / (1.0 + s)).sqrt();
                let excitation =
                    |strike: f64| 0.25 + 0.75 * (std::f64::consts::PI * (index + 1) as f64 * strike).sin().abs();
                mode.frequency_hz = reference * ratio * dispersion(structure) / dispersion(0.002);
                mode.decay_seconds = fitted.decay_seconds * self.params.get(param::DECAY) / 2.0;
                mode.amplitude = fitted.magnitude
                    * ratio.powf(1.5 * (self.params.get(param::BRIGHTNESS) - 0.5))
                    * excitation(self.params.get(param::STRIKE))
                    / excitation(0.25);
                magnitude_sum += mode.amplitude;
            }
            for mode in &mut modes {
                mode.amplitude /= magnitude_sum.max(1.0);
            }
        }
        let slot =
            self.voices.iter().position(|voice| !voice.sounding()).or_else(|| {
                self.voices.iter().enumerate().min_by_key(|(_, voice)| voice.serial).map(|(index, _)| index)
            });
        self.serial = self.serial.saturating_add(1);
        if let Some(voice) = slot.and_then(|index| self.voices.get_mut(index)) {
            voice.stolen_tail = voice.last_output;
            voice.stolen_fade = Smoother::new(
                if voice.last_output == 0.0 { 0.0 } else { 1.0 },
                Smoother::ramp_frames(0.002, self.sample_rate),
            );
            voice.stolen_fade.set_target(0.0);
            if fitted {
                let count = self.set_voice.count;
                voice.bank.reset();
                voice.fitted.strike_phased(
                    self.sample_rate,
                    self.set_voice.modes.get(..count).unwrap_or(&[]),
                    self.set_voice.phases.get(..count).unwrap_or(&[]),
                );
                voice.fitted.transpose(self.sample_rate, self.bend_semitones);
                voice.attack = self.set_voice.attack.map(|(recording, step)| (recording, 0.0, step));
                voice.attack_gain = self.set_voice.attack_gain as f32;
                // Seeded by the note's serial and key, so a render repeats exactly.
                let seed = (self.serial as u32).wrapping_mul(0x0100_0193) ^ u32::from(key);
                voice.layer.start(&self.set_voice.held, self.sample_rate, self.bend_semitones, seed);
                voice.envelope.start();
                voice.fitted_voice = true;
                voice.damped = false;
                voice.damper = None;
                voice.amplitude = (f64::from(velocity) * self.set_voice.level) as f32;
                // The attack reproduces the recorded onset; only a click guard remains.
                voice.gate = Smoother::new(0.0, Smoother::ramp_frames(0.00025, self.sample_rate));
            } else {
                voice.fitted.reset();
                voice.attack = None;
                voice.layer = Performer::default();
                voice.fitted_voice = false;
                voice.damper = None;
                voice.bank.strike(self.sample_rate, &modes);
                voice.bank.transpose(self.sample_rate, self.bend_semitones);
                voice.amplitude = velocity * 0.25;
                voice.gate = Smoother::new(0.0, Smoother::ramp_frames(0.002, self.sample_rate));
            }
            voice.key = key;
            voice.held = true;
            voice.serial = self.serial;
            voice.gate.set_target(1.0);
        }
    }
}

impl Device for Instrument {
    fn info(&self) -> &DeviceInfo {
        &INFO
    }
    fn load_model_state(&mut self, state: &[u8]) -> Result<(), DeviceError> {
        let loaded = if state.starts_with(MODEL_MAGIC) {
            ModelSet::decode(state).map(|set| self.set_model_set(set)).map_err(|error| error.0)
        } else {
            ModalProfile::decode(state).map(|profile| self.set_modal_profile(profile)).map_err(|error| error.0)
        };
        loaded.map_err(|error| {
            self.profile = None;
            self.set = None;
            self.invalid_profile = true;
            self.reset();
            DeviceError::InvalidModelState(error)
        })
    }
    fn samples(&mut self) -> Option<&mut dyn SampleSource> {
        Some(&mut self.sampler)
    }
    fn samples_ref(&self) -> Option<&dyn SampleSource> {
        Some(&self.sampler)
    }
    fn set_context(&mut self, context: &ProcessContext) {
        self.sampler.set_context(context);
    }

    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        self.sample_rate = check_sample_rate(sample_rate)?;
        self.sampler.prepare(sample_rate)?;
        let ramp = Smoother::ramp_frames(0.01, sample_rate);
        self.gain.set_ramp(ramp);
        self.sample_mix.set_ramp(ramp);
        self.modal_mix.set_ramp(ramp);
        self.reset();
        Ok(())
    }

    fn reset(&mut self) {
        self.voices.fill(Voice::default());
        self.serial = 0;
        self.pedal = false;
        self.bend_semitones = 0.0;
        self.sampler.reset();
        self.sample_silent = true;
        let (sample, modal) = self.mix_targets();
        self.sample_mix.reset(sample);
        self.modal_mix.reset(modal);
        self.gain.reset(suhina_dsp::gain::db_to_gain(self.params.get(param::GAIN) as f32));
    }

    fn set_param(&mut self, index: usize, value: f64) {
        let Some(value) = self.params.set(index, value) else {
            return;
        };
        if index == param::GAIN {
            self.gain.set_target(suhina_dsp::gain::db_to_gain(value as f32));
        }
        if matches!(index, param::SOURCE | param::BLEND) {
            let (sample, modal) = self.mix_targets();
            self.sample_mix.set_target(sample);
            self.modal_mix.set_target(modal);
        }
        // Native sample controls keep the SDK's names/semantics; global gain is applied once.
        if matches!(index, param::RELEASE | param::ROOT..=param::VELOCITY_HIGH | param::ENVELOPE_DECAY)
            && let Some(info) = PARAMS.get(index)
        {
            self.set_sample_parameter(&info.id, value);
        }
        if index == param::REVISION && !self.model_supported() {
            self.voices.fill(Voice::default());
        }
    }

    fn param(&self, index: usize) -> Option<f64> {
        self.params.param(index)
    }

    fn process(&mut self, context: &ProcessContext, buffer: &mut [f32]) {
        // Hybrid is a live blend: both layers advance even at an endpoint, so opening the
        // blend reveals a held note at its current phase, never a frozen or restarted source.
        let hybrid = self.params.get(param::SOURCE) == 2.0;
        if hybrid || self.sample_mix.current() > 0.0 || self.sample_mix.target() > 0.0 {
            self.sampler.process(context, buffer);
            self.sample_silent = false;
        } else {
            for frame in buffer.as_chunks_mut::<2>().0 {
                frame.fill(0.0);
            }
        }
        let bend_ratio = 2.0_f64.powf(self.bend_semitones / 12.0);
        let set = self.set.as_deref();
        let envelope = LinearEnvelopeSettings {
            attack: self.params.get(param::ATTACK),
            decay: self.params.get(param::ENVELOPE_DECAY),
            sustain: self.params.get(param::SUSTAIN),
            release: 0.0,
        };
        let damper_lowpass = (1.0 - (-std::f64::consts::TAU * 600.0 / self.sample_rate).exp()) as f32;
        for frame in buffer.as_chunks_mut::<2>().0 {
            let mut modal = 0.0;
            for voice in &mut self.voices {
                if !voice.sounding() && voice.stolen_fade.current() == 0.0 {
                    continue;
                }
                let gate = voice.gate.next_value();
                if gate == 0.0 && voice.gate.is_settled() {
                    voice.bank.reset();
                    voice.fitted.reset();
                    voice.attack = None;
                    voice.damper = None;
                    voice.layer = Performer::default();
                }
                let shape =
                    if voice.fitted_voice { voice.envelope.next(&envelope, self.sample_rate) as f32 } else { 1.0 };
                // Without a held layer the performer plays the fitted bank unchanged.
                let mut body = voice.bank.next_sample() + voice.layer.next(&mut voice.fitted);
                let mut attack_done = false;
                if let Some((recording, position, step)) = voice.attack.as_mut() {
                    let attack = set.map_or(&[][..], |set| set.attack(*recording));
                    let index = *position as usize;
                    if let (Some(a), Some(b)) = (attack.get(index), attack.get(index + 1)) {
                        body += (a + (b - a) * (*position - index as f64) as f32) * voice.attack_gain;
                        *position += *step * bend_ratio;
                    } else {
                        attack_done = true;
                    }
                }
                if attack_done {
                    voice.attack = None;
                }
                let mut noise = 0.0;
                let mut damper_done = false;
                if let Some(damper) = voice.damper.as_mut() {
                    damper.seed = damper.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let white = (damper.seed >> 8) as f32 / 8_388_608.0 - 1.0;
                    damper.lowpass += damper_lowpass * (white - damper.lowpass);
                    noise = damper.lowpass * damper.level * damper.remaining as f32 / damper.total as f32;
                    damper.remaining = damper.remaining.saturating_sub(1);
                    damper_done = damper.remaining == 0;
                }
                if damper_done {
                    voice.damper = None;
                }
                voice.last_output = (body * shape * voice.amplitude + noise) * gate
                    + voice.stolen_tail * voice.stolen_fade.next_value();
                modal += voice.last_output;
            }
            let sample_mix = self.sample_mix.next_value();
            let modal = modal * self.modal_mix.next_value();
            let gain = self.gain.next_value();
            for sample in frame {
                // Imported float audio may already reach the finite f32 limit. Preserve normal
                // mixing exactly, but saturate an overflow from the final positive gain.
                *sample = ((*sample * sample_mix + modal) * gain).clamp(-f32::MAX, f32::MAX);
            }
            if !hybrid && self.modal_mix.current() == 0.0 && self.modal_mix.is_settled() {
                for voice in &mut self.voices {
                    if voice.sounding() || voice.stolen_fade.current() != 0.0 {
                        *voice = Voice::default();
                    }
                }
            }
        }
        // Once its fade is inaudible, retire the source instead of freezing a hidden playhead
        // that could reappear on a later Source switch. The callback reset never frees storage.
        if !hybrid && !self.sample_silent && self.sample_mix.current() == 0.0 && self.sample_mix.is_settled() {
            self.sampler.reset();
            self.sampler.event(NoteEvent::PitchBend((self.bend_semitones / 2.0) as f32));
            self.sampler.event(NoteEvent::Controller { number: 64, value: if self.pedal { 1.0 } else { 0.0 } });
            self.sample_silent = true;
        }
    }

    fn event(&mut self, event: NoteEvent) {
        let event = match event {
            NoteEvent::On { key, velocity } if velocity.is_finite() && velocity <= 0.0 => {
                NoteEvent::Off { key, velocity: 0.0 }
            }
            NoteEvent::On { key, velocity } if velocity.is_finite() => {
                NoteEvent::On { key, velocity: velocity.min(1.0) }
            }
            NoteEvent::Off { velocity, .. } if !velocity.is_finite() => return,
            _ => event,
        };
        // A mode change applies to the next played note. Off/pedal messages still release
        // existing voices during the crossfade, even if their source is now inaudible.
        if self.params.get(param::SOURCE) != 0.0 || !matches!(event, NoteEvent::On { .. }) {
            self.sampler.event(event);
        }
        match event {
            NoteEvent::On { key, velocity } if key <= 127 && velocity.is_finite() => {
                if velocity <= 0.0 {
                    self.release(Some(key));
                } else if self.params.get(param::SOURCE) != 1.0
                    && f64::from(key) >= self.params.get(param::KEY_LOW)
                    && f64::from(key) <= self.params.get(param::KEY_HIGH)
                    && f64::from(velocity) >= self.params.get(param::VELOCITY_LOW)
                    && f64::from(velocity.min(1.0)) <= self.params.get(param::VELOCITY_HIGH)
                {
                    self.strike(key, velocity.clamp(0.0, 1.0));
                }
            }
            NoteEvent::Off { key, .. } if key <= 127 => self.release(Some(key)),
            NoteEvent::Controller { number: 64, value } if value.is_finite() => {
                let old = self.pedal;
                self.pedal = value >= 0.5;
                if old && !self.pedal {
                    let frames = Smoother::ramp_frames(self.params.get(param::RELEASE), self.sample_rate);
                    let (release, noise) = (self.params.get(param::RELEASE), self.params.get(param::DAMPER_NOISE));
                    for voice in &mut self.voices {
                        if !voice.held {
                            if voice.fitted_voice {
                                voice.damp(self.sample_rate, release, noise);
                            } else {
                                voice.gate.set_ramp(frames);
                                voice.gate.set_target(0.0);
                            }
                        }
                    }
                }
            }
            NoteEvent::Controller { number: 67, value } if value.is_finite() => {
                // The soft pedal changes the notes struck while it is down.
                self.soft_pedal = value >= 0.5;
            }
            NoteEvent::AllNotesOff => {
                self.pedal = false;
                self.release(None);
            }
            NoteEvent::PitchBend(value) if value.is_finite() => {
                self.bend_semitones = f64::from(value.clamp(-1.0, 1.0)) * 2.0;
                for voice in &mut self.voices {
                    if voice.bank.is_active() {
                        voice.bank.transpose(self.sample_rate, self.bend_semitones);
                    }
                    if voice.fitted.is_active() {
                        if voice.layer.is_active() {
                            voice.layer.bend(&mut voice.fitted, self.bend_semitones);
                        } else {
                            voice.fitted.transpose(self.sample_rate, self.bend_semitones);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod profile_tests;

#[cfg(test)]
mod set_tests;

// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Public native Instrument runtime, shared by stock and export adapters. The struck-modal
//! model is experimental revision 1; accepted modal profiles use revision 2; revision 3 keeps
//! revision 1's struck body and adds two struck-bar bodies. Sample playback uses the existing
//! SDK runtime.

use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, NoteEvent, ParamBank, ParamInfo, ProcessContext, Role,
    SampleSource, Scale, Unit, check_sample_rate,
};
use suhina_device_sdk::sample_instrument::{self, Sampler};
use suhina_dsp::{
    Smoother,
    bar::{Excitation, StruckBar},
    modal::{
        ModalBank, Mode,
        profile::{ModalProfile, PROFILE_MODEL_REVISION},
    },
};

pub mod bars;
pub mod panel;

use bars::Profile;

/// Model revision a new Instrument stores in project parameter state. Revision 1 is read as
/// revision 3, whose Struck body is revision 1's renderer; revision 2 renders an accepted
/// profile and ignores Body; unknown revisions stay silent.
pub const MODEL_REVISION: u16 = 3;
/// The original struck-modal revision, still accepted and read as [`MODEL_REVISION`].
pub const ORIGINAL_MODEL_REVISION: u16 = 1;
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
    pub const BODY: usize = 22;
    pub const HARDNESS: usize = 23;
    pub const MALLET_MODEL: usize = 24;
    pub const COUNT: usize = 25;
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
    ParamInfo::integer("root", "Sample root key", (0, 127), 60, Unit::None),
    ParamInfo::integer("key_low", "Lowest key", (0, 127), 0, Unit::None),
    ParamInfo::integer("key_high", "Highest key", (0, 127), 127, Unit::None),
    control("start", "Sample start", (0.0, 1.0), 0.0, Unit::Percent),
    control("end", "Sample end", (0.0, 1.0), 1.0, Unit::Percent),
    control("loop_start", "Loop start", (0.0, 1.0), 0.0, Unit::Percent),
    control("loop_end", "Loop end", (0.0, 1.0), 1.0, Unit::Percent),
    ParamInfo::choice("playback", "Sample playback", &["Gate", "One shot", "Loop"], 0),
    control("attack", "Sample attack", (0.0, 10.0), 0.005, Unit::Seconds),
    control("sustain", "Sample sustain", (0.0, 1.0), 1.0, Unit::Percent),
    control("velocity_low", "Lowest velocity", (0.0, 1.0), 0.0, Unit::Percent),
    control("velocity_high", "Highest velocity", (0.0, 1.0), 1.0, Unit::Percent),
    ParamInfo::choice("body", "Body", &["Struck", "Marimba bar", "Vibraphone bar"], 0),
    control("mallet_hardness", "Mallet hardness", (0.0, 1.0), 0.5, Unit::Percent),
    ParamInfo::choice("mallet_model", "Mallet model", &["Contact", "Pulse"], 0),
];

pub static INFO: DeviceInfo = DeviceInfo::new("instrument", "Instrument (experimental)", Role::Instrument, &PARAMS);

/// What a voice sounds: revision 1's struck bank, or a struck bar.
#[derive(Clone, Copy)]
enum Body {
    Struck(ModalBank<MAX_MODES>),
    Bar(StruckBar<MAX_MODES>),
}

impl Body {
    fn is_active(&self) -> bool {
        match self {
            Self::Struck(bank) => bank.is_active(),
            Self::Bar(bar) => bar.is_active(),
        }
    }

    fn next_sample(&mut self) -> f32 {
        match self {
            Self::Struck(bank) => bank.next_sample(),
            Self::Bar(bar) => bar.next_sample(),
        }
    }

    fn reset(&mut self) {
        match self {
            Self::Struck(bank) => bank.reset(),
            Self::Bar(bar) => bar.reset(),
        }
    }

    fn transpose(&mut self, sample_rate: f64, semitones: f64) {
        match self {
            Self::Struck(bank) => bank.transpose(sample_rate, semitones),
            Self::Bar(bar) => bar.transpose(semitones),
        }
    }
}

#[derive(Clone, Copy)]
struct Voice {
    body: Body,
    /// The bar profile this voice sounds; `None` for Struck.
    profile: Option<Profile>,
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
            body: Body::Struck(ModalBank::default()),
            profile: None,
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
            ORIGINAL_MODEL_REVISION | MODEL_REVISION => self.profile.is_none(),
            PROFILE_MODEL_REVISION => self.profile.is_some(),
            _ => false,
        }
    }

    /// Adopt validated modes off the audio thread. Preparation and reset retain this profile.
    pub fn set_modal_profile(&mut self, profile: ModalProfile) {
        self.profile = Some(profile);
        self.invalid_profile = false;
        let _ = self.params.set(param::REVISION, f64::from(PROFILE_MODEL_REVISION));
        self.reset();
    }

    pub fn modal_profile(&self) -> Option<&ModalProfile> {
        self.profile.as_ref()
    }

    /// Release `key` (or every key): a struck voice fades over Release; a bar that damps on
    /// release, or every bar when `all`, is damped to Release.
    fn release(&mut self, key: Option<u8>, all: bool) {
        let release = self.params.get(param::RELEASE);
        let frames = Smoother::ramp_frames(release, self.sample_rate);
        for voice in &mut self.voices {
            if key.is_none_or(|key| key == voice.key) {
                voice.held = false;
                if !self.pedal {
                    voice.damp(frames, release, all);
                }
            }
        }
    }

    fn strike(&mut self, key: u8, velocity: f32) {
        if !self.model_supported() {
            return;
        }
        // An accepted profile (revision 2) is rendered whatever Body says.
        match Profile::from_body(self.params.get(param::BODY)).filter(|_| self.profile.is_none()) {
            Some(profile) => self.strike_bar(profile, key, velocity),
            None => self.strike_struck(key, velocity),
        }
    }

    /// One body per bar: a ringing bar of this key and profile is struck again.
    fn strike_bar(&mut self, profile: Profile, key: u8, velocity: f32) {
        let count = if self.params.get(param::QUALITY) < 0.5 { 8 } else { MAX_MODES };
        let modes: [_; MAX_MODES] = profile.modes(
            key,
            count,
            self.params.get(param::DECAY),
            self.params.get(param::BRIGHTNESS),
            self.params.get(param::STRIKE),
        );
        let excitation =
            if self.params.get(param::MALLET_MODEL) < 0.5 { Excitation::Contact } else { Excitation::Pulse };
        let mallet = bars::mallet(self.params.get(param::HARDNESS));
        let same = self
            .voices
            .iter()
            .position(|voice| voice.key == key && voice.profile == Some(profile) && voice.body.is_active());
        let slot = same.or_else(|| self.free_or_oldest());
        self.serial = self.serial.saturating_add(1);
        let Some(voice) = slot.and_then(|index| self.voices.get_mut(index)) else {
            return;
        };
        if same.is_none() {
            voice.start_steal_fade(self.sample_rate);
            voice.body = Body::Bar(StruckBar::default());
        }
        if let Body::Bar(bar) = &mut voice.body {
            bar.set_modes(self.sample_rate, &modes);
            bar.transpose(self.bend_semitones);
            bar.strike(excitation, mallet, bars::MAX_SPEED * f64::from(velocity));
        }
        voice.profile = Some(profile);
        voice.key = key;
        voice.held = true;
        voice.serial = self.serial;
        voice.amplitude = 1.0;
        voice.gate = Smoother::new(1.0, 1);
    }

    fn free_or_oldest(&self) -> Option<usize> {
        self.voices
            .iter()
            .position(|voice| !voice.body.is_active())
            .or_else(|| self.voices.iter().enumerate().min_by_key(|(_, voice)| voice.serial).map(|(index, _)| index))
    }

    /// Revision 1's struck body, unchanged, or an accepted profile's modes.
    fn strike_struck(&mut self, key: u8, velocity: f32) {
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
        let slot = self.free_or_oldest();
        self.serial = self.serial.saturating_add(1);
        if let Some(voice) = slot.and_then(|index| self.voices.get_mut(index)) {
            voice.start_steal_fade(self.sample_rate);
            let mut bank = ModalBank::default();
            bank.strike(self.sample_rate, &modes);
            bank.transpose(self.sample_rate, self.bend_semitones);
            voice.body = Body::Struck(bank);
            voice.profile = None;
            voice.key = key;
            voice.held = true;
            voice.serial = self.serial;
            voice.amplitude = velocity * 0.25;
            voice.gate = Smoother::new(0.0, Smoother::ramp_frames(0.002, self.sample_rate));
            voice.gate.set_target(1.0);
        }
    }
}

impl Voice {
    /// The previous output fades for 2 ms while the voice starts another body.
    fn start_steal_fade(&mut self, sample_rate: f64) {
        self.stolen_tail = self.last_output;
        self.stolen_fade =
            Smoother::new(if self.last_output == 0.0 { 0.0 } else { 1.0 }, Smoother::ramp_frames(0.002, sample_rate));
        self.stolen_fade.set_target(0.0);
    }

    /// Damp a released voice: Struck fades over `frames`; a bar is damped to `release` seconds
    /// when its profile damps on release, or always when `all`.
    fn damp(&mut self, frames: u32, release: f64, all: bool) {
        match (&mut self.body, self.profile) {
            (Body::Bar(bar), Some(profile)) => {
                if all || profile.damps_on_release() {
                    bar.damp(release);
                }
            }
            _ => {
                self.gate.set_ramp(frames);
                self.gate.set_target(0.0);
            }
        }
    }
}

impl Device for Instrument {
    fn info(&self) -> &DeviceInfo {
        &INFO
    }
    fn load_model_state(&mut self, state: &[u8]) -> Result<(), DeviceError> {
        match ModalProfile::decode(state) {
            Ok(profile) => {
                self.set_modal_profile(profile);
                Ok(())
            }
            Err(error) => {
                self.profile = None;
                self.invalid_profile = true;
                self.reset();
                Err(DeviceError::InvalidModelState(error.0))
            }
        }
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
        if matches!(index, param::RELEASE | param::ROOT..=param::VELOCITY_HIGH)
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
        for frame in buffer.as_chunks_mut::<2>().0 {
            let mut modal = 0.0;
            for voice in &mut self.voices {
                if !voice.body.is_active() && voice.stolen_fade.current() == 0.0 {
                    continue;
                }
                let gate = voice.gate.next_value();
                if gate == 0.0 && voice.gate.is_settled() {
                    voice.body.reset();
                }
                voice.last_output = voice.body.next_sample() * gate * voice.amplitude
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
                    if voice.body.is_active() || voice.stolen_fade.current() != 0.0 {
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
                    self.release(Some(key), false);
                } else if self.params.get(param::SOURCE) != 1.0
                    && f64::from(key) >= self.params.get(param::KEY_LOW)
                    && f64::from(key) <= self.params.get(param::KEY_HIGH)
                    && f64::from(velocity) >= self.params.get(param::VELOCITY_LOW)
                    && f64::from(velocity.min(1.0)) <= self.params.get(param::VELOCITY_HIGH)
                {
                    self.strike(key, velocity.clamp(0.0, 1.0));
                }
            }
            NoteEvent::Off { key, .. } if key <= 127 => self.release(Some(key), false),
            NoteEvent::Controller { number: 64, value } if value.is_finite() => {
                let old = self.pedal;
                self.pedal = value >= 0.5;
                if old && !self.pedal {
                    let release = self.params.get(param::RELEASE);
                    let frames = Smoother::ramp_frames(release, self.sample_rate);
                    for voice in &mut self.voices {
                        if !voice.held {
                            voice.damp(frames, release, false);
                        }
                    }
                }
            }
            NoteEvent::AllNotesOff => {
                self.pedal = false;
                self.release(None, true);
            }
            NoteEvent::PitchBend(value) if value.is_finite() => {
                self.bend_semitones = f64::from(value.clamp(-1.0, 1.0)) * 2.0;
                for voice in &mut self.voices {
                    if voice.body.is_active() {
                        voice.body.transpose(self.sample_rate, self.bend_semitones);
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

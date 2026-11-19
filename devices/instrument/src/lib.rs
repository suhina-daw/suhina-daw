// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Public native Instrument runtime, shared by stock and export adapters. The struck-modal
//! model is experimental revision 1. Sample playback uses the existing SDK runtime.

use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, NoteEvent, ParamBank, ParamInfo, ProcessContext, Role,
    SampleSource, Scale, Unit, check_sample_rate,
};
use suhina_device_sdk::sample_instrument::{self, Sampler};
use suhina_dsp::{
    Smoother,
    modal::{ModalBank, Mode},
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
    pub const COUNT: usize = 22;
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
];

pub static INFO: DeviceInfo = DeviceInfo::new("instrument", "Instrument (experimental)", Role::Instrument, &PARAMS);

#[derive(Clone, Copy)]
struct Voice {
    bank: ModalBank<MAX_MODES>,
    key: u8,
    held: bool,
    serial: u64,
    amplitude: f32,
    gate: Smoother,
}

impl Default for Voice {
    fn default() -> Self {
        Self { bank: ModalBank::default(), key: 0, held: false, serial: 0, amplitude: 0.0, gate: Smoother::new(0.0, 1) }
    }
}

/// The complete public core. Construction, preparation and sample loading run off the audio
/// thread. Device rendering/events/reset use fixed storage and delegate sample voices to the SDK.
pub struct Instrument {
    sample_rate: f64,
    params: ParamBank<{ param::COUNT }>,
    sampler: Sampler,
    voices: [Voice; MAX_VOICES],
    serial: u64,
    pedal: bool,
    gain: Smoother,
    sample_mix: Smoother,
    modal_mix: Smoother,
}

impl Default for Instrument {
    fn default() -> Self {
        Self::new()
    }
}

impl Instrument {
    pub fn new() -> Self {
        Self::at_rate(DEFAULT_SAMPLE_RATE)
    }

    pub fn with_rate(sample_rate: f64) -> Result<Self, DeviceError> {
        Ok(Self::at_rate(check_sample_rate(sample_rate)?))
    }

    fn at_rate(sample_rate: f64) -> Self {
        let ramp = Smoother::ramp_frames(0.01, sample_rate);
        // The constructor has validated the rate. The fallback is only a defensive guard
        // against a future SDK constructor adding an unrelated failure mode.
        let sampler = Sampler::with_rate(sample_rate).unwrap_or_else(|_| Sampler::new());
        let mut core = Self {
            sample_rate,
            params: ParamBank::new(&PARAMS),
            sampler,
            voices: [Voice::default(); MAX_VOICES],
            serial: 0,
            pedal: false,
            gain: Smoother::new(0.0, ramp),
            sample_mix: Smoother::new(0.0, ramp),
            modal_mix: Smoother::new(1.0, ramp),
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
        self.params.get(param::REVISION) == f64::from(MODEL_REVISION)
    }

    fn release(&mut self, key: Option<u8>) {
        let frames = Smoother::ramp_frames(self.params.get(param::RELEASE), self.sample_rate);
        for voice in &mut self.voices {
            if key.is_none_or(|key| key == voice.key) {
                voice.held = false;
                if !self.pedal {
                    voice.gate.set_ramp(frames);
                    voice.gate.set_target(0.0);
                }
            }
        }
    }

    fn strike(&mut self, key: u8, velocity: f32) {
        if !self.model_supported() {
            return;
        }
        let mut modes = [Mode { frequency_hz: 0.0, decay_seconds: 0.0, amplitude: 0.0 }; MAX_MODES];
        let fundamental = 440.0 * 2.0_f64.powf((f64::from(key) - 69.0) / 12.0);
        let structure = self.params.get(param::STRUCTURE);
        let count = if self.params.get(param::QUALITY) < 0.5 { 8 } else { MAX_MODES };
        for (index, mode) in modes.iter_mut().take(count).enumerate() {
            let harmonic = (index + 1) as f64;
            mode.frequency_hz =
                fundamental * harmonic * ((1.0 + structure * harmonic * harmonic) / (1.0 + structure)).sqrt();
            mode.decay_seconds = self.params.get(param::DECAY) / (1.0 + 0.12 * (harmonic - 1.0));
            mode.amplitude = (std::f64::consts::PI * harmonic * self.params.get(param::STRIKE)).sin()
                / harmonic.powf(2.0 - 1.5 * self.params.get(param::BRIGHTNESS));
        }
        let slot =
            self.voices.iter().position(|voice| !voice.bank.is_active()).or_else(|| {
                self.voices.iter().enumerate().min_by_key(|(_, voice)| voice.serial).map(|(index, _)| index)
            });
        self.serial = self.serial.saturating_add(1);
        if let Some(voice) = slot.and_then(|index| self.voices.get_mut(index)) {
            voice.bank.strike(self.sample_rate, &modes);
            voice.key = key;
            voice.held = true;
            voice.serial = self.serial;
            voice.amplitude = velocity * 0.25;
            voice.gate = Smoother::new(0.0, Smoother::ramp_frames(0.002, self.sample_rate));
            voice.gate.set_target(1.0);
        }
    }
}

impl Device for Instrument {
    fn info(&self) -> &DeviceInfo {
        &INFO
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
        self.sampler.reset();
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
        if self.sample_mix.current() > 0.0 || self.sample_mix.target() > 0.0 {
            self.sampler.process(context, buffer);
        } else {
            for frame in buffer.chunks_exact_mut(2) {
                frame.fill(0.0);
            }
        }
        for frame in buffer.chunks_exact_mut(2) {
            let mut modal = 0.0;
            for voice in &mut self.voices {
                if !voice.bank.is_active() {
                    continue;
                }
                let gate = voice.gate.next_value();
                if gate == 0.0 && voice.gate.is_settled() {
                    voice.bank.reset();
                    continue;
                }
                modal += voice.bank.next_sample() * gate * voice.amplitude;
            }
            let sample_mix = self.sample_mix.next_value();
            let modal = modal * self.modal_mix.next_value();
            let gain = self.gain.next_value();
            for sample in frame {
                *sample = (*sample * sample_mix + modal) * gain;
            }
            if self.modal_mix.current() == 0.0 && self.modal_mix.is_settled() {
                for voice in &mut self.voices {
                    if voice.bank.is_active() {
                        voice.bank.reset();
                    }
                }
            }
        }
    }

    fn event(&mut self, event: NoteEvent) {
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
                    for voice in &mut self.voices {
                        if !voice.held {
                            voice.gate.set_ramp(frames);
                            voice.gate.set_target(0.0);
                        }
                    }
                }
            }
            NoteEvent::AllNotesOff => {
                self.pedal = false;
                self.release(None);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;

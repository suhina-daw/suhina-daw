// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sine tones a MIDI track plays without an instrument, as an instrument of its chain: a
//! chain whose MIDI effects have no instrument after them gets one, so the notes they change
//! sound as the track's notes would without them (ADR 0004 › 14). Each note is the renderer's
//! sine voice: the same pitch, attack, release and level.

use suhina_device_api::{Device, DeviceError, DeviceInfo, NoteEvent, ProcessContext, Role, check_sample_rate};

use crate::chain::CHAIN_INPUT_GAIN;
use crate::{ATTACK_SECONDS, MAX_VOICES, RELEASE_SECONDS};

pub(crate) static INFO: DeviceInfo =
    DeviceInfo { id: "sine-tones", name: "Sine tones", role: Role::Instrument, params: &[] };

#[derive(Clone, Copy, Default)]
struct Tone {
    active: bool,
    key: u8,
    /// Whether its key is still down.
    held: bool,
    /// Frames since it started, and its length when its key came up.
    age: u64,
    length: u64,
    phase: f64,
    step: f64,
    gain: f32,
}

pub(crate) struct SineTones {
    sample_rate: f64,
    tones: [Tone; MAX_VOICES],
}

impl SineTones {
    /// Silent tones at `sample_rate`, at least 1 Hz.
    pub(crate) fn new(sample_rate: u32) -> Self {
        Self { sample_rate: f64::from(sample_rate.max(1)), tones: [Tone::default(); MAX_VOICES] }
    }

    /// Starts a tone at `key` with a gain from `velocity`, in a free voice or in place of the oldest.
    fn start(&mut self, key: u8, velocity: f32) {
        // A free tone, or the oldest one when every tone sounds.
        let Some(tone) = self
            .tones
            .iter_mut()
            .reduce(|best, tone| if !best.active || (tone.active && tone.age <= best.age) { best } else { tone })
        else {
            return;
        };
        let frequency = 440.0 * 2f64.powf((f64::from(key) - 69.0) / 12.0);
        *tone = Tone {
            active: true,
            key,
            held: true,
            age: 0,
            length: 0,
            phase: 0.0,
            step: frequency / self.sample_rate,
            gain: CHAIN_INPUT_GAIN * velocity.clamp(0.0, 1.0),
        };
    }

    /// Lets go of the oldest held tone of `key`.
    fn release(&mut self, key: u8) {
        if let Some(tone) =
            self.tones.iter_mut().filter(|tone| tone.active && tone.held && tone.key == key).max_by_key(|tone| tone.age)
        {
            tone.held = false;
            tone.length = tone.age;
        }
    }
}

impl Device for SineTones {
    /// The sine tones' description: an instrument with no parameters.
    fn info(&self) -> &'static DeviceInfo {
        &INFO
    }

    /// Takes the sample rate and silences every tone.
    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        self.sample_rate = check_sample_rate(sample_rate)?;
        self.reset();
        Ok(())
    }

    /// Silences every tone at once.
    fn reset(&mut self) {
        self.tones = [Tone::default(); MAX_VOICES];
    }

    /// Has no parameters to set.
    fn set_param(&mut self, _: usize, _: f64) {}

    /// Has no parameters.
    fn param(&self, _: usize) -> Option<f64> {
        None
    }

    /// Writes the sum of the sounding tones, each with its attack and release, to both channels.
    fn process(&mut self, _: &ProcessContext, buffer: &mut [f32]) {
        let attack = (ATTACK_SECONDS * self.sample_rate).max(1.0);
        let release = (RELEASE_SECONDS * self.sample_rate).max(1.0);
        for frame in buffer.as_chunks_mut::<2>().0 {
            let mut sum = 0.0f32;
            for tone in self.tones.iter_mut().filter(|tone| tone.active) {
                let envelope = if tone.held {
                    (tone.age as f64 / attack).min(1.0)
                } else {
                    let released = (tone.age - tone.length) as f64;
                    if released >= release {
                        tone.active = false;
                        continue;
                    }
                    (tone.length as f64 / attack).min(1.0) * (1.0 - released / release)
                };
                let sample = (tone.phase * std::f64::consts::TAU).sin() * envelope;
                tone.phase = (tone.phase + tone.step).fract();
                tone.age += 1;
                sum += sample as f32 * tone.gain;
            }
            *frame = [sum, sum];
        }
    }

    /// Starts a tone on a note-on, lets go of it on a note-off (or a note-on at velocity 0), and of
    /// every tone on all-notes-off.
    fn event(&mut self, event: NoteEvent) {
        match event {
            NoteEvent::On { key, velocity } if key <= 127 && velocity > 0.0 => self.start(key, velocity),
            NoteEvent::On { key, .. } | NoteEvent::Off { key, .. } => self.release(key),
            NoteEvent::AllNotesOff => {
                for tone in self.tones.iter_mut().filter(|tone| tone.active && tone.held) {
                    tone.held = false;
                    tone.length = tone.age;
                }
            }
            NoteEvent::Controller { .. } | NoteEvent::PitchBend(_) | NoteEvent::Pressure(_) | NoteEvent::Scale(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sine tones pass every check an instrument must.
    #[test]
    fn passes_the_contract_tests() {
        let failures = suhina_device_api::conformance::check(&|| Box::new(SineTones::new(48_000)));
        assert!(failures.is_empty(), "{failures:#?}");
    }
}

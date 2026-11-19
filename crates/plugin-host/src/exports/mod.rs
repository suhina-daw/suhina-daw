// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Format-independent native-device adapter. All synthesis remains in the registry devices.

mod clap;
pub mod state;
mod vst3;

use std::sync::atomic::{AtomicU64, Ordering};
use suhina_device_api::{Device, DeviceInfo, NoteEvent, ProcessContext};

/// Maximum accepted block size, checked before buffer allocation.
pub const MAX_FRAMES: usize = 32_768;
/// Maximum parameters in an exported device or state.
pub const MAX_PARAMS: usize = 4096;

/// Devices whose entire persistent state is currently represented by parameters.
/// Resource-backed and MIDI-effect exports remain explicit follow-on work in issue 931.
pub fn exportable(info: &DeviceInfo) -> bool {
    if info.role == suhina_device_api::Role::MidiEffect || suhina_device_registry::is_file_device(&info.id) {
        return false;
    }
    suhina_device_registry::create(&info.id, 48_000.0).is_some_and(|device| device.samples_ref().is_none())
}

/// Stable host id: FNV-1a of the UTF-8 native parameter id, in the common 31-bit range.
pub fn parameter_id(id: &str) -> u32 {
    id.bytes().fold(2_166_136_261u32, |hash, byte| (hash ^ u32::from(byte)).wrapping_mul(16_777_619)) & 0x7fff_ffff
}

/// A bounded adapter error, including errors on the audio thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportError {
    UnknownDevice,
    InvalidConfiguration,
    InvalidParameters,
    InvalidBlock,
    InvalidEvents,
    InvalidState,
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "native device export: {self:?}")
    }
}

impl std::error::Error for ExportError {}

/// Parameter values shared with the host's control thread. Storage is fixed at construction.
pub struct Parameters {
    pub info: &'static DeviceInfo,
    ids: Vec<u32>,
    values: Vec<AtomicU64>,
    pending: Vec<AtomicU64>,
    revision: AtomicU64,
    applied: AtomicU64,
}

impl Parameters {
    /// Validate the static registry metadata and allocate outside audio processing.
    pub fn new(info: &'static DeviceInfo) -> Result<Self, ExportError> {
        let state_size = 12 + info.id.len() + info.params.iter().map(|p| 10 + p.id.len()).sum::<usize>();
        if info.params.len() > MAX_PARAMS || info.id.len() > 256 || state_size > state::MAX_BYTES {
            return Err(ExportError::InvalidParameters);
        }
        let mut ids = Vec::with_capacity(info.params.len());
        for parameter in info.params.iter() {
            let id = parameter_id(&parameter.id);
            if ids.contains(&id) || parameter.id.len() > 256 {
                return Err(ExportError::InvalidParameters);
            }
            ids.push(id);
        }
        Ok(Self {
            info,
            ids,
            values: info.params.iter().map(|parameter| AtomicU64::new(parameter.default.to_bits())).collect(),
            pending: info.params.iter().map(|parameter| AtomicU64::new(parameter.default.to_bits())).collect(),
            revision: AtomicU64::new(0),
            applied: AtomicU64::new(0),
        })
    }

    pub fn index(&self, id: u32) -> Option<usize> {
        self.ids.iter().position(|candidate| *candidate == id)
    }

    pub fn id(&self, index: usize) -> Option<u32> {
        self.ids.get(index).copied()
    }

    pub fn value(&self, index: usize) -> Option<f64> {
        let revision = self.revision.load(Ordering::SeqCst);
        let values = if revision.is_multiple_of(2) && revision != self.applied.load(Ordering::SeqCst) {
            &self.pending
        } else {
            &self.values
        };
        values.get(index).map(|value| f64::from_bits(value.load(Ordering::SeqCst)))
    }

    pub fn set(&self, id: u32, value: f64) {
        if let Some(index) = self.index(id)
            && let (Some(parameter), Some(destination)) = (self.info.params.get(index), self.values.get(index))
        {
            destination.store(parameter.clamp(value).to_bits(), Ordering::SeqCst);
        }
    }

    /// Main-thread edit, ordered after state even before an audio processor exists.
    pub fn control_set(&self, id: u32, value: f64) {
        if self.revision.load(Ordering::SeqCst) != self.applied.load(Ordering::SeqCst) {
            if let Some(index) = self.index(id)
                && let Some(parameter) = self.info.params.get(index)
            {
                let _ = self.restore(&[(&parameter.id, value)]);
            }
        } else {
            self.set(id, value);
        }
    }

    /// Publish a validated state off RT. All fields are atomic; a render reads a snapshot once
    /// and defers the update if the revision changes, never spinning on the control thread.
    fn restore(&self, values: &[(&str, f64)]) -> Result<(), ExportError> {
        let previous = self.revision.load(Ordering::SeqCst);
        let next = previous.checked_add(2).ok_or(ExportError::InvalidState)?;
        let restored: Vec<f64> = self
            .info
            .params
            .iter()
            .enumerate()
            .map(|(index, parameter)| {
                let value = values
                    .iter()
                    .find(|(id, _)| *id == parameter.id)
                    .map(|(_, value)| *value)
                    .unwrap_or_else(|| self.value(index).unwrap_or(parameter.default));
                parameter.clamp(value)
            })
            .collect();
        if !previous.is_multiple_of(2)
            || self.revision.compare_exchange(previous, previous + 1, Ordering::SeqCst, Ordering::SeqCst).is_err()
        {
            return Err(ExportError::InvalidState);
        }
        for (destination, value) in self.pending.iter().zip(restored) {
            destination.store(value.to_bits(), Ordering::SeqCst);
        }
        self.revision.store(next, Ordering::SeqCst);
        Ok(())
    }
}

/// An event already translated from a host format, at the block's sample offset.
#[derive(Debug, Clone, Copy)]
pub struct Event {
    pub frame: usize,
    pub kind: EventKind,
}

#[derive(Debug, Clone, Copy)]
pub enum EventKind {
    Parameter { id: u32, value: f64 },
    Note(NoteEvent),
}

/// The one native processor used by both export formats. Construction and destruction are off RT.
pub struct Processor {
    device: Box<dyn Device>,
    sample_rate: f64,
    max_frames: usize,
    values: Vec<f64>,
    revision: u64,
}

impl Processor {
    pub fn new(parameters: &Parameters, sample_rate: f64, max_frames: usize) -> Result<Self, ExportError> {
        if max_frames == 0 || max_frames > MAX_FRAMES || !sample_rate.is_finite() {
            return Err(ExportError::InvalidConfiguration);
        }
        suhina_device_api::check_sample_rate(sample_rate).map_err(|_| ExportError::InvalidConfiguration)?;
        let device =
            suhina_device_registry::create(&parameters.info.id, sample_rate).ok_or(ExportError::UnknownDevice)?;
        let mut processor = Self {
            device,
            sample_rate,
            max_frames,
            values: vec![0.0; parameters.info.params.len()],
            revision: parameters.applied.load(Ordering::SeqCst),
        };
        processor.sync_parameters(parameters);
        processor.device.reset();
        Ok(processor)
    }

    pub fn reset(&mut self) {
        self.device.reset();
    }

    pub fn latency(&self) -> u32 {
        self.device.latency()
    }

    /// Read host changes at a block boundary. Host automation inside the block uses events.
    pub fn sync_parameters(&mut self, parameters: &Parameters) {
        let revision = parameters.revision.load(Ordering::SeqCst);
        if !revision.is_multiple_of(2) {
            return;
        }
        let changed = self.revision != revision;
        let source = if changed { &parameters.pending } else { &parameters.values };
        for (value, source) in self.values.iter_mut().zip(source) {
            *value = f64::from_bits(source.load(Ordering::SeqCst));
        }
        if parameters.revision.load(Ordering::SeqCst) != revision {
            return;
        }
        for (index, &value) in self.values.iter().enumerate() {
            if changed && let Some(destination) = parameters.values.get(index) {
                destination.store(value.to_bits(), Ordering::SeqCst);
            }
            if self.device.param(index) != Some(value) {
                self.device.set_param(index, value);
            }
        }
        if self.revision != revision {
            self.device.reset();
            self.revision = revision;
            parameters.applied.store(revision, Ordering::SeqCst);
        }
    }

    pub fn apply(&mut self, parameters: &Parameters, kind: EventKind) {
        match kind {
            EventKind::Parameter { id, value } => {
                parameters.set(id, value);
                if let Some(index) = parameters.index(id)
                    && let Some(parameter) = parameters.info.params.get(index)
                {
                    self.device.set_param(index, parameter.clamp(value));
                }
            }
            EventKind::Note(note) => self.device.event(note),
        }
    }

    /// Render with sorted, bounded host events and optional stereo key input, with no allocation.
    pub fn process(
        &mut self,
        parameters: &Parameters,
        context: ProcessContext,
        audio: &mut [f32],
        key: Option<&[f32]>,
        events: &[Event],
    ) -> Result<(), ExportError> {
        let frames = audio.len() / 2;
        if !audio.len().is_multiple_of(2) || frames > self.max_frames || key.is_some_and(|key| key.len() != audio.len())
        {
            audio.fill(0.0);
            return Err(ExportError::InvalidBlock);
        }
        let mut previous = 0;
        for event in events {
            if event.frame < previous || event.frame >= frames {
                audio.fill(0.0);
                return Err(ExportError::InvalidEvents);
            }
            previous = event.frame;
        }
        self.sync_parameters(parameters);
        self.device.set_context(&self.context_at(context, 0));
        let mut cursor = 0;
        for event in events {
            self.span(context, cursor, event.frame, audio, key);
            self.device.set_context(&self.context_at(context, event.frame));
            self.apply(parameters, event.kind);
            cursor = event.frame;
        }
        self.span(context, cursor, frames, audio, key);
        Ok(())
    }

    fn context_at(&self, context: ProcessContext, frame: usize) -> ProcessContext {
        let tempo = context.tempo_or_default();
        let beat = if context.beat.is_finite() { context.beat } else { 0.0 };
        let offset = if context.playing { frame as f64 * tempo / (60.0 * self.sample_rate) } else { 0.0 };
        ProcessContext { tempo, beat: beat + offset, playing: context.playing }
    }

    fn span(&mut self, context: ProcessContext, from: usize, to: usize, audio: &mut [f32], key: Option<&[f32]>) {
        if from == to {
            return;
        }
        let context = self.context_at(context, from);
        if let Some(buffer) = audio.get_mut(from * 2..to * 2) {
            if let Some(key) = key.and_then(|key| key.get(from * 2..to * 2)) {
                self.device.process_keyed(&context, key, buffer);
            } else {
                self.device.process(&context, buffer);
            }
        }
    }
}

#[cfg(test)]
mod tests;

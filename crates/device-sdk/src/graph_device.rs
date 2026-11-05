// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A `.sd` graph as a [`Device`] like any built-in (#763 › 6): the engine runs it through the
//! same slot, automation and smoothing paths, and it passes the same conformance checks.
//!
//! Its description is read from the graph: its id, name and role, and one [`ParamInfo`] per
//! graph parameter, of the kind its control shows. It reports no latency: a delay node is the
//! sound (an echo), not a delay to compensate (`docs/spec/sdk-nodes.md`).

use std::borrow::Cow;

use suhina_device_api::{
    Device, DeviceError, DeviceInfo, Labels, NoteEvent, ParamInfo, ParamKind, ProcessContext, Role, SAMPLE_RATE_RANGE,
    Scale, Unit,
};

use crate::{DeviceGraph, DeviceRole, DeviceSdkError, Parameter, ParameterControl, PreparedEffect, PreparedMidiEffect};

/// Compiles an audio graph at a sample rate. The device registry gives one that knows the native
/// nodes and holds the graph's samples; the SDK knows neither. Runs off the audio thread.
pub type Compile = Box<dyn Fn(f64) -> Result<PreparedEffect, DeviceSdkError> + Send>;

pub struct GraphDevice {
    info: DeviceInfo,
    runtime: Runtime,
}

enum Runtime {
    /// An effect or an instrument.
    Audio {
        prepared: PreparedEffect,
        compile: Compile,
    },
    Midi(PreparedMidiEffect),
}

impl GraphDevice {
    /// An effect or instrument graph, compiled by `compile` at `sample_rate`.
    pub fn audio(graph: &DeviceGraph, sample_rate: f64, compile: Compile) -> Result<Self, DeviceSdkError> {
        if graph.role == DeviceRole::MidiEffect {
            return Err(DeviceSdkError("a midi_effect graph is made with GraphDevice::midi".into()));
        }
        let prepared = compile(sample_rate)?;
        Ok(Self { info: describe(graph), runtime: Runtime::Audio { prepared, compile } })
    }

    /// A MIDI effect graph.
    pub fn midi(graph: &DeviceGraph) -> Result<Self, DeviceSdkError> {
        Ok(Self { info: describe(graph), runtime: Runtime::Midi(graph.compile_midi()?) })
    }
}

/// What a graph is, as a device describes itself.
pub fn describe(graph: &DeviceGraph) -> DeviceInfo {
    let role = match graph.role {
        DeviceRole::Effect => Role::Effect,
        DeviceRole::Instrument => Role::Instrument,
        DeviceRole::MidiEffect => Role::MidiEffect,
    };
    DeviceInfo {
        id: Cow::Owned(graph.id.clone()),
        name: Cow::Owned(graph.name.clone()),
        role,
        params: Cow::Owned(graph.parameters.iter().map(param).collect()),
    }
}

/// A graph parameter as a device parameter: its kind from the control that shows it, so it
/// clamps, rounds, automates and reads on a knob as the panel shows it.
fn param(p: &Parameter) -> ParamInfo {
    use ParameterControl as C;
    let kind = match p.control {
        C::Integer => ParamKind::Integer,
        C::Toggle | C::Button => ParamKind::Toggle,
        // Validation gives a choice one label per value.
        C::Choice | C::Radio => ParamKind::Choice(Labels::Owned(p.ui.options.clone().into())),
        C::LogSlider | C::LogKnob if p.min > 0.0 && p.max > p.min => ParamKind::Continuous(Scale::Logarithmic),
        C::Slider | C::LogSlider | C::Knob | C::LogKnob | C::Number | C::VerticalSlider => {
            ParamKind::Continuous(Scale::Linear)
        }
    };
    ParamInfo {
        id: Cow::Owned(p.id.clone()),
        name: Cow::Owned(p.name.clone()),
        group: "",
        kind,
        min: p.min,
        max: p.max,
        default: p.default,
        unit: unit(&p.unit),
    }
}

/// The unit a graph names, as [`crate::builtin::unit`] writes it; anything else is no unit.
fn unit(name: &str) -> Unit {
    [
        Unit::Decibels,
        Unit::Hertz,
        Unit::Seconds,
        Unit::Percent,
        Unit::Balance,
        Unit::Octaves,
        Unit::Semitones,
        Unit::ScaleDegrees,
        Unit::Key,
    ]
    .into_iter()
    .find(|&u| crate::builtin::unit(u) == name)
    .unwrap_or(Unit::None)
}

impl Device for GraphDevice {
    fn info(&self) -> &DeviceInfo {
        &self.info
    }

    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        if !(SAMPLE_RATE_RANGE.0..=SAMPLE_RATE_RANGE.1).contains(&sample_rate) {
            return Err(DeviceError::InvalidSampleRate(sample_rate));
        }
        match &mut self.runtime {
            Runtime::Audio { prepared, compile } => {
                // The graph compiled once already, so only the rate can make it fail now.
                let mut fresh = compile(sample_rate).map_err(|_| DeviceError::InvalidSampleRate(sample_rate))?;
                for index in 0..self.info.params.len() {
                    if let Some(value) = prepared.parameter(index) {
                        fresh.set_parameter(index, value);
                    }
                }
                fresh.reset();
                *prepared = fresh;
            }
            Runtime::Midi(effect) => effect.reset(),
        }
        Ok(())
    }

    fn reset(&mut self) {
        match &mut self.runtime {
            Runtime::Audio { prepared, .. } => prepared.reset(),
            Runtime::Midi(effect) => effect.reset(),
        }
    }

    fn set_param(&mut self, index: usize, value: f64) {
        match &mut self.runtime {
            Runtime::Audio { prepared, .. } => prepared.set_parameter(index, value),
            Runtime::Midi(effect) => effect.set_parameter(index, value),
        }
    }

    fn param(&self, index: usize) -> Option<f64> {
        match &self.runtime {
            Runtime::Audio { prepared, .. } => prepared.parameter(index),
            Runtime::Midi(effect) => effect.parameter(index),
        }
    }

    fn process(&mut self, context: &ProcessContext, buffer: &mut [f32]) {
        // A trailing odd sample is left as it is.
        let frames = buffer.len() / 2 * 2;
        if let (Runtime::Audio { prepared, .. }, Some(frames)) = (&mut self.runtime, buffer.get_mut(..frames)) {
            prepared.process_context(context, frames);
        }
    }

    fn event(&mut self, event: NoteEvent) {
        // Only an instrument plays notes: an effect graph never had them.
        if let (Role::Instrument, Runtime::Audio { prepared, .. }) = (self.info.role, &mut self.runtime) {
            prepared.apply_event(event);
        }
    }

    fn process_events(
        &mut self,
        _context: &ProcessContext,
        frames: usize,
        input: &[(usize, NoteEvent)],
        output: &mut [(usize, NoteEvent)],
    ) -> usize {
        let Runtime::Midi(effect) = &mut self.runtime else {
            let written = input.len().min(output.len());
            output[..written].copy_from_slice(&input[..written]);
            return written;
        };
        let written = effect.process(input, output).min(output.len());
        let last = frames.saturating_sub(1);
        for event in &mut output[..written] {
            event.0 = event.0.min(last);
        }
        written
    }
}

#[cfg(test)]
#[path = "graph_device_tests.rs"]
mod tests;

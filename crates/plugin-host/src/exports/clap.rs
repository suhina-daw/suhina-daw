// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! CLAP entry and factory. Clack owns the raw ABI; synthesis uses the safe native adapter.

use std::ffi::CStr;
use std::sync::atomic::{AtomicU32, Ordering};

use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPorts, PluginAudioPortsImpl,
};
use clack_extensions::latency::{PluginLatency, PluginLatencyImpl};
use clack_extensions::note_ports::{
    NoteDialect, NoteDialects, NotePortInfo, NotePortInfoWriter, PluginNotePorts, PluginNotePortsImpl,
};
use clack_extensions::params::{
    HostParams, ParamDisplayWriter, ParamInfo, ParamInfoFlags, ParamInfoWriter, ParamRescanFlags,
    PluginAudioProcessorParams, PluginMainThreadParams, PluginParams,
};
use clack_extensions::state::{PluginState, PluginStateImpl};
use clack_plugin::entry::prelude::*;
use clack_plugin::events::event_types::TransportFlags;
use clack_plugin::events::io::{InputEvents, OutputEvents};
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::prelude::*;
use clack_plugin::stream::{InputStream, OutputStream};
use clack_plugin::utils::Cookie;
use suhina_device_api::{DeviceInfo, NoteEvent, ProcessContext, Role};

use super::{Event as NativeEvent, EventKind, Parameters, Processor, state};

const MAX_EVENTS: usize = 4096;
#[cfg(test)]
mod tests;

// SAFETY: Clack generates CLAP ABI callbacks and owns their pointer/lifetime validation.
// The entry and all raw ABI code remain inside the permitted plugin-host boundary.
clack_export_entry!(ExportEntry);

struct ExportEntry {
    factory: PluginFactoryWrapper<Factory>,
}

impl Entry for ExportEntry {
    fn new(_path: Option<&CStr>) -> Result<Self, EntryLoadError> {
        let descriptors = suhina_device_registry::DEVICES
            .iter()
            .filter(|info| super::exportable(info))
            .map(|info| {
                let features = if info.role == Role::Instrument {
                    [c"instrument", c"stereo"]
                } else {
                    [c"audio-effect", c"stereo"]
                };
                (
                    *info,
                    PluginDescriptor::new(&format!("org.suhina.device.{}", info.id), &format!("Suhina {}", info.name))
                        .with_vendor("Suhina")
                        .with_version(env!("CARGO_PKG_VERSION"))
                        .with_features(features),
                )
            })
            .collect();
        Ok(Self { factory: PluginFactoryWrapper::new(Factory { descriptors }) })
    }

    fn declare_factories<'a>(&'a self, builder: &mut EntryFactories<'a>) {
        builder.register_factory(&self.factory);
    }
}

struct Factory {
    descriptors: Vec<(&'static DeviceInfo, PluginDescriptor)>,
}

impl PluginFactoryImpl for Factory {
    fn plugin_count(&self) -> u32 {
        self.descriptors.len() as u32
    }
    fn plugin_descriptor(&self, index: u32) -> Option<&PluginDescriptor> {
        self.descriptors.get(index as usize).map(|(_, descriptor)| descriptor)
    }
    fn create_plugin<'a>(&'a self, host: HostInfo<'a>, id: &CStr) -> Option<PluginInstance<'a>> {
        let (info, descriptor) = self.descriptors.iter().find(|(_, descriptor)| descriptor.id() == Some(id))?;
        Some(PluginInstance::new::<NativePlugin>(
            host,
            descriptor,
            |_| {
                Ok(Shared {
                    parameters: Parameters::new(info).map_err(|_| PluginError::Message("invalid device metadata"))?,
                    latency: AtomicU32::new(0),
                })
            },
            |host, shared| Ok(MainThread { shared, host }),
        ))
    }
}

struct NativePlugin;
struct Shared {
    parameters: Parameters,
    latency: AtomicU32,
}
struct MainThread<'a> {
    shared: &'a Shared,
    host: HostMainThreadHandle<'a>,
}
struct AudioProcessor<'a> {
    shared: &'a Shared,
    processor: Processor,
    buffer: Vec<f32>,
    events: Vec<NativeEvent>,
}

impl PluginShared<'_> for Shared {}
impl<'a> PluginMainThread<'a, Shared> for MainThread<'a> {}
impl Plugin for NativePlugin {
    type AudioProcessor<'a> = AudioProcessor<'a>;
    type Shared<'a> = Shared;
    type MainThread<'a> = MainThread<'a>;
    fn declare_extensions(builder: &mut PluginExtensions<Self>, _shared: Option<&Shared>) {
        builder
            .register::<PluginAudioPorts>()
            .register::<PluginParams>()
            .register::<PluginState>()
            .register::<PluginNotePorts>()
            .register::<PluginLatency>();
    }
}

impl PluginNotePortsImpl for MainThread<'_> {
    fn count(&self, input: bool) -> u32 {
        u32::from(input && self.shared.parameters.info.role == Role::Instrument)
    }
    fn get(&self, index: u32, input: bool, writer: &mut NotePortInfoWriter) {
        if index < PluginNotePortsImpl::count(self, input) {
            writer.set(&NotePortInfo {
                id: ClapId::new(0),
                name: b"Notes",
                supported_dialects: NoteDialects::MIDI,
                preferred_dialect: Some(NoteDialect::Midi),
            });
        }
    }
}

impl PluginLatencyImpl for MainThread<'_> {
    fn get(&self) -> u32 {
        self.shared.latency.load(Ordering::Relaxed)
    }
}

impl PluginAudioPortsImpl for MainThread<'_> {
    fn count(&self, input: bool) -> u32 {
        u32::from(!input || self.shared.parameters.info.role != Role::Instrument)
    }
    fn get(&self, index: u32, input: bool, writer: &mut AudioPortInfoWriter) {
        if index < PluginAudioPortsImpl::count(self, input) {
            writer.set(&AudioPortInfo {
                id: ClapId::new(0),
                name: if input { b"Main input" } else { b"Main output" },
                channel_count: 2,
                flags: AudioPortFlags::IS_MAIN,
                port_type: Some(AudioPortType::STEREO),
                in_place_pair: None,
            });
        }
    }
}

impl PluginMainThreadParams for MainThread<'_> {
    fn count(&self) -> u32 {
        self.shared.parameters.info.params.len() as u32
    }
    fn get_info(&self, index: u32, writer: &mut ParamInfoWriter) {
        let parameters = &self.shared.parameters;
        if let (Some(parameter), Some(id)) = (parameters.info.params.get(index as usize), parameters.id(index as usize))
        {
            let mut flags = ParamInfoFlags::IS_AUTOMATABLE;
            if !matches!(parameter.kind, suhina_device_api::ParamKind::Continuous(_)) {
                flags |= ParamInfoFlags::IS_STEPPED;
            }
            writer.set(&ParamInfo {
                id: ClapId::new(id),
                flags,
                cookie: Cookie::empty(),
                name: parameter.name.as_bytes(),
                module: parameter.group.as_bytes(),
                min_value: parameter.min,
                max_value: parameter.max,
                default_value: parameter.default,
            });
        }
    }
    fn get_value(&self, id: ClapId) -> Option<f64> {
        self.shared.parameters.index(id.get()).and_then(|index| self.shared.parameters.value(index))
    }
    fn value_to_text(&self, id: ClapId, value: f64, writer: &mut ParamDisplayWriter) -> std::fmt::Result {
        use std::fmt::Write;
        let index = self.shared.parameters.index(id.get()).ok_or(std::fmt::Error)?;
        let parameter = self.shared.parameters.info.params.get(index).ok_or(std::fmt::Error)?;
        match &parameter.kind {
            suhina_device_api::ParamKind::Choice(options) => {
                let label = options.get(parameter.clamp(value) as usize).unwrap_or("");
                writer.write_str(label)
            }
            _ => write!(writer, "{:.3}", parameter.clamp(value)),
        }
    }
    fn text_to_value(&self, id: ClapId, text: &CStr) -> Option<f64> {
        let index = self.shared.parameters.index(id.get())?;
        let parameter = self.shared.parameters.info.params.get(index)?;
        let value = text.to_str().ok()?.parse::<f64>().ok()?;
        value.is_finite().then(|| parameter.clamp(value))
    }
    fn flush(&self, input: &InputEvents, _output: &mut OutputEvents) {
        for event in input {
            if let Some(CoreEventSpace::ParamValue(value)) = event.as_core_event()
                && let Some(id) = value.param_id()
            {
                self.shared.parameters.control_set(id.get(), value.value());
            }
        }
    }
}

fn flush(parameters: &Parameters, input: &InputEvents) {
    if input.len() as usize > MAX_EVENTS {
        return;
    }
    for event in input {
        if let Some(CoreEventSpace::ParamValue(value)) = event.as_core_event()
            && let Some(id) = value.param_id()
        {
            parameters.set(id.get(), value.value());
        }
    }
}

impl PluginAudioProcessorParams for AudioProcessor<'_> {
    fn flush(&mut self, input: &InputEvents, _output: &mut OutputEvents) {
        self.processor.sync_parameters(&self.shared.parameters);
        flush(&self.shared.parameters, input);
        self.processor.sync_parameters(&self.shared.parameters);
    }
}

impl PluginStateImpl for MainThread<'_> {
    fn save(&self, output: &mut OutputStream) -> Result<(), PluginError> {
        use std::io::Write;
        output
            .write_all(&state::save(&self.shared.parameters))
            .map_err(|_| PluginError::Message("could not save state"))
    }
    fn load(&self, input: &mut InputStream) -> Result<(), PluginError> {
        use std::io::Read;
        let mut bytes = Vec::new();
        input
            .take((state::MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| PluginError::Message("could not read state"))?;
        state::load(&self.shared.parameters, &bytes).map_err(|_| PluginError::Message("invalid native state"))?;
        if let Some(params) = self.host.get_extension::<HostParams>() {
            params.rescan(&self.host, ParamRescanFlags::VALUES);
        }
        Ok(())
    }
}

impl<'a> PluginAudioProcessor<'a, Shared, MainThread<'a>> for AudioProcessor<'a> {
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main: &MainThread<'a>,
        shared: &'a Shared,
        config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        let frames = config.max_frames_count as usize;
        let processor = Processor::new(&shared.parameters, config.sample_rate, frames)
            .map_err(|_| PluginError::Message("invalid processing configuration"))?;
        shared.latency.store(processor.latency(), Ordering::Relaxed);
        Ok(Self { shared, processor, buffer: vec![0.0; frames * 2], events: Vec::with_capacity(MAX_EVENTS) })
    }
    fn reset(&mut self) {
        self.processor.reset();
    }
    fn process(&mut self, process: Process, mut audio: Audio, events: Events) -> Result<ProcessStatus, PluginError> {
        let result = self.process_audio(process, &mut audio, events);
        if result.is_err() {
            self.processor.reset();
            for mut port in &mut audio {
                if let Ok(channels) = port.channels()
                    && let Some(pairs) = channels.into_f32()
                {
                    for pair in pairs {
                        match pair {
                            ChannelPair::OutputOnly(output)
                            | ChannelPair::InputOutput(_, output)
                            | ChannelPair::InPlace(output) => output.fill(0.0),
                            ChannelPair::InputOnly(_) => {}
                        }
                    }
                }
            }
        }
        result
    }
}

impl AudioProcessor<'_> {
    fn process_audio(
        &mut self,
        process: Process,
        audio: &mut Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        if events.input.len() as usize > MAX_EVENTS {
            return Err(PluginError::Message("too many events"));
        }
        let frames = audio.frames_count() as usize;
        let Some(buffer) = self.buffer.get_mut(..frames * 2) else {
            return Err(PluginError::Message("block too large"));
        };
        buffer.fill(0.0);
        for (port_index, mut port) in (&mut *audio).into_iter().enumerate() {
            if port_index != 0 {
                continue;
            }
            let Some(pairs) = port.channels()?.into_f32() else {
                return Err(PluginError::Message("32-bit audio required"));
            };
            for (channel, pair) in pairs.into_iter().enumerate() {
                if channel >= 2 {
                    return Err(PluginError::Message("stereo audio required"));
                }
                let input = match pair {
                    ChannelPair::InputOnly(input) | ChannelPair::InputOutput(input, _) => Some(input),
                    ChannelPair::InPlace(input) => Some(&*input),
                    ChannelPair::OutputOnly(_) => None,
                };
                if let Some(input) = input {
                    for (frame, value) in buffer.as_chunks_mut::<2>().0.iter_mut().zip(input) {
                        if let Some(destination) = frame.get_mut(channel) {
                            *destination = *value;
                        }
                    }
                }
            }
        }
        self.events.clear();
        for event in events.input {
            let kind = match event.as_core_event() {
                Some(CoreEventSpace::ParamValue(value)) => {
                    value.param_id().map(|id| EventKind::Parameter { id: id.get(), value: value.value() })
                }
                Some(CoreEventSpace::NoteOn(note)) => note
                    .pckn()
                    .key
                    .into_specific()
                    .and_then(|key| u8::try_from(key).ok())
                    .map(|key| EventKind::Note(NoteEvent::On { key, velocity: note.velocity() as f32 })),
                Some(CoreEventSpace::NoteOff(note)) => note
                    .pckn()
                    .key
                    .into_specific()
                    .and_then(|key| u8::try_from(key).ok())
                    .map(|key| EventKind::Note(NoteEvent::Off { key, velocity: note.velocity() as f32 })),
                Some(CoreEventSpace::Midi(midi)) => midi_note(midi.data()).map(EventKind::Note),
                _ => None,
            };
            if let Some(kind) = kind {
                if self.events.len() == MAX_EVENTS {
                    self.processor.reset();
                    return Err(PluginError::Message("too many events"));
                }
                self.events.push(NativeEvent { frame: event.header().time() as usize, kind });
            }
        }
        let context = process
            .transport
            .map(|transport| ProcessContext {
                tempo: if transport.flags.contains(TransportFlags::HAS_TEMPO) { transport.tempo } else { 120.0 },
                beat: if transport.flags.contains(TransportFlags::HAS_BEATS_TIMELINE) {
                    transport.song_pos_beats.to_float()
                } else {
                    0.0
                },
                playing: transport.flags.contains(TransportFlags::IS_PLAYING),
            })
            .unwrap_or_default();
        self.processor
            .process(&self.shared.parameters, context, buffer, None, &self.events)
            .map_err(|_| PluginError::Message("invalid native processing block"))?;
        for (port_index, mut port) in (&mut *audio).into_iter().enumerate() {
            if port_index != 0 {
                continue;
            }
            if let Some(pairs) = port.channels()?.into_f32() {
                for (channel, pair) in pairs.into_iter().enumerate() {
                    let output = match pair {
                        ChannelPair::OutputOnly(output)
                        | ChannelPair::InputOutput(_, output)
                        | ChannelPair::InPlace(output) => Some(output),
                        ChannelPair::InputOnly(_) => None,
                    };
                    if let Some(output) = output {
                        for (value, frame) in output.iter_mut().zip(buffer.as_chunks::<2>().0) {
                            *value = frame.get(channel).copied().unwrap_or(0.0);
                        }
                    }
                }
            }
        }
        Ok(ProcessStatus::Continue)
    }
}

fn midi_note([status, a, b]: [u8; 3]) -> Option<NoteEvent> {
    if a > 127 || b > 127 {
        return None;
    }
    let value = f32::from(b) / 127.0;
    match status & 0xf0 {
        0x90 => Some(NoteEvent::On { key: a, velocity: value }),
        0x80 => Some(NoteEvent::Off { key: a, velocity: value }),
        0xb0 if a == 120 || a == 123 => Some(NoteEvent::AllNotesOff),
        0xb0 => Some(NoteEvent::Controller { number: a, value }),
        0xd0 => Some(NoteEvent::Pressure(f32::from(a) / 127.0)),
        0xe0 => Some(NoteEvent::PitchBend((f32::from(a) + f32::from(b) * 128.0 - 8192.0) / 8192.0)),
        _ => None,
    }
}

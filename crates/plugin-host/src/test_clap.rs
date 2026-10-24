// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A CLAP plug-in built into the host for tests (`test:clap`): a gain, with its state (the gain
//! as 4 bytes, little-endian `f32`) and a reported latency of [`LATENCY`] frames. Loading it
//! through `clack-host` runs the host's whole CLAP path except opening a file, so the tests do
//! not need a plug-in installed. For the event path it also answers notes and MIDI: a note-on
//! adds its velocity to the first channel at its frame, and controller 7 sets the gain. The
//! gain is also its one parameter (id 0, 0 to 1), set through parameter events.
//!
//! `test:clap-mix` is the same plug-in, except that its first output channel is the mean of the
//! port's first two, so it shows whether its second input was filled.

use std::sync::atomic::{AtomicU32, Ordering};

use clack_extensions::latency::{PluginLatency, PluginLatencyImpl};
use clack_extensions::params::{
    ParamDisplayWriter, ParamInfo, ParamInfoFlags, ParamInfoWriter, PluginAudioProcessorParams, PluginMainThreadParams,
    PluginParams,
};
use clack_extensions::state::{PluginState, PluginStateImpl};
use clack_plugin::events::io::{InputEvents, OutputEvents};
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::prelude::*;
use clack_plugin::stream::{InputStream, OutputStream};
use clack_plugin::utils::Cookie;

/// Latency the test plug-in reports, in frames.
pub(crate) const LATENCY: u32 = 16;

pub(crate) type Entry = SinglePluginEntry<TestGain<false>>;

/// The entry of `test:clap-mix`.
pub(crate) type MixEntry = SinglePluginEntry<TestGain<true>>;

/// The gain plug-in; with `MIX`, its first output channel is the mean of the first two.
pub(crate) struct TestGain<const MIX: bool>;

pub(crate) struct GainShared {
    gain: AtomicU32,
    mix: bool,
}

impl PluginShared<'_> for GainShared {}

pub(crate) struct GainMainThread<'a> {
    shared: &'a GainShared,
}

impl<'a> PluginMainThread<'a, GainShared> for GainMainThread<'a> {}

impl<const MIX: bool> Plugin for TestGain<MIX> {
    type AudioProcessor<'a> = GainProcessor<'a>;
    type Shared<'a> = GainShared;
    type MainThread<'a> = GainMainThread<'a>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _shared: Option<&GainShared>) {
        builder.register::<PluginState>().register::<PluginLatency>().register::<PluginParams>();
    }
}

impl<const MIX: bool> DefaultPluginFactory for TestGain<MIX> {
    fn get_descriptor() -> PluginDescriptor {
        if MIX {
            PluginDescriptor::new("org.suhina.test.gain-mix", "Suhina Test Gain Mix")
        } else {
            PluginDescriptor::new("org.suhina.test.gain", "Suhina Test Gain")
        }
    }

    fn new_shared(_host: HostSharedHandle<'_>) -> Result<GainShared, PluginError> {
        Ok(GainShared { gain: AtomicU32::new(0.5f32.to_bits()), mix: MIX })
    }

    fn new_main_thread<'a>(
        _host: HostMainThreadHandle<'a>,
        shared: &'a GainShared,
    ) -> Result<GainMainThread<'a>, PluginError> {
        Ok(GainMainThread { shared })
    }
}

impl PluginStateImpl for GainMainThread<'_> {
    fn save(&self, output: &mut OutputStream) -> Result<(), PluginError> {
        use std::io::Write;
        let gain = f32::from_bits(self.shared.gain.load(Ordering::Relaxed));
        output.write_all(&gain.to_le_bytes()).map_err(|_| PluginError::Message("could not write the state"))
    }

    fn load(&self, input: &mut InputStream) -> Result<(), PluginError> {
        use std::io::Read;
        let mut bytes = [0u8; 4];
        input.read_exact(&mut bytes).map_err(|_| PluginError::Message("the state is 4 bytes"))?;
        let gain = f32::from_le_bytes(bytes);
        if !gain.is_finite() {
            return Err(PluginError::Message("the gain is not a finite number"));
        }
        self.shared.gain.store(gain.to_bits(), Ordering::Relaxed);
        Ok(())
    }
}

impl PluginLatencyImpl for GainMainThread<'_> {
    fn get(&self) -> u32 {
        LATENCY
    }
}

/// The id of its one parameter, the gain.
const GAIN_PARAM: ClapId = ClapId::new(0);

impl PluginMainThreadParams for GainMainThread<'_> {
    fn count(&self) -> u32 {
        1
    }

    fn get_info(&self, index: u32, info: &mut ParamInfoWriter) {
        if index == 0 {
            info.set(&ParamInfo {
                id: GAIN_PARAM,
                flags: ParamInfoFlags::IS_AUTOMATABLE,
                cookie: Cookie::empty(),
                name: b"Gain",
                module: b"",
                min_value: 0.0,
                max_value: 1.0,
                default_value: 0.5,
            });
        }
    }

    fn get_value(&self, id: ClapId) -> Option<f64> {
        (id == GAIN_PARAM).then(|| f64::from(f32::from_bits(self.shared.gain.load(Ordering::Relaxed))))
    }

    fn value_to_text(&self, id: ClapId, value: f64, writer: &mut ParamDisplayWriter) -> std::fmt::Result {
        if id != GAIN_PARAM {
            return Err(std::fmt::Error);
        }
        use std::fmt::Write;
        write!(writer, "{:.0} %", value * 100.0)
    }

    fn text_to_value(&self, _id: ClapId, _text: &std::ffi::CStr) -> Option<f64> {
        None
    }

    fn flush(&self, _input: &InputEvents, _output: &mut OutputEvents) {}
}

impl PluginAudioProcessorParams for GainProcessor<'_> {
    fn flush(&mut self, _input: &InputEvents, _output: &mut OutputEvents) {}
}

pub(crate) struct GainProcessor<'a> {
    shared: &'a GainShared,
}

impl<'a> PluginAudioProcessor<'a, GainShared, GainMainThread<'a>> for GainProcessor<'a> {
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main_thread: &GainMainThread<'a>,
        shared: &'a GainShared,
        _config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        Ok(Self { shared })
    }

    fn process(&mut self, _process: Process, mut audio: Audio, events: Events) -> Result<ProcessStatus, PluginError> {
        // Controller 7 sets the gain before the block (a test needs no finer timing).
        let mut notes = [(0u32, 0.0f32); 16];
        let mut note_count = 0;
        for event in events.input {
            match event.as_core_event() {
                Some(CoreEventSpace::Midi(midi)) => {
                    if let [status, 7, value] = midi.data()
                        && status & 0xF0 == 0xB0
                    {
                        self.shared.gain.store((f32::from(value) / 127.0).to_bits(), Ordering::Relaxed);
                    }
                    // Channel pressure (aftertouch, #812) sets the gain too: a two-byte message
                    // in the MIDI event's three-byte data, the third byte unused.
                    if let [status, value, _] = midi.data()
                        && status & 0xF0 == 0xD0
                    {
                        self.shared.gain.store((f32::from(value) / 127.0).to_bits(), Ordering::Relaxed);
                    }
                }
                Some(CoreEventSpace::ParamValue(param)) => {
                    if param.param_id() == Some(GAIN_PARAM) && param.value().is_finite() {
                        self.shared.gain.store((param.value().clamp(0.0, 1.0) as f32).to_bits(), Ordering::Relaxed);
                    }
                }
                Some(CoreEventSpace::NoteOn(note)) => {
                    if let Some(slot) = notes.get_mut(note_count) {
                        *slot = (note.header().time(), note.velocity() as f32);
                        note_count += 1;
                    }
                }
                _ => {}
            }
        }
        let gain = f32::from_bits(self.shared.gain.load(Ordering::Relaxed));
        for mut port in &mut audio {
            let Some(pairs) = port.channels()?.into_f32() else { continue };
            for pair in pairs {
                match pair {
                    ChannelPair::InputOnly(_) => {}
                    ChannelPair::OutputOnly(out) => out.fill(0.0),
                    ChannelPair::InputOutput(input, out) => {
                        for (o, i) in out.iter_mut().zip(input) {
                            *o = i * gain;
                        }
                    }
                    ChannelPair::InPlace(buffer) => buffer.iter_mut().for_each(|s| *s *= gain),
                }
            }
            if self.shared.mix
                && let Some(mut pairs) = port.channels()?.into_f32()
                && let (Some(first), Some(second)) = (pairs.channel_pair(0), pairs.channel_pair(1))
                && let (Some(first), Some(second)) = (written(first), written(second))
            {
                for (a, b) in first.iter_mut().zip(second.iter()) {
                    *a = (*a + *b) / 2.0;
                }
            }
            // Notes on the port's first channel.
            if let Some(Some(first)) = port.channels()?.into_f32().map(|mut c| c.channel_pair(0)) {
                let mut out = match first {
                    ChannelPair::OutputOnly(out) | ChannelPair::InputOutput(_, out) => Some(out),
                    ChannelPair::InPlace(buffer) => Some(buffer),
                    ChannelPair::InputOnly(_) => None,
                };
                for (frame, velocity) in notes.iter().take(note_count) {
                    if let Some(sample) = out.as_deref_mut().and_then(|o| o.get_mut(*frame as usize)) {
                        *sample += velocity;
                    }
                }
            }
        }
        Ok(ProcessStatus::Continue)
    }
}

/// The output a channel pair was processed into, if it has one.
fn written(pair: ChannelPair<'_, f32>) -> Option<&mut [f32]> {
    match pair {
        ChannelPair::OutputOnly(out) | ChannelPair::InputOutput(_, out) | ChannelPair::InPlace(out) => Some(out),
        ChannelPair::InputOnly(_) => None,
    }
}

// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every built-in effect or instrument by its stable id (ADR 0004): what the model stores as
//! `DeviceKind::Builtin(id)` turns into a running device here. The engine builds device chains
//! through this crate; front ends list what can be added.

use std::sync::{Arc, OnceLock};

use suhina_device_api::{Device, DeviceInfo};
use suhina_device_sdk::DeviceSdkError;
use suhina_device_sdk::builtin::{self, Layout, Panel};

/// One built-in device: everything the rest of Suhina asks the registry about it, in one row
/// (#764 › 9). [`DEVICES`], [`create`], [`layout`], [`panel`] and [`is_file_device`] read this
/// table, so a new device is one row here.
pub struct Entry {
    pub info: &'static DeviceInfo,
    /// A new device at its defaults.
    make: fn() -> Box<dyn Device>,
    /// Where its panel's layout comes from.
    layout: LayoutSource,
    /// It runs a file whose path its `Device::state` holds (docs/spec/nam-amp-and-cab.md).
    pub runs_a_file: bool,
}

/// Where a device's panel layout comes from (ADR 0021 › Amendment: device panels).
enum LayoutSource {
    /// None: the generated parameter list.
    Generated,
    /// `devices/<name>/panel.json`.
    Json(&'static str),
    /// Built in Rust by the device crate.
    Rust(fn() -> Layout),
}

/// The built-in effects, instruments and MIDI effects, in the order a device browser lists them.
pub const ENTRIES: [Entry; 16] = [
    Entry {
        info: &suhina_device_fm_synth::INFO,
        make: || Box::new(suhina_device_fm_synth::FmSynth::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/fm-synth/panel.json")),
        runs_a_file: false,
    },
    Entry {
        info: &suhina_device_sampler::INFO,
        make: || Box::new(suhina_device_sampler::Sampler::new()),
        layout: LayoutSource::Rust(suhina_device_sampler::layout),
        runs_a_file: false,
    },
    Entry {
        info: &suhina_device_utility::INFO,
        make: || Box::new(suhina_device_utility::Utility::new()),
        layout: LayoutSource::Generated,
        runs_a_file: false,
    },
    Entry {
        info: &suhina_device_auto_pan::INFO,
        make: || Box::new(suhina_device_auto_pan::AutoPan::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/auto-pan/panel.json")),
        runs_a_file: false,
    },
    Entry {
        info: &suhina_device_dj_eq::INFO,
        make: || Box::new(suhina_device_dj_eq::DjEq::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/dj-eq/panel.json")),
        runs_a_file: false,
    },
    Entry {
        info: &suhina_device_eq::INFO,
        make: || Box::new(suhina_device_eq::Eq::new()),
        layout: LayoutSource::Generated,
        runs_a_file: false,
    },
    Entry {
        info: &suhina_device_vinyl_distortion::INFO,
        make: || Box::new(suhina_device_vinyl_distortion::VinylDistortion::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/vinyl-distortion/panel.json")),
        runs_a_file: false,
    },
    Entry {
        info: &suhina_device_overdrive::INFO,
        make: || Box::new(suhina_device_overdrive::Overdrive::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/overdrive/panel.json")),
        runs_a_file: false,
    },
    Entry {
        info: &suhina_device_saturator::INFO,
        make: || Box::new(suhina_device_saturator::Saturator::new()),
        layout: LayoutSource::Generated,
        runs_a_file: false,
    },
    Entry {
        info: &suhina_device_compressor::INFO,
        make: || Box::new(suhina_device_compressor::Compressor::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/compressor/panel.json")),
        runs_a_file: false,
    },
    Entry {
        info: &suhina_device_limiter::INFO,
        make: || Box::new(suhina_device_limiter::Limiter::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/limiter/panel.json")),
        runs_a_file: false,
    },
    Entry {
        info: &suhina_device_nam_amp_modeler::INFO,
        make: || Box::new(suhina_device_nam_amp_modeler::NamAmp::new()),
        layout: LayoutSource::Generated,
        runs_a_file: true,
    },
    Entry {
        info: &suhina_device_nam_cab_loader::INFO,
        make: || Box::new(suhina_device_nam_cab_loader::CabLoader::new()),
        layout: LayoutSource::Generated,
        runs_a_file: true,
    },
    Entry {
        info: &suhina_device_delay::INFO,
        make: || Box::new(suhina_device_delay::Delay::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/delay/panel.json")),
        runs_a_file: false,
    },
    Entry {
        info: &suhina_device_reverb::INFO,
        make: || Box::new(suhina_device_reverb::Reverb::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/reverb/panel.json")),
        runs_a_file: false,
    },
    Entry {
        info: &suhina_device_pitch::INFO,
        make: || Box::new(suhina_device_pitch::Pitch::new()),
        layout: LayoutSource::Json(include_str!("../../../devices/pitch/panel.json")),
        runs_a_file: false,
    },
];

/// What each entry is, in the order of [`ENTRIES`].
pub const DEVICES: [&DeviceInfo; ENTRIES.len()] = {
    let mut out = [ENTRIES[0].info; ENTRIES.len()];
    let mut i = 0;
    while i < ENTRIES.len() {
        out[i] = ENTRIES[i].info;
        i += 1;
    }
    out
};

fn entry(id: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|entry| entry.info.id == id)
}

/// What the device with this id is, or `None` for an unknown id.
pub fn info(id: &str) -> Option<&'static DeviceInfo> {
    entry(id).map(|entry| entry.info)
}

/// The layout of the device with this id, when it has one (ADR 0021 › Amendment: device panels
/// for built-in devices): the `.sd` UI part only, its parameters named by id. A device crate
/// gives it in Rust, or as `devices/<name>/panel.json`.
pub fn layout(id: &str) -> Option<Result<Layout, DeviceSdkError>> {
    match entry(id)?.layout {
        LayoutSource::Generated => None,
        LayoutSource::Json(text) => Some(Layout::from_json(text)),
        LayoutSource::Rust(make) => Some(Ok(make())),
    }
}

/// The ids of the devices whose layout is a `panel.json`, with its text.
pub fn layout_files() -> impl Iterator<Item = (&'static str, &'static str)> {
    ENTRIES.iter().filter_map(|entry| match entry.layout {
        LayoutSource::Json(text) => Some((entry.info.id, text)),
        LayoutSource::Generated | LayoutSource::Rust(_) => None,
    })
}

/// The panel of the device with this id: its parameters from its `DeviceInfo`, laid out by its
/// layout, or the generated parameter list when it has none or its layout does not load.
/// Made once per id while Suhina runs, off the audio thread. `None` for an unknown id.
pub fn panel(id: &str) -> Option<&'static Panel> {
    /// Every device's panel, in the order of [`DEVICES`].
    static PANELS: OnceLock<Vec<Panel>> = OnceLock::new();
    let index = DEVICES.iter().position(|info| info.id == id)?;
    PANELS
        .get_or_init(|| {
            DEVICES
                .iter()
                .map(|info| {
                    layout(info.id)
                        .and_then(Result::ok)
                        .and_then(|layout| layout.panel(info).ok())
                        .unwrap_or_else(|| builtin::generated(info))
                })
                .collect()
        })
        .get(index)
}

/// A new device with this id at its defaults, prepared for 48 kHz, or `None` for an unknown
/// id. Allocates: call it off the audio thread.
pub fn create(id: &str) -> Option<Box<dyn Device>> {
    entry(id).map(|entry| (entry.make)())
}

/// The file a file-backed device runs, already read and parsed or decoded (docs/spec/nam-amp-and-cab.md):
/// a model for the Amp Modeler, an impulse response for the Cab Loader. The session keeps a
/// map of these by the file's path; the engine hands the device the map's entry when it builds
/// its chains.
#[derive(Debug, Clone)]
pub enum DeviceAsset {
    Nam(Arc<suhina_device_nam_amp_modeler::NamModel>),
    Ir(Arc<suhina_dsp::Ir>),
}

/// The ids whose devices run a file stored in `Device::state` (docs/spec/nam-amp-and-cab.md).
pub fn is_file_device(id: &str) -> bool {
    entry(id).is_some_and(|entry| entry.runs_a_file)
}

/// A new device with this id, its defaults, and `asset` already loaded — `None` for an id that
/// takes no file, or for an unknown id. Allocation: off the audio thread.
pub fn create_with(id: &str, asset: Option<&DeviceAsset>) -> Option<Box<dyn Device>> {
    match (id, asset) {
        ("nam-amp-modeler", Some(DeviceAsset::Nam(model))) => {
            Some(Box::new(suhina_device_nam_amp_modeler::NamAmp::with_model(Arc::clone(model))))
        }
        ("nam-cab-loader", Some(DeviceAsset::Ir(ir))) => {
            Some(Box::new(suhina_device_nam_cab_loader::CabLoader::with_ir((**ir).clone())))
        }
        _ => create(id),
    }
}

/// Largest preset file read.
pub const MAX_PRESET_BYTES: usize = 64 * 1024;

/// A device's values from a preset file (`docs/spec/browser.md` › Built-in effects and instruments, presets
/// and plug-ins).
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    /// The device it is for.
    pub device: &'static str,
    /// The preset's own name (a DX7 voice's name).
    pub name: String,
    /// Every parameter by its id, with the preset's value.
    pub params: Vec<(&'static str, f64)>,
}

/// The voices a preset file holds: a DX7 single voice (1) or bank (32). Each is `(number from
/// 1, name)`.
pub fn preset_voices(data: &[u8]) -> Result<Vec<(usize, String)>, String> {
    let voices = read_voices(data)?;
    Ok(voices.patches.iter().enumerate().map(|(i, p)| (i + 1, p.name().trim().to_string())).collect())
}

/// The preset `voice` (from 1) of a preset file: DX7 voice data for the FM synth.
pub fn preset(data: &[u8], voice: usize) -> Result<Preset, String> {
    let voices = read_voices(data)?;
    let count = voices.patches.len();
    let patch = voice
        .checked_sub(1)
        .and_then(|i| voices.patches.get(i))
        .ok_or_else(|| format!("voice {voice} is not in the file; it holds {count}"))?;
    let mut synth = suhina_device_fm_synth::FmSynth::new();
    synth.load_patch(patch);
    let info = &suhina_device_fm_synth::INFO;
    let params =
        info.params.iter().enumerate().filter_map(|(i, p)| synth.param(i).map(|value| (p.id, value))).collect();
    Ok(Preset { device: info.id, name: patch.name().trim().to_string(), params })
}

fn read_voices(data: &[u8]) -> Result<suhina_device_fm_synth::patch::Voices, String> {
    if data.len() > MAX_PRESET_BYTES {
        return Err(format!("a preset file is at most {} KiB", MAX_PRESET_BYTES / 1024));
    }
    suhina_device_fm_synth::patch::read(data).map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

/// Prepare a data-only graph using only registered, zero-latency built-in effects.
/// Unknown ids fail before the graph reaches the audio thread.
pub fn compile_graph(
    graph: &suhina_device_sdk::DeviceGraph,
    rate: f64,
) -> Result<suhina_device_sdk::PreparedEffect, suhina_device_sdk::DeviceSdkError> {
    compile_graph_with_samples(graph, rate, &Default::default())
}

/// Prepare graph resources supplied by the host; decoding and retirement stay off-thread.
pub fn compile_graph_with_samples(
    graph: &suhina_device_sdk::DeviceGraph,
    rate: f64,
    samples: &suhina_device_sdk::sample_resource::PreparedSamples,
) -> Result<suhina_device_sdk::PreparedEffect, suhina_device_sdk::DeviceSdkError> {
    graph.compile_with_samples_and_native_at(rate, samples, |id, rate| {
        let mut device =
            create(id).ok_or_else(|| suhina_device_sdk::DeviceSdkError(format!("unknown graph effect {id:?}")))?;
        if device.info().role != suhina_device_api::Role::Effect {
            return Err(suhina_device_sdk::DeviceSdkError(format!("graph kernel {id:?} is not an audio effect")));
        }
        device.prepare(rate).map_err(|e| suhina_device_sdk::DeviceSdkError(e.to_string()))?;
        if device.latency() != 0 {
            return Err(suhina_device_sdk::DeviceSdkError(
                "graph kernels with latency require path compensation".into(),
            ));
        }
        Ok(Box::new(GraphEffect { device, context: Default::default(), rate }))
    })
}

struct GraphEffect {
    device: Box<dyn Device>,
    context: suhina_device_api::ProcessContext,
    rate: f64,
}
impl suhina_device_sdk::native_node::NativeNode for GraphEffect {
    fn prepare_block(&mut self, context: &suhina_device_api::ProcessContext) {
        self.context = *context;
        self.device.set_context(context);
    }
    fn process(&mut self, inputs: [&[f32]; 5], output: &mut [f32]) {
        output.copy_from_slice(inputs[0]);
        self.device.process(&self.context, output);
        if self.context.playing {
            self.context.beat += (output.len() / 2) as f64 * self.context.tempo / (60.0 * self.rate);
        }
    }
    fn connected_inputs(&mut self, mask: [bool; 5]) -> Result<(), suhina_device_sdk::DeviceSdkError> {
        if mask[1..].iter().any(|&v| v) {
            return Err(suhina_device_sdk::DeviceSdkError(
                "registered effect control bindings are not implemented".into(),
            ));
        }
        Ok(())
    }
    fn parameter_range(&self, id: &str) -> Option<(f64, f64)> {
        self.device.info().params.iter().find(|p| p.id == id).map(|p| (p.min, p.max))
    }
    fn set_parameter(&mut self, id: &str, value: f64) {
        if let Some(index) = self.device.info().param_index(id) {
            self.device.set_param(index, value);
        }
    }
    fn event(&mut self, event: suhina_device_api::NoteEvent) {
        self.device.event(event);
    }
    fn reset(&mut self) {
        self.device.reset();
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod graph_kernel_tests {
    use super::*;
    use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, Node, NodeKind};
    #[test]
    fn stored_graph_resolves_registered_effect_and_rejects_unknown() {
        let mut graph = DeviceGraph::new("org.test.registry", "Registry", DeviceRole::Effect);
        graph.nodes = vec![
            Node::audio_input("in"),
            Node::new("fx", NodeKind::NativeEffect { effect: "utility".into() }),
            Node::audio_output("out"),
        ];
        graph.connections = vec![Connection::new("in", "fx"), Connection::new("fx", "out")];
        let graph = DeviceGraph::from_sd(&graph.to_sd().unwrap()).unwrap();
        let mut prepared = compile_graph(&graph, 48000.).unwrap();
        let mut audio = [0.25, -0.5, 0.125, 0.75];
        prepared.process(&mut audio);
        assert!(audio.iter().all(|v| v.is_finite()));
        assert!(audio.iter().any(|v| v.abs() > 0.01));
        let mut controlled = graph.clone();
        controlled.connections.push(Connection::ports("in", "out", "fx", "control1"));
        assert!(compile_graph(&controlled, 48000.).is_err());
        let mut nested = DeviceGraph::new("org.test.nested", "Nested", DeviceRole::Effect);
        nested.nodes = vec![
            Node::audio_input("in"),
            Node::new(
                "child",
                NodeKind::Component { name: "Hidden effect".into(), version: 1, graph: controlled.to_sd().unwrap() },
            ),
            Node::audio_output("out"),
        ];
        nested.connections = vec![Connection::new("in", "child"), Connection::new("child", "out")];
        assert!(compile_graph(&nested, 48000.).is_err());

        for id in ["missing", "sampler", "limiter"] {
            let mut invalid = graph.clone();
            invalid.nodes[1].kind = NodeKind::NativeEffect { effect: id.into() };
            assert!(compile_graph(&invalid, 48000.).is_err(), "{id}");
        }
    }
}

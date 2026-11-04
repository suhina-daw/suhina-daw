// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every built-in effect or instrument by its stable id (ADR 0004): what the model stores as
//! `DeviceKind::Builtin(id)` turns into a running device here. The engine builds device chains
//! through this crate; front ends list what can be added.

use std::sync::{Arc, OnceLock};

use suhina_device_api::{Device, DeviceInfo};
use suhina_device_sdk::DeviceSdkError;
use suhina_device_sdk::builtin::{self, Layout, Panel};

/// The built-in effects, instruments and MIDI effects, in the order a device browser lists them.
pub const DEVICES: [&DeviceInfo; 17] = [
    &suhina_device_fm_synth::INFO,
    &suhina_device_sampler::INFO,
    &suhina_device_utility::INFO,
    &suhina_device_auto_pan::INFO,
    &suhina_device_dj_eq::INFO,
    &suhina_device_eq::INFO,
    &suhina_device_vinyl_distortion::INFO,
    &suhina_device_overdrive::INFO,
    &suhina_device_saturator::INFO,
    &suhina_device_compressor::INFO,
    &suhina_device_multiband::INFO,
    &suhina_device_limiter::INFO,
    &suhina_device_nam_amp_modeler::INFO,
    &suhina_device_nam_cab_loader::INFO,
    &suhina_device_delay::INFO,
    &suhina_device_reverb::INFO,
    &suhina_device_pitch::INFO,
];

/// What the device with this id is, or `None` for an unknown id.
pub fn info(id: &str) -> Option<&'static DeviceInfo> {
    DEVICES.iter().copied().find(|info| info.id == id)
}

/// The layout of the device with this id, when it has one (ADR 0021 › Amendment: device panels
/// for built-in devices): the `.sd` UI part only, its parameters named by id. A device crate
/// gives it in Rust, or as `devices/<name>/panel.json` listed in [`LAYOUT_FILES`].
pub fn layout(id: &str) -> Option<Result<Layout, DeviceSdkError>> {
    if id == "sampler" {
        return Some(Ok(suhina_device_sampler::layout()));
    }
    LAYOUT_FILES.iter().find(|(file_id, _)| *file_id == id).map(|(_, text)| Layout::from_json(text))
}

/// The layouts kept as JSON next to a device crate, by device id.
pub const LAYOUT_FILES: &[(&str, &str)] = &[
    ("compressor", include_str!("../../../devices/compressor/panel.json")),
    ("multiband", include_str!("../../../devices/multiband/panel.json")),
    ("overdrive", include_str!("../../../devices/overdrive/panel.json")),
    ("reverb", include_str!("../../../devices/reverb/panel.json")),
    ("limiter", include_str!("../../../devices/limiter/panel.json")),
    ("dj-eq", include_str!("../../../devices/dj-eq/panel.json")),
    ("auto-pan", include_str!("../../../devices/auto-pan/panel.json")),
    ("vinyl-distortion", include_str!("../../../devices/vinyl-distortion/panel.json")),
    ("delay", include_str!("../../../devices/delay/panel.json")),
    ("pitch", include_str!("../../../devices/pitch/panel.json")),
    ("fm-synth", include_str!("../../../devices/fm-synth/panel.json")),
];

/// What a device just added to a project starts with beside its parameters' defaults.
pub fn new_device_params(id: &str) -> &'static [(&'static str, f64)] {
    if id == suhina_device_fm_synth::INFO.id { &suhina_device_fm_synth::params::NEW_DEVICE } else { &[] }
}

/// New values for a device's randomizer from its current `values`, in parameter order: which
/// parameters change and to what. `None` for a device without a randomizer.
pub fn randomize(id: &str, values: &[f64], seed: Option<u32>, amount: Option<f64>) -> Option<Vec<(usize, f64)>> {
    (id == suhina_device_fm_synth::INFO.id).then(|| suhina_device_fm_synth::random::randomize(values, seed, amount))
}

/// The panel of the device with this id: its parameters from its `DeviceInfo`, laid out by its
/// layout, or the generated parameter list when it has none or its layout does not load.
/// Made once per id while Suhina runs, off the audio thread. `None` for an unknown id.
/// The layouts that do not load, by device id with the reason: such a device shows the generated
/// parameter list instead, so the app writes these to its log at start.
pub fn layout_errors() -> Vec<(&'static str, String)> {
    DEVICES
        .iter()
        .filter_map(|info| match layout(info.id)? {
            Err(e) => Some((info.id, e.to_string())),
            Ok(layout) => layout.panel(info).err().map(|e| (info.id, e.to_string())),
        })
        .collect()
}

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
    Some(match id {
        "sampler" => Box::new(suhina_device_sampler::Sampler::new()),
        "fm-synth" => Box::new(suhina_device_fm_synth::FmSynth::new()),
        "utility" => Box::new(suhina_device_utility::Utility::new()),
        "auto-pan" => Box::new(suhina_device_auto_pan::AutoPan::new()),
        "dj-eq" => Box::new(suhina_device_dj_eq::DjEq::new()),
        "eq" => Box::new(suhina_device_eq::Eq::new()),
        "vinyl-distortion" => Box::new(suhina_device_vinyl_distortion::VinylDistortion::new()),
        "overdrive" => Box::new(suhina_device_overdrive::Overdrive::new()),
        "saturator" => Box::new(suhina_device_saturator::Saturator::new()),
        "compressor" => Box::new(suhina_device_compressor::Compressor::new()),
        "multiband" => Box::new(suhina_device_multiband::Multiband::new()),
        "limiter" => Box::new(suhina_device_limiter::Limiter::new()),
        "nam-amp-modeler" => Box::new(suhina_device_nam_amp_modeler::NamAmp::new()),
        "nam-cab-loader" => Box::new(suhina_device_nam_cab_loader::CabLoader::new()),
        "delay" => Box::new(suhina_device_delay::Delay::new()),
        "reverb" => Box::new(suhina_device_reverb::Reverb::new()),
        "pitch" => Box::new(suhina_device_pitch::Pitch::new()),
        _ => return None,
    })
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
    matches!(id, "nam-amp-modeler" | "nam-cab-loader")
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

/// A preset that ships with Suhina (ADR 0014): a file of `presets/` built into the executable.
#[derive(Debug, Clone, PartialEq)]
pub struct StockPreset {
    /// What names it in a browser item, `stock:<key>`, and in `device.add`: `<device id>/<name>`. It
    /// stays when the shown name changes, as pins and recent items keep it.
    pub key: &'static str,
    pub tags: Vec<String>,
    pub preset: Preset,
}

/// Every stock preset's key and file.
const STOCK_FILES: &[(&str, &str)] = &[
    ("fm-synth/warm-fm-bass", include_str!("../../../presets/fm-synth/Warm FM Bass.fm-synth.preset.suhina")),
    ("fm-synth/drawbar-organ", include_str!("../../../presets/fm-synth/Drawbar Organ.fm-synth.preset.suhina")),
    ("fm-synth/struck-fm-keys", include_str!("../../../presets/fm-synth/Struck FM Keys.fm-synth.preset.suhina")),
    ("fm-synth/wide-trance-pad", include_str!("../../../presets/fm-synth/Wide Trance Pad.fm-synth.preset.suhina")),
    ("fm-synth/slow-warm-pad", include_str!("../../../presets/fm-synth/Slow Warm Pad.fm-synth.preset.suhina")),
];

/// The stock presets, read once. A file that does not read is left out; a test keeps there being none.
pub fn stock_presets() -> &'static [StockPreset] {
    static STOCK: OnceLock<Vec<StockPreset>> = OnceLock::new();
    STOCK.get_or_init(|| {
        STOCK_FILES
            .iter()
            .filter_map(|&(key, text)| {
                let (preset, tags) = read_preset_file(text.as_bytes()).ok()?;
                Some(StockPreset { key, tags, preset })
            })
            .collect()
    })
}

/// The stock preset with this key.
pub fn stock_preset(key: &str) -> Option<&'static StockPreset> {
    stock_presets().iter().find(|stock| stock.key == key)
}

/// A device preset's file (ADR 0014) as a preset and its tags. A parameter it does not name takes
/// its default, and one the device no longer has is left out.
fn read_preset_file(data: &[u8]) -> Result<(Preset, Vec<String>), String> {
    let file: serde_json::Value = serde_json::from_slice(data).map_err(|e| format!("not a preset file: {e}"))?;
    if file["format"] != "suhina-preset" {
        return Err("not a preset file".into());
    }
    if file["version"] != 1 {
        return Err("a preset file of a version this Suhina does not know".into());
    }
    let id = file["device"].as_str().ok_or("a preset file names its device")?;
    let info = info(id).ok_or_else(|| format!("a preset for {id:?}, which is not a built-in device"))?;
    let role = match info.role {
        suhina_device_api::Role::Effect => "effect",
        suhina_device_api::Role::MidiEffect => "midi_effect",
        suhina_device_api::Role::Instrument => "instrument",
    };
    if file["role"] != role {
        return Err(format!("{id:?} is an {role}, and the preset says otherwise"));
    }
    let name = file["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).ok_or("a preset file has a name")?;
    let values = file["params"].as_object().ok_or("a preset file has its parameters")?;
    let mut params = Vec::with_capacity(info.params.len());
    for param in info.params {
        let value = match values.get(param.id) {
            None => param.default,
            Some(value) => value.as_f64().ok_or_else(|| format!("{} is not a number", param.id))?,
        };
        if !(param.min..=param.max).contains(&value) {
            return Err(format!("{} is {value}, outside {} to {}", param.id, param.min, param.max));
        }
        params.push((param.id, value));
    }
    let tags = file["tags"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(String::from)).collect();
    Ok((Preset { device: info.id, name: name.to_string(), params }, tags))
}

/// Whether a preset file's data is a device preset's JSON rather than DX7 voice data.
fn is_preset_json(data: &[u8]) -> bool {
    data.trim_ascii_start().first() == Some(&b'{')
}

/// The voices a preset file holds: one for a device preset or a DX7 single voice, 32 for a DX7 bank.
/// Each is `(number from 1, name)`.
pub fn preset_voices(data: &[u8]) -> Result<Vec<(usize, String)>, String> {
    if is_preset_json(data) {
        check_preset_size(data)?;
        return Ok(vec![(1, read_preset_file(data)?.0.name)]);
    }
    let voices = read_voices(data)?;
    Ok(voices.patches.iter().enumerate().map(|(i, p)| (i + 1, p.name().trim().to_string())).collect())
}

/// The preset `voice` (from 1) of a preset file: a device preset (its only voice), or DX7 voice data
/// for the FM synth.
pub fn preset(data: &[u8], voice: usize) -> Result<Preset, String> {
    if is_preset_json(data) {
        check_preset_size(data)?;
        if voice != 1 {
            return Err(format!("voice {voice} is not in the file; it holds 1"));
        }
        return Ok(read_preset_file(data)?.0);
    }
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

fn check_preset_size(data: &[u8]) -> Result<(), String> {
    if data.len() > MAX_PRESET_BYTES {
        return Err(format!("a preset file is at most {} KiB", MAX_PRESET_BYTES / 1024));
    }
    Ok(())
}

fn read_voices(data: &[u8]) -> Result<suhina_device_fm_synth::patch::Voices, String> {
    check_preset_size(data)?;
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

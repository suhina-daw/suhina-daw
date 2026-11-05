// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`SlotFactory`]: the one place a stored device becomes a running slot (#764 › 4). A chain
//! builds the devices at the top of a track with it, and a group the devices in its branches, so
//! a device behaves the same in both: its sample, its file, its automation lanes, its parameters
//! clamped and overlaid on the defaults, and its identity for graph swaps.

use suhina_device_api::Role;
use suhina_model::DeviceKind;

use crate::automation::DeviceLanes;
use crate::chain::Slot;
use crate::plugin::{PluginDevice, PluginSlot, Plugins};
use crate::plugin_params::SlotParams;

/// What builds a slot for a stored device, at the top of a chain or inside a group: the sample
/// rate, the decoded audio a Sampler plays, the files of file-backed devices, the plug-ins (their
/// slots and processors, by the keys of the channel's plug-in devices) and the tempo the lanes of
/// automated parameters are read at.
pub(crate) struct SlotFactory<'a> {
    pub(crate) sample_rate: u32,
    pub(crate) audio: &'a crate::AudioPool,
    pub(crate) assets: &'a crate::DeviceAssets,
    pub(crate) plugins: &'a mut Plugins,
    pub(crate) keys: &'a [PluginDevice],
    pub(crate) channel: suhina_model::ChannelId,
    pub(crate) frames_per_beat: f64,
}

/// A slot built for a stored device: the slot, its identity for graph swaps, its parameter
/// values, its latency and its manual sample slices.
pub(crate) struct Built {
    pub(crate) slot: Slot,
    pub(crate) id: String,
    pub(crate) values: Vec<f64>,
    pub(crate) latency: u32,
    pub(crate) slices: Option<std::sync::Arc<[f64]>>,
    pub(crate) edits: Option<std::sync::Arc<suhina_device_api::sample_edits::SampleEdits>>,
}

impl SlotFactory<'_> {
    /// The slot of the device at `path` on the channel (`[position]` at the top of a chain), or
    /// `None` for one that does not run: switched off, unknown, not compiling, or a plug-in that
    /// is not installed. A switched-off plug-in keeps its slot, and its process, and passes the sound.
    /// `lanes` is the branch's, and `slot` the index the device takes there: the lanes of its
    /// automated parameters are added under it. Allocates.
    pub(crate) fn device(
        &mut self,
        stored: &suhina_model::Device,
        depth: usize,
        path: &[usize],
        (lanes, slot): (&mut DeviceLanes, usize),
    ) -> Option<Built> {
        if matches!(stored.kind, DeviceKind::Clap { .. } | DeviceKind::Vst3 { .. } | DeviceKind::Vst2 { .. }) {
            return self.plugin(stored, path);
        }
        if !stored.enabled {
            return None;
        }
        match &stored.kind {
            DeviceKind::Builtin(id) => self.builtin(id, stored, lanes, slot),
            DeviceKind::Sd => self.sd(stored, lanes, slot),
            DeviceKind::Group(group) => self.group(stored, group, depth, path),
            _ => None,
        }
    }

    /// A plug-in's slot, by its key: it takes its processor from the plug-ins, or waits for one.
    fn plugin(&mut self, stored: &suhina_model::Device, path: &[usize]) -> Option<Built> {
        let key = self.keys.iter().find(|d| d.channel == self.channel && d.path == path)?.key;
        let info = *self.plugins.info.get(&key)?;
        let slot = PluginSlot {
            key,
            processor: self.plugins.processors.remove(&key),
            instrument: info.instrument,
            latency: info.latency,
            enabled: stored.enabled,
            missed: 0,
            busy: 0,
            longest_us: 0,
            sent_at: None,
            params: SlotParams::new(stored, self.frames_per_beat),
        };
        let latency = if stored.enabled { info.latency } else { 0 };
        Some(Built {
            slot: Slot::Plugin(slot),
            id: format!("plugin:{key:016x}"),
            values: Vec::new(),
            latency,
            slices: None,
            edits: None,
        })
    }

    fn builtin(&self, id: &str, stored: &suhina_model::Device, lanes: &mut DeviceLanes, slot: usize) -> Option<Built> {
        let asset = crate::assets::stored_path(id, stored.state.as_deref()).and_then(|path| self.assets.get(path));
        let mut device = suhina_device_registry::create_with(id, asset)?;
        // What a chain gives its Sampler: the decoded sample and the manual slices.
        if let Some(source) = stored.sample_file().and_then(|file| self.audio.files.get(file)) {
            device.load_shared_sample(source.sample_rate, source.channels, source.clone());
        }
        let slices: Option<std::sync::Arc<[f64]>> = stored.sample_slices().map(std::sync::Arc::from);
        device.load_sample_slices(slices.clone());
        let edits = crate::assets::sample_edits(stored);
        device.load_sample_edits(edits.clone());
        device.prepare(f64::from(self.sample_rate)).ok()?;
        let info = device.info();
        let mut values: Vec<f64> = info.params.iter().map(|p| p.default).collect();
        for p in &stored.params {
            if let Some(slot) = info.param_index(&p.id).and_then(|i| values.get_mut(i)) {
                *slot = p.value;
            }
        }
        lanes.add(slot, device.as_ref(), &stored.automation, self.frames_per_beat);
        for (index, &value) in values.iter().enumerate() {
            // A parameter that follows a lane starts where the lane starts.
            device.set_param(index, lanes.initial(slot, index).unwrap_or(value));
        }
        device.reset();
        let latency = device.latency();
        // A built-in MIDI effect runs on the events, before the instrument (#678).
        let kind = if info.role == Role::MidiEffect { Slot::MidiDevice(device) } else { Slot::Device(device) };
        Some(Built {
            slot: kind,
            id: crate::chain::device_identity(id, stored, self.audio),
            values,
            latency,
            slices,
            edits,
        })
    }

    fn sd(&self, stored: &suhina_model::Device, lanes: &mut DeviceLanes, slot: usize) -> Option<Built> {
        let bytes = stored.state.as_deref()?;
        let text = std::str::from_utf8(bytes).ok()?;
        let graph = suhina_device_sdk::DeviceGraph::from_sd(text).ok()?;
        let mut samples = suhina_device_sdk::sample_resource::PreparedSamples::default();
        if let Some(source) = stored.sample_file().and_then(|file| self.audio.files.get(file)) {
            let asset = suhina_device_sdk::sample_resource::SampleAsset::new(
                source.clone(),
                source.sample_rate,
                source.channels,
            )
            .ok()?;
            samples.insert("sample", std::sync::Arc::new(asset)).ok()?;
        }
        let rate = f64::from(self.sample_rate);
        let mut built = match graph.role {
            suhina_device_sdk::DeviceRole::Effect => {
                Slot::Sd(suhina_device_registry::compile_graph_with_samples(&graph, rate, &samples).ok()?)
            }
            suhina_device_sdk::DeviceRole::MidiEffect => Slot::MidiSd(graph.compile_midi().ok()?),
            suhina_device_sdk::DeviceRole::Instrument => {
                Slot::InstrumentSd(suhina_device_registry::compile_graph_with_samples(&graph, rate, &samples).ok()?)
            }
        };
        lanes.add_sd(slot, &graph.parameters, &stored.automation, self.frames_per_beat);
        let mut values = Vec::with_capacity(graph.parameters.len());
        for (index, parameter) in graph.parameters.iter().enumerate() {
            let value = stored
                .params
                .iter()
                .find(|value| value.id == parameter.id)
                .map_or(parameter.default, |value| parameter.clamp(value.value));
            built.set_parameter(index, lanes.initial(slot, index).unwrap_or(value));
            values.push(value);
        }
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        stored.sample_file().hash(&mut hasher);
        stored.sample_file().and_then(|file| self.audio.files.get(file)).map(std::sync::Arc::as_ptr).hash(&mut hasher);
        Some(Built {
            slot: built,
            id: format!("sd:{}:{:016x}", graph.id, hasher.finish()),
            values,
            latency: 0,
            slices: None,
            edits: None,
        })
    }
}

// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sample source adapter; DSP remains in the shared sample instrument kernel.
use super::{NodeInfo, PortInfo};
use crate::native_node::NativeNode;
use crate::{DeviceSdkError, NodeKind, PreparedEffect, ProcessContext};
use std::any::Any;
use suhina_device_api::{Device, NoteEvent};
pub(crate) const SAMPLER: NodeInfo = NodeInfo {
    kind: "sampler",
    name: "Sampler",
    rust_variant: "Sampler",
    inputs: &[
        PortInfo { name: "pitch", default: Some(0.), unit: "semitones" },
        PortInfo { name: "cutoff", default: Some(20000.), unit: "Hz" },
        PortInfo { name: "pan", default: Some(0.), unit: "balance" },
        PortInfo { name: "position", default: Some(0.5), unit: "normalized" },
        PortInfo { name: "events", default: Some(0.), unit: "midi" },
    ],
    outputs: &["out", "left", "right"],
    description: "Polyphonic sample source using the shared Sampler kernel. Receives timestamped instrument events; bind sample data and parameters by node id before processing. Audio can feed any signal node.",
    configuration: "none; source and controls are bound through PreparedEffect",
    example: r#"{"kind":"sampler"}"#,
};
pub(crate) fn prepare(kind: &NodeKind, rate: f64) -> Result<Option<Box<dyn NativeNode>>, DeviceSdkError> {
    match kind {
        NodeKind::Sampler => {
            let mut kernel = Box::new(crate::sampler::Sampler::new());
            kernel.prepare(rate).map_err(|e| DeviceSdkError(e.to_string()))?;
            Ok(Some(Box::new(Adapter { kernel, connected: [false; 5] })))
        }
        _ => Ok(None),
    }
}

struct Adapter {
    kernel: Box<crate::sampler::Sampler>,
    connected: [bool; 5],
}
impl NativeNode for Adapter {
    fn prepare_block(&mut self, context: &ProcessContext) {
        self.kernel.prepare_block(context);
    }
    fn process(&mut self, inputs: [&[f32]; 5], buffer: &mut [f32]) {
        if !self.connected[..4].iter().any(|v| *v) {
            self.kernel.process_prepared(buffer);
            return;
        }
        let mut offset = 0;
        let frames = buffer.len() / 2;
        while offset < frames {
            let values: [f32; 4] = std::array::from_fn(|port| inputs[port].get(offset * 2).copied().unwrap_or(0.));
            for (port, id) in ["pitch", "cutoff", "pan", "position"].iter().enumerate() {
                if self.connected[port]
                    && let Some(index) = self.kernel.info().param_index(id)
                {
                    self.kernel.set_param(index, f64::from(values[port]));
                }
            }
            let mut end = offset + 1;
            while end < frames
                && (0..4).all(|port| {
                    !self.connected[port] || inputs[port].get(end * 2).copied().unwrap_or(0.) == values[port]
                })
            {
                end += 1;
            }
            self.kernel.process_prepared(&mut buffer[offset * 2..end * 2]);
            offset = end;
        }
    }
    fn connected_inputs(&mut self, mask: [bool; 5]) -> Result<(), DeviceSdkError> {
        self.connected = mask;
        Ok(())
    }
    fn parameter_input(&self, id: &str) -> Option<usize> {
        ["pitch", "cutoff", "pan", "position"].iter().position(|&name| name == id)
    }
    fn parameter_range(&self, id: &str) -> Option<(f64, f64)> {
        self.kernel.info().params.iter().find(|p| p.id == id).map(|p| (p.min, p.max))
    }
    fn set_parameter(&mut self, id: &str, value: f64) {
        if let Some(index) = self.kernel.info().param_index(id) {
            self.kernel.set_param(index, value);
        }
    }
    fn event(&mut self, event: NoteEvent) {
        Device::event(self.kernel.as_mut(), event);
    }
    fn reset(&mut self) {
        Device::reset(self.kernel.as_mut());
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self.kernel.as_mut()
    }
}

impl PreparedEffect {
    /// Bind a sample source off the audio thread. Replacement may release old resources.
    pub fn sampler_mut(&mut self, node: &str) -> Option<&mut crate::sampler::Sampler> {
        self.native_node_mut(node)?.as_any_mut().downcast_mut()
    }
}

pub(crate) const SAMPLER_ASSET: NodeInfo = NodeInfo {
    kind: "sampler_asset",
    name: "Sampler (bound asset)",
    rust_variant: "SamplerAsset",
    configuration: "sample binding",
    example: r#"{"kind":"sampler_asset","sample":"sample"}"#,
    ..SAMPLER
};
pub(crate) fn prepare_bound(
    source: std::sync::Arc<crate::sample_resource::SampleAsset>,
    rate: f64,
) -> Result<Box<dyn NativeNode>, DeviceSdkError> {
    let mut kernel = Box::new(crate::sampler::Sampler::new());
    kernel.prepare(rate).map_err(|e| DeviceSdkError(e.to_string()))?;
    kernel.load_shared_sample(source.sample_rate(), source.channels() as u16, source.shared_pcm());
    if let Some(bpm) = source.source_bpm()
        && let Some(index) = kernel.info().param_index("source_bpm")
    {
        kernel.set_param(index, bpm);
    }
    Ok(Box::new(Adapter { kernel, connected: [false; 5] }))
}
